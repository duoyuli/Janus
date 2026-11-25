//! End-to-end runs against a scripted ACP agent (testdata/fake_acp.py): each test drives one of
//! the paths that used to leave a run stuck, and checks it either delivers or waits somewhere
//! the person can act.

use super::*;

// The flows of each area live in their own files under `orchestrator/flow_tests/`; this file keeps
// the harness and the task lifecycle.
mod actions;
mod board_rules;
mod chats;
mod collaboration;
mod delivery;
mod evidence;
mod lead;
mod library_router;
mod loops;
mod memory;
mod messages;
mod owners;
mod queue;
mod seats;
mod skill_router;
mod teamwork;
mod usage;
mod worktrees;

struct Harness {
    app: Arc<App>,
    dir: PathBuf,
    repo: PathBuf,
}

/// The branch checked out in `repo`.
/// A request to act on a run, as the HTTP API reads it from its JSON body.
fn req(body: Value) -> ActionRequest {
    serde_json::from_value(body).expect("a valid action request")
}

/// An action on a live run, as `req` reads it.
fn action_of(body: Value) -> super::actions::RunAction {
    serde_json::from_value(body).expect("a valid action")
}

fn git_branch(repo: &Path) -> String {
    let out = std::process::Command::new("git").args(["branch", "--show-current"]).current_dir(repo).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sh(cwd: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// `members`: (role id, writable). The first one leads.
// @project-doc docs/operations/20261031-testing.md#harness
fn harness(name: &str, members: &[(&str, bool)], script: Value) -> Harness {
    harness_with(name, members, script, |_| {})
}

/// An app over `dir` as a real Janus runs one: its routes served on a free local port, which the
/// MCP URLs it hands to agents point at, so the scripted agent calls the janus tools over HTTP as
/// a real one does. `set` changes some of its settings. Restarts in the tests make a new one.
pub(super) fn served_app(dir: &Path, set: impl FnOnce(&mut Config)) -> Arc<App> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let app = test_app_with(dir, |cfg| {
        cfg.addr = addr;
        set(cfg)
    });
    let served = app.clone();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let _ = axum::serve(listener, crate::http::router(served)).await;
    });
    app
}

