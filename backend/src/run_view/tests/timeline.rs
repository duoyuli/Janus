//! The progress timeline: bars per attempt, what a task waits for, its conclusion and artifacts,
//! and decisions and open items on the lane above.

use super::*;

#[test]
fn tasks_that_started_come_first_and_one_still_waiting_says_for_what() {
    let mut log = goal_run();
    let checks = json!([{ "criterion": "列出调用方", "passed": true, "evidence": "calc.py:3", "method": "test" }]);
    log.add(
        ev("result.submitted")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .data(json!({ "summary": "**三处**调用，见 [calc](calc.py)", "evidence": [], "checks": checks })),
    );
    log.add(
        ev("task.succeeded")
            .task("T1")
            .attempt(1)
            .actor("researcher")
            .data(json!({ "summary": "**三处**调用，见 [calc](calc.py)", "evidence": [], "checks": checks })),
    );
    let v = log.view();
    let t = &v.timeline.tasks;
    assert_eq!(t.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["T1", "T2"]);
    let bar = &t[0].bars[0];
    assert_eq!((bar.id.as_str(), bar.status, bar.running), ("T1#1", TaskShown::Succeeded, false));
    assert!(bar.end.is_some());
    let result = t[0].result.as_ref().unwrap();
    assert_eq!(result.title, "三处调用，见 calc · 证据 测试 1 条", "Markdown dropped, the methods after it");
    assert!(t[1].bars.is_empty());
    assert_eq!(t[1].waiting.as_deref(), Some("等待调度"), "its dependency is done");

    let v = log.view_in(Lang::En, true, &json!({}));
    assert_eq!(v.timeline.tasks[0].result.as_ref().unwrap().title, "三处调用，见 calc · Evidence: test 1");
}

#[test]
fn an_attempt_at_work_has_no_end_and_a_dependency_not_done_is_waited_for() {
    let v = goal_run().view();
    let t1 = &v.timeline.tasks[0];
    assert_eq!((t1.bars[0].end, t1.bars[0].running, t1.bars[0].status), (None, true, TaskShown::Running));
    assert_eq!(v.timeline.tasks[1].waiting.as_deref(), Some("等待 T1"));
}

#[test]
fn the_persons_requests_sit_on_the_lane_above_with_their_names() {
    let mut log = goal_run();
    log.add(ev("approval.requested").actor("researcher").task("T1").attempt(1).text("运行命令").data(json!({
        "key": "a-1", "command": "ls", "options": [],
    })));
    let v = log.view();
    let m = v.timeline.milestones.iter().find(|m| m.kind == crate::run_view::timeline::MarkKind::Open).unwrap();
    assert_eq!((m.seq, m.ts, m.you, m.by.as_str()), (5, log.last_ts(), true, "researcher"));
    assert_eq!(m.what, Some(crate::run_view::board::OpenKind::Approval));
    assert_eq!(m.label, "等你审批");
}
