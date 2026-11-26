//! What the board offers and says: the tool schemas, the orchestration check, and the board
//! messages the lead reads.

use super::*;
use crate::lang::{Lang, tr};

/// The board tools touch nothing but the board: no files, nothing outside this machine. Tools that
/// only read it or post to it are marked read-only so agents do not stop to ask the person about
/// them; the ones that start or steer work are left to the agent's own permission mode.
pub(super) fn board_read() -> Value {
    json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

pub(super) fn board_act() -> Value {
    json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

/// A tool that removes something for good.
pub(super) fn board_remove() -> Value {
    json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false })
}

/// What `ask_user` takes, from the lead and from a Loop alike.
pub(super) fn ask_user_schema(lang: Lang) -> Value {
    let t = |zh: &str, en: &str| lang.pick(zh, en).to_string();
    json!({
        "type": "object",
        "required": ["question", "why"],
        "properties": {
            "question": { "type": "string" },
            "why": { "type": "string", "description": t(
                "为什么必须由用户决定，不同答案会怎样影响结果",
                "Why the user must decide, and how the answers would change the result"
            ) },
            "options": { "type": "array", "items": { "type": "string" } },
            "recommend": { "type": "string", "description": t("你建议的选项", "The option you recommend") }
        }
    })
}

wire_enum! {
    /// A tool on the lead's board.
    pub(super) enum LeadTool {
        Board = "board",
        Orchestrate = "orchestrate",
        Wait = "wait",
        Reply = "reply",
        Retry = "retry",
        Amend = "amend",
        Cancel = "cancel",
        AskUser = "ask_user",
    }
}

