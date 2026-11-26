//! How collaborators work together on the board: shared material, amended conclusions,
//! independent verification, edits that conflict and traces checked against results.

use super::*;

/// The workspace's shared material reaches the lead (tasks: `task_prompts_carry_the_shared_material`), and the lead's suggested
/// additions come back with the delivery, apart from its summary.
#[tokio::test(flavor = "multi_thread")]
async fn shared_material_reaches_everyone_and_suggestions_come_back() {
    let h = harness("brief", &[("lead", false), ("m1", false)], json!({}));
    let mut ws = h.app.db.get_config(ConfigKind::Workspace, "ws").unwrap().unwrap();
    ws["brief"] = json!("目标用户是独立开发者");
    h.app.db.put_config(ConfigKind::Workspace, "ws", &ws, now_ms()).unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    std::fs::write(
        h.dir.join("fake/lead-go"),
        "做完了。\n\n## Suggested Updates to Shared Context\n- 定价页在 docs/pricing.md\n- 免费版限 3 个项目",
    )
    .unwrap();
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let prompts: Vec<String> = evs
        .iter()
        .filter(|e| e.kind.as_str() == "session.prompt")
        .map(|e| e.data.as_ref().unwrap()["prompt"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        prompts.iter().any(|p| p.contains("工作区共享资料") && p.contains("目标用户是独立开发者")),
        "the lead sees it: {prompts:?}"
    );
    let d = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.clone().unwrap();
    assert_eq!(d["briefSuggestions"], json!(["定价页在 docs/pricing.md", "免费版限 3 个项目"]));
    assert!(
        d["summary"].as_str().unwrap().starts_with("做完了。")
            && !d["summary"].as_str().unwrap().contains(BRIEF_HEADING.zh),
        "{d}"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// When the lead overrules a finished task's conclusion, tasks that build on it use the new one:
/// a running one hears it at once, one that starts later gets it with the handoff.
#[tokio::test(flavor = "multi_thread")]
async fn amended_conclusions_reach_downstream_tasks() {
    let h = harness(
        "amend",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "A": ["ok"], "B": ["slow", "ok"], "C": ["ok"] }),
    );
    let run = start(&h, json!([task("A", "m1", json!({})), task("B", "m2", json!({ "deps": ["A"] }))])).await;
    wait_until(&h.app, &run, "B running", |e| task_event(e, "attempt.started", "B")).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (text, err) = tool(&h.app, &run, "lead", "amend", json!({ "task": "B", "text": "x" })).await;
    assert!(err && text.contains("只能修订已完成子任务"), "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "amend", json!({ "task": "A", "text": "改用经典 8 邻域规则" })).await;
    assert!(!err && text.contains("B"), "{text}");
    wait_until(&h.app, &run, "B after the amendment", |e| task_event(e, "task.succeeded", "B")).await;
    let evs = events(&h.app, &run);
    assert!(task_event(&evs, "result.amended", "A"));
    assert!(evs.iter().any(|e| e.kind.as_str() == "user.message"
        && e.task.as_deref() == Some("B")
        && e.data.as_ref().is_some_and(|d| d["text"].as_str().unwrap_or("").contains("改用经典 8 邻域规则"))));

    let (_, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("C", "m3", json!({ "deps": ["A"] }))] }))
            .await;
    assert!(!err);
    wait_until(&h.app, &run, "C started", |e| task_event(e, "attempt.started", "C")).await;
    let started = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("C"))
        .unwrap();
    assert!(
        started.data.as_ref().unwrap()["prompt"]
            .as_str()
            .unwrap()
            .contains("负责人修订了这个结论，以修订为准：改用经典 8 邻域规则")
    );
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Team structure in an orchestration: tasks that may run together cannot own the same files;
/// a verification task goes to someone else, waits for the work, is told what to check, and
/// its failure cannot be accepted away.
#[tokio::test(flavor = "multi_thread")]
async fn verifiers_check_others_work_and_cannot_be_waved_through() {
    let h = harness("verifier", &[("lead", false), ("m1", true), ("m2", true)], json!({ "V1": ["unmet"] }));
    let run = start(&h, json!([task("T0", "m1", json!({}))])).await;

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        task("T1", "m1", json!({ "owns": ["src"] })), task("T2", "m2", json!({ "owns": ["src/main.rs"] })),
        task("V0", "m1", json!({ "verifies": "T1" }))] }),
    )
    .await;
    assert!(err && text.contains("T1 和 T2") && text.contains("V0") && text.contains("另一个协作者"), "{text}");

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        task("T1", "m1", json!({ "owns": ["src"], "accept": ["src 里有新函数"] })),
        task("V1", "m2", json!({ "verifies": "T1", "accept": ["测试通过"] })),
        task("D1", "m1", json!({ "deps": ["V1"] }))] }),
    )
    .await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "V1 reviewed", |e| task_event(e, "task.succeeded", "V1")).await;
    // What depends on a verification that rejected the work does not start on it.
    wait_until(&h.app, &run, "D1 blocked", |e| task_event(e, "task.blocked", "D1")).await;
    let evs = events(&h.app, &run);
    let at =
        |kind: &str, task: &str| evs.iter().position(|e| e.kind == kind && e.task.as_deref() == Some(task)).unwrap();
    assert!(at("task.succeeded", "T1") < at("attempt.started", "V1"), "the verifier waits for the work");
    assert!(
        !evs.iter().any(|e| e.kind.as_str() == "session.prompt"
            && e.task.as_deref() == Some("V1")
            && e.data.as_ref().is_some_and(|d| d["channel"] == "fix")),
        "a verifier reporting a defect with evidence never gets a pointless fix round"
    );
    let prompt = |task: &str| -> String {
        evs.iter()
            .find(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some(task))
            .and_then(|e| e.data.as_ref()?["prompt"].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    assert!(prompt("T1").contains("你负责的文件：src"), "{}", prompt("T1"));
    assert!(
        prompt("V1").contains("独立验证：你验证子任务 T1") && prompt("V1").contains("- src 里有新函数"),
        "{}",
        prompt("V1")
    );

    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[验证未通过] V1") && board.contains("验证 T1，判定「需要修改」"), "{board}");
    assert!(board.contains("[无法开始] D1") && board.contains("验证 V1 判定「需要修改」"), "{board}");
    let blocked = evs.iter().find(|e| e.kind.as_str() == "task.blocked" && e.task.as_deref() == Some("D1")).unwrap();
    assert_eq!(blocked.data.as_ref().unwrap()["verdict"], "rejected", "{blocked:?}");
    let (text, err) = tool(&h.app, &run, "lead", "amend", json!({ "task": "V1", "text": "我看过了" })).await;
    assert!(err && text.contains("janus.amend 不能改"), "{text}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    // The delivery says a verifier whose verdict rejected the work did not confirm the task.
    let d = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.unwrap();
    let t1 = d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == "T1").unwrap().clone();
    assert_eq!(t1["verifications"][0]["task"], "V1", "{t1}");
    assert_eq!(t1["verifications"][0]["verdict"], "rejected", "{t1}");
    assert_eq!(t1["verifications"][0]["passed"], false, "a rejected verdict is not a pass: {t1}");
    assert!(
        d["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["task"] == "T1" && r["text"].as_str().is_some_and(|r| r.contains("独立验证 V1"))),
        "{d}"
    );
    assert!(
        !events(&h.app, &run).iter().any(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("D1")),
        "D1 never ran on rejected work"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Two tasks running at once edit the same file: the lead reads a conflict on the board, both
/// collaborators that take steering get a notice, and the files stay recallable per task.
#[tokio::test(flavor = "multi_thread")]
async fn conflicting_edits_reach_the_lead_and_both_sides() {
    let h = harness(
        "conflict",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["edit:shared.txt"], "T2": ["edit:shared.txt"] }),
    );
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m2", json!({}))])).await;
    wait_until(&h.app, &run, "conflict", |e| has(e, "task.conflict")).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[冲突]") && board.contains("shared.txt"), "{board}");
    wait_until(&h.app, &run, "both done", |e| {
        task_event(e, "task.succeeded", "T1") && task_event(e, "task.succeeded", "T2")
    })
    .await;
    let notices =
        std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap().matches("steer [Janus] 子任务").count();
    assert_eq!(notices, 2, "each side hears about the other");
    assert_eq!(
        events(&h.app, &run).iter().filter(|e| e.kind.as_str() == "task.conflict").count(),
        1,
        "one report per file and pair"
    );

    let (detail, err) = tool(&h.app, &run, "lead", "board", json!({ "task": "T1" })).await;
    assert!(!err && detail.contains("改过的文件：shared.txt") && detail.contains("[冲突] 和 T2"), "{detail}");
    let (peers, err) = tool(&h.app, &run, "T2", "board", json!({})).await;
    assert!(!err && peers.contains("T1") && peers.contains("和你都改了 shared.txt"), "{peers}");
    let (full, _) = tool(&h.app, &run, "T2", "board", json!({ "task": "T1" })).await;
    assert!(full.contains("验收标准"), "{full}");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// What tasks leave in the workspace is read back: a task editing files another task owns
/// is reported at once, a result that disagrees with its traces is flagged for the lead,
/// diffs cover each task's own files, a conclusion whose files changed afterwards is marked
/// out of date for downstream tasks, and all of it survives a restart.
#[tokio::test(flavor = "multi_thread")]
async fn traces_check_results_and_mark_stale_conclusions() {
    let h = harness(
        "traces",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "T1": ["edit:a.txt"], "T2": ["edit:a.txt"], "T3": ["edit:a.txt"], "T4": ["ok"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["a.txt"] })),
            task("T2", "m2", json!({ "owns": ["c.txt"] })),
            task("T3", "m3", json!({ "deps": ["T1", "T2"] })),
            task("T4", "m1", json!({ "deps": ["T1", "T3"] })),
        ]),
    )
    .await;
    wait_until(&h.app, &run, "T4", |e| task_event(e, "task.succeeded", "T4")).await;
    let evs = events(&h.app, &run);
    let trespass = evs
        .iter()
        .find(|e| e.kind.as_str() == "task.conflict" && e.data.as_ref().unwrap()["owned"] == true)
        .expect("T2 stepped on T1's file");
    assert_eq!(
        (
            trespass.task.as_deref(),
            &trespass.data.as_ref().unwrap()["other"],
            &trespass.data.as_ref().unwrap()["paths"]
        ),
        (Some("T2"), &json!("T1"), &json!(["a.txt"]))
    );
    let gaps = |t: &str| {
        evs.iter()
            .find(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some(t))
            .unwrap()
            .data
            .as_ref()
            .unwrap()["traceGaps"]
            .clone()
    };
    assert_eq!(gaps("T1"), json!([]), "did what it owns");
    let g = gaps("T2").to_string();
    assert!(g.contains("以外的文件：a.txt") && g.contains("负责 c.txt"), "{g}");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[越界] T2") && board.contains("痕迹核对"), "{board}");
    // Each diff covers the task's own files; T4 changed nothing, so it has none.
    let diffs: Vec<&Event> =
        evs.iter().filter(|e| e.kind.as_str() == "artifact.created" && e.text.contains(".diff")).collect();
    assert!(
        diffs.iter().any(|e| e.task.as_deref() == Some("T1")) && !diffs.iter().any(|e| e.task.as_deref() == Some("T4")),
        "{diffs:?}"
    );
    // T3 changed a.txt after T1 finished: T4 is told T1's conclusion may be out of date.
    let handoff = evs
        .iter()
        .find(|e| {
            e.kind.as_str() == "context.handoff"
                && e.task.as_deref() == Some("T4")
                && e.data.as_ref().unwrap()["from"] == "T1"
        })
        .unwrap();
    assert_eq!(handoff.data.as_ref().unwrap()["stale"], json!([{ "path": "a.txt", "by": "T3" }]));
    assert_eq!(handoff.data.as_ref().unwrap()["files"], json!(["a.txt"]));

    // The traces outlive the process.
    stop(&h.app).await;
    let app2 = served_app(&h.dir, |_| {});
    app2.recover().unwrap();
    let r = app2.resume_run(&run).await.unwrap();
    while r.st.lock().unwrap().mcp_token.is_empty() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (detail, _) = tool(&app2, &run, "lead", "board", json!({ "task": "T1" })).await;
    assert!(
        detail.contains("负责的文件：a.txt")
            && detail.contains("改过的文件：a.txt")
            && detail.contains("a.txt（T3 改）")
            && detail.contains("[越界] T2"),
        "{detail}"
    );
    let (detail, _) = tool(&app2, &run, "lead", "board", json!({ "task": "T2" })).await;
    assert!(detail.contains("痕迹核对"), "{detail}");
    let (peers, _) = tool(&app2, &run, "T4", "board", json!({})).await;
    assert!(peers.contains("完成后 a.txt 又被改过"), "{peers}");
    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
