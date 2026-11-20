//! Flow tests of chats with one digital employee (`App::create_chat`,
//! docs/domains/20261120-chats.md).

use super::*;

fn calls(h: &Harness) -> String {
    std::fs::read_to_string(h.dir.join("fake").join("calls.log")).unwrap_or_default()
}

fn replied(evs: &[Event], text: &str) -> bool {
    evs.iter().any(|e| e.kind == "agent.message" && e.text.contains(&format!("回复：{text}")))
}

/// A command server in the library, so sessions get the library router (`janus-tools`).
async fn with_library_server(h: &Harness) {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let config = json!({ "mcpServers": { "probe": { "command": "python3", "args": [script] } } });
    h.app.import_mcp_config(&config.to_string()).await.unwrap();
}

/// Start a chat with `role` and send it `text` once its session is open, as the app does.
async fn chat(h: &Harness, role: &str, text: &str) -> String {
    let id = h.app.create_chat(NewChat { role: role.into() }).await.unwrap();
    act(h, &id, json!({ "type": "message", "agent": "lead", "text": text }));
    id
}

/// A chat is a run of its own kind: its session opens right away with the system layer and waits
/// for the person; the chosen employee answers in it, works in a fresh directory under Janus home,
/// has no board, and takes the next message in the same session. The first message titles it.
/// Goals and chats are listed apart.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_talks_with_one_employee_in_its_own_directory() {
    let h = harness("chat", &[("lead", false), ("m1", true)], json!({}));
    with_library_server(&h).await;
    let id = h.app.create_chat(NewChat { role: "m1".into() }).await.unwrap();
    wait_until(&h.app, &id, "the session waiting for the person", |evs| {
        evs.iter().any(|e| e.kind == "agent.status" && e.data.as_ref().is_some_and(|d| d["turn"] == "idle"))
    })
    .await;
    assert!(calls(&h).contains("chat brief"), "the system layer goes in before anything is said");
    assert!(!has(&events(&h.app, &id), "agent.message"), "the agent says nothing before the person writes");
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "", "untitled until the first message");

    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "你好\n第二行" }));
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    assert!(
        events(&h.app, &id)
            .iter()
            .any(|e| e.kind == "message.routed" && e.data.as_ref().is_some_and(|d| d["delivery"] == "prompt")),
        "the first message is a turn of the open session"
    );
    let row = h.app.db.run(&id).unwrap().unwrap();
    assert_eq!(row["goal"], "你好\n第二行", "the first message titles the chat");
    assert_eq!(row["kind"], "chat");
    assert_eq!(row["status"], "running", "the session is open");
    let dir = h.app.chat_dir(&id).canonicalize().unwrap();
    assert_eq!(row["cwd"], json!(dir), "the chat works in its own directory under Janus home");
    assert!(dir.starts_with(h.app.cfg.data_dir.canonicalize().unwrap()));
    let team = row["config"]["team"].as_array().unwrap();
    assert_eq!(team.len(), 1, "the employee is the only member");
    assert_eq!(team[0]["role_id"], "m1");
    let log = calls(&h);
    assert!(log.contains(&format!("chat brief {}", dir.display())), "the agent is told it is in a chat: {log}");
    let mcp = log.lines().find(|l| l.starts_with("mcp ")).unwrap();
    assert!(!mcp.contains("\"name\": \"janus\""), "a chat has no board: {mcp}");
    assert!(mcp.contains(crate::library::MCP_ROUTER), "the library still reaches it: {mcp}");

    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "第二条" }));
    wait_until(&h.app, &id, "the second reply", |e| replied(e, "第二条")).await;
    assert_eq!(log.matches("chat brief").count(), calls(&h).matches("chat brief").count(), "the same session");
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "你好\n第二行", "later messages keep the title");
    assert!(
        events(&h.app, &id)
            .iter()
            .any(|e| e.kind == "agent.status" && e.data.as_ref().is_some_and(|d| d["turn"] == "idle")),
        "after each turn the session waits for the person"
    );

    let goals = h.app.db.runs(RunKind::Goal).unwrap();
    let chats = h.app.db.runs(RunKind::Chat).unwrap();
    assert!(!goals.iter().any(|r| r["id"] == json!(id)), "a chat is not on a workspace's list");
    assert!(chats.iter().any(|r| r["id"] == json!(id)));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The token a chat's agent gets for its library router (read from the URL its session opened
