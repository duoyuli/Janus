//! The run page as a group chat: what the team said and did, as posts in the order it happened.
//! The group is the lead's conversation with the posts everyone should see put in at their place
//! (`merge`): tasks dispatched, started and handed over, results and verdicts, rework, conflicts,
//! what got stuck, what waits on the person, and each delivery. Each task's topic holds everything
//! said about it, the back and forth between its collaborator and the lead included. Written here
//! for every client (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#run_group

use super::card::Tone;
use super::review::{Review, ReviewStage};
use super::turns::{Gap, Row};
use super::words::Words;
use super::{RunState, list, text, texts, truthy, visible_text};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::tr;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

crate::wire::wire_enum! {
    /// What a post is.
    pub enum PostKind {
        /// The lead put tasks on the board (`tasks`).
        Dispatch = "dispatch",
        /// An attempt at a task started.
        Start = "start",
        /// A task's result, or a verification's verdict (`checks` by its seq).
        Result = "result",
        /// A task failed, or an attempt at it did.
        Failed = "failed",
        /// The lead sent a task back.
        Rework = "rework",
        /// The lead cancelled a task, revised its conclusion or took its result itself.
        Lead = "lead",
        /// Two tasks, or the lead and a task, edited the same files.
        Conflict = "conflict",
        /// A task or the lead seems stuck, or a task waits for a dependency that failed.
        Stuck = "stuck",
        /// A collaborator's post on the board: progress, a note, a blocker.
        Note = "note",
        /// A collaborator asks the lead, or proposes to split its task.
        Question = "question",
        /// A message to a collaborator from the person or the lead, or an answer to its question.
        Message = "message",
        /// Something that waits on the person (`key`): a question passed on, an approval, a form.
        Ask = "ask",
        /// A delivery (`n`).
        Delivery = "delivery",
    }
}

/// A task a dispatch put on the board, and what it covers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PostTask {
    pub id: String,
    /// 「负责 ledger/api.py · 依赖 T1」 or 「独立验证 T2」.
    pub scope: String,
}

/// One post.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub seq: i64,
    pub ts: i64,
    pub kind: PostKind,
    /// Who says it: a member's id, `user` or `janus`.
    pub by: String,
    /// The task it is about: its topic shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The other task a conflict is about: its topic shows it too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    /// In the group too, not only in its task's topic.
    pub group: bool,
    /// What it is, in a few words: 「交了结果」「验证没通过 T2」. Empty for plain words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// What it says: the agent's own words, or Janus's line.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The lines under it: a handoff, a question's options, an answer's note.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// A result's numbers: 「验收 3/3 · 测试 2 条、检查 1 条」.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub meta: String,
    /// A dispatch's tasks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<PostTask>,
    /// The decision an ask is (`RequestCard::key`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// How an ask was settled, once it is no longer open: 「你回答：…」「已批准」.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub settled: String,
    /// A delivery's number, from 1.
    #[serde(skip_serializing_if = "is_zero")]
    pub n: usize,
    /// Waits on the person now: an open ask, the delivery awaiting acceptance. Drawn as a mention.
    pub you: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The posts besides the group: each task's topic, and what waits on the person.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Feed {
    /// Everything said about each task, by its id, oldest first.
    pub topics: BTreeMap<String, Vec<Post>>,
    /// The posts in the group that wait on the person, oldest first: where 「@你」 jumps.
    pub mentions: Vec<i64>,
}

/// The event kinds posts are made from.
const KINDS: &[EventKind] = &[
    EventKind::OrchestrationAccepted,
    EventKind::ContextHandoff,
    EventKind::AttemptStarted,
    EventKind::AttemptFailed,
    EventKind::VerificationFailed,
    EventKind::TaskSucceeded,
    EventKind::TaskFailed,
    EventKind::TaskReopened,
    EventKind::TaskSkippedByDecision,
    EventKind::ResultAmended,
    EventKind::TaskConflict,
    EventKind::TaskStalled,
    EventKind::LeadStalled,
    EventKind::TaskBlocked,
    EventKind::BoardPosted,
    EventKind::QuestionAsked,
    EventKind::QuestionAnswered,
    EventKind::TaskSplitRequested,
    EventKind::UserMessage,
    EventKind::QuestionEscalated,
    EventKind::ApprovalRequested,
    EventKind::InputRequested,
    EventKind::RunDelivered,
];

