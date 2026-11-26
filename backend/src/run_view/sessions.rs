//! Each conversation of a run: which events it shows, whether its session is open, whose turn it
//! is, and what its composer offers. A task's conversation is its attempts; an agent's is
//! everything it did in the run; a chat's, a Loop's or a consolidation's one session is a
//! conversation that any message reopens.
// @project-doc docs/architecture/20261031-frontend.md#session_view

use super::{RunState, SessionTurn, text};
use crate::db::Event;
use crate::events::EventKind;
use crate::status::RunStatus;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The event kinds a conversation shows.
const SHOWN: &[EventKind] = &[
    EventKind::AgentThought,
    EventKind::AgentPlan,
    EventKind::SessionControls,
    EventKind::SessionControl,
    EventKind::SessionCommands,
    EventKind::SessionUsage,
    EventKind::AttemptStarted,
    EventKind::SessionPrompt,
    EventKind::ContextHandoff,
    EventKind::BoardPosted,
    EventKind::TaskConflict,
    EventKind::AgentMessage,
    EventKind::UserMessage,
    EventKind::MessageRouted,
    EventKind::MessageDelivered,
    EventKind::ToolCall,
    EventKind::ToolStarted,
    EventKind::ToolCompleted,
    EventKind::QuestionAsked,
    EventKind::QuestionAnswered,
    EventKind::ApprovalRequested,
    EventKind::ApprovalGranted,
    EventKind::ApprovalDenied,
    EventKind::InputRequested,
    EventKind::InputAnswered,
    EventKind::ControlTaken,
    EventKind::ControlReleased,
    EventKind::SessionInterrupted,
    EventKind::ResultSubmitted,
    EventKind::AttemptFailed,
    EventKind::LeadEdited,
    EventKind::LeadRouted,
];

/// Janus's own events that belong in the lead's conversation: the run it works on and a Loop's
/// wakes.
const LEAD_MARKS: &[EventKind] =
    &[EventKind::RunCreated, EventKind::LoopWoke, EventKind::LoopSlept, EventKind::LoopFailed];

/// The key of a task's conversation, `task:<id>`, and of an agent's, `agent:<id>`: task ids and
/// agent ids are separate names (a task may be called `lead`).
pub fn task_key(task: &str) -> String {
    format!("task:{task}")
}

pub fn agent_key(agent: &str) -> String {
    format!("agent:{agent}")
}

/// What the fold keeps for the conversations while the events go by.
#[derive(Clone, Debug, Default)]
pub(super) struct SessionFold {
    /// Each conversation's events, by key.
    events: BTreeMap<String, Vec<i64>>,
    /// Each agent's turn over its own events (the same rule as a task's session).
    turns: BTreeMap<String, SessionTurn>,
    /// The prompts that carried the person's own message (`followup`): a chat shows the message.
    followups: Vec<i64>,
}

impl SessionFold {
    pub(super) fn apply(&mut self, e: &Event, d: &Value) {
        let kind = e.kind();
        if kind == EventKind::SessionPrompt && d["channel"] == "followup" {
            self.followups.push(e.seq);
        }
        let shown = SHOWN.contains(&kind) || (kind == EventKind::AgentStatus && d["turn"] == "idle");
        if shown {
            if let Some(task) = &e.task {
                self.events.entry(task_key(task)).or_default().push(e.seq);
            }
            // A conflict is recorded on the task whose edit found it; the other task shows it too.
            let other = text(&d["other"]);
            if kind == EventKind::TaskConflict && !other.is_empty() && e.task.as_deref() != Some(&other) {
                self.events.entry(task_key(&other)).or_default().push(e.seq);
            }
        }
        let mut agents: Vec<&str> = vec![];
        if shown {
            agents.extend([e.actor.as_str(), e.to.as_deref().unwrap_or("")]);
        }
        if LEAD_MARKS.contains(&kind) {
            agents.push("lead");
        }
        agents.retain(|a| !a.is_empty() && *a != "user" && *a != "janus");
        agents.dedup();
        for a in agents {
            self.events.entry(agent_key(a)).or_default().push(e.seq);
            let turn = self.turns.entry(a.to_string()).or_insert(SessionTurn::OPENED);
            *turn = turn.after(e, d);
        }
    }
}

