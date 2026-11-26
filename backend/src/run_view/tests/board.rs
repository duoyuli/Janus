//! The shared board's facts: what holds work up, results and who took them, decisions.

use super::*;
use crate::run_view::board::OpenKind;

fn titles(v: &RunView) -> Vec<String> {
    v.board.open.iter().map(|o| o.title.clone()).collect()
}

#[test]
fn a_stalled_task_stays_open_until_the_agent_moves() {
    let stuck = "T1 可能卡住了".to_string();
    let mut log = goal_run();
    log.add(ev("task.stalled").to("lead").task("T1").attempt(1).data(json!({ "reason": "idle", "idleMs": 600000 })));
    let v = log.view();
    assert!(v.board.open.iter().any(|o| o.title == stuck && !o.you));
    assert!(log.view_in(Lang::En, true, &json!({})).board.open.iter().any(|o| o.title == "T1 may be stuck"));
    log.add(
        ev("tool.call")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .data(json!({ "tool": "read", "status": "completed" })),
    );
    assert!(!titles(&log.view()).contains(&stuck));
    log.add(ev("task.stalled").to("lead").task("T1").attempt(1).data(json!({ "reason": "idle", "idleMs": 600000 })));
    log.add(ev("task.stalled").to("lead").task("T1").attempt(1).data(json!({ "reason": "idle", "resolved": true })));
    assert!(!titles(&log.view()).contains(&stuck), "the backend saw the agent move again");
    log.add(ev("task.stalled").to("lead").task("T1").attempt(1).data(json!({ "reason": "loop", "repeats": 5 })));
    log.add(ev("attempt.started").to("researcher").task("T1").attempt(2).data(json!({ "session": "s2" })));
    assert!(!titles(&log.view()).contains(&stuck), "a new attempt starts clean");
    log.add(ev("task.stalled").to("lead").task("T1").attempt(2).data(json!({ "reason": "loop", "repeats": 5 })));
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "x" })));
    assert!(!titles(&log.view()).contains(&stuck));
}

#[test]
fn lead_stalls_and_conflicts_open_on_the_board() {
    let mut log = goal_run();
    log.add(ev("lead.stalled").to("user").data(json!({ "reason": "idle", "idleMs": 600000 })));
    log.add(ev("task.conflict").to("lead").task("T1").data(json!({ "other": "T2", "paths": ["calc.py"] })));
    let v = log.view();
    assert!(
        v.board.open.iter().any(|o| o.title == "负责人可能卡住了" && o.you),
        "nobody but the person watches the lead"
    );
    assert!(v.board.open.iter().any(|o| o.title == "T1 和 T2 改了同一个文件" && o.text == "calc.py" && !o.you));
    let en = log.view_in(Lang::En, true, &json!({}));
    assert!(titles(&en).contains(&"The lead may be stuck".to_string()));
    assert!(titles(&en).contains(&"T1 and T2 edited the same file".to_string()));
    log.add(ev("lead.stalled").to("user").data(json!({ "reason": "idle", "resolved": true })));
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "x" })));
    let v = log.view();
    assert!(!titles(&v).contains(&"负责人可能卡住了".to_string()));
    assert!(!v.board.open.iter().any(|o| o.title.contains("改了同一个文件")), "neither task is running any more");
}

/// A repeated-call report has no explicit resolution: the lead doing anything again closes it.
#[test]
fn a_lead_stall_clears_when_the_lead_moves_again() {
    let mut log = goal_run();
    log.add(ev("lead.stalled").to("user").data(json!({ "reason": "loop", "tool": "Read", "repeats": 5 })));
    assert!(log.view().board.open.iter().any(|o| o.kind == OpenKind::LeadStall && o.you));
    log.add(ev("agent.message").actor("lead").text("换个办法"));
    assert!(!log.view().board.open.iter().any(|o| o.kind == OpenKind::LeadStall));
}

/// A finished run's lead stall is not open any more: only an active run's lead can be stuck.
#[test]
fn a_lead_stall_of_a_finished_run_is_not_open() {
    let mut log = goal_run();
    log.add(ev("lead.stalled").to("user").data(json!({ "reason": "idle" })));
    log.add(ev("run.delivered").data(json!({ "summary": "好了" })));
    assert!(!log.view().board.open.iter().any(|o| o.kind == OpenKind::LeadStall));
}

