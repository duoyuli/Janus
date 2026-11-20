//! The control plane. The lead agent decides *what* to do (the plan), the orchestrator decides
//! *when* and *by whom*, coding agents decide *how*. Only the orchestrator changes task state,
//! and every change is written to SQLite as an event before anything else happens.
//!
//! This file holds the run's state and the `App` core (events, waiters, status). Each child
//! module adds one part of `App`: `lifecycle` creates and drives runs, `lead` runs the lead's
//! session, `board` and `board_tools` serve the shared board, `messages` routes messages,
//! `attempts` runs a task's attempts, `turn` streams one prompt turn, `session` opens ACP
//! sessions, `prompts` writes what sessions are told and reads their results, and `actions`
//! applies what the person does and recovers runs after a restart.

use crate::acp::{AcpConn, Incoming};
use crate::agents::Runtime;
use crate::db::{ArtifactRow, AttemptUpdate, Db, Event, VerificationRow};
use crate::evidence::{Check, Summary, TaskAcceptance, Verdict, Verification, effective_verdict};
use crate::git_probe;
use crate::status::{AgentPhase, AttemptStatus, CommandState, RunStatus, TaskStatus};
use crate::traces::Overlap;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};

mod actions;
mod attempts;
mod board;
mod board_tools;
#[cfg(test)]
mod flow_tests;
mod lead;
mod lifecycle;
mod mcp_router;
mod messages;
mod prompts;
mod session;
#[cfg(test)]
mod tests;
mod turn;

pub use actions::{ActionRefused, ActionReply, DeleteRefused};
use attempts::*;
use board::*;
use board_tools::*;
use lead::*;
use messages::Delivery;
use prompts::*;

use session::*;
pub use turn::cancelled_reply;
use turn::*;

/// Attempts per delegated task before it fails and the lead decides what to do next.
const MAX_ATTEMPTS: i64 = 2;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Spawning the adapter and opening the session; separate from the turn clock so a hung start fails fast.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long a steering request may hold up the session loop before the message is queued instead.
const STEER_TIMEOUT: Duration = Duration::from_secs(15);
const RUN_BUDGET_MS: i64 = 6 * 60 * 60 * 1000;
/// The watchdog tells the lead about an attempt whose session has shown no sign of work this long.
pub const STALL_AFTER: Duration = Duration::from_secs(10 * 60);
/// The watchdog tells the lead about an attempt that made the same tool call this many times in a row.
const LOOP_REPEAT: u32 = 5;
/// Board events that need the lead are gathered this long before they go into its session, so
/// a burst arrives as one message.
const PUSH_SETTLE: Duration = Duration::from_secs(2);
/// How much of a result the board inlines; the rest is one `task_detail` call away.
const INLINE_SUMMARY: usize = 300;
const INLINE_ITEM: usize = 160;
const INLINE_ITEMS: usize = 3;

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Where a run's driver starts: from scratch, back into the lead's goal (`resumed` when
/// continuing an interrupted run), or executing the board's tasks after the lead is done.
enum Phase {
    Start,
    Execute,
    Goal { resumed: bool },
}

/// Returned when the user cancels a run; callers stop quietly instead of recording a failure.
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("已取消")
    }
}
impl std::error::Error for Cancelled {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: String,
    pub runtime: String,
    #[serde(default)]
    pub lead: bool,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub init: String,
    /// Model and reasoning effort chosen on the role; applied with session/set_config_option.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    /// Session controls the role chose from what its agent declares (permission mode, plan, ...),
    /// by control id. Applied to every session this member opens.
    #[serde(default)]
    pub session: Value,
    /// The permission mode in the agent's own words, e.g. "Full access", shown in the run's audit
    /// view; Janus itself does not check it.
    #[serde(default)]
    pub permission: String,
    pub kind: Runtime,
    #[serde(default)]
    pub runtime_name: String,
    /// The provider's custom prompt, appended to every prompt this member receives.
    #[serde(default)]
    pub prompt: String,
    /// The role's instructions.
    #[serde(default)]
    pub role_prompt: String,
    /// The library items the role uses, as they were when the run started. Skills are copied into
    /// the member's bundle (`App::bundle_dir`); MCP values are read when a session opens and never
    /// appear here.
    #[serde(default)]
    pub resources: Vec<crate::library::Item>,
    #[serde(default)]
    pub role_id: String,
}

#[derive(Deserialize, Default)]
pub struct NewRun {
    pub goal: String,
    pub workspace: String,
    /// Images pasted with the goal: `[{data (base64), name}]`, see `attachments`.
    #[serde(default)]
    pub images: Vec<Value>,
    /// Optional display-only token reference overriding `Config.run_token_budget`.
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: Option<i64>,
    /// Optional per-run USD cost budget overriding `Config.run_cost_budget_usd`.
    #[serde(default, rename = "maxCostUsd")]
    pub max_cost_usd: Option<f64>,
}

/// A task on the board, as the lead orchestrated it (see `validate_orchestration`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub desc: String,
    /// Tasks that must succeed before this one starts.
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub assignee: String,
    #[serde(default = "default_max_attempts", rename = "maxAttempts")]
    pub max_attempts: i64,
    /// Files and directories the task may change (see `teamwork`).
    #[serde(default)]
    pub owns: Vec<String>,
    /// The task this one independently verifies (see `teamwork`).
    #[serde(default)]
    pub verifies: Option<String>,
    /// An earlier task of the same assignee whose ACP session this task continues via `session/load`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<String>,
}

fn default_max_attempts() -> i64 {
    MAX_ATTEMPTS
}

/// What a person can do inside a running attempt's ACP session.
pub enum Steer {
    Message(Msg),
    /// From the lead: joins the running turn where the agent supports steering; otherwise the
    /// turn is stopped and the message goes in right away, instead of waiting for a long turn.
    Redirect(Msg),
    Interrupt,
    TakeOver,
    Release,
    Finish,
    /// Switch one of the session's controls.
    Control(SessionControl),
}

