//! Workspace Loops: a wake is one turn of the lead's session; what changes things is dispatched as
//! goal runs (once per key) through the queue; a question's answer wakes the Loop; wakes that
//! arrive while it is awake wait for the turn and merge; its clock follows `loop.scheduled`.

use super::*;

fn count(evs: &[Event], kind: &str) -> usize {
    evs.iter().filter(|e| e.kind == kind).count()
}

/// A schedule in UTC.
fn cron(expr: &str) -> Value {
    json!({ "cron": expr, "tz": "UTC" })
}

async fn new_loop(h: &Harness, prompt: &str, schedule: Value, wake_now: bool) -> (String, String) {
    let view =
        h.app.create_loop("ws", &json!({ "prompt": prompt, "schedule": schedule, "wakeNow": wake_now })).await.unwrap();
    (view["id"].as_str().unwrap().to_string(), view["run"].as_str().unwrap().to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loop_wakes_dispatches_once_per_key_and_sleeps() {
    let h = harness("loops", &[("lead", false), ("m1", false)], json!({}));
    let (id, run) = new_loop(&h, "看看有什么新 issue", cron("0 * * * *"), true).await;
    assert_eq!(serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["kind"], "loop");
    assert_eq!(serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["loopId"], id.as_str());

    // One wake: the prompt goes in as a turn from Janus, the turn ends, the Loop sleeps until its
    // schedule comes round.
    wait_until(&h.app, &run, "the first wake ends", |e| has(e, "loop.slept")).await;
    let evs = events(&h.app, &run);
    let woke = evs.iter().find(|e| e.kind.as_str() == "loop.woke").unwrap();
    assert_eq!(woke.data.as_ref().unwrap()["reason"], "manual");
    assert_eq!(woke.data.as_ref().unwrap()["n"], 1);
    let sent = evs.iter().find(|e| e.kind.as_str() == "user.message").unwrap();
    assert_eq!(sent.actor, "janus", "a wake is Janus's prompt, not the person's");
    let text = sent.data.as_ref().unwrap()["text"].as_str().unwrap();
    assert_eq!(text, "看看有什么新 issue", "a wake sends the person's prompt and nothing else");
    let slept = evs.iter().find(|e| e.kind.as_str() == "loop.slept").unwrap().data.clone().unwrap();
    let until = slept["until"].as_i64().unwrap();
    assert_eq!(until % 3_600_000, 0, "on the hour, not counted from the end of the wake");
    assert!(until > now_ms() - 60_000 && until <= now_ms() + 3_600_000);
    assert_eq!(slept["reason"], "timer");
    assert_eq!(slept["summary"], "看过了：看看有什么新 issue");
    // Its system layer says how a Loop works; the person's prompt comes only in the wake's prompt.
    let layer = evs
        .iter()
        .find(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "system"))
        .and_then(|e| e.data.as_ref().unwrap()["prompt"].as_str().map(str::to_owned))
        .unwrap();
    assert!(layer.starts_with("你是工作区「") && !layer.contains("看看有什么新 issue"), "{layer}");
    // Its tools are named once, one line each; how a tool is used is in its own description.
    for tool in ["janus.dispatch：", "janus.save_state：", "janus.next_wake：", "janus.set_name："] {
        assert_eq!(layer.matches(tool).count(), 1, "{tool}: {layer}");
    }
    assert!(!layer.contains("github:owner/repo"), "the key's example is in the tool's description: {layer}");

    // Its system layer asks it to name itself while it has no name; a session title (the fake agent
    // gives one) does not name it.
    assert!(layer.contains("你还没有名字"), "{layer}");
    assert_eq!(h.app.loop_def(&id).unwrap().name, "");
    let (said, err) = tool(&h.app, &run, "lead", "set_name", json!({ "name": "  issue 分拣\n多余的行" })).await;
    assert!(!err, "{said}");
    assert_eq!(h.app.loop_def(&id).unwrap().name, "issue 分拣");
    assert_eq!(serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["goal"], "issue 分拣");
    assert!(has(&events(&h.app, &run), "loop.named"));
    let (said, err) = tool(&h.app, &run, "lead", "set_name", json!({ "name": "别的名字" })).await;
    assert!(err && said.contains("只起一次"), "{said}");

    // Its session has the Loop's tools, not the board's.
    let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [] })).await;
    assert!(err && text.contains("没有这个工具"), "{text}");

    // A dispatch is a goal run of the workspace, from this Loop, through the queue.
    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "dispatch",
        json!({ "goal": "修复导出乱码", "key": "github:o/r#1", "source_url": "https://example.com/1" }),
    )
    .await;
    assert!(!err, "{text}");
    let made = h.app.db.dispatched_runs(&id, None, 10).unwrap();
    assert_eq!(made.len(), 1);
    let goal_run = made[0].id.clone();
    assert_eq!(made[0].origin, RunOrigin::Loop);
    assert_eq!(made[0].dispatch_key.as_deref(), Some("github:o/r#1"));
    assert_eq!(made[0].source_url.as_deref(), Some("https://example.com/1"));
    let queued = events(&h.app, &goal_run).into_iter().find(|e| e.kind.as_str() == "run.queued").unwrap();
    assert_eq!(queued.data.unwrap()["origin"]["loop"], id.as_str());
    // The same key again: nothing new, and the Loop is told which run has it.
    let (text, err) =
        tool(&h.app, &run, "lead", "dispatch", json!({ "goal": "再修一次", "key": "github:o/r#1" })).await;
    assert!(!err && text.contains(&goal_run), "{text}");
    assert_eq!(h.app.db.dispatched_runs(&id, None, 10).unwrap().len(), 1, "one run per key");
    let (text, _) = tool(&h.app, &run, "lead", "dispatches", json!({})).await;
    assert!(text.contains(&goal_run) && text.contains("github:o/r#1"), "{text}");
    let (_, err) = tool(&h.app, &run, "lead", "dispatch", json!({ "goal": "", "key": "k" })).await;
    assert!(err, "a dispatch needs a goal");
    // A question to the person does not hold the Loop; the answer wakes it.
    let (text, err) =
        tool(&h.app, &run, "lead", "ask_user", json!({ "question": "先修哪个？", "why": "只有你知道优先级" })).await;
    assert!(!err, "{text}");
    let asked = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "question.escalated").unwrap();
    let key = asked.data.unwrap()["key"].as_str().unwrap().to_string();
    let live = h.app.run(&run).unwrap();
    h.app.action(&live, &action_of(json!({ "type": "answer", "key": key, "answer": "先修导出" }))).unwrap();
    wait_until(&h.app, &run, "the answer wakes it", |e| count(e, "loop.slept") == 2).await;
    let second = events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "loop.woke").nth(1).unwrap();
    assert_eq!(second.data.unwrap()["reason"], "answer");

    // What the Loop saves comes back in its system layer.
    let (_, err) = tool(&h.app, &run, "lead", "save_state", json!({ "text": "看到 #1 为止" })).await;
    assert!(!err);
    assert!(h.app.loop_brief(&live).contains("看到 #1 为止"));

    // The person talking to it is a turn of its own, not a wake: the clock does not move.
    let before = h.app.loop_view(&h.app.loop_def(&id).unwrap()).unwrap()["status"]["nextWakeAt"].clone();
    h.app.action(&live, &action_of(json!({ "type": "message", "agent": "lead", "text": "[me] 你好" }))).unwrap();
    wait_until(&h.app, &run, "its reply", |e| {
        e.iter().any(|e| e.kind.as_str() == "agent.message" && e.text.contains("回复：你好"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let view = h.app.loop_view(&h.app.loop_def(&id).unwrap()).unwrap();
    assert_eq!(view["status"]["nextWakeAt"], before);
    assert_eq!(count(&events(&h.app, &run), "loop.woke"), 2);
    assert_eq!(
        view["status"]["dispatched"]["planning"].as_i64().or(view["status"]["dispatched"]["running"].as_i64()),
        Some(1)
    );

    // Deleting the Loop removes its session; the run it dispatched stays.
    h.app.delete_loop(&id).await.unwrap();
    assert!(h.app.db.run(&run).unwrap().is_none());
    assert!(h.app.db.run(&goal_run).unwrap().is_some());
    assert!(h.app.loops(None).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn wakes_while_awake_merge_and_a_paused_loop_only_wakes_by_hand() {
    let h = harness("loops-pending", &[("lead", false), ("m1", false)], json!({}));
    let go = h.dir.join("fake/loop-go");
    let (id, run) = new_loop(&h, "[hold] 慢慢看 [write loop-notes.md]", Value::Null, true).await;
    wait_until(&h.app, &run, "the wake holds", |e| has(e, "loop.woke")).await;
    // Without a schedule it asks to look again in ten minutes; the wakes before then keep it.
    let (text, err) =
        tool(&h.app, &run, "lead", "next_wake", json!({ "after_seconds": 600, "reason": "等 CI 跑完" })).await;
    assert!(!err, "{text}");
    // Two more wakes while it is awake: they wait for this turn and become one.
    h.app.wake_loop(&id, WakeReason::Manual).await.unwrap();
    h.app.wake_loop(&id, WakeReason::Manual).await.unwrap();
    assert_eq!(count(&events(&h.app, &run), "loop.woke"), 1);
    std::fs::write(&go, "").unwrap();
    wait_until(&h.app, &run, "the merged wake", |e| count(e, "loop.woke") == 2).await;
    std::fs::write(&go, "").unwrap();
    wait_until(&h.app, &run, "the second wake ends", |e| count(e, "loop.slept") == 2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(count(&events(&h.app, &run), "loop.woke"), 2, "the two waiting wakes were one");
    // It works in its own copy of the repository: the file it wrote is noticed there, and the
    // person's workspace never had it.
    let copy = h.app.cfg.layout.run(RunKind::Loop, &run).work;
    let touched = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "loop.touched").unwrap();
    assert_eq!(touched.data.unwrap()["files"], json!(["loop-notes.md"]));
    assert!(!h.repo.join("loop-notes.md").exists(), "the workspace is untouched");
    // Each wake starts from a clean copy: the second one wrote the file again, it did not add to it.
    assert_eq!(std::fs::read_to_string(copy.join("loop-notes.md")).unwrap(), "loop\n");
    let session_cwd =
        serde_json::to_value(h.app.db.run(&run).unwrap().unwrap()).unwrap()["cwd"].as_str().unwrap().to_string();
    assert_eq!(PathBuf::from(&session_cwd), copy.canonicalize().unwrap(), "its session works in the copy");
    // The person commits meanwhile: the next wake starts from that commit, without its old file.
    std::fs::write(h.repo.join("new.md"), "new\n").unwrap();
    for args in [&["add", "new.md"][..], &["commit", "-q", "-m", "new"]] {
        assert!(std::process::Command::new("git").args(args).current_dir(&h.repo).status().unwrap().success());
    }
    // The time it asked for outlived the merged wake; the person sees when and why.
    let slept = events(&h.app, &run).into_iter().rfind(|e| e.kind.as_str() == "loop.slept").unwrap().data.unwrap();
    assert_eq!(slept["reason"], "self");
    assert!((slept["until"].as_i64().unwrap() - now_ms() - 600_000).abs() < 60_000, "{slept}");
    let status = h.app.loop_view(&h.app.loop_def(&id).unwrap()).unwrap()["status"].clone();
    assert_eq!((status["nextWakeReason"].as_str(), status["nextWakeWhy"].as_str()), (Some("self"), Some("等 CI 跑完")));

    // Paused: its clock does not wake it, the person still can.
    h.app.pause_loop(&id, true).unwrap();
    h.app.wake_loop(&id, WakeReason::Timer).await.unwrap();
    assert_eq!(count(&events(&h.app, &run), "loop.woke"), 2);
    h.app.wake_loop(&id, WakeReason::Manual).await.unwrap();
    std::fs::write(&go, "").unwrap();
    wait_until(&h.app, &run, "a wake by hand", |e| count(e, "loop.woke") == 3).await;
    assert!(has(&events(&h.app, &run), "loop.paused"));
    wait_until(&h.app, &run, "the wake by hand ends", |e| count(e, "loop.slept") == 3).await;
    assert_eq!(std::fs::read_to_string(copy.join("loop-notes.md")).unwrap(), "loop\n", "what it left went first");
    assert!(copy.join("new.md").exists(), "at the workspace's latest commit");

    // Deleting the Loop removes its copy and the repository's entry for it.
    h.app.delete_loop(&id).await.unwrap();
    assert!(!copy.exists());
    let list = std::process::Command::new("git").args(["worktree", "list"]).current_dir(&h.repo).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&list.stdout).lines().count(), 1, "only the person's own checkout");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// After a restart, a Loop wakes when its last `loop.scheduled` said; one whose time passed while
/// Janus was down wakes once, in a session reopened from its log.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_that_missed_its_time_wakes_once_after_a_restart() {
    let h = harness("loops-restart", &[("lead", false), ("m1", false)], json!({}));
    let (id, run) = new_loop(&h, "巡检", cron("0 * * * *"), true).await;
    wait_until(&h.app, &run, "the first wake", |e| has(e, "loop.slept")).await;
    // Pretend the hour passed while the service was down.
    let live = h.app.run(&run).unwrap();
    h.app.emit(
        &live,
        ev(EventKind::LoopScheduled, "janus", "排好下次开工").data(json!({ "until": now_ms() - 1, "reason": "timer" })),
    );
    let restarted = served_app(&h.dir, |_| {});
    restarted.recover().unwrap();
    assert_eq!(status(&restarted, &run), "interrupted");
    restarted.start_loops().unwrap();
    wait_until(&restarted, &run, "it wakes once after the restart", |e| count(e, "loop.woke") == 2).await;
    let second = events(&restarted, &run).into_iter().filter(|e| e.kind.as_str() == "loop.woke").nth(1).unwrap();
    assert_eq!(second.data.unwrap()["reason"], "timer");
    wait_until(&restarted, &run, "and sleeps again", |e| count(e, "loop.slept") == 2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(count(&events(&restarted, &run), "loop.woke"), 2, "missed wakes are not made up one by one");
    assert!(restarted.loop_def(&id).is_ok());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A Loop is not named by the person: until its agent titles its session it goes by the first
/// line of its prompt, and changing the prompt changes that.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_goes_by_its_prompt_until_its_agent_names_it() {
    let h = harness("loops-title", &[("lead", false), ("m1", false)], json!({}));
    let view = h
        .app
        .create_loop("ws", &json!({ "prompt": "\n  盯一下依赖更新\n细节……", "schedule": null, "wakeNow": false }))
        .await
        .unwrap();
    assert_eq!(view["name"], "");
    assert_eq!(view["title"], "盯一下依赖更新");
    let id = view["id"].as_str().unwrap();
    let changed = h.app.update_loop(id, &json!({ "prompt": "看 CI", "schedule": null })).unwrap();
    assert_eq!(changed["title"], "看 CI");
    assert_eq!(
        serde_json::to_value(h.app.db.run(view["run"].as_str().unwrap()).unwrap().unwrap()).unwrap()["goal"],
        "看 CI"
    );
    let refused = h.app.create_loop("ws", &json!({ "prompt": "  ", "schedule": null })).await;
    assert!(refused.is_err(), "a Loop needs something to do");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The fake agent's wake prompts so far, from its log.
fn wake_prompts(h: &Harness) -> Vec<String> {
    std::fs::read_to_string(h.dir.join("fake/calls.log"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("loop prompt "))
        .map(|j| serde_json::from_str(j).unwrap())
        .collect()
}

/// The person's answers reach the agent in the wake they cause, with the questions; a question
/// the Loop asked outlives its session and a restart, and answering it brings the session back.
#[tokio::test(flavor = "multi_thread")]
async fn answers_come_in_the_next_wake_and_questions_outlive_the_session() {
    let h = harness("loops-answers", &[("lead", false), ("m1", false)], json!({}));
    let (_, run) = new_loop(&h, "看看队列", Value::Null, true).await;
    wait_until(&h.app, &run, "the first wake", |e| has(e, "loop.slept")).await;
    let ask = |q: &str| {
        let (app, run, q) = (h.app.clone(), run.clone(), q.to_string());
        async move {
            let (text, err) = tool(&app, &run, "lead", "ask_user", json!({ "question": q, "why": "只有你能定" })).await;
            assert!(!err, "{text}");
            events(&app, &run)
                .into_iter()
                .rfind(|e| e.kind.as_str() == "question.escalated")
                .and_then(|e| e.data.unwrap()["key"].as_str().map(str::to_owned))
                .unwrap()
        }
    };
    let first = ask("先修哪个？").await;
    let live = h.app.run(&run).unwrap();
    h.app
        .action(&live, &action_of(json!({ "type": "answer", "key": first, "answer": "先修导出", "note": "今天就要" })))
        .unwrap();
    wait_until(&h.app, &run, "the answer's wake", |e| count(e, "loop.slept") == 2).await;
    let prompt = wake_prompts(&h).pop().unwrap();
    assert!(prompt.starts_with("看看队列\n\n用户回答了你的问题："), "{prompt}");
    assert!(prompt.contains("问：先修哪个？\n  答：先修导出\n  备注：今天就要"), "{prompt}");

    // Asked, then the service restarts: the question still waits, and answering it reopens the
    // session and wakes the Loop with the answer.
    let second = ask("要不要发版？").await;
    h.app.runs.lock().unwrap().clear();
    let restarted = served_app(&h.dir, |_| {});
    restarted.recover().unwrap();
    restarted.start_loops().unwrap();
    let body = json!({ "type": "answer", "key": second, "answer": "发" });
    restarted.request_action(&run, &req(body)).await.unwrap();
    wait_until(&restarted, &run, "the wake after the restart", |e| count(e, "loop.slept") == 3).await;
    let prompt = wake_prompts(&h).pop().unwrap();
    assert!(prompt.contains("问：要不要发版？\n  答：发") && !prompt.contains("先修哪个"), "{prompt}");
    assert!(!has(&events(&restarted, &run), "decision.withdrawn"), "a Loop's questions are not withdrawn");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A wake whose agent exits in the middle fails on record and frees the clock: the next wake
/// works. `next_wake` belongs to a wake's turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_wake_frees_the_clock_and_next_wake_counts_from_the_turns_end() {
    let h = harness("loops-failed", &[("lead", false), ("m1", false)], json!({}));
    std::fs::write(h.dir.join("fake/loop-exit"), "").unwrap();
    let (id, run) = new_loop(&h, "巡检", cron("*/30 * * * *"), true).await;
    wait_until(&h.app, &run, "the wake fails", |e| has(e, "loop.failed")).await;
    let view = h.app.loop_view(&h.app.loop_def(&id).unwrap()).unwrap();
    assert_eq!(view["status"]["phase"], "failed");
    assert!(view["status"]["nextWakeAt"].is_i64(), "the clock goes on");
    h.app.wake_loop(&id, WakeReason::Manual).await.unwrap();
    wait_until(&h.app, &run, "the next wake works", |e| has(e, "loop.slept")).await;
    assert_eq!(h.app.loop_view(&h.app.loop_def(&id).unwrap()).unwrap()["status"]["phase"], "sleeping");

    // Outside a wake there is nothing to ask for.
    let (text, err) = tool(&h.app, &run, "lead", "next_wake", json!({ "after_seconds": 600, "reason": "r" })).await;
    assert!(err && text.contains("不是一次醒来"), "{text}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Deleting a Loop's session as a run deletes the Loop; it is not paused or stopped as a run.
#[tokio::test(flavor = "multi_thread")]
async fn a_loops_session_is_not_a_task_to_pause_stop_or_delete_alone() {
    let h = harness("loops-session", &[("lead", false), ("m1", false)], json!({}));
    let (id, run) = new_loop(&h, "巡检", Value::Null, true).await;
    wait_until(&h.app, &run, "the wake", |e| has(e, "loop.slept")).await;
    for kind in ["pause", "cancel"] {
        let refused = h.app.request_action(&run, &req(json!({ "type": kind }))).await;
        assert!(refused.is_err(), "{kind}");
    }
    assert_eq!(h.app.loop_of(&run).filter(|d| d.run == run).map(|d| d.id).as_deref(), Some(id.as_str()));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// When a Loop wakes next is on record whenever it changes, so a restart keeps a new schedule, a
/// pause and a resume.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_keeps_when_it_wakes_after_a_new_schedule_or_a_pause() {
    let h = harness("loops-schedule", &[("lead", false), ("m1", false)], json!({}));
    let (id, _) = new_loop(&h, "巡检", Value::Null, false).await;
    let next_after_restart = || {
        let restarted = served_app(&h.dir, |_| {});
        restarted.start_loops().unwrap();
        restarted.loop_view(&restarted.loop_def(&id).unwrap()).unwrap()["status"]["nextWakeAt"].clone()
    };
    assert!(next_after_restart().is_null());
    h.app.update_loop(&id, &json!({ "prompt": "巡检", "schedule": cron("*/30 * * * *") })).unwrap();
    let next = next_after_restart().as_i64().unwrap();
    assert!(next % 1_800_000 == 0 && next > now_ms() - 60_000 && next <= now_ms() + 1_800_000, "{next}");
    let refused = h.app.update_loop(&id, &json!({ "prompt": "巡检", "schedule": cron("0 9 * *") }));
    assert!(format!("{:#}", refused.unwrap_err()).contains("5 段"));
    h.app.pause_loop(&id, true).unwrap();
    assert!(next_after_restart().is_null());
    h.app.pause_loop(&id, false).unwrap();
    assert!(next_after_restart().is_i64());
    h.app.update_loop(&id, &json!({ "prompt": "巡检", "schedule": null })).unwrap();
    assert!(next_after_restart().is_null());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A run a Loop dispatched owns its goal until it ends: the Loop can pass it what it knows while
/// it works (`note_dispatch`, only to its own runs), and its end wakes the Loop with what it came
/// to. A paused Loop is not woken, and hears of it in its next wake.
#[tokio::test(flavor = "multi_thread")]
async fn a_dispatched_runs_end_wakes_its_loop_with_what_it_came_to() {
    let h = harness("loops-ended", &[("lead", false), ("m1", false)], json!({}));
    let (id, run) = new_loop(&h, "盯着导出", Value::Null, true).await;
    wait_until(&h.app, &run, "the first wake", |e| has(e, "loop.slept")).await;
    let dispatch = |key: &str| {
        let (app, run, key) = (h.app.clone(), run.clone(), key.to_string());
        async move {
            let (text, err) =
                tool(&app, &run, "lead", "dispatch", json!({ "goal": format!("处理 {key}"), "key": key })).await;
            assert!(!err, "{text}");
            app.db
                .dispatched(&app.loop_def(&app.loop_of(&run).unwrap().id).unwrap().workspace, &key)
                .unwrap()
                .unwrap()
                .0
        }
    };
    let (a, b) = (dispatch("a").await, dispatch("b").await);
    wait_until(&h.app, &a, "a's lead works", |e| {
        e.iter().any(|e| e.kind.as_str() == "session.prompt" && e.text.contains("目标提示"))
    })
    .await;

    // While it works, what the Loop knows goes to its lead, said to come from the Loop.
    let (text, err) =
        tool(&h.app, &run, "lead", "note_dispatch", json!({ "run": a, "text": "导出要支持 UTF-8 BOM" })).await;
    assert!(!err, "{text}");
    let note = events(&h.app, &a).into_iter().rfind(|e| e.kind.as_str() == "user.message").unwrap();
    assert_eq!((note.actor.as_str(), note.to.as_deref()), ("janus", Some("lead")));
    assert!(note.data.unwrap()["text"].as_str().unwrap().contains("来自工作区的值班"), "says who it is from");
    // Only to its own runs: another Loop's run, its own session, or nothing at all are refused.
    let (_, other) = new_loop(&h, "盯着别的", Value::Null, true).await;
    wait_until(&h.app, &other, "the other wake", |e| has(e, "loop.slept")).await;
    let (text, err) = tool(&h.app, &other, "lead", "note_dispatch", json!({ "run": a, "text": "冒充" })).await;
    assert!(err && text.contains("不是这个值班派出的"), "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "note_dispatch", json!({ "run": run, "text": "x" })).await;
    assert!(err && text.contains("不是这个值班派出的"), "{text}");
    let (_, err) = tool(&h.app, &run, "lead", "note_dispatch", json!({ "run": a, "text": " " })).await;
    assert!(err, "a note needs text");

    // Cancelled by the person: the Loop wakes, told it was their decision.
    h.app.request_action(&b, &req(json!({ "type": "cancel" }))).await.unwrap();
    wait_until(&h.app, &run, "the cancel's wake", |e| count(e, "loop.slept") == 2).await;
    let woke = events(&h.app, &run).into_iter().rfind(|e| e.kind.as_str() == "loop.woke").unwrap();
    assert_eq!(woke.data.unwrap()["reason"], "dispatch");
    let prompt = wake_prompts(&h).pop().unwrap();
    assert!(prompt.starts_with("盯着导出\n\n上次醒来后结束的、你派出的任务"), "{prompt}");
    assert!(prompt.contains(&format!("- {b}（key b）：被用户取消")) && !prompt.contains(&a), "{prompt}");

    // Delivered: woken with the delivery's summary; a run that ended takes no more notes.
    finish_lead(&h);
    wait_until(&h.app, &run, "the delivery's wake", |e| count(e, "loop.slept") == 3).await;
    let prompt = wake_prompts(&h).pop().unwrap();
    let delivered = events(&h.app, &a).into_iter().find(|e| e.kind.as_str() == "run.delivered").unwrap();
    let summary = delivered.data.unwrap()["summary"].as_str().unwrap().to_string();
    assert!(
        prompt.contains(&format!("- {a}（key a）：已交付，用户在收件箱里验收\n  {}", clip(summary.trim(), 600))),
        "{prompt}"
    );
    let (text, err) = tool(&h.app, &run, "lead", "note_dispatch", json!({ "run": a, "text": "还有" })).await;
    assert!(err && text.contains("不在进行中"), "{text}");

    // Paused: the end is noted but does not wake it; the next wake tells it.
    h.app.pause_loop(&id, true).unwrap();
    let c = dispatch("c").await;
    wait_until(&h.app, &run, "c's end is noted", |e| count(e, "loop.dispatch_ended") == 3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(&events(&h.app, &run), "loop.woke"), 3, "a paused Loop sleeps on");
    h.app.wake_loop(&id, WakeReason::Manual).await.unwrap();
    wait_until(&h.app, &run, "the wake by hand", |e| count(e, "loop.slept") == 4).await;
    let prompt = wake_prompts(&h).pop().unwrap();
    assert!(prompt.contains(&format!("- {c}（key c）：已交付")) && !prompt.contains(&b), "{prompt}");

    // Its system layer says the run owns its goal and its end wakes the Loop; note_dispatch's
    // description says how to pass on what it knows.
    let layer = h.app.loop_brief(&h.app.run(&run).unwrap());
    assert!(layer.contains("在结束前对自己的目标负责") && layer.contains("它结束时会唤醒你"), "{layer}");
    let note = LoopTool::NoteDispatch.definition(crate::lang::Lang::Zh);
    assert!(note["description"].as_str().unwrap().contains("代替为同一件事再派一个任务"), "{note}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A Loop made while the app is in English is told everything in English: its system layer, its
/// tools and what they return.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_in_english_is_told_everything_in_english() {
    let h = harness("loops-english", &[("lead", false)], json!({}));
    h.app.save_language(&json!({ "language": "en" })).unwrap();
    let (_, run) = new_loop(&h, "Watch the queue", Value::Null, true).await;
    wait_until(&h.app, &run, "the first wake ends", |e| has(e, "loop.slept")).await;
    let live = h.app.run(&run).unwrap();
    assert_eq!(live.lang, crate::lang::Lang::En);
    let layer = h.app.loop_brief(&live);
    assert!(layer.starts_with("You are the Loop of the workspace") && layer.contains("- janus.set_name: "), "{layer}");
    assert!(!layer.contains("值班"), "{layer}");
    let tools = Caller::Loop.tools(live.lang);
    let about =
        |name: &str| tools.iter().find(|t| t["name"] == name).unwrap()["description"].as_str().unwrap().to_string();
    assert!(about("dispatch").starts_with("Start a goal run") && about("recall").starts_with("Search the workspace"));
    let (text, err) = tool(&h.app, &run, "lead", "dispatch", json!({ "goal": "", "key": "k" })).await;
    assert!(err && text == "goal and key must both be given", "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "set_name", json!({ "name": "Queue watch" })).await;
    assert!(!err && text == "OK, your name is \"Queue watch\".", "{text}");
    let (text, err) = tool(&h.app, &run, "lead", "dispatches", json!({})).await;
    assert!(!err && text == "No runs dispatched yet.", "{text}");
    let _ = std::fs::remove_dir_all(&h.dir);
}
