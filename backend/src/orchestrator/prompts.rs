//! What sessions are told and what they answer: role instructions, the task prompt, and the
//! structured result.

use super::*;

/// Where a member's system layer reaches the model: Janus's instructions for the session and the
/// person's standing rules. They hold for the whole session and outrank the task text, so they go
/// in the system layer wherever the agent offers one.
// @project-doc docs/interfaces/20261031-acp_integration.md#standing_instructions
#[derive(Debug, PartialEq)]
pub(super) enum Channel {
    /// Claude Code: `session/new` / `session/load` `_meta.systemPrompt.append`.
    System,
    /// Codex: `developer_instructions` in the adapter's `CODEX_CONFIG`.
    Developer,
    /// Custom agents: blocks in the first prompt of the session.
    Prompt,
}

pub(super) fn instruction_channel(kind: Runtime) -> Channel {
    match kind {
        Runtime::Claude => Channel::System,
        Runtime::Codex => Channel::Developer,
        Runtime::Custom => Channel::Prompt,
    }
}

/// The person's standing rules for this member: the provider's prompt, then the role's.
/// Skills and MCP are not here: they reach the agent through its own mechanisms.
pub(super) fn standing_instructions(m: &Member) -> String {
    let mut parts = vec![];
    if !m.prompt.trim().is_empty() {
        let from = if m.runtime_name.is_empty() { m.kind.as_str() } else { &m.runtime_name };
        parts.push(format!("Agent settings ({from}):\n{}", m.prompt.trim()));
    }
    if !m.role_prompt.trim().is_empty() {
        parts.push(format!("Role instructions ({}):\n{}", m.name, m.role_prompt.trim()));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("The following standing instructions from the user apply throughout the session.\n\n{}", parts.join("\n\n"))
}

/// The same rules for an agent with no system layer, set apart from the task in the first prompt.
pub(super) fn prompt_instructions(m: &Member) -> String {
    let text = standing_instructions(m);
    if text.is_empty() || instruction_channel(m.kind) != Channel::Prompt {
        return String::new();
    }
    format!("\n\n<standing_instructions>\n{text}\n</standing_instructions>")
}

/// Codex takes developer instructions only from its config: merge them into the one this adapter
/// process starts with. Janus starts a process per session, so each role gets its own.
pub(super) fn with_developer_instructions(mut spec: crate::acp::RuntimeSpec, text: &str) -> crate::acp::RuntimeSpec {
    if text.is_empty() {
        return spec;
    }
    let i = spec.env.iter().position(|(k, _)| k == "CODEX_CONFIG");
    let mut cfg: Value =
        i.and_then(|i| serde_json::from_str(&spec.env[i].1).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    cfg["developer_instructions"] = json!(text);
    let v = cfg.to_string();
    match i {
        Some(i) => spec.env[i].1 = v,
        None => spec.env.push(("CODEX_CONFIG".into(), v)),
    }
    spec
}

/// What a collaborator's session is told about its task: the goal, the task and its acceptance
/// criteria, the workspace (the run's), upstream results, peers and the last failure. How to work
/// with Janus is in the system layer (`collaborator_rules`).
pub(super) fn task_prompt(
    run: &Run,
    m: &Member,
    def: &TaskDef,
    upstream: &[String],
    peers: &[String],
    last_error: Option<&str>,
) -> String {
    let mut p = format!(
        "你是 Janus 团队中的 {name}，负责人把一个子任务交给了你。

总目标：{goal}{brief}
你的子任务 {id}：{title}
说明：{desc}
最多尝试 {max_attempts} 次。

验收标准：
{accept}

工作目录：{ws}{note}",
        name = m.name,
        goal = run.goal,
        brief = brief_section(&run.brief, "the subtask instructions"),
        id = def.id,
        title = def.title,
        desc = def.desc,
        max_attempts = def.max_attempts,
        accept = def.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"),
        ws = run.cwd.display(),
        note = workdir_note(run),
    );
    if !upstream.is_empty() {
        p.push_str(&format!("\n\n上游子任务的结果（摘要）：\n{}", upstream.join("\n")));
    }
    if !peers.is_empty() {
        p.push_str(&format!(
            "\n\n现在同时在这个目录里工作的其他子任务：\n{}\n改它们可能也在改的文件前，先读最新内容。",
            peers.join("\n")
        ));
    }
    if let Some(err) = last_error {
        p.push_str(&format!("\n\n上一次尝试失败：{err}。请避开同样的问题。"));
    }
    p
}

/// What the lead is told about the run's working directory. A worktree is fresh: only tracked
/// files and what `.worktreeinclude` lists are there, and the run may be the one that changes
/// the environment, so the agents prepare it there themselves.
// @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
pub(super) fn workdir_brief(run: &Run) -> String {
    match &run.worktree {
        Some(w) => format!(
            "It is a Git worktree of {repo} made for this run alone, so other runs on that repository do not touch it. It holds the tracked files as of the run's start plus the ignored files the repository's .worktreeinclude lists; dependencies, build outputs and other ignored files are not there. Collaborators prepare the environment in it the way the project does (README, justfile, package.json and the like) before running anything, and reinstall when a change touches dependencies. Changes stay uncommitted: when the user accepts the run, Janus commits them to the branch {branch}.",
            repo = w.repo.display(),
            branch = crate::worktrees::branch(&run.id)
        ),
        None if run.git => "This is a Git repository.".into(),
        None => {
            "This directory is outside a Git repository. Repository-specific Git commands are unavailable here.".into()
        }
    }
}

/// What a collaborator is told about the working directory, after its path in the task prompt.
fn workdir_note(run: &Run) -> String {
    match &run.worktree {
        Some(w) => format!(
            "（{} 为这个任务单独建的 Git worktree：只有任务开始时仓库跟踪的文件，以及仓库 .worktreeinclude 列出的被忽略文件；依赖、构建产物和其他被忽略的文件都不在这里。运行测试或程序前，先按项目自己的方式准备环境（看 README、justfile、package.json 等）；你的改动涉及依赖时重新安装。改动不要提交，用户验收时由 Janus 统一提交。）",
            w.repo.display()
        ),
        None => String::new(),
    }
}

/// What a verification brought back, for the attempt that acts on it.
pub(super) enum Review {
    /// The findings the lead's retry hands over (`from`), described like an upstream result.
    Findings(String),
    /// A verification checking its reworked target again: its own last verdict, the target's new
    /// result, and the target's diff artifacts, newest first.
    Recheck { own: String, target: String, diffs: Vec<String> },
}

impl Review {
    fn section(&self) -> String {
        match self {
            Review::Findings(line) => format!("\n\n独立验证的意见（负责人要你据此返工）：\n{line}"),
            Review::Recheck { own, target, diffs } => {
                let mut p = format!(
                    "\n\n这是一次复审：你验证过的子任务已经返工。\n你上一轮的判定：{own}\n它这次的结果：\n{target}"
                );
                match diffs.as_slice() {
                    [now, before, ..] => p.push_str(&format!(
                        "\n现在的 diff：{now}\n你上一轮核对的 diff：{before}（两者的差别就是这次返工的改动）"
                    )),
                    [now] => p.push_str(&format!("\n现在的 diff：{now}")),
                    [] => {}
                }
                p.push_str("\n先核对你上一轮指出的问题是否解决，再检查返工有没有改坏别的。验收标准仍然逐条给出：没有受返工影响的条目可以沿用上一轮的证据，写明「沿用上一轮」，method 用 inspection。");
                p
            }
        }
    }
}

/// A verification's own last verdict in a few lines: the judgment, the summary, and the criteria
/// it did not pass with their evidence.
pub(super) fn previous_verdict(def: &TaskDef, result: Option<&TaskResult>) -> String {
    let Some(r) = result else {
        return "没有记录".into();
    };
    let vd = effective_verdict(r.verdict, def.accept.len(), &r.checks);
    let mut p = format!("{}（{}）：{}", vd.label(), vd.as_str(), short(&r.summary, INLINE_SUMMARY));
    for c in r.checks.iter().filter(|c| !c.passed) {
        p.push_str(&format!(
            "\n  未通过：{}：{}",
            short(&c.criterion, INLINE_ITEM),
            short(&c.evidence, INLINE_SUMMARY)
        ));
    }
    p
}

/// What a reopened task's attempt is told on top of its task when it starts in a new session:
/// the findings or its last verdict, and the lead's note.
pub(super) fn reopen_section(reopen: &Reopen, review: Option<&Review>) -> String {
    let mut p = review.map(Review::section).unwrap_or_default();
    if let Reopen::Retry { note: Some(note), .. } = reopen {
        p.push_str(&format!("\n\n负责人的说明：{note}"));
    }
    p
}

/// The prompt for an attempt back in the task's own session. The session holds the task and the
/// work so far, so only what is new goes in: the findings, the lead's note, a revised task, the
/// last failure, and the criteria to report against.
pub(super) fn continued_prompt(
    def: &TaskDef,
    n: i64,
    reopen: &Reopen,
    review: Option<&Review>,
    last_error: Option<&str>,
    notes: &str,
) -> String {
    let mut p = match reopen {
        Reopen::Retry { .. } => format!(
            "负责人让你在原来的会话中接着做子任务 {}（第 {n} 次尝试），之前的上下文和读过的文件都还在。",
            def.id
        ),
        Reopen::Reverify { target } => format!(
            "{target} 已经返工。请在原来的会话中重新验证（子任务 {}，第 {n} 次尝试），之前的上下文和读过的文件都还在。仍然不要修改任何文件。",
            def.id
        ),
    };
    p.push_str(&reopen_section(reopen, review));
    if let Reopen::Retry { revised: true, .. } = reopen {
        p.push_str(&format!("\n\n子任务说明已更新，以这一版为准：\n{}", def.desc));
    }
    if let Some(err) = last_error {
        p.push_str(&format!("\n\n上一次尝试失败：{err}。请避开同样的问题。"));
    }
    p.push_str(&format!(
        "\n\n验收标准：\n{}",
        def.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n")
    ));
    p.push_str(match (reopen, review) {
        (Reopen::Reverify { .. }, _) => "\n\n完成后以 ```json 结构化结果结束：checks 逐条给出实际执行的命令或操作和看到的输出，并用 verdict 写明判定：approved、rejected 或 inconclusive。",
        (Reopen::Retry { .. }, Some(_)) => "\n\n同意的意见就修改并重新验证；不同意的、或和任务范围冲突的不要硬改：在对应的 check 写 passed: false 说明理由，或在 question 里请负责人决定。完成后以 ```json 结构化结果结束，checks 按验收标准逐条更新。",
        (Reopen::Retry { .. }, None) => "\n\n完成后以 ```json 结构化结果结束，checks 按验收标准逐条更新。",
    });
    p.push_str(notes);
    p
}

/// How a collaborator works with Janus, for the whole session: verifying against the criteria,
/// the working directory, the board, questions and the structured result. It goes in the system
/// layer; an agent without one gets it at the end of its first prompt.
// @project-doc docs/domains/20261031-goal_mode.md#collaborator_rules
pub(super) fn collaborator_rules(board: bool) -> String {
    let mut p = String::from(
        "You are a collaborator on the Janus team. Complete the subtask assigned by the lead in the project's working directory and report the result as described below. These rules apply throughout the session.

Acceptance: Verify each criterion before finishing. Choose a method that can establish the result, such as running tests, observing the running program, inspecting code, or comparing outputs. File existence alone is insufficient. In checks, report each criterion in order with its pass status, evidence (what you did and observed), and an optional method: test (automated test), command (command or script), inspection (code, files, UI, or actual state), reasoning (inference or judgment), or unverified (missing, irreproducible, or currently unverifiable evidence). The method identifies the evidence type. Report unverified when you cannot verify a result and use test only for an automated test you ran. For unmet criteria, set passed: false and explain why. If a criterion is unreasonable or impossible to verify in the current environment, ask the lead to revise it in question.

Working directory: Work directly in the project's working directory. Your changes take effect immediately and are visible to other members. Do not commit, push, deploy, or modify files outside the working directory.",
    );
    if board {
        p.push_str("\n\nShared board (janus MCP tools): Post progress and blockers with janus.board_post. Before editing files outside your assigned scope, claim them with janus.claim. If the subtask is much larger than expected and needs splitting, propose a split with janus.request_split for the lead to decide. Use janus.peers to see the status, owned files, and edits of other subtasks. Pass task to see a specific subtask's full result and the lead's amendments. Janus alerts you when two subtasks edit the same file or a subtask edits files owned by another. Report changes that match the working directory. Janus checks the result against your edits.");
    }
    p.push_str(
        "\n\nQuestions: If a decision is essential and beyond your authority, state the question and options in the result JSON's question field and end the turn. The lead will answer.

Result: End your reply with a fenced ```json block in this format:
{\"summary\":\"One-sentence result\",\"handoff\":\"Interfaces, key decisions, and caveats for downstream subtasks. Use null if there are none.\",\"checks\":[{\"criterion\":\"Verbatim acceptance criterion\",\"passed\":true,\"method\":\"test\",\"evidence\":\"How you verified it and what you observed\"}],\"evidence\":[\"Other verifiable evidence\"],\"risks\":[\"Known risks. Use an empty array if there are none.\"],\"question\":null}
Question format: {\"text\":\"Question\",\"options\":[\"Option A\",\"Option B\"],\"recommend\":\"Recommended option\"}",
    );
    p
}

/// A session's system layer: Janus's instructions for it, then the person's standing rules.
pub(super) fn system_layer(janus: &str, standing: &str) -> String {
    [janus.trim(), standing.trim()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// Start the adapter with the system layer when its agent takes it from the environment (Codex's
/// developer instructions). Claude Code takes it in `session/new` instead.
pub(super) fn with_system(spec: crate::acp::RuntimeSpec, channel: &Channel, text: &str) -> crate::acp::RuntimeSpec {
    if channel == &Channel::Developer { with_developer_instructions(spec, text) } else { spec }
}

pub(super) fn extract_json(text: &str) -> Option<Value> {
    let mut rest = text;
    let mut last = None;
    while let Some(start) = rest.find("```json") {
        let body = &rest[start + 7..];
        let Some(end) = body.find("```") else { break };
        if let Ok(v) = serde_json::from_str::<Value>(body[..end].trim()) {
            last = Some(v);
        }
        rest = &body[end + 3..];
    }
    if last.is_some() {
        return last;
    }
    let close = text.rfind('}')?;
    text.match_indices('{')
        .rev()
        .find_map(|(i, _)| serde_json::from_str::<Value>(&text[i..=close]).ok().filter(Value::is_object))
}

/// Read the checks of a result recorded in an event, with their evidence methods; results written
/// before the field existed keep their checks and report 「未说明」.
pub(super) fn checks_from(d: &Value) -> Vec<Check> {
    d["checks"].as_array().map(|a| a.iter().map(Check::from_json).collect()).unwrap_or_default()
}

/// The person ended the attempt from the session: keep a structured result if the agent gave one.
pub(super) fn finished_by_user(text: &str) -> TaskResult {
    let mut r = parse_result(text);
    r.question = None;
    if extract_json(text).is_none() {
        r.risks = vec!["由你在会话中直接结束，Agent 没有给出结构化结果".into()];
    }
    if r.summary.trim().is_empty() {
        r.summary = "由你在会话中结束子任务".into();
    }
    r.evidence.push("人工接管会话中的对话（见执行轨迹）".into());
    r
}

pub(super) const HANDOFF_LIMIT: usize = 1200;

fn parse_verdict(raw: &Value) -> (Option<Verdict>, Option<String>) {
    if raw.is_null() {
        return (None, None);
    }
    let written = raw.as_str().map(str::trim).unwrap_or_default();
    if written.is_empty() && raw.is_string() {
        return (None, None);
    }
    let text = raw.as_str().map(|s| s.trim().to_string()).unwrap_or_else(|| raw.to_string());
    match raw.as_str().map(str::trim).and_then(Verdict::parse) {
        Some(v) => (Some(v), None),
        None => (None, Some(text)),
    }
}

fn parse_handoff(v: &Value) -> Option<String> {
    v["handoff"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(|s| short(s, HANDOFF_LIMIT))
}

/// Rebuild a `TaskResult` recorded in an event (`result.submitted` or `task.succeeded`).
pub(super) fn result_from_event(d: &Value) -> TaskResult {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).filter(|s| !s.trim().is_empty()).collect())
            .unwrap_or_default()
    };
    let (verdict, verdict_error) = parse_verdict(&d["verdict"]);
    TaskResult {
        summary: d["summary"].as_str().unwrap_or("").to_string(),
        handoff: parse_handoff(d),
        evidence: strings(&d["evidence"]),
        risks: strings(&d["risks"]),
        checks: checks_from(d),
        verdict,
        verdict_error,
        question: None,
    }
}

// @project-doc docs/domains/20261031-run_lifecycle.md#structured_result
pub(super) fn parse_result(text: &str) -> TaskResult {
    let strings = |v: &Value| {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).filter(|s| !s.trim().is_empty()).collect())
            .unwrap_or_default()
    };
    match extract_json(text) {
        Some(v) => {
            let (verdict, verdict_error) = parse_verdict(&v["verdict"]);
            TaskResult {
                summary: v["summary"].as_str().map(str::to_owned).unwrap_or_else(|| first_line(text)),
                handoff: parse_handoff(&v),
                evidence: strings(&v["evidence"]),
                risks: strings(&v["risks"]),
                checks: v["checks"].as_array().map(|a| a.iter().map(Check::from_json).collect()).unwrap_or_default(),
                verdict,
                verdict_error,
                question: Some(v["question"].clone())
                    .filter(|q| q.is_object() || q.as_str().is_some_and(|s| !s.trim().is_empty())),
            }
        }
        None => TaskResult {
            summary: first_line(text),
            risks: vec!["Agent 没有按约定返回结构化结果，摘要取自回复第一行".into()],
            ..Default::default()
        },
    }
}
