//! What a conversation's events say about its session now, beside the conversation itself: the
//! agent's latest plan, the controls it offers (model, effort, mode …) with their current values,
//! its slash commands, its context and token usage, the Git facts it works with, the messages on
//! their way to it and the requests that wait on the person. Read from the same events the
//! conversation shows.
// @project-doc docs/architecture/20261031-frontend.md#session_view

use super::{RunState, list, text, truthy};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::Value;

fn data(e: &Event) -> &Value {
    e.data.as_ref().unwrap_or(&Value::Null)
}

/// One step of the agent's plan.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Step {
    pub content: String,
    /// `pending`, `in_progress` or `completed`, as the agent says.
    pub status: String,
}

/// The latest plan, if the agent published one.
pub(super) fn plan(evs: &[&Event]) -> Vec<Step> {
    let Some(e) = evs.iter().rev().find(|e| e.kind() == EventKind::AgentPlan) else { return vec![] };
    list(&data(e)["entries"])
        .iter()
        .map(|s| Step { content: text(&s["content"]), status: text(&s["status"]) })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choice {
    pub value: String,
    pub name: String,
}

/// A control the agent offers for its session, in its own terms.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Control {
    pub id: String,
    pub name: String,
    /// `mode` for an ACP session mode, `config` for a config option: what switching it sends.
    pub via: String,
    pub current: String,
    pub options: Vec<Choice>,
    /// `模型：GPT-5`: its name and the current choice's.
    pub label: String,
}

/// The session's controls as they stand: the last full list, then each switch since. A control
/// with one choice is not offered.
pub(super) fn controls(evs: &[&Event], l: Lang) -> Vec<Control> {
    let Some(i) = evs.iter().rposition(|e| e.kind() == EventKind::SessionControls) else { return vec![] };
    let d = data(evs[i]);
    let mut out: Vec<Control> = vec![];
    let mut add = |id: String, name: String, via: &str, c: &Value| {
        let options: Vec<Choice> = list(&c["options"])
            .iter()
            .map(|o| {
                let value = text(&o["value"]);
                let name = if truthy(&o["name"]) { text(&o["name"]) } else { value.clone() };
                Choice { value, name }
            })
            .collect();
        if options.len() > 1 {
            out.push(Control {
                id,
                name,
                via: via.into(),
                current: text(&c["current"]),
                options,
                label: String::new(),
            });
        }
    };
    if truthy(&d["model"]) {
        add(text(&d["model"]["id"]), l.pick("模型", "Model").into(), "config", &d["model"]);
    }
    if truthy(&d["effort"]) {
        add(text(&d["effort"]["id"]), l.pick("推理强度", "Reasoning Effort").into(), "config", &d["effort"]);
    }
    for c in list(&d["controls"]).iter().filter(|c| c["type"] == "select" || list(&c["options"]).len() > 1) {
        let name = if c["category"] == "mode" { l.pick("模式", "Mode").into() } else { text(&c["name"]) };
        add(text(&c["id"]), name, if c["via"] == "mode" { "mode" } else { "config" }, c);
    }
    for e in &evs[i + 1..] {
        let s = data(e);
        if e.kind() != EventKind::SessionControl || truthy(&s["failed"]) {
            continue;
        }
        let (id, via) = (text(&s["id"]), text(&s["via"]));
        if let Some(c) = out.iter_mut().find(|c| c.id == id || (c.via == "mode" && via == "mode")) {
            c.current = text(&s["value"]);
        }
    }
    for c in &mut out {
        let current = c.options.iter().find(|o| o.value == c.current).map_or(c.current.clone(), |o| o.name.clone());
        c.label = tr!(l, "{}：{}", "{}: {}", c.name, current);
    }
    out
}

/// A slash command the agent offers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Command {
    pub name: String,
    pub description: String,
    /// The arguments it takes, shown grey after `/name ` until the person types on.
    pub hint: String,
    /// A skill the agent offers as a command, listed in a group of its own.
    pub skill: bool,
}

