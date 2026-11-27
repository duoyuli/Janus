//! What a run's sessions are writing and running right now, before it is on record, as each
//! conversation shows it. The run's event stream carries `live` frames beside its events; a frame
//! only previews, so it shows only while no newer record of the same output has gone out, and the
//! end of a turn drops whatever it previewed. Each stream works this out for what it sends, and
//! sends each conversation's whole preview when it changes, so a client only puts it in place.
// @project-doc docs/architecture/20261031-frontend.md#live_preview

use super::sessions::{agent_key, task_key};
use super::text;
use crate::db::Event;
use crate::events::EventKind;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// A running tool call as its newest frame describes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tool {
    pub title: String,
    pub tool: String,
    pub input: String,
    pub output: String,
    pub status: String,
}

/// One session's preview: its text and thought not yet recorded, its running calls by call id.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Preview {
    pub text: String,
    pub thought: String,
    pub tools: BTreeMap<String, Tool>,
    /// The run's newest event when the text was previewed.
    #[serde(skip)]
    seq: i64,
}

impl Preview {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.thought.is_empty() && self.tools.is_empty()
    }
}

/// A session: the agent speaking and the task it works on (none for the lead's own).
type Session = (String, Option<String>);

/// Every session's preview of one run, and what the records already cover.
#[derive(Default)]
pub struct Previews {
    sessions: BTreeMap<Session, Preview>,
    /// The newest recorded text or thought of each session, by seq.
    said: BTreeMap<Session, i64>,
    /// Tool calls already on record, by session and call id.
    done: HashSet<(Session, String)>,
}

impl Previews {
    /// A new event: the text, thought or call it records is no longer a preview. The
    /// conversations whose preview changed.
    pub fn recorded(&mut self, e: &Event) -> Vec<String> {
        let k: Session = (e.actor.clone(), e.task.clone());
        let d = e.data.as_ref().unwrap_or(&Value::Null);
        match e.kind() {
            EventKind::AgentMessage | EventKind::AgentThought => {
                self.said.insert(k.clone(), e.seq);
                match self.sessions.get_mut(&k) {
                    Some(s) if e.seq > s.seq && !(s.text.is_empty() && s.thought.is_empty()) => {
                        s.text.clear();
                        s.thought.clear();
                        self.touched(&k)
                    }
                    _ => vec![],
                }
            }
            EventKind::ToolCall => {
                let id = text(&d["callId"]);
                if id.is_empty() {
                    return vec![];
                }
                self.done.insert((k.clone(), id.clone()));
                let removed = self.sessions.get_mut(&k).is_some_and(|s| s.tools.remove(&id).is_some());
                if removed { self.touched(&k) } else { vec![] }
            }
            _ => vec![],
        }
    }

    /// A frame from the sessions (`orchestrator::live`). The conversations whose preview changed.
    pub fn apply(&mut self, f: &Value) -> Vec<String> {
        let k: Session = (text(&f["actor"]), f["task"].as_str().map(str::to_owned));
        let seq = f["seq"].as_i64().unwrap_or(0);
        match f["kind"].as_str().unwrap_or("") {
            "say" => {
                // Made before the newest record of this session: that record already holds it.
                if self.said.get(&k).is_some_and(|&s| seq < s) {
                    return vec![];
                }
                let s = self.sessions.entry(k.clone()).or_default();
                s.text = text(&f["text"]);
                s.thought = text(&f["thought"]);
                s.seq = seq;
            }
            "tool" => {
                let id = text(&f["callId"]);
                if id.is_empty() || self.done.contains(&(k.clone(), id.clone())) {
                    return vec![];
                }
                let tool = Tool {
                    title: text(&f["title"]),
                    tool: text(&f["tool"]),
                    input: text(&f["input"]),
                    output: text(&f["output"]),
                    status: text(&f["status"]),
                };
                self.sessions.entry(k.clone()).or_default().tools.insert(id, tool);
            }
            "end" => {
                if self.sessions.remove(&k).is_none() {
                    return vec![];
                }
            }
            _ => return vec![],
        }
        self.touched(&k)
    }

    /// The conversations session `k` shows in: its task's, and its agent's.
    fn touched(&self, k: &Session) -> Vec<String> {
        let mut keys = vec![agent_key(&k.0)];
        if let Some(t) = &k.1 {
            keys.insert(0, task_key(t));
        }
        keys
    }

    /// The preview a conversation shows: a task's session, or one agent's (its own outside any
    /// task first). None when there is nothing to preview.
    pub fn of(&self, key: &str) -> Option<&Preview> {
        let found = if let Some(task) = key.strip_prefix("task:") {
            self.sessions.iter().find(|(k, _)| k.1.as_deref() == Some(task)).map(|(_, s)| s)
        } else if let Some(agent) = key.strip_prefix("agent:") {
            self.sessions
                .get(&(agent.to_string(), None))
                .or_else(|| self.sessions.iter().find(|(k, _)| k.0 == agent).map(|(_, s)| s))
        } else {
            None
        };
        found.filter(|s| !s.is_empty())
    }

    /// The `live` frame a stream sends for conversations `keys`: each one's whole preview, or
    /// null for one with nothing to preview any more.
    pub fn frame(&self, keys: &[String]) -> Option<Value> {
        if keys.is_empty() {
            return None;
        }
        let unique: BTreeSet<&String> = keys.iter().collect();
        let previews: serde_json::Map<String, Value> =
            unique.into_iter().map(|k| (k.clone(), self.of(k).map_or(Value::Null, |p| json!(p)))).collect();
        Some(json!({ "previews": previews }))
    }
}
