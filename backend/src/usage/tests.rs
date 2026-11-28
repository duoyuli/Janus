//! Usage ledger tests: what the ledger counts, how it reads each event, and the cases in
//! `testdata/usage_cases.json`.

use super::*;
use serde_json::json;

fn ev(seq: i64, ts: i64, kind: &str, actor: &str, task: Option<&str>, attempt: Option<i64>, data: Value) -> Event {
    Event {
        run_id: "r1".into(),
        seq,
        ts,
        kind: kind.into(),
        actor: actor.into(),
        to: None,
        task: task.map(str::to_owned),
        attempt,
        text: String::new(),
        data: (!data.is_null()).then_some(data),
    }
}

fn usage(e: &mut Vec<Event>, seq: i64, ts: i64, actor: &str, task: Option<&str>, attempt: Option<i64>, data: Value) {
    e.push(ev(seq, ts, "session.usage", actor, task, attempt, data));
}

fn created(e: &mut Vec<Event>) {
    e.push(ev(1, 1_000, "run.created", "user", None, None, json!({ "goal": "测试" })));
}

fn started(e: &mut Vec<Event>, seq: i64, ts: i64, actor: &str, task: &str, n: i64, sid: &str) {
    e.push(ev(seq, ts, "attempt.started", actor, Some(task), Some(n), json!({ "session": sid })));
}

/// The ledger's numbers for whole runs, as the app shows them (it reads `GET /api/runs/{id}/usage`).
#[test]
fn shared_cases_give_the_expected_numbers() {
    let cases: Value = serde_json::from_str(include_str!("../../testdata/usage_cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let events: Vec<Event> =
            case["events"].as_array().unwrap().iter().map(|v| event_from_json("case", v)).collect();
        let l = ledger(&events);
        let want = &case["expect"];
        assert_eq!(l.run.tokens.total, want["runTotal"].as_i64(), "{name}: run total");
        assert_eq!(
            l.run.completeness.label_in(Lang::Zh),
            want["completeness"].as_str().unwrap(),
            "{name}: completeness"
        );
        for (task, total) in want["tasks"].as_object().into_iter().flatten() {
            assert_eq!(l.task(task).and_then(|t| t.tokens.total), total.as_i64(), "{name}: task {task}");
        }
        for (task, usd) in want["taskUsd"].as_object().into_iter().flatten() {
            assert_eq!(l.task(task).unwrap().cost.lines[0].amount, usd.as_f64().unwrap(), "{name}: {task}");
        }
        for (task_attempt, usd) in want["attemptUsd"].as_object().into_iter().flatten() {
            let attempt = l.attempts.iter().find(|a| format!("{}#{}", a.task, a.n) == *task_attempt).unwrap();
            assert_eq!(attempt.cost.lines[0].amount, usd.as_f64().unwrap(), "{name}: {task_attempt}");
        }
        if let Some(window) = want["windowUsd"].as_array() {
            let from = window[0].as_i64().unwrap();
            let to = window[1].as_i64().unwrap();
            let total: f64 = l.sessions.iter().flat_map(|s| s.cost_between(from, to)).map(|line| line.amount).sum();
            assert_eq!(total, window[2].as_f64().unwrap(), "{name}: bill window");
        }
        for (key, state) in want["attempts"].as_object().into_iter().flatten() {
            let a = l
                .attempts
                .iter()
                .find(|a| format!("{}#{}", a.task, a.n) == *key)
                .unwrap_or_else(|| panic!("{name}: attempt {key}"));
            assert_eq!(a.state.label_in(Lang::Zh), state.as_str().unwrap(), "{name}: attempt {key}");
        }
        assert_eq!(
            l.reminders.len(),
            want["reminders"].as_u64().unwrap_or(0) as usize,
            "{name}: reminders {:?}",
            l.reminders
        );
    }
}

#[test]
fn every_turn_is_added_even_when_the_turns_grow() {
    // Real per-turn numbers rise as the conversation grows; none of them is a running total.
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(
        &mut e,
        3,
        3_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 600, "outputTokens": 100, "totalTokens": 700 }),
    );
    usage(
        &mut e,
        4,
        4_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 1200, "outputTokens": 200, "totalTokens": 1400 }),
    );
    usage(
        &mut e,
        5,
        5_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 1800, "outputTokens": 300, "totalTokens": 2100 }),
    );
    usage(
        &mut e,
        6,
        6_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 300, "outputTokens": 50, "totalTokens": 350 }),
    );
    let l = ledger(&e);
    assert_eq!(l.run.tokens.total, Some(4550));
    assert_eq!((l.run.tokens.input, l.run.tokens.output), (Some(3900), Some(650)));
    assert_eq!(l.run.turns, 4);
    assert_eq!(l.run.completeness, Completeness::Complete);
}

