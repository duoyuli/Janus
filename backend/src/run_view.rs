//! What a run's event log comes to for the person: its tasks and their attempts, what each agent
//! is doing, the requests that wait on someone, the messages on their way, the deliveries, the
//! shared board's facts and where a delivery's review stands. One fold over the events, here and
//! nowhere else: the app shows what this serves and computes none of it
//! (docs/decisions/20261126-backend_owns_derived_state.md).
//!
//! The fold takes events one at a time (`Fold::apply`), so a run that gains events is brought up
//! to date from where it stood (`Views`), and a replayed position is the same fold over the first
//! events (`?until=`). It serves facts, not wording: kinds, ids, numbers and the agents' own text;
//! the app writes them in the person's language.
// @project-doc docs/architecture/20261031-frontend.md#event_derivation

mod aside;
mod board;
pub mod docs;
mod inbox;
pub mod preview;
mod review;
mod sessions;
pub mod suggest;
#[cfg(test)]
mod tests;
mod turns;
mod timeline;
mod words;

use crate::db::Event;
use crate::events::EventKind;
use crate::lang::Lang;
use crate::status::{AgentPhase, RunKind, RunStatus};
use crate::wire::wire_enum;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub use board::BoardFacts;
pub use inbox::{Candidate, InboxEntry, inbox_entries};
pub use review::Review;
pub use words::Words;

wire_enum! {
    /// A task as the person sees it: the scheduler's states (`status::TaskStatus`) and the steps
    /// the events show on the way (ready, assigned, verifying, waiting on the person).
    #[derive(Default)]
    pub enum TaskShown {
        #[default]
        Pending = "pending",
        Ready = "ready",
        Assigned = "assigned",
        Running = "running",
        Verifying = "verifying",
        VerificationFailed = "verification_failed",
        WaitingInput = "waiting_input",
        WaitingApproval = "waiting_approval",
        Blocked = "blocked",
        Succeeded = "succeeded",
        Failed = "failed",
        Cancelled = "cancelled",
        SkippedByDecision = "skipped_by_decision",
    }
}

impl TaskShown {
    /// A status a recovery or a record names; one this Janus does not know reads as pending.
    fn read(s: &str) -> TaskShown {
        TaskShown::parse(s).unwrap_or_default()
    }

    /// No more work happens on it without a retry.
    pub fn is_ended(self) -> bool {
        matches!(self, TaskShown::Succeeded | TaskShown::Failed | TaskShown::Cancelled | TaskShown::SkippedByDecision)
    }

    /// Not started yet: waiting for its dependencies or the scheduler.
    fn is_waiting_to_start(self) -> bool {
        matches!(self, TaskShown::Pending | TaskShown::Ready | TaskShown::Assigned | TaskShown::Blocked)
    }

    /// Its attempt waits for the person to answer or approve.
    fn waits_on_person(self) -> bool {
        matches!(self, TaskShown::WaitingInput | TaskShown::WaitingApproval)
    }
}

wire_enum! {
    /// One attempt as the person follows it: running until it fails or its task succeeds.
    pub enum AttemptShown {
        Running = "running",
        Succeeded = "succeeded",
        Failed = "failed",
    }
}

wire_enum! {
    /// What waits on someone in a run: a question, an approval, a form, or a session whose turn
    /// ended and waits for the person's reply.
    pub enum RequestKind {
        Question = "question",
        Approval = "approval",
        Input = "input",
        Session = "session",
    }
}

/// Whose turn an open session is in, and whether that turn has ended (the session waits for a
/// message). One rule for every session, applied event by event (`after`). A closed session is no
/// turn at all (`TaskView::session` is `None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SessionTurn {
    /// The person holds it (took over, or interrupted it).
    pub person: bool,
    /// The turn has ended and it waits for a message.
    pub idle: bool,
}

impl SessionTurn {
    /// A session that just opened: the agent at work.
    pub const OPENED: SessionTurn = SessionTurn { person: false, idle: false };

    fn with_idle(self, idle: bool) -> SessionTurn {
        SessionTurn { idle, ..self }
    }

