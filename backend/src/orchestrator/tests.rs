//! Unit tests of the orchestrator's helpers.

use super::*;

fn member_with(kind: Runtime, provider: &str, role: &str) -> Member {
    let mut m = test_run(1).team.remove(0);
    m.name = "Reviewer".into();
    m.kind = kind;
    m.runtime_name = "Codex".into();
    m.prompt = provider.into();
    m.role_prompt = role.into();
    m
}

/// Each runtime gets its member's resources its own way, and a reload gets the same opening.
#[tokio::test]
async fn sessions_open_with_the_members_resources() {
    let dir = std::env::temp_dir().join(format!("janus-opening-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = test_app(&dir);
    let docs = json!({ "name": "docs", "transport": "http", "url": "https://d.example/mcp", "vars": [{ "name": "Authorization", "value": "Bearer t" }] });
    let server = app.save_mcp(None, &docs).await.unwrap();
    let skill = app.create_skill(&json!({ "name": "notes", "description": "d", "content": "x" })).await.unwrap();
    let run = test_run(1);
    let mut m = member_with(Runtime::Claude, "", "");
    m.resources = vec![skill, server];
    let caps = json!({ "mcpCapabilities": { "http": true } });
    let board = json!({ "type": "http", "name": "janus", "url": "http://board" });
    let o = app.opening(&run, &m, &caps, vec![board.clone()], Some("规则"));
    let bundle = app.bundle_dir(&run.id, &m.id);
    assert_eq!(o.meta["claudeCode"]["options"]["plugins"], json!([{ "type": "local", "path": bundle }]));
    assert_eq!(o.meta["claudeCode"]["options"]["settings"]["permissions"]["allow"], json!(["mcp__janus"]));
    assert_eq!(o.meta["systemPrompt"]["append"], "规则");
    let entry = json!({ "type": "http", "name": "docs", "url": "https://d.example/mcp", "headers": [{ "name": "Authorization", "value": "Bearer t" }] });
    assert_eq!(o.mcp_servers, vec![entry, board]);
    let load = o.load_params("s1", Path::new("/w"));
    assert_eq!(
        (load["sessionId"].as_str(), &load["_meta"], &load["mcpServers"]),
        (Some("s1"), &o.meta, &json!(o.mcp_servers))
    );
    assert!(!serde_json::to_string(&o.resources).unwrap().contains("Bearer t"), "values stay out of the record");

    m.kind = Runtime::Codex;
    let o = app.opening(&run, &m, &caps, vec![], Some("规则"));
    assert_eq!(o.meta, json!({ "additionalRoots": [bundle] }), "Codex has its system layer in its environment");
    let _ = std::fs::remove_dir_all(&dir);
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
        continues: None,
    };
    let m = run.team[1].clone();
    assert!(!task_prompt(&run, &m, &def, &[], &[], None).contains("Shared workspace context"));
    run.brief = "目标用户是独立开发者".into();
    let p = task_prompt(&run, &m, &def, &[], &[], None);
    assert!(
        p.contains("Shared workspace context (maintained by the user). In a conflict, follow the subtask instructions.\n目标用户是独立开发者"),
        "{p}"
    );
}

#[test]
fn the_leads_suggestions_for_the_shared_material_are_split_off() {
    for heading in ["### **Suggested Updates to Shared Context:**", "### 建议更新共享资料："] {
        let (rest, s) = split_brief_suggestions(&format!(
            "完成了。\n\n{heading}\n1. 定价页在 docs/pricing.md\n* 免费版限 3 个项目\n\n## 风险\n无"
        ));
        assert_eq!(s, vec!["定价页在 docs/pricing.md", "免费版限 3 个项目"]);
        assert_eq!(rest, "完成了。\n\n## 风险\n无");
    }
    let (rest, s) = split_brief_suggestions("完成了，没有新的事实。");
    assert!(s.is_empty());
    assert_eq!(rest, "完成了，没有新的事实。");
}

#[test]
fn standing_instructions_pick_the_system_layer_when_there_is_one() {
    assert_eq!(instruction_channel(Runtime::Claude), Channel::System);
    assert_eq!(instruction_channel(Runtime::Codex), Channel::Developer);
    assert_eq!(instruction_channel(Runtime::Custom), Channel::Prompt);
    let m = member_with(Runtime::Codex, "用中文回答", "只报有依据的问题");
    let text = standing_instructions(&m);
    assert_eq!(
        text,
        "The following standing instructions from the user apply throughout the session.\n\n\
         Agent settings (Codex):\n用中文回答\n\nRole instructions (Reviewer):\n只报有依据的问题"
    );
    assert!(standing_instructions(&member_with(Runtime::Codex, " ", "")).is_empty());
    // Only agents without a system layer get the rules in their prompt.
    assert!(prompt_instructions(&m).is_empty());
    assert!(
        prompt_instructions(&member_with(Runtime::Custom, "", "只报有依据的问题")).contains("<standing_instructions>")
    );
}

#[test]
fn developer_instructions_merge_into_codex_config() {
    let spec = |env: Vec<(String, String)>| crate::acp::RuntimeSpec {
        id: "c".into(),
        kind: Runtime::Codex,
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

/// Janus's rules for a collaborator lead its system layer, the person's standing rules follow,
/// and Codex gets both as developer instructions. The task prompt carries only the task.
#[test]
fn the_system_layer_holds_janus_rules_then_the_persons() {
    let rules = collaborator_rules(true);
    assert!(rules.contains("janus.board_post") && rules.contains("```json"), "{rules}");
    assert!(rules.is_ascii(), "{rules}");
    assert!(!collaborator_rules(false).contains("janus.board_post"), "no board, no board tools");
    let standing = standing_instructions(&member_with(Runtime::Codex, "", "先运行现有测试"));
    let text = system_layer(&rules, &standing);
    assert!(text.starts_with("You are a collaborator on the Janus team."));
    assert!(text.ends_with("Role instructions (Reviewer):\n先运行现有测试"));
    assert_eq!(system_layer(" ", ""), "");
    let spec = || crate::acp::RuntimeSpec {
        id: "c".into(),
        kind: Runtime::Codex,
        name: "Codex".into(),
        program: "node".into(),
        args: vec![],
        env: vec![],
        pin: None,
    };
    let codex = with_system(spec(), &Channel::Developer, &text);
    let cfg: Value = serde_json::from_str(&codex.env[0].1).unwrap();
    assert_eq!(cfg["developer_instructions"], text);
    assert!(with_system(spec(), &Channel::System, &text).env.is_empty(), "Claude Code takes it in session/new");

    let run = test_run(2);
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
        continues: None,
    };
    let task = task_prompt(&run, &run.team[1], &def, &[], &[], None);
    assert!(
        task.contains("你的子任务 T1") && !task.contains("```json") && !task.contains("janus.board_post"),
        "{task}"
    );
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
        worktree: None,
        team: (0..team_size)
            .map(|i| Member {
                id: format!("m{i}"),
                name: format!("成员{i}"),
                desc: String::new(),
                runtime: String::new(),
                lead: i == 0,
                color: String::new(),
                init: String::new(),
                model: String::new(),
                effort: String::new(),
                session: json!({}),
                permission: String::new(),
                kind: Runtime::Custom,
                runtime_name: String::new(),
                prompt: String::new(),
                role_prompt: String::new(),
                resources: vec![],
                role_id: String::new(),
            })
            .collect(),
        git: false,
        base_ref: String::new(),
        brief: String::new(),
        goal_images: vec![],
        last_seq: 0,
        status: RunStatus::Planning,
        cost_limit_micros: 0,
    })
}

