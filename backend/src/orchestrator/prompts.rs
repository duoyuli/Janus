//! What sessions are told and what they answer: role instructions, the task prompt, and the
//! structured result.

use super::*;

/// What a member brings beyond the task: its role's instructions and skills, then its provider's prompt.
pub(super) fn role_extras(m: &Member, workspace: &Path) -> String {
    let mut p = String::new();
    let project = crate::agents::project_skills(workspace);
    if !project.is_empty() {
        let list: Vec<String> = project
            .iter()
            .map(|s| {
                format!(
                    "- {}：{}（{}/SKILL.md）",
                    s["name"].as_str().unwrap_or(""),
                    s["description"].as_str().unwrap_or(""),
                    s["path"].as_str().unwrap_or("")
                )
            })
            .collect();
        p.push_str(&format!(
            "\n\n这个项目自带以下 skill（路径相对于工作目录）。相关时先读取它的 SKILL.md：\n{}",
            list.join("\n")
        ));
    }
    if !m.skills.is_empty() {
        let list: Vec<String> = m
            .skills
            .iter()
            .map(|s| {
                format!(
                    "- {}：{}（完整说明：{}/SKILL.md）",
                    s["name"].as_str().unwrap_or(""),
                    s["description"].as_str().unwrap_or(""),
                    s["path"].as_str().unwrap_or("")
                )
            })
            .collect();
        p.push_str(&format!(
            "\n\n你可以使用以下 skill。当任务与某个 skill 相关时，先读取它的 SKILL.md，再按其中的说明执行：\n{}",
            list.join("\n")
        ));
    }
    if !m.mcp.is_empty() {
        let names: Vec<&str> = m.mcp.iter().filter_map(|x| x["name"].as_str()).collect();
        p.push_str(&format!("\n\n这个会话已经接入以下 MCP 服务器，可以直接调用它们提供的工具：{}", names.join("、")));
    }
    p
}

/// Where a member's standing instructions reach the model. They hold for the whole session and
/// outrank the task text, so they go in the system layer wherever the agent offers one.
// @project-doc docs/interfaces/acp_integration.md#standing_instructions
#[derive(Debug, PartialEq)]
pub(super) enum Channel {
    /// Claude Code: `session/new` / `session/load` `_meta.systemPrompt.append`.
    System,
    /// Codex: `developer_instructions` in the adapter's `CODEX_CONFIG`.
    Developer,
    /// Everyone else: a block in the first prompt of the session.
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
        parts.push(format!("来自 Agent 设置（{from}）：\n{}", m.prompt.trim()));
    }
    if !m.role_prompt.trim().is_empty() {
        parts.push(format!("角色指令（{}）：\n{}", m.name, m.role_prompt.trim()));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("以下是用户为你设定的长期要求，在整个会话中都有效。\n\n{}", parts.join("\n\n"))
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
/// criteria, the workspace (the run's), upstream results, peers, the board and the last failure.
pub(super) fn task_prompt(
    run: &Run,
    m: &Member,
    def: &TaskDef,
    upstream: &[String],
    peers: &[String],
    board: bool,
    last_error: Option<&str>,
) -> String {
    let mut p = format!(
        "你是 Janus 团队中的 {name}（{role}），主控把一个任务交给了你。

总目标：{goal}{brief}
你的任务 {id}：{title}
说明：{desc}
最多尝试 {max_attempts} 次。

验收标准：
{accept}

结束前逐条验证这些标准，验证方式由你判断：运行测试、启动程序看实际表现、读代码推理、对比输出，选能真正证明结果的方式，而不是只确认文件存在。在结果的 checks 中按顺序逐条写明是否达到、证据（做了什么、看到了什么）和可选的 method：test（运行自动化测试）、command（运行命令或脚本）、inspection（检查代码、文件、界面或实际状态）、reasoning（推理或自我判断）、unverified（证据缺失、无法复现或暂时无法核实）。method 只说明依据的类型，不代替证据；无法核实时如实写 unverified，不要写成 test。没有达到的如实写 passed: false 并说明原因；标准本身不合理或在当前环境无法验证时，在 question 中请主控调整。

工作目录：{ws}
{wsnote}
约束：不要 git push、不要部署、不要修改工作目录之外的文件。",
        name = m.name,
        role = m.role,
        goal = run.goal,
        brief = brief_section(&run.brief, "任务说明"),
        id = def.id,
        title = def.title,
        desc = def.desc,
        max_attempts = def.max_attempts,
        accept = def.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"),
        ws = run.cwd.display(),
        wsnote = "你直接在项目的工作目录中工作，改动立即生效，其他成员也会看到。不要 git commit 或 push。",
    );
    if !upstream.is_empty() {
        p.push_str(&format!("\n\n上游任务的结果（摘要）：\n{}", upstream.join("\n")));
    }
    if !peers.is_empty() {
        p.push_str(&format!(
            "\n\n现在同时在这个目录里工作的其他任务：\n{}\n改它们可能也在改的文件前，先读最新内容。",
            peers.join("\n")
        ));
    }
    if board {
        p.push_str("\n\n共享看板（janus MCP 工具）：有进展或阻塞时用 janus.board_post 写回；要改负责范围以外的文件，先用 janus.claim 认领；用 janus.peers 查看其他任务的状态、负责和改过的文件，传 task 查看某个任务（例如上游）的完整结果和主控的修订。两个任务改了同一个文件、或改了别的任务负责的文件时，Janus 会提醒。你改过的文件就是给别人看的记录：结果里说的改动要和工作目录一致，Janus 会核对。");
    }
    if let Some(err) = last_error {
        p.push_str(&format!("\n\n上一次尝试失败：{err}。请避开同样的问题。"));
    }
    p.push_str(
        "\n\n如果缺少超出你授权的关键决定，不要猜：在结果 JSON 的 question 字段写清问题和选项，然后结束本轮，主控会回答你。
完成后，回复的最后必须是一个 ```json 代码块：
{\"summary\":\"一句话结果\",\"checks\":[{\"criterion\":\"验收标准原文\",\"passed\":true,\"method\":\"test\",\"evidence\":\"怎么验证的、看到了什么\"}],\"evidence\":[\"其他可核对的证据\"],\"risks\":[\"已知风险，没有就留空数组\"],\"question\":null}
question 的格式：{\"text\":\"问题\",\"options\":[\"选项 A\",\"选项 B\"],\"recommend\":\"你建议的选项\"}",
    );
    p.push_str(&role_extras(m, &run.cwd));
    p.push_str(&prompt_instructions(m));
    p
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
        r.summary = "由你在会话中结束任务".into();
    }
    r.evidence.push("人工接管会话中的对话（见执行轨迹）".into());
    r
}

// @project-doc docs/domains/run_lifecycle.md#structured_result
pub(super) fn parse_result(text: &str) -> TaskResult {
    let strings = |v: &Value| {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).filter(|s| !s.trim().is_empty()).collect())
            .unwrap_or_default()
    };
    match extract_json(text) {
        Some(v) => TaskResult {
            summary: v["summary"].as_str().map(str::to_owned).unwrap_or_else(|| first_line(text)),
            evidence: strings(&v["evidence"]),
            risks: strings(&v["risks"]),
            checks: v["checks"].as_array().map(|a| a.iter().map(Check::from_json).collect()).unwrap_or_default(),
            question: Some(v["question"].clone())
                .filter(|q| q.is_object() || q.as_str().is_some_and(|s| !s.is_empty())),
        },
        None => TaskResult {
            summary: first_line(text),
            risks: vec!["Agent 没有按约定返回结构化结果，摘要取自回复第一行".into()],
            ..Default::default()
        },
    }
}
