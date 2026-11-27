//! A conversation in turns, the way Lody shows one: each turn opens with what was sent to the
//! agent, its work (thinking, tool calls, notes along the way) folds to one line once it is done,
//! and the reply that ended it stays in view with the files it changed. What must not be missed
//! (requests, results, errors, messages cut into the turn, Janus's warnings) never folds.
//!
//! The rows are worked out at one display level (`detail`) with nothing opened or folded by hand;
//! each row says what the person's own click changes (`fold`, `open`, `whole`), so a client only
//! applies its clicks and never regroups the events.
// @project-doc docs/architecture/20261031-frontend.md#session_turns

use super::{RunState, list, text, truthy};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// What an event is to a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    /// Starts a section of the conversation: the run, an attempt, a Loop's wake.
    Section,
    /// What Janus put into the session's system layer, listed under its section.
    Injected,
    /// Sent to the agent as a new turn: a person's message, a prompt from Janus.
    Opener,
    /// Stays in view when the turn folds.
    Pinned,
    Tool,
    Message,
    /// The rest of the agent's work: thinking, status, switches, board notes.
    Work,
}

fn data(e: &Event) -> &Value {
    e.data.as_ref().unwrap_or(&Value::Null)
}

fn present(v: &Value) -> bool {
    !v.is_null()
}

pub(super) fn role(e: &Event) -> Role {
    let d = data(e);
    match e.kind() {
        EventKind::RunCreated | EventKind::AttemptStarted | EventKind::LoopWoke => Role::Section,
        EventKind::ContextHandoff => Role::Injected,
        EventKind::SessionPrompt => match d["channel"].as_str().unwrap_or("") {
            // The system layer: how to work with the team, the employee's own instructions, a
            // reopened chat's recent exchanges.
            "system" | "instructions" | "history" => Role::Injected,
            // Pushed into the turn already running, like a message cut into it.
            "board" => Role::Pinned,
            _ => Role::Opener,
        },
        EventKind::UserMessage => Role::Opener,
        EventKind::MessageRouted => {
            if d["delivery"] == "steer" {
                Role::Pinned
            } else {
                Role::Opener
            }
        }
        EventKind::MessageDelivered => {
            if present(&d["ids"]) {
                Role::Opener
            } else {
                Role::Pinned
            }
        }
        EventKind::ToolCall | EventKind::ToolStarted | EventKind::ToolCompleted => Role::Tool,
        EventKind::AgentMessage => Role::Message,
        // The session's own status lines (`agent.status` once a turn is idle) say where it stands: they stay.
        EventKind::AgentThought
        | EventKind::SessionControl
        | EventKind::BoardPosted
        | EventKind::QuestionAnswered
        | EventKind::ApprovalGranted
        | EventKind::ApprovalDenied
        | EventKind::InputAnswered => Role::Work,
        _ => Role::Pinned,
    }
}

/// Whether a conversation shows `e` at all, besides its tool calls (`tool_hidden`).
/// `conflicts`: every conflict recorded in the run.
fn visible(e: &Event, st: &RunState, conflicts: &[&Event]) -> bool {
    let d = data(e);
    let msg = |id: &str| st.msgs.iter().find(|m| m.id == id);
    match e.kind() {
        EventKind::UserMessage => !present(&d["id"]),
        // A Loop has no goal to open its session with; each wake is marked where it starts.
        EventKind::RunCreated => d["kind"] != "loop",
        // Shown beside the conversation (plan, controls, commands, usage), not in it.
        EventKind::AgentPlan | EventKind::SessionControls | EventKind::SessionCommands | EventKind::SessionUsage => {
            false
        }
        // Stepping on another task's files says more than "both edited it": show only that.
        EventKind::TaskConflict => {
            if truthy(&d["owned"]) {
                return true;
            }
            let pair = |x: &Event| -> BTreeSet<String> {
                [x.task.clone().unwrap_or_default(), text(&data(x)["other"])].into_iter().collect()
            };
            let paths: HashSet<String> = super::texts(&d["paths"]).into_iter().collect();
            !conflicts.iter().any(|x| {
                let xd = data(x);
                truthy(&xd["owned"]) && pair(x) == pair(e) && {
                    let theirs: HashSet<String> = super::texts(&xd["paths"]).into_iter().collect();
                    paths.is_subset(&theirs)
                }
            })
        }
        EventKind::MessageRouted => {
            msg(&text(&d["id"])).is_some() && ["steer", "prompt"].contains(&text(&d["delivery"]).as_str())
        }
        // A steered message is shown where it was inserted; applying it only updates that bubble.
        EventKind::MessageDelivered => {
            if !present(&d["ids"]) {
                return true;
            }
            super::texts(&d["ids"]).iter().any(|id| msg(id).is_some_and(|m| m.delivery != "steer"))
        }
        _ => true,
    }
}