/// A control the person switches in an open session: its id, the new value, and how the agent
/// takes it.
pub struct SessionControl {
    pub id: String,
    pub value: Value,
    pub via: ControlVia,
}

/// How an agent switches a control: as an ACP session mode (`session/set_mode`) or as a config
/// option (`session/set_config_option`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlVia {
    Mode,
    Config,
}

impl ControlVia {
    /// The `via` an action or a control declaration names; anything but `mode` is a config option.
    pub fn parse(s: &str) -> ControlVia {
        if s == "mode" { ControlVia::Mode } else { ControlVia::Config }
    }

    fn as_str(self) -> &'static str {
        match self {
            ControlVia::Mode => "mode",
            ControlVia::Config => "config",
        }
    }
}

/// A message for a session, from the person or the lead. It is recorded the moment it is sent
/// (`user.message`, delivery "pending"); the session then says how it went in (`message.routed`)
/// and, for one that waited, when it got there (`message.delivered` with its id).
#[derive(Clone)]
pub struct Msg {
    pub id: String,
    pub text: String,
}

/// Who decides what an open session does once a turn ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    /// The agent works on its own: Janus sends the next prompt (or takes the result).
    Agent,
    /// The person took over (or interrupted): the session waits for their message, and its
    /// silence is theirs, not the agent's.
    Person,
    /// The person asked to end with the result so far.
    Finish,
}

/// The person's side of an open session: their commands, who holds it, and the messages that
/// wait for the turn to end. Changing who holds it also tells the watchdog, so the two never
/// disagree.
struct Control {
    rx: mpsc::UnboundedReceiver<Steer>,
    /// The agent takes messages into a running turn (`_session/steering`).
    steering: bool,
    hold: Hold,
    /// Janus cancelled the running turn itself, so the `cancelled` it ends with is expected.
    /// Cleared when the turn returns.
    interrupted: bool,
    queued: Vec<Msg>,
    owner: Owner,
}

impl Control {
    fn new(rx: mpsc::UnboundedReceiver<Steer>, steering: bool, owner: Owner) -> Control {
        Control { rx, steering, hold: Hold::Agent, interrupted: false, queued: vec![], owner }
    }

    fn held_by_person(&self) -> bool {
        self.hold == Hold::Person
    }

    fn finishing(&self) -> bool {
        self.hold == Hold::Finish
    }

    /// Hand the session to `hold`; the watchdog leaves a session the person holds alone.
    fn set_hold(&mut self, run: &Run, hold: Hold) {
        self.hold = hold;
        run.set_human(&self.owner.session, self.owner.attempt, hold == Hold::Person);
    }

    /// A finished turn of the lead's conversation: the next one starts with the agent in charge.
    fn next_turn(&mut self, run: &Run) {
        self.interrupted = false;
        self.set_hold(run, Hold::Agent);
    }
}

/// How long an agent's text may sit unshown after its last chunk.
const MESSAGE_QUIET: Duration = Duration::from_secs(1);

const RELEASE_PROMPT: &str =
    "人工介入结束。请结合上面的对话，自主完成原来的子任务。完成后按最初的要求，以 ```json 结构化结果结束。";

/// The agent finished but a verification command failed. Running the same command against a new
/// session rarely changes the outcome (the command may not fit the workspace), so the lead decides.
#[derive(Debug)]
struct VerificationFailed(String);

impl std::fmt::Display for VerificationFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "验证失败：{}", self.0)
    }
}

impl std::error::Error for VerificationFailed {}

#[derive(Clone, Debug, Default)]
struct TaskResult {
    summary: String,
    /// Downstream handoff notes (interfaces, contracts, gotchas), separate from the one-line summary.
    handoff: Option<String>,
    evidence: Vec<String>,
    risks: Vec<String>,
    /// The agent's own check of each acceptance criterion, in order; `evidence::Check` carries the
    /// method behind each one.
    checks: Vec<Check>,
    /// For a `verifies` task: its explicit judgment on the verified target (`approved`, `rejected`, `inconclusive`).
    verdict: Option<Verdict>,
    /// An unknown `verdict` string kept verbatim so `verify_result` reports it instead of coercing it.
    verdict_error: Option<String>,
    question: Option<Value>,
}

/// Where a task is, with what only that state has: the attempt a running task is on, the failed
/// dependency a blocked one waits on, the task whose cancellation dropped a skipped one. Leaving a
/// state drops its data with it, so none of it outlives the state it belongs to.
// @project-doc docs/domains/20261031-run_lifecycle.md#task_states
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum TaskPhase {
    #[default]
    Pending,
    Running {
        attempt: String,
    },
    Blocked {
        by: String,
    },
    Succeeded,
    Failed,
    Cancelled,
    /// The lead dropped it with `cancel`; `because` is the upstream task whose cancellation took
    /// it along (retrying that task brings it back), none when the lead named it.
    SkippedByDecision {
        because: Option<String>,
    },
}

impl TaskPhase {
    fn status(&self) -> TaskStatus {
        match self {
            TaskPhase::Pending => TaskStatus::Pending,
            TaskPhase::Running { .. } => TaskStatus::Running,
            TaskPhase::Blocked { .. } => TaskStatus::Blocked,
            TaskPhase::Succeeded => TaskStatus::Succeeded,
            TaskPhase::Failed => TaskStatus::Failed,
            TaskPhase::Cancelled => TaskStatus::Cancelled,
            TaskPhase::SkippedByDecision { .. } => TaskStatus::SkippedByDecision,
        }
    }
}

