//! The inbox: what every run in every workspace waits on the person for, from the same facts the
//! run page shows (`RunView`), so the two never disagree; and what a run's own record asks of them
//! (a run that stopped, a consolidation to look at).
// @project-doc docs/architecture/20261031-frontend.md#inbox

use super::Asks;
use super::board::{OpenKind, QuickAction};
use super::review::ReviewStage;
use super::words::Words;
use crate::db::RunRow;
use crate::lang::tr;
use crate::status::{RunKind, RunStatus};
use serde::Serialize;

/// One thing waiting on the person.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxEntry {
    /// `<run id>/<what>`: stable while the request is open, so a snooze or an ignore sticks to it.
    pub key: String,
    pub run: String,
    pub run_kind: RunKind,
    pub goal: String,
    /// The run's workspace, by id, and its name now (a chat's entries say they are a chat).
    pub workspace: String,
    pub workspace_name: String,
    /// What it is: an open item's kind (`OpenKind`), or `delivered`, `merge_blocked`, `held`,
    /// `interrupted`, `failed`, `dream`, `dream_failed`.
    pub kind: String,
    /// The kind in a word or two, for a badge.
    pub label: String,
    pub title: String,
    pub text: String,
    pub sub: String,
    /// When it arrived (ms).
    pub ts: i64,
    /// Nothing is held up while it waits (the lead's own decision, a delivery, a lead that may be
    /// stuck, a run that stopped), so it may be put off. An approval, a form or a collaborator's
    /// question keeps an agent waiting, and cannot. Any entry can still be ignored.
    pub can_wait: bool,
    /// Answers in one click, right in the inbox.
    pub actions: Vec<QuickAction>,
    /// Put off until then (ms), by the person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
}

/// The rows the inbox reads, and what it needs to know about each.
pub struct Candidate<'a> {
    pub row: &'a RunRow,
    pub workspace_name: String,
}

impl Candidate<'_> {
    /// Its events are read: a run at work, a delivered one (after delivery the lead may still ask
    /// something in its conversation), and a Loop (its questions outlive its session). A finished
    /// run's row says all it asks.
    pub fn reads_events(&self) -> bool {
        let r = self.row;
        r.status.is_active()
            || (r.status == RunStatus::Delivered && r.kind != RunKind::Dream)
            || r.kind == RunKind::Loop
    }
}

