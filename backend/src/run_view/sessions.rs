//! Each conversation of a run: which events it shows, whether its session is open, whose turn it
//! is, and what its composer offers. A task's conversation is its attempts; an agent's is
//! everything it did in the run; a chat's, a Loop's or a consolidation's one session is a
//! conversation that any message reopens.
// @project-doc docs/architecture/20261031-frontend.md#session_view

use super::aside::{self, Command, Control, Pending, SectionNote, Step, Usage};
use super::card::Tone;
use super::turns::{self, Row};
use super::{RunState, SessionTurn, text};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::{Lang, tr};
use crate::status::{RunKind, RunStatus};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

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

/// Whether some conversation shows `e`: the events a conversation's rows are made of.
pub(super) fn in_conversation(e: &Event, d: &Value) -> bool {
    let kind = e.kind();
    SHOWN.contains(&kind) || (kind == EventKind::AgentStatus && d["turn"] == "idle") || LEAD_MARKS.contains(&kind)
}

impl SessionFold {
    /// The events conversation `key` shows, by seq.
    pub(super) fn events_of(&self, key: &str) -> Vec<i64> {
        self.events.get(key).cloned().unwrap_or_default()
    }

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
#[serde(rename_all = "camelCase")]
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
    /// Its newest turn runs: the session works on its own or on the person's message.
    pub busy: bool,
    /// The agent's latest plan, the controls it offers, its slash commands and this session's
    /// usage (`aside`).
    pub plan: Vec<Step>,
    pub controls: Vec<Control>,
    pub commands: Vec<Command>,
    pub usage: Option<Usage>,
    /// A task's latest attempt's Git facts in one line.
    pub git: Option<String>,
    /// What each section's opening event says about its session, by seq.
    pub notes: BTreeMap<i64, SectionNote>,
    /// What the composer shows and offers: its state, its buttons and its placeholder.
    pub mode: Mode,
    /// Messages on their way to it, and the keys of its requests waiting on the person.
    pub pending: Vec<Pending>,
    pub requests: Vec<String>,
    /// The conversation in turns (`turns`).
    pub rows: Vec<Row>,
}

