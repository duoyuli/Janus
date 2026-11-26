//! Unit tests of the orchestrator's helpers.

use super::*;
use crate::lang::Lang;

fn member_with(kind: Runtime, provider: &str, role: &str) -> Member {
    let mut m = test_run(1).team.remove(0);
    m.name = "Reviewer".into();
    m.runtime = kind;
    m.connection_name = "Codex".into();
    m.prompt = provider.into();
    m.role_prompt = role.into();
    m
}

/// Each runtime gets its member's skills its own way and the MCP servers through the run's router,
/// never with their values; a reload gets the same opening.
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
    let o = app.opening(&run, &SessionKey::Lead, &m, &caps, vec![board.clone()], Some("规则"));
    let bundle = app.dirs(&run).bundle(&m.id);
    assert_eq!(o.meta["claudeCode"]["options"]["plugins"], json!([{ "type": "local", "path": bundle }]));
    assert_eq!(o.meta["claudeCode"]["options"]["settings"]["permissions"]["allow"], json!(["mcp__janus"]));
    assert_eq!(o.meta["systemPrompt"]["append"], "规则");
    assert_eq!(o.mcp_servers, vec![app.tools_server(&run, &SessionKey::Lead), board]);
    let load = o.load_params("s1", Path::new("/w"));
    assert_eq!(
        (load["sessionId"].as_str(), &load["_meta"], &load["mcpServers"]),
        (Some("s1"), &o.meta, &json!(o.mcp_servers))
    );
    let opened = serde_json::to_string(&o.resources).unwrap() + &serde_json::to_string(&o.mcp_servers).unwrap();
    assert!(!opened.contains("Bearer t"), "values reach neither the session nor the record");

    m.runtime = Runtime::Codex;
    let o = app.opening(&run, &SessionKey::Lead, &m, &caps, vec![], Some("规则"));
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
    assert!(!task_prompt(&run, &def, "", &[], &[], None).contains("工作区共享资料"));
    run.brief = "目标用户是独立开发者".into();
    let p = task_prompt(&run, &def, "", &[], &[], None);
    assert!(p.starts_with("你的子任务 T1：t\n说明：d\n\n验收标准：\n- a"), "the task comes first: {p}");
    assert!(p.ends_with("工作区共享资料（用户维护；和子任务说明冲突时以说明为准）：\n目标用户是独立开发者"), "{p}");
    run.lang = Lang::En;
    let p = task_prompt(&run, &def, "", &[], &[], None);
    assert!(
        p.starts_with("Your subtask T1: t")
            && p.ends_with("Shared workspace context (kept by the user; your subtask's instructions win where they conflict):\n目标用户是独立开发者"),
        "{p}"
    );
}

#[test]
fn the_leads_suggestions_for_the_shared_material_are_split_off() {
    for heading in ["### **Suggested Updates to Shared Context:**", "### 建议更新共享资料："] {
        let r = LeadReport::of(&format!(
            "完成了。\n\n{heading}\n1. 定价页在 docs/pricing.md\n* 免费版限 3 个项目\n\n## 风险\n无"
        ));
        assert_eq!(r.brief, vec!["定价页在 docs/pricing.md", "免费版限 3 个项目"]);
        assert_eq!(r.summary, "完成了。\n\n## 风险\n无");
    }
    let r = LeadReport::of("完成了，没有新的事实。");
    assert!(r.brief.is_empty() && r.headline.is_empty() && r.decisions.is_empty());
    assert_eq!(r.summary, "完成了，没有新的事实。");
}

/// The conclusion and the questions for the person are their own sections; the report keeps the
/// rest whole, however long.
#[test]
fn the_leads_report_splits_its_conclusion_and_decisions_off() {
    let long = "细节。".repeat(1500);
    for (headline, decisions) in [("## Conclusion", "## Needs Your Decision:"), ("## **结论**", "### 需要你决定：")]
    {
        let r = LeadReport::of(&format!(
            "{headline}\n审计完成，确认 1 个高风险。\n\n{decisions}\n- 被忽略文件不带回算缺陷吗？\n2. 以哪条说明为准？\n\n## 改了什么\n{long}"
        ));
        assert_eq!(r.headline, "审计完成，确认 1 个高风险。");
        assert_eq!(r.decisions, vec!["被忽略文件不带回算缺陷吗？", "以哪条说明为准？"]);
        assert_eq!(r.summary, format!("## 改了什么\n{long}"));
    }
}