impl LeadTool {
    /// What `tools/list` says about the tool, in the run's language; its name is the enum's. Each
    /// description carries how the tool is used and its limits: no brief explains the tools.
    // @project-doc docs/interfaces/20261031-board_mcp.md#lead_tools
    pub(super) fn definition(self, lang: Lang) -> Value {
        let t = |zh, en| lang.pick(zh, en);
        let mut d = match self {
            LeadTool::Board => json!({
                "description": t(
                    "查看共享看板。不传 task：协作者（名字和职责）和所有子任务的当前状态。传 task：这个子任务的全部信息：说明、验收标准、用量、完整结果（逐条验收、证据方法、全部证据、风险）、独立验证的结果、你的修订、改过的文件、产物、看板上的进展和问答，以及它的上游和下游子任务。janus.wait 里的完成消息只有摘要，需要细节时传 task。",
                    "See the shared board. Without task: the collaborators (names and responsibilities) and the current status of every subtask. With task: everything the board holds on that subtask: instructions, acceptance criteria, usage, the full result (each check, the evidence methods, all evidence, risks), independent verification results, your revisions, files changed, artifacts, progress and questions on the board, and its upstream and downstream subtasks. Completion messages in janus.wait carry only a summary; pass task for the details."
                ),
                "inputSchema": { "type": "object", "properties": { "task": { "type": "string" } } },
                "annotations": board_read()
            }),
            LeadTool::Orchestrate => json!({
                "description": t(
                    "提交一份编排：一组交给协作者的子任务。Janus 先校验整份编排（执行者是协作者、id 不重复、依赖存在且无环、可能同时执行的子任务负责的文件不重叠、验证者不是被验证子任务的执行者），不通过时返回全部错误，不执行任何子任务；通过后写入共享看板，按依赖执行：执行者不同、相互没有依赖的子任务同时执行，同一个协作者一次做一个子任务，其余排队。可以多次提交，后提交的子任务可以依赖之前提交的子任务。一次调用的内容太长会被模型的单次输出上限截断、整份作废：子任务多或说明长时分几次提交。",
                    "Submit an orchestration: a set of subtasks for collaborators. Janus checks the whole orchestration first (assignees are collaborators, ids are unique, dependencies exist and form no cycle, subtasks that may run at the same time own no overlapping files, a verifier is not the assignee of the subtask it verifies); if anything fails, every error comes back and no subtask starts. Otherwise the subtasks go on the shared board and run by dependency: subtasks with different assignees and no dependency between them run at the same time, and a collaborator works on one subtask at a time while the rest queue. You can submit more than once; later subtasks may depend on earlier ones. A call too long to fit in one model reply is cut off and lost whole: with many subtasks or long instructions, submit them over several calls."
                ),
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
                                    "id": { "type": "string", "description": t(
                                        "子任务 id：字母、数字、- 和 _，最长 32 个字符，在整个看板里唯一，例如 T1",
                                        "Subtask id: letters, digits, - and _, at most 32 characters, unique across the board, e.g. T1"
                                    ) },
                                    "title": { "type": "string" },
                                    "instructions": { "type": "string", "description": t(
                                        "给执行者的完整说明：背景、要做什么、注意什么。执行者看不到你的对话。工作目录里已有的资料写路径，不要整段抄进来。",
                                        "Complete instructions for the assignee: context, what to do, what to watch for. The assignee cannot see your conversation. Point to material already in the working directory by its path instead of copying it in."
                                    ) },
                                    "assignee": { "type": "string", "description": t("协作者 id", "Collaborator id") },
                                    "deps": { "type": "array", "items": { "type": "string" }, "description": t(
                                        "必须先成功完成的子任务 id",
                                        "Ids of subtasks that must succeed first"
                                    ) },
                                    "accept": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": t(
                                        "可核对的验收标准：做到什么、怎样算做到。执行者自己决定怎么验证，逐条给出是否达到、证据和可选的证据方法（test、command、inspection、reasoning、unverified）；没达到或漏掉的标准，Janus 让它在原会话里补一次",
                                        "Checkable acceptance criteria: the outcome and how to tell it is reached. The assignee chooses how to verify and reports, per criterion, whether it is met, the evidence and optionally the method (test, command, inspection, reasoning, unverified); for a criterion unmet or left out, Janus gives it one more go in the same session"
                                    ) },
                                    "owns": { "type": "array", "items": { "type": "string" }, "description": t(
                                        "这个子任务负责修改的文件或目录（相对工作目录，目录包含其下所有文件）。可能同时执行的子任务不能重叠",
                                        "Files or directories this subtask is responsible for changing (relative to the working directory; a directory covers every file below it). Subtasks that may run at the same time must not overlap"
                                    ) },
                                    "verifies": { "type": "string", "description": t(
                                        "独立验证：被验证子任务的 id。交给另一个协作者，在被验证的子任务完成后自动开始，只检查不改文件，给出判定 approved、rejected 或 inconclusive；判定保留到它重新验证为止",
                                        "Independent verification: the id of the subtask to verify. Give it to another collaborator; it starts automatically once that subtask finishes, checks it without changing files and returns a verdict, approved, rejected or inconclusive, which stands until it verifies again"
                                    ) },
                                    "continues": { "type": "string", "description": t(
                                        "续接原会话：同一个协作者之前某个子任务的 id，新工作要用到那个子任务的上下文时用（自动加入依赖；上下文超过 75% 或 Agent 不支持加载时改为新会话）。不填则新开会话。验证没通过的返工不用新建子任务，用 janus.retry",
                                        "Continue a session: the id of an earlier subtask of the same collaborator, for new work that needs that subtask's context (added to deps automatically; a new session instead when that context is over 75% or the agent cannot load sessions). Leave it out for a fresh session. Rework after a failed verification needs no new subtask: use janus.retry"
                                    ) }
                                }
                            }
                        }
                    }
                },
                "annotations": board_act()
            }),
            LeadTool::Wait => json!({
                "description": t(
                    "从共享看板取回发给你的新消息：子任务开始、完成（摘要、证据、风险、产物）、失败，协作者写回的进展、阻塞和拆分提议，协作者的提问和用户的回答。有子任务完成或需要你处理的消息时立即返回，连同之前攒下的开始、进展等消息；否则最多等 timeout_sec 秒（默认也是最大值 50），超时返回攒下的消息和运行中子任务的实时进展。Janus 已经推送进你会话的看板消息算作读过，不会再出现在这里。",
                    "Fetch the new board messages for you: subtasks started, finished (summary, evidence, risks, artifacts) or failed, progress, blockers and split proposals from collaborators, their questions and the user's answers. Returns as soon as a subtask finishes or something needs you, together with the starts, progress and other messages held so far; otherwise waits up to timeout_sec seconds (50 by default, which is also the most) and then returns what it holds plus the live progress of running subtasks. Board messages Janus already pushed into your session count as read and do not come back here."
                ),
                "inputSchema": { "type": "object", "properties": { "timeout_sec": { "type": "integer", "minimum": 1, "maximum": WAIT_MAX_SECS } } },
                "annotations": board_read()
            }),
            LeadTool::Reply => json!({
                "description": t(
                    "回答协作者在某个子任务里的提问，或给它补充指示。执行中的子任务立即收到：能插话时插入当前这一轮，否则打断这一轮后发送；还没开始的，在它开始工作时带给它；已经结束的返回错误。",
                    "Answer a collaborator's question in a subtask, or give it more instructions. A running subtask gets it at once: steered into the current turn where the agent allows, otherwise the turn is interrupted and the message sent; a subtask that has not started gets it when it starts; one that has ended returns an error."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["task", "text"],
                    "properties": { "task": { "type": "string" }, "text": { "type": "string" } }
                },
                "annotations": board_act()
            }),
            LeadTool::Retry => json!({
                "description": t(
                    "让一个已经结束的子任务再做一次，重新给满尝试次数：失败或已取消的，或者已完成但需要返工的（例如独立验证判定没通过）。每次都要选会话：continue 回到这个子任务原来的会话，上下文都在；new 新开一个会话。完成后，这个子任务的独立验证和 from 指向的验证重新验证新的结果。被它阻塞的子任务在它成功后自动继续，因它被取消的子任务一并恢复。",
                    "Run an ended subtask again with a full set of attempts: one that failed or was cancelled, or a finished one that needs rework (for example after a verification rejected it). Choose the session each time: continue goes back into the subtask's own session with all its context; new opens a fresh one. Once it is done, its verifications and the one named in from check the new result again. Subtasks blocked on it continue once it succeeds, and those cancelled because of it come back."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["task", "session"],
                    "properties": {
                        "task": { "type": "string" },
                        "session": { "type": "string", "enum": ["continue", "new"], "description": t(
                            "continue：接着原来的会话做（按意见修改、补漏、接着没做完的）；new：新开一个会话（方向要换、上下文已经乱了、原来的进程出错）。原会话上下文超过 75% 时 continue 会被拒绝，改用 new 并在 note 里写清这一轮要知道的上下文；Agent 不支持加载或加载失败时 continue 改为新开会话，看板上这次「开始」会说明",
                            "continue: carry on in the original session (revise per the findings, fill gaps, finish what was left); new: a fresh session (a different direction, a muddled context, a broken process). continue is refused when the original session's context is over 75%: use new and put what this round needs to know in note. When the agent cannot load the session, or loading fails, continue opens a new session and the board's \"Started\" entry says so"
                        ) },
                        "from": { "type": "string", "description": t(
                            "依据哪个独立验证子任务的意见返工；它的判定、没通过的标准和证据、建议原样交给执行者，不需要你转述，它在返工完成后重新验证",
                            "The independent verification whose findings to rework from; its verdict, failed criteria with evidence and suggestions go to the assignee verbatim, so you need not relay them, and it checks again once the rework is done"
                        ) },
                        "note": { "type": "string", "description": t(
                            "你对这一轮的补充：判断、取舍、范围上的决定",
                            "Your additions for this round: judgments, trade-offs, decisions on scope"
                        ) },
                        "instructions": { "type": "string", "description": t(
                            "新的完整说明；不填则沿用原说明",
                            "New complete instructions; left out, the current ones stay"
                        ) },
                        "accept": { "type": "array", "items": { "type": "string" }, "description": t(
                            "新的验收标准；不填则沿用",
                            "New acceptance criteria; left out, the current ones stay"
                        ) }
                    }
                },
                "annotations": board_act()
            }),
            LeadTool::Amend => json!({
                "description": t(
                    "修订一个子任务的结论，之后以修订为准。已完成的子任务：修订它的结论（例如推翻它的判断）；正在执行的下游子任务立即收到修订，还没开始的在交接时拿到，已经基于旧结论完成的下游子任务会列出来，由你决定是否重做。没通过验收核对的子任务（执行者自认没达到某条标准）：你核对后认为结果可以接受时，用它接受这个结果；子任务算作完成，没达到的部分记为风险，依赖它的子任务继续执行。独立验证子任务的判定和结论不能用它改：验证没通过时，用 janus.retry 让有问题的子任务返工，from 指向这个验证。",
                    "Revise a subtask's conclusion; the revision then stands. For a finished subtask, revise its conclusion (for example, overturn its judgment): running downstream subtasks get it at once, those not started get it at handoff, and downstream subtasks already finished on the old conclusion are listed for you to decide whether to redo them. For a subtask that failed its acceptance check (the assignee reported falling short of a criterion), use it to accept the result once you have checked it and find it acceptable: the subtask counts as done, the shortfall is recorded as a risk, and subtasks that depend on it go on. It cannot change an independent verification's verdict or conclusion: when a verification fails, rework the subtask at fault with janus.retry, from set to that verification."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["task", "text"],
                    "properties": { "task": { "type": "string" }, "text": { "type": "string", "description": t(
                        "修订后的结论，以及和原结论的区别；接受没达到标准的结果时，写你核对了什么、为什么没达到的部分不影响目标",
                        "The revised conclusion, and how it differs from the original; when accepting a result that fell short, what you checked and why the shortfall does not affect the goal"
                    ) } }
                },
                "annotations": board_act()
            }),
            LeadTool::Cancel => json!({
                "description": t(
                    "取消不再需要的子任务：还没开始的不再开始，执行中的被停止，依赖它们的子任务一并取消。已完成的子任务不受影响。",
                    "Cancel subtasks no longer needed: those not started will not start, running ones are stopped, and subtasks that depend on them are cancelled too. Finished subtasks are not affected."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["tasks"],
                    "properties": {
                        "tasks": { "type": "array", "items": { "type": "string" } },
                        "reason": { "type": "string", "description": t("取消的理由，记在看板上", "Why, recorded on the board") }
                    }
                },
                "annotations": board_act()
            }),
            LeadTool::AskUser => json!({
                "description": t(
                    "把只有用户能做的决定交给用户：需求有多种理解且结果明显不同、需要用户的账号或付费资源、不可逆的操作。目标范围内的做法和取舍由你自己决定，不要用这个工具。不会等待：用户的回答出现在 janus.wait 里，在那之前继续做不依赖这个决定的工作。",
                    "Hand the user a decision only they can make: the requirement reads several ways with clearly different results, it needs the user's account or paid resources, or the action cannot be undone. How to do the work and the trade-offs within the goal are yours to decide; do not use this tool for them. It does not wait: the answer arrives in janus.wait, and meanwhile you carry on with work that does not depend on it."
                ),
                "inputSchema": ask_user_schema(lang),
                "annotations": board_act()
            }),
        };
        d["name"] = json!(self.as_str());
        d
    }
}

