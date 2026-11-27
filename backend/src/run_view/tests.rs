//! The fold over a run's events, as the app's derivation tests had it: tasks, agents, requests,
//! messages, deliveries; the board, the review and the inbox are in the submodules.

mod aside;
mod audit;
mod board;
mod card;
mod evidence;
mod feed;
mod inbox;
mod lines;
mod page;
mod review;
mod sessions;
mod timeline;
mod turns;

use super::*;
use crate::lang::Lang;
use serde_json::json;

/// An event under construction, as the orchestrator records one; `Janus` speaks unless `actor`.
pub(super) struct E(Event);

pub(super) fn ev(kind: &str) -> E {
    E(Event {
        run_id: "r1".into(),
        seq: 0,
        ts: 0,
        kind: kind.into(),
        actor: "janus".into(),
        to: None,
        task: None,
        attempt: None,
        text: String::new(),
        data: None,
    })
}

impl E {
    pub(super) fn actor(mut self, a: &str) -> E {
        self.0.actor = a.into();
        self
    }
    pub(super) fn to(mut self, t: &str) -> E {
        self.0.to = Some(t.into());
        self
    }
    pub(super) fn task(mut self, t: &str) -> E {
        self.0.task = Some(t.into());
        self
    }
    pub(super) fn attempt(mut self, n: i64) -> E {
        self.0.attempt = Some(n);
        self
    }
    pub(super) fn text(mut self, t: &str) -> E {
        self.0.text = t.into();
        self
    }
    pub(super) fn data(mut self, d: Value) -> E {
        self.0.data = Some(d);
        self
    }
}

/// An event log: seq from 1, a second apart from 1_000_000.
#[derive(Default, Clone)]
pub(super) struct Log {
    pub events: Vec<Event>,
}

impl Log {
    pub(super) fn add(&mut self, e: E) {
        let n = self.events.len() as i64;
        self.events.push(Event { seq: n + 1, ts: 1_000_000 + n * 1000, ..e.0 });
    }

    pub(super) fn state(&self) -> RunState {
        fold(&self.events).state
    }

    /// The view as the person reads it in Chinese, members named by their ids.
    pub(super) fn view(&self) -> RunView {
        fold(&self.events).view(true, false, &Words::of(Lang::Zh, &json!({})), Layout::default())
    }

    pub(super) fn view_in(&self, lang: Lang, live: bool, config: &Value) -> RunView {
        fold(&self.events).view(live, false, &Words::of(lang, config), Layout::default())
    }

    pub(super) fn last_ts(&self) -> i64 {
        self.events.last().unwrap().ts
    }
}

/// A goal with two tasks on the board and the first one at work.
pub(super) fn goal_run() -> Log {
    let mut log = Log::default();
    log.add(
        ev("run.created")
            .actor("user")
            .data(json!({ "mode": "goal", "goal": "给 divide 增加除零保护\n细节……", "cwd": "/tmp/x", "git": true })),
    );
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "调研", "agent": "researcher", "deps": [], "accept": ["列出调用方"] },
        { "id": "T2", "title": "实现", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过", "有除零保护"] },
    ]})));
    log.add(ev("task.ready").task("T1"));
    log.add(
        ev("attempt.started")
            .to("researcher")
            .task("T1")
            .attempt(1)
            .data(json!({ "session": "s1", "workspace": "工作目录 /tmp/x" })),
    );
    log
}

fn status(s: &RunState, task: &str) -> TaskShown {
    s.task(task).unwrap().status
}

fn msg<'a>(s: &'a RunState, id: &str) -> &'a MsgView {
    s.msgs.iter().find(|m| m.id == id).unwrap()
}

fn phase(s: &RunState, agent: &str) -> AgentPhase {
    s.agents[agent].phase
}

