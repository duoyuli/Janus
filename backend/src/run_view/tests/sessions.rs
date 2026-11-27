//! Each conversation's events and composer, as the app's session tests had them (`chatSession`,
//! `agentSession`, `taskSession`, `SessionComposer`).

use super::super::sessions::{self, Composer, Drawn, SessionView};
use std::collections::{BTreeMap, HashMap};

/// The conversations of a state alone, without their events' rows.
fn sessions(st: &RunState, live: bool, replaying: bool) -> BTreeMap<String, SessionView> {
    let none = HashMap::new();
    let counts = |_: i64, _: usize| (0, 0);
    sessions::sessions(
        st,
        live,
        replaying,
        &Drawn { events: &none, counts: &counts, lang: Lang::Zh, detail: false, name: "研究员" },
    )
}
use super::*;

/// The conversation `key` of `log` at its end: `live` in the service's memory, or `replaying`.
fn session(log: &Log, key: &str, live: bool, replaying: bool) -> SessionView {
    fold(&log.events).view(live, replaying, &Words::of(Lang::Zh, &json!({})), false).sessions[key].clone()
}

/// A chat's composer: a plain message while the session waits, cutting in while it works, and a
/// message that reopens it once the session is gone, whatever ended it.
#[test]
fn a_chat_always_takes_the_next_message() {
    let mut log = chat_opened();
    let idle = session(&log, "agent:lead", true, false);
    assert!(idle.open && idle.idle && idle.chat && idle.converse);
    assert_eq!(idle.composer, Composer::Converse);
    assert_eq!(session(&log, "agent:lead", true, true).composer, Composer::Replay);

    log.add(
        ev("user.message")
            .actor("user")
            .to("lead")
            .text("再来")
            .data(json!({ "id": "m2", "text": "再来", "delivery": "pending" })),
    );
    log.add(ev("message.routed").to("lead").data(json!({ "id": "m2", "delivery": "prompt" })));
    let working = session(&log, "agent:lead", true, false);
    assert_eq!(working.composer, Composer::Managed);
    assert!(working.chat, "a chat at work offers to stop the turn or add to it, never a takeover");

    // Stopping the turn: the chat waits for the next message, not in a takeover.
    log.add(ev("session.interrupted").actor("user").to("lead").text("停止 研究员 这一轮"));
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("这一轮已结束，等待你的消息")
            .data(json!({ "phase": "waiting_for_input", "turn": "idle" })),
    );
    let stopped = session(&log, "agent:lead", true, false);
    assert!(!stopped.human);
    assert_eq!(stopped.composer, Composer::Converse);

    log.add(ev("run.interrupted").text("编排器重启"));
    let closed = session(&log, "agent:lead", false, false);
    assert!(!closed.open);
    assert_eq!(closed.composer, Composer::Converse, "not「从中断处继续」: sending reopens it");

    log.add(
        ev("agent.status")
            .actor("lead")
            .text("恢复了之前的会话")
            .data(json!({ "phase": "thinking", "session": "s1", "mode": "chat" })),
    );
    log.add(ev("run.failed").text("对话的会话结束了").data(json!({ "error": "Agent 进程已退出" })));
    assert_eq!(session(&log, "agent:lead", false, false).composer, Composer::Converse);
}

/// A chat just started: its session is still opening, and the person can already write the first
/// message, which waits for it.
#[test]
fn a_new_chat_takes_the_first_message_while_it_opens() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "kind": "chat", "cwd": "/c", "git": false })));
    let mut st = log.state();
    st.run.status = Some(RunStatus::Planning);
    let s = &sessions(&st, true, false)["agent:lead"];
    assert!(s.open && s.idle);
    assert_eq!(s.composer, Composer::Converse);
}

/// A goal's lead conversation keeps its own composer: an open lead session is still managed.
#[test]
fn a_goal_lead_is_not_a_chat() {
    let st =
        RunState { run: RunInfo { status: Some(RunStatus::Running), ..RunInfo::default() }, ..RunState::default() };
    let lead = &sessions(&st, true, false)["agent:lead"];
    assert!(!lead.chat);
    assert_eq!(lead.composer, Composer::Managed);
}

/// What the person wrote is a chat's prompt itself: it shows once, as their message.
#[test]
fn a_chat_shows_the_persons_message_not_the_prompt_carrying_it() {
    let mut log = chat_opened();
    log.add(ev("session.prompt").to("lead").data(json!({ "channel": "followup", "prompt": "再来" })));
    let s = session(&log, "agent:lead", true, false);
    let shown: Vec<&str> = s.events.iter().map(|seq| log.events[*seq as usize - 1].kind.as_str()).collect();
    assert!(!shown.contains(&"session.prompt"), "{shown:?}");
    assert!(shown.contains(&"user.message") && shown.contains(&"agent.message"));
    assert!(shown.contains(&"run.created"), "the run it works on opens the lead's conversation");
}

