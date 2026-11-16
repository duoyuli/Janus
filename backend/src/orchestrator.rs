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
use crate::db::{ArtifactRow, AttemptUpdate, Db, Event, VerificationRow};
use crate::evidence::{Check, Summary, TaskAcceptance, Verification};
use crate::git_probe;
use crate::status::{AgentPhase, AttemptStatus, RunStatus, TaskStatus};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
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
mod messages;
mod prompts;
mod session;
#[cfg(test)]
mod tests;
mod turn;

pub use actions::{ActionRefused, ActionReply};
use attempts::*;
use board::*;
use board_tools::*;
pub use lead::lead_capable;
use lead::*;
use prompts::*;
pub use session::session_meta;
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

/// Where a run's driver starts: from scratch, back into the lead's goal (`true` when continuing
/// an interrupted run), or executing the board's tasks after the lead is done.
enum Phase {
    Start,
    Execute,
    Goal(bool),
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
    pub role: String,
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
    /// The permission mode in the agent's own words, e.g. "Agent (full access) · Off". Handed
    /// to the lead, which assigns work by it; Janus itself does not check it.
    #[serde(default)]
    pub permission: String,
    /// Model and reasoning effort as the lead is told them, e.g. "GPT-6.1 Sol · high（Agent 默认）".
    #[serde(default)]
    pub model_label: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub runtime_name: String,
    /// The provider's custom prompt, appended to every prompt this member receives.
    #[serde(default)]
    pub prompt: String,
    /// What the role mounts on top of its agent: instructions, skills (name, description, path) and stdio MCP servers.
    #[serde(default)]
    pub role_prompt: String,
    #[serde(default)]
    pub skills: Vec<Value>,
    /// The MCP servers this member opens with: the role's own hand-written ones, then the
    /// resources it mounts from the MCP library. Their values never appear here.
    #[serde(default)]
    pub mcp: Vec<Value>,
    /// The MCP library resources the role mounted (`<source id>/<key>`), for the record.
    #[serde(default)]
    pub mcp_refs: Vec<String>,
    #[serde(default)]
    pub role_id: String,
    /// MCP environment values by server name. Never serialized, so they stay out of the run record.
    #[serde(skip)]
    pub mcp_env: Value,
    /// HTTP header values by server name, for library resources reached by URL. Never serialized.
    #[serde(skip)]
    pub mcp_headers: Value,
}

#[derive(Deserialize, Default)]
pub struct NewRun {
    pub goal: String,
    pub workspace: String,
    /// Images pasted with the goal: `[{data (base64), name}]`, see `attachments`.
    #[serde(default)]
    pub images: Vec<Value>,
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
}

fn default_max_attempts() -> i64 {
    MAX_ATTEMPTS
}

impl TaskDef {
    fn dep_failed(&self, statuses: &HashMap<String, TaskStatus>) -> Option<String> {
        self.deps.iter().find(|id| statuses.get(*id).is_some_and(|s| s.failed_dependency())).cloned()
    }
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
    /// Switch a session control: `id`, its new value, and whether it is an ACP mode or a config option.
    Control(String, Value, String),
}

/// A message for a session, from the person or the lead. It is recorded the moment it is sent
/// (`user.message`, delivery "pending"); the session then says how it went in (`message.routed`)
/// and, for one that waited, when it got there (`message.delivered` with its id).
#[derive(Clone)]
pub struct Msg {
    pub id: String,
    pub text: String,
}

/// Per-attempt control state. In auto mode the orchestrator finishes the attempt when the agent
/// returns a result; in human mode it keeps the session open and waits for the person instead.
struct Control {
    rx: mpsc::UnboundedReceiver<Steer>,
    steering: bool,
    human: bool,
    interrupted: bool,
    finish: bool,
    queued: Vec<Msg>,
}

impl Control {
    fn new(rx: mpsc::UnboundedReceiver<Steer>, steering: bool) -> Control {
        Control { rx, steering, human: false, interrupted: false, finish: false, queued: vec![] }
    }
}

/// How long an agent's text may sit unshown after its last chunk.
const MESSAGE_QUIET: Duration = Duration::from_secs(1);

const RELEASE_PROMPT: &str =
    "人工介入结束。请结合上面的对话，自主完成原来的任务。完成后按最初的要求，以 ```json 结构化结果结束。";

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
    evidence: Vec<String>,
    risks: Vec<String>,
    /// The agent's own check of each acceptance criterion, in order; `evidence::Check` carries the
    /// method behind each one.
    checks: Vec<Check>,
    question: Option<Value>,
}

struct Task {
    def: TaskDef,
    agent: String,
    status: TaskStatus,
    attempts: i64,
    result: Option<TaskResult>,
    /// The failed dependency a blocked task waits on.
    blocked_by: Option<String>,
    succeeded_seq: Option<i64>,
    artifacts: Vec<Value>,
    active_attempt: Option<String>,
    /// Attempts made before the current retry budget started (a user retry).
    retry_base: i64,
    /// The lead's corrections to this task's conclusion, handed on with it (`result.amended`).
    amendments: Vec<String>,
    /// The upstream task whose cancellation dropped this one (`task.skipped_by_decision` with a
    /// `dependency`); retrying that task brings this one back.
    skipped_because: Option<String>,
}

impl Task {
    fn new(def: TaskDef, agent: String) -> Task {
        Task {
            def,
            agent,
            status: TaskStatus::Pending,
            attempts: 0,
            result: None,
            blocked_by: None,
            succeeded_seq: None,
            artifacts: vec![],
            active_attempt: None,
            retry_base: 0,
            amendments: vec![],
            skipped_because: None,
        }
    }

