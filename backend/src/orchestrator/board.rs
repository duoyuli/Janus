//! The `janus` MCP server of a run's sessions: who calls it (`Caller`), what it tells each caller,
//! and how a tool call is recorded and dispatched. The tools themselves are in `lead_tools`,
//! `hand_in`, `board_views`, `loops`, `memory` and `chat`.

use super::*;
use crate::lang::{Lang, tr};

/// A tool named a subtask the board does not have.
pub(super) fn no_task(lang: Lang, task: &str) -> String {
    tr!(lang, "看板上没有子任务 {task}", "There is no subtask {task} on the board")
}

/// Who reaches the `janus` MCP server, from the run's kind and the session's `who`: the tools it
/// has. A chat has none, unless its digital employee proposes digital employees. The server's
/// `instructions` name the caller's tools, one line each (`Caller::instructions`); how each is
/// used is in its own description.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Caller {
    /// A goal run's lead: the board.
    Lead,
    /// A collaborator in one of a goal run's tasks.
    Member,
    /// A workspace Loop's session.
    // @project-doc docs/interfaces/20261031-board_mcp.md#loop_tools
    Loop,
    /// The lead consolidating the workspace's team memory: the memory tools alone.
    // @project-doc docs/interfaces/20261031-board_mcp.md#memory_tools
    Consolidation,
    /// A chat whose digital employee proposes digital employees (`Member::hires`): that tool
    /// alone. A chat belongs to no workspace, so it has no team memory.
    // @project-doc docs/interfaces/20261031-board_mcp.md#chat_tools
    Hiring,
}

impl Caller {
    /// The caller in a run of `kind` at `seat`; `hires` is whether the run's member proposes
    /// digital employees, which only a chat reads.
    pub(super) fn of(kind: RunKind, seat: &SessionKey, hires: bool) -> Option<Caller> {
        match (kind, seat) {
            (RunKind::Chat, _) => hires.then_some(Caller::Hiring),
            (RunKind::Loop, _) => Some(Caller::Loop),
            (RunKind::Dream, _) => Some(Caller::Consolidation),
            (RunKind::Goal, SessionKey::Lead) => Some(Caller::Lead),
            (RunKind::Goal, SessionKey::Task(_)) => Some(Caller::Member),
        }
    }

    /// The caller of one of `run`'s sessions.
    pub(super) fn of_run(run: &Run, seat: &SessionKey) -> Option<Caller> {
        Caller::of(run.kind, seat, run.team.first().is_some_and(|m| m.hires))
    }

    /// What the caller may do with the workspace's notes; nothing without a workspace.
    fn memory(self) -> Option<MemoryAccess> {
        match self {
            Caller::Hiring => None,
            Caller::Consolidation => Some(MemoryAccess::Consolidate),
            _ => Some(MemoryAccess::Write),
        }
    }

    /// Every tool the caller has (`tools/list`), described in the run's language: its own, then
    /// the memory tools it may use.
    pub(super) fn tools(self, lang: Lang) -> Vec<Value> {
        let own: Vec<Value> = match self {
            Caller::Lead => LeadTool::ALL.iter().map(|t| t.definition(lang)).collect(),
            Caller::Member => MemberTool::ALL.iter().map(|t| t.definition(lang)).collect(),
            Caller::Loop => LoopTool::ALL.iter().map(|t| t.definition(lang)).collect(),
            Caller::Hiring => ChatTool::ALL.iter().map(|t| t.definition(lang)).collect(),
            Caller::Consolidation => vec![],
        };
        let memory = MemoryTool::ALL
            .iter()
            .filter(|t| self.memory().is_some_and(|access| t.allowed(access)))
            .map(|t| t.definition(lang));
        own.into_iter().chain(memory).collect()
    }

