//! Acceptance evidence as the person reads it: a task's checks against its criteria, its
//! independent verification, and the delivery's rows and line of numbers.

use super::*;

fn delivered(log: &mut Log, d: Value) {
    log.add(ev("run.delivered").actor("lead").data(d));
}

#[test]
fn a_tasks_checks_pair_with_its_criteria_and_its_verifier_concludes_on_it() {
    let mut log = goal_run();
    // T2 has two criteria; its result checks the first and adds one that names none.
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "V2", "title": "核对实现", "agent": "researcher", "deps": [], "accept": ["测试通过"], "verifies": "T2" },
    ]})));
    log.add(ev("attempt.started").to("implementer").task("T2").attempt(1).data(json!({ "session": "s2" })));
    let checks = json!([
        { "criterion": "测试通过", "passed": true, "evidence": "cargo test", "method": "test" },
        { "criterion": "有除零保护", "passed": false, "evidence": "", "method": "reasoning" },
        { "criterion": "多写的一条", "passed": true, "evidence": "看过" },
    ]);
    log.add(
        ev("task.succeeded").task("T2").attempt(1).data(json!({ "summary": "完成", "evidence": [], "checks": checks })),
    );
    log.add(ev("attempt.started").to("researcher").task("V2").attempt(1).data(json!({ "session": "s3" })));
    log.add(ev("task.succeeded").task("V2").attempt(1).data(json!({
        "summary": "没问题", "evidence": [], "checks": [{ "criterion": "测试通过", "passed": true, "evidence": "跑过" }],
    })));
    let v = log.view();
    let e = &v.evidence["T2"];
    assert_eq!((e.criteria, e.passed), (2, 1));
    assert_eq!(e.counts, "测试 1 条、推理判断 1 条、未说明 1 条");
    assert_eq!(
        e.rows.iter().map(|r| (r.n, r.criterion.as_str())).collect::<Vec<_>>(),
        [(1, "测试通过"), (2, "有除零保护"), (3, "多写的一条")]
    );
    assert_eq!(e.reminders.len(), 2, "reasoning and no method remind, they do not fail: {:?}", e.reminders);
    let ver = &e.verifications[0];
    assert_eq!((ver.task.as_str(), ver.verdict, ver.passed), ("V2", Some(crate::evidence::Verdict::Approved), true));
    assert!(v.evidence["T1"].rows[0].check.is_none(), "a criterion nothing answered yet still has its row");
}

#[test]
fn the_delivery_rows_say_what_needs_attention_and_the_round_comes_to_one_line() {
    let mut log = goal_run();
    let record = json!({
        "task": "T2", "title": "实现", "status": "succeeded", "criteria": 2, "passed": 1,
        "methods": [{ "method": "test", "count": 1 }, { "method": "inspection", "count": 1 }], "unspecified": 1,
        "invalid": [], "reminders": ["第 3 条没有说明证据方法"],
        "checks": [
            { "criterion": "测试通过", "passed": true, "evidence": "cargo test", "method": "test" },
            { "criterion": "有除零保护", "passed": false, "evidence": "读了代码", "method": "inspection" },
            { "criterion": "多的", "passed": true, "evidence": "x" },
        ],
        "verifications": [{ "task": "V2", "status": "succeeded", "verdict": "rejected", "checks": [] }],
        "unverified": false, "traceGaps": [], "risks": ["可能慢"],
    });
    delivered(&mut log, json!({ "summary": "做完了", "acceptance": [record], "changed": ["calc.py", "test.py"] }));
    let v = log.view();
    let d = v.delivery.as_ref().unwrap();
    let row = &d.tasks[0];
    assert_eq!(row.metrics, "通过 1 / 2 条 · 测试 1 条、检查 1 条、未说明 1 条");
    assert_eq!(row.markers, ["自述风险 1 条", "提醒 1 条"]);
    assert!(row.needs_attention, "a criterion failed and the verifier did not confirm it");
    assert_eq!(row.verifications[0]["passed"], false);
    assert_eq!((row.strength.bands.ran, row.strength.bands.read, row.strength.bands.said), (1, 1, 1));
    assert_eq!(row.strength.text, "运行 1 · 检查 1 · 只是说法 1");
    assert_eq!(d.facts, "子任务 1/1 完成 · 验收标准 1/2 达到 · 运行 1 · 检查 1 · 只是说法 1 · 改了 2 个文件");

    let en = log.view_in(Lang::En, true, &json!({}));
    let d = en.delivery.as_ref().unwrap();
    assert_eq!(d.facts, "Subtasks done: 1/1 · Criteria met: 1/2 · Ran 1 · Read 1 · Only said 1 · Files changed: 2");
    assert_eq!(
        d.tasks[0].reminders,
        ["#3 \"多的\" names no evidence method"],
        "reminders follow the reader's language"
    );
}

/// The row a delivery record comes to, read in Chinese.
fn row_of(record: Value) -> AcceptanceRowSeen {
    let mut log = goal_run();
    delivered(&mut log, json!({ "acceptance": [record], "changed": [] }));
    let r = log.view().delivery.unwrap().tasks.remove(0);
    AcceptanceRowSeen { markers: r.markers, open: r.needs_attention }
}

struct AcceptanceRowSeen {
    markers: Vec<String>,
    open: bool,
}

fn record(status: &str, checks: Value, extra: Value) -> Value {
    let mut r = json!({ "task": "T1", "title": "t", "status": status, "criteria": 1, "passed": 1, "checks": checks });
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

#[test]
fn only_what_janus_finds_opens_a_row_and_the_executors_own_reports_only_mark_it() {
    let passed = json!([{ "criterion": "一", "passed": true, "method": "test", "evidence": "x" }]);
    let failed = json!([{ "criterion": "一", "passed": false, "method": "test", "evidence": "x" }]);
    let clean = row_of(record("succeeded", passed.clone(), json!({})));
    assert!(clean.markers.is_empty() && !clean.open);
    assert!(row_of(record("succeeded", failed, json!({ "passed": 0 }))).open, "a failed criterion");
    assert!(row_of(record("succeeded", passed.clone(), json!({ "traceGaps": ["改了没有的文件"] }))).open);
    let risky = row_of(record("succeeded", passed.clone(), json!({ "risks": ["没有测试覆盖"] })));
    assert_eq!((risky.markers, risky.open), (vec!["自述风险 1 条".to_string()], false));
    let unconfirmed = json!({ "verifications": [{ "task": "V1", "passed": false }] });
    assert!(row_of(record("succeeded", passed.clone(), unconfirmed)).open);
    let confirmed = json!({ "verifications": [{ "task": "V1", "passed": true }] });
    assert!(!row_of(record("succeeded", passed.clone(), confirmed)).open);
    let unverified = row_of(record("succeeded", passed.clone(), json!({ "unverified": true })));
    assert_eq!((unverified.markers, unverified.open), (vec!["没有独立验证".to_string()], false));
    let not_done = row_of(record("verification_failed", passed, json!({})));
    assert_eq!(
        (not_done.markers, not_done.open),
        (vec!["验收没通过".to_string()], false),
        "the status alone does not open it"
    );
}

#[test]
fn a_delivery_without_subtasks_says_the_lead_did_it() {
    let mut log = goal_run();
    delivered(&mut log, json!({ "summary": "自己做了", "acceptance": [], "changed": [] }));
    assert_eq!(log.view().delivery.unwrap().facts, "没有子任务，负责人自己完成 · 没有改动文件");
    assert!(goal_run().view().delivery.is_none());
}
