//! What the board accepts and how it keeps order: orchestrations checked one at a time, a
//! dependency named twice, collaborator input sizes, a lead's reply kept for its own task, and the
//! pause before a failed attempt is tried again (`docs/review/20261120-agent_team_collaboration.md`).

use super::*;

/// Two orchestrations submitted at once with the same task id: one goes on the board, the other is
/// refused, and the board never holds the id twice (AT-05). A dependency written twice is one
/// dependency, not a cycle (AT-06).
#[tokio::test(flavor = "multi_thread")]
async fn orchestrations_are_checked_one_at_a_time() {
    let h = harness("orch-race", &[("lead", false), ("m1", true), ("m2", true)], json!({}));
    let run = start(&h, json!([task("T0", "m1", json!({}))])).await;
    let one = json!({ "tasks": [task("T1", "m1", json!({}))] });
    let other = json!({ "tasks": [task("T1", "m2", json!({}))] });
    let (a, b) =
        tokio::join!(tool(&h.app, &run, "lead", "orchestrate", one), tool(&h.app, &run, "lead", "orchestrate", other));
    assert!(a.1 != b.1, "exactly one is refused: {a:?} {b:?}");
    let ids: Vec<String> = h.app.run(&run).unwrap().st.lock().unwrap().tasks.iter().map(|t| t.def.id.clone()).collect();
    assert_eq!(ids.iter().filter(|id| *id == "T1").count(), 1, "{ids:?}");

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("T2", "m1", json!({ "deps": ["T0", "T0", "T1"] }))] }),
    )
    .await;
    assert!(!err, "{text}");
    let deps =
        h.app.run(&run).unwrap().st.lock().unwrap().tasks.iter().find(|t| t.def.id == "T2").unwrap().def.deps.clone();
    assert_eq!(deps, vec!["T0", "T1"]);
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A collaborator cannot put more on the board in one call than a report needs (AT-17).
#[tokio::test(flavor = "multi_thread")]
async fn collaborator_input_has_limits() {
    let h = harness("board-limits", &[("lead", false), ("m1", true)], json!({ "T1": ["hold:go"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let (text, err) = tool(&h.app, &run, "T1", "board_post", json!({ "text": "长".repeat(8001) })).await;
    assert!(err && text.contains("8000"), "{text}");
    let (text, err) = tool(&h.app, &run, "T1", "board_post", json!({ "text": "长".repeat(8000) })).await;
    assert!(!err, "{text}");
    let many: Vec<String> = (0..101).map(|i| format!("src/f{i}.rs")).collect();
    let (text, err) = tool(&h.app, &run, "T1", "claim", json!({ "paths": many })).await;
    assert!(err && text.contains("100"), "{text}");
    let subtasks: Vec<Value> = (0..9).map(|i| json!({ "title": format!("部分 {i}") })).collect();
    let (text, err) =
        tool(&h.app, &run, "T1", "request_split", json!({ "reason": "太大", "subtasks": subtasks })).await;
    assert!(err && text.contains('8'), "{text}");
    std::fs::write(h.dir.join("fake/go"), "").unwrap();
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead's reply to a task that has not started waits for that task: the collaborator's other
/// task, started first, does not get it (AT-12).
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_for_a_task_waits_for_that_task() {
    let h = harness("note-task", &[("lead", false), ("m1", true), ("m2", true)], json!({ "T1": ["hold:go"] }));
    let run = start(&h, json!([task("T1", "m2", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "用 alpha 命名" })).await;
    assert!(!err && text.contains("下一次开始工作"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T3", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T3 done", |e| task_event(e, "task.succeeded", "T3")).await;
    std::fs::write(h.dir.join("fake/go"), "").unwrap();
    wait_until(&h.app, &run, "T2 done", |e| task_event(e, "task.succeeded", "T2")).await;
    // What went into each task's sessions: the attempt's opening prompt and later prompts.
    let prompt_of = |task: &str| -> String {
        events(&h.app, &run)
            .iter()
            .filter(|e| {
                matches!(e.kind.as_str(), "attempt.started" | "session.prompt") && e.task.as_deref() == Some(task)
            })
            .map(|e| e.data.as_ref().map(|d| d.to_string()).unwrap_or_default())
            .collect()
    };
    assert!(!prompt_of("T3").contains("用 alpha 命名"));
    assert!(prompt_of("T2").contains("用 alpha 命名"));
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A failed attempt is tried again after a pause, not the moment it fails (AT-10).
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_attempt_is_retried_after_a_pause() {
    let h = harness_with("retry-pause", &[("lead", false), ("m1", true)], json!({ "T1": ["fail", "ok"] }), |c| {
        c.retry_after = Duration::from_millis(1500)
    });
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    let evs = events(&h.app, &run);
    let started: Vec<i64> =
        evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).map(|e| e.ts).collect();
    assert!(started.len() == 2 && started[1] - started[0] >= 1400, "attempts started at {started:?}");
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A push that cannot reach the lead's session (it ended in between) does not count as read:
/// the entries come with the next `wait` (AT-03).
#[tokio::test(flavor = "multi_thread")]
async fn a_push_that_does_not_arrive_stays_unread() {
    let h = harness("push-lost", &[("lead", false), ("m1", true)], json!({ "T1": ["hold:go"] }));
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 started", |e| task_event(e, "attempt.started", "T1")).await;
    let r = h.app.run(&run).unwrap();
    let ready = || {
        let st = r.st.lock().unwrap();
        st.steerable.contains(&SessionKey::Lead) && st.lead.session.is_some()
    };
    for _ in 0..100 {
        if ready() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready(), "the lead's session is open and takes steering");
    // The session's own sender is kept, so only this push finds the channel closed.
    let (gone, _) = tokio::sync::mpsc::unbounded_channel();
    let _open = r.st.lock().unwrap().live.insert(SessionKey::Lead, gone);
    let (text, err) =
        tool(&h.app, &run, "T1", "board_post", json!({ "kind": "blocker", "text": "缺少测试数据" })).await;
    assert!(!err, "{text}");
    let before = r.st.lock().unwrap().lead.cursor;
    h.app.push_board(&r);
    assert_eq!(r.st.lock().unwrap().lead.cursor, before, "nothing counts as read");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("缺少测试数据"), "{board}");
    std::fs::write(h.dir.join("fake/go"), "").unwrap();
    let _ = std::fs::remove_dir_all(&h.dir);
}