struct Task {
    def: TaskDef,
    agent: String,
    /// The run's last event when the lead wrote this task to the board: what the lead knew when
    /// it planned the task (see `DepGate::holds`).
    planned_seq: i64,
    /// Changed only through `set_phase` (or `restore` when the board is rebuilt from events).
    phase: TaskPhase,
    attempts: i64,
    result: Option<TaskResult>,
    /// The seq of its latest `task.succeeded`: kept after it is reopened, since what others took
    /// from that result is judged against it. Whether it is succeeded now is `status()`.
    last_success_seq: Option<i64>,
    artifacts: Vec<Value>,
    /// Attempts made before the current retry budget started (a user retry).
    retry_base: i64,
    /// The lead's corrections to this task's conclusion, handed on with it (`result.amended`).
    amendments: Vec<String>,
    /// Where the finished task's result disagreed with the workspace (`traces::gaps`).
    trace_gaps: Vec<String>,
    /// Why the next attempt starts after the task had ended, and in which session; taken when
    /// that attempt starts (see `Reopen`).
    reopen: Option<Reopen>,
}

impl Task {
    fn new(def: TaskDef, agent: String, planned_seq: i64) -> Task {
        Task {
            def,
            agent,
            planned_seq,
            phase: TaskPhase::Pending,
            attempts: 0,
            result: None,
            last_success_seq: None,
            artifacts: vec![],
            retry_base: 0,
            amendments: vec![],
            trace_gaps: vec![],
            reopen: None,
        }
    }

    fn status(&self) -> TaskStatus {
        self.phase.status()
    }

    /// Move to `to`; an illegal step is a scheduler bug, so tests stop on it and a release
    /// build records it instead of silently accepting it.
    fn set_phase(&mut self, to: TaskPhase) {
        let (from, next) = (self.status(), to.status());
        if !from.can_transition_to(next) {
            crate::config::log(format!("janus: task {}: illegal status {from:?} -> {next:?}", self.def.id));
        }
        debug_assert!(from.can_transition_to(next), "task {}: {from:?} -> {next:?}", self.def.id);
        self.phase = to;
    }

    /// The board rebuilt from the event log: not a transition, the log already decided it.
    fn restore(&mut self, phase: TaskPhase) {
        self.phase = phase;
    }

    /// The attempt a running task is on.
    fn attempt(&self) -> Option<&str> {
        match &self.phase {
            TaskPhase::Running { attempt } => Some(attempt),
            _ => None,
        }
    }

    /// Whether `attempt` still owns this task: running, on that attempt. A cancel or retry by the
    /// lead in between ends that.
    fn on_attempt(&self, attempt: &str) -> bool {
        self.attempt() == Some(attempt)
    }

    /// The failed dependency a blocked task waits on.
    fn blocked_by(&self) -> Option<&str> {
        match &self.phase {
            TaskPhase::Blocked { by } => Some(by),
            _ => None,
        }
    }

    /// The upstream task whose cancellation dropped this one.
    fn skipped_because(&self) -> Option<&str> {
        match &self.phase {
            TaskPhase::SkippedByDecision { because } => because.as_deref(),
            _ => None,
        }
    }

    /// Where its current success was recorded; none unless it is succeeded now.
    fn success_seq(&self) -> Option<i64> {
        self.last_success_seq.filter(|_| self.phase == TaskPhase::Succeeded)
    }

    /// A finished independent verification's judgment of the task it verifies; `None` for any
    /// other task and for a verification that has not succeeded.
    fn verdict(&self) -> Option<Verdict> {
        if self.def.verifies.is_none() || self.status() != TaskStatus::Succeeded {
            return None;
        }
        Some(match &self.result {
            Some(r) => effective_verdict(r.verdict, self.def.accept.len(), &r.checks),
            None => effective_verdict(None, self.def.accept.len(), &[]),
        })
    }

    /// The judgment of a verification's last result, also while it waits to check reworked work
    /// again: what the rework acts on.
    fn last_verdict(&self) -> Option<Verdict> {
        self.verdict().or_else(|| {
            let r = self.result.as_ref().filter(|_| self.def.verifies.is_some())?;
            Some(effective_verdict(r.verdict, self.def.accept.len(), &r.checks))
        })
    }
}

/// Which ACP session the next attempt of a retried task starts in. The lead names it with every
/// `retry`: the task's own last session, or a fresh one.
// @project-doc docs/domains/20261031-run_lifecycle.md#retry
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionChoice {
    Continue,
    New,
}

impl SessionChoice {
    pub(crate) fn parse(s: &str) -> Option<SessionChoice> {
        match s {
            "continue" => Some(SessionChoice::Continue),
            "new" => Some(SessionChoice::New),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SessionChoice::Continue => "continue",
            SessionChoice::New => "new",
        }
    }
}

/// Why an ended task runs again, as `task.reopened` records it (`data.reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReopenReason {
    /// The lead's `retry`.
    LeadRetry,
    /// Dropped only because an upstream task was cancelled; it comes back with that task's retry.
    UpstreamRetry,
    /// A verification whose target the lead retried: its verdict was about the old result.
    TargetReworked,
}

impl ReopenReason {
    pub(crate) fn parse(s: &str) -> Option<ReopenReason> {
        match s {
            "lead.retry" => Some(ReopenReason::LeadRetry),
            "upstream.retry" => Some(ReopenReason::UpstreamRetry),
            "target.reworked" => Some(ReopenReason::TargetReworked),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReopenReason::LeadRetry => "lead.retry",
            ReopenReason::UpstreamRetry => "upstream.retry",
            ReopenReason::TargetReworked => "target.reworked",
        }
    }
}

/// How the next attempt of a task that had ended starts: what the lead's `retry` asked for, or a
/// verification checking its reworked target again. Rebuilt from `task.reopened` on resume.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reopen {
    /// The lead's `retry`: in which session, with the findings of which verification (`from`), the
    /// lead's note for this round, and whether it revised the instructions or criteria.
    Retry { session: SessionChoice, from: Option<String>, note: Option<String>, revised: bool },
    /// The lead retried the task this verification checks: it checks the new result again, in
    /// its own last session.
    Reverify { target: String },
}