/// Run `c`'s entries: its open items and its latest delivery from `asks` (when its events are
/// read), and what its record asks.
pub fn inbox_entries(c: &Candidate<'_>, asks: Option<&Asks>, words: &Words) -> Vec<InboxEntry> {
    let row = c.row;
    let entry = |what: &str, label: String, title: String, text: String, sub: String, ts: i64| InboxEntry {
        key: format!("{}/{what}", row.id),
        run: row.id.clone(),
        run_kind: row.kind,
        goal: row.goal.clone(),
        workspace: super::text(&row.config["workspace"]["id"]),
        workspace_name: c.workspace_name.clone(),
        kind: what.split('-').next().unwrap_or(what).to_string(),
        label,
        title,
        text,
        sub,
        ts,
        can_wait: true,
        actions: vec![],
        snoozed_until: None,
    };
    let l = words.lang;
    let mut out = vec![];
    if let Some(v) = asks {
        for f in &v.open {
            let mut e = entry(
                &f.key,
                f.label.clone(),
                f.title.clone(),
                f.text.clone(),
                f.sub.clone(),
                f.ts.unwrap_or(row.created_at),
            );
            e.kind = f.kind.as_str().into();
            e.can_wait = matches!(f.kind, OpenKind::LeadStall | OpenKind::LoopTouched | OpenKind::LoopFailed)
                || (f.kind == OpenKind::Question && f.tasks.is_empty());
            e.actions = f.actions.clone();
            out.push(e);
        }
        // What a delivery asks, by its stage: each delivery is its own entry, so one ignored does
        // not hide the next. Nothing while the lead or the team works on it.
        if let (Some(r), Some(review)) = (&v.result, &v.review) {
            let n = r.n;
            let first_line = {
                let headline = super::text(&r.d["headline"]);
                let t = if headline.is_empty() { super::text(&r.d["summary"]) } else { headline };
                t.lines().next().unwrap_or("").to_string()
            };
            match review.stage {
                ReviewStage::Awaiting => {
                    let title = if n > 1 {
                        tr!(l, "第 {} 次交付，等你验收", "Delivery {}, waiting for your acceptance", n)
                    } else {
                        l.pick("已交付，等你验收", "Delivery awaits your review").into()
                    };
                    let sub = if review.signals.is_empty() {
                        l.pick("没有需要你看的，核对后验收", "Nothing to look at; check and accept").into()
                    } else {
                        tr!(l, "需要你看的 {} 件", "To look at: {}", review.signals.len())
                    };
                    let label = l.pick("等你验收", "Ready to Accept").into();
                    out.push(entry(&format!("delivered-{n}"), label, title, first_line, sub, r.ts));
                }
                ReviewStage::Blocked => {
                    let sub = if review.conflict {
                        l.pick(
                            "让负责人再解决冲突；忽略的话这一版不会进你的分支",
                            "Let the lead resolve the conflict again; if ignored, this version never reaches your branch",
                        )
                    } else {
                        l.pick(
                            "处理好挡路的东西再合并；忽略的话这一版不会进你的分支",
                            "Clear what is in the way, then merge; if ignored, this version never reaches your branch",
                        )
                    };
                    let title = l.pick("结果没进你的分支", "The result is not on your branch").into();
                    let label = l.pick("结果没进分支", "Not on branch").into();
                    let note = super::text(&r.d["merge"]["note"]);
                    out.push(entry(&format!("merge_blocked-{n}"), label, title, note, sub.into(), r.ts));
                }
                ReviewStage::Held => out.push(entry(
                    &format!("held-{n}"),
                    l.pick("等你决定", "Your call").into(),
                    l.pick("你打断了负责人", "You interrupted the lead").into(),
                    l.pick("这一轮改的东西还没有交付", "What this turn changed is not delivered yet").into(),
                    l.pick("让它接着做，或按现在的样子交付", "Let it go on, or deliver it as it is").into(),
                    r.ts,
                )),
                _ => {}
            }
        }
    }
    // A chat or a Loop asks none of this: its session closing between messages or wakes is how it
    // rests, and the next message or wake reopens it.
    let at = row.created_at;
    match (row.kind, row.status) {
        (RunKind::Chat | RunKind::Loop, _) => {}
        // A consolidation of the team memory asks only to be looked at, and is ignored once seen.
        // @project-doc docs/domains/20261123-team_memory.md#memory_review
        (RunKind::Dream, RunStatus::Delivered) => out.push(entry(
            "dream",
            l.pick("整理好了", "Consolidated").into(),
            l.pick("团队记忆整理好了", "Team memory consolidated").into(),
            String::new(),
            l.pick("看看改了什么，看过就忽略", "See what changed, then ignore it").into(),
            at,
        )),
        (RunKind::Dream, RunStatus::Interrupted | RunStatus::Failed) => out.push(entry(
            "dream_failed",
            l.pick("没整理完", "Not finished").into(),
            l.pick("团队记忆没整理完", "Team memory not fully consolidated").into(),
            String::new(),
            l.pick(
                "已经改的保留；可以在工作区页重新整理",
                "Changes so far stay; consolidate again from the workspace page",
            )
            .into(),
            at,
        )),
        (RunKind::Goal, RunStatus::Interrupted) => out.push(entry(
            "interrupted",
            l.pick("已中断", "Interrupted").into(),
            l.pick("已中断，可以从中断处继续", "Interrupted and ready to resume").into(),
            String::new(),
            l.pick("继续，或者忽略", "Resume the run or ignore it").into(),
            at,
        )),
        (RunKind::Goal, RunStatus::Failed) => out.push(entry(
            "failed",
            l.pick("失败了", "Failed").into(),
            l.pick("失败了，可以从中断处继续", "Failed and ready to resume").into(),
            String::new(),
            l.pick("查看原因后继续，或者忽略", "Review the cause, then resume the run or ignore it").into(),
            at,
        )),
        _ => {}
    }
    out
}
