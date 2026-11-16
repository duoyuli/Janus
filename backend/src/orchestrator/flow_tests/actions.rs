//! Flow tests of the person's actions as the HTTP API receives them (`App::request_action`,
//! docs/interfaces/http_api.md#run_actions).

use super::*;

/// One `commandId` acts once even when two requests carry it at the same time, an action that
/// fails gives its id back for a retry, and an unknown run is refused before anything is claimed.
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
    let duplicate = replies.iter().filter(|r| matches!(r, Ok(ActionReply::Duplicate))).count();
    assert_eq!((done, duplicate), (1, 1), "the second request with the id does nothing");
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

    assert!(matches!(h.app.request_action("no-such-run", &body).await, Err(ActionRefused::NotFound)));
    h.app.run(&run).unwrap().st.lock().unwrap().cancelled = true;
    let _ = std::fs::remove_dir_all(&h.dir);
}
