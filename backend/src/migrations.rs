//! Data upgrades. Everything Janus keeps in `janus.db` has a data version (`PRAGMA user_version`).
//! A release that changes what is stored (a table, a field of a setup object or of a run's
//! snapshot, the default roles) adds one `Migration` to the end of `MIGRATIONS`; the service runs
//! the missing ones when it opens the database, after copying it to `backups/`. Readers only ever
//! see the latest shape, so no code keeps reading old shapes. The events of a run are history and
//! are never rewritten.
// @project-doc docs/architecture/20261031-data_model.md#migrations

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, Transaction, params};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// One data version: what changed and how existing data gets there.
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub steps: &'static [Step],
}

/// A change to stored data. Field steps edit JSON objects in place and leave rows they do not
/// apply to alone, so running one twice changes nothing.
pub enum Step {
    /// Schema changes: new tables, columns, indexes.
    Sql(&'static str),
    /// A new field, with the value existing objects take.
    #[cfg_attr(not(test), expect(dead_code, reason = "no migration adds a field yet; the next one that does uses it"))]
    Add {
        at: Target,
        field: &'static str,
        default: fn() -> Value,
    },
    Drop {
        at: Target,
        field: &'static str,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no migration renames a field yet; the next one that does uses it")
    )]
    Rename {
        at: Target,
        from: &'static str,
        to: &'static str,
    },
    /// Anything else (a default computed from other fields, a type change); returns how many
    /// objects it changed.
    Custom(fn(&Transaction) -> Result<usize>),
}

/// Where a field step applies.
#[derive(Clone, Copy)]
pub enum Target {
    /// Setup objects of one kind in `configs` (`connection`, `role`, `workspace`, ...).
    Config(&'static str),
    /// Each member in a run's snapshot (`runs.config.team`), read back when a run resumes.
    RunMember,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "数字员工去掉「定位」，Run 快照的成员去掉定位和模型标签",
        steps: &[
            Step::Drop { at: Target::Config("role"), field: "role" },
            Step::Drop { at: Target::RunMember, field: "role" },
            Step::Drop { at: Target::RunMember, field: "model_label" },
        ],
    },
    Migration {
        version: 2,
        name: "默认数字员工记录来源，之后可以更新没改过的职责和指令",
        steps: &[Step::Custom(crate::agents::record_seed_origins)],
    },
    Migration {
        version: 3,
        name: "操作的 commandId 记录执行状态，服务重启时还在执行的标为中断",
        // Commands recorded before this version are ones whose action finished (a failed one
        // released its id); one a crash left behind could not be told apart and counts as done.
        steps: &[Step::Sql("ALTER TABLE commands ADD COLUMN state TEXT NOT NULL DEFAULT 'done';")],
    },
    Migration {
        version: 4,
        name: "去掉 Run 租约：一个 Janus home 只有一个服务，由目录锁保证",
        steps: &[Step::Sql("DROP TABLE IF EXISTS leases;")],
    },
    Migration {
        version: 5,
        name: "资源记录给谁用：Skill 给所有数字员工，MCP 只给选中它的数字员工",
        steps: &[Step::Custom(crate::library::record_reach)],
    },
    Migration {
        version: 6, name: "新增默认的通用数字员工", steps: &[Step::Custom(crate::agents::add_generalist)]
    },
    Migration {
        version: 7,
        name: "去掉预设的 Agent 类型：DeepSeek、MiniMax、GLM over Claude Code 改为用 API Key 登录的 Claude Code，原来的变量写进它自己的环境变量",
        steps: &[Step::Custom(crate::agents::drop_preset_kinds)],
    },
    Migration {
        version: 8,
        name: "命令启动的 MCP 服务器记录 MCP 路由怎样运行它：默认同一个任务共用一个进程",
        steps: &[Step::Custom(crate::library::record_mcp_process)],
    },
];

/// The data version this build reads and writes.
pub fn latest() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

