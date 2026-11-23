//! Data upgrades. Everything Janus keeps in `janus.db` has a data version (`PRAGMA user_version`).
//! A release that changes what is stored (a table, a field of a setup object or of a run's
//! snapshot, the default roles) adds one `Migration` to the end of `MIGRATIONS`; the service runs
//! the missing ones when it opens the database, after copying it to `backups/`. Readers only ever
//! see the latest shape, so no code keeps reading old shapes. The events of a run are history and
//! are never rewritten, except for where a file now is when a migration moves Janus home's folders
//! (`layout::Relocation`).
// @project-doc docs/architecture/20261031-data_model.md#migrations

use crate::layout::Layout;
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
    Add {
        at: Target,
        field: &'static str,
        default: fn() -> Value,
    },
    Drop {
        at: Target,
        field: &'static str,
    },
    /// Anything else (a default computed from other fields, a type change); returns how many
    /// objects it changed.
    Custom(fn(&Transaction) -> Result<usize>),
    /// A change that also reaches files in Janus home (credentials, a move of folders). It gets
    /// the home as that data version laid it out; the dry run passes none and changes no file.
    Home(fn(&Transaction, Option<&Path>) -> Result<usize>),
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
    Migration {
        version: 9,
        name: "去掉「自定义命令」的 Agent：删除这类 Agent，绑定它的数字员工改为没有绑定 Agent，旧 Run 里用它的成员记为 Claude Code（只能查看，不能继续）",
        steps: &[Step::Custom(crate::agents::drop_custom_agents)],
    },
    Migration {
        version: 10,
        name: "worktree 中的 Run 记录验收时合并回哪个分支：旧 Run 取升级时工作目录检出、并且包含 Run 起点的分支",
        steps: &[Step::Custom(crate::worktrees::record_source_branch)],
    },
    Migration {
        version: 11,
        name: "资源库的每项资源都给所有数字员工：资源和 Run 快照中的资源去掉「给谁用」，数字员工去掉选中的资源",
        steps: &[
            Step::Custom(crate::library::drop_reach),
            Step::Drop { at: Target::Config("role"), field: "resources" },
        ],
    },
    Migration {
        version: 12,
        name: "Run 记录是目标还是和一名数字员工的对话：已有的 Run 都是目标",
        steps: &[Step::Sql("ALTER TABLE runs ADD COLUMN kind TEXT NOT NULL DEFAULT 'goal';")],
    },
    Migration {
        version: 13,
        name: "Claude 的自定义 API 改用环境变量：单独保存的 API Key 成为 ANTHROPIC_API_KEY，私有变量移出数据库",
        steps: &[Step::Home(crate::agents::migrate_claude_env)],
    },
    Migration {
        version: 14,
        name: "Janus home 按类别整理：data、work、cache、tmp、logs，任务和对话分开存放",
        steps: &[Step::Home(crate::layout::move_to_categories)],
    },
    Migration {
        version: 15,
        name: "新增默认的数字员工「老友」",
        steps: &[Step::Custom(crate::agents::add_old_friend)],
    },
    Migration {
        version: 16,
        name: "数字员工可以有自己的头像：已有的数字员工没有头像，显示文字",
        steps: &[Step::Add { at: Target::Config("role"), field: "avatar", default: || serde_json::json!("") }],
    },
    Migration {
        version: 17,
        name: "默认数字员工的 id 改为它的职能：pmo、marketing、developer、qa、friend",
        steps: &[Step::Home(crate::agents::rename_default_roles)],
    },
    Migration {
        version: 18,
        name: "去掉任务的时间和费用上限：等你追加预算的任务改为已中断，快照去掉费用上限",
        steps: &[Step::Sql(
            "INSERT INTO events (run_id, seq, ts, type, actor, text)
               SELECT id, (SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE run_id = runs.id),
                      CAST(strftime('%s', 'now') AS INTEGER) * 1000, 'run.interrupted', 'janus',
                      '任务不再有时间和费用上限：等你追加预算的任务改为已中断，可以从中断处继续'
               FROM runs WHERE status = 'waiting_input';
             UPDATE runs SET status = 'interrupted' WHERE status = 'waiting_input';
             UPDATE runs SET config = json_remove(config, '$.costLimitUsd') WHERE json_type(config, '$.costLimitUsd') IS NOT NULL;",
        )],
    },
    Migration {
        version: 19,
        name: "工作区 Loop：Run 记录是谁派出的（用户或哪个 Loop）、派发的 key 和来源链接，同一工作区的 key 不重复",
        // Every run so far was the person's own.
        steps: &[Step::Custom(add_run_origin)],
    },
    Migration {
        version: 20,
        name: "值班的节奏改为时间表（cron）：固定间隔换成对应的 cron，自己决定换成每 30 分钟，只手动换成不按时间",
        steps: &[Step::Custom(rhythm_to_schedule)],
    },
    Migration {
        version: 21,
        name: "本机环境改为默认镜像用户目录、只挡列出的路径：去掉原来逐项共享的记录",
        steps: &[Step::Custom(drop_host_links)],
    },
    Migration {
        version: 22,
        name: "工作区的团队记忆：Agent 在 Run 里记下、之后的 Run 读到的笔记，记着是谁在哪个 Run 里写的",
        steps: &[Step::Sql(
            "CREATE TABLE IF NOT EXISTS memories (
               id TEXT PRIMARY KEY,
               workspace TEXT NOT NULL,
               kind TEXT NOT NULL,
               title TEXT NOT NULL,
               hook TEXT NOT NULL,
               body TEXT NOT NULL,
               run_id TEXT,
               who TEXT NOT NULL,
               author TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS memories_workspace ON memories (workspace, updated_at);",
        )],
    },
];