/// What one collaborator call may put on the board: enough for any real report, not enough to
/// flood the database or the lead's context (AT-17). The tools' schemas state them too.
pub(super) const POST_MAX_CHARS: usize = 8000;
pub(super) const SPLIT_MAX_SUBTASKS: usize = 8;

wire_enum! {
    /// A tool a collaborator has on the board.
    pub(super) enum MemberTool {
        BoardPost = "board_post",
        Board = "board",
        Submit = "submit",
        Ask = "ask",
    }
}

impl MemberTool {
    /// What `tools/list` says about the tool, in the run's language; its name is the enum's. Each
    /// description carries how the tool is used and its limits: the collaborator's rules do not
    /// explain the tools.
    // @project-doc docs/interfaces/20261031-board_mcp.md#member_tools
    pub(super) fn definition(self, lang: Lang) -> Value {
        let t = |zh, en| lang.pick(zh, en);
        let mut d = match self {
            MemberTool::BoardPost => json!({
                "description": t(
                    "执行子任务期间，把进展、阻塞或需要负责人注意的事写到共享看板。kind=blocker 立即送到负责人那里；progress 和 note 随负责人下一次查看看板送达。需要负责人做决定才能继续时，不要在这里等：用 janus.ask 提问并结束这一轮。",
                    "While working on your subtask, write progress, a blocker or anything the lead should notice to the shared board. kind=blocker reaches the lead at once; progress and note reach it the next time it reads the board. When you need the lead's decision to go on, do not wait here: ask with janus.ask and end your turn."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "kind": { "type": "string", "enum": PostKind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>(), "description": t("默认 progress", "Defaults to progress") },
                        "text": { "type": "string", "maxLength": POST_MAX_CHARS, "description": t(
                            "写结论和负责人要知道的事；长内容写进文件，在这里给出路径",
                            "Conclusions and what the lead needs to know; put long content in a file and give its path here"
                        ) }
                    }
                },
                "annotations": board_read()
            }),
            MemberTool::Board => json!({
                "description": t(
                    "查看共享看板。不传 task：其他子任务的状态、负责的文件、和你的关系（你的上游、依赖你、和你改了同一批文件）、正在执行的子任务改过的文件、完成后又被别人改过的文件，以及最近写回看板的内容。传 task：这个子任务的完整说明、结果和负责人的修订，例如上游子任务的证据。",
                    "See the shared board. Without task: the other subtasks' status, owned files, how they relate to yours (your upstream, depends on you, changed the same files), files running subtasks changed, files changed by others after a subtask finished, and what was recently written to the board. With task: that subtask's full instructions, result and the lead's revisions, for example an upstream subtask's evidence."
                ),
                "inputSchema": { "type": "object", "properties": { "task": { "type": "string" } } },
                "annotations": board_read()
            }),
            MemberTool::Submit => json!({
                "description": t(
                    "交子任务的结果。checks 按验收标准的顺序逐条回报：是否达到、证据（你做了什么、看到了什么），以及可选的证据方法。Janus 当场核对：漏了标准、没有证据、方法不合法、验证子任务没给 verdict 时不收下，返回全部问题，改好再交；收下的结果里有标准写了没达到时，第一次会提醒你可以修好后再交一次，确实做不到就结束这一轮，交给负责人。收下后结束这一轮，子任务才算交了；同一轮里再交会替换上一份。",
                    "Hand in your subtask's result. In checks, report each acceptance criterion in order: whether it is met, the evidence (what you did and observed), and optionally the evidence method. Janus checks it at once: a result that leaves out a criterion, gives no evidence, names a method that does not exist, or (for a verification subtask) has no verdict is not taken, and every problem comes back for you to fix and hand in again. When a result it takes reports a criterion unmet, the first time you are told you may fix it and hand in once more; if it really cannot be met, end your turn and the lead takes it from there. Once a result is taken, end your turn to hand the subtask in; handing in again in the same turn replaces the earlier result."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["summary", "checks"],
                    "properties": {
                        "summary": { "type": "string", "description": t("一句话结果", "The result in one sentence") },
                        "checks": {
                            "type": "array",
                            "description": t("每条验收标准一项，按顺序", "One item per acceptance criterion, in order"),
                            "items": {
                                "type": "object",
                                "required": ["criterion", "passed", "evidence"],
                                "properties": {
                                    "criterion": { "type": "string", "description": t("原样复述的验收标准", "The acceptance criterion, verbatim") },
                                    "passed": { "type": "boolean", "description": t("是否达到", "Whether it is met") },
                                    "evidence": { "type": "string", "description": t(
                                        "你怎样核实的、看到了什么；没达到时写原因",
                                        "How you verified it and what you observed; for an unmet one, why"
                                    ) },
                                    "method": { "type": "string", "enum": ["test", "command", "inspection", "reasoning", "unverified"], "description": t(
                                        "证据方法：test（你运行的自动化测试）、command（命令或脚本）、inspection（检查代码、文件、界面或实际状态）、reasoning（推理或判断）、unverified（证据缺失、无法复现或现在无法核实）",
                                        "Evidence method: test (an automated test you ran), command (a command or script), inspection (code, files, UI, or actual state), reasoning (inference or judgment), unverified (evidence missing, irreproducible, or not checkable now)"
                                    ) }
                                }
                            }
                        },
                        "handoff": { "type": "string", "description": t(
                            "给下游子任务的接口、关键决定和注意事项",
                            "Interfaces, key decisions, and caveats for downstream subtasks"
                        ) },
                        "evidence": { "type": "array", "items": { "type": "string" }, "description": t("其他可核实的证据", "Other verifiable evidence") },
                        "risks": { "type": "array", "items": { "type": "string" }, "description": t("已知风险", "Known risks") },
                        "verdict": { "type": "string", "enum": ["approved", "rejected", "inconclusive"], "description": t(
                            "只有独立验证子任务填：approved（达标）、rejected（有阻断问题，要原执行者修改）或 inconclusive（这里的证据不足以判定）",
                            "Only for a verification subtask: approved (it meets the criteria), rejected (a blocking problem the assignee must fix) or inconclusive (the evidence here cannot settle it)"
                        ) }
                    }
                },
                "annotations": board_act()
            }),
            MemberTool::Ask => json!({
                "description": t(
                    "向负责人提问：需要超出你权限的决定才能继续时用。问题在这一轮结束时转给负责人；调用后结束这一轮，回答会作为下一条消息送来。子任务规模远超预期、需要拆成多个子任务或调整负责边界时，附上 split 拆分提议：负责人立即收到，由它决定是否调整编排。验证子任务不能提拆分。",
                    "Ask the lead a question when you need a decision beyond your authority to go on. The question goes to the lead when your turn ends; end your turn after calling this, and the answer comes as your next message. When your subtask turns out far larger than expected and should be split into several or have its ownership boundaries changed, add a split proposal: the lead gets it at once and decides whether to change the orchestration. Verification subtasks cannot propose a split."
                ),
                "inputSchema": {
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": { "type": "string", "description": t("问题", "The question") },
                        "options": { "type": "array", "items": { "type": "string" }, "description": t("可选的答案", "Possible answers") },
                        "recommend": { "type": "string", "description": t("你建议的选项", "The option you recommend") },
                        "split": {
                            "type": "object",
                            "description": t("拆分提议（可选）", "A split proposal (optional)"),
                            "required": ["reason", "subtasks"],
                            "properties": {
                                "reason": { "type": "string", "description": t(
                                    "为什么需要拆分或调整边界",
                                    "Why a split or a boundary change is needed"
                                ) },
                                "completed": { "type": "string", "description": t(
                                    "当前已经完成、可保留的前置部分（可选）",
                                    "The part already done that can be kept (optional)"
                                ) },
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
                        }
                    }
                },
                "annotations": board_act()
            }),
        };
        d["name"] = json!(self.as_str());
        d
    }
}

