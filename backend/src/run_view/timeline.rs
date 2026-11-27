//! The run's progress timeline: one row per task with a bar per attempt, what the task produced at
//! the end of its bar, what it waits for before it starts, and the decisions and open items on a
//! lane above. The client lays it out on a time axis; what is on it is worked out here.
// @project-doc docs/architecture/20261126-web.md#run_timeline

use super::board::{BoardFacts, OpenKind};
use super::words::Words;
use super::{AttemptShown, RunState, TaskShown, plain, short};
use crate::evidence::{Check, Summary};
use crate::lang::tr;
use crate::wire::wire_enum;
use serde::Serialize;
use std::collections::BTreeMap;

wire_enum! {
    /// What a mark on the timeline is: a task's conclusion or artifact at the end of its bar, or a
    /// decision or open item on the lane above.
    pub enum MarkKind {
        Result = "result",
        Artifact = "artifact",
        Decision = "decision",
        Open = "open",
    }
}

/// Something that happened at one moment, backed by one event.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub id: String,
    pub seq: i64,
    pub ts: i64,
    pub kind: MarkKind,
    pub title: String,
    /// Who made it (`user`, `lead` or an agent id).
    pub by: String,
    /// For an open item: what it is (`OpenKind`), its name, and whether it waits on the person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what: Option<OpenKind>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub you: bool,
}

/// One attempt of a task, from when it started to when it stopped (`end`; `None` while it runs).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub id: String,
    pub start: i64,
    pub end: Option<i64>,
    pub attempt: i64,
    /// A finished attempt keeps its own outcome; the running one shows the task's status.
    pub status: TaskShown,
    pub running: bool,
}

/// One task on the timeline.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTask {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub status: TaskShown,
    pub deps: Vec<String>,
    pub bars: Vec<Bar>,
    /// Not started yet: what it waits for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Mark>,
    pub artifacts: Vec<Mark>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    /// In the order they started, then the ones still waiting in the order the lead gave them; a
    /// task that neither ran nor waits (cancelled or skipped before it started) is a row without
    /// a bar.
    pub tasks: Vec<TimelineTask>,
    /// Decisions (the person's, the lead's) and open items, at the moment they happened.
    pub milestones: Vec<Mark>,
}

/// What a task still waits for before it starts: an upstream task to be fixed, its dependencies,
/// or the scheduler.
pub(super) fn waiting_for(st: &RunState, deps: &[String], status: TaskShown, blocked_by: &str, w: &Words) -> String {
    let l = w.lang;
    if status == TaskShown::Blocked {
        return tr!(l, "等待 {blocked_by} 修复", "Waiting for a fix to {blocked_by}");
    }
    let open: Vec<&str> = deps
        .iter()
        .filter(|d| st.task(d).is_none_or(|t| t.status != TaskShown::Succeeded))
        .map(String::as_str)
        .collect();
    if open.is_empty() {
        l.pick("等待调度", "Waiting to be scheduled").into()
    } else {
        tr!(l, "等待 {}", "Waiting for {}", open.join(w.and()))
    }
}

/// The timeline of `st`, from the board's worded facts; `stamps` gives each event's time and
/// actor. `replaying`: a past position, where what waits is shown whatever the run did later.
pub(super) fn timeline(
    st: &RunState,
    board: &BoardFacts,
    stamps: &BTreeMap<i64, (i64, String)>,
    replaying: bool,
    w: &Words,
) -> Timeline {
    let l = w.lang;
    let waits = st.run.active() || replaying;
    let ts_of = |seq: i64| stamps.get(&seq).map(|(ts, _)| *ts);
    let mut out = vec![];
    for t in &st.tasks {
        let bars = t
            .attempts
            .iter()
            .map(|a| {
                let ended = a.status != AttemptShown::Running;
                let status = match (a.end, ended) {
                    (Some(_), true) => match a.status {
                        AttemptShown::Running => TaskShown::Running,
                        AttemptShown::Succeeded => TaskShown::Succeeded,
                        AttemptShown::Failed => TaskShown::Failed,
                    },
                    _ => t.status,
                };
                Bar {
                    id: format!("{}#{}", t.id, a.n),
                    start: a.start,
                    end: a.end,
                    attempt: a.n,
                    status,
                    running: a.end.is_none(),
                }
            })
            .collect::<Vec<_>>();
        let waiting = (t.attempts.is_empty() && waits && t.status.is_waiting_to_start())
            .then(|| waiting_for(st, &t.deps, t.status, &t.blocked_by, w));
        // The mark stays small; its tooltip carries the evidence methods. Each check with its
        // evidence is on the task page, the delivery and the audit log.
        let result = board.results.iter().rev().find(|r| r.task == t.id).map(|r| {
            let counts = if r.checks.is_empty() {
                String::new()
            } else {
                let checks: Vec<Check> = r.checks.iter().map(Check::from_json).collect();
                let counts = Summary::of_in(&t.accept, &checks, l).counts(l);
                tr!(l, " · 证据 {counts}", " · Evidence: {counts}")
            };
            Mark {
                id: r.key.clone(),
                seq: r.seq,
                ts: r.ts,
                kind: MarkKind::Result,
                title: short(&plain(&r.summary), 120) + &counts,
                by: r.by.clone(),
                what: None,
                label: String::new(),
                you: false,
            }
        });
        let artifacts = board
            .arts
            .iter()
            .filter(|a| a.a.task == t.id && !a.old)
            .map(|a| Mark {
                id: format!("a{}", a.a.seq),
                seq: a.a.seq,
                ts: ts_of(a.a.seq).unwrap_or(st.run.start),
                kind: MarkKind::Artifact,
                title: a.a.name.clone(),
                by: a.a.by.clone(),
                what: None,
                label: String::new(),
                you: false,
            })
            .collect();
        out.push(TimelineTask {
            id: t.id.clone(),
            title: t.title.clone(),
            agent: t.agent.clone(),
            status: t.status,
            deps: t.deps.clone(),
            bars,
            waiting,
            result,
            artifacts,
        });
    }
    let (mut started, waiting): (Vec<_>, Vec<_>) = out.into_iter().partition(|t| !t.bars.is_empty());
    started.sort_by_key(|t| t.bars[0].start);
    started.extend(waiting);
    let mut milestones: Vec<Mark> = board
        .decisions
        .iter()
        .map(|x| Mark {
            id: x.key.clone(),
            seq: x.seq,
            ts: x.ts,
            kind: MarkKind::Decision,
            title: tr!(l, "{}：{}", "{}: {}", x.title, short(&plain(&x.text), 120)),
            by: x.by.clone(),
            what: None,
            label: String::new(),
            you: false,
        })
        .collect();
    for x in &board.open {
        let Some((seq, (ts, actor))) = x.seq.and_then(|s| stamps.get(&s).map(|v| (s, v))) else { continue };
        milestones.push(Mark {
            id: x.key.clone(),
            seq,
            ts: *ts,
            kind: MarkKind::Open,
            title: x.title.clone(),
            by: x.agents.first().cloned().unwrap_or_else(|| actor.clone()),
            what: Some(x.kind),
            label: x.label.clone(),
            you: x.you,
        });
    }
    Timeline { tasks: started, milestones }
}
