//! Flow tests of the usage ledger over real runs against the scripted agent
//! (docs/interfaces/20261031-events.md#usage_accounting).

use super::*;

/// The usage ledger over a real run. T1's agent reports tokens with a subagent in its quota rows, a
/// cost and three context snapshots; T2's reports context only; T3's reports two turns that grow,
/// which add up. All of it comes from the events, so the board, a task's detail, the delivery and
/// a replay after a restart say the same numbers.
#[tokio::test(flavor = "multi_thread")]
async fn usage_ledger_covers_a_run_and_survives_a_restart() {
    let h = harness(
        "usage-ledger",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["usage"], "T2": ["usage-quiet"], "T3": ["usage-turns"] }),
    );
    let run =
        start(&h, json!([task("T1", "m1", json!({})), task("T2", "m2", json!({})), task("T3", "m1", json!({}))])).await;
    // T3 ends its first turn with a question; the answered turn reports its own tokens.
    wait_until(&h.app, &run, "T3 asks", |e| task_event(e, "question.asked", "T3")).await;
    let asked = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind.as_str() == "question.asked" && e.task.as_deref() == Some("T3"))
        .unwrap();
    act(&h, &run, json!({ "type": "answer", "key": asked.data.unwrap()["key"], "task": "T3", "answer": "alpha" }));
    wait_until(&h.app, &run, "all done", |e| e.iter().filter(|e| e.kind.as_str() == "task.succeeded").count() == 3)
        .await;

    // The lead reads each finished subtask's time and how full its session got.
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    let t1 = board.split("[完成] T1").nth(1).expect("T1 on the board");
    assert!(t1.contains("\n  用时：") && t1.contains("会话上下文 30%"), "{board}");

    use crate::usage::Completeness;
    let l = crate::usage::ledger(&events(&h.app, &run));
    // The quota rows replace the main-loop usage: 1500 for the main model and 500 for a subagent.
    let t1 = l.task("T1").unwrap();
    assert_eq!(
        (t1.tokens.total, t1.tokens.input, t1.tokens.output, t1.tokens.cached_read),
        (Some(2000), Some(1400), Some(400), Some(200)),
        "{t1:?}"
    );
    assert_eq!(t1.completeness, Completeness::Complete);
    assert_eq!(
        (t1.context.used, t1.context.peak, t1.context.size, t1.context.reports),
        (Some(61000), Some(61000), Some(200000), 3),
        "snapshots are never added to the tokens"
    );
    assert_eq!((t1.cost.lines.len(), t1.cost.lines[0].currency.as_str(), t1.cost.lines[0].amount), (1, "USD", 0.25));
    // A session that only reports context has no token usage: 「未上报」, not 0.
    let t2 = l.task("T2").unwrap();
    assert_eq!((t2.tokens.total, t2.completeness, t2.context.used), (None, Completeness::Missing, Some(18000)));
    // Two turns that grow, 1000 then 2500: each is the work of its own prompt.
    assert_eq!((l.task("T3").unwrap().tokens.total, l.task("T3").unwrap().turns), (Some(3500), 2));
    // The lead reported nothing, so the run is partial; no cost is invented.
    assert_eq!((l.run.tokens.total, l.run.completeness), (Some(5500), Completeness::Partial));
    assert!(l.run.missing_sessions >= 1, "the lead reported no tokens: {:?}", l.run);
    assert!(
        l.summary_line().starts_with("用量：5,500 tokens（部分上报） · 费用：0.25 USD · 耗时 "),
        "{}",
        l.summary_line()
    );

    // The team view shows who is on the team, not what they spent; a task's detail carries the
    // numbers and answers when usage is missing.
    let (text, err) = tool(&h.app, &run, "lead", "team", json!({})).await;
    assert!(!err && !text.contains("tokens") && !text.contains("用量"), "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T1" })).await;
    assert!(
        !err && text.contains("2,000 tokens") && text.contains("0.25 USD") && text.contains("上下文 61,000 / 200,000"),
        "{text}"
    );
    let (text, err) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T2" })).await;
    assert!(!err && text.contains("未上报") && !text.contains("0 tokens"), "{text}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let delivered = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap();
    let usage = delivered.data.as_ref().unwrap()["usage"].as_str().unwrap_or_default();
    assert!(usage.starts_with("用量 5,500 tokens（部分上报） · 费用：0.25 USD"), "{usage}");

    // The same events, read back by a new process, give the same ledger.
    let last = evs.last().unwrap().seq;
    let before = crate::usage::ledger(&evs);
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    let replayed: Vec<Event> = app2.db.events(&run, 0).unwrap().into_iter().filter(|e| e.seq <= last).collect();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(crate::usage::ledger(&replayed)).unwrap(),
        "replay and restart give the same numbers"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A cancelled run: the attempt that was running is marked cancelled — Janus writes no attempt
/// event for it — while what the other sessions already reported stays counted, and the ledger
/// still answers.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_run_marks_the_running_attempt_and_keeps_reported_usage() {
    let h = harness("usage-cancel", &[("lead", false), ("m1", true)], json!({ "T1": ["usage"], "T2": ["slow"] }));
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T1"))
    })
    .await;
    wait_until(&h.app, &run, "T2 running", |e| {
        e.iter().any(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("T2"))
    })
    .await;
    act(&h, &run, json!({ "type": "cancel" }));
    wait_until(&h.app, &run, "cancel", |e| has(e, "run.cancelled")).await;
    let l = crate::usage::ledger(&events(&h.app, &run));
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(2000), "usage reported before the cancel stays");
    let t2 = l.attempts.iter().find(|a| a.task == "T2").unwrap();
    assert_eq!(t2.state, crate::usage::AttemptState::Cancelled);
    assert_eq!(t2.note.as_deref(), Some("任务被取消"));
    assert!(t2.duration_ms.is_some_and(|ms| ms >= 0), "{:?}", t2);
    assert_eq!(t2.tokens.total, None, "the cancelled attempt reported nothing: {:?}", t2);
    assert!(
        l.task_lines("T2", &crate::usage::Names::new(), crate::lang::Lang::Zh).iter().any(|x| x.contains("已取消")),
        "{:?}",
        l.task_lines("T2", &crate::usage::Names::new(), crate::lang::Lang::Zh)
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}
