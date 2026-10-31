//! The control plane. The lead agent decides *what* to do (the plan), the orchestrator decides
//! *when* and *by whom*, coding agents decide *how*. Only the orchestrator changes task state,
//! and every change is written to SQLite as an event before anything else happens.

use crate::acp::{AcpConn, Incoming};
use crate::db::{Db, Event};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};

/// Attempts per delegated task before it fails and the lead decides what to do next.
const MAX_ATTEMPTS: i64 = 2;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Spawning the adapter and opening the session; separate from the turn clock so a hung start fails fast.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long a steering request may hold up the session loop before the message is queued instead.
const STEER_TIMEOUT: Duration = Duration::from_secs(15);
const RUN_BUDGET_MS: i64 = 6 * 60 * 60 * 1000;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
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
    #[serde(default)]
    pub limits: Vec<String>,
    #[serde(default)]
    pub ws: String,
    #[serde(default)]
    pub writable: bool,
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
    #[serde(default)]
    pub mcp: Vec<Value>,
    #[serde(default)]
    pub role_id: String,
    /// MCP environment values by server name. Never serialized, so they stay out of the run record.
    #[serde(skip)]
    pub mcp_env: Value,
}

#[derive(Deserialize)]
pub struct NewRun {
    pub goal: String,
    pub workspace: String,
}

/// A task on the board, as the lead orchestrated it (see `validate_orchestration`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDef {
    pub id: String,
    /// "implement" when it changes files, "explore" otherwise.
    #[serde(default)]
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub desc: String,
    /// Tasks that must succeed before this one starts.
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub assignee: String,
    #[serde(default = "default_max_attempts", rename = "maxAttempts")]
    pub max_attempts: i64,
    #[serde(default)]
    pub verification: VerificationPolicy,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerificationPolicy {
    /// Commands Janus runs in the workspace once the task reports its result.
    #[serde(default)]
    pub commands: Vec<String>,
}

fn default_max_attempts() -> i64 {
    MAX_ATTEMPTS
}

fn terminal_task_status(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "cancelled" | "skipped_by_decision"
    )
}

impl TaskDef {
    fn dep_failed(&self, statuses: &HashMap<String, &'static str>) -> Option<String> {
        self.deps
            .iter()
            .find(|id| {
                matches!(
                    statuses.get(*id),
                    Some(&"failed") | Some(&"cancelled") | Some(&"blocked") | Some(&"skipped_by_decision")
                )
            })
            .cloned()
    }
}

/// What a person can do inside a running attempt's ACP session.
pub enum Steer {
    Message(Msg),
    Interrupt,
    TakeOver,
    Release,
    Finish,
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

const RELEASE_PROMPT: &str = "人工介入结束。请结合上面的对话，自主完成原来的任务。完成后按最初的要求，以 ```json 结构化结果结束。";

#[derive(Clone, Debug, Default)]
struct TaskResult {
    summary: String,
    evidence: Vec<String>,
    risks: Vec<String>,
    question: Option<Value>,
}

struct Task {
    def: TaskDef,
    agent: String,
    status: &'static str,
    attempts: i64,
    result: Option<TaskResult>,
    /// The failed dependency a blocked task waits on.
    blocked_by: Option<String>,
    succeeded_seq: Option<i64>,
    artifacts: Vec<Value>,
    active_attempt: Option<String>,
    /// Attempts made before the current retry budget started (a user retry).
    retry_base: i64,
}

impl Task {
    fn new(def: TaskDef, agent: String) -> Task {
        Task {
            def,
            agent,
            status: "pending",
            attempts: 0,
            result: None,
            blocked_by: None,
            succeeded_seq: None,
            artifacts: vec![],
            active_attempt: None,
            retry_base: 0,
        }
    }
}

#[derive(Default)]
struct RunState {
    status: String,
    paused: bool,
    cancelled: bool,
    tasks: Vec<Task>,
    waiters: HashMap<String, oneshot::Sender<Value>>,
    conns: HashMap<String, Arc<AcpConn>>,
    /// Control channels of open ACP sessions, by task id ("lead" while the lead is planning).
    live: HashMap<String, mpsc::UnboundedSender<Steer>>,
    /// Which open session each agent is in, so a person can talk to an agent without knowing its task.
    live_agent: HashMap<String, String>,
    /// Messages for an agent that had no open session; delivered with its next session.
    notes: HashMap<String, Vec<Msg>>,
    /// Sessions to reload when an interrupted task starts again, by task id.
    resume_session: HashMap<String, String>,
    /// Working in place: the working-tree diff last saved as an artifact, so a task that changed
    /// nothing does not repeat what an earlier one changed.
    last_diff: String,
    /// The person accepted a partial result; the driver delivers what finished and stops.
    accept_risk: bool,
    /// The lead's goal session ended: its last message, or why it failed.
    lead_done: Option<std::result::Result<String, String>>,
    /// Secret in this run's MCP URL, so only its lead reaches the team tools.
    mcp_token: String,
    /// The goal status the lead's agent last reported (Codex keeps working between prompts while
    /// its goal is active).
    goal_status: Option<String>,
    /// The last event the lead has seen on the board through `wait`.
    lead_cursor: i64,
    /// Each pause point is announced once, until the run moves on.
    budget_notified: bool,
}

pub struct Run {
    pub id: String,
    pub goal: String,
    pub cwd: PathBuf,
    pub team: Vec<Member>,
    git: bool,
    base_ref: String,
    seq: AtomicI64,
    deadline_ms: AtomicI64,
    st: StdMutex<RunState>,
    wake: Notify,
    /// A driver task (planning or execution) is alive for this run.
    driving: std::sync::atomic::AtomicBool,
}

impl Run {
    fn member(&self, id: &str) -> Option<&Member> {
        self.team.iter().find(|m| m.id == id)
    }
    fn task_mut<'a>(st: &'a mut RunState, id: &str) -> Option<&'a mut Task> {
        st.tasks.iter_mut().find(|t| t.def.id == id)
    }
}

pub struct Config {
    /// Janus home (~/.janus): the database, ACP adapters (adapters/), run data (artifacts/,
    /// logs/, runs/) and everything else that belongs to this machine's user, not to a checkout.
    pub data_dir: PathBuf,
    /// Coding agent configs and credentials, kept in the user's home like ~/.lody: ~/.janus/agents.
    pub agents_dir: PathBuf,
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
    Ev {
        kind: kind.into(),
        actor: actor.into(),
        text: text.into(),
        to: None,
        task: None,
        attempt: None,
        data: None,
    }
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

impl App {
    pub fn run(&self, id: &str) -> Option<Arc<Run>> {
        self.runs.lock().unwrap().get(id).cloned()
    }

    // @project-doc docs/interfaces/events.md#event_shape
    fn emit(&self, run: &Run, e: Ev) -> i64 {
        let seq = run.seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.store(Event {
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
        });
        seq
    }

    fn store(&self, event: Event) {
        if let Err(err) = self.db.insert_event(&event) {
            crate::config::log(format!("janus: failed to persist event {}#{}: {err:#}", event.run_id, event.seq));
        }
        let _ = self.tx.send(event);
    }

    fn set_status(&self, run: &Run, status: &str) {
        run.st.lock().unwrap().status = status.into();
        let _ = self.db.set_run_status(&run.id, status);
    }

    async fn wait_for(&self, run: &Run, key: &str) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        {
            let mut st = run.st.lock().unwrap();
            // Cancelling clears the waiters; one registered afterwards would never be resolved.
            if st.cancelled {
                return Err(Cancelled.into());
            }
            st.waiters.insert(key.into(), tx);
        }
        rx.await.map_err(|_| anyhow::Error::new(Cancelled))
    }

    /// Checked before a decision is logged, so the audit trail never records an answer nobody was waiting for.
    fn expect_waiter(&self, run: &Run, key: &str) -> Result<()> {
        if run.st.lock().unwrap().waiters.contains_key(key) {
            Ok(())
        } else {
            bail!("没有等待中的决定：{key}")
        }
    }

