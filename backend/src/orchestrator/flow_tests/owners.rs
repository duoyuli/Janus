//! Sessions and the decisions they wait on belong to someone named outright: a task's attempt or
//! the lead. Nothing is matched by the shape of a key or an id.

use super::*;

fn asked(task: &'static str) -> impl Fn(&[Event]) -> bool {
    move |e| e.iter().any(|e| e.kind == "question.asked" && e.task.as_deref() == Some(task))
}

fn succeeded(task: &'static str) -> impl Fn(&[Event]) -> bool {
    move |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some(task))
}

/// `api` and `api-1` both ask. The lead's reply to `api` answers `api`, and `api` ending leaves
/// the question of `api-1` waiting for its own answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_never_touches_the_decisions_of_a_task_whose_id_extends_its_own() {
    let h = harness(
        "owners-prefix",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "api": ["ask", "ok"], "api-1": ["ask", "ok"] }),
    );
    let run = start(&h, json!([task("api", "m1", json!({})), task("api-1", "m2", json!({}))])).await;
    wait_until(&h.app, &run, "api asks", asked("api")).await;
    wait_until(&h.app, &run, "api-1 asks", asked("api-1")).await;
    let key_of = |t: &str| {
        events(&h.app, &run)
            .into_iter()
            .find(|e| e.kind == "question.asked" && e.task.as_deref() == Some(t))
            .unwrap()
            .data
            .unwrap()["key"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (mine, theirs) = (key_of("api"), key_of("api-1"));

    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "api", "text": "用 A 方案" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "api succeeds", succeeded("api")).await;
    let evs = events(&h.app, &run);
    let answered: Vec<String> = evs
        .iter()
        .filter(|e| e.kind == "question.answered")
        .map(|e| e.data.as_ref().unwrap()["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(answered, vec![mine], "the reply to api answers api's question");
    assert!(
        !evs.iter().any(|e| e.kind == "decision.withdrawn" && e.data.as_ref().unwrap()["key"] == theirs.as_str()),
        "api ending leaves api-1's question"
    );
    assert!(h.app.run(&run).unwrap().st.lock().unwrap().pending.contains_key(&theirs));

    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "api-1", "text": "用 B 方案" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "api-1 succeeds", succeeded("api-1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A task may be called `lead`: its session and the lead's are different sessions, so the task
/// ending leaves the lead's session open, and a message to the lead still reaches the lead.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_called_lead_is_not_the_lead() {
    let h = harness("owners-lead", &[("lead", false), ("m1", true)], json!({ "lead": ["ok"] }));
    let run = start(&h, json!([task("lead", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "the task called lead succeeds", succeeded("lead")).await;
    let r = h.app.run(&run).unwrap();
    {
        let st = r.st.lock().unwrap();
        assert!(st.live.contains_key(&SessionKey::Lead), "the lead's session is still open");
        assert_eq!(st.live_agent.get("lead"), Some(&SessionKey::Lead));
    }
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "主控请看一下" }));
    wait_until(&h.app, &run, "the message reaches the lead's session", |e| {
        e.iter().any(|e| e.kind == "message.routed" && e.to.as_deref() == Some("lead") && e.task.is_none())
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
