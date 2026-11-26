//! The shared board as an MCP server: authentication, the lead's and collaborators' tool calls, and
//! role statistics.

use super::*;
use crate::lang::{Lang, tr};

/// A message the lead sends a collaborator, with a prefix so the session and the record both show
/// who wrote it.
fn lead_reply(lang: Lang, text: &str) -> String {
    tr!(lang, "负责人：{text}", "The lead: {text}")
}

/// A tool named a subtask the board does not have.
fn no_task(lang: Lang, task: &str) -> String {
    tr!(lang, "看板上没有子任务 {task}", "There is no subtask {task} on the board")
}

/// The tasks that took a finished task's result (they depend on it): the ones running now, with
/// their session channel, and the ones already finished on it. Changing that result, by
/// `amend` or by sending the task back with `retry`, leaves them on the old one.
struct Takers {
    running: Vec<(String, String, mpsc::UnboundedSender<Steer>)>,
    done: Vec<String>,
}

impl Takers {
    fn of(st: &RunState, task: &str) -> Takers {
        let downstream: Vec<&Task> = st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).collect();
        Takers {
            running: downstream
                .iter()
                .filter_map(|x| {
                    st.live.get(&SessionKey::task(&x.def.id)).map(|tx| (x.def.id.clone(), x.agent.clone(), tx.clone()))
                })
                .collect(),
            done: downstream.iter().filter(|x| x.status() == TaskStatus::Succeeded).map(|x| x.def.id.clone()).collect(),
        }
    }

    fn ids(running: &[(String, String, mpsc::UnboundedSender<Steer>)], lang: Lang) -> String {
        running.iter().map(|r| r.0.as_str()).collect::<Vec<_>>().join(list_sep(lang))
    }
}

/// The longest `wait` holds its call. Claude Code and Codex give an MCP tool call 60 seconds and
/// then fail it ("The operation timed out."), so a longer wait never returns: the lead gets an
/// error instead of what the board holds and the live progress of its subtasks.
// @project-doc docs/interfaces/20261031-board_mcp.md#wait
pub(super) const WAIT_MAX_SECS: u64 = 50;

