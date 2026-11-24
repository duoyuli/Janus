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
    events(&h.app, run).iter().filter(|e| e.kind.as_str() == "run.delivered").count()
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
    // The turn says it ended, so the app knows the delivery stands and the lead waits again.
    wait_until(&h.app, &run, "the turn's end", |e| {
        e.iter().any(|x| x.kind.as_str() == "agent.status" && x.text == "这一轮已结束，等待你的消息")
    })
    .await;
    assert_eq!((status(&h.app, &run).as_str(), deliveries(&h, &run)), ("accepted", 1), "a question changes nothing");

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "补一份说明 [write extra.md]" }));
    wait_until(&h.app, &run, "a second delivery", |e| {
        e.iter().filter(|x| x.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    assert_eq!(status(&h.app, &run), "delivered", "the new work waits for acceptance");
    // The second delivery is this round alone: no subtask worked in it, the lead wrote one file.
    let second =
        events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "run.delivered").nth(1).unwrap().data.unwrap();
    assert_eq!(second["acceptance"], json!([]), "{second}");
    assert_eq!(second["evidence"], json!([]), "{second}");
    assert_eq!(second["changed"], json!(["extra.md"]), "{second}");
    assert_eq!(second["leadEdits"], json!(["extra.md"]), "{second}");
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
        e.iter()
            .any(|x| x.kind.as_str() == "session.prompt" && x.data.as_ref().is_some_and(|d| d["channel"] == "followup"))
    })
    .await;
    let r = h.app.run(&run).unwrap();
    let refused = h.app.action(&r, &action_of(json!({ "type": "accept" }))).unwrap_err();
    assert!(format!("{refused:#}").contains("负责人还在处理"), "{refused:#}");
    assert_eq!(status(&h.app, &run), "delivered");
    // The result is out already: stopping would only end the conversation, so it is refused.
    let refused = h.app.action(&r, &action_of(json!({ "type": "cancel" }))).unwrap_err();
    assert!(format!("{refused:#}").contains("已经交付过"), "{refused:#}");
    assert_eq!(status(&h.app, &run), "delivered");
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
    assert!(matches!(h.app.request_action(&run, &req(body)).await, Ok(ActionReply::Done)));
    wait_until(&h.app, &run, "the message in a reopened session", |e| {
        e.iter().any(|x| {
            x.kind.as_str() == "session.prompt"
                && x.data.as_ref().is_some_and(|d| d["prompt"].as_str().is_some_and(|t| t.contains("还在吗")))
        })
    })
    .await;
    let prompts = events(&h.app, &run)
        .iter()
        .filter(|x| {
            x.kind.as_str() == "session.prompt"
                && x.data.as_ref().is_some_and(|d| d["prompt"].as_str().is_some_and(|t| t.contains("还在吗")))
        })
        .count();
    assert_eq!(prompts, 1, "delivered once");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A run delivered before the service restarted is accepted from its stored record: acceptance
/// needs no agent, so no session is reopened, and the app offers it as it does for a live run.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_delivered_before_a_restart_is_accepted() {
    let h = harness("deliver-restart", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"] }));
    let run = delivered(&h).await;
    // Only the stored run is left, as after a restart.
    let old = h.app.runs.lock().unwrap().remove(&run).unwrap();
    let conns: Vec<Arc<AcpConn>> = old.st.lock().unwrap().conns.drain().map(|(_, c)| c).collect();
    for c in conns {
        c.kill().await;
    }
    let loads = || {
        std::fs::read_to_string(h.dir.join("fake/calls.log"))
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("load "))
            .count()
    };
    let before = loads();
    assert!(matches!(h.app.request_action(&run, &req(json!({ "type": "accept" }))).await, Ok(ActionReply::Done)));
    assert_eq!(status(&h.app, &run), "accepted");
    assert!(has(&events(&h.app, &run), "risk.accepted"));
    assert!(h.app.run(&run).is_none() && loads() == before, "accepting opened no session");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A new session's history leaves out the message still waiting for the agent: that message is
