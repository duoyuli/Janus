//! What the run card and the team panel say: the run's status in a word or two (the first of the
//! states that applies), and for each member what it is doing, what it is on and how its work went.
// @project-doc docs/architecture/20261031-frontend.md#run_page

use super::review::{Review, ReviewStage};
use super::words::Words;
use super::{RunState, TaskShown};
use crate::lang::tr;
use crate::status::{AgentPhase, RunStatus};
use crate::wire::wire_enum;
use serde::Serialize;
use std::collections::BTreeMap;

wire_enum! {
    /// The colour a status is drawn in; the text says the same.
    pub enum Tone {
        Gray = "gray",
        Blue = "blue",
        Green = "green",
        Amber = "amber",
        Red = "red",
        Violet = "violet",
    }
}

/// A status in a word or two, and its colour.
#[derive(Clone, Debug, Serialize)]
pub struct Pill {
    pub text: String,
    pub tone: Tone,
}

/// The run card's status: stopped, failed or interrupted first, then paused, then where the
/// delivery's review stands, then the lead starting, then what waits on the person, else the team
/// at work.
pub(super) fn capsule(st: &RunState, review: Option<&Review>, w: &Words) -> Pill {
    let l = w.lang;
    let pill = |text: &str, tone| Pill { text: text.into(), tone };
    match st.run.status {
        Some(RunStatus::Cancelled) => return pill(l.pick("已停止", "Stopped"), Tone::Gray),
        Some(RunStatus::Failed) => return pill(l.pick("任务失败", "Run Failed"), Tone::Red),
        Some(RunStatus::Interrupted) => return pill(l.pick("已中断", "Interrupted"), Tone::Gray),
        _ => {}
    }
    if st.run.paused {
        return pill(l.pick("已暂停", "Paused"), Tone::Gray);
    }
    match review.map(|r| r.stage) {
        Some(ReviewStage::Awaiting) => {
            return pill(l.pick("已交付 · 等你验收", "Delivered · Awaiting acceptance"), Tone::Green);
        }
        Some(ReviewStage::Blocked) => {
            return pill(l.pick("结果没进你的分支", "The result is not on your branch"), Tone::Amber);
        }
        Some(ReviewStage::Resolving) => {
            return pill(l.pick("负责人在解决合并冲突", "Lead resolving a merge conflict"), Tone::Blue);
        }
        Some(ReviewStage::LeadWorking) => {
            return pill(l.pick("负责人在处理你的消息", "Lead working on your message"), Tone::Blue);
        }
        Some(ReviewStage::Held) => return pill(l.pick("你打断了负责人", "You interrupted the lead"), Tone::Amber),
        Some(ReviewStage::Accepted) => return pill(l.pick("已验收", "Accepted"), Tone::Green),
        _ => {}
    }
    if st.run.status == Some(RunStatus::Planning) {
        return pill(l.pick("负责人正在上岗", "Lead getting started"), Tone::Blue);
    }
    if !st.inbox.is_empty() {
        let n = st.inbox.len();
        return Pill { text: tr!(l, "{n} 件待处理", "{n} pending"), tone: Tone::Amber };
    }
    pill(l.pick("团队干活中", "Team working"), Tone::Blue)
}