/// Data version 21: agents' HOME mirrors the person's home folder but a hidden list (`host_env`),
/// which starts as the defaults; the paths shared one by one before (`host_link`) mean nothing now.
fn drop_host_links(tx: &Transaction) -> Result<usize> {
    Ok(tx.execute("DELETE FROM configs WHERE kind = 'host_link'", [])?)
}

/// Data version 20: a Loop's `rhythm` becomes a `schedule` in the service's time zone. An interval
/// comes round on the clock now (every 30 minutes: on the hour and the half hour); a self-paced
/// Loop wakes every 30 minutes and can still ask for another time; a manual one has none.
fn rhythm_to_schedule(tx: &Transaction) -> Result<usize> {
    let tz = crate::schedule::local_zone();
    edit_configs(tx, "loop", |_, o| {
        let Some(rhythm) = o.remove("rhythm") else { return false };
        let cron = match rhythm["kind"].as_str() {
            Some("interval") => match rhythm["minutes"].as_u64().unwrap_or(30).max(1) {
                m if m < 60 => Some(format!("*/{m} * * * *")),
                m if m < 24 * 60 => Some(format!("0 */{} * * *", m / 60)),
                _ => Some("0 0 * * *".to_string()),
            },
            Some("manual") => None,
            _ => Some("*/30 * * * *".to_string()),
        };
        o.insert("schedule".into(), cron.map_or(Value::Null, |c| serde_json::json!({ "cron": c, "tz": tz })));
        true
    })
}

/// Data version 19: `runs` records who a goal run is for (`origin`, `loop_id`) and a Loop's
/// dispatch key and source, the key unique in its workspace. Columns already there are left alone,
/// so it can run again.
fn add_run_origin(tx: &Transaction) -> Result<usize> {
    let has: Vec<String> = tx
        .prepare("SELECT name FROM pragma_table_info('runs')")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut added = 0;
    for (column, ty) in [
        ("origin", "TEXT NOT NULL DEFAULT 'user'"),
        ("loop_id", "TEXT"),
        ("dispatch_key", "TEXT"),
        ("source_url", "TEXT"),
    ] {
        if !has.iter().any(|c| c == column) {
            tx.execute_batch(&format!("ALTER TABLE runs ADD COLUMN {column} {ty};"))?;
            added += 1;
        }
    }
    tx.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS runs_dispatch_key ON runs (json_extract(config, '$.workspace.id'), dispatch_key)
           WHERE dispatch_key IS NOT NULL;
         CREATE INDEX IF NOT EXISTS runs_loop ON runs (loop_id) WHERE loop_id IS NOT NULL;",
    )?;
    Ok(added)
}

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

/// Bring a database to `latest()`. A database with data is copied to `backups/` of its home first;
/// each migration runs in its own transaction with its version. A database from a newer Janus is
/// refused: this build does not know what that data means and could damage it.
pub fn upgrade(conn: &mut Connection, layout: Option<&Layout>) -> Result<Report> {
    let fresh: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0))?;
    let from: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // Only a database at version 0 (new, or from before migrations) still needs the baseline:
    // later ones have it already, and running it again would bring back tables a migration dropped.
    if from == 0 {
        conn.execute_batch(BASELINE)?;
    }
    let to = latest();
    if from > to {
        let backups = layout.map(|l| l.backups().display().to_string()).unwrap_or_default();
        bail!(
            "这份数据来自更新的 Janus（数据版本 {from}，这个版本的 Janus 只支持到 {to}）。请升级 Janus；要退回这个版本，先从 {backups} 恢复升级前的备份。"
        );
    }
    let mut report = Report { from, to, backup: None, applied: vec![] };
    if from == to {
        return Ok(report);
    }
    if fresh > 0
        && let Some(layout) = layout
    {
        report.backup = Some(backup(conn, layout, from)?);
    }
    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        let tx = conn.transaction()?;
        let mut changed = 0;
        for step in m.steps {
            changed += apply(&tx, step, layout.map(Layout::home))
                .with_context(|| format!("数据升级 {}「{}」失败", m.version, m.name))?;
        }
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
        report.applied.push((m.version, m.name, changed));
    }
    Ok(report)
}

/// Copy the database to `backups/janus-v<version>-<ms>.db` and keep the newest few.
fn backup(conn: &Connection, layout: &Layout, version: u32) -> Result<PathBuf> {
    let dir = layout.backups();
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

fn apply(tx: &Transaction, step: &Step, home: Option<&Path>) -> Result<usize> {
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
        Step::Custom(f) => f(tx),
        Step::Home(f) => f(tx, home),
    }
}

/// Apply `f` to every object at `at`, writing back the ones it changed. Returns how many changed.
pub(crate) fn edit(tx: &Transaction, at: Target, mut f: impl FnMut(&mut Map<String, Value>) -> bool) -> Result<usize> {
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
    try_edit_configs(tx, kind, |id, config| Ok(f(id, config)))
}

pub(crate) fn try_edit_configs(
    tx: &Transaction,
    kind: &str,
    mut f: impl FnMut(&str, &mut Map<String, Value>) -> Result<bool>,
) -> Result<usize> {
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, data FROM configs WHERE kind = ?1")?
        .query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = 0;
    for (id, data) in rows {
        let Ok(mut data) = serde_json::from_str::<Value>(&data) else { continue };
        let Some(o) = data.as_object_mut() else { continue };
        if f(&id, o)? {
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
