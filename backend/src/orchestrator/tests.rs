//! Unit tests of the orchestrator's helpers.

use super::*;

fn member_with(kind: &str, provider: &str, role: &str) -> Member {
    let mut m = test_run(1).team.remove(0);
    m.name = "Reviewer".into();
    m.kind = kind.into();
    m.runtime_name = "Codex".into();
    m.prompt = provider.into();
    m.role_prompt = role.into();
    m
}

#[test]
fn mcp_servers_use_declared_keys_and_keep_values_out_of_the_snapshot() {
    let mut m = member_with("claude", "", "");
    m.mcp = vec![
        json!({ "name": "local", "command": "node", "args": ["local.js"], "envKeys": ["LOCAL_TOKEN"] }),
        json!({ "name": "docs", "transport": "http", "url": "https://example.com/mcp", "headerKeys": ["Authorization"] }),
    ];
    m.mcp_env = json!({ "local": { "LOCAL_TOKEN": "env-secret", "UNDECLARED": "x" }, "docs": { "LEFTOVER": "y" } });
    m.mcp_headers = json!({ "docs": { "Authorization": "Bearer header-secret", "UNDECLARED": "z" } });

    let all = mcp_servers(&m, true);
    assert_eq!(all[0]["name"], "local");
    assert_eq!(all[0]["env"], json!([{ "name": "LOCAL_TOKEN", "value": "env-secret" }]), "only declared keys");
    assert_eq!(all[1]["type"], "http");
    assert_eq!(all[1]["url"], "https://example.com/mcp");
    assert_eq!(all[1]["headers"], json!([{ "name": "Authorization", "value": "Bearer header-secret" }]));

    // An Agent without HTTP MCP support still opens with the stdio servers.
    let stdio_only = mcp_servers(&m, false);
    assert_eq!(stdio_only.len(), 1);
    assert_eq!(stdio_only[0]["name"], "local");

    let snapshot = serde_json::to_string(&m).unwrap();
    assert!(
        !snapshot.contains("env-secret") && !snapshot.contains("header-secret"),
        "values stay out of the run record: {snapshot}"
    );
    assert!(snapshot.contains("LOCAL_TOKEN") && snapshot.contains("Authorization"), "definitions and key names remain");
}

#[test]
fn task_prompts_carry_the_shared_material() {
    let mut run = test_run(2);
    let def = TaskDef {
        id: "T1".into(),
        title: "t".into(),
        desc: "d".into(),
        deps: vec![],
        accept: vec!["a".into()],
        assignee: run.team[1].id.clone(),
        max_attempts: 2,
        owns: vec![],
        verifies: None,
    };
    let m = run.team[1].clone();
    assert!(
        !task_prompt(&run, &m, &def, &[], &[], false, None).contains("工作区共享资料"),
        "nothing when there is none"
    );
    run.brief = "目标用户是独立开发者".into();
    let p = task_prompt(&run, &m, &def, &[], &[], false, None);
    assert!(
        p.contains("工作区共享资料（用户维护的长期背景，与任务说明冲突时以任务说明为准）：\n目标用户是独立开发者"),
        "{p}"
    );
}

#[test]
fn the_leads_suggestions_for_the_shared_material_are_split_off() {
    let (rest, s) = split_brief_suggestions(
        "完成了。\n\n### 建议更新共享资料：\n1. 定价页在 docs/pricing.md\n* 免费版限 3 个项目\n\n## 风险\n无",
    );
    assert_eq!(s, vec!["定价页在 docs/pricing.md", "免费版限 3 个项目"]);
    assert_eq!(rest, "完成了。\n\n## 风险\n无");
    let (rest, s) = split_brief_suggestions("完成了，没有新的事实。");
    assert!(s.is_empty());
    assert_eq!(rest, "完成了，没有新的事实。");
}

#[test]
fn standing_instructions_pick_the_system_layer_when_there_is_one() {
    assert_eq!(instruction_channel("claude"), Channel::System);
    assert_eq!(instruction_channel("codex"), Channel::Developer);
    assert_eq!(instruction_channel("pi"), Channel::Prompt);
    assert_eq!(instruction_channel("grok"), Channel::Prompt);
    let m = member_with("codex", "用中文回答", "只报有依据的问题");
    let text = standing_instructions(&m);
    assert!(text.find("用中文回答").unwrap() < text.find("只报有依据的问题").unwrap(), "provider first, then role");
    assert!(standing_instructions(&member_with("codex", " ", "")).is_empty());
    // Only agents without a system layer get the rules in their prompt.
    assert!(prompt_instructions(&m).is_empty());
    assert!(prompt_instructions(&member_with("grok", "", "只报有依据的问题")).contains("<standing_instructions>"));
    assert!(prompt_instructions(&member_with("pi", "", "只报有依据的问题")).contains("<standing_instructions>"));
}

