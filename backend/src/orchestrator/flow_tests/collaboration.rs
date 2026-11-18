//! End-to-end tests for the multi-agent collaboration improvements (`C-01`..`C-13`): event-driven
//! `wait` + live task pulse, verifier `verdict` + targeted context, structured `handoff`, multi-step
//! loop detection, Lead edit conflict/stale tracking, warm session continuation (`continues` /
//! `continue_session`), role `slots` + concurrency cap, token budget pause/resume, collaborator
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
    // Drain initial `[开始]` entry so cursor sits at the active tool call.
    let (first, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(first.contains("[开始] T1"), "{first}");

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
            task("V1", "m2", json!({ "verifies": "T1", "accept": ["独立核验 parse 边界"] }))
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

    // Disable `loadSession` and ask `retry` for `continue_session: true`: falls back cleanly to `session/new`.
    std::fs::write(h.dir.join("fake/no-load"), "").unwrap();
    let (_, err) = tool(
        &h.app,
        &run,
        "lead",
        "retry",
        json!({ "task": "T2", "instructions": "换个做法", "continue_session": true }),
    )
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
        json!({ "T1": ["handoff"], "T2": ["loop2"], "T3": ["ask", "ok"], "T4": ["ok"] }),
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

    // C-06: Start `T3` owning `owned.rs` (waiting on `ask`), then have the Lead edit both `owned.rs`
    // (active task's file -> `byLead` conflict) and `api.rs` (finished `T1`'s file -> `负责人 改` stale note).
    let (_, err) = tool(
        &h.app,
        &run,
        "lead",
        "orchestrate",
        json!({ "tasks": [task("T3", "m1", json!({ "owns": ["owned.rs"] }))] }),
    )
    .await;
    assert!(!err);
    wait_until(&h.app, &run, "T3 asks", |e| task_event(e, "question.asked", "T3")).await;

    let r = h.app.run(&run).unwrap();
    let seq = h.app.emit(
        &r,
        ev("tool.call", "lead", "Edit owned.rs & api.rs")
            .data(json!({ "tool": "edit", "status": "completed", "paths": ["owned.rs", "api.rs"] })),
    );
    h.app.record_lead_edits(&r, &["owned.rs".into(), "api.rs".into()], seq);

    let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
    assert!(board.contains("连续 4 轮重复调用序列"), "{board}");
    assert!(board.contains("你（负责人）修改了正在进行的子任务 T3"), "{board}");

    let (detail, _) = tool(&h.app, &run, "lead", "task_detail", json!({ "task": "T3" })).await;
    assert!(detail.contains("负责人改了这个子任务的文件：owned.rs"), "{detail}");

    let (_, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T3", "text": "alpha" })).await;
    assert!(!err);
    wait_until(&h.app, &run, "T3 done", |e| task_event(e, "task.succeeded", "T3")).await;

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
async fn token_budget_pauses_run_and_resumes_on_extend_budget() {
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

    // `T1` reports 1,200 tokens >= 1,000 limit -> Run pauses in `waiting_input` before starting `T2`.
    wait_until(&h.app, &run, "token budget exhausted", |e| has(e, "run.budget_exhausted")).await;
    assert_eq!(status(&h.app, &run), "waiting_input");
    let exhausted =
        events(&h.app, &run).into_iter().find(|e| e.kind == "run.budget_exhausted").and_then(|e| e.data).unwrap();
    assert_eq!(exhausted["kind"], "tokens", "{exhausted}");
    assert!(!task_event(&events(&h.app, &run), "attempt.started", "T2"));

    // Extending budget resumes `T2` automatically.
    act(&h, &run, json!({ "type": "extendBudget", "tokens": 5_000 }));
    wait_until(&h.app, &run, "T2 finishes after budget extension", |e| task_event(e, "task.succeeded", "T2")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
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

    // Part 2: Verify `recover_native_usage` reads Janus's isolated `<agents_dir>/fake/home/.claude/projects/.../<sid>.jsonl`
    // when an attempt crashed without reporting ACP usage, and never double-counts once reported.
    let crash_sid = events(&h.app, &run)
        .into_iter()
        .find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T_CRASH"))
        .and_then(|e| e.data?["session"].as_str().map(str::to_owned))
        .unwrap();
    let proj_dir = h.app.cfg.agents_dir.join("fake").join("home/.claude/projects/demo");
    std::fs::create_dir_all(&proj_dir).unwrap();
    std::fs::write(
        proj_dir.join(format!("{crash_sid}.jsonl")),
        format!(
            "{}\n{}\n",
            json!({ "type": "assistant", "message": { "id": "m1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 300, "output_tokens": 50, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0 } } }),
            json!({ "type": "assistant", "message": { "id": "m1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 300, "output_tokens": 80, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0 } } })
        ),
    )
    .unwrap();
    let r = h.app.run(&run).unwrap();
    let member = r.member("m1").unwrap().clone();
    let ctx = Ctx { agent: "m1", task: Some("T_CRASH"), attempt: Some(1), goal: false };
    h.app.recover_native_usage(&r, &member, &crash_sid, &ctx);
    // Calling a second time is a no-op because `session_has_reported_tokens` sees the recovered event.
    h.app.recover_native_usage(&r, &member, &crash_sid, &ctx);
    let ledger = h.app.usage(&r);
    assert_eq!(ledger.task("T_CRASH").unwrap().tokens.total, Some(480));

    // Exercise `stream_prompt` short timeout directly against `cancel-usage` to verify post-cancel `prompt_usage`.
    let spec = h.app.spec_for("fake").unwrap();
    let owner = Owner { session: SessionKey::task("T_TIMEOUT"), attempt: 1 };
    let log = h.app.cfg.data_dir.join("logs").join(&run).join("T_TIMEOUT-a1.log");
    let (conn, mut rx, mut ctl) = h.app.open_session(&r, &owner, "m1", &spec, &log, "Agent ").await.unwrap();
    let opening = h.app.opening(&r, &member, &conn.capabilities, vec![], None);
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