/// A tool call the conversation leaves out: an agent's echo of Janus's own board tools (Codex
/// names them `mcp.janus.*`, Claude Code `mcp__janus__*`; Janus's own record is the one shown), and
/// a call still marked running once it came back (`done`: `actor:callId`).
fn tool_hidden(e: &Event, done: &HashSet<String>) -> bool {
    let d = data(e);
    let tool = text(&d["tool"]);
    let mcp = |s: &str| s.starts_with("mcp.janus.") || s.starts_with("mcp__janus__");
    let kind = e.kind();
    let echo = matches!(kind, EventKind::ToolCall | EventKind::ToolStarted)
        && !tool.starts_with("janus.")
        && (mcp(&e.text) || mcp(&tool) || text(&d["input"]).starts_with("{\"arguments\""));
    echo || (kind == EventKind::ToolStarted && done.contains(&format!("{}:{}", e.actor, text(&d["callId"]))))
}

/// The events of a conversation it shows as rows: tool calls not left out, the rest visible.
pub(super) fn shown<'a>(evs: &[&'a Event], st: &RunState, conflicts: &[&Event]) -> Vec<&'a Event> {
    // Calls that came back: Janus's board tools (`tool.completed`) and the agent's own (`tool.call`
    // with the id its running row had).
    let done: HashSet<String> = evs
        .iter()
        .filter(|e| {
            e.kind() == EventKind::ToolCompleted || (e.kind() == EventKind::ToolCall && truthy(&data(e)["callId"]))
        })
        .map(|e| format!("{}:{}", e.actor, text(&data(e)["callId"])))
        .collect();
    evs.iter()
        .copied()
        .filter(|e| match e.kind() {
            EventKind::ToolCall | EventKind::ToolStarted | EventKind::ToolCompleted => !tool_hidden(e, &done),
            _ => visible(e, st, conflicts),
        })
        .collect()
}

/// What a turn's tools did, counted for its folded line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stats {
    pub commands: usize,
    pub reads: usize,
    pub searches: usize,
    pub edited: usize,
    pub other: usize,
    /// Janus's own tools: the board, or a Loop's duty tools.
    pub board: usize,
}

impl Stats {
    /// Over the tool calls among `items`; a file named by its path in `cwd` and by its full path
    /// is one file.
    pub(super) fn of(items: &[&Event], cwd: &str) -> Stats {
        let mut s = Stats::default();
        let mut calls = HashSet::new();
        let mut paths = HashSet::new();
        let rel = |p: &str| -> String {
            match p.strip_prefix(cwd).and_then(|r| r.strip_prefix('/')) {
                Some(r) if !cwd.is_empty() => r.to_string(),
                _ => p.to_string(),
            }
        };
        for e in items.iter().filter(|e| role(e) == Role::Tool) {
            let d = data(e);
            let id = if truthy(&d["callId"]) {
                format!("{}:{}", e.actor, text(&d["callId"]))
            } else {
                format!("seq:{}", e.seq)
            };
            if !calls.insert(id) {
                continue;
            }
            match text(&d["tool"]).as_str() {
                "execute" => s.commands += 1,
                "read" => s.reads += 1,
                "search" | "fetch" => s.searches += 1,
                "edit" | "delete" | "move" => {}
                t if t.starts_with("janus.") => s.board += 1,
                _ => s.other += 1,
            }
            for p in super::texts(&d["paths"]) {
                paths.insert(rel(&p));
            }
            for x in list(&d["diffs"]) {
                paths.insert(rel(&text(&x["path"])));
            }
        }
        s.edited = paths.len();
        s
    }

