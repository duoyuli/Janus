//! What sessions are told and what they answer: role instructions, the task prompt, and the
//! result it hands in.

use super::*;
use crate::lang::{Lang, tr};

/// The person's standing rules for this member: the provider's prompt, then the role's.
/// Skills and MCP are not here: they reach the agent through its own mechanisms.
pub(super) fn standing_instructions(m: &Member, lang: Lang) -> String {
    let mut parts = vec![];
    if !m.prompt.trim().is_empty() {
        let from = if m.connection_name.is_empty() { m.runtime.as_str() } else { &m.connection_name };
        let prompt = m.prompt.trim();
        parts.push(tr!(lang, "Agent 设置（{from}）：\n{prompt}", "Agent settings ({from}):\n{prompt}"));
    }
    if !m.role_prompt.trim().is_empty() {
        let (name, prompt) = (&m.name, m.role_prompt.trim());
        parts.push(tr!(lang, "数字员工的指令（{name}）：\n{prompt}", "Role instructions ({name}):\n{prompt}"));
    }
    if parts.is_empty() {
        return String::new();
    }
    let parts = parts.join("\n\n");
    tr!(
        lang,
        "以下是用户的长期要求，整个会话都有效。\n\n{parts}",
        "The following standing instructions from the user apply throughout the session.\n\n{parts}"
    )
}