#[test]
fn a_goal_run_tracks_its_tasks_and_agents() {
    let mut log = goal_run();
    let s1 = log.state();
    assert_eq!(s1.run.status, Some(RunStatus::Running));
    assert_eq!(s1.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["T1", "T2"]);
    assert_eq!(status(&s1, "T1"), TaskShown::Running);
    assert!(s1.task("T1").unwrap().session.is_some());
    assert_eq!(status(&s1, "T2"), TaskShown::Pending);
    assert_eq!(phase(&s1, "researcher"), AgentPhase::Thinking);

    let checks = json!([{ "criterion": "列出调用方", "passed": true, "evidence": "calc.py:3" }]);
    log.add(
        ev("result.submitted")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .data(json!({ "summary": "三处调用", "evidence": ["calc.py:3"], "checks": checks })),
    );
    log.add(
        ev("task.succeeded")
            .task("T1")
            .attempt(1)
            .data(json!({ "summary": "三处调用", "evidence": ["calc.py:3"], "risks": [], "checks": checks })),
    );
    let s2 = log.state();
    let t1 = s2.task("T1").unwrap();
    assert_eq!(t1.status, TaskShown::Succeeded);
    assert_eq!(t1.checks[0]["passed"], true);
    assert!(t1.session.is_none());
    assert_eq!(t1.attempts.last().unwrap().status, AttemptShown::Succeeded);
    assert_eq!(phase(&s2, "researcher"), AgentPhase::Idle);
}

#[test]
fn requests_enter_and_leave_the_inbox() {
    let mut log = goal_run();
    log.add(ev("approval.requested").actor("researcher").task("T1").attempt(1).text("运行命令").data(json!({
        "key": "a-1", "command": "rm -rf build", "options": [{ "optionId": "ok", "name": "允许", "kind": "allow_once" }],
    })));
    log.add(ev("question.escalated").actor("lead").data(json!({
        "key": "q-lead-1", "question": "用哪个框架？", "options": ["A", "B"], "recommend": "A", "reason": "影响整体",
    })));
    let s = log.state();
    assert_eq!(s.inbox.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["a-1", "q-lead-1"]);
    assert_eq!(status(&s, "T1"), TaskShown::WaitingApproval);
    assert!(s.inbox[1].task.is_none());
    assert_eq!(s.inbox[1].d["options"], json!(["A", "B"]));
    assert_eq!(phase(&s, "researcher"), AgentPhase::WaitingApproval);

    log.add(ev("approval.granted").actor("user").to("researcher").task("T1").data(json!({ "key": "a-1" })));
    log.add(ev("question.answered").actor("user").to("lead").data(json!({ "key": "q-lead-1", "answer": "A" })));
    let s2 = log.state();
    assert!(s2.inbox.is_empty());
    assert_eq!(status(&s2, "T1"), TaskShown::Running);
    assert_eq!(s2.answers["a-1"], "granted");
}

#[test]
fn failures_stay_with_the_lead() {
    let mut log = goal_run();
    log.add(ev("verification.failed").task("T1").data(json!({ "failures": ["漏了验收标准"] })));
    log.add(ev("task.failed").task("T1").data(json!({ "error": "用完重试次数" })));
    let v = log.view();
    assert_eq!(status(&v.state, "T1"), TaskShown::Failed);
    // The lead deals with failures: the person is not asked.
    assert!(v.state.inbox.is_empty());
    let failed = v.board.open.iter().find(|o| o.key == "failed:T1").unwrap();
    assert!(!failed.you);
    assert_eq!(failed.title, "T1 失败，等负责人决定");
    assert_eq!(failed.text, "用完重试次数");
}

#[test]
fn messages_are_tracked_until_delivered() {
    let mut log = goal_run();
    log.add(
        ev("user.message")
            .actor("user")
            .to("researcher")
            .task("T1")
            .data(json!({ "id": "m1", "text": "顺便看看测试" })),
    );
    assert_eq!(msg(&log.state(), "m1").delivery, "pending");
    log.add(ev("message.routed").to("researcher").task("T1").data(json!({ "id": "m1", "delivery": "queued" })));
    let s = log.state();
    assert_eq!(msg(&s, "m1").delivery, "queued");
    assert!(msg(&s, "m1").delivered_seq.is_none());
    log.add(ev("message.delivered").to("researcher").task("T1").data(json!({ "ids": ["m1"] })));
    assert_eq!(msg(&log.state(), "m1").delivered_seq, Some(log.events.len() as i64));
}

