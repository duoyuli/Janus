//! Moving Janus home from one layout to the next. A move is a rename inside the home; paths the
//! database and the agents' own files recorded are rewritten from the old prefix to the new one,
//! so what a run showed and what a session remembers still open. Every step can run again: a
//! thing already moved is skipped, a path already rewritten no longer matches.
//!
//! Version 14 sorts the first layout (everything at the top of the home) into the categories of
//! `Layout`. The paths of the first layout are written out here as they were; the rest of Janus
//! only knows `Layout`.

// @project-doc docs/architecture/20261031-data_model.md#migrations
use super::Layout;
use crate::status::RunKind;
use anyhow::{Context, Result};
use rusqlite::{Transaction, params};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Before the database opens: from version 14 on it lives in `data/`. A database still at the
/// top of the home moves there with its WAL files, renamed, nothing rewritten. The main file goes
/// last, so a move cut short is finished the next time.
pub(crate) fn adopt_db(layout: &Layout) -> Result<()> {
    let old = layout.home().join("janus.db");
    let new = layout.db();
    if !old.exists() || new.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(new.parent().context("data")?)?;
    for suffix in ["-wal", "-shm", ""] {
        let from = layout.home().join(format!("janus.db{suffix}"));
        if from.exists() {
            std::fs::rename(&from, new.with_file_name(format!("janus.db{suffix}")))?;
        }
    }
    Ok(())
}

/// The database as the home has it now: still at the top before version 14 moved it. Only
/// `janus migrate --dry-run` asks, since it reads the database without opening the service.
pub(crate) fn current_db(layout: &Layout) -> PathBuf {
    let old = layout.home().join("janus.db");
    if layout.db().exists() || !old.exists() { layout.db() } else { old }
}

/// One change of layout: what moves where, what goes, and what the moved paths mean to others.
/// A later migration that moves files builds one of these and calls `apply`.
#[derive(Default)]
pub(crate) struct Relocation {
    moves: Vec<(PathBuf, PathBuf)>,
    drops: Vec<PathBuf>,
    /// Directories agents worked in: Claude Code names its projects after them.
    workdirs: Vec<(PathBuf, PathBuf)>,
    /// Worktrees that move, with their repository, for git to learn the new place.
    worktrees: Vec<(PathBuf, PathBuf)>,
    /// Old folders left empty by the moves, removed at the end.
    emptied: Vec<PathBuf>,
}

impl Relocation {
    pub(crate) fn mv(&mut self, from: PathBuf, to: PathBuf) {
        self.moves.push((from, to));
    }

    pub(crate) fn drop(&mut self, path: PathBuf) {
        self.drops.push(path);
    }

    /// A directory an agent works in moves; `repo` when it is a worktree of that repository.
    pub(crate) fn workdir(&mut self, from: PathBuf, to: PathBuf, repo: Option<PathBuf>) {
        if let Some(repo) = repo {
            self.worktrees.push((repo, to.clone()));
        }
        self.workdirs.push((from.clone(), to.clone()));
        self.mv(from, to);
    }

    pub(crate) fn emptied(&mut self, dir: PathBuf) {
        self.emptied.push(dir);
    }

    /// Move, drop, then rewrite: the database rows; the files processes wrote under each of
    /// `own_files` (agents' settings, transcripts, the tools they installed; servers' caches), text
    /// and symbolic links alike; and the Claude Code projects under each of `projects`. Returns how
    /// many paths moved and records changed.
    pub(crate) fn apply(
        &self,
        tx: &Transaction,
        home: &Path,
        own_files: &[PathBuf],
        projects: &[PathBuf],
    ) -> Result<usize> {
        let mut changed = 0;
        for (from, to) in &self.moves {
            changed += usize::from(rename(from, to)?);
        }
        for d in &self.drops {
            if d.is_dir() {
                std::fs::remove_dir_all(d).with_context(|| format!("删除 {} 失败", d.display()))?;
            } else if d.exists() {
                std::fs::remove_file(d).with_context(|| format!("删除 {} 失败", d.display()))?;
            }
        }
        for dir in &self.emptied {
            prune_empty(dir);
        }
        let map = prefixes(home, &self.moves);
        changed += rewrite_rows(tx, &map)?;
        for dir in projects {
            rename_projects(dir, &self.workdirs, home);
        }
        for dir in own_files {
            changed += rewrite_files(dir, &map)?;
        }
        for (repo, path) in &self.worktrees {
            if path.exists() {
                crate::worktrees::repair(repo, path);
            }
        }
        Ok(changed)
    }
}

