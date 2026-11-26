//! What the person did with inbox entries (`inbox_marks`): put one off until a time, or stopped
//! being reminded of it. Kept by the service, so every client shows the same inbox; nothing about
//! the run itself changes.
// @project-doc docs/architecture/20261031-frontend.md#inbox

use super::Db;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

/// One entry's mark.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InboxMark {
    pub snoozed_until: Option<i64>,
    pub ignored: bool,
}

impl Db {
    /// Every entry's mark, by its key.
    pub fn inbox_marks(&self) -> Result<HashMap<String, InboxMark>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, snoozed_until, ignored FROM inbox_marks")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, InboxMark { snoozed_until: r.get(1)?, ignored: r.get::<_, i64>(2)? != 0 }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Set entry `key`'s mark; a mark of nothing is removed.
    pub fn set_inbox_mark(&self, key: &str, mark: InboxMark) -> Result<()> {
        let conn = self.0.lock().unwrap();
        if mark == InboxMark::default() {
            conn.execute("DELETE FROM inbox_marks WHERE key = ?1", params![key])?;
        } else {
            conn.execute(
                "INSERT INTO inbox_marks (key, snoozed_until, ignored) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET snoozed_until = ?2, ignored = ?3",
                params![key, mark.snoozed_until, mark.ignored as i64],
            )?;
        }
        Ok(())
    }

    /// Forget the marks of entries that are gone (answered elsewhere, a run that moved on): only
    /// `keep` are still in the inbox.
    pub fn keep_inbox_marks(&self, keep: &std::collections::HashSet<String>) -> Result<()> {
        let gone: Vec<String> = self.inbox_marks()?.into_keys().filter(|k| !keep.contains(k)).collect();
        let conn = self.0.lock().unwrap();
        for k in gone {
            conn.execute("DELETE FROM inbox_marks WHERE key = ?1", params![k])?;
        }
        Ok(())
    }
}
