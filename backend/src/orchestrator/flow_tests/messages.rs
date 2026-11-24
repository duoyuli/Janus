//! Messages from the person and the lead: shown, routed and delivered, after delivery and
//! acceptance too, into a long turn without waiting for it, and with pasted images.

use super::*;

/// A message shows the moment it is sent (`pending`), then says how it went in: queued while
/// the agent waits on a request, delivered by id when the turn ends; a note for an agent with
/// no session goes in with its next one.
#[tokio::test(flavor = "multi_thread")]
async fn messages_are_shown_then_routed_and_delivered() {
    let h = harness("messages", &[("lead", false), ("m1", false)], json!({ "T1": ["permission", "ok"], "T2": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "permission request", |e| has(e, "approval.requested")).await;
    act(&h, &run, json!({ "type": "message", "agent": "m1", "text": "顺便看看 README" }));
    let evs = events(&h.app, &run);
    let sent = evs.iter().find(|e| e.kind.as_str() == "user.message").expect("recorded at once");
    let d = sent.data.as_ref().unwrap();
    assert_eq!((d["delivery"].as_str(), sent.to.as_deref(), sent.task.as_deref()), (Some("pending"), Some("m1"), None));
    let id = d["id"].as_str().unwrap().to_string();
    let routed = |evs: &[Event], how: &str| {
        evs.iter().any(|e| {
            e.kind.as_str() == "message.routed"
                && e.data.as_ref().is_some_and(|d| d["id"] == id.as_str() && d["delivery"] == how)
        })
    };
    wait_until(&h.app, &run, "queued", |e| routed(e, "queued")).await;
    let req = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "approval.requested").unwrap();
    act(
        &h,
        &run,
        json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T1", "allow": true, "option": "a1" }),
    );
    wait_until(&h.app, &run, "delivered", |e| {
        e.iter().any(|e| {
            e.kind.as_str() == "message.delivered"
                && e.data
                    .as_ref()
                    .is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id.as_str())))
        })
    })
    .await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;

    // No session now: a note, carried into m1's next session.
    act(&h, &run, json!({ "type": "message", "agent": "m1", "text": "下一个子任务注意测试" }));
    let note = events(&h.app, &run).into_iter().rev().find(|e| e.kind.as_str() == "user.message").unwrap();
    let note_id = note.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
    assert!(events(&h.app, &run).iter().any(|e| e.kind.as_str() == "message.routed"
        && e.data.as_ref().is_some_and(|d| d["id"] == note_id.as_str() && d["delivery"] == "next")));
    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T2", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "note delivered", |e| {
        e.iter().any(|e| {
            e.kind.as_str() == "message.delivered"
                && e.data
                    .as_ref()
                    .is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == note_id.as_str())))
        })
    })
    .await;
    // The note is taken (and reported delivered) while T2's prompt is written, a moment before
    // the attempt is recorded as started.
    let started = |e: &Event| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("T2");
    wait_until(&h.app, &run, "T2 started", |e| e.iter().any(started)).await;
    let t2 = events(&h.app, &run).into_iter().find(started).unwrap();
    assert!(
        t2.data.as_ref().unwrap()["prompt"].as_str().unwrap().contains("下一个子任务注意测试"),
        "the note is in T2's prompt"
    );
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// After delivery the lead session stays open: the person's next instruction runs as an
/// ordinary turn in the same session, without creating another run.
#[tokio::test(flavor = "multi_thread")]
async fn a_delivered_goal_stays_open_for_the_next_instruction() {
    let h = harness("converse", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "再看看 README" }));
    wait_until(&h.app, &run, "follow-up prompt", |evs| {
        evs.iter().any(|e| {
            e.kind.as_str() == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "followup" && d["prompt"].as_str().is_some_and(|t| t.contains("再看看 README"))
                })
        })
    })
    .await;
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| e.kind.as_str() == "user.message"
            && e.data.as_ref().is_some_and(|d| d["text"].as_str().is_some_and(|t| t.contains("再看看 README")))),
        "the message is shown in the same session"
    );
    assert_eq!(evs.iter().filter(|e| e.kind.as_str() == "run.created").count(), 1, "no new run");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Acceptance does not close the conversation: the person may still give it another task.
