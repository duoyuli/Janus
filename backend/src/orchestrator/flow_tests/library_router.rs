//! The library reaches every digital worker through the MCP router (`janus-tools`): a command
//! server for everyone is not handed to each session but served by the run's router, one process
//! per session, started when the session opens, in the run's working directory and a sandbox of
//! its own, its tool list always current, stopped with everything it spawned when its session ends
//! or the run stops.

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

async fn tool(h: &Harness, at: &(String, String, String), name: &str, args: Value) -> Value {
    call(h, at, "tools/call", json!({ "name": name, "arguments": args })).await.unwrap()
}

async fn listed(h: &Harness, at: &(String, String, String)) -> Vec<String> {
    let r = call(h, at, "tools/list", json!({})).await.unwrap();
    r["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

/// Polls `check` for up to five seconds.
async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..250 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what}");
}

fn process_alive(pid: &str) -> bool {
    std::process::Command::new("kill").args(["-0", pid]).status().is_ok_and(|s| s.success())
}

#[tokio::test(flavor = "multi_thread")]
async fn servers_for_everyone_are_served_by_the_runs_router() {
    let h = harness("library-router", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"] }));
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
    let lead = router_url(&h, "lead").await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(!calls.contains("\"name\":\"probe\""), "the server is not handed to the session itself: {calls}");
    let count = || std::fs::read_to_string(&starts).unwrap_or_default().lines().count();
    eventually("each session's server starts as the session opens", || count() == 2).await;

    let init = call(&h, &at, "initialize", json!({})).await.unwrap();
    assert!(init["instructions"].as_str().unwrap().contains("probe"), "{init}");
    let names = listed(&h, &at).await;
    assert_eq!(
        names,
        ["probe__cwd", "probe__echo", "probe__roots", "probe__env", "probe__grow", "probe__quit", "probe__spawn"]
    );
    assert_eq!(count(), 2, "listing asks the session's running server");

    let cwd = h.app.db.run(&run).unwrap().unwrap()["cwd"].as_str().unwrap().to_string();
    assert_eq!(text(&tool(&h, &at, "probe__cwd", json!({})).await), cwd, "it runs in the run's working directory");
    assert_eq!(
        text(&tool(&h, &at, "probe__roots", json!({})).await),
        format!("file://{cwd}"),
        "the router answers the server's roots/list"
    );
    // Its own sandbox, never Janus's environment: a variable of this process is not there, its own
    // values are, and they can't move HOME.
    let env: Value =
        serde_json::from_str(&text(&tool(&h, &at, "probe__env", json!({ "name": "CARGO_PKG_NAME" })).await)).unwrap();
    assert!(
        std::env::var("CARGO_PKG_NAME").is_ok() && env["CARGO_PKG_NAME"].is_null(),
        "Janus's variables stay out: {env}"
    );
    assert_eq!(env["HOME"], json!(h.app.server_home(&probe.id).join("root")), "a HOME of its own");
    let own: Value = serde_json::from_str(&text(&tool(&h, &at, "probe__env", json!({ "name": "OWN" })).await)).unwrap();
    assert_eq!(own["OWN"], "mine", "its own values are there");
    let missing = tool(&h, &at, "probe__nope", json!({})).await;
    assert!(missing["isError"] == true && text(&missing).contains("没有这个工具"), "{missing}");

    // Sessions don't share a process: T1's server grows a tool, the lead's does not.
    tool(&h, &at, "probe__grow", json!({})).await;
    let mut grown = vec![];
    for _ in 0..50 {
        grown = listed(&h, &at).await;
        if grown.iter().any(|n| n == "probe__extra") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(grown.iter().any(|n| n == "probe__extra"), "asked again after it said its tools changed: {grown:?}");
    assert!(!listed(&h, &lead).await.iter().any(|n| n == "probe__extra"), "the lead's own process");
    assert_eq!(count(), 2, "both still the processes started at first");

    // A server that exits starts again on the next use, with its list asked afresh.
    tool(&h, &at, "probe__quit", json!({})).await;
    let mut back = Value::Null;
    for _ in 0..50 {
        back = tool(&h, &at, "probe__cwd", json!({})).await;
        if back["isError"] != true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(text(&back), cwd, "{back}");
    assert_eq!(count(), 3, "started again after it exited");

    // Editing the item stops its servers; the next use starts it with what was saved.
    let edited = json!({ "name": "probe", "command": "python3", "args": [script],
        "vars": [{ "name": "FAKE_MCP_LOG", "value": "" }, { "name": "OWN", "value": "changed" }] });
    h.app.save_mcp(Some(&probe.id), &edited).await.unwrap();
    let own: Value = serde_json::from_str(&text(&tool(&h, &at, "probe__env", json!({ "name": "OWN" })).await)).unwrap();
    assert_eq!(own["OWN"], "changed", "restarted with the new value");
    assert_eq!(count(), 4);

    // Every call is recorded without its arguments; a collaborator's token can't act as the lead.
    let routed: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind == "mcp.routed").filter_map(|e| e.data).collect();
    assert_eq!(routed[0]["server"], "probe");
    assert!(routed.iter().all(|d| d.get("arguments").is_none() && d.get("args").is_none()), "never the arguments");
    let (r, token, _) = &at;
    assert!(
        h.app.mcp_tools(r, token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );

    // After handing out subtasks the lead only looks: each tool it calls is noted once, for it on
    // the board and as a delivery risk.
    tool(&h, &lead, "probe__echo", json!({ "text": "hi" })).await;
    tool(&h, &lead, "probe__echo", json!({ "text": "again" })).await;
    let noted: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind == "lead.routed").filter_map(|e| e.data).collect();
    assert_eq!(noted.len(), 1, "once per tool: {noted:?}");
    assert_eq!(noted[0]["tool"], "probe__echo");
    let live = h.app.run(&run).unwrap();
    assert!(live.st.lock().unwrap().traces.lead_tools_after_plan().contains("probe__echo"));

    // A session's servers end with the session, with what they spawned; the lead's keep running.
    let child = text(&tool(&h, &at, "probe__spawn", json!({})).await);
    assert!(process_alive(&child));
    let owner = Owner { session: SessionKey::task("T1"), attempt: 1 };
    h.app.close_session(&live, &owner, None, "测试关闭会话").await;
    eventually("T1's server stops with its session", || !process_alive(&child)).await;
    assert_eq!(text(&tool(&h, &lead, "probe__cwd", json!({})).await), cwd, "the lead's server is untouched");

    // Stopping the run stops the rest at once and starts nothing more.
    let child = text(&tool(&h, &lead, "probe__spawn", json!({})).await);
    assert!(process_alive(&child));
    h.app.action(&live, &json!({ "type": "cancel" })).unwrap();
    eventually("the lead's server and its child stop with the run", || !process_alive(&child)).await;
    let after = tool(&h, &lead, "probe__cwd", json!({})).await;
    assert!(after["isError"] == true, "a stopped run starts nothing: {after}");

    // Deleting the item takes its sandbox along.
    let home = h.app.server_home(&probe.id);
    assert!(home.exists());
    h.app.delete_item(&probe.id).await.unwrap();
    assert!(!home.exists());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Names fit what models take after an agent's `mcp__janus-tools__` prefix; a long or cleaned name
/// is cut and ends with a hash of the original, so two tools never share one.
#[test]
fn routed_names_fit_and_stay_apart() {
    use super::super::mcp_router::routed_name;
    assert_eq!(routed_name("github", "create_issue"), "github__create_issue");
    let long = "a".repeat(40);
    let one = routed_name(&long, "list_open_pull_requests_for_repository");
    let two = routed_name(&long, "list_open_pull_requests_for_organization");
    assert!(one.len() <= 46 && two.len() <= 46, "{one} {two}");
    assert_ne!(one, two);
    assert_eq!(one, routed_name(&long, "list_open_pull_requests_for_repository"), "the same every run");
    assert_ne!(routed_name("fs", "read.file"), routed_name("fs", "read_file"), "cleaned names don't collide");
    assert!(routed_name("fs", "read.file").chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
}