#[test]
fn quota_rows_by_model_replace_the_main_loop_usage() {
    // Claude Code's `usage` is its main loop only; the quota rows add a subagent.
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(
        &mut e,
        3,
        3_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({
            "inputTokens": 1200, "outputTokens": 300, "cachedReadTokens": 5000, "cachedWriteTokens": 100, "totalTokens": 6600,
            "models": [
                { "model": "opus", "inputTokens": 1200, "outputTokens": 300, "cachedReadTokens": 5000, "cachedWriteTokens": 100, "totalTokens": 6600 },
                { "model": "haiku", "inputTokens": 800, "outputTokens": 100, "cachedReadTokens": 0, "cachedWriteTokens": 0, "totalTokens": 900 }
            ]
        }),
    );
    usage(
        &mut e,
        4,
        4_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 50, "outputTokens": 10, "totalTokens": 60 }),
    );
    let l = ledger(&e);
    assert_eq!(l.run.tokens.total, Some(7560), "6600 + 900 from the quota, then 60 from a turn without it");
    assert_eq!((l.run.tokens.cached_read, l.run.tokens.cached_write), (Some(5000), Some(100)));
    assert_eq!(
        breakdown_text(&l.run.tokens, Lang::Zh).as_deref(),
        Some("构成：输入 2,050 · 缓存读取 5,000 · 缓存写入 100 · 输出 410")
    );
}

#[test]
fn a_turn_without_a_total_adds_its_parts() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(
        &mut e,
        3,
        3_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 500, "outputTokens": 100, "cachedReadTokens": 2000, "totalTokens": null }),
    );
    assert_eq!(ledger(&e).run.tokens.total, Some(2600));
}

#[test]
fn context_snapshots_never_become_tokens_even_when_repeated() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    for (i, (ts, used)) in
        [(3_000, 52000), (4_000, 52000), (5_000, 61000), (6_000, 61000), (7_000, 52000)].iter().enumerate()
    {
        usage(&mut e, 3 + i as i64, *ts, "m1", Some("T1"), Some(1), json!({ "used": used, "size": 200000 }));
    }
    let l = ledger(&e);
    assert_eq!(l.run.tokens.total, None, "a snapshot is not consumption");
    assert_eq!(l.run.completeness, Completeness::Missing);
    assert_eq!(
        (l.run.context.used, l.run.context.peak, l.run.context.size, l.run.context.reports),
        (Some(52000), Some(61000), Some(200000), 5)
    );
    let line = context_text(&l.run.context, Lang::Zh).unwrap();
    assert!(line.contains("上下文 52,000 / 200,000（26%） · 峰值 61,000"), "{line}");
}

