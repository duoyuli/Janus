//! Reviewing a delivery: where the run stands after its latest one (`ReviewStage`), what in it
//! the person should look at before accepting (`Signal`), and what they asked for in its round.
// @project-doc docs/architecture/20261031-frontend.md#review

use super::{DeliveryView, RequestKind, RunState, SessionTurn, list, text, texts, truthy};
use crate::status::RunStatus;
use serde::Serialize;

crate::wire::wire_enum! {
    /// Where a run stands after a delivery: the delivery is reviewed in full only while it waits
    /// for the person (`awaiting`, `blocked`); otherwise the page says who it waits on now.
    pub enum ReviewStage {
        /// The latest delivery waits for acceptance.
        Awaiting = "awaiting",
        /// The merge conflicted; the lead resolves it, then delivers again.
        Resolving = "resolving",
        /// The result is not on the branch and the person decides (`Review::conflict` says whether
        /// it is a conflict to hand back to the lead, or something on their side in the way).
        Blocked = "blocked",
        /// The lead works on what the person asked after the delivery.
        LeadWorking = "lead_working",
        /// Subtasks the lead handed out after the delivery are at work.
        TeamWorking = "team_working",
        Accepted = "accepted",
        /// Something waits for the person's answer (an approval, a form, a question).
        Asking = "asking",
        /// The person interrupted the lead and holds it.
        Held = "held",
        /// The run stopped after its delivery (interrupted, failed).
        Stopped = "stopped",
    }
}

crate::wire::wire_enum! {
    /// One thing in a delivery the person should look at.
    pub enum SignalKind {
        /// A question the lead leaves for the person (`text`).
        Decision = "decision",
        /// A risk Janus found (`text`, about `task` if any).
        Risk = "risk",
        /// `count` acceptance criteria of `task` did not pass.
        FailedChecks = "failed_checks",
        /// `task`'s result disagrees with the workspace in `count` places (`lines`).
        TraceGaps = "trace_gaps",
        /// `tasks` changed files and no independent verification checks them.
        Unverified = "unverified",
        /// The lead changed `count` files itself (`lines`): no subtask's acceptance covers them.
        LeadEdits = "lead_edits",
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    /// `lead` for the lead's own questions, `janus` for what Janus found.
    pub source: &'static str,
    pub kind: SignalKind,
    /// What to look at, in the person's language (`words`).
    pub text: String,
    /// Where to look, or what the lead says about it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The task it is about, to open.
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
    pub count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
}

impl Signal {
    fn janus(kind: SignalKind) -> Signal {
        Signal {
            source: "janus",
            kind,
            text: String::new(),
            detail: String::new(),
            task: None,
            tasks: vec![],
            count: 0,
            lines: vec![],
        }
    }
}

/// A request the person made in a delivery's round.
#[derive(Clone, Debug, Serialize)]
pub struct Asked {
    pub seq: i64,
    pub text: String,
}

/// The latest delivery's review.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub stage: ReviewStage,
    /// At `blocked`: still conflicting after the lead tried (it goes back to the lead), rather
    /// than something on the person's side in the way.
    pub conflict: bool,
    /// What the person should look at, in order: the lead's questions first, then what Janus
    /// found. The executors' own risks stay with their tasks.
    pub signals: Vec<Signal>,
    /// What the person asked the lead for in this round; nothing for the first delivery.
    pub requests: Vec<Asked>,
}

/// The review of the run's latest delivery; `None` before the first one, and for a chat, a Loop
/// or a consolidation. `live`: the run is in the service's memory.
pub fn review(s: &RunState, live: bool) -> Option<Review> {
    let r = s.result().filter(|_| !s.run.kind.single_session())?;
    let (stage, conflict) = stage(s, r, live);
    let requests = s.round_requests(r).into_iter().map(|(seq, text)| Asked { seq, text }).collect();
    Some(Review { stage, conflict, signals: signals(r), requests })
}

fn stage(s: &RunState, r: &DeliveryView, live: bool) -> (ReviewStage, bool) {
    let asking = s.inbox.iter().any(|i| match i.kind {
        RequestKind::Question | RequestKind::Approval | RequestKind::Input => true,
        RequestKind::Session => i.task.is_some(),
    });
    match s.run.status {
        Some(RunStatus::Running | RunStatus::Planning) => {
            return (if asking { ReviewStage::Asking } else { ReviewStage::TeamWorking }, false);
        }
        Some(RunStatus::Delivered | RunStatus::Accepted) => {}
        _ => return (ReviewStage::Stopped, false),
    }
    if asking {
        return (ReviewStage::Asking, false);
    }
    let merge = text(&r.d["merge"]["state"]);
    if merge == "resolving" {
        return (ReviewStage::Resolving, false);
    }
    // Without the run in memory no lead session is there: a restart cut its turn off.
    let turn = if live { s.lead_turn } else { SessionTurn { person: false, idle: true } };
    if turn.person {
        return (if turn.idle { ReviewStage::Held } else { ReviewStage::LeadWorking }, false);
    }
    if !turn.idle {
        return (ReviewStage::LeadWorking, false);
    }
    if s.run.status == Some(RunStatus::Accepted) {
        return (ReviewStage::Accepted, false);
    }
    if merge == "conflict" || merge == "blocked" {
        return (ReviewStage::Blocked, merge == "conflict");
    }
    (ReviewStage::Awaiting, false)
}

/// What the person should look at in delivery `r`.
fn signals(r: &DeliveryView) -> Vec<Signal> {
    let d = &r.d;
    let mut out: Vec<Signal> = texts(&d["decisions"])
        .into_iter()
        .map(|q| Signal { source: "lead", text: q, ..Signal::janus(SignalKind::Decision) })
        .collect();
    for risk in list(&d["risks"]) {
        let says = text(&risk["text"]);
        if !says.is_empty() {
            out.push(Signal {
                text: says,
                task: risk["task"].as_str().map(str::to_owned),
                ..Signal::janus(SignalKind::Risk)
            });
        }
    }
    let records: Vec<_> = list(&d["acceptance"]).iter().filter(|a| !text(&a["task"]).is_empty()).collect();
    for a in &records {
        let task = text(&a["task"]);
        let failed = a["criteria"].as_i64().unwrap_or(0) - a["passed"].as_i64().unwrap_or(0);
        if failed > 0 || list(&a["checks"]).iter().any(|c| c["passed"] != true) {
            out.push(Signal {
                task: Some(task.clone()),
                count: failed.max(1) as usize,
                ..Signal::janus(SignalKind::FailedChecks)
            });
        }
        let gaps = texts(&a["traceGaps"]);
        if !gaps.is_empty() {
            out.push(Signal {
                task: Some(task),
                count: gaps.len(),
                lines: gaps,
                ..Signal::janus(SignalKind::TraceGaps)
            });
        }
    }
    let unverified: Vec<String> =
        records.iter().filter(|a| truthy(&a["unverified"])).map(|a| text(&a["task"])).collect();
    if !unverified.is_empty() {
        let task = (unverified.len() == 1).then(|| unverified[0].clone());
        out.push(Signal { task, count: unverified.len(), tasks: unverified, ..Signal::janus(SignalKind::Unverified) });
    }
    let edits = texts(&d["leadEdits"]);
    if !edits.is_empty() {
        out.push(Signal { count: edits.len(), lines: edits, ..Signal::janus(SignalKind::LeadEdits) });
    }
    out
}