/// Check an orchestration the lead submitted, against the team and the tasks already on the
/// board. Every problem is reported at once, so one round of fixes is enough.
// @project-doc docs/interfaces/20261031-board_mcp.md#orchestration_validation
pub(super) fn validate_orchestration(
    run: &Run,
    board: &[(TaskDef, TaskStatus)],
    args: &Value,
) -> std::result::Result<Vec<TaskDef>, Vec<String>> {
    let lang = run.lang;
    let mut errors = vec![];
    let existing: Vec<String> = board.iter().map(|(d, _)| d.id.clone()).collect();
    let Some(items) = args["tasks"].as_array().filter(|a| !a.is_empty()) else {
        return Err(vec![tr!(lang, "tasks 必须是非空数组", "tasks must be a non-empty array")]);
    };
    let members: Vec<&str> = run.team.iter().filter(|m| !m.lead).map(|m| m.id.as_str()).collect();
    let mut ids: Vec<String> = vec![];
    let mut defs = vec![];
    for (i, t) in items.iter().enumerate() {
        let id = t["id"].as_str().unwrap_or("").trim().to_string();
        let n = i + 1;
        let label = if id.is_empty() {
            tr!(lang, "第 {n} 个子任务", "Subtask #{n}")
        } else {
            tr!(lang, "子任务 {id}", "Subtask {id}")
        };
        if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            errors.push(tr!(
                lang,
                "{label}：id 只能用字母、数字、- 和 _，最长 32 个字符",
                "{label}: id may use only letters, digits, - and _, at most 32 characters"
            ));
        } else if existing.contains(&id) || ids.contains(&id) {
            errors.push(tr!(
                lang,
                "{label}：id 和看板上已有的子任务重复",
                "{label}: id repeats a subtask already on the board"
            ));
        }
        let title = t["title"].as_str().unwrap_or("").trim().to_string();
        let instructions = t["instructions"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            errors.push(tr!(lang, "{label}：缺少 title", "{label}: title is missing"));
        }
        if instructions.is_empty() {
            errors.push(tr!(lang, "{label}：缺少 instructions", "{label}: instructions are missing"));
        }
        let assignee = t["assignee"].as_str().unwrap_or("").trim().to_string();
        if !members.contains(&assignee.as_str()) {
            let choices = if members.is_empty() {
                tr!(lang, "（没有协作者）", "(no collaborators)")
            } else {
                members.join(list_sep(lang))
            };
            errors.push(tr!(
                lang,
                "{label}：执行者 `{assignee}` 不是协作者，可选：{choices}",
                "{label}: assignee `{assignee}` is not a collaborator; choose from: {choices}"
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
            errors.push(tr!(lang, "{label}：缺少 accept", "{label}: accept is missing"));
        }
        let owns = crate::teamwork::read_owns(&t["owns"], &label, &mut errors, lang);
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
        let id = &d.id;
        for dep in &d.deps {
            if dep == id {
                errors.push(tr!(lang, "子任务 {id}：不能依赖自己", "Subtask {id}: cannot depend on itself"));
            } else if !ids.contains(dep) && !existing.contains(dep) {
                errors.push(tr!(
                    lang,
                    "子任务 {id}：依赖 {dep} 不存在",
                    "Subtask {id}: dependency {dep} does not exist"
                ));
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
        errors.push(tr!(lang, "依赖之间有环", "The dependencies form a cycle"));
    }
    if errors.is_empty() {
        crate::teamwork::check(&mut defs, board, &mut errors, lang);
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
/// cancelled. Entries that only inform (a start, progress, a note, a failed attempt
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
        EventKind::BoardPosted => d.map(PostKind::of) == Some(PostKind::Blocker),
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

/// What separates the names in a list (「、」 or ", ").
pub(super) fn list_sep(lang: Lang) -> &'static str {
    lang.pick("、", ", ")
}

/// What separates the items of a longer list (「；」 or "; ").
pub(super) fn item_sep(lang: Lang) -> &'static str {
    lang.pick("；", "; ")
}

/// A subtask as the board names it: `T1「标题」` or `T1 "title"`.
pub(super) fn titled(lang: Lang, task: &str, title: &str) -> String {
    tr!(lang, "{task}「{title}」", "{task} \"{title}\"")
}

wire_enum! {
    /// What a collaborator's `board_post` is (`board.posted` `data.kind`).
    #[derive(Default)]
    pub(super) enum PostKind {
        #[default]
        Progress = "progress",
        /// Something stops it: the lead hears of it like a question.
        Blocker = "blocker",
        Note = "note",
    }
}

impl PostKind {
    /// What a post's `data.kind` says, progress when it says nothing Janus knows.
    pub(super) fn of(data: &Value) -> PostKind {
        data["kind"].as_str().and_then(PostKind::parse).unwrap_or_default()
    }

    pub(super) fn label(self, lang: Lang) -> &'static str {
        match self {
            PostKind::Blocker => lang.pick("阻塞", "Blocker"),
            PostKind::Note => lang.pick("备注", "Note"),
            PostKind::Progress => lang.pick("进展", "Progress"),
        }
    }
}

wire_enum! {
    /// Why the watchdog found a session stalled (`task.stalled` and `lead.stalled` `data.reason`).
    pub(super) enum StallReason {
        /// Nothing happened in it for a while.
        Idle = "idle",
        /// It made the same call, or the same sequence of calls, again and again.
        Loop = "loop",
    }
}

/// The paths of a board event (`data.paths`), as one list.
fn paths(d: &Value, lang: Lang) -> String {
    d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect::<Vec<_>>().join(list_sep(lang))
}

/// The first few items of a list, each clipped, and whether anything was left out.
pub(super) fn inline_list(items: &[String], lang: Lang) -> (String, bool) {
    let shown: Vec<String> = items.iter().take(INLINE_ITEMS).map(|x| clip(x, INLINE_ITEM)).collect();
    let clipped =
        items.len() > INLINE_ITEMS || items.iter().take(INLINE_ITEMS).any(|x| x.chars().count() > INLINE_ITEM);
    let mut text = shown.join(item_sep(lang));
    if items.len() > INLINE_ITEMS {
        let n = items.len();
        text.push_str(&tr!(lang, "；等共 {n} 条", "; {n} in all"));
    }
    (text, clipped)
}

/// The person's step into a collaborator's session (a message, a takeover, handing it back, an
/// interrupt or ending the subtask with the result so far) as the lead reads it; `None` for
/// anything else, and for the lead's own session.
fn user_intervention(run: &Run, e: &Event, d: &Value) -> Option<String> {
    let lang = run.lang;
    let task = e.task.as_deref();
    let agent = match (e.to.as_deref(), task) {
        (Some(to), _) if to != "lead" => to.to_string(),
        (None, Some(t)) => run.st.lock().unwrap().tasks.iter().find(|x| x.def.id == t)?.agent.clone(),
        _ => return None,
    };
    let name = run.member(&agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.clone());
    let what = match e.kind() {
        EventKind::UserMessage => {
            let text = clip(d["text"].as_str().unwrap_or(&e.text), INLINE_SUMMARY);
            tr!(lang, "对 {name} 说：{text}", "said to {name}: {text}")
        }
        EventKind::ControlTaken => tr!(lang, "接管了 {name} 的会话", "took over {name}'s session"),
        EventKind::ControlReleased => tr!(
            lang,
            "把会话交还给 {name}，它继续自主完成",
            "handed the session back to {name}, which carries on by itself"
        ),
        EventKind::SessionInterrupted if d["finish"] == true => {
            tr!(lang, "让 {name} 以当前结果结束这个子任务", "had {name} end this subtask with its current result")
        }
        EventKind::SessionInterrupted => {
            tr!(lang, "打断了 {name} 这一轮，转为人工接管", "interrupted {name}'s turn and took over by hand")
        }
        _ => return None,
    };
    let at = match task {
        Some(t) => {
            let title = run.st.lock().unwrap().tasks.iter().find(|x| x.def.id == t).map(|x| x.def.title.clone());
            let it = titled(lang, t, &title.unwrap_or_default());
            tr!(lang, "{it}· ", "{it} · ")
        }
        None => String::new(),
    };
    Some(tr!(
        lang,
        "- [用户介入] {at}用户{what}。以用户的意思为准，不要改回去。",
        "- [User stepped in] {at}The user {what}. The user's intent stands; do not undo it."
    ))
}

/// One board event as the lead reads it in `wait`, or nothing if it is not for the lead. Every
/// entry has one shape: `- [tag] task · who: fact`, then indented detail lines. Entries state
/// facts; what the lead does about them is in its brief and the tools' descriptions.
// @project-doc docs/interfaces/20261031-board_mcp.md#wait
pub(super) fn board_entry(run: &Run, e: &Event) -> Option<String> {
    let lang = run.lang;
    let d = e.data.clone().unwrap_or_default();
    // The person's answer to a decision the lead handed them (`ask_user`): addressed to the lead,
    // about no task.
    if e.kind() == EventKind::QuestionAnswered && e.task.is_none() && e.to.as_deref() == Some("lead") {
        let note = d["note"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(|n| tr!(lang, "（补充：{n}）", " (note: {n})"))
            .unwrap_or_default();
        let answer = d["answer"].as_str().unwrap_or("");
        return Some(tr!(
            lang,
            "- [用户决定] 用户回答了你交出的问题：{answer}{note}",
            "- [User decision] The user's answer to the question you handed them: {answer}{note}"
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
        let paths = paths(&d, lang);
        return Some(tr!(
            lang,
            "- [负责人改了文件] 派出子任务后你改了 {paths}，这些改动没有经过任何子任务的验收。",
            "- [Lead changed files] After handing out subtasks you changed {paths}; no subtask's acceptance covered these changes."
        ));
    }
    // The lead's own call to a library tool after it handed out subtasks: noted, never blocked.
    if e.kind() == EventKind::LeadRouted {
        let tool = d["tool"].as_str().unwrap_or_default();
        return Some(tr!(
            lang,
            "- [负责人用了资源库工具] 派出子任务后你调用了 {tool}，这次调用会列为交付风险。",
            "- [Lead used a library tool] After handing out subtasks you called {tool}; the call is listed as a delivery risk."
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
    let it = titled(lang, task, &title);
    let sep = list_sep(lang);
    Some(match e.kind() {
        EventKind::AttemptStarted => {
            let n = e.attempt.unwrap_or(1);
            let mut line = tr!(lang, "- [开始] {it}· {name}，第 {n} 次尝试", "- [Started] {it} · {name}, attempt {n}");
            // Asked to continue an earlier session, it could not: the lead learns it starts afresh.
            if let Some(why) = d["continuation"]["fallback"].as_str().and_then(Fallback::parse).map(|f| f.reason(lang))
            {
                let from = d["continuation"]["from"].as_str().unwrap_or(task);
                line.push_str(&tr!(
                    lang,
                    "\n  没能回到 {from} 原来的会话（{why}），新会话不记得之前的工作；它需要的上下文用 janus.reply 补给它。",
                    "\n  It could not go back into {from}'s session ({why}); the new session remembers nothing of the earlier work. Give it the context it needs with janus.reply."
                ));
            }
            line
        }
        // Inlined up to a budget: enough to decide what to do next; the full result stays one
        // `janus.board` call away, so a long run does not fill the lead's context.
        EventKind::TaskSucceeded => {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == task)?;
            let r = t.result.clone().unwrap_or_default();
            let arts: Vec<String> = t.artifacts.iter().map(|a| a.path.clone()).collect();
            let checks: Vec<String> = r.checks.iter().map(|c| clip(&c.line(lang), 60 + INLINE_ITEM)).collect();
            let (evidence, more_evidence) = inline_list(&r.evidence, lang);
            let (risks, more_risks) = inline_list(&r.risks, lang);
            let totals = Summary::of_in(&t.def.accept, &r.checks, lang);
            let (reminders, more_reminders) = inline_list(&totals.reminders, lang);
            // Who checked this task independently, once they are done: their verdict is separate
            // from the executor's own evidence.
            let verifiers: Vec<String> = st
                .tasks
                .iter()
                .filter(|v| v.def.verifies.as_deref() == Some(task) && v.status().terminal())
                .map(|v| {
                    let id = &v.def.id;
                    match v.verdict() {
                        Some(vd) => {
                            let label = vd.label_in(lang);
                            tr!(lang, "{id} 判定「{label}」", "{id} verdict \"{label}\"")
                        }
                        None => format!("{id} {}", v.status().label_in(lang)),
                    }
                })
                .collect();
            let clipped = more_evidence
                || more_risks
                || more_reminders
                || r.summary.chars().count() > INLINE_SUMMARY
                || r.checks.iter().any(|c| c.line(lang).chars().count() > 60 + INLINE_ITEM);
            let gaps: Vec<&str> = d["traceGaps"].as_array().into_iter().flatten().filter_map(|g| g.as_str()).collect();
            let verdict = t.verdict();
            let summary = clip(&r.summary, INLINE_SUMMARY);
            let mut msg = match (&verifies, verdict) {
                (Some(target), Some(vd)) => {
                    let label = vd.label_in(lang);
                    let tag = match vd {
                        Verdict::Approved => lang.pick("完成", "Done"),
                        Verdict::Rejected | Verdict::Inconclusive => lang.pick("验证未通过", "Verification not passed"),
                    };
                    tr!(
                        lang,
                        "- [{tag}] {it}· {name} 验证 {target}，判定「{label}」：{summary}",
                        "- [{tag}] {it} · {name} verified {target}, verdict \"{label}\": {summary}"
                    )
                }
                _ => tr!(lang, "- [完成] {it}· {name}：{summary}", "- [Done] {it} · {name}: {summary}"),
            };
            if let Some(ms) = d["durationMs"].as_i64() {
                let context = d["contextRatio"]
                    .as_f64()
                    .map(|r| {
                        let pct = format!("{:.0}%", r * 100.0);
                        tr!(lang, " · 会话上下文 {pct}", " · session context {pct}")
                    })
                    .unwrap_or_default();
                let took = crate::text::duration(ms, lang);
                msg.push_str(&tr!(lang, "\n  用时：{took}{context}", "\n  Time: {took}{context}"));
            }
            let (criteria, passed) = (totals.criteria, totals.passed);
            msg.push_str(&tr!(
                lang,
                "\n  验收标准：{criteria} 条，通过 {passed} 条",
                "\n  Acceptance criteria: {criteria}, {passed} passed"
            ));
            let counts = totals.counts(lang);
            msg.push_str(&tr!(lang, "\n  证据方法：{counts}", "\n  Evidence methods: {counts}"));
            if !reminders.is_empty() {
                msg.push_str(&tr!(
                    lang,
                    "\n  提醒（不影响是否通过）：{reminders}",
                    "\n  Reminders (they do not affect passing): {reminders}"
                ));
            }
            if !checks.is_empty() {
                let checks = checks.join(item_sep(lang));
                msg.push_str(&tr!(lang, "\n  逐条验收：{checks}", "\n  Checks: {checks}"));
            }
            if !evidence.is_empty() {
                msg.push_str(&tr!(lang, "\n  其他证据：{evidence}", "\n  Other evidence: {evidence}"));
            }
            if !verifiers.is_empty() {
                let verifiers = verifiers.join(sep);
                msg.push_str(&tr!(lang, "\n  独立验证：{verifiers}", "\n  Independent verification: {verifiers}"));
            }
            // A verification says whether it checked the files the verified task handed in.
            if let Some(target) = &verifies
                && let Some(now) = &r.files
                && let Some(earlier) =
                    st.tasks.iter().find(|x| &x.def.id == target).and_then(|x| x.result.as_ref()?.files.as_ref())
            {
                msg.push_str(&format!("\n  {}", crate::teamwork::FilesSince::of(earlier, now).for_lead(target, lang)));
            }
            if !risks.is_empty() {
                msg.push_str(&tr!(lang, "\n  风险：{risks}", "\n  Risks: {risks}"));
            }
            if !arts.is_empty() {
                let arts = arts.join(sep);
                msg.push_str(&tr!(lang, "\n  产物：{arts}", "\n  Artifacts: {arts}"));
            }
            if verifies.is_some() && matches!(verdict, Some(Verdict::Rejected | Verdict::Inconclusive)) {
                msg.push_str(&tr!(
                    lang,
                    "\n  返工用 janus.retry 发回有问题的子任务，from 填 {task}。",
                    "\n  To rework, send the subtask at fault back with janus.retry, from set to {task}."
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
                    let downstream = downstream.join(sep);
                    msg.push_str(&tr!(
                        lang,
                        "\n  受这个判定阻塞的下游：{downstream}",
                        "\n  Downstream blocked by this verdict: {downstream}"
                    ));
                }
            }
            if !gaps.is_empty() {
                let gaps = gaps.join(item_sep(lang));
                msg.push_str(&tr!(
                    lang,
                    "\n  痕迹核对（结果和工作目录对不上）：{gaps}",
                    "\n  Trace check (the result and the working directory disagree): {gaps}"
                ));
            }
            if clipped {
                msg.push_str(&tr!(
                    lang,
                    "\n  （有删节，完整结果见 janus.board {task}）",
                    "\n  (Clipped; janus.board {task} has the full result)"
                ));
            }
            msg
        }
        EventKind::TaskConflict if d["byLead"] == true => {
            let paths = paths(&d, lang);
            tr!(
                lang,
                "- [冲突] {it}· {name}：你改了它负责或正在改的 {paths}。",
                "- [Conflict] {it} · {name}: you changed {paths}, which it owns or is changing."
            )
        }
        EventKind::TaskConflict if d["owned"] == true => {
            let (other, paths) = (d["other"].as_str().unwrap_or("?"), paths(&d, lang));
            tr!(
                lang,
                "- [越界] {it}· {name} 改了 {other} 负责的 {paths}，两个子任务可能同时在改。确认分工后用 janus.reply 告诉它。",
                "- [Out of scope] {it} · {name} changed {paths}, which {other} owns; both subtasks may be changing them at once. Settle the split and tell it with janus.reply."
            )
        }
        EventKind::TaskConflict => {
            let (other, files) = (d["other"].as_str().unwrap_or("?"), paths(&d, lang));
            tr!(
                lang,
                "- [冲突] {it}· {name} 和 {other} 同时在改 {files}，可能覆盖彼此的改动。确认分工后用 janus.reply 告诉它们。",
                "- [Conflict] {it} · {name} and {other} are changing {files} at the same time and may overwrite each other. Settle the split and tell them with janus.reply."
            )
        }
        EventKind::AttemptFailed => {
            let (n, error) = (e.attempt.unwrap_or(1), d["error"].as_str().unwrap_or(&e.text));
            tr!(
                lang,
                "- [尝试失败] {it}· {name}，第 {n} 次尝试：{error}",
                "- [Attempt failed] {it} · {name}, attempt {n}: {error}"
            )
        }
        EventKind::TaskFailed if d["reason"] == "verification" => {
            let output = verification_output(run, task).lines().map(|l| format!("\n  {l}")).collect::<String>();
            tr!(
                lang,
                "- [没通过验收] {it}· {name} 交的结果有标准没达到：{output}",
                "- [Acceptance not met] {it} · {name} handed in a result with criteria unmet:{output}"
            )
        }
        EventKind::TaskFailed => {
            let error = d["error"].as_str().unwrap_or(&e.text);
            tr!(
                lang,
                "- [失败] {it}· {name} 用完了尝试次数：{error}",
                "- [Failed] {it} · {name} used up its attempts: {error}"
            )
        }
        EventKind::TaskBlocked => {
            let dep = d["dependency"].as_str().unwrap_or("?");
            match d["verdict"].as_str().and_then(Verdict::parse) {
                Some(vd) => {
                    let label = vd.label_in(lang);
                    tr!(
                        lang,
                        "- [无法开始] {it}· {name}：它依赖的验证 {dep} 判定「{label}」，{dep} 复审通过后自动开始。",
                        "- [Cannot start] {it} · {name}: the verification {dep} it depends on returned \"{label}\"; it starts once {dep} approves on review."
                    )
                }
                None => tr!(
                    lang,
                    "- [无法开始] {it}· {name}：依赖 {dep} 失败，{dep} 处理好后自动继续。",
                    "- [Cannot start] {it} · {name}: its dependency {dep} failed; it continues once {dep} is settled."
                ),
            }
        }
        // A collaborator's question the person answered in its session: the lead must not answer again.
        EventKind::QuestionAnswered if d["via"] != "lead" => {
            let answer = d["answer"].as_str().unwrap_or("");
            tr!(
                lang,
                "- [用户已回答] {it}· {name} 的提问由用户直接回答了：{answer}。你不用再回答。",
                "- [User answered] {it} · The user answered {name}'s question directly: {answer}. You need not answer it."
            )
        }
        EventKind::BoardPosted => {
            let label = PostKind::of(&d).label(lang);
            let text = d["text"].as_str().unwrap_or(&e.text);
            tr!(lang, "- [{label}] {it}· {name}：{text}", "- [{label}] {it} · {name}: {text}")
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
                        .map(|a| a.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join(sep))
                        .filter(|o| !o.is_empty())
                        .map(|o| tr!(lang, "（负责 {o}）", " (owns {o})"))
                        .unwrap_or_default();
                    let title = s["title"].as_str().unwrap_or(lang.pick("子任务", "subtask"));
                    format!("{}. {title}{owns}", i + 1)
                })
                .collect();
            let completed = d["completed"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let s = clip(s, INLINE_ITEM);
                    tr!(lang, "\n  已完成部分：{s}", "\n  Done so far: {s}")
                })
                .unwrap_or_default();
            let (reason, items) =
                (clip(d["reason"].as_str().unwrap_or(&e.text), INLINE_ITEM), items.join(item_sep(lang)));
            tr!(
                lang,
                "- [请求拆分] {it}· {name}：{reason}{completed}\n  建议拆出：{items}",
                "- [Split requested] {it} · {name}: {reason}{completed}\n  Proposed subtasks: {items}"
            )
        }
        EventKind::QuestionAsked => {
            let options: Vec<String> = d["options"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let question = d["question"].as_str().unwrap_or(&e.text);
            let options = if options.is_empty() {
                String::new()
            } else {
                let options = options.join(" / ");
                tr!(lang, "（选项：{options}）", " (options: {options})")
            };
            let options = match d["recommend"].as_str() {
                Some(r) => options + &tr!(lang, "，建议：{r}", "; recommends: {r}"),
                None => options,
            };
            tr!(
                lang,
                "- [提问] {it}· {name} 等你回答：{question}{options}",
                "- [Question] {it} · {name} waits for your answer: {question}{options}"
            )
        }
        EventKind::ApprovalRequested | EventKind::InputRequested => {
            let text = &e.text;
            tr!(
                lang,
                "- [等用户] {it}· {name} 在等用户处理：{text}",
                "- [Waiting for the user] {it} · {name} is waiting for the user: {text}"
            )
        }
        EventKind::TaskStalled if d["resolved"] == true => return None,
        EventKind::TaskStalled
            if d["reason"] == StallReason::Loop.as_str() && d["period"].as_u64().unwrap_or(1) > 1 =>
        {
            let (repeats, tool) = (d["repeats"].as_i64().unwrap_or(4), clip(d["tool"].as_str().unwrap_or(""), 80));
            tr!(
                lang,
                "- [停滞] {it}· {name} 连续 {repeats} 轮重复同一组调用（{tool}），可能在来回打转。",
                "- [Stalled] {it} · {name} repeated the same sequence of calls {repeats} times in a row ({tool}); it may be going back and forth."
            )
        }
        EventKind::TaskStalled if d["reason"] == StallReason::Loop.as_str() => {
            let (repeats, tool) =
                (d["repeats"].as_i64().unwrap_or(LOOP_REPEAT as i64), clip(d["tool"].as_str().unwrap_or(""), 60));
            tr!(
                lang,
                "- [停滞] {it}· {name} 连续 {repeats} 次做同样的调用（{tool}），可能在原地打转。",
                "- [Stalled] {it} · {name} made the same call {repeats} times in a row ({tool}); it may be going in circles."
            )
        }
        EventKind::TaskStalled => {
            let idle = span(d["idleMs"].as_i64().unwrap_or(0), lang);
            tr!(
                lang,
                "- [停滞] {it}· {name} 已有 {idle} 没有输出或工具调用，可能在做耗时的操作，也可能卡住了。",
                "- [Stalled] {it} · {name} has shown no output or tool call for {idle}; it may be on a long operation, or stuck."
            )
        }
        _ => return None,
    })
}