#[test]
fn developer_instructions_merge_into_codex_config() {
    let spec = |env: Vec<(String, String)>| crate::acp::RuntimeSpec {
        id: "c".into(),
        kind: "codex".into(),
        name: "Codex".into(),
        program: "node".into(),
        args: vec![],
        env,
        pin: None,
    };
    let gw = r#"{"model_provider":"janus"}"#.to_string();
    let out = with_developer_instructions(spec(vec![("CODEX_CONFIG".into(), gw)]), "规则");
    let cfg: Value = serde_json::from_str(&out.env[0].1).unwrap();
    assert_eq!(cfg["model_provider"], "janus", "the gateway config stays");
    assert_eq!(cfg["developer_instructions"], "规则");
    let out = with_developer_instructions(spec(vec![]), "规则");
    assert_eq!(out.env, vec![("CODEX_CONFIG".to_string(), r#"{"developer_instructions":"规则"}"#.to_string())]);
    assert!(with_developer_instructions(spec(vec![]), "").env.is_empty());
}

/// A reply that streamed in pieces is the whole reply again, and only the last one.
#[test]
fn the_lead_summary_joins_the_last_reply_back_together() {
    let e = |seq: i64, kind: &str, text: &str| Event {
        run_id: "r".into(),
        seq,
        ts: 0,
        kind: kind.into(),
        actor: "lead".into(),
        to: None,
        task: None,
        attempt: None,
        text: text.into(),
        data: Some(json!({ "text": text })),
    };
    let evs = vec![
        e(1, "agent.message", "先看看代码。"),
        e(2, "tool.call", "Read"),
        e(3, "agent.message", "已完成：\n```console\n$ python3"),
        e(4, "agent.thought", "想一想"),
        e(5, "agent.message", " -m unittest\nOK\n```\n"),
    ];
    // A prompt from Janus in between starts a new reply.
    let mut asked = evs.clone();
    asked.push(Event { actor: "janus".into(), to: Some("lead".into()), ..e(6, "session.prompt", "[Janus] …") });
    asked.push(e(7, "agent.message", "GOAL_DONE\n好了"));
    assert_eq!(last_reply(&asked).as_deref(), Some("GOAL_DONE\n好了"));
    assert_eq!(last_reply(&evs).as_deref(), Some("已完成：\n```console\n$ python3 -m unittest\nOK\n```"));
    assert!(last_reply(&evs[..2]).is_some_and(|t| t == "先看看代码。"));
    assert!(last_reply(&[]).is_none());
}

/// Claude Code is told to allow Janus's board tools, and only those, without asking.
#[test]
fn claude_sessions_allow_only_the_board_tools() {
    let meta = session_meta();
    assert_eq!(meta["claudeCode"]["options"]["settings"]["permissions"]["allow"], json!(["mcp__janus"]));
}

/// Both official adapters report /goal under `_meta.jetbrains.air`.
#[test]
fn goal_status_comes_from_the_air_metadata() {
    let dir = std::env::temp_dir().join(format!("janus-goal-meta-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = test_app(&dir);
    let run = test_run(1);
    let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
    let (mut out, mut flushed, mut turn) = (String::new(), 0, crate::acp_view::Turn::default());
    for status in ["active", "complete"] {
        let u = json!({ "sessionUpdate": "session_info_update", "_meta": { "jetbrains": { "air": { "goal": { "objective": "x", "status": status } } } } });
        app.on_update(&run, &u, &mut out, &mut flushed, &mut turn, &ctx);
        assert_eq!(run.st.lock().unwrap().lead.goal_active, status == "active");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn long_results_are_clipped_on_the_board() {
    let items: Vec<String> = (0..5).map(|i| format!("证据{i}")).collect();
    let (text, clipped) = inline_list(&items);
    assert!(clipped && text.contains("证据2") && !text.contains("证据3") && text.ends_with("等共 5 条"), "{text}");
    let (text, clipped) = inline_list(&items[..2]);
    assert!(!clipped && text == "证据0；证据1");
    let edit = json!({ "kind": "edit", "locations": [{ "path": "/w/src/a.rs" }, { "path": "/w/src/a.rs" }] });
    assert_eq!(edited_paths(&edit, Path::new("/w")), ["src/a.rs"]);
    assert!(edited_paths(&json!({ "kind": "read", "locations": [{ "path": "/w/a" }] }), Path::new("/w")).is_empty());
    assert_eq!(
        edited_paths(&json!({ "kind": "edit", "rawInput": { "file_path": "/w/b.md" } }), Path::new("/w")),
        ["b.md"]
    );
}

fn test_run(team_size: usize) -> Run {
    Run::new(RunRecord {
        id: "r-test".into(),
        goal: "测试目标".into(),
        cwd: PathBuf::from("/tmp"),
        team: (0..team_size)
            .map(|i| Member {
                id: format!("m{i}"),
                name: format!("成员{i}"),
                role: "测试".into(),
                desc: String::new(),
                runtime: String::new(),
                lead: i == 0,
                color: String::new(),
                init: String::new(),
                model: String::new(),
                effort: String::new(),
                session: json!({}),
                permission: String::new(),
                model_label: String::new(),
                kind: String::new(),
                runtime_name: String::new(),
                prompt: String::new(),
                role_prompt: String::new(),
                skills: vec![],
                mcp: vec![],
                mcp_refs: vec![],
                role_id: String::new(),
                mcp_env: Value::Null,
                mcp_headers: Value::Null,
            })
            .collect(),
        git: false,
        base_ref: String::new(),
        brief: String::new(),
        goal_images: vec![],
        last_seq: 0,
        status: RunStatus::Planning,
    })
}

#[test]
fn the_lead_is_told_each_collaborator() {
    let mut run = test_run(2);
    run.team[1].desc = "独立检查变更，寻找风险与反例".into();
    let lines = team_lines(&run);
    assert!(lines.contains("- m1：成员1（测试）· 职责：独立检查变更，寻找风险与反例 · Agent "), "{lines}");
    assert!(!lines.contains("m0"), "the lead is not its own collaborator: {lines}");
}

/// Events written at the same time from several threads reach the stream in seq order;
/// the app treats a gap as missed events.
#[test]
fn concurrent_events_are_broadcast_in_seq_order() {
    let dir = std::env::temp_dir().join(format!("janus-unit-emit-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = test_app(&dir);
    let run = Arc::new(test_run(2));
    app.db.insert_run(&run.id, &run.goal, "/tmp", &json!({}), 1).unwrap();
    let mut rx = app.tx.subscribe();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (app, run) = (app.clone(), run.clone());
            std::thread::spawn(move || {
                for _ in 0..50 {
                    app.emit(&run, ev("board.posted", "m1", "进展"));
                }
            })
        })
        .collect();
    threads.into_iter().for_each(|t| t.join().unwrap());
    let seqs: Vec<i64> = std::iter::from_fn(|| rx.try_recv().ok()).map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=400).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An event the database refused is not broadcast: what the app sees is always in the log.
#[test]
fn an_event_that_was_not_written_is_not_broadcast() {
    let dir = std::env::temp_dir().join(format!("janus-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = test_app(&dir);
    app.db.insert_run("r1", "g", "/tmp", &json!({}), 1).unwrap();
    let mut rx = app.tx.subscribe();
    let event = |text: &str| Event {
        run_id: "r1".into(),
        seq: 1,
        ts: 1,
        kind: "run.created".into(),
        actor: "user".into(),
        to: None,
        task: None,
        attempt: None,
        text: text.into(),
        data: None,
    };
    assert!(app.store(event("written")));
    assert_eq!(rx.try_recv().unwrap().text, "written");
    // The same seq again: the database refuses it, and nobody hears of it.
    assert!(!app.store(event("refused")));
    assert!(rx.try_recv().is_err(), "a refused event is not broadcast");
    assert_eq!(app.db.events("r1", 0).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// One driver per run: a second is refused while the first is alive, and its end can be awaited
/// instead of polled.
#[tokio::test]
async fn a_run_has_one_driver_and_its_end_can_be_awaited() {
    let driving = Arc::new(Driving::default());
    assert!(driving.claim());
    assert!(!driving.claim(), "a second driver is refused while one is alive");
    let waiter = {
        let driving = driving.clone();
        tokio::spawn(async move { driving.idle().await })
    };
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished(), "still driving");
    driving.set(false);
    tokio::time::timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap();
    assert!(driving.claim(), "the run can be driven again");
}

/// A cancelled run is cancelled by its status alone, and takes no new pending decisions.
#[test]
fn cancelling_is_the_status() {
    let dir = std::env::temp_dir().join(format!("janus-cancel-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = test_app(&dir);
    let run = test_run(1);
    let owner = Owner { session: SessionKey::Lead, attempt: 0 };
    assert!(app.add_pending(&run, "k1", owner.clone(), Ask::Question).is_ok());
    run.st.lock().unwrap().status = RunStatus::Cancelled;
    assert!(run.st.lock().unwrap().cancelled());
    assert!(app.add_pending(&run, "k2", owner, Ask::Question).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