/// Version 14: the first layout into categories (`data/`, `work/`, `cache/`, `tmp/`, `logs/`).
pub(crate) fn move_to_categories(tx: &Transaction, home: Option<&Path>) -> Result<usize> {
    // The dry run looks at a copy of the database only; there is nothing to move.
    let Some(home) = home else { return Ok(0) };
    let new = Layout::new(home);
    let old = |p: &str| home.join(p);
    let mut plan = Relocation::default();

    let runs: Vec<(String, String, String)> = tx
        .prepare("SELECT id, kind, config FROM runs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, kind, config) in &runs {
        let dirs = new.run(RunKind::parse(kind).unwrap_or_default(), id);
        let run = old("runs").join(id);
        plan.mv(old("artifacts").join(id), dirs.data.join("artifacts"));
        plan.mv(run.join("attachments"), dirs.attachments());
        for member in names(&run.join("members")) {
            plan.mv(run.join("members").join(&member).join("bundle"), dirs.bundle(&member));
        }
        plan.mv(old("logs").join(id), dirs.logs.clone());
        let config: Value = serde_json::from_str(config).unwrap_or_default();
        let repo = config["worktree"]["repo"].as_str().map(PathBuf::from);
        plan.workdir(old("chats").join(id), dirs.work.clone(), None);
        plan.workdir(old("worktrees").join(id), dirs.work.clone(), repo);
    }
    plan.mv(old("worktrees/deleted"), new.deleted_runs());

    for agent in names(&old("agents")) {
        let dir = old("agents").join(&agent);
        plan.mv(dir.join("home"), new.agent_config(&agent));
        plan.mv(dir.join("root"), new.agent_home(&agent));
        plan.mv(dir.join("secrets.json"), new.agent_secrets(&agent));
        for log in ["test.log", "login.log"] {
            plan.mv(dir.join(log), new.agent_logs(&agent).join(log));
        }
        plan.drop(dir.join("tmp"));
        plan.drop(dir.join("probe"));
    }

    plan.mv(old("library/store"), new.skills());
    for source in names(&old("library/sources")) {
        plan.mv(old("library/sources").join(&source), new.source(&source));
    }
    for item in names(&old("library/servers")) {
        let dir = old("library/servers").join(&item);
        plan.mv(dir.join("root"), new.server_home(&item));
        plan.drop(dir.join("tmp"));
        plan.drop(dir.join("warm"));
    }
    let items: Vec<(String, String)> = tx
        .prepare("SELECT id, data FROM configs WHERE kind = 'library_item'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (item, data) in &items {
        plan.mv(old("secrets/library").join(format!("{item}.json")), new.mcp_values(item));
        // Tool lists were `<id>-<revision>.json`; the id comes from the record, not the name.
        for file in names(&old("library/tools")) {
            if let Some(revision) = file.strip_prefix(&format!("{item}-")).and_then(|r| r.strip_suffix(".json")) {
                plan.mv(old("library/tools").join(&file), new.mcp_tools(item).join(format!("{revision}.json")));
            }
        }
        let name =
            serde_json::from_str::<Value>(data).unwrap_or_default()["name"].as_str().unwrap_or_default().to_string();
        plan.mv(old("logs/library").join(format!("mcp-{name}.log")), new.library_log(item));
    }
    // Tool lists and logs of items gone are not worth keeping. The new logs (`<id>.log`) share
    // the folder, so only the old names (`mcp-<name>.log`) go.
    plan.drop(old("library/tools"));
    for log in names(&old("logs/library")).into_iter().filter(|n| n.starts_with("mcp-")) {
        plan.drop(old("logs/library").join(log));
    }

    plan.mv(old("adapters"), new.adapters());
    plan.mv(old("prices/models.json"), new.prices());
    plan.drop(old("prices"));
    plan.mv(old("updates"), new.updates());
    plan.mv(old("permissions/review.json"), new.app_state().join("permissions.json"));
    // The app opens before the service and may have written its new record already; that one
    // stands, and the old folder goes either way.
    plan.drop(old("permissions"));

    for dir in ["runs", "artifacts", "chats", "worktrees", "agents", "library", "secrets", "permissions"] {
        plan.emptied(old(dir));
    }
    // The agents being moved and those already in `data/agents` (a move cut short finishes now).
    let mut agents = names(&old("agents"));
    agents.extend(names(&old("data/agents")));
    agents.sort();
    agents.dedup();
    let projects: Vec<PathBuf> = agents.iter().map(|a| new.agent_config(a).join("projects")).collect();
    plan.apply(tx, home, &[old("data/agents"), old("data/library/servers")], &projects)
}

/// The names in a directory; none when it is not there.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".DS_Store").collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Move `from` to `to` unless it is gone already or something is there. Returns whether it moved.
fn rename(from: &Path, to: &Path) -> Result<bool> {
    if !from.exists() || to.exists() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to).with_context(|| format!("把 {} 移到 {} 失败", from.display(), to.display()))?;
    Ok(true)
}

/// Remove `dir` and the directories in it that hold nothing (Finder's `.DS_Store` aside).
fn prune_empty(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            prune_empty(&e.path());
        }
    }
    let _ = std::fs::remove_file(dir.join(".DS_Store"));
    let _ = std::fs::remove_dir(dir);
}