#[test]
fn missing_usage_is_reported_as_missing_never_as_zero() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    e.push(ev(3, 3_000, "task.succeeded", "m1", Some("T1"), Some(1), json!({ "summary": "完成" })));
    let l = ledger(&e);
    assert_eq!(l.run.tokens, Tokens::default());
    assert_eq!(l.run.completeness, Completeness::Missing);
    assert_eq!(tokens_text(&l.run.tokens, l.run.completeness, Lang::Zh), "未上报");
    assert!(l.task_lines("T1", &Names::new(), Lang::Zh).join("\n").contains("未上报"));
    assert!(l.task_lines("T9", &Names::new(), Lang::Zh)[0].contains("未开始"));
    // A turn that really used nothing is a reported 0.
    usage(
        &mut e,
        4,
        4_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "inputTokens": 0, "outputTokens": 0, "totalTokens": 0 }),
    );
    assert_eq!(ledger(&e).run.tokens.total, Some(0));
}

#[test]
fn cost_takes_the_latest_reading_and_keeps_what_came_before_a_reset() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    let cost = |amount: f64| json!({ "used": 1000, "size": 2000, "cost": { "amount": amount, "currency": "USD" } });
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), cost(0.1));
    usage(&mut e, 4, 4_000, "m1", Some("T1"), Some(1), cost(0.3));
    usage(&mut e, 5, 5_000, "m1", Some("T1"), Some(1), cost(0.0)); // a crashed result
    usage(&mut e, 6, 6_000, "m1", Some("T1"), Some(1), cost(0.05)); // after /clear
    usage(&mut e, 7, 7_000, "m1", Some("T1"), Some(1), cost(0.2));
    let l = ledger(&e);
    assert_eq!(l.run.cost.lines.len(), 1);
    assert!((l.run.cost.lines[0].amount - 0.5).abs() < 1e-9, "0.3 before the reset + 0.2 after it: {:?}", l.run.cost);
    assert_eq!(l.run.cost.reported_sessions, 1);
}

#[test]
fn cost_is_never_added_across_currencies() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), json!({ "cost": { "amount": 0.25, "currency": "USD" } }));
    started(&mut e, 4, 4_000, "m2", "T2", 1, "s2");
    usage(&mut e, 5, 5_000, "m2", Some("T2"), Some(1), json!({ "cost": { "amount": 40, "currency": "credits" } }));
    started(&mut e, 6, 6_000, "m3", "T3", 1, "s3");
    usage(&mut e, 7, 7_000, "m3", Some("T3"), Some(1), json!({ "cost": { "amount": 5 } }));
    let l = ledger(&e);
    let line = cost_text(&l.run.cost, Lang::Zh);
    assert!(line.contains("费用：0.25 USD + 40 credits + 5（未标注币种）（币种或口径不同，未合并）"), "{line}");
    assert_eq!(l.task("T1").unwrap().cost.lines[0].amount, 0.25);
}

#[test]
fn the_lead_is_counted_and_shown_apart_from_the_collaborators() {
    let mut e = vec![];
    created(&mut e);
    e.push(ev(2, 2_000, "agent.status", "lead", None, None, json!({ "phase": "thinking", "session": "lead-1" })));
    usage(&mut e, 3, 3_000, "lead", None, None, json!({ "totalTokens": 1500 }));
    let only_lead = ledger(&e);
    assert_eq!(only_lead.run.tokens.total, Some(1500));
    assert_eq!(
        (
            tokens_text(&only_lead.lead.tokens, only_lead.lead.completeness, Lang::Zh),
            tokens_text(&only_lead.collaborators.tokens, only_lead.collaborators.completeness, Lang::Zh)
        ),
        ("1,500 tokens（完整）".to_string(), "未上报".to_string())
    );
    started(&mut e, 4, 4_000, "m1", "T1", 1, "s1");
    usage(&mut e, 5, 5_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": 800 }));
    let l = ledger(&e);
    assert_eq!(
        (l.lead.tokens.total, l.collaborators.tokens.total, l.run.tokens.total),
        (Some(1500), Some(800), Some(2300))
    );
}

#[test]
fn attempts_keep_their_state_and_all_count_into_the_task() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": 700 }));
    e.push(ev(4, 4_000, "attempt.failed", "m1", Some("T1"), Some(1), json!({ "error": "脚本失败" })));
    started(&mut e, 5, 6_000, "m1", "T1", 2, "s2");
    usage(&mut e, 6, 7_000, "m1", Some("T1"), Some(2), json!({ "totalTokens": 900 }));
    e.push(ev(7, 8_000, "task.succeeded", "m1", Some("T1"), Some(2), json!({ "summary": "完成" })));
    let l = ledger(&e);
    assert_eq!(
        (l.attempts[0].state, l.attempts[0].note.as_deref(), l.attempts[0].duration_ms),
        (AttemptState::Failed, Some("脚本失败"), Some(2000))
    );
    assert_eq!(l.attempts[1].state, AttemptState::Succeeded);
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(1600), "a failed attempt's usage still counts");
    let lines = l.task_lines("T1", &Names::new(), Lang::Zh);
    assert!(lines.iter().any(|x| x.starts_with("尝试 1：m1 · 失败")), "{lines:?}");
    assert!(
        lines.iter().any(|x| x == "成员 m1：1,600 tokens · 模型 未知模型 1,600"),
        "a session that named no model: {lines:?}"
    );
    // An English run reads the same lines in English.
    let en = l.task_lines("T1", &Names::new(), Lang::En);
    assert!(en.iter().any(|x| x.starts_with("Attempt 1: m1 · failed")), "{en:?}");
    assert!(en[0].starts_with("Usage: 1,600 tokens"), "{en:?}");
}