    fn resolve(&self, run: &Run, key: &str, value: Value) -> Result<()> {
        let waiter = run
            .st
            .lock()
            .unwrap()
            .waiters
            .remove(key)
            .ok_or_else(|| anyhow!("没有等待中的决定：{key}"))?;
        let _ = waiter.send(value);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Run lifecycle
    // ------------------------------------------------------------------

    // @project-doc docs/domains/run_lifecycle.md#create_run
    // @project-doc docs/domains/run_lifecycle.md#workspace_model
    pub async fn create_run(self: &Arc<Self>, req: NewRun) -> Result<String> {
        if req.goal.trim().is_empty() {
            bail!("目标不能为空");
        }
        let ws = self.workspace(&req.workspace)?;
        let (cwd, team) = self.team_for(&ws)?;
        if !cwd.is_dir() {
            bail!("工作目录不存在：{}", cwd.display());
        }
        for m in &team {
            self.spec_for(&m.runtime)?;
        }
        if let Some(lead) = team.iter().find(|m| m.lead).filter(|m| !lead_capable(&m.kind)) {
            bail!("主控「{}」用的是 {}，只有 Claude Code 或 Codex 能做主控（需要 /goal），请在工作区里换一个主控", lead.name, lead.runtime_name);
        }
        let is_git = git(&cwd, &["rev-parse", "--is-inside-work-tree"])
            .await
            .is_ok();
        let mut team = team;
        let base_ref = if is_git {
            git(&cwd, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };
        let id = format!("r{}", to_base36(now_ms() as u64));
        self.snapshot_skills(&id, &mut team)?;
        let config = json!({ "team": team, "git": is_git, "baseRef": base_ref, "workspace": ws });
        self.db.insert_run(
            &id,
            req.goal.trim(),
            &cwd.to_string_lossy(),
            &config,
            now_ms(),
        )?;
        let owner = format!("janus-{}", std::process::id());
        if self
            .db
            .acquire_lease(&id, "run", &owner, now_ms(), 10 * 60 * 1000)?
            .is_none()
        {
            bail!("Run 已被另一个 Janus 实例接管");
        }
        let run = Arc::new(Run {
            id: id.clone(),
            goal: req.goal.trim().into(),
            cwd,
            team,
            git: is_git && !base_ref.is_empty(),
            base_ref,
            seq: AtomicI64::new(0),
            deadline_ms: AtomicI64::new(now_ms() + RUN_BUDGET_MS),
            st: StdMutex::new(RunState {
                status: "planning".into(),
                ..Default::default()
            }),
            wake: Notify::new(),
            driving: Default::default(),
        });
        self.runs.lock().unwrap().insert(id.clone(), run.clone());
        self.launch(run, Phase::Start);
        Ok(id)
    }

    fn launch(self: &Arc<Self>, run: Arc<Run>, phase: Phase) {
        run.driving.store(true, Ordering::SeqCst);
        let app = self.clone();
        tokio::spawn(async move {
            let result = match phase {
                Phase::Start => app.drive(&run).await,
                Phase::Execute => app.execute(&run).await,
                Phase::Goal(resumed) => app.goal(&run, resumed).await,
            };
            run.driving.store(false, Ordering::SeqCst);
            if let Err(err) = result {
                if err.downcast_ref::<Cancelled>().is_none() && !run.st.lock().unwrap().cancelled {
                    app.emit(
                        &run,
                        ev("run.failed", "janus", format!("Run 失败：{err:#}"))
                            .data(json!({ "error": format!("{err:#}") })),
                    );
                    app.set_status(&run, "failed");
                    // A failed run is rebuilt from its event log when the person continues it.
                    app.runs.lock().unwrap().remove(&run.id);
                }
            }
        });
    }

    /// Restart execution when a decision arrives after the driver has stopped, e.g. a retry
    /// after delivery. Only runs whose plan is frozen have anything to execute.
    fn ensure_driver(self: &Arc<Self>, run: &Arc<Run>) {
        let executable = {
            let st = run.st.lock().unwrap();
            !st.cancelled && !st.tasks.is_empty()
        };
        if executable
            && run
                .driving
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            self.launch(run.clone(), Phase::Execute);
        }
    }

    /// Copy each member's skills into this run's own directory. Agents get the snapshot path,
    /// so later edits to the library do not change a run, and no agent reads the library itself.
    fn snapshot_skills(&self, run_id: &str, team: &mut [Member]) -> Result<()> {
        let root = self.cfg.data_dir.join("runs").join(run_id).join("skills");
        for m in team.iter_mut() {
            for s in m.skills.iter_mut() {
                let (Some(id), Some(src)) = (
                    s["id"].as_str().map(str::to_owned),
                    s["path"].as_str().map(PathBuf::from),
                ) else {
                    continue;
                };
                let dest = root.join(&id);
                if !dest.exists() {
                    copy_dir(&src, &dest)?;
                }
                s["path"] = json!(dest);
            }
        }
        Ok(())
    }

    async fn drive(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        let workspace_note = if run.git {
            format!("Git 仓库，基线 {}", short_sha(&run.base_ref))
        } else {
            "非 Git 目录：任务直接在这个目录中执行，没有 diff".to_string()
        };
        self.emit(run, ev("run.created", "user", format!("创建目标：{}", run.goal)).data(json!({
            "goal": run.goal, "cwd": run.cwd, "git": run.git, "baseRef": run.base_ref, "workspace": workspace_note, "kind": "live",
            // Whether tasks change the directory itself: the page describes the run from this fact.
            "inPlace": true,
            "mode": "goal"
        })));
        self.goal(run, false).await
    }

    /// The scheduling loop. It recomputes the run's status from the task graph on every pass and
    /// only returns once the run is delivered (fully, or partially after the person accepted the
    /// risk) or cancelled, so every later decision still has a driver to act on it.
    // @project-doc docs/domains/run_lifecycle.md#scheduling
    // @project-doc docs/domains/run_lifecycle.md#delivery_conditions
    async fn execute(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        enum Outcome {
            Deliver { partial: bool },
            /// Goal mode: the lead's session broke off; the person continues the run to restart it.
            LeadFailed(String),
            Budget,
            Paused,
            Running,
        }
        loop {
            if run.st.lock().unwrap().cancelled {
                return Err(Cancelled.into());
            }
            let over_budget = now_ms() > run.deadline_ms.load(Ordering::SeqCst);
            let (starts, blocked, outcome) = {
                let mut st = run.st.lock().unwrap();
                let statuses: HashMap<String, &'static str> = st
                    .tasks
                    .iter()
                    .map(|t| (t.def.id.clone(), t.status))
                    .collect();
                let succeeded: HashSet<String> = statuses
                    .iter()
                    .filter_map(|(id, status)| (*status == "succeeded").then_some(id.clone()))
                    .collect();
                let mut blocked = vec![];
                for t in st.tasks.iter_mut().filter(|t| t.status == "pending") {
                    if let Some(dep) = t.def.dep_failed(&statuses) {
                        t.status = "blocked";
                        t.blocked_by = Some(dep.clone());
                        blocked.push((t.def.id.clone(), dep));
                    }
                }
                // A blocked task starts again once its failed dependency was retried and succeeded.
                for t in st.tasks.iter_mut() {
                    if t.status == "blocked" && t.blocked_by.as_ref().is_some_and(|b| succeeded.contains(b)) {
                        t.status = "pending";
                        t.blocked_by = None;
                    }
                }
                let mut starts = vec![];
                if !st.paused && !over_budget && !st.accept_risk {
                    let mut busy: HashSet<String> = st
                        .tasks
                        .iter()
                        .filter(|t| t.status == "running")
                        .map(|t| t.agent.clone())
                        .collect();
                    for t in st.tasks.iter_mut().filter(|t| t.status == "pending") {
                        if t.def.deps.iter().all(|d| succeeded.contains(d)) && !busy.contains(&t.agent) {
                            busy.insert(t.agent.clone());
                            t.status = "running";
                            t.attempts += 1;
                            let attempt_id =
                                format!("{}-{}-{}-{}", run.id, t.def.id, t.attempts, now_ms());
                            t.active_attempt = Some(attempt_id.clone());
                            starts.push((
                                t.def.id.clone(),
                                t.attempts,
                                t.agent.clone(),
                                attempt_id,
                            ));
                        }
                    }
                }
                let active = st.tasks.iter().any(|t| t.status == "running");
                let stalled = !active && starts.is_empty();
                let all_terminal = st.tasks.iter().all(|t| terminal_task_status(t.status));
                // The lead decides what a failure means (it reads it on the board), so nothing
                // waits for the person; the run ends once the lead is done and no task is running
                // or can still start.
                let outcome = if stalled && st.accept_risk {
                    Outcome::Deliver { partial: true }
                } else if let (true, Some(Err(err))) = (stalled, &st.lead_done) {
                    Outcome::LeadFailed(err.clone())
                } else if stalled && st.lead_done.is_some() {
                    Outcome::Deliver { partial: false }
                } else if over_budget && !all_terminal {
                    Outcome::Budget
                } else if stalled && st.paused {
                    Outcome::Paused
                } else {
                    Outcome::Running
                };
                (starts, blocked, outcome)
            };
            for (id, dep) in blocked {
                self.emit(
                    run,
                    ev(
                        "task.blocked",
                        "janus",
                        format!("{id} 被失败的硬依赖 {dep} 阻塞"),
                    )
                    .task(&id)
                    .data(json!({ "dependency": dep, "kind": "hard" })),
                );
            }
            for (id, n, agent, attempt_id) in starts {
                if n == 1 {
                    self.emit(
                        run,
                        ev(
                            "task.ready",
                            "janus",
                            format!("{id} 的依赖已满足，进入 ready"),
                        )
                        .task(&id)
                        .data(json!({ "tasks": [id] })),
                    );
                    self.emit(
                        run,
                        ev(
                            "task.assigned",
                            "janus",
                            format!(
                                "{id} 分配给 {}",
                                run.member(&agent)
                                    .map(|m| m.name.as_str())
                                    .unwrap_or(&agent)
                            ),
                        )
                        .task(&id)
                        .to(&agent)
                        .data(json!({ "agent": agent })),
                    );
                }
                self.emit(
                    run,
                    ev(
                        "attempt.launch_requested",
                        "janus",
                        format!("准备启动 {id} 第 {n} 次尝试"),
                    )
                    .task(&id)
                    .attempt(n)
                    .data(json!({ "attemptId": attempt_id })),
                );
                let (app, run, id) = (self.clone(), run.clone(), id.clone());
                tokio::spawn(async move { app.run_attempt(&run, &id, n, &attempt_id).await });
            }
            match outcome {
                Outcome::Deliver { partial } => {
                    self.deliver(run, partial);
                    return Ok(());
                }
                Outcome::LeadFailed(err) => {
                    // Like any failed run: continuing it rebuilds the board and restarts the lead.
                    bail!("主控的会话异常结束：{err}");
                }
                Outcome::Budget => {
                    let announce =
                        !std::mem::replace(&mut run.st.lock().unwrap().budget_notified, true);
                    if announce {
                        self.set_status(run, "waiting_input");
                        self.emit(
                            run,
                            ev(
                                "run.budget_exhausted",
                                "janus",
                                "Run 达到墙钟预算，等待你追加预算、接受部分结果或取消",
                            )
                            .data(json!({ "budgetMs": RUN_BUDGET_MS })),
                        );
                    }
                }
                Outcome::Running | Outcome::Paused => {
                    let status = run.st.lock().unwrap().status.clone();
                    if status != "running" {
                        self.set_status(run, "running");
                    }
                }
            }
            let _ = tokio::time::timeout(Duration::from_secs(2), run.wake.notified()).await;
        }
    }

    // ------------------------------------------------------------------
    // Goal mode
    // ------------------------------------------------------------------

    /// Goal mode: the lead pursues the goal in its own session (Claude Code and Codex `/goal`) and
    /// does it alone or brings in the team. Bringing them in means submitting an orchestration
    /// (tasks, assignees, dependencies) through Janus's MCP tools; Janus validates it, runs it
    /// with the same scheduler as a plan, and the shared board carries everything in between.
    // @project-doc docs/domains/goal_mode.md#lead_session
    async fn goal(self: &Arc<Self>, run: &Arc<Run>, resumed: bool) -> Result<()> {
        {
            let mut st = run.st.lock().unwrap();
            st.lead_done = None;
            // Changes already in the directory are not this run's: a task's diff starts from here.
            if run.git && st.last_diff.is_empty() {
                st.last_diff = working_diff(&run.cwd);
            }
            // Continuing: the restarted lead is told the board so far; `wait` brings only what is new.
            if resumed {
                st.lead_cursor = run.seq.load(Ordering::SeqCst);
            }
            if st.mcp_token.is_empty() {
                st.mcp_token = random_token();
            }
        }
        self.set_status(run, "running");
        let (app, r) = (self.clone(), run.clone());
        tokio::spawn(async move {
            let outcome = app.lead_goal(&r, resumed).await;
            if r.st.lock().unwrap().cancelled {
                return;
            }
            let done = match outcome {
                // The lead's last message is its report; earlier ones narrated the work.
                Ok(text) => Ok(app
                    .db
                    .events(&r.id, 0)
                    .unwrap_or_default()
                    .iter()
                    .rev()
                    .find(|e| e.kind == "agent.message" && e.actor == "lead" && e.task.is_none())
                    .and_then(|e| e.data.as_ref().and_then(|d| d["text"].as_str().map(str::to_owned)))
                    .unwrap_or(text)),
                Err(err) if err.downcast_ref::<Cancelled>().is_some() => return,
                Err(err) => {
                    app.emit(&r, ev("lead.failed", "janus", format!("主控的会话异常结束：{}", short(&format!("{err:#}"), 120))).data(json!({ "error": format!("{err:#}") })));
                    Err(format!("{err:#}"))
                }
            };
            r.st.lock().unwrap().lead_done = Some(done);
            app.escalate_open_questions(&r);
            r.wake.notify_one();
        });
        self.execute(run).await
    }

    /// Goal mode: once the lead's session has ended nobody reads the board, so a collaborator's
    /// question still waiting for the lead goes to the person instead.
    // @project-doc docs/domains/goal_mode.md#question_routing
    fn escalate_open_questions(&self, run: &Run) {
        let events = self.db.events(&run.id, 0).unwrap_or_default();
        let key_of = |e: &Event| e.data.as_ref().and_then(|d| d["key"].as_str().map(str::to_owned));
        let settled: HashSet<String> = events
            .iter()
            .filter(|e| matches!(e.kind.as_str(), "question.answered" | "question.escalated"))
            .filter_map(key_of)
            .collect();
        let running: HashSet<String> = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .filter(|t| t.status == "running")
            .map(|t| t.def.id.clone())
            .collect();
        for e in events.iter().filter(|e| e.kind == "question.asked") {
            let Some(key) = key_of(e).filter(|k| !settled.contains(k)) else { continue };
            // A question from an attempt that has since ended is no longer waiting for anyone.
            let live = e.task.as_ref().is_some_and(|t| {
                running.contains(t)
                    && !events.iter().any(|x| x.seq > e.seq && x.kind == "attempt.started" && x.task.as_ref() == Some(t))
            });
            if !live {
                continue;
            }
            let esc = ev("question.escalated", "janus", "主控已经结束，协作者的提问转交给你").to("user").data(json!({
                "key": key, "reason": "主控的会话已经结束，没有人在看板上回答这个问题。"
            }));
            self.emit(run, esc.task(e.task.as_deref().unwrap_or_default()));
        }
    }

    /// The lead's goal session: `/goal <goal>` in the workspace directory, with the team and the
    /// board tools in reach. Open to the person like any session: steering, takeover, messages.
    async fn lead_goal(self: &Arc<Self>, run: &Arc<Run>, resumed: bool) -> Result<String> {
        let lead = run.team.iter().find(|m| m.lead).context("no lead")?.clone();
        if !lead_capable(&lead.kind) {
            bail!("主控只能是 Claude Code 或 Codex：只有它们支持 /goal");
        }
        let spec = self.spec_for(&lead.runtime)?;
        let workspace = run.cwd.clone();
        if lead.kind == "claude" {
            // Claude Code only runs /goal in a workspace it trusts; its isolated config has no record yet.
            self.trust_claude_workspace(&lead.runtime, &workspace)?;
        }
        let log = self.cfg.data_dir.join("logs").join(&run.id).join("lead-goal.log");
        let result = async {
            let (conn, mut rx) = tokio::time::timeout(STARTUP_TIMEOUT, AcpConn::spawn(&spec, &workspace, &log))
                .await
                .map_err(|_| anyhow!("主控启动超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
            let (tx, steer_rx) = mpsc::unbounded_channel();
            {
                let mut st = run.st.lock().unwrap();
                st.conns.insert("lead".into(), conn.clone());
                if st.cancelled {
                    return Err(Cancelled.into());
                }
                st.live.insert("lead".into(), tx);
                st.live_agent.insert("lead".into(), "lead".into());
            }
            let mut ctl = Control {
                rx: steer_rx,
                steering: !conn.capabilities["_meta"]["lody"]["steering"].is_null(),
                human: false,
                interrupted: false,
                finish: false,
                queued: vec![],
            };
            let brief = format!("{}{}", self.goal_brief(run, resumed), self.take_notes(run, "lead"));
            let claude = lead.kind == "claude";
            let (sid, applied) = tokio::time::timeout(STARTUP_TIMEOUT, async {
                let session = new_session(&conn, &workspace, &lead, &[self.mcp_server(run, "lead")], claude.then_some(brief.as_str())).await?;
                let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
                let applied = configure_session(&conn, &session, &lead).await?;
                Ok::<_, anyhow::Error>((sid, applied))
            })
            .await
            .map_err(|_| anyhow!("主控打开会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
            self.emit(run, ev("agent.status", "lead", if resumed { "从中断处继续推进目标" } else { "开始推进目标" }).data(json!({ "phase": "thinking", "session": sid, "config": applied, "mode": "goal" })));
            let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
            if !claude {
                // Codex has no slot for extra instructions: the brief goes in first, as context.
                let ack = format!("{brief}\n\n以上是背景信息。现在只回复“已了解”，不要开始工作，目标会在下一条消息里给出。");
                self.record_session_prompt(run, &ctx, &sid, "context", &ack);
                self.stream_prompt(run, &conn, &mut rx, &sid, &ack, &Ctx { goal: false, ..ctx }, Some(&mut ctl), Some(STARTUP_TIMEOUT)).await?;
            } else {
                // Claude receives the brief through session/new's systemPrompt.append. Record it
                // explicitly so the session view shows the same context the model receives.
                self.record_session_prompt(run, &ctx, &sid, "system", &brief);
            }
            let mut prompt = format!("/goal {}", goal_condition(&run.goal));
            loop {
                self.record_session_prompt(
                    run,
                    &ctx,
                    &sid,
                    if prompt.starts_with("/goal ") { "goal" } else { "followup" },
                    &prompt,
                );
                let text = self.stream_prompt(run, &conn, &mut rx, &sid, &prompt, &ctx, Some(&mut ctl), None).await?;
                if let Some(reason) = goal_refused(&text) {
                    bail!("{} 拒绝了 /goal：{reason}", lead.runtime_name);
                }
                if !ctl.queued.is_empty() {
                    let msgs: Vec<Msg> = ctl.queued.drain(..).collect();
                    let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
                    self.emit(run, ev("message.delivered", "janus", format!("这一轮结束，把你的 {} 条消息发给主控", msgs.len())).to("lead").data(json!({ "ids": ids })));
                    let texts: Vec<&str> = msgs.iter().map(|m| m.text.as_str()).collect();
                    prompt = format!("{}\n\n请结合这些信息继续推进目标：{}", texts.join("\n\n"), run.goal);
                    continue;
                }
                if ctl.human && !ctl.finish {
                    self.emit(run, ev("agent.status", "lead", "这一轮已结束，等待你的消息（人工接管中）").data(json!({ "phase": "waiting_for_input", "turn": "idle" })));
                    loop {
                        match ctl.rx.recv().await {
                            None => return Err(Cancelled.into()),
                            Some(Steer::Message(m)) => {
                                self.route_message(run, "lead", "", 0, &m.id, "prompt");
                                prompt = m.text;
                            }
                            Some(Steer::Release) => {
                                ctl.human = false;
                                self.emit(run, ev("control.released", "user", "交还托管：主控继续推进目标").to("lead"));
                                prompt = format!("人工介入结束。请结合上面的对话，继续推进目标直到完成：{}", run.goal);
                            }
                            Some(Steer::Finish) => ctl.finish = true,
                            Some(Steer::TakeOver | Steer::Interrupt) => continue,
                        }
                        break;
                    }
                    if !ctl.finish {
                        continue;
                    }
                }
                return Ok(text);
            }
        }
        .await;
        let (conn, stale) = {
            let mut st = run.st.lock().unwrap();
            st.live.remove("lead");
            st.live_agent.remove("lead");
            let stale: Vec<String> = st.waiters.keys().filter(|k| k.starts_with("req-lead-0-")).cloned().collect();
            (st.conns.remove("lead"), stale)
        };
        for key in stale {
            self.withdraw(run, &key, "", 0, "主控的会话已结束，撤回这个请求");
        }
        if let Some(conn) = conn {
            conn.kill().await;
        }
        result
    }

    fn record_session_prompt(&self, run: &Run, ctx: &Ctx<'_>, sid: &str, channel: &str, prompt: &str) {
        let label = match channel {
            "system" => "向 Agent 注入主控上下文",
            "context" => "向主控发送团队上下文",
            "goal" => "向主控发送目标提示",
            _ => "向主控发送继续提示",
        };
        let mut e = ev("session.prompt", "janus", label)
            .to(ctx.agent)
            .data(json!({ "channel": channel, "session": sid, "prompt": prompt }));
        if let Some(task) = ctx.task {
            e = e.task(task);
        }
        if let Some(attempt) = ctx.attempt {
            e = e.attempt(attempt);
        }
        self.emit(run, e);
    }

    /// What the lead knows about its team and how to work with it. Everything it learns about
    /// the collaborators comes from here (and the `team` tool, which repeats it).
    // @project-doc docs/domains/goal_mode.md#lead_brief
    fn goal_brief(&self, run: &Run, resumed: bool) -> String {
        let mut p = format!(
            "你是 Janus 团队的主控，负责完成用户交给你的目标。你可以自己从头做到尾；也可以在需要时把一部分工作交给下面的协作者。是否需要协作、交给谁、怎么拆，都由你判断，不要为了协作而协作。

工作目录：{cwd}（你和协作者都直接在这里工作，改动立即彼此可见）

协作者：
{team}

和协作者协作，都通过 janus MCP 工具和共享看板：
- janus.orchestrate：提交一份编排（一组任务：id、标题、给执行者的完整说明、执行者、依赖、是否修改文件、验收标准）。Janus 先校验（执行者存在、依赖存在且无环、验证命令合规），不通过会返回具体错误，改好再提交；通过后任务写入看板并按依赖执行。可以多次提交，后提交的任务可以依赖之前的任务。
- janus.wait：从看板取回发给你的新消息并等待新进展：任务开始、完成（摘要、证据、风险、产物）、失败，协作者写回的进展和阻塞，以及协作者的提问。每次最多等 45 秒，没有新消息就再调用。
- janus.reply：回答协作者的提问，或给正在执行的协作者补充指示。
- janus.team：查看协作者和看板上所有任务的当前状态。
协作要点：
- 协作者看不到你的对话，任务说明要写清背景、具体要做什么和验收标准。
- 每个协作者同一时间只做一个任务，派给同一人的任务会排队；会改同一批文件的任务要用依赖串行。
- 协作者提问时会停下来等你，用 janus.reply 回答；失败的任务需要你决定重新编排、换人，还是自己做。
- 协作者完成后，你要自己核对结果（读文件、运行测试），确认目标真正达成后再结束。
- 不要 git commit、push 或部署。",
            cwd = run.cwd.display(),
            team = team_lines(run),
        );
        if resumed {
            let st = run.st.lock().unwrap();
            let tasks: Vec<String> = st
                .tasks
                .iter()
                .map(|t| format!("- {}（{}）：{} · {}{}", t.def.id, t.agent, t.def.title, t.status, t.result.as_ref().map(|r| format!(" · {}", short(&r.summary, 80))).unwrap_or_default()))
                .collect();
            p.push_str(&format!(
                "\n\n这是从中断处继续：上一次会话已经结束，工作目录里保留了之前的改动。{}",
                if tasks.is_empty() { "之前没有交给协作者的任务。".to_string() } else { format!("看板上之前的任务：\n{}\n未完成的任务会重新开始。", tasks.join("\n")) }
            ));
        }
        let lead = run.team.iter().find(|m| m.lead);
        if let Some(lead) = lead {
            p.push_str(&role_extras(lead, &run.cwd));
        }
        p
    }

    /// The Janus MCP server one session reaches: the lead's board tools, or a collaborator's.
    fn mcp_server(&self, run: &Run, who: &str) -> Value {
        let token = run.st.lock().unwrap().mcp_token.clone();
        json!({ "type": "http", "name": "janus", "url": format!("http://{}/mcp/{}/{}/{}", janus_addr(), run.id, token, who), "headers": [] })
    }

    /// One MCP request (streamable HTTP, JSON responses) from a session of this run. `who` is
    /// "lead" or the task id of the collaborator's session. Notifications get no response.
    // @project-doc docs/interfaces/board_mcp.md#transport
    pub async fn mcp(self: &Arc<Self>, run_id: &str, token: &str, who: &str, req: Value) -> Result<Option<Value>> {
        let run = self.run(run_id).context("这个 Run 不在运行")?;
        if run.st.lock().unwrap().mcp_token != token || token.is_empty() {
            bail!("token 不对");
        }
        let Some(id) = req.get("id").cloned() else {
            return Ok(None);
        };
        let lead = who == "lead";
        let tool_call = req["method"].as_str() == Some("tools/call");
        let tool_name = req["params"]["name"].as_str().unwrap_or("工具").to_string();
        let tool_input = req["params"]["arguments"].clone();
        let call_id = id.to_string();
        if tool_call {
            let mut e = ev("tool.started", who, format!("调用 janus.{tool_name}，等待返回"))
                .data(json!({
                    "callId": call_id,
                    "tool": format!("janus.{tool_name}"),
                    "input": summarize_input(&tool_input, &tool_name),
                    "status": "pending"
                }));
            if !lead {
                e = e.task(who);
            }
            self.emit(&run, e);
        }
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "janus", "version": env!("CARGO_PKG_VERSION") },
                "instructions": if lead { "Janus 团队的共享看板。用 orchestrate 提交编排，wait 取回进展和提问，reply 回答协作者，team 查看团队和任务状态。" } else { "Janus 团队的共享看板。执行中有进展、阻塞或需要主控注意的事，用 board_post 写回看板。" },
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": if lead { lead_tools() } else { member_tools() } }),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = &req["params"]["arguments"];
                let out = match (lead, name) {
                    (true, "team") => Ok(self.board_team(&run)),
                    (true, "orchestrate") => self.board_orchestrate(&run, args),
                    (true, "wait") => Ok(self.board_wait(&run, args).await),
                    (true, "reply") => self.board_reply(&run, args),
                    (false, "board_post") => self.board_post(&run, who, args),
                    _ => Err(format!("没有这个工具：{name}")),
                };
                match out {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
                }
            }
            m => return Ok(Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unknown method {m}") } }))),
        };
        if tool_call {
            let failed = result["isError"] == true;
            let output = result["content"][0]["text"].as_str().unwrap_or("");
            let mut e = ev(
                "tool.completed",
                who,
                if failed { format!("janus.{tool_name} 失败") } else { format!("janus.{tool_name} 返回结果") },
            )
            .data(json!({
                "callId": call_id,
                "tool": format!("janus.{tool_name}"),
                "input": summarize_input(&tool_input, &tool_name),
                "output": short(output, 8000),
                "status": if failed { "failed" } else { "completed" }
            }));
            if !lead {
                e = e.task(who);
            }
            self.emit(&run, e);
        }
        Ok(Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })))
    }

    fn board_team(&self, run: &Run) -> String {
        let st = run.st.lock().unwrap();
        let tasks: Vec<String> = st
            .tasks
            .iter()
            .map(|t| format!("- {} · {} · {} · {}", t.def.id, t.agent, t.status, t.def.title))
            .collect();
        format!(
            "协作者：\n{}\n\n看板上的任务：\n{}",
            team_lines(run),
            if tasks.is_empty() { "（还没有）".into() } else { tasks.join("\n") }
        )
    }

    /// Validate an orchestration and, if it holds, put its tasks on the board for the scheduler.
    fn board_orchestrate(&self, run: &Run, args: &Value) -> std::result::Result<String, String> {
        let defs = {
            let st = run.st.lock().unwrap();
            let existing: Vec<String> = st.tasks.iter().map(|t| t.def.id.clone()).collect();
            validate_orchestration(run, &existing, args)
        };
        let defs = match defs {
            Ok(defs) => defs,
            Err(errors) => {
                self.emit(run, ev("orchestration.rejected", "lead", format!("编排没有通过校验：{}", short(&errors.join("；"), 120))).data(json!({ "errors": errors, "submitted": args })));
                return Err(format!("编排没有通过校验，没有任何任务开始。请修正后重新提交：\n- {}", errors.join("\n- ")));
            }
        };
        let tasks: Vec<Value> = defs
            .iter()
            .map(|d| json!({
                "id": d.id, "kind": d.kind, "title": d.title, "desc": d.desc, "deps": d.deps,
                "write": d.write, "accept": d.accept, "maxAttempts": d.max_attempts, "verification": d.verification,
                "agent": d.assignee, "delegated": true,
            }))
            .collect();
        let n = {
            let mut st = run.st.lock().unwrap();
            for d in &defs {
                st.tasks.push(Task::new(d.clone(), d.assignee.clone()));
            }
            st.tasks.len()
        };
        self.emit(run, ev("orchestration.accepted", "lead", format!("编排通过校验：{} 个任务写入看板，开始执行", defs.len())).data(json!({ "tasks": tasks, "total": n })));
        run.wake.notify_one();
        Ok(format!(
            "编排已通过校验并开始执行：{}。用 janus.wait 取回进展和结果。",
            defs.iter().map(|d| format!("{}→{}", d.id, d.assignee)).collect::<Vec<_>>().join("、")
        ))
    }