/// How long one `wait` call may hold: what it asks for, at most `WAIT_MAX_SECS`.
pub(super) fn wait_limit(args: &Value) -> Duration {
    Duration::from_secs(args["timeout_sec"].as_u64().unwrap_or(WAIT_MAX_SECS).clamp(1, WAIT_MAX_SECS))
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
                (
                    "recall、remember：查团队记忆；记下之后的任务还用得上的事。",
                    "recall, remember: search the team memory; keep what later runs will need.",
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
                (
                    "recall、remember：查团队记忆；记下之后的任务还用得上的事。",
                    "recall, remember: search the team memory; keep what later runs will need.",
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
                (
                    "recall、remember：查团队记忆；记下之后还用得上的事。",
                    "recall, remember: search the team memory; keep what later runs will need.",
                ),
            ],
            Caller::Consolidation => &[
                ("recall：搜笔记，读全文。", "recall: search the notes and read them in full."),
                ("remember：写一条笔记，或改写已有的一条。", "remember: write a note, or rewrite one."),
                ("forget：删一条笔记。", "forget: remove a note."),
            ],
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
        let lines: Vec<String> = lines.iter().map(|(zh, en)| format!("- {}", lang.pick(zh, en))).collect();
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

    pub(super) fn board_team(&self, run: &Run) -> String {
        let lang = run.lang;
        let st = run.st.lock().unwrap();
        let tasks: Vec<String> = st
            .tasks
            .iter()
            .map(|t| {
                let (it, agent, status) = (titled(lang, &t.def.id, &t.def.title), &t.agent, t.status().label_in(lang));
                tr!(lang, "- {it}· {agent} · {status}", "- {it} · {agent} · {status}")
            })
            .collect();
        drop(st);
        let team = team_lines(run);
        let tasks = if tasks.is_empty() { lang.pick("（还没有）", "(none yet)").into() } else { tasks.join("\n") };
        tr!(
            lang,
            "协作者：\n{team}\n\n看板上的子任务：\n{tasks}",
            "Collaborators:\n{team}\n\nSubtasks on the board:\n{tasks}"
        )
    }

    /// Validate an orchestration and, if it holds, put its tasks on the board for the scheduler.
    pub(super) fn board_orchestrate(
        self: &Arc<Self>,
        run: &Arc<Run>,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let _one_at_a_time = run.orchestrating.lock().unwrap();
        let defs = {
            let st = run.st.lock().unwrap();
            let board: Vec<(TaskDef, TaskStatus)> = st.tasks.iter().map(|t| (t.def.clone(), t.status())).collect();
            validate_orchestration(run, &board, args)
        };
        let defs = match defs {
            Ok(defs) => defs,
            Err(errors) => {
                self.emit(
                    run,
                    ev(
                        EventKind::OrchestrationRejected,
                        "lead",
                        format!("编排没有通过校验：{}", clip(&errors.join("；"), 120)),
                    )
                    .data(json!({ "errors": errors, "submitted": args })),
                );
                let errors = errors.join("\n- ");
                return Err(tr!(
                    run.lang,
                    "编排没有通过校验，没有子任务开始：\n- {errors}",
                    "The orchestration failed its check and no subtask started:\n- {errors}"
                ));
            }
        };
        let tasks: Vec<Value> = defs
            .iter()
            .map(|d| {
                let mut obj = json!({
                    "id": d.id, "title": d.title, "desc": d.desc, "deps": d.deps,
                    "accept": d.accept, "maxAttempts": d.max_attempts, "agent": d.assignee,
                    "owns": d.owns, "verifies": d.verifies,
                });
                if let Some(c) = &d.continues {
                    obj["continues"] = json!(c);
                }
                obj
            })
            .collect();
        // The record comes first: tasks go on the board only once their orchestration is in the
        // log, which is what a restart rebuilds the board from (FL-08).
        // @project-doc docs/interfaces/20261031-board_mcp.md#orchestrate
        let planned = run.seq.load(Ordering::SeqCst);
        let n = run.st.lock().unwrap().tasks.len() + defs.len();
        let kept = self.emit_kept(
            run,
            ev(
                EventKind::OrchestrationAccepted,
                "lead",
                format!("编排通过校验：{} 个子任务写入看板，开始执行", defs.len()),
            )
            .data(json!({ "tasks": tasks, "total": n, "plannedSeq": planned })),
        );
        if kept.is_none() {
            return Err(tr!(
                run.lang,
                "编排没有写进事件日志（Janus 的数据库写入失败），所以没有生效，没有子任务开始。稍后重试同一份编排。",
                "The orchestration was not written to the event log (Janus's database write failed), so it did not take effect and no subtask started. Try the same orchestration again shortly."
            ));
        }
        {
            let mut st = run.st.lock().unwrap();
            for d in &defs {
                st.tasks.push(Task::new(d.clone(), d.assignee.clone(), planned));
            }
        }
        // After delivery the scheduler has returned; new work written in a conversation starts it again.
        self.ensure_driver(run);
        run.wake.notify_one();
        let started =
            defs.iter().map(|d| format!("{}→{}", d.id, d.assignee)).collect::<Vec<_>>().join(list_sep(run.lang));
        Ok(tr!(run.lang, "编排已写入看板：{started}。", "The orchestration is on the board: {started}."))
    }

    /// Board entries for the lead since it last looked, waiting up to the given time for some.
    pub(super) async fn board_wait(&self, run: &Run, args: &Value) -> String {
        struct WaitGuard<'a>(&'a Run);
        impl Drop for WaitGuard<'_> {
            fn drop(&mut self) {
                self.0.st.lock().unwrap().lead.waiting = false;
            }
        }
        run.st.lock().unwrap().lead.waiting = true;
        let _guard = WaitGuard(run);
        self.board_wait_inner(run, args).await
    }

    pub(super) async fn board_wait_inner(&self, run: &Run, args: &Value) -> String {
        let lang = run.lang;
        let limit = wait_limit(args);
        let until = tokio::time::Instant::now() + limit;
        loop {
            let notified = run.board_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let cursor = run.st.lock().unwrap().lead.cursor;
            let events = self.db.events(&run.id, cursor).unwrap_or_default();
            let last = events.last().map(|e| e.seq).unwrap_or(cursor);
            let mut entries = vec![];
            let mut urgent = false;
            for e in &events {
                if let Some(entry) = board_entry(run, e) {
                    entries.push(entry);
                    urgent |= ends_board_wait(e);
                }
            }
            let (open, cancelled) = {
                let st = run.st.lock().unwrap();
                (st.tasks.iter().any(|t| !t.status().terminal()), st.cancelled())
            };
            let timed_out = tokio::time::Instant::now() >= until || cancelled;
            // Entries that only inform wait for one that needs the lead, or for the timeout.
            if !entries.is_empty() && (urgent || !open || timed_out) {
                run.st.lock().unwrap().lead.cursor = last;
                let entries = entries.join("\n");
                let mut text = tr!(lang, "看板上的新消息：\n{entries}", "New on the board:\n{entries}");
                let pulse = if open && !urgent { running_pulse(run) } else { String::new() };
                if !pulse.is_empty() {
                    text.push_str(&tr!(lang, "\n子任务实时进展：\n{pulse}", "\nLive progress of subtasks:\n{pulse}"));
                }
                return text;
            }
            if entries.is_empty() {
                run.st.lock().unwrap().lead.cursor = last;
            }
            if !open {
                return tr!(
                    lang,
                    "看板上没有新消息，也没有在执行或等待的子任务。",
                    "Nothing new on the board, and no subtask running or waiting."
                );
            }
            if timed_out {
                let (secs, pulse) = (limit.as_secs(), running_pulse(run));
                return if pulse.is_empty() {
                    tr!(
                        lang,
                        "{secs} 秒内看板上没有新消息，子任务仍在进行。",
                        "Nothing new on the board in {secs} s; subtasks are still going."
                    )
                } else {
                    tr!(
                        lang,
                        "{secs} 秒内看板上没有新消息。子任务实时进展：\n{pulse}",
                        "Nothing new on the board in {secs} s. Live progress of subtasks:\n{pulse}"
                    )
                };
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// The lead answers a collaborator's question, or adds instructions to its task.
    pub(super) fn board_reply(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(tr!(lang, "text 不能为空", "text cannot be empty"));
        }
        let (agent, question, live) = {
            let st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(no_task(lang, task));
            };
            let session = SessionKey::task(task);
            let question = st
                .pending
                .iter()
                .find(|(_, p)| p.ask == Ask::Question && p.owner.session == session)
                .map(|(k, _)| k.clone());
            (t.agent.clone(), question, st.live.get(&session).cloned())
        };
        if let Some(key) = question {
            self.emit(
                run,
                ev(EventKind::QuestionAnswered, "lead", format!("负责人回答：{}", clip(text, 40)))
                    .task(task)
                    .to(&agent)
                    .data(json!({ "key": key, "answer": text, "via": "lead" })),
            );
            self.resolve(run, &key, Decision::Answer { answer: text.to_string(), note: String::new() })
                .map_err(|e| format!("{e:#}"))?;
            return Ok(tr!(
                lang,
                "已回答 {task} 的提问，{agent} 会接着做。",
                "Answered {task}'s question; {agent} carries on."
            ));
        }
        let status = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .map(|t| t.status())
            .unwrap_or(TaskStatus::Pending);
        if live.is_none() && status.terminal() {
            // A reply to a finished task would otherwise wait for whatever this agent does next.
            let status = status.label_in(lang);
            return Err(tr!(
                lang,
                "{task} 已经结束（{status}），消息不会送达。要它接着做，用 janus.retry。",
                "{task} has ended ({status}), so the message would not arrive. To have it go on, use janus.retry."
            ));
        }
        let msg = self.post_message(run, "lead", &agent, task, &lead_reply(lang, text), &[]);
        match live {
            Some(tx) => {
                tx.send(Steer::Redirect(msg))
                    .map_err(|_| tr!(lang, "{task} 的会话刚刚结束", "{task}'s session has just ended"))?;
                Ok(tr!(lang, "已发给正在执行 {task} 的 {agent}。", "Sent to {agent}, working on {task}."))
            }
            None => {
                self.route_message(run, &agent, task, 0, &msg.id, Delivery::Next);
                run.st.lock().unwrap().notes.entry(agent.clone()).or_default().push(Note::new(msg, task));
                Ok(tr!(
                    lang,
                    "{task} 现在没有在执行，{agent} 下一次开始工作时收到。",
                    "{task} is not running now; {agent} gets it the next time it starts work."
                ))
            }
        }
    }

    /// The lead runs an ended task again: a failed or cancelled one, or a finished one it sends
    /// back for rework. It names the session the next attempt starts in, and may hand over the
    /// findings of a verification (`from`) and a note for this round. The verifications of the
    /// task, and `from`, check its new result again once it is done. Tasks blocked on it start
    /// once it succeeds.
    // @project-doc docs/domains/20261031-run_lifecycle.md#retry
    pub(super) fn board_retry(self: &Arc<Self>, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty());
        let instructions = text("instructions");
        let note = text("note");
        let from = text("from");
        let accept: Option<Vec<String>> = args["accept"].as_array().map(|a| {
            a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect()
        });
        if accept.as_ref().is_some_and(|a| a.is_empty()) {
            return Err(tr!(
                lang,
                "accept 不能是空数组；不传则沿用原来的",
                "accept cannot be an empty array; leave it out to keep the current criteria"
            ));
        }
        let Some(session) = text("session").and_then(SessionChoice::parse) else {
            return Err(tr!(lang, "session 必填：continue 或 new", "session is required: continue or new"));
        };
        // Its own session too full to go back into: refused here, so the lead knows the next attempt
        // starts without that context and writes what it needs into the note.
        let too_full = match session {
            SessionChoice::Continue => {
                self.last_task_session(&run.id, task).and_then(|sid| self.session_too_full(run, &sid))
            }
            SessionChoice::New => None,
        };
        let reverify: Vec<String> = {
            let mut st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(no_task(lang, task));
            };
            if !t.status().terminal() {
                let status = t.status().label_in(lang);
                return Err(tr!(
                    lang,
                    "{task} 现在是「{status}」，还没有结束；要补充指示请用 janus.reply",
                    "{task} is {status} and has not ended; to add instructions, use janus.reply"
                ));
            }
            if let Some(ratio) = too_full {
                let (used, limit) =
                    (format!("{:.0}%", ratio * 100.0), format!("{:.0}%", CONTINUE_CONTEXT_LIMIT * 100.0));
                return Err(tr!(
                    lang,
                    "{task} 原来的会话上下文已用了 {used}（超过 {limit}），不能续接，改用 session=\"new\"。",
                    "{task}'s own session has used {used} of its context (over {limit}) and cannot be continued; use session=\"new\"."
                ));
            }
            if let Some(v) = from {
                let Some(vt) = st.tasks.iter().find(|x| x.def.id == v) else {
                    return Err(tr!(
                        lang,
                        "from 指向的子任务 {v} 不存在",
                        "from names subtask {v}, which does not exist"
                    ));
                };
                if vt.verdict().is_none() {
                    return Err(tr!(
                        lang,
                        "from 要指向一个已经给出判定的独立验证子任务，{v} 不是",
                        "from must name an independent verification that has given its verdict; {v} is not one"
                    ));
                }
                if v == task {
                    return Err(tr!(lang, "from 不能是 {task} 自己", "from cannot be {task} itself"));
                }
                // `from` checks the reworked result after it, so `task` must not wait on `from`.
                let deps: HashMap<&str, &[String]> =
                    st.tasks.iter().map(|x| (x.def.id.as_str(), x.def.deps.as_slice())).collect();
                if crate::teamwork::upstream_of(task, &deps).contains(v) {
                    return Err(tr!(
                        lang,
                        "{task} 依赖 {v}，{v} 不能在 {task} 返工之后再复审它；去掉 from，或返工 {v} 验证的子任务",
                        "{task} depends on {v}, so {v} cannot review it again after the rework; drop from, or rework the subtask {v} verifies"
                    ));
                }
            }
            // Verifications of the task's old result, and the one whose findings it takes, check
            // the new result again.
            let reverify: Vec<String> = st
                .tasks
                .iter()
                .filter(|v| v.status() == TaskStatus::Succeeded)
                .filter(|v| v.def.verifies.as_deref() == Some(task) || Some(v.def.id.as_str()) == from)
                .map(|v| v.def.id.clone())
                .collect();
            let t = Run::task_mut(&mut st, task).expect("found above");
            // A new run of the task brings a new conclusion; earlier corrections were about the old one.
            t.reopen(Reopen::Retry {
                session,
                from: from.map(str::to_owned),
                note: note.map(str::to_owned),
                revised: instructions.is_some() || accept.is_some(),
            });
            if let Some(i) = instructions {
                t.def.desc = i.to_string();
            }
            if let Some(a) = &accept {
                t.def.accept = a.clone();
            }
            for id in &reverify {
                Run::task_mut(&mut st, id).expect("listed above").reverify(task);
            }
            reverify
        };
        // Tasks the lead's `cancel` dropped only because of this one come back with it (and theirs
        // with them); a task the lead cancelled by name stays cancelled.
        let mut revived: Vec<(String, String)> = vec![];
        {
            let mut st = run.st.lock().unwrap();
            let mut queue = vec![task.to_string()];
            while let Some(up) = queue.pop() {
                for t in st.tasks.iter_mut().filter(|t| t.skipped_because() == Some(up.as_str())) {
                    t.set_phase(TaskPhase::Pending);
                    t.retry_base = t.attempts;
                    revived.push((t.def.id.clone(), up.clone()));
                    queue.push(t.def.id.clone());
                }
            }
        }
        let mut data = json!({ "reason": ReopenReason::LeadRetry.as_str(), "session": session.as_str() });
        if let Some(i) = instructions {
            data["instructions"] = json!(i);
        }
        if let Some(a) = &accept {
            data["accept"] = json!(a);
        }
        if let Some(v) = from {
            data["from"] = json!(v);
        }
        if let Some(n) = note {
            data["note"] = json!(n);
        }
        let how = match session {
            SessionChoice::Continue => "在原会话中",
            SessionChoice::New => "在新会话中",
        };
        let title = match from {
            Some(v) => format!("负责人要求 {task} 按 {v} 的意见{how}返工"),
            None => format!("负责人要求 {task} {how}重新执行"),
        };
        self.emit(run, ev(EventKind::TaskReopened, "lead", title).task(task).data(data));
        for id in &reverify {
            self.emit(
                run,
                ev(EventKind::TaskReopened, "janus", format!("{task} 返工，{id} 在它完成后重新验证"))
                    .task(id)
                    .data(json!({ "reason": ReopenReason::TargetReworked.as_str(), "target": task })),
            );
        }
        for (id, up) in &revived {
            self.emit(
                run,
                ev(EventKind::TaskReopened, "janus", format!("{up} 重新执行，因它取消的 {id} 恢复等待"))
                    .task(id)
                    .data(json!({ "reason": ReopenReason::UpstreamRetry.as_str(), "upstream": up })),
            );
        }
        self.ensure_driver(run);
        run.wake.notify_one();
        let sep = list_sep(lang);
        let never_opened = session == SessionChoice::Continue && self.last_task_session(&run.id, task).is_none();
        let mut reply = match session {
            SessionChoice::Continue if never_opened => tr!(
                lang,
                "{task} 之前没有开过会话，会在新会话中重新执行。",
                "{task} never opened a session, so it runs again in a new one."
            ),
            SessionChoice::Continue => {
                tr!(lang, "{task} 会在原会话中重新执行。", "{task} runs again in its own session.")
            }
            SessionChoice::New => tr!(lang, "{task} 会在新会话中重新执行。", "{task} runs again in a new session."),
        };
        if !reverify.is_empty() {
            let ids = reverify.join(sep);
            reply.push_str(&tr!(lang, "它完成后 {ids} 重新验证。", " Once it is done, {ids} verify it again."));
        }
        if !revived.is_empty() {
            let ids = revived.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(sep);
            reply.push_str(&tr!(lang, "因它被取消的 {ids} 也恢复了。", " {ids}, cancelled because of it, are back."));
        }
        // What already took the old result: a running one is told the work under it is changing
        // (it may share files with the rework), a finished one is the lead's to redo or keep.
        let Takers { running, done } = Takers::of(&run.st.lock().unwrap(), task);
        self.tell_takers(
            run,
            &running,
            &tr!(
                lang,
                "上游子任务 {task} 正在返工，它的结果和改过的文件可能会变，用到时先读最新内容。必须等它返工完才能继续时，用 janus.board_post 写一条 blocker。",
                "The upstream subtask {task} is being reworked; its result and the files it changed may change, so read the latest content where you use them. If you cannot go on until the rework is done, post a blocker with janus.board_post."
            ),
        );
        if !running.is_empty() {
            let ids = Takers::ids(&running, lang);
            reply.push_str(&tr!(
                lang,
                "{ids} 正在基于 {task} 的旧结果执行，已告诉它们上游在返工，它们可能和返工改同一批文件。",
                " {ids} are running on {task}'s old result and were told of the rework; they may change the same files as the rework."
            ));
        }
        if !done.is_empty() {
            let ids = done.join(sep);
            reply.push_str(&tr!(
                lang,
                "{ids} 已经基于 {task} 的旧结果完成。",
                " {ids} already finished on {task}'s old result."
            ));
        }
        Ok(reply)
    }

    /// `amend` on a task that failed its acceptance check: the lead accepts the result it handed
    /// in. The task counts as done; the failure stays on record as a risk, with `reason`.
    fn accept_result(&self, run: &Arc<Run>, task: &str, reason: &str) -> std::result::Result<String, String> {
        let lang = run.lang;
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        let mine = |kind: EventKind| events.iter().rev().find(|e| e.kind() == kind && e.task.as_deref() == Some(task));
        let failed =
            mine(EventKind::TaskFailed).filter(|e| e.data.as_ref().is_some_and(|d| d["reason"] == "verification"));
        let submitted = mine(EventKind::ResultSubmitted).filter(|r| failed.is_some_and(|f| r.seq < f.seq));
        let (Some(failed), Some(submitted)) = (failed, submitted) else {
            return Err(tr!(
                lang,
                "{task} 不是没通过验收的子任务，没有可以接受的结果",
                "{task} did not fail its acceptance check, so there is no result to accept"
            ));
        };
        let d = submitted.data.clone().unwrap_or_default();
        // The accepted result keeps the checks and handoff, so its evidence methods are still shown.
        let mut result = result_from_json(&d);
        let error = failed.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or("验证失败").to_string();
        result.risks.push(format!("{error}；负责人接受了结果：{reason}"));
        {
            // Checked and settled in one step, so a retry or cancel in between is not overwritten.
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task).filter(|t| t.status() == TaskStatus::Failed) else {
                return Err(tr!(lang, "{task} 已经不在失败状态", "{task} is no longer failed"));
            };
            t.set_phase(TaskPhase::Succeeded);
            t.result = Some(result.clone());
        }
        let mut event_data = json!({
            "summary": result.summary, "evidence": result.evidence, "risks": result.risks,
            "checks": result.checks, "acceptedBy": "lead", "reason": reason
        });
        if let Some(h) = &result.handoff {
            event_data["handoff"] = json!(h);
        }
        let seq = self.emit(
            run,
            ev(EventKind::TaskSucceeded, "lead", format!("负责人接受 {task} 的结果：{}", clip(reason, 60)))
                .task(task)
                .attempt(submitted.attempt.unwrap_or(0))
                .data(event_data),
        );
        if let Some(t) = Run::task_mut(&mut run.st.lock().unwrap(), task) {
            t.last_success_seq = Some(seq);
        }
        self.recheck_verifications(run, task, submitted.attempt.unwrap_or(0), seq);
        run.wake.notify_one();
        Ok(tr!(lang, "已接受 {task} 的结果。", "Accepted {task}'s result."))
    }

    /// How each role's tasks went in recent runs (the ones `runs()` lists): how many finished,
    /// passed on their first attempt, failed verification, and how long a success took. The
    /// numbers behind choosing a role's agent and model, or rewriting its instructions.
    // @project-doc docs/domains/20261031-agents_and_roles.md#role_stats
    pub fn role_stats(&self) -> Result<Value> {
        let mut out: HashMap<String, RoleStat> = HashMap::new();
        let runs = self.db.runs(RunKind::Goal)?;
        for r in &runs {
            let run_id = r.id.as_str();
            let role_of: HashMap<String, String> = r.config["team"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| m["lead"] != true)
                .filter_map(|m| Some((m["id"].as_str()?.to_string(), m["role_id"].as_str()?.to_string())))
                .collect();
            let events = self.db.events(run_id, 0)?;
            for stat in task_outcomes(&events).into_values() {
                let Some(role) = stat.agent.as_ref().and_then(|a| role_of.get(a)) else { continue };
                let s = out.entry(role.clone()).or_default();
                match stat.outcome {
                    Some(true) => {
                        s.succeeded += 1;
                        if stat.attempts == 1 && stat.verification_failures == 0 {
                            s.first_pass += 1;
                        }
                        if let (Some(a), Some(b)) = (stat.started, stat.ended) {
                            s.success_ms += b - a;
                        }
                    }
                    Some(false) => s.failed += 1,
                    None => continue,
                }
                if stat.verification_failures > 0 {
                    s.verification_failed += 1;
                }
            }
        }
        let roles: serde_json::Map<String, Value> = out
            .into_iter()
            .map(|(k, s)| {
                let done = s.succeeded + s.failed;
                (k, json!({
                    "tasks": done, "succeeded": s.succeeded, "failed": s.failed, "firstPass": s.first_pass,
                    "verificationFailed": s.verification_failed,
                    "avgMinutes": if s.succeeded > 0 { (s.success_ms as f64 / s.succeeded as f64 / 60000.0 * 10.0).round() / 10.0 } else { 0.0 },
                }))
            })
            .collect();
        Ok(json!({ "runs": runs.len(), "roles": roles }))
    }

    /// `task` has a new result (attempt `n`, recorded at `seq`). A verification's conclusion is
    /// about one result of its target, so: verifications of `task` that concluded before this result
    /// check it again, and when `task` is itself a verification whose attempt began before its
    /// target's current result, or whose target is being reworked, it checks again once the target
    /// is done. Each is reopened as the lead's `retry` reopens one (`target.reworked`), so a restart
    /// rebuilds it, and its conclusion on the earlier result stays in the log.
    // @project-doc docs/domains/20261031-goal_mode.md#team_structure
    pub(super) fn recheck_verifications(&self, run: &Arc<Run>, task: &str, n: i64, seq: i64) {
        let started = self
            .db
            .events(&run.id, 0)
            .unwrap_or_default()
            .iter()
            .rfind(|e| e.kind() == EventKind::AttemptStarted && e.task.as_deref() == Some(task) && e.attempt == Some(n))
            .map_or(0, |e| e.seq);
        let reopened: Vec<(String, String)> = {
            let mut st = run.st.lock().unwrap();
            let mut stale: Vec<(String, String)> = st
                .tasks
                .iter()
                .filter(|v| v.def.verifies.as_deref() == Some(task) && v.def.id != task)
                .filter(|v| v.status() == TaskStatus::Succeeded && v.last_success_seq.is_some_and(|s| s < seq))
                .map(|v| (v.def.id.clone(), task.to_string()))
                .collect();
            let own = st.tasks.iter().find(|t| t.def.id == task).and_then(|t| t.def.verifies.clone());
            if let Some(target) = own
                && let Some(tt) = st.tasks.iter().find(|t| t.def.id == target)
                && (tt.status() != TaskStatus::Succeeded || tt.last_success_seq.is_some_and(|s| s > started))
            {
                stale.push((task.to_string(), target));
            }
            for (id, target) in &stale {
                Run::task_mut(&mut st, id).expect("listed above").reverify(target);
            }
            stale
        };
        for (id, target) in &reopened {
            self.emit(
                run,
                ev(
                    EventKind::TaskReopened,
                    "janus",
                    format!("{id} 的结论针对 {target} 之前的结果，在 {target} 的新结果上重新验证"),
                )
                .task(id)
                .data(json!({ "reason": ReopenReason::TargetReworked.as_str(), "target": target })),
            );
        }
        if !reopened.is_empty() {
            run.wake.notify_one();
        }
    }

    /// Tell the running tasks that took a result it changed, as a notice from Janus into their
    /// current turn.
    fn tell_takers(&self, run: &Arc<Run>, running: &[(String, String, mpsc::UnboundedSender<Steer>)], text: &str) {
        for (id, agent, tx) in running {
            let msg = self.post_message(run, "janus", agent, id, &format!("[Janus] {text}"), &[]);
            let _ = tx.send(Steer::Redirect(msg));
        }
    }

    /// The lead corrects the conclusion of a finished task. Tasks that build on it hear about it:
    /// running ones right away, ones that have not started with their handoff. Finished ones are
    /// named in the reply so the lead can decide whether they need redoing. On a task that failed
    /// its acceptance check, the lead accepts its result instead (`accept_result`). A verification
    /// is never amended: its verdict changes only when it verifies again.
    pub(super) fn board_amend(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(tr!(lang, "text 不能为空", "text cannot be empty"));
        }
        let (status, refused) = {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == task);
            (t.map(|t| t.status()), t.and_then(|t| crate::teamwork::amend_refused(&t.def, lang)))
        };
        if let Some(why) = refused {
            return Err(why);
        }
        if status == Some(TaskStatus::Failed) {
            return self.accept_result(run, task, text);
        }
        let takers = {
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task) else {
                return Err(no_task(lang, task));
            };
            if t.status() != TaskStatus::Succeeded {
                let status = t.status().label_in(lang);
                return Err(tr!(
                    lang,
                    "{task} 现在是「{status}」：只能修订已完成子任务的结论，或接受没通过验收核对的结果；还在执行的用 janus.reply",
                    "{task} is {status}: only a finished subtask's conclusion can be revised, or the result of one that failed its acceptance check accepted. For one still running, use janus.reply"
                ));
            }
            t.amendments.push(text.to_string());
            Takers::of(&st, task)
        };
        let Takers { running, done } = takers;
        self.emit(
            run,
            ev(EventKind::ResultAmended, "lead", format!("负责人修订 {task} 的结论：{}", clip(text, 60)))
                .task(task)
                .data(json!({ "text": text })),
        );
        self.tell_takers(
            run,
            &running,
            &tr!(
                lang,
                "负责人修订了上游子任务 {task} 的结论，以修订为准：{text}",
                "The lead revised the conclusion of the upstream subtask {task}, and the revision stands: {text}"
            ),
        );
        let mut out = tr!(lang, "已修订 {task} 的结论。", "Revised {task}'s conclusion.");
        if !running.is_empty() {
            let ids = Takers::ids(&running, lang);
            out.push_str(&tr!(lang, "正在执行的 {ids} 已收到。", " The running {ids} got it."));
        }
        if !done.is_empty() {
            let ids = done.join(list_sep(lang));
            out.push_str(&tr!(lang, "{ids} 已经基于旧结论完成。", " {ids} already finished on the old conclusion."));
        }
        Ok(out)
    }

    /// The lead drops tasks it no longer needs; tasks that depend on them are dropped too.
    /// A running one is stopped.
    pub(super) fn board_cancel(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let reason = args["reason"].as_str().unwrap_or("").trim().to_string();
        let wanted: Vec<String> = args["tasks"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect())
            .unwrap_or_default();
        if wanted.is_empty() {
            return Err(tr!(lang, "tasks 不能为空", "tasks cannot be empty"));
        }
        let (dropped, conns) = {
            let mut st = run.st.lock().unwrap();
            if let Some(missing) = wanted.iter().find(|id| !st.tasks.iter().any(|t| &t.def.id == *id)) {
                return Err(no_task(lang, missing));
            }
            let mut dropped: Vec<(String, Option<String>)> = vec![];
            let mut queue: Vec<(String, Option<String>)> = wanted.iter().map(|id| (id.clone(), None)).collect();
            while let Some((id, because)) = queue.pop() {
                let t = Run::task_mut(&mut st, &id).unwrap();
                if matches!(t.status(), TaskStatus::Succeeded | TaskStatus::SkippedByDecision)
                    || dropped.iter().any(|(d, _)| d == &id)
                {
                    continue;
                }
                t.set_phase(TaskPhase::SkippedByDecision { because: because.clone() });
                dropped.push((id.clone(), because));
                for d in st.tasks.iter().filter(|x| x.def.deps.contains(&id)) {
                    queue.push((d.def.id.clone(), Some(id.clone())));
                }
            }
            let conns: Vec<Arc<AcpConn>> =
                dropped.iter().filter_map(|(id, _)| st.conns.remove(&SessionKey::task(id))).collect();
            (dropped, conns)
        };
        for conn in conns {
            tokio::spawn(async move { conn.kill().await });
        }
        for (id, because) in &dropped {
            let text = match because {
                Some(dep) => format!("{id} 依赖的 {dep} 被负责人取消，一并取消"),
                None => format!("负责人取消 {id}：{}", if reason.is_empty() { "不再需要" } else { &reason }),
            };
            self.emit(
                run,
                ev(EventKind::TaskSkippedByDecision, "lead", text)
                    .task(id)
                    .data(json!({ "reason": reason, "dependency": because })),
            );
        }
        run.wake.notify_one();
        if dropped.is_empty() {
            let st = run.st.lock().unwrap();
            if let Some(t) = st.tasks.iter().find(|t| {
                wanted.contains(&t.def.id)
                    && t.status() == TaskStatus::Succeeded
                    && matches!(t.verdict(), Some(Verdict::Rejected | Verdict::Inconclusive))
            }) {
                let id = &t.def.id;
                return Err(tr!(
                    lang,
                    "{id} 的独立验证已完成，原判定保留，取消不能撤销它。",
                    "{id}'s independent verification is complete and its verdict stands; cancelling cannot undo it."
                ));
            }
            return Ok(tr!(
                lang,
                "这些子任务都已经结束，没有需要取消的。",
                "These subtasks have all ended; nothing to cancel."
            ));
        }
        let ids = dropped.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(list_sep(lang));
        Ok(tr!(lang, "已取消：{ids}。", "Cancelled: {ids}."))
    }

    /// The lead hands the person a decision only they can make. It does not wait here: the answer
    /// comes back through `wait`, and the lead keeps working on what does not depend on it.
    pub(super) fn board_ask_user(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        self.escalate(run, args)?;
        Ok(tr!(run.lang, "已交给用户。", "Handed to the user."))
    }

    /// The lead's (or a Loop's) question for the person (`ask_user`): checked, put on record and
    /// left pending for the person's answer, which nobody awaits here.
    pub(super) fn escalate(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<(), String> {
        let question = args["question"].as_str().unwrap_or("").trim();
        let why = args["why"].as_str().unwrap_or("").trim();
        if question.is_empty() || why.is_empty() {
            return Err(tr!(run.lang, "question 和 why 都不能为空", "question and why cannot be empty"));
        }
        let options: Vec<String> = args["options"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let recommend = args["recommend"].as_str().unwrap_or("");
        let key = crate::util::new_id("q-lead-");
        // Nobody awaits the answer here: it reaches the lead through `wait`.
        self.add_pending(run, &key, Owner { session: SessionKey::Lead, attempt: 0 }, Ask::Question)
            .map_err(|e| format!("{e:#}"))?;
        self.emit(run, ev(EventKind::QuestionEscalated, "lead", format!("负责人请你决定：{}", clip(question, 60))).to("user").data(json!({
            "key": key, "question": question, "options": options, "recommend": recommend, "reason": why, "from": "lead"
        })));
        Ok(())
    }

    /// Everything the board knows about one task, by id: what the board messages and handoffs only
    /// summarise. Its upstream and downstream tasks are named, so a caller can follow the chain
    /// one task at a time.
    // @project-doc docs/interfaces/20261031-board_mcp.md#task_detail
    pub(super) fn task_detail(&self, run: &Run, task: &str) -> std::result::Result<String, String> {
        let lang = run.lang;
        let sep = list_sep(lang);
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        // The usage ledger is read-only and never a reason to fail: a task without usage still
        // answers, saying 「未上报」.
        let usage = crate::usage::ledger(&events);
        let st = run.st.lock().unwrap();
        let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
            return Err(no_task(lang, task));
        };
        let name = |agent: &str| run.member(agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.to_string());
        let label = |id: &str| {
            let status = st
                .tasks
                .iter()
                .find(|x| x.def.id == id)
                .map(|x| x.status().label_in(lang))
                .unwrap_or(lang.pick("不在看板上", "not on the board"));
            tr!(lang, "{id}（{status}）", "{id} ({status})")
        };
        let bullets = |items: &[String], indent: &str| {
            items.iter().map(|x| format!("{indent}- {x}")).collect::<Vec<_>>().join("\n")
        };
        let mine = |e: &&Event| e.task.as_deref() == Some(task);
        let (it, who, agent, status, tried, desc) = (
            titled(lang, task, &t.def.title),
            name(&t.agent),
            &t.agent,
            t.status().label_in(lang),
            t.attempts,
            &t.def.desc,
        );
        let mut out = vec![
            tr!(
                lang,
                "子任务 {it}· 执行者 {who}（{agent}）· {status} · 已尝试 {tried} 次",
                "Subtask {it} · assignee {who} ({agent}) · {status} · {tried} attempts so far"
            ),
            tr!(lang, "说明：{desc}", "Instructions: {desc}"),
        ];
        let accept = bullets(&t.def.accept, "");
        out.push(tr!(lang, "验收标准：\n{accept}", "Acceptance criteria:\n{accept}"));
        if !t.def.owns.is_empty() {
            let owns = t.def.owns.join(sep);
            out.push(tr!(lang, "负责的文件：{owns}", "Owns: {owns}"));
        }
        if let Some(v) = &t.def.verifies {
            out.push(tr!(lang, "独立验证：{v}", "Independently verifies: {v}"));
        }
        if let Some(c) = &t.def.continues {
            out.push(tr!(lang, "续接会话：{c}", "Continues the session of: {c}"));
        }
        let downstream: Vec<String> =
            st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).map(|x| label(&x.def.id)).collect();
        if !t.def.deps.is_empty() {
            let up = t.def.deps.iter().map(|d| label(d)).collect::<Vec<_>>().join(sep);
            out.push(tr!(lang, "上游：{up}", "Upstream: {up}"));
        }
        if !downstream.is_empty() {
            let down = downstream.join(sep);
            out.push(tr!(lang, "下游：{down}", "Downstream: {down}"));
        }
        if let Some(r) = &t.result {
            let summary = &r.summary;
            out.push(tr!(lang, "结果：{summary}", "Result: {summary}"));
            if let Some(vd) = t.verdict() {
                let label = vd.label_in(lang);
                out.push(tr!(lang, "验证判定：{label}", "Verdict: {label}"));
            }
            if let Some(h) = &r.handoff {
                out.push(tr!(lang, "交接说明：{h}", "Handoff: {h}"));
            }
            let totals = Summary::of_in(&t.def.accept, &r.checks, lang);
            let (passed, criteria, counts) = (totals.passed, totals.criteria, totals.counts(lang));
            let checks = r.checks.iter().map(|c| format!("\n  {}", c.line(lang))).collect::<String>();
            out.push(tr!(
                lang,
                "逐条验收（{criteria} 条，通过 {passed} 条）：{checks}",
                "Checks ({criteria} criteria, {passed} passed):{checks}"
            ));
            out.push(tr!(lang, "证据方法：{counts}", "Evidence methods: {counts}"));
            if !totals.reminders.is_empty() {
                let reminders = totals.reminders.join(item_sep(lang));
                out.push(tr!(
                    lang,
                    "提醒（不影响是否通过）：{reminders}",
                    "Reminders (they do not affect passing): {reminders}"
                ));
            }
            if !r.evidence.is_empty() {
                let evidence = bullets(&r.evidence, "");
                out.push(tr!(lang, "其他证据：\n{evidence}", "Other evidence:\n{evidence}"));
            }
            if !r.risks.is_empty() {
                let risks = bullets(&r.risks, "");
                out.push(tr!(lang, "风险：\n{risks}", "Risks:\n{risks}"));
            }
        }
        // Those who verified this task independently: their own conclusion, kept apart from the
        // evidence the executor reported.
        for v in st.tasks.iter().filter(|v| v.def.verifies.as_deref() == Some(task)) {
            let verdict_tag = v
                .verdict()
                .map(|vd| {
                    let label = vd.label_in(lang);
                    tr!(lang, " · 判定「{label}」", " · verdict \"{label}\"")
                })
                .unwrap_or_default();
            let (id, status) = (&v.def.id, v.status().label_in(lang));
            out.push(tr!(
                lang,
                "独立验证 {id}：{status}{verdict_tag}",
                "Independent verification {id}: {status}{verdict_tag}"
            ));
            if let Some(r) = &v.result {
                out.push(format!("  {}", r.summary));
                for c in &r.checks {
                    out.push(format!("  {}", c.line(lang)));
                }
                if !r.evidence.is_empty() {
                    let evidence = bullets(&r.evidence, "  ");
                    out.push(tr!(lang, "  独立证据：\n{evidence}", "  Its own evidence:\n{evidence}"));
                }
            }
            if let Some(f) = st.last_verification.get(&v.def.id).filter(|_| v.status() != TaskStatus::Succeeded) {
                out.push(tr!(lang, "  没通过核对：\n{f}", "  Failed its check:\n{f}"));
            }
        }
        if !t.amendments.is_empty() {
            let amendments = t.amendments.join(item_sep(lang));
            out.push(tr!(
                lang,
                "负责人的修订（以此为准）：{amendments}",
                "The lead's revisions (these stand): {amendments}"
            ));
        }
        if let Some(v) = st.last_verification.get(task).filter(|_| t.status() != TaskStatus::Succeeded) {
            out.push(tr!(lang, "最近一次没通过验收核对：\n{v}", "Last failed acceptance check:\n{v}"));
        }
        if let Some(e) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind() == EventKind::AttemptFailed)
            .filter(|_| t.status() != TaskStatus::Succeeded)
        {
            let error = e.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or(&e.text);
            out.push(tr!(lang, "最近一次尝试失败：{error}", "Last failed attempt: {error}"));
        }
        let files = st.traces.files(task);
        if !files.is_empty() {
            let files = files.into_iter().collect::<Vec<_>>().join(sep);
            out.push(tr!(lang, "改过的文件：{files}", "Files changed: {files}"));
        }
        if let Some(s) = t.success_seq() {
            let note = crate::traces::stale_note(&st.traces.edited_after(task, s), lang);
            if !note.is_empty() {
                out.push(note);
            }
        }
        if let Some(gaps) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind() == EventKind::TaskSucceeded)
            .filter(|_| t.status() == TaskStatus::Succeeded)
            .and_then(|e| e.data.as_ref()?["traceGaps"].as_array().cloned())
            .filter(|g| !g.is_empty())
        {
            let gaps = gaps.iter().filter_map(|g| g.as_str()).map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n");
            out.push(tr!(
                lang,
                "痕迹核对（结果和工作目录对不上的地方）：\n{gaps}",
                "Trace check (where the result and the working directory disagree):\n{gaps}"
            ));
        }
        let arts: Vec<String> = t.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
        if !arts.is_empty() {
            let arts = arts.join(sep);
            out.push(tr!(lang, "产物：{arts}", "Artifacts: {arts}"));
        }
        // A conflict is recorded on the task whose edit found it; both sides see it here.
        let theirs = |e: &&Event| {
            e.kind() == EventKind::TaskConflict
                && e.data.as_ref().is_some_and(|d| d["byLead"] != true && d["other"] == task)
        };
        let talk: Vec<String> = events
            .iter()
            .filter(|e| mine(e) || theirs(e))
            .filter_map(|e| {
                let mut d = e.data.clone().unwrap_or_default();
                if theirs(&e) {
                    d["other"] = json!(e.task.clone().unwrap_or_default());
                }
                let paths = d["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(sep);
                let other = d["other"].as_str().unwrap_or("?");
                match e.kind() {
                    EventKind::BoardPosted => {
                        let (label, text) = (PostKind::of(&d).label(lang), d["text"].as_str().unwrap_or(&e.text));
                        Some(format!("- [{label}] {text}"))
                    }
                    EventKind::QuestionAsked => {
                        let q = d["question"].as_str().unwrap_or(&e.text);
                        Some(tr!(lang, "- [提问] {q}", "- [Question] {q}"))
                    }
                    EventKind::QuestionAnswered => {
                        let a = d["answer"].as_str().unwrap_or(&e.text);
                        Some(tr!(lang, "- [回答] {a}", "- [Answer] {a}"))
                    }
                    EventKind::TaskConflict if d["byLead"] == true => Some(tr!(
                        lang,
                        "- [冲突] 负责人改了这个子任务的文件：{paths}",
                        "- [Conflict] The lead changed this subtask's files: {paths}"
                    )),
                    EventKind::TaskConflict if d["owned"] == true && theirs(&e) => Some(tr!(
                        lang,
                        "- [越界] {other} 改了这个子任务负责的 {paths}",
                        "- [Out of scope] {other} changed {paths}, which this subtask owns"
                    )),
                    EventKind::TaskConflict if d["owned"] == true => Some(tr!(
                        lang,
                        "- [越界] 改了 {other} 负责的 {paths}",
                        "- [Out of scope] Changed {paths}, which {other} owns"
                    )),
                    EventKind::TaskConflict => Some(tr!(
                        lang,
                        "- [冲突] 和 {other} 同时改了 {paths}",
                        "- [Conflict] Changed {paths} at the same time as {other}"
                    )),
                    EventKind::TaskSplitRequested => {
                        let reason = d["reason"].as_str().unwrap_or(&e.text);
                        let n = d["subtasks"].as_array().map(Vec::len).unwrap_or(0);
                        Some(tr!(
                            lang,
                            "- [请求拆分] {reason}（建议 {n} 个子任务）",
                            "- [Split requested] {reason} ({n} subtasks proposed)"
                        ))
                    }
                    _ => None,
                }
            })
            .collect();
        if !talk.is_empty() {
            let talk = talk.join("\n");
            out.push(tr!(lang, "看板记录：\n{talk}", "On the board:\n{talk}"));
        }
        out.extend(usage.task_lines(task, &Self::usage_names(run), lang));
        Ok(out.join("\n"))
    }

    /// A collaborator's view of the rest of the board: the other tasks, how they relate to its
    /// own (upstream, downstream, the same files) and what they wrote back lately. With `task`,
    /// the full detail of that one.
    // @project-doc docs/interfaces/20261031-board_mcp.md#peers
    pub(super) fn board_peers(&self, run: &Run, me: &str, args: &Value) -> std::result::Result<String, String> {
        if let Some(t) = args["task"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            return self.task_detail(run, t);
        }
        let lang = run.lang;
        let sep = list_sep(lang);
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        let st = run.st.lock().unwrap();
        let deps: Vec<String> =
            st.tasks.iter().find(|t| t.def.id == me).map(|t| t.def.deps.clone()).unwrap_or_default();
        let my_files = st.traces.files(me);
        let lines: Vec<String> = st
            .tasks
            .iter()
            .filter(|t| t.def.id != me)
            .map(|t| {
                let mut tags = vec![];
                if deps.contains(&t.def.id) {
                    tags.push(lang.pick("你的上游", "your upstream").to_string());
                }
                if t.def.deps.iter().any(|d| d == me) {
                    tags.push(lang.pick("依赖你", "depends on you").to_string());
                }
                let theirs = st.traces.files(&t.def.id);
                let common: Vec<String> = theirs.intersection(&my_files).cloned().collect();
                if !t.def.owns.is_empty() && !t.status().terminal() {
                    let owns = t.def.owns.join(sep);
                    tags.push(tr!(lang, "负责 {owns}", "owns {owns}"));
                }
                if !common.is_empty() {
                    let common = common.join(sep);
                    tags.push(tr!(lang, "和你都改了 {common}", "changed {common}, as you did"));
                } else if t.status() == TaskStatus::Running && !theirs.is_empty() {
                    let files = theirs.iter().take(5).cloned().collect::<Vec<_>>().join(sep);
                    tags.push(tr!(lang, "改过 {files}", "changed {files}"));
                }
                if let Some(s) = t.success_seq() {
                    let stale = st.traces.edited_after(&t.def.id, s);
                    if !stale.is_empty() {
                        let files = stale.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(sep);
                        tags.push(tr!(
                            lang,
                            "完成后 {files} 又被改过，结论可能已过期",
                            "{files} changed again after it finished; its conclusion may be out of date"
                        ));
                    }
                }
                let who = run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent);
                let (it, status) = (titled(lang, &t.def.id, &t.def.title), t.status().label_in(lang));
                let tags = if tags.is_empty() { String::new() } else { format!(" · {}", tags.join(" · ")) };
                tr!(lang, "- {it}· {who} · {status}{tags}", "- {it} · {who} · {status}{tags}")
            })
            .collect();
        let posts: Vec<String> = events
            .iter()
            .filter(|e| e.kind() == EventKind::BoardPosted && e.task.as_deref() != Some(me))
            .rev()
            .take(5)
            .map(|e| {
                let task = e.task.as_deref().unwrap_or("?");
                let text = clip(e.data.as_ref().and_then(|d| d["text"].as_str()).unwrap_or(&e.text), INLINE_ITEM);
                tr!(lang, "- {task}：{text}", "- {task}: {text}")
            })
            .collect();
        let lines = if lines.is_empty() { lang.pick("（没有）", "(none)").into() } else { lines.join("\n") };
        let posts = if posts.is_empty() {
            String::new()
        } else {
            let posts = posts.join("\n");
            tr!(lang, "\n\n最近写回看板的内容：\n{posts}", "\n\nRecently written to the board:\n{posts}")
        };
        Ok(tr!(lang, "看板上的其他子任务：\n{lines}{posts}", "Other subtasks on the board:\n{lines}{posts}"))
    }

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

