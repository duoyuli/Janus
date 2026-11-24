//! What the board offers and says: the tool schemas, the orchestration check, and the board
//! messages the lead reads.

use super::*;

/// The board tools touch nothing but the board: no files, nothing outside this machine. Tools that
/// only read it or post to it are marked read-only so agents do not stop to ask the person about
/// them; the ones that start or steer work are left to the agent's own permission mode.
pub(super) fn board_read() -> Value {
    json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

pub(super) fn board_act() -> Value {
    json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

pub(super) fn lead_tools() -> Value {
    json!([
        {
            "name": "team",
            "description": "查看协作者（名字和职责）和共享看板上所有子任务的当前状态。",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": board_read()
        },
        {
            "name": "orchestrate",
            "description": "提交一份编排：一组交给协作者的子任务。Janus 先校验（执行者是协作者、id 不重复、依赖存在且无环、可能同时执行的子任务负责的文件不重叠、验证者不是被验证子任务的执行者），不通过时返回全部错误且不执行任何子任务；通过后写入共享看板并按依赖执行。可以多次提交，后提交的子任务可以依赖之前提交的子任务。",
            "inputSchema": {
                "type": "object",
                "required": ["tasks"],
                "properties": {
                    "tasks": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["id", "title", "instructions", "assignee", "accept"],
                            "properties": {
                                "id": { "type": "string", "description": "子任务 id，字母数字和 - _，在整个看板里唯一，例如 T1" },
                                "title": { "type": "string" },
                                "instructions": { "type": "string", "description": "给执行者的完整说明：背景、要做什么、注意什么。执行者看不到你的对话。" },
                                "assignee": { "type": "string", "description": "协作者 id" },
                                "deps": { "type": "array", "items": { "type": "string" }, "description": "必须先成功完成的子任务 id" },
                                "accept": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "可核对的验收标准：做到什么、怎样算做到。执行者自己决定怎么验证，逐条给出是否达到、证据和可选的证据方法（test、command、inspection、reasoning、unverified）" },
                                "owns": { "type": "array", "items": { "type": "string" }, "description": "这个子任务负责修改的文件或目录（相对工作目录，目录包含其下所有文件）。可能同时执行的子任务不能重叠" },
                                "verifies": { "type": "string", "description": "独立验证：被验证子任务的 id。交给另一个协作者，自动在被验证的子任务完成后开始，只检查不改文件" },
                                "continues": { "type": "string", "description": "续接原会话：同一个协作者之前子任务的 id，适合要用到那个子任务上下文的新工作（自动加入依赖；上下文超过 75% 或不支持时自动回退新建会话）。验证没通过的返工不用新建子任务，用 janus.retry" }
                            }
                        }
                    }
                }
            },
            "annotations": board_act()
        },
        {
            "name": "wait",
            "description": "从共享看板取回发给你的新消息：子任务开始、完成（摘要、证据、风险、产物）、失败，协作者写回的进展、阻塞和拆分提议，以及协作者的提问。有子任务完成或需要你处理的消息时立即返回，连同之前攒下的开始、进展等消息一起；否则最多等 timeout_sec 秒（默认 120，最大 180），超时返回攒下的消息和运行中子任务的实时进展。",
            "inputSchema": { "type": "object", "properties": { "timeout_sec": { "type": "integer", "minimum": 1, "maximum": 180 } } },
            "annotations": board_read()
        },
        {
            "name": "reply",
            "description": "回答协作者在某个子任务里的提问，或给正在执行的协作者补充指示。消息立即送达：能插话时插入当前这一轮，否则打断它当前这一轮后发送。子任务已经结束时会返回错误。",
            "inputSchema": {
                "type": "object",
                "required": ["task", "text"],
                "properties": { "task": { "type": "string" }, "text": { "type": "string" } }
            },
            "annotations": board_act()
        },
        {
            "name": "retry",
            "description": "让一个已经结束的子任务再做一次，重新给满尝试次数：失败或已取消的，或者已完成但需要返工的（例如独立验证判定没通过）。每次都要选会话：continue 回到这个子任务原来的会话，上下文都在；new 新开一个会话。from 指向一个独立验证子任务时，它的意见原样交给执行者，返工完成后它重新验证；这个子任务的其他独立验证也会重新验证。被它阻塞的子任务会在它成功后自动继续。",
            "inputSchema": {
                "type": "object",
                "required": ["task", "session"],
                "properties": {
                    "task": { "type": "string" },
                    "session": { "type": "string", "enum": ["continue", "new"], "description": "continue：接着这个子任务原来的会话做（按意见修改、补漏、接着没做完的）；new：新开一个会话做（方向要换、上下文已经乱了、原来的进程出错）。上下文超过 75% 或 Agent 不支持加载时，continue 自动改为新开会话" },
                    "from": { "type": "string", "description": "依据哪个独立验证子任务的意见返工；它的判定、没通过的标准和证据、建议会原样交给执行者，不需要你转述" },
                    "note": { "type": "string", "description": "你对这一轮的补充：判断、取舍、范围上的决定" },
                    "instructions": { "type": "string", "description": "新的完整说明；不填则沿用原说明" },
                    "accept": { "type": "array", "items": { "type": "string" }, "description": "新的验收标准；不填则沿用" }
                }
            },
            "annotations": board_act()
        },
        {
            "name": "accept",
            "description": "接受一个没有通过验收核对的子任务的结果：执行者自认没有完全达到某条标准，但你核对后认为可以接受。子任务算作完成，没达到的部分作为风险记录，依赖它的子任务继续执行。",
            "inputSchema": {
                "type": "object",
                "required": ["task", "reason"],
                "properties": { "task": { "type": "string" }, "reason": { "type": "string", "description": "你核对了什么，为什么没达到的部分不影响目标" } }
            },
            "annotations": board_act()
        },
        {
            "name": "amend",
            "description": "修订一个已完成子任务的结论（例如推翻它的选型）。正在执行的下游子任务立即收到修订，还没开始的在交接时拿到；已经基于旧结论完成的下游子任务会列出来，由你决定是否重做。",
            "inputSchema": {
                "type": "object",
                "required": ["task", "text"],
                "properties": { "task": { "type": "string" }, "text": { "type": "string", "description": "修订后的结论，以及和原结论的区别" } }
            },
            "annotations": board_act()
        },
        {
            "name": "cancel",
            "description": "取消不再需要的子任务：还没开始的不再开始，执行中的被停止，依赖它们的子任务一并取消。",
            "inputSchema": {
                "type": "object",
                "required": ["tasks"],
                "properties": { "tasks": { "type": "array", "items": { "type": "string" } }, "reason": { "type": "string" } }
            },
            "annotations": board_act()
        },
        {
            "name": "task_detail",
            "description": "查看看板上一个子任务的全部信息：说明、验收标准、完整结果（逐条验收及其证据方法、全部证据、风险、证据汇总）、独立验证的结果、你的修订、改过的文件、产物、看板上的进展和问答，以及它的上游和下游子任务。janus.wait 里的完成消息只有摘要，需要细节时用它。",
            "inputSchema": { "type": "object", "required": ["task"], "properties": { "task": { "type": "string" } } },
            "annotations": board_read()
        },
        {
            "name": "ask_user",
            "description": "把只有用户能做的决定交给用户：需求有多种理解且结果明显不同、需要用户的账号或付费资源、不可逆的操作。技术选型和实现方式由你自己决定，不要用这个工具。不会等待：用户的回答出现在 janus.wait 里。",
            "inputSchema": {
                "type": "object",
                "required": ["question", "why"],
                "properties": {
                    "question": { "type": "string" },
                    "why": { "type": "string", "description": "为什么必须由用户决定，不同答案会怎样影响结果" },
                    "options": { "type": "array", "items": { "type": "string" } },
                    "recommend": { "type": "string", "description": "你建议的选项" }
                }
            },
            "annotations": board_act()
        }
    ])
}