    /// Move to `to`; an illegal step is a scheduler bug, so tests stop on it and a release
    /// build records it instead of silently accepting it.
    fn set_status(&mut self, to: TaskStatus) {
        if !self.status.can_transition_to(to) {
            crate::config::log(format!("janus: task {}: illegal status {:?} -> {:?}", self.def.id, self.status, to));
        }
        debug_assert!(self.status.can_transition_to(to), "task {}: {:?} -> {:?}", self.def.id, self.status, to);
        self.status = to;
    }
}

#[derive(Default)]
struct RunState {
    status: RunStatus,
    paused: bool,
    cancelled: bool,
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
    /// The lead's goal session ended: its last message, or why it failed.
    lead_done: Option<std::result::Result<String, String>>,
    /// Secret in this run's MCP URL, so only its lead reaches the team tools.
    mcp_token: String,
    /// The goal status the lead's agent last reported (Codex keeps working between prompts while
    /// its goal is active).
    goal_status: Option<String>,
    /// The last event the lead has seen on the board, through `wait` or pushed into its session.
    lead_cursor: i64,
    /// The lead is inside `wait`, which hands it the board itself.
    lead_waiting: bool,
    /// The lead's open session id, for recording what Janus pushes into it.
    lead_session: Option<String>,
    /// Sessions whose agent can take a message into the running turn. Only they get board pushes
    /// and conflict notices: queued until the turn ends, a notice would arrive after the fact.
    steerable: HashSet<SessionKey>,
    /// What each task left in the workspace: the files it edited, who edited each file last,
    /// and the conflicts already reported (see `traces`).
    traces: crate::traces::Traces,
}

pub struct Run {
    pub id: String,
    pub goal: String,
    pub cwd: PathBuf,
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
    emitting: StdMutex<()>,
    deadline_ms: AtomicI64,
    st: StdMutex<RunState>,
    wake: Notify,
    /// A driver task (planning or execution) is alive for this run.
    driving: std::sync::atomic::AtomicBool,
    /// Something the lead should hear about reached the board (see `push_board`).
    lead_nudge: Notify,
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
/// work, and the tool call it has been repeating.
#[derive(Default)]
struct Watch {
    attempt: i64,
    last_ms: i64,
    /// This quiet spell was already reported; any sign of work starts a new one.
    idle_noticed: bool,
    /// The person holds the session (takeover or interrupt), so its silence is theirs.
    human: bool,
    /// Questions, approvals and forms of this attempt waiting on a person (`Held` guards).
    waits: u32,
    /// The last finished tool call (a hash of kind, title and input) and how often in a row.
    call: u64,
    repeats: u32,
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

pub struct Config {
    /// Janus home (~/.janus): the database, ACP adapters (adapters/), run data (artifacts/,
    /// logs/, runs/) and everything else that belongs to this machine's user, not to a checkout.
    pub data_dir: PathBuf,
    /// Coding agent configs and credentials, kept in the user's Janus home: ~/.janus/agents.
    pub agents_dir: PathBuf,
    /// How long an attempt may show no sign of work before the watchdog tells the lead.
    pub stall_after: Duration,
    /// How long a lead's session stays quiet with its goal still active before Janus asks it (`goal_settle`).
    pub goal_settle: Duration,
}

pub struct App {
    pub db: Db,
    pub cfg: Config,
    pub tx: broadcast::Sender<Event>,
    pub runs: StdMutex<HashMap<String, Arc<Run>>>,
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

/// A control the person switches in an open session (`set_control`).
struct ControlChange<'a> {
    id: &'a str,
    value: &'a Value,
    /// `mode` for an ACP session mode, else a config option.
    via: &'a str,
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

    // @project-doc docs/interfaces/events.md#event_shape
    fn emit(&self, run: &Run, e: Ev) -> i64 {
        let _order = run.emitting.lock().unwrap_or_else(|p| p.into_inner());
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
        if self.store(event) && nudge {
            run.lead_nudge.notify_one();
        }
        seq
    }

    /// Write an event, then broadcast it. The log is the source of truth: an event that could not
    /// be written is not broadcast either, so the app never shows what a reload would not find.
    /// Its `seq` stays unused.
    fn store(&self, event: Event) -> bool {
        if let Err(err) = self.db.insert_event(&event) {
            crate::config::log(format!("janus: failed to persist event {}#{}: {err:#}", event.run_id, event.seq));
            return false;
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
            if status == RunStatus::Cancelled {
                st.cancelled = true;
            }
            // Pausing only makes sense while work can still start; a finished Run is not paused.
            if !matches!(status, RunStatus::Planning | RunStatus::Running | RunStatus::WaitingInput) {
                st.paused = false;
            }
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
        if st.cancelled {
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
const LEAD_REPLY: &str = "主控回复：";

/// The heading the lead puts its suggested additions to the shared material under (see `goal_brief`).
const BRIEF_HEADING: &str = "建议更新共享资料";

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

/// An `App` over a fresh database in `dir`, for tests.
#[cfg(test)]
pub(crate) fn test_app(dir: &Path) -> Arc<App> {
    let (tx, _) = broadcast::channel(4096);
    Arc::new(App {
        db: Db::open(&dir.join("janus.db")).unwrap(),
        cfg: Config {
            data_dir: dir.join("data"),
            agents_dir: dir.join("agents"),
            stall_after: STALL_AFTER,
            goal_settle: crate::goal_settle::SETTLE,
        },
        tx,
        runs: StdMutex::new(HashMap::new()),
    })
}