    /// `运行 3 条命令 · 读 2 个文件 · 改 1 个文件`, only what happened.
    pub(super) fn text(&self, l: Lang) -> String {
        let mut parts = vec![];
        if self.commands > 0 {
            parts.push(tr!(l, "运行 {} 条命令", "Ran {} commands", self.commands));
        }
        if self.reads > 0 {
            parts.push(tr!(l, "读 {} 个文件", "Read {} files", self.reads));
        }
        if self.searches > 0 {
            parts.push(tr!(l, "搜索 {} 次", "Searched {} times", self.searches));
        }
        if self.edited > 0 {
            parts.push(tr!(l, "改 {} 个文件", "Edited {} files", self.edited));
        }
        if self.board > 0 {
            parts.push(tr!(l, "用看板 {} 次", "Used the board {} times", self.board));
        }
        if self.other > 0 {
            parts.push(tr!(l, "其他操作 {} 次", "{} other actions", self.other));
        }
        parts.join(" · ")
    }
}

/// The space above a row, as a client lays the conversation out: turns and sections stand apart,
/// the blocks of one turn sit close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Gap {
    /// The first row.
    None,
    /// Before what opens a turn or a section.
    Round,
    /// Between the blocks of one turn: the agent's line, its reply, its changed files.
    Block,
    /// Half a block: an item inside a turn, a prompt under the message it carries.
    Half,
    /// Between the lines of a turn's work.
    Line,
    /// Before the agent's words along the way.
    Loose,
}

/// A file a turn changed: its path, the lines added and removed over all its edits, and each edit
/// that changed it (an event's seq and the place of the change in its `diffs`, whose lines the
/// view's `diffs` hold).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub changes: Vec<ChangeRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ChangeRef {
    pub seq: i64,
    pub index: usize,
}

/// One row of a conversation. `id`s are stable while events arrive: an event's own seq, or a
/// negative id made from the seq that starts what the row stands for. `turn` is the turn a row
/// belongs to (the seq of its first event); a `fold` row is left out while the person has that
/// turn's line (`head`) folded.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    /// A section's opening event and what Janus injected under it.
    #[serde(rename_all = "camelCase")]
    Section { id: i64, seq: i64, injected: Vec<i64>, gap: Gap },
    /// What opened a turn, or any single event shown as itself.
    #[serde(rename_all = "camelCase")]
    Item { id: i64, seq: i64, turn: Option<i64>, fold: bool, gap: Gap },
    /// The agent's name and what its turn did. `open` is how it starts (nil when the turn did
    /// nothing but reply, so there is nothing to fold); `summary` the finished turn's line,
    /// `stats` what a running turn's line adds after its clock (from `start`).
    #[serde(rename_all = "camelCase")]
    Head {
        id: i64,
        turn: i64,
        agent: String,
        running: bool,
        open: Option<bool>,
        start: i64,
        end: i64,
        stats: String,
        summary: String,
        gap: Gap,
    },
    /// Tool calls and thinking in a row: one line (`summary`) that opens to every call. `open` is
    /// how it starts, nil when the calls show under no line of their own. A running turn's newest
    /// calls in the summary level show the step the agent is on apart (`partOf` names the row it
    /// was split from); a click on that row shows `whole` instead, with no line when `sole`.
    #[serde(rename_all = "camelCase")]
    Activity {
        id: i64,
        events: Vec<i64>,
        open: Option<bool>,
        summary: String,
        turn: i64,
        fold: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        whole: Option<Vec<i64>>,
        sole: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_of: Option<i64>,
        gap: Gap,
    },
    /// The agent's text, `reply` when it is what ended the turn.
    #[serde(rename_all = "camelCase")]
    Text { id: i64, events: Vec<i64>, reply: bool, turn: i64, fold: bool, gap: Gap },
    /// The files the turn changed.
    #[serde(rename_all = "camelCase")]
    Edits { id: i64, turn: i64, files: Vec<EditedFile>, gap: Gap },
    /// One of the group's posts in the lead's conversation (`feed::merge`): it stands apart like a
    /// turn and never folds.
    #[serde(rename_all = "camelCase")]
    Post { id: i64, post: Box<super::feed::Post>, gap: Gap },
}

