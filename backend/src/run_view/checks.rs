//! A criterion with the check that answered it, as the person reads it (the apps' `CheckLine`
//! and `MethodTag`): passed, failed or not checked; how it was checked, in a word, a colour, a
//! symbol and what that means; the evidence, and whether it is long enough to fold; and the whole
//! line read out. Written once here for every client.
// @project-doc docs/architecture/20261031-frontend.md#evidence_methods

use super::card::Tone;
use super::text;
use crate::evidence::Method;
use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::Value;

crate::wire::wire_enum! {
    /// How a check came out.
    pub enum Mark {
        Passed = "passed",
        Failed = "failed",
        /// Nothing answered the criterion yet.
        Unchecked = "unchecked",
    }
}

/// How a check says it was verified.
#[derive(Clone, Debug, Serialize)]
pub struct MethodTag {
    pub text: String,
    pub tone: Tone,
    /// The symbol it is drawn with, by its SF Symbols name (the web client draws the same names).
    pub icon: &'static str,
    /// What the method means.
    pub help: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckLine {
    pub mark: Mark,
    pub method: MethodTag,
    /// The criterion, or that none was written.
    pub criterion: String,
    /// The evidence as given; empty when none was (`no_evidence` then says so).
    pub evidence: String,
    pub no_evidence: Option<String>,
    /// Long or several lines: shown cut to two lines until asked for.
    pub folds: bool,
    /// The whole line read out, for a screen reader.
    pub voice: String,
}

fn method(check: Option<&Value>, l: Lang) -> MethodTag {
    let tag = |text: String, tone, icon, help: &str| MethodTag { text, tone, icon, help: help.into() };
    let Some(c) = check else {
        return tag(
            l.pick("未核对", "Not checked").into(),
            Tone::Gray,
            "minus.circle",
            l.pick("还没有核对这条标准", "This criterion has not been checked"),
        );
    };
    let raw = &c["method"];
    match raw.as_str().and_then(Method::parse) {
        Some(Method::Test) => tag(
            l.pick("测试", "Test").into(),
            Tone::Green,
            "checkmark.seal",
            l.pick("运行自动化测试得到的结果", "Result from an automated test"),
        ),
        Some(Method::Command) => tag(
            l.pick("命令", "Command").into(),
            Tone::Blue,
            "terminal",
            l.pick("运行命令、脚本或检查工具得到的结果", "Result from a command, script, or checking tool"),
        ),
        Some(Method::Inspection) => tag(
            l.pick("检查", "Inspection").into(),
            Tone::Blue,
            "doc.text.magnifyingglass",
            l.pick(
                "检查代码、文件、界面或实际状态得到的结果",
                "Result from inspecting code, files, the interface, or the actual state",
            ),
        ),
        Some(Method::Reasoning) => tag(
            l.pick("推理判断", "Reasoning").into(),
            Tone::Amber,
            "brain",
            l.pick(
                "根据推理或自我判断得到的结果，不是运行得到的结果",
                "Result based on reasoning or judgment without an executed check",
            ),
        ),
        Some(Method::Unverified) => tag(
            l.pick("未核实", "Unverified").into(),
            Tone::Amber,
            "questionmark.circle",
            l.pick(
                "证据缺失、复现不了或暂时没法核实",
                "Evidence is missing, cannot be reproduced, or cannot be checked right now",
            ),
        ),
        None if raw.is_null() => tag(
            l.pick("未说明", "Unspecified").into(),
            Tone::Gray,
            "minus.circle",
            l.pick("这次结果没有说明证据方法", "This result does not specify an evidence method"),
        ),
        None => {
            let bad = raw.as_str().map(str::to_string).unwrap_or_else(|| raw.to_string());
            tag(
                tr!(l, "证据方法「{bad}」不认识（没说明）", "Unknown evidence method “{bad}” (not given)"),
                Tone::Red,
                "exclamationmark.triangle",
                l.pick("不认识的证据方法", "Unknown evidence method"),
            )
        }
    }
}

/// Criterion `criterion` (the `n`th, when it has a place) with `check`, the check that answered it.
pub fn line(n: Option<usize>, criterion: &str, check: Option<&Value>, l: Lang) -> CheckLine {
    let method = method(check, l);
    let mark = match check {
        None => Mark::Unchecked,
        Some(c) if c["passed"] == true => Mark::Passed,
        Some(_) => Mark::Failed,
    };
    let evidence = check.map(|c| text(&c["evidence"]).trim().to_string()).unwrap_or_default();
    let criterion = if criterion.is_empty() {
        l.pick("（没有写明标准）", "(No criterion specified)").to_string()
    } else {
        criterion.to_string()
    };
    let head = n.map(|n| tr!(l, "第 {n} 条，", "Criterion {n}, ")).unwrap_or_default();
    let voice = match mark {
        Mark::Unchecked => tr!(l, "{head}未核对，{criterion}", "{head}not checked, {criterion}"),
        _ => {
            let state = if mark == Mark::Passed { l.pick("通过", "Passed") } else { l.pick("没通过", "Failed") };
            let m = &method.text;
            let said = if evidence.is_empty() {
                l.pick("，没有给出证据", ", no evidence provided").to_string()
            } else {
                tr!(l, "，证据：{evidence}", ", evidence: {evidence}")
            };
            tr!(
                l,
                "{head}{state}，证据方法：{m}，{criterion}{said}",
                "{head}{state}, Evidence method: {m}, {criterion}{said}"
            )
        }
    };
    CheckLine {
        folds: evidence.chars().count() > 120 || evidence.contains('\n'),
        no_evidence: (check.is_some() && evidence.is_empty())
            .then(|| l.pick("没有给出证据", "No evidence provided").to_string()),
        mark,
        method,
        criterion,
        evidence,
        voice,
    }
}

/// The checks of a record as lines, numbered in their order.
pub fn lines(checks: &[Value], l: Lang) -> Vec<CheckLine> {
    checks.iter().enumerate().map(|(i, c)| line(Some(i + 1), &text(&c["criterion"]), Some(c), l)).collect()
}