#[test]
fn the_lead_is_told_each_collaborator() {
    let mut run = test_run(2);
    run.team[1].desc = "独立检查变更，寻找风险与反例".into();
    run.team[1].runtime_name = "Codex".into();
    run.team[1].permission = "Full access".into();
    let lines = team_lines(&run);
    assert_eq!(lines, "- m1: 成员1 · Responsibilities: 独立检查变更，寻找风险与反例", "only who and what for");
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

/// After persistence, a cost reading advances the scheduler exactly once. Polling the budget
/// through context-only updates keeps the same amount and needs no ledger rebuild.
#[test]
fn cost_budget_uses_only_committed_cost_deltas() {
    let dir = std::env::temp_dir().join(format!("janus-cost-meter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = test_app(&dir);
    let run = test_run(2);
    app.db.insert_run(&run.id, "g", "/tmp", &json!({}), 1).unwrap();
    run.cost_limit_micros.store(4_000_000, Ordering::SeqCst);
    app.emit(&run, ev("session.usage", "m1", "").data(json!({"session":"s","cost":{"currency":"USD","amount":3}})));
    let mut duplicate = app.db.events(&run.id, 0).unwrap().remove(0);
    duplicate.data = Some(json!({"session":"s","cost":{"currency":"USD","amount":8}}));
    assert!(!app.store(duplicate, Some(&run)));
    for used in 0..1000 {
        app.emit(&run, ev("session.usage", "m1", "").data(json!({"used":used,"size":1024})));
        assert!(app.check_budget(&run).is_none());
    }
    app.emit(&run, ev("session.usage", "m1", "").data(json!({"session":"s","cost":{"currency":"USD","amount":4}})));
    let ledger = app.usage(&run);
    assert_eq!(ledger.run.cost.lines[0].amount, 4.0);
    assert!(matches!(app.check_budget(&run), Some(BudgetHit::CostUsd { used: 4.0, limit: 4.0 })));
    let _ = std::fs::remove_dir_all(dir);
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
    assert!(app.store(event("written"), None));
    assert_eq!(rx.try_recv().unwrap().text, "written");
    // The same seq again: the database refuses it, and nobody hears of it.
    assert!(!app.store(event("refused"), None));
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

/// Untracked new files are part of a task's diff, content included, so a review can show them;
/// the trailing list keeps marking them as new files.
#[test]
fn untracked_files_come_with_their_content() {
    let dir = std::env::temp_dir().join(format!("janus-untracked-diff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "init"]);
    std::fs::write(dir.join("研究.md"), "# 笔记\n内容\n").unwrap();

    let diff = working_diff(&dir, &[]);
    assert!(diff.contains("diff --git a/研究.md b/研究.md"), "{diff}");
    assert!(diff.contains("\n+# 笔记\n"), "{diff}");
    assert!(diff.contains("\n+内容\n"), "{diff}");
    assert!(diff.contains("# 未跟踪的新文件\n研究.md"), "{diff}");

    let scoped = working_diff(&dir, &["a.txt".into()]);
    assert!(!scoped.contains("研究.md"), "outside the scope: {scoped}");
    std::fs::remove_dir_all(&dir).unwrap();
}
