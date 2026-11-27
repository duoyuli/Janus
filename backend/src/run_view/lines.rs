//! What each event of a conversation says in one line, in the person's language: a section's
//! divider, a note from Janus, a message in a bubble with how it went in, a submitted result, a
//! request, a tool call's verb and what it worked on. The rows (`turns`) say where an event goes;
//! this says what it reads, so no client words an event itself
//! (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#session_turns

use super::card::{Pill, Tone};
use super::words::Words;
use super::{RunState, goal_title, image_paths, list, short, text, texts, truthy, visible_text};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::{Lang, tr};
use crate::status::RunKind;
use crate::text::{date_time, duration};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// One event as its conversation shows it. A reply or a thought has none: it is the agent's own
/// text.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Line {
    /// A section's opening line: the run, a chat, a consolidation, a Loop's wake.
    Divider {
        text: String,
        /// The line below it: where a chat works.
        #[serde(skip_serializing_if = "Option::is_none")]
        sub: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        git: Option<String>,
        /// The goal's images.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
    },
    /// An attempt's header, its Git facts and what the brief Janus sent it is called.
    Attempt {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        git: Option<String>,
        brief: String,
    },
    /// A note from Janus in the flow.
    Note {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
    },
    /// What Janus sent the agent: its name and a hint; the prompt itself is the event's.
    Prompt {
        label: String,
        hint: String,
    },
    /// Messages to the agent, each in a bubble with how it went in.
    Bubbles {
        items: Vec<Bubble>,
    },
    /// A submitted result: its title, its counts, its evidence and risks.
    Result {
        title: String,
        pills: Vec<Pill>,
        #[serde(skip_serializing_if = "Option::is_none")]
        evidence: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        risks: Option<String>,
    },
    /// An approval or a form: answered with its card while `open`, else one line (`text`);
    /// `docked` is the line while its card sits above the composer.
    Request {
        key: String,
        open: bool,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
        docked: String,
    },
    Tool(ToolLine),
}

/// A message to the agent in its bubble.
#[derive(Clone, Debug, Serialize)]
pub struct Bubble {
    /// Its words, without the lines that name its images.
    pub text: String,
    pub images: Vec<String>,
    /// How it went in: queued, cut into the turn, left for the next session.
    pub via: String,
    /// Cut into the turn running: drawn apart, its `via` above it.
    pub insert: bool,
}

/// A tool call in one line.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLine {
    /// One of Janus's own tools (`janus.*`): `tag` names whose (the board's, a Loop's).
    pub board: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tag: String,
    /// What it does, `正在运行` while it runs, and what it worked on.
    pub verb: String,
    pub what: String,
    /// `what` is a command or a file name, set in monospace.
    pub mono: bool,
    pub running: bool,
    pub failed: bool,
    /// Started and not back yet, and no newer frame of it either.
    pub waiting: bool,
    /// The first line of what came back, once it did.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// `改了 2 个文件`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub files: String,
}

/// What a tool call does, in a word, for its kind: running or done.
pub fn verb(kind: &str, running: bool, l: Lang) -> &'static str {
    let (now, done) = match kind {
        "execute" => (l.pick("正在运行", "Running"), l.pick("运行", "Ran")),
        "read" => (l.pick("正在读", "Reading"), l.pick("读", "Read")),
        "edit" => (l.pick("正在改", "Editing"), l.pick("改", "Edited")),
        "delete" => (l.pick("正在删除", "Deleting"), l.pick("删除", "Delete")),
        "move" => (l.pick("正在移动", "Moving"), l.pick("移动", "Moved")),
        "search" => (l.pick("正在搜索", "Searching"), l.pick("搜索", "Searched")),
        "fetch" => (l.pick("正在获取", "Fetching"), l.pick("获取", "Fetched")),
        "think" => (l.pick("正在思考", "Thinking"), l.pick("思考", "Thought")),
        _ => ("", ""),
    };
    if running { now } else { done }
}