impl Reopen {
    /// The reopen a `task.reopened` event asks for; `None` for one that changes nothing about how
    /// the next attempt starts (`upstream.retry`).
    fn of_event(d: &Value) -> Option<Reopen> {
        let text = |k: &str| d[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        match ReopenReason::parse(d["reason"].as_str()?)? {
            ReopenReason::LeadRetry => Some(Reopen::Retry {
                session: d["session"].as_str().and_then(SessionChoice::parse).unwrap_or(SessionChoice::New),
                from: text("from"),
                note: text("note"),
                revised: d["instructions"].is_string() || d["accept"].is_array(),
            }),
            ReopenReason::TargetReworked => Some(Reopen::Reverify { target: text("target")? }),
            ReopenReason::UpstreamRetry => None,
        }
    }

    /// What `attempt.started` records about it (`data.reopen`), so the app can say why the attempt
    /// started and on whose findings.
    fn record(&self) -> Value {
        match self {
            Reopen::Retry { session, from, .. } => {
                let mut v = json!({ "reason": ReopenReason::LeadRetry.as_str(), "session": session.as_str() });
                if let Some(f) = from {
                    v["from"] = json!(f);
                }
                v
            }
            Reopen::Reverify { target } => {
                json!({ "reason": ReopenReason::TargetReworked.as_str(), "target": target })
            }
        }
    }

    /// Whether the attempt goes back into the task's own last session.
    fn continues_own_session(&self) -> bool {
        match self {
            Reopen::Retry { session, .. } => *session == SessionChoice::Continue,
            Reopen::Reverify { .. } => true,
        }
    }
}

/// What keeps tasks from starting on their dependencies, as of one scheduler pass.
struct DepGate {
    /// Tasks that failed, were cancelled, blocked or skipped.
    failed: HashSet<String>,
    /// Verifications that finished without approving their target: the verdict and when it was
    /// recorded (`i64::MAX` until its event is stored).
    rejected: HashMap<String, (Verdict, i64)>,
}

impl DepGate {
    fn of(tasks: &[Task]) -> DepGate {
        DepGate {
            failed: tasks.iter().filter(|t| t.status().failed_dependency()).map(|t| t.def.id.clone()).collect(),
            rejected: tasks
                .iter()
                .filter_map(|t| {
                    let vd = t.verdict().filter(|vd| *vd != Verdict::Approved)?;
                    Some((t.def.id.clone(), (vd, t.last_success_seq.unwrap_or(i64::MAX))))
                })
                .collect(),
        }
    }

    /// The first dependency that keeps `t` from starting, with the verdict when it is a
    /// verification that did not approve its target. Such a verification holds back only the
    /// tasks planned before its verdict, which assumed the work would pass; a task the lead
    /// orchestrates after reading the verdict (the fix) depends on it knowingly and starts.
    fn holds(&self, t: &Task) -> Option<(String, Option<Verdict>)> {
        t.def.deps.iter().find_map(|id| {
            if self.failed.contains(id) {
                return Some((id.clone(), None));
            }
            let (vd, at) = self.rejected.get(id)?;
            (*at > t.planned_seq).then(|| (id.clone(), Some(*vd)))
        })
    }
}

#[derive(Default)]
struct RunState {
    status: RunStatus,
    /// Only through `set_paused` and `paused`: a run is paused only while it is active.
    paused: bool,
    tasks: Vec<Task>,
    /// Decisions a session waits on, by the key the app and the lead answer them with.
    pending: HashMap<String, Pending>,
    conns: HashMap<SessionKey, Arc<AcpConn>>,
    /// Control channels of open ACP sessions.
    live: HashMap<SessionKey, mpsc::UnboundedSender<Steer>>,
    /// Which open session each agent is in, so a person can talk to an agent without knowing its task.
    live_agent: HashMap<String, SessionKey>,
    /// Messages for an agent that had no open session; delivered with its next session.
    notes: HashMap<String, Vec<Msg>>,
    /// Why each task's last result did not pass its acceptance check, for the fix round and the lead.
    last_verification: HashMap<String, String>,
    /// Sessions to reload when an interrupted task starts again, by task id.
    resume_session: HashMap<String, String>,
    /// Working in place: the working-tree diff last saved as an artifact, so a task that changed
    /// nothing does not repeat what an earlier one changed.
    last_diff: String,
    /// Secret in this run's MCP URL, so only its lead reaches the team tools.
    mcp_token: String,
    lead: LeadState,
    /// Sessions whose agent can take a message into the running turn. Only they get board pushes
    /// and conflict notices: queued until the turn ends, a notice would arrive after the fact.
    steerable: HashSet<SessionKey>,
    /// What each task left in the workspace: the files it edited, who edited each file last,
    /// and the conflicts already reported (see `traces`).
    traces: crate::traces::Traces,
}

impl RunState {
    /// Cancelling is the run's status, not a flag of its own: nothing leaves `Cancelled`.
    fn cancelled(&self) -> bool {
        self.status == RunStatus::Cancelled
    }

    /// Paused by the person, and still active: a finished run is never paused.
    fn paused(&self) -> bool {
        self.paused && self.status.is_active()
    }