#[test]
fn a_cancel_ends_the_attempts_still_running_and_says_who_cancelled() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    started(&mut e, 3, 2_000, "m2", "T2", 1, "s2");
    usage(&mut e, 4, 3_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": 1500 }));
    e.push(ev(5, 5_000, "task.skipped_by_decision", "lead", Some("T1"), None, json!({ "reason": "不需要了" })));
    let l = ledger(&e);
    let of = |l: &Ledger, task: &str| l.attempts.iter().find(|a| a.task == task).unwrap().clone();
    assert_eq!(
        (of(&l, "T1").state, of(&l, "T1").note.as_deref()),
        (AttemptState::Cancelled, Some("负责人取消了这个子任务"))
    );
    assert_eq!(of(&l, "T2").state, AttemptState::Unfinished, "cancelling one task leaves the others running");
    e.push(ev(6, 9_000, "run.cancelled", "user", None, None, json!({})));
    e.push(ev(7, 9_100, "attempt.failed", "m2", Some("T2"), Some(1), json!({ "error": "连接已关闭" })));
    let l = ledger(&e);
    assert_eq!(
        (of(&l, "T2").state, of(&l, "T2").note.as_deref(), of(&l, "T2").duration_ms),
        (AttemptState::Cancelled, Some("任务被取消"), Some(7100))
    );
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(1500), "what it reported before the cancel still counts");
}

#[test]
fn an_expired_attempt_is_counted_and_labeled() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": 400 }));
    e.push(ev(4, 4_000, "attempt.late_result", "m1", Some("T1"), Some(1), json!({})));
    let l = ledger(&e);
    assert_eq!(l.attempts[0].state, AttemptState::Expired);
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(400));
}

#[test]
fn waits_are_kept_apart_from_the_wall_clock() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    e.push(ev(3, 3_000, "approval.requested", "m1", Some("T1"), Some(1), json!({ "key": "req-T1-1", "options": [] })));
    e.push(ev(4, 33_000, "approval.granted", "user", Some("T1"), Some(1), json!({ "key": "req-T1-1", "allow": true })));
    e.push(ev(5, 60_000, "task.succeeded", "m1", Some("T1"), Some(1), json!({ "summary": "完成" })));
    let l = ledger(&e);
    assert_eq!(
        (l.run.duration_ms, l.run.wait_ms, l.task("T1").unwrap().wait_ms),
        (Some(59_000), Some(30_000), Some(30_000))
    );
    assert!(l.task_lines("T1", &Names::new(), Lang::Zh)[0].contains("（其中等待用户 30 秒）"));
}

