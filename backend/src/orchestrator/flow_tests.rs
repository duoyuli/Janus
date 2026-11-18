//! End-to-end runs against a scripted ACP agent (testdata/fake_acp.py): each test drives one of
//! the paths that used to leave a run stuck, and checks it either delivers or waits somewhere
//! the person can act.

use super::*;

// Newer flows live in their own files under `orchestrator/flow_tests/`, so this file stops growing.
mod actions;
mod collaboration;
mod evidence;
mod owners;
mod usage;

struct Harness {
    app: Arc<App>,
    dir: PathBuf,
    repo: PathBuf,
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
    std::fs::write(repo.join("README.md"), "test\n").unwrap();
    sh(&repo, &["add", "-A"]);
    sh(&repo, &["commit", "-q", "-m", "init"]);
    let app = test_app(&dir);
    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_acp.py");
    // What a check of the fake agent reads: whether a member writes follows from its mode.
    let modes: Vec<Value> =
        ["read-only", "ask", "agent", "full"].iter().map(|v| json!({ "value": v, "name": v })).collect();
    app.db.put_config("connection", "fake", &json!({
        "id": "fake", "kind": "custom", "base": "custom", "name": "Fake", "command": format!("python3 {}", script_path.display()),
        "env": { "FAKE_DIR": fake.to_string_lossy() },
        "options": { "controls": [{ "id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "config", "current": "ask", "permission": true, "options": modes }] }
    }), now_ms()).unwrap();
    for (i, (id, writable)) in members.iter().enumerate() {
        app.db
            .put_config(
                "role",
                id,
                &json!({
                    "id": id, "name": id, "connection": "fake",
                    "lead": i == 0,
                    "session": if *writable { json!({}) } else { json!({ "mode": "read-only" }) },
                }),
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
                "id": "ws", "name": "ws", "path": repo.canonicalize().unwrap(), "lead": members[0].0, "members": ms
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
    app.db.runs().unwrap().into_iter().find(|r| r["id"] == run).unwrap()["status"].as_str().unwrap().to_string()
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
        json!({ "task": "T3", "instructions": "换个做法再做一次", "accept": ["换了做法"] }),
    )
    .await;
    assert!(!err && text.contains("因它被取消的 T4 也恢复了"), "{text}");
    // T4 was dropped only because of T3, so it came back with it.
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T4" })).await;
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
    assert!(evs.iter().any(|e| e.kind == "message.routed"
        && e.task.as_deref() == Some("T1")
        && e.data.as_ref().is_some_and(|d| d["delivery"] == "interrupt")));
    assert!(task_event(&evs, "message.delivered", "T1"));

    // The person takes T2 over: the lead's message waits until the session is handed back.
    act(&h, &run, json!({ "type": "interrupt", "task": "T2" }));
    wait_until(&h.app, &run, "T2 waiting for the person", |e| {
        e.iter().any(|e| e.kind == "agent.status" && e.task.as_deref() == Some("T2") && e.text.contains("人工接管"))
    })
    .await;
    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "先停一下" })).await;
    assert!(!err);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind == "message.routed"
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

/// The workspace's shared material reaches the lead (tasks: `task_prompts_carry_the_shared_material`), and the lead's suggested
/// additions come back with the delivery, apart from its summary.
#[tokio::test(flavor = "multi_thread")]
async fn shared_material_reaches_everyone_and_suggestions_come_back() {
    let h = harness("brief", &[("lead", false), ("m1", false)], json!({}));
    let mut ws = h.app.db.get_config("workspace", "ws").unwrap().unwrap();
    ws["brief"] = json!("目标用户是独立开发者");
    h.app.db.put_config("workspace", "ws", &ws, now_ms()).unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    std::fs::write(
        h.dir.join("fake/lead-go"),
        "做完了。\n\n## Suggested Updates to Shared Context\n- 定价页在 docs/pricing.md\n- 免费版限 3 个项目",
    )
    .unwrap();
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let prompts: Vec<String> = evs
        .iter()
        .filter(|e| e.kind == "session.prompt")
        .map(|e| e.data.as_ref().unwrap()["prompt"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        prompts.iter().any(|p| p.contains("Shared workspace context") && p.contains("目标用户是独立开发者")),
        "the lead sees it: {prompts:?}"
    );
    let d = evs.iter().find(|e| e.kind == "run.delivered").unwrap().data.clone().unwrap();
    assert_eq!(d["briefSuggestions"], json!(["定价页在 docs/pricing.md", "免费版限 3 个项目"]));
    assert!(
        d["summary"].as_str().unwrap().starts_with("做完了。")
            && !d["summary"].as_str().unwrap().contains(BRIEF_HEADING),
        "{d}"
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// When the lead overrules a finished task's conclusion, tasks that build on it use the new one:
/// a running one hears it at once, one that starts later gets it with the handoff.
#[tokio::test(flavor = "multi_thread")]
async fn amended_conclusions_reach_downstream_tasks() {
    let h = harness(
        "amend",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "A": ["ok"], "B": ["slow", "ok"], "C": ["ok"] }),
    );
    let run = start(&h, json!([task("A", "m1", json!({})), task("B", "m2", json!({ "deps": ["A"] }))])).await;
    wait_until(&h.app, &run, "B running", |e| task_event(e, "attempt.started", "B")).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (text, err) = tool(&h.app, &run, "lead", "amend", json!({ "task": "B", "text": "x" })).await;
    assert!(err && text.contains("只能修订已完成子任务"), "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "amend", json!({ "task": "A", "text": "改用经典 8 邻域规则" })).await;
    assert!(!err && text.contains("B"), "{text}");
    wait_until(&h.app, &run, "B after the amendment", |e| task_event(e, "task.succeeded", "B")).await;
    let evs = events(&h.app, &run);
    assert!(task_event(&evs, "result.amended", "A"));
    assert!(evs.iter().any(|e| e.kind == "user.message"
        && e.task.as_deref() == Some("B")
        && e.data.as_ref().is_some_and(|d| d["text"].as_str().unwrap_or("").contains("改用经典 8 邻域规则"))));

    let (_, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("C", "m3", json!({ "deps": ["A"] }))] }))
            .await;
    assert!(!err);
    wait_until(&h.app, &run, "C started", |e| task_event(e, "attempt.started", "C")).await;
    let started = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("C"))
        .unwrap();
    assert!(
        started.data.as_ref().unwrap()["prompt"]
            .as_str()
            .unwrap()
            .contains("负责人修订了这个结论，以修订为准：改用经典 8 邻域规则")
    );
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Team structure in an orchestration: tasks that may run together cannot own the same files;
/// a verification task goes to someone else, waits for the work, is told what to check, and
/// its failure cannot be accepted away.
#[tokio::test(flavor = "multi_thread")]
async fn verifiers_check_others_work_and_cannot_be_waved_through() {
    let h = harness("verifier", &[("lead", false), ("m1", true), ("m2", true)], json!({ "V1": ["unmet"] }));
    let run = start(&h, json!([task("T0", "m1", json!({}))])).await;

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        task("T1", "m1", json!({ "owns": ["src"] })), task("T2", "m2", json!({ "owns": ["src/main.rs"] })),
        task("V0", "m1", json!({ "verifies": "T1" }))] }),
    )
    .await;
    assert!(err && text.contains("T1 和 T2") && text.contains("V0") && text.contains("另一个协作者"), "{text}");

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        task("T1", "m1", json!({ "owns": ["src"], "accept": ["src 里有新函数"] })),
        task("V1", "m2", json!({ "verifies": "T1", "accept": ["测试通过"] })),
        task("D1", "m1", json!({ "deps": ["V1"] }))] }),
    )
    .await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "V1 reviewed", |e| task_event(e, "task.succeeded", "V1")).await;
    // What depends on a verification that rejected the work does not start on it.
    wait_until(&h.app, &run, "D1 blocked", |e| task_event(e, "task.blocked", "D1")).await;
    let evs = events(&h.app, &run);
    let at =
        |kind: &str, task: &str| evs.iter().position(|e| e.kind == kind && e.task.as_deref() == Some(task)).unwrap();
    assert!(at("task.succeeded", "T1") < at("attempt.started", "V1"), "the verifier waits for the work");
    assert!(
        !evs.iter().any(|e| e.kind == "session.prompt"
            && e.task.as_deref() == Some("V1")
            && e.data.as_ref().is_some_and(|d| d["channel"] == "fix")),
        "a verifier reporting a defect with evidence never gets a pointless fix round"
    );
    let prompt = |task: &str| -> String {
        evs.iter()
            .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(task))
            .and_then(|e| e.data.as_ref()?["prompt"].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    assert!(prompt("T1").contains("你负责的文件：src"), "{}", prompt("T1"));
    assert!(prompt("V1").contains("独立验证者") && prompt("V1").contains("- src 里有新函数"), "{}", prompt("V1"));

    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[验证未通过] V1") && board.contains("独立验证") && board.contains("rejected"), "{board}");
    assert!(board.contains("[无法开始] D1") && board.contains("V1 判定 rejected"), "{board}");
    let blocked = evs.iter().find(|e| e.kind == "task.blocked" && e.task.as_deref() == Some("D1")).unwrap();
    assert_eq!(blocked.data.as_ref().unwrap()["verdict"], "rejected", "{blocked:?}");
    let (text, err) = tool(&h.app, &run, "lead", "accept", json!({ "task": "V1", "reason": "我看过了" })).await;
    assert!(err && text.contains("不能直接接受"), "{text}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    // The delivery says a verifier whose verdict rejected the work did not confirm the task.
    let d = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap().data.unwrap();
    let t1 = d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == "T1").unwrap().clone();
    assert_eq!(t1["verifications"][0]["task"], "V1", "{t1}");
    assert_eq!(t1["verifications"][0]["verdict"], "rejected", "{t1}");
    assert_eq!(t1["verifications"][0]["passed"], false, "a rejected verdict is not a pass: {t1}");
    assert!(
        d["risks"].as_array().unwrap().iter().any(|r| r.as_str().is_some_and(|r| r.contains("独立验证 V1"))),
        "{d}"
    );
    assert!(
        !events(&h.app, &run).iter().any(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("D1")),
        "D1 never ran on rejected work"
    );
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

/// Goal mode end to end: the lead (a fake Codex) gets the brief and `/goal`; it orchestrates
/// through the board tools, which validate before anything runs; collaborators report back
/// on the board, a question goes to the lead and its answer back into the session; the run
/// is delivered once the lead is done, with the lead's summary.
#[tokio::test(flavor = "multi_thread")]
async fn goal_mode_orchestrates_through_the_board() {
    let h = harness(
        "goal",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["write:notes.md"], "T2": ["ask", "ok"] }),
    );
    let run = h
        .app
        .create_run(NewRun { goal: "写一份笔记".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..100 {
        if std::fs::read_to_string(&calls).unwrap_or_default().contains("goal 写一份笔记") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let log = std::fs::read_to_string(&calls).unwrap_or_else(|_| {
        panic!(
            "the lead got nothing: {:#?}",
            events(&h.app, &run).iter().map(|e| format!("{} {}", e.kind, e.text)).collect::<Vec<_>>()
        )
    });
    assert!(log.contains("brief ok") && log.contains("goal 写一份笔记"), "brief, then /goal: {log}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| {
        e.kind == "session.prompt"
            && e.to.as_deref() == Some("lead")
            && e.data.as_ref().is_some_and(|d| d["channel"] == "context")
    }));
    assert!(evs.iter().any(|e| {
        e.kind == "session.prompt"
            && e.to.as_deref() == Some("lead")
            && e.data.as_ref().is_some_and(|d| d["channel"] == "goal")
    }));

    // The board's MCP surface, and its token.
    let secret = h.app.run(&run).unwrap().st.lock().unwrap().mcp_token.clone();
    let token = session_token(&secret, "lead");
    let member_token = session_token(&secret, "T1");
    let tools = h
        .app
        .mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 10);
    let member = h
        .app
        .mcp(&run, &member_token, "T1", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> =
        member["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["board_post", "claim", "peers", "request_split"]);
    // Claims are a collaborator's: the lead assigns files through `owns` instead.
    let (text, err) = tool(&h.app, &run, "lead", "claim", json!({ "paths": ["x"] })).await;
    assert!(err && text.contains("没有这个工具"), "{text}");
    assert!(
        h.app.mcp(&run, "wrong", "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );
    // A collaborator's URL renamed to the lead, or to another task, is refused.
    assert!(
        h.app
            .mcp(&run, &member_token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .await
            .is_err()
    );
    assert!(
        h.app
            .mcp(&run, &member_token, "T2", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .await
            .is_err()
    );
    assert!(
        h.app.mcp(&run, &secret, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err()
    );
    assert!(
        h.app
            .mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await
            .unwrap()
            .is_none()
    );

    // Validation first: every problem at once, and nothing starts.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
        { "id": "T1", "title": "写笔记", "instructions": "写", "assignee": "nobody" },
        { "id": "T2", "title": "定名", "instructions": "定", "assignee": "m2", "deps": ["T9"] },
        { "id": "T3", "title": "查", "instructions": "查", "assignee": "m1" }] }),
    )
    .await;
    assert!(err && text.contains("nobody") && text.contains("T9") && text.contains("子任务 T3：缺少 accept"), "{text}");
    assert!(h.app.run(&run).unwrap().st.lock().unwrap().tasks.is_empty());

    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [
        { "id": "T1", "title": "写笔记", "instructions": "在 notes.md 写笔记", "assignee": "m1", "accept": ["notes.md 里有笔记"] },
        { "id": "T2", "title": "定名", "instructions": "给笔记定个名字", "assignee": "m2", "deps": ["T1"], "accept": ["给出一个名字"] }] })).await;
    assert!(!err, "{text}");
    let evs = events(&h.app, &run);
    assert!(
        evs.iter()
            .any(|e| { e.kind == "tool.started" && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.orchestrate") })
    );
    assert!(evs.iter().any(|e| {
        e.kind == "tool.completed"
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.orchestrate" && d["status"] == "completed")
    }));
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [{ "id": "T1", "title": "x", "instructions": "x", "assignee": "m1" }] }),
    )
    .await;
    assert!(err && text.contains("重复"), "ids stay unique across orchestrations: {text}");

    // A collaborator writes back while it works.
    let (text, err) =
        tool(&h.app, &run, "T1", "board_post", json!({ "kind": "progress", "text": "笔记写了一半" })).await;
    assert!(!err, "{text}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| {
        e.kind == "tool.started"
            && e.task.as_deref() == Some("T1")
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.board_post")
    }));
    assert!(evs.iter().any(|e| {
        e.kind == "tool.completed"
            && e.task.as_deref() == Some("T1")
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.board_post" && d["status"] == "completed")
    }));
    let (_, err) = tool(&h.app, &run, "T1", "orchestrate", json!({ "tasks": [] })).await;
    assert!(err, "collaborators cannot orchestrate");

    // The lead reads the board until the question arrives, answers it, and reads on.
    let mut board = String::new();
    let mut answered = false;
    for _ in 0..60 {
        let (text, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 2 })).await;
        board.push_str(&text);
        board.push('\n');
        if !answered && board.contains("[提问] T2") {
            let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "alpha" })).await;
            assert!(!err && text.contains("已回答"), "{text}");
            answered = true;
        }
        if board.contains("[完成] T2") {
            break;
        }
    }
    assert!(board.contains("[进展] T1") && board.contains("[完成] T1"), "{board}");
    assert!(board.contains("按回答 alpha 完成"), "the answer went back into T2's session: {board}");
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| { e.kind == "tool.started" && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.wait") })
    );
    assert!(evs.iter().any(|e| {
        e.kind == "tool.completed"
            && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.wait" && d["status"] == "completed")
    }));
    assert!(!has(&evs, "question.escalated"), "questions go to the lead, not the person");
    assert!(has(&evs, "orchestration.rejected") && has(&evs, "orchestration.accepted") && has(&evs, "board.posted"));
    assert!(!has(&evs, "run.delivered"), "not before the lead is done");

    // The lead finishes; the run is delivered with its summary.
    std::fs::write(h.dir.join("fake/lead-go"), "目标已完成：笔记写好了").unwrap();
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let delivered = evs.iter().find(|e| e.kind == "run.delivered").unwrap();
    assert!(delivered.data.as_ref().unwrap()["summary"].as_str().unwrap().contains("笔记写好了"));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead can end its goal while a collaborator still waits on a question; nobody reads