#[test]
fn a_delivery_is_accepted() {
    let mut log = goal_run();
    log.add(ev("run.delivered").data(json!({
        "summary": "完成", "briefSuggestions": ["定价页在 docs/pricing.md"], "risks": [], "evidence": [], "deliverables": [],
    })));
    let s = log.state();
    assert_eq!(s.run.status, Some(RunStatus::Delivered));
    assert_eq!(s.result().unwrap().d["briefSuggestions"], json!(["定价页在 docs/pricing.md"]));
    log.add(ev("risk.accepted").actor("user"));
    let s = log.state();
    assert_eq!(s.run.status, Some(RunStatus::Accepted));
    assert!(s.result().unwrap().accepted.is_some());
}

#[test]
fn a_reopened_task_drops_its_amendments() {
    let mut log = goal_run();
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "x" })));
    log.add(ev("result.amended").actor("lead").task("T1").data(json!({ "text": "以修订为准" })));
    assert_eq!(log.state().task("T1").unwrap().amendments, ["以修订为准"]);
    log.add(ev("task.reopened").actor("lead").task("T1").data(json!({ "instructions": "再查一遍" })));
    let s = log.state();
    let t1 = s.task("T1").unwrap();
    assert!(t1.amendments.is_empty());
    assert_eq!(t1.desc, "再查一遍");
    assert_eq!(t1.status, TaskShown::Ready);
}

/// The progress timeline draws each attempt from its start to its end.
#[test]
fn attempts_record_when_they_end() {
    let mut log = goal_run();
    assert!(log.state().task("T1").unwrap().attempts.last().unwrap().end.is_none());
    log.add(ev("attempt.failed").task("T1").attempt(1).data(json!({ "error": "超时" })));
    assert_eq!(log.state().task("T1").unwrap().attempts.last().unwrap().end, Some(log.last_ts()));
    log.add(ev("attempt.started").to("researcher").task("T1").attempt(2).data(json!({ "session": "s2" })));
    log.add(ev("run.interrupted"));
    let s = log.state();
    let t1 = s.task("T1").unwrap();
    assert_eq!(t1.attempts.len(), 2);
    assert_eq!(t1.attempts.last().unwrap().end, Some(log.last_ts()));
}

/// The task page reads the checks, evidence and risks from the conclusion event too, so a replayed
/// or resumed run shows the same evidence; the independent verifier is kept apart.
#[test]
fn a_conclusion_keeps_methods_evidence_risks_and_its_verifier() {
    let mut log = goal_run();
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "调研", "agent": "researcher", "deps": [], "accept": ["列出调用方"] },
        { "id": "T2", "title": "实现", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过", "有除零保护"] },
        { "id": "V1", "title": "验证", "agent": "reviewer", "deps": ["T2"], "accept": ["复现行为"], "verifies": "T2" },
    ]})));
    log.add(ev("result.submitted").actor("implementer").task("T2").attempt(1).data(json!({
        "summary": "加了保护", "evidence": ["calc.py:12", "docs/calc.md"], "risks": ["没有覆盖负数"],
        "checks": [
            { "criterion": "测试通过", "passed": true, "method": "test", "evidence": "cargo test 通过" },
            { "criterion": "有除零保护", "passed": true, "method": "reasoning", "evidence": "调用方只有这一处" },
        ],
    })));
    log.add(ev("task.succeeded").task("T2").attempt(1).data(json!({
        "summary": "加了保护", "evidence": ["calc.py:12", "docs/calc.md"], "risks": ["没有覆盖负数"], "traceGaps": [],
        "checks": [{ "criterion": "测试通过", "passed": true, "method": "test", "evidence": "cargo test 通过" }],
    })));
    log.add(ev("task.succeeded").task("V1").attempt(1).data(json!({
        "summary": "复现了", "evidence": ["python3 calc.py"], "risks": [],
        "checks": [{ "criterion": "复现行为", "passed": true, "method": "command", "evidence": "python3 calc.py → 除零提示" }],
    })));
    let s = log.state();
    let t2 = s.task("T2").unwrap();
    assert_eq!(t2.checks.len(), 1, "the conclusion's checks are kept");
    assert_eq!(t2.checks[0]["method"], "test");
    assert_eq!(t2.risks, ["没有覆盖负数"], "the executor's risks stay with the task");
    assert_eq!(t2.evidence.len(), 2);
    let verifiers: Vec<&TaskView> = s.tasks.iter().filter(|t| t.verifies == "T2").collect();
    assert_eq!(verifiers.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["V1"]);
    assert_eq!(verifiers[0].checks[0]["method"], "command");
    assert!(!s.tasks.iter().any(|t| t.verifies == "T1"));
}