/// with, as the agent reads it) is valid there, and refused by the board: a chat has none to
/// orchestrate on.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_agent_cannot_reach_a_board() {
    let h = harness("chat-board", &[("lead", false), ("m1", true)], json!({}));
    with_library_server(&h).await;
    let id = chat(&h, "m1", "你好").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    let log = calls(&h);
    let mcp: Value = serde_json::from_str(log.lines().find(|l| l.starts_with("mcp ")).unwrap()[4..].trim()).unwrap();
    let url = mcp.as_array().unwrap().iter().find_map(|s| s["url"].as_str()).unwrap().to_string();
    let parts: Vec<&str> = url.split('/').collect();
    let (token, who) = (parts[parts.len() - 3], parts[parts.len() - 2]);
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} });
    assert!(h.app.mcp_tools(&id, token, who, list.clone()).await.is_ok(), "the router takes its own token");
    let call = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "orchestrate", "arguments": { "tasks": [] } } });
    for req in [list, call] {
        let refused = h.app.mcp(&id, token, who, req).await.unwrap_err();
        assert!(format!("{refused:#}").contains("对话没有共享看板"), "{refused:#}");
    }
    assert!(!has(&events(&h.app, &id), "orchestration.accepted"));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// After a restart the chat is not live; the next message reopens it, back in the session it had.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_reopens_with_the_next_message() {
    let h = harness("chat-reopen", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "你好").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    let sid = events(&h.app, &id)
        .iter()
        .find_map(|e| e.data.as_ref()?["session"].as_str().filter(|_| e.kind == "agent.status").map(str::to_owned))
        .unwrap();

    // Only the event log survives a restart.
    let old = h.app.runs.lock().unwrap().remove(&id).unwrap();
    for c in old.st.lock().unwrap().conns.values() {
        let c = c.clone();
        tokio::spawn(async move { c.kill().await });
    }
    h.app.db.set_run_status(&id, RunStatus::Interrupted).unwrap();
    let body = json!({ "type": "message", "agent": "lead", "text": "还在吗" });
    assert!(matches!(h.app.request_action(&id, &body).await, Ok(ActionReply::Done)));
    wait_until(&h.app, &id, "the reply after reopening", |e| replied(e, "还在吗")).await;
    assert!(calls(&h).contains(&format!("load {sid}")), "the old session is loaded");
    assert_eq!(status(&h.app, &id), "running");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Deleting a chat stops its session and removes its directory with everything Janus kept; a
/// role without an agent cannot be chatted with.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_is_deleted_with_its_directory() {
    let h = harness("chat-delete", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "写个文件").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "写个文件")).await;
    let dir = h.app.chat_dir(&id);
    std::fs::write(dir.join("notes.md"), "scratch").unwrap();

    assert!(h.app.delete_run(&id).await.is_ok(), "an open chat can be deleted");
    assert!(!dir.exists(), "its directory goes with it");
    assert!(h.app.db.run(&id).unwrap().is_none());
    assert!(h.app.run(&id).is_none());

    let mut unbound = crate::agents::test_role("m2", json!({ "connection": "" }));
    unbound["connection"] = json!("");
    h.app.db.put_config("role", "m2", &unbound, now_ms()).unwrap();
    let refused = h.app.create_chat(NewChat { role: "m2".into() }).await;
    assert!(format!("{:#}", refused.unwrap_err()).contains("还没有绑定 Agent"));
    let missing = h.app.create_chat(NewChat { role: "nobody".into() }).await;
    assert!(missing.is_err());
    assert_eq!(h.app.db.runs(RunKind::Chat).unwrap().len(), 0, "nothing was created for a refused chat");
    let _ = std::fs::remove_dir_all(&h.dir);
}