/// the board any more, so the question goes to the person and the run still finishes.
#[tokio::test(flavor = "multi_thread")]
async fn questions_left_by_the_lead_go_to_the_person() {
    let h = harness("goal-left", &[("lead", false), ("m1", true)], json!({ "T1": ["ask", "ok"] }));
    let run = h
        .app
        .create_run(NewRun { goal: "定个名字".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "定名", "instructions": "定个名字", "assignee": "m1", "accept": ["给出一个名字"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "question", |e| has(e, "question.asked")).await;
    assert!(!has(&events(&h.app, &run), "question.escalated"), "the lead answers while it is there");

    std::fs::write(h.dir.join("fake/lead-go"), "目标已完成").unwrap();
    wait_until(&h.app, &run, "escalation", |e| has(e, "question.escalated")).await;
    let evs = events(&h.app, &run);
    assert!(!has(&evs, "run.delivered"), "the run waits for the answer");
    let key = evs.iter().find(|e| e.kind == "question.asked").unwrap().data.as_ref().unwrap()["key"].clone();
    act(&h, &run, json!({ "type": "answer", "key": key, "task": "T1", "answer": "beta" }));
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    assert_eq!(evs.iter().filter(|e| e.kind == "question.escalated").count(), 1);
    assert!(evs.iter().any(|e| e.kind == "result.submitted" && e.text.contains("beta")), "the answer reached T1");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Goal mode survives a restart: the board's tasks come back as they were, the lead starts
/// again knowing them, and can orchestrate on top of them.
#[tokio::test(flavor = "multi_thread")]
async fn goal_mode_resumes_with_the_board() {
    let h = harness("goal-resume", &[("lead", false), ("m1", true)], json!({}));
    // Left over from earlier work: not this run's change.
    std::fs::write(h.repo.join("leftover.txt"), "old\n").unwrap();
    let run = h
        .app
        .create_run(NewRun { goal: "两步完成".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let calls = h.dir.join("fake/calls.log");
    let lead_started = |n: usize| {
        let calls = calls.clone();
        async move {
            for _ in 0..100 {
                if std::fs::read_to_string(&calls).unwrap_or_default().matches("goal 两步完成").count() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("lead did not start");
        }
    };
    lead_started(1).await;
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "第一步", "instructions": "做第一步", "assignee": "m1", "accept": ["第一步完成"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T1")))
        .await;
    assert!(
        !events(&h.app, &run).iter().any(|e| e.kind == "artifact.created" && e.text.contains("T1-a1.diff")),
        "T1 changed nothing"
    );

    // The process goes away while the lead is still working.
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    app2.resume_run(&run).await.unwrap();
    let evs = events(&app2, &run);
    let recovered = evs.iter().rev().find(|e| e.kind == "run.recovered").unwrap();
    assert_eq!(recovered.data.as_ref().unwrap()["tasks"][0]["status"], "succeeded");
    lead_started(2).await;
    assert!(std::fs::read_to_string(&calls).unwrap().contains("brief ok resumed"), "the lead is told it continues");

    // It builds on the board it found.
    let (text, err) = tool(&app2, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T2", "title": "第二步", "instructions": "做第二步", "assignee": "m1", "deps": ["T1"], "accept": ["第二步完成"] }] })).await;
    assert!(!err, "{text}");
    wait_until(&app2, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2")))
        .await;
    let (board, _) = tool(&app2, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[完成] T2") && !board.contains("[完成] T1"), "only what is new: {board}");
    std::fs::write(h.dir.join("fake/lead-go"), "两步都完成了").unwrap();
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

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
    let sent = evs.iter().find(|e| e.kind == "user.message").expect("recorded at once");
    let d = sent.data.as_ref().unwrap();
    assert_eq!((d["delivery"].as_str(), sent.to.as_deref(), sent.task.as_deref()), (Some("pending"), Some("m1"), None));
    let id = d["id"].as_str().unwrap().to_string();
    let routed = |evs: &[Event], how: &str| {
        evs.iter().any(|e| {
            e.kind == "message.routed"
                && e.data.as_ref().is_some_and(|d| d["id"] == id.as_str() && d["delivery"] == how)
        })
    };
    wait_until(&h.app, &run, "queued", |e| routed(e, "queued")).await;
    let req = events(&h.app, &run).into_iter().find(|e| e.kind == "approval.requested").unwrap();
    act(
        &h,
        &run,
        json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T1", "allow": true, "option": "a1" }),
    );
    wait_until(&h.app, &run, "delivered", |e| {
        e.iter().any(|e| {
            e.kind == "message.delivered"
                && e.data
                    .as_ref()
                    .is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id.as_str())))
        })
    })
    .await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;

    // No session now: a note, carried into m1's next session.
    act(&h, &run, json!({ "type": "message", "agent": "m1", "text": "下一个子任务注意测试" }));
    let note = events(&h.app, &run).into_iter().rev().find(|e| e.kind == "user.message").unwrap();
    let note_id = note.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
    assert!(events(&h.app, &run).iter().any(|e| e.kind == "message.routed"
        && e.data.as_ref().is_some_and(|d| d["id"] == note_id.as_str() && d["delivery"] == "next")));
    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T2", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "note delivered", |e| {
        e.iter().any(|e| {
            e.kind == "message.delivered"
                && e.data
                    .as_ref()
                    .is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == note_id.as_str())))
        })
    })
    .await;
    let t2 = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2"))
        .unwrap();
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
            e.kind == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "followup" && d["prompt"].as_str().is_some_and(|t| t.contains("再看看 README"))
                })
        })
    })
    .await;
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| e.kind == "user.message"
            && e.data.as_ref().is_some_and(|d| d["text"].as_str().is_some_and(|t| t.contains("再看看 README")))),
        "the message is shown in the same session"
    );
    assert_eq!(evs.iter().filter(|e| e.kind == "run.created").count(), 1, "no new run");
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
            e.kind == "session.prompt"
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
    h.app.action(&reopened, &json!({ "type": "message", "agent": "lead", "text": "历史子任务继续" })).unwrap();
    wait_until(&h.app, &run, "reopened prompt", |evs| {
        evs.iter().any(|e| {
            e.kind == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "followup" && d["prompt"].as_str().is_some_and(|t| t.contains("历史子任务继续"))
                })
        })
    })
    .await;
    assert!(
        events(&h.app, &run).iter().filter(|e| e.kind == "session.controls").count() >= 2,
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
            e.kind == "session.prompt"
                && e.data.as_ref().is_some_and(|d| {
                    d["channel"] == "goal" && d["prompt"].as_str().is_some_and(|t| t.contains("新目标"))
                })
        })
    })
    .await;
    assert_eq!(events(&h.app, &run).iter().filter(|e| e.kind == "run.created").count(), 1, "no new run");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A lead whose agent takes steering hears about a collaborator's question while it works,
