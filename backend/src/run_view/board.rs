//! The shared board's facts: what holds work up until someone acts (`open`), the results handed
//! in and who took them, the artifacts, the handoffs between tasks and the decisions taken. Facts
//! only; the app writes each kind's title in the person's language.
// @project-doc docs/architecture/20261031-frontend.md#event_derivation

use super::{
    ArtifactView, Request, RequestKind, RunState, TaskShown, image_paths, list, text, texts, truthy, visible_text,
};
use crate::db::Event;
use crate::events::EventKind;
use crate::status::RunStatus;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Which open item a fact is, as text: built from typed parts so two never share an id (a task id
/// holds letters, digits, `-` and `_` only). The person's own items keep the form the inbox
/// remembers a snooze or an ignore by.
enum OpenKey<'a> {
    Request { seq: i64, kind: &'a str },
    Loop(i64),
    LeadStall(i64),
    Conflict(i64),
    Split(&'a str, i64),
    Gap(i64),
    Stall(&'a str, i64),
    Blocker(i64),
    LeadQuestion(&'a str, i64),
    Failed(&'a str),
    Blocked(&'a str),
}

impl OpenKey<'_> {
    fn text(&self) -> String {
        match self {
            OpenKey::Request { seq, kind } => format!("o{seq}{kind}"),
            OpenKey::Loop(seq) => format!("oloop{seq}"),
            OpenKey::LeadStall(seq) => format!("olead{seq}"),
            OpenKey::Conflict(seq) => format!("conflict:{seq}"),
            OpenKey::Split(task, seq) => format!("split:{task}:{seq}"),
            OpenKey::Gap(seq) => format!("gap:{seq}"),
            OpenKey::Stall(task, seq) => format!("stall:{task}:{seq}"),
            OpenKey::Blocker(seq) => format!("blocker:{seq}"),
            OpenKey::LeadQuestion(task, seq) => format!("lead_question:{task}:{seq}"),
            OpenKey::Failed(task) => format!("failed:{task}"),
            OpenKey::Blocked(task) => format!("blocked:{task}"),
        }
    }
}

crate::wire::wire_enum! {
    /// What an open item is. The first six wait on the person; the rest are the lead's to settle.
    pub enum OpenKind {
        /// A collaborator's question passed on, or the lead's own decision (no task).
        Question = "question",
        Approval = "approval",
        Input = "input",
        /// A session whose turn ended, waiting for the person's reply.
        Session = "session",
        /// A Loop changed files in its own copy on its last wake.
        LoopTouched = "loop_touched",
        /// A Loop could not wake.
        LoopFailed = "loop_failed",
        /// The lead may be stuck.
        LeadStall = "lead_stall",
        /// Two tasks, or the lead and a task, edited the same files.
        Conflict = "conflict",
        /// A task, or the lead, edited files another task owns.
        Overstep = "overstep",
        SplitRequest = "split_request",
        /// A finished result disagrees with what its task left in the workspace.
        Trace = "trace",
        Stall = "stall",
        Blocker = "blocker",
        /// A collaborator asks the lead.
        LeadQuestion = "lead_question",
        Failed = "failed",
        /// Waiting for a dependency that did not finish.
        Blocked = "blocked",
    }
}

/// Something that holds work up until someone acts.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFact {
    pub key: String,
    pub kind: OpenKind,
    pub seq: Option<i64>,
    /// When it arrived, for the person's items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// The person handles it; otherwise the lead does.
    pub you: bool,
    /// What it is, in the person's language (`words`).
    pub title: String,
    /// The line below it: the task it is about, or who handles it.
    pub sub: String,
    /// Its kind in a word or two, for a badge.
    pub label: String,
    /// Answers in one click, where it is a plain choice: an approval's options, a question's.
    pub actions: Vec<QuickAction>,
    pub tasks: Vec<String>,
    pub agents: Vec<String>,
    /// Who asks, for the person's requests: the task's agent, or the agent that asked.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub who: String,
    /// The task it is about, with its title (the person's requests).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub task_title: String,
    /// What it says: the question, the command, the form's message, the reason, the report.
    pub text: String,
    /// Its items: files, trace gaps, a verification's failures.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// The decision a request answers (its key); empty for a session's turn.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub decision: String,
    /// The lead's own question, for its title.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub question: String,
    /// The answers a click can give: a question's options, or an approval's as the agent gave them.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub options: Value,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub recommend: String,
    /// A form's mode (`url` asks to open a link).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// The other task of a conflict, or the dependency a blocked task waits for.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub other: String,
    /// A conflict or an overstep is the lead's own edit.
    pub by_lead: bool,
}

impl OpenFact {
    fn new(key: OpenKey<'_>, kind: OpenKind, seq: Option<i64>, you: bool) -> OpenFact {
        OpenFact {
            key: key.text(),
            kind,
            seq,
            ts: None,
            you,
            title: String::new(),
            sub: String::new(),
            label: String::new(),
            actions: vec![],
            tasks: vec![],
            agents: vec![],
            who: String::new(),
            task_title: String::new(),
            text: String::new(),
            lines: vec![],
            decision: String::new(),
            question: String::new(),
            options: Value::Null,
            recommend: String::new(),
            mode: String::new(),
            other: String::new(),
            by_lead: false,
        }
    }

    fn about(mut self, tasks: &[&str], agents: &[&str]) -> OpenFact {
        self.tasks = tasks.iter().map(|t| t.to_string()).collect();
        self.agents = agents.iter().map(|a| a.to_string()).collect();
        self
    }

    fn says(mut self, text: impl Into<String>) -> OpenFact {
        self.text = text.into();
        self
    }
}

/// A result handed in (`result.submitted`), who took it, and whether a newer one replaced it.
/// One click that settles a request: the request body to send as it is.
#[derive(Clone, Debug, Serialize)]
pub struct QuickAction {
    pub label: String,
    pub help: String,
    pub prominent: bool,
    /// An approval option's kind as the agent gave it (`allow_once`, `allow_always`, `reject_once`,
    /// `reject_always`); empty for an answer to a question.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// What `POST /api/runs/{id}/actions` takes for it.
    pub body: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFact {
    pub key: String,
    pub seq: i64,
    pub ts: i64,
    pub by: String,
    pub task: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub risks: Vec<String>,
    pub checks: Vec<Value>,
    pub used_by: Vec<String>,
    pub old: bool,
    pub amended: Vec<String>,
}

crate::wire::wire_enum! {
    /// What passed between tasks or to the lead.
    pub enum HandoffKind {
        /// A collaborator asks the lead to split its task (`count` subtasks).
        Split = "split",
        /// One task's context handed to another.
        Handoff = "handoff",
        Progress = "progress",
        Note = "note",
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffFact {
    pub key: String,
    pub seq: i64,
    pub ts: i64,
    pub by: String,
    pub kind: HandoffKind,
    /// What it says, in the person's language (`words`).
    pub text: String,
    pub from: String,
    pub to: String,
    pub agent: String,
    /// The reason, the summary or the post, as written.
    pub summary: String,
    /// How many subtasks a split proposes.
    pub count: usize,
    pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtFact {
    pub a: ArtifactView,
    /// A newer artifact of the same kind from the same task replaced it.
    pub old: bool,
    pub used_by: Vec<String>,
}

crate::wire::wire_enum! {
    /// A decision on the board.
    pub enum DecisionKind {
        /// The lead put tasks on the board (`dispatched`).
        Dispatched = "dispatched",
        /// The lead's orchestration did not pass its checks (`text`: the errors, one a line).
        Rejected = "rejected",
        /// A question was answered (`answer`, `note`).
        Answered = "answered",
        Approved = "approved",
        Denied = "denied",
        /// The person accepted the delivery and its risks.
        RiskAccepted = "risk_accepted",
        /// A delivery merged the result to the person's branch.
        Merged = "merged",
        /// The lead revised a task's conclusion.
        Amended = "amended",
        /// The lead cancelled a task.
        Cancelled = "cancelled",
        /// The lead sent a task back (`from` the verification it follows, `session`, `note`,
        /// `instructions`, `accept`).
        Reopened = "reopened",
        /// The lead accepted a task's result itself.
        AcceptedByLead = "accepted_by_lead",
        /// A message to an agent (`to`, `images`).
        Message = "message",
    }
}

/// A task the lead put on the board.
#[derive(Clone, Debug, Serialize)]
pub struct Dispatched {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub deps: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionFact {
    pub key: String,
    pub seq: i64,
    pub ts: i64,
    pub by: String,
    pub kind: DecisionKind,
    /// What it is, in the person's language (`words`).
    pub title: String,
    /// What it says: the event's text, the revision, the reason, the message; composed in the
    /// person's language where it is made of parts (`words`).
    pub text: String,
    /// The task it is about, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub task: String,
    pub tasks: Vec<String>,
    pub agents: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dispatched: Vec<Dispatched>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub answer: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    /// New acceptance criteria a rework gave; `None` when it kept the old ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<Vec<String>>,
    /// Images sent with a message.
    pub images: usize,
    /// The message's addressee.
    pub to: Option<String>,
    /// The run is over and this message never reached its agent.
    pub lost: bool,
}

impl DecisionFact {
    fn new(e: &Event, kind: DecisionKind, text: impl Into<String>) -> DecisionFact {
        DecisionFact {
            key: format!("d{}", e.seq),
            seq: e.seq,
            ts: e.ts,
            by: e.actor.clone(),
            kind,
            title: String::new(),
            text: text.into(),
            task: e.task.clone().unwrap_or_default(),
            tasks: vec![],
            agents: vec![],
            dispatched: vec![],
            answer: String::new(),
            note: String::new(),
            session: String::new(),
            from: String::new(),
            instructions: String::new(),
            accept: None,
            images: 0,
            to: None,
            lost: false,
        }
    }

    /// About the event's task, by the lead.
    fn by_lead_about(mut self, e: &Event) -> DecisionFact {
        self.tasks = e.task.iter().cloned().collect();
        self.agents = vec!["lead".into()];
        self
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BoardFacts {
    pub open: Vec<OpenFact>,
    pub results: Vec<ResultFact>,
    pub arts: Vec<ArtFact>,
    pub handoffs: Vec<HandoffFact>,
    pub decisions: Vec<DecisionFact>,
}

#[derive(Clone, Debug)]
struct Conflict {
    seq: i64,
    task: String,
    other: String,
    paths: Vec<String>,
    owned: bool,
    by_lead: bool,
}

/// What the board keeps while the events go by, besides the run's state.
#[derive(Clone, Default)]
pub(super) struct BoardFold {
    results: Vec<ResultFact>,
    handoffs: Vec<HandoffFact>,
    decisions: Vec<DecisionFact>,
    /// Messages without an id still waiting for their agent, by agent: their seqs.
    notes: HashMap<String, Vec<i64>>,
    blockers: Vec<(i64, String, String, String)>,
    /// The watchdog's latest report per task, until the agent moves again or a new attempt starts.
    stalls: HashMap<String, (i64, String)>,
    /// The lead's own stall goes to the person: nobody else watches the lead.
    lead_stall: Option<(i64, i64, String)>,
    /// A Loop's last wake: files it changed in its copy, or why it could not wake.
    loop_notice: Option<(i64, i64, OpenKind, String, Vec<String>)>,
    conflicts: Vec<Conflict>,
    /// Collaborator split proposals waiting on the lead's decision.
    splits: HashMap<String, (i64, String)>,
    /// Finished results that disagree with what their task left in the workspace.
    gaps: HashMap<String, (i64, Vec<String>)>,
}

impl BoardFold {
    /// Fold one more event in; `st` is the run's state with it already applied.
    pub(super) fn apply(&mut self, st: &RunState, e: &Event) {
        let null = Value::Null;
        let d = e.data.as_ref().unwrap_or(&null);
        if let Some(task) = &e.task
            && self.stalls.contains_key(task)
            && (st.task(task).is_some_and(|t| t.agent == e.actor) || e.kind() == EventKind::AttemptStarted)
        {
            self.stalls.remove(task);
        }
        // A lead stall has no "resolved" of its own besides the watchdog's: the lead moving again
        // is the resolution.
        if self.lead_stall.is_some() && e.actor == "lead" {
            self.lead_stall = None;
        }
        let task = e.task.clone().unwrap_or_default();
        match e.kind() {
            EventKind::TaskStalled => {
                if e.task.is_some() {
                    if truthy(&d["resolved"]) {
                        self.stalls.remove(&task);
                    } else {
                        self.stalls.insert(task, (e.seq, e.text.clone()));
                    }
                }
            }
            EventKind::LeadStalled => {
                self.lead_stall = (!truthy(&d["resolved"])).then(|| (e.seq, e.ts, e.text.clone()));
            }
            EventKind::LoopWoke => self.loop_notice = None,
            EventKind::LoopTouched => {
                self.loop_notice = Some((e.seq, e.ts, OpenKind::LoopTouched, e.text.clone(), texts(&d["files"])))
            }
            EventKind::LoopFailed => {
                self.loop_notice = Some((e.seq, e.ts, OpenKind::LoopFailed, e.text.clone(), vec![]))
            }
            EventKind::TaskConflict => {
                if e.task.is_some() {
                    self.conflicts.push(Conflict {
                        seq: e.seq,
                        task,
                        other: text(&d["other"]),
                        paths: texts(&d["paths"]),
                        owned: truthy(&d["owned"]),
                        by_lead: truthy(&d["byLead"]),
                    });
                }
            }
            EventKind::TaskSplitRequested => {
                let reason = d["reason"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
                if e.task.is_some() {
                    self.splits.insert(task.clone(), (e.seq, reason.clone()));
                }
                self.handoffs.push(HandoffFact {
                    key: format!("h{}", e.seq),
                    seq: e.seq,
                    ts: e.ts,
                    by: e.actor.clone(),
                    kind: HandoffKind::Split,
                    text: String::new(),
                    from: task,
                    to: "lead".into(),
                    agent: "lead".into(),
                    summary: reason,
                    count: list(&d["subtasks"]).len(),
                    artifacts: vec![],
                });
            }
            EventKind::ResultSubmitted => self.results.push(ResultFact {
                key: format!("r{}", e.seq),
                seq: e.seq,
                ts: e.ts,
                by: e.actor.clone(),
                task,
                summary: d["summary"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone()),
                evidence: texts(&d["evidence"]),
                risks: texts(&d["risks"]),
                checks: list(&d["checks"]).to_vec(),
                used_by: vec![],
                old: false,
                amended: vec![],
            }),
            EventKind::ContextHandoff => self.handoffs.push(HandoffFact {
                key: format!("h{}", e.seq),
                seq: e.seq,
                ts: e.ts,
                by: e.actor.clone(),
                kind: HandoffKind::Handoff,
                text: String::new(),
                from: text(&d["from"]),
                to: task,
                agent: e.to.clone().unwrap_or_default(),
                summary: text(&d["summary"]),
                count: 0,
                artifacts: texts(&d["artifacts"]),
            }),
            EventKind::OrchestrationAccepted => {
                let tasks = list(&d["tasks"]);
                let mut f = DecisionFact::new(e, DecisionKind::Dispatched, "");
                f.dispatched = tasks
                    .iter()
                    .map(|x| Dispatched {
                        id: text(&x["id"]),
                        title: text(&x["title"]),
                        agent: text(&x["agent"]),
                        deps: texts(&x["deps"]),
                    })
                    .collect();
                f.tasks = tasks.iter().map(|x| text(&x["id"])).collect();
                f.agents = vec!["lead".into()];
                self.decisions.push(f);
            }
            EventKind::OrchestrationRejected => {
                let errors = texts(&d["errors"]);
                let says = if errors.is_empty() { e.text.clone() } else { errors.join("\n") };
                let mut f = DecisionFact::new(e, DecisionKind::Rejected, says);
                f.agents = vec!["lead".into()];
                self.decisions.push(f);
            }
            EventKind::BoardPosted => {
                let says = d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
                if d["kind"] == "blocker" {
                    self.blockers.push((e.seq, task, says, e.actor.clone()));
                } else {
                    self.handoffs.push(HandoffFact {
                        key: format!("h{}", e.seq),
                        seq: e.seq,
                        ts: e.ts,
                        by: e.actor.clone(),
                        kind: if d["kind"] == "note" { HandoffKind::Note } else { HandoffKind::Progress },
                        text: String::new(),
                        from: task,
                        to: "lead".into(),
                        agent: "lead".into(),
                        summary: says,
                        count: 0,
                        artifacts: vec![],
                    });
                }
            }
            EventKind::QuestionAnswered => {
                self.splits.remove(&task);
                let mut f = DecisionFact::new(e, DecisionKind::Answered, "");
                f.answer = text(&d["answer"]);
                f.note = if truthy(&d["note"]) { text(&d["note"]) } else { String::new() };
                f.tasks = e.task.iter().cloned().collect();
                f.agents = e.to.iter().cloned().collect();
                self.decisions.push(f);
            }
            EventKind::ApprovalGranted | EventKind::ApprovalDenied => {
                let kind =
                    if e.kind() == EventKind::ApprovalGranted { DecisionKind::Approved } else { DecisionKind::Denied };
                let mut f = DecisionFact::new(e, kind, e.text.clone());
                f.tasks = e.task.iter().cloned().collect();
                f.agents = e.to.iter().cloned().collect();
                self.decisions.push(f);
            }
            EventKind::RiskAccepted => {
                self.decisions.push(DecisionFact::new(e, DecisionKind::RiskAccepted, e.text.clone()))
            }
            EventKind::RunMerged => self.decisions.push(DecisionFact::new(e, DecisionKind::Merged, e.text.clone())),
            EventKind::ResultAmended => {
                self.gaps.remove(&task);
                let says = d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
                self.decisions.push(DecisionFact::new(e, DecisionKind::Amended, says).by_lead_about(e));
            }
            EventKind::TaskSkippedByDecision => {
                self.splits.remove(&task);
                if e.actor == "lead" {
                    self.decisions.push(DecisionFact::new(e, DecisionKind::Cancelled, e.text.clone()).by_lead_about(e));
                }
            }
            EventKind::TaskReopened => {
                self.gaps.remove(&task);
                self.splits.remove(&task);
                if e.actor == "lead" {
                    let mut f = DecisionFact::new(e, DecisionKind::Reopened, e.text.clone());
                    f.session = text(&d["session"]);
                    f.from = if truthy(&d["from"]) { text(&d["from"]) } else { String::new() };
                    f.note = if truthy(&d["note"]) { text(&d["note"]) } else { String::new() };
                    f.instructions = if truthy(&d["instructions"]) { text(&d["instructions"]) } else { String::new() };
                    f.accept = truthy(&d["accept"]).then(|| texts(&d["accept"]));
                    f.tasks = e.task.iter().cloned().chain(d["from"].as_str().map(str::to_owned)).collect();
                    f.agents = vec!["lead".into()];
                    self.decisions.push(f);
                }
            }
            EventKind::TaskSucceeded => {
                self.splits.remove(&task);
                let gaps = texts(&d["traceGaps"]);
                if e.task.is_some() {
                    if gaps.is_empty() {
                        self.gaps.remove(&task);
                    } else {
                        self.gaps.insert(task, (e.seq, gaps));
                    }
                }
                if d["acceptedBy"] == "lead" {
                    let says = d["reason"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
                    self.decisions.push(DecisionFact::new(e, DecisionKind::AcceptedByLead, says).by_lead_about(e));
                }
            }
            EventKind::UserMessage => {
                if e.actor == "lead" {
                    self.splits.remove(&task);
                }
                let images = image_paths(d).len();
                let raw = d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
                let words = if images == 0 { raw } else { visible_text(&raw) };
                let dup = d["delivery"] == "prompt"
                    && self.decisions.iter().any(|f| {
                        f.kind == DecisionKind::Message && f.to == e.to && f.text == words && f.images == images
                    });
                if !dup {
                    let mut f = DecisionFact::new(e, DecisionKind::Message, words);
                    f.images = images;
                    f.tasks = e.task.iter().cloned().collect();
                    f.agents = e.to.iter().cloned().collect();
                    f.to = e.to.clone();
                    self.decisions.push(f);
                }
                if d["id"].is_null() && (d["delivery"] == "queued" || d["delivery"] == "next") {
                    self.notes.entry(e.to.clone().unwrap_or_default()).or_default().push(e.seq);
                }
            }
            EventKind::MessageDelivered if d["ids"].is_null() => {
                self.notes.remove(e.to.as_deref().unwrap_or(""));
            }
            _ => {}
        }
    }

    /// The board at this point, with `st` the run's state there.
    // @project-doc docs/architecture/20261031-frontend.md#event_derivation
    pub(super) fn facts(&self, st: &RunState) -> BoardFacts {
        let mut f = BoardFacts {
            open: vec![],
            results: self.results.clone(),
            arts: vec![],
            handoffs: self.handoffs.clone(),
            decisions: self.decisions.clone(),
        };
        // Each result is used by whoever it was handed to; a newer attempt supersedes an older one.
        for i in 0..f.results.len() {
            let r = &self.results[i];
            let newer = |seq: i64| self.results.iter().any(|x| x.task == r.task && x.seq > r.seq && x.seq < seq);
            let used_by = self.handoffs.iter().filter(|h| h.from == r.task && h.seq > r.seq && !newer(h.seq));
            f.results[i].used_by = used_by.map(|h| h.to.clone()).collect();
            f.results[i].old = self.results.iter().any(|x| x.task == r.task && x.seq > r.seq);
            f.results[i].amended = if f.results[i].old {
                vec![]
            } else {
                st.task(&r.task).map(|t| t.amendments.clone()).unwrap_or_default()
            };
        }
        f.arts = st
            .artifacts
            .iter()
            .map(|a| {
                let old = st.artifacts.iter().any(|b| {
                    b.task == a.task
                        && if a.kind == "report" {
                            b.kind == "report" && b.attempt > a.attempt
                        } else {
                            b.kind == a.kind && b.seq > a.seq
                        }
                });
                let mut used_by: Vec<String> = vec![];
                for h in &self.handoffs {
                    if h.artifacts.contains(&a.name) && !used_by.contains(&h.to) {
                        used_by.push(h.to.clone());
                    }
                }
                ArtFact { a: a.clone(), old, used_by }
            })
            .collect();
        f.open = self.yours(st);
        self.lead_facts(st, &mut f.open);
        // Once the run is over, a message that never arrived is marked on its decision card.
        if st.run.is_done() || matches!(st.run.status, Some(RunStatus::Cancelled | RunStatus::Failed)) {
            let mut lost: HashSet<i64> = self.notes.values().flatten().copied().collect();
            for m in st.msgs.iter().filter(|m| m.delivered_seq.is_none() && m.waits()) {
                lost.insert(m.seq);
            }
            for d in &mut f.decisions {
                d.lost = lost.contains(&d.seq);
            }
        }
        f.open.sort_by(|a, b| b.you.cmp(&a.you).then(b.seq.unwrap_or(0).cmp(&a.seq.unwrap_or(0))));
        f
    }

    /// What waits on the person: the requests, a Loop's last wake that changed files or failed,
    /// and a lead that may be stuck.
    pub(super) fn yours(&self, st: &RunState) -> Vec<OpenFact> {
        let mut open: Vec<OpenFact> = st.inbox.iter().map(|r| request_fact(st, r)).collect();
        if let Some((seq, ts, kind, says, files)) = &self.loop_notice {
            let mut o = OpenFact::new(OpenKey::Loop(*seq), *kind, Some(*seq), true).about(&[], &["lead"]).says(says);
            o.lines = files.clone();
            o.ts = Some(*ts);
            open.push(o);
        }
        if let Some((seq, ts, says)) = &self.lead_stall
            && st.run.active()
        {
            let mut o = OpenFact::new(OpenKey::LeadStall(*seq), OpenKind::LeadStall, Some(*seq), true);
            o.ts = Some(*ts);
            open.push(o.about(&[], &["lead"]).says(says));
        }
        open
    }

    /// Everything else that holds work up: the lead's to settle, not the person's.
    fn lead_facts(&self, st: &RunState, open: &mut Vec<OpenFact>) {
        let status = |task: &str| st.task(task).map(|t| t.status);
        let agent = |task: &str| st.task(task).map(|t| t.agent.clone()).unwrap_or_default();
        // A shared edit that an overstep already covers is the same fact: keep the overstep.
        let covered = |c: &Conflict| {
            !c.owned
                && self.conflicts.iter().any(|o| {
                    o.owned
                        && o.by_lead == c.by_lead
                        && HashSet::from([&o.task, &o.other]) == HashSet::from([&c.task, &c.other])
                        && c.paths.iter().all(|p| o.paths.contains(p))
                })
        };
        for c in self.conflicts.iter().filter(|c| !covered(c)) {
            let kind = if c.owned { OpenKind::Overstep } else { OpenKind::Conflict };
            let fact = if c.by_lead {
                if status(&c.task).is_none_or(TaskShown::is_ended) {
                    continue;
                }
                OpenFact::new(OpenKey::Conflict(c.seq), kind, Some(c.seq), false)
                    .about(&[&c.task], &[&agent(&c.task), "lead"])
            } else {
                let running = |t: &str| status(t) == Some(TaskShown::Running);
                if !(if c.owned { running(&c.task) } else { running(&c.task) || running(&c.other) }) {
                    continue;
                }
                OpenFact::new(OpenKey::Conflict(c.seq), kind, Some(c.seq), false)
                    .about(&[&c.task, &c.other], &[&agent(&c.task), &agent(&c.other), "lead"])
            };
            let mut fact = fact;
            fact.other = c.other.clone();
            fact.by_lead = c.by_lead;
            fact.lines = c.paths.clone();
            open.push(fact);
        }
        let by_seq = |m: &HashMap<String, (i64, String)>| {
            let mut v: Vec<(String, i64, String)> = m.iter().map(|(k, (s, t))| (k.clone(), *s, t.clone())).collect();
            v.sort_by_key(|x| x.1);
            v
        };
        for (task, seq, says) in by_seq(&self.splits) {
            if status(&task).is_none_or(TaskShown::is_ended) {
                continue;
            }
            let o = OpenFact::new(OpenKey::Split(&task, seq), OpenKind::SplitRequest, Some(seq), false);
            open.push(o.about(&[&task], &[&agent(&task), "lead"]).says(says));
        }
        // Until the lead redoes or corrects the task, or the run is handed over.
        if !(st.run.is_done() || st.run.status == Some(RunStatus::Cancelled)) {
            let mut gaps: Vec<_> = self.gaps.iter().collect();
            gaps.sort_by_key(|(_, (seq, _))| *seq);
            for (task, (seq, lines)) in gaps {
                if status(task) != Some(TaskShown::Succeeded) {
                    continue;
                }
                let mut o = OpenFact::new(OpenKey::Gap(*seq), OpenKind::Trace, Some(*seq), false)
                    .about(&[task], &[&agent(task), "lead"]);
                o.lines = lines.clone();
                open.push(o);
            }
        }
        for (task, seq, says) in by_seq(&self.stalls) {
            if status(&task) != Some(TaskShown::Running) {
                continue;
            }
            let o = OpenFact::new(OpenKey::Stall(&task, seq), OpenKind::Stall, Some(seq), false);
            open.push(o.about(&[&task], &[&agent(&task), "lead"]).says(says));
        }
        for (seq, task, says, actor) in &self.blockers {
            if status(task).is_some_and(TaskShown::is_ended) {
                continue;
            }
            let o = OpenFact::new(OpenKey::Blocker(*seq), OpenKind::Blocker, Some(*seq), false);
            open.push(o.about(&[task], &[actor, "lead"]).says(says));
        }
        for t in st.tasks.iter().filter(|t| t.status == TaskShown::WaitingInput) {
            let Some(q) = t.question.as_ref().filter(|q| q.answer.is_none()) else { continue };
            if st.inbox.iter().any(|r| r.kind != RequestKind::Session && r.key == q.key) {
                continue;
            }
            let o = OpenFact::new(OpenKey::LeadQuestion(&t.id, q.seq), OpenKind::LeadQuestion, Some(q.seq), false);
            open.push(o.about(&[&t.id], &[&t.agent, "lead"]).says(&q.question));
        }
        for t in st.tasks.iter().filter(|t| matches!(t.status, TaskShown::Failed | TaskShown::VerificationFailed)) {
            let seq = t.attempts.last().map(|a| a.seq);
            let mut o = OpenFact::new(OpenKey::Failed(&t.id), OpenKind::Failed, seq, false)
                .about(&[&t.id], &[&t.agent, "lead"])
                .says(&t.error);
            o.lines = t.verification_failures.clone();
            open.push(o);
        }
        for t in st.tasks.iter().filter(|t| t.status == TaskShown::Blocked) {
            let mut o = OpenFact::new(OpenKey::Blocked(&t.id), OpenKind::Blocked, None, false)
                .about(&[&t.blocked_by, &t.id], &[&t.agent, "lead"]);
            o.other = t.blocked_by.clone();
            open.push(o);
        }
    }
}

/// A request of the person's, as an open fact: who asks, what, and the answers one click gives.
fn request_fact(st: &RunState, r: &Request) -> OpenFact {
    let t = r.task.as_deref().and_then(|id| st.task(id));
    let kind = match r.kind {
        RequestKind::Question => OpenKind::Question,
        RequestKind::Approval => OpenKind::Approval,
        RequestKind::Input => OpenKind::Input,
        RequestKind::Session => OpenKind::Session,
    };
    let who = t.map(|t| t.agent.clone()).or_else(|| r.agent.clone()).unwrap_or_default();
    let mut o = OpenFact::new(OpenKey::Request { seq: r.seq, kind: r.kind.as_str() }, kind, Some(r.seq), true);
    o.tasks = r.task.iter().cloned().collect();
    o.agents = vec![if who.is_empty() { "lead".into() } else { who.clone() }];
    o.who = who;
    o.ts = Some(r.ts);
    o.decision = r.key.clone();
    if let Some(t) = t {
        o.task_title = t.title.clone();
    }
    o.recommend = text(&r.d["recommend"]);
    match r.kind {
        RequestKind::Question if r.task.is_some() => {
            let q = t.and_then(|t| t.question.as_ref());
            o.options = Value::from(q.map(|q| q.options.clone()).unwrap_or_default());
            o.text = q.map(|q| q.question.clone()).unwrap_or_default();
        }
        RequestKind::Question => {
            o.question = text(&r.d["question"]);
            o.options = Value::from(texts(&r.d["options"]));
        }
        RequestKind::Approval => {
            o.options = r.d["options"].clone();
            o.text = text(&r.d["command"]);
        }
        RequestKind::Input => {
            o.mode = text(&r.d["mode"]);
            o.text = text(&r.d["message"]);
        }
        RequestKind::Session => {}
    }
    if o.text.is_empty() {
        o.text = text(&r.d["reason"]);
    }
    o
}