    /// Pause or resume an active run; a finished one has nothing to pause.
    fn set_paused(&mut self, on: bool) -> Result<()> {
        if !self.status.is_active() {
            bail!("任务现在是「{}」，不能{}", self.status, if on { "暂停" } else { "继续" });
        }
        self.paused = on;
        Ok(())
    }
}

/// What the run knows about its lead.
#[derive(Default)]
struct LeadState {
    goal: LeadGoal,
    /// The lead's agent reports its `/goal` active: it keeps working between prompts until the
    /// goal settles (Codex), so the session is followed after a prompt returns.
    goal_active: bool,
    /// The last event the lead has seen on the board, through `wait` or pushed into its session.
    cursor: i64,
    /// The lead is inside `wait`, which hands it the board itself.
    waiting: bool,
    /// The lead's open session id, for recording what Janus pushes into it.
    session: Option<String>,
    /// The lead is working on a message (a follow-up goal included), not idle waiting for the
    /// person. Only matters after delivery: see `on_duty`.
    in_turn: bool,
}

impl LeadState {
    /// The lead reads the board: until its first goal ends, and after delivery while it works on
    /// a message. Otherwise nobody does, so questions go to the person and nothing is pushed.
    // @project-doc docs/domains/20261031-goal_mode.md#question_routing
    fn on_duty(&self) -> bool {
        !self.goal.ended() || self.in_turn
    }
}

/// How far the lead's goal is: still worked on, done with the summary it delivered, or ended
/// because its session broke off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum LeadGoal {
    #[default]
    Working,
    Done(String),
    Failed(String),
}

impl LeadGoal {
    fn ended(&self) -> bool {
        *self != LeadGoal::Working
    }
}

/// Whether a driver task (the goal or the scheduler) is alive for a run, and a way to wait until
/// none is.
struct Driving(tokio::sync::watch::Sender<bool>);

impl Default for Driving {
    fn default() -> Self {
        Driving(tokio::sync::watch::channel(false).0)
    }
}

impl Driving {
    /// Take the run for a new driver; false when one is already alive.
    fn claim(&self) -> bool {
        self.0.send_if_modified(|on| !std::mem::replace(on, true))
    }

    fn set(&self, on: bool) {
        self.0.send_replace(on);
    }

    fn active(&self) -> bool {
        *self.0.borrow()
    }

    /// Returns once no driver is alive.
    async fn idle(&self) {
        let _ = self.0.subscribe().wait_for(|on| !*on).await;
    }
}

/// Which run-level ceiling stopped new work (`run.budget_exhausted`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum BudgetHit {
    WallClock,
    CostUsd { used: f64, limit: f64 },
}

/// What a run is made of, from its creation or its record. Everything else about it starts empty.
/// Where a run on a git repository works: its own worktree of `repo` (see `crate::worktrees`).
/// A run without one works in the workspace directory itself.
#[derive(Clone, Debug)]
struct WorktreeOf {
    repo: PathBuf,
    /// The branch checked out in `repo` when the run started, where accepting brings the result
    /// back; `None` when `repo` was on a detached `HEAD`.
    branch: Option<String>,
    /// The ignored files `.worktreeinclude` brought along when the worktree was made (only known
    /// then; a resumed run has none listed).
    included: Vec<String>,
}

impl WorktreeOf {
    /// The worktree a run's record (`runs.config`) names; the files `.worktreeinclude` brought
    /// along are only known when it was made.
    fn of_config(config: &Value) -> Option<WorktreeOf> {
        let w = &config["worktree"];
        Some(WorktreeOf {
            repo: w["repo"].as_str()?.into(),
            branch: w["branch"].as_str().map(str::to_owned),
            included: vec![],
        })
    }

    /// Where accepting brings the result back, for the person to read.
    fn source(&self) -> String {
        match &self.branch {
            Some(b) => format!("{} 的分支 {b}", self.repo.display()),
            None => format!("{} 当前的 detached HEAD", self.repo.display()),
        }
    }
}

struct RunRecord {
    id: String,
    goal: String,
    cwd: PathBuf,
    worktree: Option<WorktreeOf>,
    team: Vec<Member>,
    git: bool,
    base_ref: String,
    brief: String,
    goal_images: Vec<Value>,
    /// The last event already in the log; the next one gets the following seq.
    last_seq: i64,
    status: RunStatus,
    cost_limit_micros: i64,
}

pub struct Run {
    pub id: String,
    pub goal: String,
    /// Where the run's sessions work: the worktree for a run on a git repository, otherwise the
    /// workspace directory.
    pub cwd: PathBuf,
    worktree: Option<WorktreeOf>,
    pub team: Vec<Member>,
    git: bool,
    base_ref: String,
    /// The workspace's shared material when the run was created (see `brief_section`).
    brief: String,
    /// Images pasted with the goal, as `attachments::save` stored them (kept in the run's config).
    goal_images: Vec<Value>,
    seq: AtomicI64,
    /// Held while an event takes its seq, is stored and broadcast, so the stream carries a
    /// run's events in seq order; the app reads a gap as missed events and reloads the run.
    /// `true` once the run is deleted: what its sessions still say on the way out is not written.
    emitting: StdMutex<bool>,
    deadline_ms: AtomicI64,
    /// Optional USD cost ceiling in micro-dollars (`0` = unlimited).
    cost_limit_micros: AtomicI64,
    /// Updated in persistence order, seeded from the same events on recovery.
    costs: StdMutex<crate::usage::SessionCosts>,
    st: StdMutex<RunState>,
    wake: Notify,
    driving: Driving,
    /// Something the lead should hear about reached the board (see `push_board`).
    lead_nudge: Notify,
    /// Any board-relevant event or task terminal change arrived (wakes `board_wait` immediately).
    board_wake: Notify,
    /// The library's command servers for everyone, run for this run's sessions (`mcp_router`).
    router: mcp_router::McpRouter,
    /// The watchdog's view of each open session. Its own lock: session updates touch it often,
    /// and nothing else is held while it is.
    watch: StdMutex<HashMap<SessionKey, Watch>>,
}

/// Whose open ACP session something belongs to: the lead's, or a task's. The two never share a
/// namespace, so a task may be called anything the board allows, `lead` included.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SessionKey {
    Lead,
    Task(String),
}

impl SessionKey {
    fn task(id: &str) -> SessionKey {
        SessionKey::Task(id.to_string())
    }
}