/// Paths inside the workspace `cwd` written relative to it, as the agent's own interface shows
/// them.
pub fn in_workspace(t: &str, cwd: &str) -> String {
    if cwd.is_empty() {
        return t.to_string();
    }
    let root = cwd.strip_suffix('/').unwrap_or(cwd);
    t.replace(&format!("{root}/"), "")
}

/// A call in one line: a verb for its kind and what it worked on, the command itself for a
/// command and the file's name for a file. An unknown kind reads as its own title.
pub fn label(kind: &str, input: &str, title: &str, running: bool, cwd: &str, l: Lang) -> (String, String, bool) {
    let v = verb(kind, running, l).to_string();
    let path = in_workspace(input, cwd);
    match kind {
        "execute" => (v, input.split('\n').next().unwrap_or(input).to_string(), true),
        "read" | "edit" | "delete" | "move" => {
            // A plain path names the file; anything else (a range, a pattern) reads as titled.
            if !path.contains(' ') && !path.is_empty() {
                (v, path.rsplit('/').next().unwrap_or(&path).to_string(), true)
            } else {
                (v, title.to_string(), false)
            }
        }
        "search" | "fetch" => (v, if input.is_empty() { title.to_string() } else { input.to_string() }, false),
        "think" => (v, String::new(), false),
        _ => (String::new(), if title.is_empty() { input.to_string() } else { title.to_string() }, false),
    }
}

/// The mark and the colour of an event noted in one line.
fn mark(kind: EventKind) -> (&'static str, Option<Tone>) {
    use EventKind as K;
    let icon = match kind {
        K::RunCreated => "✦",
        K::RunCancelled => "■",
        K::RunPaused => "Ⅱ",
        K::RunResumed => "▶",
        K::TaskReady => "▸",
        K::TaskAssigned => "↗",
        K::AttemptStarted => "▶",
        K::AttemptFailed => "✕",
        K::TaskRetryScheduled => "↻",
        K::TaskReopened => "↺",
        K::ResultSubmitted => "⇪",
        K::TaskSucceeded => "✓",
        K::ArtifactCreated => "▤",
        K::ContextHandoff => "⇄",
        K::QuestionAsked => "?",
        K::QuestionEscalated => "⇡",
        K::QuestionAnswered => "✎",
        K::AnswerDelivered => "↩",
        K::ApprovalRequested => "⚑",
        K::InputRequested => "✎",
        K::InputAnswered => "↩",
        K::ApprovalGranted => "✓",
        K::ApprovalDenied => "✕",
        K::RunDelivered => "★",
        K::RiskAccepted => "✓",
        K::RunMerged => "✓",
        K::TaskFailed => "✕",
        K::TaskBlocked => "⊘",
        K::TaskSkippedByDecision => "→",
        K::VerificationStarted => "◌",
        K::VerificationFailed => "✕",
        K::VerificationPassed => "✓",
        K::RunFailed => "✕",
        K::RunInterrupted => "■",
        K::AgentStatus => "·",
        K::ToolCall | K::ToolStarted => "⌘",
        K::ToolCompleted => "✓",
        K::SessionPrompt => "→",
        K::ResultAmended => "✎",
        K::RunRecovered => "↻",
        K::MessageRouted => "↪",
        K::DecisionWithdrawn => "↩",
        K::AgentMessage => "❝",
        K::UserMessage => "✎",
        K::MessageDelivered => "↪",
        K::SessionParked => "◌",
        K::ControlTaken => "✋",
        K::ControlReleased => "↻",
        K::SessionInterrupted => "‖",
        K::TaskConflict => "⚠",
        K::LeadStalled | K::TaskStalled => "⏳",
        K::TaskSplitRequested => "✂",
        _ => "·",
    };
    let tone = match kind {
        K::TaskSucceeded | K::ApprovalGranted | K::InputAnswered | K::RunDelivered | K::RiskAccepted | K::RunMerged => {
            Some(Tone::Green)
        }
        K::QuestionAsked
        | K::QuestionEscalated
        | K::ApprovalRequested
        | K::InputRequested
        | K::RunPaused
        | K::ControlTaken
        | K::SessionInterrupted
        | K::UserMessage
        | K::TaskConflict
        | K::LeadStalled
        | K::TaskStalled
        | K::TaskSplitRequested => Some(Tone::Amber),
        K::AttemptFailed | K::ApprovalDenied | K::RunCancelled | K::TaskFailed | K::RunFailed | K::RunInterrupted => {
            Some(Tone::Red)
        }
        K::AgentStatus | K::ToolCall | K::ToolCompleted | K::TaskReady | K::SessionParked => Some(Tone::Gray),
        K::ToolStarted | K::SessionPrompt => Some(Tone::Blue),
        _ => None,
    };
    (icon, tone)
}