/// The tables as data version 0 has them, the first release with migrations. Never edit this:
/// a schema change is a `Step::Sql` in a new migration, so old and new databases end up alike.
pub const BASELINE: &str = "
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
);
";

/// What opening the database did.
pub struct Report {
    pub from: u32,
    pub to: u32,
    pub backup: Option<PathBuf>,
    /// Version, name and number of objects changed, for each migration run.
    pub applied: Vec<(u32, &'static str, usize)>,
}

/// Backups kept in `<home>/backups`; older ones are removed.
const KEEP_BACKUPS: usize = 5;

/// Bring a database to `latest()`. A database with data is copied to `<dir of db>/backups` first;
/// each migration runs in its own transaction with its version. A database from a newer Janus is
/// refused: this build does not know what that data means and could damage it.
pub fn upgrade(conn: &mut Connection, db: Option<&Path>) -> Result<Report> {
    let fresh: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0))?;
    let from: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // Only a database at version 0 (new, or from before migrations) still needs the baseline:
    // later ones have it already, and running it again would bring back tables a migration dropped.
    if from == 0 {
        conn.execute_batch(BASELINE)?;
    }
    let to = latest();
    if from > to {
        let backups = db.and_then(Path::parent).map(|d| d.join("backups").display().to_string()).unwrap_or_default();
        bail!(
            "这份数据来自更新的 Janus（数据版本 {from}，这个版本的 Janus 只支持到 {to}）。请升级 Janus；要退回这个版本，先从 {backups} 恢复升级前的备份。"
        );
    }
    let mut report = Report { from, to, backup: None, applied: vec![] };
    if from == to {
        return Ok(report);
    }
    if fresh > 0
        && let Some(db) = db
    {
        report.backup = Some(backup(conn, db, from)?);
    }
    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        let tx = conn.transaction()?;
        let mut changed = 0;
        for step in m.steps {
            changed += apply(&tx, step).with_context(|| format!("数据升级 {}「{}」失败", m.version, m.name))?;
        }
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
        report.applied.push((m.version, m.name, changed));
    }
    Ok(report)
}

/// Copy the database to `backups/janus-v<version>-<ms>.db` and keep the newest few.
fn backup(conn: &Connection, db: &Path, version: u32) -> Result<PathBuf> {
    let dir = db.parent().unwrap_or(Path::new(".")).join("backups");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("janus-v{version}-{}.db", crate::orchestrator::now_ms()));
    conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()]).context("升级前备份数据库失败")?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("janus-v") && n.ends_with(".db"))
        })
        .collect();
    old.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let excess = old.len().saturating_sub(KEEP_BACKUPS);
    for p in old.into_iter().take(excess) {
        let _ = std::fs::remove_file(p);
    }
    Ok(path)
}

fn apply(tx: &Transaction, step: &Step) -> Result<usize> {
    match step {
        Step::Sql(sql) => {
            tx.execute_batch(sql)?;
            Ok(0)
        }
        Step::Add { at, field, default } => edit(tx, *at, |o| {
            if o.contains_key(*field) {
                return false;
            }
            o.insert(field.to_string(), default());
            true
        }),
        Step::Drop { at, field } => edit(tx, *at, |o| o.remove(*field).is_some()),
        Step::Rename { at, from, to } => edit(tx, *at, |o| {
            if o.contains_key(*to) {
                return false;
            }
            match o.remove(*from) {
                Some(v) => {
                    o.insert(to.to_string(), v);
                    true
                }
                None => false,
            }
        }),
        Step::Custom(f) => f(tx),
    }
}

