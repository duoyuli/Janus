//! The usage bill over a period, across runs, for the app's 「用量账单」 settings pane. It applies the
//! run ledger's rules (`backend/src/usage.rs`) to every run with usage in the period and keeps only
//! what happened inside it: the turns in the period, and the part of each session's running cost
//! reached in the period. Payers are Agents (a member's runtime, such as Codex or Claude Code),
//! each with its models. Read-only, computed from the event log on request; nothing is stored.
//! See `docs/interfaces/20261031-http_api.md#usage_bill`.

use crate::db::Db;
use crate::prices::{Estimate, Table};
use crate::usage::{self, BillGroup, Cost, Tokens};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The bill of `[from, to)`.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodBill {
    pub from: i64,
    pub to: i64,
    /// Runs with at least one turn in the period.
    pub runs: usize,
    /// Turns (prompts) that reported tokens.
    pub turns: usize,
    /// Sessions active in the period, and those of them that reported no tokens in it.
    pub sessions: usize,
    pub unreported_sessions: usize,
    /// Days with at least one turn, counted in the person's time zone, out of the period's days.
    pub active_days: usize,
    pub days: usize,
    pub tokens: Tokens,
    pub cache_hit_rate: Option<f64>,
    pub cost: Cost,
    pub agents: Vec<BillGroup>,
    /// What the tokens would cost at models.dev list prices; `None` before prices were synced.
    pub estimate: Option<Estimate>,
}

/// The bill of `[from, to)` (milliseconds), priced with `prices` when there are any. `offset_minutes`
/// is the person's time zone, east of UTC, so active days are their days.
// @project-doc docs/interfaces/20261031-http_api.md#usage_bill
pub fn period(db: &Db, from: i64, to: i64, offset_minutes: i64, prices: Option<&Table>) -> Result<PeriodBill> {
    let mut bill = PeriodBill { from, to, days: ((to - from + DAY_MS - 1) / DAY_MS) as usize, ..Default::default() };
    let mut days: HashSet<i64> = HashSet::new();
    for (run, config) in db.runs_with_usage(from, to)? {
        let ledger = usage::ledger(&db.events(&run, 0)?);
        let mut counted = false;
        for s in &ledger.sessions {
            let spend: Vec<&usage::Spend> = s.spend.iter().filter(|x| x.ts >= from && x.ts < to).collect();
            let cost = s.cost_between(from, to);
            if spend.is_empty() && cost.is_empty() && (s.last_ms < from || s.first_ms >= to) {
                continue;
            }
            bill.sessions += 1;
            bill.unreported_sessions += usize::from(spend.is_empty());
            let (id, name) = agent_of(&config, &s.actor);
            let i = match bill.agents.iter().position(|g| g.id == id) {
                Some(i) => i,
                None => {
                    bill.agents.push(BillGroup::new(&id, &name));
                    bill.agents.len() - 1
                }
            };
            for x in spend {
                bill.agents[i].spend(x);
                bill.tokens.add(&x.tokens);
                bill.turns += usize::from(x.first);
                days.insert((x.ts + offset_minutes * 60_000).div_euclid(DAY_MS));
                counted = true;
            }
            bill.agents[i].cost.add(&cost);
            bill.cost.add(&cost);
        }
        bill.runs += usize::from(counted);
    }
    BillGroup::sort(&mut bill.agents);
    bill.estimate = prices.map(|t| t.price(&mut bill.agents));
    bill.active_days = days.len();
    bill.cache_hit_rate = bill.tokens.cache_hit_rate();
    Ok(bill)
}