/// the session's first prompt, and history plus prompt would hand it over twice.
#[test]
fn history_leaves_out_the_message_still_waiting() {
    let e = |seq: i64, kind: &str, text: &str, data: Value| Event {
        run_id: "r".into(),
        seq,
        ts: seq,
        kind: kind.into(),
        actor: "user".into(),
        to: Some("lead".into()),
        task: None,
        attempt: None,
        text: text.into(),
        data: Some(data),
    };
    let events = vec![
        e(1, "user.message", "先做这个", json!({ "id": "a" })),
        e(2, "message.routed", "", json!({ "id": "a", "delivery": "prompt" })),
        e(3, "agent.message", "做好了", json!({})),
        e(4, "user.message", "还在吗？", json!({ "id": "b" })),
        e(5, "message.routed", "", json!({ "id": "b", "delivery": "next" })),
    ];
    let history = crate::orchestrator::lead::recent_exchanges(&events, "Lead");
    assert_eq!(history, "User: 先做这个\nLead: 做好了");
}

/// Outside Git there is no branch to compare with: the lead's own edits after a delivery are what
/// makes the run delivered again.
#[tokio::test(flavor = "multi_thread")]
async fn work_after_delivery_outside_git_is_delivered_again() {
    let h = harness("deliver-plain", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"] }));
    std::fs::remove_dir_all(h.repo.join(".git")).unwrap();
    let run = delivered(&h).await;
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "补一份说明 [write extra.md]" }));
    wait_until(&h.app, &run, "a second delivery", |e| {
        e.iter().filter(|x| x.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    let second =
        events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "run.delivered").nth(1).unwrap().data.unwrap();
    assert_eq!(second["leadEdits"], json!(["extra.md"]), "{second}");
    assert_eq!(second["changed"], json!(["extra.md"]), "{second}");
    assert_eq!(second["merge"]["state"], "inPlace", "{second}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Interrupting the lead after a delivery hands it to the person, in a reopened conversation as in
/// the first session: what the cut-off turn changed is not delivered (and merged) until the person
/// says the turn's work stands as it is.
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_turn_after_delivery_waits_for_the_person() {
    let h = harness("deliver-held", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"], "unknown": ["slow"] }));
    let run = delivered(&h).await;
    // The lead's agent exits: the next message reopens the run as a conversation.
    let conn = h.app.run(&run).unwrap().st.lock().unwrap().conns.get(&SessionKey::Lead).cloned().unwrap();
    conn.kill().await;
    for _ in 0..100 {
        if h.app.run(&run).is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let body = json!({ "type": "message", "agent": "lead", "text": "再检查一遍" });
    assert!(matches!(h.app.request_action(&run, &req(body)).await, Ok(ActionReply::Done)));
    wait_until(&h.app, &run, "the slow turn", |e| {
        e.iter().any(|x| {
            x.kind.as_str() == "session.prompt" && x.data.as_ref().is_some_and(|d| d["prompt"] == "再检查一遍")
        })
    })
    .await;
    let cwd = h.app.run(&run).unwrap().cwd.clone();
    std::fs::write(cwd.join("half.md"), "half\n").unwrap();
    act(&h, &run, json!({ "type": "interrupt", "agent": "lead" }));
    wait_until(&h.app, &run, "the person holds the lead", |e| {
        e.iter().any(|x| x.kind.as_str() == "agent.status" && x.text.contains("人工接管中"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(deliveries(&h, &run), 1, "nothing half done is delivered while the person holds it");
    assert!(!h.repo.join("half.md").exists());
    act(&h, &run, json!({ "type": "finish", "agent": "lead" }));
    wait_until(&h.app, &run, "delivered as it is", |e| {
        e.iter().filter(|x| x.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    assert!(h.repo.join("half.md").exists(), "the person let the turn's work stand");
    let _ = std::fs::remove_dir_all(&h.dir);
}