/// What one collaborator call may put on the board: enough for any real report, not enough to
/// flood the database or the lead's context (AT-17). The tools' schemas state them too.
pub(super) const POST_MAX_CHARS: usize = 8000;
pub(super) const CLAIM_MAX_PATHS: usize = 100;
pub(super) const SPLIT_MAX_SUBTASKS: usize = 8;

pub(super) fn member_tools() -> Value {
    json!([{
        "name": "board_post",
        "description": "执行子任务期间，把进展、阻塞或需要负责人注意的事写到共享看板，负责人会看到。需要负责人做决定才能继续时，不要在这里等：在结果 JSON 的 question 字段提问并结束本轮。",
        "inputSchema": {
            "type": "object",
            "required": ["text"],
            "properties": {
                "kind": { "type": "string", "enum": ["progress", "blocker", "note"] },
                "text": { "type": "string", "maxLength": POST_MAX_CHARS }
            }
        },
        "annotations": board_read()
    }, {
        "name": "claim",
        "description": "开始改一批不在你负责范围内的文件前先认领：认领后它们归你的子任务负责，同时进行的其他子任务改它们时 Janus 会提醒对方和负责人；子任务结束后认领自动失效。已经归另一个可能同时进行的子任务负责的文件不能认领，会返回负责它的子任务。验证子任务不改文件，不能认领。",
        "inputSchema": {
            "type": "object",
            "required": ["paths"],
            "properties": {
                "paths": { "type": "array", "items": { "type": "string" }, "maxItems": CLAIM_MAX_PATHS, "description": "相对工作目录的文件或目录，目录包含其下所有文件；文件多时认领所在的目录" }
            }
        },
        "annotations": board_read()
    }, {
        "name": "peers",
        "description": "查看看板上其他子任务：状态、负责的文件、和你的关系（你的上游、依赖你、和你改了同一批文件）、正在执行的子任务改过的文件、完成后又被别人改过的文件，以及最近写回看板的内容。传 task 查看某个子任务的完整说明、结果和负责人的修订，例如上游子任务的证据。",
        "inputSchema": { "type": "object", "properties": { "task": { "type": "string" } } },
        "annotations": board_read()
    }, {
        "name": "request_split",
        "description": "发现子任务规模远超预期、需要拆成多个子任务或调整负责边界时，向负责人提交拆分提议。负责人会立即收到推送并用 orchestrate / amend / reply 处理。验证子任务不能请求拆分。",
        "inputSchema": {
            "type": "object",
            "required": ["reason", "subtasks"],
            "properties": {
                "reason": { "type": "string", "description": "为什么需要拆分或调整边界" },
                "completed": { "type": "string", "description": "当前已经完成、可保留的前置部分（可选）" },
                "subtasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": SPLIT_MAX_SUBTASKS,
                    "items": {
                        "type": "object",
                        "required": ["title"],
                        "properties": {
                            "title": { "type": "string" },
                            "owns": { "type": "array", "items": { "type": "string" } },
                            "accept": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                }
            }
        },
        "annotations": board_read()
    }])
}

/// Check an orchestration the lead submitted, against the team and the tasks already on the
/// board. Every problem is reported at once, so one round of fixes is enough.
// @project-doc docs/interfaces/20261031-board_mcp.md#orchestration_validation
pub(super) fn validate_orchestration(
    run: &Run,
    board: &[(TaskDef, TaskStatus)],
    args: &Value,
) -> std::result::Result<Vec<TaskDef>, Vec<String>> {
    let mut errors = vec![];
    let existing: Vec<String> = board.iter().map(|(d, _)| d.id.clone()).collect();
    let Some(items) = args["tasks"].as_array().filter(|a| !a.is_empty()) else {
        return Err(vec!["tasks 必须是非空数组".into()]);
    };
    let members: Vec<&str> = run.team.iter().filter(|m| !m.lead).map(|m| m.id.as_str()).collect();
    let mut ids: Vec<String> = vec![];
    let mut defs = vec![];
    for (i, t) in items.iter().enumerate() {
        let id = t["id"].as_str().unwrap_or("").trim().to_string();
        let label = if id.is_empty() { format!("第 {} 个子任务", i + 1) } else { format!("子任务 {id}") };
        if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            errors.push(format!("{label}：id 只能用字母、数字、- 和 _，最长 32 个字符"));
        } else if existing.contains(&id) || ids.contains(&id) {
            errors.push(format!("{label}：id 和看板上已有的子任务重复"));
        }
        let title = t["title"].as_str().unwrap_or("").trim().to_string();
        let instructions = t["instructions"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            errors.push(format!("{label}：缺少 title"));
        }
        if instructions.is_empty() {
            errors.push(format!("{label}：缺少 instructions，执行者看不到你的对话，必须写清要做什么"));
        }
        let assignee = t["assignee"].as_str().unwrap_or("").trim().to_string();
        if !members.contains(&assignee.as_str()) {
            errors.push(format!(
                "{label}：执行者 `{assignee}` 不是协作者，可选：{}",
                if members.is_empty() { "（没有协作者）".to_string() } else { members.join("、") }
            ));
        }
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        let accept = strings(&t["accept"]);
        if accept.is_empty() {
            errors.push(format!(
                "{label}：缺少 accept：写出可核对的验收标准（做到什么、怎样算做到），执行者会逐条验证并给出证据"
            ));
        }
        let owns = crate::teamwork::read_owns(&t["owns"], &label, &mut errors);
        let verifies = t["verifies"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let continues = t["continues"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        ids.push(id.clone());
        defs.push(TaskDef {
            id,
            title,
            desc: instructions,
            // A dependency named twice is one dependency, not a cycle (AT-06).
            deps: strings(&t["deps"]).into_iter().fold(vec![], |mut deps, d| {
                if !deps.contains(&d) {
                    deps.push(d);
                }
                deps
            }),
            accept,
            assignee,
            max_attempts: MAX_ATTEMPTS,
            owns,
            verifies,
            continues,
        });
    }
    for d in &defs {
        for dep in &d.deps {
            if dep == &d.id {
                errors.push(format!("子任务 {}：不能依赖自己", d.id));
            } else if !ids.contains(dep) && !existing.contains(dep) {
                errors.push(format!("子任务 {}：依赖 {dep} 不存在", d.id));
            }
        }
    }
    // Only the new tasks can form a cycle: the board's earlier tasks never depend on them.
    let mut indegree: HashMap<&str, usize> = defs.iter().map(|d| (d.id.as_str(), 0)).collect();
    for d in &defs {
        *indegree.get_mut(d.id.as_str()).unwrap() += d.deps.iter().filter(|x| ids.contains(x)).count();
    }
    let mut queue: Vec<&str> = indegree.iter().filter(|(_, n)| **n == 0).map(|(id, _)| *id).collect();
    let mut seen = 0;
    while let Some(id) = queue.pop() {
        seen += 1;
        for d in defs.iter().filter(|d| d.deps.iter().any(|x| x == id)) {
            let n = indegree.get_mut(d.id.as_str()).unwrap();
            *n -= 1;
            if *n == 0 {
                queue.push(d.id.as_str());
            }
        }
    }
    if seen != defs.len() && errors.is_empty() {
        errors.push("依赖之间有环".into());
    }
    if errors.is_empty() {
        crate::teamwork::check(&mut defs, board, &mut errors);
    }
    if errors.is_empty() { Ok(defs) } else { Err(errors) }
}

/// Events that wake an active `janus.wait` call to look at the board again, so the lead never
/// polls in a sleep loop: those that end it (`ends_board_wait`), and a task dropped by a cancel,
/// which may leave nothing open on the board.
pub(super) fn wakes_board_wait(e: &Event) -> bool {
    ends_board_wait(e) || e.kind() == EventKind::TaskSkippedByDecision
}

/// Board events that end an active `janus.wait` at once: what the lead has to act on
/// (`needs_lead`), a finished task (what comes next may depend on it) and the Run being
/// cancelled. Entries that only inform (a start, progress, a note, a claim, a failed attempt
/// that is retried, a wait on the person, the lead's own answer or cancel) are handed over with
/// the next of these, or when the wait times out, so they cost the lead no turn of their own.
// @project-doc docs/interfaces/20261031-board_mcp.md#wait
pub(super) fn ends_board_wait(e: &Event) -> bool {
    needs_lead(e) || matches!(e.kind(), EventKind::TaskSucceeded | EventKind::RunCancelled)
}

/// Board events the lead has to act on, pushed into its session as they come (`push_board`).
pub(super) fn needs_lead(e: &Event) -> bool {
    let d = e.data.as_ref();
    let field = |k: &str| d.map(|d| d[k].clone()).unwrap_or_default();
    match e.kind() {
        EventKind::QuestionAsked
        | EventKind::TaskFailed
        | EventKind::TaskBlocked
        | EventKind::TaskConflict
        | EventKind::TaskSplitRequested
        | EventKind::LeadEdited
        | EventKind::LeadRouted => true,
        EventKind::TaskStalled => field("resolved") != true,
        EventKind::BoardPosted => field("kind") == "blocker",
        // A result that disagrees with the workspace, or an independent verification that rejected/questioned the target.
        EventKind::TaskSucceeded => {
            field("traceGaps").as_array().is_some_and(|g| !g.is_empty())
                || matches!(field("verdict").as_str(), Some("rejected" | "inconclusive"))
        }
        // The person's answer, to the lead's own question or to a collaborator's; not the lead's.
        EventKind::QuestionAnswered => field("via") != "lead",
        _ => false,
    }
}

/// The first few items of a list, each clipped, and whether anything was left out.
pub(super) fn inline_list(items: &[String]) -> (String, bool) {
    let shown: Vec<String> = items.iter().take(INLINE_ITEMS).map(|x| clip(x, INLINE_ITEM)).collect();
    let clipped =
        items.len() > INLINE_ITEMS || items.iter().take(INLINE_ITEMS).any(|x| x.chars().count() > INLINE_ITEM);
    let mut text = shown.join("；");
    if items.len() > INLINE_ITEMS {
        text.push_str(&format!("；等共 {} 条", items.len()));
    }
    (text, clipped)
}

/// The person's step into a collaborator's session (a message, a takeover, handing it back, an
/// interrupt or ending the subtask with the result so far) as the lead reads it; `None` for
/// anything else, and for the lead's own session.
fn user_intervention(run: &Run, e: &Event, d: &Value) -> Option<String> {
    let task = e.task.as_deref();
    let agent = match (e.to.as_deref(), task) {
        (Some(to), _) if to != "lead" => to.to_string(),
        (None, Some(t)) => run.st.lock().unwrap().tasks.iter().find(|x| x.def.id == t)?.agent.clone(),
        _ => return None,
    };
    let name = run.member(&agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.clone());
    let what = match e.kind() {
        EventKind::UserMessage => {
            format!("对 {name} 说：{}", clip(d["text"].as_str().unwrap_or(&e.text), INLINE_SUMMARY))
        }
        EventKind::ControlTaken => format!("接管了 {name} 的会话"),
        EventKind::ControlReleased => format!("把会话交还给 {name}，它继续自主完成"),
        EventKind::SessionInterrupted if d["finish"] == true => format!("让 {name} 以当前结果结束这个子任务"),
        EventKind::SessionInterrupted => format!("打断了 {name} 这一轮，转为人工接管"),
        _ => return None,
    };
    let at = match task {
        Some(t) => {
            let title = run.st.lock().unwrap().tasks.iter().find(|x| x.def.id == t).map(|x| x.def.title.clone());
            format!("{t}「{}」· ", title.unwrap_or_default())
        }
        None => String::new(),
    };
    Some(format!(
        "- [用户介入] {at}用户{what}。以用户的意思为准；需要调整计划时再处理，不要用 janus.reply 或 janus.retry 把它改回去。"
    ))
}

/// One board event as the lead reads it in `wait`, or nothing if it is not for the lead.
// @project-doc docs/interfaces/20261031-board_mcp.md#wait
pub(super) fn board_entry(run: &Run, e: &Event) -> Option<String> {
    let d = e.data.clone().unwrap_or_default();
    // The person's answer to a decision the lead handed them (`ask_user`): addressed to the lead,
    // about no task.
    if e.kind() == EventKind::QuestionAnswered && e.task.is_none() && e.to.as_deref() == Some("lead") {
        let note = d["note"].as_str().filter(|n| !n.is_empty()).map(|n| format!("（补充：{n}）")).unwrap_or_default();
        return Some(format!(
            "- [用户决定] 你交给用户的问题，用户的回答：{}{note}",
            d["answer"].as_str().unwrap_or("")
        ));
    }
    // The person stepping into a collaborator's session: the lead runs the board, so it hears of
    // it, with its next wake-up (this does not wake it by itself).
    // @project-doc docs/domains/20261031-goal_mode.md#board_flow
    if e.actor == "user"
        && let Some(entry) = user_intervention(run, e, &d)
    {
        return Some(entry);
    }
    // The lead's own edit after it handed out subtasks: from then on it only manages.
    if e.kind() == EventKind::LeadEdited {
        let paths: Vec<&str> = d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect();
        return Some(format!(
            "- [负责人改了文件] 你已经派出了子任务，按约定只做管理、不再改工作目录，这次改了：{}。这些改动没有经过子任务的验收，交付时会单独列出，你的报告要写明怎么核对的。需要改代码时交给对应的子任务：janus.retry 让它返工，或 janus.orchestrate 派新的子任务。",
            paths.join("、")
        ));
    }
    // The lead's own call to a library tool after it handed out subtasks: noted, never blocked.
    if e.kind() == EventKind::LeadRouted {
        return Some(format!(
            "- [负责人用了资源库工具] 你已经派出了子任务，按约定资源库的工具只用来查看；这次调用了 {}。如果它改了文件、数据或对外发了东西，这些没有经过子任务的验收，会写进交付风险。有副作用的操作交给对应的子任务。",
            d["tool"].as_str().unwrap_or_default()
        ));
    }
    let task = e.task.as_deref()?;
    let who = |agent: &str| run.member(agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.to_string());
    let (agent, title, verifies) = {
        let st = run.st.lock().unwrap();
        let t = st.tasks.iter().find(|t| t.def.id == task)?;
        (t.agent.clone(), t.def.title.clone(), t.def.verifies.clone())
    };
    let name = who(&agent);
    Some(match e.kind() {
        EventKind::AttemptStarted => format!("- [开始] {task}「{title}」· {name} 第 {} 次尝试", e.attempt.unwrap_or(1)),
        // Inlined up to a budget: enough to decide what to do next; the full result stays one
        // `task_detail` call away, so a long run does not fill the lead's context.
        EventKind::TaskSucceeded => {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == task)?;
            let r = t.result.clone().unwrap_or_default();
            let arts: Vec<String> = t.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
            let checks: Vec<String> = r.checks.iter().map(|c| clip(&c.line(), 60 + INLINE_ITEM)).collect();
            let (evidence, more_evidence) = inline_list(&r.evidence);
            let (risks, more_risks) = inline_list(&r.risks);
            let totals = Summary::of(&t.def.accept, &r.checks);
            let (reminders, more_reminders) = inline_list(&totals.reminders);
            // Who checked this task independently, once they are done: their verdict is separate
            // from the executor's own evidence.
            let verifiers: Vec<String> = st
                .tasks
                .iter()
                .filter(|v| v.def.verifies.as_deref() == Some(task) && v.status().terminal())
                .map(|v| match v.verdict() {
                    Some(vd) => format!("{} 判定{}", v.def.id, vd.label()),
                    None => format!("{} {}", v.def.id, v.status().label()),
                })
                .collect();
            let clipped = more_evidence
                || more_risks
                || more_reminders
                || r.summary.chars().count() > INLINE_SUMMARY
                || r.checks.iter().any(|c| c.line().chars().count() > 60 + INLINE_ITEM);
            let gaps: Vec<&str> = d["traceGaps"].as_array().into_iter().flatten().filter_map(|g| g.as_str()).collect();
            let verdict = t.verdict();
            let mut msg = match (&verifies, verdict) {
                (Some(target), Some(vd @ (Verdict::Rejected | Verdict::Inconclusive))) => format!(
                    "- [验证未通过] {task}「{title}」· {name} 完成对 {target} 的独立验证，判定「{}」（{}）：{}",
                    vd.label(),
                    vd.as_str(),
                    clip(&r.summary, INLINE_SUMMARY)
                ),
                (Some(target), Some(Verdict::Approved)) => format!(
                    "- [完成] {task}「{title}」· {name}（独立验证 {target}：通过）：{}",
                    clip(&r.summary, INLINE_SUMMARY)
                ),
                _ => format!("- [完成] {task}「{title}」· {name}：{}", clip(&r.summary, INLINE_SUMMARY)),
            };
            msg.push_str(&format!("\n  验收标准：{} 条，通过 {} 条", totals.criteria, totals.passed));
            msg.push_str(&format!("\n  证据：{}", totals.counts()));
            if !reminders.is_empty() {
                msg.push_str(&format!("\n  提醒（不影响是否通过）：{reminders}"));
            }
            if !checks.is_empty() {
                msg.push_str(&format!("\n  逐条验收：{}", checks.join("；")));
            }
            if !evidence.is_empty() {
                msg.push_str(&format!("\n  证据：{evidence}"));
            }
            if !verifiers.is_empty() {
                msg.push_str(&format!("\n  独立验证：{}", verifiers.join("、")));
            }
            if !risks.is_empty() {
                msg.push_str(&format!("\n  风险：{risks}"));
            }
            if !arts.is_empty() {
                msg.push_str(&format!("\n  产物：{}", arts.join("、")));
            }
            if let (Some(target), Some(Verdict::Rejected | Verdict::Inconclusive)) = (&verifies, verdict) {
                msg.push_str(&format!(
                    "\n  请处理：不能用 janus.accept 放行。先判断问题出在哪个子任务，用 janus.retry 返工（例如 task=\"{target}\"、from=\"{task}\"，session 选 continue 回到原会话按这份意见修改，或选 new 重新开始），返工完成后 {task} 自动重新验证；需要换人或另开一项工作时用 janus.orchestrate。"
                ));
                let downstream: Vec<_> = st
                    .tasks
                    .iter()
                    .filter(|d| {
                        matches!(d.status(), TaskStatus::Pending | TaskStatus::Blocked)
                            && d.def.deps.iter().any(|dep| dep == task)
                            && d.planned_seq < e.seq
                    })
                    .map(|d| d.def.id.as_str())
                    .collect();
                if !downstream.is_empty() {
                    msg.push_str(&format!(
                        " 已有下游任务 {} 受此判定阻塞；可用 janus.cancel 取消其中不再需要的任务并写明理由。",
                        downstream.join("、")
                    ));
                }
            }
            if !gaps.is_empty() {
                msg.push_str(&format!("\n  痕迹核对（结果和工作目录对不上）：{}。以工作目录的现状为准核对；需要返工用 janus.retry，结论要更正用 janus.amend。", gaps.join("；")));
            }
            if clipped {
                msg.push_str(&format!("\n  （有删节，完整结果用 janus.task_detail 查看 {task}）"));
            }
            msg
        }
        EventKind::TaskConflict if d["byLead"] == true => format!(
            "- [冲突] 你（负责人）修改了正在进行的子任务 {task}「{title}」（{name}）负责或正在修改的文件：{}。子任务派出去后请避免直接改它的文件；需要调整请用 janus.reply 或先 janus.cancel。",
            d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("、")
        ),
        EventKind::TaskConflict if d["owned"] == true => format!(
            "- [越界] {task}「{title}」· {name} 改了 {} 负责的 {}，两个子任务可能同时在改。Janus 已提醒 {task} 不要再改：确认分工，用 janus.reply 告诉它怎样配合；确实要它改，就让负责的一方先完成，或者取消并重新编排。",
            d["other"].as_str().unwrap_or("?"),
            d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("、")
        ),
        EventKind::TaskClaimed => format!(
            "- [认领] {task} · {name} 开始改 {}，认领为它负责的文件，到子任务结束为止。",
            d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("、")
        ),
        EventKind::TaskConflict => {
            let files: Vec<String> =
                d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
            format!(
                "- [冲突] {task} 和 {} 同时在改 {}。它们直接在同一个目录里工作，可能覆盖彼此的改动：确认分工，用 janus.reply 告诉其中一方先停下或怎样配合；之后会改同一批文件的子任务用依赖串行。",
                d["other"].as_str().unwrap_or("?"),
                files.join("、")
            )
        }
        EventKind::AttemptFailed => format!(
            "- [尝试失败] {task}「{title}」· {name} 第 {} 次：{}",
            e.attempt.unwrap_or(1),
            d["error"].as_str().unwrap_or(&e.text)
        ),
        EventKind::TaskFailed if d["reason"] == "verification" && verifies.is_some() => format!(
            "- [验证没通过] {task} · {name} 独立验证 {} 时发现问题：\n{}\n  不能用 janus.accept 放行：用 janus.retry 让 {task} 重新验证，或按它发现的问题用 janus.retry 让 {target} 返工，或者 janus.cancel 取消 {task} 并写明理由。",
            verifies.as_deref().unwrap_or_default(),
            verification_output(run, task),
            target = verifies.as_deref().unwrap_or_default(),
        ),
        EventKind::TaskFailed if d["reason"] == "verification" => format!(
            "- [没通过验收] {task}「{title}」· {name} 补过一次仍没有满足验收标准：\n{}\n  请核对后判断：结果可以接受 → janus.accept；需要返工或标准要调整 → janus.retry（可改 instructions / accept）；不再需要 → janus.cancel。",
            verification_output(run, task)
        ),
        EventKind::TaskFailed => format!(
            "- [失败] {task}「{title}」· {name} 用完了尝试次数：{}。请决定：janus.retry 重做（选原会话或新会话，可改说明）；交给别人做时 janus.cancel 后用 janus.orchestrate 重新派；团队里没人能做时 janus.ask_user；不做了就 janus.cancel 并在交付中说明。你已经派出了子任务，不要自己接手。",
            d["error"].as_str().unwrap_or(&e.text)
        ),
        EventKind::TaskBlocked => match d["verdict"].as_str() {
            Some(vd) => format!(
                "- [无法开始] {task}「{title}」：依赖的独立验证 {} 判定 {vd}，被验证的工作没有得到确认。按 {0} 的结论用 janus.retry 返工（from=\"{0}\"），{0} 复审通过后 {task} 自动开始；不再需要就用 janus.cancel 取消它。",
                d["dependency"].as_str().unwrap_or("?")
            ),
            None => format!(
                "- [无法开始] {task}「{title}」：依赖 {} 失败。用 janus.retry 或 janus.accept 处理 {0} 后会自动继续；不再需要就 janus.cancel。",
                d["dependency"].as_str().unwrap_or("?")
            ),
        },
        // A collaborator's question the person answered in its session: the lead must not answer again.
        EventKind::QuestionAnswered if d["via"] != "lead" => format!(
            "- [用户已回答] {task} 的提问由用户直接回答：{}。不要再回答这个问题，按用户的回答推进。",
            d["answer"].as_str().unwrap_or("")
        ),
        EventKind::BoardPosted => {
            let label = match d["kind"].as_str() {
                Some("blocker") => "阻塞",
                Some("note") => "备注",
                _ => "进展",
            };
            format!("- [{label}] {task} · {name}：{}", d["text"].as_str().unwrap_or(&e.text))
        }
        EventKind::TaskSplitRequested => {
            let items: Vec<String> = d["subtasks"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, s)| {
                    let owns = s["owns"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("、"))
                        .filter(|o| !o.is_empty())
                        .map(|o| format!("（owns: {o}）"))
                        .unwrap_or_default();
                    format!("{}. {}{owns}", i + 1, s["title"].as_str().unwrap_or("子任务"))
                })
                .collect();
            let completed = d["completed"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| format!("\n  已完成部分：{}", clip(s, INLINE_ITEM)))
                .unwrap_or_default();
            format!(
                "- [请求拆分] {task}「{title}」· {name} 请求拆分：{}{completed}\n  建议拆出：{}\n  请决定：采纳时用 janus.orchestrate 提交新子任务并用 janus.cancel / janus.amend 处理 {task}；不需要拆分时用 janus.reply 指示它继续完成。",
                clip(d["reason"].as_str().unwrap_or(&e.text), INLINE_ITEM),
                items.join("；")
            )
        }
        EventKind::QuestionAsked => {
            let options: Vec<String> = d["options"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            format!(
                "- [提问] {task} · {name} 停下来等你回答：{}{}\n  用 janus.reply 回答（task={task}）。",
                d["question"].as_str().unwrap_or(&e.text),
                if options.is_empty() { String::new() } else { format!("（选项：{}）", options.join(" / ")) }
            )
        }
        EventKind::ApprovalRequested | EventKind::InputRequested => {
            format!("- [等用户] {task} · {name} 在等用户处理：{}", e.text)
        }
        EventKind::TaskStalled if d["resolved"] == true => return None,
        EventKind::TaskStalled if d["reason"] == "loop" && d["period"].as_u64().unwrap_or(1) > 1 => format!(
            "- [停滞] {task}「{title}」· {name} 连续 {} 轮重复调用序列（{}），可能在交替死循环。请判断：用 janus.reply 指出问题或给个新思路；janus.retry 换个说明重做；janus.cancel 取消后交给别人。你已经派出了子任务，不要自己接手。",
            d["repeats"].as_i64().unwrap_or(4),
            clip(d["tool"].as_str().unwrap_or(""), 80)
        ),
        EventKind::TaskStalled if d["reason"] == "loop" => format!(
            "- [停滞] {task}「{title}」· {name} 连续 {} 次做同样的调用（{}），可能在原地打转。请判断：用 janus.reply 指出问题或给个新思路；janus.retry 换个说明重做；janus.cancel 取消后交给别人。你已经派出了子任务，不要自己接手。",
            d["repeats"].as_i64().unwrap_or(LOOP_REPEAT as i64),
            clip(d["tool"].as_str().unwrap_or(""), 60)
        ),
        EventKind::TaskStalled => format!(
            "- [停滞] {task}「{title}」· {name} 已有 {} 没有任何输出或工具调用。它可能在跑一条很久的命令，也可能卡住了。请判断：继续等；用 janus.reply 询问或补充指示；janus.retry 重做；janus.cancel 取消。",
            span(d["idleMs"].as_i64().unwrap_or(0))
        ),
        _ => return None,
    })
}
