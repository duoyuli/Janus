//! Flow tests of the person's actions as the HTTP API receives them (`App::request_action`,
//! docs/interfaces/20261031-http_api.md#run_actions).

use super::*;

/// One `commandId` acts once even when two requests carry it at the same time, a repeat after it
/// took effect is a duplicate, one whose first request a restart cut off is refused instead of
/// reported as done, an action that fails gives its id back for a retry, and an unknown run is
/// refused before anything is claimed.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_id_acts_once_and_a_failed_action_can_be_retried() {
    let h = harness("actions-once", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;

    let body = json!({ "type": "message", "agent": "lead", "text": "只说一次", "commandId": "c-1" });
    let spawn = |body: Value| {
        let (app, run) = (h.app.clone(), run.clone());
        tokio::spawn(async move { app.request_action(&run, &body).await })
    };
    let (a, b) = (spawn(body.clone()), spawn(body.clone()));
    let replies = [a.await.unwrap(), b.await.unwrap()];
    let done = replies.iter().filter(|r| matches!(r, Ok(ActionReply::Done))).count();
    // The other one finds the action running (refused) or finished (a duplicate), and does nothing.
    let other =
        replies.iter().filter(|r| matches!(r, Ok(ActionReply::Duplicate) | Err(ActionRefused::Conflict(_)))).count();
    assert_eq!((done, other), (1, 1), "the second request with the id does nothing");
    assert!(matches!(h.app.request_action(&run, &body).await, Ok(ActionReply::Duplicate)));
    let said = |text: &str| {
        events(&h.app, &run)
            .iter()
            .filter(|e| e.kind == "user.message" && e.data.as_ref().is_some_and(|d| d["text"] == text))
            .count()
    };
    assert_eq!(said("只说一次"), 1);

    // An action that fails releases its id: the same id then acts.
    let failing = json!({ "type": "nonsense", "commandId": "c-2" });
    assert!(matches!(h.app.request_action(&run, &failing).await, Err(ActionRefused::Invalid(_))));
    let retry = json!({ "type": "message", "agent": "lead", "text": "第二次", "commandId": "c-2" });
    assert!(matches!(h.app.request_action(&run, &retry).await, Ok(ActionReply::Done)));
    assert_eq!(said("第二次"), 1);

    // A restart between claiming an id and the action taking effect: the outcome is unknown, so
    // the same id is refused rather than answered as done, and nothing is said.
    h.app.db.claim_command(&run, "c-3", &json!({ "type": "message" }), now_ms()).unwrap();
    h.app.db.interrupt_commands().unwrap();
    let cut_off = json!({ "type": "message", "agent": "lead", "text": "第三次", "commandId": "c-3" });
    assert!(matches!(h.app.request_action(&run, &cut_off).await, Err(ActionRefused::Conflict(_))));
    assert_eq!(said("第三次"), 0);

    assert!(matches!(h.app.request_action("no-such-run", &body).await, Err(ActionRefused::NotFound)));
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Runs created at the same moment get distinct ids: the second used to fail on the runs table's
/// unique id when both landed in the same millisecond.
#[tokio::test(flavor = "multi_thread")]
async fn runs_created_together_get_distinct_ids() {
    let h = harness("actions-ids", &[("lead", false), ("m1", false)], json!({}));
    let made: Vec<_> = (0..8)
        .map(|i| {
            let app = h.app.clone();
            tokio::spawn(async move {
                app.create_run(NewRun { goal: format!("目标 {i}"), workspace: "ws".into(), ..Default::default() }).await
            })
        })
        .collect();
    let mut ids = std::collections::HashSet::new();
    for m in made {
        ids.insert(m.await.unwrap().unwrap());
    }
    assert_eq!(ids.len(), 8);
    for id in &ids {
        h.app.action(&h.app.run(id).unwrap(), &json!({ "type": "cancel" })).unwrap();
    }
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Requests for a run that is not live take turns: two messages sent at once to a delivered run
/// whose session is gone bring it back once (one lead session, both messages said), and once
/// the run is deleted, nothing its closing session says is written for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_is_not_live_comes_back_once() {
    let h = harness("actions-revive", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    // Only the event log is left, as after a restart.
    let old = h.app.runs.lock().unwrap().remove(&run).unwrap();
    let conns: Vec<Arc<AcpConn>> = old.st.lock().unwrap().conns.drain().map(|(_, c)| c).collect();
    for c in conns {
        c.kill().await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let calls = h.dir.join("fake/calls.log");
    let loads = || std::fs::read_to_string(&calls).unwrap().lines().filter(|l| l.starts_with("load ")).count();
    let loaded_before = loads();

    let send = |text: &'static str| {
        let (app, run) = (h.app.clone(), run.clone());
        tokio::spawn(async move {
            app.request_action(&run, &json!({ "type": "message", "agent": "lead", "text": text })).await
        })
    };
    let (a, b) = (send("第一条"), send("第二条"));
    assert!(matches!(a.await.unwrap(), Ok(ActionReply::Done)));
    assert!(matches!(b.await.unwrap(), Ok(ActionReply::Done)));
    wait_until(&h.app, &run, "both messages said", |evs| {
        ["第一条", "第二条"].iter().all(|t| {
            evs.iter().any(|e| {
                e.kind == "session.prompt"
                    && e.data.as_ref().is_some_and(|d| d["prompt"].as_str().is_some_and(|p| p.contains(t)))
            })
        })
    })
    .await;
    assert_eq!(loads() - loaded_before, 1, "one lead session reopened");

    h.app.delete_run(&run).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(h.app.db.events(&run, 0).unwrap().is_empty(), "nothing written after the delete");
    let _ = std::fs::remove_dir_all(&h.dir);
}
