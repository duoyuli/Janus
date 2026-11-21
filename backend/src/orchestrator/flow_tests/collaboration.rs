//! End-to-end tests for the multi-agent collaboration improvements (`C-01`..`C-13`): event-driven
//! `wait` + live task pulse, verifier `verdict` + targeted context, structured `handoff`, multi-step
//! loop detection, Lead edit conflict/stale tracking, warm session continuation (`continues` /
//! `retry` with `session: "continue"`, rework on a verification's findings), role `slots` + concurrency cap, observational token budget, collaborator
//! `request_split`, and abnormal-session usage capture.

use super::*;

/// Extract the `(token, who)` credential handed to a session over ACP (`mcp [...]` in `calls.log`),
/// never reading `RunState::mcp_token` directly (`AGENTS.md` Rule 1).
async fn client_mcp_cred(h: &Harness, expected_who: &str) -> (String, String) {
    let calls = h.dir.join("fake/calls.log");
    for _ in 0..200 {
        let content = std::fs::read_to_string(&calls).unwrap_or_default();
        for line in content.lines().rev() {
            let Some(rest) = line.strip_prefix("mcp ") else { continue };
            let Ok(servers) = serde_json::from_str::<Vec<Value>>(rest) else { continue };
            for s in servers {
                let Some(url) = s["url"].as_str() else { continue };
                let parts: Vec<&str> = url.split('/').collect();
                if parts.len() >= 3 {
                    let who = parts[parts.len() - 1];
                    let token = parts[parts.len() - 2];
                    if who == expected_who {
                        return (token.to_string(), who.to_string());
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no MCP URL for {expected_who} in {}", std::fs::read_to_string(&calls).unwrap_or_default());
}

async fn call_with_cred(
    app: &Arc<App>,
    run: &str,
    token: &str,
    who: &str,
    name: &str,
    args: Value,
) -> Result<(String, bool)> {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    let r = app.mcp(run, token, who, req).await?.unwrap();
    Ok((r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string(), r["result"]["isError"] == true))
}

#[tokio::test(flavor = "multi_thread")]
async fn wait_wakes_immediately_on_board_event_and_reports_live_pulse_on_timeout() {
    let h = harness("collab-wait", &[("lead", false), ("m1", true)], json!({ "T1": ["busy-tool"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running tool", |e| {
        e.iter().any(|e| e.kind == "tool.started" && e.task.as_deref() == Some("T1"))
    })
    .await;
    // A start alone does not end the wait: it comes back at the timeout, with the live pulse.
    let (first, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(first.contains("[开始] T1") && first.contains("子任务实时进展"), "{first}");

    // Short timeout while `busy-tool` is still in progress returns live pulse instead of blind silence.
    let (pulse, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(
        pulse.contains("子任务实时进展") && pulse.contains("T1") && pulse.contains("cargo test --workspace"),
        "{pulse}"
    );

    // A 120s wait wakes via `Notify` as soon as `T1` finishes (~1.5s), not after 120s.
    let began = tokio::time::Instant::now();
    let (woke, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 120 })).await;
    assert!(began.elapsed() < Duration::from_secs(10), "took {:?}", began.elapsed());
    assert!(woke.contains("[完成] T1"), "{woke}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Entries that only inform ride along with the next one the lead has to see: a start does not
/// cost the lead a turn of its own, a finished task or a question still ends the wait at once.
#[tokio::test(flavor = "multi_thread")]
async fn wait_holds_informational_entries_until_something_needs_the_lead() {
    let h = harness(
        "collab-wait-hold",
        &[("lead", false), ("m1", true), ("m2", true)],
        json!({ "T1": ["busy-tool"], "T2": ["ask", "ok"] }),
    );
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m2", json!({ "deps": ["T1"] }))])).await;
    wait_until(&h.app, &run, "T1 started", |e| task_event(e, "attempt.started", "T1")).await;

    let began = tokio::time::Instant::now();
    let (done, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 120 })).await;
    assert!(began.elapsed() < Duration::from_secs(10), "took {:?}", began.elapsed());
    assert!(done.contains("[开始] T1") && done.contains("[完成] T1"), "the start comes with the finish: {done}");
    assert!(!done.contains("子任务实时进展"), "{done}");

    let began = tokio::time::Instant::now();
    let (asked, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 120 })).await;
    assert!(began.elapsed() < Duration::from_secs(10), "took {:?}", began.elapsed());
    assert!(asked.contains("[提问] T2"), "{asked}");
    let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "alpha" })).await;
    assert!(!err, "{text}");

    wait_until(&h.app, &run, "T2 finished", |e| task_event(e, "task.succeeded", "T2")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Editing one file again and again, or editing and testing in turn, with a different diff or
/// output each time is work going on, not a loop.
#[tokio::test(flavor = "multi_thread")]
async fn steady_edits_of_one_file_are_not_reported_as_a_loop() {
    let h = harness("collab-steady-edits", &[("lead", false), ("m1", true)], json!({ "T1": ["progress"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 finished", |e| task_event(e, "task.succeeded", "T1")).await;
    let evs = events(&h.app, &run);
    assert!(evs.iter().filter(|e| e.kind == "tool.call" && e.task.as_deref() == Some("T1")).count() >= 15);
    assert!(!evs.iter().any(|e| e.kind == "task.stalled"), "no stall for steady work");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn verifier_receives_targeted_evidence_and_drives_warm_fix_and_reverify() {
    let h = harness(
        "collab-verify-continue",
        &[("lead", false), ("m1", true), ("m2", false)],
        json!({
            "T1": ["handoff"],
            "TD": ["ok"],
            "V1": ["reject"],
            "T2": ["ok"],
            "V2": ["approve"]
        }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["api.rs"], "accept": ["parse 接口完整"] })),
            task("TD", "m1", json!({ "deps": ["T1"], "accept": ["调用端对接完成"] })),
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["独立核验 parse 边界"] })),
            task("SHIP", "m1", json!({ "deps": ["V1"] }))
        ]),
    )
    .await;

    wait_until(&h.app, &run, "V1 and TD finished", |e| {
        task_event(e, "task.succeeded", "V1") && task_event(e, "task.succeeded", "TD")
    })
    .await;

    // Rule 1 negative auth check: collaborator's real MCP URL token cannot call lead tools or spoof `who="lead"`.
    let (t1_token, t1_who) = client_mcp_cred(&h, "T1").await;
    assert_eq!(t1_who, "T1");
    let (denied, is_err) =
        call_with_cred(&h.app, &run, &t1_token, "T1", "orchestrate", json!({ "tasks": [] })).await.unwrap();
    assert!(is_err && denied.contains("没有这个工具"), "{denied}");
    assert!(
        call_with_cred(&h.app, &run, &t1_token, "lead", "team", json!({})).await.is_err(),
        "forging who=lead with collaborator token must fail authentication"
    );

    let evs = events(&h.app, &run);
    let prompt_of = |id: &str| -> String {
        evs.iter()
            .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id))
            .and_then(|e| e.data.as_ref()?["prompt"].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    // C-03: Downstream `TD` gets structured `handoff` and full `risks` items.
    let td_prompt = prompt_of("TD");
    assert!(td_prompt.contains("交接说明：接口签名：parse(input: &str)"), "{td_prompt}");
    assert!(td_prompt.contains("上游风险：空字符串边界尚未压测"), "{td_prompt}");

    let v1_done = evs.iter().find(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("V1")).unwrap();
    let live = h.app.run(&run).unwrap();
    let guidance = board_entry(&live, v1_done).unwrap();
    assert!(guidance.contains("下游任务 SHIP") && guidance.contains("janus.cancel"), "{guidance}");
    let (note, invalid) = tool(&h.app, &run, "lead", "cancel", json!({"tasks":["V1"],"reason":"过时"})).await;
    assert!(invalid && note.contains("原判定保留"), "{note}");
    let planned = evs.iter().find(|e| e.kind == "orchestration.accepted").unwrap().data.as_ref().unwrap()["plannedSeq"]
        .as_i64()
        .unwrap();
    assert_eq!(live.st.lock().unwrap().tasks.iter().find(|t| t.def.id == "SHIP").unwrap().planned_seq, planned);
    let (cancelled, err) = tool(&h.app, &run, "lead", "cancel", json!({"tasks":["SHIP"],"reason":"先修复"})).await;
    assert!(!err && cancelled.contains("已取消：SHIP"), "{cancelled}");
    assert!(task_event(&events(&h.app, &run), "task.skipped_by_decision", "SHIP"));
    // C-07: Verifier `V1` gets scoped diff artifact, edited files, weak-evidence reminders and risks.
    let v1_prompt = prompt_of("V1");
    assert!(v1_prompt.contains("专属 diff 产物：") && v1_prompt.contains("T1-a1.diff"), "{v1_prompt}");
    assert!(v1_prompt.contains("执行者实际改动的文件：api.rs"), "{v1_prompt}");
    assert!(v1_prompt.contains("执行者证据较弱的条目") && v1_prompt.contains("推理判断"), "{v1_prompt}");
    assert!(v1_prompt.contains("执行者上报的风险") && v1_prompt.contains("空字符串边界尚未压测"), "{v1_prompt}");

    // Negative validation on `continues`: wrong assignee or combining with `verifies` is rejected.
    let (bad1, err1) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("BAD1", "m2", json!({ "continues": "T1" }))] }),
    )
    .await;
    assert!(err1 && bad1.contains("同一个协作者"), "{bad1}");
    let (bad2, err2) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("BAD2", "m2", json!({ "verifies": "T1", "continues": "V1" }))] }),
    )
    .await;
    assert!(err2 && bad2.contains("验证子任务"), "{bad2}");

    // C-08: Warm fix `T2` continues `T1`'s ACP session, then `V2` approves `T2`.
    let (ok_text, err3) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
            task("T2", "m1", json!({ "continues": "T1", "deps": ["V1", "TD"], "owns": ["api.rs"], "accept": ["空输入返回错误"] })),
            task("V2", "m2", json!({ "verifies": "T2", "accept": ["复测空输入通过"] }))
        ] }),
    )
    .await;
    assert!(!err3, "{ok_text}");
    wait_until(&h.app, &run, "V2 succeeded", |e| task_event(e, "task.succeeded", "V2")).await;

    let evs = events(&h.app, &run);
    let t1_sid = evs
        .iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1"))
        .and_then(|e| e.data.as_ref()?["session"].as_str().map(str::to_owned))
        .unwrap();
    let t2_started = evs
        .iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2"))
        .and_then(|e| e.data.clone())
        .unwrap();
    assert_eq!(t2_started["session"].as_str(), Some(t1_sid.as_str()), "T2 reused T1's sessionId");
    assert_eq!(t2_started["continuation"]["loaded"], true, "{t2_started}");
    assert!(
        t2_started["prompt"].as_str().unwrap().starts_with("你在子任务 T1 的原会话中继续工作")
            && t2_started["prompt"].as_str().unwrap().contains("空输入返回错误"),
        "{}",
        t2_started["prompt"]
    );
    let handoff = evs
        .iter()
        .find(|e| {
            e.kind == "context.handoff"
                && e.task.as_deref() == Some("T2")
                && e.data.as_ref().is_some_and(|d| d["from"] == "V1")
        })
        .unwrap()
        .data
        .as_ref()
        .unwrap();
    assert_eq!(handoff["verdict"], "rejected");
    assert_eq!(handoff["checks"][0]["passed"], false);
    let prompt = t2_started["prompt"].as_str().unwrap();
    assert!(prompt.contains("rejected") && prompt.contains(handoff["checks"][0]["evidence"].as_str().unwrap()));
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    assert!(calls.contains(&format!("load {t1_sid}")), "{calls}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let d = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap().data.unwrap();
    let t2_acc = d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == "T2").unwrap();
    assert_eq!(t2_acc["verifications"][0]["verdict"], "approved");
    assert_eq!(t2_acc["verifications"][0]["passed"], true);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Recover both event orders. A task planned before a rejection stays held even when its