/// The session a pending decision came from, and for a task's session its attempt (the lead's is 0).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Owner {
    session: SessionKey,
    attempt: i64,
}

/// What a pending decision asks for: an answer to a question, or a reply to an agent's request
/// (an approval or a form).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Question,
    Request,
}

/// A decision someone waits on. It belongs to the session that asked: when that session ends,
/// its decisions are withdrawn; nothing about it is read from the shape of its key.
struct Pending {
    owner: Owner,
    ask: Ask,
    tx: oneshot::Sender<Value>,
}

/// What the watchdog knows about a running attempt: when its session last showed any sign of
/// work, what phase/tool it is currently in, and recent tool calls for loop detection.
#[derive(Default)]
struct Watch {
    attempt: i64,
    started_ms: i64,
    last_ms: i64,
    /// This quiet spell was already reported; any sign of work starts a new one.
    idle_noticed: bool,
    /// The person holds the session (takeover or interrupt), so its silence is theirs.
    human: bool,
    /// Questions, approvals and forms of this attempt waiting on a person (`Held` guards).
    waits: u32,
    /// Current phase and latest tool title, shown in `janus.wait` live pulse on timeout.
    phase: AgentPhase,
    last_tool: String,
    /// Recent finished tool calls, for loop detection.
    calls: lifecycle::CallLoop,
}

/// While alive, the session waits on a person and the watchdog leaves it alone; dropping it
/// starts the quiet clock again.
struct Held<'a> {
    run: &'a Run,
    session: SessionKey,
    attempt: i64,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.run.with_watch(&self.session, self.attempt, |w| {
            w.waits = w.waits.saturating_sub(1);
            w.last_ms = now_ms();
        });
    }
}

impl Run {
    /// The Git facts a session of this run works with, for the person to see (`data.git` of
    /// `attempt.started`): the repository, the branch accepting merges back into (`null` for a
    /// detached `HEAD`), the worktree and the commit it is at now. `None` outside a worktree.
    // @project-doc docs/interfaces/20261031-events.md#event_types
    async fn git_facts(&self) -> Option<Value> {
        let w = self.worktree.as_ref()?;
        let head = git(&self.cwd, &["rev-parse", "HEAD"]).await.ok().map(|s| s.trim().to_string());
        Some(json!({ "repo": w.repo, "branch": w.branch, "worktree": self.cwd, "head": head }))
    }

    fn new(r: RunRecord) -> Run {
        Run {
            id: r.id,
            goal: r.goal,
            cwd: r.cwd,
            worktree: r.worktree,
            team: r.team,
            git: r.git,
            base_ref: r.base_ref,
            brief: r.brief,
            goal_images: r.goal_images,
            seq: AtomicI64::new(r.last_seq),
            emitting: StdMutex::new(false),
            deadline_ms: AtomicI64::new(now_ms() + RUN_BUDGET_MS),
            cost_limit_micros: AtomicI64::new(r.cost_limit_micros.max(0)),
            costs: StdMutex::default(),
            st: StdMutex::new(RunState { status: r.status, ..Default::default() }),
            wake: Notify::new(),
            driving: Driving::default(),
            lead_nudge: Notify::new(),
            board_wake: Notify::new(),
            watch: Default::default(),
            router: Default::default(),
        }
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.team.iter().find(|m| m.id == id)
    }
    fn task_mut<'a>(st: &'a mut RunState, id: &str) -> Option<&'a mut Task> {
        st.tasks.iter_mut().find(|t| t.def.id == id)
    }

    /// Change what the watchdog knows about attempt `n` of a session, if that attempt is open.
    fn with_watch<T>(&self, session: &SessionKey, n: i64, f: impl FnOnce(&mut Watch) -> T) -> Option<T> {
        self.watch.lock().unwrap().get_mut(session).filter(|w| w.attempt == n).map(f)
    }

    /// The session waits on a person until the returned guard is dropped.
    fn held(&self, session: &SessionKey, n: i64) -> Held<'_> {
        self.with_watch(session, n, |w| w.waits += 1);
        Held { run: self, session: session.clone(), attempt: n }
    }

    /// The person took over (or interrupted) the session, or handed it back.
    fn set_human(&self, session: &SessionKey, n: i64, human: bool) {
        self.with_watch(session, n, |w| {
            w.human = human;
            w.last_ms = now_ms();
        });
    }
}

/// Everything `App` reads about where it lives and how it runs. Code reads these, never the
/// process's settings, so an `App` over another home (a test's) stays inside it.
pub struct Config {
    /// Janus home (~/.janus): the database, ACP adapters (adapters/), run data (artifacts/,
    /// logs/, runs/) and everything else that belongs to this machine's user, not to a checkout.
    pub data_dir: PathBuf,
    /// Coding agent configs and credentials, kept in the user's Janus home: ~/.janus/agents.
    pub agents_dir: PathBuf,
    /// Where this Janus listens, for the MCP URLs handed to agents.
    pub addr: String,
    /// The Node executable the ACP adapters run on and are installed with.
    pub node: String,
    /// How long an attempt may show no sign of work before the watchdog tells the lead.
    pub stall_after: Duration,
    /// How long a lead's session stays quiet with its goal still active before Janus asks it (`goal_settle`).
    pub goal_settle: Duration,
    /// Maximum collaborator task attempts running concurrently in a run (`1..=32`).
    pub max_concurrent_attempts: usize,
    /// Default reference shown alongside reported token usage.
    pub run_token_budget: Option<i64>,
    /// Default per-run reported USD cost budget (`None` = unlimited).
    pub run_cost_budget_usd: Option<f64>,
}

