//! Acceptance evidence: the method behind each check, the rules that decide whether a result
//! passes, and the summary the board, `janus.board` and the delivery show.
//!
//! A check may name how it was verified (`method`). A check that names none is shown as
//! 「未说明」 and is never read as `reasoning` or as a failure. A value outside [`Method::ALL`] is kept as written in `Check::method_error` and fails
//! the result check, so a typo cannot pass as an allowed method.
//!
//! `reasoning`, `unverified` and a missing method only produce [`Summary::reminders`]: they are
//! weaker evidence, and the lead or the person decides what to do with it. A method says which
//! kind of evidence a check claims to rest on, not whether that claim is true.
//!
//! The result check that uses these gaps is described in
//! `docs/domains/20261031-run_lifecycle.md#verification`; the wire format is in
//! `docs/interfaces/20261031-events.md#evidence_methods`.

use crate::lang::{Lang, tr};
use crate::wire::wire_enum;
use serde::Serialize;
use serde_json::Value;

wire_enum! {
    /// How a check was verified; the five values the structured result may name, in the order
    /// counts and labels are listed. Only the exact values parse: anything else is not a method,
    /// not a guess.
    pub enum Method {
        /// Automated tests were run.
        Test = "test",
        /// A command, script or checker was run.
        Command = "command",
        /// Code, files, the interface or the actual state were inspected.
        Inspection = "inspection",
        /// The agent reasons about it or judges it itself.
        Reasoning = "reasoning",
        /// The evidence is missing, cannot be reproduced, or cannot be checked right now.
        Unverified = "unverified",
    }
}

impl Method {
    /// What the app reads.
    pub fn label(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// What an agent reads, in the run's language.
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            Method::Test => lang.pick("测试", "test"),
            Method::Command => lang.pick("命令", "command"),
            Method::Inspection => lang.pick("检查", "inspection"),
            Method::Reasoning => lang.pick("推理判断", "reasoning"),
            Method::Unverified => lang.pick("未核实", "unverified"),
        }
    }
}

/// One acceptance criterion as the agent verified it.
#[derive(Clone, Debug, Default)]
pub struct Check {
    pub criterion: String,
    pub passed: bool,
    pub evidence: String,
    /// The evidence method the agent named, if it named one.
    pub method: Option<Method>,
    /// A `method` value outside the allowed set, kept exactly as written so it is neither mapped
    /// onto an allowed method nor lost when the check is recorded again.
    pub method_error: Option<String>,
}

/// The fields of a recorded check: the method is written exactly as the agent gave it, so an
/// illegal value reaches the event log unchanged and is re-read the same way after a restart.
impl Serialize for Check {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let written = self.method.map(Method::as_str).or(self.method_error.as_deref());
        let mut check = s.serialize_struct("Check", 3 + usize::from(written.is_some()))?;
        check.serialize_field("criterion", &self.criterion)?;
        check.serialize_field("passed", &self.passed)?;
        check.serialize_field("evidence", &self.evidence)?;
        // No method at all keeps the old shape: absent on the wire, shown as 「未说明」.
        if let Some(method) = written {
            check.serialize_field("method", method)?;
        }
        check.end()
    }
}

impl Check {
    /// Read one check from a structured result or from an event recorded with one. A missing
    /// `method` stays `None`; an unknown one is kept in `method_error` instead of being dropped or
    /// mapped onto an allowed method.
    pub fn from_json(v: &Value) -> Check {
        let raw = &v["method"];
        let (method, method_error) = if raw.is_null() {
            (None, None)
        } else {
            let written = raw.as_str().map(str::to_owned).unwrap_or_else(|| raw.to_string());
            match raw.as_str().and_then(Method::parse) {
                Some(m) => (Some(m), None),
                None => (None, Some(written)),
            }
        };
        Check {
            criterion: v["criterion"].as_str().unwrap_or("").to_string(),
            passed: v["passed"].as_bool().unwrap_or(false),
            evidence: v["evidence"].as_str().unwrap_or("").to_string(),
            method,
            method_error,
        }
    }