/// One member's row in the team panel.
#[derive(Clone, Debug, Serialize)]
pub struct MemberLine {
    pub id: String,
    pub lead: bool,
    /// What it is doing: a collaborator with no task in progress is idle, whatever its last
    /// status said; nobody works in a finished run.
    pub phase: AgentPhase,
    /// What it is on, or what it waits for next.
    pub line: String,
    /// Its newest event while it works (ms), for 「最近活动」.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beat: Option<i64>,
    /// A collaborator's record in this run: 「本次 2/3 首次通过 · 验证没通过 1」.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<String>,
    /// How far its running task is, when it said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

/// Each member of the run's team in the snapshot's order; `failed_checks` counts each task's
/// failed verifications.
pub(super) fn members(st: &RunState, failed_checks: &BTreeMap<String, usize>, w: &Words) -> Vec<MemberLine> {
    let l = w.lang;
    let finished = st.run.is_finished();
    w.team
        .iter()
        .map(|(id, lead)| {
            let a = st.agents.get(id);
            let mine: Vec<_> = st.tasks.iter().filter(|t| &t.agent == id).collect();
            let cur = mine.iter().find(|t| t.status.is_busy());
            let next = mine.iter().find(|t| t.status.is_waiting_to_start());
            let said = if finished { AgentPhase::Idle } else { a.map(|a| a.phase).unwrap_or_default() };
            let phase = if !lead && cur.is_none() && said != AgentPhase::Error { AgentPhase::Idle } else { said };
            let note = a.map(|a| a.note.as_str()).unwrap_or("");
            let line = if *lead {
                if !st.inbox.is_empty() {
                    let n = st.inbox.len();
                    tr!(l, "{n} 件待处理，其他子任务照常推进", "{n} pending · other subtasks keep going")
                } else if let Some(c) = cur {
                    format!("{} {}", c.id, c.title)
                } else if !note.is_empty() {
                    note.to_string()
                } else if st.run.status == Some(RunStatus::Delivered) {
                    l.pick("已完成最终验收", "Final acceptance complete").into()
                } else {
                    l.pick("按依赖调度子任务", "Scheduling subtasks by dependency").into()
                }
            } else if let Some(c) = cur {
                let mut line = format!("{} {}", c.id, c.title);
                match c.status {
                    TaskShown::WaitingInput => line += l.pick(" · 等你回答", " · Waiting for your answer"),
                    TaskShown::WaitingApproval => line += l.pick(" · 等你审批", " · Waiting for approval"),
                    _ if !note.is_empty() => line += &format!(" · {note}"),
                    _ => {}
                }
                if c.session.is_some_and(|s| s.person) {
                    line += l.pick(" · 人工接管中", " · Under manual control");
                }
                line
            } else if let Some(n) = next {
                let id = &n.id;
                if n.status == TaskShown::Blocked {
                    let by = &n.blocked_by;
                    tr!(l, "{id} 等待 {by} 修复", "{id} is waiting for {by} to be fixed")
                } else {
                    let open: Vec<&str> = n
                        .deps
                        .iter()
                        .filter(|d| st.task(d).is_none_or(|t| t.status != TaskShown::Succeeded))
                        .map(String::as_str)
                        .collect();
                    let on =
                        if open.is_empty() { l.pick("调度", "Scheduling").to_string() } else { open.join(w.and()) };
                    tr!(l, "下一步 {id} · 等待 {on}", "Next: {id} · Waiting for {on}")
                }
            } else if !mine.is_empty() {
                l.pick("派给它的子任务都做完了", "All its subtasks are done").into()
            } else if !st.tasks.is_empty() {
                l.pick("负责人没有给它派子任务", "The lead has assigned no subtasks").into()
            } else {
                l.pick("待命", "Standing by").into()
            };
            let beat = a.map(|a| a.beat).filter(|b| phase != AgentPhase::Idle && *b > 0);
            let stats = (!lead).then(|| {
                let ended: Vec<_> =
                    mine.iter().filter(|t| matches!(t.status, TaskShown::Succeeded | TaskShown::Failed)).collect();
                let vf = |t: &str| failed_checks.get(t).copied().unwrap_or(0);
                let first = ended
                    .iter()
                    .filter(|t| t.status == TaskShown::Succeeded && t.attempts.len() == 1 && vf(&t.id) == 0)
                    .count();
                let vfail = ended.iter().filter(|t| vf(&t.id) > 0).count();
                let n = ended.len();
                let fails = if vfail > 0 {
                    tr!(l, " · 验证没通过 {vfail}", " · Verification failures: {vfail}")
                } else {
                    String::new()
                };
                (n > 0)
                    .then(|| tr!(l, "本次 {first}/{n} 首次通过{fails}", "Passed on first attempt: {first}/{n}{fails}"))
            });
            let progress = cur
                .filter(|c| c.status == TaskShown::Running)
                .and_then(|_| a.and_then(|a| a.progress))
                .filter(|p| *p > 0.0);
            MemberLine { id: id.clone(), lead: *lead, phase, line, beat, stats: stats.flatten(), progress }
        })
        .collect()
}
