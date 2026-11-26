//! How each role's tasks went in recent runs, for choosing its agent and model or rewriting its
//! instructions.

use super::*;

impl App {
    /// How each role's tasks went in recent runs (the ones `runs()` lists): how many finished,
    /// passed on their first attempt, failed verification, and how long a success took. The
    /// numbers behind choosing a role's agent and model, or rewriting its instructions.
    // @project-doc docs/domains/20261031-agents_and_roles.md#role_stats
    pub fn role_stats(&self) -> Result<Value> {
        let mut out: HashMap<String, RoleStat> = HashMap::new();
        let runs = self.db.runs(RunKind::Goal)?;
        for r in &runs {
            let run_id = r.id.as_str();
            let role_of: HashMap<String, String> = r.config["team"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| m["lead"] != true)
                .filter_map(|m| Some((m["id"].as_str()?.to_string(), m["role_id"].as_str()?.to_string())))
                .collect();
            let events = self.db.events(run_id, 0)?;
            for stat in task_outcomes(&events).into_values() {
                let Some(role) = stat.agent.as_ref().and_then(|a| role_of.get(a)) else { continue };
                let s = out.entry(role.clone()).or_default();
                match stat.outcome {
                    Some(true) => {
                        s.succeeded += 1;
                        if stat.attempts == 1 && stat.verification_failures == 0 {
                            s.first_pass += 1;
                        }
                        if let (Some(a), Some(b)) = (stat.started, stat.ended) {
                            s.success_ms += b - a;
                        }
                    }
                    Some(false) => s.failed += 1,
                    None => continue,
                }
                if stat.verification_failures > 0 {
                    s.verification_failed += 1;
                }
            }
        }
        let roles: serde_json::Map<String, Value> = out
            .into_iter()
            .map(|(k, s)| {
                let done = s.succeeded + s.failed;
                (k, json!({
                    "tasks": done, "succeeded": s.succeeded, "failed": s.failed, "firstPass": s.first_pass,
                    "verificationFailed": s.verification_failed,
                    "avgMinutes": if s.succeeded > 0 { (s.success_ms as f64 / s.succeeded as f64 / 60000.0 * 10.0).round() / 10.0 } else { 0.0 },
                }))
            })
            .collect();
        Ok(json!({ "runs": runs.len(), "roles": roles }))
    }
}

#[derive(Default)]
pub(super) struct RoleStat {
    succeeded: i64,
    failed: i64,
    first_pass: i64,
    verification_failed: i64,
    success_ms: i64,
}

/// How one task went, read from a run's events.
#[derive(Default)]
pub(super) struct TaskOutcome {
    agent: Option<String>,
    attempts: i64,
    verification_failures: i64,
    started: Option<i64>,
    ended: Option<i64>,
    /// Some(true) succeeded, Some(false) failed or cancelled after trying, None never finished.
    outcome: Option<bool>,
}

pub(super) fn task_outcomes(events: &[Event]) -> HashMap<String, TaskOutcome> {
    let mut out: HashMap<String, TaskOutcome> = HashMap::new();
    for e in events {
        let Some(task) = e.task.as_deref() else { continue };
        let t = out.entry(task.to_string()).or_default();
        match e.kind() {
            EventKind::TaskAssigned => t.agent = e.to.clone().or(t.agent.take()),
            EventKind::AttemptStarted => {
                t.attempts += 1;
                t.started.get_or_insert(e.ts);
                if t.agent.is_none() {
                    t.agent = e.to.clone();
                }
            }
            EventKind::VerificationFailed => t.verification_failures += 1,
            EventKind::TaskSucceeded => {
                t.outcome = Some(true);
                t.ended = Some(e.ts);
            }
            EventKind::TaskFailed => t.outcome = Some(false),
            // A retry starts the count again: what matters is how the task finally went.
            EventKind::TaskReopened => t.outcome = None,
            _ => {}
        }
    }
    out
}
