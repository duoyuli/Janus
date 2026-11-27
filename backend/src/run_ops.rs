//! What the person may do to a whole run right now, and why not: pause or go on, stop, start a
//! queued run now, accept, continue where it stopped, delete. One rule, `allowed`, both for
//! carrying the action out (`App::request_action`, `App::delete_run`) and for the clients, which
//! show each action's answer from the run's view (`RunView::actions`) instead of judging the run's
//! state themselves.
// @project-doc docs/interfaces/20261031-http_api.md#run_actions

use crate::lang::{Lang, tr};
use crate::status::{RunKind, RunStatus};
use crate::wire::wire_enum;
use serde::Serialize;
use std::collections::BTreeMap;

wire_enum! {
    /// An action on the whole run, by the `type` its request names (`delete` is
    /// `DELETE /api/runs/{id}`).
    pub enum RunOp {
        Pause = "pause",
        Resume = "resume",
        Cancel = "cancel",
        Start = "start",
        Accept = "accept",
        ResumeRun = "resumeRun",
        Delete = "delete",
    }
}

/// What the rule reads about a run: the service's memory of it when it is live, its record
/// otherwise.
#[derive(Clone, Copy, Debug)]
pub struct OpFacts {
    pub status: RunStatus,
    pub kind: RunKind,
    /// In the service's memory.
    pub live: bool,
    /// A delivery already put its result on the person's branch (or in their folder).
    pub delivered: bool,
    /// A goal run that left the queue but never got its snapshot: it queues again.
    pub never_started: bool,
    /// The lead is in a turn (what the person asked after a delivery).
    pub lead_busy: bool,
    /// A request waits for the person's answer.
    pub asking: bool,
}

/// Why an action cannot be done now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    Queued,
    NeverStarted,
    /// It just left the queue and is being set up.
    Starting,
    /// Not live, and stopped short (interrupted, failed).
    Stopped,
    /// Not live, and not stopped short: nobody waits for the action.
    NotGoing,
    /// A Loop's session is paused on its Loop and goes with it.
    OnLoopPage,
    /// Stopping cannot take a delivered result back.
    AlreadyDelivered,
    /// Pausing, going on or stopping needs a run at work.
    NotActive(RunOp, RunStatus),
    NotDelivered(RunStatus),
    LeadBusy,
    Asking,
    /// Continuing a live run: it is at work.
    Live,
    NotQueued,
    /// It cannot be brought back (a consolidation, or a run that was stopped).
    NotResumable(Option<&'static str>),
    /// A goal at work is stopped before it is deleted.
    StillWorking,
    /// A past position of the log: nothing can be done there.
    Replaying,
}

impl Why {
    /// It is the run's state that refuses (HTTP 409), not the request (400).
    pub fn conflict(&self) -> bool {
        matches!(
            self,
            Why::Queued
                | Why::NeverStarted
                | Why::Starting
                | Why::Stopped
                | Why::NotGoing
                | Why::Live
                | Why::StillWorking
        )
    }