/// Apply `f` to every object at `at`, writing back the ones it changed. Returns how many changed.
fn edit(tx: &Transaction, at: Target, mut f: impl FnMut(&mut Map<String, Value>) -> bool) -> Result<usize> {
    match at {
        Target::Config(kind) => edit_configs(tx, kind, |_, o| f(o)),
        Target::RunMember => {
            let rows: Vec<(String, String)> = tx
                .prepare("SELECT id, config FROM runs")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut changed = 0;
            for (id, config) in rows {
                let Ok(mut config) = serde_json::from_str::<Value>(&config) else { continue };
                let mut any = false;
                for m in config["team"].as_array_mut().into_iter().flatten() {
                    if let Some(o) = m.as_object_mut()
                        && f(o)
                    {
                        any = true;
                        changed += 1;
                    }
                }
                if any {
                    tx.execute("UPDATE runs SET config = ?1 WHERE id = ?2", params![config.to_string(), id])?;
                }
            }
            Ok(changed)
        }
    }
}

/// Apply `f` (given the id) to every setup object of `kind`, writing back the ones it changed.
pub fn edit_configs(
    tx: &Transaction,
    kind: &str,
    mut f: impl FnMut(&str, &mut Map<String, Value>) -> bool,
) -> Result<usize> {
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, data FROM configs WHERE kind = ?1")?
        .query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = 0;
    for (id, data) in rows {
        let Ok(mut data) = serde_json::from_str::<Value>(&data) else { continue };
        let Some(o) = data.as_object_mut() else { continue };
        if f(&id, o) {
            tx.execute(
                "UPDATE configs SET data = ?1 WHERE kind = ?2 AND id = ?3",
                params![data.to_string(), kind, id],
            )?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// What upgrading the database at `db` would do, worked out on a copy: the migrations it would
/// run and anything still not in the latest shape afterwards. The database itself is untouched.
pub fn dry_run(db: &Path) -> Result<String> {
    if !db.exists() {
        return Ok(format!("{} 不存在：第一次启动时直接建成数据版本 {}。\n", db.display(), latest()));
    }
    let copy = std::env::temp_dir().join(format!("janus-dry-run-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&copy);
    Connection::open(db)?.execute("VACUUM INTO ?1", params![copy.to_string_lossy()])?;
    let result = (|| {
        let mut conn = Connection::open(&copy)?;
        let report = upgrade(&mut conn, None)?;
        Ok::<_, anyhow::Error>((report, problems(&conn)?))
    })();
    let _ = std::fs::remove_file(&copy);
    let (report, problems) = result?;
    let mut out = if report.applied.is_empty() {
        format!("数据版本 {}，已是最新。\n", report.from)
    } else {
        let mut out = format!("数据版本 {} → {}：\n", report.from, report.to);
        for (version, name, changed) in &report.applied {
            out.push_str(&format!("  {version}「{name}」：改动 {changed} 处\n"));
        }
        out
    };
    if problems.is_empty() {
        out.push_str("升级后所有数据都符合当前的数据结构。\n");
    } else {
        out.push_str(&format!("升级后仍有 {} 处不符合当前的数据结构：\n", problems.len()));
        for p in &problems {
            out.push_str(&format!("  {p}\n"));
        }
    }
    Ok(out)
}

/// Every stored setup object and run member that does not have the latest shape, as
/// `kind id: problem`.
pub fn problems(conn: &Connection) -> Result<Vec<String>> {
    let mut out = vec![];
    let configs: Vec<(String, String, String)> = conn
        .prepare("SELECT kind, id, data FROM configs ORDER BY kind, id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (kind, id, data) in configs {
        let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        out.extend(crate::data_shape::check(&kind, &data).into_iter().map(|p| format!("{kind} {id}: {p}")));
    }
    let runs: Vec<(String, String)> = conn
        .prepare("SELECT id, config FROM runs ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, config) in runs {
        let config: Value = serde_json::from_str(&config).unwrap_or(Value::Null);
        for m in config["team"].as_array().into_iter().flatten() {
            out.extend(
                crate::data_shape::check_member(m)
                    .into_iter()
                    .map(|p| format!("run {id} 成员 {}: {p}", m["id"].as_str().unwrap_or("?"))),
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
