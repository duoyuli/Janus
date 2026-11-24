//! Flow tests of chats with one digital employee (`App::create_chat`,
//! docs/domains/20261120-chats.md).

use super::*;

fn calls(h: &Harness) -> String {
    std::fs::read_to_string(h.dir.join("fake").join("calls.log")).unwrap_or_default()
}

fn replied(evs: &[Event], text: &str) -> bool {
    evs.iter().any(|e| e.kind.as_str() == "agent.message" && e.text.contains(&format!("回复：{text}")))
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
        evs.iter().any(|e| e.kind.as_str() == "agent.status" && e.data.as_ref().is_some_and(|d| d["turn"] == "idle"))
    })
    .await;
    assert!(calls(&h).contains("chat session"), "the session opens before anything is said");
    assert!(!has(&events(&h.app, &id), "agent.message"), "the agent says nothing before the person writes");
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "", "untitled until the first message");

    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "你好\n第二行" }));
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    assert!(
        events(&h.app, &id)
            .iter()
            .any(|e| e.kind.as_str() == "message.routed" && e.data.as_ref().is_some_and(|d| d["delivery"] == "prompt")),
        "the first message is a turn of the open session"
    );
    let row = h.app.db.run(&id).unwrap().unwrap();
    assert_eq!(row["goal"], "你好\n第二行", "the first message titles the chat");
    assert_eq!(row["kind"], "chat");
    assert_eq!(row["status"], "running", "the session is open");
    let dir = h.app.cfg.layout.run(RunKind::Chat, &id).work.canonicalize().unwrap();
    assert_eq!(row["cwd"], json!(dir), "the chat works in its own directory under Janus home");
    assert!(dir.starts_with(h.app.cfg.layout.home().join("work/chats").canonicalize().unwrap()), "apart from the runs");
    let team = row["config"]["team"].as_array().unwrap();
    assert_eq!(team.len(), 1, "the employee is the only member");
    assert_eq!(team[0]["role_id"], "m1");
    let log = calls(&h);
    assert!(
        log.contains(&format!("chat session {}", dir.display())),
        "the session works in the chat's directory: {log}"
    );
    assert!(!log.contains("janus-brief"), "Janus adds no instructions of its own to a chat: {log}");
    assert!(
        !events(&h.app, &id)
            .iter()
            .any(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "system")),
        "only the employee's own instructions go into the system layer"
    );
    let mcp = log.lines().find(|l| l.starts_with("mcp ")).unwrap();
    assert!(!mcp.contains("\"name\": \"janus\""), "a chat has no board: {mcp}");
    assert!(mcp.contains(crate::library::MCP_ROUTER), "the library still reaches it: {mcp}");

    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "第二条" }));
    wait_until(&h.app, &id, "the second reply", |e| replied(e, "第二条")).await;
    assert_eq!(log.matches("chat session").count(), calls(&h).matches("chat session").count(), "the same session");
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "你好\n第二行", "later messages keep the title");
    assert!(
        events(&h.app, &id)
            .iter()
            .any(|e| e.kind.as_str() == "agent.status" && e.data.as_ref().is_some_and(|d| d["turn"] == "idle")),
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

/// Give `role` standing instructions, so its sessions have a system layer of the person's own.
fn with_instructions(h: &Harness, role: &str) {
    let r = crate::agents::test_role(role, json!({ "connection": "fake", "prompt": "像朋友一样聊" }));
    h.app.db.put_config(ConfigKind::Role, role, &r, now_ms()).unwrap();
}

/// Lose the chat's live session the way a restart does: only the event log survives.
async fn restart(h: &Harness, id: &str) {
    let old = h.app.runs.lock().unwrap().remove(id).unwrap();
    let conns: Vec<_> = old.st.lock().unwrap().conns.values().cloned().collect();
    for c in conns {
        c.kill().await;
    }
    h.app.db.set_run_status(id, RunStatus::Interrupted).unwrap();
}

/// The system layers the stand-in agent saw, in order: (`new` or `load`, session, layer).
fn layers(h: &Harness) -> Vec<(String, String, String)> {
    calls(h)
        .lines()
        .filter_map(|l| l.strip_prefix("layer "))
        .map(|l| {
            let mut p = l.splitn(3, ' ');
            let (m, s, t) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
            (m.to_string(), s.to_string(), serde_json::from_str::<String>(t).unwrap())
        })
        .collect()
}

/// What Janus recorded as going into the system layer (`session.prompt` with a `via`), by channel.
fn recorded_layer(evs: &[Event]) -> Vec<String> {
    evs.iter()
        .filter(|e| e.kind.as_str() == "session.prompt")
        .filter_map(|e| e.data.as_ref().filter(|d| d["via"].is_string()))
        .map(|d| d["channel"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// After a restart the chat is not live; the next message reopens it, back in the session it had.
/// The session is loaded with exactly the system layer it opened with, so the agent is told
/// nothing again: no history, no second copy of the instructions, and nothing new recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_reopens_with_the_next_message() {
    let h = harness("chat-reopen", &[("lead", false), ("m1", true)], json!({}));
    with_instructions(&h, "m1");
    let id = chat(&h, "m1", "你好").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    let sid = events(&h.app, &id)
        .iter()
        .find_map(|e| {
            e.data.as_ref()?["session"].as_str().filter(|_| e.kind.as_str() == "agent.status").map(str::to_owned)
        })
        .unwrap();
    let before = recorded_layer(&events(&h.app, &id));
    assert_eq!(before, vec!["instructions".to_string()], "a new chat records only the employee's instructions");

    restart(&h, &id).await;
    let body = json!({ "type": "message", "agent": "lead", "text": "还在吗" });
    assert!(matches!(h.app.request_action(&id, &body).await, Ok(ActionReply::Done)));
    wait_until(&h.app, &id, "the reply after reopening", |e| replied(e, "还在吗")).await;
    assert!(calls(&h).contains(&format!("load {sid}")), "the old session is loaded");
    assert_eq!(status(&h.app, &id), "running");
    let seen = layers(&h);
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!((seen[1].0.as_str(), seen[1].1.as_str()), ("load", sid.as_str()));
    assert_eq!(seen[1].2, seen[0].2, "the loaded session gets the system layer it opened with");
    assert!(seen[1].2.contains("像朋友一样聊") && !seen[1].2.contains("Recent conversation"), "{}", seen[1].2);
    assert_eq!(recorded_layer(&events(&h.app, &id)), before, "nothing new went into the loaded session");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A chat whose old session cannot be loaded goes on in a new one that gets the conversation so
/// far, recorded as history and never as Janus's collaboration instructions. Codex reads its
/// system layer from its environment, so it gets a new adapter started with the history in it.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_that_cannot_load_its_session_starts_one_with_its_history() {
    for kind in ["claude", "codex"] {
        let h = harness(&format!("chat-fresh-{kind}"), &[("lead", false), ("m1", true)], json!({}));
        let mut c = h.app.db.get_config(ConfigKind::Connection, "fake").unwrap().unwrap();
        c["kind"] = json!(kind);
        c["base"] = json!(kind);
        h.app.db.put_config(ConfigKind::Connection, "fake", &c, now_ms()).unwrap();
        with_instructions(&h, "m1");
        let id = chat(&h, "m1", "你好").await;
        wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;

        restart(&h, &id).await;
        std::fs::write(h.dir.join("fake").join("fail-load"), "").unwrap();
        let body = json!({ "type": "message", "agent": "lead", "text": "还在吗" });
        assert!(matches!(h.app.request_action(&id, &body).await, Ok(ActionReply::Done)));
        wait_until(&h.app, &id, "the reply in a new session", |e| replied(e, "还在吗")).await;
        let seen = layers(&h);
        let (method, _, layer) = seen.last().unwrap();
        assert_eq!(method, "new", "{kind}: {seen:?}");
        assert!(layer.contains("Recent conversation") && layer.contains("你好"), "{kind}: {layer}");
        assert!(layer.contains("像朋友一样聊") && !layer.contains("还在吗"), "{kind}: {layer}");
        let channels = recorded_layer(&events(&h.app, &id));
        assert_eq!(channels, vec!["instructions", "history", "instructions"], "{kind}");
        let _ = std::fs::remove_dir_all(&h.dir);
    }
}

/// Deleting a chat stops its session and removes its directory with everything Janus kept; a
/// role without an agent cannot be chatted with.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_is_deleted_with_its_directory() {
    let h = harness("chat-delete", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "写个文件").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "写个文件")).await;
    let dirs = h.app.cfg.layout.run(RunKind::Chat, &id);
    std::fs::write(dirs.work.join("notes.md"), "scratch").unwrap();
    // What the agent itself kept of the chat's session, beside another session's.
    let sid = events(&h.app, &id)
        .into_iter()
        .find_map(|e| {
            (e.kind.as_str() == "agent.status").then(|| e.data?["session"].as_str().map(str::to_owned)).flatten()
        })
        .unwrap();
    let runtime = h.app.run(&id).unwrap().team[0].runtime.clone();
    let config = h.app.cfg.layout.agent_config(&runtime);
    let transcript = config.join("projects/-chat").join(format!("{sid}.jsonl"));
    let other = config.join("projects/-chat/00000000-0000-4000-8000-000000000000.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::create_dir_all(config.join("session-env").join(&sid)).unwrap();
    std::fs::write(&transcript, "{}").unwrap();
    std::fs::write(&other, "{}").unwrap();

    assert!(h.app.delete_run(&id).await.is_ok(), "an open chat can be deleted");
    for (path, what) in [(&dirs.work, "its directory"), (&dirs.data, "its bundle"), (&dirs.logs, "its logs")] {
        assert!(!path.exists(), "{what} goes with it");
    }
    assert!(
        !transcript.exists() && !config.join("session-env").join(&sid).exists(),
        "the agent's record of it goes too"
    );
    assert!(other.exists(), "another session's record stays");
    assert!(h.app.db.run(&id).unwrap().is_none());
    assert!(h.app.run(&id).is_none());

    let mut unbound = crate::agents::test_role("m2", json!({ "connection": "" }));
    unbound["connection"] = json!("");
    h.app.db.put_config(ConfigKind::Role, "m2", &unbound, now_ms()).unwrap();
    let refused = h.app.create_chat(NewChat { role: "m2".into() }).await;
    assert!(format!("{:#}", refused.unwrap_err()).contains("还没有绑定 Agent"));
    let missing = h.app.create_chat(NewChat { role: "nobody".into() }).await;
    assert!(missing.is_err());
    assert_eq!(h.app.db.runs(RunKind::Chat).unwrap().len(), 0, "nothing was created for a refused chat");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A chat deleted while something of it is still making its log (a session opening right after
/// the chat was created, a library server starting) is removed whole: deleting waits for that log,
/// then takes the folders with it, and nothing of the run writes there again.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_chat_waits_for_a_log_being_made() {
    let h = harness("chat-delete-opening", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "你好").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    let run = h.app.run(&id).unwrap();
    let dirs = h.app.cfg.layout.run(RunKind::Chat, &id);

    // Something of the run making its log, on a thread of its own as the real ones are brief and
    // never wait on the runtime while they hold the lock.
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let making = {
        let (run, logs) = (run.clone(), dirs.logs.clone());
        std::thread::spawn(move || {
            let _files = run.files.read().unwrap();
            held_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            let waited = logs.exists();
            std::fs::write(logs.join("mcp-run-probe.log"), "started").unwrap();
            waited
        })
    };
    held_rx.recv().unwrap();
    assert!(h.app.delete_run(&id).await.is_ok(), "the chat is deleted once the log is made");
    assert!(making.join().unwrap(), "the folders waited for the log being made");
    assert!(!dirs.logs.exists() && !dirs.work.exists(), "with the log made meanwhile");
    assert!(*run.emitting.lock().unwrap(), "what still opens for it sees the mark and makes no log");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!dirs.logs.exists(), "nothing writes there again");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A first message the service refuses (an image that is not one) does not title the chat; the
/// first one it takes does (FL-15).
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_message_does_not_title_a_chat() {
    let h = harness("chat-title", &[("lead", false), ("m1", true)], json!({}));
    let id = h.app.create_chat(NewChat { role: "m1".into() }).await.unwrap();
    let r = h.app.run(&id).unwrap();
    let bad = json!({ "type": "message", "agent": "lead", "text": "草稿", "images": [{ "data": "not base64!", "name": "x.png" }] });
    assert!(h.app.action(&r, &bad).is_err());
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "");
    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "正式的问题" }));
    assert_eq!(h.app.db.run(&id).unwrap().unwrap()["goal"], "正式的问题");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Switching the model (or any control) of an open chat keeps the chat working: the next message