    pub fn text(&self, lang: Lang) -> String {
        let l = lang;
        match self {
            Why::Queued => {
                l.pick("这个任务还在排队：可以立即开始，或者取消", "This run is still queued: start it now, or cancel it")
                    .into()
            }
            Why::NeverStarted => l
                .pick(
                    "这个任务没能开始：点「从中断处继续」重新排队",
                    "This run could not start: choose Resume to queue it again",
                )
                .into(),
            Why::Starting => l.pick("这个任务正在开始，稍后再试", "This run is starting; try again in a moment").into(),
            Why::Stopped => l
                .pick(
                    "这个任务已经中断：先点「从中断处继续」，或者给某个 Agent 发消息",
                    "This run was interrupted: choose Resume first, or send an Agent a message",
                )
                .into(),
            Why::NotGoing => l
                .pick(
                    "这个任务已经不在进行，没有人在等这个操作：要补充什么，直接给某个 Agent 发消息",
                    "This run is no longer at work and nothing waits for this: to add something, send an Agent a message",
                )
                .into(),
            Why::OnLoopPage => l
                .pick(
                    "值班在值班页暂停或删除，不能当作任务暂停或停止",
                    "A Loop is paused or deleted on its own page, not paused or stopped as a run",
                )
                .into(),
            Why::AlreadyDelivered => l
                .pick(
                    "这个任务已经交付过，结果已经在你的分支或目录里，停止撤不回它。要停下负责人这一轮，就打断它；不想要这个结果，让负责人回退。",
                    "This run already delivered: its result is on your branch or in your folder, and stopping cannot take it back. Interrupt the lead to stop its turn; ask the lead to revert the result if you do not want it.",
                )
                .into(),
            Why::NotActive(op, status) => {
                let (zh, en) = match op {
                    RunOp::Pause => ("暂停", "paused"),
                    RunOp::Resume => ("继续", "resumed"),
                    _ => ("停止", "stopped"),
                };
                tr!(l, "任务现在是「{status}」，不能{zh}", "This run is {status} and cannot be {en}")
            }
            Why::NotDelivered(status) => tr!(
                l,
                "任务现在是「{status}」，只有已交付的任务 可以验收",
                "This run is {status}; only a delivered run can be accepted"
            ),
            Why::LeadBusy => l
                .pick(
                    "负责人还在处理你交代的事，等它这一轮结束后再验收",
                    "The lead is still working on what you asked; accept once its turn ends",
                )
                .into(),
            Why::Asking => l
                .pick(
                    "还有等你回答的请求，先处理它们再验收",
                    "Requests still wait for your answer; handle them before accepting",
                )
                .into(),
            Why::Live => l
                .pick("这个任务正在进行，不需要从中断处继续", "This run is at work; there is nothing to resume")
                .into(),
            Why::NotQueued => l.pick("只有排队中的任务可以立即开始", "Only a queued run can be started now").into(),
            Why::NotResumable(Some(why)) if l == Lang::Zh => (*why).into(),
            Why::NotResumable(Some(_)) => {
                "A team memory consolidation is one turn and cannot go on; consolidate again from the workspace page"
                    .into()
            }
            Why::NotResumable(None) => l
                .pick(
                    "只有中断、失败、已交付或已验收的子任务可以继续",
                    "Only an interrupted, failed, delivered or accepted run can go on",
                )
                .into(),
            Why::StillWorking => {
                l.pick("任务还在进行，先停止任务再删除", "This run is still at work; stop it before deleting it").into()
            }
            Why::Replaying => {
                l.pick("这是回放的位置，回到实时再操作", "This is a replayed position; go back to live to act").into()
            }
        }
    }
}

/// The text the HTTP API answers with (its errors are in Chinese).
impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text(Lang::Zh))
    }
}

impl std::error::Error for Why {}

/// A run that is not live: what refuses any action before its own rule is asked. A queued run
/// only starts now or is cancelled; one that never started only queues again; one that just left
/// the queue is being set up.
pub fn gate(f: &OpFacts) -> Result<(), Why> {
    if f.live {
        return Ok(());
    }
    match f.status {
        RunStatus::Queued => Err(Why::Queued),
        RunStatus::Failed if f.never_started => Err(Why::NeverStarted),
        RunStatus::Planning if f.kind.delivers() => Err(Why::Starting),
        _ => Ok(()),
    }
}

/// Nothing is in the service to take the action, and nobody waits for it.
pub fn not_going(f: &OpFacts) -> Why {
    if matches!(f.status, RunStatus::Interrupted | RunStatus::Failed) { Why::Stopped } else { Why::NotGoing }
}

