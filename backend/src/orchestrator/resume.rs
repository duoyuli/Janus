//! The lead going back into its goal after a restart or after its session broke off: where its
//! subtasks stand and what broke it off, in its brief (a new session) or its next prompt (its own
//! session loaded again).
// @project-doc docs/domains/20261031-run_lifecycle.md#recovery

use super::*;
use crate::lang::{Lang, tr};

impl App {
    /// Whether the lead did anything in run `run` after event `seq`: spoke, called a tool, put
    /// tasks on the board.
    pub(super) fn lead_worked_since(&self, run: &Run, seq: i64) -> bool {
        self.db
            .events(&run.id, seq)
            .is_ok_and(|evs| evs.iter().any(|e| e.actor == "lead" && e.kind().is_agent_progress()))
    }
}

/// Where each subtask on the board stands as a run continues after a restart, as the lead reads
/// it: what is done, what was at work and carries on in its own session, what the lead had sent
/// back and with what note, and the last thing the lead told the ones still open. A result from an
/// earlier attempt is marked as such, so it is not taken for the current state.
// @project-doc docs/domains/20261031-run_lifecycle.md#recovery
pub(super) fn board_on_resume(run: &Run, events: &[Event]) -> String {
    let lang = run.lang;
    let st = run.st.lock().unwrap();
    if st.tasks.is_empty() {
        return lang.pick("还没有给协作者派过子任务。", "No subtasks were assigned to collaborators.").into();
    }
    let lines: Vec<String> = st
        .tasks
        .iter()
        .map(|t| {
            let id = t.def.id.as_str();
            let mine = |e: &&Event| e.task.as_deref() == Some(id);
            let mut parts: Vec<String> = vec![];
            match t.status() {
                TaskStatus::Succeeded => {
                    let summary = t.result.as_ref().map(|r| clip(&r.summary, 200)).unwrap_or_default();
                    parts.push(match t.verdict() {
                        Some(vd) => {
                            let vd = vd.as_str();
                            tr!(lang, "已完成，判定 {vd}：{summary}", "done, verdict {vd}: {summary}")
                        }
                        None => tr!(lang, "已完成：{summary}", "done: {summary}"),
                    });
                }
                TaskStatus::Pending => {
                    // Why it runs again: the lead's retry or a re-verification, unless it already
                    // succeeded since.
                    let reopened = events
                        .iter()
                        .filter(mine)
                        .rfind(|e| matches!(e.kind(), EventKind::TaskReopened | EventKind::TaskSucceeded))
                        .filter(|e| e.kind() == EventKind::TaskReopened)
                        .and_then(|e| e.data.as_ref());
                    match reopened.and_then(Reopen::of_event) {
                        Some(Reopen::Retry { session, from, note, .. }) => {
                            let session = session.as_str();
                            let mut why = tr!(
                                lang,
                                "你用 janus.retry 把它发回返工（session={session}）",
                                "you sent it back with janus.retry (session={session})"
                            );
                            if let Some(v) = from {
                                why.push_str(&tr!(lang, "，依据 {v} 的意见", " on {v}'s findings"));
                            }
                            if let Some(n) = note {
                                let n = clip(&n, 300);
                                why.push_str(&tr!(lang, "；你的说明：{n}", "; your note: {n}"));
                            }
                            parts.push(why);
                        }
                        Some(Reopen::Reverify { target }) => {
                            parts.push(tr!(lang, "在 {target} 返工后重新验证", "checks {target} again after its rework"))
                        }
                        None => {}
                    }
                    parts.push(if let Some(reload) = st.resume_session.get(id) {
                        match &reload.broke {
                            BrokeOff::Restart => lang.pick(
                                "Janus 停下时它正在工作，会打开自己原来的会话接着做（Agent 加载不了时新开会话）",
                                "it was at work when Janus stopped and reopens its own session to carry on (a new session if its agent cannot load it)",
                            ).into(),
                            BrokeOff::Failure(err) => {
                                let err = clip(err, 160);
                                tr!(
                                    lang,
                                    "它上一次尝试中断了（{err}），会回到自己原来的会话接着做（加载不了或快满时新开会话）",
                                    "its last attempt broke off ({err}) and goes back into its own session to carry on (a new session if that cannot be loaded or is nearly full)"
                                )
                            }
                        }
                    } else if t.attempts == 0 {
                        lang.pick("还没开始", "not started yet").into()
                    } else {
                        lang.pick("会重新开始", "starts again").into()
                    });
                    if let Some(r) = &t.result {
                        let summary = clip(&r.summary, 160);
                        parts.push(match t.last_verdict() {
                            Some(v) => {
                                let v = v.as_str();
                                tr!(lang, "之前一次尝试的结果（判定 {v}）：{summary}", "result of an earlier attempt (verdict {v}): {summary}")
                            }
                            None => tr!(lang, "之前一次尝试的结果：{summary}", "result of an earlier attempt: {summary}"),
                        });
                    }
                }
                TaskStatus::Blocked => {
                    let by = t.blocked_by().unwrap_or(lang.pick("某个依赖", "a dependency"));
                    parts.push(tr!(lang, "被 {by} 挡住", "blocked by {by}"))
                }
                TaskStatus::Failed => parts.push(lang.pick("失败", "failed").into()),
                TaskStatus::SkippedByDecision => parts.push(lang.pick("被你取消", "cancelled by you").into()),
                other => parts.push(other.as_str().into()),
            }
            if !t.status().terminal()
                && let Some(told) = events
                    .iter()
                    .filter(mine)
                    .rfind(|e| e.kind() == EventKind::UserMessage && e.actor == "lead")
            {
                let told = clip(told.text.trim(), 200);
                parts.push(tr!(lang, "你最后对它说：{told}", "your last message to it: {told}"));
            }
            format!("- {id} ({}): {} · {}", t.agent, t.def.title, parts.join(" · "))
        })
        .collect();
    lines.join("\n")
}

