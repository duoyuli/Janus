//! The lead's goal session end to end: orchestrating through the board, resuming with it,
//! questions it leaves behind, board pushes into its turn, and a goal that does not settle or a
//! lead that falls silent.

use super::*;

/// Goal mode end to end: the lead (a fake Codex) gets the brief and `/goal`; it orchestrates
/// through the board tools, which validate before anything runs; collaborators report back
/// on the board, a question goes to the lead and its answer back into the session; the run
/// is delivered once the lead is done, with the lead's summary.
#[tokio::test(flavor = "multi_thread")]
async fn goal_mode_orchestrates_through_the_board() {
    let h = harness(
        "goal",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["write:notes.md"], "T2": ["ask", "ok"] }),
    );
    let run = h
        .app
        .create_run(NewRun { goal: "写一份笔记".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..100 {
        if std::fs::read_to_string(&calls).unwrap_or_default().contains("goal 写一份笔记") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let log = std::fs::read_to_string(&calls).unwrap_or_else(|_| {
        panic!(
            "the lead got nothing: {:#?}",
            events(&h.app, &run).iter().map(|e| format!("{} {}", e.kind, e.text)).collect::<Vec<_>>()
        )
    });
    assert!(log.contains("brief ok") && log.contains("goal 写一份笔记"), "brief, then /goal: {log}");
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| {
            e.kind.as_str() == "session.prompt"
                && e.to.as_deref() == Some("lead")
                && e.data.as_ref().is_some_and(|d| d["channel"] == "system" && d["via"] == "system")
        }),
        "the brief goes in the system layer"
    );
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "session.prompt"
            && e.to.as_deref() == Some("lead")
            && e.data.as_ref().is_some_and(|d| d["channel"] == "goal")
    }));

    // The board's MCP surface, and its token.
    let secret = h.app.run(&run).unwrap().st.lock().unwrap().mcp_token.clone();
    let token = session_token(&secret, "lead");
    let member_token = session_token(&secret, "T1");
    let tools = h
        .app
        .mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .unwrap()
        .unwrap();
    // The board's ten, and the team memory's two (`forget` is a consolidation's).
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 12);
    let member = h
        .app
        .mcp(&run, &member_token, "T1", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> =
        member["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["board_post", "claim", "peers", "request_split", "recall", "remember"]);
    // Claims are a collaborator's: the lead assigns files through `owns` instead.
    let (text, err) = tool(&h.app, &run, "lead", "claim", json!({ "paths": ["x"] })).await;
    assert!(err && text.contains("没有这个工具"), "{text}");
    assert!(
        h.app.mcp(&run, "wrong", "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );
    // A collaborator's URL renamed to the lead, or to another task, is refused.
    assert!(
        h.app
            .mcp(&run, &member_token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .await
            .is_err()
    );
    assert!(
        h.app
            .mcp(&run, &member_token, "T2", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .await
            .is_err()
    );
    assert!(
        h.app.mcp(&run, &secret, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );
    assert!(
        h.app
            .mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await
            .unwrap()
            .is_none()
    );

    // Validation first: every problem at once, and nothing starts.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        { "id": "T1", "title": "写笔记", "instructions": "写", "assignee": "nobody" },
        { "id": "T2", "title": "定名", "instructions": "定", "assignee": "m2", "deps": ["T9"] },
        { "id": "T3", "title": "查", "instructions": "查", "assignee": "m1" }] }),
    )
    .await;
    assert!(err && text.contains("nobody") && text.contains("T9") && text.contains("子任务 T3：缺少 accept"), "{text}");
    assert!(h.app.run(&run).unwrap().st.lock().unwrap().tasks.is_empty());

    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [
        { "id": "T1", "title": "写笔记", "instructions": "在 notes.md 写笔记", "assignee": "m1", "accept": ["notes.md 里有笔记"] },
        { "id": "T2", "title": "定名", "instructions": "给笔记定个名字", "assignee": "m2", "deps": ["T1"], "accept": ["给出一个名字"] }] })).await;
    assert!(!err, "{text}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "tool.started" && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.orchestrate")
    }));
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "tool.completed"
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.orchestrate" && d["status"] == "completed")
    }));
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [{ "id": "T1", "title": "x", "instructions": "x", "assignee": "m1" }] }),
    )
    .await;
    assert!(err && text.contains("重复"), "ids stay unique across orchestrations: {text}");

    // A collaborator writes back while it works.
    let (text, err) =
        tool(&h.app, &run, "T1", "board_post", json!({ "kind": "progress", "text": "笔记写了一半" })).await;
    assert!(!err, "{text}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "tool.started"
            && e.task.as_deref() == Some("T1")
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.board_post")
    }));
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "tool.completed"
            && e.task.as_deref() == Some("T1")
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.board_post" && d["status"] == "completed")
    }));
    let (_, err) = tool(&h.app, &run, "T1", "orchestrate", json!({ "tasks": [] })).await;
    assert!(err, "collaborators cannot orchestrate");

    // The lead reads the board until the question arrives, answers it, and reads on.
    let mut board = String::new();
    let mut answered = false;
    for _ in 0..60 {
        let (text, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 2 })).await;
        board.push_str(&text);
        board.push('\n');
        if !answered && board.contains("[提问] T2") {
            let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "alpha" })).await;
            assert!(!err && text.contains("已回答"), "{text}");
            answered = true;
        }
        if board.contains("[完成] T2") {
            break;
        }
    }
    assert!(board.contains("[进展] T1") && board.contains("[完成] T1"), "{board}");
    assert!(board.contains("按回答 alpha 完成"), "the answer went back into T2's session: {board}");
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| {
            e.kind.as_str() == "tool.started" && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.wait")
        })
    );
    assert!(evs.iter().any(|e| {
        e.kind.as_str() == "tool.completed"
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.wait" && d["status"] == "completed")
    }));
    assert!(!has(&evs, "question.escalated"), "questions go to the lead, not the person");
    assert!(has(&evs, "orchestration.rejected") && has(&evs, "orchestration.accepted") && has(&evs, "board.posted"));
    assert!(!has(&evs, "run.delivered"), "not before the lead is done");

    // The lead finishes; the run is delivered with its summary.
    std::fs::write(h.dir.join("fake/lead-go"), "目标已完成：笔记写好了").unwrap();
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let delivered = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap();
    assert!(delivered.data.as_ref().unwrap()["summary"].as_str().unwrap().contains("笔记写好了"));
    let cwd =
        PathBuf::from(serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["cwd"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(cwd.join("notes.md")).unwrap(), "T1\n");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead can end its goal while a collaborator still waits on a question; nobody reads
/// the board any more, so the question goes to the person and the run still finishes.
#[tokio::test(flavor = "multi_thread")]
async fn questions_left_by_the_lead_go_to_the_person() {
    let h = harness("goal-left", &[("lead", false), ("m1", true)], json!({ "T1": ["ask", "ok"] }));
    let run = h
        .app
        .create_run(NewRun { goal: "定个名字".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "定名", "instructions": "定个名字", "assignee": "m1", "accept": ["给出一个名字"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "question", |e| has(e, "question.asked")).await;
    assert!(!has(&events(&h.app, &run), "question.escalated"), "the lead answers while it is there");

    std::fs::write(h.dir.join("fake/lead-go"), "目标已完成").unwrap();
    wait_until(&h.app, &run, "escalation", |e| has(e, "question.escalated")).await;
    let evs = events(&h.app, &run);
    assert!(!has(&evs, "run.delivered"), "the run waits for the answer");
    let key = evs.iter().find(|e| e.kind.as_str() == "question.asked").unwrap().data.as_ref().unwrap()["key"].clone();
    act(&h, &run, json!({ "type": "answer", "key": key, "task": "T1", "answer": "beta" }));
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    assert_eq!(evs.iter().filter(|e| e.kind.as_str() == "question.escalated").count(), 1);
    assert!(
        evs.iter().any(|e| e.kind.as_str() == "result.submitted" && e.text.contains("beta")),
        "the answer reached T1"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Goal mode survives a restart: the board's tasks come back as they were, the lead starts
/// again knowing them, and can orchestrate on top of them.
#[tokio::test(flavor = "multi_thread")]
async fn goal_mode_resumes_with_the_board() {
    let h = harness("goal-resume", &[("lead", false), ("m1", true)], json!({}));
    // Left over from earlier work: not this run's change.
    std::fs::write(h.repo.join("leftover.txt"), "old\n").unwrap();
    let run = h
        .app
        .create_run(NewRun { goal: "两步完成".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    let lead_started = |n: usize| {
        let calls = calls.clone();
        async move {
            for _ in 0..100 {
                if std::fs::read_to_string(&calls).unwrap_or_default().matches("goal 两步完成").count() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("lead did not start");
        }
    };
    lead_started(1).await;
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "第一步", "instructions": "做第一步", "assignee": "m1", "accept": ["第一步完成"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T1"))
    })
    .await;
    assert!(
        !events(&h.app, &run).iter().any(|e| e.kind.as_str() == "artifact.created" && e.text.contains("T1-a1.diff")),
        "T1 changed nothing"
    );

    // The process goes away while the lead is still working.
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    app2.resume_run(&run).await.unwrap();
    let evs = events(&app2, &run);
    let recovered = evs.iter().rev().find(|e| e.kind.as_str() == "run.recovered").unwrap();
    assert_eq!(recovered.data.as_ref().unwrap()["tasks"][0]["status"], "succeeded");
    lead_started(2).await;
    assert!(std::fs::read_to_string(&calls).unwrap().contains("brief ok resumed"), "the lead is told it continues");

    // It builds on the board it found.
    let (text, err) = tool(&app2, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T2", "title": "第二步", "instructions": "做第二步", "assignee": "m1", "deps": ["T1"], "accept": ["第二步完成"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&app2, &run, "T2", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T2"))
    })
    .await;
    let (board, _) = tool(&app2, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[完成] T2") && !board.contains("[完成] T1"), "only what is new: {board}");
    std::fs::write(h.dir.join("fake/lead-go"), "两步都完成了").unwrap();
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A lead whose agent takes steering hears about a collaborator's question while it works,
/// without calling `wait`: one board push steered into its turn, recorded in its session and
/// counted as seen. A message the person steers in is marked delivered once the agent says it
/// applied it.
#[tokio::test(flavor = "multi_thread")]
async fn board_events_are_pushed_into_a_steerable_lead() {
    let h = harness("push", &[("lead", false), ("m1", true)], json!({ "T1": ["ask", "ok"] }));
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..100 {
        let log = std::fs::read_to_string(&calls).unwrap_or_default();
        if log.contains("steer [Janus 看板推送]") && log.contains("[提问] T1") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let log = std::fs::read_to_string(&calls).unwrap_or_default();
    assert!(log.contains("steer [Janus 看板推送]") && log.contains("[提问] T1"), "{log}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind.as_str() == "session.prompt"
        && e.to.as_deref() == Some("lead")
        && e.data.as_ref().is_some_and(|d| d["channel"] == "board")));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(!board.contains("[提问]"), "a pushed entry does not come again: {board}");

    // The person steers into the lead's turn: routed at once, delivered when applied.
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "先处理 T1 的提问" }));
    let id = events(&h.app, &run).into_iter().rev().find(|e| e.kind.as_str() == "user.message").unwrap().data.unwrap()
        ["id"]
        .as_str()
        .unwrap()
        .to_string();
    let with = |e: &Event, kind: &str| {
        e.kind == kind
            && e.data.as_ref().is_some_and(|d| {
                d["id"] == id.as_str() || d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id.as_str()))
            })
    };
    wait_until(&h.app, &run, "steered and applied", |e| {
        e.iter().any(|e| with(e, "message.routed") && e.data.as_ref().unwrap()["delivery"] == "steer")
            && e.iter().any(|e| with(e, "message.delivered") && e.data.as_ref().unwrap()["applied"] == true)
    })
    .await;

    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T1", "text": "alpha" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A lead whose goal never settles by itself (Claude Code whose own check does not run) is
/// asked once its session goes quiet; it confirms, Janus clears the goal, and the run delivers
/// with the lead's summary.
#[tokio::test(flavor = "multi_thread")]
async fn a_goal_that_never_settles_is_confirmed_with_the_lead() {
    let h = harness_with("goalsettle", &[("lead", false), ("m1", true)], json!({}), |c| {
        c.goal_settle = Duration::from_secs(1)
    });
    std::fs::write(h.dir.join("fake/goal-sticky"), "").unwrap();
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    wait_until(&h.app, &run, "goal active", |e| {
        e.iter().any(|e| e.kind.as_str() == "goal.status" && e.data.as_ref().unwrap()["status"] == "active")
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(calls.contains("goal check") && calls.contains("goal clear"), "{calls}");
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().unwrap()["channel"] == "goal_check")
    );
    let delivered = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap();
    assert!(
        !delivered.data.as_ref().unwrap().to_string().contains("GOAL_DONE"),
        "the marker is not part of the summary: {}",
        delivered.data.as_ref().unwrap()
    );
    assert!(delivered.data.as_ref().unwrap()["summary"].as_str().unwrap().starts_with("目标已完成"));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A lead that shows no sign of work is reported to the person, once, and the report is
/// withdrawn when it moves again.
#[tokio::test(flavor = "multi_thread")]
async fn a_silent_lead_is_reported_to_the_person() {
    let h = harness_with("leadstall", &[("lead", false), ("m1", true)], json!({}), |c| {
        c.stall_after = Duration::from_secs(1)
    });
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    let stalls = |e: &[Event], resolved: bool| {
        e.iter()
            .filter(|e| {
                e.kind.as_str() == "lead.stalled"
                    && e.to.as_deref() == Some("user")
                    && (e.data.as_ref().unwrap()["resolved"] == true) == resolved
            })
            .count()
    };
    wait_until(&h.app, &run, "lead stall", |e| stalls(e, false) > 0).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(stalls(&events(&h.app, &run), false), 1, "one report per quiet spell");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    assert_eq!(stalls(&events(&h.app, &run), true), 1, "withdrawn once the lead moves");
    let _ = std::fs::remove_dir_all(&h.dir);
}