    /// What `initialize` says of the server to this caller: its tools, one line each, saying only
    /// what each is for. Agents load MCP tools on demand (Claude Code starts with their names and
    /// each server's instructions, Codex with each server's namespace and these words), so this is
    /// how a session learns which tools it has; their usage and limits are in their descriptions.
    // @project-doc docs/interfaces/20261031-board_mcp.md#transport
    pub(super) fn instructions(self, lang: Lang) -> String {
        let lines: &[(&str, &str)] = match self {
            Caller::Lead => &[
                (
                    "orchestrate：把一组子任务派给协作者，写明执行者、依赖、验收标准，可选负责的文件、独立验证和续接会话。",
                    "orchestrate: hand a set of subtasks to collaborators, with assignee, dependencies, and acceptance criteria, and optionally owned files, independent verification, and a continued session.",
                ),
                (
                    "wait：读看板，等子任务完成或有需要你处理的消息。",
                    "wait: read the board, waiting for a subtask to finish or for a message that needs you.",
                ),
                (
                    "board：看团队和所有子任务的状态；传 task 看一个子任务的全部信息。",
                    "board: see the team and every subtask's status; with task, everything about one subtask.",
                ),
                (
                    "reply：回答协作者的提问，或给正在执行的子任务补充指示。",
                    "reply: answer a collaborator's question, or add instructions to a running subtask.",
                ),
                (
                    "retry、cancel：让结束的子任务返工；取消不再需要的子任务。",
                    "retry, cancel: send an ended subtask back for rework; cancel a subtask no longer needed.",
                ),
                (
                    "amend：修订已完成子任务的结论，让下游用修订后的；或接受没完全达标但可以接受的结果。",
                    "amend: correct a finished subtask's findings so downstream work uses the correction, or accept a result that falls short but is acceptable.",
                ),
                (
                    "ask_user：把只有用户能做的决定交给用户。",
                    "ask_user: hand a decision only the user can make to the user.",
                ),
            ],
            Caller::Member => &[
                ("submit：交子任务的结果。", "submit: hand in your subtask's result."),
                (
                    "ask：向负责人提问；子任务远超预期时，附上拆分提议。",
                    "ask: ask the lead a question; when the subtask is far bigger than expected, add a split proposal.",
                ),
                (
                    "board_post：在看板上写进展、阻塞或备注；阻塞会立即推给负责人。",
                    "board_post: post progress, a blocker, or a note on the board; a blocker reaches the lead at once.",
                ),
                (
                    "board：看其他子任务的状态、负责和改过的文件，以及某个子任务的完整结果。",
                    "board: see the other subtasks' status, owned and changed files, and any subtask's full result.",
                ),
            ],
            Caller::Loop => &[
                (
                    "dispatch：把一件事派成这个工作区的一个目标任务；同一件事的任务还在进行时，把新情况转告给它。",
                    "dispatch: start a goal run in this workspace for something; while the run for it is still at work, pass it the news instead.",
                ),
                (
                    "ask_user：把只有用户能做的决定交给用户。",
                    "ask_user: hand a decision only the person can make to them.",
                ),
                ("next_wake：在时间表之外约一次醒来。", "next_wake: wake once more outside your schedule."),
            ],
            Caller::Consolidation => &[],
            Caller::Hiring => &[(
                "propose_employees：把你为用户设计的一批数字员工交给用户，由用户挑选后创建。",
                "propose_employees: hand the person a set of digital employees you designed; they pick which to create.",
            )],
        };
        let head = tr!(
            lang,
            "Janus 的协作工具。每个工具的详细用法和限制写在它自己的说明里，用到时再看：",
            "Janus's collaboration tools. Each tool's own description has its full usage and limits; read it when you need the tool:"
        );
        let memory = self.memory().map_or(&[][..], MemoryAccess::lines);
        let lines: Vec<String> =
            lines.iter().chain(memory).map(|(zh, en)| format!("- {}", lang.pick(zh, en))).collect();
        format!("{head}\n{}", lines.join("\n"))
    }
}

impl App {
    /// The Janus MCP server one session reaches, with the tools its caller has (`Caller`).
    pub(super) fn mcp_server(&self, run: &Run, seat: &SessionKey) -> Value {
        self.session_server(run, seat, "janus", "")
    }