/// Codex takes developer instructions, and its thread's approval and sandbox settings, only from
/// its config: merge `settings` into the one this adapter process starts with. Janus starts a
/// process per session, so each member gets its own.
fn with_codex_config(mut spec: crate::acp::RuntimeSpec, settings: &[(&str, Value)]) -> crate::acp::RuntimeSpec {
    if settings.is_empty() {
        return spec;
    }
    let i = spec.env.iter().position(|(k, _)| k == "CODEX_CONFIG");
    let mut cfg: Value =
        i.and_then(|i| serde_json::from_str(&spec.env[i].1).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    for (k, v) in settings {
        cfg[*k] = v.clone();
    }
    let v = cfg.to_string();
    match i {
        Some(i) => spec.env[i].1 = v,
        None => spec.env.push(("CODEX_CONFIG".into(), v)),
    }
    spec
}

/// What a collaborator's session is told about its task: the task and its acceptance criteria
/// first, then where it sits (the goal, the working directory, the shared material), upstream
/// results, peers and the last failure. How to work with Janus is in the system layer
/// (`collaborator_rules`).
pub(super) fn task_prompt(
    run: &Run,
    def: &TaskDef,
    team_note: &str,
    upstream: &[String],
    peers: &[String],
    last_error: Option<&str>,
) -> String {
    let lang = run.lang;
    let (goal, id, title, desc, max_attempts) = (&run.goal, &def.id, &def.title, &def.desc, def.max_attempts);
    let accept = criteria(&def.accept);
    let ws = run.cwd.display();
    let note = workdir_note(run, def.verifies.is_some());
    let brief = brief_section(&run.brief, Wins::Task, lang);
    let mut p = tr!(
        lang,
        "你的子任务 {id}：{title}
说明：{desc}

验收标准：
{accept}{team_note}

总目标：{goal}
工作目录：{ws}{note}
最多尝试 {max_attempts} 次。{brief}",
        "Your subtask {id}: {title}
Instructions: {desc}

Acceptance criteria:
{accept}{team_note}

Overall goal: {goal}
Working directory: {ws}{note}
Up to {max_attempts} attempts.{brief}"
    );
    if !upstream.is_empty() {
        let lines = upstream.join("\n");
        p.push_str(&tr!(
            lang,
            "\n\n上游子任务的结果（摘要）：\n{lines}",
            "\n\nResults of upstream subtasks (summaries):\n{lines}"
        ));
    }
    if !peers.is_empty() {
        let lines = peers.join("\n");
        p.push_str(&tr!(
            lang,
            "\n\n现在同时在这个目录里工作的其他子任务（改它们可能也在改的文件前，先读最新内容）：\n{lines}",
            "\n\nOther subtasks working in this directory now (read the latest content before you change a file they may also be changing):\n{lines}"
        ));
    }
    if let Some(err) = last_error {
        p.push_str(&last_failure(err, lang));
    }
    p
}

/// Acceptance criteria, one per line.
fn criteria(accept: &[String]) -> String {
    accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n")
}

/// Why the last attempt failed, for the next one.
fn last_failure(err: &str, lang: Lang) -> String {
    let err = err.trim_end_matches(['。', '.']);
    tr!(
        lang,
        "\n\n上一次尝试失败：{err}。请避开同样的问题。",
        "\n\nThe last attempt failed: {err}. Avoid the same problem."
    )
}

/// Where the run's result goes back to: the branch the user was on, or the commit they were at.
fn result_home(w: &WorktreeOf, lang: Lang) -> String {
    match (&w.branch, lang) {
        (Some(b), Lang::Zh) => format!("用户的 {b} 分支"),
        (Some(b), Lang::En) => format!("the user's branch {b}"),
        (None, Lang::Zh) => "用户所在的提交（detached HEAD）".into(),
        (None, Lang::En) => "the commit the user is on (a detached HEAD)".into(),
    }
}

/// What the lead is told about the run's working directory, after its path: a worktree is the
/// run's own checkout, which delivery brings back to the user's branch.
// @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
pub(super) fn workdir_brief(run: &Run) -> String {
    let lang = run.lang;
    match &run.worktree {
        Some(w) => {
            let (repo, home) = (w.repo.display(), result_home(w, lang));
            tr!(
                lang,
                "这是为这个任务单独建的 Git worktree，处于 detached HEAD；依赖和构建产物不在这里，运行前按项目的方式准备。交付时 Janus 把结果提交并合并回{home}。不要切换分支，也不要在用户的仓库目录 {repo} 里运行 git。",
                "It is a Git worktree made for this run alone, on a detached HEAD; dependencies and build outputs are not in it, so prepare them the way the project does before running anything. On delivery, Janus commits the result and merges it back into {home}. Do not check out a branch here, and do not run git in the user's repository at {repo}."
            )
        }
        None if run.git => lang.pick("这是一个 Git 仓库。", "It is a Git repository.").into(),
        None => lang.pick("这个目录不在 Git 仓库中。", "It is not in a Git repository.").into(),
    }
}

/// When the lead commits, pushes, deploys or releases: only as far as the user asked. In a
/// worktree, a commit or merge the user asks for is what delivery does, and a push goes out from
/// the latest branch.
pub(super) fn commit_rule(run: &Run) -> String {
    let lang = run.lang;
    let rule = lang.pick(
        "只有用户要求时（在目标里、回答里或之后的消息里）才 commit、push、部署或发布，并且只做到要求的程度：要求提交不等于要求推送。照做时不再询问；子任务要做其中一件时，写进它的说明。",
        "Commit, push, deploy, or release only when the user asks (in the goal, an answer, or a later message), and only as far as asked: a request to commit is not one to push. Carry it out without asking again; when a subtask is to do one of them, say so in its instructions.",
    );
    let Some(w) = &run.worktree else { return rule.into() };
    let (home, target) = (result_home(w, lang), w.branch.as_deref().unwrap_or("<branch>"));
    match lang {
        Lang::Zh => format!(
            "{rule}要求提交或合并时，做完并交付即可，交付会把结果合并到{home}；要求推送时，先 git rebase {target}（有冲突就解决并重新检查），再 git push origin HEAD:{target}。"
        ),
        Lang::En => format!(
            "{rule} When asked to commit or merge, finish and deliver: delivery merges the result into {home}. When asked to push, first git rebase {target} (resolving any conflict and checking again), then git push origin HEAD:{target}."
        ),
    }
}

/// What a collaborator is told about the working directory, after its path in the task prompt. A
/// verifier changes nothing, so it hears only what is and is not in the worktree.
fn workdir_note(run: &Run, verifier: bool) -> String {
    if run.worktree.is_none() {
        return String::new();
    }
    let note = match (verifier, run.lang) {
        (true, Lang::Zh) => "（为这个任务单独建的 Git worktree：依赖和构建产物不在这里，需要时按项目的方式准备。）",
        (true, Lang::En) => {
            " (a Git worktree made for this run: dependencies and build outputs are not here, so prepare them the way the project does when you need them.)"
        }
        (false, Lang::Zh) => {
            "（为这个任务单独建的 Git worktree，处于 detached HEAD：依赖和构建产物不在这里，需要时按项目的方式准备，改了依赖就重新安装。说明没要求提交时不要提交，交付时由 Janus 统一提交；不要切换分支，也不要在用户的仓库目录里运行 git。）"
        }
        (false, Lang::En) => {
            " (a Git worktree made for this run, on a detached HEAD: dependencies and build outputs are not here, so prepare them the way the project does when you need them, and reinstall when your change touches dependencies. Unless your instructions ask you to commit, do not: Janus commits on delivery. Do not check out a branch, and do not run git in the user's repository.)"
        }
    };
    note.into()
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
    fn section(&self, lang: Lang) -> String {
        match self {
            Review::Findings(line) => tr!(
                lang,
                "\n\n独立验证的意见（负责人要你据此返工）：\n{line}",
                "\n\nFindings of the independent verification (the lead wants you to rework on them):\n{line}"
            ),
            Review::Recheck { own, target, diffs } => {
                let mut p = tr!(
                    lang,
                    "\n\n这是一次复审：你验证过的子任务已经返工。\n你上一轮的判定：{own}\n它这次的结果：\n{target}",
                    "\n\nThis is a recheck: the subtask you verified has been reworked.\nYour verdict last round: {own}\nIts result this time:\n{target}"
                );
                match diffs.as_slice() {
                    [now, before, ..] => p.push_str(&tr!(
                        lang,
                        "\n现在的 diff：{now}\n你上一轮核对的 diff：{before}（两者的差别就是这次返工的改动）",
                        "\nThe diff now: {now}\nThe diff you checked last round: {before} (the difference is what the rework changed)"
                    )),
                    [now] => p.push_str(&tr!(lang, "\n现在的 diff：{now}", "\nThe diff now: {now}")),
                    [] => {}
                }
                p.push_str(lang.pick(
                    "\n先核对你上一轮指出的问题是否解决，再检查返工有没有影响别的部分。验收标准仍然逐条给出：没有受返工影响的条目可以沿用上一轮的证据，写明「沿用上一轮」。",
                    "\nFirst check whether the problems you raised last round are resolved, then whether the rework affected anything else. Still report every criterion: those the rework did not touch may keep last round's evidence, marked \"as last round\".",
                ));
                p
            }
        }
    }
}

/// A verification's own last verdict in a few lines: the judgment, the summary, and the criteria
/// it did not pass with their evidence.
pub(super) fn previous_verdict(def: &TaskDef, result: Option<&TaskResult>, lang: Lang) -> String {
    let Some(r) = result else {
        return lang.pick("没有记录", "none on record").into();
    };
    let vd = effective_verdict(r.verdict, def.accept.len(), &r.checks);
    let summary = clip(&r.summary, INLINE_SUMMARY);
    let mut p = match lang {
        Lang::Zh => format!("{}（{}）：{summary}", vd.label(), vd.as_str()),
        Lang::En => format!("{}: {summary}", vd.as_str()),
    };
    for c in r.checks.iter().filter(|c| !c.passed) {
        let (criterion, evidence) = (clip(&c.criterion, INLINE_ITEM), clip(&c.evidence, INLINE_SUMMARY));
        p.push_str(&tr!(lang, "\n  未通过：{criterion}：{evidence}", "\n  Not met: {criterion}: {evidence}"));
    }
    p
}

/// What a reopened task's attempt is told on top of its task when it starts in a new session:
/// the findings or its last verdict, and the lead's note.
pub(super) fn reopen_section(reopen: &Reopen, review: Option<&Review>, lang: Lang) -> String {
    let mut p = review.map(|r| r.section(lang)).unwrap_or_default();
    if let Reopen::Retry { note: Some(note), .. } = reopen {
        p.push_str(&tr!(lang, "\n\n负责人的说明：{note}", "\n\nThe lead's note: {note}"));
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
    lang: Lang,
) -> String {
    let id = &def.id;
    let mut p = match reopen {
        Reopen::Retry { .. } => tr!(
            lang,
            "负责人让你在原来的会话中接着做子任务 {id}（第 {n} 次尝试），之前的上下文和读过的文件都还在。",
            "The lead wants you to go on with subtask {id} in your original session (attempt {n}); the earlier context and the files you read are still here."
        ),
        Reopen::Reverify { target } => tr!(
            lang,
            "{target} 已经返工。请在原来的会话中重新验证（子任务 {id}，第 {n} 次尝试），之前的上下文和读过的文件都还在。仍然不要修改任何文件。",
            "{target} has been reworked. Verify it again in your original session (subtask {id}, attempt {n}); the earlier context and the files you read are still here. Still do not change any file."
        ),
    };
    p.push_str(&reopen_section(reopen, review, lang));
    if let Reopen::Retry { revised: true, .. } = reopen {
        let desc = &def.desc;
        p.push_str(&tr!(
            lang,
            "\n\n子任务说明已更新，以这一版为准：\n{desc}",
            "\n\nThe subtask's instructions were revised; this version holds:\n{desc}"
        ));
    }
    if let Some(err) = last_error {
        p.push_str(&last_failure(err, lang));
    }
    let accept = criteria(&def.accept);
    p.push_str(&tr!(lang, "\n\n验收标准：\n{accept}", "\n\nAcceptance criteria:\n{accept}"));
    p.push_str(match (reopen, review) {
        (Reopen::Reverify { .. }, _) => lang.pick(
            "\n\n做完后用 janus.submit 交结果：checks 逐条写明你做了什么、看到了什么，并给出 verdict：approved、rejected 或 inconclusive。",
            "\n\nWhen done, hand in your result with janus.submit: in checks, say for each criterion what you did and what you saw, and give your verdict: approved, rejected or inconclusive.",
        ),
        (Reopen::Retry { .. }, Some(_)) => lang.pick(
            "\n\n同意的意见就修改并重新验证；不同意的、或和任务范围冲突的不要硬改：在对应的 check 写 passed: false 说明理由，或用 janus.ask 请负责人决定。做完后用 janus.submit 交结果，checks 按验收标准逐条更新。",
            "\n\nFix what you agree with and check it again; do not force changes you disagree with or that conflict with the subtask's scope: set passed: false on that check with your reason, or ask the lead in question. When done, end with the ```json structured result, with checks updated for every criterion.",
        ),
        (Reopen::Retry { .. }, None) => lang.pick(
            "\n\n做完后用 janus.submit 交结果，checks 按验收标准逐条更新。",
            "\n\nWhen done, hand in your result with janus.submit, with checks updated for every criterion.",
        ),
    });
    p.push_str(notes);
    p
}

/// How a collaborator works with Janus, for the whole session: reporting against the criteria,
/// the working directory, questions and handing in. It goes in the system layer; an
/// agent without one gets it at the end of its first prompt. The board's tools explain themselves.
// @project-doc docs/domains/20261031-goal_mode.md#collaborator_rules
pub(super) fn collaborator_rules(lang: Lang) -> String {
    lang.pick(
        "你是 Janus 团队的协作者，完成负责人交给你的子任务，按下面的约定回报。

工作方式：
- 直接在工作目录里工作，改动对其他成员立即可见；不要改工作目录之外的文件。Janus 会拿你的结果和你实际改过的文件对账。
- 只在子任务说明要求时 commit、push 或部署，并且只做到要求的程度。
- Janus 告诉你某份结果提交后文件没变时，那份结果的证据仍然描述这些文件，可以引用。

需要决定时：遇到超出你权限的决定，向负责人提问，然后结束这一轮；回答会作为下一条消息送来。

完成时：交结果，按验收标准的顺序逐条回报是否达到和证据。没达到的写明原因；标准本身不合理或在这里无法核实时，向负责人提问请它修改。交上以后结束这一轮，子任务才算交了。",
        "You are a collaborator on the Janus team: do the subtask the lead gives you and report as below.

How you work:
- Work directly in the working directory; your changes are visible to other members at once. Do not change files outside it. Janus checks your result against the files you actually changed.
- Commit, push, or deploy only when your subtask's instructions ask for it, and only as far as they ask.
- When Janus tells you the files have not changed since another member handed in a result, that result's evidence still describes them, and you may cite it.

When you need a decision: If a decision is beyond your authority, ask the lead and end your turn; the answer comes as your next message.

When you finish: Hand in your result, reporting each acceptance criterion in order with whether it is met and the evidence. Say why an unmet one is not met; if a criterion is unreasonable or cannot be checked here, ask the lead to revise it. Once it is taken, end your turn: that hands the subtask in.",
    )
    .to_string()
}

/// A session's system layer: Janus's instructions for it (its brief or rules, then where the
/// library's skills are listed), then the person's standing rules.
pub(super) fn system_layer(parts: &[&str]) -> String {
    parts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// How the adapter starts for `member`'s session: with the system layer when its agent takes it
/// from the environment (Codex's developer instructions; Claude Code takes it in `session/new`),
/// and with the settings its permission mode needs from the start (`Runtime::mode_settings`).
pub(super) fn member_spec(spec: crate::acp::RuntimeSpec, member: &Member, system: &str) -> crate::acp::RuntimeSpec {
    let mut settings: Vec<(&str, Value)> = vec![];
    if member.runtime.instruction_channel() == InstructionChannel::Developer && !system.is_empty() {
        settings.push(("developer_instructions", json!(system)));
    }
    let mode = member.session["mode"].as_str().unwrap_or_default();
    settings.extend(member.runtime.mode_settings(mode).iter().map(|(k, v)| (*k, json!(v))));
    with_codex_config(spec, &settings)
}

/// Read the checks of a result, with their evidence methods; a check without one reports 「未说明」.
pub(super) fn checks_from(d: &Value) -> Vec<Check> {
    d["checks"].as_array().map(|a| a.iter().map(Check::from_json).collect()).unwrap_or_default()
}

/// The person ended the attempt from the session: the result the agent handed in, if it did;
/// otherwise its last reply's first line, marked as such.
pub(super) fn finished_by_user(text: &str, handed: Option<TaskResult>) -> TaskResult {
    let mut r = handed.unwrap_or_else(|| TaskResult {
        summary: first_line(text),
        risks: vec!["由你在会话中直接结束，Agent 没有交结果".into()],
        ..Default::default()
    });
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
    v["handoff"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(|s| clip(s, HANDOFF_LIMIT))
}

/// A result as JSON: the arguments of `janus.submit`, or a result recorded in an event
/// (`result.submitted`, `task.succeeded`, which also keep the files it was made on).
// @project-doc docs/domains/20261031-run_lifecycle.md#structured_result
pub(super) fn result_from_json(d: &Value) -> TaskResult {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).filter(|s| !s.trim().is_empty()).collect())
            .unwrap_or_default()
    };
    let (verdict, verdict_error) = parse_verdict(&d["verdict"]);
    TaskResult {
        summary: d["summary"].as_str().unwrap_or("").trim().to_string(),
        handoff: parse_handoff(d),
        evidence: strings(&d["evidence"]),
        risks: strings(&d["risks"]),
        checks: checks_from(d),
        verdict,
        verdict_error,
        files: serde_json::from_value(d["files"].clone()).ok(),
    }
}

/// When bringing the run's result back stopped on something the lead resolves. Either way the lead
/// hears it as a message, and the turn that deals with it delivers the run again, which merges.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MergeAt {
    /// The run was being delivered.
    Delivery,
    /// The person accepted a delivery that had not brought its result back.
    Acceptance,
}