/// The lead sending a task back on a verification's findings is a decision on the board: which
/// session, whose findings and the lead's note. The verification it brings back loses its verdict.
#[test]
fn a_rework_is_the_leads_decision_and_clears_the_old_verdict() {
    let mut log = goal_run();
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "初版" })));
    log.add(ev("task.succeeded").task("T2").attempt(1).data(json!({ "summary": "空输入崩溃", "verdict": "rejected" })));
    assert_eq!(log.state().task("T2").unwrap().verdict.as_deref(), Some("rejected"));
    log.add(ev("task.reopened").actor("lead").task("T1").data(json!({
        "reason": "lead.retry", "session": "continue", "from": "T2", "note": "空输入按错误处理",
    })));
    log.add(ev("task.reopened").task("T2").data(json!({ "reason": "target.reworked", "target": "T1" })));
    let v = log.view();
    assert_eq!(status(&v.state, "T1"), TaskShown::Ready);
    assert!(v.state.task("T2").unwrap().verdict.is_none());
    let d = v.board.decisions.iter().find(|d| d.title == "负责人让 T1 返工").unwrap();
    assert!(d.text.contains("会话：接上原会话"), "{}", d.text);
    assert!(d.text.contains("依据 T2 的意见"));
    assert!(d.text.contains("空输入按错误处理"));
    assert_eq!(d.tasks, ["T1", "T2"]);
    let en = log.view_in(Lang::En, true, &json!({}));
    let d = en.board.decisions.iter().find(|d| d.title == "Lead sent T1 back for rework").unwrap();
    assert!(d.text.contains("Session: picks up its own") && d.text.contains("On T2's findings"), "{}", d.text);
}

/// A run in its own worktree knows its repository and branch, and whether a delivery (or the
/// acceptance) merged its result.
#[test]
fn a_worktree_run_says_where_its_result_is() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({
        "goal": "g", "cwd": "/h/work/runs/r1", "git": true,
        "worktree": { "repo": "/code/app", "included": [".env"], "branch": "main" },
    })));
    let s = log.state();
    assert!(s.run.repo == "/code/app" && s.run.branch == "main" && !s.run.merged);
    log.add(ev("risk.accepted").actor("user").data(json!({ "branch": "main", "commit": "abc", "repo": "/code/app" })));
    assert!(log.state().run.merged);
    // A delivery merged before the person accepted: the result is on the branch.
    let mut delivered = Log::default();
    delivered.add(ev("run.created").actor("user").data(json!({
        "goal": "g", "cwd": "/h/work/runs/r3", "git": true, "worktree": { "repo": "/code/app", "branch": "main" },
    })));
    delivered
        .add(ev("run.merged").data(json!({ "branch": "main", "commit": "def", "result": "def", "repo": "/code/app" })));
    let merged = delivered.view();
    assert!(merged.state.run.merged && merged.state.run.status != Some(RunStatus::Accepted));
    assert_eq!(merged.board.decisions.iter().map(|d| d.title.as_str()).collect::<Vec<_>>(), ["交付时合并到你的分支"]);
    // Started on a detached HEAD; an acceptance with nothing new merges nothing.
    let mut detached = Log::default();
    detached.add(ev("run.created").actor("user").data(json!({
        "goal": "g", "cwd": "/h/work/runs/r2", "git": true, "worktree": { "repo": "/code/app", "branch": null },
    })));
    detached
        .add(ev("risk.accepted").actor("user").data(json!({ "branch": null, "commit": null, "repo": "/code/app" })));
    let s = detached.state();
    assert!(s.run.branch.is_empty() && !s.run.merged);
}