/// Whether `op` can be done to the run `f` describes now, or why not.
// @project-doc docs/interfaces/20261031-http_api.md#run_actions
pub fn allowed(f: &OpFacts, op: RunOp) -> Result<(), Why> {
    if op == RunOp::Delete {
        // A goal's sessions and attempts would keep writing; a chat or a Loop goes while it talks.
        return if f.kind.delivers() && f.status.is_active() { Err(Why::StillWorking) } else { Ok(()) };
    }
    if !f.live {
        match (op, f.status) {
            (RunOp::Start | RunOp::Cancel, RunStatus::Queued) => return Ok(()),
            (RunOp::ResumeRun, RunStatus::Failed) if f.never_started => return Ok(()),
            _ => {}
        }
        gate(f)?;
        return match op {
            // Acceptance needs no live agents: a run delivered before a restart is accepted from
            // its record.
            RunOp::Accept if f.status == RunStatus::Delivered => Ok(()),
            RunOp::Accept => Err(Why::NotDelivered(f.status)),
            RunOp::ResumeRun => match f.kind.resumable(f.status) {
                Ok(true) => Ok(()),
                Ok(false) => Err(Why::NotResumable(None)),
                Err(why) => Err(Why::NotResumable(Some(why))),
            },
            _ => Err(not_going(f)),
        };
    }
    match op {
        RunOp::Pause | RunOp::Resume | RunOp::Cancel if !f.kind.stopped_on_run_page() => Err(Why::OnLoopPage),
        // A delivery already put its result on the person's branch: stopping would only end the
        // lead's conversation and call the run cancelled.
        // @project-doc docs/domains/20261031-run_lifecycle.md#user_actions
        RunOp::Cancel if f.delivered => Err(Why::AlreadyDelivered),
        RunOp::Pause | RunOp::Resume | RunOp::Cancel if !f.status.is_active() => Err(Why::NotActive(op, f.status)),
        RunOp::Pause | RunOp::Resume | RunOp::Cancel => Ok(()),
        // Only a delivered run has results to accept; a delivery the lead still works on, or one
        // that waits on the person's answer, is not the result to accept yet.
        // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
        RunOp::Accept if f.status != RunStatus::Delivered => Err(Why::NotDelivered(f.status)),
        RunOp::Accept if f.lead_busy => Err(Why::LeadBusy),
        RunOp::Accept if f.asking => Err(Why::Asking),
        RunOp::Accept => Ok(()),
        RunOp::Start => Err(Why::NotQueued),
        RunOp::ResumeRun => Err(Why::Live),
        RunOp::Delete => unreachable!("answered above"),
    }
}

/// One action's answer as the clients read it.
#[derive(Clone, Debug, Serialize)]
pub struct OpAnswer {
    pub ok: bool,
    /// Why not, in the client's language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl OpAnswer {
    fn of(r: Result<(), Why>, lang: Lang) -> OpAnswer {
        OpAnswer { ok: r.is_ok(), why: r.err().map(|w| w.text(lang)) }
    }
}

/// One action's answer, why not in `lang`.
pub fn answer(f: &OpFacts, op: RunOp, lang: Lang) -> OpAnswer {
    OpAnswer::of(allowed(f, op), lang)
}