pub struct App {
    pub db: Db,
    pub cfg: Config,
    pub tx: broadcast::Sender<Event>,
    pub runs: StdMutex<HashMap<String, Arc<Run>>>,
    pub adapter_state: crate::adapters::AdapterState,
    /// Configuration checks running now, see `start_check`.
    pub checks: crate::agents::CheckState,
    pub prices: crate::prices::PriceBook,
    pub library: crate::library::LibraryState,
    /// Taken while a run that is not live is brought back, accepted from the archive or deleted,
    /// so two requests for it (two messages sent quickly, a message and a delete) take turns and
    /// the second finds what the first did.
    pub revival: tokio::sync::Mutex<()>,
}

/// Builder for an event before the orchestrator stamps it with seq and time.
pub struct Ev {
    kind: String,
    actor: String,
    text: String,
    to: Option<String>,
    task: Option<String>,
    attempt: Option<i64>,
    data: Option<Value>,
}

pub fn ev(kind: &str, actor: &str, text: impl Into<String>) -> Ev {
    Ev { kind: kind.into(), actor: actor.into(), text: text.into(), to: None, task: None, attempt: None, data: None }
}

impl Ev {
    fn to(mut self, to: &str) -> Ev {
        self.to = Some(to.into());
        self
    }
    fn task(mut self, task: &str) -> Ev {
        self.task = (!task.is_empty()).then(|| task.into());
        self
    }
    fn attempt(mut self, n: i64) -> Ev {
        self.attempt = (n > 0).then_some(n);
        self
    }
    fn data(mut self, data: Value) -> Ev {
        self.data = Some(data);
        self
    }
    /// The event happens in the session `ctx` speaks in: its task and attempt, if it has them.
    fn scope(mut self, ctx: &Ctx<'_>) -> Ev {
        self.task = ctx.task.filter(|t| !t.is_empty()).map(str::to_owned);
        self.attempt = ctx.attempt.filter(|n| *n > 0);
        self
    }
}

struct Ctx<'a> {
    agent: &'a str,
    task: Option<&'a str>,
    attempt: Option<i64>,
    /// The lead's goal session: a prompt that returns while the agent's goal is still active
    /// does not end the turn; the session is followed until the goal settles.
    goal: bool,
}

impl Ctx<'_> {
    /// The session this context speaks in: its task's, or the lead's.
    fn session(&self) -> SessionKey {
        self.task.map_or(SessionKey::Lead, SessionKey::task)
    }

    /// Who the session's pending decisions belong to.
    fn owner(&self) -> Owner {
        Owner { session: self.session(), attempt: self.attempt.unwrap_or(0) }
    }
}

/// The open ACP session a prompt turn runs in: its connection, its incoming messages and its id.
struct PromptSession<'a> {
    conn: &'a Arc<AcpConn>,
    rx: &'a mut mpsc::UnboundedReceiver<Incoming>,
    sid: &'a str,
}

/// A file an attempt produced, saved under the run's artifacts (`save_artifact`).
struct NewArtifact<'a> {
    task: &'a str,
    attempt: i64,
    agent: &'a str,
    name: &'a str,
    kind: &'a str,
    bytes: &'a [u8],
    desc: &'a str,
}

impl App {
    pub fn run(&self, id: &str) -> Option<Arc<Run>> {
        self.runs.lock().unwrap().get(id).cloned()
    }

    // @project-doc docs/interfaces/20261031-events.md#event_shape
    fn emit(&self, run: &Run, e: Ev) -> i64 {
        let deleted = run.emitting.lock().unwrap_or_else(|p| p.into_inner());
        if *deleted {
            return 0;
        }
        let seq = run.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let event = Event {
            run_id: run.id.clone(),
            seq,
            ts: now_ms(),
            kind: e.kind,
            actor: e.actor,
            to: e.to,
            task: e.task,
            attempt: e.attempt,
            text: e.text,
            data: e.data,
        };
        let nudge = needs_lead(&event);
        let wakes_board = wakes_board_wait(&event);
        let stored = self.store(event, Some(run));
        drop(deleted);
        if stored {
            if wakes_board {
                run.board_wake.notify_waiters();
            }
            if nudge {
                run.lead_nudge.notify_one();
            }
        }
        seq
    }

    /// Write an event, then broadcast it. The log is the source of truth: an event that could not
    /// be written is not broadcast either, so the app never shows what a reload would not find.
    /// Its `seq` stays unused.
    fn store(&self, event: Event, run: Option<&Run>) -> bool {
        if let Err(err) = self.db.insert_event(&event) {
            crate::config::log(format!("janus: failed to persist event {}#{}: {err:#}", event.run_id, event.seq));
            return false;
        }
        if let Some(run) = run
            && matches!(event.kind.as_str(), "attempt.started" | "agent.status" | "session.usage")
        {
            run.costs.lock().unwrap().observe(&event);
        }
        let _ = self.tx.send(event);
        true
    }

    /// The only place a run's lifecycle state changes. `RunStatus` states the legal moves;
    /// cancelling also records the flag the schedulers check.
    fn set_status(&self, run: &Run, status: RunStatus) {
        {
            let mut st = run.st.lock().unwrap();
            if !st.status.can_transition_to(status) {
                crate::config::log(format!("janus: run {}: illegal status {:?} -> {:?}", run.id, st.status, status));
            }
            debug_assert!(st.status.can_transition_to(status), "run {}: {:?} -> {:?}", run.id, st.status, status);
            st.status = status;
            // Pausing only makes sense while work can still start; a finished Run is not paused.
            if !status.is_active() {
                st.paused = false;
            }
        }
        // A stopped run's library servers stop with it, mid-call too: nothing it started keeps
        // acting after the person stopped it. A delivered run keeps them for the conversation.
        // @project-doc docs/domains/20261116-resource_library.md#mcp_router
        if matches!(status, RunStatus::Cancelled | RunStatus::Failed | RunStatus::Interrupted) {
            run.router.close();
        }
        let _ = self.db.set_run_status(&run.id, status);
    }