    /// The turn after `e`.
    fn after(self, e: &Event, d: &Value) -> SessionTurn {
        match e.kind() {
            EventKind::AttemptStarted | EventKind::RunCreated | EventKind::ControlReleased => SessionTurn::OPENED,
            EventKind::ControlTaken => SessionTurn { person: true, idle: self.idle },
            EventKind::SessionInterrupted if truthy(&d["finish"]) => self,
            EventKind::SessionInterrupted => SessionTurn { person: true, idle: self.idle },
            EventKind::AgentStatus if d["turn"] == "idle" => self.with_idle(true),
            EventKind::UserMessage | EventKind::MessageRouted if d["delivery"] == "prompt" => self.with_idle(false),
            EventKind::MessageDelivered => self.with_idle(false),
            _ => self,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    /// `None` before the run's first event.
    pub status: Option<RunStatus>,
    pub goal: String,
    /// Stored paths of the images pasted with the goal.
    pub images: Vec<String>,
    pub start: i64,
    /// The time of the newest event.
    pub now: i64,
    pub paused: bool,
    pub cwd: String,
    /// The repository the run's own worktree was made from; empty for a run working in place.
    pub repo: String,
    /// The branch a run in a worktree started on; empty when it started on a detached HEAD.
    pub branch: String,
    /// Set once an acceptance or a delivery merged a result back.
    pub merged: bool,
    pub git: bool,
    pub base_ref: String,
    pub workspace: String,
    pub error: String,
    pub kind: RunKind,
}

impl RunInfo {
    fn is_done(&self) -> bool {
        matches!(self.status, Some(RunStatus::Delivered | RunStatus::Accepted))
    }

    fn is_finished(&self) -> bool {
        matches!(
            self.status,
            Some(
                RunStatus::Delivered
                    | RunStatus::Accepted
                    | RunStatus::Cancelled
                    | RunStatus::Failed
                    | RunStatus::Interrupted
            )
        )
    }

    /// Its goal is known and it has not finished.
    fn active(&self) -> bool {
        !self.goal.is_empty() && !self.is_finished()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub n: i64,
    pub session: String,
    pub workspace: String,
    pub start: i64,
    pub status: AttemptShown,
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When it stopped running (succeeded, failed, cancelled, interrupted); `None` while it runs.
    pub end: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub desc: String,
    pub hash: String,
    pub bytes: Option<i64>,
    /// The task it belongs to; empty for the whole run's diff (`run_diff`).
    pub task: String,
    pub attempt: i64,
    pub seq: i64,
    pub by: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub key: String,
    pub question: String,
    pub options: Vec<String>,
    pub blocking: bool,
    pub seq: i64,
    pub from: String,
    pub answer: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub desc: String,
    pub agent: String,
    pub deps: Vec<String>,
    pub accept: Vec<String>,
    pub status: TaskShown,
    pub attempts: Vec<AttemptView>,
    pub progress: f64,
    pub artifacts: Vec<ArtifactView>,
    /// Its open session's turn; `None` while it has none.
    pub session: Option<SessionTurn>,
    pub blocked_by: String,
    /// The checks of its result, as the agent handed them in.
    pub checks: Vec<Value>,
    pub verification_failures: Vec<String>,
    pub error: String,
    pub question: Option<QuestionView>,
    pub amendments: Vec<String>,
    pub summary: String,
    pub evidence: Vec<String>,
    /// The risks the executor reported with its result.
    pub risks: Vec<String>,
    /// Files the task owns, as its orchestration gave them.
    pub owns: Vec<String>,
    pub verifies: String,
    pub continues: String,
    pub handoff: String,
    pub verdict: Option<String>,
    /// Where the finished result disagrees with what the task left in the workspace.
    pub trace_gaps: Vec<String>,
}

impl TaskView {
    fn new(x: &Value) -> TaskView {
        TaskView {
            id: text(&x["id"]),
            title: text(&x["title"]),
            desc: text(&x["desc"]),
            agent: text(&x["agent"]),
            deps: texts(&x["deps"]),
            accept: texts(&x["accept"]),
            status: TaskShown::Pending,
            attempts: vec![],
            progress: 0.0,
            artifacts: vec![],
            session: None,
            blocked_by: String::new(),
            checks: vec![],
            verification_failures: vec![],
            error: String::new(),
            question: None,
            amendments: vec![],
            summary: String::new(),
            evidence: vec![],
            risks: vec![],
            owns: texts(&x["owns"]),
            verifies: text(&x["verifies"]),
            continues: text(&x["continues"]),
            handoff: String::new(),
            verdict: None,
            trace_gaps: vec![],
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub phase: AgentPhase,
    pub note: String,
    pub progress: Option<f64>,
    /// The time of its newest event.
    pub beat: i64,
    pub session: Option<String>,
}

/// A message sent to a session, followed from being sent to its delivery.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgView {
    pub id: String,
    pub text: String,
    pub by: String,
    pub to: String,
    pub task: Option<String>,
    pub seq: i64,
    /// Stored paths of the images sent with it.
    pub images: Vec<String>,
    /// How it went in (`messages::Delivery`), `pending` until the server routed it.
    pub delivery: String,
    pub routed_task: Option<String>,
    pub routed_seq: Option<i64>,
    pub delivered_seq: Option<i64>,
}

impl MsgView {
    /// Still to reach the agent: waiting for the turn to end, or for its next session.
    fn waits(&self) -> bool {
        self.delivery == "queued" || self.delivery == "next"
    }
}

/// Something in a run that waits on the person: a decision the backend waits on (`key`), or a
/// session whose turn ended (`task`, or `agent` outside any task).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub kind: RequestKind,
    /// The backend's key of the decision it answers; empty for a session's turn.
    pub key: String,
    pub seq: i64,
    /// When it arrived.
    pub ts: i64,
    pub task: Option<String>,
    pub agent: Option<String>,
    /// The rest of the request as it was asked: reason, recommend, question, options, schema,
    /// url, command …
    pub d: Value,
}

impl Request {
    fn session_of(&self, target: &SessionTarget) -> bool {
        self.kind == RequestKind::Session
            && match target {
                SessionTarget::Task(t) => self.task.as_deref() == Some(t),
                SessionTarget::Agent(a) => self.task.is_none() && self.agent.as_deref() == Some(a),
            }
    }
}

/// Whose session a turn belongs to: a task's, or an agent's outside any task.
enum SessionTarget {
    Task(String),
    Agent(String),
}

/// One delivery (`run.delivered`): what the lead handed over at the end of a round.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    /// The delivery as recorded.
    pub d: Value,
    pub seq: i64,
    pub ts: i64,
    pub accepted: Option<i64>,
    /// Which delivery of the run it is, from 1.
    pub n: usize,
    /// The seq of the delivery before it (0 for the first): its round is what happened after it.
    pub since: i64,
}

/// The run's state at one point of its log. `tasks` are in the order the lead put them on the
/// board.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub run: RunInfo,
    pub tasks: Vec<TaskView>,
    pub agents: BTreeMap<String, AgentView>,
    /// What waits on someone, in the order it arrived.
    pub inbox: Vec<Request>,
    pub artifacts: Vec<ArtifactView>,
    /// The answer each settled request got, by its key.
    pub answers: BTreeMap<String, Value>,
    pub msgs: Vec<MsgView>,
    /// Every delivery, oldest first; the last is the one under review.
    pub deliveries: Vec<DeliveryView>,
    /// The lead's own turn in its conversation (over its events outside any task).
    pub lead_turn: SessionTurn,
    /// The newest event folded in.
    pub seq: i64,
    /// The person's messages to the lead outside any task, for the requests a round answers.
    #[serde(skip)]
    lead_asks: Vec<(i64, String)>,
    /// Each session's events (`sessions::target`), and each agent's own turn over its events.
    #[serde(skip)]
    sessions: sessions::SessionFold,
}

impl Default for SessionTurn {
    fn default() -> SessionTurn {
        SessionTurn::OPENED
    }
}

impl RunState {
    pub fn task(&self, id: &str) -> Option<&TaskView> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut TaskView> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    fn agent(&mut self, id: &str) -> &mut AgentView {
        self.agents.entry(id.to_string()).or_default()
    }

