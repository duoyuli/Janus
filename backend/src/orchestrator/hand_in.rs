//! How a collaborator hands its subtask back: its result (`janus.submit`) or a question for the
//! lead (`janus.ask`), each a tool call instead of text in its reply. A result is checked as it
//! comes in, so a malformed one goes straight back to the agent in the same turn; what the agent
//! handed in is taken by the attempt when the turn ends.
// @project-doc docs/domains/20261031-run_lifecycle.md#structured_result

use super::board::no_task;
use super::prompts::result_from_json;
use super::*;
use crate::lang::{Lang, tr};

/// What a collaborator's session handed in during its attempt, kept until the turn ends.
#[derive(Clone, Debug)]
pub(super) enum Handed {
    /// Its result; `warned` once Janus told it a criterion is unmet and it may hand in once more.
    Result { attempt: String, result: TaskResult, warned: bool },
    /// A question for the lead.
    Question { attempt: String, question: Question },
}

impl Handed {
    fn attempt(&self) -> &str {
        match self {
            Handed::Result { attempt, .. } | Handed::Question { attempt, .. } => attempt,
        }
    }
}

/// A question for the lead, as the session asked it with `janus.ask`.
#[derive(Clone, Debug)]
pub(super) struct Question {
    pub text: String,
    pub options: Vec<String>,
    /// The option the collaborator recommends, if any.
    pub recommend: Option<String>,
}

impl Question {
    fn of(args: &Value) -> Option<Question> {
        let text = args["text"].as_str().map(str::trim).filter(|t| !t.is_empty())?.to_string();
        let options = args["options"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| o.as_str().map(str::trim))
                    .filter(|o| !o.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let recommend = args["recommend"].as_str().map(str::trim).filter(|r| !r.is_empty()).map(str::to_owned);
        Some(Question { text, options, recommend })
    }
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
        let Some(question) = Question::of(args) else {
            return Err(lang.pick("text 不能为空", "text cannot be empty").into());
        };
        let split = !args["split"].is_null();
        if split {
            self.propose_split(run, task, &args["split"])?;
        }
        run.st.lock().unwrap().handed.insert(task.to_string(), Handed::Question { attempt, question });
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

impl App {
    /// A collaborator writes progress, a blocker or a note to the board while it works.
    pub(super) fn board_post(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(tr!(lang, "text 不能为空", "text cannot be empty"));
        }
        if text.chars().count() > POST_MAX_CHARS {
            return Err(tr!(lang, "text 超过 {POST_MAX_CHARS} 个字符", "text is over {POST_MAX_CHARS} characters"));
        }
        let kind = match args["kind"].as_str().map(|k| (k, PostKind::parse(k))) {
            None => PostKind::Progress,
            Some((_, Some(kind))) => kind,
            Some((other, None)) => {
                return Err(tr!(
                    lang,
                    "kind 只能是 progress、blocker 或 note，不能是 {other}",
                    "kind must be progress, blocker or note, not {other}"
                ));
            }
        };
        let agent = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .map(|t| t.agent.clone())
            .ok_or_else(|| no_task(lang, task))?;
        let label = kind.label(Lang::Zh);
        self.emit(
            run,
            ev(EventKind::BoardPosted, &agent, format!("{label}：{}", clip(text, 80)))
                .task(task)
                .to("lead")
                .data(json!({ "kind": kind, "text": text })),
        );
        Ok(lang.pick("已写到看板。", "Written to the board.").into())
    }

    /// A collaborator's question comes with a proposal to split its oversized task into smaller
    /// subtasks (`janus.ask`, `split`); only the lead may actually submit an orchestration
    /// (`janus.orchestrate`). The lead gets the proposal at once.
    // @project-doc docs/interfaces/20261031-board_mcp.md#ask
    pub(super) fn propose_split(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<(), String> {
        let lang = run.lang;
        let reason = args["reason"].as_str().unwrap_or("").trim();
        if reason.is_empty() {
            return Err(tr!(lang, "reason 不能为空", "reason cannot be empty"));
        }
        let completed = args["completed"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let Some(items) = args["subtasks"].as_array().filter(|a| !a.is_empty()) else {
            return Err(tr!(lang, "subtasks 必须是非空数组", "subtasks must be a non-empty array"));
        };
        if items.len() > SPLIT_MAX_SUBTASKS {
            return Err(tr!(
                lang,
                "一次最多建议 {SPLIT_MAX_SUBTASKS} 个子任务",
                "At most {SPLIT_MAX_SUBTASKS} subtasks per proposal"
            ));
        }
        let agent = {
            let st = run.st.lock().unwrap();
            let me = st.tasks.iter().find(|t| t.def.id == task).ok_or_else(|| no_task(lang, task))?;
            if me.def.verifies.is_some() {
                return Err(tr!(
                    lang,
                    "{task} 是验证子任务，不能提拆分",
                    "{task} is a verification subtask and cannot propose a split"
                ));
            }
            me.agent.clone()
        };
        let mut errors = vec![];
        let mut subtasks = vec![];
        for (i, item) in items.iter().enumerate() {
            let title = item["title"].as_str().unwrap_or("").trim();
            let n = i + 1;
            if title.is_empty() {
                errors.push(tr!(lang, "第 {n} 个建议子任务缺少 title", "Proposed subtask #{n} has no title"));
                continue;
            }
            let label = tr!(lang, "建议子任务 {title}", "Proposed subtask {title}");
            let owns = crate::teamwork::read_owns(&item["owns"], &label, &mut errors, lang);
            let accept: Vec<String> = item["accept"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::trim))
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            subtasks.push(json!({ "title": title, "owns": owns, "accept": accept }));
        }
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        let mut data = json!({ "reason": reason, "subtasks": subtasks });
        if let Some(c) = completed {
            data["completed"] = json!(c);
        }
        self.emit(
            run,
            ev(EventKind::TaskSplitRequested, &agent, format!("请求拆分子任务：{}", clip(reason, 80)))
                .task(task)
                .to("lead")
                .data(data),
        );
        Ok(())
    }
}