    /// 「测试」/「推理判断」/「未核实」/「未说明」; an illegal value is shown as written, so it is
    /// never read as an allowed method.
    pub fn method_label(&self, lang: Lang) -> String {
        match (self.method, &self.method_error) {
            (Some(m), _) => m.label_in(lang).to_string(),
            (None, Some(raw)) => tr!(lang, "方法「{raw}」不合法（未说明）", "invalid method \"{raw}\" (none given)"),
            (None, None) => lang.pick("未说明", "no method").to_string(),
        }
    }

    /// One line as an agent reads it: `✓ [测试] 标准：证据`.
    pub fn line(&self, lang: Lang) -> String {
        let mark = if self.passed { "✓" } else { "✗" };
        let (method, criterion) = (self.method_label(lang), &self.criterion);
        let evidence = match self.evidence.trim() {
            "" => lang.pick("（没有给出证据）", "(no evidence given)"),
            e => e,
        };
        tr!(lang, "{mark} [{method}] {criterion}：{evidence}", "{mark} [{method}] {criterion}: {evidence}")
    }
}

/// How many checks used one method.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MethodCount {
    pub method: &'static str,
    pub label: &'static str,
    pub count: usize,
}

/// The numbers and reminders behind one task's checks, shown after them on the board, in
/// `janus.board` and in the delivery.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub criteria: usize,
    pub passed: usize,
    /// Checks per method, in [`Method::ALL`] order; methods nobody used are left out.
    pub methods: Vec<MethodCount>,
    /// Checks that named no method.
    pub unspecified: usize,
    /// `method` values outside the allowed set, each with the criterion it belongs to.
    pub invalid: Vec<String>,
    /// Weaker evidence that the lead should read but that does not fail the result:
    /// `reasoning`, `unverified` and checks with no method.
    pub reminders: Vec<String>,
}

impl Summary {
    /// Summarise `checks` for a task whose acceptance criteria are `accept`, as the app and the
    /// delivery record it.
    pub fn of(accept: &[String], checks: &[Check]) -> Summary {
        Summary::of_in(accept, checks, Lang::Zh)
    }

    /// The same, with the reminders in `lang`, for an agent to read.
    pub fn of_in(accept: &[String], checks: &[Check], lang: Lang) -> Summary {
        let criteria = accept.len();
        let mut s = Summary {
            criteria,
            passed: checks.iter().take(criteria).filter(|c| c.passed).count(),
            ..Default::default()
        };
        for &m in Method::ALL {
            let count = checks.iter().filter(|c| c.method == Some(m)).count();
            if count > 0 {
                s.methods.push(MethodCount { method: m.as_str(), label: m.label(), count });
            }
        }
        for (i, c) in checks.iter().enumerate() {
            let (n, name) = (i + 1, criterion_name(&c.criterion, lang));
            match (c.method, &c.method_error) {
                (Some(Method::Reasoning), _) => s.reminders.push(tr!(
                    lang,
                    "第 {n} 条「{name}」是推理判断，不是运行得到的结果",
                    "#{n} \"{name}\" rests on reasoning, not on something run"
                )),
                (Some(Method::Unverified), _) => {
                    let why = match c.evidence.trim() {
                        "" => lang.pick("没有说明原因", "no reason given"),
                        e => e,
                    };
                    s.reminders.push(tr!(
                        lang,
                        "第 {n} 条「{name}」未核实：{why}",
                        "#{n} \"{name}\" is unverified: {why}"
                    ))
                }
                (Some(_), _) => {}
                (None, Some(raw)) => s.invalid.push(invalid_method(i, name, raw, lang)),
                (None, None) => {
                    s.unspecified += 1;
                    s.reminders.push(tr!(
                        lang,
                        "第 {n} 条「{name}」没有说明证据方法",
                        "#{n} \"{name}\" names no evidence method"
                    ));
                }
            }
        }
        s
    }