/// One turn rule for every session: who holds it and whether its turn ended move together.
#[test]
fn a_sessions_turn_follows_its_events() {
    let after = |turn: SessionTurn, kind: &str, d: Value| {
        let e = ev(kind).actor("m1").data(d.clone()).0;
        turn.after(&e, &d)
    };
    let mut turn = SessionTurn::OPENED;
    turn = after(turn, "agent.status", json!({ "turn": "idle" }));
    assert_eq!(turn, SessionTurn { person: false, idle: true });
    turn = after(turn, "control.taken", json!({}));
    assert!(turn.person && turn.idle, "taking over keeps the turn's state");
    turn = after(turn, "user.message", json!({ "delivery": "prompt" }));
    assert_eq!(turn, SessionTurn { person: true, idle: false });
    assert_eq!(after(turn, "session.interrupted", json!({ "finish": true })), turn, "finishing is not taking over");
    assert_eq!(after(turn, "control.released", json!({})), SessionTurn::OPENED);
}

/// A queued run is only its goal until it starts.
#[test]
fn a_queued_run_shows_its_goal_until_it_starts() {
    let mut log = Log::default();
    log.add(
        ev("run.queued")
            .actor("user")
            .data(json!({ "goal": "修复导出乱码", "workspace": { "id": "w1", "name": "W" }, "images": [] })),
    );
    let q = log.state();
    assert_eq!(q.run.status, Some(RunStatus::Queued));
    assert_eq!(q.run.goal, "修复导出乱码");
    log.add(ev("run.starting").text("有了空位，开始准备"));
    assert_eq!(log.state().run.status, Some(RunStatus::Planning));
    log.add(ev("run.created").actor("user").data(json!({ "goal": "修复导出乱码", "cwd": "/tmp/x", "git": true })));
    assert_eq!(log.state().run.status, Some(RunStatus::Running));
    let mut again = Log::default();
    again.add(ev("run.queued").actor("user").data(json!({ "goal": "g", "images": [] })));
    again.add(ev("run.starting"));
    again.add(ev("run.requeued"));
    assert_eq!(again.state().run.status, Some(RunStatus::Queued));
}

/// A task may be called `lead`: its turn ending and the lead's are two inbox entries, and the lead
/// taking its session back leaves the task's entry in place (rule 10: owner as a field).
#[test]
fn a_task_named_lead_keeps_its_own_inbox_entry() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "mode": "goal", "goal": "g", "cwd": "/tmp/x", "git": true })));
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "lead", "title": "叫 lead 的子任务", "agent": "m1", "deps": [], "accept": [] },
    ]})));
    log.add(ev("attempt.started").to("m1").task("lead").attempt(1).data(json!({ "session": "s1" })));
    log.add(ev("agent.status").actor("m1").task("lead").data(json!({ "turn": "idle" })));
    log.add(ev("agent.status").actor("lead").data(json!({ "turn": "idle" })));
    let refs = |s: &RunState| s.inbox.iter().map(|r| (r.task.clone(), r.agent.clone())).collect::<Vec<_>>();
    assert_eq!(refs(&log.state()), [(Some("lead".to_string()), None), (None, Some("lead".to_string()))]);
    log.add(ev("control.released").actor("user").to("lead"));
    assert_eq!(refs(&log.state()), [(Some("lead".to_string()), None)]);
}

/// A chat is known from its creation; its session's idle turn is its normal state, so it waits
/// on nobody.
pub(super) fn chat_opened() -> Log {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({
        "kind": "chat", "cwd": "/Users/me/.janus/chats/r1", "git": false, "role": { "id": "m1", "name": "研究员" },
    })));
    log.add(
        ev("user.message")
            .actor("user")
            .to("lead")
            .text("你好")
            .data(json!({ "id": "m1", "text": "你好", "delivery": "pending" })),
    );
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("开始对话")
            .data(json!({ "phase": "thinking", "session": "s1", "mode": "chat" })),
    );
    log.add(ev("agent.message").actor("lead").text("回复：你好").data(json!({ "text": "回复：你好" })));
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("这一轮已结束，等待你的消息")
            .data(json!({ "phase": "waiting_for_input", "turn": "idle" })),
    );
    log
}