/// What an agent reads of a delivery later: the conclusion, the report and where the result is.
#[test]
fn a_delivery_reads_as_its_conclusion_report_and_merge() {
    let d = json!({ "headline": "做完了。", "summary": "改了 a.rs。", "merge": { "state": "merged", "note": "结果已合并到 main。" } });
    assert_eq!(delivery_text(&d), "做完了。\n\n改了 a.rs。\n\n结果已合并到 main。");
    assert_eq!(delivery_text(&json!({ "headline": "重启后交付。", "summary": "重启后交付。" })), "重启后交付。");
}

#[test]
fn standing_instructions_go_in_the_system_layer() {
    assert_eq!(Runtime::Claude.instruction_channel(), InstructionChannel::System);
    assert_eq!(Runtime::Codex.instruction_channel(), InstructionChannel::Developer);
    let m = member_with(Runtime::Codex, "用中文回答", "只报有依据的问题");
    assert_eq!(
        standing_instructions(&m, Lang::En),
        "The following standing instructions from the user apply throughout the session.\n\n\
         Agent settings (Codex):\n用中文回答\n\nRole instructions (Reviewer):\n只报有依据的问题"
    );
    assert_eq!(
        standing_instructions(&m, Lang::Zh),
        "以下是用户的长期要求，整个会话都有效。\n\nAgent 设置（Codex）：\n用中文回答\n\n数字员工的指令（Reviewer）：\n只报有依据的问题"
    );
    assert!(standing_instructions(&member_with(Runtime::Codex, " ", ""), Lang::Zh).is_empty());
}

/// Codex starts with its system layer and its permission mode's thread settings in the adapter's
/// config, next to the gateway's; Claude Code takes neither from there. A goal runs under the
/// thread's settings, so Full access starts as never asking and no sandbox.
#[test]
fn a_codex_session_starts_with_its_instructions_and_its_modes_settings() {
    let spec = |kind: Runtime, env: Vec<(String, String)>| crate::acp::RuntimeSpec {
        id: "c".into(),
        kind,
        name: "Codex".into(),
        program: "node".into(),
        args: vec![],
        env,
        pin: None,
    };
    let with_mode = |kind: Runtime, mode: &str| {
        let mut m = member_with(kind, "", "");
        m.session = json!({ "mode": mode });
        m
    };
    let gw = r#"{"model_provider":"janus"}"#.to_string();
    let codex = with_mode(Runtime::Codex, "agent-full-access");
    let out = member_spec(spec(Runtime::Codex, vec![("CODEX_CONFIG".into(), gw)]), &codex, "规则");
    let cfg: Value = serde_json::from_str(&out.env[0].1).unwrap();
    assert_eq!(cfg["model_provider"], "janus", "the gateway config stays");
    assert_eq!(cfg["developer_instructions"], "规则");
    assert_eq!(
        (cfg["approval_policy"].as_str(), cfg["sandbox_mode"].as_str()),
        (Some("never"), Some("danger-full-access"))
    );
    let cfg: Value = serde_json::from_str(
        &member_spec(spec(Runtime::Codex, vec![]), &with_mode(Runtime::Codex, "read-only"), "").env[0].1,
    )
    .unwrap();
    assert_eq!(
        cfg,
        json!({ "approval_policy": "on-request", "sandbox_mode": "read-only" }),
        "no instructions, still the mode"
    );
    let cfg: Value = serde_json::from_str(
        &member_spec(spec(Runtime::Codex, vec![]), &with_mode(Runtime::Codex, "agent"), "").env[0].1,
    )
    .unwrap();
    assert_eq!(cfg["approvals_reviewer"], "auto_review");
    // A mode Janus does not know (a permission profile), or none: the agent's own config decides.
    assert!(
        member_spec(spec(Runtime::Codex, vec![]), &with_mode(Runtime::Codex, "permission-profile:x"), "")
            .env
            .is_empty()
    );
    assert!(member_spec(spec(Runtime::Codex, vec![]), &member_with(Runtime::Codex, "", ""), "").env.is_empty());
    // Claude Code applies its mode to the whole session and takes its system layer in session/new.
    let claude = with_mode(Runtime::Claude, "bypassPermissions");
    assert!(member_spec(spec(Runtime::Claude, vec![]), &claude, "规则").env.is_empty());
}