#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_goal_still_takes_instructions() {
    let h = harness("converse-accepted", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    act(&h, &run, json!({ "type": "accept" }));
    wait_until(&h.app, &run, "accepted", |e| has(e, "risk.accepted")).await;

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "再补一份说明" }));
    wait_until(&h.app, &run, "follow-up prompt", |evs| {
        evs.iter().any(|e| {
            e.kind.as_str() == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "followup" && d["prompt"].as_str().is_some_and(|t| t.contains("再补一份说明"))
                })
        })
    })
    .await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A delivered run whose live session is gone (after a restart) reopens when
/// the person writes to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_historical_delivered_run_reopens_for_chat() {
    let h = harness("converse-history", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    // Only the event log survives: the live Run is gone.
    h.app.runs.lock().unwrap().remove(&run);
    let reopened = h.app.resume_run(&run).await.expect("a delivered run reopens");
    h.app
        .action(&reopened, &action_of(json!({ "type": "message", "agent": "lead", "text": "历史子任务继续" })))
        .unwrap();
    wait_until(&h.app, &run, "reopened prompt", |evs| {
        evs.iter().any(|e| {
            e.kind.as_str() == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "followup" && d["prompt"].as_str().is_some_and(|t| t.contains("历史子任务继续"))
                })
        })
    })
    .await;
    assert!(
        events(&h.app, &run).iter().filter(|e| e.kind.as_str() == "session.controls").count() >= 2,
        "the reopened session offers its model and controls"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A slash command after delivery sets the next goal in the same session.
#[tokio::test(flavor = "multi_thread")]
async fn a_delivered_goal_takes_a_new_slash_command() {
    let h = harness("converse-goal", &[("lead", false), ("m1", false)], json!({ "T1": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "/goal 新目标" }));
    wait_until(&h.app, &run, "new goal prompt", |evs| {
        evs.iter().any(|e| {
            e.kind.as_str() == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "goal" && d["prompt"].as_str().is_some_and(|t| t.contains("新目标"))
                })
        })
    })
    .await;
    assert_eq!(events(&h.app, &run).iter().filter(|e| e.kind.as_str() == "run.created").count(), 1, "no new run");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// After delivery the lead is on the board again while it works on a follow-up goal: a question
