//! The agent's own records of its sessions, in its configuration directory inside Janus home
//! (`CLAUDE_CONFIG_DIR` or `CODEX_HOME`, `Layout::agent_config`; never the person's own `~/.claude`
//! or `~/.codex`): which files belong to a session, and the tokens they say it used so far.
//!
//! - Claude Code: `projects/<project>/<session>.jsonl` and its subagents'
//!   `projects/<project>/<session>/subagents/*.jsonl`. Each `assistant` line carries
//!   `message.usage` and `message.model`; one response is written on several lines, so a response
//!   (`message.id` with `requestId`, or `requestRef` in later versions) counts once, its last line.
//! - Codex: `sessions/<yyyy>/<mm>/<dd>/rollout-<time>-<session>.jsonl` (the ACP session id is the
//!   Codex thread id). `token_count` events carry the thread's running total; its `input_tokens`
//!   include the cache reads and writes, which are taken out as the adapter does. The model is the
//!   latest `turn_context`'s.
//!
//! Files are only read, from where the last read stopped; a line still being written waits for the
//! next read. The readings are cumulative per session and model, recorded as `session.usage` with
//! `source: "records"`; the usage ledger turns them into increments.
// @project-doc docs/interfaces/20261031-events.md#usage_records

use crate::agents::Runtime;
use crate::usage::Tokens;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Where Claude Code keeps its sessions' transcripts, and their environments.
const CLAUDE_PROJECTS: &str = "projects";
const CLAUDE_SESSION_ENV: &str = "session-env";
/// Where Codex keeps its threads' rollouts.
const CODEX_SESSIONS: &str = "sessions";

/// How often a running turn's records are read.
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(20);

/// Remove what the agent kept of `sessions` in its configuration directory `config`: Claude
/// Code's `projects/<project>/<session>.jsonl` (with `<session>/` beside it) and
/// `session-env/<session>/`, Codex's `sessions/<date>/rollout-<time>-<session>.jsonl`.
pub fn forget(config: &Path, sessions: &HashSet<String>) {
    fn walk(dir: &Path, sessions: &HashSet<String>, depth: u8) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if sessions.iter().any(|s| of_session(&name, s)) {
                crate::layout::remove(&e.path());
            } else if depth > 0 && e.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&e.path(), sessions, depth - 1);
            }
        }
    }
    if sessions.is_empty() {
        return;
    }
    for top in [CLAUDE_PROJECTS, CLAUDE_SESSION_ENV, CODEX_SESSIONS] {
        walk(&config.join(top), sessions, 4);
    }
}

/// Whether a file or directory name in the agent's configuration directory is session `sid`'s:
/// `<sid>`, `<sid>.jsonl`, or Codex's `rollout-<time>-<sid>.jsonl`.
fn of_session(name: &str, sid: &str) -> bool {
    let file = name.strip_suffix(".jsonl").unwrap_or(name);
    !sid.is_empty() && (file == sid || file.strip_suffix(sid).is_some_and(|rest| rest.ends_with('-')))
}

/// Counts a record line gives: every part reported, so adding them never leaves one unreported.
fn counts(input: i64, output: i64, cache_read: i64, cache_write: i64, thought: i64, total: i64) -> Tokens {
    Tokens {
        input: Some(input),
        output: Some(output),
        cached_read: Some(cache_read),
        cached_write: Some(cache_write),
        thought: Some(thought),
        total: Some(total),
    }
}

/// One file read so far: up to `offset`, the end of its last whole line.
struct Tail {
    path: PathBuf,
    offset: u64,
}

impl Tail {
    /// The whole lines appended since the last read; `None` when the file shrank (it was rewritten).
    fn lines(&mut self) -> Option<Vec<String>> {
        let Ok(mut file) = std::fs::File::open(&self.path) else { return Some(vec![]) };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            return None;
        }
        let mut buf = Vec::new();
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() || file.read_to_end(&mut buf).is_err()
        {
            return Some(vec![]);
        }
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else { return Some(vec![]) };
        self.offset += end as u64 + 1;
        Some(String::from_utf8_lossy(&buf[..end]).lines().map(str::to_owned).collect())
    }
}

/// What the lines of one runtime's records add up to.
enum Totals {
    /// Each response's last usage, by `(message id, request id)`, with its model.
    Claude(HashMap<(String, String), (String, Tokens)>),
    /// The thread's running total as last read, the model in use, and what each model used.
    Codex { last: Option<Value>, model: String, by_model: BTreeMap<String, Tokens> },
}

impl Totals {
    fn runtime(&self) -> Runtime {
        match self {
            Totals::Claude(_) => Runtime::Claude,
            Totals::Codex { .. } => Runtime::Codex,
        }
    }
}