    fn msg_mut(&mut self, id: &str) -> Option<&mut MsgView> {
        self.msgs.iter_mut().find(|m| m.id == id)
    }

    /// The latest delivery, the one under review.
    pub fn result(&self) -> Option<&DeliveryView> {
        self.deliveries.last()
    }

    fn drop_session(&mut self, target: SessionTarget) {
        self.inbox.retain(|r| !r.session_of(&target));
    }

    fn drop_key(&mut self, key: &str) {
        self.inbox.retain(|r| r.kind == RequestKind::Session || r.key != key);
    }

    fn has_session(&self, target: SessionTarget) -> bool {
        self.inbox.iter().any(|r| r.session_of(&target))
    }

    /// Its session closes: no turn, no waiting reply, and its attempt stops at `now`.
    fn end_live(&mut self, task: &str) {
        let now = self.run.now;
        if let Some(t) = self.task_mut(task) {
            t.session = None;
            if let Some(at) = t.attempts.last_mut().filter(|a| a.end.is_none()) {
                at.end = Some(now);
            }
        }
        self.drop_session(SessionTarget::Task(task.into()));
    }

    /// A message the person or an agent sent was put into a turn: the task's session goes on.
    fn prompted(&mut self, e: &Event, d: &Value) {
        let task = e.task.as_deref().filter(|t| self.task(t).is_some());
        if let Some(id) = task {
            let t = self.task_mut(id).expect("task");
            t.session = t.session.map(|s| s.after(e, d));
            let agent = t.agent.clone();
            self.drop_session(SessionTarget::Task(id.into()));
            self.agent(&agent).phase = AgentPhase::Thinking;
        } else if let Some(to) = &e.to {
            self.drop_session(SessionTarget::Agent(to.clone()));
        }
    }