#[test]
fn a_chat_waiting_for_the_next_message_asks_nothing() {
    let v = chat_opened().view();
    assert_eq!(v.state.run.kind, RunKind::Chat);
    assert_eq!(v.state.run.status, Some(RunStatus::Running));
    assert!(v.state.inbox.is_empty());
    assert!(v.board.open.is_empty());
}

/// A chat's session closing reads as interrupted; it opening again means it is going; a failure
/// keeps its error.
#[test]
fn a_chat_reopens_and_keeps_its_failure() {
    let mut log = chat_opened();
    log.add(ev("run.interrupted").text("编排器重启"));
    assert_eq!(log.state().run.status, Some(RunStatus::Interrupted));
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("恢复了之前的会话")
            .data(json!({ "phase": "thinking", "session": "s1", "mode": "chat" })),
    );
    assert_eq!(log.state().run.status, Some(RunStatus::Running));
    log.add(ev("run.failed").text("对话的会话结束了").data(json!({ "error": "Agent 进程已退出" })));
    let s = log.state();
    assert_eq!(s.run.status, Some(RunStatus::Failed));
    assert_eq!(s.run.error, "Agent 进程已退出");
    let mut new = Log::default();
    new.add(ev("run.created").actor("user").data(json!({ "kind": "chat", "cwd": "/c", "git": false })));
    assert_eq!(new.state().run.status, Some(RunStatus::Running), "run.created starts it, like any run");
}

/// Work written to the board after a delivery makes the run running again; the next delivery makes
/// it delivered again.
#[test]
fn new_work_after_a_delivery_runs_again() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "goal": "目标", "cwd": "/tmp/x" })));
    log.add(ev("run.delivered").data(json!({ "summary": "第一次" })));
    assert_eq!(log.state().run.status, Some(RunStatus::Delivered));
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T9", "title": "追加", "agent": "m1", "deps": [], "accept": ["做完"] },
    ]})));
    assert_eq!(log.state().run.status, Some(RunStatus::Running));
    log.add(ev("run.delivered").data(json!({ "summary": "第二次" })));
    assert_eq!(log.state().run.status, Some(RunStatus::Delivered));
}

/// Rework that revises the criteria shows the new ones, and a run resumed after a restart is no
/// longer paused; replaying to before the rework still shows the old criteria.
#[test]
fn rework_and_resume_show_what_the_server_does() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "goal": "目标", "cwd": "/tmp/x" })));
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "t", "agent": "m1", "deps": [], "accept": ["A"] },
    ]})));
    log.add(ev("run.paused").actor("user"));
    let before = log.state();
    assert!(before.task("T1").unwrap().accept == ["A"] && before.run.paused);
    log.add(
        ev("task.reopened")
            .actor("lead")
            .task("T1")
            .data(json!({ "reason": "lead.retry", "session": "new", "accept": ["B", "C"] })),
    );
    log.add(ev("run.interrupted"));
    log.add(ev("run.recovered").actor("user").data(json!({ "phase": "running", "tasks": [] })));
    let after = log.state();
    assert_eq!(after.task("T1").unwrap().accept, ["B", "C"]);
    assert!(!after.run.paused);
    assert_eq!(fold(&log.events[..3]).state.task("T1").unwrap().accept, ["A"]);
}