/// The Agent a run's member runs on, from the run's team: its runtime id and name. A member the
/// team does not list (a run recorded before the team was kept) is its own Agent.
fn agent_of(config: &Value, actor: &str) -> (String, String) {
    let member = config["team"].as_array().into_iter().flatten().find(|m| m["id"] == actor);
    let runtime = member.and_then(|m| m["runtime"].as_str()).filter(|r| !r.is_empty());
    match runtime {
        Some(r) => (
            r.to_string(),
            member.and_then(|m| m["runtime_name"].as_str()).filter(|n| !n.is_empty()).unwrap_or(r).to_string(),
        ),
        None => (format!("member:{actor}"), actor.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Event;
    use crate::status::RunKind;
    use serde_json::json;

    fn ev(seq: i64, ts: i64, kind: &str, actor: &str, to: Option<&str>, task: Option<&str>, data: Value) -> Event {
        Event {
            run_id: "r1".into(),
            seq,
            ts,
            kind: kind.into(),
            actor: actor.into(),
            to: to.map(str::to_owned),
            task: task.map(str::to_owned),
            attempt: task.map(|_| 1),
            text: String::new(),
            data: Some(data),
        }
    }

    fn team() -> Value {
        json!({ "team": [
            { "id": "lead", "runtime": "codex", "runtime_name": "Codex" },
            { "id": "m1", "runtime": "claude", "runtime_name": "Claude Code" },
            { "id": "m2", "runtime": "codex", "runtime_name": "Codex" }
        ] })
    }

    #[test]
    fn a_period_bill_counts_only_its_turns_and_its_part_of_the_cost() {
        let path = std::env::temp_dir().join(format!("janus-bill-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path).unwrap().0;
        let day = DAY_MS;
        db.insert_run("r1", RunKind::Goal, "g", "/tmp", &team(), 0).unwrap();
        let events = [
            ev(1, 0, "run.created", "user", None, None, json!({ "goal": "g" })),
            ev(2, 10, "agent.status", "lead", None, None, json!({ "session": "lead-1" })),
            ev(3, 20, "session.controls", "lead", None, None, json!({ "model": { "current": "gpt-6" } })),
            // Before the period: not billed.
            ev(
                4,
                day - 1,
                "session.usage",
                "lead",
                None,
                None,
                json!({ "inputTokens": 900, "outputTokens": 100, "totalTokens": 1000 }),
            ),
            ev(
                5,
                day + 10,
                "session.usage",
                "lead",
                None,
                None,
                json!({ "inputTokens": 100, "cachedReadTokens": 800, "outputTokens": 100, "totalTokens": 1000 }),
            ),
            ev(6, day + 20, "attempt.started", "janus", Some("m1"), Some("T1"), json!({ "session": "s1" })),
            ev(
                7,
                day + 30,
                "session.usage",
                "m1",
                None,
                Some("T1"),
                json!({ "used": 100, "size": 1000, "cost": { "amount": 0.5, "currency": "USD" } }),
            ),
            ev(
                8,
                3 * day + 30,
                "session.usage",
                "m1",
                None,
                Some("T1"),
                json!({ "totalTokens": 600, "models": [
                { "model": "opus", "inputTokens": 100, "cachedReadTokens": 300, "outputTokens": 100, "totalTokens": 500 },
                { "model": "haiku", "inputTokens": 80, "outputTokens": 20, "totalTokens": 100 }
            ] }),
            ),
            ev(
                9,
                3 * day + 40,
                "session.usage",
                "m1",
                None,
                Some("T1"),
                json!({ "used": 200, "size": 1000, "cost": { "amount": 0.8, "currency": "USD" } }),
            ),
            ev(10, 3 * day + 50, "attempt.started", "janus", Some("m2"), Some("T2"), json!({ "session": "s2" })),
            // After the period: not billed.
            ev(11, 9 * day, "session.usage", "m1", None, Some("T1"), json!({ "totalTokens": 5000 })),
        ];
        for e in &events {
            db.insert_event(e).unwrap();
        }
        // A run without usage in the period is not read.
        db.insert_run("r2", RunKind::Goal, "g", "/tmp", &team(), 0).unwrap();
        db.insert_event(&Event {
            run_id: "r2".into(),
            ..ev(1, 0, "session.usage", "lead", None, None, json!({ "totalTokens": 77 }))
        })
        .unwrap();

        let bill = period(&db, day, 8 * day, 0, None).unwrap();
        assert!(bill.estimate.is_none(), "no prices synced yet");
        assert_eq!((bill.runs, bill.days, bill.active_days, bill.turns), (1, 7, 2, 2));
        assert_eq!(
            bill.tokens.total,
            Some(1600),
            "one lead turn and one turn by two models; the turns outside are left out"
        );
        assert_eq!((bill.sessions, bill.unreported_sessions), (3, 1), "m2 opened a session and reported nothing");
        assert_eq!(bill.agents.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Codex", "Claude Code"]);
        let codex = &bill.agents[0];
        assert_eq!((codex.tokens.total, codex.models[0].model.as_str(), codex.turns), (Some(1000), "gpt-6", 1));
        let claude = &bill.agents[1];
        assert_eq!(
            claude.models.iter().map(|m| (m.model.as_str(), m.tokens.total)).collect::<Vec<_>>(),
            [("opus", Some(500)), ("haiku", Some(100))]
        );
        assert_eq!(
            (claude.cost.lines[0].currency.as_str(), claude.cost.lines[0].amount),
            ("USD", 0.8),
            "both readings fall in the period"
        );
        // The cache hit rate: 1,100 cache reads of 1,380 prompt tokens.
        assert!((bill.cache_hit_rate.unwrap() - 1100.0 / 1380.0).abs() < 1e-9, "{:?}", bill.cache_hit_rate);

        // A later period starts from the running cost reached before it, and a time zone moves days.
        let late = period(&db, 3 * day + 35, 8 * day, 0, None).unwrap();
        assert!(
            (late.cost.lines[0].amount - 0.3).abs() < 1e-9,
            "0.8 reached in it minus 0.5 before it: {:?}",
            late.cost
        );
        assert_eq!(late.tokens.total, None, "no turn in it");
        assert_eq!(period(&db, day, 8 * day, 23 * 60, None).unwrap().active_days, 2);
        // With prices, each model line is priced at its list price and the rest are counted.
        let table = Table::from_models_dev(
            &json!({ "anthropic": { "models": { "opus": { "cost": { "input": 5, "output": 25, "cache_read": 0.5 } } } } }),
            1,
        );
        let priced = period(&db, day, 8 * day, 0, Some(&table)).unwrap();
        let e = priced.estimate.unwrap();
        let opus = (100.0 * 5.0 + 100.0 * 25.0 + 300.0 * 0.5) / 1e6;
        assert!((e.amount - opus).abs() < 1e-12, "{e:?}");
        assert_eq!((e.unpriced_turns, e.unpriced_models.len()), (2, 2), "gpt-6 and haiku have no price here: {e:?}");
        assert_eq!(priced.agents[1].estimate, Some(opus));
        let _ = std::fs::remove_file(path);
    }
}
