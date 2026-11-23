//! A workspace's team memory (`memories`): notes its runs' agents keep for the runs after them,
//! each with where it came from. The rules are the orchestrator's (`orchestrator/memory.rs`).
// @project-doc docs/domains/20261123-team_memory.md#memory_model

use super::Db;
use crate::status::RunStatus;
use crate::wire::wire_enum;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

wire_enum! {
    /// What a note is: a fact about the project, a decision and its reason, a trap someone fell
    /// into, or how to do something.
    #[derive(Default)]
    pub enum MemoryKind {
        #[default]
        Fact = "fact",
        Decision = "decision",
        Pitfall = "pitfall",
        HowTo = "howto",
    }
}

/// One note, as stored and as the app and the agents read it. `run` and `who` say who wrote it
/// last: a member of that run (`who` is its member id, `author` its name), or the person (`who`
/// is `user`, no run). `run_goal` and `run_status` are read from the run beside it: a note from a
/// cancelled run is not handed to later runs, and the next consolidation removes it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub workspace: String,
    pub kind: MemoryKind,
    pub title: String,
    pub hook: String,
    pub body: String,
    pub run: Option<String>,
    pub who: String,
    pub author: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub run_goal: Option<String>,
    pub run_status: Option<String>,
}

impl Memory {
    /// Whether later runs get it: not when the run that wrote it was cancelled.
    pub fn in_use(&self) -> bool {
        self.run_status.as_deref().and_then(RunStatus::parse) != Some(RunStatus::Cancelled)
    }
}

/// What a write sets; who wrote it and when come with it.
pub struct MemoryWrite<'a> {
    pub kind: MemoryKind,
    pub title: &'a str,
    pub hook: &'a str,
    pub body: &'a str,
    pub run: Option<&'a str>,
    pub who: &'a str,
    pub author: &'a str,
}

const SELECT: &str = "SELECT m.id, m.workspace, m.kind, m.title, m.hook, m.body, m.run_id, m.who, m.author,
        m.created_at, m.updated_at, r.goal, r.status
   FROM memories m LEFT JOIN runs r ON r.id = m.run_id";

fn memory_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        workspace: r.get(1)?,
        kind: MemoryKind::parse(&r.get::<_, String>(2)?).unwrap_or_default(),
        title: r.get(3)?,
        hook: r.get(4)?,
        body: r.get(5)?,
        run: r.get(6)?,
        who: r.get(7)?,
        author: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        run_goal: r.get(11)?,
        run_status: r.get(12)?,
    })
}

impl Db {
    /// A workspace's notes, the most recently changed first.
    pub fn memories(&self, workspace: &str) -> Result<Vec<Memory>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(&format!("{SELECT} WHERE m.workspace = ?1 ORDER BY m.updated_at DESC, m.id"))?;
        let rows = stmt.query_map(params![workspace], memory_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn memory(&self, id: &str) -> Result<Option<Memory>> {
        let conn = self.0.lock().unwrap();
        Ok(conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), params![id], memory_row).optional()?)
    }

    pub fn insert_memory(&self, id: &str, workspace: &str, w: &MemoryWrite<'_>, now: i64) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO memories (id, workspace, kind, title, hook, body, run_id, who, author, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![id, workspace, w.kind.as_str(), w.title, w.hook, w.body, w.run, w.who, w.author, now],
        )?;
        Ok(())
    }

    /// Rewrite a note; whoever changed it last is who it is from now.
    pub fn update_memory(&self, id: &str, w: &MemoryWrite<'_>, now: i64) -> Result<()> {
        self.0.lock().unwrap().execute(
            "UPDATE memories SET kind = ?2, title = ?3, hook = ?4, body = ?5, run_id = ?6, who = ?7, author = ?8, updated_at = ?9
             WHERE id = ?1",
            params![id, w.kind.as_str(), w.title, w.hook, w.body, w.run, w.who, w.author, now],
        )?;
        Ok(())
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.0.lock().unwrap().execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// A deleted workspace takes its notes with it.
    pub fn delete_workspace_memories(&self, workspace: &str) -> Result<()> {
        self.0.lock().unwrap().execute("DELETE FROM memories WHERE workspace = ?1", params![workspace])?;
        Ok(())
    }
}
