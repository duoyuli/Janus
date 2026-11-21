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
mod messages;
mod owners;
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
    let app = test_app(&dir);
    // What a check of the fake agent reads: whether a member writes follows from its mode.
    let modes: Vec<Value> =
        ["read-only", "ask", "agent", "full"].iter().map(|v| json!({ "value": v, "name": v })).collect();
    // The scripted agent, through the stand-in Claude Code adapter (`stand_in_adapters`).
    app.db.put_config("connection", "fake", &crate::agents::test_connection("fake", "claude", "claude", json!({
        "name": "Fake",
        "env": { "FAKE_DIR": fake.to_string_lossy() },
        "options": { "controls": [{ "id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "config", "current": "ask", "permission": true, "options": modes }] }
    })), now_ms()).unwrap();
    for (i, (id, writable)) in members.iter().enumerate() {
        app.db
            .put_config(
                "role",
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
            "workspace",
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
    app.db.run(run).unwrap().unwrap()["status"].as_str().unwrap().to_string()
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
    h.app.action(&r, &a).unwrap();
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
    let mut conn = h.app.db.get_config("connection", "fake").unwrap().unwrap();
    conn["options"]["controls"].as_array_mut().unwrap().push(json!({ "id": "plan_mode", "name": "Plan", "type": "boolean", "via": "config", "current": false, "permission": true, "options": [] }));
    h.app.db.put_config("connection", "fake", &conn, now_ms()).unwrap();
    // The agent starts in Plan mode and the role asks for it: Janus switches it off.
    std::fs::write(h.dir.join("fake/plan-on"), "").unwrap();
    let mut role = h.app.db.get_config("role", "m1").unwrap().unwrap();
    role["session"] = json!({ "mode": "full", "plan_mode": true });
    h.app.db.put_config("role", "m1", &role, now_ms()).unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;

    wait_until(&h.app, &run, "permission request", |e| has(e, "approval.requested")).await;
    let req = events(&h.app, &run).into_iter().find(|e| e.kind == "approval.requested").unwrap();
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
    let form = events(&h.app, &run).into_iter().find(|e| e.kind == "input.requested").unwrap();
    assert_eq!(form.data.as_ref().unwrap()["schema"]["required"], json!(["name"]));
    act(
        &h,
        &run,
        json!({ "type": "input", "key": form.data.as_ref().unwrap()["key"], "task": "T2", "action": "accept", "content": { "name": "janus" } }),
    );

    wait_until(&h.app, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2")))
        .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let summary = |t: &str| {
        evs.iter()
            .find(|e| e.kind == "result.submitted" && e.task.as_deref() == Some(t))
            .map(|e| e.data.as_ref().unwrap()["summary"].as_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    };
    assert_eq!(summary("T1"), "picked a2", "the chosen option, not a generic allow");
    assert_eq!(summary("T2"), "accept janus");
    let started = evs.iter().find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).unwrap();
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
    let mut h = harness(
        "watchdog",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["slow"], "T2": ["loop"], "T3": ["permission"] }),
    );
    Arc::get_mut(&mut h.app).unwrap().cfg.stall_after = Duration::from_secs(1);
    let run = start(
        &h,
        json!([task("T1", "m1", json!({})), task("T2", "m2", json!({})), task("T3", "m2", json!({ "deps": ["T2"] }))]),
    )
    .await;
    let stalled = |e: &[Event], t: &str, reason: &str| {
        e.iter()
            .filter(|e| {
                e.kind == "task.stalled" && e.task.as_deref() == Some(t) && e.data.as_ref().unwrap()["reason"] == reason
            })
            .count()
    };
    wait_until(&h.app, &run, "both stalls", |e| stalled(e, "T1", "idle") > 0 && stalled(e, "T2", "loop") > 0).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[停滞] T1") && board.contains("没有任何输出或工具调用"), "{board}");
    assert!(board.contains("[停滞] T2") && board.contains("连续 5 次") && board.contains("cat missing.txt"), "{board}");

    // T3 waits on the person's approval: that silence is not the agent's.
    wait_until(&h.app, &run, "T3 asks", |e| has(e, "approval.requested")).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let evs = events(&h.app, &run);
    assert_eq!(stalled(&evs, "T1", "idle"), 1, "one report per quiet spell");
    assert_eq!(stalled(&evs, "T3", "idle"), 0, "waiting on the person is not a stall");
    let req = evs.iter().find(|e| e.kind == "approval.requested").unwrap();
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
        evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).count(),
        1,
        "no fresh retry for a failed verification"
    );
    assert!(!task_event(&evs, "task.retry_scheduled", "T1"));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(
        board.contains("[没通过验收] T1")
            && board.contains("自认没有达到「页面能打开」")
            && board.contains("janus.accept"),
        "{board}"
    );
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
        .find(|e| e.kind == "task.reopened" && e.task.as_deref() == Some("T3"))
        .unwrap();
    assert_eq!(reopened.data.as_ref().unwrap()["instructions"], "换个做法再做一次");
    assert_eq!(reopened.data.as_ref().unwrap()["accept"], json!(["换了做法"]));

    // A decision only the person can make: the answer reaches the lead through wait.
    let (text, err) = tool(&h.app, &run, "lead", "ask_user", json!({ "question": "部署到哪个环境？", "why": "需要用户的账号", "options": ["测试", "生产"], "recommend": "测试" })).await;
    assert!(!err, "{text}");
    let asked = events(&h.app, &run).into_iter().find(|e| e.kind == "question.escalated").unwrap();
    let key = asked.data.as_ref().unwrap()["key"].as_str().unwrap().to_string();
    act(&h, &run, json!({ "type": "answer", "key": key, "task": "", "answer": "测试" }));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[用户决定]") && board.contains("测试"), "{board}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A result that does not meet its criteria goes back into the same session once; the agent
/// fixes its work there.
/// A second failure goes to the lead, still within one attempt.
#[tokio::test(flavor = "multi_thread")]
async fn verification_failure_is_fixed_in_the_same_session() {
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
    assert_eq!(count("attempt.started", "T1"), 1, "fixed in the same session, no new attempt");
    assert_eq!(count("verification.failed", "T1"), 1);
    assert!(evs.iter().any(|e| {
        e.kind == "session.prompt"
            && e.task.as_deref() == Some("T1")
            && e.data.as_ref().is_some_and(|d| {
                d["channel"] == "fix" && d["prompt"].as_str().unwrap_or("").contains("自认没有达到「做好 done 功能」")
            })
    }));
    assert_eq!(count("attempt.started", "T2"), 1);
    assert_eq!(count("verification.failed", "T2"), 2, "one fix round, then the lead decides");
    let failed = evs.iter().find(|e| e.kind == "task.failed" && e.task.as_deref() == Some("T2")).unwrap();
    assert_eq!(failed.data.as_ref().unwrap()["reason"], "verification");

    // The numbers per role: T1 succeeded but not on first pass, T2 failed.
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let stats = h.app.role_stats().unwrap();
    assert_eq!(stats["roles"]["m1"]["succeeded"], 1);
    assert_eq!(stats["roles"]["m1"]["firstPass"], 0, "it needed the fix round");
    assert_eq!(stats["roles"]["m1"]["verificationFailed"], 1);
    assert_eq!(stats["roles"]["m2"]["failed"], 1);
    let _ = std::fs::remove_dir_all(&h.dir);
}

async fn tool(app: &Arc<App>, run: &str, who: &str, name: &str, args: Value) -> (String, bool) {
    // A run that was just started or resumed gets its board token when its lead starts.
    let r = app.run(run).unwrap();
    for _ in 0..600 {
        if !r.st.lock().unwrap().mcp_token.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let token = session_token(&r.st.lock().unwrap().mcp_token, who);
    let req =
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
    let r = app.mcp(run, &token, who, req).await.unwrap().unwrap();
    (r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string(), r["result"]["isError"] == true)
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
            e.kind == "agent.status" && e.task.as_deref() == Some("T1") && e.data.as_ref().unwrap()["turn"] == "idle"
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
        .find(|e| e.kind == "tool.call" && e.data.as_ref().unwrap()["callId"] == "rich-1")
        .expect("the finished call replaces the running one");
    assert_eq!(call.data.as_ref().unwrap()["diffs"][0]["new"], "a\nsdk()\n");
    let usage: Vec<Value> = evs.iter().filter(|e| e.kind == "session.usage").map(|e| e.data.clone().unwrap()).collect();
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
        e.iter().any(|e| e.kind == "session.control" && e.data.as_ref().unwrap()["by"] == "user")
    })
    .await;
    // An empty control, or an empty value, is refused.
    for bad in [json!({ "id": "", "value": "x" }), json!({ "id": "mode", "value": " " })] {
        let mut a = json!({ "type": "control", "task": "T1", "agent": "" });
        a["id"] = bad["id"].clone();
        a["value"] = bad["value"].clone();
        assert!(h.app.action(&h.app.run(&run).unwrap(), &a).is_err(), "{a}");
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
        evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).count(),
        2,
        "two attempts per task"
    );
    assert!(has(&evs, "task.failed"), "the task failed, and the run keeps going");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[失败] T1") && board.contains("[无法开始] T2"), "{board}");
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "session": "new" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2")))
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
/// stays blocked until both are settled; running out of budget is announced again when the
/// lead adds work later; what never ran is listed as a risk.
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
    assert!(h.app.action(&r, &json!({ "type": "accept" })).is_err(), "nothing delivered yet");

    // A is accepted; C still waits on B and says so.
    let blocked_on = |dep: &str| {
        events(&h.app, &run).iter().any(|e| {
            e.kind == "task.blocked"
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

    // Out of budget: announced; back to running once nothing is left; announced again when
    // the lead adds work.
    let r = h.app.run(&run).unwrap();
    r.deadline_ms.store(now_ms() - 1, Ordering::SeqCst);
    r.wake.notify_one();
    wait_until(&h.app, &run, "budget", |e| has(e, "run.budget_exhausted")).await;
    act(&h, &run, json!({ "type": "extendBudget", "minutes": 1 }));
    let (_, err) = tool(&h.app, &run, "lead", "cancel", json!({ "tasks": [other, "C"] })).await;
    assert!(!err);
    r.deadline_ms.store(now_ms() - 1, Ordering::SeqCst);
    r.wake.notify_one();
    wait_until(&h.app, &run, "running again", |_| status(&h.app, &run) == "running").await;
    let (_, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("D", "m1", json!({}))] })).await;
    assert!(!err);
    wait_until(&h.app, &run, "budget again", |e| e.iter().filter(|e| e.kind == "run.budget_exhausted").count() == 2)
        .await;
    assert_eq!(status(&h.app, &run), "waiting_input");

    // Paused with D still to run: the lead finishing does not deliver it away.
    act(&h, &run, json!({ "type": "extendBudget", "minutes": 60 }));
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
    let delivered = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap();
    let risks = delivered.data.unwrap()["risks"].to_string();
    assert!(risks.contains(&format!("{other}：被负责人取消")), "{risks}");

    // After acceptance nothing reopens the run.
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(status(&h.app, &run), "accepted");
    let r = h.app.run(&run).unwrap();
    assert!(h.app.action(&r, &json!({ "type": "extendBudget", "minutes": 5 })).is_err());
    assert!(h.app.action(&r, &json!({ "type": "cancel" })).is_err());
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
    // Older runs can have both reported usage and a persisted token ceiling at restart.
    let old = h.app.run(&run).unwrap();
    h.app.emit(&old, ev("session.usage", "m1", "用量").task("T1").attempt(1).data(json!({ "totalTokens": 2_000 })));
    h.app.emit(&old, ev("run.budget_extended", "user", "旧预算").data(json!({ "kind": "tokens", "tokenLimit": 1000 })));
    h.app.emit(&old, ev("run.budget_exhausted", "janus", "旧预算用完").data(json!({ "kind": "tokens" })));
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    assert_eq!(status(&app2, &run), "interrupted");
    let r = app2.resume_run(&run).await.unwrap();
    let evs = events(&app2, &run);
    let recovered = evs.iter().rev().find(|e| e.kind == "run.recovered").unwrap();
    let tasks = recovered.data.as_ref().unwrap()["tasks"].clone();
    assert_eq!(tasks[0]["status"], "failed", "{tasks}");
    assert_eq!(tasks[1]["status"], "blocked", "{tasks}");
    while r.st.lock().unwrap().mcp_token.is_empty() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) = tool(&app2, &run, "lead", "retry", json!({ "task": "T1", "session": "new" })).await;
    assert!(!err, "{text}");
    wait_until(&app2, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2")))
        .await;
    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