/// The slash commands the agent offers now.
pub(super) fn commands(evs: &[&Event]) -> Vec<Command> {
    let Some(e) = evs.iter().rev().find(|e| e.kind() == EventKind::SessionCommands) else { return vec![] };
    list(&data(e)["commands"])
        .iter()
        .map(|c| Command {
            name: text(&c["name"]),
            description: text(&c["description"]),
            hint: text(&c["hint"]),
            skill: c["skill"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// This session's context and its last turn's tokens: a snapshot of one session, never part of
/// the run's token total.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Usage {
    /// Context used and its size, when the agent reports them.
    pub used: Option<i64>,
    pub size: Option<i64>,
    /// Tokens of the last turn.
    pub tokens: Option<i64>,
    pub percent: Option<i64>,
    /// What the meter in the composer's row shows: `26%`, or `15.4K tokens` without a size.
    pub meter: String,
    /// `本会话 上下文 26% · 上一轮 1,500 tokens`.
    pub labels: String,
    /// The meter's help: the labels, the raw numbers, and that they are not the run's total.
    pub help: String,
}

pub(super) fn usage(evs: &[&Event], l: Lang) -> Option<Usage> {
    let (mut used, mut size, mut tokens) = (None, None, None);
    for e in evs.iter().filter(|e| e.kind() == EventKind::SessionUsage) {
        let d = data(e);
        // An agent that reports 0 before it has read anything says nothing yet.
        if let Some(u) = d["used"].as_i64().filter(|&u| u > 0) {
            used = Some(u);
        }
        if let Some(s) = d["size"].as_i64() {
            size = Some(s);
        }
        if let Some(t) = d["totalTokens"].as_i64() {
            tokens = Some(t);
        }
    }
    if used.is_none() && tokens.is_none() {
        return None;
    }
    let percent = match (used, size) {
        (Some(u), Some(s)) if s > 0 => Some((u * 100 / s).min(100)),
        _ => None,
    };
    let num = crate::usage::num;
    let context = used.map(|u| match percent {
        Some(p) => tr!(l, "本会话 上下文 {p}%", "Session context: {p}%"),
        None => tr!(l, "本会话 上下文 {}", "Session context: {}", num(u)),
    });
    let turn = tokens.map(|t| tr!(l, "上一轮 {} tokens", "Last turn: {} tokens", num(t)));
    let labels = [context, turn].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let meter = match percent {
        Some(p) => format!("{p}%"),
        None => format!("{} tokens", compact(tokens.or(used).unwrap_or(0))),
    };
    let mut help: Vec<String> = vec![labels.clone()];
    help.extend(used.map(|u| tr!(l, "上下文已用 {}", "Context used: {}", compact(u))));
    help.extend(size.map(|s| tr!(l, "共 {}", "Total: {}", compact(s))));
    let help = help.join(" · ")
        + l.pick("\n本会话的数字不计入任务累计用量", "\nSession context is excluded from cumulative run usage");
    Some(Usage { used, size, tokens, percent, meter, labels, help })
}

/// `15.4K`, `21.4M`, `6.45B`: three significant digits.
pub fn compact(n: i64) -> String {
    for (unit_size, unit) in [(1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if (n.abs() as f64) < unit_size {
            continue;
        }
        let v = n as f64 / unit_size;
        let digits = if v.abs() >= 100.0 {
            0
        } else if v.abs() >= 10.0 {
            1
        } else {
            2
        };
        let mut s = format!("{v:.digits$}");
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        return s + unit;
    }
    crate::usage::num(n)
}

/// The Git facts a session works with, in one line: the repository, the branch accepting merges
/// back into and the commit; where Janus keeps its copy is not the person's concern. None for a
/// run outside a worktree.
fn git_note(repo: &str, branch: &str, head: &str, l: Lang) -> Option<String> {
    if repo.is_empty() {
        return None;
    }
    let mut parts = vec![
        tr!(l, "Git 仓库 {repo}", "Git repository {repo}"),
        if branch.is_empty() { "detached HEAD".into() } else { tr!(l, "分支 {branch}", "branch {branch}") },
    ];
    if !head.is_empty() {
        let short = crate::text::short_sha(head);
        parts.push(tr!(l, "提交 {short}", "commit {short}"));
    }
    Some(parts.join(" · "))
}

/// What an event that starts a section says about where and how its session works: for an attempt
/// (`attempt.started`), which session it is in when not a plain new one and why it runs again,
/// and its Git facts; for the run (`run.created`), where the lead works.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SectionNote {
    /// ` · 接上原会话 · 按 V1 的意见返工`: what an attempt's header adds after its runtime.
    pub note: String,
    pub git: Option<String>,
}

pub(super) fn section_note(e: &Event, l: Lang) -> Option<SectionNote> {
    let d = data(e);
    match e.kind() {
        EventKind::AttemptStarted => {
            let g = &d["git"];
            Some(SectionNote {
                note: attempt_note(d, e.task.as_deref(), l),
                git: git_note(&text(&g["repo"]), &text(&g["branch"]), &text(&g["head"]), l),
            })
        }
        EventKind::RunCreated => Some(SectionNote {
            note: String::new(),
            git: git_note(&text(&d["worktree"]["repo"]), &text(&d["worktree"]["branch"]), &text(&d["baseRef"]), l),
        }),
        _ => None,
    }
}

/// The Git facts of a task's latest attempt.
pub(super) fn task_git(evs: &[&Event], task: &str, l: Lang) -> Option<String> {
    let e = evs.iter().rev().find(|e| e.kind() == EventKind::AttemptStarted && e.task.as_deref() == Some(task))?;
    let g = &data(e)["git"];
    git_note(&text(&g["repo"]), &text(&g["branch"]), &text(&g["head"]), l)
}

/// What an attempt's header adds about how it started: which session it is in when not a plain
/// new one, and why it runs again. A rework or a re-check continues the task's own conversation,
/// so the header says so instead of looking like a new session.
fn attempt_note(d: &Value, task: Option<&str>, l: Lang) -> String {
    let mut parts: Vec<String> = vec![];
    if truthy(&d["resumed"]) {
        parts.push(l.pick("接上了原会话", "Picked up the previous session").into());
    }
    let c = &d["continuation"];
    if truthy(&c["loaded"]) {
        let from = text(&c["from"]);
        parts.push(if Some(from.as_str()) == task {
            l.pick("接上原会话", "Picks up its own session").into()
        } else {
            tr!(l, "接上 {from} 的会话", "Picks up {from}'s session")
        });
    } else if !c.is_null() {
        parts.push(
            match c["fallback"].as_str().unwrap_or("") {
                "context_over_75pct" => {
                    l.pick("新开会话（原会话上下文超过 75%）", "New session (previous one was over 75% of its context)")
                }
                "unsupported" => {
                    l.pick("新开会话（Agent 不支持加载会话）", "New session (the agent cannot load sessions)")
                }
                "no_session" => {
                    l.pick("新开会话（这个子任务之前没有会话）", "New session (this subtask had no session before)")
                }
                _ => l.pick("新开会话（原会话加载失败）", "New session (the previous one failed to load)"),
            }
            .into(),
        );
    }
    let r = &d["reopen"];
    match r["reason"].as_str().unwrap_or("") {
        "lead.retry" if truthy(&r["from"]) => {
            parts.push(tr!(l, "按 {} 的意见返工", "Rework on {}'s findings", text(&r["from"])));
        }
        "target.reworked" => parts.push(tr!(l, "{} 返工后复审", "Re-check after {} was reworked", text(&r["target"]))),
        _ => {}
    }
    parts.iter().map(|p| format!(" · {p}")).collect()
}

/// A message sent to this session that has not reached the agent yet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pending {
    pub id: String,
    pub text: String,
    /// How it waits: `pending`, `queued`, `next` or `interrupt`.
    pub delivery: String,
    /// The tray's badge for it.
    pub label: String,
}

/// The messages to a conversation still on their way, oldest first: to the task (or routed to
/// it), or to an agent; a message to a task's agent naming no task waits in that task's tray too.
pub(super) fn pending(st: &RunState, task: Option<&str>, agent: Option<&str>, l: Lang) -> Vec<Pending> {
    let label = |d: &str| -> Option<&'static str> {
        Some(match d {
            "pending" => l.pick("发送中…", "Sending…"),
            "queued" => l.pick("排队 · 这一轮结束后发送", "Queued · Sends after this turn"),
            "next" => l.pick("留言 · 下次会话带给它", "Note · Delivered at its next session"),
            "interrupt" => l.pick("正在打断这一轮，马上送达", "Interrupting this turn to deliver the message"),
            _ => return None,
        })
    };
    let task_agent = task.and_then(|t| st.task(t)).map(|t| t.agent.as_str());
    let mut out: Vec<(i64, Pending)> = st
        .msgs
        .iter()
        .filter(|m| m.delivered_seq.is_none())
        .filter(|m| match (task, agent) {
            (Some(t), _) => {
                m.task.as_deref().or(m.routed_task.as_deref()) == Some(t)
                    || (m.task.is_none() && m.routed_task.is_none() && Some(m.to.as_str()) == task_agent)
            }
            (None, Some(a)) => m.to == a,
            _ => false,
        })
        .filter_map(|m| {
            let label = label(&m.delivery)?;
            Some((
                m.seq,
                Pending { id: m.id.clone(), text: m.text.clone(), delivery: m.delivery.clone(), label: label.into() },
            ))
        })
        .collect();
    out.sort_by_key(|(seq, _)| *seq);
    out.into_iter().map(|(_, p)| p).collect()
}

/// The keys of this conversation's approvals and forms still waiting for the person, oldest
/// first: they sit above its composer.
pub(super) fn requests(evs: &[&Event], st: &RunState) -> Vec<String> {
    evs.iter()
        .filter(|e| matches!(e.kind(), EventKind::ApprovalRequested | EventKind::InputRequested))
        .map(|e| text(&data(e)["key"]))
        .filter(|k| st.inbox.iter().any(|r| &r.key == k))
        .collect()
}