/// What the feed keeps while the events go by: the events posts are made from. Posts are worded
/// when the view is asked for, in the person's language and with the state at that point.
#[derive(Clone, Default)]
pub(super) struct FeedFold {
    events: Vec<Event>,
}

impl FeedFold {
    pub(super) fn apply(&mut self, e: &Event) {
        if KINDS.contains(&e.kind()) {
            self.events.push(e.clone());
        }
    }

    /// Every post at this point, oldest first.
    pub(super) fn posts(&self, st: &RunState, review: Option<&Review>, w: &Words) -> Vec<Post> {
        let mut out = vec![];
        // The handoffs a task's next attempt starts with, by task.
        let mut handed: HashMap<String, Vec<String>> = HashMap::new();
        let delivered = st.deliveries.len();
        for e in &self.events {
            let d = e.data.as_ref().unwrap_or(&Value::Null);
            if e.kind() == EventKind::ContextHandoff {
                let from = text(&d["from"]);
                if let Some(t) = &e.task
                    && !from.is_empty()
                {
                    handed.entry(t.clone()).or_default().push(from);
                }
                continue;
            }
            if let Some(mut p) = post(e, d, st, &mut handed, w) {
                p.you = match p.kind {
                    PostKind::Ask => st.inbox.iter().any(|r| r.key == p.key && !r.key.is_empty()),
                    PostKind::Delivery => p.n == delivered && review.is_some_and(|r| r.stage == ReviewStage::Awaiting),
                    _ => false,
                };
                if p.kind == PostKind::Ask && !p.you {
                    p.settled = settled(st, &p.key, e.kind(), w);
                }
                out.push(p);
            }
        }
        out
    }
}

