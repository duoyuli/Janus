//! Settings' data versions: a Loop's rhythm becoming a schedule (20), and the per-path sharing of
//! the person's home that the mirrored HOME replaced (21).

use super::super::edit_configs;
use crate::db::ConfigKind;
use anyhow::Result;
use rusqlite::Transaction;
use serde_json::Value;

/// Data version 20: a Loop's `rhythm` becomes a `schedule` in the service's time zone. An interval
/// comes round on the clock now (every 30 minutes: on the hour and the half hour); a self-paced
/// Loop wakes every 30 minutes and can still ask for another time; a manual one has none.
pub(crate) fn rhythm_to_schedule(tx: &Transaction) -> Result<usize> {
    let tz = crate::schedule::local_zone();
    edit_configs(tx, ConfigKind::Loop, |_, o| {
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

/// Data version 21: agents' HOME mirrors the person's home folder but a hidden list (`host_env`),
/// which starts as the defaults; the paths shared one by one before (`host_link`) mean nothing now.
pub(crate) fn drop_host_links(tx: &Transaction) -> Result<usize> {
    Ok(tx.execute("DELETE FROM configs WHERE kind = 'host_link'", [])?)
}
