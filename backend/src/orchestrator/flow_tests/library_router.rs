//! The library reaches every digital worker through the MCP router (`janus-tools`): a command
//! server for everyone is not handed to each session but served by the run's router, started when
//! its tools are first listed or called, in the run's working directory, its list always current.

use super::*;

/// The router's address as a session got it in `session/new` (`mcp [...]` in `calls.log`): the
/// credential a real client holds, never read from the run's state (AGENTS.md rule 1).
async fn router_url(h: &Harness, who: &str) -> (String, String, String) {
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..200 {
        let content = std::fs::read_to_string(&calls).unwrap_or_default();
        for line in content.lines().rev().filter_map(|l| l.strip_prefix("mcp ")) {
            let servers: Vec<Value> = serde_json::from_str(line).unwrap_or_default();
            for s in servers.iter().filter(|s| s["name"] == crate::library::MCP_ROUTER) {
                let parts: Vec<&str> = s["url"].as_str().unwrap_or_default().split('/').collect();
                let n = parts.len();
                if n >= 5 && parts[n - 2] == who {
                    return (parts[n - 4].to_string(), parts[n - 3].to_string(), parts[n - 2].to_string());
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no router for {who} in {}", std::fs::read_to_string(&calls).unwrap_or_default());
}

async fn call(h: &Harness, (run, token, who): &(String, String, String), method: &str, params: Value) -> Result<Value> {
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    Ok(h.app.mcp_tools(run, token, who, req).await?.unwrap()["result"].clone())
}

async fn listed(h: &Harness, at: &(String, String, String)) -> Vec<String> {
    let r = call(h, at, "tools/list", json!({})).await.unwrap();
    r["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn servers_for_everyone_are_served_by_the_runs_router() {
    let h = harness("library-router", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"], "T2": ["slow"] }));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let starts = h.dir.join("mcp-starts.log");
    let config = json!({ "mcpServers": { "probe": {
        "command": "python3", "args": [script], "env": { "FAKE_MCP_LOG": starts, "OWN": "mine", "HOME": "/etc" }
    } } });
    h.app.import_mcp_config(&config.to_string()).await.unwrap();
    let probe = h.app.items().unwrap().into_iter().find(|i| i.name == "probe").unwrap();
    assert_eq!(probe.reach, crate::library::Reach::Picked, "an MCP server starts kept for some");
    h.app.set_reach(&probe.id, crate::library::Reach::Everyone).await.unwrap();

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    let at = router_url(&h, "T1").await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(!calls.contains("\"name\":\"probe\""), "the server is not handed to the session itself: {calls}");
    let count = || std::fs::read_to_string(&starts).unwrap_or_default().lines().count();
    assert_eq!(count(), 0, "nothing starts before it is needed");

    let init = call(&h, &at, "initialize", json!({})).await.unwrap();
    assert!(init["instructions"].as_str().unwrap().contains("probe"), "{init}");
    let tools = call(&h, &at, "tools/list", json!({})).await.unwrap();
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["probe__cwd", "probe__echo", "probe__roots", "probe__env", "probe__grow", "probe__quit"]);
    assert_eq!(count(), 1, "listing starts the server to ask for its current tools");

    let cwd = h.app.db.run(&run).unwrap().unwrap()["cwd"].as_str().unwrap().to_string();
    let got = call(&h, &at, "tools/call", json!({ "name": "probe__cwd", "arguments": {} })).await.unwrap();
    assert_eq!(text(&got), cwd, "it runs in the run's working directory (its worktree)");
    let roots = call(&h, &at, "tools/call", json!({ "name": "probe__roots", "arguments": {} })).await.unwrap();
    assert_eq!(text(&roots), format!("file://{cwd}"), "the router answers the server's roots/list");
    assert_eq!(count(), 1, "the run's sessions share the running server");
    // Its own sandbox, never Janus's environment: a variable of this process is not there, its own
    // values are, and they can't move HOME.
    let seen = |name: &str| {
        let h = &h;
        let at = &at;
        let name = name.to_string();
        async move {
            let r = call(h, at, "tools/call", json!({ "name": "probe__env", "arguments": { "name": name } }))
                .await
                .unwrap();
            serde_json::from_str::<Value>(&text(&r)).unwrap()
        }
    };
    let env = seen("CARGO_PKG_NAME").await;
    assert!(
        std::env::var("CARGO_PKG_NAME").is_ok() && env["CARGO_PKG_NAME"].is_null(),
        "Janus's variables stay out: {env}"
    );
    assert_eq!(env["HOME"], json!(h.app.server_home(&probe.id).join("root")), "a HOME of its own");
    assert_eq!(seen("OWN").await["OWN"], "mine", "its own values are there");
    let missing = call(&h, &at, "tools/call", json!({ "name": "probe__nope", "arguments": {} })).await.unwrap();
    assert!(missing["isError"] == true && text(&missing).contains("没有这个工具"), "{missing}");
    let routed: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind == "mcp.routed").filter_map(|e| e.data).collect();
    assert_eq!(routed[0]["server"], "probe");
    assert_eq!(routed[0]["tool"], "probe__cwd");
    assert!(routed.iter().all(|d| d.get("arguments").is_none() && d.get("args").is_none()), "never the arguments");

    // A collaborator's token is bound to its name: it cannot reach the router as the lead.
    let (r, token, _) = &at;
    assert!(
        h.app.mcp_tools(r, token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );

    // A server that says its tools changed is asked again: the new tool shows and can be called.
    call(&h, &at, "tools/call", json!({ "name": "probe__grow", "arguments": {} })).await.unwrap();
    let mut names = vec![];
    for _ in 0..50 {
        names = listed(&h, &at).await;
        if names.iter().any(|n| n == "probe__extra") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(names.iter().any(|n| n == "probe__extra"), "{names:?}");
    assert_eq!(count(), 1, "the same process, asked again");

    // A server that exits starts again on the next use, with its list asked afresh.
    call(&h, &at, "tools/call", json!({ "name": "probe__quit", "arguments": {} })).await.unwrap();
    let mut back = Value::Null;
    for _ in 0..50 {
        back = call(&h, &at, "tools/call", json!({ "name": "probe__cwd", "arguments": {} })).await.unwrap();
        if back["isError"] != true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(text(&back), cwd, "{back}");
    assert_eq!(count(), 2, "started again after it exited");
    assert!(!listed(&h, &at).await.iter().any(|n| n == "probe__extra"), "the new process's own list");

    // Another run starts its own process and asks it for its tools: nothing carries over.
    let second = start(&h, json!([task("T2", "m1", json!({}))])).await;
    wait_until(&h.app, &second, "T2 running", |e| task_event(e, "attempt.started", "T2")).await;
    let other = router_url(&h, "T2").await;
    assert_eq!(listed(&h, &other).await.len(), 6);
    assert_eq!(count(), 3, "each run lists from its own process");

    // Deleting the item takes its sandbox along.
    let home = h.app.server_home(&probe.id);
    assert!(home.exists());
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    h.app.run(&second).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    h.app.delete_item(&probe.id).await.unwrap();
    assert!(!home.exists());
    let _ = std::fs::remove_dir_all(&h.dir);
}