/// What a lead in a new session after a restart or a failure is told, in its brief: its old session is gone,
/// so where the board stands and what it and the person last said.
pub(super) fn resume_brief(board: &str, broke: &BrokeOff, events: &[Event], lang: Lang) -> String {
    let recent = recent_exchanges(events, Speakers::You, lang);
    let recent = if recent.is_empty() { lang.pick("（无）", "(none)").to_string() } else { recent };
    let (why, when) = broke_off(broke, lang);
    tr!(
        lang,
        "

从中断处继续：{why}。这是一个新会话，之前做过、读过的都不在里面；已经做的改动还在工作目录里。从现状接着推进，不要重做已经完成的工作。

{when}看板上的子任务：
{board}

你和用户最近的对话（按时间先后）：
{recent}",
        "

Resuming after an interruption: {why}. This is a new session without what you did and read before; the changes made so far are still in the working directory. Carry on from where things stand; do not redo finished work.

Subtasks on the board {when}:
{board}

Your recent conversation with the user, oldest first:
{recent}"
    )
}

/// What broke the lead off, and when the board below was taken.
pub(super) fn broke_off(broke: &BrokeOff, lang: Lang) -> (String, &'static str) {
    match broke {
        BrokeOff::Restart => (
            lang.pick("你推进这个目标时 Janus 重启了", "Janus restarted while you were working on this goal").into(),
            lang.pick("Janus 停下时", "when Janus stopped"),
        ),
        BrokeOff::Failure(err) => {
            let err = clip(err, 300);
            (
                tr!(
                    lang,
                    "你推进这个目标时会话中断了（{err}）",
                    "Your session broke off while you were working on this goal ({err})"
                ),
                lang.pick("现在", "now"),
            )
        }
    }
}

/// What a lead whose own session was loaded again is told first: it remembers its work, so only
/// what broke it off, where the board stands and the messages left for it meanwhile.
pub(super) fn resume_prompt(board: &str, broke: &BrokeOff, notes: &str, lang: Lang) -> String {
    let (why, when) = broke_off(broke, lang);
    let others = match broke {
        BrokeOff::Restart => lang.pick("协作者的会话都已关闭", "the collaborators' sessions were closed"),
        BrokeOff::Failure(_) => lang.pick("协作者一直在工作", "the collaborators kept working"),
    };
    tr!(
        lang,
        "[Janus] {why}。你的会话已经恢复，之前做过、读过的都还在；{others}。从停下的地方接着推进，不要重做已经完成的工作。

{when}看板上的子任务：
{board}{notes}

看板从现在起只带来之后发生的事。",
        "[Janus] {why}. Your session was restored, so what you did and read before is still here; {others}. Carry on from where you stopped; do not redo finished work.

Subtasks on the board {when}:
{board}{notes}

From here on, the board brings only what happens next."
    )
}

/// Where a lead goes back into its goal from: the board as it stands and what broke it off.
pub(super) struct LeadResume {
    pub board: String,
    pub broke: BrokeOff,
}

/// What the lead hears when the person who interrupted it hands the turn back: before the goal is
/// delivered, to go on with it; after, to go on with what the person asked.
pub(super) fn resume_after_hold(run: &Run, delivered: bool) -> String {
    let goal = &run.goal;
    match delivered {
        true => run
            .lang
            .pick(
                "人工介入结束。请结合上面的对话，继续处理用户的要求。",
                "The person has handed the session back. Taking the conversation above into account, go on with what the user asked.",
            )
            .into(),
        false => tr!(
            run.lang,
            "人工介入结束。请结合上面的对话，继续推进目标直到完成：{goal}",
            "The person has handed the session back. Taking the conversation above into account, carry the goal on to completion: {goal}"
        ),
    }
}
