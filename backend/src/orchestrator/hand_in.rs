//! How a collaborator hands its subtask back: its result (`janus.submit`) or a question for the
//! lead (`janus.ask`), each a tool call instead of text in its reply. A result is checked as it
//! comes in, so a malformed one goes straight back to the agent in the same turn; what the agent
//! handed in is taken by the attempt when the turn ends.
// @project-doc docs/domains/20261031-run_lifecycle.md#structured_result

use super::prompts::result_from_json;
use super::*;
use crate::lang::{Lang, tr};

/// What a collaborator's session handed in during its attempt, kept until the turn ends.
#[derive(Clone, Debug)]
pub(super) enum Handed {
    /// Its result; `warned` once Janus told it a criterion is unmet and it may hand in once more.
    Result { attempt: String, result: TaskResult, warned: bool },
    /// A question for the lead.
    Question { attempt: String, question: Value },
}

impl Handed {
    fn attempt(&self) -> &str {
        match self {
            Handed::Result { attempt, .. } | Handed::Question { attempt, .. } => attempt,
        }
    }
}

/// A question as the attempt asks it: its text and options.
pub(super) fn question_parts(q: &Value, lang: Lang) -> (String, Vec<String>) {
    let text = q["text"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(lang.pick("需要你的决定", "A decision is needed"));
    let options = q["options"]
        .as_array()
        .map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    (text.to_string(), options)
}

impl App {
    /// The attempt task `task` is on, and its definition, for a tool call from its session.
    fn running(&self, run: &Run, task: &str) -> std::result::Result<(String, TaskDef), String> {
        let st = run.st.lock().unwrap();
        let t = st.tasks.iter().find(|t| t.def.id == task);
        match t.and_then(|t| Some((t.attempt()?.to_string(), t.def.clone()))) {
            Some(running) => Ok(running),
            None => Err(tr!(run.lang, "子任务 {task} 现在没有在执行", "Subtask {task} is not running now")),
        }
    }

    /// `janus.submit`: take the subtask's result. One that leaves out a criterion, gives no
    /// evidence, names a method that does not exist or (for a verification) has no verdict goes
    /// back with every problem; one that reports a criterion unmet is kept, and the first time the
    /// agent hears it may fix it and hand in once more.
    // @project-doc docs/domains/20261031-run_lifecycle.md#structured_result
    pub(super) fn board_submit(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let (attempt, def) = self.running(run, task)?;
        let result = result_from_json(args);
        let verifier = def.verifies.is_some();
        // What is wrong with its form: every criterion answered with evidence and a known method,
        // a verdict from a verification. A criterion reported unmet is not a problem of form.
        let as_formed: Vec<Check> =
            result.checks.iter().map(|c| Check { passed: c.passed || !verifier, ..c.clone() }).collect();
        let malformed = crate::evidence::gaps_for(
            &def.accept,
            &result.summary,
            &as_formed,
            verifier,
            result.verdict,
            result.verdict_error.as_deref(),
            lang,
        );
        let unmet: Vec<&str> = def
            .accept
            .iter()
            .zip(&result.checks)
            .filter(|(_, c)| !verifier && !c.passed)
            .map(|(a, _)| a.as_str())
            .collect();
        if !malformed.is_empty() {
            let list = malformed.iter().map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n");
            return Err(tr!(lang, "结果没有收下：\n{list}", "The result was not taken:\n{list}"));
        }
        let mut st = run.st.lock().unwrap();
        let warned_before =
            matches!(st.handed.get(task), Some(Handed::Result { attempt: a, warned: true, .. }) if *a == attempt);
        let warn = !unmet.is_empty() && !warned_before;
        st.handed.insert(task.to_string(), Handed::Result { attempt, result, warned: warned_before || warn });
        drop(st);
        if warn {
            let list = unmet.iter().map(|u| format!("- {u}")).collect::<Vec<_>>().join("\n");
            return Ok(tr!(
                lang,
                "已收下，但这些标准你写了没达到：\n{list}\n能修好就修好再交一次，否则结束这一轮。",
                "Taken, but you reported these criteria as not met:\n{list}\nFix them and hand in once more if you can; otherwise end your turn."
            ));
        }
        Ok(lang.pick("已收下。结束这一轮就交上了。", "Taken. End your turn to hand it in.").into())
    }

    /// `janus.ask`: a question for the lead. It goes to the lead when the turn ends; the answer
    /// comes as the next prompt. A split proposal with it goes to the lead at once.
    pub(super) fn board_ask(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let (attempt, _) = self.running(run, task)?;
        if args["text"].as_str().is_none_or(|t| t.trim().is_empty()) {
            return Err(lang.pick("text 不能为空", "text cannot be empty").into());
        }
        let split = !args["split"].is_null();
        if split {
            self.propose_split(run, task, &args["split"])?;
        }
        run.st.lock().unwrap().handed.insert(task.to_string(), Handed::Question { attempt, question: args.clone() });
        Ok(if split {
            lang.pick(
                "拆分提议已发给负责人；问题在你结束这一轮后转给它，回答会作为下一条消息送来。",
                "The split proposal went to the lead; the question goes to it when you end your turn, and the answer comes as your next message.",
            )
        } else {
            lang.pick(
                "已记下。结束这一轮后转给负责人，回答会作为下一条消息送来。",
                "Noted. It goes to the lead when you end your turn, and the answer comes as your next message.",
            )
        }
        .into())
    }

    /// What the session of task `task` handed in during attempt `attempt`, taken for good.
    pub(super) fn take_handed(&self, run: &Run, task: &str, attempt: &str) -> Option<Handed> {
        let mut st = run.st.lock().unwrap();
        match st.handed.get(task) {
            Some(h) if h.attempt() == attempt => st.handed.remove(task),
            _ => None,
        }
    }
}
