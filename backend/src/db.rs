//! SQLite fact store. Events are the source of truth; `runs`, `attempts` and `artifacts`
//! are indexes kept alongside them so the scheduler and UI can query without replaying.

use crate::status::{AttemptStatus, CommandState, RunKind, RunStatus};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

#[derive(Serialize, Clone, Debug)]
pub struct Event {
    pub run_id: String,
    pub seq: i64,
    pub ts: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub struct Db(Mutex<Connection>);

/// What changed about an attempt besides its status: whichever of these are known now.
#[derive(Default)]
pub struct AttemptUpdate<'a> {
    pub session: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub error: Option<&'a str>,
    pub ended: Option<i64>,
}

/// One row of `artifacts`.
pub struct ArtifactRow<'a> {
    pub run_id: &'a str,
    pub task: &'a str,
    pub attempt: i64,
    pub name: &'a str,
    pub kind: &'a str,
    pub path: &'a str,
    pub sha256: &'a str,
    pub bytes: i64,
    pub created_at: i64,
}

/// One row of `verifications`: how an attempt's result check went.
pub struct VerificationRow<'a> {
    pub run_id: &'a str,
    pub task: &'a str,
    pub attempt: i64,
    pub attempt_id: &'a str,
    pub status: &'a str,
    pub details: &'a Value,
    pub created_at: i64,
}

/// Which runs a list asks for (`Db::runs_page`).
pub struct RunsPage {
    pub kind: RunKind,
    pub workspace: Option<String>,
    /// The creation time and id of the last run already listed; `None` for the first page.
    pub before: Option<(i64, String)>,
    pub limit: usize,
}

/// A `runs` row as the API and the orchestrator read it.
fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "goal": r.get::<_, String>(1)?,
        "cwd": r.get::<_, String>(2)?,
        "config": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
        "status": r.get::<_, String>(4)?,
        "createdAt": r.get::<_, i64>(5)?,
        "kind": r.get::<_, String>(6)?,
    }))
}

impl Db {
    /// Opens the database and brings it up to this build's data version (see `migrations`), saying
    /// what the upgrade did.
    // @project-doc docs/architecture/20261031-data_model.md#migrations
    pub fn open(path: &Path) -> Result<(Db, crate::migrations::Report)> {
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let report = crate::migrations::upgrade(&mut conn, Some(path))?;
        Ok((Db(Mutex::new(conn)), report))
    }

    /// The data version of this database (`PRAGMA user_version`).
    pub fn data_version(&self) -> Result<u32> {
        Ok(self.0.lock().unwrap().pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn insert_run(&self, id: &str, kind: RunKind, goal: &str, cwd: &str, config: &Value, now: i64) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO runs (id, kind, goal, cwd, config, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'planning', ?6)",
            params![id, kind.as_str(), goal, cwd, config.to_string(), now],
        )?;
        Ok(())
    }

    /// A chat's title, from its first message.
    pub fn set_run_goal(&self, id: &str, goal: &str) -> Result<()> {
        self.0.lock().unwrap().execute("UPDATE runs SET goal = ?2 WHERE id = ?1", params![id, goal])?;
        Ok(())
    }

    pub fn set_run_status(&self, id: &str, status: RunStatus) -> Result<()> {
        self.0.lock().unwrap().execute("UPDATE runs SET status = ?2 WHERE id = ?1", params![id, status.as_str()])?;
        Ok(())
    }