    /// Fold one more event in.
    // @project-doc docs/architecture/20261031-frontend.md#event_derivation
    pub fn apply(&mut self, e: &Event) {
        let null = Value::Null;
        let d = e.data.as_ref().unwrap_or(&null);
        self.run.now = e.ts;
        self.seq = e.seq;
        self.sessions.apply(e, d);
        if e.task.is_none() && (e.actor == "lead" || e.to.as_deref() == Some("lead")) {
            self.lead_turn = self.lead_turn.after(e, d);
        }
        if !e.actor.is_empty() && e.actor != "user" && e.actor != "janus" {
            self.agent(&e.actor).beat = e.ts;
        }
        let task = e.task.clone().filter(|t| self.task(t).is_some());
        let mut t = task.clone();
        match e.kind() {
            EventKind::RunQueued => {
                self.run.status = Some(RunStatus::Queued);
                self.run.goal = text(&d["goal"]);
                self.run.images = image_paths(d);
                self.run.start = e.ts;
            }
            EventKind::RunStarting => self.run.status = Some(RunStatus::Planning),
            EventKind::RunRequeued => self.run.status = Some(RunStatus::Queued),
            EventKind::RunCreated => {
                self.run.status = Some(RunStatus::Running);
                self.run.goal = text(&d["goal"]);
                self.run.images = image_paths(d);
                self.run.start = e.ts;
                self.run.cwd = text(&d["cwd"]);
                self.run.repo = text(&d["worktree"]["repo"]);
                self.run.branch = text(&d["worktree"]["branch"]);
                self.run.git = truthy(&d["git"]);
                self.run.base_ref = text(&d["baseRef"]);
                self.run.workspace = text(&d["workspace"]);
                self.run.kind = RunKind::parse(&text(&d["kind"])).unwrap_or_default();
                self.agent("lead").phase = AgentPhase::Thinking;
            }
            EventKind::OrchestrationAccepted => {
                for x in list(&d["tasks"]) {
                    let nt = TaskView::new(x);
                    match self.tasks.iter().position(|t| t.id == nt.id) {
                        Some(i) => self.tasks[i] = nt,
                        None => self.tasks.push(nt),
                    }
                }
                self.agent("lead").phase = AgentPhase::Coordinating;
                // New work after a delivery: the run is running again until it is delivered anew,
                // so the earlier delivery is not offered for acceptance meanwhile (FL-10).
                if self.run.is_done() {
                    self.run.status = Some(RunStatus::Running);
                }
            }
            EventKind::GoalStatus => self.agent("lead").note = e.text.clone(),
            EventKind::DecisionWithdrawn => {
                self.drop_key(&text(&d["key"]));
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id))
                    && t.status.waits_on_person()
                {
                    t.status = TaskShown::Running;
                }
            }
            EventKind::RunCancelled => {
                self.run.status = Some(RunStatus::Cancelled);
                self.inbox.clear();
                for x in &mut self.tasks {
                    x.session = None;
                    if x.status != TaskShown::Succeeded {
                        x.status = TaskShown::Cancelled;
                    }
                    if let Some(at) = x.attempts.last_mut().filter(|a| a.end.is_none()) {
                        at.end = Some(e.ts);
                    }
                }
            }
            EventKind::RunPaused => self.run.paused = true,
            EventKind::RunResumed => self.run.paused = false,
            EventKind::ResultSubmitted => {
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    t.checks = list(&d["checks"]).to_vec();
                    t.risks = texts(&d["risks"]);
                    t.handoff = text(&d["handoff"]);
                    if let Some(v) = verdict(&d["verdict"]) {
                        t.verdict = Some(v);
                    }
                }
            }
            EventKind::VerificationStarted => {
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    t.status = TaskShown::Verifying;
                }
            }
            // The checks and their methods the backend checked; `task.succeeded` repeats them with
            // the conclusion, so a replayed or resumed run still shows them from one place.
            EventKind::VerificationPassed | EventKind::VerificationFailed => {
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    if !list(&d["checks"]).is_empty() {
                        t.checks = list(&d["checks"]).to_vec();
                    }
                    if e.kind() == EventKind::VerificationFailed {
                        t.status = TaskShown::VerificationFailed;
                        t.verification_failures = texts(&d["failures"]);
                    }
                }
            }
            EventKind::TaskReady => {
                let ids = if d["tasks"].is_null() { e.task.iter().cloned().collect() } else { texts(&d["tasks"]) };
                for id in ids {
                    if let Some(x) = self.task_mut(&id) {
                        x.status = TaskShown::Ready;
                    }
                }
            }
            EventKind::TaskAssigned => {
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    t.status = TaskShown::Assigned;
                }
            }
            EventKind::TaskBlocked => {
                if let Some(t) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    t.status = TaskShown::Blocked;
                    t.blocked_by = text(&d["dependency"]);
                }
            }
            EventKind::TaskSkippedByDecision => {
                if let Some(id) = t.take() {
                    self.task_mut(&id).expect("task").status = TaskShown::SkippedByDecision;
                    self.end_live(&id);
                }
            }
            EventKind::AttemptStarted => {
                if let Some(id) = t.as_deref() {
                    let session = d["session"].as_str().map(str::to_owned);
                    let x = self.task_mut(id).expect("task");
                    if let Some(prev) = x.attempts.last_mut().filter(|a| a.end.is_none()) {
                        prev.end = Some(e.ts);
                    }
                    let n = e.attempt.unwrap_or(x.attempts.len() as i64 + 1);
                    x.attempts.push(AttemptView {
                        n,
                        session: text(&d["session"]),
                        workspace: text(&d["workspace"]),
                        start: e.ts,
                        status: AttemptShown::Running,
                        seq: e.seq,
                        error: None,
                        end: None,
                    });
                    x.session = Some(SessionTurn::OPENED);
                    x.status = TaskShown::Running;
                    x.progress = 5.0;
                    let agent = x.agent.clone();
                    let a = self.agent(&agent);
                    a.session = session;
                    a.phase = AgentPhase::Thinking;
                    a.note.clear();
                    a.progress = Some(5.0);
                }
            }
            EventKind::AgentStatus => {
                let single = self.run.kind.single_session();
                let a = self.agent(&e.actor);
                a.phase = AgentPhase::parse(&text(&d["phase"])).unwrap_or_default();
                a.note = e.text.clone();
                a.progress = d["progress"].as_f64();
                if !d["session"].is_null() {
                    a.session = d["session"].as_str().map(str::to_owned);
                }
                if let (Some(x), Some(p)) = (t.as_deref().and_then(|id| self.task_mut(id)), d["progress"].as_f64()) {
                    x.progress = p;
                }
                // A chat's session opened (again): the conversation is going, whatever ended the
                // last one.
                if single && !d["session"].is_null() {
                    self.run.status = Some(RunStatus::Running);
                    self.run.error.clear();
                }
                // A chat (or a Loop) waiting for the next message is its normal state, not
                // something to handle.
                if d["turn"] == "idle" && !single {
                    if let Some(id) = t.as_deref() {
                        let x = self.task_mut(id).expect("task");
                        x.session = x.session.map(|s| s.after(e, d));
                        if !self.has_session(SessionTarget::Task(id.into())) {
                            self.inbox.push(Request {
                                kind: RequestKind::Session,
                                key: String::new(),
                                seq: e.seq,
                                ts: e.ts,
                                task: Some(id.into()),
                                agent: None,
                                d: json!({}),
                            });
                        }
                    // After a delivery the review says where the lead is, so its turn ending is not
                    // a second entry.
                    } else if !(e.actor == "lead" && self.result().is_some())
                        && !self.has_session(SessionTarget::Agent(e.actor.clone()))
                    {
                        self.inbox.push(Request {
                            kind: RequestKind::Session,
                            key: String::new(),
                            seq: e.seq,
                            ts: e.ts,
                            task: None,
                            agent: Some(e.actor.clone()),
                            d: json!({}),
                        });
                    }
                }
            }
            EventKind::ToolStarted => {
                let a = self.agent(&e.actor);
                a.phase = if d["tool"] == "janus.wait" { AgentPhase::WaitingForAgent } else { AgentPhase::RunningTool };
                a.note = e.text.clone();
            }
            EventKind::ToolCompleted => {
                if ["janus.wait", "janus.reply", "janus.orchestrate"].contains(&text(&d["tool"]).as_str()) {
                    let lead = e.actor == "lead";
                    let a = self.agent(&e.actor);
                    a.phase = if lead { AgentPhase::Coordinating } else { AgentPhase::Thinking };
                    a.note = e.text.clone();
                }
            }
            EventKind::ControlTaken | EventKind::SessionInterrupted => {
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.session = x.session.map(|s| s.after(e, d));
                }
            }
            EventKind::ControlReleased => {
                if let Some(id) = t.as_deref() {
                    let x = self.task_mut(id).expect("task");
                    x.session = x.session.map(|s| s.after(e, d));
                    self.drop_session(SessionTarget::Task(id.into()));
                } else if let Some(to) = &e.to {
                    self.drop_session(SessionTarget::Agent(to.clone()));
                }
            }
            EventKind::UserMessage => {
                if let Some(id) = d["id"].as_str() {
                    if e.actor == "user" && e.to.as_deref() == Some("lead") && e.task.is_none() {
                        self.lead_asks.push((e.seq, visible_text(&text(&d["text"]))));
                    }
                    self.msgs.push(MsgView {
                        id: id.into(),
                        text: text(&d["text"]),
                        by: e.actor.clone(),
                        to: e.to.clone().unwrap_or_default(),
                        task: e.task.clone(),
                        seq: e.seq,
                        images: image_paths(d),
                        delivery: "pending".into(),
                        routed_task: None,
                        routed_seq: None,
                        delivered_seq: None,
                    });
                } else if d["delivery"] == "prompt" {
                    self.prompted(e, d);
                }
            }
            EventKind::MessageRouted => {
                let delivery = text(&d["delivery"]);
                if let Some(m) = self.msg_mut(&text(&d["id"])) {
                    if is_delivery(&delivery) {
                        m.delivery = delivery.clone();
                    }
                    m.routed_task = e.task.clone();
                    m.routed_seq = Some(e.seq);
                }
                if delivery == "prompt" {
                    self.prompted(e, d);
                }
            }
            EventKind::MessageDelivered => {
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.session = x.session.map(|s| s.after(e, d));
                }
                for id in texts(&d["ids"]) {
                    if let Some(m) = self.msg_mut(&id) {
                        m.delivered_seq = Some(e.seq);
                    }
                }
            }
            EventKind::QuestionAsked => {
                if let Some(id) = t.as_deref() {
                    let x = self.task_mut(id).expect("task");
                    x.status = TaskShown::WaitingInput;
                    x.question = Some(QuestionView {
                        key: text(&d["key"]),
                        question: text(&d["question"]),
                        options: texts(&d["options"]),
                        blocking: truthy(&d["blocking"]),
                        seq: e.seq,
                        from: e.actor.clone(),
                        answer: None,
                    });
                    let a = self.agent(&e.actor);
                    a.phase = AgentPhase::WaitingForInput;
                    a.note = e.text.clone();
                }
            }
            // A collaborator's question passed on to the person, or a decision the lead hands over
            // itself (no task).
            EventKind::QuestionEscalated => {
                let key = text(&d["key"]);
                self.drop_key(&key);
                let mut asked = json!({ "reason": d["reason"], "recommend": d["recommend"] });
                if e.task.is_none() {
                    asked["question"] = d["question"].clone();
                    asked["options"] = if truthy(&d["options"]) { d["options"].clone() } else { json!([]) };
                    asked["from"] = json!(e.actor);
                }
                self.inbox.push(Request {
                    kind: RequestKind::Question,
                    key,
                    seq: e.seq,
                    ts: e.ts,
                    task: e.task.clone(),
                    agent: None,
                    d: asked,
                });
            }
            EventKind::QuestionAnswered => {
                let key = text(&d["key"]);
                self.answers.insert(key.clone(), d["answer"].clone());
                self.drop_key(&key);
                if let Some(id) = t.as_deref() {
                    let x = self.task_mut(id).expect("task");
                    if x.status == TaskShown::WaitingInput {
                        x.status = TaskShown::Running;
                    }
                    if let Some(q) = &mut x.question {
                        q.answer = Some(text(&d["answer"]));
                    }
                    let agent = x.agent.clone();
                    self.agent(&agent).phase = AgentPhase::Thinking;
                }
            }
            EventKind::ApprovalRequested | EventKind::InputRequested => {
                let approval = e.kind() == EventKind::ApprovalRequested;
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.status = if approval { TaskShown::WaitingApproval } else { TaskShown::WaitingInput };
                }
                self.inbox.push(Request {
                    kind: if approval { RequestKind::Approval } else { RequestKind::Input },
                    key: text(&d["key"]),
                    seq: e.seq,
                    ts: e.ts,
                    task: e.task.clone(),
                    agent: Some(e.actor.clone()),
                    d: d.clone(),
                });
                let a = self.agent(&e.actor);
                a.phase = if approval { AgentPhase::WaitingApproval } else { AgentPhase::WaitingForInput };
                a.note = e.text.clone();
            }
            EventKind::InputAnswered => {
                let key = text(&d["key"]);
                self.answers.insert(key.clone(), d["action"].clone());
                self.drop_key(&key);
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id))
                    && x.status == TaskShown::WaitingInput
                {
                    x.status = TaskShown::Running;
                }
                if let Some(to) = &e.to {
                    self.agent(to).phase = AgentPhase::Thinking;
                }
            }
            EventKind::ApprovalGranted | EventKind::ApprovalDenied => {
                let key = text(&d["key"]);
                let granted = e.kind() == EventKind::ApprovalGranted;
                self.answers.insert(key.clone(), json!(if granted { "granted" } else { "denied" }));
                self.drop_key(&key);
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id))
                    && x.status == TaskShown::WaitingApproval
                {
                    x.status = TaskShown::Running;
                    let agent = x.agent.clone();
                    self.agent(&agent).phase = AgentPhase::RunningTool;
                }
            }
            // The lead deals with a failure (it reads it on the board), so nothing waits for the
            // person.
            EventKind::TaskFailed => {
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.status = TaskShown::Failed;
                    x.error = text(&d["error"]);
                }
            }
            EventKind::RunFailed => {
                self.run.status = Some(RunStatus::Failed);
                self.run.error = d["error"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
            }
            EventKind::RunInterrupted => {
                self.run.status = Some(RunStatus::Interrupted);
                // A Loop's questions outlive its session: the answer wakes it whenever it comes.
                if self.run.kind == RunKind::Loop {
                    self.inbox.retain(|r| r.kind == RequestKind::Question);
                } else {
                    self.inbox.clear();
                }
                for x in &mut self.tasks {
                    x.session = None;
                    if let Some(at) = x.attempts.last_mut().filter(|a| a.end.is_none()) {
                        at.end = Some(e.ts);
                    }
                }
                for a in self.agents.values_mut() {
                    a.phase = AgentPhase::Idle;
                }
            }
            // The lead's agent closed while idle to give its seat back: a chat or a Loop's session
            // reads as closed, as after a restart; a goal keeps its status.
            EventKind::SessionParked => {
                if matches!(self.run.kind, RunKind::Chat | RunKind::Loop) {
                    self.run.status = Some(RunStatus::Interrupted);
                }
                let a = self.agent("lead");
                a.phase = AgentPhase::Idle;
                a.note = e.text.clone();
            }
            EventKind::RunRecovered => {
                self.run.status = RunStatus::parse(&text(&d["phase"]));
                // A resumed run schedules again: whatever pause came before the restart is over
                // (FL-12).
                self.run.paused = false;
                for x in list(&d["tasks"]) {
                    if let Some(rt) = self.task_mut(&text(&x["id"])) {
                        rt.status = TaskShown::read(&text(&x["status"]));
                        rt.blocked_by = text(&x["blockedBy"]);
                        rt.session = None;
                    }
                }
            }
            EventKind::ArtifactCreated => {
                let a = ArtifactView {
                    name: text(&d["name"]),
                    kind: text(&d["kind"]),
                    path: text(&d["path"]),
                    desc: text(&d["desc"]),
                    hash: text(&d["hash"]),
                    bytes: d["bytes"].as_i64(),
                    task: e.task.clone().unwrap_or_default(),
                    attempt: e.attempt.unwrap_or(0),
                    seq: e.seq,
                    by: e.actor.clone(),
                };
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.artifacts.push(a.clone());
                }
                self.artifacts.push(a);
            }
            EventKind::AttemptFailed => {
                if let Some(id) = t.as_deref() {
                    self.end_live(id);
                    let x = self.task_mut(id).expect("task");
                    if let Some(at) = x.attempts.last_mut() {
                        at.status = AttemptShown::Failed;
                        at.error = Some(text(&d["error"]));
                    }
                    x.status = TaskShown::Ready;
                    let agent = x.agent.clone();
                    let a = self.agent(&agent);
                    a.phase = AgentPhase::Error;
                    a.note = text(&d["error"]);
                    a.progress = None;
                }
            }
            EventKind::ResultAmended => {
                if let Some(x) = t.as_deref().and_then(|id| self.task_mut(id)) {
                    x.amendments.push(text(&d["text"]));
                }
            }
            EventKind::SessionPrompt => {
                if d["channel"] == "fix"
                    && let Some(x) = t.as_deref().and_then(|id| self.task_mut(id))
                {
                    x.status = TaskShown::Running;
                }
            }
            EventKind::TaskReopened => {
                if let Some(id) = t.as_deref() {
                    let x = self.task_mut(id).expect("task");
                    x.amendments.clear();
                    x.trace_gaps.clear();
                    x.verdict = None;
                    x.handoff.clear();
                    if let Some(ins) = d["instructions"].as_str().filter(|s| !s.is_empty()) {
                        x.desc = ins.into();
                    }
                    // Rework with revised criteria: the new checks are about these (FL-11).
                    if !list(&d["accept"]).is_empty() {
                        x.accept = texts(&d["accept"]);
                    }
                    x.status = TaskShown::Ready;
                    // Tasks held by this one's failure wait for it again instead of showing as stuck.
                    for y in &mut self.tasks {
                        if y.status == TaskShown::Blocked && y.blocked_by == id {
                            y.status = TaskShown::Pending;
                        }
                    }
                    if self.run.is_done() {
                        self.run.status = Some(RunStatus::Running);
                    }
                }
            }
            EventKind::TaskSucceeded => {
                if let Some(id) = t.as_deref() {
                    let x = self.task_mut(id).expect("task");
                    x.status = TaskShown::Succeeded;
                    x.progress = 100.0;
                    x.summary = text(&d["summary"]);
                    x.evidence = texts(&d["evidence"]);
                    x.handoff = text(&d["handoff"]);
                    if let Some(v) = verdict(&d["verdict"]) {
                        x.verdict = Some(v);
                    }
                    // The conclusion carries the whole result: its checks with their methods and
                    // its risks.
                    x.checks = list(&d["checks"]).to_vec();
                    x.risks = texts(&d["risks"]);
                    if d["acceptedBy"] != "lead" {
                        x.trace_gaps = texts(&d["traceGaps"]);
                    }
                    self.end_live(id);
                    let x = self.task_mut(id).expect("task");
                    if let Some(at) = x.attempts.last_mut().filter(|a| a.status == AttemptShown::Running) {
                        at.status = AttemptShown::Succeeded;
                    }
                    let agent = x.agent.clone();
                    let a = self.agent(&agent);
                    a.phase = if agent == "lead" { AgentPhase::Coordinating } else { AgentPhase::Idle };
                    a.note.clear();
                    a.progress = None;
                }
            }
            EventKind::RunDelivered => {
                let since = self.deliveries.last().map_or(0, |r| r.seq);
                self.deliveries.push(DeliveryView {
                    d: d.clone(),
                    seq: e.seq,
                    ts: e.ts,
                    accepted: None,
                    n: self.deliveries.len() + 1,
                    since,
                });
                self.run.status = Some(RunStatus::Delivered);
                let a = self.agent("lead");
                a.phase = AgentPhase::Idle;
                a.note.clear();
                self.lead_turn = SessionTurn { person: false, idle: true };
                self.drop_session(SessionTarget::Agent("lead".into()));
            }
            EventKind::RiskAccepted => {
                self.run.status = Some(RunStatus::Accepted);
                if let Some(r) = self.deliveries.last_mut() {
                    r.accepted = Some(e.ts);
                }
                if d["commit"].is_string() {
                    self.run.merged = true;
                }
            }
            // A delivery brought the result to the person's branch.
            EventKind::RunMerged if d["commit"].is_string() => self.run.merged = true,
            _ => {}
        }
    }

    /// What the person asked the lead between the delivery before the one at `seq` (`since`) and
    /// it: the requests this round answers.
    pub fn round_requests(&self, r: &DeliveryView) -> Vec<(i64, String)> {
        if r.n <= 1 {
            return vec![];
        }
        self.lead_asks.iter().filter(|(seq, _)| *seq > r.since && *seq < r.seq).cloned().collect()
    }
}

