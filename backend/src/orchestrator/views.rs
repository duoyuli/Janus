//! What the person's clients read about runs, from `run_view`: one run's view at a position of
//! its log, and the inbox across every run, with what the person did with its entries.
// @project-doc docs/interfaces/20261031-http_api.md#run_view

use super::App;
use crate::db::{InboxMark, RunRow, RunsPage};
use crate::lang::Lang;
use crate::run_ops;
use crate::run_ops::RunOp;
use crate::run_view::{Candidate, Fold, InboxEntry, RunView, Words, fold, inbox_entries};
use crate::status::{RunKind, RunStatus};
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// The inbox as `GET /api/inbox` serves it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inbox {
    /// What waits now, newest first.
    pub open: Vec<InboxEntry>,
    /// What the person put off, the soonest back first.
    pub later: Vec<InboxEntry>,
    /// When the next entry put off comes back (ms): ask again then.
    pub next_wake: Option<i64>,
}

/// One run as the run lists show it: its record, and whether it can be deleted now.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedRun {
    #[serde(flatten)]
    pub row: RunRow,
    pub delete: run_ops::OpAnswer,
}

impl App {
    /// `rows` as the run lists show them, written in `lang`.
    // @project-doc docs/interfaces/20261031-http_api.md#run_list
    pub fn listed(&self, rows: Vec<RunRow>, lang: Lang) -> Vec<ListedRun> {
        rows.into_iter()
            .map(|row| {
                let facts = match self.run(&row.id) {
                    Some(run) => self.live_facts(&run),
                    None => self.stored_facts(&row),
                };
                let delete = run_ops::answer(&facts, RunOp::Delete, lang);
                ListedRun { row, delete }
            })
            .collect()
    }

    /// Run `id`'s view written in `lang`, up to and including seq `until` (all of it without);
    /// its conversations at the display level `detail`. `None` when there is no such run.
    pub fn run_view(&self, id: &str, until: Option<i64>, lang: Lang, detail: bool) -> Result<Option<RunView>> {
        let Some(row) = self.db.run(id)? else { return Ok(None) };
        let words = Words::of(lang, &row.config);
        let held = self.run(id);
        // A replayed position has no live session to work or to be held.
        let live = until.is_none() && held.is_some();
        let facts = match &held {
            Some(run) => self.live_facts(run),
            None => self.stored_facts(&row),
        };
        let work = self.cfg.layout.run(row.kind, id).work;
        let show = |f: &Fold, replaying: bool| {
            let mut view = f.view(live, replaying, &words, detail);
            view.docs = f.docs(&work);
            view
        };
        let mut view = match until {
            Some(until) => show(&self.fold_until(id, until)?, true),
            None => self.views.current(id, |after| self.db.events(id, after), |f| show(f, false))?,
        };
        view.actions = run_ops::answers(&facts, until.is_some(), lang);
        Ok(Some(view))
    }

    /// Run `id`'s fold up to and including seq `until`.
    fn fold_until(&self, id: &str, until: i64) -> Result<Fold> {
        let mut events = self.db.events(id, 0)?;
        events.retain(|e| e.seq <= until);
        Ok(fold(&events))
    }

    /// `with` over run `id`'s fold now, or at seq `until`; `None` when there is no such run.
    pub fn with_fold<R>(&self, id: &str, until: Option<i64>, with: impl FnOnce(&Fold) -> R) -> Result<Option<R>> {
        if self.db.run(id)?.is_none() {
            return Ok(None);
        }
        Ok(Some(match until {
            Some(until) => with(&self.fold_until(id, until)?),
            None => self.views.current(id, |after| self.db.events(id, after), with)?,
        }))
    }

    /// Everything that waits on the person, in `lang`, split by what they put off; `now` is the
    /// time a snooze is compared with. Marks of entries that are gone are forgotten.
    // @project-doc docs/architecture/20261031-frontend.md#inbox
    pub fn inbox(&self, lang: Lang, now: i64) -> Result<Inbox> {
        use RunStatus::*;
        let rows = |kind: RunKind| -> Result<Vec<RunRow>> {
            let page = RunsPage {
                kind,
                workspace: None,
                statuses: Some(vec![Planning, Running, Delivered, Interrupted, Failed]),
                loop_id: None,
                before: None,
                limit: 500,
            };
            Ok(self.db.runs_page(&page)?.0)
        };
        // A chat with no open session asks nothing (the next message reopens it); a Loop asks
        // whether or not its session is open.
        let mut candidates = rows(RunKind::Goal)?;
        candidates.extend(rows(RunKind::Chat)?.into_iter().filter(|r| r.status.is_active()));
        candidates.extend(rows(RunKind::Loop)?);
        candidates.extend(rows(RunKind::Dream)?);
        let names: HashMap<String, String> = self.workspaces()?.into_iter().map(|w| (w.id, w.name)).collect();
        let mut all = vec![];
        let mut read = HashSet::new();
        for row in &candidates {
            let ws = crate::run_view::text(&row.config["workspace"]["id"]);
            let workspace_name = if row.kind == RunKind::Chat {
                lang.pick("对话", "Chat").to_string()
            } else {
                names.get(&ws).cloned().unwrap_or_else(|| crate::run_view::text(&row.config["workspace"]["name"]))
            };
            let c = Candidate { row, workspace_name };
            let words = Words::of(lang, &row.config);
            let asks = if c.reads_events() {
                read.insert(row.id.clone());
                let live = self.run(&row.id).is_some();
                Some(self.views.current(&row.id, |after| self.db.events(&row.id, after), |f| f.asks(live, &words))?)
            } else {
                None
            };
            all.extend(inbox_entries(&c, asks.as_ref(), &words));
        }
        self.views.keep_only(&read);
        let keys: HashSet<String> = all.iter().map(|e| e.key.clone()).collect();
        self.db.keep_inbox_marks(&keys)?;
        let marks = self.db.inbox_marks()?;
        let (mut open, mut later) = (vec![], vec![]);
        for mut e in all {
            let mark = marks.get(&e.key).copied().unwrap_or_default();
            if mark.ignored {
                continue;
            }
            match mark.snoozed_until.filter(|until| e.can_wait && *until > now) {
                Some(until) => {
                    e.snoozed_until = Some(until);
                    later.push(e);
                }
                None => open.push(e),
            }
        }
        open.sort_by_key(|e| std::cmp::Reverse(e.ts));
        later.sort_by_key(|e| e.snoozed_until);
        let next_wake = later.first().and_then(|e| e.snoozed_until);
        Ok(Inbox { open, later, next_wake })
    }

    /// What the person did with inbox entry `key`: put it off until a time (`None` brings it
    /// back), or stopped being reminded of it.
    pub fn mark_inbox(&self, key: &str, mark: InboxMark) -> Result<()> {
        self.db.set_inbox_mark(key, mark)?;
        let _ = self.changes.send(super::Change::Inbox);
        Ok(())
    }
}