/// without calling `wait`: one board push steered into its turn, recorded in its session and
/// counted as seen. A message the person steers in is marked delivered once the agent says it
/// applied it.
#[tokio::test(flavor = "multi_thread")]
async fn board_events_are_pushed_into_a_steerable_lead() {
    let h = harness("push", &[("lead", false), ("m1", true)], json!({ "T1": ["ask", "ok"] }));
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..100 {
        let log = std::fs::read_to_string(&calls).unwrap_or_default();
        if log.contains("steer [Janus 看板推送]") && log.contains("[提问] T1") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let log = std::fs::read_to_string(&calls).unwrap_or_default();
    assert!(log.contains("steer [Janus 看板推送]") && log.contains("[提问] T1"), "{log}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind == "session.prompt"
        && e.to.as_deref() == Some("lead")
        && e.data.as_ref().is_some_and(|d| d["channel"] == "board")));
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(!board.contains("[提问]"), "a pushed entry does not come again: {board}");

    // The person steers into the lead's turn: routed at once, delivered when applied.
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": "先处理 T1 的提问" }));
    let id = events(&h.app, &run).into_iter().rev().find(|e| e.kind == "user.message").unwrap().data.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let with = |e: &Event, kind: &str| {
        e.kind == kind
            && e.data.as_ref().is_some_and(|d| {
                d["id"] == id.as_str() || d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id.as_str()))
            })
    };
    wait_until(&h.app, &run, "steered and applied", |e| {
        e.iter().any(|e| with(e, "message.routed") && e.data.as_ref().unwrap()["delivery"] == "steer")
            && e.iter().any(|e| with(e, "message.delivered") && e.data.as_ref().unwrap()["applied"] == true)
    })
    .await;

    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T1", "text": "alpha" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Two tasks running at once edit the same file: the lead reads a conflict on the board, both