    /// The method counts alone, as in 「测试 2 条、命令 1 条、未说明 1 条」.
    pub fn counts(&self, lang: Lang) -> String {
        let mut parts: Vec<String> = Method::ALL
            .iter()
            .filter_map(|&m| {
                let n = self.methods.iter().find(|c| c.method == m.as_str())?.count;
                let label = m.label_in(lang);
                Some(tr!(lang, "{label} {n} 条", "{label} {n}"))
            })
            .collect();
        let (unspecified, invalid) = (self.unspecified, self.invalid.len());
        if unspecified > 0 {
            parts.push(tr!(lang, "未说明 {unspecified} 条", "no method {unspecified}"));
        }
        if invalid > 0 {
            parts.push(tr!(lang, "方法不合法 {invalid} 条", "invalid method {invalid}"));
        }
        if parts.is_empty() {
            lang.pick("没有记录", "none recorded").to_string()
        } else {
            parts.join(lang.pick("、", ", "))
        }
    }
}

wire_enum! {
    /// One independent verification of a task (a task whose `verifies` names it), as the delivery
    /// records it. Completing the review (`status == "succeeded"`) and confirming the target's output
    /// (`verdict == Approved`) are separate facts: a verifier that finds blocking defects finishes its
    /// review cleanly with `verdict: "rejected"` and `passed: false`.
    pub enum Verdict {
        /// The verified task's output meets its acceptance criteria.
        Approved = "approved",
        /// Blocking issues were found; the executor needs to revise its work.
        Rejected = "rejected",
        /// Evidence is insufficient to confirm the target's output right now.
        Inconclusive = "inconclusive",
    }
}

impl Verdict {
    /// What the app reads.
    pub fn label(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// What an agent reads, in the run's language.
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            Verdict::Approved => lang.pick("通过", "approved"),
            Verdict::Rejected => lang.pick("需要修改", "needs changes"),
            Verdict::Inconclusive => lang.pick("证据不足", "not enough evidence"),
        }
    }
}