/// A reader of one session's records, kept between its turns so each read takes only what is new.
pub struct Reader {
    config: PathBuf,
    sid: String,
    files: Vec<Tail>,
    totals: Totals,
    /// Lines without a response id, numbered so each counts once.
    anonymous: usize,
    /// The rows of the last reading given out.
    shown: Option<Vec<Value>>,
}

impl Reader {
    /// A reader of session `sid` in the agent configuration directory `config`; `None` when `sid`
    /// is not a plain file name.
    pub fn new(runtime: Runtime, config: PathBuf, sid: &str) -> Option<Reader> {
        if sid.is_empty() || Path::new(sid).file_name().and_then(|n| n.to_str()) != Some(sid) {
            return None;
        }
        let totals = match runtime {
            Runtime::Claude => Totals::Claude(HashMap::new()),
            Runtime::Codex => Totals::Codex { last: None, model: String::new(), by_model: BTreeMap::new() },
        };
        Some(Reader { config, sid: sid.into(), files: vec![], totals, anonymous: 0, shown: None })
    }

    /// The session read.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// The session's tokens by model so far, as `session.usage` data: always once records were
    /// found when the turn `ended`, else only when they changed since the last reading given out.
    pub fn reading(&mut self, ended: bool) -> Option<Value> {
        self.discover();
        let mut rewritten = false;
        let mut lines = vec![];
        for t in &mut self.files {
            match t.lines() {
                Some(mut new) => lines.append(&mut new),
                None => rewritten = true,
            }
        }
        if rewritten {
            // A file shrank: read everything again from the start.
            let runtime = self.totals.runtime();
            *self = Reader { shown: self.shown.take(), ..Reader::new(runtime, self.config.clone(), &self.sid)? };
            return self.reading(ended);
        }
        for line in &lines {
            self.take(line);
        }
        if self.files.is_empty() {
            return None;
        }
        let rows = self.rows();
        if !ended && self.shown.as_ref() == Some(&rows) {
            return None;
        }
        self.shown = Some(rows.clone());
        Some(json!({
            "source": crate::usage::RECORDS_SOURCE,
            "session": self.sid,
            "turnEnded": ended,
            "models": rows,
        }))
    }

    /// Find the session's files not yet followed.
    fn discover(&mut self) {
        let follow = |files: &mut Vec<Tail>, path: PathBuf| {
            if !files.iter().any(|t| t.path == path) {
                files.push(Tail { path, offset: 0 });
            }
        };
        match self.totals.runtime() {
            Runtime::Claude => {
                if self.files.is_empty() {
                    let name = format!("{}.jsonl", self.sid);
                    match find(&self.config.join(CLAUDE_PROJECTS), 2, &|n| n == name) {
                        Some(main) => follow(&mut self.files, main),
                        None => return,
                    }
                }
                let subagents = self.files[0].path.with_extension("").join("subagents");
                let mut found: Vec<PathBuf> = std::fs::read_dir(subagents)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                    .collect();
                found.sort();
                for p in found {
                    follow(&mut self.files, p);
                }
            }
            Runtime::Codex if self.files.is_empty() => {
                let sid = self.sid.clone();
                if let Some(p) =
                    find(&self.config.join(CODEX_SESSIONS), 4, &|n| n.starts_with("rollout-") && of_session(n, &sid))
                {
                    follow(&mut self.files, p);
                }
            }
            Runtime::Codex => {}
        }
    }