#[test]
fn a_replay_prefix_gives_the_numbers_of_that_point() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    for (i, n) in [700, 1400, 2100].iter().enumerate() {
        usage(&mut e, 3 + i as i64, 3_000 + i as i64 * 1000, "m1", Some("T1"), Some(1), json!({ "totalTokens": n }));
    }
    assert_eq!(ledger(&e[..3]).run.tokens.total, Some(700));
    assert_eq!(ledger(&e[..4]).run.tokens.total, Some(2100));
    assert_eq!(ledger(&e).run.tokens.total, Some(4200));
    assert_eq!(
        serde_json::to_value(ledger(&e)).unwrap(),
        serde_json::to_value(ledger(&e)).unwrap(),
        "nothing carries over between reads"
    );
}

#[test]
fn reminders_carry_their_numbers() {
    let run = |total: i64| {
        let mut e = vec![];
        created(&mut e);
        e.push(ev(2, 2_000, "agent.status", "lead", None, None, json!({ "session": "lead-1" })));
        usage(&mut e, 3, 3_000, "lead", None, None, json!({ "used": 4000, "size": 200000 }));
        started(&mut e, 4, 4_000, "m1", "T1", 1, "s1");
        usage(&mut e, 5, 5_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": total }));
        ledger(&e)
    };
    let l = run(120_000_000);
    assert_eq!(l.run.completeness, Completeness::Partial, "the lead reported context but no tokens");
    assert_eq!(
        l.reminders,
        [
            Reminder::Tokens { total: 120_000_000, threshold: TOKEN_REMINDER },
            Reminder::OneTask { task: "T1".into(), total: 120_000_000, share: 1.0 },
            Reminder::CostUnknown,
        ]
    );
    assert_eq!(
        serde_json::to_value(&l.reminders[1]).unwrap(),
        json!({ "kind": "oneTask", "task": "T1", "total": 120000000, "share": 1.0 })
    );
    // Six million tokens are an ordinary run's: only the missing cost is worth a word.
    assert_eq!(run(6_000_000).reminders, [Reminder::CostUnknown]);
}

#[test]
fn continued_session_attributes_each_tasks_turns_without_double_counting() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), json!({ "totalTokens": 600 }));
    e.push(ev(4, 4_000, "task.succeeded", "m1", Some("T1"), Some(1), json!({ "summary": "初稿" })));
    started(&mut e, 5, 5_000, "m1", "T2", 1, "s1");
    usage(&mut e, 6, 6_000, "m1", Some("T2"), Some(1), json!({ "totalTokens": 250 }));
    e.push(ev(7, 7_000, "task.succeeded", "m1", Some("T2"), Some(1), json!({ "summary": "修复" })));
    let l = ledger(&e);
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(600));
    assert_eq!(l.task("T2").unwrap().tokens.total, Some(250));
    assert_eq!(l.run.tokens.total, Some(850));
}

#[test]
fn numbers_and_durations_read_the_way_the_app_writes_them() {
    assert_eq!(
        (num(0), num(999), num(15400), num(-1234567)),
        ("0".into(), "999".into(), "15,400".into(), "-1,234,567".into())
    );
}

/// Live aggregation stays linear on a large log (measured instead of adding an index).
#[test]
fn aggregating_a_large_log_stays_fast() {
    let mut e = Vec::new();
    created(&mut e);
    let mut seq = 2;
    for task in 0..40 {
        let id = format!("T{task}");
        for attempt in 1..=3 {
            started(&mut e, seq, 2_000 + seq * 10, "m1", &id, attempt, &format!("s{task}-{attempt}"));
            seq += 1;
            for turn in 1..=400 {
                usage(
                    &mut e,
                    seq,
                    2_000 + seq * 10,
                    "m1",
                    Some(&id),
                    Some(attempt),
                    json!({ "inputTokens": 100 * turn, "outputTokens": 10 * turn, "totalTokens": 110 * turn, "used": 1000 + turn, "size": 200000 }),
                );
                seq += 1;
            }
        }
    }
    let began = std::time::Instant::now();
    let l = ledger(&e);
    let elapsed = began.elapsed();
    println!("usage::ledger over {} events took {elapsed:?}", e.len());
    assert_eq!((l.tasks.len(), l.attempts.len()), (40, 120));
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
}