/// What the conversations are drawn from besides the run's state: the events they show, by seq,
/// each change's line counts, the language and the display level.
pub(super) struct Drawn<'a> {
    pub events: &'a HashMap<i64, Event>,
    pub counts: &'a dyn Fn(i64, usize) -> (usize, usize),
    pub lang: Lang,
    pub detail: bool,
    /// What a chat's or a Loop's composer calls the one it talks to: the employee, the Loop.
    pub name: &'a str,
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
pub(super) fn sessions(st: &RunState, live: bool, replaying: bool, drawn: &Drawn<'_>) -> BTreeMap<String, SessionView> {
    let status = st.run.status;
    let l = drawn.lang;
    let mut conflicts: Vec<&Event> = drawn.events.values().filter(|e| e.kind() == EventKind::TaskConflict).collect();
    conflicts.sort_by_key(|e| e.seq);
    let resume = matches!(status, Some(RunStatus::Interrupted | RunStatus::Failed));
    let working = matches!(status, Some(RunStatus::Planning | RunStatus::Running));
    let seqs = |key: &str| st.sessions.events.get(key).cloned().unwrap_or_default();
    let mut out = BTreeMap::new();
    let mut put = |key: String, events: Vec<i64>, f: Facts, task: Option<&str>, agent: Option<&str>| {
        let composer = f.composer(live, replaying);
        let evs: Vec<&Event> = events.iter().filter_map(|s| drawn.events.get(s)).collect();
        let busy = f.open && live && !f.idle && !replaying;
        let shown = turns::shown(&evs, st, &conflicts);
        let options = turns::Options { detail: drawn.detail, busy, cwd: &st.run.cwd, lang: l, counts: drawn.counts };
        let notes = evs.iter().filter_map(|e| aside::section_note(e, l).map(|n| (e.seq, n))).collect();
        let mode = mode(&f, composer, live, st.run.kind, drawn.name, l);
        out.insert(
            key,
            SessionView {
                open: f.open,
                human: f.human,
                idle: f.idle,
                converse: f.converse,
                chat: f.chat,
                composer,
                busy,
                plan: aside::plan(&evs),
                controls: aside::controls(&evs, l),
                commands: aside::commands(&evs),
                usage: aside::usage(&evs, l),
                git: task.and_then(|t| aside::task_git(&evs, t, l)),
                notes,
                mode,
                pending: aside::pending(st, task, agent, l),
                requests: aside::requests(&evs, st),
                rows: turns::rows(&shown, &options),
                events,
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
        put(key.clone(), seqs(&key), f, Some(&t.id), None);
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
        put(key, events, f, None, Some(a));
    }
    out
}

crate::wire::wire_enum! {
    /// What a composer button does to its session: send the draft, or one of the session actions.
    pub enum Act {
        Message = "message",
        Interrupt = "interrupt",
        Takeover = "takeover",
        Release = "release",
        Finish = "finish",
        ResumeRun = "resumeRun",
    }
}

/// A composer button.
#[derive(Clone, Debug, Serialize)]
pub struct Button {
    pub act: Act,
    pub label: String,
    /// Sends the draft: the main button, off while the draft is empty.
    pub primary: bool,
    /// Stops the turn.
    pub danger: bool,
}

/// What a conversation's composer shows: the session's state in a pill, its buttons, its
/// placeholder (`keys`: the client adds how to paste and send), and whether it takes a message.
#[derive(Clone, Debug, Serialize)]
pub struct Mode {
    pub state: String,
    pub tone: Tone,
    pub buttons: Vec<Button>,
    pub placeholder: String,
    pub keys: bool,
    pub enabled: bool,
}

/// The composer of a session with facts `f`, whose composer is `c`; `name` is who a chat or a
/// Loop talks to.
fn mode(f: &Facts, c: Composer, live: bool, kind: RunKind, name: &str, l: Lang) -> Mode {
    let b = |act, zh: &str, en: &str, primary: bool, danger: bool| Button {
        act,
        label: l.pick(zh, en).into(),
        primary,
        danger,
    };
    let send = |zh: &str, en: &str| b(Act::Message, zh, en, true, false);
    let agent = || l.pick("发消息给 Agent", "Message the Agent").to_string();
    let m = |state: &str, tone, buttons, placeholder: String, keys, enabled| Mode {
        state: state.into(),
        tone,
        buttons,
        placeholder,
        keys,
        enabled,
    };
    let here = f.open && live;
    // A consolidation is one turn: once it ends there is nothing to send it.
    if kind == RunKind::Dream && !here && c != Composer::Replay {
        let ph = l.pick(
            "整理只有一轮；要再整理，在工作区页点「现在整理」",
            "A consolidation is one turn; to consolidate again, click Consolidate Now on the workspace page",
        );
        return m(l.pick("整理结束", "Consolidation finished"), Tone::Gray, vec![], ph.into(), false, false);
    }
    match c {
        Composer::Replay => m(
            l.pick("回放中", "Replaying"),
            Tone::Gray,
            vec![],
            l.pick("回放中只能看，回到实时再发消息", "Replay is read-only. Go live to send messages.").into(),
            false,
            false,
        ),
        Composer::Human if f.idle => m(
            l.pick("人工接管 · 等你回话", "Manual control · Waiting for you"),
            Tone::Amber,
            vec![
                b(Act::Finish, "以当前结果完成", "Finish with Current Result", false, false),
                b(Act::Release, "交还托管", "Return Control", false, false),
                send("发送", "Send"),
            ],
            agent(),
            true,
            true,
        ),
        Composer::Human => m(
            l.pick("人工接管 · 正在处理", "Manual control · Working"),
            Tone::Blue,
            vec![
                b(Act::Interrupt, "打断这一轮", "Interrupt Turn", false, true),
                b(Act::Release, "交还托管", "Return Control", false, false),
                send("发送", "Send"),
            ],
            agent(),
            true,
            true,
        ),
        // A chat at work: a model may think for minutes before it writes anything; it can only
        // be stopped or added to.
        Composer::Managed if f.chat => m(
            l.pick("正在处理", "Working"),
            Tone::Blue,
            vec![b(Act::Interrupt, "停止这一轮", "Stop This Turn", false, true), send("插话", "Steer")],
            l.pick("补充或更正，会插入正在进行的这一轮", "Add or correct something; it joins the turn in progress")
                .into(),
            false,
            true,
        ),
        Composer::Managed => m(
            l.pick("托管中 · Agent 自主推进", "Autopilot · Agent driving"),
            Tone::Blue,
            vec![
                b(Act::Interrupt, "打断并接管", "Interrupt and Take Over", false, true),
                b(Act::Takeover, "接管", "Take Over", false, false),
                send("插话", "Steer"),
            ],
            agent(),
            true,
            true,
        ),
        Composer::Interrupted => m(
            l.pick("已中断", "Interrupted"),
            Tone::Gray,
            vec![
                b(Act::ResumeRun, "从中断处继续", "Resume Interrupted Run", false, false),
                send("发送并继续", "Send and Resume"),
            ],
            l.pick(
                "补充信息或直接写「继续」，发送后从中断处继续",
                "Add details or just say “continue” to pick up where it stopped",
            )
            .into(),
            false,
            true,
        ),
        // A chat waits for the next message; one whose session is gone reopens with it.
        Composer::Converse if f.chat => {
            let state = match (kind == RunKind::Loop, here) {
                (true, true) => l.pick("摸鱼中，等下次开工或你的消息", "Waiting for its next wake, or your message"),
                (true, false) => l.pick("会话已结束，下次开工时再接上", "Session ended · picks up at the next wake"),
                (false, true) => l.pick("等你回话", "Waiting for You"),
                (false, false) => l.pick("会话已结束", "Session Ended"),
            };
            let (ph, keys) = if here {
                (tr!(l, "发消息给 {name}", "Message {name}"), true)
            } else {
                (tr!(l, "发送后重新打开和 {name} 的会话", "Sending reopens the session with {name}"), false)
            };
            m(state, Tone::Gray, vec![send("发送", "Send")], ph, keys, true)
        }
        // The first goal is done; the same session now takes ordinary prompts, and a slash
        // command starts a new goal. A run no longer held reopens its session with the message.
        Composer::Converse => m(
            l.pick("可以继续对话", "Conversation can continue"),
            Tone::Gray,
            vec![send("发送", "Send")],
            if live {
                l.pick("接下来做什么？也可以用 /goal 换个目标", "What's next? Or use /goal to set a new goal")
            } else {
                l.pick("写下任务，发送后恢复负责人会话", "Send a task to reopen the lead’s session")
            }
            .into(),
            false,
            true,
        ),
        Composer::Leave => m(
            l.pick("当前没有会话", "No Active Session"),
            Tone::Gray,
            vec![send("留言", "Leave Message")],
            l.pick("留言 · 下次会话带给它", "Note · Delivered at its next session").into(),
            false,
            true,
        ),
        Composer::Ended => m(
            l.pick("会话已结束", "Session Ended"),
            Tone::Gray,
            vec![],
            if live { l.pick("会话已结束", "Session Ended") } else { l.pick("只读查看", "Read-only") }.into(),
            false,
            false,
        ),
    }
}