    /// Add one line of the records.
    fn take(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        match &mut self.totals {
            Totals::Claude(responses) => {
                let usage = &v["message"]["usage"];
                if v["type"] != "assistant" || !usage.is_object() {
                    return;
                }
                let n = |k: &str| usage[k].as_i64().unwrap_or(0);
                let (input, output, cache_read, cache_write) = (
                    n("input_tokens"),
                    n("output_tokens"),
                    n("cache_read_input_tokens"),
                    n("cache_creation_input_tokens"),
                );
                let thought = usage["output_tokens_details"]["thinking_tokens"].as_i64().unwrap_or(0);
                let counts =
                    counts(input, output, cache_read, cache_write, thought, input + output + cache_read + cache_write);
                let request = v["requestId"].as_str().or_else(|| v["requestRef"].as_str()).unwrap_or("");
                let key = match v["message"]["id"].as_str().filter(|id| !id.is_empty()) {
                    Some(id) => (id.to_owned(), request.to_owned()),
                    None => {
                        self.anonymous += 1;
                        (String::new(), self.anonymous.to_string())
                    }
                };
                let model = v["message"]["model"].as_str().unwrap_or("").to_owned();
                responses.insert(key, (model, counts));
            }
            Totals::Codex { last, model, by_model } => match (v["type"].as_str(), v["payload"]["type"].as_str()) {
                (Some("turn_context"), _) => {
                    if let Some(m) = v["payload"]["model"].as_str().filter(|m| !m.is_empty()) {
                        *model = m.to_owned();
                    }
                }
                (Some("event_msg"), Some("token_count")) => {
                    let info = &v["payload"]["info"];
                    let total = &info["total_token_usage"];
                    if !total.is_object() {
                        return;
                    }
                    let n = |u: &Value, k: &str| u[k].as_i64().unwrap_or(0);
                    // The increment over the previous total; when the total went back (a rewound
                    // thread), this request's own usage.
                    let delta = |k: &str| match last.as_ref() {
                        Some(prev) if n(total, "total_tokens") >= n(prev, "total_tokens") => {
                            (n(total, k) - n(prev, k)).max(0)
                        }
                        Some(_) => n(&info["last_token_usage"], k),
                        None => n(total, k),
                    };
                    let (input, cached, cache_write, output) = (
                        delta("input_tokens"),
                        delta("cached_input_tokens"),
                        delta("cache_write_input_tokens"),
                        delta("output_tokens"),
                    );
                    let added = counts(
                        (input - cached - cache_write).max(0),
                        output,
                        cached,
                        cache_write,
                        delta("reasoning_output_tokens"),
                        match delta("total_tokens") {
                            0 => input + output,
                            t => t,
                        },
                    );
                    *last = Some(total.clone());
                    if added != counts(0, 0, 0, 0, 0, 0) {
                        by_model.entry(model.clone()).or_default().add(&added);
                    }
                }
                _ => {}
            },
        }
    }

    /// The running totals by model, models without any tokens left out.
    fn rows(&self) -> Vec<Value> {
        let by_model: BTreeMap<String, Tokens> = match &self.totals {
            Totals::Claude(responses) => {
                let mut out: BTreeMap<String, Tokens> = BTreeMap::new();
                for (model, c) in responses.values() {
                    out.entry(model.clone()).or_default().add(c);
                }
                out
            }
            Totals::Codex { by_model, .. } => by_model.clone(),
        };
        by_model
            .iter()
            .filter(|(_, c)| c.total.is_some_and(|t| t > 0))
            .map(|(model, c)| {
                let mut row = c.write();
                row["model"] = json!(model);
                row
            })
            .collect()
    }
}