/// What the lead is told when bringing the result back stopped on something in its worktree: the
/// branch's new commits conflict with the run's (replay the run onto the branch and resolve it),
/// or an operation is left unfinished there (finish or abort it). `None` for any other failure,
/// which concerns the person's own directory or branch and goes back to them.
// @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
pub(super) fn merge_handback(err: &anyhow::Error, at: MergeAt, lang: Lang) -> Option<String> {
    let opening = match at {
        MergeAt::Delivery => lang.pick(
            "[Janus] 交付时没能把结果合并到用户的分支",
            "[Janus] Delivery could not merge the result into the user's branch",
        ),
        MergeAt::Acceptance => lang.pick(
            "[Janus] 用户点了验收，但没能合并",
            "[Janus] The user accepted the delivery, but it could not be merged",
        ),
    };
    let then = lang.pick(
        "最后用几句话告诉用户发生了什么、怎么处理的。这一轮结束后 Janus 会再次交付，并把结果合并到用户的分支。",
        "Then tell the user in a few sentences what happened and how you resolved it. After this turn Janus delivers again and merges the result into the user's branch.",
    );
    let resolve = lang.pick(
        "- 逐个解决冲突：按两边的意图合在一起，不要丢掉任何一边；拿不准保留哪边时问用户。",
        "- Resolve each conflict by combining what both sides meant to do; do not drop either side. When unsure which to keep, ask the user.",
    );
    if let Some(c) = err.downcast_ref::<crate::worktrees::Conflict>() {
        let files = match (c.files.is_empty(), lang) {
            (true, _) => String::new(),
            (false, Lang::Zh) => format!("（{}）", c.files.join("、")),
            (false, Lang::En) => format!(" ({})", c.files.join(", ")),
        };
        let (branch, head, onto) = (&c.branch, &c.head[..c.head.len().min(8)], &c.onto);
        return Some(tr!(
            lang,
            "{opening}：分支 {branch} 在任务开始后有了新的提交，和这个任务的改动冲突{files}。用户的目录和分支都没有动；任务的结果已由 Janus 提交在这个 worktree 的 {head}。

在这个 worktree 里：
- 把任务自己的提交重放到 {branch} 的最新提交上：`GIT_EDITOR=true git rebase {onto}`
{resolve}
- 变基完成后，跑和这些文件相关的检查。不要改 {branch}，也不要把它 merge 进来。

{then}",
            "{opening}: branch {branch} got new commits after the run started, and they conflict with this run's changes{files}. The user's directory and branch are untouched; Janus has committed the run's result in this worktree at {head}.

In this worktree:
- Replay the run's own commits onto the latest commit of {branch}: `GIT_EDITOR=true git rebase {onto}`
{resolve}
- When the rebase is done, run the checks that concern these files. Do not change {branch} and do not merge it in.

{then}"
        ));
    }
    let u = err.downcast_ref::<crate::worktrees::Unfinished>()?;
    let files = match (u.files.is_empty(), lang) {
        (true, _) => String::new(),
        (false, Lang::Zh) => format!("（还有冲突的文件：{}）", u.files.join("、")),
        (false, Lang::En) => format!(" (files still in conflict: {})", u.files.join(", ")),
    };
    let what = u.what.label(lang);
    Some(tr!(
        lang,
        "{opening}：这个 worktree 里还有没做完的{what}{files}，Janus 没有提交，也没有动用户的目录和分支。

在这个 worktree 里：
- 先把它做完，直到 git status 不再显示进行中的操作；确实做不下去时中止它（git rebase --abort 或 git merge --abort），改动仍在提交里。
{resolve}
- 然后跑和这些文件相关的检查。

{then}",
        "{opening}: this worktree has an unfinished {what}{files}. Janus committed nothing and left the user's directory and branch alone.

In this worktree:
- Finish it first, until git status shows no operation in progress; if it really cannot be done, abort it (git rebase --abort or git merge --abort), and the changes stay in the commits.
{resolve}
- Then run the checks that concern these files.

{then}"
    ))
}
