//! The audit page's system rules, checked over the whole log: at most one attempt of a task at a
//! time, retries numbered in order, task states changed only by Janus, results and evidence before
//! completion, a question with something to answer. Counted event by event.
// @project-doc docs/architecture/20261031-frontend.md#run_page

use super::words::Words;
use super::{list, truthy};
use crate::db::Event;
use crate::events::EventKind;
use crate::lang::tr;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// One rule and whether the log keeps it.
#[derive(Clone, Debug, Serialize)]
pub struct Rule {
    pub text: String,
    pub ok: bool,
    /// What it was checked over: 「3 次尝试」.
    pub note: String,
}

/// What the rules count while the events go by.
#[derive(Clone, Debug, Default)]
pub(super) struct AuditFold {
    /// Each task's running attempt.
    active: BTreeMap<String, i64>,
    /// Each task's attempt numbers, in the order they started.
    numbers: BTreeMap<String, Vec<i64>>,
    /// `task#attempt` of every submitted result.
    submitted: HashSet<String>,
    overlap: usize,
    no_evidence: usize,
    unsubmitted: usize,
    bad_question: usize,
    bad_actor: usize,
    succeeded: usize,
    questions: usize,
}

impl AuditFold {
    pub(super) fn apply(&mut self, e: &Event, d: &Value) {
        let task = e.task.clone().unwrap_or_default();
        let n = e.attempt.unwrap_or(0);
        let kind = e.kind();
        match kind {
            EventKind::AttemptStarted => {
                if self.active.contains_key(&task) {
                    self.overlap += 1;
                }
                self.active.insert(task.clone(), n);
                self.numbers.entry(task.clone()).or_default().push(n);
            }
            EventKind::AttemptFailed => {
                self.active.remove(&task);
            }
            EventKind::ResultSubmitted => {
                self.submitted.insert(format!("{task}#{n}"));
            }
            EventKind::TaskSucceeded => {
                self.active.remove(&task);
                self.succeeded += 1;
                if !truthy(&d["summary"]) || list(&d["evidence"]).is_empty() {
                    self.no_evidence += 1;
                }
                if !self.submitted.contains(&format!("{task}#{n}")) {
                    self.unsubmitted += 1;
                }
            }
            EventKind::QuestionAsked => {
                self.questions += 1;
                if !(truthy(&d["question"]) && !list(&d["options"]).is_empty()) {
                    self.bad_question += 1;
                }
            }
            _ => {}
        }
        // Event types only the orchestrator writes: a task's state changes and its attempts.
        let orchestrator_only = matches!(
            kind,
            EventKind::TaskReady
                | EventKind::TaskAssigned
                | EventKind::TaskSucceeded
                | EventKind::TaskReopened
                | EventKind::TaskRetryScheduled
        ) || e.kind.starts_with("attempt.");
        if orchestrator_only && e.actor != "janus" {
            self.bad_actor += 1;
        }
    }

    /// The five rules, worded.
    pub(super) fn rules(&self, w: &Words) -> Vec<Rule> {
        let l = w.lang;
        let gaps: usize =
            self.numbers.values().map(|ns| ns.iter().enumerate().filter(|(i, n)| **n != *i as i64 + 1).count()).sum();
        let attempts: usize = self.numbers.values().map(Vec::len).sum();
        let (tasks, succeeded, questions) = (self.numbers.len(), self.succeeded, self.questions);
        let rule = |text: &str, ok, note| Rule { text: text.into(), ok, note };
        vec![
            rule(
                l.pick("每个子任务同一时间最多一个进行中的尝试", "At most one active attempt per subtask"),
                self.overlap == 0,
                tr!(l, "{attempts} 次尝试", "Attempts: {attempts}"),
            ),
            rule(
                l.pick("重试会新建尝试，旧记录保留", "A retry starts a new attempt and retains previous records"),
                gaps == 0,
                tr!(l, "{tasks} 个子任务", "Subtasks: {tasks}"),
            ),
            rule(
                l.pick("子任务状态只由 Janus 改变", "Only Janus changes subtask state"),
                self.bad_actor == 0,
                l.pick("Agent 只提交进度与结果", "Agents submit progress and results").into(),
            ),
            rule(
                l.pick("标记完成前已提交结构化结果和证据", "Structured results and evidence precede completion"),
                self.no_evidence == 0 && self.unsubmitted == 0,
                tr!(l, "{succeeded} 次完成", "Completions: {succeeded}"),
            ),
            rule(
                l.pick("等待输入时附带可回答的问题", "Waiting for input requires an answerable question"),
                self.bad_question == 0,
                tr!(l, "{questions} 个问题", "Questions: {questions}"),
            ),
        ]
    }
}
