//! The library's data versions: how the MCP router runs a command server (8), and items no
//! longer saying who may use them (11).

use crate::db::ConfigKind;
use anyhow::Result;
use serde_json::{Value, json};

/// Data version 11: every item is for every digital worker, so items, and the items pinned in
/// runs' member snapshots, no longer say who may use them.
pub(crate) fn drop_reach(tx: &rusqlite::Transaction) -> Result<usize> {
    edit_items(tx, |o| o.remove("reach").is_some())
}

/// Data version 8: each command server says how the MCP router runs it; ones from before run
/// once per run, the default.
pub(crate) fn record_mcp_process(tx: &rusqlite::Transaction) -> Result<usize> {
    edit_items(tx, |o| {
        let Some(server) = o.get_mut("server").and_then(Value::as_object_mut) else { return false };
        if server.get("transport").and_then(Value::as_str) != Some("stdio") || server.contains_key("process") {
            return false;
        }
        server.insert("process".into(), json!("perRun"));
        true
    })
}

/// Apply `fill` to every library item: the library's own, and the copies pinned in runs' team
/// snapshots (read back when a run resumes). Returns how many objects it changed.
pub(crate) fn edit_items(
    tx: &rusqlite::Transaction,
    fill: impl Fn(&mut serde_json::Map<String, Value>) -> bool,
) -> Result<usize> {
    let mut changed = super::super::edit_configs(tx, ConfigKind::LibraryItem, |_, o| fill(o))?;
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, config FROM runs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, config) in rows {
        let Ok(mut config) = serde_json::from_str::<Value>(&config) else { continue };
        let mut any = false;
        for m in config["team"].as_array_mut().into_iter().flatten() {
            for r in m["resources"].as_array_mut().into_iter().flatten() {
                if let Some(o) = r.as_object_mut()
                    && fill(o)
                {
                    any = true;
                    changed += 1;
                }
            }
        }
        if any {
            tx.execute("UPDATE runs SET config = ?1 WHERE id = ?2", rusqlite::params![config.to_string(), id])?;
        }
    }
    Ok(changed)
}
