//! SQLite fact store. Events are the source of truth; `runs`, `attempts` and `artifacts`
//! are indexes kept alongside them so the scheduler and UI can query without replaying.

use crate::status::{AttemptStatus, RunStatus};
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

/// A `runs` row as the API and the orchestrator read it.
fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "goal": r.get::<_, String>(1)?,
        "cwd": r.get::<_, String>(2)?,
        "config": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
        "status": r.get::<_, String>(4)?,
        "createdAt": r.get::<_, i64>(5)?,
    }))
}

impl Db {
    // @project-doc docs/architecture/20261031-data_model.md#sqlite_tables
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS runs (
               id TEXT PRIMARY KEY,
               goal TEXT NOT NULL,
               cwd TEXT NOT NULL,
               config TEXT NOT NULL,
               status TEXT NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS events (
               run_id TEXT NOT NULL REFERENCES runs(id),
               seq INTEGER NOT NULL,
               ts INTEGER NOT NULL,
               type TEXT NOT NULL,
               actor TEXT NOT NULL,
               to_agent TEXT,
               task TEXT,
               attempt INTEGER,
               text TEXT NOT NULL,
               data TEXT,
               PRIMARY KEY (run_id, seq)
             );
             CREATE INDEX IF NOT EXISTS events_type_ts ON events (type, ts);
             CREATE TABLE IF NOT EXISTS attempts (
               run_id TEXT NOT NULL REFERENCES runs(id),
               task TEXT NOT NULL,
               n INTEGER NOT NULL,
               agent TEXT NOT NULL,
               runtime TEXT NOT NULL,
               session TEXT,
               workspace TEXT,
               status TEXT NOT NULL,
               error TEXT,
               started_at INTEGER NOT NULL,
               ended_at INTEGER,
               PRIMARY KEY (run_id, task, n)
             );
             CREATE TABLE IF NOT EXISTS artifacts (
               run_id TEXT NOT NULL REFERENCES runs(id),
               task TEXT NOT NULL,
               attempt INTEGER NOT NULL,
               name TEXT NOT NULL,
               kind TEXT NOT NULL,
               path TEXT NOT NULL,
               sha256 TEXT NOT NULL,
               bytes INTEGER NOT NULL,
               created_at INTEGER NOT NULL
             );
             -- Setup objects: agent connections, roles and execution environments.
             -- Secrets never go here; they stay in each connection's own directory.
             CREATE TABLE IF NOT EXISTS configs (
               kind TEXT NOT NULL,
               id TEXT NOT NULL,
               data TEXT NOT NULL,
               updated_at INTEGER NOT NULL,
               PRIMARY KEY (kind, id)
             );
             CREATE TABLE IF NOT EXISTS verifications (
               run_id TEXT NOT NULL REFERENCES runs(id),
               task TEXT NOT NULL,
               attempt INTEGER NOT NULL,
               attempt_id TEXT NOT NULL,
               status TEXT NOT NULL,
               details TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               PRIMARY KEY (run_id, task, attempt, attempt_id)
             );
             CREATE TABLE IF NOT EXISTS commands (
               run_id TEXT NOT NULL REFERENCES runs(id),
               command_id TEXT NOT NULL,
               result TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               PRIMARY KEY (run_id, command_id)
             );
             CREATE TABLE IF NOT EXISTS leases (
               run_id TEXT NOT NULL REFERENCES runs(id),
               name TEXT NOT NULL,
               owner TEXT NOT NULL,
               fencing INTEGER NOT NULL,
               expires_at INTEGER NOT NULL,
               PRIMARY KEY (run_id, name)
             );",
        )?;
        Ok(Db(Mutex::new(conn)))
    }

    pub fn insert_run(&self, id: &str, goal: &str, cwd: &str, config: &Value, now: i64) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES (?1, ?2, ?3, ?4, 'planning', ?5)",
            params![id, goal, cwd, config.to_string(), now],
        )?;
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

    /// The newest 50 runs, for the app's list and the role statistics.
    pub fn runs(&self) -> Result<Vec<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, goal, cwd, config, status, created_at FROM runs ORDER BY created_at DESC LIMIT 50")?;
        let rows = stmt.query_map([], run_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One run by id, however old it is.
    pub fn run(&self, id: &str) -> Result<Option<Value>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT id, goal, cwd, config, status, created_at FROM runs WHERE id = ?1", params![id], run_row)
            .optional()?)
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

    /// Claim a command id for a run before acting on it. Only the first claim of one id succeeds,
    /// so two requests carrying it cannot both act.
    pub fn claim_command(&self, run_id: &str, command_id: &str, request: &Value, now: i64) -> Result<bool> {
        let changed = self.0.lock().unwrap().execute(
            "INSERT OR IGNORE INTO commands (run_id, command_id, result, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![run_id, command_id, request.to_string(), now],
        )?;
        Ok(changed == 1)
    }

    /// Give a claimed command id back after its action failed, so a retry can act.
    pub fn release_command(&self, run_id: &str, command_id: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM commands WHERE run_id = ?1 AND command_id = ?2", params![run_id, command_id])?;
        Ok(())
    }

    pub fn acquire_lease(&self, run_id: &str, name: &str, owner: &str, now: i64, ttl_ms: i64) -> Result<Option<i64>> {
        let conn = self.0.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        let existing: Option<(String, i64, i64)> = tx
            .query_row(
                "SELECT owner, fencing, expires_at FROM leases WHERE run_id = ?1 AND name = ?2",
                params![run_id, name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let fencing = match existing {
            Some((current_owner, _current_fencing, expires))
                if expires > now && current_owner != owner && lease_owner_alive(&current_owner) =>
            {
                return Ok(None);
            }
            Some((current_owner, current_fencing, _)) if current_owner == owner => current_fencing,
            Some((_, current_fencing, _)) => current_fencing + 1,
            None => 1,
        };
        tx.execute(
            "INSERT INTO leases (run_id, name, owner, fencing, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(run_id, name) DO UPDATE SET owner=excluded.owner, fencing=excluded.fencing, expires_at=excluded.expires_at",
            params![run_id, name, owner, fencing, now + ttl_ms],
        )?;
        tx.commit()?;
        Ok(Some(fencing))
    }

    pub fn put_config(&self, kind: &str, id: &str, data: &Value, now: i64) -> Result<()> {
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

fn lease_owner_alive(owner: &str) -> bool {
    let Some(pid) = owner.strip_prefix("janus-") else {
        return true;
    };
    std::process::Command::new("kill").args(["-0", pid]).status().is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_db(name: &str) -> (Db, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("janus-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (Db::open(&path).unwrap(), path)
    }

    /// The app's list stops at the newest 50 runs; a run by id is found however old it is.
    #[test]
    fn a_run_past_the_newest_fifty_is_still_found() {
        let (db, path) = temp_db("run-lookup");
        for i in 0..60 {
            db.insert_run(&format!("r{i}"), "test", "/tmp", &json!({}), i).unwrap();
        }
        assert_eq!(db.runs().unwrap().len(), 50);
        assert!(!db.runs().unwrap().iter().any(|r| r["id"] == "r0"), "the oldest is not on the list");
        assert_eq!(db.run("r0").unwrap().unwrap()["createdAt"], 0);
        assert!(db.run("nope").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_command_id_is_claimed_once_until_it_is_released() {
        let (db, path) = temp_db("commands");
        db.insert_run("r1", "test", "/tmp", &json!({}), 1).unwrap();
        assert!(db.claim_command("r1", "c1", &json!({ "type": "pause" }), 2).unwrap());
        assert!(
            !db.claim_command("r1", "c1", &json!({ "type": "pause" }), 3).unwrap(),
            "a second claim of one id fails"
        );
        assert!(db.claim_command("r1", "c2", &json!({ "type": "pause" }), 3).unwrap());
        db.release_command("r1", "c1").unwrap();
        assert!(
            db.claim_command("r1", "c1", &json!({ "type": "pause" }), 4).unwrap(),
            "a released id can be claimed again"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn lease_fencing_rejects_live_other_owner_and_increments_after_expiry() {
        let (db, path) = temp_db("leases");
        db.insert_run("r1", "test", "/tmp", &json!({}), 1).unwrap();
        assert_eq!(db.acquire_lease("r1", "run", "a", 10, 100).unwrap(), Some(1));
        assert_eq!(db.acquire_lease("r1", "run", "b", 20, 100).unwrap(), None);
        assert_eq!(db.acquire_lease("r1", "run", "b", 200, 100).unwrap(), Some(2));
        let _ = std::fs::remove_file(path);
    }
}
