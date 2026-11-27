//! Acceptance evidence as the person reads it: each task's checks paired with its criteria, the
//! method counts and reminders, who verified it independently; and for the delivery under review,
//! each task's one-line row (numbers, how strong the evidence is, what needs attention) and the
//! round's line of numbers. Whether a task passed stays with the scheduler's records; this only
//! groups what was recorded (`crate::evidence` holds the rules).
// @project-doc docs/architecture/20261031-frontend.md#evidence_methods

use super::words::Words;
use super::{RunState, TaskShown, TaskView, list, text, texts, truthy};
use crate::evidence::{Check, Method, MethodCount, Summary, Verdict, effective_verdict};
use crate::lang::tr;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// A criterion with the check that answered it: every criterion gets a row (even when nothing
/// answered it) and every check is kept (even when it names no criterion), so no evidence is
/// dropped.
#[derive(Clone, Debug, Serialize)]
pub struct CheckRow {
    pub n: usize,
    pub criterion: String,
    /// The check as recorded; `None` for a criterion nothing answered yet.
    pub check: Option<Value>,
}

/// One task's evidence, as its page shows it.
#[derive(Clone, Debug, Serialize)]
pub struct TaskEvidence {
    pub criteria: usize,
    pub passed: usize,
    /// 「测试 2 条、命令 1 条、未说明 1 条」, or that nothing was recorded.
    pub counts: String,
    /// Weaker evidence the lead should read, which is not a failure by itself.
    pub reminders: Vec<String>,
    pub rows: Vec<CheckRow>,
    /// The tasks that verify it independently, each with its own conclusion.
    pub verifications: Vec<VerificationView>,
}

/// One independent verification of a task, as the event log has it now.
#[derive(Clone, Debug, Serialize)]
pub struct VerificationView {
    pub task: String,
    pub status: TaskShown,
    /// Its verdict once its review is done: the one it gave, or what its checks come to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// It confirmed the task's conclusion.
    pub passed: bool,
    pub summary: String,
    pub checks: Vec<Value>,
    pub evidence: Vec<String>,
    pub risks: Vec<String>,
}

/// How a task's checks were verified, in three bands: run (tests, commands), read (inspection),
/// or only said (reasoning, unverified, no method).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Strength {
    pub ran: usize,
    pub read: usize,
    pub said: usize,
}

impl Strength {
    fn of(s: &Summary) -> Strength {
        let mut out = Strength::default();
        for m in &s.methods {
            match Method::parse(m.method) {
                Some(Method::Test | Method::Command) => out.ran += m.count,
                Some(Method::Inspection) => out.read += m.count,
                _ => out.said += m.count,
            }
        }
        out.said += s.unspecified;
        out
    }

    fn add(&mut self, o: Strength) {
        self.ran += o.ran;
        self.read += o.read;
        self.said += o.said;
    }

    fn total(&self) -> usize {
        self.ran + self.read + self.said
    }

