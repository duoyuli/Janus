//! Flow tests of acceptance evidence: evidence methods on the board, in `task_detail` and in the
//! delivery, after a restart, and an illegal method failing the result check
//! (docs/interfaces/20261031-events.md#evidence_methods).

use super::*;

/// Every criterion records how it was checked. The board message, `task_detail` and the
/// delivery keep all checks, all evidence and the independent verification, and a restart
/// reads the same values back from the event log.
#[tokio::test(flavor = "multi_thread")]
async fn evidence_methods_are_summarised_and_survive_a_restart() {
    let h = harness("evidence", &[("lead", false), ("m1", true), ("m2", true)], json!({ "T1": ["methods"] }));
    let accept = json!(["结果一写清楚", "结果二写清楚", "结果三写清楚", "结果四写清楚", "结果五写清楚"]);
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "accept": accept })),
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["独立核对 T1"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "V1", |e| task_event(e, "task.succeeded", "V1")).await;
    let evs = events(&h.app, &run);
    let submitted =
        evs.iter().find(|e| e.kind.as_str() == "result.submitted" && e.task.as_deref() == Some("T1")).unwrap();
    let names: Vec<&str> = submitted.data.as_ref().unwrap()["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["method"].as_str())
        .collect();
    assert_eq!(
        names,
        ["test", "command", "inspection", "reasoning", "unverified"],
        "every method reaches the event log"
    );

    // The board message: totals, method counts, the reminders and the independent verification.
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[完成] T1"), "{board}");
    assert!(board.contains("验收标准：5 条，通过 5 条"), "{board}");
    assert!(board.contains("证据：测试 1 条、命令 1 条、检查 1 条、推理判断 1 条、未核实 1 条"), "{board}");
    assert!(
        board.contains("提醒（不影响是否通过）") && board.contains("推理判断") && board.contains("未核实"),
        "{board}"
    );
    assert!(board.contains("独立验证：V1 判定通过"), "{board}");

    // `task_detail`: each criterion with its method, and the verifier's own result apart.
    let (detail, _) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T1" })).await;
    assert!(
        detail.contains("✓ [测试]")
            && detail.contains("✓ [检查]")
            && detail.contains("✓ [推理判断]")
            && detail.contains("✓ [未核实]"),
        "{detail}"
    );
    assert!(detail.contains("独立验证 V1：已完成"), "{detail}");

    // The process goes away; the board is rebuilt from the event log.
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    app2.resume_run(&run).await.unwrap();
    let (detail, _) = tool(&app2, &run, "lead", "task_detail", json!({ "task": "T1" })).await;
    for method in ["[测试]", "[命令]", "[检查]", "[推理判断]", "[未核实]"] {
        assert!(detail.contains(method), "{method} survives a restart: {detail}");
    }
    assert!(detail.contains("证据：测试 1 条、命令 1 条、检查 1 条、推理判断 1 条、未核实 1 条"), "{detail}");

    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let d = events(&app2, &run).into_iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.unwrap();
    let t1 = d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == "T1").unwrap().clone();
    assert_eq!((t1["criteria"].as_u64(), t1["passed"].as_u64()), (Some(5), Some(5)), "{t1}");
    assert_eq!(t1["methods"].as_array().unwrap().len(), 5, "{t1}");
    assert_eq!(t1["checks"].as_array().unwrap().len(), 5, "every check is kept: {t1}");
    assert_eq!(t1["verifications"][0]["task"], "V1");
    assert_eq!(t1["verifications"][0]["passed"], true);
    // All three pieces of task-level evidence, not only the first.
    let texts: Vec<&str> = d["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["text"].as_str())
        .filter(|t| t.starts_with("T1："))
        .collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// An illegal evidence method is refused by name and never turned into an allowed one; the
/// fix round reports no method at all, which is 「未说明」 and does not fail the task.
#[tokio::test(flavor = "multi_thread")]
async fn an_illegal_evidence_method_fails_the_result_check() {
    let h = harness("badmethod", &[("lead", false), ("m1", true)], json!({ "T1": ["badmethod", "ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "verification failed", |e| task_event(e, "verification.failed", "T1")).await;
    let evs = events(&h.app, &run);
    let failed =
        evs.iter().find(|e| e.kind.as_str() == "verification.failed" && e.task.as_deref() == Some("T1")).unwrap();
    let failures = failed.data.as_ref().unwrap()["failures"].as_array().unwrap().clone();
    assert!(
        failures.iter().any(|f| f.as_str().is_some_and(|f| f.contains("证据方法「vibes」不是允许值"))),
        "{failures:?}"
    );
    // The recorded check keeps exactly the value it was given.
    let submitted =
        evs.iter().find(|e| e.kind.as_str() == "result.submitted" && e.task.as_deref() == Some("T1")).unwrap();
    assert_eq!(submitted.data.as_ref().unwrap()["checks"][0]["method"], "vibes");
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    let done = events(&h.app, &run)
        .into_iter()
        .rev()
        .find(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T1"))
        .unwrap();
    assert_eq!(
        done.data.as_ref().unwrap()["acceptance"]["unspecified"],
        1,
        "no method is 「未说明」, not a failure: {:?}",
        done.data
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A verification hears whether the files are still those the verified task handed its result in
/// on: the same files give its checks the same results, so it reruns only what its verdict needs.
/// Files changed in between are named, and the lead reads on the board which it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_verification_knows_whether_the_files_changed_since_the_result() {
    let h = harness(
        "evidence-files",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "T1": ["write:a.txt"], "T2": ["write:b.txt"], "V1": ["approve"], "V2": ["approve"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({})),
            task("V1", "m2", json!({ "verifies": "T1" })),
            task("T2", "m1", json!({ "deps": ["V1"] })),
            task("V2", "m3", json!({ "verifies": "T1", "deps": ["T2"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "both verified", |e| {
        task_event(e, "task.succeeded", "V1") && task_event(e, "task.succeeded", "V2")
    })
    .await;
    let evs = events(&h.app, &run);
    let prompt = |task: &str| {
        evs.iter()
            .find(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some(task))
            .and_then(|e| e.data.as_ref()?["prompt"].as_str().map(str::to_owned))
            .unwrap()
    };
    let submitted =
        evs.iter().find(|e| e.kind.as_str() == "result.submitted" && e.task.as_deref() == Some("T1")).unwrap();
    assert!(
        submitted.data.as_ref().unwrap()["files"]["files"]["a.txt"].is_string(),
        "the files T1 handed in are on record"
    );
    assert!(prompt("V1").contains("和 T1 提交结果时完全相同"), "{}", prompt("V1"));
    let v2 = prompt("V2");
    assert!(v2.contains("T1 提交结果之后又有变化") && v2.contains("b.txt"), "{v2}");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("核对时的文件和 T1 提交结果时相同"), "{board}");
    assert!(board.contains("核对时的文件和 T1 提交结果时不同（改了 b.txt）"), "{board}");
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}