// Rows made of several events take negative ids from the seq they start at, one slot of eight per
// seq, so they never meet an event's own (positive) seq or each other.
pub fn head_id(turn: i64) -> i64 {
    -(turn * 8 + 1)
}
pub fn activity_id(seq: i64) -> i64 {
    -(seq * 8 + 2)
}
pub fn edits_id(turn: i64) -> i64 {
    -(turn * 8 + 3)
}
pub fn post_id(seq: i64) -> i64 {
    -(seq * 8 + 4)
}

/// One turn: what opened it and everything the agent did until the next one.
struct Turn<'a> {
    /// The seq of its first event.
    id: i64,
    opener: Option<&'a Event>,
    items: Vec<&'a Event>,
    start: i64,
    end: i64,
}

impl<'a> Turn<'a> {
    fn new(e: &'a Event, opener: bool) -> Turn<'a> {
        Turn { id: e.seq, opener: opener.then_some(e), items: vec![], start: e.ts, end: e.ts }
    }

    fn push(&mut self, e: &'a Event) {
        self.items.push(e);
        self.end = e.ts;
    }

    /// Whose work the turn is: the first agent that did something in it.
    fn agent(&self) -> String {
        self.items
            .iter()
            .find(|e| e.actor != "janus" && e.actor != "user" && role(e) != Role::Pinned)
            .map(|e| e.actor.clone())
            .or_else(|| self.opener.and_then(|o| o.to.clone()))
            .unwrap_or_default()
    }

    /// The agent's text that ended the turn: the messages after its last piece of work. Pinned
    /// items in between (a result handed in, a request) do not end the reply.
    fn reply_start(&self) -> Option<usize> {
        let last = self.items.iter().rposition(|e| e.kind() == EventKind::AgentMessage)?;
        let mut i = last;
        while i > 0 && matches!(role(self.items[i - 1]), Role::Message | Role::Pinned) {
            i -= 1;
        }
        // The first message of the reply: pinned items before it are not part of it.
        while i < last && self.items[i].kind() != EventKind::AgentMessage {
            i += 1;
        }
        Some(i)
    }
}

enum Part<'a> {
    Section(&'a Event, Vec<&'a Event>),
    Turn(Turn<'a>),
}

/// The turns of a conversation's shown events, in order, with the sections between them.
fn parts<'a>(evs: &[&'a Event]) -> Vec<Part<'a>> {
    let mut out: Vec<Part<'a>> = vec![];
    let mut turn: Option<Turn<'a>> = None;
    for &e in evs {
        match role(e) {
            Role::Section => {
                out.extend(turn.take().map(Part::Turn));
                out.push(Part::Section(e, vec![]));
            }
            Role::Injected => {
                // Under the section it belongs to when nothing was said since; otherwise in the turn.
                if let Some(t) = turn.as_mut() {
                    t.push(e);
                } else if let Some(Part::Section(_, injected)) = out.last_mut() {
                    injected.push(e);
                } else {
                    out.push(Part::Section(e, vec![]));
                }
            }
            Role::Opener => {
                // Janus's prompt carrying the message that just opened the turn belongs to that turn.
                if let Some(t) = turn
                    .as_mut()
                    .filter(|t| e.kind() == EventKind::SessionPrompt && t.opener.is_some() && t.items.is_empty())
                {
                    t.push(e);
                    continue;
                }
                out.extend(turn.take().map(Part::Turn));
                turn = Some(Turn::new(e, true));
            }
            _ => turn.get_or_insert_with(|| Turn::new(e, false)).push(e),
        }
    }
    out.extend(turn.map(Part::Turn));
    out
}