/// Everything one fold keeps: the run's state, what the board needs besides it, the events the
/// conversations show, the lines each of their changes shows, the run's Markdown files, and each
/// event's time and actor (for what the timeline places at an event).
#[derive(Clone, Default)]
pub struct Fold {
    pub state: RunState,
    board: board::BoardFold,
    stamps: BTreeMap<i64, (i64, String)>,
    shown: HashMap<i64, Event>,
    diffs: BTreeMap<i64, Vec<DiffView>>,
    docs: docs::DocsFold,
}

/// One file change an event carries (a tool call's edit, the edit an approval asks about), as it
/// reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiffView {
    pub path: String,
    /// Lines added and removed over the change.
    pub added: usize,
    pub removed: usize,
    /// The changed lines with three lines of context.
    pub lines: Vec<crate::diff::Mark>,
}

impl Fold {
    pub fn apply(&mut self, e: &Event) {
        self.state.apply(e);
        self.board.apply(&self.state, e);
        self.stamps.insert(e.seq, (e.ts, e.actor.clone()));
        let null = Value::Null;
        let d = e.data.as_ref().unwrap_or(&null);
        self.docs.apply(e, d);
        if sessions::in_conversation(e, d) {
            // A tool call's edits are its `diffs` (`old`, `new`); an approval's are the diffs in the
            // content of the call it asks about (`oldText`, `newText`).
            let approval = e.kind() == EventKind::ApprovalRequested;
            let (old, new) = if approval { ("oldText", "newText") } else { ("old", "new") };
            let items: Vec<&Value> = if approval {
                list(&d["content"]).iter().filter(|c| c["type"] == "diff").collect()
            } else {
                list(&d["diffs"]).iter().collect()
            };
            let changes: Vec<DiffView> = items
                .into_iter()
                .map(|x| {
                    let (a, b) = (text(&x[old]), text(&x[new]));
                    let (added, removed) = crate::diff::line_counts(&a, &b);
                    DiffView { path: text(&x["path"]), added, removed, lines: crate::diff::inline(&a, &b) }
                })
                .collect();
            if !changes.is_empty() {
                self.diffs.insert(e.seq, changes);
            }
            self.shown.insert(e.seq, e.clone());
        }
    }

