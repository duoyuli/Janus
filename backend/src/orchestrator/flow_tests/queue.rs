//! The dispatch queue: goal runs wait for a free slot, in the order they were made; a queued run
//! only starts or is cancelled; a paused run gives its slot away; the queue survives a restart.

use super::*;

async fn new_run(app: &Arc<App>, goal: &str) -> String {
    app.create_run(NewRun { goal: goal.into(), workspace: "ws".into(), ..Default::default() }).await.unwrap()
}

async fn wait_status(app: &App, run: &str, want: &str) {
    for _ in 0..300 {
        if status(app, run) == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{run} is {} and not {want}", status(app, run));
}

async fn wait_live(app: &App, run: &str) {
    for _ in 0..300 {
        if app.run(run).is_some_and(|r| r.st.lock().unwrap().status == RunStatus::Running) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{run} did not start: {}", status(app, run));
}

#[tokio::test(flavor = "multi_thread")]
async fn goal_runs_wait_for_a_free_slot_in_order() {
    let h = harness("queue", &[("lead", false), ("m1", false)], json!({}));
    let app = test_app_with(&h.dir, |c| c.max_running_runs = 1);

    // The first run takes the one slot and is set up before create_run returns.
    let first = new_run(&app, "第一个").await;
    assert!(app.run(&first).is_some(), "a run with a free slot starts at once");
    wait_live(&app, &first).await;

    // The next ones wait, with nothing of them started: no worktree, no team, no session.
    let second = new_run(&app, "第二个").await;
    let third = new_run(&app, "第三个").await;
    assert_eq!(status(&app, &second), "queued");
    assert!(app.run(&second).is_none());
    assert!(!app.worktree_dir(&second).exists(), "a queued run has no worktree yet");
    let queued = events(&app, &second);
    assert_eq!(queued.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["run.queued"]);
    assert_eq!(queued[0].data.as_ref().unwrap()["goal"], "第二个");
    assert!(
        serde_json::to_value(app.db.run(&second).unwrap().unwrap()).unwrap()["config"]["team"].is_null(),
        "no team snapshot while queued"
    );
    assert_eq!(app.db.queued_runs().unwrap(), [second.clone(), third.clone()], "oldest first");

    // A queued run can only start now or be cancelled.
    let talk = app.request_action(&second, &req(json!({ "type": "message", "text": "你好" }))).await;
    assert!(matches!(talk, Err(Refused::Conflict(_))), "a queued run has no one to talk to");
    app.request_action(&third, &req(json!({ "type": "cancel" }))).await.map_err(|e| format!("{e:?}")).unwrap();
    assert_eq!(status(&app, &third), "cancelled");
    assert!(has(&events(&app, &third), "run.cancelled"));
    let again = app.request_action(&third, &req(json!({ "type": "start" }))).await;
    assert!(matches!(again, Err(Refused::Conflict(_))), "a cancelled run does not start: the state moved on");

    // Pausing the working run frees its slot: the second one starts, with its team taken now.
    act_on(&app, &first, json!({ "type": "pause" }));
    wait_live(&app, &second).await;
    let started = serde_json::to_value(app.db.run(&second).unwrap().unwrap()).unwrap();
    assert!(started["config"]["team"].is_array(), "the team is taken when the run starts");
    let log: Vec<String> = events(&app, &second).into_iter().map(|e| e.kind).collect();
    assert_eq!(log[..2], ["run.queued", "run.starting"], "leaving the queue is on record before the setup");
    assert!(log.contains(&"run.created".to_string()));

    // Resuming does not stop anyone: both work, and new runs wait until a slot is free again.
    act_on(&app, &first, json!({ "type": "resume" }));
    let fourth = new_run(&app, "第四个").await;
    assert_eq!(status(&app, &fourth), "queued");
    // The person may start one past the slots.
    app.request_action(&fourth, &req(json!({ "type": "start" }))).await.map_err(|e| format!("{e:?}")).unwrap();
    wait_live(&app, &fourth).await;

    // A queued run can be deleted; it leaves nothing behind.
    let fifth = new_run(&app, "第五个").await;
    assert_eq!(status(&app, &fifth), "queued");
    app.delete_run(&fifth).await.map_err(|e| format!("{e:?}")).unwrap();
    assert!(app.db.run(&fifth).unwrap().is_none());

    // The queue survives a restart: queued runs stay queued, working ones are interrupted.
    let sixth = new_run(&app, "第六个").await;
    for run in [&first, &second, &fourth] {
        act_on(&app, run, json!({ "type": "cancel" }));
    }
    // Cancelling frees the slots, so the sixth starts on its own; the seventh waits behind it.
    wait_live(&app, &sixth).await;
    let seventh = new_run(&app, "第七个").await;
    assert_eq!(status(&app, &seventh), "queued");
    // The process goes away: the sixth's setup stops writing before the restarted service writes
    // to the same log (two writers would take the same seq).
    app.run(&sixth).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let restarted = test_app_with(&h.dir, |c| c.max_running_runs = 1);
    restarted.recover().unwrap();
    assert_eq!(status(&restarted, &sixth), "interrupted");
    assert_eq!(status(&restarted, &seventh), "queued");
    // With the interrupted run holding no slot, the restarted service starts it.
    restarted.pump();
    wait_status(&restarted, &seventh, "running").await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A person's action on a live run of `app`.
fn act_on(app: &Arc<App>, run: &str, a: Value) {
    app.action(&app.run(run).unwrap(), &action_of(a)).unwrap();
}

/// A run taken off the queue whose setup never finished (the service stopped, its agent could not
/// run) has no snapshot to continue from: after a restart it is back in its place, and once it
/// failed to start, continuing it queues it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_never_got_set_up_queues_again() {
    let h = harness("queue-requeue", &[("lead", false), ("m1", false)], json!({}));
    let app = test_app_with(&h.dir, |c| c.max_running_runs = 1);
    let first = new_run(&app, "占着位置").await;
    let second = new_run(&app, "被领走但没准备好").await;
    // As if a pump took it off the queue and the service stopped before its setup.
    app.db.set_run_status(&second, RunStatus::Planning).unwrap();
    let restarted = test_app_with(&h.dir, |c| c.max_running_runs = 1);
    restarted.recover().unwrap();
    assert_eq!(status(&restarted, &first), "interrupted");
    assert_eq!(status(&restarted, &second), "queued", "back on the queue, not interrupted");
    assert!(has(&events(&restarted, &second), "run.requeued"));

    // Its workspace's agent cannot run now: it fails to start, and stays failed.
    let mut conn = restarted.db.get_config(ConfigKind::Connection, "fake").unwrap().unwrap();
    conn["bin"] = json!("/nonexistent/claude-acp");
    restarted.db.put_config(ConfigKind::Connection, "fake", &conn, now_ms()).unwrap();
    let mut ws = restarted.db.get_config(ConfigKind::Workspace, "ws").unwrap().unwrap();
    ws["path"] = json!(h.dir.join("gone").to_string_lossy());
    restarted.db.put_config(ConfigKind::Workspace, "ws", &ws, now_ms()).unwrap();
    restarted.pump();
    wait_status(&restarted, &second, "failed").await;
    let talk =
        restarted.request_action(&second, &req(json!({ "type": "message", "agent": "lead", "text": "继续" }))).await;
    assert!(matches!(talk, Err(Refused::Conflict(_))), "nothing to talk to");
    // Continuing it puts it back on the queue.
    let mut ws = restarted.db.get_config(ConfigKind::Workspace, "ws").unwrap().unwrap();
    ws["path"] = json!(h.repo.canonicalize().unwrap().to_string_lossy());
    restarted.db.put_config(ConfigKind::Workspace, "ws", &ws, now_ms()).unwrap();
    restarted
        .request_action(&second, &req(json!({ "type": "resumeRun" })))
        .await
        .map_err(|e| format!("{e:?}"))
        .unwrap();
    wait_live(&restarted, &second).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