/// is answered in the same session.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_takes_messages_after_a_control_switch() {
    let h = harness("chat-control", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "你好").await;
    wait_until(&h.app, &id, "the first reply", |e| replied(e, "你好")).await;
    act(&h, &id, json!({ "type": "control", "agent": "lead", "id": "mode", "value": "full", "via": "config" }));
    wait_until(&h.app, &id, "the switch", |e| has(e, "session.control")).await;
    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "换了之后" }));
    wait_until(&h.app, &id, "the reply after the switch", |e| replied(e, "换了之后")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Stopping a chat's turn leaves the chat waiting for the next message: no takeover, and the next
/// message is answered in the same session.
#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_chat_turn_waits_for_the_next_message() {
    let h = harness("chat-stop", &[("lead", false), ("m1", true)], json!({}));
    let id = chat(&h, "m1", "[slow] 想一想").await;
    wait_until(&h.app, &id, "the turn at work", |e| {
        e.iter()
            .any(|x| x.kind.as_str() == "session.prompt" && x.data.as_ref().is_some_and(|d| d["channel"] == "followup"))
    })
    .await;
    act(&h, &id, json!({ "type": "interrupt", "agent": "lead" }));
    wait_until(&h.app, &id, "the stop", |e| has(e, "session.interrupted")).await;
    let stop = events(&h.app, &id).into_iter().find(|e| e.kind.as_str() == "session.interrupted").unwrap();
    assert!(stop.text.contains("停止") && !stop.text.contains("接管"), "{}", stop.text);
    assert!(!has(&events(&h.app, &id), "control.taken"));
    act(&h, &id, json!({ "type": "message", "agent": "lead", "text": "下一条" }));
    wait_until(&h.app, &id, "the next reply", |e| replied(e, "下一条")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