/// A Loop's and a consolidation's one session is a conversation too, with its wakes in it.
#[test]
fn a_loop_and_a_consolidation_are_conversations() {
    for kind in ["loop", "dream"] {
        let mut log = Log::default();
        log.add(ev("run.created").actor("user").data(json!({ "kind": kind, "cwd": "/repo", "git": false })));
        log.add(ev("loop.woke").data(json!({ "reason": "manual", "n": 1 })));
        log.add(ev("agent.message").actor("lead").data(json!({ "text": "看过了" })));
        log.add(ev("loop.slept").data(json!({ "until": null, "reason": null, "summary": "看过了" })));
        log.add(ev("loop.failed").text("没能醒来"));
        let s = session(&log, "agent:lead", true, false);
        assert!(s.chat && s.converse, "{kind}");
        assert_eq!(s.events, [1, 2, 3, 4, 5], "{kind}: the run's start and each wake are in the lead's conversation");
    }
}

/// After delivery the lead's session is a regular conversation, not a closed one; other agents
/// stay closed; a stopped run is no conversation.
#[test]
fn a_delivered_lead_stays_in_conversation() {
    let mut st = RunState::default();
    st.run.goal = "目标".into();
    st.agents.insert("m1".into(), AgentView::default());
    let at = |st: &RunState, agent: &str| sessions(st, false, false)[&format!("agent:{agent}")].converse;
    st.run.status = Some(RunStatus::Delivered);
    assert!(at(&st, "lead"));
    assert!(!at(&st, "m1"));
    st.run.status = Some(RunStatus::Accepted);
    assert!(at(&st, "lead"));
    st.run.status = Some(RunStatus::Cancelled);
    assert!(!at(&st, "lead"));
}

/// Replaying a delivered or interrupted run shows its sessions read-only: the composer the live
/// run offers there is not offered at a replayed position.
#[test]
fn replayed_sessions_take_no_messages() {
    let mut st = RunState::default();
    st.run.goal = "目标".into();
    st.run.status = Some(RunStatus::Delivered);
    assert_eq!(
        sessions(&st, false, false)["agent:lead"].composer,
        Composer::Converse,
        "a historical run reopens on send"
    );
    assert_eq!(sessions(&st, false, true)["agent:lead"].composer, Composer::Replay);
    st.run.status = Some(RunStatus::Interrupted);
    assert_eq!(sessions(&st, false, false)["agent:lead"].composer, Composer::Interrupted);
    assert_eq!(sessions(&st, false, true)["agent:lead"].composer, Composer::Replay);
}

/// A task's conversation is its attempts: open while its session is, the person holding it after a
/// takeover, and a conflict found by another task's edit shows in both.
#[test]
fn a_tasks_conversation_follows_its_session() {
    let mut log = goal_run();
    let s = session(&log, "task:T1", true, false);
    assert!(s.open && !s.human && !s.idle);
    assert_eq!(s.composer, Composer::Managed);
    assert_eq!(session(&log, "task:T2", true, false).composer, Composer::Ended, "a task not started takes nothing");

    log.add(ev("control.taken").actor("user").to("researcher").task("T1"));
    let held = session(&log, "task:T1", true, false);
    assert!(held.human);
    assert_eq!(held.composer, Composer::Human);

    log.add(ev("task.conflict").actor("janus").task("T2").data(json!({ "other": "T1", "paths": ["calc.py"] })));
    let conflict = log.events.len() as i64;
    assert!(session(&log, "task:T1", true, false).events.contains(&conflict), "the other task shows it");
    assert!(session(&log, "task:T2", true, false).events.contains(&conflict));

    log.add(ev("run.interrupted"));
    let s = session(&log, "task:T1", false, false);
    assert!(!s.open);
    assert_eq!(s.composer, Composer::Interrupted, "sending resumes a run stopped part-way");
}

/// An agent's conversation is everything it did or was told in the run, and its turn follows the
/// same rule as a task's session; with no open session a message waits for its next one.
#[test]
fn an_agents_conversation_is_what_it_did_in_the_run() {
    let mut log = goal_run();
    log.add(ev("agent.message").actor("researcher").task("T1").data(json!({ "text": "读完了" })));
    log.add(ev("agent.status").actor("researcher").task("T1").data(json!({ "phase": "thinking" })));
    let s = session(&log, "agent:researcher", true, false);
    let shown: Vec<&str> = s.events.iter().map(|seq| log.events[*seq as usize - 1].kind.as_str()).collect();
    assert_eq!(shown, ["attempt.started", "agent.message"], "a status without an idle turn is not shown");
    assert!(s.open, "its task's session is open");
    log.add(ev("task.succeeded").task("T1").data(json!({ "summary": "x" })));
    let done = session(&log, "agent:researcher", true, false);
    assert!(!done.open);
    assert_eq!(done.composer, Composer::Leave);
}
