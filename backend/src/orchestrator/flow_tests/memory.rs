//! A workspace's team memory: members keep notes through the `janus` tools, every session of the
//! workspace is handed their index, a note never crosses to another workspace, only a
//! consolidation removes notes, and enough delivered runs start one.

use super::*;
use crate::db::{MemoryKind, MemoryWrite};

fn note(h: &Harness, workspace: &str, id: &str, title: &str) {
    let write = MemoryWrite {
        kind: MemoryKind::Fact,
        title,
        hook: "一句话",
        body: "正文",
        run: None,
        who: "user",
        author: "你",
    };
    h.app.db.insert_memory(id, workspace, &write, now_ms()).unwrap();
}

fn remember(title: &str) -> Value {
    json!({ "kind": "howto", "title": title, "hook": "跑测试之前先装依赖", "body": "先运行 just setup，再运行 just test。" })
}

/// The system layer a session of `task` (empty: the lead) of `run` was opened with.
fn layer(h: &Harness, run: &str, task: Option<&str>) -> String {
    events(&h.app, run)
        .iter()
        .find(|e| {
            e.kind.as_str() == "session.prompt"
                && e.task.as_deref() == task
                && e.data.as_ref().is_some_and(|d| d["channel"] == "system")
        })
        .and_then(|e| e.data.as_ref().unwrap()["prompt"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn members_keep_notes_for_later_runs_and_never_for_another_workspace() {
    let h = harness("memory", &[("lead", false), ("m1", true)], json!({ "T1": ["hold:go"] }));
    // A note of another workspace, which no session of `ws` may read or change.
    note(&h, "other", "m-other", "别的工作区的事");
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 starts", |e| task_event(e, "attempt.started", "T1")).await;

    // The lead and a collaborator both keep notes; each says who wrote it in which run.
    let (said, err) = tool(&h.app, &run, "lead", "remember", remember("怎么跑测试")).await;
    assert!(!err, "{said}");
    let (said, err) = tool(
        &h.app,
        &run,
        "T1",
        "remember",
        json!({ "kind": "pitfall", "title": "端口被占用", "hook": "7878 常被占用", "body": "测试用随机端口。" }),
    )
    .await;
    assert!(!err, "{said}");
    let notes = h.app.db.memories("ws").unwrap();
    assert_eq!(notes.len(), 2);
    let lead_note = notes.iter().find(|m| m.title == "怎么跑测试").unwrap();
    assert_eq!((lead_note.who.as_str(), lead_note.run.as_deref()), ("lead", Some(run.as_str())));
    let member_note = notes.iter().find(|m| m.title == "端口被占用").unwrap();
    assert_eq!((member_note.who.as_str(), member_note.kind), ("m1", MemoryKind::Pitfall));
    assert_eq!(member_note.author, h.app.run(&run).unwrap().member("m1").unwrap().name);
    assert_eq!(events(&h.app, &run).iter().filter(|e| e.kind.as_str() == "memory.saved").count(), 2);

    // Updating by id changes that note instead of adding one.
    let (said, err) =
        tool(&h.app, &run, "T1", "remember", json!({ "id": member_note.id, "kind": "pitfall", "title": "端口被占用", "hook": "别用 7878", "body": "测试一律用空闲端口。" })).await;
    assert!(!err, "{said}");
    assert_eq!(h.app.db.memories("ws").unwrap().len(), 2);
    assert_eq!(h.app.db.memory(&member_note.id).unwrap().unwrap().hook, "别用 7878");

    // Reading: by id, by what one wants to know (ranked, any word counts, Chinese without spaces
    // too), and the list of notes.
    let (text, err) = tool(&h.app, &run, "T1", "recall", json!({ "id": lead_note.id })).await;
    assert!(!err && text.contains("just setup"), "{text}");
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "空闲 端口" })).await;
    assert!(text.contains("端口被占用") && !text.contains("怎么跑测试"), "{text}");
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "测试的端口被占用" })).await;
    assert!(
        text.find("端口被占用").unwrap() < text.find("怎么跑测试").unwrap_or(usize::MAX),
        "the better match first: {text}"
    );
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "部署 回滚" })).await;
    assert!(text.contains("没有和「部署 回滚」相关的笔记"), "{text}");
    let (text, _) = tool(&h.app, &run, "T1", "recall", json!({})).await;
    assert!(text.starts_with("共 2 条笔记") && text.contains(&lead_note.id) && text.contains("端口被占用"), "{text}");

    // What a note must not be: empty, of no known kind, or carrying a credential.
    let (text, err) =
        tool(&h.app, &run, "lead", "remember", json!({ "kind": "fact", "title": "", "hook": "h", "body": "b" })).await;
    assert!(err && text.contains("title"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "remember", json!({ "kind": "rumor", "title": "t", "hook": "h", "body": "b" }))
            .await;
    assert!(err && text.contains("kind"), "{text}");
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "remember",
        json!({ "kind": "fact", "title": "令牌", "hook": "h", "body": "GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789" }),
    )
    .await;
    assert!(err && text.contains("凭据"), "{text}");
    assert_eq!(h.app.db.memories("ws").unwrap().len(), 2);

    // Another workspace's note is out of reach: not read, not overwritten.
    let (text, err) = tool(&h.app, &run, "lead", "recall", json!({ "id": "m-other" })).await;
    assert!(err && !text.contains("别的工作区的事"), "{text}");
    let mut over = remember("改写别人的笔记");
    over["id"] = json!("m-other");
    let (text, err) = tool(&h.app, &run, "T1", "remember", over).await;
    assert!(err, "{text}");
    assert_eq!(h.app.db.memory("m-other").unwrap().unwrap().title, "别的工作区的事");
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "别的工作区" })).await;
    assert!(!text.contains("m-other"), "{text}");

    // Only a consolidation removes notes: neither the lead nor a collaborator can.
    for who in ["lead", "T1"] {
        let (text, err) = tool(&h.app, &run, who, "forget", json!({ "id": lead_note.id, "reason": "x" })).await;
        assert!(err && text.contains("没有这个工具"), "{who}: {text}");
    }
    assert_eq!(h.app.db.memories("ws").unwrap().len(), 2);

    // The sessions of the workspace are told the notes are there, not handed them; the tools are
    // named once among the collaboration tools, and their descriptions say how to use them.
    let tool_line = "- janus.recall、janus.remember：";
    let lead_layer = layer(&h, &run, None);
    assert!(lead_layer.contains("团队记忆：") && lead_layer.matches("janus.remember").count() == 1, "{lead_layer}");
    assert!(lead_layer.contains(tool_line), "{lead_layer}");
    let task_layer = layer(&h, &run, Some("T1"));
    assert!(task_layer.contains("团队记忆：") && task_layer.matches("janus.recall").count() == 1, "{task_layer}");
    assert!(task_layer.contains(tool_line), "{task_layer}");
    let live = h.app.run(&run).unwrap();
    let section = h.app.memory_section(&live, MemoryAccess::Write);
    assert!(section.contains("有 2 条之前的任务留下的笔记") && section.contains("笔记不是规则"), "{section}");
    assert!(
        !section.contains(&lead_note.id) && !section.contains("端口被占用"),
        "no note in the instructions: {section}"
    );

    std::fs::write(h.dir.join("fake/go"), "").unwrap();
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    // A note from a cancelled run is not handed to later runs. The fake lead works on a goal until
    // `lead-go` appears: left in place, the next run would deliver at once and could no longer be
    // stopped.
    std::fs::remove_file(h.dir.join("fake/lead-go")).unwrap();
    let cancelled = h
        .app
        .create_run(NewRun { goal: "试试另一条路".into(), workspace: "ws".into(), ..Default::default() })
        .await
        .unwrap();
    let (said, err) = tool(&h.app, &cancelled, "lead", "remember", remember("被取消的结论")).await;
    assert!(!err, "{said}");
    act(&h, &cancelled, json!({ "type": "cancel" }));
    wait_until(&h.app, &cancelled, "cancel", |_| status(&h.app, &cancelled) == "cancelled").await;
    assert!(h.app.memory_section(&live, MemoryAccess::Write).contains("有 2 条"), "the cancelled run's not counted");
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "被取消的结论" })).await;
    assert!(!text.contains("被取消的结论 ·"), "not found: {text}");
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({})).await;
    assert!(!text.contains("被取消的结论") && text.contains("怎么跑测试"), "nor listed: {text}");
    assert!(
        h.app
            .memory_section(&live, MemoryAccess::Consolidate)
            .contains("被取消的结论 — 跑测试之前先装依赖（来自被取消的任务）")
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_consolidation_merges_and_prunes_notes_in_one_turn() {
    let h = harness("memory-dream", &[("lead", false), ("m1", true)], json!({}));
    let err = h.app.consolidate("ws").await.unwrap_err();
    assert!(format!("{err:?}").contains("还没有团队记忆"), "nothing to consolidate yet");
    note(&h, "ws", "m-a", "测试命令");
    note(&h, "ws", "m-b", "测试命令（重复）");
    note(&h, "ws", "m-c", "上周换了数据库");

    let dream = h.app.consolidate("ws").await.unwrap();
    let row = serde_json::to_value(h.app.db.run(&dream).unwrap().unwrap()).unwrap();
    assert_eq!((row["kind"].as_str(), row["config"]["workspace"]["id"].as_str()), (Some("dream"), Some("ws")));
    let again = h.app.consolidate("ws").await.unwrap_err();
    assert!(matches!(again, Refused::Conflict(_)), "one consolidation at a time");
    assert_eq!(h.app.memories("ws").unwrap()["consolidation"]["running"], dream.as_str());

    // Its session has the memory tools and `forget`, and no board.
    let (text, err) = tool(&h.app, &dream, "lead", "orchestrate", json!({ "tasks": [] })).await;
    assert!(err && text.contains("没有这个工具"), "{text}");
    let (said, err) = tool(&h.app, &dream, "lead", "forget", json!({ "id": "m-b" })).await;
    assert!(err && said.contains("reason"), "a removal says why: {said}");
    let (said, err) = tool(&h.app, &dream, "lead", "forget", json!({ "id": "m-b", "reason": "和 m-a 重复" })).await;
    assert!(!err, "{said}");
    let mut merged = remember("测试命令");
    merged["id"] = json!("m-c");
    merged["title"] = json!("2026-11-16 换成了 Postgres");
    let (said, err) = tool(&h.app, &dream, "lead", "remember", merged).await;
    assert!(!err, "{said}");

    // Its layer lists every note; its prompt says it should consolidate.
    // The session records its layer once it is open, which a slow machine may not have done yet.
    wait_until(&h.app, &dream, "the consolidation's layer", |_| !layer(&h, &dream, None).is_empty()).await;
    let layer = layer(&h, &dream, None);
    assert!(
        layer.contains("你在整理这个工作区的团队记忆") && layer.contains("m-a") && layer.contains("m-c"),
        "{layer}"
    );
    let sent = events(&h.app, &dream).into_iter().find(|e| e.kind.as_str() == "user.message").unwrap();
    assert!(sent.data.unwrap()["text"].as_str().unwrap().starts_with("现在整理团队记忆。"));

    // One turn: on record with what changed, delivered, its session closed.
    std::fs::write(h.dir.join("fake/dream-go"), "合并了 1 条，改正了 1 条\n- m-b 并入 m-a").unwrap();
    wait_until(&h.app, &dream, "the consolidation ends", |e| has(e, "memory.consolidated")).await;
    let done =
        events(&h.app, &dream).into_iter().find(|e| e.kind.as_str() == "memory.consolidated").unwrap().data.unwrap();
    assert_eq!((done["saved"].as_u64(), done["forgotten"].as_u64()), (Some(1), Some(1)));
    assert_eq!(done["summary"], "合并了 1 条，改正了 1 条");
    wait_until(&h.app, &dream, "it is delivered", |_| status(&h.app, &dream) == "delivered").await;
    let ids: Vec<String> = h.app.db.memories("ws").unwrap().into_iter().map(|m| m.id).collect();
    assert!(ids.contains(&"m-a".to_string()) && !ids.contains(&"m-b".to_string()) && ids.len() == 2);
    let view = h.app.memories("ws").unwrap();
    assert!(view["consolidation"]["running"].is_null());
    assert_eq!(view["consolidation"]["last"]["run"], dream.as_str());
    // It is one turn: it does not go on as a conversation.
    assert!(h.app.resume_run(&dream).await.is_err());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Five delivered runs since the last consolidation, and that a day ago or more, start one.
#[tokio::test(flavor = "multi_thread")]
async fn enough_delivered_runs_start_a_consolidation() {
    let h = harness("memory-due", &[("lead", false), ("m1", true)], json!({}));
    note(&h, "ws", "m-a", "测试命令");
    let delivered = |i: usize| {
        let id = format!("r-done-{i}");
        let config = json!({ "workspace": { "id": "ws", "name": "ws" } });
        h.app.db.insert_run(&id, RunKind::Goal, "目标", "/tmp", &config, now_ms() - 1000).unwrap();
        h.app.db.set_run_status(&id, RunStatus::Delivered).unwrap();
        id
    };
    let dreams = |h: &Harness| h.app.memories("ws").unwrap()["consolidation"].clone();
    for i in 0..4 {
        delivered(i);
    }
    h.app.run_delivered("r-done-3", RunStatus::Delivered);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(dreams(&h)["running"].is_null(), "four runs are not enough");
    let fifth = delivered(4);
    h.app.run_delivered(&fifth, RunStatus::Delivered);
    for _ in 0..100 {
        if !dreams(&h)["running"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let dream = dreams(&h)["running"].as_str().expect("the fifth starts one").to_string();
    let sent = loop {
        if let Some(e) = events(&h.app, &dream).into_iter().find(|e| e.kind.as_str() == "user.message") {
            break e;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(sent.data.unwrap()["text"].as_str().unwrap().contains("r-done-4"), "it is told the runs since");
    std::fs::write(h.dir.join("fake/dream-go"), "没有要改的").unwrap();
    wait_until(&h.app, &dream, "it ends", |_| status(&h.app, &dream) == "delivered").await;
    // Another delivered run soon after does not start another: the last one is less than a day old.
    let sixth = delivered(5);
    h.app.run_delivered(&sixth, RunStatus::Delivered);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(dreams(&h)["running"].is_null());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The person's changes: a note they rewrite is theirs from then on; one they delete is gone.
#[tokio::test(flavor = "multi_thread")]
async fn the_person_edits_and_deletes_notes() {
    let h = harness("memory-person", &[("lead", false)], json!({}));
    note(&h, "ws", "m-a", "测试命令");
    let edited = h.app.edit_memory("m-a", &json!({ "body": "用 just test", "kind": "howto" })).unwrap().unwrap();
    assert_eq!((edited.body.as_str(), edited.kind, edited.who.as_str()), ("用 just test", MemoryKind::HowTo, "user"));
    assert_eq!(edited.title, "测试命令", "what is not sent stays");
    assert!(h.app.edit_memory("m-a", &json!({ "title": "" })).is_err());
    assert!(h.app.edit_memory("m-a", &json!({ "kind": "rumor" })).is_err());
    assert!(h.app.edit_memory("missing", &json!({})).unwrap().is_none());
    assert!(h.app.delete_memory("m-a").unwrap());
    assert!(!h.app.delete_memory("m-a").unwrap());
    assert!(h.app.db.memories("ws").unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A run in English is told about its team memory in English, and so are its tools' answers; a
/// consolidation in English is asked in English.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_in_english_hears_about_its_notes_in_english() {
    let h = harness("memory-english", &[("lead", false), ("m1", true)], json!({ "T1": ["hold:go"] }));
    h.app.save_language(&json!({ "language": "en" })).unwrap();
    note(&h, "ws", "m-a", "测试命令");
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 starts", |e| task_event(e, "attempt.started", "T1")).await;
    let live = h.app.run(&run).unwrap();
    let section = h.app.memory_section(&live, MemoryAccess::Write);
    assert!(
        section.contains("Team memory: this workspace has 1 notes") && section.contains("Notes are not rules"),
        "{section}"
    );
    let (text, _) = tool(&h.app, &run, "lead", "recall", json!({ "query": "部署" })).await;
    assert!(text.starts_with("No notes match"), "{text}");
    let (text, _) = tool(&h.app, &run, "T1", "recall", json!({})).await;
    assert!(text.starts_with("1 notes, most recently changed first:"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "remember", json!({ "kind": "rumor", "title": "t", "hook": "h", "body": "b" }))
            .await;
    assert!(err && text == "kind is one of fact, decision, pitfall or howto", "{text}");
    let recall = MemoryTool::Recall.definition(live.lang);
    assert!(recall["description"].as_str().unwrap().contains("before you start"), "{recall}");

    std::fs::write(h.dir.join("fake/go"), "").unwrap();
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let dream = h.app.consolidate("ws").await.unwrap();
    wait_until(&h.app, &dream, "the consolidation's layer", |_| !layer(&h, &dream, None).is_empty()).await;
    let brief = layer(&h, &dream, None);
    assert!(brief.contains("You are consolidating") && brief.contains("Note index (1):"), "{brief}");
    let sent = events(&h.app, &dream).into_iter().find(|e| e.kind.as_str() == "user.message").unwrap();
    assert!(sent.data.unwrap()["text"].as_str().unwrap().starts_with("Consolidate the team memory now."));
    std::fs::write(h.dir.join("fake/dream-go"), "Nothing to change").unwrap();
    wait_until(&h.app, &dream, "the consolidation ends", |e| has(e, "memory.consolidated")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