crate::wire::wire_enum! {
    /// What a conversation's composer offers.
    pub enum Composer {
        /// A past position: nothing can be sent.
        Replay = "replay",
        /// The person holds the open session (`SessionView::idle` when it waits for them).
        Human = "human",
        /// The open session runs on its own; a message cuts in.
        Managed = "managed",
        /// The run stopped part-way; sending resumes it.
        Interrupted = "interrupted",
        /// A conversation: a plain message (reopening the session first when it is gone).
        Converse = "converse",
        /// No open session; a message waits for the agent's next one.
        Leave = "leave",
        Ended = "ended",
    }
}

/// One conversation of the run.
#[derive(Clone, Debug, Serialize)]
pub struct SessionView {
    /// The events it shows, by seq.
    pub events: Vec<i64>,
    /// Its session is open now.
    pub open: bool,
    /// The person holds it (took over, or interrupted it).
    pub human: bool,
    /// Its turn has ended and it waits for a message.
    pub idle: bool,
    /// A conversation: the lead's after its goal is done, or a chat's.
    pub converse: bool,
    /// A chat's, a Loop's or a consolidation's one session: sending reopens it whenever it is gone.
    pub chat: bool,
    pub composer: Composer,
}

/// What each session's facts come to, before the composer is chosen.
struct Facts {
    open: bool,
    human: bool,
    idle: bool,
    resume: bool,
    leave: bool,
    converse: bool,
    chat: bool,
}

impl Facts {
    fn composer(&self, live: bool, replaying: bool) -> Composer {
        if replaying {
            Composer::Replay
        } else if self.open && live && self.converse && self.idle && !self.human {
            // A conversation's open session that waits for the person takes a plain message.
            Composer::Converse
        } else if self.open && live {
            if self.human { Composer::Human } else { Composer::Managed }
        } else if self.resume {
            Composer::Interrupted
        } else if self.converse {
            Composer::Converse
        } else if self.leave && live {
            Composer::Leave
        } else {
            Composer::Ended
        }
    }
}

/// Every conversation of the run: each task's, each agent's (the lead's is the run's one session
/// in a chat, a Loop or a consolidation). `live`: the run is in the service's memory.
// @project-doc docs/architecture/20261031-frontend.md#session_view
pub(super) fn sessions(st: &RunState, live: bool, replaying: bool) -> BTreeMap<String, SessionView> {
    let status = st.run.status;
    let resume = matches!(status, Some(RunStatus::Interrupted | RunStatus::Failed));
    let working = matches!(status, Some(RunStatus::Planning | RunStatus::Running));
    let seqs = |key: &str| st.sessions.events.get(key).cloned().unwrap_or_default();
    let mut out = BTreeMap::new();
    let mut put = |key: String, events: Vec<i64>, f: Facts| {
        let composer = f.composer(live, replaying);
        out.insert(
            key,
            SessionView {
                events,
                open: f.open,
                human: f.human,
                idle: f.idle,
                converse: f.converse,
                chat: f.chat,
                composer,
            },
        );
    };
    for t in &st.tasks {
        let key = task_key(&t.id);
        let turn = t.session;
        let f = Facts {
            open: turn.is_some(),
            human: turn.is_some_and(|s| s.person),
            idle: turn.is_some_and(|s| s.idle),
            resume,
            leave: false,
            converse: false,
            chat: false,
        };
        put(key.clone(), seqs(&key), f);
    }
    let mut agents: Vec<&str> = st.agents.keys().map(String::as_str).collect();
    if !agents.contains(&"lead") {
        agents.push("lead");
    }
    let done = st.run.is_done();
    for a in agents {
        let key = agent_key(a);
        let turn = st.sessions.turns.get(a).copied().unwrap_or(SessionTurn::OPENED);
        let mut events = seqs(&key);
        let f = if a == "lead" && st.run.kind.single_session() {
            // What the person wrote is the prompt itself: it shows once, as their message.
            events.retain(|s| !st.sessions.followups.contains(s));
            Facts {
                open: live && working,
                // While its session is still starting a message waits for it, sent the plain way.
                idle: turn.idle || status == Some(RunStatus::Planning),
                // A chat has no takeover: stopping a turn leaves it waiting for the next message.
                human: false,
                resume: false,
                leave: false,
                converse: true,
                chat: true,
            }
        } else {
            Facts {
                open: st.tasks.iter().any(|t| t.agent == a && t.session.is_some()) || (a == "lead" && working),
                human: turn.person,
                idle: turn.idle,
                resume,
                leave: !(done || status == Some(RunStatus::Cancelled)),
                converse: a == "lead" && done,
                chat: false,
            }
        };
        put(key, events, f);
    }
    out
}