/// One-line live heartbeat per running task (plus pending count) when `janus.wait` times out.
fn running_pulse(run: &Run) -> String {
    let now = now_ms();
    let watches: HashMap<SessionKey, (i64, AgentPhase, String)> = run
        .watch
        .lock()
        .unwrap()
        .iter()
        .map(|(k, w)| (k.clone(), (w.started_ms, w.phase, w.last_tool.clone())))
        .collect();
    let lang = run.lang;
    let st = run.st.lock().unwrap();
    let mut lines = vec![];
    for t in st.tasks.iter().filter(|t| t.status() == TaskStatus::Running) {
        let who = run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent);
        let edited = st.traces.files(&t.def.id).len();
        let detail = match watches.get(&SessionKey::task(&t.def.id)) {
            Some((started, phase, tool)) => {
                let elapsed = span((now - *started).max(0), lang);
                let tool_part = if tool.is_empty() {
                    String::new()
                } else {
                    let tool = clip(tool, 40);
                    tr!(lang, "（{tool}）", " ({tool})")
                };
                let phase = phase.label_in(lang);
                tr!(lang, "{phase}{tool_part}，已运行 {elapsed}", "{phase}{tool_part}, running for {elapsed}")
            }
            None => lang.pick("正在启动会话", "starting its session").to_string(),
        };
        let files_part = if edited > 0 {
            tr!(lang, "，已改 {edited} 个文件", ", {edited} files changed")
        } else {
            String::new()
        };
        let it = titled(lang, &t.def.id, &t.def.title);
        lines.push(tr!(lang, "- {it}· {who}：{detail}{files_part}", "- {it} · {who}: {detail}{files_part}"));
    }
    let pending = st.tasks.iter().filter(|t| t.status() == TaskStatus::Pending).count();
    if pending > 0 {
        lines.push(tr!(lang, "- 另有 {pending} 个子任务在等待开始", "- {pending} more subtasks waiting to start"));
    }
    lines.join("\n")
}