    /// One MCP request (streamable HTTP, JSON responses) from a session of this run. Its `seat`,
    /// with the run's kind, says which tools the session has (`Caller`). Notifications get no
    /// response.
    // @project-doc docs/interfaces/20261031-board_mcp.md#transport
    pub async fn mcp(
        self: &Arc<Self>,
        run_id: &str,
        token: &str,
        seat: &SessionKey,
        req: Value,
    ) -> Result<Option<Value>> {
        let run = self.session_run(run_id, token, seat)?;
        // A chat's agent holds a valid token for its library router; the same token must not
        // reach a board, which a chat does not have, nor a tool its digital employee lacks.
        // @project-doc docs/domains/20261120-chats.md#chat_model
        let caller = Caller::of_run(&run, seat).ok_or_else(|| anyhow!("这个对话没有 Janus 工具"))?;
        let Some(id) = req.get("id").cloned() else {
            return Ok(None);
        };
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => initialize(&req, "janus", &caller.instructions(run.lang)),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": caller.tools(run.lang) }),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                self.janus_tool_call(&run, seat, caller, name, &req["params"]["arguments"], &id.to_string()).await
            }
            m => return Ok(Some(rpc_error(id, METHOD_NOT_FOUND, tr!(run.lang, "不支持 {m}", "{m} is not supported")))),
        };
        Ok(Some(rpc_result(id, result)))
    }

    /// A call of one of the `janus` server's tools, recorded in the caller's session as it starts
    /// and ends (`tool.started`, `tool.completed`).
    async fn janus_tool_call(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        caller: Caller,
        name: &str,
        args: &Value,
        call_id: &str,
    ) -> Value {
        let who = seat.who();
        let tool = format!("janus.{}", if name.is_empty() { "工具" } else { name });
        let input = summarize_input(args, name);
        let started = ev(EventKind::ToolStarted, who, format!("调用 {tool}，等待返回")).data(json!({
            "callId": call_id, "tool": tool, "input": input, "args": args, "status": "pending"
        }));
        self.emit(run, started.in_session(seat));
        let result = match self.janus_tool(run, seat, caller, name, args).await {
            Ok(text) => tool_text(&text, false),
            Err(text) => tool_text(&text, true),
        };
        let failed = result["isError"] == true;
        let output = result["content"][0]["text"].as_str().unwrap_or("");
        let text = if failed { format!("{tool} 失败") } else { format!("{tool} 返回结果") };
        let completed = ev(EventKind::ToolCompleted, who, text).data(json!({
            "callId": call_id, "tool": tool, "input": input, "args": args, "output": clip(output, 8000),
            "status": if failed { "failed" } else { "completed" }
        }));
        self.emit(run, completed.in_session(seat));
        result
    }

    /// Run one of the caller's tools; its error is the text the agent reads.
    async fn janus_tool(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        caller: Caller,
        name: &str,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let missing = || Err(tr!(run.lang, "没有这个工具：{name}", "No such tool: {name}"));
        if let Some(tool) = MemoryTool::parse(name) {
            return match caller.memory().filter(|access| tool.allowed(*access)) {
                Some(access) => self.memory_tool(run, seat, access, tool, args),
                None => missing(),
            };
        }
        match caller {
            Caller::Consolidation => missing(),
            Caller::Hiring => match ChatTool::parse(name) {
                Some(ChatTool::ProposeEmployees) => self.propose_employees(run, args),
                None => missing(),
            },
            Caller::Loop => match LoopTool::parse(name) {
                Some(tool) => self.loop_tool(run, tool, args).await,
                None => missing(),
            },
            Caller::Lead => match LeadTool::parse(name) {
                Some(LeadTool::Board) => match args["task"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
                    Some(task) => self.task_detail(run, task),
                    None => Ok(self.board_team(run)),
                },
                Some(LeadTool::Orchestrate) => self.board_orchestrate(run, args),
                Some(LeadTool::Wait) => Ok(self.board_wait(run, args).await),
                Some(LeadTool::Reply) => self.board_reply(run, args),
                Some(LeadTool::Retry) => self.board_retry(run, args),
                Some(LeadTool::Amend) => self.board_amend(run, args),
                Some(LeadTool::Cancel) => self.board_cancel(run, args),
                Some(LeadTool::AskUser) => self.board_ask_user(run, args),
                None => missing(),
            },
            Caller::Member => {
                let SessionKey::Task(task) = seat else { return missing() };
                match MemberTool::parse(name) {
                    Some(MemberTool::BoardPost) => self.board_post(run, task, args),
                    Some(MemberTool::Board) => self.board_peers(run, task, args),
                    Some(MemberTool::Submit) => self.board_submit(run, task, args),
                    Some(MemberTool::Ask) => self.board_ask(run, task, args),
                    None => missing(),
                }
            }
        }
    }

    /// The run's usage ledger, from its events (rules live in `usage`). Read-only: a failure to
    /// read the log gives an empty ledger, never an error, so nothing a person or the lead does
    /// depends on usage being available.
    pub(super) fn usage(&self, run: &Run) -> crate::usage::Ledger {
        match self.db.events(&run.id, 0) {
            Ok(events) => crate::usage::ledger(&events),
            Err(err) => {
                crate::config::log(format!("janus: usage of {} unavailable: {err:#}", run.id));
                crate::usage::ledger(&[])
            }
        }
    }

    /// Agent ids to display names, for the usage text. Empty when the run is gone (history).
    pub(super) fn usage_names(run: &Run) -> crate::usage::Names {
        run.team.iter().map(|m| (m.id.clone(), m.name.clone())).collect()
    }
}