    /// Board entries for the lead since it last looked, waiting up to the given time for some.
    async fn board_wait(&self, run: &Run, args: &Value) -> String {
        let limit = Duration::from_secs(args["timeout_sec"].as_u64().unwrap_or(40).clamp(1, 45));
        let until = tokio::time::Instant::now() + limit;
        loop {
            let cursor = run.st.lock().unwrap().lead_cursor;
            let events = self.db.events(&run.id, cursor).unwrap_or_default();
            let last = events.last().map(|e| e.seq).unwrap_or(cursor);
            let entries: Vec<String> = events.iter().filter_map(|e| board_entry(run, e)).collect();
            run.st.lock().unwrap().lead_cursor = last;
            if !entries.is_empty() {
                return format!("看板上的新消息：\n{}", entries.join("\n"));
            }
            let open = run.st.lock().unwrap().tasks.iter().any(|t| !terminal_task_status(t.status));
            if !open {
                return "看板上没有新消息，也没有在执行或等待的任务。".into();
            }
            if tokio::time::Instant::now() >= until || run.st.lock().unwrap().cancelled {
                return format!("{} 秒内看板上没有新消息，任务仍在进行。可以再次调用 janus.wait。", limit.as_secs());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// The lead answers a collaborator's question, or adds instructions to its task.
    fn board_reply(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err("text 不能为空".into());
        }
        let (agent, question, live) = {
            let st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(format!("看板上没有任务 {task}"));
            };
            let prefix = format!("q-{task}-");
            let question = st.waiters.keys().find(|k| k.starts_with(&prefix)).cloned();
            (t.agent.clone(), question, st.live.get(task).cloned())
        };
        if let Some(key) = question {
            self.emit(run, ev("question.answered", "lead", format!("主控回答：{}", short(text, 40))).task(task).to(&agent).data(json!({ "key": key, "answer": text, "via": "lead" })));
            self.resolve(run, &key, json!({ "answer": text, "note": "" })).map_err(|e| format!("{e:#}"))?;
            return Ok(format!("已回答 {task} 的提问，{agent} 会接着做。"));
        }
        let msg = self.post_message(run, "lead", &agent, task, &format!("{LEAD_REPLY}{text}"));
        match live {
            Some(tx) => {
                tx.send(Steer::Message(msg)).map_err(|_| format!("{task} 的会话刚刚结束"))?;
                Ok(format!("已把消息发给正在执行 {task} 的 {agent}。"))
            }
            None => {
                self.route_message(run, &agent, task, 0, &msg.id, "next");
                run.st.lock().unwrap().notes.entry(agent.clone()).or_default().push(msg);
                Ok(format!("{task} 现在没有在执行，消息会在 {agent} 下一次开始工作时带给它。"))
            }
        }
    }

    /// A collaborator writes progress, a blocker or a note to the board while it works.
    fn board_post(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err("text 不能为空".into());
        }
        let kind = match args["kind"].as_str().unwrap_or("progress") {
            k @ ("progress" | "blocker" | "note") => k,
            other => return Err(format!("kind 只能是 progress、blocker 或 note，不能是 {other}")),
        };
        let agent = run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == task).map(|t| t.agent.clone()).ok_or_else(|| format!("看板上没有任务 {task}"))?;
        let label = match kind { "blocker" => "阻塞", "note" => "备注", _ => "进展" };
        self.emit(run, ev("board.posted", &agent, format!("{label}：{}", short(text, 80))).task(task).to("lead").data(json!({ "kind": kind, "text": text })));
        Ok("已写到共享看板，主控会看到。".into())
    }

    // ------------------------------------------------------------------
    // Planning
    // ------------------------------------------------------------------

    /// Messages left for an agent while it had no open session, handed over with its next prompt.
    /// Record a message the moment it is sent, before any session has picked it up.
    // @project-doc docs/interfaces/events.md#message_flow
    fn post_message(&self, run: &Run, actor: &str, to: &str, task: &str, text: &str) -> Msg {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("m{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        let mut e = ev("user.message", actor, short(text, 80)).to(to).data(json!({ "id": id, "text": text, "delivery": "pending" }));
        if !task.is_empty() {
            e = e.task(task);
        }
        self.emit(run, e);
        Msg { id, text: text.to_string() }
    }

    /// How a sent message went in: `steer` (joined the running turn), `queued` (after this turn),
    /// `prompt` (a turn of its own) or `next` (with the agent's next session).
    fn route_message(&self, run: &Run, agent: &str, task: &str, n: i64, id: &str, delivery: &str) {
        let text = match delivery {
            "steer" => "插入到正在进行的这一轮",
            "queued" => "排队：这一轮结束后发送",
            "prompt" => "作为新的一轮发送",
            _ => "留言：下次会话开始时带给它",
        };
        let mut e = ev("message.routed", "janus", text).to(agent).data(json!({ "id": id, "delivery": delivery }));
        if !task.is_empty() {
            e = e.task(task);
            if n > 0 {
                e = e.attempt(n);
            }
        }
        self.emit(run, e);
    }

    fn take_notes(&self, run: &Run, agent: &str) -> String {
        let notes = run
            .st
            .lock()
            .unwrap()
            .notes
            .remove(agent)
            .unwrap_or_default();
        if notes.is_empty() {
            return String::new();
        }
        let ids: Vec<&str> = notes.iter().map(|m| m.id.as_str()).collect();
        self.emit(
            run,
            ev(
                "message.delivered",
                "janus",
                format!("把你留给 {agent} 的 {} 条消息带进这次会话", notes.len()),
            )
            .to(agent)
            .data(json!({ "ids": ids })),
        );
        format!(
            "\n\n用户补充的信息（请优先考虑）：\n{}",
            notes
                .iter()
                .map(|n| format!("- {}", n.text))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    // ------------------------------------------------------------------
    // Attempts
    // ------------------------------------------------------------------

    async fn run_attempt(self: &Arc<Self>, run: &Arc<Run>, id: &str, n: i64, attempt_id: &str) {
        let result = self.attempt(run, id, n, attempt_id).await;
        let withdrawn: Vec<String> = {
            let mut st = run.st.lock().unwrap();
            st.live.remove(id);
            st.live_agent.retain(|_, task| task != id);
            // Questions, permission requests and forms of this attempt can no longer be answered.
            let prefixes = [format!("q-{id}-{n}-"), format!("req-{id}-{n}-")];
            let keys: Vec<String> = st
                .waiters
                .keys()
                .filter(|k| prefixes.iter().any(|p| k.starts_with(p.as_str())))
                .cloned()
                .collect();
            for k in &keys {
                st.waiters.remove(k);
            }
            keys
        };
        for key in withdrawn {
            self.emit(
                run,
                ev(
                    "decision.withdrawn",
                    "janus",
                    format!("{id} 的这次尝试已经结束，撤回待处理的请求"),
                )
                .task(id)
                .attempt(n)
                .data(json!({ "key": key })),
            );
        }
        if let Some(conn) = run.st.lock().unwrap().conns.remove(id) {
            tokio::spawn(async move { conn.kill().await });
        }
        if let Err(err) = result {
            let msg = format!("{err:#}");
            let (current, cancelled) = {
                let st = run.st.lock().unwrap();
                let t = st.tasks.iter().find(|t| t.def.id == id);
                (t.and_then(|t| t.active_attempt.as_deref()) == Some(attempt_id), st.cancelled)
            };
            if !current {
                self.emit(
                    run,
                    ev(
                        "attempt.late_result",
                        "janus",
                        format!("忽略已过期 Attempt {attempt_id} 的结果"),
                    )
                    .task(id)
                    .attempt(n)
                    .data(json!({ "attemptId": attempt_id, "error": msg })),
                );
                return;
            }
            // Only a cancelled run counts as cancellation. Anything else, including an agent
            // ending its own turn as cancelled, is a failed attempt.
            if cancelled {
                let mut st = run.st.lock().unwrap();
                let t = Run::task_mut(&mut st, id).unwrap();
                t.active_attempt = None;
                if t.status == "running" {
                    t.status = "cancelled";
                }
            }
            let _ = self.db.update_attempt(
                &run.id,
                id,
                n,
                if cancelled { "cancelled" } else { "failed" },
                None,
                None,
                Some(&msg),
                Some(now_ms()),
            );
            if !cancelled {
                self.emit(
                    run,
                    ev("attempt.failed", "janus", format!("{id} 第 {n} 次尝试失败"))
                        .task(id)
                        .attempt(n)
                        .data(json!({ "error": msg })),
                );
                let (retry, max_attempts) = {
                    let mut st = run.st.lock().unwrap();
                    let t = Run::task_mut(&mut st, id).unwrap();
                    t.status = if t.attempts - t.retry_base < t.def.max_attempts {
                        "pending"
                    } else {
                        "failed"
                    };
                    t.active_attempt = None;
                    (t.status == "pending", t.def.max_attempts)
                };
                if retry {
                    self.emit(run, ev("task.retry_scheduled", "janus", format!("按重试策略为 {id} 创建第 {} 次尝试", n + 1)).task(id).data(json!({
                        "next": n + 1, "strategy": "新建 ACP 会话，注入任务说明、上游结果和上次失败原因"
                    })));
                } else {
                    self.emit(
                        run,
                        ev(
                            "task.failed",
                            "janus",
                            format!("{id} 已用完尝试次数，交给主控处理"),
                        )
                        .task(id)
                        .data(json!({ "error": msg, "maxAttempts": max_attempts })),
                    );
                }
            }
        }
        run.wake.notify_one();
    }

    async fn attempt(
        self: &Arc<Self>,
        run: &Arc<Run>,
        id: &str,
        n: i64,
        attempt_id: &str,
    ) -> Result<()> {
        let (def, agent, upstream, handoffs, last_error) = {
            let st = run.st.lock().unwrap();
            let t = st
                .tasks
                .iter()
                .find(|t| t.def.id == id)
                .context("task missing")?;
            let (upstream, handoffs): (Vec<String>, Vec<Value>) = t.def.deps.iter().filter_map(|d| st.tasks.iter().find(|x| &x.def.id == d)).map(|u| {
                let r = u.result.clone().unwrap_or_default();
                let files: Vec<String> = u.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
                let names: Vec<&str> = u.artifacts.iter().filter_map(|a| a["name"].as_str()).collect();
                let line = format!("- {} {}（{}，状态 {}）：{}\n  产物：{}", u.def.id, u.def.title, u.agent, u.status, r.summary, if files.is_empty() { "无".into() } else { files.join("，") });
                let handoff = json!({ "from": u.def.id, "fromAgent": u.agent, "status": u.status, "summary": r.summary, "artifacts": names, "risks": r.risks.len() });
                (line, handoff)
            }).unzip();
            (
                t.def.clone(),
                t.agent.clone(),
                upstream,
                handoffs,
                self.db_last_error(&run.id, id),
            )
        };
        let member = run.member(&agent).context("member missing")?.clone();
        let spec = self.spec_for(&member.runtime)?;
        self.db
            .start_attempt(&run.id, id, n, &agent, &spec.id, now_ms())?;

        let workspace = run.cwd.clone();
        let log = self
            .cfg
            .data_dir
            .join("logs")
            .join(&run.id)
            .join(format!("{id}-a{n}.log"));
        let (conn, mut rx) =
            tokio::time::timeout(STARTUP_TIMEOUT, AcpConn::spawn(&spec, &workspace, &log))
                .await
                .map_err(|_| {
                    anyhow!(
                        "Agent 启动超过 {} 分钟没有完成",
                        STARTUP_TIMEOUT.as_secs() / 60
                    )
                })??;
        run.st.lock().unwrap().conns.insert(id.into(), conn.clone());
        if run.st.lock().unwrap().cancelled {
            return Err(Cancelled.into());
        }
        // A task interrupted by a restart reopens its old session when the agent can load it.
        let previous = run.st.lock().unwrap().resume_session.remove(id);
        let opened = tokio::time::timeout(STARTUP_TIMEOUT, async {
            let mut resumed = false;
            // Collaborators write progress and blockers to the shared board. The board is an HTTP
            // MCP server; agents that only take stdio servers (Pi) work without it and report
            // through their result and questions.
            let board: Vec<Value> = if conn.capabilities["mcpCapabilities"]["http"] == true {
                vec![self.mcp_server(run, id)]
            } else {
                vec![]
            };
            let session = match previous {
                Some(old) if conn.capabilities["loadSession"] == true => {
                    match conn.request("session/load", json!({ "sessionId": old, "cwd": workspace, "mcpServers": mcp_servers(&member).into_iter().chain(board.iter().cloned()).collect::<Vec<_>>(), "_meta": session_meta(&format!("janus-{}", member.id), &session_values(&member)) })).await {
                        Ok(r) => {
                            // The reload replays history as updates; it is already in the event log.
                            // Updates can trail the response, so wait until the agent goes quiet.
                            drain_replay(&conn, &mut rx).await;
                            resumed = true;
                            let mut r = if r.is_object() { r } else { json!({}) };
                            r["sessionId"] = json!(old);
                            r
                        }
                        Err(_) => new_session(&conn, &workspace, &member, &board, None).await?,
                    }
                }
                _ => new_session(&conn, &workspace, &member, &board, None).await?,
            };
            let applied = configure_session(&conn, &session, &member).await?;
            Ok::<_, anyhow::Error>((session, applied, resumed))
        })
        .await
        .map_err(|_| anyhow!("打开 ACP 会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
        let (session, applied, resumed) = opened;
        let sid = session["sessionId"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let prompt = if resumed {
            format!(
                "编排器中断后，这个会话已经恢复。请从中断的地方继续完成原来的任务，完成后按最初的要求以 ```json 结构化结果结束。{}",
                self.take_notes(run, &agent)
            )
        } else {
            format!(
                "{}{}",
                task_prompt(run, &member, &def, &workspace, &upstream, last_error.as_deref()),
                self.take_notes(run, &agent)
            )
        };
        self.db.update_attempt(
            &run.id,
            id,
            n,
            "running",
            Some(&sid),
            Some(&workspace.to_string_lossy()),
            None,
            None,
        )?;
        self.emit(run, ev("attempt.started", "janus", format!("启动 {id} 第 {n} 次尝试 · {}", spec.name)).task(id).attempt(n).to(&agent).data(json!({
            "attemptId": attempt_id, "session": sid, "workspace": format!("工作目录 {}", workspace.display()), "path": workspace, "runtime": spec.name, "kind": spec.kind, "agentInfo": conn.agent_info,
            "config": applied, "enforcement": enforcement(&applied), "prompt": prompt, "resumed": resumed
        })));
        // What each upstream task handed to this session: the fact behind "who used whose result".
        if !resumed {
            for h in handoffs {
                let from = h["from"].as_str().unwrap_or_default().to_string();
                let actor = h["fromAgent"].as_str().unwrap_or("janus").to_string();
                self.emit(
                    run,
                    ev("context.handoff", &actor, format!("{from} 的结果交给 {id}"))
                        .task(id)
                        .attempt(n)
                        .to(&agent)
                        .data(h),
                );
            }
        }

        let ctx = Ctx {
            agent: &agent,
            task: Some(id),
            attempt: Some(n),
            goal: false,
        };
        let (steer_tx, steer_rx) = mpsc::unbounded_channel();
        {
            let mut st = run.st.lock().unwrap();
            st.live.insert(id.into(), steer_tx);
            st.live_agent.insert(agent.clone(), id.into());
        }
        let mut ctl = Control {
            rx: steer_rx,
            steering: !conn.capabilities["_meta"]["lody"]["steering"].is_null(),
            human: false,
            interrupted: false,
            finish: false,
            queued: vec![],
        };
        let mut next = Some(prompt);
        let mut text = String::new();
        let mut round = 0;
        let mut asked = 0;
        let mut result = loop {
            if let Some(p) = next.take() {
                text = self
                    .stream_prompt(
                        run,
                        &conn,
                        &mut rx,
                        &sid,
                        &p,
                        &ctx,
                        Some(&mut ctl),
                        Some(ATTEMPT_TIMEOUT),
                    )
                    .await?;
                let suffix = if round == 0 {
                    String::new()
                } else {
                    format!("-r{round}")
                };
                round += 1;
                self.save_artifact(
                    run,
                    id,
                    n,
                    &agent,
                    &format!("{id}-a{n}-report{suffix}.md"),
                    "report",
                    text.as_bytes(),
                    "Agent 这一轮的最终回复",
                )?;
            }
            if ctl.finish {
                break finished_by_user(&text);
            }
            if !ctl.queued.is_empty() {
                let msgs: Vec<Msg> = ctl.queued.drain(..).collect();
                let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
                self.emit(
                    run,
                    ev(
                        "message.delivered",
                        "janus",
                        format!(
                            "这一轮结束，把你排队的 {} 条消息作为新一轮发给 {agent}",
                            msgs.len()
                        ),
                    )
                    .task(id)
                    .attempt(n)
                    .to(&agent)
                    .data(json!({ "ids": ids })),
                );
                next = Some(msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n"));
                continue;
            }
            if ctl.human {
                self.emit(
                    run,
                    ev(
                        "agent.status",
                        &agent,
                        "这一轮已结束，等待你的消息（人工接管中）",
                    )
                    .task(id)
                    .data(json!({ "phase": "waiting_for_input", "turn": "idle" })),
                );
                match ctl.rx.recv().await {
                    None => return Err(Cancelled.into()),
                    Some(Steer::Message(m)) => {
                        self.route_message(run, &agent, id, n, &m.id, "prompt");
                        next = Some(m.text);
                    }
                    Some(Steer::Release) => {
                        ctl.human = false;
                        self.emit(
                            run,
                            ev(
                                "control.released",
                                "user",
                                format!("交还托管：{agent} 继续自主完成任务"),
                            )
                            .task(id)
                            .attempt(n)
                            .to(&agent),
                        );
                        next = Some(RELEASE_PROMPT.into());
                    }
                    Some(Steer::Finish) => ctl.finish = true,
                    Some(Steer::TakeOver | Steer::Interrupt) => {}
                }
                continue;
            }
            let parsed = parse_result(&text);
            let Some(q) = parsed.question.clone() else {
                break parsed;
            };
            asked += 1;
            if asked > 4 {
                bail!("连续 4 轮仍在提问，没有完成任务");
            }
            let key = format!("q-{id}-{n}-{round}");
            let question = q["text"]
                .as_str()
                .or(q.as_str())
                .unwrap_or("需要你的决定")
                .to_string();
            let options: Vec<String> = q["options"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| o.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            self.emit(
                run,
                ev("question.asked", &agent, short(&question, 60))
                    .task(id)
                    .attempt(n)
                    .to("lead")
                    .data(json!({
                        "key": key, "question": question, "options": options, "blocking": true
                    })),
            );
            // Goal mode: the lead orchestrated this task and answers its questions (it reads them
            // with `wait`). Otherwise the question goes to the person.
            // The lead answers questions on the board; once it is gone, they go to the person.
            if run.st.lock().unwrap().lead_done.is_some() {
                self.escalate_open_questions(run);
            }
            // The question card and the session both reach the same agent; whichever you use first wins.
            tokio::select! {
                answer = self.wait_for(run, &key) => {
                    let answer = answer?;
                    next = Some(format!(
                        "你的问题已得到回答：{}\n补充说明：{}\n请据此继续完成任务。完成后同样以 ```json 结构化结果结束。",
                        answer["answer"].as_str().unwrap_or(""),
                        answer["note"].as_str().filter(|s| !s.is_empty()).unwrap_or("无")
                    ));
                }
                cmd = ctl.rx.recv() => {
                    run.st.lock().unwrap().waiters.remove(&key);
                    let reply = match cmd {
                        None => return Err(Cancelled.into()),
                        Some(Steer::Message(m)) => {
                            self.route_message(run, &agent, id, n, &m.id, "prompt");
                            next = Some(m.text.clone());
                            m.text
                        }
                        Some(Steer::Finish) => { ctl.finish = true; "（直接以当前结果完成）".into() }
                        Some(_) => {
                            ctl.human = true;
                            self.emit(run, ev("control.taken", "user", format!("接管 {agent} 的会话")).task(id).attempt(n).to(&agent));
                            "（转为人工接管，在会话中继续）".into()
                        }
                    };
                    self.emit(run, ev("question.answered", "user", format!("在会话中回答：{}", short(&reply, 40))).task(id).to(&agent).data(json!({ "key": key, "answer": reply, "via": "session" })));
                }
            }
        };
        conn.kill().await;
        // An abort that arrived between turns (or a cancel) must not end up as a success.
        self.check_aborted(run, id, &mut ctl)?;

        self.record_changes(run, id, n, &agent, &workspace)?;
        if result.evidence.is_empty() {
            result
                .evidence
                .push(format!("最终回复：{id}-a{n}-report.md"));
        }
        self.emit(
            run,
            ev(
                "result.submitted",
                &agent,
                format!("提交结构化结果：{}", short(&result.summary, 60)),
            )
            .task(id)
            .attempt(n)
            .data(json!({
                "attemptId": attempt_id,
                "summary": result.summary, "evidence": result.evidence, "risks": result.risks
            })),
        );

        self.verify_result(run, id, n, attempt_id, &def, &workspace, &mut result)
            .await?;
        self.check_aborted(run, id, &mut ctl)?;

        let current = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == id)
            .and_then(|t| t.active_attempt.as_deref())
            == Some(attempt_id);
        if !current {
            self.emit(
                run,
                ev(
                    "attempt.late_result",
                    "janus",
                    format!("忽略已过期 Attempt {attempt_id} 的成功结果"),
                )
                .task(id)
                .attempt(n),
            );
            return Ok(());
        }
        let seq = self.emit(
            run,
            ev(
                "task.succeeded",
                "janus",
                format!("校验结构化结果与证据后，将 {id} 标记为完成"),
            )
            .task(id)
            .attempt(n)
            .data(json!({
                "attemptId": attempt_id,
                "summary": result.summary, "evidence": result.evidence
            })),
        );
        {
            let mut st = run.st.lock().unwrap();
            let t = Run::task_mut(&mut st, id).unwrap();
            t.status = "succeeded";
            t.succeeded_seq = Some(seq);
            t.result = Some(result);
            t.active_attempt = None;
        }
        // The event above is the fact; a failed bookkeeping write must not turn it into a retry.
        if let Err(err) = self.db.update_attempt(
            &run.id,
            id,
            n,
            "succeeded",
            None,
            None,
            None,
            Some(now_ms()),
        ) {
            crate::config::log(format!("janus: failed to record attempt {id}#{n}: {err:#}"));
        }
        Ok(())
    }

    /// Stop an attempt whose run was cancelled, even if the cancel reached it while it was not
    /// listening (between turns or during verification).
    fn check_aborted(&self, run: &Run, id: &str, ctl: &mut Control) -> Result<()> {
        let mut late = vec![];
        while let Ok(cmd) = ctl.rx.try_recv() {
            if let Steer::Message(m) = cmd {
                late.push(m);
            }
        }
        if let Some(agent) = run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == id).map(|t| t.agent.clone()) {
            for m in &late {
                self.route_message(run, &agent, id, 0, &m.id, "next");
            }
        }
        let mut st = run.st.lock().unwrap();
        // Messages sent after the last turn are kept for this agent's next session.
        if let Some(agent) = st
            .tasks
            .iter()
            .find(|t| t.def.id == id && !late.is_empty())
            .map(|t| t.agent.clone())
        {
            st.notes.entry(agent).or_default().extend(late);
        }
        if st.cancelled {
            return Err(Cancelled.into());
        }
        Ok(())
    }

    // @project-doc docs/domains/run_lifecycle.md#verification
    async fn verify_result(
        &self,
        run: &Run,
        id: &str,
        n: i64,
        attempt_id: &str,
        def: &TaskDef,
        workspace: &Path,
        result: &mut TaskResult,
    ) -> Result<()> {
        self.emit(
            run,
            ev(
                "verification.started",
                "janus",
                format!("开始验证 {id} 第 {n} 次尝试"),
            )
            .task(id)
            .attempt(n)
            .data(json!({ "attemptId": attempt_id, "policy": def.verification })),
        );
        let mut failures = vec![];
        if result.summary.trim().is_empty() {
            failures.push("结构化结果缺少 summary".to_string());
        }
        if result.evidence.is_empty() {
            failures.push("没有可核对的 evidence".to_string());
        }
        for command in &def.verification.commands {
            // Orchestrations are checked against the same rules (validate_orchestration). A command
            // that still fails here has a defect the agent cannot fix, so it is skipped as a risk
            // instead of failing every attempt.
            let (program, args, pipeline) = match verification_argv(command) {
                Ok(v) => v,
                Err(reason) => {
                    let note = format!("验证命令未执行（{reason}）：{command}");
                    result.risks.push(note.clone());
                    self.emit(
                        run,
                        ev("verification.command_rejected", "janus", note)
                            .task(id)
                            .attempt(n)
                            .data(json!({ "attemptId": attempt_id, "command": command, "reason": reason })),
                    );
                    continue;
                }
            };
            let unavailable_name = if pipeline { "sh" } else { program.as_str() };
            let output = tokio::time::timeout(
                Duration::from_secs(10 * 60),
                tokio::process::Command::new(&program)
                    .args(&args)
                    .current_dir(workspace)
                    .output(),
            )
            .await
            .map_err(|_| anyhow!("验证命令超时：{command}"))?;
            let output = match output {
                Ok(output) => output,
                Err(err)
                    if err.kind() == std::io::ErrorKind::NotFound
                        && (READ_ONLY_COMMANDS.contains(&unavailable_name) || pipeline) =>
                {
                    let note = format!("只读验证命令不可用，保留 Agent 已提交的证据：{command}");
                    result.risks.push(note.clone());
                    self.emit(
                        run,
                        ev("verification.command_unavailable", "janus", note)
                            .task(id)
                            .attempt(n)
                            .data(json!({ "attemptId": attempt_id, "command": command })),
                    );
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status = output.status.code().unwrap_or(-1);
            self.emit(run, ev("verification.command", "janus", format!("验证命令：{command}（exit {status}）")).task(id).attempt(n).data(json!({
                "attemptId": attempt_id, "command": command, "exitCode": status, "output": short(&format!("{stdout}{stderr}"), 1200)
            })));
            result
                .evidence
                .push(format!("验证命令 `{command}` exit {status}"));
            if !output.status.success() {
                failures.push(format!("验证命令失败：{command}（exit {status}）"));
            }
        }
        if !failures.is_empty() {
            let message = failures.join("；");
            self.db.insert_verification(
                &run.id,
                id,
                n,
                attempt_id,
                "failed",
                &json!({ "attemptId": attempt_id, "failures": failures }),
                now_ms(),
            )?;
            self.emit(
                run,
                ev(
                    "verification.failed",
                    "janus",
                    format!("{id} 验证失败：{message}"),
                )
                .task(id)
                .attempt(n)
                .data(json!({ "attemptId": attempt_id, "failures": failures })),
            );
            bail!("验证失败：{message}");
        }
        self.db.insert_verification(
            &run.id,
            id,
            n,
            attempt_id,
            "passed",
            &json!({ "attemptId": attempt_id, "commands": def.verification.commands }),
            now_ms(),
        )?;
        self.emit(
            run,
            ev("verification.passed", "janus", format!("{id} 验证通过"))
                .task(id)
                .attempt(n)
                .data(json!({ "attemptId": attempt_id, "commands": def.verification.commands })),
        );
        Ok(())
    }

    /// The failure of the previous attempt, if the previous attempt failed. A task that succeeded
    /// since (and was retried by the person) starts fresh, not from an old error.
    fn db_last_error(&self, run_id: &str, task: &str) -> Option<String> {
        self.db
            .events(run_id, 0)
            .ok()?
            .into_iter()
            .rev()
            .find(|e| {
                matches!(e.kind.as_str(), "attempt.failed" | "task.succeeded")
                    && e.task.as_deref() == Some(task)
            })
            .filter(|e| e.kind == "attempt.failed")
            .and_then(|e| e.data?["error"].as_str().map(str::to_owned))
    }

    /// Stream one prompt turn: bridge ACP updates into events, route permission requests to the user,
    /// and apply what the person does in the session (steer, interrupt, take over) while the turn runs.
    async fn stream_prompt(
        &self,
        run: &Run,
        conn: &Arc<AcpConn>,
        rx: &mut mpsc::UnboundedReceiver<Incoming>,
        sid: &str,
        text: &str,
        ctx: &Ctx<'_>,
        mut ctl: Option<&mut Control>,
        limit: Option<Duration>,
    ) -> Result<String> {
        // The turn's own deadline replaces the ACP request timeout, so that time spent waiting
        // for a person to decide on a permission request does not count against the agent.
        let prompt = conn.request_timeout(
            "session/prompt",
            json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": text }] }),
            None,
        );
        tokio::pin!(prompt);
        let mut deadline = limit.map(|l| tokio::time::Instant::now() + l);
        let mut out = String::new();
        let mut flushed = 0usize;
        let mut tools: HashMap<String, Value> = HashMap::new();
        let mut last_status = 0i64;
        // Goal mode: the prompt has returned but the agent's goal is still active, so the agent
        // keeps working in further turns of its own; the session is followed until it settles.
        let mut following = false;
        let goal_active = || ctx.goal && run.st.lock().unwrap().goal_status.as_deref() == Some("active");
        loop {
            let steer = async {
                match ctl.as_mut() {
                    Some(c) => c.rx.recv().await,
                    None => std::future::pending().await,
                }
            };
            let expire = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                res = &mut prompt, if !following => {
                    let res = res?;
                    // Updates that arrived with the response; a request among them still gets an answer.
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            Incoming::Notification { method, params } => {
                                if method == "session/update" {
                                    self.on_update(run, &params["update"], &mut out, &mut flushed, &mut tools, &mut last_status, ctx);
                                }
                            }
                            Incoming::Request { id, method, .. } => {
                                if is_user_request(&method) {
                                    conn.respond(id, cancelled_reply(&method)).await?;
                                } else {
                                    conn.respond_error(id, "janus does not implement this client method").await?;
                                }
                            }
                            Incoming::Closed => break,
                        }
                    }
                    self.flush_message(run, &out, &mut flushed, ctx);
                    if run.st.lock().unwrap().cancelled {
                        return Err(Cancelled.into());
                    }
                    let interrupted = ctl.as_mut().is_some_and(|c| std::mem::take(&mut c.interrupted));
                    if res["stopReason"] == "cancelled" && !interrupted {
                        // Nobody asked for this: the agent stopped on its own. Retry it like any failure.
                        bail!("Agent 自行以 cancelled 结束了这一轮");
                    }
                    if goal_active() && !interrupted {
                        following = true;
                        continue;
                    }
                    return Ok(out);
                }
                _ = expire => {
                    let _ = conn.notify("session/cancel", json!({ "sessionId": sid })).await;
                    bail!("超过 {} 分钟没有完成", limit.unwrap_or_default().as_secs() / 60);
                }
                cmd = steer => {
                    let Some(c) = ctl.as_mut() else { continue };
                    match cmd {
                        // The attempt's sender only goes away when the run is cancelled.
                        None => return Err(Cancelled.into()),
                        Some(cmd) => self.steer_turn(run, conn, sid, c, cmd, ctx).await?,
                    }
                }
                msg = rx.recv() => match msg {
                    None | Some(Incoming::Closed) => bail!("Agent 进程意外退出"),
                    Some(Incoming::Notification { method, params }) => {
                        if method == "session/update" {
                            self.on_update(run, &params["update"], &mut out, &mut flushed, &mut tools, &mut last_status, ctx);
                        }
                        if following && !goal_active() {
                            self.flush_message(run, &out, &mut flushed, ctx);
                            return Ok(out);
                        }
                    }
                    Some(Incoming::Request { id, method, params }) => {
                        if is_user_request(&method) {
                            let waited = tokio::time::Instant::now();
                            let outcome = self.user_request(run, conn, sid, &method, &params, ctx, ctl.as_deref_mut()).await?;
                            if let Some(d) = deadline.as_mut() {
                                *d += waited.elapsed();
                            }
                            conn.respond(id, outcome).await?;
                        } else {
                            conn.respond_error(id, "janus does not implement this client method").await?;
                        }
                    }
                }
            }
        }
    }

    /// Apply one session command while a turn is running.
    async fn steer_turn(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        c: &mut Control,
        cmd: Steer,
        ctx: &Ctx<'_>,
    ) -> Result<()> {
        let (agent, task, n) = (
            ctx.agent,
            ctx.task.unwrap_or_default(),
            ctx.attempt.unwrap_or(0),
        );
        match cmd {
            Steer::Message(m) => {
                // Lody steering joins the running turn; without it the message waits for the turn to end.
                // A short timeout: while this waits, the session loop answers nothing else, and the
                // agent may itself be waiting for us (for example on a permission request).
                let injected = c.steering
                    && conn
                        .request_timeout("_lody/session/steer", json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": m.text }], "steerId": m.id }), Some(STEER_TIMEOUT))
                        .await
                        .is_ok_and(|r| r["outcome"] == "injected");
                self.route_message(run, agent, task, n, &m.id, if injected { "steer" } else { "queued" });
                if !injected {
                    c.queued.push(m);
                }
            }
            Steer::Interrupt | Steer::Finish => {
                let finish = matches!(cmd, Steer::Finish);
                conn.notify("session/cancel", json!({ "sessionId": sid }))
                    .await?;
                c.interrupted = true;
                if finish {
                    c.finish = true;
                } else if !c.human {
                    c.human = true;
                }
                let text = if finish {
                    format!("停止 {agent} 这一轮，并以当前结果完成任务")
                } else {
                    format!("打断 {agent} 这一轮，转为人工接管")
                };
                self.emit(
                    run,
                    ev("session.interrupted", "user", text)
                        .task(task)
                        .attempt(n)
                        .to(agent)
                        .data(json!({ "finish": finish })),
                );
            }
            Steer::TakeOver => {
                if !c.human {
                    c.human = true;
                    self.emit(
                        run,
                        ev(
                            "control.taken",
                            "user",
                            format!("接管 {agent} 的会话：这一轮结束后等你的消息"),
                        )
                        .task(task)
                        .attempt(n)
                        .to(agent),
                    );
                }
            }
            Steer::Release => {
                if c.human {
                    c.human = false;
                    self.emit(
                        run,
                        ev(
                            "control.released",
                            "user",
                            format!("交还托管：{agent} 继续自主完成任务"),
                        )
                        .task(task)
                        .attempt(n)
                        .to(agent),
                    );
                }
            }
        }
        Ok(())
    }

    /// Agent text since the last tool call becomes one `agent.message` event, so the session reads like a chat.
    fn flush_message(&self, run: &Run, out: &str, flushed: &mut usize, ctx: &Ctx<'_>) {
        let text = out[*flushed..].trim();
        *flushed = out.len();
        if text.is_empty() {
            return;
        }
        let mut e = ev("agent.message", ctx.agent, short(text, 80))
            .data(json!({ "text": short(text, 8000) }));
        if let Some(t) = ctx.task {
            e = e.task(t);
        }
        if let Some(n) = ctx.attempt {
            e = e.attempt(n);
        }
        self.emit(run, e);
    }

    fn on_update(
        &self,
        run: &Run,
        u: &Value,
        out: &mut String,
        flushed: &mut usize,
        tools: &mut HashMap<String, Value>,
        last_status: &mut i64,
        ctx: &Ctx<'_>,
    ) {
        match u["sessionUpdate"].as_str().unwrap_or("") {
            // Codex reports its /goal as session info: { goal: { objective, status } }, or null once cleared.
            "session_info_update" if ctx.goal && u["_meta"]["codex"].get("goal").is_some() => {
                let goal = &u["_meta"]["codex"]["goal"];
                let status = goal["status"].as_str().map(str::to_owned);
                run.st.lock().unwrap().goal_status = status.clone();
                self.emit(run, ev("goal.status", ctx.agent, match status.as_deref() {
                    Some("active") => "目标进行中".to_string(),
                    Some(other) => format!("目标状态：{other}"),
                    None => "目标已清除".to_string(),
                }).data(json!({ "status": status, "objective": goal["objective"] })));
            }
            "agent_message_chunk" => {
                if let Some(t) = u["content"]["text"].as_str() {
                    out.push_str(t);
                }
            }
            "tool_call" | "tool_call_update" => {
                if u["sessionUpdate"] == "tool_call" {
                    self.flush_message(run, out, flushed, ctx);
                }
                let id = u["toolCallId"].as_str().unwrap_or("").to_string();
                let entry = tools.entry(id.clone()).or_insert_with(|| json!({}));
                for key in [
                    "title",
                    "kind",
                    "rawInput",
                    "rawOutput",
                    "content",
                    "status",
                ] {
                    if !u[key].is_null() {
                        entry[key] = u[key].clone();
                    }
                }
                let status = entry["status"].as_str().unwrap_or("pending").to_string();
                let title = entry["title"].as_str().unwrap_or("工具调用").to_string();
                if matches!(status.as_str(), "completed" | "failed") {
                    let entry = tools.remove(&id).unwrap();
                    let mut e = ev("tool.call", ctx.agent, short(&title, 80)).data(json!({
                        "tool": entry["kind"].as_str().unwrap_or("tool"),
                        "input": summarize_input(&entry["rawInput"], &title),
                        "output": summarize_output(&entry),
                        "status": status,
                    }));
                    if let Some(t) = ctx.task {
                        e = e.task(t);
                    }
                    if let Some(n) = ctx.attempt {
                        e = e.attempt(n);
                    }
                    self.emit(run, e);
                } else if now_ms() - *last_status > 1500 {
                    *last_status = now_ms();
                    let kind = entry["kind"].as_str().unwrap_or("");
                    let phase = match kind {
                        "read" | "search" | "fetch" => "reading_files",
                        "edit" | "delete" | "move" => "editing",
                        _ if title.contains("test") || title.contains("测试") => "running_test",
                        _ => "running_tool",
                    };
                    let mut e = ev("agent.status", ctx.agent, short(&title, 80))
                        .data(json!({ "phase": phase }));
                    if let Some(t) = ctx.task {
                        e = e.task(t);
                    }
                    self.emit(run, e);
                }
            }
            _ => {}
        }
    }

    /// Put an agent's request in front of the person and answer it in the request's own terms:
    /// a permission request with the option they pick (every option the agent offered, not just
    /// allow/deny), an elicitation with the form they fill in or the link they finish. What an
    /// agent may do without asking is its permission mode, chosen on the role; whatever it does
    /// ask lands here. While waiting, the session stays responsive: an interrupt or finish
    /// withdraws the request, an abort ends the attempt, and messages queue for the end of the turn.
    #[allow(clippy::too_many_arguments)]
    // @project-doc docs/interfaces/acp_integration.md#user_request_replies
    async fn user_request(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        method: &str,
        params: &Value,
        ctx: &Ctx<'_>,
        mut ctl: Option<&mut Control>,
    ) -> Result<Value> {
        let task = ctx.task.unwrap_or_default();
        let n = ctx.attempt.unwrap_or(0);
        let elicit = method == "elicitation/create";
        let options = params["options"].as_array().cloned().unwrap_or_default();
        let pick = |prefix: &str| {
            options
                .iter()
                .find(|o| o["kind"].as_str().is_some_and(|k| k.starts_with(prefix)))
                .and_then(|o| o["optionId"].as_str())
                .map(str::to_owned)
        };
        let allow = pick("allow_once")
            .or_else(|| pick("allow"))
            .or_else(|| {
                options
                    .first()
                    .and_then(|o| o["optionId"].as_str().map(str::to_owned))
            })
            .unwrap_or_default();
        let reject = pick("reject").unwrap_or_else(|| allow.clone());
        let tool = &params["toolCall"];
        let title = if elicit {
            params["message"].as_str().unwrap_or("Agent 需要你填写信息").to_string()
        } else {
            tool["title"].as_str().unwrap_or("执行操作").to_string()
        };
        let command = summarize_input(&tool["rawInput"], &title);
        let selected = |id: &str| json!({ "outcome": { "outcome": "selected", "optionId": id } });

        let key = format!("req-{}-{}-{}", if task.is_empty() { ctx.agent } else { task }, n, now_ms());
        let mode = params["mode"].as_str().unwrap_or("form");
        let mut e = if elicit {
            ev("input.requested", ctx.agent, format!("请你填写：{}", short(&title, 50))).data(json!({
                "key": key, "agent": ctx.agent, "mode": mode, "message": title,
                "schema": params["requestedSchema"], "url": params["url"],
                "toolCallId": params["toolCallId"], "meta": params["_meta"],
            }))
        } else {
            ev("approval.requested", ctx.agent, format!("请求批准：{}", short(&title, 50))).data(json!({
                "key": key, "agent": ctx.agent, "command": command, "title": title,
                "toolKind": tool["kind"], "content": tool["content"], "rawInput": tool["rawInput"],
                "policy": "这个角色的权限模式要求这一步由你决定",
                "impact": "Agent 在等你选择；选项由 Agent 提供，按它自己的含义生效。",
                "options": options,
            }))
        };
        if !task.is_empty() {
            e = e.task(task).attempt(n);
        }
        self.emit(run, e);
        let wait = self.wait_for(run, &key);
        tokio::pin!(wait);
        loop {
            let steer = async {
                match ctl.as_mut() {
                    Some(c) => c.rx.recv().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                decision = &mut wait => {
                    let decision = decision?;
                    if elicit {
                        return Ok(match decision["action"].as_str().unwrap_or("cancel") {
                            "accept" => json!({ "action": "accept", "content": decision["content"].clone() }),
                            "decline" => json!({ "action": "decline" }),
                            _ => json!({ "action": "cancel" }),
                        });
                    }
                    if let Some(o) = decision["option"].as_str().filter(|o| options.iter().any(|x| x["optionId"] == *o)) {
                        return Ok(selected(o));
                    }
                    return Ok(selected(if decision["allow"].as_bool().unwrap_or(false) {
                        &allow
                    } else {
                        &reject
                    }));
                }
                cmd = steer => {
                    let Some(c) = ctl.as_mut() else { continue };
                    match cmd {
                        None => {
                            self.withdraw(run, &key, task, n, "会话已结束，撤回这个请求");
                            return Err(Cancelled.into());
                        }
                        Some(cmd @ (Steer::Interrupt | Steer::Finish)) => {
                            self.withdraw(run, &key, task, n, "这一轮被打断，撤回这个请求");
                            self.steer_turn(run, conn, sid, c, cmd, ctx).await?;
                            return Ok(cancelled_reply(method));
                        }
                        Some(Steer::Message(m)) => {
                            // The agent is blocked on this request; the message goes in after the turn.
                            self.route_message(run, ctx.agent, task, n, &m.id, "queued");
                            c.queued.push(m);
                        }
                        Some(cmd) => self.steer_turn(run, conn, sid, c, cmd, ctx).await?,
                    }
                }
            }
        }
    }

    /// Drop a pending decision nobody can act on any more, and tell the page to remove its card.
    fn withdraw(&self, run: &Run, key: &str, task: &str, n: i64, text: &str) {
        run.st.lock().unwrap().waiters.remove(key);
        let mut e = ev("decision.withdrawn", "janus", text).data(json!({ "key": key }));
        if !task.is_empty() {
            e = e.task(task).attempt(n);
        }
        self.emit(run, e);
    }

    // ------------------------------------------------------------------
    // Workspaces and artifacts
    // ------------------------------------------------------------------

    /// Working in place there is nothing to commit. In a Git directory, keep what the working
    /// tree now differs from HEAD by whenever a task leaves it different, as that task's diff.
    /// Tasks running side by side share the directory, so the diff can include their changes too.
    fn record_changes(&self, run: &Run, id: &str, n: i64, agent: &str, workspace: &Path) -> Result<()> {
        if !run.git {
            return Ok(());
        }
        let out = |args: &[&str]| git_out(workspace, args);
        let diff = working_diff(workspace);
        let untracked = out(&["ls-files", "--others", "--exclude-standard"]);
        let mut st = run.st.lock().unwrap();
        if diff.trim().is_empty() || diff == st.last_diff {
            return Ok(());
        }
        st.last_diff = diff.clone();
        drop(st);
        let stat = out(&["diff", "--shortstat", "HEAD"]).trim().to_string();
        let new_files = untracked.lines().count();
        let desc = [
            (!stat.is_empty()).then_some(stat),
            (new_files > 0).then(|| format!("{new_files} 个新文件")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        self.save_artifact(run, id, n, agent, &format!("{id}-a{n}.diff"), "git_diff", diff.as_bytes(), &format!("{desc} · 工作目录相对 HEAD 的改动，未提交"))
    }

    fn save_artifact(
        &self,
        run: &Run,
        task: &str,
        n: i64,
        agent: &str,
        name: &str,
        kind: &str,
        bytes: &[u8],
        desc: &str,
    ) -> Result<()> {
        let dir = self.cfg.data_dir.join("artifacts").join(&run.id).join(task);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        let sha: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.db.insert_artifact(
            &run.id,
            task,
            n,
            name,
            kind,
            &path.to_string_lossy(),
            &sha,
            bytes.len() as i64,
            now_ms(),
        )?;
        let seq = self.emit(run, ev("artifact.created", agent, format!("提交产物 {name}")).task(task).attempt(n).data(json!({
            "name": name, "kind": kind, "desc": desc, "path": path, "hash": &sha[..12], "bytes": bytes.len()
        })));
        let mut st = run.st.lock().unwrap();
        if let Some(t) = Run::task_mut(&mut st, task) {
            t.artifacts
                .retain(|a| a["kind"] != kind || kind == "report");
            t.artifacts.push(
                json!({ "name": name, "kind": kind, "desc": desc, "path": path, "seq": seq }),
            );
        }
        Ok(())
    }

    /// `partial`: the person accepted the risk; failed and skipped tasks are listed as risks.
    fn deliver(&self, run: &Run, partial: bool) {
        let result = {
            let st = run.st.lock().unwrap();
            let mut deliverables = vec![];
            let mut evidence = vec![];
            let mut risks = vec![];
            for t in &st.tasks {
                let unfinished = match t.status {
                    "failed" => Some("失败"),
                    "cancelled" => Some("已取消"),
                    "skipped_by_decision" => Some("按你的决定跳过"),
                    _ => None,
                };
                if let Some(state) = unfinished {
                    risks.push(format!("{}：{state}，没有交付结果", t.def.id));
                }
                let r = t.result.clone().unwrap_or_default();
                for a in &t.artifacts {
                    if a["kind"] == "git_diff" {
                        deliverables.push(a.clone());
                    }
                }
                if let Some(e) = r.evidence.first() {
                    evidence.push(
                        json!({ "text": format!("{}：{}", t.def.id, e), "seq": t.succeeded_seq }),
                    );
                }
                risks.extend(r.risks.iter().map(|x| format!("{}：{}", t.def.id, x)));
            }
            if let Some(last) = st.tasks.last() {
                if let Some(report) = last.artifacts.iter().rev().find(|a| a["kind"] == "report") {
                    deliverables.push(report.clone());
                }
            }
            // The lead reports how the goal went, in its last message.
            let summary = match &st.lead_done {
                Some(Ok(text)) => lead_summary(text),
                Some(Err(err)) => {
                    risks.push(format!("主控的会话异常结束：{}", short(err, 200)));
                    "主控没能完成目标，下面是协作者已完成的部分。".to_string()
                }
                None => String::new(),
            };
            let place_note = format!(" 改动直接在工作目录 {} 中，Janus 没有提交。", run.cwd.display());
            json!({
                "headline": if partial { format!("「{}」部分交付", short(&run.goal, 30)) } else { format!("「{}」已完成", short(&run.goal, 30)) },
                "partial": partial,
                "summary": format!("{summary}{place_note}"),
                "confidence": if risks.is_empty() { "高" } else { "中" },
                "deliverables": deliverables,
                "evidence": evidence,
                "risks": risks,
                "review": "由主控自行核对协作者的结果。",
                "diagnosis": if risks.is_empty() {
                    json!({ "cat": null, "title": "没有已知偏差", "text": "所有任务都提交了结构化结果，没有报告风险。", "refs": [], "actions": [] })
                } else {
                    json!({ "cat": null, "title": format!("有 {} 条已知风险需要你判断", risks.len()), "text": "风险由各 Agent 在结构化结果中自行报告，或由编排器在校验时发现。请逐条核对证据后再验收。", "refs": [], "actions": [] })
                }
            })
        };
        if partial {
            self.emit(
                run,
                ev(
                    "run.delivered",
                    "janus",
                    "已按你的决定结束，交付已完成的部分",
                )
                .data(result),
            );
            self.emit(
                run,
                ev(
                    "run.accepted_with_risk",
                    "user",
                    "用户接受部分结果和已知风险",
                ),
            );
            self.set_status(run, "accepted_with_risk");
        } else {
            self.emit(
                run,
                ev("run.delivered", "janus", "主控完成了目标，交付结果已准备好").data(result),
            );
            self.set_status(run, "delivered");
        }
    }

    // ------------------------------------------------------------------
    // User actions
    // ------------------------------------------------------------------

    // @project-doc docs/interfaces/http_api.md#run_actions
    pub fn action(self: &Arc<Self>, run: &Arc<Run>, a: &Value) -> Result<()> {
        let kind = a["type"].as_str().unwrap_or("");
        let key = a["key"].as_str().unwrap_or("");
        let task = a["task"].as_str().unwrap_or("");
        match kind {
            "answer" => {
                self.expect_waiter(run, key)?;
                let answer = a["answer"].as_str().unwrap_or("").to_string();
                let agent = run
                    .st
                    .lock()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|t| t.def.id == task)
                    .map(|t| t.agent.clone())
                    .unwrap_or_default();
                self.emit(
                    run,
                    ev("question.answered", "user", format!("回答：{answer}"))
                        .task(task)
                        .to(&agent)
                        .data(json!({ "key": key, "answer": answer, "note": a["note"] })),
                );
                self.resolve(run, key, a.clone())?;
                self.emit(
                    run,
                    ev(
                        "answer.delivered",
                        "janus",
                        format!("通过 ACP 将回答发回 {task} 的会话"),
                    )
                    .task(task)
                    .to(&agent)
                    .data(json!({ "key": key })),
                );
            }
            "approve" | "input" => {
                self.expect_waiter(run, key)?;
                let agent = run
                    .st
                    .lock()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|t| t.def.id == task)
                    .map(|t| t.agent.clone())
                    .unwrap_or_else(|| a["agent"].as_str().unwrap_or("lead").to_string());
                let (ty, text, value) = if kind == "approve" {
                    let allow = a["allow"].as_bool().unwrap_or(false);
                    let label = a["label"].as_str().filter(|l| !l.is_empty());
                    (
                        if allow { "approval.granted" } else { "approval.denied" },
                        match (label, allow) {
                            (Some(l), _) => format!("选择：{l}"),
                            (None, true) => "批准执行这一次".into(),
                            (None, false) => "拒绝执行".into(),
                        },
                        json!({ "allow": allow, "option": a["option"] }),
                    )
                } else {
                    let action = match a["action"].as_str() {
                        Some(x @ ("accept" | "decline")) => x,
                        _ => "cancel",
                    };
                    let content = if a["content"].is_object() { a["content"].clone() } else { json!({}) };
                    (
                        "input.answered",
                        match action {
                            "accept" => "提交了填写的内容".into(),
                            "decline" => "拒绝填写".into(),
                            _ => "取消了这个请求".into(),
                        },
                        json!({ "action": action, "content": content }),
                    )
                };
                let mut e = ev(ty, "user", text).to(&agent).data(json!({ "key": key, "option": a["option"], "action": value["action"], "content": value["content"] }));
                if !task.is_empty() {
                    e = e.task(task);
                }
                self.emit(run, e);
                self.resolve(run, key, value)?;
            }
            "pause" | "resume" => {
                let pause = kind == "pause";
                {
                    let mut st = run.st.lock().unwrap();
                    st.paused = pause;
                }
                self.emit(
                    run,
                    ev(
                        if pause { "run.paused" } else { "run.resumed" },
                        "user",
                        if pause {
                            "暂停 Run：进行中的尝试会继续完成，不再启动新任务"
                        } else {
                            "继续 Run"
                        },
                    ),
                );
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "extendBudget" => {
                let minutes = a["minutes"].as_i64().unwrap_or(60).clamp(1, 24 * 60);
                let added = minutes * 60 * 1000;
                // Counted from now when the budget already ran out, so one extension is enough.
                let deadline = run.deadline_ms.load(Ordering::SeqCst).max(now_ms()) + added;
                run.deadline_ms.store(deadline, Ordering::SeqCst);
                run.st.lock().unwrap().budget_notified = false;
                self.emit(
                    run,
                    ev(
                        "run.budget_extended",
                        "user",
                        format!("追加 Run 预算 {minutes} 分钟"),
                    )
                    .data(json!({ "minutes": minutes, "addedMs": added })),
                );
                self.set_status(run, "running");
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "cancel" => {
                let conns: Vec<Arc<AcpConn>> = {
                    let mut st = run.st.lock().unwrap();
                    st.cancelled = true;
                    st.waiters.clear();
                    st.live.clear();
                    st.conns.drain().map(|(_, c)| c).collect()
                };
                for c in conns {
                    tokio::spawn(async move { c.kill().await });
                }
                self.emit(run, ev("run.cancelled", "user", "取消这个目标"));
                self.set_status(run, "cancelled");
                run.wake.notify_one();
            }
            "retry" => {
                {
                    let mut st = run.st.lock().unwrap();
                    if st.accept_risk {
                        bail!("已经接受部分结果，不能再重试");
                    }
                    let t = Run::task_mut(&mut st, task).context("task missing")?;
                    if !matches!(t.status, "failed" | "cancelled") {
                        bail!("{task} 不在失败状态");
                    }
                    t.status = "pending";
                    t.active_attempt = None;
                    // A retry the person asks for gets a full attempt budget again.
                    t.retry_base = t.attempts;
                }
                self.set_status(run, "running");
                self.emit(
                    run,
                    ev("task.reopened", "user", format!("要求重试 {task}"))
                        .task(task)
                        .data(json!({ "reason": "user.retry" })),
                );
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "message" | "interrupt" | "takeover" | "release" | "finish" => {
                let agent = a["agent"].as_str().unwrap_or("");
                let cmd = match kind {
                    "message" => {
                        let text = a["text"].as_str().unwrap_or("").trim().to_string();
                        if text.is_empty() {
                            bail!("消息不能为空");
                        }
                        // Shown the moment it is sent; its session routes it once it can.
                        let to = if agent.is_empty() {
                            run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == task).map(|t| t.agent.clone()).unwrap_or_default()
                        } else {
                            agent.to_string()
                        };
                        Steer::Message(self.post_message(run, "user", &to, task, &text))
                    }
                    "interrupt" => Steer::Interrupt,
                    "takeover" => Steer::TakeOver,
                    "release" => Steer::Release,
                    _ => Steer::Finish,
                };
                let tx = {
                    let st = run.st.lock().unwrap();
                    let key = if task.is_empty() {
                        st.live_agent.get(agent).cloned().unwrap_or_default()
                    } else {
                        task.to_string()
                    };
                    st.live.get(&key).cloned()
                };
                match (tx, cmd) {
                    (Some(tx), cmd) => tx.send(cmd).map_err(|_| anyhow!("这个会话已经结束"))?,
                    // No open session: keep the message for this agent's next one.
                    (None, Steer::Message(m)) if !agent.is_empty() => {
                        self.route_message(run, agent, "", 0, &m.id, "next");
                        run.st.lock().unwrap().notes.entry(agent.to_string()).or_default().push(m);
                    }
                    (None, _) => bail!(
                        "{} 现在没有打开的会话",
                        if agent.is_empty() { task } else { agent }
                    ),
                }
            }
            "acceptRisk" => {
                let skipped = {
                    let mut st = run.st.lock().unwrap();
                    if st
                        .tasks
                        .iter()
                        .any(|t| matches!(t.status, "running" | "cancelling"))
                    {
                        bail!("仍有 Attempt 正在运行，清理完成后才能接受部分结果");
                    }
                    if st.accept_risk {
                        bail!("已经接受部分结果");
                    }
                    let mut skipped = vec![];
                    for t in st.tasks.iter_mut() {
                        if !terminal_task_status(t.status) {
                            t.status = "skipped_by_decision";
                            skipped.push(t.def.id.clone());
                        }
                    }
                    st.accept_risk = true;
                    skipped
                };
                for id in &skipped {
                    self.emit(
                        run,
                        ev(
                            "task.skipped_by_decision",
                            "user",
                            format!("用户接受风险，跳过 {id}"),
                        )
                        .task(id)
                        .data(json!({ "reason": a["reason"], "acceptedRisk": true })),
                    );
                }
                // The driver delivers the finished part and stops (see execute).
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "accept" => {
                self.emit(run, ev("risk.accepted", "user", "核对证据后验收通过"));
                self.set_status(run, "accepted");
            }
            _ => bail!("未知操作：{kind}"),
        }
        Ok(())
    }

    pub fn accept_archived(&self, run_id: &str) -> Result<()> {
        let status = self
            .db
            .runs()?
            .into_iter()
            .find(|r| r["id"] == run_id)
            .map(|r| r["status"].clone())
            .context("run not found")?;
        if status != "delivered" {
            bail!("只有已交付的 Run 可以验收");
        }
        let seq = self.db.max_seq(run_id)? + 1;
        self.store(Event {
            run_id: run_id.into(),
            seq,
            ts: now_ms(),
            kind: "risk.accepted".into(),
            actor: "user".into(),
            to: None,
            task: None,
            attempt: None,
            text: "核对证据后验收通过".into(),
            data: None,
        });
        self.db.set_run_status(run_id, "accepted")
    }

    /// After a restart nothing is running: close orphaned attempts and mark their runs interrupted.
    /// Bring an interrupted run back from its event log: finished tasks stay finished, unfinished
    /// ones start again (reopening their old session when possible), and planning resumes where it was.
    // @project-doc docs/domains/run_lifecycle.md#recovery
    pub async fn resume_run(self: &Arc<Self>, run_id: &str) -> Result<Arc<Run>> {
        if let Some(run) = self.run(run_id) {
            return Ok(run);
        }
        let row = self
            .db
            .runs()?
            .into_iter()
            .find(|r| r["id"] == run_id)
            .context("run not found")?;
        if !matches!(row["status"].as_str(), Some("interrupted") | Some("failed")) {
            bail!("只有中断或失败的任务可以继续");
        }
        let events = self.db.events(run_id, 0)?;
        let goal_mode = events
            .iter()
            .find(|e| e.kind == "run.created")
            .and_then(|e| e.data.as_ref())
            .is_some_and(|d| d["mode"] == "goal");
        if !goal_mode {
            bail!("这是早期「先出计划再执行」模式的 Run，这种模式已经移除，只能查看记录。请用同样的目标新建一个 Run");
        }
        let config = &row["config"];
        let mut team: Vec<Member> =
            serde_json::from_value(config["team"].clone()).context("这个任务的团队记录无法读取")?;
        let roles = self.roles()?;
        for m in team.iter_mut() {
            let role = roles.iter().find(|r| {
                (!m.role_id.is_empty() && r["id"] == m.role_id.as_str())
                    || r["name"] == m.name.as_str()
            });
            if let Some(r) = role {
                m.mcp_env = self.role_secrets_of(r["id"].as_str().unwrap_or(""));
            }
            self.spec_for(&m.runtime)?;
        }
        let base_ref = config["baseRef"].as_str().unwrap_or("").to_string();
        let owner = format!("janus-{}", std::process::id());
        if self
            .db
            .acquire_lease(run_id, "run", &owner, now_ms(), 10 * 60 * 1000)?
            .is_none()
        {
            bail!("Run 已被另一个 Janus 实例接管");
        }
        let run = Arc::new(Run {
            id: run_id.to_string(),
            goal: row["goal"].as_str().unwrap_or("").to_string(),
            cwd: PathBuf::from(row["cwd"].as_str().unwrap_or("")),
            team,
            git: config["git"] == true && !base_ref.is_empty(),
            base_ref,
            seq: AtomicI64::new(self.db.max_seq(run_id)?),
            deadline_ms: AtomicI64::new(now_ms() + RUN_BUDGET_MS),
            st: StdMutex::new(RunState {
                status: "running".into(),
                ..Default::default()
            }),
            wake: Notify::new(),
            driving: Default::default(),
        });

        // The board's tasks are every orchestration the lead got accepted.
        let board: Vec<Value> = events
            .iter()
            .filter(|e| e.kind == "orchestration.accepted")
            .filter_map(|e| e.data.as_ref().and_then(|d| d["tasks"].as_array().cloned()))
            .flatten()
            .collect();
        let (mut done, mut open) = (0, 0);
        let mut tasks = vec![];
        for item in board {
            let mut def: TaskDef = serde_json::from_value(item.clone())?;
            def.assignee = item["agent"].as_str().unwrap_or("").to_string();
            let id = def.id.clone();
            let agent = def.assignee.clone();
            let mine = |kind: &str| -> Vec<&Event> {
                events
                    .iter()
                    .filter(|e| e.kind == kind && e.task.as_deref() == Some(id.as_str()))
                    .collect()
            };
            let attempts = mine("attempt.started").len() as i64;
            // The current retry budget starts at the last user retry.
            let retry_base = events
                .iter()
                .rposition(|e| e.kind == "task.reopened" && e.task.as_deref() == Some(id.as_str()))
                .map(|j| {
                    events[..j]
                        .iter()
                        .filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id.as_str()))
                        .count() as i64
                })
                .unwrap_or(0);
            // Only an attempt that was still open when the orchestrator stopped is reloaded;
            // a finished one (failed or succeeded) starts a fresh session.
            let interrupted_attempt = events
                .iter()
                .rposition(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id.as_str()))
                .is_some_and(|j| {
                    !events[j + 1..].iter().any(|e| {
                        e.task.as_deref() == Some(id.as_str())
                            && matches!(
                                e.kind.as_str(),
                                "attempt.failed" | "task.succeeded" | "task.reopened" | "attempt.late_result"
                            )
                    })
                });
            let succeeded = events
                .iter()
                .filter(|e| {
                    e.task.as_deref() == Some(id.as_str())
                        && matches!(e.kind.as_str(), "task.succeeded" | "task.reopened")
                })
                .last()
                .filter(|e| e.kind == "task.succeeded");
            let result = mine("result.submitted").last().and_then(|e| e.data.clone()).map(|d| {
                let strings = |v: &Value| {
                    v.as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default()
                };
                TaskResult {
                    summary: d["summary"].as_str().unwrap_or("").to_string(),
                    evidence: strings(&d["evidence"]),
                    risks: strings(&d["risks"]),
                    ..Default::default()
                }
            });
            let artifacts: Vec<Value> = mine("artifact.created").iter().filter_map(|e| e.data.clone()).collect();
            let last_session = mine("attempt.started")
                .last()
                .and_then(|e| e.data.as_ref().and_then(|d| d["session"].as_str().map(str::to_owned)));
            let last_task_event = events
                .iter()
                .filter(|e| e.task.as_deref() == Some(id.as_str()))
                .filter(|e| {
                    matches!(
                        e.kind.as_str(),
                        "task.succeeded" | "task.failed" | "task.blocked" | "task.skipped_by_decision" | "task.reopened"
                    )
                })
                .last()
                .map(|e| e.kind.as_str());
            let status = match last_task_event {
                Some("task.succeeded") => "succeeded",
                Some("task.failed") => "failed",
                Some("task.blocked") => "blocked",
                Some("task.skipped_by_decision") => "skipped_by_decision",
                _ => "pending",
            };
            // What a blocked task waits on: its failed dependency.
            let blocked_by = events
                .iter()
                .rev()
                .find(|e| e.task.as_deref() == Some(id.as_str()) && e.kind == "task.blocked")
                .and_then(|e| e.data.as_ref())
                .and_then(|d| d["dependency"].as_str())
                .map(str::to_owned);
            if status == "succeeded" {
                done += 1;
            } else {
                open += 1;
                if status == "pending" && interrupted_attempt {
                    if let Some(sid) = last_session {
                        run.st.lock().unwrap().resume_session.insert(id.clone(), sid);
                    }
                }
            }
            let mut task = Task::new(def, agent);
            task.status = status;
            task.attempts = attempts;
            task.result = result;
            task.blocked_by = blocked_by;
            task.succeeded_seq = succeeded.map(|e| e.seq);
            task.artifacts = artifacts;
            task.retry_base = retry_base;
            tasks.push(task);
        }
        run.st.lock().unwrap().tasks = tasks;
        // Messages that were sent, queued or left for an agent but never reached it survive the
        // restart: every message not delivered and not joined into a turn (steer / prompt).
        {
            let mut st = run.st.lock().unwrap();
            for (i, e) in events.iter().enumerate() {
                let Some(agent) = e.to.as_deref() else {
                    continue;
                };
                let d = e.data.clone().unwrap_or_default();
                if e.kind != "user.message" {
                    continue;
                }
                let text = d["text"].as_str().unwrap_or_default().to_string();
                let later = &events[i + 1..];
                let waiting = match d["id"].as_str() {
                    Some(id) => {
                        let routed = later.iter().rev().find(|x| x.kind == "message.routed" && x.data.as_ref().is_some_and(|x| x["id"] == id));
                        let joined = routed.is_some_and(|x| matches!(x.data.as_ref().unwrap()["delivery"].as_str(), Some("steer" | "prompt")));
                        let delivered = later.iter().any(|x| x.kind == "message.delivered" && x.data.as_ref().is_some_and(|x| x["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id))));
                        (!joined && !delivered).then(|| Msg { id: id.to_string(), text })
                    }
                    // Recorded before messages had ids: only queued and next ones waited.
                    None => {
                        let waited = matches!(d["delivery"].as_str(), Some("next" | "queued"));
                        let delivered = later.iter().any(|x| x.kind == "message.delivered" && x.to.as_deref() == Some(agent));
                        (waited && !delivered).then(|| Msg { id: format!("old-{}", e.seq), text })
                    }
                };
                if let Some(m) = waiting {
                    st.notes.entry(agent.to_string()).or_default().push(m);
                }
            }
        }
        self.runs
            .lock()
            .unwrap()
            .insert(run_id.to_string(), run.clone());
        self.set_status(&run, "running");
        // What each task came back as, so the page rebuilds the same state.
        let restored: Vec<Value> = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .map(|t| json!({ "id": t.def.id, "status": t.status, "blockedBy": t.blocked_by }))
            .collect();
        self.emit(
            &run,
            ev(
                "run.recovered",
                "user",
                format!("从中断处继续：看板上 {done} 个任务已完成，{open} 个任务重新开始，主控重新推进目标"),
            )
            .data(json!({ "phase": "running", "done": done, "open": open, "tasks": restored })),
        );
        self.launch(run.clone(), Phase::Goal(true));
        Ok(run)
    }

    pub fn recover(&self) -> Result<()> {
        for (run_id, task, n) in self.db.orphaned_attempts()? {
            self.db.update_attempt(
                &run_id,
                &task,
                n,
                "interrupted",
                None,
                None,
                Some("编排器重启"),
                Some(now_ms()),
            )?;
        }
        for run_id in self.db.unfinished_runs()? {
            let seq = self.db.max_seq(&run_id)? + 1;
            self.store(Event {
                run_id: run_id.clone(), seq, ts: now_ms(), kind: "run.interrupted".into(), actor: "janus".into(), to: None, task: None, attempt: None,
                text: "编排器重启，进行中的 Agent 会话已关闭。可以从中断处继续，或者直接给某个 Agent 发消息".into(), data: None,
            });
            self.db.set_run_status(&run_id, "interrupted")?;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------
// Prompts and parsing
// ----------------------------------------------------------------------

/// What a member brings beyond the task: its role's instructions and skills, then its provider's prompt.
fn role_extras(m: &Member, workspace: &Path) -> String {
    let mut p = String::new();
    let project = crate::agents::project_skills(workspace);
    if !project.is_empty() {
        let list: Vec<String> = project
            .iter()
            .map(|s| {
                format!(
                    "- {}：{}（{}/SKILL.md）",
                    s["name"].as_str().unwrap_or(""),
                    s["description"].as_str().unwrap_or(""),
                    s["path"].as_str().unwrap_or("")
                )
            })
            .collect();
        p.push_str(&format!(
            "\n\n这个项目自带以下 skill（路径相对于工作目录）。相关时先读取它的 SKILL.md：\n{}",
            list.join("\n")
        ));
    }
    if !m.role_prompt.is_empty() {
        p.push_str(&format!("\n\n角色指令（{}）：\n{}", m.name, m.role_prompt));
    }
    if !m.skills.is_empty() {
        let list: Vec<String> = m
            .skills
            .iter()
            .map(|s| {
                format!(
                    "- {}：{}（完整说明：{}/SKILL.md）",
                    s["name"].as_str().unwrap_or(""),
                    s["description"].as_str().unwrap_or(""),
                    s["path"].as_str().unwrap_or("")
                )
            })
            .collect();
        p.push_str(&format!("\n\n你可以使用以下 skill。当任务与某个 skill 相关时，先读取它的 SKILL.md，再按其中的说明执行：\n{}", list.join("\n")));
    }
    if !m.mcp.is_empty() {
        let names: Vec<&str> = m.mcp.iter().filter_map(|x| x["name"].as_str()).collect();
        p.push_str(&format!(
            "\n\n这个会话已经接入以下 MCP 服务器，可以直接调用它们提供的工具：{}",
            names.join("、")
        ));
    }
    if !m.prompt.is_empty() {
        p.push_str(&format!(
            "\n\n附加指令（来自这个 Agent 的 Provider 设置）：\n{}",
            m.prompt
        ));
    }
    p
}

/// One line per collaborator, as the lead is told about them: duty, agent, model, permission mode.
fn team_lines_vec(run: &Run) -> Vec<String> {
    run.team
        .iter()
        .filter(|m| !m.lead)
        .map(|m| {
            format!(
                "- {}：{}（{}）· 职责：{} · Agent {} · 模型 {} · 权限模式 {}（{}）",
                m.id,
                m.name,
                m.role,
                if m.desc.trim().is_empty() { "未填写" } else { m.desc.trim() },
                if m.runtime_name.is_empty() { &m.kind } else { &m.runtime_name },
                if m.model_label.is_empty() { "Agent 默认" } else { &m.model_label },
                if m.permission.is_empty() { "未声明，工具调用直接执行" } else { &m.permission },
                if m.writable { "按这个模式会修改文件" } else { "按这个模式只读，不会修改文件" }
            )
        })
        .collect()
}

fn team_lines(run: &Run) -> String {
    let lines = team_lines_vec(run);
    if lines.is_empty() {
        "- （没有协作者，所有工作由你自己完成）".into()
    } else {
        lines.join("\n")
    }
}

/// Prefix of a message the lead sends a collaborator, so the session and the record both show who wrote it.
const LEAD_REPLY: &str = "主控回复：";

/// Where this Janus listens, for the MCP URLs handed to agents.
fn janus_addr() -> String {
    crate::config::settings().addr.clone()
}

fn random_token() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        b.copy_from_slice(&Sha256::digest(format!("{}-{}", now_ms(), std::process::id()).as_bytes())[..16]);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The `/goal` text: the user's goal alone (Claude Code judges completion against it), within
/// the agents' length limits.
fn goal_condition(goal: &str) -> String {
    let goal = goal.trim().replace('\n', " ");
    goal.chars().take(3800).collect()
}

/// The agent turned `/goal` down instead of starting on it.
fn goal_refused(text: &str) -> Option<String> {
    [
        "only available in trusted workspaces",
        "can't run while hooks are restricted",
        "Goal condition is limited to",
        "requires goal text of at most",
    ]
    .iter()
    .find(|m| text.contains(**m))
    .map(|_| short(text.trim(), 200))
}

/// The board tools touch nothing but the board: no files, nothing outside this machine. Tools that
/// only read it or post to it are marked read-only so agents do not stop to ask the person about
/// them; the ones that start or steer work are left to the agent's own permission mode.
fn board_read() -> Value {
    json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

fn board_act() -> Value {
    json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

fn lead_tools() -> Value {
    json!([
        {
            "name": "team",
            "description": "查看协作者（职责、Agent、模型、权限模式）和共享看板上所有任务的当前状态。",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": board_read()
        },
        {
            "name": "orchestrate",
            "description": "提交一份编排：一组交给协作者的任务。Janus 先校验（执行者是协作者、id 不重复、依赖存在且无环、验证命令合规），不通过时返回全部错误且不执行任何任务；通过后写入共享看板并按依赖执行。可以多次提交，后提交的任务可以依赖之前提交的任务。",
            "inputSchema": {
                "type": "object",
                "required": ["tasks"],
                "properties": {
                    "tasks": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["id", "title", "instructions", "assignee"],
                            "properties": {
                                "id": { "type": "string", "description": "任务 id，字母数字和 - _，在整个看板里唯一，例如 T1" },
                                "title": { "type": "string" },
                                "instructions": { "type": "string", "description": "给执行者的完整说明：背景、要做什么、注意什么。执行者看不到你的对话。" },
                                "assignee": { "type": "string", "description": "协作者 id" },
                                "deps": { "type": "array", "items": { "type": "string" }, "description": "必须先成功完成的任务 id" },
                                "write": { "type": "boolean", "description": "这个任务会修改文件" },
                                "accept": { "type": "array", "items": { "type": "string" }, "description": "可核对的验收标准" },
                                "commands": { "type": "array", "items": { "type": "string" }, "description": "完成后由 Janus 执行的验证命令（只读命令、git diff/status/log/show、cargo test/check、npm/pnpm test、python3 -m …）" }
                            }
                        }
                    }
                }
            },
            "annotations": board_act()
        },
        {
            "name": "wait",
            "description": "从共享看板取回发给你的新消息：任务开始、完成（摘要、证据、风险、产物）、失败，协作者写回的进展和阻塞，以及协作者的提问。有新消息立即返回，否则最多等 timeout_sec 秒。",
            "inputSchema": { "type": "object", "properties": { "timeout_sec": { "type": "integer", "minimum": 1, "maximum": 45 } } },
            "annotations": board_read()
        },
        {
            "name": "reply",
            "description": "回答协作者在某个任务里的提问，或给正在执行的协作者补充指示。",
            "inputSchema": {
                "type": "object",
                "required": ["task", "text"],
                "properties": { "task": { "type": "string" }, "text": { "type": "string" } }
            },
            "annotations": board_act()
        }
    ])
}

fn member_tools() -> Value {
    json!([{
        "name": "board_post",
        "description": "执行任务期间，把进展、阻塞或需要主控注意的事写到共享看板，主控会看到。需要主控做决定才能继续时，不要在这里等：在结果 JSON 的 question 字段提问并结束本轮。",
        "inputSchema": {
            "type": "object",
            "required": ["text"],
            "properties": {
                "kind": { "type": "string", "enum": ["progress", "blocker", "note"] },
                "text": { "type": "string" }
            }
        },
        "annotations": board_read()
    }])
}

/// Check an orchestration the lead submitted, against the team and the tasks already on the
/// board. Every problem is reported at once, so one round of fixes is enough.
// @project-doc docs/interfaces/board_mcp.md#orchestration_validation
fn validate_orchestration(run: &Run, existing: &[String], args: &Value) -> std::result::Result<Vec<TaskDef>, Vec<String>> {
    let mut errors = vec![];
    let Some(items) = args["tasks"].as_array().filter(|a| !a.is_empty()) else {
        return Err(vec!["tasks 必须是非空数组".into()]);
    };
    let members: Vec<&str> = run.team.iter().filter(|m| !m.lead).map(|m| m.id.as_str()).collect();
    let mut ids: Vec<String> = vec![];
    let mut defs = vec![];
    for (i, t) in items.iter().enumerate() {
        let id = t["id"].as_str().unwrap_or("").trim().to_string();
        let label = if id.is_empty() { format!("第 {} 个任务", i + 1) } else { format!("任务 {id}") };
        if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            errors.push(format!("{label}：id 只能用字母、数字、- 和 _，最长 32 个字符"));
        } else if existing.contains(&id) || ids.contains(&id) {
            errors.push(format!("{label}：id 和看板上已有的任务重复"));
        }
        let title = t["title"].as_str().unwrap_or("").trim().to_string();
        let instructions = t["instructions"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            errors.push(format!("{label}：缺少 title"));
        }
        if instructions.is_empty() {
            errors.push(format!("{label}：缺少 instructions，执行者看不到你的对话，必须写清要做什么"));
        }
        let assignee = t["assignee"].as_str().unwrap_or("").trim().to_string();
        if !members.contains(&assignee.as_str()) {
            errors.push(format!(
                "{label}：执行者 `{assignee}` 不是协作者，可选：{}",
                if members.is_empty() { "（没有协作者）".to_string() } else { members.join("、") }
            ));
        }
        let strings = |v: &Value| -> Vec<String> {
            v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
        };
        let commands = strings(&t["commands"]);
        for c in &commands {
            if let Err(reason) = verification_argv(c) {
                errors.push(format!("{label}：验证命令 `{c}` 不能使用：{reason}"));
            }
        }
        let write = t["write"].as_bool().unwrap_or(false);
        ids.push(id.clone());
        defs.push(TaskDef {
            id,
            kind: if write { "implement".into() } else { "explore".into() },
            title,
            desc: instructions,
            deps: strings(&t["deps"]),
            write,
            accept: strings(&t["accept"]),
            assignee,
            max_attempts: MAX_ATTEMPTS,
            verification: VerificationPolicy { commands },
        });
    }
    for d in &defs {
        for dep in &d.deps {
            if dep == &d.id {
                errors.push(format!("任务 {}：不能依赖自己", d.id));
            } else if !ids.contains(dep) && !existing.contains(dep) {
                errors.push(format!("任务 {}：依赖 {dep} 不存在", d.id));
            }
        }
    }
    // Only the new tasks can form a cycle: the board's earlier tasks never depend on them.
    let mut indegree: HashMap<&str, usize> = defs.iter().map(|d| (d.id.as_str(), 0)).collect();
    for d in &defs {
        for dep in d.deps.iter().filter(|x| ids.contains(x)) {
            let _ = dep;
            *indegree.get_mut(d.id.as_str()).unwrap() += 1;
        }
    }
    let mut queue: Vec<&str> = indegree.iter().filter(|(_, n)| **n == 0).map(|(id, _)| *id).collect();
    let mut seen = 0;
    while let Some(id) = queue.pop() {
        seen += 1;
        for d in defs.iter().filter(|d| d.deps.iter().any(|x| x == id)) {
            let n = indegree.get_mut(d.id.as_str()).unwrap();
            *n -= 1;
            if *n == 0 {
                queue.push(d.id.as_str());
            }
        }
    }
    if seen != defs.len() && errors.is_empty() {
        errors.push("依赖之间有环".into());
    }
    if errors.is_empty() { Ok(defs) } else { Err(errors) }
}

/// One board event as the lead reads it in `wait`, or nothing if it is not for the lead.
// @project-doc docs/interfaces/board_mcp.md#wait
fn board_entry(run: &Run, e: &Event) -> Option<String> {
    let task = e.task.as_deref()?;
    let d = e.data.clone().unwrap_or_default();
    let who = |agent: &str| run.member(agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.to_string());
    let (agent, title) = {
        let st = run.st.lock().unwrap();
        let t = st.tasks.iter().find(|t| t.def.id == task)?;
        (t.agent.clone(), t.def.title.clone())
    };
    let name = who(&agent);
    Some(match e.kind.as_str() {
        "attempt.started" => format!("- [开始] {task}「{title}」· {name} 第 {} 次尝试", e.attempt.unwrap_or(1)),
        "task.succeeded" => {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == task)?;
            let r = t.result.clone().unwrap_or_default();
            let arts: Vec<String> = t.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
            format!(
                "- [完成] {task}「{title}」· {name}：{}{}{}{}",
                r.summary,
                if r.evidence.is_empty() { String::new() } else { format!("\n  证据：{}", r.evidence.join("；")) },
                if r.risks.is_empty() { String::new() } else { format!("\n  风险：{}", r.risks.join("；")) },
                if arts.is_empty() { String::new() } else { format!("\n  产物：{}", arts.join("、")) }
            )
        }
        "attempt.failed" => format!("- [尝试失败] {task}「{title}」· {name} 第 {} 次：{}", e.attempt.unwrap_or(1), d["error"].as_str().unwrap_or(&e.text)),
        "task.failed" => format!("- [失败] {task}「{title}」· {name} 用完了尝试次数：{}。请决定重新编排、换人，还是自己做。", d["error"].as_str().unwrap_or(&e.text)),
        "task.blocked" => format!("- [无法开始] {task}「{title}」：依赖 {} 失败", d["dependency"].as_str().unwrap_or("?")),
        "board.posted" => {
            let label = match d["kind"].as_str() { Some("blocker") => "阻塞", Some("note") => "备注", _ => "进展" };
            format!("- [{label}] {task} · {name}：{}", d["text"].as_str().unwrap_or(&e.text))
        }
        "question.asked" => {
            let options: Vec<String> = d["options"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            format!(
                "- [提问] {task} · {name} 停下来等你回答：{}{}\n  用 janus.reply 回答（task={task}）。",
                d["question"].as_str().unwrap_or(&e.text),
                if options.is_empty() { String::new() } else { format!("（选项：{}）", options.join(" / ")) }
            )
        }
        "approval.requested" | "input.requested" => format!("- [等用户] {task} · {name} 在等用户处理：{}", e.text),
        _ => return None,
    })
}

/// The lead's last message as the run's summary, without a trailing JSON block if it left one.
fn lead_summary(text: &str) -> String {
    let text = match text.rfind("```json") {
        Some(i) => &text[..i],
        None => text,
    };
    short(text.trim(), 2000)
}

/// Agents that can lead: they pursue a goal on their own with `/goal`.
pub fn lead_capable(kind: &str) -> bool {
    // Tests lead with the scripted custom agent.
    matches!(kind, "claude" | "codex") || (cfg!(test) && kind == "custom")
}

fn git_out(cwd: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// What the working tree differs from HEAD by, untracked files included.
fn working_diff(cwd: &Path) -> String {
    let mut diff = git_out(cwd, &["diff", "HEAD"]);
    let untracked = git_out(cwd, &["ls-files", "--others", "--exclude-standard"]);
    if !untracked.trim().is_empty() {
        diff.push_str(&format!("\n# 未跟踪的新文件\n{untracked}"));
    }
    diff
}

fn task_prompt(
    run: &Run,
    m: &Member,
    def: &TaskDef,
    workspace: &Path,
    upstream: &[String],
    last_error: Option<&str>,
) -> String {
    let mut p = format!(
        "你是 Janus 团队中的 {name}（{role}），主控把一个任务交给了你。

总目标：{goal}
你的任务 {id}：{title}
说明：{desc}
最多尝试 {max_attempts} 次。完成后 Janus 会执行这些验证命令：[{commands}]

验收标准：
{accept}

工作目录：{ws}
{wsnote}
约束：不要 git push、不要部署、不要修改工作目录之外的文件。",
        name = m.name,
        role = m.role,
        goal = run.goal,
        id = def.id,
        title = def.title,
        desc = def.desc,
        max_attempts = def.max_attempts,
        commands = def.verification.commands.join(", "),
        accept = def.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"),
        ws = workspace.display(),
        wsnote = if def.write {
            "你直接在项目的工作目录中工作，改动立即生效，其他成员也会看到。不要 git commit 或 push。"
        } else {
            "你直接在项目的工作目录中工作。这个任务只需要阅读和分析，不要修改文件。"
        },
    );
    if !upstream.is_empty() {
        p.push_str(&format!("\n\n上游任务的结果：\n{}", upstream.join("\n")));
    }
    if let Some(err) = last_error {
        p.push_str(&format!("\n\n上一次尝试失败：{err}。请避开同样的问题。"));
    }
    p.push_str(
        "\n\n如果缺少超出你授权的关键决定，不要猜：在结果 JSON 的 question 字段写清问题和选项，然后结束本轮，主控会回答你。
完成后，回复的最后必须是一个 ```json 代码块：
{\"summary\":\"一句话结果\",\"evidence\":[\"可核对的证据，例如运行的命令和输出\"],\"risks\":[\"已知风险，没有就留空数组\"],\"question\":null}
question 的格式：{\"text\":\"问题\",\"options\":[\"选项 A\",\"选项 B\"],\"recommend\":\"你建议的选项\"}",
    );
    p.push_str(&role_extras(m, workspace));
    p
}

fn extract_json(text: &str) -> Option<Value> {
    let mut rest = text;
    let mut last = None;
    while let Some(start) = rest.find("```json") {
        let body = &rest[start + 7..];
        let Some(end) = body.find("```") else { break };
        if let Ok(v) = serde_json::from_str::<Value>(body[..end].trim()) {
            last = Some(v);
        }
        rest = &body[end + 3..];
    }
    if last.is_some() {
        return last;
    }
    let close = text.rfind('}')?;
    text.match_indices('{').rev().find_map(|(i, _)| {
        serde_json::from_str::<Value>(&text[i..=close])
            .ok()
            .filter(Value::is_object)
    })
}

/// Programs a verification command may run without side effects; also allowed inside a pipeline.
const READ_ONLY_COMMANDS: &[&str] = &["pwd", "cat", "rg", "sed", "nl", "find", "sort"];

/// Split a command at the pipes that are shell syntax, refusing any other shell syntax. Quotes
/// are respected: `rg '(a|b)'` is one read-only command with a regex, not a pipeline or a
/// subshell. Inside double quotes `$` and `` ` `` still expand, so they stay refused there.
fn pipeline_segments(command: &str) -> std::result::Result<Vec<String>, String> {
    let (mut segments, mut cur, mut quote) = (vec![], String::new(), None::<char>);
    for c in command.chars() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                }
            }
            Some(_) => match c {
                '"' => quote = None,
                '$' | '`' => return Err(format!("包含不允许的 shell 语法 `{c}`")),
                _ => {}
            },
            None => match c {
                '\'' | '"' => quote = Some(c),
                '|' => {
                    segments.push(std::mem::take(&mut cur));
                    continue;
                }
                ';' | '&' | '>' | '<' | '$' | '`' | '\n' | '(' | ')' | '\\' => {
                    return Err(format!("包含不允许的 shell 语法 `{c}`"))
                }
                _ => {}
            },
        }
        cur.push(c);
    }
    if quote.is_some() {
        return Err("引号没有闭合".into());
    }
    segments.push(cur);
    Ok(segments)
}

/// Check a verification command and turn it into (program, args, pipeline). Pipelines of
/// read-only programs run through `sh -c`; any other shell syntax is refused, since it would let
/// a read-only-looking command run something else.
// @project-doc docs/interfaces/board_mcp.md#verification_commands
fn verification_argv(command: &str) -> std::result::Result<(String, Vec<String>, bool), String> {
    let segments = pipeline_segments(command)?;
    let argv = |segment: &str| -> std::result::Result<Vec<String>, String> {
        let mut parts =
            crate::agents::split_command(segment.trim()).map_err(|e| format!("无法解析：{e}"))?;
        while parts
            .first()
            .is_some_and(|part| part.len() > 2 && part.contains('=') && !part.starts_with('-'))
        {
            parts.remove(0);
        }
        if parts.is_empty() {
            return Err("命令为空".into());
        }
        Ok(parts)
    };
    let read_only = |parts: &[String]| -> std::result::Result<(), String> {
        let program = parts[0].as_str();
        if !READ_ONLY_COMMANDS.contains(&program) {
            return Err(format!("{program} 不在只读命令列表"));
        }
        let writes = match program {
            "sed" => parts
                .iter()
                .any(|p| p.starts_with("-i") || p == "--in-place" || p.starts_with("--in-place=")),
            "find" => parts.iter().any(|p| {
                matches!(
                    p.as_str(),
                    "-delete"
                        | "-exec"
                        | "-execdir"
                        | "-ok"
                        | "-okdir"
                        | "-fprint"
                        | "-fprintf"
                        | "-fls"
                )
            }),
            "sort" => parts.iter().any(|p| p == "-o" || p.starts_with("--output")),
            _ => false,
        };
        if writes {
            return Err(format!("{program} 的这个用法会写文件"));
        }
        Ok(())
    };
    if segments.len() > 1 {
        for segment in &segments {
            read_only(&argv(segment)?)?;
        }
        return Ok(("sh".into(), vec!["-c".into(), command.to_string()], true));
    }
    let parts = argv(command)?;
    let first_arg = parts.get(1).map(String::as_str).unwrap_or("");
    let allowed = match parts[0].as_str() {
        "git" => matches!(first_arg, "diff" | "status" | "log" | "show"),
        "cargo" => {
            matches!(first_arg, "test" | "check")
                || (first_arg == "fmt" && parts.iter().any(|p| p == "--check"))
        }
        "npm" | "pnpm" => first_arg == "test",
        "python3" | "python" => first_arg == "-m",
        _ => {
            read_only(&parts)?;
            true
        }
    };
    if !allowed {
        return Err(format!("{} {first_arg} 不在允许列表", parts[0]));
    }
    let mut parts = parts.into_iter();
    let program = parts.next().unwrap_or_default();
    Ok((program, parts.collect(), false))
}

/// The person ended the attempt from the session: keep a structured result if the agent gave one.
fn finished_by_user(text: &str) -> TaskResult {
    let mut r = parse_result(text);
    r.question = None;
    if extract_json(text).is_none() {
        r.risks = vec!["由你在会话中直接结束，Agent 没有给出结构化结果".into()];
    }
    if r.summary.trim().is_empty() {
        r.summary = "由你在会话中结束任务".into();
    }
    r.evidence.push("人工接管会话中的对话（见执行轨迹）".into());
    r
}

// @project-doc docs/domains/run_lifecycle.md#structured_result
fn parse_result(text: &str) -> TaskResult {
    let strings = |v: &Value| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .filter(|s| !s.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    match extract_json(text) {
        Some(v) => TaskResult {
            summary: v["summary"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| first_line(text)),
            evidence: strings(&v["evidence"]),
            risks: strings(&v["risks"]),
            question: Some(v["question"].clone())
                .filter(|q| q.is_object() || q.as_str().is_some_and(|s| !s.is_empty())),
        },
        None => TaskResult {
            summary: first_line(text),
            risks: vec!["Agent 没有按约定返回结构化结果，摘要取自回复第一行".into()],
            ..Default::default()
        },
    }
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

/// Open a session with the role's stdio MCP servers, in ACP's `McpServer` shape.
fn mcp_servers(m: &Member) -> Vec<Value> {
    m.mcp
        .iter()
        .map(|s| {
            let env = &m.mcp_env[s["name"].as_str().unwrap_or("")];
            json!({
                "name": s["name"], "command": s["command"], "args": s["args"],
                "env": env.as_object().map(|e| e.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect()
}

/// Swallow the history a `session/load` replays. Updates may trail the response, so wait until
/// the agent has been quiet for a moment; anything it asks meanwhile is declined.
async fn drain_replay(conn: &AcpConn, rx: &mut mpsc::UnboundedReceiver<Incoming>) {
    let quiet = Duration::from_millis(500);
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < until {
        match tokio::time::timeout(quiet, rx.recv()).await {
            Ok(Some(Incoming::Notification { .. })) => {}
            Ok(Some(Incoming::Request { id, .. })) => {
                let _ = conn.respond_error(id, "session is still loading").await;
            }
            Ok(Some(Incoming::Closed)) | Ok(None) | Err(_) => break,
        }
    }
}

async fn new_session(conn: &AcpConn, cwd: &Path, m: &Member, extra: &[Value], system: Option<&str>) -> Result<Value> {
    let mut servers = mcp_servers(m);
    servers.extend_from_slice(extra);
    let mut meta = session_meta(&format!("janus-{}", m.id), &session_values(m));
    // Claude Code takes an appended system prompt; other agents ignore it.
    if let Some(text) = system {
        meta["systemPrompt"] = json!({ "append": text });
    }
    let r = conn
        .request(
            "session/new",
            json!({ "cwd": cwd, "mcpServers": servers, "_meta": meta }),
        )
        .await?;
    r["sessionId"]
        .as_str()
        .context("session/new 没有返回 sessionId")?;
    Ok(r)
}

/// Lody's session seed: some agents (Grok) only take a permission mode when the session starts,
/// and scope it to a stable client identifier. Agents that do not read it ignore it.
// @project-doc docs/interfaces/acp_integration.md#session_meta
pub fn session_meta(client: &str, values: &Value) -> Value {
    json!({
        "clientIdentifier": client,
        "lody": { "sessionConfig": { "version": 1, "configOptionValues": values } },
    })
}

/// The reply to an agent request Janus cannot put in front of anyone: a session that is closing,
/// or a connection check.
fn is_user_request(method: &str) -> bool {
    matches!(method, "session/request_permission" | "elicitation/create")
}

pub fn cancelled_reply(method: &str) -> Value {
    match method {
        "elicitation/create" => json!({ "action": "cancel" }),
        _ => json!({ "outcome": { "outcome": "cancelled" } }),
    }
}

/// The control values a session of this member opens with: exactly what its role chose. Read
/// tasks get no override; they run in a disposable snapshot whatever the mode.
fn session_values(m: &Member) -> Value {
    match &m.session {
        Value::Object(o) => Value::Object(o.clone()),
        _ => json!({}),
    }
}

/// Apply the role's model, effort and session controls (permission mode and whatever else its
/// agent declares). Returns what was applied, so the attempt records the exact settings it ran with.
// @project-doc docs/interfaces/acp_integration.md#session_setup
async fn configure_session(
    conn: &AcpConn,
    session: &Value,
    m: &Member,
) -> Result<Value> {
    let sid = session["sessionId"].as_str().unwrap_or_default();
    let mut opts = crate::agents::session_options(session);
    let mut applied = json!({ "controls": {} });
    for (cat, want, required) in [
        ("model", m.model.as_str(), true),
        ("effort", m.effort.as_str(), false),
    ] {
        let opt = opts[cat].clone();
        if opt.is_null() {
            continue;
        }
        let current = opt["current"].as_str().unwrap_or_default();
        if want.is_empty() || want == current {
            applied[cat] = json!(current);
            continue;
        }
        if !opt["options"]
            .as_array()
            .is_some_and(|o| o.iter().any(|x| x["value"] == want))
        {
            if required {
                bail!(
                    "{} 不提供模型 {want}，请在角色设置中重新选择",
                    m.runtime_name
                );
            }
            applied[cat] = json!(current);
            continue;
        }
        match conn
            .request(
                "session/set_config_option",
                json!({ "sessionId": sid, "configId": opt["id"], "value": want }),
            )
            .await
        {
            Ok(r) => {
                applied[cat] = json!(want);
                // A model switch can change what else is on offer (Claude Code drops Auto on some models).
                if r["configOptions"].is_array() {
                    opts = crate::agents::session_options(&r);
                }
            }
            Err(err) if required => return Err(err.context(format!("设置模型 {want} 失败"))),
            Err(_) => applied[cat] = json!(current),
        }
    }
    let wants = session_values(m);
    for c in opts["controls"].as_array().cloned().unwrap_or_default() {
        let id = c["id"].as_str().unwrap_or_default();
        let current = c["current"].clone();
        let want = wants.get(id).cloned().unwrap_or(Value::Null);
        let offered = match &want {
            Value::Bool(_) => c["type"] == "boolean",
            Value::String(_) => c["options"]
                .as_array()
                .is_some_and(|o| o.iter().any(|x| x["value"] == want)),
            _ => false,
        };
        if want.is_null() || want == current || !offered {
            if c["permission"] == true || !want.is_null() {
                applied["controls"][id] = current;
            }
            continue;
        }
        let res = if c["via"] == "mode" {
            conn.request(
                "session/set_mode",
                json!({ "sessionId": sid, "modeId": want }),
            )
            .await
        } else {
            conn.request(
                "session/set_config_option",
                json!({ "sessionId": sid, "configId": id, "value": want }),
            )
            .await
        };
        applied["controls"][id] = if res.is_ok() { want } else { current };
    }
    if let Some(mode) = opts["controls"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["category"] == "mode"))
    {
        applied["mode"] = applied["controls"][mode["id"].as_str().unwrap_or_default()].clone();
    }
    applied["label"] = json!(applied_label(&opts, &applied["controls"]));
    Ok(applied)
}

/// "Agent · Plan 关闭": the permission controls a session actually runs with, by display name.
fn applied_label(opts: &Value, values: &Value) -> String {
    let conn = json!({ "options": opts });
    crate::agents::session_label(values, &conn)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else if src.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub async fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What limits this session, as shown with the attempt: only the agent's own permission mode.
fn enforcement(applied: &Value) -> String {
    let mode = match applied["label"].as_str() {
        Some(l) if !l.is_empty() => format!("权限 {l}"),
        _ => "Agent 没有权限模式".into(),
    };
    format!("{mode}：直接在工作目录中执行，能做什么由 Agent 的权限模式决定")
}

fn summarize_input(raw: &Value, title: &str) -> String {
    let s = match raw {
        Value::Null => title.to_string(),
        Value::String(s) => s.clone(),
        v => match &v["command"] {
            Value::Array(parts) => parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
            Value::String(s) => s.clone(),
            _ => v
                .get("path")
                .or(v.get("file_path"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string()),
        },
    };
    short(&s, 240)
}

/// Codex reports command output as a JSON envelope; show the text a person would read.
fn unwrap_output(s: &str) -> String {
    match serde_json::from_str::<Value>(s) {
        Ok(v) if v.is_object() => {
            let text = v
                .get("formatted_output")
                .or(v.get("output"))
                .or(v.get("stdout"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            match v["exit_code"].as_i64() {
                Some(code) if code != 0 => format!("exit {code} · {text}"),
                _ => text,
            }
        }
        _ => s.to_string(),
    }
}

fn summarize_output(entry: &Value) -> String {
    if let Some(items) = entry["content"].as_array() {
        let parts: Vec<String> = items
            .iter()
            .filter_map(|c| match c["type"].as_str() {
                Some("content") => c["content"]["text"].as_str().map(unwrap_output),
                Some("diff") => Some(format!("修改 {}", c["path"].as_str().unwrap_or("文件"))),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            return short(&parts.join(" · "), 300);
        }
    }
    match &entry["rawOutput"] {
        Value::Null => entry["status"].as_str().unwrap_or("").to_string(),
        Value::String(s) => short(&unwrap_output(s), 300),
        v => short(&unwrap_output(&v.to_string()), 300),
    }
}

pub fn short(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

fn first_line(text: &str) -> String {
    short(
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("（空回复）"),
        120,
    )
}

fn short_sha(s: &str) -> &str {
    &s[..s.len().min(8)]
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn test_run(team_size: usize) -> Run {
        Run {
            id: "r-test".into(),
            goal: "测试目标".into(),
            cwd: PathBuf::from("/tmp"),
            team: (0..team_size)
                .map(|i| Member {
                    id: format!("m{i}"),
                    name: format!("成员{i}"),
                    role: "测试".into(),
                    desc: String::new(),
                    limits: vec![],
                    ws: String::new(),
                    writable: true,
                    runtime: String::new(),
                    lead: i == 0,
                    color: String::new(),
                    init: String::new(),
                    model: String::new(),
                    effort: String::new(),
                    session: json!({}),
                    permission: String::new(),
                    model_label: String::new(),
                    kind: String::new(),
                    runtime_name: String::new(),
                    prompt: String::new(),
                    role_prompt: String::new(),
                    skills: vec![],
                    mcp: vec![],
                    role_id: String::new(),
                    mcp_env: Value::Null,
                })
                .collect(),
            git: false,
            base_ref: String::new(),
            seq: AtomicI64::new(0),
            deadline_ms: AtomicI64::new(now_ms() + RUN_BUDGET_MS),
            st: StdMutex::new(RunState {
                status: "planning".into(),
                ..Default::default()
            }),
            wake: Notify::new(),
            driving: Default::default(),
        }
    }

    #[test]
    fn the_lead_is_told_each_collaborator() {
        let mut run = test_run(2);
        run.team[1].desc = "独立检查变更，寻找风险与反例".into();
        let lines = team_lines(&run);
        assert!(lines.contains("- m1：成员1（测试）· 职责：独立检查变更，寻找风险与反例 · Agent "), "{lines}");
        assert!(!lines.contains("m0"), "the lead is not its own collaborator: {lines}");
    }
}

pub fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}{rest}"),
        _ => p.to_string(),
    }
}

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

/// End-to-end runs against a scripted ACP agent (testdata/fake_acp.py): each test drives one of
/// the paths that used to leave a run stuck, and checks it either delivers or waits somewhere
/// the person can act.
#[cfg(test)]
mod flow_tests {
    use super::*;

    struct Harness {
        app: Arc<App>,
        dir: PathBuf,
        repo: PathBuf,
    }

    fn sh(cwd: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn app_at(dir: &Path) -> Arc<App> {
        let (tx, _) = broadcast::channel(4096);
        Arc::new(App {
            db: Db::open(&dir.join("janus.db")).unwrap(),
            cfg: Config {
                data_dir: dir.join("data"),
                agents_dir: dir.join("agents"),
            },
            tx,
            runs: StdMutex::new(HashMap::new()),
        })
    }

    /// `members`: (role id, writable). The first one leads.
    // @project-doc docs/operations/testing.md#harness
    fn harness(name: &str, members: &[(&str, bool)], script: Value) -> Harness {
        let dir = std::env::temp_dir().join(format!("janus-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (repo, fake) = (dir.join("repo"), dir.join("fake"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("script.json"), script.to_string()).unwrap();
        sh(&repo, &["init", "-q"]);
        std::fs::write(repo.join("README.md"), "test\n").unwrap();
        sh(&repo, &["add", "-A"]);
        sh(&repo, &["commit", "-q", "-m", "init"]);
        let app = app_at(&dir);
        let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_acp.py");
        // What a check of the fake agent reads: whether a member writes follows from its mode.
        let modes: Vec<Value> = ["read-only", "ask", "agent", "full"].iter().map(|v| json!({ "value": v, "name": v })).collect();
        app.db.put_config("connection", "fake", &json!({
            "id": "fake", "kind": "custom", "base": "custom", "name": "Fake", "command": format!("python3 {}", script_path.display()),
            "env": { "FAKE_DIR": fake.to_string_lossy() },
            "options": { "controls": [{ "id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "config", "current": "ask", "permission": true, "options": modes }] }
        }), now_ms()).unwrap();
        for (id, writable) in members {
            app.db.put_config("role", id, &json!({
                "id": id, "name": id, "role": "测试", "connection": "fake",
                "session": if *writable { json!({}) } else { json!({ "mode": "read-only" }) },
            }), now_ms()).unwrap();
        }
        let ms: Vec<Value> = members
            .iter()
            .map(|(id, _)| json!({ "role": id }))
            .collect();
        app.db.put_config("workspace", "ws", &json!({
            "id": "ws", "name": "ws", "path": repo.canonicalize().unwrap(), "lead": members[0].0, "members": ms
        }), now_ms()).unwrap();
        Harness { app, dir, repo }
    }

    fn events(app: &App, run: &str) -> Vec<Event> {
        app.db.events(run, 0).unwrap()
    }

    async fn wait_until(app: &App, run: &str, what: &str, f: impl Fn(&[Event]) -> bool) {
        for _ in 0..600 {
            if f(&events(app, run)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let tail: Vec<String> = events(app, run)
            .iter()
            .rev()
            .take(25)
            .rev()
            .map(|e| format!("#{} {} {:?} {}", e.seq, e.kind, e.task, e.text))
            .collect();
        panic!("timed out waiting for {what}:\n{}", tail.join("\n"));
    }

    fn has(evs: &[Event], kind: &str) -> bool {
        evs.iter().any(|e| e.kind == kind)
    }

    fn status(app: &App, run: &str) -> String {
        app.db
            .runs()
            .unwrap()
            .into_iter()
            .find(|r| r["id"] == run)
            .unwrap()["status"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Start a run and play its lead: submit `tasks` as one orchestration through the board.
    /// The scripted lead keeps working until `finish_lead`.
    async fn start(h: &Harness, tasks: Value) -> String {
        let run = h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into() }).await.unwrap();
        for _ in 0..100 {
            if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": tasks })).await;
        assert!(!err, "{text}");
        run
    }

    fn finish_lead(h: &Harness) {
        std::fs::write(h.dir.join("fake/lead-go"), "目标已完成").unwrap();
    }

    fn act(h: &Harness, run: &str, a: Value) {
        let r = h.app.run(run).unwrap();
        h.app.action(&r, &a).unwrap();
    }

    fn task(id: &str, assignee: &str, extra: Value) -> Value {
        let mut t = json!({ "id": id, "title": id, "instructions": id, "assignee": assignee });
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t
    }

    /// A role's session controls reach the agent (seeded at session/new and set per session),
    /// and the agent's own permission options and forms come back to the person and are
    /// answered in the agent's terms.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_requests_reach_the_person() {
        let h = harness("requests", &[("lead", false), ("m1", false)], json!({ "T1": ["permission"], "T2": ["elicit"] }));
        let mut conn = h.app.db.get_config("connection", "fake").unwrap().unwrap();
        conn["options"]["controls"].as_array_mut().unwrap().push(json!({ "id": "plan_mode", "name": "Plan", "type": "boolean", "via": "config", "current": false, "permission": true, "options": [] }));
        h.app.db.put_config("connection", "fake", &conn, now_ms()).unwrap();
        let mut role = h.app.db.get_config("role", "m1").unwrap().unwrap();
        role["session"] = json!({ "mode": "full", "plan_mode": true });
        h.app.db.put_config("role", "m1", &role, now_ms()).unwrap();
        let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;

        wait_until(&h.app, &run, "permission request", |e| has(e, "approval.requested")).await;
        let req = events(&h.app, &run).into_iter().find(|e| e.kind == "approval.requested").unwrap();
        assert_eq!(req.data.as_ref().unwrap()["options"].as_array().unwrap().len(), 3, "every option the agent offered is shown");
        act(&h, &run, json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T1", "allow": true, "option": "a2", "label": "Always allow" }));

        wait_until(&h.app, &run, "form request", |e| has(e, "input.requested")).await;
        let form = events(&h.app, &run).into_iter().find(|e| e.kind == "input.requested").unwrap();
        assert_eq!(form.data.as_ref().unwrap()["schema"]["required"], json!(["name"]));
        act(&h, &run, json!({ "type": "input", "key": form.data.as_ref().unwrap()["key"], "task": "T2", "action": "accept", "content": { "name": "janus" } }));

        wait_until(&h.app, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2"))).await;
        finish_lead(&h);
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        let evs = events(&h.app, &run);
        let summary = |t: &str| evs.iter().find(|e| e.kind == "result.submitted" && e.task.as_deref() == Some(t)).map(|e| e.data.as_ref().unwrap()["summary"].as_str().unwrap_or_default().to_string()).unwrap_or_default();
        assert_eq!(summary("T1"), "picked a2", "the chosen option, not a generic allow");
        assert_eq!(summary("T2"), "accept janus");
        let started = evs.iter().find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).unwrap();
        assert_eq!(started.data.as_ref().unwrap()["config"]["controls"]["mode"], "full");
        assert_eq!(started.data.as_ref().unwrap()["config"]["controls"]["plan_mode"], true);
        let calls = std::fs::read_to_string(h.dir.join("fake/calls.log")).unwrap();
        assert!(calls.contains(r#"seed {"mode": "full", "plan_mode": true}"#), "{calls}");
        assert!(calls.contains("set mode \"full\"") && calls.contains("set plan_mode true"), "{calls}");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    async fn tool(app: &Arc<App>, run: &str, who: &str, name: &str, args: Value) -> (String, bool) {
        let token = app.run(run).unwrap().st.lock().unwrap().mcp_token.clone();
        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } });
        let r = app.mcp(run, &token, who, req).await.unwrap().unwrap();
        (r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string(), r["result"]["isError"] == true)
    }

    /// Goal mode end to end: the lead (a fake Codex) gets the brief and `/goal`; it orchestrates
    /// through the board tools, which validate before anything runs; collaborators report back
    /// on the board, a question goes to the lead and its answer back into the session; the run
    /// is delivered once the lead is done, with the lead's summary.
    #[tokio::test(flavor = "multi_thread")]
    async fn goal_mode_orchestrates_through_the_board() {
        let h = harness("goal", &[("lead", false), ("m1", true), ("m2", true)], json!({ "T1": ["write:notes.md"], "T2": ["ask", "ok"] }));
        let run = h.app.create_run(NewRun { goal: "写一份笔记".into(), workspace: "ws".into() }).await.unwrap();
        let calls = h.dir.join("fake/calls.log");
        for _ in 0..100 {
            if std::fs::read_to_string(&calls).unwrap_or_default().contains("goal 写一份笔记") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let log = std::fs::read_to_string(&calls).unwrap_or_else(|_| {
            panic!("the lead got nothing: {:#?}", events(&h.app, &run).iter().map(|e| format!("{} {}", e.kind, e.text)).collect::<Vec<_>>())
        });
        assert!(log.contains("brief ok") && log.contains("goal 写一份笔记"), "brief, then /goal: {log}");
        let evs = events(&h.app, &run);
        assert!(evs.iter().any(|e| {
            e.kind == "session.prompt"
                && e.to.as_deref() == Some("lead")
                && e.data.as_ref().is_some_and(|d| d["channel"] == "context")
        }));
        assert!(evs.iter().any(|e| {
            e.kind == "session.prompt"
                && e.to.as_deref() == Some("lead")
                && e.data.as_ref().is_some_and(|d| d["channel"] == "goal")
        }));

        // The board's MCP surface, and its token.
        let token = h.app.run(&run).unwrap().st.lock().unwrap().mcp_token.clone();
        let list = |who: &str| json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string() + who;
        let _ = list;
        let tools = h.app.mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.unwrap().unwrap();
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 4);
        assert!(h.app.mcp(&run, "wrong", "lead", json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await.is_err());
        assert!(h.app.mcp(&run, &token, "lead", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.unwrap().is_none());

        // Validation first: every problem at once, and nothing starts.
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [
            { "id": "T1", "title": "写笔记", "instructions": "写", "assignee": "nobody" },
            { "id": "T2", "title": "定名", "instructions": "定", "assignee": "m2", "deps": ["T9"] },
            { "id": "T3", "title": "查", "instructions": "查", "assignee": "m1", "commands": ["rm -rf build"] }] })).await;
        assert!(err && text.contains("nobody") && text.contains("T9") && text.contains("rm -rf build"), "{text}");
        assert!(h.app.run(&run).unwrap().st.lock().unwrap().tasks.is_empty());

        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [
            { "id": "T1", "title": "写笔记", "instructions": "在 notes.md 写笔记", "assignee": "m1", "write": true },
            { "id": "T2", "title": "定名", "instructions": "给笔记定个名字", "assignee": "m2", "deps": ["T1"] }] })).await;
        assert!(!err, "{text}");
        let evs = events(&h.app, &run);
        assert!(evs.iter().any(|e| {
            e.kind == "tool.started"
                && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.orchestrate")
        }));
        assert!(evs.iter().any(|e| {
            e.kind == "tool.completed"
                && e.data.as_ref().is_some_and(|d| {
                    d["tool"] == "janus.orchestrate" && d["status"] == "completed"
                })
        }));
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "x", "instructions": "x", "assignee": "m1" }] })).await;
        assert!(err && text.contains("重复"), "ids stay unique across orchestrations: {text}");

        // A collaborator writes back while it works.
        let (text, err) = tool(&h.app, &run, "T1", "board_post", json!({ "kind": "progress", "text": "笔记写了一半" })).await;
        assert!(!err, "{text}");
        let evs = events(&h.app, &run);
        assert!(evs.iter().any(|e| {
            e.kind == "tool.started"
                && e.task.as_deref() == Some("T1")
                && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.board_post")
        }));
        assert!(evs.iter().any(|e| {
            e.kind == "tool.completed"
                && e.task.as_deref() == Some("T1")
                && e.data.as_ref().is_some_and(|d| {
                    d["tool"] == "janus.board_post" && d["status"] == "completed"
                })
        }));
        let (_, err) = tool(&h.app, &run, "T1", "orchestrate", json!({ "tasks": [] })).await;
        assert!(err, "collaborators cannot orchestrate");

        // The lead reads the board until the question arrives, answers it, and reads on.
        let mut board = String::new();
        let mut answered = false;
        for _ in 0..60 {
            let (text, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 2 })).await;
            board.push_str(&text);
            board.push('\n');
            if !answered && board.contains("[提问] T2") {
                let (text, err) = tool(&h.app, &run, "lead", "reply", json!({ "task": "T2", "text": "alpha" })).await;
                assert!(!err && text.contains("已回答"), "{text}");
                answered = true;
            }
            if board.contains("[完成] T2") {
                break;
            }
        }
        assert!(board.contains("[进展] T1") && board.contains("[完成] T1"), "{board}");
        assert!(board.contains("按回答 alpha 完成"), "the answer went back into T2's session: {board}");
        let evs = events(&h.app, &run);
        assert!(evs.iter().any(|e| {
            e.kind == "tool.started"
                && e.data.as_ref().is_some_and(|d| d["tool"] == "janus.wait")
        }));
        assert!(evs.iter().any(|e| {
            e.kind == "tool.completed"
                && e.data.as_ref().is_some_and(|d| {
                    d["tool"] == "janus.wait" && d["status"] == "completed"
                })
        }));
        assert!(!has(&evs, "question.escalated"), "questions go to the lead, not the person");
        assert!(has(&evs, "orchestration.rejected") && has(&evs, "orchestration.accepted") && has(&evs, "board.posted"));
        assert!(!has(&evs, "run.delivered"), "not before the lead is done");

        // The lead finishes; the run is delivered with its summary.
        std::fs::write(h.dir.join("fake/lead-go"), "目标已完成：笔记写好了").unwrap();
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        let evs = events(&h.app, &run);
        let delivered = evs.iter().find(|e| e.kind == "run.delivered").unwrap();
        assert!(delivered.data.as_ref().unwrap()["summary"].as_str().unwrap().contains("笔记写好了"));
        assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// The lead can end its goal while a collaborator still waits on a question; nobody reads
    /// the board any more, so the question goes to the person and the run still finishes.
    #[tokio::test(flavor = "multi_thread")]
    async fn questions_left_by_the_lead_go_to_the_person() {
        let h = harness("goal-left", &[("lead", false), ("m1", true)], json!({ "T1": ["ask", "ok"] }));
        let run = h.app.create_run(NewRun { goal: "定个名字".into(), workspace: "ws".into() }).await.unwrap();
        for _ in 0..100 {
            if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "定名", "instructions": "定个名字", "assignee": "m1" }] })).await;
        assert!(!err, "{text}");
        wait_until(&h.app, &run, "question", |e| has(e, "question.asked")).await;
        assert!(!has(&events(&h.app, &run), "question.escalated"), "the lead answers while it is there");

        std::fs::write(h.dir.join("fake/lead-go"), "目标已完成").unwrap();
        wait_until(&h.app, &run, "escalation", |e| has(e, "question.escalated")).await;
        let evs = events(&h.app, &run);
        assert!(!has(&evs, "run.delivered"), "the run waits for the answer");
        let key = evs.iter().find(|e| e.kind == "question.asked").unwrap().data.as_ref().unwrap()["key"].clone();
        act(&h, &run, json!({ "type": "answer", "key": key, "task": "T1", "answer": "beta" }));
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        let evs = events(&h.app, &run);
        assert_eq!(evs.iter().filter(|e| e.kind == "question.escalated").count(), 1);
        assert!(evs.iter().any(|e| e.kind == "result.submitted" && e.text.contains("beta")), "the answer reached T1");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// Goal mode survives a restart: the board's tasks come back as they were, the lead starts
    /// again knowing them, and can orchestrate on top of them.
    #[tokio::test(flavor = "multi_thread")]
    async fn goal_mode_resumes_with_the_board() {
        let h = harness("goal-resume", &[("lead", false), ("m1", true)], json!({}));
        // Left over from earlier work: not this run's change.
        std::fs::write(h.repo.join("leftover.txt"), "old\n").unwrap();
        let run = h.app.create_run(NewRun { goal: "两步完成".into(), workspace: "ws".into() }).await.unwrap();
        let calls = h.dir.join("fake/calls.log");
        let lead_started = |n: usize| {
            let calls = calls.clone();
            async move {
                for _ in 0..100 {
                    if std::fs::read_to_string(&calls).unwrap_or_default().matches("goal 两步完成").count() >= n {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                panic!("lead did not start");
            }
        };
        lead_started(1).await;
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T1", "title": "第一步", "instructions": "做第一步", "assignee": "m1" }] })).await;
        assert!(!err, "{text}");
        wait_until(&h.app, &run, "T1", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T1"))).await;
        assert!(!events(&h.app, &run).iter().any(|e| e.kind == "artifact.created" && e.text.contains("T1-a1.diff")), "T1 changed nothing");

        // The process goes away while the lead is still working.
        h.app.run(&run).unwrap().st.lock().unwrap().cancelled = true;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let app2 = app_at(&h.dir);
        app2.recover().unwrap();
        app2.resume_run(&run).await.unwrap();
        let evs = events(&app2, &run);
        let recovered = evs.iter().rev().find(|e| e.kind == "run.recovered").unwrap();
        assert_eq!(recovered.data.as_ref().unwrap()["tasks"][0]["status"], "succeeded");
        lead_started(2).await;
        assert!(std::fs::read_to_string(&calls).unwrap().contains("brief ok resumed"), "the lead is told it continues");

        // It builds on the board it found.
        let (text, err) = tool(&app2, &run, "lead", "orchestrate", json!({ "tasks": [{ "id": "T2", "title": "第二步", "instructions": "做第二步", "assignee": "m1", "deps": ["T1"] }] })).await;
        assert!(!err, "{text}");
        wait_until(&app2, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2"))).await;
        let (board, _) = tool(&app2, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
        assert!(board.contains("[完成] T2") && !board.contains("[完成] T1"), "only what is new: {board}");
        std::fs::write(h.dir.join("fake/lead-go"), "两步都完成了").unwrap();
        wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// A message shows the moment it is sent (`pending`), then says how it went in: queued while
    /// the agent waits on a request, delivered by id when the turn ends; a note for an agent with
    /// no session goes in with its next one.
    #[tokio::test(flavor = "multi_thread")]
    async fn messages_are_shown_then_routed_and_delivered() {
        let h = harness("messages", &[("lead", false), ("m1", false)], json!({ "T1": ["permission", "ok"], "T2": ["ok"] }));
        let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
        wait_until(&h.app, &run, "permission request", |e| has(e, "approval.requested")).await;
        act(&h, &run, json!({ "type": "message", "agent": "m1", "text": "顺便看看 README" }));
        let evs = events(&h.app, &run);
        let sent = evs.iter().find(|e| e.kind == "user.message").expect("recorded at once");
        let d = sent.data.as_ref().unwrap();
        assert_eq!((d["delivery"].as_str(), sent.to.as_deref(), sent.task.as_deref()), (Some("pending"), Some("m1"), None));
        let id = d["id"].as_str().unwrap().to_string();
        let routed = |evs: &[Event], how: &str| evs.iter().any(|e| e.kind == "message.routed" && e.data.as_ref().is_some_and(|d| d["id"] == id.as_str() && d["delivery"] == how));
        wait_until(&h.app, &run, "queued", |e| routed(e, "queued")).await;
        let req = events(&h.app, &run).into_iter().find(|e| e.kind == "approval.requested").unwrap();
        act(&h, &run, json!({ "type": "approve", "key": req.data.as_ref().unwrap()["key"], "task": "T1", "allow": true, "option": "a1" }));
        wait_until(&h.app, &run, "delivered", |e| e.iter().any(|e| e.kind == "message.delivered" && e.data.as_ref().is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id.as_str()))))).await;
        wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;

        // No session now: a note, carried into m1's next session.
        act(&h, &run, json!({ "type": "message", "agent": "m1", "text": "下一个任务注意测试" }));
        let note = events(&h.app, &run).into_iter().rev().find(|e| e.kind == "user.message").unwrap();
        let note_id = note.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();
        assert!(events(&h.app, &run).iter().any(|e| e.kind == "message.routed" && e.data.as_ref().is_some_and(|d| d["id"] == note_id.as_str() && d["delivery"] == "next")));
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T2", "m1", json!({}))] })).await;
        assert!(!err, "{text}");
        wait_until(&h.app, &run, "note delivered", |e| e.iter().any(|e| e.kind == "message.delivered" && e.data.as_ref().is_some_and(|d| d["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == note_id.as_str()))))).await;
        let t2 = events(&h.app, &run).into_iter().find(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T2")).unwrap();
        assert!(t2.data.as_ref().unwrap()["prompt"].as_str().unwrap().contains("下一个任务注意测试"), "the note is in T2's prompt");
        finish_lead(&h);
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// Working in place: a write task runs in the workspace directory itself, even when every
    /// member's mode says read-only (the lead's call, not Janus's), and its change is kept as a diff.
    #[tokio::test(flavor = "multi_thread")]
    async fn tasks_work_in_the_workspace_directory() {
        let h = harness("inplace", &[("lead", false), ("m1", false)], json!({ "T1": ["write:notes.md"] }));
        let run = start(&h, json!([task("T1", "m1", json!({ "write": true }))])).await;
        wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
        finish_lead(&h);
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n", "written straight into the repo");
        let evs = events(&h.app, &run);
        let started = evs.iter().find(|e| e.kind == "attempt.started").unwrap();
        assert_eq!(started.data.as_ref().unwrap()["path"], json!(h.repo.canonicalize().unwrap()));
        assert!(evs.iter().any(|e| e.kind == "artifact.created" && e.text.contains("T1-a1.diff")), "the change is kept as a diff");
        assert!(!has(&evs, "verification.failed"));
        let created = evs.iter().find(|e| e.kind == "run.created").unwrap();
        assert_eq!(created.data.as_ref().unwrap()["inPlace"], true, "the page describes the run from this");
        assert_eq!(created.data.as_ref().unwrap()["mode"], "goal");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// A task that uses up its attempts fails without stopping the run (the lead reads it on the
    /// board); its dependent is blocked, and both carry on after the person retries it.
    #[tokio::test(flavor = "multi_thread")]
    async fn retry_revives_a_failed_task_and_its_dependent() {
        let h = harness("retry", &[("lead", false), ("m1", false)], json!({ "T1": ["fail", "fail", "ok"] }));
        let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
        wait_until(&h.app, &run, "blocked", |e| has(e, "task.blocked")).await;
        let evs = events(&h.app, &run);
        assert_eq!(evs.iter().filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some("T1")).count(), 2, "two attempts per task");
        assert!(has(&evs, "task.failed") && !has(&evs, "run.waiting_input"), "nothing waits for the person");
        let (board, _) = tool(&h.app, &run, "lead", "wait", json!({ "timeout_sec": 1 })).await;
        assert!(board.contains("[失败] T1") && board.contains("[无法开始] T2"), "{board}");
        act(&h, &run, json!({ "type": "retry", "task": "T1" }));
        wait_until(&h.app, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2"))).await;
        finish_lead(&h);
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        assert_eq!(status(&h.app, &run), "delivered");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// Pausing keeps the run running (it only stops new tasks), and resuming starts them.
    #[tokio::test(flavor = "multi_thread")]
    async fn pause_keeps_running_status() {
        let h = harness("pause", &[("lead", false), ("m1", false)], json!({}));
        let run = h.app.create_run(NewRun { goal: "测试".into(), workspace: "ws".into() }).await.unwrap();
        for _ in 0..100 {
            if h.app.run(&run).is_some_and(|r| !r.st.lock().unwrap().mcp_token.is_empty()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        act(&h, &run, json!({ "type": "pause" }));
        let (text, err) = tool(&h.app, &run, "lead", "orchestrate", json!({ "tasks": [task("T1", "m1", json!({}))] })).await;
        assert!(!err, "{text}");
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(status(&h.app, &run), "running");
        assert!(!has(&events(&h.app, &run), "attempt.started"), "paused: nothing starts");
        act(&h, &run, json!({ "type": "resume" }));
        wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
        finish_lead(&h);
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// Accepting the risk delivers what finished, even while the lead is still working.
    #[tokio::test(flavor = "multi_thread")]
    async fn accept_risk_delivers_partially() {
        let h = harness("accept", &[("lead", false), ("m1", false)], json!({ "T1": ["fail"] }));
        let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
        wait_until(&h.app, &run, "blocked", |e| has(e, "task.blocked")).await;
        act(&h, &run, json!({ "type": "acceptRisk", "reason": "test" }));
        wait_until(&h.app, &run, "delivery", |e| has(e, "run.accepted_with_risk")).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(status(&h.app, &run), "accepted_with_risk");
        let evs = events(&h.app, &run);
        let delivered = evs.iter().find(|e| e.kind == "run.delivered").expect("partial delivery");
        assert_eq!(delivered.data.as_ref().unwrap()["partial"], true);
        assert!(delivered.data.as_ref().unwrap()["risks"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("T1")));
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// After a restart the board comes back with the same statuses (failed, blocked), and a retry
    /// still revives both.
    #[tokio::test(flavor = "multi_thread")]
    async fn restart_restores_failed_and_blocked() {
        let h = harness("restart", &[("lead", false), ("m1", false)], json!({ "T1": ["fail", "fail", "ok"] }));
        let run = start(&h, json!([task("T1", "m1", json!({})), task("T2", "m1", json!({ "deps": ["T1"] }))])).await;
        wait_until(&h.app, &run, "blocked", |e| has(e, "task.blocked")).await;
        // Simulate the process going away: stop the old driver without writing anything.
        h.app.run(&run).unwrap().st.lock().unwrap().cancelled = true;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let app2 = app_at(&h.dir);
        app2.recover().unwrap();
        assert_eq!(status(&app2, &run), "interrupted");
        let r = app2.resume_run(&run).await.unwrap();
        let evs = events(&app2, &run);
        let recovered = evs.iter().rev().find(|e| e.kind == "run.recovered").unwrap();
        let tasks = recovered.data.as_ref().unwrap()["tasks"].clone();
        assert_eq!(tasks[0]["status"], "failed", "{tasks}");
        assert_eq!(tasks[1]["status"], "blocked", "{tasks}");
        app2.action(&r, &json!({ "type": "retry", "task": "T1" })).unwrap();
        wait_until(&app2, &run, "T2", |e| e.iter().any(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some("T2"))).await;
        finish_lead(&h);
        wait_until(&app2, &run, "delivery", |e| has(e, "run.delivered")).await;
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    /// A run from the removed plan-first flow can be read but not continued.
    #[tokio::test(flavor = "multi_thread")]
    async fn plan_runs_are_read_only() {
        let h = harness("oldplan", &[("lead", false), ("m1", false)], json!({}));
        h.app.db.insert_run("rold", "旧目标", &h.repo.to_string_lossy(), &json!({ "team": [] }), now_ms()).unwrap();
        h.app.store(Event { run_id: "rold".into(), seq: 1, ts: now_ms(), kind: "run.created".into(), actor: "user".into(), to: None, task: None, attempt: None, text: "创建目标".into(), data: Some(json!({ "goal": "旧目标" })) });
        h.app.db.set_run_status("rold", "interrupted").unwrap();
        let err = h.app.resume_run("rold").await.err().expect("refused");
        assert!(err.to_string().contains("已经移除"), "{err}");
        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[test]
    fn verification_commands_are_checked() {
        assert!(verification_argv("rg -n foo src").is_ok());
        assert!(verification_argv("find janus -type f | sort").unwrap().2);
        assert!(verification_argv("cargo test --quiet").is_ok());
        assert!(verification_argv("cat a; rm -rf ~").is_err());
        assert!(verification_argv("cat $(echo a)").is_err());
        assert!(verification_argv("sed -i s/a/b/ f").is_err());
        assert!(verification_argv("find . -delete").is_err());
        // Shell syntax inside quotes is regex text, not a subshell or a pipeline.
        let (program, args, pipeline) = verification_argv("rg -n '^#{1,4} |JANUS_|/api/|cargo (run|check|test)|worktree' README.md docs").unwrap();
        assert_eq!((program.as_str(), pipeline), ("rg", false));
        assert_eq!(args[1], "^#{1,4} |JANUS_|/api/|cargo (run|check|test)|worktree");
        assert!(verification_argv("rg -c '(a|b)' src | sort").unwrap().2);
        assert!(verification_argv("rg \"$(rm -rf ~)\" src").is_err(), "double quotes still expand");
        assert!(verification_argv("rg 'a' src | rm -rf ~").is_err());
        assert!(verification_argv("rg 'unclosed src").is_err());
        assert!(verification_argv("ls -la").is_err());
        assert!(verification_argv("cargo fmt").is_err());
        assert!(verification_argv("cargo fmt -- --check").is_ok());
    }

}