/// Each attempt's time goes to work, verification or rework: a verification's attempts are all
/// checking, a task's first attempt is work and any later one rework, and an attempt that only went
/// back into its session after a restart is whatever the attempt before it was.
#[test]
fn attempts_split_time_into_work_verification_and_rework() {
    let mut e = vec![];
    created(&mut e);
    e.push(ev(
        2,
        1_000,
        "orchestration.accepted",
        "lead",
        None,
        None,
        json!({ "tasks": [{ "id": "T1", "agent": "m1" }, { "id": "V1", "agent": "m2", "verifies": "T1" }] }),
    ));
    let start = |e: &mut Vec<Event>, seq, ts, task: &str, n, data: Value| {
        e.push(ev(seq, ts, "attempt.started", "janus", Some(task), Some(n), data));
    };
    let done = |e: &mut Vec<Event>, seq, ts, task: &str, n| {
        e.push(ev(seq, ts, "task.succeeded", "m", Some(task), Some(n), json!({})));
    };
    start(&mut e, 3, 10_000, "T1", 1, json!({ "session": "a" }));
    done(&mut e, 4, 70_000, "T1", 1);
    start(&mut e, 5, 70_000, "V1", 1, json!({ "session": "b" }));
    done(&mut e, 6, 100_000, "V1", 1);
    // Sent back by the lead, cut off by a restart, then picked up again in its session.
    start(
        &mut e,
        7,
        100_000,
        "T1",
        2,
        json!({ "session": "a", "reopen": { "reason": "lead.retry", "session": "continue" } }),
    );
    start(&mut e, 8, 120_000, "T1", 3, json!({ "session": "a", "brokeOff": "restart" }));
    done(&mut e, 9, 140_000, "T1", 3);
    start(&mut e, 10, 140_000, "V1", 2, json!({ "session": "b", "reopen": { "reason": "target.reworked" } }));
    done(&mut e, 11, 150_000, "V1", 2);
    let l = ledger(&e);
    let kinds: Vec<(String, AttemptKind)> =
        l.attempts.iter().map(|a| (format!("{}#{}", a.task, a.n), a.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("T1#1".to_string(), AttemptKind::Work),
            ("V1#1".to_string(), AttemptKind::Verify),
            ("T1#2".to_string(), AttemptKind::Rework),
            ("T1#3".to_string(), AttemptKind::Rework),
            ("V1#2".to_string(), AttemptKind::Verify),
        ]
    );
    assert_eq!(l.time.work_ms, 60_000);
    assert_eq!(l.time.verify_ms, 40_000);
    assert_eq!(l.time.span_ms, 140_000);
    assert_eq!(l.time.rework_ms, 40_000);
}

/// A reading of the agent's records: the session's running totals by model.
fn records(sid: &str, ended: bool, rows: &[(&str, i64)]) -> Value {
    let models: Vec<Value> = rows
        .iter()
        .map(|(model, total)| json!({ "model": model, "inputTokens": total / 2, "outputTokens": total - total / 2, "totalTokens": total }))
        .collect();
    json!({ "source": RECORDS_SOURCE, "session": sid, "turnEnded": ended, "models": models })
}

