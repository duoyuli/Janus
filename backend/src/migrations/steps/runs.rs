//! Runs' data versions: the branch a worktree run goes back to (10), and who a goal run is for (19).

use anyhow::Result;
use rusqlite::Transaction;
use std::path::PathBuf;

/// Data version 10: a run in a worktree records the branch accepting brings its result back to
/// (`runs.config.worktree.branch`, `null` for a detached `HEAD`). Runs from before take the branch
/// the checkout is on now when the run's start is in its history, so replaying the run's commits
/// there replays only them; otherwise `null`, and accepting such a run leaves merging its commit
/// to the person unless the checkout is detached where the run can fast-forward it.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn record_source_branch(tx: &rusqlite::Transaction) -> Result<usize> {
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, config FROM runs WHERE json_extract(config, '$.worktree.repo') IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = 0;
    for (id, config) in rows {
        let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&config) else { continue };
        if config["worktree"].get("branch").is_some() {
            continue;
        }
        let repo = PathBuf::from(config["worktree"]["repo"].as_str().unwrap_or_default());
        let base = config["baseRef"].as_str().unwrap_or_default();
        let branch = crate::worktrees::current_branch(&repo).filter(|b| {
            let tip =
                crate::git::read(&repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}^{{commit}}")]);
            tip.is_some_and(|tip| {
                !base.is_empty()
                    && crate::git::read(&repo, &["merge-base", "--is-ancestor", base, tip.trim()]).is_some()
            })
        });
        config["worktree"]["branch"] = serde_json::json!(branch);
        tx.execute("UPDATE runs SET config = ?1 WHERE id = ?2", rusqlite::params![config.to_string(), id])?;
        changed += 1;
    }
    Ok(changed)
}

/// Data version 19: `runs` records who a goal run is for (`origin`, `loop_id`) and a Loop's
/// dispatch key and source, the key unique in its workspace. Columns already there are left alone,
/// so it can run again.
pub(crate) fn add_run_origin(tx: &Transaction) -> Result<usize> {
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