#[test]
fn traces_open_on_the_board_until_the_lead_acts() {
    let mut log = goal_run();
    log.add(
        ev("task.conflict").to("lead").task("T1").data(json!({ "other": "T2", "paths": ["calc.py"], "owned": true })),
    );
    let v = log.view();
    assert!(v.board.open.iter().any(|o| o.title == "T1 改了 T2 负责的文件" && o.text == "calc.py" && !o.you));
    // The same edit also seen as both tasks editing one file: the overstep says it all.
    log.add(ev("task.conflict").to("lead").task("T1").data(json!({ "other": "T2", "paths": ["calc.py"] })));
    let v = log.view();
    assert_eq!(v.board.open.iter().filter(|o| o.tasks.contains(&"T2".to_string()) && o.text == "calc.py").count(), 1);
    assert_eq!(v.board.open.iter().find(|o| o.text == "calc.py").unwrap().kind, OpenKind::Overstep);
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({
        "summary": "x", "traceGaps": ["改了负责范围（notes.md）以外的文件：calc.py"],
    })));
    let v = log.view();
    assert!(!v.board.open.iter().any(|o| o.title.contains("负责的文件")), "the task that stepped over stopped");
    assert_eq!(v.state.task("T1").unwrap().trace_gaps.len(), 1);
    assert!(v.board.open.iter().any(|o| o.title == "T1 的结果和工作目录对不上" && o.text.contains("calc.py")));
    log.add(ev("result.amended").actor("lead").task("T1").data(json!({ "text": "calc.py 的改动由 T2 负责" })));
    assert!(!log.view().board.open.iter().any(|o| o.title.contains("对不上")), "the lead corrected it");
    log.add(ev("task.reopened").actor("lead").task("T1").data(json!({ "reason": "lead.retry" })));
    assert!(log.state().task("T1").unwrap().trace_gaps.is_empty());
}

#[test]
fn a_steered_message_is_marked_read() {
    let mut log = goal_run();
    log.add(
        ev("user.message")
            .actor("user")
            .to("lead")
            .data(json!({ "id": "m1", "text": "先看 T1", "delivery": "pending" })),
    );
    log.add(ev("message.routed").to("lead").data(json!({ "id": "m1", "delivery": "steer" })));
    assert!(log.state().msgs[0].delivered_seq.is_none());
    log.add(ev("message.delivered").to("lead").data(json!({ "ids": ["m1"], "applied": true })));
    assert!(log.state().msgs[0].delivered_seq.is_some());
}

#[test]
fn board_facts_link_results_to_handoffs() {
    let mut log = goal_run();
    log.add(
        ev("result.submitted")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .data(json!({ "summary": "三处调用", "evidence": ["a"], "risks": ["b"] })),
    );
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "三处调用" })));
    log.add(
        ev("context.handoff")
            .to("implementer")
            .task("T2")
            .data(json!({ "from": "T1", "summary": "三处调用", "artifacts": [] })),
    );
    log.add(ev("user.message").actor("user").to("lead").data(json!({ "text": "注意兼容", "delivery": "steer" })));
    let f = log.view().board;
    assert_eq!(f.results.len(), 1);
    assert_eq!(f.results[0].used_by, ["T2"]);
    assert_eq!(f.handoffs.iter().map(|h| h.to.as_str()).collect::<Vec<_>>(), ["T2"]);
    let decisions: Vec<&str> = f.decisions.iter().map(|d| d.title.as_str()).collect();
    assert!(decisions.contains(&"负责人派出子任务"));
    assert!(decisions.contains(&"给 lead 的补充"));
    let d = f.decisions.iter().find(|d| d.title == "负责人派出子任务").unwrap();
    assert_eq!(d.text, "T1 调研 → researcher\nT2 实现 → implementer（依赖 T1）");
    let en = log.view_in(Lang::En, true, &json!({})).board;
    let titles: Vec<&str> = en.decisions.iter().map(|d| d.title.as_str()).collect();
    assert!(titles.contains(&"The lead dispatched subtasks") && titles.contains(&"Follow-up for lead"), "{titles:?}");
}

/// A message that never reached its agent is marked on its decision once the run is over.
#[test]
fn a_message_that_never_arrived_is_lost_once_the_run_is_over() {
    let mut log = goal_run();
    log.add(
        ev("user.message")
            .actor("user")
            .to("researcher")
            .task("T1")
            .data(json!({ "id": "m1", "text": "顺便", "delivery": "pending" })),
    );
    log.add(ev("message.routed").to("researcher").task("T1").data(json!({ "id": "m1", "delivery": "next" })));
    let seq = log.events[4].seq;
    let lost = |v: &RunView| v.board.decisions.iter().find(|d| d.seq == seq).unwrap().lost;
    assert!(!lost(&log.view()), "still on its way while the run goes on");
    log.add(ev("run.cancelled").actor("user"));
    assert!(lost(&log.view()));
}

