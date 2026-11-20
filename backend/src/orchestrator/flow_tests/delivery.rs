//! Flow tests of what follows a delivery (docs/domains/20261031-run_lifecycle.md#delivery_conditions):
//! work after acceptance is delivered again, acceptance waits for the lead, and a lead session
//! that ended is reopened by the next message.

use super::*;

async fn delivered(h: &Harness) -> String {
    let run = start(h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    run
}

fn deliveries(h: &Harness, run: &str) -> usize {
    events(&h.app, run).iter().filter(|e| e.kind == "run.delivered").count()
}

/// After acceptance the person asks the lead for more: the files it writes are delivered again, so
/// a second acceptance brings them back to the person's branch instead of leaving them in the
/// worktree (FL-03). A question that changes nothing leaves the acceptance as it was.
#[tokio::test(flavor = "multi_thread")]
async fn work_after_acceptance_is_delivered_again() {
    let h = harness("deliver-again", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"] }));
    let run = delivered(&h).await;
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(status(&h.app, &run), "accepted");

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "结果是什么？" }));
    wait_until(&h.app, &run, "an answer", |e| e.iter().filter(|x| x.kind == "agent.message").count() >= 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!((status(&h.app, &run).as_str(), deliveries(&h, &run)), ("accepted", 1), "a question changes nothing");

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "补一份说明 [write extra.md]" }));
    wait_until(&h.app, &run, "a second delivery", |e| e.iter().filter(|x| x.kind == "run.delivered").count() == 2)
        .await;
    assert_eq!(status(&h.app, &run), "delivered", "the new work waits for acceptance");
    act(&h, &run, json!({ "type": "accept" }));
    assert!(h.repo.join("extra.md").exists(), "accepting brings the follow-up's file back");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Acceptance waits while the lead still works on what the person asked after delivery.
#[tokio::test(flavor = "multi_thread")]
async fn acceptance_waits_for_the_lead() {
    let h = harness("deliver-busy", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"], "unknown": ["slow"] }));
    let run = delivered(&h).await;
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "再检查一遍" }));
    wait_until(&h.app, &run, "the follow-up prompt", |e| {
        e.iter().any(|x| x.kind == "session.prompt" && x.data.as_ref().is_some_and(|d| d["channel"] == "followup"))
    })
    .await;
    let r = h.app.run(&run).unwrap();
    let refused = h.app.action(&r, &json!({ "type": "accept" })).unwrap_err();
    assert!(format!("{refused:#}").contains("负责人还在处理"), "{refused:#}");
    assert_eq!(status(&h.app, &run), "delivered");
    act(&h, &run, json!({ "type": "cancel" }));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead's agent exits after delivery: the run leaves memory, and the next message reopens the
/// conversation and reaches the lead once (FL-21).
#[tokio::test(flavor = "multi_thread")]
async fn a_lead_that_exited_after_delivery_is_reopened_by_the_next_message() {
    let h = harness("deliver-exit", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"] }));
    let run = delivered(&h).await;
    let conn = h.app.run(&run).unwrap().st.lock().unwrap().conns.get(&SessionKey::Lead).cloned().unwrap();
    conn.kill().await;
    for _ in 0..100 {
        if h.app.run(&run).is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(h.app.run(&run).is_none(), "nothing listens in the run any more");
    let body = json!({ "type": "message", "agent": "lead", "text": "还在吗？" });
    assert!(matches!(h.app.request_action(&run, &body).await, Ok(ActionReply::Done)));
    wait_until(&h.app, &run, "the message in a reopened session", |e| {
        e.iter().any(|x| {
            x.kind == "session.prompt"
                && x.data.as_ref().is_some_and(|d| d["prompt"].as_str().is_some_and(|t| t.contains("还在吗")))
        })
    })
    .await;
    let prompts = events(&h.app, &run)
        .iter()
        .filter(|x| {
            x.kind == "session.prompt"
                && x.data.as_ref().is_some_and(|d| d["prompt"].as_str().is_some_and(|t| t.contains("还在吗")))
        })
        .count();
    assert_eq!(prompts, 1, "delivered once");
    let _ = std::fs::remove_dir_all(&h.dir);
}
