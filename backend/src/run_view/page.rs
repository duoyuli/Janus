//! What the run page shows around the board, written once for every client: the delivery folded
//! to one line, the delivery as text to copy, what the round changed and the deliveries before it,
//! the inspector's lists of changes, files and deliverables, each task page's trace and setting,
//! and the team page's notes on where the run works.
// @project-doc docs/architecture/20261031-frontend.md#run_page

use super::docs::{Doc, Source};
use super::evidence::DeliveryEvidence;
use super::review::{Review, ReviewStage};
use super::words::Words;
use super::{ArtifactView, DeliveryView, RunState, TaskShown, list, text, texts};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::tr;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The run page's own lines and lists (`RunView::page`).
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// The latest delivery in one line, at every stage but the two that wait for the person's
    /// review (`awaiting`, `blocked`); `None` then and before any delivery.
    pub review_line: Option<String>,
    /// The latest delivery as text to copy: the conclusion, what needs looking at, where the result
    /// is, each task's numbers and the report. `None` before any delivery.
    pub summary: Option<String>,
    /// The run card's 「子任务 2 / 3」 (succeeded of all), before the ledger's numbers; `None`
    /// before the lead hands out any.
    pub progress: Option<String>,
    /// What the latest delivery's round changed: the diff to review and the other deliverables.
    pub changes: Changes,
    /// The deliveries before the latest one, newest first.
    pub earlier: Vec<Earlier>,
    /// Beside the lead's conversation: each task's newest diff (the whole run's first), its newest
    /// other artifacts, and the documents the deliveries hand over that are not among those.
    pub inspector: Inspector,
    /// Each task's page, by its id.
    pub tasks: BTreeMap<String, TaskPage>,
    /// The team page's notes on the workspace and, for a run in its own worktree, the branch.
    pub team: TeamNotes,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    /// The round's diff artifact, to review.
    pub diff: Option<ArtifactView>,
    /// The other deliverables as the delivery lists them (`name`, `desc`, `path`, `seq`).
    pub others: Vec<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Earlier {
    pub n: usize,
    pub seq: i64,
    /// Its conclusion, or its report's first line.
    pub line: String,
    pub accepted: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspector {
    pub diffs: Vec<ArtifactView>,
    pub files: Vec<ArtifactView>,
    pub delivered: Vec<Doc>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPage {
    /// The events of its execution trace, by seq: its own, and those that name it.
    pub trace: Vec<i64>,
    /// The tasks that wait on it.
    pub downstream: Vec<String>,
    /// 「负责 a、b · 独立验证 T1」, empty when it owns and verifies nothing.
    pub structure: String,
    /// Its latest attempt's setting: runtime, isolation, session and workspace.
    pub env: Vec<Line>,
}

/// One labelled line.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamNotes {
    pub workspace: String,
    pub branch: Option<String>,
}

/// The events that belong to each task's trace, collected as they are folded.
#[derive(Clone, Debug, Default)]
pub(super) struct TraceFold {
    seqs: BTreeMap<String, Vec<i64>>,
    /// Each task's latest `attempt.started`, for its setting.
    started: BTreeMap<String, Value>,
}

impl TraceFold {
    pub(super) fn apply(&mut self, e: &Event, d: &Value) {
        let mut named: Vec<String> = e.task.iter().cloned().collect();
        let target = text(&d["target"]);
        if !target.is_empty() {
            named.push(target);
        }
        named.extend(texts(&d["tasks"]));
        named.extend(texts(&d["unlocked"]));
        named.sort();
        named.dedup();
        for t in named {
            self.seqs.entry(t).or_default().push(e.seq);
        }
        if let (EventKind::AttemptStarted, Some(t)) = (e.kind(), &e.task) {
            self.started.insert(t.clone(), d.clone());
        }
    }
}

/// The run page's lines and lists. `docs` are the run's Markdown files that exist; `title` is what
/// the run is called.
pub(super) fn page(
    st: &RunState,
    traces: &TraceFold,
    review: Option<&Review>,
    delivery: Option<&DeliveryEvidence>,
    docs: &[Doc],
    title: &str,
    w: &Words,
) -> Page {
    let result = st.result().filter(|_| !st.run.kind.single_session());
    Page {
        review_line: match (result, review) {
            (Some(r), Some(rv)) => review_line(r, rv.stage, w),
            _ => None,
        },
        summary: result.map(|r| summary(st, r, review, delivery, title, w)),
        progress: (!st.tasks.is_empty()).then(|| {
            let (done, all) = (st.tasks.iter().filter(|t| t.status == TaskShown::Succeeded).count(), st.tasks.len());
            tr!(w.lang, "子任务 {done} / {all}", "Subtasks: {done} / {all}")
        }),
        changes: result.map(|r| changes(st, r)).unwrap_or_default(),
        earlier: result.map(|r| earlier(st, r)).unwrap_or_default(),
        inspector: inspector(st, docs),
        tasks: st.tasks.iter().map(|t| (t.id.clone(), task_page(st, traces, &t.id, w))).collect(),
        team: team(st, w),
    }
}

/// A delivery in one line: its conclusion, or its report's first line.
fn first_line(d: &Value) -> String {
    let headline = text(&d["headline"]);
    let t = if headline.is_empty() { text(&d["summary"]) } else { headline };
    t.lines().next().unwrap_or("").to_string()
}

fn review_line(r: &DeliveryView, stage: ReviewStage, w: &Words) -> Option<String> {
    let l = w.lang;
    let line = match stage {
        ReviewStage::Awaiting | ReviewStage::Blocked => return None,
        ReviewStage::Resolving => l
            .pick(
                "合并到你的分支时冲突了，你的目录和分支都没动。负责人在任务的副本里解决，解决后再次交付。",
                "Merging into your branch conflicted; your folder and branch are untouched. The lead is resolving it in the run's copy and will deliver again.",
            )
            .into(),
        ReviewStage::LeadWorking => {
            l.pick("负责人在处理你的消息，处理完再验收。", "The lead is working on your message. Accept once it is done.").into()
        }
        ReviewStage::TeamWorking => l
            .pick(
                "负责人派了新的子任务，做完后再次交付。",
                "The lead handed out new subtasks; it delivers again when they are done.",
            )
            .into(),
        ReviewStage::Asking => l
            .pick(
                "有事等你回答，见上面的「待处理」。回答后负责人接着做。",
                "Something waits for your answer under Needs You above. The lead goes on once you answer.",
            )
            .into(),
        ReviewStage::Held => l
            .pick(
                "你打断了负责人这一轮，它在等你。这一轮改的东西还没有交付。",
                "You interrupted the lead's turn and it is waiting for you. What this turn changed is not delivered yet.",
            )
            .into(),
        ReviewStage::Accepted => {
            let when = r.accepted.map(clock).unwrap_or_default();
            let mut line = tr!(l, "你已于 {when} 验收了这次交付。", "You accepted this delivery at {when}.");
            let note = text(&r.d["merge"]["note"]);
            if !note.is_empty() {
                line.push(' ');
                line.push_str(&note);
            }
            line
        }
        ReviewStage::Stopped => {
            l.pick("交付之后任务停下了，见上面的运行卡片。", "The run stopped after this delivery; see the run card above.").into()
        }
    };
    Some(line)
}

/// A time of day on this machine's clock, to the second (the app's `L.clock`).
fn clock(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// The delivery under review as text: the conclusion, what needs looking at, where the result is,
/// each task's numbers, and the report.
fn summary(
    st: &RunState,
    r: &DeliveryView,
    review: Option<&Review>,
    delivery: Option<&DeliveryEvidence>,
    title: &str,
    w: &Words,
) -> String {
    let l = w.lang;
    let d = &r.d;
    let mut lines = vec![tr!(l, "已完成：{title}", "Completed: {title}")];
    if st.deliveries.len() > 1 {
        let n = r.n;
        lines.push(tr!(l, "第 {n} 次交付", "Delivery {n}"));
    }
    let headline = text(&d["headline"]);
    if !headline.is_empty() {
        lines.push(headline);
    }
    let signals = review.map(|rv| rv.signals.as_slice()).unwrap_or_default();
    if !signals.is_empty() {
        lines.push(String::new());
        lines.push(l.pick("需要你看的：", "To look at:").into());
        for s in signals {
            let detail = &s.detail;
            let more = if detail.is_empty() { String::new() } else { tr!(l, "（{detail}）", " ({detail})") };
            lines.push(format!("- {}{more}", s.text));
        }
    }
    let note = text(&d["merge"]["note"]);
    if !note.is_empty() {
        lines.push(String::new());
        lines.push(note);
    }
    lines.push(String::new());
    lines.push(delivery.map(|e| e.facts.clone()).unwrap_or_default());
    let rows = delivery.map(|e| e.tasks.as_slice()).unwrap_or_default();
    if !rows.is_empty() {
        lines.push(String::new());
        lines.push(l.pick("逐条验收：", "Acceptance checklist:").into());
        for a in rows {
            let verified: Vec<String> = a
                .verifications
                .iter()
                .map(|v| {
                    let task = text(&v["task"]);
                    if v["passed"] == true {
                        tr!(l, "{task} 通过", "{task} passed")
                    } else {
                        let label = TaskShown::parse(&text(&v["status"]))
                            .map(|s| w.task_status(s))
                            .unwrap_or_else(|| text(&v["label"]));
                        tr!(l, "{task} 未确认（{label}）", "{task} unconfirmed ({label})")
                    }
                })
                .collect();
            let (task, passed, criteria, counts) = (&a.task, a.passed, a.criteria, &a.counts);
            let mut line = tr!(
                l,
                "- {task}：通过 {passed}/{criteria} · 证据：{counts}",
                "- {task}: passed {passed} of {criteria} · Evidence: {counts}"
            );
            if !verified.is_empty() {
                line.push_str(l.pick(" · 独立验证：", " · Independent verification: "));
                line.push_str(&verified.join(w.and()));
            }
            if a.unverified {
                line.push_str(l.pick(" · 没有独立验证", " · No independent verification"));
            }
            lines.push(line);
        }
    }
    let report = text(&d["summary"]);
    if !report.is_empty() {
        lines.push(String::new());
        lines.push(report);
    }
    lines.join("\n")
}

/// The round's diff to review, and the deliverables that are not it.
fn changes(st: &RunState, r: &DeliveryView) -> Changes {
    let is_diff = |path: &str| st.artifacts.iter().find(|a| a.path == path && is_diff(a));
    let deliverables = list(&r.d["deliverables"]);
    Changes {
        diff: deliverables.iter().find_map(|a| is_diff(&text(&a["path"]))).cloned(),
        others: deliverables.iter().filter(|a| is_diff(&text(&a["path"])).is_none()).cloned().collect(),
    }
}

fn earlier(st: &RunState, r: &DeliveryView) -> Vec<Earlier> {
    st.deliveries
        .iter()
        .filter(|e| e.seq < r.seq)
        .rev()
        .map(|e| Earlier { n: e.n, seq: e.seq, line: first_line(&e.d), accepted: e.accepted.is_some() })
        .collect()
}

fn is_diff(a: &ArtifactView) -> bool {
    a.kind == "git_diff" || a.kind == "run_diff"
}

/// The newest artifact of each task and kind (and name, for files): an older attempt's is history.
fn latest<'a>(xs: impl Iterator<Item = &'a ArtifactView>) -> Vec<ArtifactView> {
    let mut xs: Vec<&ArtifactView> = xs.collect();
    xs.sort_by_key(|a| std::cmp::Reverse(a.seq));
    let mut seen = std::collections::HashSet::new();
    xs.into_iter()
        .filter(|a| seen.insert(format!("{}/{}/{}", a.task, a.kind, if is_diff(a) { "" } else { &a.name })))
        .cloned()
        .collect()
}

fn inspector(st: &RunState, docs: &[Doc]) -> Inspector {
    let mut diffs = latest(st.artifacts.iter().filter(|a| is_diff(a)));
    // The whole run's diff first: it is what accepting brings back, the lead's own edits included.
    diffs.sort_by_key(|a| a.kind != "run_diff");
    let files = latest(st.artifacts.iter().filter(|a| !is_diff(a)));
    let delivered = docs
        .iter()
        .filter(|d| d.source == Source::Deliverable && !files.iter().any(|f| f.path == d.path))
        .cloned()
        .collect();
    Inspector { diffs, files, delivered }
}

fn task_page(st: &RunState, traces: &TraceFold, id: &str, w: &Words) -> TaskPage {
    let l = w.lang;
    let Some(t) = st.task(id) else { return TaskPage::default() };
    let mut structure = vec![];
    if !t.owns.is_empty() {
        let owns = t.owns.join(w.and());
        structure.push(tr!(l, "负责 {owns}", "Owns {owns}"));
    }
    if !t.verifies.is_empty() {
        let v = &t.verifies;
        structure.push(tr!(l, "独立验证 {v}", "Independently verifies {v}"));
    }
    let null = Value::Null;
    let started = traces.started.get(id).unwrap_or(&null);
    let cfg = &started["config"];
    let mut runtime = vec![];
    let rt = text(&started["runtime"]);
    runtime.push(if rt.is_empty() { w.runtime(&t.agent) } else { rt });
    let model = text(&cfg["model"]);
    if !model.is_empty() {
        let effort = text(&cfg["effort"]);
        runtime.push(if effort.is_empty() { model } else { format!("{model} ({effort})") });
    }
    let mode = [text(&cfg["label"]), text(&cfg["mode"])].into_iter().find(|m| !m.is_empty());
    if let Some(mode) = mode {
        runtime.push(tr!(l, "权限 {mode}", "Permissions: {mode}"));
    }
    let info = &started["agentInfo"];
    if !info.is_null() && info != &Value::Object(Default::default()) {
        runtime.push(format!("{} {}", text(&info["name"]), text(&info["version"])));
    }
    runtime.retain(|s| !s.is_empty());
    let enforcement = text(&started["enforcement"]);
    let last = t.attempts.last();
    let line = |label: &str, value: String| Line { label: label.into(), value };
    TaskPage {
        trace: traces.seqs.get(id).cloned().unwrap_or_default(),
        downstream: st.tasks.iter().filter(|x| x.deps.iter().any(|d| d == id)).map(|x| x.id.clone()).collect(),
        structure: structure.join(" · "),
        env: vec![
            line(l.pick("运行时", "Runtime"), runtime.join(" · ")),
            line(l.pick("隔离方式", "Isolation"), if enforcement.is_empty() { "—".into() } else { enforcement }),
            line(l.pick("会话", "Sessions"), last.map(|a| a.session.clone()).unwrap_or_else(|| "—".into())),
            line(
                l.pick("工作区", "Workspace"),
                last.map(|a| a.workspace.clone()).unwrap_or_else(|| l.pick("工作目录", "Working Directory").into()),
            ),
        ],
    }
}

fn team(st: &RunState, w: &Words) -> TeamNotes {
    let l = w.lang;
    let r = &st.run;
    let workspace = if r.git {
        if r.workspace.is_empty() {
            let base: String = r.base_ref.chars().take(7).collect();
            tr!(l, "Git 仓库，基线 {base}", "Git repository, baseline {base}")
        } else {
            r.workspace.clone()
        }
    } else {
        l.pick(
            "不是 Git 仓库：直接改这个目录，没有 diff",
            "Not a Git repository: work happens right in this folder, with no diff",
        )
        .into()
    };
    let branch = (!r.repo.is_empty()).then(|| {
        let source = if r.branch.is_empty() { "detached HEAD".to_string() } else { r.branch.clone() };
        if r.merged {
            tr!(l, "已合并到 {source}", "Merged into {source}")
        } else {
            tr!(l, "交付时合并到 {source}", "Merged into {source} on delivery")
        }
    });
    TeamNotes { workspace, branch }
}