/// collaborators that take steering get a notice, and the files stay recallable per task.
#[tokio::test(flavor = "multi_thread")]
async fn conflicting_edits_reach_the_lead_and_both_sides() {
    let h = harness(
        "conflict",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["edit:shared.txt"], "T2": ["edit:shared.txt"] }),
    );
    std::fs::write(h.dir.join("fake/steer"), "").unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m2", json!({}))])).await;
    wait_until(&h.app, &run, "conflict", |e| has(e, "task.conflict")).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[冲突]") && board.contains("shared.txt"), "{board}");
    wait_until(&h.app, &run, "both done", |e| {
        task_event(e, "task.succeeded", "T1") && task_event(e, "task.succeeded", "T2")
    })
    .await;
    let notices = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap().matches("steer [Janus 提醒]").count();
    assert_eq!(notices, 2, "each side hears about the other");
    assert_eq!(
        events(&h.app, &run).iter().filter(|e| e.kind == "task.conflict").count(),
        1,
        "one report per file and pair"
    );

    let (detail, err) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T1" })).await;
    assert!(!err && detail.contains("改过的文件：shared.txt") && detail.contains("[冲突] 和 T2"), "{detail}");
    let (peers, err) = tool(&h.app, &run, "T2", "peers", json!({})).await;
    assert!(!err && peers.contains("T1") && peers.contains("和你都改了 shared.txt"), "{peers}");
    let (full, _) = tool(&h.app, &run, "T2", "peers", json!({ "task": "T1" })).await;
    assert!(full.contains("验收标准"), "{full}");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// What tasks leave in the workspace is read back: a task editing files another task owns