/// A Loop sleeping between wakes asks nothing of the person; its question outlives its session,
/// a restart included, and its session opening again means it is going.
#[test]
fn a_sleeping_loop_asks_nothing_and_its_question_outlives_its_session() {
    let mut log = Log::default();
    log.add(
        ev("run.created")
            .actor("user")
            .data(json!({ "kind": "loop", "cwd": "/repo", "git": false, "loop": { "id": "l1", "name": "巡检" } })),
    );
    log.add(ev("loop.woke").data(json!({ "reason": "manual", "n": 1 })));
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("开始对话")
            .data(json!({ "phase": "thinking", "session": "s1", "mode": "chat" })),
    );
    log.add(ev("question.escalated").actor("lead").to("user").text("负责人请你决定：先修哪个？").data(json!({
        "key": "q1", "question": "先修哪个？", "options": ["导出"], "from": "lead",
    })));
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("这一轮已结束，等待你的消息")
            .data(json!({ "phase": "waiting_for_input", "turn": "idle" })),
    );
    let s = log.state();
    assert!(s.run.kind == RunKind::Loop && s.run.kind.single_session());
    assert_eq!(s.inbox.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["q1"], "only its question waits");
    log.add(ev("run.interrupted").text("编排器重启"));
    let s = log.state();
    assert_eq!(s.run.status, Some(RunStatus::Interrupted));
    assert_eq!(
        s.inbox.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["q1"],
        "a restart does not take its question away"
    );
    log.add(
        ev("agent.status")
            .actor("lead")
            .text("恢复了之前的会话")
            .data(json!({ "phase": "thinking", "session": "s1", "mode": "chat" })),
    );
    assert_eq!(log.state().run.status, Some(RunStatus::Running));
}

#[test]
fn messages_keep_their_images() {
    let path = "/h/runs/r/attachments/1.png";
    let mut log = Log::default();
    log.add(
        ev("run.created")
            .actor("user")
            .data(json!({ "goal": "照着图改", "images": [{ "path": "/h/runs/r/attachments/0.png" }] })),
    );
    log.add(ev("user.message").actor("user").to("lead").data(json!({
        "id": "m1", "text": format!("看这里\n\nAttached images:\n[Image 1] {path}"), "delivery": "pending", "images": [{ "path": path }],
    })));
    let s = log.state();
    assert_eq!(s.run.images, ["/h/runs/r/attachments/0.png"]);
    assert_eq!(msg(&s, "m1").images, [path]);
}

#[test]
fn the_lines_naming_images_are_hidden() {
    assert_eq!(
        visible_text(
            "看看这个报错\n\nAttached images:\n[Image 1] /h/runs/r/attachments/1.png\n[Image 2] /h/runs/r/attachments/2.png"
        ),
        "看看这个报错"
    );
    assert_eq!(visible_text("Review the user's attached images:\n[Image 1] /h/runs/r/attachments/1.png"), "");
    assert_eq!(visible_text("普通消息\n附图在后面"), "普通消息\n附图在后面", "text without image lines is unchanged");
    assert_eq!(image_paths(&json!({ "images": [{ "path": "/a.png" }, { "path": "" }] })), ["/a.png"]);
}

/// A replay is the same fold over the first events, and folding events one by one into a kept fold
/// (`Views`) gives what a fresh fold over all of them gives.
#[test]
fn a_kept_fold_brought_up_to_date_equals_a_fresh_one() {
    let log = goal_run();
    let views = Views::default();
    let first = views.current("r1", |after| Ok(log.events[after as usize..2].to_vec()), Fold::clone).unwrap();
    assert_eq!(first.seq(), 2);
    let all = views.current("r1", |after| Ok(log.events[after as usize..].to_vec()), Fold::clone).unwrap();
    assert_eq!(all.seq(), log.events.len() as i64);
    let fresh =
        serde_json::to_value(fold(&log.events).view(true, false, &Words::of(Lang::Zh, &json!({})), Layout::default()))
            .unwrap();
    assert_eq!(
        serde_json::to_value(all.view(true, false, &Words::of(Lang::Zh, &json!({})), Layout::default())).unwrap(),
        fresh
    );
}

/// A consolidation's page says what it changed, as its last `memory.consolidated` says, in the
/// person's language; before it finishes there is nothing to say.
#[test]
fn a_consolidation_says_what_it_changed() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "goal": "整理团队记忆" })));
    assert!(log.view().consolidated.is_none());
    log.add(ev("memory.consolidated").data(json!({ "saved": 2, "forgotten": 1, "summary": "合并了重复的笔记。" })));
    let done = log.view().consolidated.unwrap();
    assert_eq!((done.saved, done.forgotten), (2, 1));
    assert_eq!(done.text, "整理完了：改了 2 条，删了 1 条。合并了重复的笔记。");
    let en = log.view_in(Lang::En, true, &json!({})).consolidated.unwrap();
    assert_eq!(en.text, "Consolidated: 2 changed, 1 removed. 合并了重复的笔记。");
}