/// What one run's conversations need to word their events.
pub(super) struct Ctx<'a> {
    pub st: &'a RunState,
    pub words: &'a Words,
    /// The run's events in its conversations, by seq.
    pub events: &'a HashMap<i64, Event>,
    /// Now (ms), for 「今天 …」.
    pub now: i64,
}

/// Every event of the run's conversations in one line, by seq.
pub(super) fn lines(c: &Ctx<'_>) -> BTreeMap<i64, Line> {
    let mut seqs: Vec<&i64> = c.events.keys().collect();
    seqs.sort();
    let mut woke: Option<i64> = None;
    let mut out = BTreeMap::new();
    for s in seqs {
        let e = &c.events[s];
        if let Some(line) = line(c, e, woke) {
            out.insert(e.seq, line);
        }
        if e.kind() == EventKind::LoopWoke {
            woke = Some(e.ts);
        }
    }
    out
}

fn note(text: String, tone: Option<Tone>) -> Option<Line> {
    Some(Line::Note { text, tone })
}

/// Event `e` in one line; `woke` is when the Loop's last wake before it started.
fn line(c: &Ctx<'_>, e: &Event, woke: Option<i64>) -> Option<Line> {
    let null = Value::Null;
    let d = e.data.as_ref().unwrap_or(&null);
    let w = c.words;
    let l = w.lang;
    let and = w.and();
    let st = c.st;
    let kind = e.kind();
    let when = |ms: i64| date_time(ms, c.now, l);
    let task = e.task.clone().unwrap_or_default();
    match kind {
        EventKind::AttemptStarted => {
            let n = super::aside::section_note(e, l);
            let prefix = e.task.as_ref().map(|t| format!("{t} · ")).unwrap_or_default();
            let attempt = e.attempt.unwrap_or(0);
            let runtime = text(&d["runtime"]);
            let model = text(&d["config"]["model"]);
            let model = if model.is_empty() { String::new() } else { format!(" · {model}") };
            let note = n.as_ref().map(|n| n.note.clone()).unwrap_or_default();
            Some(Line::Attempt {
                title: tr!(
                    l,
                    "{prefix}尝试 {attempt} · {runtime}{model}{note}",
                    "{prefix}Attempt {attempt} · {runtime}{model}{note}"
                ),
                git: n.and_then(|n| n.git),
                brief: l.pick("Janus 发出的子任务说明", "Subtask brief from Janus").into(),
            })
        }
        EventKind::RunCreated => match RunKind::parse(&text(&d["kind"])) {
            Some(RunKind::Chat) => {
                let name = text(&d["role"]["name"]);
                let at = when(e.ts);
                let cwd = super::docs::tilde(&text(&d["cwd"]));
                Some(Line::Divider {
                    text: tr!(l, "对话 · {name} · {at}", "Chat · {name} · {at}"),
                    sub: Some(tr!(
                        l,
                        "工作目录 {cwd} · 对话专用，删对话时一起删",
                        "Working directory {cwd} · just for this chat, deleted with it"
                    )),
                    git: None,
                    images: vec![],
                })
            }
            Some(RunKind::Dream) => {
                let at = when(e.ts);
                Some(Line::Divider {
                    text: tr!(l, "整理团队记忆 · {at}", "Consolidate team memory · {at}"),
                    sub: None,
                    git: None,
                    images: vec![],
                })
            }
            _ => {
                let title = goal_title(&st.run.goal);
                Some(Line::Divider {
                    text: tr!(l, "目标 · {title}", "Goal · {title}"),
                    sub: None,
                    git: super::aside::section_note(e, l).and_then(|n| n.git),
                    images: image_paths(d),
                })
            }
        },
        EventKind::AgentMessage | EventKind::AgentThought => None,
        EventKind::SessionControl => {
            let failed = truthy(&d["failed"]);
            note(format!("{} {}", if failed { "⚠" } else { "↺" }, e.text), failed.then_some(Tone::Amber))
        }
        EventKind::SessionPrompt => {
            let via = if text(&d["via"]) == "developer" {
                l.pick("开发者指令", "Developer Instructions")
            } else {
                l.pick("系统提示", "System Prompt")
            };
            let run_kind = st.run.kind;
            let (label, hint) = match text(&d["channel"]).as_str() {
                "instructions" => (
                    tr!(l, "数字员工的指令 · {via}", "Digital Employee Instructions · {via}"),
                    l.pick(
                        "数字员工的指令 + Agent 的自定义提示词。一对一对话只带这一份。",
                        "The digital employee's instructions plus the Agent's custom prompt. A one-on-one chat gets only this.",
                    ),
                ),
                "system" => match run_kind {
                    RunKind::Loop => (
                        tr!(l, "值班的工作方式 · {via}", "How a Loop works · {via}"),
                        l.pick(
                            "值班守则：不亲自改文件，活儿派成任务，拿不准才问你；附带共享资料。",
                            "Loop rules: never edits files itself, dispatches work as runs, asks you only when unsure; plus shared context.",
                        ),
                    ),
                    RunKind::Dream => (
                        tr!(l, "怎么整理团队记忆 · {via}", "How to consolidate team memory · {via}"),
                        l.pick(
                            "只动笔记：合并重复的、改正过时的、删掉没用的；附带全部笔记的索引。",
                            "Notes only: merge duplicates, correct what is outdated, remove what is of no use; with the index of every note.",
                        ),
                    ),
                    _ => (
                        tr!(l, "团队协作指令 · {via}", "Team Instructions · {via}"),
                        l.pick(
                            "团队任务专属：分工、看板、派工和验证规则。",
                            "Team runs only: roles, the board, dispatching, and verification rules.",
                        ),
                    ),
                },
                "history" => (
                    tr!(l, "之前的对话 · {via}", "Earlier Conversation · {via}"),
                    l.pick(
                        "重开对话时带上最近的聊天，接着聊。",
                        "When a chat reopens, recent messages come along so it picks up where it left off.",
                    ),
                ),
                other => {
                    let label = match other {
                        "goal" => l.pick("目标提示", "Goal Prompt"),
                        "followup" => l.pick("继续提示", "Resume Prompt"),
                        "fix" => l.pick(
                            "提醒交结果 · 这一轮结束了还没交",
                            "Reminded to hand in · the turn ended without a result",
                        ),
                        "board" => l.pick(
                            "看板推送 · 插入正在进行的这一轮",
                            "Board Update · Steered into the Active Turn",
                        ),
                        "goal_check" => l.pick(
                            "目标确认 · 负责人停了，但目标还没完成",
                            "Goal check · The lead stopped, but the goal isn't done",
                        ),
                        "resume" => l.pick(
                            "恢复会话 · 从中断处接着推进目标",
                            "Session Restored · Continuing the Goal Where It Stopped",
                        ),
                        _ => l.pick("Janus 提示", "Janus Prompt"),
                    };
                    (label.to_string(), l.pick("Janus 发给 Agent", "Sent by Janus"))
                }
            };
            Some(Line::Prompt { label, hint: hint.into() })
        }
        // Goal mode: a collaborator asks the lead, which answers on the board.
        EventKind::QuestionAsked => {
            let q = if d["question"].is_string() { text(&d["question"]) } else { e.text.clone() };
            let options = texts(&d["options"]);
            let options = if options.is_empty() {
                String::new()
            } else {
                let o = options.join(" / ");
                tr!(l, "（{o}）", " ({o})")
            };
            note(
                tr!(
                    l,
                    "? 问负责人：{q}{options} · 负责人会回答，不需要你处理",
                    "? Question for the lead: {q}{options} · The lead will answer"
                ),
                Some(Tone::Amber),
            )
        }
        // The lead edited files after handing out subtasks: it only manages from then on.
        EventKind::LeadEdited => {
            let paths = texts(&d["paths"]).join(and);
            note(
                tr!(
                    l,
                    "⚠ 派出子任务后改了文件：{paths} · 交付时会单独列出",
                    "⚠ Edited files after handing out subtasks: {paths} · listed on their own in the delivery"
                ),
                Some(Tone::Amber),
            )
        }
        EventKind::LeadRouted => {
            let tool = text(&d["tool"]);
            note(
                tr!(
                    l,
                    "⚠ 派出子任务后调用了资源库工具 {tool} · 会写进交付风险",
                    "⚠ Called the library tool {tool} after handing out subtasks · Listed as a delivery risk"
                ),
                Some(Tone::Amber),
            )
        }
        // A handoff with a verdict is a verification's findings (for a rework), not a result.
        EventKind::ContextHandoff => {
            let from = if d["from"].is_string() { text(&d["from"]) } else { e.text.clone() };
            note(
                if d["verdict"].is_null() {
                    tr!(
                        l,
                        "⇄ 上游子任务 {from} 的结果已带入本次子任务",
                        "⇄ Results from upstream subtask {from} were included in this subtask"
                    )
                } else {
                    tr!(
                        l,
                        "⇄ 独立验证 {from} 的意见已带入本次子任务",
                        "⇄ Findings of verification {from} were included in this subtask"
                    )
                },
                None,
            )
        }
        EventKind::BoardPosted => {
            let m = match text(&d["kind"]).as_str() {
                "blocker" => "⚠",
                "note" => "·",
                _ => "↗",
            };
            let t = if d["text"].is_string() { text(&d["text"]) } else { e.text.clone() };
            note(tr!(l, "{m} 协作者写回看板：{t}", "{m} Collaborator posted to the board: {t}"), None)
        }
        EventKind::TaskConflict => {
            let other = text(&d["other"]);
            let paths = texts(&d["paths"]).join(and);
            note(
                if truthy(&d["owned"]) {
                    tr!(
                        l,
                        "⚠ 越界：{task} 改了 {other} 负责的 {paths}。已提醒 {task} 不要再改，并告诉负责人",
                        "⚠ {task} edited {paths}, which belongs to {other}. Janus asked {task} to stop editing these files and notified the lead."
                    )
                } else {
                    tr!(
                        l,
                        "⚠ {task} 和 {other} 同时在改 {paths}。已提醒双方先读最新内容，并告诉负责人",
                        "⚠ {task} and {other} are both editing {paths}. Janus asked both to read the latest contents first and notified the lead."
                    )
                },
                Some(Tone::Amber),
            )
        }
        EventKind::UserMessage => {
            let via = match text(&d["delivery"]).as_str() {
                "steer" => l.pick("插话 · 已插进这一轮", "Steered into this turn"),
                "queued" => l.pick("排队 · 这一轮结束后发送", "Queued · Sends after this turn"),
                "prompt" => l.pick("作为新一轮发送", "Sent as a new turn"),
                "next" => l.pick("留言 · 下次会话带给它", "Note · Delivered at its next session"),
                _ => "",
            };
            let images = image_paths(d);
            Some(Line::Bubbles { items: vec![bubble(&text(&d["text"]), images, via.into(), false)] })
        }
        EventKind::MessageRouted => {
            let m = st.msgs.iter().find(|m| m.id == text(&d["id"]))?;
            let lead = m.by == "lead";
            let item = if text(&d["delivery"]) == "steer" {
                let who = if lead { l.pick("负责人插话", "Lead Steering") } else { l.pick("插话", "Steer") };
                let state = if m.delivered_seq.is_some() {
                    l.pick("Agent 已读到", "Seen by the Agent")
                } else {
                    l.pick(
                        "插话 · 已插进这一轮，做完手头这步就会读到",
                        "Steered into this turn · read after the current step",
                    )
                };
                bubble(&m.text, m.images.clone(), format!("{who} · {state}"), true)
            } else {
                let by = if lead { l.pick("负责人 · ", "Lead · ") } else { "" };
                bubble(
                    &m.text,
                    m.images.clone(),
                    format!("{by}{}", l.pick("作为新一轮发送", "Sent as a new turn")),
                    false,
                )
            };
            Some(Line::Bubbles { items: vec![item] })
        }
        EventKind::MessageDelivered if !d["ids"].is_null() => {
            let items = texts(&d["ids"])
                .iter()
                .filter_map(|id| st.msgs.iter().find(|m| &m.id == id))
                .filter(|m| m.delivery != "steer")
                .map(|m| {
                    let by = if m.by == "lead" { l.pick("负责人 · ", "Lead · ") } else { "" };
                    let how = match m.delivery.as_str() {
                        "next" => l.pick("留言 · 这次会话开始时已带上", "Note · Delivered when this session started"),
                        "interrupt" => {
                            l.pick("插话 · 打断上一轮后立刻送达", "Interrupted the previous turn and delivered")
                        }
                        _ => l.pick("排队 · 这一轮结束后发送", "Queued · Sends after this turn"),
                    };
                    bubble(&m.text, m.images.clone(), format!("{by}{how}"), false)
                })
                .collect();
            Some(Line::Bubbles { items })
        }
        EventKind::MessageDelivered => {
            let (icon, tone) = mark(kind);
            note(format!("{icon} {}", e.text), tone)
        }
        // One line in the conversation; each criterion is a click away (and on the task page).
        EventKind::ResultSubmitted => {
            let checks = list(&d["checks"]);
            let passed = checks.iter().filter(|c| truthy(&c["passed"])).count();
            let total = checks.len();
            let risks = texts(&d["risks"]);
            let evidence = texts(&d["evidence"]);
            let semi = l.pick("；", " · ");
            let summary = if d["summary"].is_string() { text(&d["summary"]) } else { e.text.clone() };
            let mut pills = vec![];
            if total > 0 {
                pills.push(Pill {
                    text: tr!(l, "{passed}/{total} 通过", "{passed}/{total} passed"),
                    tone: if passed == total { Tone::Green } else { Tone::Amber },
                });
            }
            if !risks.is_empty() {
                let n = risks.len();
                pills.push(Pill { text: tr!(l, "{n} 个风险", "Risks: {n}"), tone: Tone::Amber });
            }
            Some(Line::Result {
                title: tr!(l, "⇪ 提交结果：{summary}", "⇪ Result submitted: {summary}"),
                pills,
                evidence: (!evidence.is_empty()).then(|| {
                    let t = evidence.join(semi);
                    tr!(l, "证据：{t}", "Evidence: {t}")
                }),
                risks: (!risks.is_empty()).then(|| {
                    let t = risks.join(semi);
                    tr!(l, "风险：{t}", "Risks: {t}")
                }),
            })
        }
        EventKind::AgentStatus => note(format!("◌ {}", e.text), Some(Tone::Amber)),
        // A Loop's wake: when it started and why, then when it ended and when the next one is.
        EventKind::LoopWoke => {
            let n = d["n"].as_i64().unwrap_or(0);
            let at = when(e.ts);
            let reason = text(&d["reason"]);
            let why = match reason.as_str() {
                "timer" => l.pick("按时间表", "on schedule").to_string(),
                "self" => l.pick("它约的时间到了", "at the time it asked for").into(),
                "manual" => l.pick("你让它开工", "you woke it").into(),
                "answer" => l.pick("你回答了它的问题", "you answered its question").into(),
                "dispatch" => l.pick("派出的任务结束了", "a run it dispatched ended").into(),
                _ => reason,
            };
            Some(Line::Divider {
                text: tr!(l, "第 {n} 次开工 · {at} · {why}", "Wake {n} · {at} · {why}"),
                sub: None,
                git: None,
                images: vec![],
            })
        }
        EventKind::LoopSlept => {
            let next = match d["until"].as_i64() {
                Some(t) => {
                    let at = when(t);
                    if text(&d["reason"]) == "self" {
                        tr!(l, "下次开工：{at}（它约的）", "next wake: {at} (it asked)")
                    } else {
                        tr!(l, "下次开工：{at}", "next wake: {at}")
                    }
                }
                None => l.pick("下次等你叫它", "next when you call it").into(),
            };
            let at = when(e.ts);
            note(
                match woke {
                    Some(start) => {
                        let took = duration(e.ts - start, l);
                        tr!(l, "○ {at} 收工（{took}）· {next}", "○ Ended {at} ({took}) · {next}")
                    }
                    None => tr!(l, "○ {at} 收工 · {next}", "○ Ended {at} · {next}"),
                },
                None,
            )
        }
        EventKind::LoopFailed => note(format!("⚠ {} · {}", when(e.ts), e.text), Some(Tone::Amber)),
        // A request still waiting is answered with its card; once answered it is one line.
        EventKind::ApprovalRequested | EventKind::InputRequested => {
            let key = text(&d["key"]);
            let (icon, tone) = mark(kind);
            let who = w.name(&e.actor);
            let said = &e.text;
            Some(Line::Request {
                open: st.inbox.iter().any(|r| r.key == key),
                key,
                text: tr!(l, "{icon} {who}：{said}", "{icon} {who}: {said}"),
                tone,
                docked: tr!(
                    l,
                    "{icon} {who}：{said} · 等你处理，在输入框上方",
                    "{icon} {who}: {said} · Waiting for you, above the message box"
                ),
            })
        }
        EventKind::ToolCall | EventKind::ToolStarted | EventKind::ToolCompleted => Some(Line::Tool(tool(c, e, d))),
        _ => {
            let (icon, tone) = mark(kind);
            let who = w.name(&e.actor);
            let said = &e.text;
            note(tr!(l, "{icon} {who}：{said}", "{icon} {who}: {said}"), tone)
        }
    }
}

