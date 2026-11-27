//! The Markdown files of a run, for the 文档 menu: what the lead delivered, the artifacts Janus
//! saved, and files the agents wrote or mentioned (absolute paths, or paths relative to the
//! run's directory). The fold only collects candidates as events go by; the view lists those
//! that exist. These are also the only files of the workspace a client may read through the run
//! (`GET /api/runs/{id}/doc`).
// @project-doc docs/architecture/20261031-frontend.md#markdown_viewer

use super::text;
use crate::db::Event;
use crate::events::EventKind;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

crate::wire::wire_enum! {
    /// Where a document comes from, strongest first.
    #[derive(PartialOrd, Ord)]
    pub enum Source {
        /// A delivery handed it over, or the round changed it.
        Deliverable = "deliverable",
        /// An artifact Janus saved for a task.
        Artifact = "artifact",
        /// An agent wrote or mentioned it.
        Mentioned = "mentioned",
    }
}

/// A candidate as the events name it: its strongest source and its latest mention.
#[derive(Clone, Copy, Debug)]
struct Found {
    source: Source,
    seq: i64,
}

/// The candidates collected so far, by the path as written.
#[derive(Clone, Debug, Default)]
pub(super) struct DocsFold {
    found: BTreeMap<String, Found>,
}

impl DocsFold {
    fn add(&mut self, path: &str, source: Source, seq: i64) {
        if !is_markdown(path) {
            return;
        }
        let f = self.found.entry(path.to_string()).or_insert(Found { source, seq });
        f.source = f.source.min(source);
        f.seq = f.seq.max(seq);
    }

    pub(super) fn apply(&mut self, e: &Event, d: &Value) {
        match e.kind() {
            EventKind::RunDelivered => {
                for a in super::list(&d["deliverables"]) {
                    self.add(&text(&a["path"]), Source::Deliverable, e.seq);
                }
                // The files the round changed are what it delivers too, Git or not: without Git
                // they are the only record of what the team wrote (a report in the work folder).
                for p in super::texts(&d["changed"]) {
                    self.add(&p, Source::Deliverable, e.seq);
                }
            }
            EventKind::ArtifactCreated => {
                self.add(&text(&d["path"]), Source::Artifact, e.seq);
                return;
            }
            // Janus's own prompts quote instructions, not files the team produced; status lines
            // repeat tool calls.
            EventKind::SessionPrompt | EventKind::AgentStatus => return,
            _ => {}
        }
        let mut all = vec![];
        strings(d, &mut all);
        all.push(e.text.as_str());
        for s in all {
            for p in paths_in(s) {
                self.add(&p, Source::Mentioned, e.seq);
            }
        }
    }

    /// The documents that exist now, deliverables first, then artifacts, then mentions; newest
    /// first in each. Relative paths are read from `cwd`; `moved` maps a worktree that is gone (a
    /// run accepted before a restart) to the repository its result was brought back to, so files
    /// in it are found where they are now (FL-19).
    pub(super) fn existing(&self, cwd: &str, moved: Option<&Moved>, exists: impl Fn(&str) -> bool) -> Vec<Doc> {
        let mut docs: BTreeMap<String, Found> = BTreeMap::new();
        for (raw, f) in &self.found {
            let p = normalize(raw, cwd);
            let p = moved.map_or(p.clone(), |m| m.apply(&p));
            let d = docs.entry(p).or_insert(*f);
            d.source = d.source.min(f.source);
            d.seq = d.seq.max(f.seq);
        }
        let mut out: Vec<Doc> =
            docs.into_iter().filter(|(p, _)| exists(p)).map(|(path, f)| Doc::new(path, f.source, f.seq, cwd)).collect();
        out.sort_by(|a, b| (a.source, -a.seq, &a.path).cmp(&(b.source, -b.seq, &b.path)));
        out
    }
}

/// A document of the run, and how the menu names it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Doc {
    pub path: String,
    pub source: Source,
    /// The latest event that delivered, saved or mentioned it.
    pub seq: i64,
    /// Inside the run's directory, the path from there; elsewhere, the path with `~`.
    pub label: String,
    /// Its file name, and the folder it is in (with `~`).
    pub name: String,
    pub dir: String,
}

impl Doc {
    fn new(path: String, source: Source, seq: i64, cwd: &str) -> Doc {
        let dir_of = |p: &str| Path::new(p).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let root = if cwd.ends_with('/') { cwd.to_string() } else { format!("{cwd}/") };
        let label = match path.strip_prefix(&root) {
            Some(rel) if !cwd.is_empty() => rel.to_string(),
            _ => tilde(&path),
        };
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = tilde(&dir_of(&path));
        Doc { path, source, seq, label, name, dir }
    }
}

/// A home-relative path for display: `/Users/me/x` reads `~/x`.
pub fn tilde(p: &str) -> String {
    for root in ["/Users/", "/home/"] {
        if let Some(rest) = p.strip_prefix(root)
            && let Some((user, tail)) = rest.split_once('/')
            && !user.is_empty()
        {
            return format!("~/{tail}");
        }
    }
    p.to_string()
}