#[test]
fn records_readings_count_each_increment_where_it_is_read() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "warm");
    usage(&mut e, 3, 3_000, "m1", Some("T1"), Some(1), records("warm", false, &[("opus", 400)]));
    usage(&mut e, 4, 4_000, "m1", Some("T1"), Some(1), records("warm", false, &[("opus", 400)]));
    usage(&mut e, 5, 5_000, "m1", Some("T1"), Some(1), records("warm", true, &[("opus", 700)]));
    // The next task continues the same native session: only what came after is its own.
    started(&mut e, 6, 6_000, "m1", "T2", 1, "warm");
    usage(&mut e, 7, 7_000, "m1", Some("T2"), Some(1), records("warm", true, &[("opus", 1000), ("haiku", 50)]));
    let l = ledger(&e);
    assert_eq!(l.task("T1").unwrap().tokens.total, Some(700));
    assert_eq!(l.task("T2").unwrap().tokens.total, Some(350));
    assert_eq!((l.task("T1").unwrap().turns, l.task("T2").unwrap().turns), (1, 1), "a turn is counted as it ends");
    let bill = l.task_bill("T1", &Names::new(), Lang::Zh);
    let lines: Vec<(&str, Option<i64>, usize)> =
        bill[0].models.iter().map(|m| (m.model.as_str(), m.tokens.total, m.turns)).collect();
    assert_eq!(lines, [("opus", Some(700), 1)], "two readings of one turn are one turn of the model");
    assert_eq!(bill[0].turns, 1);
    // A bill over part of the session takes the increments read in it.
    let early: i64 =
        l.sessions_of("warm").flat_map(|s| &s.spend).filter(|x| x.ts < 4_500).filter_map(|x| x.tokens.total).sum();
    assert_eq!(early, 400);
}

#[test]
fn a_session_with_records_counts_no_prompt_response_tokens() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    // The prompt response comes before the turn's last reading, and once after it.
    usage(
        &mut e,
        3,
        3_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "totalTokens": 9000, "models": [{ "model": "opus", "totalTokens": 9000 }] }),
    );
    usage(&mut e, 4, 3_100, "m1", Some("T1"), Some(1), records("s1", true, &[("opus", 1200)]));
    usage(
        &mut e,
        5,
        4_000,
        "m1",
        Some("T1"),
        Some(1),
        json!({ "totalTokens": 800, "used": 5000, "size": 200000, "cost": { "amount": 0.5, "currency": "USD" } }),
    );
    // Another member's session without records keeps its prompt responses.
    e.push(ev(6, 5_000, "attempt.started", "janus", Some("T2"), Some(1), json!({ "session": "s2" })));
    e.last_mut().unwrap().to = Some("m2".into());
    usage(&mut e, 7, 6_000, "m2", Some("T2"), Some(1), json!({ "totalTokens": 300 }));
    let l = ledger(&e);
    let t1 = l.task("T1").unwrap();
    assert_eq!(t1.tokens.total, Some(1200), "the records alone");
    assert_eq!((t1.context.used, t1.cost.lines[0].amount), (Some(5000), 0.5), "context and cost as before");
    assert_eq!(t1.turns, 1);
    assert_eq!(l.task("T2").unwrap().tokens.total, Some(300));
    assert_eq!(l.run.completeness, Completeness::Complete);
}

#[test]
fn a_crashed_turn_is_counted_from_its_records() {
    let mut e = vec![];
    created(&mut e);
    started(&mut e, 2, 2_000, "m1", "T1", 1, "s1");
    usage(&mut e, 3, 22_000, "m1", Some("T1"), Some(1), records("s1", false, &[("opus", 500)]));
    usage(&mut e, 4, 30_000, "m1", Some("T1"), Some(1), records("s1", true, &[("opus", 650)]));
    e.push(ev(5, 30_100, "attempt.failed", "m1", Some("T1"), Some(1), json!({ "error": "Agent 进程意外退出" })));
    // Before the turn ends, its tokens so far are already there, and reported.
    let running = ledger(&e[..3]);
    assert_eq!((running.run.tokens.total, running.run.completeness), (Some(500), Completeness::Complete));
    let l = ledger(&e);
    let a = &l.attempts[0];
    assert_eq!((a.state, a.tokens.total, a.completeness), (AttemptState::Failed, Some(650), Completeness::Complete));
}