/// Effective verdict of a completed verification task: an explicit `verdict` wins; when omitted,
/// any unmet check (or missing check) means `Rejected`, otherwise `Approved`.
pub fn effective_verdict(verdict: Option<Verdict>, accept_len: usize, checks: &[Check]) -> Verdict {
    if let Some(v) = verdict {
        return v;
    }
    if checks.len() < accept_len || checks.iter().take(accept_len).any(|c| !c.passed) {
        Verdict::Rejected
    } else {
        Verdict::Approved
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Verification {
    pub task: String,
    pub status: &'static str,
    pub label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    pub passed: bool,
    pub summary: String,
    pub checks: Vec<Check>,
    pub evidence: Vec<String>,
    pub risks: Vec<String>,
}

/// One task's acceptance record in `run.delivered`: the numbers and reminders of its checks, every
/// check, its independent verifications (or that it changed files without one), where its result
/// disagreed with the workspace, and its risks (docs/interfaces/20261031-events.md#evidence_methods).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAcceptance {
    pub task: String,
    pub title: String,
    pub status: &'static str,
    pub label: &'static str,
    #[serde(flatten)]
    pub summary: Summary,
    pub checks: Vec<Check>,
    pub verifications: Vec<Verification>,
    /// It finished with changes to the working directory and no independent verification checks
    /// it: a fact Janus can tell, not a judgment. The lead's summary says how it checked such a
    /// task itself.
    pub unverified: bool,
    pub trace_gaps: Vec<String>,
    pub risks: Vec<String>,
}

/// Result validation aware of whether the task is an independent verifier (`is_verifier`).
/// A read-only verifier's job is to inspect and report, not to edit files: when it checks every
/// criterion with concrete evidence and reports `passed: false` or `verdict: "rejected"` /
/// `"inconclusive"`, its review is complete and valid, not a failed attempt to be retried in place.
pub fn gaps_for(
    accept: &[String],
    summary: &str,
    checks: &[Check],
    is_verifier: bool,
    verdict: Option<Verdict>,
    verdict_error: Option<&str>,
    lang: Lang,
) -> Vec<String> {
    let mut gaps = vec![];
    if summary.trim().is_empty() {
        gaps.push(tr!(lang, "结果缺少 summary", "The result has no summary"));
    }
    if let Some(raw) = verdict_error {
        let allowed = Verdict::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(lang.pick("、", ", "));
        gaps.push(tr!(
            lang,
            "结果的 verdict「{raw}」不是允许值，只能是 {allowed}",
            "The result's verdict \"{raw}\" is not allowed; it must be one of {allowed}"
        ));
    }
    for (i, criterion) in accept.iter().enumerate() {
        let n = i + 1;
        let Some(c) = checks.get(i) else {
            gaps.push(tr!(
                lang,
                "没有核对第 {n} 条验收标准「{criterion}」",
                "Acceptance criterion #{n} \"{criterion}\" was not checked"
            ));
            continue;
        };
        if !is_verifier && !c.passed {
            let why = match c.evidence.trim() {
                "" => lang.pick("没有说明原因", "no reason given"),
                e => e,
            };
            gaps.push(tr!(lang, "自认没有达到「{criterion}」：{why}", "Reported as not met: \"{criterion}\": {why}"));
        } else if c.evidence.trim().is_empty() {
            gaps.push(tr!(lang, "「{criterion}」没有给出证据", "\"{criterion}\" gives no evidence"));
        }
        if let Some(raw) = &c.method_error {
            gaps.push(invalid_method(i, criterion, raw, lang));
        }
    }
    if is_verifier && verdict == Some(Verdict::Approved) && checks.iter().take(accept.len()).any(|c| !c.passed) {
        gaps.push(tr!(
            lang,
            "verdict 写了 approved，但有验收标准填了 passed: false，请统一判定与逐条结论",
            "verdict says approved, but a criterion has passed: false; make the verdict and the checks agree"
        ));
    }
    gaps
}

/// The criterion as it reads in a reminder; an empty one is named as such instead of as「」.
fn criterion_name(criterion: &str, lang: Lang) -> &str {
    if criterion.trim().is_empty() {
        lang.pick("（没有写明标准）", "(criterion not written)")
    } else {
        criterion.trim()
    }
}

fn invalid_method(index: usize, criterion: &str, raw: &str, lang: Lang) -> String {
    let allowed = Method::ALL.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(lang.pick("、", ", "));
    let n = index + 1;
    tr!(
        lang,
        "第 {n} 条验收标准「{criterion}」的证据方法「{raw}」不是允许值，只能是 {allowed}",
        "Acceptance criterion #{n} \"{criterion}\" names the evidence method \"{raw}\", which is not allowed; it must be one of {allowed}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(criterion: &str, method: Option<&str>, evidence: &str) -> Check {
        let mut v = json!({ "criterion": criterion, "passed": true, "evidence": evidence });
        if let Some(m) = method {
            v["method"] = json!(m);
        }
        Check::from_json(&v)
    }

    fn accept(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("标准{}", i + 1)).collect()
    }

    fn gaps(accept: &[String], summary: &str, checks: &[Check]) -> Vec<String> {
        gaps_for(accept, summary, checks, false, None, None, Lang::Zh)
    }

    #[test]
    fn every_allowed_method_round_trips_and_labels_itself() {
        for (raw, label) in [
            ("test", "测试"),
            ("command", "命令"),
            ("inspection", "检查"),
            ("reasoning", "推理判断"),
            ("unverified", "未核实"),
        ] {
            let c = check("标准1", Some(raw), "证据");
            assert_eq!(c.method, Method::parse(raw), "{raw}");
            assert_eq!(c.method_label(Lang::Zh), label);
            assert_eq!(
                serde_json::to_value(&c).unwrap()["method"],
                json!(raw),
                "the event keeps the value it was given"
            );
        }
    }

    #[test]
    fn a_result_without_a_method_stays_readable_and_is_not_reasoning() {
        let c = check("标准1", None, "看起来没问题");
        assert_eq!(c.method, None);
        assert_eq!(c.method_error, None);
        assert_eq!(c.method_label(Lang::Zh), "未说明");
        // A check without a method is recorded without the field.
        assert!(serde_json::to_value(&c).unwrap().get("method").is_none());

        let s = Summary::of(&accept(2), &[c, check("标准2", Some("test"), "cargo test 通过")]);
        assert_eq!((s.criteria, s.passed, s.unspecified), (2, 2, 1));
        assert_eq!(s.counts(Lang::Zh), "测试 1 条、未说明 1 条");
        assert!(s.reminders.iter().any(|r| r.contains("没有说明证据方法")), "{:?}", s.reminders);
        assert!(
            gaps(&accept(2), "完成", &[check("标准1", None, "说明"), check("标准2", None, "说明")]).is_empty(),
            "no method is no gap"
        );
    }

    #[test]
    fn an_unknown_method_is_kept_as_written_and_fails_the_check() {
        for raw in [json!("vibes"), json!(1), json!(["test"]), json!("Test")] {
            let mut v = json!({ "criterion": "标准1", "passed": true, "evidence": "看着没问题" });
            v["method"] = raw.clone();
            let c = Check::from_json(&v);
            assert_eq!(c.method, None, "never mapped onto an allowed method: {raw}");
            let written = raw.as_str().map(str::to_owned).unwrap_or_else(|| raw.to_string());
            assert_eq!(c.method_error.as_deref(), Some(written.as_str()));
            assert!(c.method_label(Lang::Zh).contains("不合法"), "{}", c.method_label(Lang::Zh));

            let g = gaps(&accept(1), "完成", &[c]);
            assert_eq!(g.len(), 1, "{g:?}");
            assert!(g[0].contains("不是允许值") && g[0].contains(&written), "{g:?}");
        }
        // The invalid value travels on exactly as written, so recording the check again (a fix
        // round, `task.succeeded` of an accepted result, a resumed run) does not lose it and it
        // cannot turn into an allowed method along the way.
        let c = check("标准1", Some("vibes"), "看着没问题");
        let recorded = serde_json::to_value(&c).unwrap();
        assert_eq!(recorded["method"], json!("vibes"), "kept verbatim on the wire: {recorded}");
        let reread = Check::from_json(&recorded);
        assert_eq!(reread.method, None);
        assert_eq!(reread.method_error.as_deref(), Some("vibes"), "one parser reads it back the same way");
        assert!(!gaps(&accept(1), "完成", &[reread]).is_empty(), "still a failed check after a round trip");
    }

    #[test]
    fn gaps_still_find_missing_checks_evidence_and_unmet_criteria() {
        let none: Vec<Check> = vec![];
        assert!(gaps(&accept(1), "完成", &none)[0].contains("没有核对第 1 条"));
        assert!(gaps(&accept(0), "", &none)[0].contains("缺少 summary"));
        let unmet = Check::from_json(&json!({ "criterion": "标准1", "passed": false, "evidence": "没做到" }));
        assert!(gaps(&accept(1), "完成", &[unmet])[0].contains("自认没有达到"));
        let empty = check("标准1", Some("test"), " ");
        assert!(gaps(&accept(1), "完成", &[empty])[0].contains("没有给出证据"));
        // A gap in one criterion does not hide a bad method in another.
        let mixed = vec![check("标准1", None, ""), check("标准2", Some("vibes"), "看着没问题")];
        let g = gaps(&accept(2), "完成", &mixed);
        assert_eq!(g.len(), 2, "{g:?}");
    }

    #[test]
    fn reasoning_unverified_and_missing_methods_only_remind() {
        let checks = vec![
            check("测试过", Some("test"), "cargo test"),
            check("跑过命令", Some("command"), "just test"),
            check("读过代码", Some("inspection"), "读了 evidence.rs"),
            check("推断不影响", Some("reasoning"), "调用方只有这一处"),
            check("环境缺失", Some("unverified"), "本机没有 Xcode"),
            check("没写方法", None, "看起来没问题"),
        ];
        let s = Summary::of(&accept(6), &checks);
        assert!(gaps(&accept(6), "完成", &checks).is_empty(), "reminders never fail the result");
        assert_eq!((s.criteria, s.passed, s.unspecified), (6, 6, 1));
        assert_eq!(s.counts(Lang::Zh), "测试 1 条、命令 1 条、检查 1 条、推理判断 1 条、未核实 1 条、未说明 1 条");
        assert_eq!(s.reminders.len(), 3, "{:?}", s.reminders);
        assert!(s.reminders.iter().any(|r| r.contains("推理判断") && r.contains("不是运行得到的结果")));
        assert!(s.reminders.iter().any(|r| r.contains("未核实") && r.contains("本机没有 Xcode")));
        assert!(s.reminders.iter().any(|r| r.contains("没有说明证据方法")));
    }

    #[test]
    fn verifier_verdicts_separate_review_completion_from_product_approval() {
        let unmet = Check::from_json(&json!({
            "criterion": "标准1", "passed": false, "method": "test", "evidence": "cargo test 报 1 个失败"
        }));
        // A read-only verifier reporting a real defect with evidence has completed its review.
        assert!(
            gaps_for(
                &accept(1),
                "发现阻断缺陷",
                std::slice::from_ref(&unmet),
                true,
                Some(Verdict::Rejected),
                None,
                Lang::Zh
            )
            .is_empty()
        );
        assert_eq!(effective_verdict(None, 1, std::slice::from_ref(&unmet)), Verdict::Rejected);
        // Without evidence, even a negative check is incomplete.
        let bare = Check::from_json(&json!({ "criterion": "标准1", "passed": false, "evidence": " " }));
        assert!(
            gaps_for(&accept(1), "发现问题", &[bare], true, Some(Verdict::Rejected), None, Lang::Zh)
                .iter()
                .any(|g| g.contains("没有给出证据"))
        );
        // Contradicting `approved` with `passed: false` or naming an unknown verdict fails the check.
        assert!(
            gaps_for(&accept(1), "没问题", &[unmet], true, Some(Verdict::Approved), None, Lang::Zh)
                .iter()
                .any(|g| g.contains("approved"))
        );
        assert!(
            gaps_for(&accept(1), "存疑", &[check("标准1", Some("test"), "ok")], true, None, Some("maybe"), Lang::Zh)
                .iter()
                .any(|g| g.contains("不是允许值"))
        );
        assert_eq!(
            effective_verdict(Some(Verdict::Rejected), 1, &[check("标准1", Some("test"), "已完成核查")]),
            Verdict::Rejected
        );
    }

    #[test]
    fn an_english_run_reads_the_evidence_in_english() {
        let checks =
            vec![check("Tests pass", Some("test"), "cargo test"), check("Docs", Some("reasoning"), "only caller")];
        let s = Summary::of_in(&accept(2), &checks, Lang::En);
        assert_eq!(s.counts(Lang::En), "test 1, reasoning 1");
        assert!(s.reminders[0].contains("rests on reasoning"), "{:?}", s.reminders);
        assert_eq!(checks[0].line(Lang::En), "✓ [test] Tests pass: cargo test");
        assert_eq!(Summary::of(&accept(2), &checks).methods[0].label, "测试", "the app's record stays as it was");
        let g = gaps_for(&accept(1), "", &[], false, None, None, Lang::En);
        assert!(
            g.iter().any(|x| x.contains("has no summary")) && g.iter().any(|x| x.contains("was not checked")),
            "{g:?}"
        );
        assert_eq!(Verdict::Rejected.label_in(Lang::En), "needs changes");
    }
}