/// Every action's answer, by `RunOp`; at a replayed position none can be done.
pub fn answers(f: &OpFacts, replaying: bool, lang: Lang) -> BTreeMap<&'static str, OpAnswer> {
    RunOp::ALL
        .iter()
        .map(|&op| (op.as_str(), if replaying { OpAnswer::of(Err(Why::Replaying), lang) } else { answer(f, op, lang) }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(status: RunStatus, live: bool) -> OpFacts {
        OpFacts {
            status,
            kind: RunKind::Goal,
            live,
            delivered: false,
            never_started: false,
            lead_busy: false,
            asking: false,
        }
    }

    #[test]
    fn a_queued_run_only_starts_now_is_cancelled_or_deleted() {
        let f = facts(RunStatus::Queued, false);
        for op in RunOp::ALL {
            let ok = matches!(op, RunOp::Start | RunOp::Cancel | RunOp::Delete);
            assert_eq!(allowed(&f, *op).is_ok(), ok, "{op}");
        }
        assert_eq!(allowed(&f, RunOp::Pause), Err(Why::Queued));
    }

    #[test]
    fn a_finished_goal_that_is_not_live_resumes_as_the_service_allows() {
        // Delivered and accepted runs reopen as a conversation; a stopped one does not.
        for (status, ok) in [
            (RunStatus::Interrupted, true),
            (RunStatus::Failed, true),
            (RunStatus::Delivered, true),
            (RunStatus::Accepted, true),
            (RunStatus::Cancelled, false),
        ] {
            assert_eq!(allowed(&facts(status, false), RunOp::ResumeRun).is_ok(), ok, "{status}");
        }
        assert_eq!(allowed(&facts(RunStatus::Running, true), RunOp::ResumeRun), Err(Why::Live));
        let dream = OpFacts { kind: RunKind::Dream, ..facts(RunStatus::Interrupted, false) };
        assert!(matches!(allowed(&dream, RunOp::ResumeRun), Err(Why::NotResumable(Some(_)))));
        let never = OpFacts { never_started: true, ..facts(RunStatus::Failed, false) };
        assert!(allowed(&never, RunOp::ResumeRun).is_ok());
        assert_eq!(allowed(&never, RunOp::Accept), Err(Why::NeverStarted));
    }

    #[test]
    fn a_live_run_is_stopped_only_before_its_first_delivery_and_accepted_only_when_it_rests() {
        let running = facts(RunStatus::Running, true);
        assert!(allowed(&running, RunOp::Cancel).is_ok());
        assert!(allowed(&running, RunOp::Pause).is_ok());
        assert_eq!(allowed(&OpFacts { delivered: true, ..running }, RunOp::Cancel), Err(Why::AlreadyDelivered));
        let delivered = facts(RunStatus::Delivered, true);
        assert!(allowed(&delivered, RunOp::Accept).is_ok());
        assert_eq!(allowed(&OpFacts { lead_busy: true, ..delivered }, RunOp::Accept), Err(Why::LeadBusy));
        assert_eq!(allowed(&OpFacts { asking: true, ..delivered }, RunOp::Accept), Err(Why::Asking));
        assert_eq!(allowed(&delivered, RunOp::Pause), Err(Why::NotActive(RunOp::Pause, RunStatus::Delivered)));
        assert_eq!(allowed(&running, RunOp::Accept), Err(Why::NotDelivered(RunStatus::Running)));
        let lp = OpFacts { kind: RunKind::Loop, ..running };
        assert_eq!(allowed(&lp, RunOp::Cancel), Err(Why::OnLoopPage));
    }

    #[test]
    fn a_goal_at_work_is_stopped_before_it_is_deleted_and_a_chat_goes_while_it_talks() {
        assert_eq!(allowed(&facts(RunStatus::Running, true), RunOp::Delete), Err(Why::StillWorking));
        let chat = OpFacts { kind: RunKind::Chat, ..facts(RunStatus::Running, true) };
        assert!(allowed(&chat, RunOp::Delete).is_ok());
        assert!(allowed(&facts(RunStatus::Cancelled, false), RunOp::Delete).is_ok());
    }

    #[test]
    fn a_replayed_position_allows_nothing_and_says_why_in_the_clients_language() {
        let a = answers(&facts(RunStatus::Running, true), true, Lang::En);
        assert!(a.values().all(|x| !x.ok));
        assert!(a["pause"].why.as_deref().unwrap().contains("replayed"));
        let a = answers(&facts(RunStatus::Interrupted, false), false, Lang::Zh);
        assert!(a["resumeRun"].ok);
        assert_eq!(a["pause"].why.as_deref(), Some(Why::Stopped.text(Lang::Zh).as_str()));
    }
}
