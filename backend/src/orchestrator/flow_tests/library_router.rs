//! The library reaches every digital worker through the MCP router (`janus-tools`): no server is
//! handed to a session; the run's router serves them behind `find_tools` and `call_tool`, and its
//! instructions name each server and its tools. A command server starts on use, shared by the
//! run's sessions, in the run's working directory and a sandbox of its own, found without starting
//! it, stopped with everything it spawned when the run stops. A URL server, Streamable HTTP or SSE,
//! is reached by Janus with the values saved on it.

use super::*;

/// The router's address as a session got it in `session/new` (`mcp [...]` in `calls.log`): the
/// credential a real client holds, never read from the run's state (AGENTS.md rule 1).
async fn router_url(h: &Harness, who: &str) -> (String, String, Seat) {
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..200 {
        let content = std::fs::read_to_string(&calls).unwrap_or_default();
        for line in content.lines().rev().filter_map(|l| l.strip_prefix("mcp ")) {
            let servers: Vec<Value> = serde_json::from_str(line).unwrap_or_default();
            for s in servers.iter().filter(|s| s["name"] == crate::library::MCP_ROUTER) {
                if let Some(at) =
                    mcp_credential(s["url"].as_str().unwrap_or_default()).filter(|(_, _, seat)| seat.who() == who)
                {
                    return at;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no router for {who} in {}", std::fs::read_to_string(&calls).unwrap_or_default());
}

async fn call(h: &Harness, at: &(String, String, Seat), method: &str, params: Value) -> Result<Value> {
    Ok(reply(h, at, json!(1), method, params).await?["result"].clone())
}

/// The whole JSON-RPC reply to request `id`, error and all.
async fn reply(
    h: &Harness,
    (run, token, seat): &(String, String, Seat),
    id: Value,
    method: &str,
    params: Value,
) -> Result<Value> {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    Ok(h.app.mcp_tools(run, token, seat, req).await?.unwrap())
}

/// A name no server has is answered as MCP says for an unknown tool: a protocol error, -32602.
async fn unknown(h: &Harness, at: &(String, String, Seat), name: &str) {
    let r = reply(h, at, json!(1), "tools/call", json!({ "name": name, "arguments": {} })).await.unwrap();
    assert!(r["error"]["code"] == -32602 && r["error"]["message"].as_str().unwrap().contains("没有叫"), "{r}");
}

async fn tool(h: &Harness, at: &(String, String, Seat), name: &str, args: Value) -> Value {
    call(h, at, "tools/call", json!({ "name": name, "arguments": args })).await.unwrap()
}

/// The tools the session is given: the router's own, never the servers'.
async fn listed(h: &Harness, at: &(String, String, Seat)) -> Vec<String> {
    let r = call(h, at, "tools/list", json!({})).await.unwrap();
    r["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()
}

/// Every tool behind the router, as `find_tools` with no query names them.
async fn found(h: &Harness, at: &(String, String, Seat)) -> Vec<String> {
    let said = text(&tool(h, at, "find_tools", json!({})).await);
    said.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| l.split(':').next().unwrap_or_default().to_string())
        .collect()
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
async fn command_servers_are_served_by_the_runs_router() {
    let h = harness("library-router", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"], "T2": ["slow"] }));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let starts = h.dir.join("mcp-starts.log");
    let cancelled = h.dir.join("mcp-cancelled.log");
    let config = json!({ "mcpServers": { "probe": {
        "command": "python3", "args": [script],
        "env": { "FAKE_MCP_LOG": starts, "FAKE_MCP_CANCELLED": cancelled, "OWN": "mine", "HOME": "/etc" }
    } } });
    h.app.import_mcp_config(&config.to_string()).await.unwrap();
    let probe = h.app.items().unwrap().into_iter().find(|i| i.name == "probe").unwrap();

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    let at = router_url(&h, "T1").await;
    let lead = router_url(&h, "lead").await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(!calls.contains("\"name\":\"probe\""), "the server is not handed to the session itself: {calls}");
    let count = || std::fs::read_to_string(&starts).unwrap_or_default().lines().count();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(), 0, "opening a session starts nothing");

    // The instructions name each server, what it says it is for and its tools, and how to reach
    // them; a server that never described itself starts once to say so.
    let init = call(&h, &at, "initialize", json!({ "protocolVersion": "2025-06-18" })).await.unwrap();
    let say = init["instructions"].as_str().unwrap();
    assert!(say.starts_with("Janus 资源库的 MCP 服务器：probe（一个测试用的 MCP 服务器）。"), "{say}");
    assert!(say.contains("- probe：一个测试用的 MCP 服务器。工具：cwd（") && say.contains("find_tools"), "{say}");
    assert_eq!(count(), 1, "it started to say what it is");
    assert_eq!(init["protocolVersion"], "2025-06-18", "a version Janus speaks is agreed to");
    let newer = call(&h, &at, "initialize", json!({ "protocolVersion": "2099-01-01" })).await.unwrap();
    assert_eq!(newer["protocolVersion"], "2025-11-25", "one it doesn't is answered with its newest");
    assert_eq!(listed(&h, &at).await, ["find_tools", "call_tool", "run_code"], "the servers' tools are not listed");
    let names = found(&h, &at).await;
    // The second page (`nextCursor`) is read too.
    let probe_tools = [
        "probe__cwd",
        "probe__echo",
        "probe__roots",
        "probe__env",
        "probe__grow",
        "probe__quit",
        "probe__spawn",
        "probe__hang",
    ];
    assert!(probe_tools.iter().all(|t| names.contains(&t.to_string())), "{names:?}");
    assert!(!names.contains(&"probe__task_only".to_string()), "a task-only tool is left out: {names:?}");
    let tools = call(&h, &at, "tools/list", json!({})).await.unwrap();
    let find = tools["tools"][0]["description"].as_str().unwrap();
    assert!(find.contains("probe") && find.contains("echo"), "find_tools says what there is to find: {find}");
    let echo = text(&tool(&h, &at, "find_tools", json!({ "query": "echo" })).await);
    assert!(echo.contains("\"name\": \"probe__echo\"") && echo.contains("原样返回 text"), "{echo}");
    assert_eq!(found(&h, &lead).await, names, "the lead finds the same tools");
    assert_eq!(count(), 1, "finding again starts nothing: the running one, or the card kept");
    // call_tool runs one by name, its arguments even as a string of JSON.
    let echoed = tool(&h, &at, "call_tool", json!({ "name": "probe__echo", "arguments": "{\"text\": \"via\"}" })).await;
    assert_eq!(text(&echoed), "via");
    let near = tool(&h, &at, "call_tool", json!({ "name": "probe__ecko" })).await;
    assert!(near["isError"] == true && text(&near).contains("probe__echo"), "a near name: {near}");

    let cwd = serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["cwd"].as_str().unwrap().to_string();
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
    assert_eq!(env["HOME"], json!(h.app.cfg.layout.server_home(&probe.id)), "a HOME of its own");
    let own: Value = serde_json::from_str(&text(&tool(&h, &at, "probe__env", json!({ "name": "OWN" })).await)).unwrap();
    assert_eq!(own["OWN"], "mine", "its own values are there");
    unknown(&h, &at, "probe__nope").await;
    unknown(&h, &at, "probe__task_only").await;

    // The agent gives up on a call: the router stops waiting and tells the server, which stops.
    let (app, hung) = (h.app.clone(), at.clone());
    let pending = tokio::spawn(async move {
        let req = json!({ "jsonrpc": "2.0", "id": "c-7", "method": "tools/call",
            "params": { "name": "probe__hang", "arguments": {} } });
        app.mcp_tools(&hung.0, &hung.1, &hung.2, req).await.unwrap().unwrap()
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!pending.is_finished(), "the call waits for the server");
    let cancel = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": "c-7" } });
    assert!(h.app.mcp_tools(&at.0, &at.1, &at.2, cancel).await.unwrap().is_none());
    let ended = tokio::time::timeout(Duration::from_secs(5), pending).await.unwrap().unwrap();
    assert!(ended["result"]["isError"] == true && text(&ended["result"]).contains("取消"), "{ended}");
    eventually("the server is told", || {
        std::fs::read_to_string(&cancelled).unwrap_or_default().contains("cancelled cancelled by the client")
    })
    .await;

    // The run's sessions share the server: the lead's call goes to the process T1 started.
    assert_eq!(text(&tool(&h, &lead, "probe__cwd", json!({})).await), cwd);
    assert_eq!(count(), 1, "one process for the run");
    // A server that says its tools changed is asked again, for everyone sharing it.
    tool(&h, &at, "probe__grow", json!({})).await;
    eventually_async(|| async { found(&h, &lead).await.iter().any(|n| n == "probe__extra") }).await;

    // A server that exits starts again on the next use.
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
    assert_eq!(count(), 2, "started again after it exited");

    // Editing the item stops its servers; the next use starts it with what was saved.
    let edited = json!({ "name": "probe", "command": "python3", "args": [script],
        "vars": [{ "name": "FAKE_MCP_LOG" }, { "name": "OWN", "value": "changed" }] });
    h.app.save_mcp(Some(&probe.id), &edited).await.unwrap();
    let probe = h.app.item(&probe.id).unwrap();
    let own: Value = serde_json::from_str(&text(&tool(&h, &at, "probe__env", json!({ "name": "OWN" })).await)).unwrap();
    assert_eq!(own["OWN"], "changed", "restarted with the new value");

    // Every call is recorded without its arguments; a collaborator's token can't act as the lead.
    let routed: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "mcp.routed").filter_map(|e| e.data).collect();
    assert_eq!(routed[0]["server"], "probe");
    assert!(routed.iter().all(|d| d.get("arguments").is_none() && d.get("args").is_none()), "never the arguments");
    let (r, token, _) = &at;
    assert!(
        h.app
            .mcp_tools(r, token, &Seat::Lead, json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .await
            .is_err()
    );

    // After handing out subtasks the lead only looks: each tool it calls is noted once, for it on
    // the board and as a delivery risk.
    tool(&h, &lead, "probe__echo", json!({ "text": "hi" })).await;
    tool(&h, &lead, "probe__echo", json!({ "text": "again" })).await;
    let noted: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "lead.routed").filter_map(|e| e.data).collect();
    assert_eq!(noted.iter().filter(|d| d["tool"] == "probe__echo").count(), 1, "once per tool: {noted:?}");
    let live = h.app.run(&run).unwrap();
    assert!(live.st.lock().unwrap().traces.lead_tools_after_plan().contains("probe__echo"));

    // A session that closes leaves the run's server, and what it spawned, to the run.
    let shared_child = text(&tool(&h, &at, "probe__spawn", json!({})).await);
    let owner = Owner { session: SessionKey::task("T1"), attempt: 1 };
    h.app.close_session(&live, &owner, None, "测试关闭会话").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(process_alive(&shared_child), "the run's server keeps going");

    // Another run finds from the card the server last gave, starting nothing.
    assert!(h.app.cfg.layout.home().join("cache/mcp-tools").read_dir().unwrap().count() >= 1, "cards are written down");
    let second = start(&h, json!([task("T2", "m1", json!({}))])).await;
    wait_until(&h.app, &second, "T2 running", |e| task_event(e, "attempt.started", "T2")).await;
    let other = router_url(&h, "T2").await;
    let b = count();
    assert!(found(&h, &other).await.iter().any(|n| n == "probe__cwd"));
    assert_eq!(count(), b, "found from what it gave last time");

    // Stopping the run stops the rest at once, shared ones too, and starts nothing more.
    h.app.action(&live, &action_of(json!({ "type": "cancel" }))).unwrap();
    eventually("the shared server and its child stop with the run", || !process_alive(&shared_child)).await;
    let after = tool(&h, &lead, "probe__cwd", json!({})).await;
    assert!(after["isError"] == true, "a stopped run starts nothing: {after}");
    let second_run = h.app.run(&second).unwrap();
    h.app.action(&second_run, &action_of(json!({ "type": "cancel" }))).unwrap();

    // Deleting the item takes its sandbox and remembered lists along.
    let owned = h.app.cfg.layout.item_paths(&probe.id);
    assert!(h.app.cfg.layout.server_home(&probe.id).exists() && h.app.cfg.layout.mcp_tools(&probe.id).exists());
    h.app.delete_item(&probe.id).await.unwrap();
    assert!(owned.iter().all(|p| !p.exists()), "everything it owned goes: {owned:?}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The scripted HTTP MCP server (`testdata/fake_mcp_http.py`) on a free port, stopped when this
/// is dropped.
struct HttpServer {
    child: std::process::Child,
    base: String,
}

impl HttpServer {
    fn start(log: &Path) -> HttpServer {
        use std::io::BufRead;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp_http.py");
        let mut child = std::process::Command::new("python3")
            .arg(script)
            .arg(log)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut port = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut port).unwrap();
        HttpServer { child, base: format!("http://127.0.0.1:{}", port.trim()) }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// A URL server is never handed to the session: Janus reaches it, Streamable HTTP or SSE, with
/// the header saved on it, which the agent never sees. A wrong value is refused by the server and
/// the call says so. A session the server forgot is started again on a later call, and stopping
/// the run ends the session on the server.
#[tokio::test(flavor = "multi_thread")]
async fn url_servers_are_reached_by_the_router_with_their_values() {
    let h = harness("library-router-http", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"] }));
    let log = h.dir.join("http-mcp.log");
    let server = HttpServer::start(&log);
    let lines = || std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>();
    let auth = |v: &str| json!([{ "name": "Authorization", "value": v }]);
    for (name, transport, path, value) in [
        ("remote", "http", "/mcp", "Bearer good"),
        ("legacy", "sse", "/sse", "Bearer good"),
        ("denied", "http", "/mcp", "Bearer bad"),
    ] {
        let body = json!({ "name": name, "transport": transport, "url": format!("{}{path}", server.base), "vars": auth(value) });
        h.app.save_mcp(None, &body).await.unwrap();
    }

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    let at = router_url(&h, "T1").await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(!calls.contains("Bearer") && !calls.contains(&server.base), "the agent gets no URL server: {calls}");

    let names = found(&h, &at).await;
    for t in ["remote__whoami", "remote__expire", "legacy__whoami"] {
        assert!(names.contains(&t.to_string()), "{t} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.starts_with("denied__")), "a server that refuses lists nothing: {names:?}");

    let seen: Value = serde_json::from_str(&text(&tool(&h, &at, "remote__whoami", json!({})).await)).unwrap();
    assert_eq!(seen, json!({ "authorization": "Bearer good", "version": "2025-06-18" }), "over Streamable HTTP");
    let seen: Value = serde_json::from_str(&text(&tool(&h, &at, "legacy__whoami", json!({})).await)).unwrap();
    assert_eq!(seen["authorization"], "Bearer good", "over SSE");
    unknown(&h, &at, "denied__whoami").await;
    assert_eq!(lines().iter().filter(|l| *l == "init").count(), 1, "one Streamable HTTP session for the run");

    // The server forgets the session: the call that finds out fails, the next one starts again.
    tool(&h, &at, "remote__expire", json!({})).await;
    let mut back = Value::Null;
    for _ in 0..3 {
        back = tool(&h, &at, "remote__whoami", json!({})).await;
        if back["isError"] != true {
            break;
        }
    }
    assert!(text(&back).contains("Bearer good"), "{back}");
    assert_eq!(lines().iter().filter(|l| *l == "init").count(), 2);

    let live = h.app.run(&run).unwrap();
    h.app.action(&live, &action_of(json!({ "type": "cancel" }))).unwrap();
    eventually("stopping the run ends the server's session", || lines().contains(&"delete".to_string())).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Code mode: `run_code` runs a script against the routed tools, each call recorded as the
/// router's; a script that ran is saved as a skill with code, which the next run calls on
/// `skills`. Turned off in the library, the router gives the two other tools only.
#[tokio::test(flavor = "multi_thread")]
async fn code_mode_runs_scripts_and_keeps_them_as_skills() {
    let h = harness("library-router-code", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"], "T2": ["slow"] }));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let config = json!({ "mcpServers": { "probe": { "command": "python3", "args": [script] } } });
    h.app.import_mcp_config(&config.to_string()).await.unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    let at = router_url(&h, "T1").await;
    let init = call(&h, &at, "initialize", json!({})).await.unwrap();
    assert!(init["instructions"].as_str().unwrap().contains("run_code"), "{init}");

    let code = r#"
        const [a, b] = await Promise.all([tools.probe__echo({ text: args.first }), tools["probe__echo"]({ text: "two" })]);
        console.log("got", a);
        return `${a}+${b}`;
    "#;
    let ran = tool(
        &h,
        &at,
        "run_code",
        json!({ "code": code, "args": { "first": "one" },
        "save_as": { "name": "echo-twice", "description": "Echo two texts; args.first is the first" } }),
    )
    .await;
    assert_ne!(ran["isError"], true, "{ran}");
    let said: Vec<&str> = ran["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect();
    assert_eq!(said[0], "got one\none+two", "the logs, then what it returned: {ran}");
    assert!(said.last().unwrap().contains("echo-twice"), "saved: {ran}");
    let routed = events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "mcp.routed").count();
    assert_eq!(routed, 2, "each call the script made is recorded");
    let missing = tool(&h, &at, "run_code", json!({ "code": "return await tools.probe__ecko({})" })).await;
    assert!(missing["isError"] == true && text(&missing).contains("probe__echo"), "{missing}");
    // What is not saved: a script that failed, a name the library has, code past the limit.
    let save = |name: &str| json!({ "name": name, "description": "d" });
    let said =
        |r: &Value| -> String { r["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect() };
    let failed = tool(&h, &at, "run_code", json!({ "code": "throw new Error('no')", "save_as": save("broken") })).await;
    assert!(said(&failed).contains("没有存成 Skill：代码没有跑通"), "{failed}");
    let taken = tool(&h, &at, "run_code", json!({ "code": "return 1", "save_as": save("echo-twice") })).await;
    assert!(said(&taken).contains("没有存成 Skill") && said(&taken).contains("echo-twice"), "{taken}");
    let big = format!("return {:?}", "x".repeat(300 * 1024));
    let large = tool(&h, &at, "run_code", json!({ "code": big, "save_as": save("too-big") })).await;
    assert!(
        said(&large).ends_with("没有存成 Skill：代码超过 256 KB，存不成 Skill"),
        "{}",
        said(&large).chars().rev().take(80).collect::<String>()
    );
    let names: Vec<String> = h.app.items().unwrap().into_iter().map(|i| i.name).collect();
    assert!(!names.iter().any(|n| n == "broken" || n == "too-big"), "{names:?}");

    // The skill is the library's, with its code, for every digital employee from the next run.
    let saved = h.app.items().unwrap().into_iter().find(|i| i.name == "echo-twice").unwrap();
    assert!(h.app.skill_code(&saved).unwrap().contains("export default async (args)"));
    let second = start(&h, json!([task("T2", "m1", json!({}))])).await;
    wait_until(&h.app, &second, "T2 running", |e| task_event(e, "attempt.started", "T2")).await;
    let other = router_url(&h, "T2").await;
    let tools = call(&h, &other, "tools/list", json!({})).await.unwrap();
    assert!(tools["tools"][2]["description"].as_str().unwrap().contains("skills.echo_twice"), "{tools}");
    let again = tool(&h, &other, "run_code", json!({ "code": "return await skills.echo_twice({ first: 'x' })" })).await;
    assert_eq!(text(&again), "got x\nx+two", "the skill logs as the script did: {again}");

    // Code mode off: the two other tools only, and the instructions say nothing of it.
    h.app.save_code_mode(&json!({ "code": false })).unwrap();
    assert_eq!(listed(&h, &other).await, ["find_tools", "call_tool"]);
    let init = call(&h, &other, "initialize", json!({})).await.unwrap();
    assert!(!init["instructions"].as_str().unwrap().contains("run_code"));
    assert_eq!(h.app.library_view().unwrap()["mcpRouter"]["code"], false);
    for r in [&run, &second] {
        h.app.action(&h.app.run(r).unwrap(), &action_of(json!({ "type": "cancel" }))).unwrap();
    }
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Polls an async `check` for up to five seconds.
async fn eventually_async<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("not within five seconds");
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
    // A plain name splits at its first `__`: a server name that would blur the split is hashed.
    assert_ne!(routed_name("a_", "b"), routed_name("a", "_b"));
    assert_ne!(routed_name("a__b", "c"), routed_name("a", "b__c"));
    assert_eq!(routed_name("a", "_b"), "a___b", "a plain one still reads back as one pair");
    assert!(routed_name("fs", "read.file").chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
}

/// A server is prepared once when it is added: a command started in its sandbox (where a package
/// download stays for later runs), a URL reached; its tools counted, then let go. One that can't
/// start, or answers in a protocol version Janus doesn't speak, says why.
#[tokio::test(flavor = "multi_thread")]
async fn servers_are_prepared_when_added() {
    let dir = std::env::temp_dir().join(format!("janus-warm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = crate::orchestrator::test_app(&dir);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let starts = dir.join("starts.log");
    let probe = app
        .save_mcp(None, &json!({ "name": "probe", "command": "python3", "args": [script], "vars": [{ "name": "FAKE_MCP_LOG", "value": starts }] }))
        .await
        .unwrap();
    let broken = app.save_mcp(None, &json!({ "name": "broken", "command": "janus-no-such-command" })).await.unwrap();
    let future = app
        .save_mcp(None, &json!({ "name": "future", "command": "python3", "args": [script], "vars": [{ "name": "FAKE_MCP_VERSION", "value": "2099-01-01" }] }))
        .await
        .unwrap();
    let warm = |id: &str| {
        let view = app.library_view().unwrap();
        view["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["warm"].clone()
    };
    let log = dir.join("http-mcp.log");
    let server = HttpServer::start(&log);
    let remote = app
        .save_mcp(
            None,
            &json!({ "name": "remote", "transport": "http", "url": format!("{}/mcp", server.base),
            "vars": [{ "name": "Authorization", "value": "Bearer good" }] }),
        )
        .await
        .unwrap();
    app.warm_mcp(&remote.id);
    eventually("the URL server is prepared", || warm(&remote.id)["state"] == "ready").await;
    assert_eq!(warm(&remote.id)["tools"], 2);
    assert!(warm(&probe.id).is_null(), "nothing to say before it is prepared");
    app.warm_mcp(&probe.id);
    app.warm_mcp(&broken.id);
    eventually("the server is prepared", || warm(&probe.id)["state"] == "ready").await;
    assert_eq!(warm(&probe.id)["tools"], 8, "both pages, without the task-only tool");
    assert_eq!(std::fs::read_to_string(&starts).unwrap().lines().count(), 1, "started once");
    assert!(app.cfg.layout.server_home(&probe.id).is_dir(), "in its own sandbox, which runs reuse");
    eventually("the broken one says why", || warm(&broken.id)["state"] == "failed").await;
    assert!(warm(&broken.id)["error"].as_str().unwrap().contains("janus-no-such-command"), "{}", warm(&broken.id));
    // A server that only speaks a protocol version Janus doesn't is not used, and says which.
    app.warm_mcp(&future.id);
    eventually("the one on another version says why", || warm(&future.id)["state"] == "failed").await;
    assert!(warm(&future.id)["error"].as_str().unwrap().contains("2099-01-01"), "{}", warm(&future.id));

    // A change makes the old result stale: nothing is claimed for the new revision until prepared.
    app.save_mcp(Some(&probe.id), &json!({ "name": "probe", "command": "python3", "args": [script, "--changed"] }))
        .await
        .unwrap();
    assert!(warm(&probe.id).is_null());
    let _ = std::fs::remove_dir_all(&dir);
}

/// For a run already at work, the library decides how its servers run and whether they still
/// exist: a server deleted from the library is no longer listed or started (FL-07).
#[tokio::test(flavor = "multi_thread")]
async fn a_server_deleted_from_the_library_is_gone_from_running_runs() {
    let h = harness("library-router-deleted", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"] }));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
    let config = json!({ "mcpServers": { "probe": { "command": "python3", "args": [script] } } });
    h.app.import_mcp_config(&config.to_string()).await.unwrap();
    let probe = h.app.items().unwrap().into_iter().find(|i| i.name == "probe").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let lead = router_url(&h, "lead").await;
    assert!(found(&h, &lead).await.iter().any(|t| t.starts_with("probe__")));
    assert_eq!(text(&tool(&h, &lead, "probe__echo", json!({ "text": "hi" })).await), "hi");

    h.app.delete_item(&probe.id).await.unwrap();
    assert!(!found(&h, &lead).await.iter().any(|t| t.starts_with("probe__")), "no longer found");
    unknown(&h, &lead, "probe__echo").await;
    let r = h.app.run(&run).unwrap();
    h.app.action(&r, &action_of(json!({ "type": "cancel" }))).unwrap();
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// End to end through a run: the library holds servers pasted in and added by hand, of every
/// transport, and a skill, all for every digital worker. The agent's `session/new` carries none of them: only the run's MCP
/// router and the board, so the session's tool list does not grow with the library, and no value
/// reaches the agent. The run record and its events hold none of the values either. Skills reach
/// Claude Code as a plugin with the one `janus-skills`.
#[tokio::test(flavor = "multi_thread")]
async fn library_resources_reach_the_session_only_through_the_routers() {
    let h = harness("mcp-session", &[("lead", false)], json!({}));
    let pasted = r#"{"mcpServers": {
        "fetch": {"command": "uvx", "args": ["mcp-server-fetch"], "env": {"FETCH_TOKEN": "lib-env-secret"}},
        "docs": {"type": "http", "url": "https://example.com/mcp", "headers": {"Authorization": "Bearer lib-header-secret"}}
    }}"#;
    h.app.import_mcp_config(pasted).await.unwrap();
    let local = h
        .app
        .save_mcp(None, &json!({ "name": "local", "command": "node local.js", "vars": [{ "name": "LOCAL_TOKEN", "value": "own-env-secret" }] }))
        .await
        .unwrap();
    h.app.create_skill(&json!({ "name": "notes", "description": "记笔记", "content": "写下来。" })).await.unwrap();

    let run = h
        .app
        .create_run(NewRun { goal: "带上 MCP".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    let mut line = String::new();
    for _ in 0..100 {
        if let Some(l) = std::fs::read_to_string(&calls).unwrap_or_default().lines().find(|l| l.starts_with("mcp ")) {
            line = l.to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let servers: Vec<Value> =
        serde_json::from_str(line.strip_prefix("mcp ").expect("the lead opened a session")).unwrap();
    let names: Vec<&str> = servers.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, [crate::library::MCP_ROUTER, "janus"], "only the router and the board: {servers:?}");
    for secret in ["own-env-secret", "lib-env-secret", "lib-header-secret"] {
        assert!(!line.contains(secret), "no value reaches the agent: {secret}");
    }

    wait_until(&h.app, &run, "the lead's session record", |e| has(e, "agent.status")).await;
    let evs = events(&h.app, &run);
    let status = evs.iter().find(|e| e.kind.as_str() == "agent.status").unwrap().data.clone().unwrap();
    let via: Vec<(String, String)> = status["config"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["via"].as_str().unwrap().to_string()))
        .collect();
    assert!(via.contains(&("notes".into(), "skillRouter".into())), "{via:?}");
    for server in ["fetch", "docs", "local"] {
        assert!(via.contains(&(server.into(), "mcpRouter".into())), "{server}: {via:?}");
    }
    let bundle = h.app.cfg.layout.run(RunKind::Goal, &run).bundle("lead");
    assert!(bundle.join("skills/janus-skills/.skills/notes/SKILL.md").is_file());
    assert!(!bundle.join("skills/notes").exists(), "the skill is not loaded on its own");
    let record = serde_json::to_string(&serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()).unwrap()
        + &serde_json::to_string(&evs).unwrap();
    for secret in ["own-env-secret", "lib-env-secret", "lib-header-secret"] {
        assert!(!record.contains(secret), "the run record and its events hold no values: {secret}");
    }
    assert!(record.contains(&local.id), "the items the member uses are pinned in the run");
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    let _ = std::fs::remove_dir_all(&h.dir);
}