/// Paths under `from` (a run's worktree) read under `to` (its repository) instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub from: String,
    pub to: String,
}

impl Moved {
    fn apply(&self, path: &str) -> String {
        match path.strip_prefix(&self.from) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", self.to),
            _ => path.to_string(),
        }
    }

    /// Where a run's worktree went when it is gone: `cwd` is in `work` (the run's worktree, or a
    /// folder of it), `repo` is where accepting brought the result. None while it is there.
    pub fn of(cwd: &str, repo: &str, work: &Path, exists: impl Fn(&str) -> bool) -> Option<Moved> {
        let from = work.to_string_lossy().into_owned();
        let inside = cwd == from || cwd.strip_prefix(&from).is_some_and(|r| r.starts_with('/'));
        (!repo.is_empty() && !cwd.is_empty() && inside && !exists(cwd)).then(|| Moved { from, to: repo.to_string() })
    }
}

pub fn is_markdown(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// `~/x` from the home directory, a relative path from `cwd`, with `.` and `..` worked out.
pub fn normalize(path: &str, cwd: &str) -> String {
    let mut p = PathBuf::from(path);
    if let Some(rest) = path.strip_prefix("~/") {
        p = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest);
    } else if !path.starts_with('/') && !cwd.is_empty() {
        p = Path::new(cwd).join(path);
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Every string in a JSON value.
fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// A character that ends an absolute path written in prose.
fn stops(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{}|,;，。；：、（）「」！？*".contains(c)
}

/// A character before which no path starts: it would be the middle of a word or a path.
fn joins(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_.-/~".contains(c)
}

/// A character a name in a path is made of.
fn name_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || "_.-".contains(c)
}

/// Whether the text after a match continues a word, so the match is not a whole name.
fn continues(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
}

/// `.md` or `.markdown` ends `s[..end]`.
fn markdown_end(s: &[char], end: usize) -> bool {
    let ends = |suffix: &str| {
        let n = suffix.chars().count();
        end >= n && s[end - n..end].iter().copied().eq(suffix.chars())
    };
    ends(".md") || ends(".markdown")
}

/// Absolute paths (`/…` or `~…`) ending in a Markdown name, the shortest from each start.
fn absolute(s: &[char]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = 0;
    'start: while i < s.len() {
        if (s[i] == '/' || s[i] == '~') && (i == 0 || !joins(s[i - 1])) {
            let mut end = i + 1;
            loop {
                if markdown_end(s, end) && end >= i + 4 && !continues(s.get(end)) {
                    out.push((i, end));
                    i = end;
                    continue 'start;
                }
                if end >= s.len() || stops(s[end]) {
                    break;
                }
                end += 1;
            }
        }
        i += 1;
    }
    out
}

/// Relative paths (names joined by `/`) ending in a Markdown name, the longest from each start.
fn relative(s: &[char]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = 0;
    while i < s.len() {
        if !name_char(s[i]) || (i > 0 && joins(s[i - 1])) {
            i += 1;
            continue;
        }
        // The names from here: a `/` only between two of them.
        let mut end = i;
        while end < s.len() && (name_char(s[end]) || (s[end] == '/' && s.get(end + 1).is_some_and(|c| name_char(*c)))) {
            end += 1;
        }
        let found = (i + 1..=end).rev().find(|&e| {
            let n = if markdown_end(s, e) { if s[e - 1] == 'n' { 9 } else { 3 } } else { return false };
            // The name before the suffix is not empty and does not end in `/`.
            e > i + n && s[e - n - 1] != '/' && !continues(s.get(e))
        });
        match found {
            Some(e) => {
                out.push((i, e));
                i = e;
            }
            None => i += 1,
        }
    }
    out
}

/// Markdown paths written in a piece of text. A relative name glued to Chinese text
/// ("生成README.md") also yields the name without the Chinese prefix, and "a.md/b.md" yields each
/// name; the existence check later keeps the real one.
pub fn paths_in(text: &str) -> Vec<String> {
    if !text.contains(".md") && !text.contains(".markdown") {
        return vec![];
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = vec![];
    let mut rest = text.to_string();
    for (a, b) in absolute(&chars) {
        let m: String = chars[a..b].iter().collect();
        rest = rest.replace(&m, " ");
        out.push(m);
    }
    let rest: Vec<char> = rest.chars().collect();
    for (a, b) in relative(&rest) {
        let m: String = rest[a..b].iter().collect();
        out.push(m.clone());
        // The Latin end of a name glued to Chinese text: 「生成README.md」 → `README.md`.
        let latin = |c: char| c.is_ascii_alphanumeric() || "_.-/".contains(c);
        let tail = m.char_indices().rev().take_while(|(_, c)| latin(*c)).last().map(|(k, _)| k);
        if let Some(k) = tail {
            let t = m[k..].trim_start_matches('/');
            let start = m.len() - t.len();
            if start != 0 && !t.is_empty() {
                out.push(t.to_string());
            }
        }
        let parts: Vec<&str> = m.split('/').filter(|p| is_markdown(p)).collect();
        if parts.len() > 1 {
            out.extend(parts.into_iter().map(str::to_string));
        }
    }
    out
}