/// is reported at once, a result that disagrees with its traces is flagged for the lead,
/// diffs cover each task's own files, a conclusion whose files changed afterwards is marked
/// out of date for downstream tasks, and all of it survives a restart.
#[tokio::test(flavor = "multi_thread")]
async fn traces_check_results_and_mark_stale_conclusions() {
    let h = harness(
        "traces",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "T1": ["edit:a.txt"], "T2": ["edit:a.txt"], "T3": ["edit:a.txt"], "T4": ["ok"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["a.txt"] })),
            task("T2", "m2", json!({ "owns": ["c.txt"] })),
            task("T3", "m3", json!({ "deps": ["T1", "T2"] })),
            task("T4", "m1", json!({ "deps": ["T1", "T3"] })),
        ]),
    )
    .await;
    wait_until(&h.app, &run, "T4", |e| task_event(e, "task.succeeded", "T4")).await;
    let evs = events(&h.app, &run);
    let trespass = evs
        .iter()
        .find(|e| e.kind == "task.conflict" && e.data.as_ref().unwrap()["owned"] == true)
        .expect("T2 stepped on T1's file");
    assert_eq!(
        (
            trespass.task.as_deref(),
            &trespass.data.as_ref().unwrap()["other"],
            &trespass.data.as_ref().unwrap()["paths"]
        ),
        (Some("T2"), &json!("T1"), &json!(["a.txt"]))
    );
    let gaps = |t: &str| {
        evs.iter().find(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some(t)).unwrap().data.as_ref().unwrap()
            ["traceGaps"]
            .clone()
    };
    assert_eq!(gaps("T1"), json!([]), "did what it owns");
    let g = gaps("T2").to_string();
    assert!(g.contains("以外的文件：a.txt") && g.contains("负责 c.txt"), "{g}");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[越界] T2") && board.contains("痕迹核对"), "{board}");
    // Each diff covers the task's own files; T4 changed nothing, so it has none.
    let diffs: Vec<&Event> = evs.iter().filter(|e| e.kind == "artifact.created" && e.text.contains(".diff")).collect();
    assert!(
        diffs.iter().any(|e| e.task.as_deref() == Some("T1")) && !diffs.iter().any(|e| e.task.as_deref() == Some("T4")),
        "{diffs:?}"
    );
    // T3 changed a.txt after T1 finished: T4 is told T1's conclusion may be out of date.
    let handoff = evs
        .iter()
        .find(|e| {
            e.kind == "context.handoff" && e.task.as_deref() == Some("T4") && e.data.as_ref().unwrap()["from"] == "T1"
        })
        .unwrap();
    assert_eq!(handoff.data.as_ref().unwrap()["stale"], json!([{ "path": "a.txt", "by": "T3" }]));
    assert_eq!(handoff.data.as_ref().unwrap()["files"], json!(["a.txt"]));

    // The traces outlive the process.
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    let r = app2.resume_run(&run).await.unwrap();
    while r.st.lock().unwrap().mcp_token.is_empty() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (detail, _) = tool(&app2, &run, "lead", "task_detail", json!({ "task": "T1" })).await;
    assert!(
        detail.contains("负责的文件：a.txt")
            && detail.contains("改过的文件：a.txt")
            && detail.contains("a.txt（T3 改）")
            && detail.contains("[越界] T2"),
        "{detail}"
    );
    let (detail, _) = tool(&app2, &run, "lead", "task_detail", json!({ "task": "T2" })).await;
    assert!(detail.contains("痕迹核对"), "{detail}");
    let (peers, _) = tool(&app2, &run, "T4", "peers", json!({})).await;
    assert!(peers.contains("完成后 a.txt 又被改过"), "{peers}");
    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A collaborator claims files before changing them: free files become its own until the
