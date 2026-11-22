//! Flow tests of agent seats and parking (`seats`, `parking.rs`,
//! docs/domains/20261031-run_lifecycle.md#agent_seats).

use super::*;

fn replied(evs: &[Event], text: &str) -> bool {
    evs.iter().any(|e| e.kind == "agent.message" && e.text.contains(&format!("回复：{text}")))
}

fn parked(evs: &[Event], why: &str) -> bool {
    evs.iter().any(|e| e.kind == "session.parked" && e.data.as_ref().is_some_and(|d| d["reason"] == why))
}

async fn chat(h: &Harness, text: &str) -> String {
    let id = h.app.create_chat(NewChat { role: "m1".into() }).await.unwrap();
    act(h, &id, json!({ "type": "message", "agent": "lead", "text": text }));
    wait_until(&h.app, &id, "the reply", |e| replied(e, text)).await;
    id
}

async fn gone(h: &Harness, id: &str) {
    for _ in 0..200 {
        if h.app.run(id).is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{id} stayed in memory after it was parked");
}

/// A chat idle for `idle_after` closes its agent and gives its seat back; it reads as closed, and
/// the next message opens it again where it was.
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_session_closes_and_the_next_message_opens_it() {
    let h = harness_with("seats-idle", &[("lead", false), ("m1", true)], json!({}), |c| {
        c.idle_after = Duration::from_millis(400);
    });
    let id = chat(&h, "你好").await;
    wait_until(&h.app, &id, "the session parked when idle", |e| parked(e, "idle")).await;
    gone(&h, &id).await;
    assert_eq!(status(&h.app, &id), "interrupted", "a parked chat reads as closed");
    assert_eq!(h.app.seats.open(), 0, "its seat is back");

    // As the app sends it: the closed run opens again first.
    let message = json!({ "type": "message", "agent": "lead", "text": "还在吗" });
    assert!(h.app.request_action(&id, &message).await.is_ok());
    wait_until(&h.app, &id, "the reply after reopening", |e| replied(e, "还在吗")).await;
    assert_eq!(status(&h.app, &id), "running");
    assert_eq!(h.app.seats.open(), 1);
}

/// With every seat taken, a new session waits and says so; the idlest session is parked for it,
/// and a busy one is not.
#[tokio::test(flavor = "multi_thread")]
async fn a_full_house_parks_the_idlest_session() {
    let h = harness_with("seats-full", &[("lead", false), ("m1", true)], json!({}), |c| {
        c.max_running_runs = 1;
        c.max_agent_sessions = 2;
        c.idle_after = Duration::from_secs(3600);
    });
    assert_eq!(h.app.seats.cap(), 2);
    let first = chat(&h, "一").await;
    let second = chat(&h, "二").await;
    assert_eq!(h.app.seats.open(), 2);
    let third = chat(&h, "三").await;
    assert!(parked(&events(&h.app, &first), "full"), "the idlest gave its seat up");
    assert!(!has(&events(&h.app, &second), "session.parked"), "only as many as needed");
    assert!(
        events(&h.app, &third)
            .iter()
            .any(|e| e.kind == "agent.status" && e.data.as_ref().is_some_and(|d| d["phase"] == "waiting_for_seat")),
        "the waiting session says it waits for a seat"
    );
    gone(&h, &first).await;
    assert_eq!(h.app.seats.open(), 2);
}

/// Never fewer seats than working goal runs plus one, so working leads cannot hold every seat
/// their collaborators wait for.
#[test]
fn working_leads_never_take_every_seat() {
    let dir = tempfile::tempdir().unwrap();
    let app = test_app_with(dir.path(), |c| {
        c.max_running_runs = 8;
        c.max_agent_sessions = 3;
    });
    assert_eq!(app.seats.cap(), 9);
}

/// A goal's lead is not parked while the goal is worked on, however long it waits for its team;
/// once delivered and idle it is, keeps the run's status, and a message opens it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_goal_lead_is_parked_only_after_delivery() {
    let h = harness_with("seats-goal", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"] }), |c| {
        c.idle_after = Duration::from_millis(300);
    });
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(!has(&events(&h.app, &run), "session.parked"), "the lead works on its goal, past the idle time");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    wait_until(&h.app, &run, "the delivered lead parked", |e| parked(e, "idle")).await;
    gone(&h, &run).await;
    assert_eq!(status(&h.app, &run), "delivered", "parking keeps a goal's status");

    let message = json!({ "type": "message", "agent": "lead", "text": "结果是什么？" });
    assert!(h.app.request_action(&run, &message).await.is_ok());
    wait_until(&h.app, &run, "the lead answers again", |e| e.iter().filter(|x| x.kind == "agent.message").count() >= 2)
        .await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