    /// The run's Markdown files that exist now (`docs`); `work` is where its worktree is or was.
    pub fn docs(&self, work: &std::path::Path) -> Vec<docs::Doc> {
        let run = &self.state.run;
        let exists = |p: &str| std::path::Path::new(p).exists();
        let moved = docs::Moved::of(&run.cwd, &run.repo, work, exists);
        self.docs.existing(&run.cwd, moved.as_ref(), exists)
    }

    /// The suggestions for a draft in conversation `key` (`suggest`); `files` finds the workspace
    /// files matching what follows `@`.
    pub fn suggest(
        &self,
        key: &str,
        draft: &str,
        lang: Lang,
        files: impl FnOnce(&str) -> Vec<String>,
    ) -> suggest::Suggestions {
        let seqs = self.state.sessions.events_of(key);
        let evs: Vec<&Event> = seqs.iter().filter_map(|s| self.shown.get(s)).collect();
        suggest::suggestions(&aside::commands(&evs), draft, lang, files)
    }

    /// The newest event folded in.
    pub fn seq(&self) -> i64 {
        self.state.seq
    }

    /// What the run page shows, written in `words`. `live`: the run is in the service's memory;
    /// without it no lead session is there to work or to be held, whatever the last events said.
    /// `replaying`: a past position, where nothing can be sent.
    pub fn view(&self, live: bool, replaying: bool, words: &Words, detail: bool) -> RunView {
        let mut board = self.board.facts(&self.state);
        words.board(&mut board, &self.state);
        let mut review = review::review(&self.state, live);
        for s in review.iter_mut().flat_map(|r| r.signals.iter_mut()) {
            words.signal(s);
        }
        let counts =
            |seq: i64, i: usize| self.diffs.get(&seq).and_then(|c| c.get(i)).map_or((0, 0), |c| (c.added, c.removed));
        let drawn = sessions::Drawn { events: &self.shown, counts: &counts, lang: words.lang, detail };
        let sessions = sessions::sessions(&self.state, live, replaying, &drawn);
        let timeline = timeline::timeline(&self.state, &board, &self.stamps, replaying, words);
        let status = self.state.run.status;
        let read_only = !live && status == Some(RunStatus::Cancelled);
        let resume_note = match status {
            _ if replaying => None,
            Some(RunStatus::Interrupted) => Some(words.resume_note(true)),
            Some(RunStatus::Failed) => Some(words.resume_note(false)),
            _ => None,
        };
        RunView {
            state: self.state.clone(),
            board,
            review,
            sessions,
            diffs: self.diffs.clone(),
            docs: vec![],
            timeline,
            live,
            actions: BTreeMap::new(),
            read_only,
            resume_note,
        }
    }