#[test]
fn a_verifiers_verdict_split_proposals_and_lead_conflicts() {
    let mut log = Log::default();
    log.add(
        ev("run.created")
            .actor("user")
            .data(json!({ "mode": "goal", "goal": "核验与协作", "cwd": "/tmp/x", "git": true })),
    );
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "实现", "agent": "dev", "deps": [], "owns": ["src/api.rs"], "accept": ["接口可用"] },
        { "id": "V1", "title": "核验", "agent": "qa", "deps": ["T1"], "verifies": "T1", "accept": ["边界通过"] },
        { "id": "T2", "title": "修复", "agent": "dev", "deps": ["V1"], "continues": "T1", "accept": ["修复通过"] },
    ]})));
    log.add(ev("attempt.started").to("dev").task("T1").attempt(1).data(json!({ "session": "s1" })));
    log.add(ev("task.split_requested").actor("dev").to("lead").task("T1").data(json!({
        "reason": "模块过大", "subtasks": [{ "title": "解析器" }, { "title": "生成器" }],
    })));
    log.add(
        ev("task.conflict")
            .to("lead")
            .task("T1")
            .data(json!({ "other": "lead", "byLead": true, "owned": true, "paths": ["src/api.rs"] })),
    );
    let v1 = log.view();
    assert_eq!(v1.state.task("T2").unwrap().continues, "T1");
    assert!(v1.board.open.iter().any(|o| o.kind == OpenKind::SplitRequest && o.title == "T1 请求拆分子任务"));
    assert!(v1.board.open.iter().any(|o| o.kind == OpenKind::Overstep && o.title == "负责人改了 T1 负责的文件"));
    assert!(v1.board.handoffs.iter().any(|h| h.text == "请求拆分（建议 2 个子任务）：模块过大"));
    let en = log.view_in(Lang::En, true, &json!({}));
    assert!(titles(&en).contains(&"T1 requested a subtask split".to_string()));
    assert!(titles(&en).contains(&"The lead edited files owned by T1".to_string()));

    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({
        "summary": "初版", "handoff": "parse() 返回 Result",
        "checks": [{ "criterion": "接口可用", "passed": true, "method": "test", "evidence": "ok" }],
    })));
    log.add(ev("attempt.started").to("qa").task("V1").attempt(1).data(json!({ "session": "s2" })));
    log.add(ev("task.succeeded").task("V1").attempt(1).data(json!({
        "summary": "空输入崩溃", "verdict": "rejected",
        "checks": [{ "criterion": "边界通过", "passed": false, "method": "test", "evidence": "cargo test panic" }],
    })));
    let v2 = log.view();
    assert_eq!(v2.state.task("T1").unwrap().handoff, "parse() 返回 Result");
    assert_eq!(v2.state.task("V1").unwrap().status, TaskShown::Succeeded);
    assert_eq!(v2.state.task("V1").unwrap().verdict.as_deref(), Some("rejected"));
    assert_eq!(v2.state.run.status, Some(RunStatus::Running));
    assert!(!v2.board.open.iter().any(|o| o.kind == OpenKind::SplitRequest));
}

/// A Loop's last wake reaches the person when it changed workspace files or could not wake; the
/// next wake replaces it.
#[test]
fn a_loops_wake_notices_reach_the_person_until_the_next_wake() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "kind": "loop", "cwd": "/tmp/ws", "git": false })));
    log.add(ev("loop.woke").data(json!({ "reason": "timer", "n": 1 })));
    log.add(ev("loop.touched").text("Loop 醒来期间工作区多了 1 处改动").data(json!({ "files": ["notes.md"] })));
    let v = log.view();
    let touched = v.board.open.iter().find(|o| o.kind == OpenKind::LoopTouched).unwrap();
    assert!(touched.you && touched.sub == "notes.md");
    log.add(ev("loop.woke").data(json!({ "reason": "timer", "n": 2 })));
    assert!(!log.view().board.open.iter().any(|o| o.kind == OpenKind::LoopTouched), "the next wake replaces it");
    log.add(ev("loop.failed").text("Loop 没能醒来：工作区不存在"));
    assert!(log.view().board.open.iter().any(|o| o.kind == OpenKind::LoopFailed));
}

/// Open items never share an id, whatever the task ids: `T1` stalled at 24 and `T12` at 4 are two.
#[test]
fn open_items_never_share_an_id() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "mode": "goal", "goal": "g", "cwd": "/tmp/x", "git": true })));
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "a", "agent": "m1", "deps": [], "accept": [] },
        { "id": "T12", "title": "b", "agent": "m2", "deps": [], "accept": [] },
    ]})));
    log.add(ev("attempt.started").to("m2").task("T12").attempt(1).data(json!({ "session": "s2" })));
    log.add(ev("task.stalled").task("T12").text("卡住"));
    while log.events.len() < 22 {
        log.add(ev("task.ready").task("T1"));
    }
    log.add(ev("attempt.started").to("m1").task("T1").attempt(1).data(json!({ "session": "s1" })));
    log.add(ev("task.stalled").task("T1").text("卡住"));
    assert!(log.events[3].seq == 4 && log.events.last().unwrap().seq == 24);
    let open = log.view().board.open;
    assert_eq!(open.iter().filter(|o| o.kind == OpenKind::Stall).count(), 2);
    let keys: std::collections::HashSet<&str> = open.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys.len(), open.len());
}

/// What is the person's comes first, then the newest.
#[test]
fn the_persons_items_come_first() {
    let mut log = goal_run();
    log.add(
        ev("question.escalated").actor("lead").data(json!({ "key": "q1", "question": "哪个？", "options": ["A"] })),
    );
    log.add(ev("task.stalled").task("T1").text("卡住"));
    let open = log.view().board.open;
    assert!(open[0].you && !open.last().unwrap().you);
}