/// accepted event follows the rejection. A repair actually planned later must still run.
#[tokio::test(flavor = "multi_thread")]
async fn recovery_uses_the_recorded_plan_boundary_and_preserves_legacy_holds() {
    let h = harness("recover-plan-order", &[("lead", false), ("m1", true), ("m2", false)], json!({}));
    let original = start(&h, json!([task("SEED", "m1", json!({}))])).await;
    finish_lead(&h);
    wait_until(&h.app, &original, "seed delivered", |e| has(e, "run.delivered")).await;
    let row = h.app.db.run(&original).unwrap().unwrap();
    let id = "replay-order";
    let mut config = row["config"].clone();
    config["costLimitUsd"] = json!(4.0);
    h.app.db.insert_run(id, RunKind::Goal, "恢复交错序列", row["cwd"].as_str().unwrap(), &config, now_ms()).unwrap();
    let def = |id, agent, deps: &[&str]| json!({"id":id,"title":id,"agent":agent,"deps":deps,"accept":["完成"]});
    let mut verifier = def("V1", "m2", &["T1"]);
    verifier["verifies"] = json!("T1");
    let facts = [
        ("orchestration.accepted", None, json!({"tasks":[def("T1","m1",&[]),verifier],"plannedSeq":0})),
        ("task.succeeded", Some("T1"), json!({"summary":"初版"})),
        ("task.succeeded", Some("V1"), json!({"summary":"独立核验","verdict":"rejected"})),
        ("orchestration.accepted", None, json!({"tasks":[def("BEFORE","m1",&["V1"])],"plannedSeq":2})),
        ("orchestration.accepted", None, json!({"tasks":[def("LEGACY","m1",&["V1"])]})),
        ("task.blocked", Some("LEGACY"), json!({"dependency":"V1","verdict":"rejected"})),
        ("orchestration.accepted", None, json!({"tasks":[def("FIX","m1",&["V1"])],"plannedSeq":6})),
        ("session.usage", Some("T1"), json!({"session":"warm","cost":{"amount":2,"currency":"USD"}})),
        ("session.usage", Some("FIX"), json!({"session":"warm","cost":{"amount":3,"currency":"USD"}})),
    ];
    for (i, (kind, task, data)) in facts.into_iter().enumerate() {
        h.app
            .db
            .insert_event(&crate::usage::event_from_json(
                id,
                &json!({"seq":i+1,"ts":now_ms(),"type":kind,"task":task,"actor":"m1","data":data}),
            ))
            .unwrap();
    }
    h.app.recover().unwrap();
    let live = h.app.resume_run(id).await.unwrap();
    wait_until(&h.app, id, "repair completed", |e| task_event(e, "task.succeeded", "FIX")).await;
    {
        let st = live.st.lock().unwrap();
        for id in ["BEFORE", "LEGACY"] {
            assert_eq!(st.tasks.iter().find(|t| t.def.id == id).unwrap().status(), TaskStatus::Blocked);
        }
    }
    assert_eq!(live.costs.lock().unwrap().usd, Some(3.0));
    let log = events(&h.app, id);
    assert!(
        !has(&log, "run.budget_exhausted")
            && !task_event(&log, "attempt.started", "BEFORE")
            && !task_event(&log, "attempt.started", "LEGACY")
    );
    wait_until(&h.app, id, "recovered delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_continuation_falls_back_cleanly_on_high_context_or_no_load() {
    let h = harness(
        "collab-continue-fallback",
        &[("lead", false), ("m1", true)],
        json!({ "T1": ["high-ctx"], "T2": ["unmet", "unmet", "ok"] }),
    );
    let run = start(
        &h,
        json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "continues": "T1", "max_attempts": 1 }))]),
    )
    .await;
    wait_until(&h.app, &run, "T2 failed verification", |e| task_event(e, "task.failed", "T2")).await;

    let evs = events(&h.app, &run);
    let t1_sid = evs
        .iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1"))
        .and_then(|e| e.data.as_ref()?["session"].as_str().map(str::to_owned))
        .unwrap();
    let t2_a1 = evs
        .iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2") && e.attempt == Some(1))
        .and_then(|e| e.data.clone())
        .unwrap();
    assert_ne!(t2_a1["session"].as_str(), Some(t1_sid.as_str()), "80% context skips session/load");
    assert_eq!(t2_a1["continuation"]["fallback"], "context_over_75pct", "{t2_a1}");

    // Disable `loadSession` and ask `retry` for `session: "continue"`: falls back cleanly to `session/new`.
    std::fs::write(h.dir.join("fake/no-load"), "").unwrap();
    let (_, err) =
        tool(&h.app, &run, "lead", "retry", json!({ "task": "T2", "instructions": "换个做法", "session": "continue" }))
            .await;
    assert!(!err);
    wait_until(&h.app, &run, "T2 succeeded on retry", |e| task_event(e, "task.succeeded", "T2")).await;
    let t2_a2 = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2") && e.attempt == Some(2))
        .and_then(|e| e.data)
        .unwrap();
    assert_eq!(t2_a2["continuation"]["fallback"], "unsupported", "{t2_a2}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn alternating_tool_loops_and_lead_edit_conflicts_reach_the_board() {
    let h = harness(
        "collab-loop-lead-edits",
        &[("lead", true), ("m1", true), ("m2", true)],
        json!({ "T1": ["handoff"], "T2": ["loop2"], "T3": ["ask", "ok"], "T4": ["ok"], "T5": ["ok"] }),
    );
    let run = start(
        &h,
        json!([task("T1", "m1", json!({ "owns": ["api.rs"] })), task("T2", "m2", json!({ "owns": ["a.rs"] }))]),
    )
    .await;
    wait_until(&h.app, &run, "T1 and T2 finished", |e| {
        task_event(e, "task.succeeded", "T1") && task_event(e, "task.succeeded", "T2")
    })
    .await;

    // C-05: `T2` ran `Edit a.rs` / `cargo test` 4 times in alternation -> `period: 2` loop stall.
    let stalled = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "task.stalled" && e.task.as_deref() == Some("T2"))
        .and_then(|e| e.data)
        .unwrap();
    assert_eq!((stalled["reason"].as_str(), stalled["period"].as_u64()), (Some("loop"), Some(2)), "{stalled}");

    // C-06: Start `T3` owning `owned.rs` (waiting on `ask`), then have the Lead edit `owned.rs`
    // (active task's file -> `byLead` conflict), `api.rs` (finished `T1`'s file -> `负责人 改` stale
    // note) and `later.rs` (owned by `T5`, which has not started: settling it first is no conflict).
    let (_, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
            task("T3", "m1", json!({ "owns": ["owned.rs"] })),
            task("T5", "m2", json!({ "owns": ["later.rs"], "deps": ["T3"] }))] }),
    )
    .await;
    assert!(!err);
    wait_until(&h.app, &run, "T3 asks", |e| task_event(e, "question.asked", "T3")).await;

    let r = h.app.run(&run).unwrap();
    let seq = h.app.emit(
        &r,
        ev("tool.call", "lead", "Edit owned.rs & api.rs")
            .data(json!({ "tool": "edit", "status": "completed", "paths": ["owned.rs", "api.rs", "later.rs"] })),
    );
    h.app.record_lead_edits(&r, &["owned.rs".into(), "api.rs".into(), "later.rs".into()], seq);
    assert!(
        !events(&h.app, &run).iter().any(|e| e.kind == "task.conflict" && e.task.as_deref() == Some("T5")),
        "a task that has not started is not in conflict with the lead's edit"
    );

    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("连续 4 轮重复调用序列"), "{board}");
    assert!(board.contains("你（负责人）修改了正在进行的子任务 T3"), "{board}");

    let (detail, _) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T3" })).await;
    assert!(detail.contains("负责人改了这个子任务的文件：owned.rs"), "{detail}");

    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T3", "text": "alpha" })).await;
    assert!(!err);
    wait_until(&h.app, &run, "T3 and T5 done", |e| {
        task_event(e, "task.succeeded", "T3") && task_event(e, "task.succeeded", "T5")
    })
    .await;

    let (_, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T4", "m2", json!({ "deps": ["T1"] }))] }))
            .await;
    assert!(!err);
    wait_until(&h.app, &run, "T4 done", |e| task_event(e, "task.succeeded", "T4")).await;
    let t4_prompt = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T4"))
        .and_then(|e| e.data?["prompt"].as_str().map(str::to_owned))
        .unwrap();
    assert!(t4_prompt.contains("api.rs（负责人 改）"), "{t4_prompt}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn role_slots_and_concurrency_cap_schedule_parallel_work_safely() {
    let mut h = harness(
        "collab-slots-cap",
        &[("lead", false), ("dev", true), ("qa", true)],
        json!({ "T1": ["busy-tool"], "T2": ["busy-tool"], "T3": ["ok"] }),
    );
    // Cap concurrent attempts at 2 while 3 tasks (`dev-1`, `dev-2`, `qa`) are ready at once.
    Arc::get_mut(&mut h.app).unwrap().cfg.max_concurrent_attempts = 2;
    let ws = h
        .app
        .save_workspace(&json!({
            "id": "ws",
            "name": "ws",
            "path": h.repo.canonicalize().unwrap(),
            "lead": "lead",
            "members": [
                { "role": "lead" },
                { "role": "dev", "slots": 2 },
                { "role": "qa", "slots": 1 }
            ]
        }))
        .unwrap();
    assert_eq!(ws["members"][1]["slots"], 2);

    let run = start(
        &h,
        json!([
            task("T1", "dev-1", json!({ "owns": ["m1.rs"] })),
            task("T2", "dev-2", json!({ "owns": ["m2.rs"] })),
            task("T3", "qa", json!({ "owns": ["m3.rs"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "two slots running", |e| {
        task_event(e, "tool.started", "T1") && task_event(e, "tool.started", "T2")
    })
    .await;
    // While `T1` and `T2` occupy both concurrency slots, `T3` stays `Pending`.
    assert!(
        !task_event(&events(&h.app, &run), "attempt.started", "T3"),
        "T3 must wait for a concurrency slot to free up"
    );

    wait_until(&h.app, &run, "all three finish", |e| {
        task_event(e, "task.succeeded", "T1")
            && task_event(e, "task.succeeded", "T2")
            && task_event(e, "task.succeeded", "T3")
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;

    let stats = h.app.role_stats().unwrap();
    assert_eq!(stats["roles"]["dev"]["tasks"], 2, "{stats}");
    assert_eq!(stats["roles"]["dev"]["succeeded"], 2, "{stats}");
    assert_eq!(stats["roles"]["qa"]["tasks"], 1, "{stats}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn token_reference_keeps_usage_visible_and_downstream_work_running() {
    let h =
        harness("collab-token-budget", &[("lead", false), ("m1", true)], json!({ "T1": ["handoff"], "T2": ["ok"] }));
    let run = h
        .app
        .create_run(NewRun {
            goal: "受控预算".into(),
            workspace: "ws".into(),
            max_tokens: Some(1_000),
            ..Default::default()
        })
        .await
        .unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (_, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [
            task("T1", "m1", json!({ "owns": ["api.rs"] })),
            task("T2", "m1", json!({ "deps": ["T1"] }))
        ] }),
    )
    .await;
    assert!(!err);

    wait_until(&h.app, &run, "T2 finishes without extending any budget", |e| task_event(e, "task.succeeded", "T2"))
        .await;
    let live = h.app.run(&run).unwrap();
    assert!(h.app.usage(&live).run.tokens.total.unwrap() >= 1_200);
    assert_eq!(h.app.db.run(&run).unwrap().unwrap()["config"]["tokenLimit"], 1_000);
    let deadline = live.deadline_ms.load(Ordering::SeqCst);
    assert!(h.app.action(&live, &json!({ "type": "extendBudget", "tokens": 5000 })).is_err());
    assert_eq!(live.deadline_ms.load(Ordering::SeqCst), deadline);
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let log = events(&h.app, &run);
    assert!(!has(&log, "run.budget_exhausted") && !has(&log, "run.budget_extended"));
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn collaborator_split_request_wakes_lead_and_refuses_verifiers() {
    let h = harness(
        "collab-split",
        &[("lead", false), ("m1", true), ("m2", false)],
        json!({ "T1": ["ask", "ok"], "V1": ["ask", "approve"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["src"] })),
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["核验通过"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "T1 asks", |e| task_event(e, "question.asked", "T1")).await;

    let (t1_token, _) = client_mcp_cred(&h, "T1").await;
    // Negative path check on `request_split`.
    let (bad_path, is_err) = call_with_cred(
        &h.app,
        &run,
        &t1_token,
        "T1",
        "request_split",
        json!({ "reason": "太大", "subtasks": [{ "title": "越界", "owns": ["../outside"] }] }),
    )
    .await
    .unwrap();
    assert!(is_err && bad_path.contains("不在工作目录内"), "{bad_path}");

    // Valid split proposal from `T1` emits `task.split_requested` and appears in `janus.wait`.
    let (ok_split, is_err) = call_with_cred(
        &h.app,
        &run,
        &t1_token,
        "T1",
        "request_split",
        json!({
            "reason": "涉及解析器与代码生成两个独立子系统",
            "completed": "已完成 AST 定义",
            "subtasks": [
                { "title": "实现解析器", "owns": ["src/parser.rs"], "accept": ["单测通过"] },
                { "title": "实现代码生成", "owns": ["src/codegen.rs"], "accept": ["单测通过"] }
            ]
        }),
    )
    .await
    .unwrap();
    assert!(!is_err, "{ok_split}");

    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[请求拆分] T1") && board.contains("实现解析器"), "{board}");

    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T1", "text": "先完成基础部分" })).await;
    assert!(!err);
    wait_until(&h.app, &run, "V1 asks", |e| task_event(e, "question.asked", "V1")).await;

    // Rule 1 negative check: verifier `V1` calling `request_split` with its own credential is refused.
    let (v1_token, _) = client_mcp_cred(&h, "V1").await;
    let (refused, is_err) = call_with_cred(
        &h.app,
        &run,
        &v1_token,
        "V1",
        "request_split",
        json!({ "reason": "想拆", "subtasks": [{ "title": "子项" }] }),
    )
    .await
    .unwrap();
    assert!(is_err && refused.contains("验证子任务"), "{refused}");

    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "V1", "text": "继续核验" })).await;
    assert!(!err);
    wait_until(&h.app, &run, "V1 done", |e| task_event(e, "task.succeeded", "V1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn abnormal_attempts_preserve_cancelled_and_isolated_native_usage() {
    // Part 1: `stream_prompt` timeout sends `session/cancel` and still captures `prompt_usage` from the cancelled turn.
    let h = harness(
        "collab-abnormal-usage",
        &[("lead", false), ("m1", true)],
        json!({ "T_TIMEOUT": ["cancel-usage"], "T_CRASH": ["fail"] }),
    );
    let run = start(&h, json!([task("T_CRASH", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T_CRASH failed", |e| task_event(e, "task.failed", "T_CRASH")).await;

    // Part 2: Verify `recover_native_usage` reads Janus's isolated `<agents_dir>/fake/home/projects/.../<sid>.jsonl`
    // (the agent's `CLAUDE_CONFIG_DIR` is `<agents_dir>/fake/home`)
    // when an attempt crashed without reporting ACP usage, and never double-counts once reported.
    // Janus recovers usage as the attempt ends, while its session is the agent's current one: the last attempt's.
    let crash_sid = events(&h.app, &run)
        .into_iter()
        .rfind(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T_CRASH"))
        .and_then(|e| e.data?["session"].as_str().map(str::to_owned))
        .unwrap();
    let proj_dir = h.app.cfg.layout.agent_config("fake").join("projects/demo");
    std::fs::create_dir_all(&proj_dir).unwrap();
    let r = h.app.run(&run).unwrap();
    let member = r.member("m1").unwrap().clone();
    let ctx = Ctx { agent: "m1", task: Some("T_CRASH"), attempt: Some(1), goal: false };
    let path = proj_dir.join(format!("{crash_sid}.jsonl"));
    let prior = json!({"message":{"id":"prior","usage":{"input_tokens":80}}}).to_string();
    std::fs::write(&path, format!("{prior}\n")).unwrap();
    h.app.show(&r, ("session.usage", "上一轮".into(), json!({"totalTokens":80, "session":crash_sid})), &ctx);
    let turn = h.app.native_turn(&r, &member, &crash_sid, None).unwrap();
    std::fs::write(
        &path,
        format!(
            "{prior}\n{}\n{}\n",
            json!({ "type": "assistant", "message": { "id": "m1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 300, "output_tokens": 50, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0 } } }),
            json!({ "type": "assistant", "message": { "id": "m1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 300, "output_tokens": 80, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0 } } })
        ),
    )
    .unwrap();
    h.app.recover_native_usage(&r, &turn, &ctx);
    // Calling a second time is a no-op: the ledger already has the recovered turn for this session.
    h.app.recover_native_usage(&r, &turn, &ctx);
    let ledger = h.app.usage(&r);
    assert_eq!(ledger.task("T_CRASH").unwrap().tokens.total, Some(560));

    // Exercise `stream_prompt` short timeout directly against `cancel-usage` to verify post-cancel `prompt_usage`.
    let spec = h.app.spec_for("fake").unwrap();
    let owner = Owner { session: SessionKey::task("T_TIMEOUT"), attempt: 1 };
    let log = h.app.dirs(&r).log("T_TIMEOUT-a1.log");
    let (conn, mut rx, mut ctl) = h.app.open_session(&r, &owner, "m1", &spec, &log, "Agent ").await.unwrap();
    let opening = h.app.opening(&r, "T1", &member, &conn.capabilities, vec![], None);
    let session = new_session(&conn, &r.cwd, &opening).await.unwrap();
    let sid = session["sessionId"].as_str().unwrap().to_string();
    h.app.emit(
        &r,
        ev("attempt.started", "janus", "启动 T_TIMEOUT")
            .task("T_TIMEOUT")
            .attempt(1)
            .to("m1")
            .data(json!({ "session": sid })),
    );
    let t_ctx = Ctx { agent: "m1", task: Some("T_TIMEOUT"), attempt: Some(1), goal: false };
    let res = h
        .app
        .stream_prompt(
            &r,
            &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
            "你的子任务 T_TIMEOUT：慢操作\n\n验收标准：\n- 完成",
            &t_ctx,
            Some(&mut ctl),
            Some(Duration::from_millis(400)),
        )
        .await;
    assert!(res.is_err(), "short timeout must fail the turn");
    h.app.close_session(&r, &owner, Some(&conn), "关闭").await;
    let ledger2 = h.app.usage(&r);
    assert_eq!(
        ledger2.task("T_TIMEOUT").unwrap().tokens.total,
        Some(900),
        "post-cancel prompt_usage must be preserved on timeout"
    );

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead sends a task back to its own session on a verification's findings: no new subtask, the
/// findings arrive verbatim, and the verification checks the reworked result in its own session.
/// The lead must name the session, and `from` must be a verification that does not wait on the task.
#[tokio::test(flavor = "multi_thread")]
async fn rework_goes_back_into_the_original_session_and_is_rechecked() {
    let h = harness(
        "collab-rework",
        &[("lead", false), ("m1", true), ("m2", false)],
        json!({ "T1": ["ok", "ok"], "V1": ["reject", "approve"], "D1": ["ok"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "accept": ["parse 接口完整"] })),
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["独立核验 parse 边界"] })),
            task("D1", "m1", json!({ "deps": ["V1"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "V1 rejected T1, D1 held", |e| {
        task_event(e, "task.succeeded", "V1") && task_event(e, "task.blocked", "D1")
    })
    .await;
    let v1_done =
        events(&h.app, &run).into_iter().find(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("V1"));
    let guidance = board_entry(&h.app.run(&run).unwrap(), &v1_done.unwrap()).unwrap();
    assert!(guidance.contains("janus.retry") && guidance.contains("from=\"V1\""), "{guidance}");

    // The lead has to choose the session, and `from` has to be a verification with a verdict.
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "from": "V1" })).await;
    assert!(err && text.contains("session 必填"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "session": "keep", "from": "V1" })).await;
    assert!(err && text.contains("session 必填"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "retry", json!({ "task": "V1", "session": "continue", "from": "T1" })).await;
    assert!(err && text.contains("独立验证子任务"), "{text}");
    let (text, err) =
        tool(&h.app, &run, "lead", "retry", json!({ "task": "D1", "session": "continue", "from": "V1" })).await;
    assert!(err && text.contains("还没有结束"), "a blocked task is not reworked: {text}");

    let (text, err) = tool(
        &h.app,
        &run,
        "lead",
        "retry",
        json!({ "task": "T1", "session": "continue", "from": "V1", "note": "空输入按 ParseError 处理" }),
    )
    .await;
    assert!(!err && text.contains("V1 重新验证"), "{text}");
    wait_until(&h.app, &run, "D1 ran after the re-check", |e| task_event(e, "task.succeeded", "D1")).await;

    let evs = events(&h.app, &run);
    let started = |id: &str, n: i64| -> Value {
        evs.iter()
            .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id) && e.attempt == Some(n))
            .and_then(|e| e.data.clone())
            .unwrap_or_else(|| panic!("{id} attempt {n} did not start"))
    };
    let (t1a, t1b) = (started("T1", 1), started("T1", 2));
    assert_eq!(t1b["session"], t1a["session"], "the rework is in T1's own session");
    assert_eq!(t1b["continuation"], json!({ "from": "T1", "session": t1a["session"], "loaded": true }));
    assert_eq!(t1b["reopen"], json!({ "reason": "lead.retry", "session": "continue", "from": "V1" }));
    let prompt = t1b["prompt"].as_str().unwrap();
    assert!(prompt.starts_with("负责人让你在原来的会话中接着做子任务 T1"), "{prompt}");
    assert!(
        prompt.contains("运行 cargo test 复现空输入崩溃") && prompt.contains("空输入按 ParseError 处理"),
        "the findings arrive verbatim, with the lead's note: {prompt}"
    );
    assert!(!prompt.contains("你是 Janus 团队中的"), "the session already has the task: {prompt}");
    assert!(
        evs.iter().any(|e| e.kind == "context.handoff"
            && e.task.as_deref() == Some("T1")
            && e.attempt == Some(2)
            && e.data.as_ref().is_some_and(|d| d["from"] == "V1" && d["verdict"] == "rejected")),
        "the record says T1 used V1's findings"
    );
    let (v1a, v1b) = (started("V1", 1), started("V1", 2));
    assert_eq!(v1b["session"], v1a["session"], "the verifier re-checks in its own session");
    assert_eq!(v1b["reopen"], json!({ "reason": "target.reworked", "target": "T1" }));
    let recheck = v1b["prompt"].as_str().unwrap();
    assert!(
        recheck.starts_with("T1 已经返工")
            && recheck.contains("你上一轮的判定：需要修改（rejected）")
            && recheck.contains("运行 cargo test 复现空输入崩溃"),
        "{recheck}"
    );
    assert!(evs.iter().filter(|e| e.kind == "orchestration.accepted").count() == 1, "no new subtasks for the rework");
    let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
    for sid in [&t1a["session"], &v1a["session"]] {
        assert!(calls.contains(&format!("load {}", sid.as_str().unwrap())), "{calls}");
    }

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let d = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap().data.unwrap();
    let t1 = d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == "T1").unwrap();
    assert_eq!(t1["verifications"][0]["verdict"], "approved", "{t1}");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A rework recorded before a restart carries on: the task goes back into the session the lead
/// chose, the verification waits for the reworked task (even an upstream one it did not verify)
/// and checks it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_rework_survives_a_restart() {
    let h = harness("recover-rework", &[("lead", false), ("m1", true), ("m2", false)], json!({}));
    let original = start(&h, json!([task("SEED", "m1", json!({}))])).await;
    finish_lead(&h);
    wait_until(&h.app, &original, "seed delivered", |e| has(e, "run.delivered")).await;
    let row = h.app.db.run(&original).unwrap().unwrap();
    let id = "replay-rework";
    h.app
        .db
        .insert_run(id, RunKind::Goal, "返工后重启", row["cwd"].as_str().unwrap(), &row["config"], now_ms())
        .unwrap();
    let def = |id, agent, deps: &[&str]| json!({"id":id,"title":id,"agent":agent,"deps":deps,"accept":["完成"]});
    let mut verifier = def("V1", "m2", &["T1"]);
    verifier["verifies"] = json!("T1");
    let facts = [
        (
            "orchestration.accepted",
            None,
            json!({"tasks":[def("T0","m1",&[]),def("T1","m1",&["T0"]),verifier],"plannedSeq":0}),
        ),
        ("attempt.started", Some("T0"), json!({"session":"s-t0"})),
        ("task.succeeded", Some("T0"), json!({"summary":"接口"})),
        ("task.succeeded", Some("T1"), json!({"summary":"调用"})),
        ("task.succeeded", Some("V1"), json!({"summary":"接口不对","verdict":"rejected"})),
        ("task.reopened", Some("T0"), json!({"reason":"lead.retry","session":"continue","from":"V1"})),
        ("task.reopened", Some("V1"), json!({"reason":"target.reworked","target":"T0"})),
    ];
    for (i, (kind, task, data)) in facts.into_iter().enumerate() {
        h.app
            .db
            .insert_event(&crate::usage::event_from_json(
                id,
                &json!({"seq":i+1,"ts":now_ms(),"type":kind,"task":task,"actor":"m1","data":data}),
            ))
            .unwrap();
    }
    // The fake agent keeps what each session was told; T0's session was opened before the restart.
    std::fs::write(h.dir.join("fake/session-s-t0.json"), json!({ "task": "T0", "criteria": ["完成"] }).to_string())
        .unwrap();
    h.app.recover().unwrap();
    h.app.resume_run(id).await.unwrap();
    wait_until(&h.app, id, "V1 checked again", |e| {
        e.iter().filter(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("V1")).count() == 2
    })
    .await;
    let log = events(&h.app, id);
    let at = |kind: &str, task: &str| log.iter().rposition(|e| e.kind == kind && e.task.as_deref() == Some(task));
    let t0 = log.iter().rfind(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T0")).unwrap();
    assert_eq!(t0.data.as_ref().unwrap()["reopen"]["from"], "V1", "{t0:?}");
    assert_eq!(t0.data.as_ref().unwrap()["continuation"]["session"], "s-t0", "{t0:?}");
    let v1 = log.iter().rfind(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("V1")).unwrap();
    assert_eq!(v1.data.as_ref().unwrap()["reopen"]["reason"], "target.reworked");
    assert!(at("attempt.started", "V1") > at("task.succeeded", "T0"), "V1 waited for the reworked T0");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Sending a finished task back does not leave what already took its old result unnoticed: a
/// running downstream task is told its upstream is being reworked, and the lead's reply names
/// both it and the downstream task that already finished on the old result. Neither is redone
/// or stopped: that stays the lead's call.
#[tokio::test(flavor = "multi_thread")]
async fn a_rework_names_the_tasks_built_on_the_old_result() {
    let h = harness(
        "collab-rework-takers",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", true)],
        json!({ "T1": ["ok", "ok"], "T2": ["ok"], "T3": ["slow", "ok"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({})),
            task("T2", "m2", json!({ "deps": ["T1"] })),
            task("T3", "m3", json!({ "deps": ["T1"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "T2 done, T3 running", |e| {
        task_event(e, "task.succeeded", "T2") && task_event(e, "attempt.started", "T3")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "session": "continue" })).await;
    assert!(!err, "{text}");
    assert!(text.contains("T3 正在基于 T1 的旧结果执行"), "{text}");
    assert!(text.contains("T2 已经基于 T1 的旧结果完成"), "{text}");
    wait_until(&h.app, &run, "T1 reworked and T3 done", |e| {
        task_event(e, "task.succeeded", "T3")
            && e.iter().filter(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T1")).count() == 2
    })
    .await;
    let evs = events(&h.app, &run);
    assert!(
        evs.iter().any(|e| e.kind == "user.message"
            && e.task.as_deref() == Some("T3")
            && e.data.as_ref().is_some_and(|d| d["text"].as_str().unwrap_or("").contains("上游子任务 T1 正在返工"))),
        "the running downstream task is told"
    );
    assert!(
        !evs.iter().any(|e| e.kind == "task.reopened" && e.task.as_deref() == Some("T2")),
        "the finished downstream task is not redone without the lead"
    );
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The lead asks to continue the session of a task that never opened one (cancelled before it
/// started): the reply says a new session opens, and the attempt records why it is a new one.
#[tokio::test(flavor = "multi_thread")]
async fn continuing_a_task_that_never_had_a_session_says_so() {
    let h = harness("collab-no-session", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"], "T2": ["ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    let (text, err) = tool(&h.app, &run, "lead", "cancel", json!({ "tasks": ["T2", "T1"], "reason": "先停下" })).await;
    assert!(!err, "{text}");
    wait_until(&h.app, &run, "T1 stopped", |e| task_event(e, "task.skipped_by_decision", "T1")).await;
    assert!(!task_event(&events(&h.app, &run), "attempt.started", "T2"), "T2 never started");
    let (text, err) = tool(&h.app, &run, "lead", "retry", json!({ "task": "T2", "session": "continue" })).await;
    assert!(!err && text.contains("T2 之前没有开过会话，这次会新开一个"), "{text}");
    wait_until(&h.app, &run, "T2 done", |e| task_event(e, "task.succeeded", "T2")).await;
    let started = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2"))
        .and_then(|e| e.data)
        .unwrap();
    assert_eq!(
        started["continuation"],
        json!({ "from": "T2", "loaded": false, "fallback": "no_session" }),
        "{started}"
    );
    assert!(started["prompt"].as_str().unwrap().contains("你的子任务 T2"), "a new session gets the whole task");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Before the lead hands out a subtask it may do the work itself; once one is on the board it only
/// manages. Janus does not stop an edit (the agent's own permission mode decides) but reports each
/// file the lead edits after delegating once, on the board, and lists it as a delivery risk. The
/// board no longer suggests the lead take over a failed subtask.
#[tokio::test(flavor = "multi_thread")]
async fn the_lead_only_manages_once_it_delegates() {
    let h = harness("lead-manages", &[("lead", false), ("m1", true)], json!({ "T1": ["fail", "fail"] }));
    let run =
        h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into(), ..Default::default() }).await.unwrap();
    for _ in 0..100 {
        if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let r = h.app.run(&run).unwrap();
    let edit = |paths: &[&str]| {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        let seq = h.app.emit(&r, ev("tool.call", "lead", "Edit").data(json!({ "tool": "edit", "paths": paths })));
        h.app.record_lead_edits(&r, &paths, seq);
    };
    edit(&["small.rs"]);
    assert!(!has(&events(&h.app, &run), "lead.edited"), "before delegating the lead may do the work itself");

    let (text, err) =
        tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T1", "m1", json!({}))] })).await;
    assert!(!err, "{text}");
    edit(&["small.rs", "api.rs"]);
    edit(&["api.rs"]);
    let edited: Vec<Value> =
        events(&h.app, &run).into_iter().filter(|e| e.kind == "lead.edited").filter_map(|e| e.data).collect();
    assert_eq!(edited, vec![json!({ "paths": ["small.rs", "api.rs"] })], "each file once, after delegating");

    wait_until(&h.app, &run, "T1 failed", |e| task_event(e, "task.failed", "T1")).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("[负责人改了文件]") && board.contains("small.rs、api.rs"), "{board}");
    assert!(board.contains("[失败] T1") && board.contains("不要自己接手") && !board.contains("或自己做"), "{board}");

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let d = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap().data.unwrap();
    assert!(
        d["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x.as_str().unwrap().contains("负责人在派出子任务后自己改了文件：api.rs、small.rs")),
        "{}",
        d["risks"]
    );
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The delivery says which finished subtasks changed files with no independent verification, as a
/// fact; it does not call them unconfirmed, since the lead may have checked them itself.
#[tokio::test(flavor = "multi_thread")]
async fn the_delivery_marks_changes_without_an_independent_verification() {
    let h = harness(
        "unverified-changes",
        &[("lead", false), ("m1", true), ("m2", true), ("m3", false)],
        json!({ "T1": ["edit:a.rs"], "T2": ["edit:b.rs"], "V2": ["approve"], "T3": ["ok"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({})),
            task("T2", "m2", json!({})),
            task("V2", "m3", json!({ "verifies": "T2" })),
            task("T3", "m1", json!({ "deps": ["T1"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "all done", |e| {
        ["T1", "T2", "V2", "T3"].iter().all(|t| task_event(e, "task.succeeded", t))
    })
    .await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let d = events(&h.app, &run).into_iter().find(|e| e.kind == "run.delivered").unwrap().data.unwrap();
    let unverified = |id: &str| {
        d["acceptance"].as_array().unwrap().iter().find(|a| a["task"] == id).unwrap()["unverified"].as_bool().unwrap()
    };
    assert!(unverified("T1"), "changed a.rs and nobody verified it");
    assert!(!unverified("T2"), "V2 verified it");
    assert!(!unverified("V2"), "a verification is not itself verified");
    assert!(!unverified("T3"), "changed nothing");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The person may step into a collaborator's session directly. The lead runs the board, so each
/// step reaches it as "[用户介入]" with its next wait; it does not wake the lead by itself.
#[tokio::test(flavor = "multi_thread")]
async fn the_lead_hears_when_the_person_steps_into_a_collaborators_session() {
    let h = harness("user-steps-in", &[("lead", false), ("m1", true)], json!({ "T1": ["slow", "ok"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;

    act(&h, &run, json!({ "type": "takeover", "task": "T1" }));
    wait_until(&h.app, &run, "taken over", |e| has(e, "control.taken")).await;
    act(&h, &run, json!({ "type": "message", "task": "T1", "text": "先别动 README" }));
    let started = std::time::Instant::now();
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 2 })).await;
    assert!(started.elapsed() >= Duration::from_millis(1500), "the person's steps alone do not wake the lead");
    assert!(board.contains("[用户介入] T1「T1」· 用户接管了 m1 的会话"), "{board}");
    assert!(board.contains("[用户介入] T1「T1」· 用户对 m1 说：先别动 README"), "{board}");
    assert!(board.contains("不要用 janus.reply 或 janus.retry 把它改回去"), "{board}");

    act(&h, &run, json!({ "type": "finish", "task": "T1" }));
    // Ended with an empty reply, the result fails its acceptance check: the lead decides next.
    wait_until(&h.app, &run, "T1 ended by the person", |e| task_event(e, "task.failed", "T1")).await;
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("用户让 m1 以当前结果结束这个子任务"), "{board}");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Ending a subtask with the result so far while the person holds its session (no turn running)
/// is on record too, so the lead reads it like any other step the person takes.
#[tokio::test(flavor = "multi_thread")]
async fn ending_a_held_session_is_on_record_for_the_lead() {
    let h = harness("user-finishes-held", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 running", |e| task_event(e, "attempt.started", "T1")).await;
    act(&h, &run, json!({ "type": "interrupt", "task": "T1" }));
    wait_until(&h.app, &run, "held by the person", |e| {
        e.iter().any(|e| e.kind == "agent.status" && e.task.as_deref() == Some("T1") && e.text.contains("人工接管"))
    })
    .await;
    act(&h, &run, json!({ "type": "finish", "task": "T1" }));
    wait_until(&h.app, &run, "T1 ended", |e| {
        task_event(e, "task.failed", "T1") || task_event(e, "task.succeeded", "T1")
    })
    .await;
    let finished = events(&h.app, &run)
        .into_iter()
        .filter(|e| e.kind == "session.interrupted" && e.data.as_ref().is_some_and(|d| d["finish"] == true))
        .count();
    assert_eq!(finished, 1, "the person's finish is recorded once");
    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("用户打断了 m1 这一轮") && board.contains("用户让 m1 以当前结果结束这个子任务"), "{board}");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A verification still running when the lead reworks its target concludes on the old result; it
/// is not taken for the new one: it checks again once the rework is done, and the delivery shows
/// the verdict of that second check (FL-04).
#[tokio::test(flavor = "multi_thread")]
async fn a_verification_of_an_old_result_checks_again_after_the_rework() {
    let h = harness(
        "collab-stale-verdict",
        &[("lead", false), ("m1", true), ("m2", false)],
        json!({ "T1": ["write:a.txt", "write:a.txt"], "V1": ["hold:v1-go", "approve"] }),
    );
    let run = start(
        &h,
        json!([
            task("T1", "m1", json!({ "owns": ["a.txt"] })),
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["a.txt 正确"] }))
        ]),
    )
    .await;
    wait_until(&h.app, &run, "V1 at work on the first result", |e| task_event(e, "attempt.started", "V1")).await;
    let (text, err) =
        tool(&h.app, &run, "lead", "retry", json!({ "task": "T1", "session": "new", "note": "再补一行" })).await;
    assert!(!err, "{text}");
    // V1 approves what it saw, the first result, while T1 is being reworked.
    std::fs::write(h.dir.join("fake/v1-go"), "").unwrap();
    wait_until(&h.app, &run, "V1 checks again after T1's rework", |evs| {
        let started = evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("V1")).count();
        started == 2 && task_event(evs, "task.succeeded", "V1")
    })
    .await;
    let evs = events(&h.app, &run);
    let t1_done = evs.iter().rfind(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T1")).unwrap().seq;
    let v1_second =
        evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("V1")).nth(1).unwrap().seq;
    assert!(v1_second > t1_done, "the second check starts after the reworked result exists");
    assert!(
        evs.iter().any(|e| e.kind == "task.reopened"
            && e.task.as_deref() == Some("V1")
            && e.data.as_ref().is_some_and(|d| d["reason"] == "target.reworked")),
        "the reopen is on record, so a restart rebuilds it"
    );
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let _ = std::fs::remove_dir_all(&h.dir);
}