/// Janus's rules for a collaborator lead its system layer, the person's standing rules follow,
/// and Codex gets both as developer instructions. The task prompt carries only the task.
#[test]
fn the_system_layer_holds_janus_rules_then_the_persons() {
    // The board names its tools itself (its instructions): the rules hold no tool list and no
    // skills sentence.
    let rules = collaborator_rules(Lang::En);
    assert!(
        !rules.contains("```json")
            && !rules.contains("Collaboration tools")
            && !rules.contains("janus.board_post")
            && !rules.contains("janus-skills"),
        "{rules}"
    );
    let say = Caller::Member.instructions(Lang::En);
    assert!(say.contains("- submit: ") && say.contains("- board_post: ") && say.is_ascii(), "{say}");
    assert!(rules.is_ascii(), "{rules}");
    let zh = collaborator_rules(Lang::Zh);
    assert!(zh.starts_with("你是 Janus 团队的协作者，") && !zh.contains("协作工具") && !zh.contains("```json"), "{zh}");
    // Committing, pushing and deploying follow the subtask's instructions, not a ban of Janus's own.
    assert!(rules.contains("Commit, push, or deploy only when your subtask's instructions ask for it"), "{rules}");
    assert!(!rules.contains("Do not commit"), "{rules}");
    let standing = standing_instructions(&member_with(Runtime::Codex, "", "先运行现有测试"), Lang::En);
    let text = system_layer(&[&rules, "", &standing]);
    assert!(text.starts_with("You are a collaborator on the Janus team:"));
    assert!(text.ends_with("Role instructions (Reviewer):\n先运行现有测试"));
    assert_eq!(system_layer(&[" ", ""]), "");
    let spec = || crate::acp::RuntimeSpec {
        id: "c".into(),
        kind: Runtime::Codex,
        name: "Codex".into(),
        program: "node".into(),
        args: vec![],
        env: vec![],
        pin: None,
    };
    let codex = member_spec(spec(), &member_with(Runtime::Codex, "", ""), &text);
    let cfg: Value = serde_json::from_str(&codex.env[0].1).unwrap();
    assert_eq!(cfg["developer_instructions"], text);
    assert!(
        member_spec(spec(), &member_with(Runtime::Claude, "", ""), &text).env.is_empty(),
        "Claude Code takes it in session/new"
    );

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
    let task = task_prompt(&run, &def, "", &[], &[], None);
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
    let (text, clipped) = inline_list(&items, crate::lang::Lang::Zh);
    assert!(clipped && text.contains("证据2") && !text.contains("证据3") && text.ends_with("等共 5 条"), "{text}");
    let (text, clipped) = inline_list(&items[..2], crate::lang::Lang::Zh);
    assert!(!clipped && text == "证据0；证据1");
    let (text, _) = inline_list(&items, crate::lang::Lang::En);
    assert!(text.ends_with("; 5 in all"), "{text}");
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
        kind: RunKind::Goal,
        goal: "测试目标".into(),
        cwd: PathBuf::from("/tmp"),
        worktree: None,
        team: (0..team_size)
            .map(|i| Member {
                id: format!("m{i}"),
                name: format!("成员{i}"),
                desc: String::new(),
                connection: String::new(),
                connection_name: String::new(),
                lead: i == 0,
                color: String::new(),
                init: String::new(),
                model: String::new(),
                effort: String::new(),
                session: json!({}),
                permission: String::new(),
                runtime: Runtime::Claude,
                prompt: String::new(),
                role_prompt: String::new(),
                resources: vec![],
                role_id: String::new(),
                hires: false,
            })
            .collect(),
        git: false,
        base_ref: String::new(),
        brief: String::new(),
        goal_images: vec![],
        lang: crate::lang::Lang::Zh,
        last_seq: 0,
        status: RunStatus::Planning,
        asker: Asker::Person,
    })
}