/// How a conversation's rows are worked out.
pub(super) struct Options<'a> {
    /// Every turn and every group of calls opens; otherwise a finished turn shows one line, and
    /// only the running turn's newest group is open.
    pub detail: bool,
    /// The conversation's newest turn is still running.
    pub busy: bool,
    /// The session's working directory.
    pub cwd: &'a str,
    pub lang: Lang,
    /// Each changed file's lines by event and change, `(seq, index) → (added, removed)`.
    pub counts: &'a dyn Fn(i64, usize) -> (usize, usize),
}

/// The rows to show for a conversation's shown events.
pub(super) fn rows(evs: &[&Event], o: &Options<'_>) -> Vec<Row> {
    let parts = parts(evs);
    let last_turn = parts.iter().rposition(|p| matches!(p, Part::Turn(_)));
    let mut rows: Vec<Row> = vec![];
    for (k, p) in parts.into_iter().enumerate() {
        match p {
            Part::Section(e, injected) => rows.push(Row::Section {
                id: e.seq,
                seq: e.seq,
                injected: injected.iter().map(|x| x.seq).collect(),
                gap: Gap::Round,
            }),
            Part::Turn(t) => {
                let running = o.busy && Some(k) == last_turn;
                turn_rows(&mut rows, t, running, o);
            }
        }
    }
    gaps(&mut rows, evs);
    rows
}

fn item(e: &Event, turn: Option<i64>, fold: bool) -> Row {
    Row::Item { id: e.seq, seq: e.seq, turn, fold, gap: Gap::None }
}

fn turn_rows(rows: &mut Vec<Row>, mut t: Turn<'_>, running: bool, o: &Options<'_>) {
    if let Some(opener) = t.opener {
        rows.push(item(opener, Some(t.id), false));
    }
    // Janus's prompt carrying the opening message sits with it, above the agent's line.
    while let Some(&first) = t.items.first().filter(|e| role(e) == Role::Opener) {
        rows.push(item(first, Some(t.id), false));
        t.items.remove(0);
    }
    if t.items.is_empty() {
        return;
    }
    let reply = if running { None } else { t.reply_start() };
    let work: Vec<&Event> = t.items[..reply.unwrap_or(t.items.len())].to_vec();
    let has_work = work.iter().any(|e| !matches!(role(e), Role::Pinned | Role::Opener));
    // A turn cut off or with nothing but calls has no reply to show instead: it stays open.
    let open = o.detail || running || reply.is_none();
    if has_work || reply.is_some() {
        let stats = Stats::of(&t.items, o.cwd).text(o.lang);
        let summary = if running {
            String::new()
        } else {
            let worked = crate::text::duration(t.end - t.start, o.lang);
            let head = tr!(o.lang, "工作了 {worked}", "Worked for {worked}");
            if stats.is_empty() { head } else { format!("{head} · {stats}") }
        };
        rows.push(Row::Head {
            id: head_id(t.id),
            turn: t.id,
            agent: t.agent(),
            running,
            open: has_work.then_some(open),
            start: t.start,
            end: t.end,
            stats,
            summary,
            gap: Gap::None,
        });
    }
    rows.extend(work_rows(&work, t.id, running, o));
    if let Some(reply) = reply {
        let mut texts: Vec<&Event> = vec![];
        let flush = |texts: &mut Vec<&Event>, rows: &mut Vec<Row>| {
            if let Some(first) = texts.first() {
                rows.push(Row::Text {
                    id: first.seq,
                    events: texts.iter().map(|e| e.seq).collect(),
                    reply: true,
                    turn: t.id,
                    fold: false,
                    gap: Gap::None,
                });
                texts.clear();
            }
        };
        for &e in &t.items[reply..] {
            let message = e.kind() == EventKind::AgentMessage;
            if message && texts.first().is_none_or(|f| f.actor == e.actor) {
                texts.push(e);
                continue;
            }
            flush(&mut texts, rows);
            if message {
                texts.push(e);
            } else {
                rows.push(item(e, Some(t.id), false));
            }
        }
        flush(&mut texts, rows);
    }
    let files = edited(&t.items, o.counts);
    if !files.is_empty() {
        rows.push(Row::Edits { id: edits_id(t.id), turn: t.id, files, gap: Gap::None });
    }
}