    /// Only what the inbox reads: the person's open items and the latest delivery's review,
    /// without copying the run's state or working out the rest of the board and the sessions.
    pub fn asks(&self, live: bool, words: &Words) -> Asks {
        let mut open = self.board.yours(&self.state);
        for o in &mut open {
            words.open_fact(o, &self.state);
        }
        let mut review = review::review(&self.state, live);
        for s in review.iter_mut().flat_map(|r| r.signals.iter_mut()) {
            words.signal(s);
        }
        Asks { open, review, result: self.state.result().cloned() }
    }
}

/// What a run asks of the person, as the inbox reads it (`Fold::asks`).
pub struct Asks {
    /// The person's open items, worded.
    pub open: Vec<board::OpenFact>,
    pub review: Option<Review>,
    /// The latest delivery.
    pub result: Option<DeliveryView>,
}

/// The fold over a whole log.
pub fn fold(events: &[Event]) -> Fold {
    let mut f = Fold::default();
    for e in events {
        f.apply(e);
    }
    f
}

/// What `GET /api/runs/{id}/view` serves: the state, the board and the review at one position.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    #[serde(flatten)]
    pub state: RunState,
    pub board: BoardFacts,
    pub review: Option<Review>,
    /// Each conversation's events and what its composer offers, by `task:<id>` or `agent:<id>`.
    pub sessions: std::collections::BTreeMap<String, sessions::SessionView>,
    /// The file changes the conversations' events carry, by event seq, in their order there.
    pub diffs: BTreeMap<i64, Vec<DiffView>>,
    /// The run's Markdown files that exist (`docs`).
    pub docs: Vec<docs::Doc>,
    /// The progress timeline's rows and marks.
    pub timeline: timeline::Timeline,
    /// The run is in the service's memory.
    pub live: bool,
    /// What can be done to the whole run now, by `RunOp` (`run_ops::answers`); filled in by the
    /// service, which knows its memory of the run.
    pub actions: BTreeMap<&'static str, crate::run_ops::OpAnswer>,
    /// Nothing can be done in it any more: a stopped run the service no longer holds.
    pub read_only: bool,
    /// Above the run card while it stopped short and can go on: what happened and what to do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_note: Option<String>,
}