fn bubble(t: &str, images: Vec<String>, via: String, insert: bool) -> Bubble {
    let text = if images.is_empty() { t.to_string() } else { visible_text(t) };
    Bubble { text, images, via, insert }
}

/// A tool call in one line. Janus's own tools read as what they did on the board.
fn tool(c: &Ctx<'_>, e: &Event, d: &Value) -> ToolLine {
    let l = c.words.lang;
    let cwd = &c.st.run.cwd;
    let name = text(&d["tool"]);
    let running = e.kind() == EventKind::ToolStarted;
    let output = in_workspace(&text(&d["output"]), cwd);
    let input = if truthy(&d["input"]) { text(&d["input"]) } else { e.text.clone() };
    let failed = text(&d["status"]) == "failed";
    // The first line that says something: not a Markdown fence around the output.
    let first =
        output.split('\n').find(|x| !x.trim().starts_with("```") && !x.trim().is_empty()).unwrap_or("").to_string();
    let n = list(&d["diffs"]).len();
    let files = if n > 0 { tr!(l, "改了 {n} 个文件", "Files changed: {n}") } else { String::new() };
    let waiting = running && !truthy(&d["live"]);
    if let Some(call) = name.strip_prefix("janus.") {
        let (what, said) = janus_label(c.words, call, &d["args"], &output);
        let tag = if c.st.run.kind == RunKind::Loop { l.pick("值班", "Loop") } else { l.pick("看板", "Board") };
        return ToolLine {
            board: true,
            tag: tag.into(),
            verb: String::new(),
            what,
            mono: false,
            running,
            failed,
            waiting,
            summary: if running || said.is_empty() { String::new() } else { format!("→ {}", short(&said, 160)) },
            files,
        };
    }
    let (verb, what, mono) = label(&name, &input, &e.text, running, cwd, l);
    ToolLine {
        board: false,
        tag: String::new(),
        verb,
        what,
        mono,
        running,
        failed,
        waiting,
        summary: if running || first.is_empty() { String::new() } else { format!("→ {}", short(&first, 160)) },
        files,
    }
}

