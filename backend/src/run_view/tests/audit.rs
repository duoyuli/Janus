//! The audit page's system rules over the log.

use super::*;

fn kept(v: &RunView) -> Vec<bool> {
    v.audit.iter().map(|r| r.ok).collect()
}

#[test]
fn a_clean_log_keeps_every_rule_and_says_what_it_counted() {
    let mut log = goal_run();
    log.add(ev("result.submitted").actor("researcher").task("T1").attempt(1).data(json!({ "summary": "好" })));
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "好", "evidence": ["calc.py:3"] })));
    let v = log.view();
    assert_eq!(kept(&v), [true; 5]);
    assert_eq!(
        v.audit.iter().map(|r| r.note.as_str()).collect::<Vec<_>>(),
        ["1 次尝试", "1 个子任务", "Agent 只提交进度与结果", "1 次完成", "0 个问题"]
    );
    assert_eq!(log.view_in(Lang::En, true, &json!({})).audit[0].note, "Attempts: 1");
}

#[test]
fn each_broken_rule_is_reported() {
    let mut log = goal_run();
    // A second attempt while the first runs, numbered out of order, started by an agent.
    log.add(ev("attempt.started").actor("researcher").task("T1").attempt(3));
    // Done without a submitted result or evidence.
    log.add(ev("task.succeeded").task("T1").attempt(3).data(json!({ "summary": "" })));
    // A question with nothing to choose.
    log.add(
        ev("question.asked").actor("implementer").task("T2").data(json!({ "question": "怎么办？", "options": [] })),
    );
    assert_eq!(kept(&log.view()), [false; 5]);
}