/// Each moved path as text, old to new, in the form the home was recorded in and in its resolved
/// form (a worktree's path was recorded resolved); longest first, so a deeper path wins.
fn prefixes(home: &Path, moves: &[(PathBuf, PathBuf)]) -> Vec<(String, String)> {
    let resolved = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let mut map = vec![];
    for (from, to) in moves {
        for base in [home, resolved.as_path()] {
            let (Ok(f), Ok(t)) = (from.strip_prefix(home), to.strip_prefix(home)) else { continue };
            map.push((base.join(f).to_string_lossy().into_owned(), base.join(t).to_string_lossy().into_owned()));
        }
    }
    map.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    map.dedup();
    map
}

/// `text` with every old path replaced by its new one. An old path only counts as a whole name:
/// `…/logs/r1` is not a prefix of `…/logs/r10`.
pub(super) fn replace_paths(text: &str, map: &[(String, String)]) -> Option<String> {
    let mut out = text.to_string();
    let mut changed = false;
    for (from, to) in map {
        if !out.contains(from.as_str()) {
            continue;
        }
        let mut next = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.find(from.as_str()) {
            let after = &rest[i + from.len()..];
            let whole = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || "_-.".contains(c)));
            next.push_str(&rest[..i]);
            next.push_str(if whole { to } else { from });
            changed |= whole;
            rest = after;
        }
        next.push_str(rest);
        out = next;
    }
    changed.then_some(out)
}

/// The recorded paths: where runs and attempts worked, artifacts, run snapshots and events (the
/// one change made to events: where a file now is, not what happened).
fn rewrite_rows(tx: &Transaction, map: &[(String, String)]) -> Result<usize> {
    if map.is_empty() {
        return Ok(0);
    }
    let mut changed = 0;
    for (table, key, columns) in [
        ("runs", "rowid", &["cwd", "config"][..]),
        ("attempts", "rowid", &["workspace"][..]),
        ("artifacts", "rowid", &["path"][..]),
        ("events", "rowid", &["text", "data"][..]),
    ] {
        let select = format!("SELECT {key}, {} FROM {table}", columns.join(", "));
        let rows: Vec<(i64, Vec<Option<String>>)> = tx
            .prepare(&select)?
            .query_map([], |r| {
                Ok((r.get(0)?, (1..=columns.len()).map(|i| r.get(i)).collect::<rusqlite::Result<_>>()?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (rowid, values) in rows {
            for (column, value) in columns.iter().zip(values) {
                let Some(new) = value.as_deref().and_then(|v| replace_paths(v, map)) else { continue };
                tx.execute(&format!("UPDATE {table} SET {column} = ?1 WHERE {key} = ?2"), params![new, rowid])?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// Claude Code names a project after its working directory, every character other than a letter
/// or digit made `-`; a moved directory takes its sessions along under its new name.
fn rename_projects(projects: &Path, workdirs: &[(PathBuf, PathBuf)], home: &Path) {
    let key = |p: &Path| {
        p.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>()
    };
    let resolved = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    for name in names(projects) {
        for (from, to) in workdirs {
            let forms = [
                (from.clone(), to.clone()),
                (
                    resolved.join(from.strip_prefix(home).unwrap_or(from)),
                    resolved.join(to.strip_prefix(home).unwrap_or(to)),
                ),
            ];
            for (f, t) in forms {
                let (old, new) = (key(&f), key(&t));
                if let Some(rest) = name.strip_prefix(&old).filter(|r| r.is_empty() || r.starts_with('-')) {
                    let _ = rename(&projects.join(&name), &projects.join(format!("{new}{rest}")));
                }
            }
        }
    }
}

/// Larger files are not text a process keeps its paths in (toolchains, archives).
const MAX_REWRITE: u64 = 64 * 1024 * 1024;

/// Paths in files processes wrote for themselves: every text file under `dir` that names an old
/// path, and every symbolic link pointing at one. Other files are left as they are.
fn rewrite_files(dir: &Path, map: &[(String, String)]) -> Result<usize> {
    let mut changed = 0;
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_symlink() {
            let Ok(target) = std::fs::read_link(&path) else { continue };
            if let Some(new) = replace_paths(&target.to_string_lossy(), map) {
                std::fs::remove_file(&path)?;
                std::os::unix::fs::symlink(new, &path)?;
                changed += 1;
            }
        } else if kind.is_dir() {
            changed += rewrite_files(&path, map)?;
        } else if kind.is_file()
            && e.metadata().is_ok_and(|m| m.len() <= MAX_REWRITE)
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Some(new) = replace_paths(&text, map)
        {
            std::fs::write(&path, new)?;
            changed += 1;
        }
    }
    Ok(changed)
}