/// One of Janus's own calls as what it did, and what came back first.
fn janus_label(w: &Words, call: &str, a: &Value, out: &str) -> (String, String) {
    let l = w.lang;
    let and = w.and();
    let first = out.split('\n').next().unwrap_or("").to_string();
    let t = |k: &str| text(&a[k]);
    match call {
        "orchestrate" => {
            let semi = l.pick("；", " · ");
            let tasks = list(&a["tasks"])
                .iter()
                .map(|x| format!("{} {} → {}", text(&x["id"]), text(&x["title"]), w.name(&text(&x["assignee"]))))
                .collect::<Vec<_>>()
                .join(semi);
            (tr!(l, "派出子任务：{tasks}", "Dispatched subtasks: {tasks}"), first)
        }
        "wait" => (l.pick("等待看板上的进展", "Wait for board activity").into(), first),
        "reply" => {
            let (task, said) = (t("task"), t("text"));
            (tr!(l, "回复 {task}：{said}", "Reply to {task}: {said}"), first)
        }
        "retry" => {
            let task = t("task");
            let ins =
                if truthy(&a["instructions"]) { l.pick("（改了说明）", " (instructions changed)") } else { "" };
            let acc = if truthy(&a["accept"]) {
                l.pick("（改了验收标准）", " (acceptance criteria changed)")
            } else {
                ""
            };
            (tr!(l, "重新执行 {task}{ins}{acc}", "Retry {task}{ins}{acc}"), first)
        }
        "amend" => {
            let (task, said) = (t("task"), t("text"));
            (tr!(l, "修订 {task}：{said}", "Amend {task}: {said}"), first)
        }
        "cancel" => {
            let tasks = texts(&a["tasks"]).join(and);
            let reason = t("reason");
            let why = if reason.is_empty() { String::new() } else { tr!(l, "：{reason}", ": {reason}") };
            (tr!(l, "取消 {tasks}{why}", "Cancel {tasks}{why}"), first)
        }
        "ask_user" => {
            let q = t("question");
            (tr!(l, "等你拍板：{q}", "Your call: {q}"), first)
        }
        "board" => {
            let task = t("task");
            let what = if task.is_empty() {
                l.pick("查看团队和子任务状态", "View team and subtask status").to_string()
            } else {
                tr!(l, "查看 {task} 的全部信息", "View everything about {task}")
            };
            (what, String::new())
        }
        "board_post" => {
            let said = t("text");
            (tr!(l, "写回看板：{said}", "Post to the board: {said}"), String::new())
        }
        _ => (format!("janus.{call}"), first),
    }
}