#[derive(Default)]
pub(super) struct RoleStat {
    succeeded: i64,
    failed: i64,
    first_pass: i64,
    verification_failed: i64,
    success_ms: i64,
}

/// How one task went, read from a run's events.
#[derive(Default)]
pub(super) struct TaskOutcome {
    agent: Option<String>,
    attempts: i64,
    verification_failures: i64,
    started: Option<i64>,
    ended: Option<i64>,
    /// Some(true) succeeded, Some(false) failed or cancelled after trying, None never finished.
    outcome: Option<bool>,
}

pub(super) fn task_outcomes(events: &[Event]) -> HashMap<String, TaskOutcome> {
    let mut out: HashMap<String, TaskOutcome> = HashMap::new();
    for e in events {
        let Some(task) = e.task.as_deref() else { continue };
        let t = out.entry(task.to_string()).or_default();
        match e.kind() {
            EventKind::TaskAssigned => t.agent = e.to.clone().or(t.agent.take()),
            EventKind::AttemptStarted => {
                t.attempts += 1;
                t.started.get_or_insert(e.ts);
                if t.agent.is_none() {
                    t.agent = e.to.clone();
                }
            }
            EventKind::VerificationFailed => t.verification_failures += 1,
            EventKind::TaskSucceeded => {
                t.outcome = Some(true);
                t.ended = Some(e.ts);
            }
            EventKind::TaskFailed => t.outcome = Some(false),
            // A retry starts the count again: what matters is how the task finally went.
            EventKind::TaskReopened => t.outcome = None,
            _ => {}
        }
    }
    out
}

/// Why a task's last result did not pass its acceptance check, one line per gap.
pub(super) fn verification_output(run: &Run, task: &str) -> String {
    let st = run.st.lock().unwrap();
    st.last_verification.get(task).cloned().unwrap_or_default()
}