#[test]
fn the_lead_is_told_each_collaborator() {
    let mut run = test_run(2);
    run.team[1].desc = "独立检查变更，寻找风险与反例".into();
    run.team[1].connection_name = "Codex".into();
    run.team[1].permission = "Full access".into();
    let lines = team_lines(&run);
    assert_eq!(lines, "- m1：成员1 · 职责：独立检查变更，寻找风险与反例", "only who and what for");
    assert!(!lines.contains("m0"), "the lead is not its own collaborator: {lines}");
    run.lang = Lang::En;
    assert_eq!(team_lines(&run), "- m1: 成员1 · Responsibilities: 独立检查变更，寻找风险与反例");
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
    app.db.insert_run(&run.id, RunKind::Goal, &run.goal, "/tmp", &json!({}), 1).unwrap();
    let mut rx = app.tx.subscribe();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (app, run) = (app.clone(), run.clone());
            std::thread::spawn(move || {
                for _ in 0..50 {
                    app.emit(&run, ev(EventKind::BoardPosted, "m1", "进展"));
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
    app.db.insert_run("r1", RunKind::Goal, "g", "/tmp", &json!({}), 1).unwrap();
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

    let diff = working_diff(&dir, "HEAD", &[]);
    assert!(diff.contains("diff --git a/研究.md b/研究.md"), "{diff}");
    assert!(diff.contains("\n+# 笔记\n"), "{diff}");
    assert!(diff.contains("\n+内容\n"), "{diff}");
    assert!(diff.contains("# 未跟踪的新文件\n研究.md"), "{diff}");

    let scoped = working_diff(&dir, "HEAD", &["a.txt".into()]);
    assert!(!scoped.contains("研究.md"), "outside the scope: {scoped}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What only one state has goes with it: a running task's attempt, a blocked one's dependency, a
/// skipped one's upstream; whether it is succeeded now is its state, not a leftover seq.
#[test]
fn a_tasks_state_carries_its_own_data() {
    let def = TaskDef {
        id: "T1".into(),
        title: "t".into(),
        desc: "d".into(),
        deps: vec![],
        accept: vec!["a".into()],
        assignee: "m1".into(),
        max_attempts: 2,
        owns: vec![],
        verifies: None,
        continues: None,
    };
    let mut t = Task::new(def, "m1".into(), 0);
    t.set_phase(TaskPhase::Running { attempt: "a1".into() });
    assert!(t.on_attempt("a1") && !t.on_attempt("a0"));
    t.set_phase(TaskPhase::Succeeded);
    t.last_success_seq = Some(7);
    assert_eq!((t.attempt(), t.success_seq()), (None, Some(7)), "the attempt ends with the running state");
    t.set_phase(TaskPhase::Pending);
    assert_eq!(t.success_seq(), None, "reopened: not succeeded now");
    assert_eq!(t.last_success_seq, Some(7), "but where it last succeeded is kept");
    t.set_phase(TaskPhase::Blocked { by: "T0".into() });
    assert_eq!(t.blocked_by(), Some("T0"));
    t.set_phase(TaskPhase::Pending);
    assert_eq!(t.blocked_by(), None, "unblocked: nothing waits any more");
    t.set_phase(TaskPhase::SkippedByDecision { because: Some("T0".into()) });
    assert_eq!((t.status(), t.skipped_because()), (TaskStatus::SkippedByDecision, Some("T0")));
}

/// A goal with a field this build does not know is refused rather than quietly done without it:
/// a cost ceiling from an older client is not kept any more.
#[test]
fn a_goal_with_an_unknown_field_is_refused() {
    assert!(serde_json::from_value::<NewRun>(json!({ "goal": "g", "workspace": "w" })).is_ok());
    assert!(serde_json::from_value::<NewRun>(json!({ "goal": "g", "workspace": "w", "maxCostUsd": 1 })).is_err());
}

/// Each caller of the `janus` server lists its own tools and the memory tools it may use, every
/// name once, and a chat reaches none.
#[test]
fn each_caller_lists_its_own_tools_once() {
    let names = |c: Caller| -> Vec<String> {
        c.tools(crate::lang::Lang::Zh).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    let (lead, t1) = (SessionKey::Lead, SessionKey::Task("T1".into()));
    for (kind, seat) in [(RunKind::Goal, &lead), (RunKind::Goal, &t1), (RunKind::Loop, &lead), (RunKind::Dream, &lead)]
    {
        let c = Caller::of(kind, seat, false).unwrap();
        let listed = names(c);
        let unique: HashSet<&String> = listed.iter().collect();
        assert_eq!(unique.len(), listed.len(), "{c:?} lists a tool twice: {listed:?}");
        assert!(listed.iter().all(|n| !n.is_empty()), "{c:?}");
        assert_eq!(listed.contains(&"forget".to_string()), c == Caller::Consolidation, "{c:?}");
        assert!(listed.contains(&"recall".to_string()), "{c:?}");
    }
    assert_eq!(names(Caller::of(RunKind::Dream, &lead, false).unwrap()), ["recall", "remember", "forget"]);
    assert!(names(Caller::of(RunKind::Goal, &lead, false).unwrap()).contains(&"orchestrate".to_string()));
    let member = names(Caller::of(RunKind::Goal, &t1, false).unwrap());
    assert!(["board_post", "submit", "ask"].iter().all(|t| member.contains(&t.to_string())), "{member:?}");
    // A subtask whose id is `lead` is still a subtask: its seat says so, not its id.
    let named_lead = names(Caller::of(RunKind::Goal, &SessionKey::Task("lead".into()), false).unwrap());
    assert!(named_lead.contains(&"submit".to_string()) && !named_lead.contains(&"orchestrate".to_string()));
    assert!(names(Caller::of(RunKind::Loop, &lead, false).unwrap()).contains(&"dispatch".to_string()));
    assert!(Caller::of(RunKind::Chat, &lead, false).is_none(), "a chat has no tools of its own");
    assert_eq!(names(Caller::of(RunKind::Chat, &lead, true).unwrap()), ["propose_employees"], "nor team memory");
}

/// Claude Code and Codex fail an MCP tool call after 60 seconds, so `wait` never holds that
/// long, whatever it is asked, and says so: a longer one only ever came back as an error.
#[test]
fn wait_returns_before_the_agent_gives_up_on_it() {
    use crate::lang::Lang;
    for asked in [json!({}), json!({ "timeout_sec": 180 }), json!({ "timeout_sec": 120 })] {
        assert!(wait_limit(&asked) < Duration::from_secs(60), "{asked}");
    }
    assert_eq!(wait_limit(&json!({ "timeout_sec": 10 })), Duration::from_secs(10), "a shorter wait as asked");
    assert_eq!(wait_limit(&json!({ "timeout_sec": 0 })), Duration::from_secs(1));
    for lang in [Lang::Zh, Lang::En] {
        let d = LeadTool::Wait.definition(lang);
        assert!(d["inputSchema"]["properties"]["timeout_sec"]["maximum"].as_u64().unwrap() < 60);
        assert!(d["description"].as_str().unwrap().contains("50"), "{lang:?}");
    }
}

/// A lead that packs every subtask's long instructions into one `orchestrate` call can run past
/// its model's output limit, and the call is lost whole before Janus sees it: the tool says to
/// split the submission and to point to material already in the working directory, in both
/// languages.
#[test]
fn orchestrate_says_to_keep_one_call_within_one_reply() {
    use crate::lang::Lang;
    for (lang, split, point) in [
        (Lang::Zh, "分几次提交", "写路径，不要整段抄进来"),
        (Lang::En, "submit them over several calls", "by its path instead of copying it in"),
    ] {
        let d = LeadTool::Orchestrate.definition(lang);
        assert!(d["description"].as_str().unwrap().contains(split), "{lang:?}");
        let instructions =
            &d["inputSchema"]["properties"]["tasks"]["items"]["properties"]["instructions"]["description"];
        assert!(instructions.as_str().unwrap().contains(point), "{lang:?}");
    }
}

/// The board speaks the run's language: every tool description and parameter description, and
/// the board entries the lead reads, come in Chinese or English.
#[test]
fn the_board_speaks_the_runs_language() {
    use crate::lang::Lang;
    fn texts(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    match x {
                        Value::String(s) if k == "description" => out.push(s.clone()),
                        _ => texts(x, out),
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| texts(x, out)),
            _ => {}
        }
    }
    let cjk = |s: &str| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    for caller in [Caller::Lead, Caller::Member, Caller::Hiring] {
        for (lang, chinese) in [(Lang::Zh, true), (Lang::En, false)] {
            let mut all = vec![];
            texts(&json!(caller.tools(lang)), &mut all);
            assert!(!all.is_empty());
            for d in all.iter().filter(|d| !d.is_empty()) {
                assert_eq!(cjk(d), chinese, "{caller:?} {lang:?}: {d}");
            }
        }
    }
    let wait = LeadTool::Wait.definition(Lang::En);
    let about = wait["description"].as_str().unwrap();
    assert!(about.contains("which is also the most") && about.contains("pushed into your session"), "{about}");
    let amend = LeadTool::Amend.definition(Lang::Zh);
    assert!(amend["description"].as_str().unwrap().contains("独立验证子任务的判定和结论不能用它改"), "{amend}");

    let mut run = test_run(2);
    run.lang = Lang::En;
    let def = TaskDef {
        id: "T1".into(),
        title: "Parser".into(),
        desc: "d".into(),
        deps: vec![],
        accept: vec!["a".into()],
        assignee: "m1".into(),
        max_attempts: 2,
        owns: vec![],
        verifies: None,
        continues: None,
    };
    run.st.lock().unwrap().tasks.push(Task::new(def, "m1".into(), 0));
    let asked = crate::db::Event {
        run_id: run.id.clone(),
        seq: 5,
        ts: 0,
        kind: EventKind::QuestionAsked.as_str().into(),
        actor: "m1".into(),
        to: Some("lead".into()),
        task: Some("T1".into()),
        attempt: Some(1),
        text: String::new(),
        data: Some(json!({ "question": "Which name?", "options": ["a", "b"] })),
    };
    let entry = board_entry(&run, &asked).unwrap();
    assert_eq!(entry, "- [Question] T1 \"Parser\" · 成员1 waits for your answer: Which name? (options: a / b)");
    run.lang = Lang::Zh;
    assert!(
        board_entry(&run, &asked)
            .unwrap()
            .starts_with("- [提问] T1「Parser」· 成员1 等你回答：Which name?（选项：a / b）")
    );
}

/// The seat, not the id, says whose session a URL is: a subtask whose id is `lead` gets its own
/// token, which does not open the lead's seat, and the lead's does not open the subtask's.
#[test]
fn a_subtask_named_lead_has_its_own_seat() {
    let (lead, task) = (SessionKey::Lead, SessionKey::Task("lead".into()));
    let (a, b) = (session_token("secret", &lead), session_token("secret", &task));
    assert_ne!(a, b);
    assert_eq!(task.who(), "lead", "inside the run it is still the subtask called lead");
    assert_eq!(Caller::of(RunKind::Goal, &task, false), Some(Caller::Member));
    assert_eq!(Caller::of(RunKind::Goal, &lead, false), Some(Caller::Lead));
}