    /// Wait for the decision `key`, which `owner` asked for.
    async fn wait_for(&self, run: &Run, key: &str, owner: Owner, ask: Ask) -> Result<Value> {
        let rx = self.add_pending(run, key, owner, ask)?;
        rx.await.map_err(|_| anyhow::Error::new(Cancelled))
    }

    /// Register a pending decision; its answer arrives on the returned receiver.
    fn add_pending(&self, run: &Run, key: &str, owner: Owner, ask: Ask) -> Result<oneshot::Receiver<Value>> {
        let (tx, rx) = oneshot::channel();
        let mut st = run.st.lock().unwrap();
        // Cancelling clears the pending decisions; one added afterwards would never be resolved.
        if st.cancelled() {
            return Err(Cancelled.into());
        }
        st.pending.insert(key.into(), Pending { owner, ask, tx });
        Ok(rx)
    }

    /// Who is waiting on `key`. Checked before a decision is logged, so the audit trail never
    /// records an answer nobody was waiting for.
    fn pending_owner(&self, run: &Run, key: &str) -> Result<Owner> {
        match run.st.lock().unwrap().pending.get(key) {
            Some(p) => Ok(p.owner.clone()),
            None => bail!("没有等待中的决定：{key}"),
        }
    }

    fn resolve(&self, run: &Run, key: &str, value: Value) -> Result<()> {
        let pending = run.st.lock().unwrap().pending.remove(key).ok_or_else(|| anyhow!("没有等待中的决定：{key}"))?;
        let _ = pending.tx.send(value);
        Ok(())
    }

    /// Withdraw every decision `owner` was waiting on: its session ended, so nobody can act on
    /// them any more, and the app removes their cards.
    fn withdraw_owned(&self, run: &Run, owner: &Owner, text: &str) {
        let keys: Vec<String> = {
            let mut st = run.st.lock().unwrap();
            let keys: Vec<String> =
                st.pending.iter().filter(|(_, p)| &p.owner == owner).map(|(k, _)| k.clone()).collect();
            for k in &keys {
                st.pending.remove(k);
            }
            keys
        };
        let task = match &owner.session {
            SessionKey::Task(t) => t.as_str(),
            SessionKey::Lead => "",
        };
        for key in keys {
            self.emit(
                run,
                ev("decision.withdrawn", "janus", text).task(task).attempt(owner.attempt).data(json!({ "key": key })),
            );
        }
    }
}

/// Prefix of a message the lead sends a collaborator, so the session and the record both show who wrote it.
const LEAD_REPLY: &str = "负责人回复：";

/// The heading the lead puts its suggested additions to the shared material under (see `goal_brief`).
const BRIEF_HEADING: &str = "Suggested Updates to Shared Context";

/// Read-only git query (see `git_probe::read_only`); not for commands that write the repository.
pub async fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::from(git_probe::read_only(cwd, args)).output().await?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A duration in words: seconds under a minute, minutes after that.
fn span(ms: i64) -> String {
    if ms < 60_000 { format!("{} 秒", ms / 1000) } else { format!("{} 分钟", ms / 60_000) }
}

pub fn short(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() > n { format!("{}…", s.chars().take(n).collect::<String>()) } else { s.to_string() }
}

fn first_line(text: &str) -> String {
    short(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("（空回复）"), 120)
}

fn short_sha(s: &str) -> &str {
    &s[..s.len().min(8)]
}

/// A path with a leading `~` resolved against `HOME`.
pub fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}{rest}"),
        _ => p.to_string(),
    }
}

/// `n` in base 36, lowercase: run ids are `r` plus the time in it.
pub fn to_base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = vec![];
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// Codex and Claude Code adapters that run the scripted agent (`testdata/fake_acp.py`, driven by
/// the `FAKE_DIR` its connection sets) instead of the real one. Janus starts a runtime as
/// `<node> <bin of the adapter package>` from `<data>/adapters/<id>/current`; tests run Python in
/// place of Node, so the bin is the script itself and the launch path is the real one.
// @project-doc docs/operations/20261031-testing.md#fake_agent
#[cfg(test)]
pub(crate) fn stand_in_adapters(data: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_acp.py");
    for a in crate::adapters::ADAPTERS {
        let dir = data.join("adapters").join(a.id);
        let pkg = dir.join("9.9.9").join("node_modules").join(a.package);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("package.json"), json!({ "name": a.package, "bin": "fake_acp.py" }).to_string())
            .unwrap();
        let _ = std::os::unix::fs::symlink(&script, pkg.join("fake_acp.py"));
        let _ = std::os::unix::fs::symlink("9.9.9", dir.join("current"));
    }
}

/// An `App` over a fresh database in `dir`, for tests, with the stand-in adapters installed.
#[cfg(test)]
pub(crate) fn test_app(dir: &Path) -> Arc<App> {
    std::fs::create_dir_all(dir).unwrap();
    stand_in_adapters(&dir.join("data"));
    let (tx, _) = broadcast::channel(4096);
    Arc::new(App {
        db: Db::open(&dir.join("janus.db")).unwrap().0,
        cfg: Config {
            data_dir: dir.join("data"),
            agents_dir: dir.join("agents"),
            addr: "127.0.0.1:7878".into(),
            // The stand-in adapters are Python scripts: see `stand_in_adapters`.
            node: "python3".into(),
            stall_after: STALL_AFTER,
            goal_settle: crate::goal_settle::SETTLE,
            max_concurrent_attempts: crate::config::DEFAULT_MAX_CONCURRENT_ATTEMPTS,
            run_token_budget: None,
            run_cost_budget_usd: None,
        },
        tx,
        runs: StdMutex::new(HashMap::new()),
        adapter_state: Default::default(),
        checks: Default::default(),
        prices: Default::default(),
        library: Default::default(),
        revival: Default::default(),
    })
}