/// The folds of the runs someone asked about, each kept at the newest event it has seen, so the
/// next ask reads only the events after it.
#[derive(Default)]
pub struct Views(std::sync::Mutex<HashMap<String, Fold>>);

impl Views {
    /// `with` over run `id`'s fold, brought up to its newest stored event by `read` (the events
    /// after a seq) first. The fold is not copied.
    pub fn current<R>(
        &self,
        id: &str,
        read: impl FnOnce(i64) -> anyhow::Result<Vec<Event>>,
        with: impl FnOnce(&Fold) -> R,
    ) -> anyhow::Result<R> {
        let mut views = self.0.lock().unwrap();
        let f = views.entry(id.to_string()).or_default();
        for e in read(f.seq())? {
            f.apply(&e);
        }
        Ok(with(f))
    }

    /// Forget the runs `keep` does not name: nobody asks about them any more.
    pub fn keep_only(&self, keep: &std::collections::HashSet<String>) {
        self.0.lock().unwrap().retain(|id, _| keep.contains(id));
    }

    /// Forget run `id` (deleted).
    pub fn forget(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

// JSON as the events carry it, read as the app used to: a missing field is empty.

/// A field as text: a string as it is, a number or a boolean written out, null empty.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Number(n) => n.as_i64().map_or_else(|| n.to_string(), |i| i.to_string()),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// An array field's items as text.
fn texts(v: &Value) -> Vec<String> {
    list(v).iter().map(text).collect()
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Present and not empty, zero or false (an array or object counts whatever it holds).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// A verdict an independent verification may give.
fn verdict(v: &Value) -> Option<String> {
    v.as_str().filter(|s| ["approved", "rejected", "inconclusive"].contains(s)).map(str::to_owned)
}

/// How a message went into a session (`messages::Delivery`).
fn is_delivery(s: &str) -> bool {
    ["pending", "steer", "queued", "interrupt", "prompt", "next"].contains(&s)
}

/// The stored paths of a goal's or a message's images.
fn image_paths(d: &Value) -> Vec<String> {
    list(&d["images"]).iter().map(|i| text(&i["path"])).filter(|p| !p.is_empty()).collect()
}

/// At most `n` characters of `t`, marked with `…` when cut: a preview in a list or a tooltip.
fn short(t: &str, n: usize) -> String {
    if t.chars().count() > n { t.chars().take(n).collect::<String>() + "…" } else { t.to_string() }
}

/// `t` with its Markdown marks dropped (links to their text, `**`, backticks, heading marks), for
/// a preview cut to a length.
fn plain(t: &str) -> String {
    // A link `[text](target)` keeps its text: the target has no whitespace, the text no newline.
    let mut s = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find(']').filter(|&close| close > 0 && !after[..close].contains('\n')).and_then(|close| {
            let target = after[close + 1..].strip_prefix('(')?;
            let end = target.find(')').filter(|&e| e > 0 && !target[..e].contains(char::is_whitespace))?;
            Some((close, close + 2 + end + 1))
        });
        match link {
            Some((close, len)) => {
                s.push_str(&rest[..open]);
                s.push_str(&after[..close]);
                rest = &after[len..];
            }
            None => {
                s.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    s.push_str(rest);
    let s = s.replace("**", "").replace('`', "");
    s.split('\n')
        .map(|line| {
            let marks = line.len() - line.trim_start_matches('#').len();
            if marks > 0 { line[marks..].trim_start() } else { line }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A message's text without the lines that name its attached images.
pub fn visible_text(t: &str) -> String {
    let image_line = |l: &str| {
        l.strip_prefix("[Image ")
            .and_then(|rest| rest.split_once("] "))
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    let lines: Vec<&str> = t.split('\n').collect();
    let kept: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| {
            let l = line.trim_matches(' ');
            !image_line(l) && l != "Attached images:" && l != "Review the user's attached images:"
        })
        .collect();
    if kept.len() == lines.len() {
        return t.to_string();
    }
    kept.join("\n").trim().to_string()
}