/// A group of calls' line: `运行 2 条命令 · 读 1 个文件 · 思考`.
fn activity_summary(events: &[&Event], o: &Options<'_>) -> String {
    let mut parts: Vec<String> = vec![Stats::of(events, o.cwd).text(o.lang)];
    parts.retain(|s| !s.is_empty());
    if events.iter().any(|e| e.kind() == EventKind::AgentThought) {
        parts.push(o.lang.pick("思考", "Thought").into());
    }
    if parts.is_empty() { tr!(o.lang, "{} 个操作", "{} actions", events.len()) } else { parts.join(" · ") }
}

fn activity(events: &[&Event], turn: i64, open: Option<bool>, o: &Options<'_>) -> Row {
    Row::Activity {
        id: activity_id(events[0].seq),
        events: events.iter().map(|e| e.seq).collect(),
        open,
        summary: activity_summary(events, o),
        turn,
        fold: true,
        whole: None,
        sole: false,
        part_of: None,
        gap: Gap::None,
    }
}

/// The work of a turn, open: groups of calls, the agent's text between them, notes. Everything
/// but what is pinned folds with the turn.
fn work_rows(work: &[&Event], turn: i64, running: bool, o: &Options<'_>) -> Vec<Row> {
    let mut rows: Vec<Row> = vec![];
    let mut group: Vec<&Event> = vec![];
    let mut text: Vec<&Event> = vec![];
    let flush_group = |group: &mut Vec<&Event>, rows: &mut Vec<Row>| {
        if !group.is_empty() {
            rows.push(activity(group, turn, Some(o.detail), o));
            group.clear();
        }
    };
    let flush_text = |text: &mut Vec<&Event>, rows: &mut Vec<Row>| {
        if let Some(first) = text.first() {
            rows.push(Row::Text {
                id: first.seq,
                events: text.iter().map(|e| e.seq).collect(),
                reply: false,
                turn,
                fold: true,
                gap: Gap::None,
            });
            text.clear();
        }
    };
    for &e in work {
        let r = role(e);
        if r == Role::Tool || e.kind() == EventKind::AgentThought {
            flush_text(&mut text, &mut rows);
            group.push(e);
        } else if r == Role::Message {
            flush_group(&mut group, &mut rows);
            if text.first().is_some_and(|f| f.actor != e.actor) {
                flush_text(&mut text, &mut rows);
            }
            text.push(e);
        } else {
            flush_group(&mut group, &mut rows);
            flush_text(&mut text, &mut rows);
            rows.push(item(e, Some(turn), !matches!(r, Role::Pinned | Role::Opener)));
        }
    }
    flush_group(&mut group, &mut rows);
    flush_text(&mut text, &mut rows);
    let last = rows.iter().rposition(|r| matches!(r, Row::Activity { .. }));
    let sole = rows.len() == 1 && last.is_some();
    // The summary of a running turn: its newest calls fold too, all but the step the agent is on,
    // so switching levels changes what is in view at the bottom, not only rows above it.
    if let Some(k) = last.filter(|_| running && !o.detail) {
        let Row::Activity { events, .. } = &rows[k] else { unreachable!() };
        if events.len() > 2 {
            let all: Vec<&Event> = events.iter().map(|s| *work.iter().find(|e| e.seq == *s).unwrap()).collect();
            let (before, now) = all.split_at(all.len() - 1);
            let mut folded = activity(before, turn, Some(false), o);
            if let Row::Activity { whole, sole: s, .. } = &mut folded {
                *whole = Some(all.iter().map(|e| e.seq).collect());
                *s = sole;
            }
            let id = activity_id(before[0].seq);
            let mut step = activity(now, turn, None, o);
            if let Row::Activity { part_of, .. } = &mut step {
                *part_of = Some(id);
            }
            rows.splice(k..=k, [folded, step]);
            return rows;
        }
    }
    // Calls are all the turn did: the turn's own line sums them up, so they need no line of their own.
    if sole && let Row::Activity { open, .. } = &mut rows[0] {
        *open = None;
        return rows;
    }
    // While the turn runs, its newest calls are open: that is where the agent is now.
    if let Some(k) = last.filter(|_| running)
        && let Row::Activity { open, .. } = &mut rows[k]
    {
        *open = Some(true);
    }
    rows
}