    /// 「运行 2 · 检查 1 · 只是说法 1」
    fn text(&self, w: &Words) -> String {
        let l = w.lang;
        let (ran, read, said) = (self.ran, self.read, self.said);
        [
            (ran, tr!(l, "运行 {ran}", "Ran {ran}")),
            (read, tr!(l, "检查 {read}", "Read {read}")),
            (said, tr!(l, "只是说法 {said}", "Only said {said}")),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(_, t)| t)
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

/// The strength as the delivery row shows it: the three bands and the same in words.
#[derive(Clone, Debug, Serialize)]
pub struct StrengthView {
    #[serde(flatten)]
    pub bands: Strength,
    pub text: String,
}

/// One task's row in the delivery's acceptance list: what the record says, with its numbers in
/// words and whether it starts open.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceRow {
    pub task: String,
    pub title: String,
    pub status: TaskShown,
    pub criteria: usize,
    pub passed: usize,
    pub counts: String,
    pub reminders: Vec<String>,
    pub strength: StrengthView,
    /// 「通过 4 / 5 条 · 测试 1 条、命令 2 条」
    pub metrics: String,
    /// What needs attention, as text: the status when it is not success, risks, trace gaps,
    /// reminders, no independent verification.
    pub markers: Vec<String>,
    /// Starts open: a criterion failed, a trace gap exists, or an independent verification did not
    /// confirm it. The executor's own risks and reminders only mark the row.
    pub needs_attention: bool,
    pub checks: Vec<Value>,
    /// Each independent verification as recorded, with `passed` filled in.
    pub verifications: Vec<Value>,
    pub unverified: bool,
    pub trace_gaps: Vec<String>,
    pub risks: Vec<String>,
}

/// The delivery under review as its card shows it.
#[derive(Clone, Debug, Serialize)]
pub struct DeliveryEvidence {
    /// 「子任务 3/3 完成 · 验收标准 12/12 达到 · 运行 5 · 检查 7 · 改了 2 个文件」
    pub facts: String,
    pub tasks: Vec<AcceptanceRow>,
}

fn checks_of(v: &[Value]) -> Vec<Check> {
    v.iter().map(Check::from_json).collect()
}

/// Every criterion with the check at its place, then the checks beyond the criteria.
fn rows(accept: &[String], checks: &[Value]) -> Vec<CheckRow> {
    let mut out: Vec<CheckRow> = accept
        .iter()
        .enumerate()
        .map(|(i, c)| CheckRow { n: i + 1, criterion: c.clone(), check: checks.get(i).cloned() })
        .collect();
    for (i, c) in checks.iter().enumerate().skip(accept.len()) {
        out.push(CheckRow { n: i + 1, criterion: text(&c["criterion"]), check: Some(c.clone()) });
    }
    out
}

fn verification(v: &TaskView) -> VerificationView {
    let verdict = (v.status == TaskShown::Succeeded).then(|| {
        let given = v.verdict.as_deref().and_then(Verdict::parse);
        effective_verdict(given, v.accept.len(), &checks_of(&v.checks))
    });
    VerificationView {
        task: v.id.clone(),
        status: v.status,
        verdict,
        passed: verdict == Some(Verdict::Approved),
        summary: v.summary.clone(),
        checks: v.checks.clone(),
        evidence: v.evidence.clone(),
        risks: v.risks.clone(),
    }
}

/// Each task's evidence, by its id.
pub(super) fn tasks(st: &RunState, w: &Words) -> BTreeMap<String, TaskEvidence> {
    let l = w.lang;
    st.tasks
        .iter()
        .map(|t| {
            let s = Summary::of_in(&t.accept, &checks_of(&t.checks), l);
            let verifications = st.tasks.iter().filter(|v| v.verifies == t.id).map(verification).collect();
            let e = TaskEvidence {
                criteria: s.criteria,
                passed: s.passed,
                counts: s.counts(l),
                reminders: s.reminders,
                rows: rows(&t.accept, &t.checks),
                verifications,
            };
            (t.id.clone(), e)
        })
        .collect()
}

/// The summary a delivery recorded for one task, with its reminders in the reader's language when
/// the checks behind them were recorded too.
fn recorded_summary(r: &Value, checks: &[Check], w: &Words) -> Summary {
    let recorded = texts(&r["reminders"]);
    let reminders = if !recorded.is_empty() && !r["checks"].is_null() {
        Summary::of_in(&[], checks, w.lang).reminders
    } else {
        recorded
    };
    Summary {
        criteria: r["criteria"].as_u64().unwrap_or(0) as usize,
        passed: r["passed"].as_u64().unwrap_or(0) as usize,
        methods: list(&r["methods"])
            .iter()
            .filter_map(|m| {
                let method = Method::parse(&text(&m["method"]))?;
                let count = m["count"].as_u64().unwrap_or(0) as usize;
                Some(MethodCount { method: method.as_str(), label: method.label(), count })
            })
            .collect(),
        unspecified: r["unspecified"].as_u64().unwrap_or(0) as usize,
        invalid: texts(&r["invalid"]),
        reminders,
    }
}

/// One task's row, from the acceptance record the delivery wrote.
fn acceptance_row(r: &Value, w: &Words) -> AcceptanceRow {
    let l = w.lang;
    let checks = checks_of(list(&r["checks"]));
    let s = recorded_summary(r, &checks, w);
    let status = TaskShown::read(&text(&r["status"]));
    let verifications: Vec<Value> = list(&r["verifications"])
        .iter()
        .filter(|v| !text(&v["task"]).is_empty())
        .map(|v| {
            let mut v = v.clone();
            let passed = v["passed"].as_bool().unwrap_or(v["verdict"] == "approved");
            v["passed"] = passed.into();
            v
        })
        .collect();
    let (risks, trace_gaps) = (texts(&r["risks"]), texts(&r["traceGaps"]));
    let unverified = truthy(&r["unverified"]);
    let mut markers = vec![];
    if status != TaskShown::Succeeded {
        markers.push(w.task_status(status));
    }
    if !risks.is_empty() {
        markers.push(tr!(l, "自述风险 {} 条", "Self-reported risks: {}", risks.len()));
    }
    if !trace_gaps.is_empty() {
        markers.push(tr!(l, "对不上 {} 处", "Mismatches: {}", trace_gaps.len()));
    }
    if !s.reminders.is_empty() {
        markers.push(tr!(l, "提醒 {} 条", "Notes: {}", s.reminders.len()));
    }
    if unverified {
        markers.push(l.pick("没有独立验证", "No independent verification").into());
    }
    let unconfirmed = verifications.iter().any(|v| v["passed"] != true);
    let needs_attention =
        s.criteria > s.passed || checks.iter().any(|c| !c.passed) || !trace_gaps.is_empty() || unconfirmed;
    let counts = s.counts(l);
    let strength = Strength::of(&s);
    AcceptanceRow {
        task: text(&r["task"]),
        title: text(&r["title"]),
        status,
        criteria: s.criteria,
        passed: s.passed,
        metrics: tr!(l, "通过 {} / {} 条 · {counts}", "Passed {} of {} · {counts}", s.passed, s.criteria),
        counts,
        reminders: s.reminders,
        strength: StrengthView { bands: strength, text: strength.text(w) },
        markers,
        needs_attention,
        checks: list(&r["checks"]).to_vec(),
        verifications,
        unverified,
        trace_gaps,
        risks,
    }
}

/// The latest delivery's evidence, when there is one.
pub(super) fn delivery(st: &RunState, w: &Words) -> Option<DeliveryEvidence> {
    let l = w.lang;
    let d = &st.result()?.d;
    let tasks: Vec<AcceptanceRow> =
        list(&d["acceptance"]).iter().filter(|r| !text(&r["task"]).is_empty()).map(|r| acceptance_row(r, w)).collect();
    let mut parts = vec![];
    if tasks.is_empty() {
        parts.push(l.pick("没有子任务，负责人自己完成", "No subtasks, the lead did it itself").to_string());
    } else {
        let done = tasks.iter().filter(|t| t.status == TaskShown::Succeeded).count();
        parts.push(tr!(l, "子任务 {done}/{} 完成", "Subtasks done: {done}/{}", tasks.len()));
        let criteria: usize = tasks.iter().map(|t| t.criteria).sum();
        let passed: usize = tasks.iter().map(|t| t.passed).sum();
        parts.push(tr!(l, "验收标准 {passed}/{criteria} 达到", "Criteria met: {passed}/{criteria}"));
        let mut all = Strength::default();
        for t in &tasks {
            all.add(t.strength.bands);
        }
        if all.total() > 0 {
            parts.push(all.text(w));
        }
    }
    let changed = list(&d["changed"]).len();
    parts.push(if changed == 0 {
        l.pick("没有改动文件", "No files changed").into()
    } else {
        tr!(l, "改了 {changed} 个文件", "Files changed: {changed}")
    });
    Some(DeliveryEvidence { facts: parts.join(" · "), tasks })
}