/// from a task it orchestrated there is pushed to it, not escalated. Once its turn ends and it
/// waits for the person, the question still open goes to the person.
#[tokio::test(flavor = "multi_thread")]
async fn a_follow_up_goal_keeps_questions_with_the_lead_while_it_works() {
    let h = harness("converse-duty", &[("lead", false), ("m1", true)], json!({ "T1": ["ok"], "T2": ["ask", "ok"] }));
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    // The fake lead works on a goal until `lead-go` appears again.
    std::fs::remove_file(h.dir.join("fake/lead-go")).unwrap();
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "/goal 第二个目标" }));
    let calls = h.dir.join("fake/calls.log");
    let logged = |text: &str| std::fs::read_to_string(&calls).unwrap_or_default().contains(text);
    for _ in 0..100 {
        if logged("goal 第二个目标") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T2", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T2 asks", |e| task_event(e, "question.asked", "T2")).await;
    for _ in 0..100 {
        if logged("[提问] T2") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(logged("steer [Janus 看板推送]") && logged("[提问] T2"), "the working lead hears of the question");
    assert!(!has(&events(&h.app, &run), "question.escalated"), "the lead answers while it works");

    // The lead's turn ends: it waits for the person, and so does the question.
    finish_lead(&h);
    wait_until(&h.app, &run, "escalation", |e| has(e, "question.escalated")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead's word reaches a collaborator in a long turn right away (the turn is stopped when
/// the agent cannot take it mid-turn), but waits while the person holds the session.
#[tokio::test(flavor = "multi_thread")]
async fn lead_reply_does_not_wait_for_a_long_turn() {
    let h = harness(
        "redirect",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["slow", "ok"], "T2": ["slow", "ok"] }),
    );
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m2", json!({}))])).await;
    wait_until(&h.app, &run, "both running", |e| {
        task_event(e, "attempt.started", "T1") && task_event(e, "attempt.started", "T2")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T1", "text": "改用方案 B" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1 after the reply", |e| task_event(e, "task.succeeded", "T1")).await;
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind.as_str() == "message.routed"
        && e.task.as_deref() == Some("T1")
        && e.data.as_ref().is_some_and(|d| d["delivery"] == "interrupt")));
    assert!(task_event(&evs, "message.delivered", "T1"));

    // The person takes T2 over: the lead's message waits until the session is handed back.
    act(&h, &run, json!({ "type": "interrupt", "task": "T2" }));
    wait_until(&h.app, &run, "T2 waiting for the person", |e| {
        e.iter().any(|e| {
            e.kind.as_str() == "agent.status" && e.task.as_deref() == Some("T2") && e.text.contains("人工接管")
        })
    })
    .await;
    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "先停一下" })).await;
    assert!(!err);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind.as_str() == "message.routed"
        && e.task.as_deref() == Some("T2")
        && e.data.as_ref().is_some_and(|d| d["delivery"] == "queued")));
    assert!(!task_event(&evs, "message.delivered", "T2"), "nothing goes in while the person holds the session");
    act(&h, &run, json!({ "type": "release", "task": "T2" }));
    wait_until(&h.app, &run, "T2 after release", |e| {
        task_event(e, "message.delivered", "T2") && task_event(e, "task.succeeded", "T2")
    })
    .await;

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Pasted images reach the agent: the goal's are named in the lead's brief, for it to read, and a
/// message's go into the turn it joins as image blocks, with their paths named in the text. They are stored
/// with the run, and the events carry the paths, not the bytes.
#[tokio::test(flavor = "multi_thread")]
async fn pasted_images_reach_the_agent() {
    use base64::Engine;
    let h = harness("images", &[("lead", false)], json!({}));
    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-goal");
    let shot = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-message");
    let run = h
        .app
        .create_run(NewRun {
            goal: "照着图改".into(),
            workspace: "ws".into(),
            images: vec![json!({ "data": png, "name": "设计稿.png" })],
            max_tokens: None,
        })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    let logged = |what: &'static str, n: usize| {
        let calls = calls.clone();
        async move {
            for _ in 0..100 {
                if std::fs::read_to_string(&calls).unwrap_or_default().matches(what).count() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("{what} ×{n} not in {}", std::fs::read_to_string(&calls).unwrap_or_default());
        }
    };
    logged("goal 照着图改", 1).await;
    let evs = events(&h.app, &run);
    let created = evs.iter().find(|e| e.kind.as_str() == "run.created").unwrap().data.clone().unwrap();
    let goal_image = created["images"][0]["path"].as_str().unwrap().to_string();
    assert!(goal_image.contains(&format!("/runs/{run}/attachments/")), "{goal_image}");
    assert_eq!(std::fs::read(&goal_image).unwrap(), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-goal");
    let brief = evs
        .iter()
        .find(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "system"))
        .unwrap();
    assert!(
        brief.data.as_ref().unwrap()["prompt"].as_str().unwrap().contains(&format!("[Image 1] {goal_image}")),
        "the brief in the system layer names the goal's image for the lead to read"
    );

    // A message with an image and no words: queued while the goal turn runs, then its own turn.
    act(
        &h,
        &run,
        json!({ "type": "message", "agent": "lead", "text": "", "images": [{ "data": shot, "name": "报错.png" }] }),
    );
    let evs = events(&h.app, &run);
    let sent = evs.iter().find(|e| e.kind.as_str() == "user.message").unwrap();
    let d = sent.data.as_ref().unwrap();
    let path = d["images"][0]["path"].as_str().unwrap();
    assert!(d["text"].as_str().unwrap().contains(&format!("[Image 1] {path}")), "{d}");
    assert_eq!(d["images"][0]["name"], "报错.png");
    assert!(!serde_json::to_string(&evs).unwrap().contains(&shot), "events hold paths, not bytes");
    std::fs::write(h.dir.join("fake/lead-go"), "看过了").unwrap();
    logged("images image/png", 1).await;
    assert!(std::fs::read_to_string(&calls).unwrap().contains(&format!("images image/png:{}", shot.len())));

    // Not an image: refused, nothing recorded.
    let bad = h.app.action(
        &h.app.run(&run).unwrap(),
        &action_of(json!({ "type": "message", "agent": "lead", "text": "x", "images": [{ "data": "aGVsbG8=" }] })),
    );
    assert!(bad.is_err());
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    let _ = std::fs::remove_dir_all(&h.dir);
}