/// `harness` with some of the app's settings changed by `set`.
fn harness_with(name: &str, members: &[(&str, bool)], script: Value, set: impl FnOnce(&mut Config)) -> Harness {
    let dir = std::env::temp_dir().join(format!("janus-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (repo, fake) = (dir.join("repo"), dir.join("fake"));
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(fake.join("script.json"), script.to_string()).unwrap();
    sh(&repo, &["init", "-q"]);
    // Accepting a run commits as the repository's own identity.
    sh(&repo, &["config", "user.name", "t"]);
    sh(&repo, &["config", "user.email", "t@t"]);
    std::fs::write(repo.join("README.md"), "test\n").unwrap();
    sh(&repo, &["add", "-A"]);
    sh(&repo, &["commit", "-q", "-m", "init"]);
    let app = served_app(&dir, set);
    // What a check of the fake agent reads: whether a member writes follows from its mode.
    let modes: Vec<Value> =
        ["read-only", "ask", "agent", "full"].iter().map(|v| json!({ "value": v, "name": v })).collect();
    // The scripted agent, through the stand-in Claude Code adapter (`stand_in_adapters`).
    app.db.put_config(ConfigKind::Connection, "fake", &crate::agents::test_connection("fake", "claude", "claude", json!({
        "name": "Fake",
        "env": { "FAKE_DIR": fake.to_string_lossy() },
        "options": { "controls": [{ "id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "config", "current": "ask", "permission": true, "options": modes }] }
    })), now_ms()).unwrap();
    for (i, (id, writable)) in members.iter().enumerate() {
        app.db
            .put_config(
                ConfigKind::Role,
                id,
                &crate::agents::test_role(
                    id,
                    json!({
                        "connection": "fake",
                        "lead": i == 0,
                        "session": if *writable { json!({}) } else { json!({ "mode": "read-only" }) },
                    }),
                ),
                now_ms(),
            )
            .unwrap();
    }
    let ms: Vec<Value> = members.iter().map(|(id, _)| json!({ "role": id })).collect();
    app.db
        .put_config(
            ConfigKind::Workspace,
            "ws",
            &json!({
                "id": "ws", "name": "ws", "path": repo.canonicalize().unwrap(), "lead": members[0].0, "members": ms, "brief": ""
            }),
            now_ms(),
        )
        .unwrap();
    Harness { app, dir, repo }
}

fn events(app: &App, run: &str) -> Vec<Event> {
    app.db.events(run, 0).unwrap()
}

async fn wait_until(app: &App, run: &str, what: &str, f: impl Fn(&[Event]) -> bool) {
    for _ in 0..600 {
        if f(&events(app, run)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let tail: Vec<String> = events(app, run)
        .iter()
        .rev()
        .take(25)
        .rev()
        .map(|e| format!("#{} {} {:?} {}", e.seq, e.kind, e.task, e.text))
        .collect();
    panic!("timed out waiting for {what}:\n{}", tail.join("\n"));
}

fn has(evs: &[Event], kind: &str) -> bool {
    evs.iter().any(|e| e.kind == kind)
}

fn status(app: &App, run: &str) -> String {
    serde_json::to_value(app.db.run(run).unwrap().unwrap()).unwrap()["status"].as_str().unwrap().to_string()
}

/// Start a run and play its lead: submit `tasks` as one orchestration through the board.
/// The scripted lead keeps working until `finish_lead`.
async fn start(h: &Harness, tasks: Value) -> String {
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": tasks })).await;
    assert!(!err, "{text}");
    run
}

fn finish_lead(h: &Harness) {
    std::fs::write(h.dir.join("fake/lead-go"), "目标已完成").unwrap();
}

fn act(h: &Harness, run: &str, a: Value) {
    let r = h.app.run(run).unwrap();
    h.app.action(&r, &action_of(a)).unwrap();
}

fn task(id: &str, assignee: &str, extra: Value) -> Value {
    let mut t = json!({ "id": id, "title": id, "instructions": id, "assignee": assignee, "accept": [format!("{id} 的结果写清楚")] });
    for (k, v) in extra.as_object().unwrap() {
        t[k] = v.clone();
    }
    t
}

/// A role's session controls reach the agent (seeded at session/new and set per session), a
/// plan mode never runs, and the agent's own permission options and forms come back to the
/// person and are answered in the agent's terms.
#[tokio::test(flavor = "multi_thread")]
async fn agent_requests_reach_the_person() {
    let h = harness("requests", &[("lead", false), ("m1", false)], json!({ "T1": ["permission"], "T2": ["elicit"] }));
    let mut conn = h.app.db.get_config(ConfigKind::Connection, "fake").unwrap().unwrap();
    conn["options"]["controls"].as_array_mut().unwrap().push(json!({ "id": "plan_mode", "name": "Plan", "type": "boolean", "via": "config", "current": false, "permission": true, "options": [] }));
    h.app.db.put_config(ConfigKind::Connection, "fake", &conn, now_ms()).unwrap();
    // The agent starts in Plan mode and the role asks for it: Janus switches it off.
    std::fs::write(h.dir.join("fake/plan-on"), "").unwrap();
    let mut role = h.app.db.get_config(ConfigKind::Role, "m1").unwrap().unwrap();
    role["session"] = json!({ "mode": "full", "plan_mode": true });
    h.app.db.put_config(ConfigKind::Role, "m1", &role, now_ms()).unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;

    wait_until(&h.app, &run, "permission request", |e| has(e, "approval.requested")).await;
    let req = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "approval.requested").unwrap();
    assert_eq!(
        req.data.as_ref().unwrap()["options"].as_array().unwrap().len(),
        3,
        "every option the agent offered is shown"
    );
    act(
        &h,
        &run,
        json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T1", "allow": true, "option": "a2", "label": "Always allow" }),
    );

    wait_until(&h.app, &run, "form request", |e| has(e, "input.requested")).await;
    let form = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "input.requested").unwrap();
    assert_eq!(form.data.as_ref().unwrap()["schema"]["required"], json!(["name"]));
    act(
        &h,
        &run,
        json!({ "type": "input", "key": form.data.as_ref().unwrap()["key"], "task": "T2", "action": "accept", "content": { "name": "janus" } }),
    );

    wait_until(&h.app, &run, "T2", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T2"))
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let summary = |t: &str| {
        evs.iter()
            .find(|e| e.kind.as_str() == "result.submitted" && e.task.as_deref() == Some(t))
            .map(|e| e.data.as_ref().unwrap()["summary"].as_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    };
    assert_eq!(summary("T1"), "picked a2", "the chosen option, not a generic allow");
    assert_eq!(summary("T2"), "accept janus");
    let started = evs.iter().find(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("T1")).unwrap();
    assert_eq!(started.data.as_ref().unwrap()["config"]["controls"]["mode"], "full");
    assert!(
        started.data.as_ref().unwrap()["config"]["controls"].get("plan_mode").is_none(),
        "plan mode is not a role choice"
    );
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(calls.contains("set mode \"full\"") && calls.contains("set plan_mode false"), "{calls}");
    assert!(!calls.contains("set plan_mode true"), "{calls}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

fn task_event(evs: &[Event], kind: &str, task: &str) -> bool {
    evs.iter().any(|e| e.kind == kind && e.task.as_deref() == Some(task))
}

/// The watchdog tells the lead about a silent attempt (once per quiet spell) and one repeating
/// the same tool call, and leaves alone an attempt that waits on the person.
#[tokio::test(flavor = "multi_thread")]
async fn watchdog_reports_stalled_tasks() {
    let h = harness_with(
        "watchdog",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["slow"], "T2": ["loop"], "T3": ["permission"] }),
        |c| c.stall_after = Duration::from_secs(1),
    );
    let run = start(
        &h,
        json!([task("T1", "m1", json!({})), task("T2", "m2", json!({})), task("T3", "m2", json!({ "deps": ["T2"] }))]),
    )
    .await;
    let stalled = |e: &[Event], t: &str, reason: &str| {
        e.iter()
            .filter(|e| {
                e.kind.as_str() == "task.stalled"
                    && e.task.as_deref() == Some(t)
                    && e.data.as_ref().unwrap()["reason"] == reason
            })
            .count()
    };
    wait_until(&h.app, &run, "both stalls", |e| stalled(e, "T1", "idle") > 0 && stalled(e, "T2", "loop") > 0).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[停滞] T1") && board.contains("没有输出或工具调用"), "{board}");
    assert!(board.contains("[停滞] T2") && board.contains("连续 5 次") && board.contains("cat missing.txt"), "{board}");

    // T3 waits on the person's approval: that silence is not the agent's.
    wait_until(&h.app, &run, "T3 asks", |e| has(e, "approval.requested")).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let evs = events(&h.app, &run);
    assert_eq!(stalled(&evs, "T1", "idle"), 1, "one report per quiet spell");
    assert_eq!(stalled(&evs, "T3", "idle"), 0, "waiting on the person is not a stall");
    let req = evs.iter().find(|e| e.kind.as_str() == "approval.requested").unwrap();
    act(&h, &run, json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T3", "allow": true }));

    // The person interrupts T1 and holds its session (no `control.taken`): the silence is theirs.
    act(&h, &run, json!({ "type": "interrupt", "task": "T1" }));
    wait_until(&h.app, &run, "T1 held", |e| task_event(e, "session.interrupted", "T1")).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(stalled(&events(&h.app, &run), "T1", "idle"), 1, "a session the person holds is not stalled");
    let (text, err) = tool(&h.app, &run, "lead", "cancel", json!({ "tasks": ["T1"], "reason": "卡住了" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T3", |e| task_event(e, "task.succeeded", "T3")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A failed verification goes to the lead instead of a fresh retry; the lead accepts, retries,
/// cancels or hands a decision to the person, and blocked tasks follow without re-orchestration.
#[tokio::test(flavor = "multi_thread")]
async fn lead_resolves_failed_tasks() {
    let h = harness(
        "resolve",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["unmet"], "T2": ["ok"], "T3": ["slow", "ok"], "T4": ["ok"] }),
    );
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Every task needs acceptance criteria: the executor verifies against them.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [{ "id": "T1", "title": "T1", "instructions": "T1", "assignee": "m1" }] }),
    )
    .await;
    assert!(err && text.contains("缺少 accept"), "{text}");

    // T1's executor cannot meet its criterion, even after the fix round; T2 waits on T1.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        task("T1", "m1", json!({ "accept": ["页面能打开"] })), task("T2", "m2", json!({ "deps": ["T1"] }))] }),
    )
    .await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1 failed", |e| task_event(e, "task.failed", "T1")).await;
    let evs = events(&h.app, &run);
    assert_eq!(
        evs.iter().filter(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("T1")).count(),
        1,
        "no fresh retry for a failed verification"
    );
    assert!(!task_event(&evs, "task.retry_scheduled", "T1"));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[没通过验收] T1") && board.contains("\n  - 自认没有达到「页面能打开」"), "{board}");
    // A finished task does not take messages: they would land in some later session.
    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T1", "text": "继续" })).await;
    assert!(err && text.contains("已经结束"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "accept", json!({ "task": "T1", "reason": "我打开过页面，可以用" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T2 after accept", |e| task_event(e, "task.succeeded", "T2")).await;

    // Cancelling a running task stops it and drops what depends on it; retry brings them back.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("T3", "m1", json!({})), task("T4", "m2", json!({ "deps": ["T3"] }))] }),
    )
    .await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T3 running", |e| task_event(e, "attempt.started", "T3")).await;
    let (text, err) = tool(&h.app, &run, "lead", "cancel", json!({ "tasks": ["T3"], "reason": "换个做法" })).await;
    assert!(!err && text.contains("T3") && text.contains("T4"), "{text}");
    wait_until(&h.app, &run, "T4 dropped", |e| task_event(e, "task.skipped_by_decision", "T4")).await;
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "retry",
        json!({ "task": "T3", "session": "new", "instructions": "换个做法再做一次", "accept": ["换了做法"] }),
    )
    .await;
    assert!(!err && text.contains("因它被取消的 T4 也恢复了"), "{text}");
    // T4 was dropped only because of T3, so it came back with it.
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T4", "session": "new" })).await;
    assert!(err && text.contains("等待开始"), "{text}");
    wait_until(&h.app, &run, "T3 and T4 after retry", |e| {
        task_event(e, "task.succeeded", "T3") && task_event(e, "task.succeeded", "T4")
    })
    .await;
    let reopened = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind.as_str() == "task.reopened" && e.task.as_deref() == Some("T3"))
        .unwrap();
    assert_eq!(reopened.data.as_ref().unwrap()["instructions"], "换个做法再做一次");
    assert_eq!(reopened.data.as_ref().unwrap()["accept"], json!(["换了做法"]));

    // A decision only the person can make: the answer reaches the lead through wait.
    let (text, err) = tool(&h.app, &run, "lead", "ask_user", json!({ "question": "部署到哪个环境？", "why": "需要用户的账号", "options": ["测试", "生产"], "recommend": "测试" })).await;
    assert!(!err, "{text}");
    let asked = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "question.escalated").unwrap();
    let key = asked.data.as_ref().unwrap()["key"].as_str().unwrap().to_string();
    act(&h, &run, json!({ "type": "answer", "key": key, "task": "", "answer": "测试" }));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[用户决定]") && board.contains("测试"), "{board}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A result handed in with a criterion unmet is taken, and the agent hears once that it may fix it
/// and hand in again in the same turn. One that does gets through; one that does not goes to the
/// lead, still within one attempt and without another round.
#[tokio::test(flavor = "multi_thread")]
async fn an_unmet_criterion_may_be_handed_in_once_more() {
    let h = harness(
        "fix",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["unmet", "ok"], "T2": ["unmet"] }),
    );
    let run = start(
        &h,
        json!([task("T1", "m1", json!({ "accept": ["做好 done 功能", "写清用法"] })), task("T2", "m2", json!({}))]),
    )
    .await;
    wait_until(&h.app, &run, "T1 done and T2 failed", |e| {
        task_event(e, "task.succeeded", "T1") && task_event(e, "task.failed", "T2")
    })
    .await;
    let evs = events(&h.app, &run);
    let count = |kind: &str, t: &str| evs.iter().filter(|e| e.kind == kind && e.task.as_deref() == Some(t)).count();
    let submits = |t: &str| -> Vec<String> {
        evs.iter()
            .filter(|e| e.kind.as_str() == "tool.completed" && e.task.as_deref() == Some(t))
            .filter_map(|e| e.data.as_ref())
            .filter(|d| d["tool"] == "janus.submit")
            .map(|d| d["output"].as_str().unwrap_or("").to_string())
            .collect()
    };
    assert_eq!(count("attempt.started", "T1"), 1, "fixed in the same session, no new attempt");
    let t1 = submits("T1");
    assert!(
        t1.len() == 2 && t1[0].contains("已收下，但这些标准你写了没达到") && t1[0].contains("- 做好 done 功能"),
        "{t1:?}"
    );
    assert!(t1[1].starts_with("已收下。"), "the second hand-in is simply taken: {t1:?}");
    assert_eq!(count("verification.failed", "T1"), 0, "the result that counts met every criterion");
    assert_eq!(count("attempt.started", "T2"), 1);
    assert_eq!(submits("T2").len(), 1, "it did not hand in again");
    assert_eq!(count("verification.failed", "T2"), 1, "then the lead decides");
    let failed = evs.iter().find(|e| e.kind.as_str() == "task.failed" && e.task.as_deref() == Some("T2")).unwrap();
    assert_eq!(failed.data.as_ref().unwrap()["reason"], "verification");

    // The numbers per role: T1 succeeded but not on first pass, T2 failed.
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let stats = h.app.role_stats().unwrap();
    assert_eq!(stats["roles"]["m1"]["succeeded"], 1);
    assert_eq!(stats["roles"]["m1"]["firstPass"], 1, "its result passed the first check");
    assert_eq!(stats["roles"]["m2"]["failed"], 1);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The run, token and seat in a session's MCP URL (`…/mcp/<run>/<token>/lead[/tools]` or
/// `…/mcp/<run>/<token>/task/<id>[/tools]`): the credential a real client holds.
pub(super) fn mcp_credential(url: &str) -> Option<(String, String, Seat)> {
    let rest = url.split("/mcp/").nth(1)?;
    let rest = rest.strip_suffix("/tools").unwrap_or(rest);
    let mut parts = rest.splitn(3, '/');
    let (run, token, seat) = (parts.next()?, parts.next()?, parts.next()?);
    let seat = match seat {
        "lead" => Seat::Lead,
        task => Seat::Task(task.strip_prefix("task/")?.to_string()),
    };
    Some((run.to_string(), token.to_string(), seat))
}

/// Call a board tool from the session in `seat`.
async fn tool_as(app: &Arc<App>, run: &str, seat: &Seat, name: &str, args: Value) -> (String, bool) {
    // A run that was just started or resumed gets its board token when its lead starts.
    let r = app.run(run).unwrap();
    for _ in 0..600 {
        if !r.st.lock().unwrap().mcp_token.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token = session_token(&r.st.lock().unwrap().mcp_token, seat);
    let req =
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
    let r = app.mcp(run, &token, seat, req).await.unwrap().unwrap();
    (r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string(), r["result"]["isError"] == true)
}

/// Call a board tool as the lead (`lead`) or as the session of subtask `who`.
async fn tool(app: &Arc<App>, run: &str, who: &str, name: &str, args: Value) -> (String, bool) {
    let seat = if who == "lead" { Seat::Lead } else { Seat::Task(who.to_string()) };
    tool_as(app, run, &seat, name, args).await
}

/// A session in Janus shows what the coding agent itself shows: its thinking, its plan, its
/// slash commands, a tool while it runs and the diff it made, its usage and its mode; the
/// person can switch a control mid-session and point it at a file with `@path`.
#[tokio::test(flavor = "multi_thread")]
async fn sessions_show_what_the_agent_shows_and_take_its_controls() {
    let h = harness("acpview", &[("lead", false), ("m1", true)], json!({ "T1": ["rich"] }));
    std::fs::write(h.repo.join("pay.js"), "a\n").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 started", |e| task_event(e, "attempt.started", "T1")).await;
    act(&h, &run, json!({ "type": "takeover", "task": "T1", "agent": "" }));
    wait_until(&h.app, &run, "T1 waits for the person", |e| {
        e.iter().any(|e| {
            e.kind.as_str() == "agent.status"
                && e.task.as_deref() == Some("T1")
                && e.data.as_ref().unwrap()["turn"] == "idle"
        })
    })
    .await;
    let evs = events(&h.app, &run);
    let one = |kind: &str| {
        evs.iter()
            .find(|e| e.kind == kind && e.task.as_deref() == Some("T1"))
            .unwrap_or_else(|| panic!("no {kind}"))
            .data
            .clone()
            .unwrap()
    };
    assert_eq!(one("agent.thought")["text"], "先看看 pay.js 再改。");
    assert_eq!(one("agent.plan")["entries"].as_array().unwrap().len(), 2);
    assert_eq!(one("session.commands")["commands"][1]["name"], "review");
    assert!(
        one("session.controls")["controls"].as_array().unwrap().iter().any(|c| c["id"] == "mode"),
        "{}",
        one("session.controls")
    );
    let started = one("tool.started");
    assert_eq!((&started["callId"], &started["live"]), (&json!("rich-1"), &json!(true)));
    let call = evs
        .iter()
        .find(|e| e.kind.as_str() == "tool.call" && e.data.as_ref().unwrap()["callId"] == "rich-1")
        .expect("the finished call replaces the running one");
    assert_eq!(call.data.as_ref().unwrap()["diffs"][0]["new"], "a\nsdk()\n");
    let usage: Vec<Value> =
        evs.iter().filter(|e| e.kind.as_str() == "session.usage").map(|e| e.data.clone().unwrap()).collect();
    assert!(usage.iter().any(|u| u["used"] == 52000) && usage.iter().any(|u| u["totalTokens"] == 1500), "{usage:?}");
    assert_eq!(one("session.control")["value"], "agent", "the agent switched its own mode");

    // The person switches the mode and points the agent at a file.
    act(
        &h,
        &run,
        json!({ "type": "control", "task": "T1", "agent": "", "id": "mode", "value": "full", "via": "config" }),
    );
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..50 {
        if std::fs::read_to_string(&calls).unwrap_or_default().contains("set mode \"full\"") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(std::fs::read_to_string(&calls).unwrap().contains("set mode \"full\""));
    wait_until(&h.app, &run, "control recorded", |e| {
        e.iter().any(|e| e.kind.as_str() == "session.control" && e.data.as_ref().unwrap()["by"] == "user")
    })
    .await;
    // An empty control, or an empty value, is refused.
    for bad in [json!({ "id": "", "value": "x" }), json!({ "id": "mode", "value": " " })] {
        let mut a = json!({ "type": "control", "task": "T1", "agent": "" });
        a["id"] = bad["id"].clone();
        a["value"] = bad["value"].clone();
        assert!(h.app.action(&h.app.run(&run).unwrap(), &action_of(a.clone())).is_err(), "{a}");
    }
    act(&h, &run, json!({ "type": "message", "task": "T1", "agent": "", "text": "看看 @pay.js 还有 @nope.js" }));
    for _ in 0..50 {
        if std::fs::read_to_string(&calls).unwrap_or_default().contains("links pay.js") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let log = std::fs::read_to_string(&calls).unwrap();
    assert!(log.contains("links pay.js\n"), "only the file that exists is linked: {log}");
    act(&h, &run, json!({ "type": "finish", "task": "T1", "agent": "" }));
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A task that uses up its attempts fails without stopping the run (the lead reads it on the
/// board); its dependent is blocked, and both carry on after the person retries it.
#[tokio::test(flavor = "multi_thread")]
async fn retry_revives_a_failed_task_and_its_dependent() {
    let h = harness("retry", &[("lead", false), ("m1", false)], json!({ "T1": ["fail", "fail", "ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
    wait_until(&h.app, &run, "blocked", |e| has(e, "task.blocked")).await;
    let evs = events(&h.app, &run);
    assert_eq!(
        evs.iter().filter(|e| e.kind.as_str() == "attempt.started" && e.task.as_deref() == Some("T1")).count(),
        2,
        "two attempts per task"
    );
    assert!(has(&evs, "task.failed"), "the task failed, and the run keeps going");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[失败] T1") && board.contains("[无法开始] T2"), "{board}");
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "session": "new" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T2", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T2"))
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    assert_eq!(status(&h.app, &run), "delivered");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Pausing keeps the run running (it only stops new tasks), and resuming starts them.
#[tokio::test(flavor = "multi_thread")]
async fn pause_keeps_running_status() {
    let h = harness("pause", &[("lead", false), ("m1", false)], json!({}));
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    act(&h, &run, json!({ "type": "pause" }));
    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T1", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(status(&h.app, &run), "running");
    assert!(!has(&events(&h.app, &run), "attempt.started"), "paused: nothing starts");
    act(&h, &run, json!({ "type": "resume" }));
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The run's own state follows the board: a lead that finishes while work is paused does not
/// deliver without it; acceptance only follows delivery; a task with two failed dependencies
/// stays blocked until both are settled; what never ran is listed as a risk.
#[tokio::test(flavor = "multi_thread")]
async fn run_state_follows_the_board() {
    let h = harness(
        "runstate",
        &[("lead", false), ("m1", false), ("m2", false), ("m3", false)],
        json!({ "A": ["fail"], "B": ["fail"] }),
    );
    let run = start(
        &h,
        json!([task("A", "m1", json!({})), task("B", "m2", json!({})), task("C", "m3", json!({ "deps": ["A", "B"] }))]),
    )
    .await;
    wait_until(&h.app, &run, "A and B failed", |e| {
        task_event(e, "task.failed", "A") && task_event(e, "task.failed", "B")
    })
    .await;
    wait_until(&h.app, &run, "C blocked", |e| task_event(e, "task.blocked", "C")).await;
    let r = h.app.run(&run).unwrap();
    assert!(h.app.action(&r, &action_of(json!({ "type": "accept" }))).is_err(), "nothing delivered yet");

    // A is accepted; C still waits on B and says so.
    let blocked_on = |dep: &str| {
        events(&h.app, &run).iter().any(|e| {
            e.kind.as_str() == "task.blocked"
                && e.task.as_deref() == Some("C")
                && e.data.as_ref().is_some_and(|d| d["dependency"] == dep)
        })
    };
    let first = if blocked_on("A") { "A" } else { "B" };
    let other = if first == "A" { "B" } else { "A" };
    std::fs::write(h.dir.join("fake/script.json"), json!({}).to_string()).unwrap();
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": first, "session": "new" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "C blocked on the other one", |_| blocked_on(other)).await;
    assert!(!task_event(&events(&h.app, &run), "attempt.started", "C"));

    // The lead gives up on the other one and C, and adds D.
    let (_, err) = tool(&h.app, &run, "lead", "cancel", json!({ "tasks": [other, "C"] })).await;
    assert!(!err);
    let (_, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("D", "m1", json!({}))] })).await;
    assert!(!err);

    // Paused with work still to run: the lead finishing does not deliver it away.
    act(&h, &run, json!({ "type": "pause" }));
    let (_, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("E", "m2", json!({}))] })).await;
    assert!(!err);
    finish_lead(&h);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let evs = events(&h.app, &run);
    assert!(
        !has(&evs, "run.delivered") && !task_event(&evs, "attempt.started", "E"),
        "paused work is not delivered away"
    );
    act(&h, &run, json!({ "type": "resume" }));
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    assert!(task_event(&events(&h.app, &run), "task.succeeded", "E"));
    let delivered = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "run.delivered").unwrap();
    let risks = delivered.data.unwrap()["risks"].to_string();
    assert!(risks.contains(&format!("{other}：被负责人取消")), "{risks}");

    // After acceptance nothing reopens the run.
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(status(&h.app, &run), "accepted");
    let r = h.app.run(&run).unwrap();
    assert!(h.app.action(&r, &action_of(json!({ "type": "pause" }))).is_err());
    assert!(h.app.action(&r, &action_of(json!({ "type": "cancel" }))).is_err());
    assert_eq!(status(&h.app, &run), "accepted");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// After a restart the board comes back with the same statuses (failed, blocked), and a retry
/// still revives both.
#[tokio::test(flavor = "multi_thread")]
async fn restart_restores_failed_and_blocked() {
    let h = harness("restart", &[("lead", false), ("m1", false)], json!({ "T1": ["fail", "fail", "ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
    wait_until(&h.app, &run, "blocked", |e| has(e, "task.blocked")).await;
    // Simulate the process going away: stop the old driver without writing anything.
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    // Reported usage survives the restart.
    let old = h.app.run(&run).unwrap();
    h.app.emit(
        &old,
        ev(EventKind::SessionUsage, "m1", "用量").task("T1").attempt(1).data(json!({ "totalTokens": 2_000 })),
    );
    let app2 = served_app(&h.dir, |_| {});
    app2.recover().unwrap();
    assert_eq!(status(&app2, &run), "interrupted");
    let r = app2.resume_run(&run).await.unwrap();
    let evs = events(&app2, &run);
    let recovered = evs.iter().rev().find(|e| e.kind.as_str() == "run.recovered").unwrap();
    let tasks = recovered.data.as_ref().unwrap()["tasks"].clone();
    assert_eq!(tasks[0]["status"], "failed", "{tasks}");
    assert_eq!(tasks[1]["status"], "blocked", "{tasks}");
    while r.st.lock().unwrap().mcp_token.is_empty() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) = tool(&app2, &run, "lead", "retry", json!({ "task": "T1", "session": "new" })).await;
    assert!(!err, "{text}");
    wait_until(&app2, &run, "T2", |e| {
        e.iter().any(|e| e.kind.as_str() == "task.succeeded" && e.task.as_deref() == Some("T2"))
    })
    .await;
    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