/// task ends, files another open task owns are refused, a verifier claims nothing, and later
/// orchestrations respect the claim.
#[tokio::test(flavor = "multi_thread")]
async fn collaborators_claim_files_before_changing_them() {
    let h = harness(
        "claim",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "T1": ["slow"], "T2": ["slow"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["src"] })),
            task("T2", "m2", json!({})),
            task("V1", "m3", json!({ "verifies": "T1" })),
        ]),
    )
    .await;
    wait_until(&h.app, &run, "both running", |e| {
        task_event(e, "attempt.started", "T1") && task_event(e, "attempt.started", "T2")
    })
    .await;
    let (text, err) = tool(&h.app, &run, "T2", "claim", json!({ "paths": ["src/x.rs"] })).await;
    assert!(err && text.contains("src/x.rs 归 T1 负责"), "{text}");
    let (text, err) = tool(&h.app, &run, "T2", "claim", json!({ "paths": ["docs", "../x"] })).await;
    assert!(err && text.contains("不在工作目录内"), "{text}");
    let (text, err) = tool(&h.app, &run, "T2", "claim", json!({ "paths": ["docs/"] })).await;
    assert!(!err && text.contains("已认领 docs"), "{text}");
    let (text, err) = tool(&h.app, &run, "T2", "claim", json!({ "paths": ["docs/a.md"] })).await;
    assert!(!err && text.contains("已经由你的子任务负责"), "{text}");
    let (text, err) = tool(&h.app, &run, "T1", "claim", json!({ "paths": ["docs/b.md"] })).await;
    assert!(err && text.contains("归 T2 负责"), "{text}");
    let (text, err) = tool(&h.app, &run, "V1", "claim", json!({ "paths": ["x"] })).await;
    assert!(err && text.contains("验证子任务"), "{text}");
    assert_eq!(events(&h.app, &run).iter().filter(|e| e.kind == "task.claimed").count(), 1);
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[认领] T2") && board.contains("docs"), "{board}");
    let (peers, _) = tool(&h.app, &run, "T1", "peers", json!({})).await;
    assert!(peers.contains("负责 docs"), "{peers}");
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("T5", "m3", json!({ "owns": ["docs/z.md"] }))] }),
    )
    .await;
    assert!(err && text.contains("T2 和 T5"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "cancel", json!({ "tasks": ["T1", "T2"], "reason": "测试结束" })).await;
    assert!(!err, "{text}");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
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
    // An empty control is refused.
    assert!(
        h.app
            .action(
                &h.app.run(&run).unwrap(),
                &json!({ "type": "control", "task": "T1", "agent": "", "id": "", "value": "x" })
            )
            .is_err()
    );
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

/// A lead whose goal never settles by itself (Claude Code whose own check does not run) is
/// asked once its session goes quiet; it confirms, Janus clears the goal, and the run delivers
/// with the lead's summary.
#[tokio::test(flavor = "multi_thread")]
async fn a_goal_that_never_settles_is_confirmed_with_the_lead() {
    let mut h = harness("goalsettle", &[("lead", false), ("m1", true)], json!({}));
    Arc::get_mut(&mut h.app).unwrap().cfg.goal_settle = Duration::from_secs(1);
    std::fs::write(h.dir.join("fake/goal-sticky"), "").unwrap();
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    wait_until(&h.app, &run, "goal active", |e| {
        e.iter().any(|e| e.kind == "goal.status" && e.data.as_ref().unwrap()["status"] == "active")
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(calls.contains("goal check") && calls.contains("goal clear"), "{calls}");
    let evs = events(&h.app, &run);
    assert!(evs.iter().any(|e| e.kind == "session.prompt" && e.data.as_ref().unwrap()["channel"] == "goal_check"));
    let delivered = evs.iter().find(|e| e.kind == "run.delivered").unwrap();
    assert!(
        !delivered.data.as_ref().unwrap().to_string().contains("GOAL_DONE"),
        "the marker is not part of the summary: {}",
        delivered.data.as_ref().unwrap()
    );
    assert!(delivered.data.as_ref().unwrap()["summary"].as_str().unwrap().starts_with("目标已完成"));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A lead that shows no sign of work is reported to the person, once, and the report is
/// withdrawn when it moves again.
#[tokio::test(flavor = "multi_thread")]
async fn a_silent_lead_is_reported_to_the_person() {
    let mut h = harness("leadstall", &[("lead", false), ("m1", true)], json!({}));
    Arc::get_mut(&mut h.app).unwrap().cfg.stall_after = Duration::from_secs(1);
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    let stalls = |e: &[Event], resolved: bool| {
        e.iter()
            .filter(|e| {
                e.kind == "lead.stalled"
                    && e.to.as_deref() == Some("user")
                    && (e.data.as_ref().unwrap()["resolved"] == true) == resolved
            })
            .count()
    };
    wait_until(&h.app, &run, "lead stall", |e| stalls(e, false) > 0).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(stalls(&events(&h.app, &run), false), 1, "one report per quiet spell");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    assert_eq!(stalls(&events(&h.app, &run), true), 1, "withdrawn once the lead moves");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Working in place: a write task runs in the workspace directory itself, even when every
/// member's mode says read-only (the lead's call, not Janus's), and its change is kept as a diff.
#[tokio::test(flavor = "multi_thread")]
async fn tasks_work_in_the_workspace_directory() {
    let h = harness("inplace", &[("lead", false), ("m1", false)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n", "written straight into the repo");
    let evs = events(&h.app, &run);
    let started = evs.iter().find(|e| e.kind == "attempt.started").unwrap();
    assert_eq!(started.data.as_ref().unwrap()["path"], json!(h.repo.canonicalize().unwrap()));
    assert!(
        evs.iter().any(|e| e.kind == "artifact.created" && e.text.contains("T1-a1.diff")),
        "the change is kept as a diff"
    );
    assert!(!has(&evs, "verification.failed"));
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
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T1" })).await;
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
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": first })).await;
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
    let (text, err) = tool(&app2, &run, "lead", "retry", json!({ "task": "T1" })).await;
    assert!(!err, "{text}");
    wait_until(&app2, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2")))
        .await;
    finish_lead(&h);
    wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// End to end through a run: a role uses library servers imported from a source and added by
/// hand, and the agent's `session/new` carries them with their values, while the run record and
/// its events hold none of the values. A skill the custom agent can't load is recorded as skipped.
#[tokio::test(flavor = "multi_thread")]
async fn library_mcp_reaches_the_session_with_its_values() {
    let h = harness("mcp-session", &[("lead", false)], json!({}));
    let upstream = h.dir.join("upstream");
    std::fs::create_dir_all(&upstream).unwrap();
    std::fs::write(
        upstream.join("mcp.json"),
        r#"{"mcpServers": {
            "fetch": {"command": "uvx", "args": ["mcp-server-fetch"], "env": {"FETCH_TOKEN": "lib-env-secret"}},
            "docs": {"type": "http", "url": "https://example.com/mcp", "headers": {"Authorization": "Bearer lib-header-secret"}}
        }}"#,
    )
    .unwrap();
    sh(&upstream, &["init", "-q"]);
    sh(&upstream, &["add", "-A"]);
    sh(&upstream, &["commit", "-q", "-m", "init"]);
    let at = crate::library::RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: String::new() };
    let source = h.app.add_source_at(at, &format!("file://{}", upstream.display())).await.unwrap();
    let paths = ["mcp.json#fetch".to_string(), "mcp.json#docs".to_string()];
    let imported = h.app.import_from_source(source["id"].as_str().unwrap(), &paths).await.unwrap();
    let local = h
        .app
        .save_mcp(None, &json!({ "name": "local", "command": "node local.js", "vars": [{ "name": "LOCAL_TOKEN", "value": "own-env-secret" }] }))
        .await
        .unwrap();
    let skill =
        h.app.create_skill(&json!({ "name": "notes", "description": "记笔记", "content": "写下来。" })).await.unwrap();
    let ids: Vec<&str> = imported.iter().chain([&local, &skill]).map(|i| i.id.as_str()).collect();
    h.app
        .save_role(&json!({ "id": "lead", "name": "lead", "connection": "fake", "lead": true, "resources": ids }))
        .unwrap();

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
    let by = |name: &str| {
        servers.iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("{name} in {servers:?}")).clone()
    };
    assert_eq!(by("local")["env"], json!([{ "name": "LOCAL_TOKEN", "value": "own-env-secret" }]));
    assert_eq!(by("fetch")["command"], "uvx");
    assert_eq!(by("fetch")["env"], json!([{ "name": "FETCH_TOKEN", "value": "lib-env-secret" }]));
    assert_eq!(by("docs")["type"], "http");
    assert_eq!(by("docs")["url"], "https://example.com/mcp");
    assert_eq!(by("docs")["headers"], json!([{ "name": "Authorization", "value": "Bearer lib-header-secret" }]));
    assert_eq!(by("janus")["type"], "http", "the board is attached next to them");

    wait_until(&h.app, &run, "the lead's session record", |e| has(e, "agent.status")).await;
    let evs = events(&h.app, &run);
    let status = evs.iter().find(|e| e.kind == "agent.status").unwrap().data.clone().unwrap();
    let via: Vec<(String, String)> = status["config"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["via"].as_str().unwrap().to_string()))
        .collect();
    assert!(via.contains(&("notes".into(), "skipped".into())), "a custom agent loads no skills: {via:?}");
    assert!(via.contains(&("docs".into(), "mcpServers".into())), "{via:?}");
    let record =
        serde_json::to_string(&h.app.db.run(&run).unwrap().unwrap()).unwrap() + &serde_json::to_string(&evs).unwrap();
    for secret in ["own-env-secret", "lib-env-secret", "lib-header-secret"] {
        assert!(!record.contains(secret), "the run record and its events hold no values: {secret}");
    }
    assert!(record.contains(&local.id), "the items the member uses are pinned in the run");
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Pasted images reach the agent: the goal's go into the lead's brief, a message's into the
/// turn it joins, each as an image block with its path named in the text. They are stored
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
            max_cost_usd: None,
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
    let created = evs.iter().find(|e| e.kind == "run.created").unwrap().data.clone().unwrap();
    let goal_image = created["images"][0]["path"].as_str().unwrap().to_string();
    assert!(goal_image.contains(&format!("/runs/{run}/attachments/")), "{goal_image}");
    assert_eq!(std::fs::read(&goal_image).unwrap(), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-goal");
    let brief = evs
        .iter()
        .find(|e| e.kind == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "context"))
        .unwrap();
    assert!(
        brief.data.as_ref().unwrap()["prompt"].as_str().unwrap().contains(&format!("[Image 1] {goal_image}")),
        "the brief names the goal's image"
    );
    assert!(
        std::fs::read_to_string(&calls).unwrap().contains(&format!("images image/png:{}", png.len())),
        "the brief carries it as an image block"
    );

    // A message with an image and no words: queued while the goal turn runs, then its own turn.
    act(
        &h,
        &run,
        json!({ "type": "message", "agent": "lead", "text": "", "images": [{ "data": shot, "name": "报错.png" }] }),
    );
    let evs = events(&h.app, &run);
    let sent = evs.iter().find(|e| e.kind == "user.message").unwrap();
    let d = sent.data.as_ref().unwrap();
    let path = d["images"][0]["path"].as_str().unwrap();
    assert!(d["text"].as_str().unwrap().contains(&format!("[Image 1] {path}")), "{d}");
    assert_eq!(d["images"][0]["name"], "报错.png");
    assert!(!serde_json::to_string(&evs).unwrap().contains(&shot), "events hold paths, not bytes");
    std::fs::write(h.dir.join("fake/lead-go"), "看过了").unwrap();
    logged("images image/png", 2).await;
    assert!(std::fs::read_to_string(&calls).unwrap().contains(&format!("images image/png:{}", shot.len())));

    // Not an image: refused, nothing recorded.
    let bad = h.app.action(
        &h.app.run(&run).unwrap(),
        &json!({ "type": "message", "agent": "lead", "text": "x", "images": [{ "data": "aGVsbG8=" }] }),
    );
    assert!(bad.is_err());
    h.app.run(&run).unwrap().st.lock().unwrap().status = RunStatus::Cancelled;
    let _ = std::fs::remove_dir_all(&h.dir);
}