/// The first file under `dir`, at most `depth` levels down, whose name `matches`. Links are not
/// followed: only what the agent wrote there is read.
fn find(dir: &Path, depth: u8, matches: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    // Newest names last in Codex's dated folders: look there first.
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    if let Some(e) =
        entries.iter().find(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_str().is_some_and(matches))
    {
        return Some(e.path());
    }
    if depth <= 1 {
        return None;
    }
    entries.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).find_map(|e| find(&e.path(), depth - 1, matches))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("janus-records-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn claude_line(id: &str, request: &str, model: &str, input: i64, output: i64, read: i64, write: i64) -> String {
        json!({ "type": "assistant", "requestId": request, "message": { "id": id, "model": model, "usage": {
            "input_tokens": input, "output_tokens": output, "cache_read_input_tokens": read, "cache_creation_input_tokens": write
        } } })
        .to_string()
    }

    fn total(reading: &Value, model: &str) -> i64 {
        reading["models"].as_array().unwrap().iter().find(|r| r["model"] == model).unwrap()["totalTokens"]
            .as_i64()
            .unwrap()
    }

    #[test]
    fn names_of_a_session() {
        assert!(of_session("abc.jsonl", "abc"));
        assert!(of_session("abc", "abc"));
        assert!(of_session("rollout-2026-11-26T10-00-00-abc.jsonl", "abc"));
        assert!(!of_session("xabc.jsonl", "abc"));
        assert!(!of_session("abc.jsonl", ""));
    }

    #[test]
    fn claude_counts_each_response_once_its_last_line_and_reads_only_what_is_new() {
        let config = dir("claude");
        let project = config.join("projects/-tmp-w");
        std::fs::create_dir_all(&project).unwrap();
        let main = project.join("s1.jsonl");
        let mut r = Reader::new(Runtime::Claude, config.clone(), "s1").unwrap();
        assert_eq!(r.reading(false), None, "no records yet");
        assert_eq!(r.reading(true), None, "nor at the end of the turn");

        // The same response streamed on two lines, then another one half written.
        let a1 = claude_line("m1", "q1", "opus", 100, 10, 500, 50);
        let a2 = claude_line("m1", "q1", "opus", 100, 45, 500, 50);
        let b = claude_line("m2", "q2", "haiku", 80, 20, 0, 0);
        let (head, rest) = b.split_at(20);
        std::fs::write(&main, format!("{{\"type\":\"user\"}}\n{a1}\n{a2}\n{head}")).unwrap();
        let first = r.reading(false).unwrap();
        assert_eq!(first["source"], "records");
        assert_eq!(first["session"], "s1");
        assert_eq!(first["turnEnded"], false);
        assert_eq!(total(&first, "opus"), 695);
        assert_eq!(first["models"].as_array().unwrap().len(), 1, "the half line waits");
        assert_eq!(r.reading(false), None, "nothing new: nothing to record");

        // The rest of the line, and a subagent of the session.
        let mut f = std::fs::OpenOptions::new().append(true).open(&main).unwrap();
        std::io::Write::write_all(&mut f, format!("{rest}\n").as_bytes()).unwrap();
        let sub = project.join("s1/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("agent-1.jsonl"), format!("{}\n", claude_line("m3", "q3", "haiku", 5, 5, 0, 0)))
            .unwrap();
        let second = r.reading(true).unwrap();
        assert_eq!((total(&second, "opus"), total(&second, "haiku")), (695, 110));
        assert_eq!(second["turnEnded"], true);
        assert!(r.reading(true).is_some(), "a turn's end is always recorded");
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn claude_reads_again_from_the_start_when_a_file_shrinks() {
        let config = dir("claude-shrink");
        let project = config.join("projects/p");
        std::fs::create_dir_all(&project).unwrap();
        let main = project.join("s2.jsonl");
        std::fs::write(
            &main,
            format!("{}\n{}\n", claude_line("a", "", "m", 10, 0, 0, 0), claude_line("b", "", "m", 20, 0, 0, 0)),
        )
        .unwrap();
        let mut r = Reader::new(Runtime::Claude, config.clone(), "s2").unwrap();
        assert_eq!(total(&r.reading(false).unwrap(), "m"), 30);
        std::fs::write(&main, format!("{}\n", claude_line("c", "", "m", 7, 0, 0, 0))).unwrap();
        assert_eq!(total(&r.reading(false).unwrap(), "m"), 7);
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn codex_takes_the_cache_out_of_input_and_follows_the_model() {
        let config = dir("codex");
        let day = config.join("sessions/2026/11/26");
        std::fs::create_dir_all(&day).unwrap();
        let count = |input: i64, cached: i64, write: i64, output: i64, reasoning: i64| {
            json!({ "type": "event_msg", "payload": { "type": "token_count", "info": {
                "total_token_usage": { "input_tokens": input, "cached_input_tokens": cached, "cache_write_input_tokens": write,
                    "output_tokens": output, "reasoning_output_tokens": reasoning, "total_tokens": input + output },
                "last_token_usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 } } } })
            .to_string()
        };
        let lines = [
            json!({ "type": "session_meta", "payload": { "id": "t1" } }).to_string(),
            json!({ "type": "turn_context", "payload": { "model": "gpt-a" } }).to_string(),
            json!({ "type": "event_msg", "payload": { "type": "token_count", "info": null } }).to_string(),
            count(1000, 600, 100, 200, 50),
            // Codex repeats the total when a request starts: nothing new.
            count(1000, 600, 100, 200, 50),
            json!({ "type": "turn_context", "payload": { "model": "gpt-b" } }).to_string(),
            count(3000, 2000, 100, 500, 80),
        ];
        std::fs::write(day.join("rollout-2026-11-26T10-00-00-t1.jsonl"), lines.join("\n") + "\n").unwrap();
        std::fs::write(day.join("rollout-2026-11-26T09-00-00-other.jsonl"), count(9, 0, 0, 9, 0) + "\n").unwrap();
        let mut r = Reader::new(Runtime::Codex, config.clone(), "t1").unwrap();
        let reading = r.reading(true).unwrap();
        let rows = reading["models"].as_array().unwrap();
        let a = rows.iter().find(|r| r["model"] == "gpt-a").unwrap();
        assert_eq!(
            (
                a["inputTokens"].as_i64(),
                a["cachedReadTokens"].as_i64(),
                a["cachedWriteTokens"].as_i64(),
                a["outputTokens"].as_i64()
            ),
            (Some(300), Some(600), Some(100), Some(200))
        );
        assert_eq!((a["thoughtTokens"].as_i64(), a["totalTokens"].as_i64()), (Some(50), Some(1200)));
        let b = rows.iter().find(|r| r["model"] == "gpt-b").unwrap();
        assert_eq!(
            (b["inputTokens"].as_i64(), b["cachedReadTokens"].as_i64(), b["totalTokens"].as_i64()),
            (Some(600), Some(1400), Some(2300))
        );
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn a_session_id_must_be_a_plain_name() {
        assert!(Reader::new(Runtime::Claude, PathBuf::from("/tmp"), "../x").is_none());
        assert!(Reader::new(Runtime::Claude, PathBuf::from("/tmp"), "").is_none());
    }
}