/// The files the calls of a turn changed, with lines added and removed over all their edits.
fn edited(items: &[&Event], counts: &dyn Fn(i64, usize) -> (usize, usize)) -> Vec<EditedFile> {
    let mut files: Vec<EditedFile> = vec![];
    for e in items.iter().filter(|e| e.kind() == EventKind::ToolCall) {
        for (index, x) in list(&data(e)["diffs"]).iter().enumerate() {
            let path = text(&x["path"]);
            let (added, removed) = counts(e.seq, index);
            let change = ChangeRef { seq: e.seq, index };
            match files.iter_mut().find(|f| f.path == path) {
                Some(f) => {
                    f.changes.push(change);
                    f.added += added;
                    f.removed += removed;
                }
                None => files.push(EditedFile { path, added, removed, changes: vec![change] }),
            }
        }
    }
    files
}

/// The space above each row, after the row before it (`Gap`).
fn gaps(rows: &mut [Row], evs: &[&Event]) {
    let opener = |seq: i64| evs.iter().find(|e| e.seq == seq).is_some_and(|e| role(e) == Role::Opener);
    let mut prev_opener = false;
    for (i, r) in rows.iter_mut().enumerate() {
        let (gap, is_opener) = match r {
            Row::Section { .. } => (Gap::Round, false),
            Row::Item { seq, .. } => {
                let o = opener(*seq);
                // A prompt right under the message it carries sits with it.
                (
                    if prev_opener {
                        Gap::Half
                    } else if o {
                        Gap::Round
                    } else {
                        Gap::Half
                    },
                    o,
                )
            }
            Row::Head { .. } | Row::Edits { .. } => (Gap::Block, false),
            Row::Activity { .. } => (Gap::Line, false),
            Row::Text { reply, .. } => (if *reply { Gap::Block } else { Gap::Loose }, false),
            Row::Post { .. } => (Gap::Round, false),
        };
        let slot = match r {
            Row::Section { gap, .. }
            | Row::Item { gap, .. }
            | Row::Head { gap, .. }
            | Row::Activity { gap, .. }
            | Row::Text { gap, .. }
            | Row::Edits { gap, .. }
            | Row::Post { gap, .. } => gap,
        };
        *slot = if i == 0 { Gap::None } else { gap };
        prev_opener = is_opener;
    }
}