    pub fn insert_event(&self, e: &Event) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO events (run_id, seq, ts, type, actor, to_agent, task, attempt, text, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                e.run_id,
                e.seq,
                e.ts,
                e.kind,
                e.actor,
                e.to,
                e.task,
                e.attempt,
                e.text,
                e.data.as_ref().map(Value::to_string)
            ],
        )?;
        Ok(())
    }

    pub fn events(&self, run_id: &str, after: i64) -> Result<Vec<Event>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, seq, ts, type, actor, to_agent, task, attempt, text, data
             FROM events WHERE run_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![run_id, after], |r| {
            Ok(Event {
                run_id: r.get(0)?,
                seq: r.get(1)?,
                ts: r.get(2)?,
                kind: r.get(3)?,
                actor: r.get(4)?,
                to: r.get(5)?,
                task: r.get(6)?,
                attempt: r.get(7)?,
                text: r.get(8)?,
                data: r.get::<_, Option<String>>(9)?.and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The runs with usage reported in `[from, to)` and their configs, oldest first, for a bill over
    /// a period (`events_type_ts` keeps this from reading every event).
    pub fn runs_with_usage(&self, from: i64, to: i64) -> Result<Vec<(String, Value)>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, config FROM runs WHERE id IN
               (SELECT DISTINCT run_id FROM events WHERE type = 'session.usage' AND ts >= ?1 AND ts < ?2)
             ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, serde_json::from_str::<Value>(&r.get::<_, String>(1)?).unwrap_or(Value::Null)))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn max_seq(&self, run_id: &str) -> Result<i64> {
        Ok(self.0.lock().unwrap().query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM events WHERE run_id = ?1",
            params![run_id],
            |r| r.get(0),
        )?)
    }

    /// The newest 50 runs of one kind, for the role statistics. Goals and chats are counted apart.
    pub fn runs(&self, kind: RunKind) -> Result<Vec<Value>> {
        Ok(self.runs_page(&RunsPage { kind, workspace: None, before: None, limit: 50 })?.0)
    }

    /// One page of runs, newest first: of one kind, of one workspace when given (`config.workspace.id`),
    /// older than the cursor `before` (the last run of the previous page). Whether more follow is
    /// the second value. The order is total (creation time, then id), so paging never skips or
    /// repeats a run, however many were created in the same millisecond.
    // @project-doc docs/interfaces/20261031-http_api.md#run_list
    pub fn runs_page(&self, page: &RunsPage) -> Result<(Vec<Value>, bool)> {
        let conn = self.0.lock().unwrap();
        let (time, id) = page.before.clone().unwrap_or((i64::MAX, String::new()));
        let mut stmt = conn.prepare(
            "SELECT id, goal, cwd, config, status, created_at, kind FROM runs
             WHERE kind = ?1
               AND (?2 IS NULL OR json_extract(config, '$.workspace.id') = ?2)
               AND (created_at < ?3 OR (created_at = ?3 AND ?4 != '' AND id < ?4))
             ORDER BY created_at DESC, id DESC LIMIT ?5",
        )?;
        let rows =
            stmt.query_map(params![page.kind.as_str(), page.workspace, time, id, page.limit as i64 + 1], run_row)?;
        let mut runs: Vec<Value> = rows.collect::<rusqlite::Result<_>>()?;
        let more = runs.len() > page.limit;
        runs.truncate(page.limit);
        Ok((runs, more))
    }

    /// The runs still working (planning, running or waiting for the person) in directory `cwd`.
    pub fn active_runs_in(&self, cwd: &str) -> Result<Vec<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, goal, cwd, config, status, created_at, kind FROM runs
             WHERE cwd = ?1 AND status IN ('planning', 'running', 'waiting_input') ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![cwd], run_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Accepted runs that worked in a worktree (`config.worktree`).
    pub fn accepted_worktrees(&self) -> Result<Vec<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, goal, cwd, config, status, created_at, kind FROM runs
             WHERE status = 'accepted' AND json_extract(config, '$.worktree.repo') IS NOT NULL",
        )?;
        let rows = stmt.query_map([], run_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One run by id, however old it is.
    pub fn run(&self, id: &str) -> Result<Option<Value>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, goal, cwd, config, status, created_at, kind FROM runs WHERE id = ?1",
                params![id],
                run_row,
            )
            .optional()?)
    }

    /// Remove a run's facts and the indexes beside them. The run's directories under Janus home
    /// are the caller's; the event log goes with the row it belongs to.
    pub fn delete_run(&self, run_id: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        for table in ["events", "attempts", "artifacts", "verifications", "commands"] {
            tx.execute(&format!("DELETE FROM {table} WHERE run_id = ?1"), params![run_id])?;
        }
        tx.execute("DELETE FROM runs WHERE id = ?1", params![run_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn start_attempt(&self, run_id: &str, task: &str, n: i64, agent: &str, runtime: &str, now: i64) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO attempts (run_id, task, n, agent, runtime, status, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run_id, task, n, agent, runtime, AttemptStatus::Starting.as_str(), now],
        )?;
        Ok(())
    }

    pub fn update_attempt(
        &self,
        run_id: &str,
        task: &str,
        n: i64,
        status: AttemptStatus,
        update: AttemptUpdate<'_>,
    ) -> Result<()> {
        let AttemptUpdate { session, workspace, error, ended } = update;
        self.0.lock().unwrap().execute(
            "UPDATE attempts SET status = ?4,
               session = COALESCE(?5, session), workspace = COALESCE(?6, workspace),
               error = COALESCE(?7, error), ended_at = COALESCE(?8, ended_at)
             WHERE run_id = ?1 AND task = ?2 AND n = ?3",
            params![run_id, task, n, status.as_str(), session, workspace, error, ended],
        )?;
        Ok(())
    }

    /// Attempts left running by a previous process; recovery closes them instead of guessing.
    pub fn orphaned_attempts(&self) -> Result<Vec<(String, String, i64)>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT run_id, task, n FROM attempts WHERE status IN ('starting', 'running')")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn unfinished_runs(&self) -> Result<Vec<String>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM runs WHERE status NOT IN ('delivered', 'accepted', 'cancelled', 'failed', 'interrupted')",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn insert_artifact(&self, a: &ArtifactRow<'_>) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO artifacts (run_id, task, attempt, name, kind, path, sha256, bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![a.run_id, a.task, a.attempt, a.name, a.kind, a.path, a.sha256, a.bytes, a.created_at],
        )?;
        Ok(())
    }

    pub fn insert_verification(&self, v: &VerificationRow<'_>) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO verifications (run_id, task, attempt, attempt_id, status, details, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![v.run_id, v.task, v.attempt, v.attempt_id, v.status, v.details.to_string(), v.created_at],
        )?;
        Ok(())
    }

    /// Claim a command id for a run before acting on it. Only the first claim of one id succeeds
    /// (`None`), so two requests carrying it cannot both act; a later claim gets the state the
    /// first one is in.
    pub fn claim_command(
        &self,
        run_id: &str,
        command_id: &str,
        request: &Value,
        now: i64,
    ) -> Result<Option<CommandState>> {
        let conn = self.0.lock().unwrap();
        let changed = conn.execute(
            "INSERT OR IGNORE INTO commands (run_id, command_id, result, created_at, state) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, command_id, request.to_string(), now, CommandState::Running.as_str()],
        )?;
        if changed == 1 {
            return Ok(None);
        }
        let state: String = conn.query_row(
            "SELECT state FROM commands WHERE run_id = ?1 AND command_id = ?2",
            params![run_id, command_id],
            |r| r.get(0),
        )?;
        CommandState::parse(&state).map(Some).ok_or_else(|| anyhow::anyhow!("命令状态 {state} 无法识别"))
    }

    /// Record that a claimed command's action took effect.
    pub fn finish_command(&self, run_id: &str, command_id: &str) -> Result<()> {
        self.0.lock().unwrap().execute(
            "UPDATE commands SET state = ?3 WHERE run_id = ?1 AND command_id = ?2",
            params![run_id, command_id, CommandState::Done.as_str()],
        )?;
        Ok(())
    }

    /// Give a claimed command id back after its action failed, so a retry can act.
    pub fn release_command(&self, run_id: &str, command_id: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM commands WHERE run_id = ?1 AND command_id = ?2", params![run_id, command_id])?;
        Ok(())
    }

    /// Commands a previous process was still carrying out: whether they took effect is unknown.
    pub fn interrupt_commands(&self) -> Result<()> {
        self.0.lock().unwrap().execute(
            "UPDATE commands SET state = ?1 WHERE state = ?2",
            params![CommandState::Interrupted.as_str(), CommandState::Running.as_str()],
        )?;
        Ok(())
    }

    pub fn put_config(&self, kind: &str, id: &str, data: &Value, now: i64) -> Result<()> {
        // Every write in tests must have the shape the current migrations leave behind.
        #[cfg(test)]
        {
            let problems = crate::data_shape::check(kind, data);
            assert!(problems.is_empty(), "{kind} {id} 的字段不符合登记的数据结构：{problems:?}");
        }
        self.0.lock().unwrap().execute(
            "INSERT INTO configs (kind, id, data, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (kind, id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
            params![kind, id, data.to_string(), now],
        )?;
        Ok(())
    }

    pub fn get_config(&self, kind: &str, id: &str) -> Result<Option<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT data FROM configs WHERE kind = ?1 AND id = ?2")?;
        let mut rows = stmt.query(params![kind, id])?;
        Ok(match rows.next()? {
            Some(row) => Some(serde_json::from_str(&row.get::<_, String>(0)?)?),
            None => None,
        })
    }

    pub fn list_config(&self, kind: &str) -> Result<Vec<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT data FROM configs WHERE kind = ?1 ORDER BY rowid")?;
        let rows = stmt.query_map(params![kind], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn delete_config(&self, kind: &str, id: &str) -> Result<()> {
        self.0.lock().unwrap().execute("DELETE FROM configs WHERE kind = ?1 AND id = ?2", params![kind, id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_db(name: &str) -> (Db, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("janus-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (Db::open(&path).unwrap().0, path)
    }

    /// Paging walks every run once, newest first, also through runs created in the same
    /// millisecond, and keeps to one kind and one workspace when asked.
    #[test]
    fn pages_list_every_run_once() {
        let (db, path) = temp_db("pages");
        for i in 0..130 {
            let ws = if i % 2 == 0 { "w1" } else { "w2" };
            // Three runs share each millisecond.
            db.insert_run(
                &format!("r{i:03}"),
                RunKind::Goal,
                "g",
                "/tmp",
                &json!({ "workspace": { "id": ws } }),
                i / 3,
            )
            .unwrap();
        }
        db.insert_run("c1", RunKind::Chat, "", "/tmp", &json!({}), 5).unwrap();
        let walk = |workspace: Option<&str>, limit: usize| {
            let (mut seen, mut before) = (vec![], None);
            loop {
                let page = RunsPage { kind: RunKind::Goal, workspace: workspace.map(str::to_owned), before, limit };
                let (runs, more) = db.runs_page(&page).unwrap();
                seen.extend(runs.iter().map(|r| r["id"].as_str().unwrap().to_string()));
                let last = runs.last().unwrap();
                before = Some((last["createdAt"].as_i64().unwrap(), last["id"].as_str().unwrap().to_string()));
                if !more {
                    return seen;
                }
            }
        };
        let all = walk(None, 50);
        assert_eq!(all.len(), 130, "every goal, no chat");
        assert_eq!(all.iter().collect::<std::collections::HashSet<_>>().len(), 130, "none twice");
        assert_eq!(all[0], "r129", "newest first");
        assert_eq!(walk(None, 7), all, "the page size does not change the order");
        let w1 = walk(Some("w1"), 20);
        assert_eq!(w1.len(), 65);
        assert!(w1.iter().all(|id| id[1..].parse::<u32>().unwrap() % 2 == 0), "only the workspace's runs");
        let _ = std::fs::remove_file(path);
    }

    /// The app's list stops at the newest 50 runs; a run by id is found however old it is.
    #[test]
    fn a_run_past_the_newest_fifty_is_still_found() {
        let (db, path) = temp_db("run-lookup");
        for i in 0..60 {
            db.insert_run(&format!("r{i}"), RunKind::Goal, "test", "/tmp", &json!({}), i).unwrap();
        }
        assert_eq!(db.runs(RunKind::Goal).unwrap().len(), 50);
        assert!(!db.runs(RunKind::Goal).unwrap().iter().any(|r| r["id"] == "r0"), "the oldest is not on the list");
        assert_eq!(db.run("r0").unwrap().unwrap()["createdAt"], 0);
        assert!(db.run("nope").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_command_id_is_claimed_once_until_it_is_released() {
        let (db, path) = temp_db("commands");
        db.insert_run("r1", RunKind::Goal, "test", "/tmp", &json!({}), 1).unwrap();
        let claim = |id: &str| db.claim_command("r1", id, &json!({ "type": "pause" }), 2).unwrap();
        assert_eq!(claim("c1"), None);
        assert_eq!(claim("c1"), Some(CommandState::Running), "a second claim of one id fails");
        db.finish_command("r1", "c1").unwrap();
        assert_eq!(claim("c1"), Some(CommandState::Done));
        assert_eq!(claim("c2"), None);
        db.release_command("r1", "c2").unwrap();
        assert_eq!(claim("c2"), None, "a released id can be claimed again");
        // A restart leaves the unfinished one unknown; the finished one stays finished.
        db.interrupt_commands().unwrap();
        assert_eq!(claim("c2"), Some(CommandState::Interrupted));
        assert_eq!(claim("c1"), Some(CommandState::Done));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn deleting_a_run_removes_its_rows_and_leaves_other_runs_whole() {
        let (db, path) = temp_db("delete-run");
        for (id, created) in [("r1", 1), ("r2", 2)] {
            db.insert_run(id, RunKind::Goal, "g", "/tmp", &json!({}), created).unwrap();
            db.insert_event(&Event {
                run_id: id.into(),
                seq: 1,
                ts: created,
                kind: "run.created".into(),
                actor: "user".into(),
                to: None,
                task: None,
                attempt: None,
                text: "g".into(),
                data: None,
            })
            .unwrap();
            db.start_attempt(id, "T1", 1, "m", "codex", created).unwrap();
            db.insert_artifact(&ArtifactRow {
                run_id: id,
                task: "T1",
                attempt: 1,
                name: "T1-a1-report.md",
                kind: "report",
                path: "/p",
                sha256: "s",
                bytes: 1,
                created_at: created,
            })
            .unwrap();
            db.insert_verification(&VerificationRow {
                run_id: id,
                task: "T1",
                attempt: 1,
                attempt_id: "a",
                status: "passed",
                details: &json!({}),
                created_at: created,
            })
            .unwrap();
            db.claim_command(id, "c", &json!({}), created).unwrap();
        }
        db.delete_run("r1").unwrap();
        assert!(db.run("r1").unwrap().is_none());
        assert!(db.events("r1", 0).unwrap().is_empty());
        assert_eq!(db.run("r2").unwrap().unwrap()["goal"], "g");
        assert_eq!(db.events("r2", 0).unwrap().len(), 1);
        // Deleting it again changes nothing else.
        db.delete_run("r1").unwrap();
        assert!(db.run("r2").unwrap().is_some());
        let _ = std::fs::remove_file(path);
    }
}