/// The post one event makes, if any.
fn post(e: &Event, d: &Value, st: &RunState, handed: &mut HashMap<String, Vec<String>>, w: &Words) -> Option<Post> {
    let l = w.lang;
    let task = e.task.clone().filter(|t| st.task(t).is_some());
    let name = |id: &str| w.name(id);
    let mut p = Post {
        seq: e.seq,
        ts: e.ts,
        kind: PostKind::Note,
        by: e.actor.clone(),
        task: task.clone(),
        other: None,
        group: false,
        title: String::new(),
        text: String::new(),
        lines: vec![],
        meta: String::new(),
        tasks: vec![],
        key: String::new(),
        settled: String::new(),
        n: 0,
        you: false,
        tone: None,
    };
    let id = task.clone().unwrap_or_default();
    match e.kind() {
        EventKind::OrchestrationAccepted => {
            p.kind = PostKind::Dispatch;
            p.by = "lead".into();
            p.group = true;
            p.tasks = list(&d["tasks"])
                .iter()
                .map(|x| {
                    let tid = text(&x["id"]);
                    PostTask { scope: scope(st, &tid, w), id: tid }
                })
                .collect();
            let n = p.tasks.len();
            p.title = tr!(l, "派了 {n} 件事", "Dispatched {n}");
        }
        EventKind::AttemptStarted => {
            let t = st.task(&id)?;
            p.kind = PostKind::Start;
            p.by = t.agent.clone();
            p.group = true;
            let who = name(&t.agent);
            let n = e.attempt.unwrap_or(1);
            let from = handed.remove(&id).unwrap_or_default();
            p.text = match (n > 1, t.verifies.is_empty()) {
                (true, false) => tr!(l, "{who} 复审 {id}", "{who} verifies {id} again"),
                (true, true) => tr!(l, "{who} 第 {n} 次做 {id}", "{who} starts attempt {n} at {id}"),
                _ => tr!(l, "{who} 开始 {id}", "{who} starts {id}"),
            };
            if !from.is_empty() {
                let from = from.join(w.and());
                p.text = tr!(l, "{from} 的结果交给了 {id} · ", "{from} handed over to {id} · ") + &p.text;
            }
        }
        EventKind::AttemptFailed => {
            p.kind = PostKind::Failed;
            p.by = "janus".into();
            let n = e.attempt.unwrap_or(1);
            p.text = tr!(l, "第 {n} 次没做成：{}", "Attempt {n} did not finish: {}", short(&text(&d["error"])));
            p.tone = Some(Tone::Amber);
            task.as_ref()?;
        }
        EventKind::VerificationFailed => {
            p.kind = PostKind::Failed;
            p.by = "janus".into();
            p.text = l.pick("交上来的结果没通过核对", "The result handed in did not pass the checks").into();
            p.lines = texts(&d["failures"]);
            p.tone = Some(Tone::Amber);
            task.as_ref()?;
        }
        EventKind::TaskSucceeded => {
            let t = st.task(&id)?;
            p.kind = PostKind::Result;
            p.by = t.agent.clone();
            p.group = true;
            p.text = text(&d["summary"]);
            let verdict = text(&d["verdict"]);
            let target = &t.verifies;
            (p.title, p.tone) = match verdict.as_str() {
                "approved" => (tr!(l, "验证通过 {target}", "Verified {target}"), Some(Tone::Green)),
                "rejected" => (tr!(l, "验证没通过 {target}", "{target} failed verification"), Some(Tone::Red)),
                "inconclusive" => (tr!(l, "验证没有结论 {target}", "No verdict on {target}"), Some(Tone::Amber)),
                _ if d["acceptedBy"] == "lead" => {
                    (l.pick("负责人接受了结果", "The lead took the result").into(), Some(Tone::Amber))
                }
                _ => (l.pick("交了结果", "Handed in").into(), Some(Tone::Green)),
            };
            if d["acceptedBy"] == "lead" {
                p.by = "lead".into();
                p.text = d["reason"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
            }
            let handoff = text(&d["handoff"]);
            if !handoff.is_empty() {
                p.lines.push(tr!(l, "交接给下游：{handoff}", "For downstream: {handoff}"));
            }
            for r in texts(&d["risks"]) {
                p.lines.push(tr!(l, "风险：{r}", "Risk: {r}"));
            }
            for g in texts(&d["traceGaps"]) {
                p.lines.push(tr!(l, "对不上：{g}", "Mismatch: {g}"));
            }
            p.meta = checks_line(list(&d["checks"]), w);
        }
        EventKind::TaskFailed => {
            p.kind = PostKind::Failed;
            p.by = "janus".into();
            p.group = true;
            p.tone = Some(Tone::Red);
            p.text = if d["reason"] == "verification" {
                tr!(l, "{id} 没通过验收，不会自动重试", "{id} failed its acceptance and will not be retried")
            } else {
                tr!(l, "{id} 失败了：{}", "{id} failed: {}", short(&text(&d["error"])))
            };
            task.as_ref()?;
        }
        EventKind::TaskReopened => {
            task.as_ref()?;
            if d["reason"] == "lead.retry" {
                p.kind = PostKind::Rework;
                p.by = "lead".into();
                p.group = true;
                p.tone = Some(Tone::Amber);
                let from = text(&d["from"]);
                p.title = if from.is_empty() {
                    tr!(l, "返工 {id}", "Rework {id}")
                } else {
                    tr!(l, "按 {from} 的意见返工 {id}", "Rework {id} as {from} found")
                };
                p.text = [text(&d["note"]), text(&d["instructions"])]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .unwrap_or_default();
            } else {
                p.kind = PostKind::Start;
                p.by = "janus".into();
                let target = text(&d["target"]);
                p.text = if d["reason"] == "target.reworked" {
                    tr!(
                        l,
                        "{target} 返工了，{id} 等它做完再复审",
                        "{target} is being reworked; {id} checks it again after"
                    )
                } else {
                    tr!(l, "上游重做，{id} 也重新开始", "Upstream is redone, so {id} starts over")
                };
            }
        }
        EventKind::TaskSkippedByDecision => {
            task.as_ref()?;
            p.kind = PostKind::Lead;
            p.by = if e.actor == "lead" { "lead".into() } else { "janus".into() };
            p.group = true;
            let reason = text(&d["reason"]);
            p.text = if reason.is_empty() {
                tr!(l, "取消了 {id}", "Cancelled {id}")
            } else {
                tr!(l, "取消了 {id}：{reason}", "Cancelled {id}: {reason}")
            };
        }
        EventKind::ResultAmended => {
            task.as_ref()?;
            p.kind = PostKind::Lead;
            p.by = "lead".into();
            p.group = true;
            p.title = tr!(l, "修订了 {id} 的结论", "Revised {id}'s conclusion");
            p.text = d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
        }
        EventKind::TaskConflict => {
            task.as_ref()?;
            p.kind = PostKind::Conflict;
            p.by = "janus".into();
            p.group = true;
            p.tone = Some(Tone::Amber);
            let other = text(&d["other"]);
            let paths = texts(&d["paths"]).join(w.and());
            p.text = if truthy(&d["byLead"]) {
                tr!(l, "负责人改了 {id} 在改的 {paths}", "The lead edited {paths}, which {id} is working on")
            } else if truthy(&d["owned"]) {
                tr!(
                    l,
                    "{id} 改了 {other} 负责的 {paths}，已提醒它别再改",
                    "{id} edited {paths}, which {other} owns; told it to stop"
                )
            } else {
                tr!(
                    l,
                    "{id} 和 {other} 都改了 {paths}，已提醒双方先读最新内容",
                    "{id} and {other} both edited {paths}; told both to read the latest first"
                )
            };
            p.other = Some(other).filter(|o| st.task(o).is_some());
        }
        EventKind::TaskStalled => {
            if truthy(&d["resolved"]) {
                return None;
            }
            task.as_ref()?;
            p.kind = PostKind::Stuck;
            p.by = "janus".into();
            p.group = true;
            p.tone = Some(Tone::Amber);
            p.text = stalled(&id, d, w);
        }
        EventKind::LeadStalled => {
            if truthy(&d["resolved"]) {
                return None;
            }
            p.kind = PostKind::Stuck;
            p.by = "janus".into();
            p.task = None;
            p.group = true;
            p.tone = Some(Tone::Amber);
            p.text = stalled(&name("lead"), d, w);
        }
        EventKind::TaskBlocked => {
            task.as_ref()?;
            p.kind = PostKind::Stuck;
            p.by = "janus".into();
            p.group = true;
            p.tone = Some(Tone::Red);
            let dep = text(&d["dependency"]);
            p.text = tr!(l, "{id} 等的 {dep} 没做成，停在这里", "{id} waits for {dep}, which did not finish");
        }
        EventKind::BoardPosted => {
            task.as_ref()?;
            p.kind = PostKind::Note;
            p.text = d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
            p.title = match d["kind"].as_str() {
                Some("blocker") => {
                    p.group = true;
                    p.tone = Some(Tone::Amber);
                    l.pick("阻塞", "Blocker")
                }
                Some("note") => l.pick("备注", "Note"),
                _ => l.pick("进展", "Progress"),
            }
            .into();
        }
        EventKind::QuestionAsked => {
            task.as_ref()?;
            p.kind = PostKind::Question;
            p.title = l.pick("提问", "Question").into();
            p.text = text(&d["question"]);
            p.lines = texts(&d["options"]);
        }
        EventKind::TaskSplitRequested => {
            task.as_ref()?;
            p.kind = PostKind::Question;
            let n = list(&d["subtasks"]).len();
            p.title = tr!(l, "提议拆成 {n} 件", "Proposes splitting into {n}");
            p.text = d["reason"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone());
        }
        EventKind::QuestionAnswered => {
            task.as_ref()?;
            p.kind = PostKind::Message;
            p.title = l.pick("回答", "Answer").into();
            p.text = text(&d["answer"]);
            p.lines = [text(&d["note"])].into_iter().filter(|s| !s.is_empty()).collect();
        }
        // A message to a collaborator with its own id: the person's or the lead's, once; Janus's
        // notices to it stay in its conversation.
        EventKind::UserMessage => {
            task.as_ref()?;
            if d["id"].is_null() || !["user", "lead"].contains(&e.actor.as_str()) {
                return None;
            }
            p.kind = PostKind::Message;
            p.group = e.actor == "user";
            p.text = visible_text(&d["text"].as_str().map(str::to_owned).unwrap_or_else(|| e.text.clone()));
        }
        EventKind::QuestionEscalated => {
            p.kind = PostKind::Ask;
            p.by = "lead".into();
            p.group = true;
            p.key = text(&d["key"]);
            p.text = match task.as_deref().and_then(|t| st.task(t)).and_then(|t| t.question.as_ref()) {
                Some(q) if d["question"].is_null() => q.question.clone(),
                _ => text(&d["question"]),
            };
            let reason = text(&d["reason"]);
            if !reason.is_empty() {
                p.lines.push(reason);
            }
        }
        EventKind::ApprovalRequested | EventKind::InputRequested => {
            // The lead's own requests are in its conversation already.
            task.as_ref()?;
            p.kind = PostKind::Ask;
            p.group = true;
            p.key = text(&d["key"]);
            p.title = if e.kind() == EventKind::ApprovalRequested {
                l.pick("请你批准", "Needs your approval")
            } else {
                l.pick("请你填写", "Needs your input")
            }
            .into();
            p.text = match text(&d["message"]) {
                m if !m.is_empty() => m,
                _ => e.text.clone(),
            };
        }
        EventKind::RunDelivered => {
            p.kind = PostKind::Delivery;
            p.by = "lead".into();
            p.task = None;
            p.group = true;
            p.n = st.deliveries.iter().find(|r| r.seq == e.seq)?.n;
            // Its conclusion in a line, for when a later delivery took its place.
            p.text = [text(&d["headline"]), text(&d["summary"])]
                .into_iter()
                .map(|s| super::goal_title(&s))
                .find(|s| !s.is_empty())
                .unwrap_or_default();
        }
        _ => return None,
    }
    Some(p)
}

/// What a task covers, in a line: the files it owns and the tasks it waits for, or the task it
/// verifies.
fn scope(st: &RunState, id: &str, w: &Words) -> String {
    let l = w.lang;
    let Some(t) = st.task(id) else { return String::new() };
    if !t.verifies.is_empty() {
        let v = &t.verifies;
        return tr!(l, "独立验证 {v}", "Verifies {v}");
    }
    let mut parts = vec![];
    if !t.owns.is_empty() {
        let owns = t.owns.join(w.and());
        parts.push(tr!(l, "负责 {owns}", "Owns {owns}"));
    }
    if !t.deps.is_empty() {
        let deps = t.deps.join(w.and());
        parts.push(tr!(l, "依赖 {deps}", "After {deps}"));
    }
    parts.join(" · ")
}

/// A result's acceptance in a line: how many criteria passed, and by which methods.
fn checks_line(checks: &[Value], w: &Words) -> String {
    let l = w.lang;
    if checks.is_empty() {
        return String::new();
    }
    let n = checks.len();
    let ok = checks.iter().filter(|c| truthy(&c["passed"])).count();
    let mut counts: Vec<(&str, usize)> = vec![];
    for c in checks {
        let m = c["method"].as_str().unwrap_or("");
        let label = match m {
            "test" => l.pick("测试", "test"),
            "command" => l.pick("命令", "command"),
            "inspection" => l.pick("检查", "inspection"),
            "reasoning" => l.pick("推理", "reasoning"),
            "unverified" => l.pick("未核实", "unverified"),
            _ => continue,
        };
        match counts.iter_mut().find(|(k, _)| *k == label) {
            Some(x) => x.1 += 1,
            None => counts.push((label, 1)),
        }
    }
    let methods: Vec<String> = counts.iter().map(|(k, c)| tr!(l, "{k} {c} 条", "{c} {k}")).collect();
    let head = tr!(l, "验收 {ok}/{n}", "Criteria {ok}/{n}");
    if methods.is_empty() { head } else { format!("{head} · {}", methods.join(w.and())) }
}

/// What a stall report says about `who`.
fn stalled(who: &str, d: &Value, w: &Words) -> String {
    let l = w.lang;
    if d["reason"] == "loop" {
        let tool = text(&d["tool"]);
        let n = d["repeats"].as_i64().unwrap_or(0);
        tr!(
            l,
            "{who} 在重复同一个调用（{tool}，{n} 次），可能卡住了",
            "{who} keeps repeating one call ({tool}, {n} times) and may be stuck"
        )
    } else {
        let mins = d["idleMs"].as_i64().unwrap_or(0) / 60_000;
        tr!(l, "{who} {mins} 分钟没有动静，可能卡住了", "{who} has been quiet for {mins} min and may be stuck")
    }
}

/// How an ask no longer open was settled.
fn settled(st: &RunState, key: &str, kind: EventKind, w: &Words) -> String {
    let l = w.lang;
    match st.answers.get(key) {
        Some(Value::String(s)) if kind == EventKind::ApprovalRequested => {
            if s == "granted" { l.pick("已批准", "Approved") } else { l.pick("已拒绝", "Denied") }.into()
        }
        Some(_) if kind == EventKind::InputRequested => l.pick("已回复", "Answered").into(),
        Some(v) => {
            let a = match v {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            tr!(l, "回答：{}", "Answered: {}", short(&a))
        }
        None => l.pick("已撤回", "Withdrawn").into(),
    }
}

fn short(t: &str) -> String {
    super::short(t.trim(), 200)
}

/// The rows of the lead's conversation with the group's posts put in at their place: before the
/// first row that starts after them. A post stands apart like a turn, and so does the row after it.
/// The board's pushes into the lead's turn say again what the posts say, so the group leaves them
/// out (`pushes`: their seqs).
pub(super) fn merge(rows: &mut Vec<Row>, posts: &[Post], pushes: &[i64]) {
    if posts.iter().all(|p| !p.group) {
        return;
    }
    rows.retain(|r| !matches!(r, Row::Item { seq, .. } if pushes.contains(seq)));
    let start = |r: &Row| -> i64 {
        match r {
            Row::Section { seq, .. } | Row::Item { seq, .. } => *seq,
            Row::Head { turn, .. } | Row::Edits { turn, .. } => *turn,
            Row::Activity { events, .. } | Row::Text { events, .. } => events.first().copied().unwrap_or(i64::MAX),
            Row::Post { post, .. } => post.seq,
        }
    };
    let row = |p: &Post| Row::Post { id: super::turns::post_id(p.seq), post: Box::new(p.clone()), gap: Gap::Round };
    let mut out = Vec::with_capacity(rows.len() + posts.len());
    let mut it = posts.iter().filter(|p| p.group).peekable();
    for mut r in rows.drain(..) {
        let at = start(&r);
        let mut after_post = false;
        while let Some(p) = it.next_if(|p| p.seq < at) {
            out.push(row(p));
            after_post = true;
        }
        if after_post {
            lift(&mut r);
        }
        out.push(r);
    }
    out.extend(it.map(row));
    if let Some(first) = out.first_mut() {
        set_gap(first, Gap::None);
    }
    *rows = out;
}

/// The row after a post stands apart from it: at least a block's space above it.
fn lift(r: &mut Row) {
    let g = gap_of(r);
    if matches!(g, Gap::None | Gap::Line | Gap::Half | Gap::Loose) {
        set_gap(r, Gap::Block);
    }
}

fn gap_of(r: &Row) -> Gap {
    match r {
        Row::Section { gap, .. }
        | Row::Item { gap, .. }
        | Row::Head { gap, .. }
        | Row::Activity { gap, .. }
        | Row::Text { gap, .. }
        | Row::Edits { gap, .. }
        | Row::Post { gap, .. } => *gap,
    }
}

fn set_gap(r: &mut Row, to: Gap) {
    match r {
        Row::Section { gap, .. }
        | Row::Item { gap, .. }
        | Row::Head { gap, .. }
        | Row::Activity { gap, .. }
        | Row::Text { gap, .. }
        | Row::Edits { gap, .. }
        | Row::Post { gap, .. } => *gap = to,
    }
}

/// The feed beside the group: each task's topic and the posts waiting on the person.
pub(super) fn feed(posts: &[Post]) -> Feed {
    let mut topics: BTreeMap<String, Vec<Post>> = BTreeMap::new();
    for p in posts {
        for t in p.task.iter().chain(p.other.iter()) {
            topics.entry(t.clone()).or_default().push(p.clone());
        }
    }
    let mentions = posts.iter().filter(|p| p.group && p.you).map(|p| super::turns::post_id(p.seq)).collect();
    Feed { topics, mentions }
}
