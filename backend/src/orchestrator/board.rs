//! The shared board as an MCP server: authentication, the lead's and collaborators' tool calls, and
//! role statistics.

use super::*;

impl App {
    /// The Janus MCP server one session reaches: the lead's board tools, or a collaborator's.
    pub(super) fn mcp_server(&self, run: &Run, who: &str) -> Value {
        let token = session_token(&run.st.lock().unwrap().mcp_token, who);
        json!({ "type": "http", "name": "janus", "url": format!("http://{}/mcp/{}/{}/{}", janus_addr(), run.id, token, who), "headers": [] })
    }

    /// One MCP request (streamable HTTP, JSON responses) from a session of this run. `who` is
    /// "lead" or the task id of the collaborator's session. Notifications get no response.
    // @project-doc docs/interfaces/board_mcp.md#transport
    pub async fn mcp(self: &Arc<Self>, run_id: &str, token: &str, who: &str, req: Value) -> Result<Option<Value>> {
        let run = self.run(run_id).context("这个 Run 不在运行")?;
        // The URL's token is bound to its `who`, so a collaborator cannot rename itself to `lead`
        // or to another task: only the session Janus started holds the token for that name.
        let run_token = run.st.lock().unwrap().mcp_token.clone();
        if run_token.is_empty() || token != session_token(&run_token, who) {
            bail!("token 不对");
        }
        let Some(id) = req.get("id").cloned() else {
            return Ok(None);
        };
        let lead = who == "lead";
        let tool_call = req["method"].as_str() == Some("tools/call");
        let tool_name = req["params"]["name"].as_str().unwrap_or("工具").to_string();
        let tool_input = req["params"]["arguments"].clone();
        let call_id = id.to_string();
        if tool_call {
            let mut e = ev("tool.started", who, format!("调用 janus.{tool_name}，等待返回")).data(json!({
                "callId": call_id,
                "tool": format!("janus.{tool_name}"),
                "input": summarize_input(&tool_input, &tool_name),
                "args": tool_input,
                "status": "pending"
            }));
            if !lead {
                e = e.task(who);
            }
            self.emit(&run, e);
        }
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "janus", "version": env!("CARGO_PKG_VERSION") },
                "instructions": if lead { "Janus 团队的共享看板。用 orchestrate 提交编排，wait 取回进展和提问，reply 回答协作者或补充指示，retry / accept / cancel 处理失败或不再需要的任务，ask_user 把只有用户能做的决定交给用户，team 查看团队和任务状态，task_detail 查看某个任务的完整说明和结果。" } else { "Janus 团队的共享看板。执行中有进展、阻塞或需要主控注意的事，用 board_post 写回看板；改负责范围以外的文件前用 claim 认领；用 peers 查看其他任务的状态、改过的文件和完整结果。" },
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": if lead { lead_tools() } else { member_tools() } }),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = &req["params"]["arguments"];
                let out = match (lead, name) {
                    (true, "team") => Ok(self.board_team(&run)),
                    (true, "orchestrate") => self.board_orchestrate(&run, args),
                    (true, "wait") => Ok(self.board_wait(&run, args).await),
                    (true, "reply") => self.board_reply(&run, args),
                    (true, "retry") => self.board_retry(&run, args),
                    (true, "accept") => self.board_accept(&run, args),
                    (true, "amend") => self.board_amend(&run, args),
                    (true, "cancel") => self.board_cancel(&run, args),
                    (true, "ask_user") => self.board_ask_user(&run, args),
                    (true, "task_detail") => self.task_detail(&run, args["task"].as_str().unwrap_or("").trim()),
                    (false, "board_post") => self.board_post(&run, who, args),
                    (false, "peers") => self.board_peers(&run, who, args),
                    (false, "claim") => self.board_claim(&run, who, args),
                    _ => Err(format!("没有这个工具：{name}")),
                };
                match out {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
                }
            }
            m => {
                return Ok(Some(
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("unknown method {m}") } }),
                ));
            }
        };
        if tool_call {
            let failed = result["isError"] == true;
            let output = result["content"][0]["text"].as_str().unwrap_or("");
            let mut e = ev(
                "tool.completed",
                who,
                if failed { format!("janus.{tool_name} 失败") } else { format!("janus.{tool_name} 返回结果") },
            )
            .data(json!({
                "callId": call_id,
                "tool": format!("janus.{tool_name}"),
                "input": summarize_input(&tool_input, &tool_name),
                "args": tool_input,
                "output": short(output, 8000),
                "status": if failed { "failed" } else { "completed" }
            }));
            if !lead {
                e = e.task(who);
            }
            self.emit(&run, e);
        }
        Ok(Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })))
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
        let st = run.st.lock().unwrap();
        let tasks: Vec<String> =
            st.tasks.iter().map(|t| format!("- {} · {} · {} · {}", t.def.id, t.agent, t.status, t.def.title)).collect();
        drop(st);
        format!(
            "协作者：\n{}\n\n看板上的任务：\n{}\n\n用量（只读，缺失写「未上报」而不是 0）：\n{}",
            team_lines(run),
            if tasks.is_empty() { "（还没有）".into() } else { tasks.join("\n") },
            self.usage(run).board_lines(&Self::usage_names(run))
        )
    }

    /// Validate an orchestration and, if it holds, put its tasks on the board for the scheduler.
    pub(super) fn board_orchestrate(
        self: &Arc<Self>,
        run: &Arc<Run>,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let defs = {
            let st = run.st.lock().unwrap();
            let board: Vec<(TaskDef, TaskStatus)> = st.tasks.iter().map(|t| (t.def.clone(), t.status)).collect();
            validate_orchestration(run, &board, args)
        };
        let defs = match defs {
            Ok(defs) => defs,
            Err(errors) => {
                self.emit(
                    run,
                    ev(
                        "orchestration.rejected",
                        "lead",
                        format!("编排没有通过校验：{}", short(&errors.join("；"), 120)),
                    )
                    .data(json!({ "errors": errors, "submitted": args })),
                );
                return Err(format!(
                    "编排没有通过校验，没有任何任务开始。请修正后重新提交：\n- {}",
                    errors.join("\n- ")
                ));
            }
        };
        let tasks: Vec<Value> = defs
            .iter()
            .map(|d| {
                json!({
                    "id": d.id, "title": d.title, "desc": d.desc, "deps": d.deps,
                    "accept": d.accept, "maxAttempts": d.max_attempts, "agent": d.assignee,
                    "owns": d.owns, "verifies": d.verifies,
                })
            })
            .collect();
        let n = {
            let mut st = run.st.lock().unwrap();
            for d in &defs {
                st.tasks.push(Task::new(d.clone(), d.assignee.clone()));
            }
            st.tasks.len()
        };
        self.emit(
            run,
            ev("orchestration.accepted", "lead", format!("编排通过校验：{} 个任务写入看板，开始执行", defs.len()))
                .data(json!({ "tasks": tasks, "total": n })),
        );
        // After delivery the scheduler has returned; new work written in a conversation starts it again.
        self.ensure_driver(run);
        run.wake.notify_one();
        Ok(format!(
            "编排已通过校验并开始执行：{}。用 janus.wait 取回进展和结果。",
            defs.iter().map(|d| format!("{}→{}", d.id, d.assignee)).collect::<Vec<_>>().join("、")
        ))
    }

    /// Board entries for the lead since it last looked, waiting up to the given time for some.
    pub(super) async fn board_wait(&self, run: &Run, args: &Value) -> String {
        run.st.lock().unwrap().lead_waiting = true;
        let out = self.board_wait_inner(run, args).await;
        run.st.lock().unwrap().lead_waiting = false;
        out
    }

    pub(super) async fn board_wait_inner(&self, run: &Run, args: &Value) -> String {
        let limit = Duration::from_secs(args["timeout_sec"].as_u64().unwrap_or(40).clamp(1, 45));
        let until = tokio::time::Instant::now() + limit;
        loop {
            let cursor = run.st.lock().unwrap().lead_cursor;
            let events = self.db.events(&run.id, cursor).unwrap_or_default();
            let last = events.last().map(|e| e.seq).unwrap_or(cursor);
            let entries: Vec<String> = events.iter().filter_map(|e| board_entry(run, e)).collect();
            run.st.lock().unwrap().lead_cursor = last;
            if !entries.is_empty() {
                return format!("看板上的新消息：\n{}", entries.join("\n"));
            }
            let open = run.st.lock().unwrap().tasks.iter().any(|t| !t.status.terminal());
            if !open {
                return "看板上没有新消息，也没有在执行或等待的任务。".into();
            }
            if tokio::time::Instant::now() >= until || run.st.lock().unwrap().cancelled {
                return format!("{} 秒内看板上没有新消息，任务仍在进行。可以再次调用 janus.wait。", limit.as_secs());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// The lead answers a collaborator's question, or adds instructions to its task.
    pub(super) fn board_reply(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err("text 不能为空".into());
        }
        let (agent, question, live) = {
            let st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(format!("看板上没有任务 {task}"));
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
                ev("question.answered", "lead", format!("主控回答：{}", short(text, 40)))
                    .task(task)
                    .to(&agent)
                    .data(json!({ "key": key, "answer": text, "via": "lead" })),
            );
            self.resolve(run, &key, json!({ "answer": text, "note": "" })).map_err(|e| format!("{e:#}"))?;
            return Ok(format!("已回答 {task} 的提问，{agent} 会接着做。"));
        }
        let status = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .map(|t| t.status)
            .unwrap_or(TaskStatus::Pending);
        if live.is_none() && status.terminal() {
            // A reply to a finished task would otherwise wait for whatever this agent does next.
            return Err(format!(
                "{task} 已经结束（{}），{agent} 没有在做这个任务，消息不会送达。需要它继续做事，请用 janus.retry 重新执行 {task}，或用 janus.orchestrate 新建任务。",
                status.label()
            ));
        }
        let msg = self.post_message(run, "lead", &agent, task, &format!("{LEAD_REPLY}{text}"), &[]);
        match live {
            Some(tx) => {
                tx.send(Steer::Redirect(msg)).map_err(|_| format!("{task} 的会话刚刚结束"))?;
                Ok(format!(
                    "已把消息发给正在执行 {task} 的 {agent}：能插话时插入当前这一轮，否则打断当前这一轮后立即发送。"
                ))
            }
            None => {
                self.route_message(run, &agent, task, 0, &msg.id, Delivery::Next);
                run.st.lock().unwrap().notes.entry(agent.clone()).or_default().push(msg);
                Ok(format!("{task} 现在没有在执行，消息会在 {agent} 下一次开始工作时带给它。"))
            }
        }
    }

    /// The lead runs a failed or cancelled task again, optionally with new instructions or
    /// acceptance criteria. Tasks blocked on it start once it succeeds.
    pub(super) fn board_retry(self: &Arc<Self>, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let task = args["task"].as_str().unwrap_or("").trim();
        let instructions = args["instructions"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let accept: Option<Vec<String>> = args["accept"].as_array().map(|a| {
            a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect()
        });
        if accept.as_ref().is_some_and(|a| a.is_empty()) {
            return Err("accept 不能是空的：写出可核对的验收标准，或不传以沿用原来的".into());
        }
        {
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task) else {
                return Err(format!("看板上没有任务 {task}"));
            };
            if !matches!(t.status, TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::SkippedByDecision) {
                return Err(format!("{task} 现在是「{}」，只能重试失败或已取消的任务", t.status.label()));
            }
            t.set_status(TaskStatus::Pending);
            t.active_attempt = None;
            t.retry_base = t.attempts;
            t.skipped_because = None;
            // A new run of the task brings a new conclusion; earlier corrections were about the old one.
            t.amendments.clear();
            if let Some(i) = instructions {
                t.def.desc = i.to_string();
            }
            if let Some(a) = &accept {
                t.def.accept = a.clone();
            }
        }
        // Tasks the lead's `cancel` dropped only because of this one come back with it (and theirs
        // with them); a task the lead cancelled by name stays cancelled.
        let mut revived: Vec<(String, String)> = vec![];
        {
            let mut st = run.st.lock().unwrap();
            let mut queue = vec![task.to_string()];
            while let Some(up) = queue.pop() {
                for t in st.tasks.iter_mut().filter(|t| {
                    t.status == TaskStatus::SkippedByDecision && t.skipped_because.as_deref() == Some(up.as_str())
                }) {
                    t.set_status(TaskStatus::Pending);
                    t.skipped_because = None;
                    t.retry_base = t.attempts;
                    revived.push((t.def.id.clone(), up.clone()));
                    queue.push(t.def.id.clone());
                }
            }
        }
        let mut data = json!({ "reason": "lead.retry" });
        if let Some(i) = instructions {
            data["instructions"] = json!(i);
        }
        if let Some(a) = &accept {
            data["accept"] = json!(a);
        }
        self.emit(run, ev("task.reopened", "lead", format!("主控要求重新执行 {task}")).task(task).data(data));
        for (id, up) in &revived {
            self.emit(
                run,
                ev("task.reopened", "janus", format!("{up} 重新执行，因它取消的 {id} 恢复等待"))
                    .task(id)
                    .data(json!({ "reason": "upstream.retry", "upstream": up })),
            );
        }
        self.ensure_driver(run);
        run.wake.notify_one();
        if revived.is_empty() {
            Ok(format!("{task} 会重新执行，依赖它的任务会在它成功后继续。"))
        } else {
            Ok(format!(
                "{task} 会重新执行，依赖它的任务会在它成功后继续；因它被取消的 {} 也恢复了。",
                revived.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join("、")
            ))
        }
    }

    /// The lead accepts a result that failed only its verification (for example a command that
    /// does not fit the workspace). The task counts as done; the failure stays on record as a risk.
    pub(super) fn board_accept(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let task = args["task"].as_str().unwrap_or("").trim();
        let reason = args["reason"].as_str().unwrap_or("").trim();
        if reason.is_empty() {
            return Err("reason 不能为空：写明你核对了什么，为什么没达到的部分不影响目标".into());
        }
        let refused = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .and_then(|t| crate::teamwork::accept_refused(&t.def));
        if let Some(why) = refused {
            return Err(why);
        }
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        let mine = |kind: &str| events.iter().rev().find(|e| e.kind == kind && e.task.as_deref() == Some(task));
        let failed = mine("task.failed").filter(|e| e.data.as_ref().is_some_and(|d| d["reason"] == "verification"));
        let submitted = mine("result.submitted").filter(|r| failed.is_some_and(|f| r.seq < f.seq));
        let (Some(failed), Some(submitted)) = (failed, submitted) else {
            return Err(format!("{task} 不是因没通过验收核对而失败的任务，没有可以接受的结果"));
        };
        let d = submitted.data.clone().unwrap_or_default();
        let strings = |v: &Value| -> Vec<String> {
            v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default()
        };
        let mut result = TaskResult {
            summary: d["summary"].as_str().unwrap_or("").to_string(),
            evidence: strings(&d["evidence"]),
            risks: strings(&d["risks"]),
            // The accepted result keeps the checks, so its evidence methods are still shown.
            checks: checks_from(&d),
            ..Default::default()
        };
        let error = failed.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or("验证失败").to_string();
        result.risks.push(format!("{error}；主控接受了结果：{reason}"));
        {
            // Checked and settled in one step, so a retry or cancel in between is not overwritten.
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task).filter(|t| t.status == TaskStatus::Failed) else {
                return Err(format!("{task} 已经不在失败状态"));
            };
            t.set_status(TaskStatus::Succeeded);
            t.result = Some(result.clone());
        }
        let seq = self.emit(run, ev("task.succeeded", "lead", format!("主控接受 {task} 的结果：{}", short(reason, 60))).task(task).attempt(submitted.attempt.unwrap_or(0)).data(json!({
            "summary": result.summary, "evidence": result.evidence, "risks": result.risks, "checks": result.checks, "acceptedBy": "lead", "reason": reason
        })));
        if let Some(t) = Run::task_mut(&mut run.st.lock().unwrap(), task) {
            t.succeeded_seq = Some(seq);
        }
        run.wake.notify_one();
        Ok(format!("已接受 {task} 的结果，依赖它的任务会继续。"))
    }

    /// How each role's tasks went in recent runs (the ones `runs()` lists): how many finished,
    /// passed on their first attempt, failed verification, and how long a success took. The
    /// numbers behind choosing a role's agent and model, or rewriting its instructions.
    // @project-doc docs/domains/agents_and_roles.md#role_stats
    pub fn role_stats(&self) -> Result<Value> {
        let mut out: HashMap<String, RoleStat> = HashMap::new();
        let runs = self.db.runs()?;
        for r in &runs {
            let run_id = r["id"].as_str().unwrap_or_default();
            let role_of: HashMap<String, String> = r["config"]["team"]
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

    /// The lead corrects the conclusion of a finished task. Tasks that build on it hear about it:
    /// running ones right away, ones that have not started with their handoff. Finished ones are
    /// named in the reply so the lead can decide whether they need redoing.
    pub(super) fn board_amend(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err("text 不能为空：写明修订后的结论".into());
        }
        let (running, done) = {
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task) else {
                return Err(format!("看板上没有任务 {task}"));
            };
            if t.status != TaskStatus::Succeeded {
                return Err(format!(
                    "{task} 现在是「{}」，只能修订已完成任务的结论；还在执行的任务用 janus.reply",
                    t.status.label()
                ));
            }
            t.amendments.push(text.to_string());
            let downstream: Vec<&Task> = st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).collect();
            let running: Vec<(String, String, mpsc::UnboundedSender<Steer>)> = downstream
                .iter()
                .filter_map(|x| {
                    st.live.get(&SessionKey::task(&x.def.id)).map(|tx| (x.def.id.clone(), x.agent.clone(), tx.clone()))
                })
                .collect();
            let done: Vec<String> =
                downstream.iter().filter(|x| x.status == TaskStatus::Succeeded).map(|x| x.def.id.clone()).collect();
            (running, done)
        };
        self.emit(
            run,
            ev("result.amended", "lead", format!("主控修订 {task} 的结论：{}", short(text, 60)))
                .task(task)
                .data(json!({ "text": text })),
        );
        for (id, agent, tx) in &running {
            let msg = self.post_message(
                run,
                "lead",
                agent,
                id,
                &format!("{LEAD_REPLY}上游任务 {task} 的结论被修订，以修订为准：{text}。请据此调整你的工作。"),
                &[],
            );
            let _ = tx.send(Steer::Redirect(msg));
        }
        let mut out = format!("已修订 {task} 的结论。");
        if !running.is_empty() {
            out.push_str(&format!(
                "正在执行的 {} 已收到修订。",
                running.iter().map(|r| r.0.as_str()).collect::<Vec<_>>().join("、")
            ));
        }
        if !done.is_empty() {
            out.push_str(&format!("{} 已经基于旧结论完成，需要的话用 janus.retry 重做。", done.join("、")));
        }
        out.push_str("还没开始的下游任务会在交接时拿到修订。");
        Ok(out)
    }

    /// The lead drops tasks it no longer needs; tasks that depend on them are dropped too.
    /// A running one is stopped.
    pub(super) fn board_cancel(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let reason = args["reason"].as_str().unwrap_or("").trim().to_string();
        let wanted: Vec<String> = args["tasks"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect())
            .unwrap_or_default();
        if wanted.is_empty() {
            return Err("tasks 不能为空".into());
        }
        let (dropped, conns) = {
            let mut st = run.st.lock().unwrap();
            if let Some(missing) = wanted.iter().find(|id| !st.tasks.iter().any(|t| &t.def.id == *id)) {
                return Err(format!("看板上没有任务 {missing}"));
            }
            let mut dropped: Vec<(String, Option<String>)> = vec![];
            let mut queue: Vec<(String, Option<String>)> = wanted.iter().map(|id| (id.clone(), None)).collect();
            while let Some((id, because)) = queue.pop() {
                let t = Run::task_mut(&mut st, &id).unwrap();
                if matches!(t.status, TaskStatus::Succeeded | TaskStatus::SkippedByDecision)
                    || dropped.iter().any(|(d, _)| d == &id)
                {
                    continue;
                }
                t.set_status(TaskStatus::SkippedByDecision);
                t.active_attempt = None;
                t.skipped_because = because.clone();
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
                Some(dep) => format!("{id} 依赖的 {dep} 被主控取消，一并取消"),
                None => format!("主控取消 {id}：{}", if reason.is_empty() { "不再需要" } else { &reason }),
            };
            self.emit(
                run,
                ev("task.skipped_by_decision", "lead", text)
                    .task(id)
                    .data(json!({ "reason": reason, "dependency": because })),
            );
        }
        run.wake.notify_one();
        if dropped.is_empty() {
            return Ok("这些任务都已经结束，没有需要取消的。".into());
        }
        Ok(format!("已取消：{}。", dropped.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join("、")))
    }

    /// The lead hands the person a decision only they can make. It does not wait here: the answer
    /// comes back through `wait`, and the lead keeps working on what does not depend on it.
    pub(super) fn board_ask_user(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let question = args["question"].as_str().unwrap_or("").trim();
        let why = args["why"].as_str().unwrap_or("").trim();
        if question.is_empty() || why.is_empty() {
            return Err("question 和 why 都不能为空：why 写明为什么只有用户能决定、不同答案会怎样影响结果".into());
        }
        let options: Vec<String> = args["options"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let recommend = args["recommend"].as_str().unwrap_or("");
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let key = format!("q-lead-{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        // Nobody awaits the answer here: it reaches the lead through `wait`.
        self.add_pending(run, &key, Owner { session: SessionKey::Lead, attempt: 0 }, Ask::Question)
            .map_err(|e| format!("{e:#}"))?;
        self.emit(run, ev("question.escalated", "lead", format!("主控请你决定：{}", short(question, 60))).to("user").data(json!({
            "key": key, "question": question, "options": options, "recommend": recommend, "reason": why, "from": "lead"
        })));
        Ok("已交给用户决定。用户回答后会出现在 janus.wait 的结果里；在那之前继续做不依赖这个决定的工作。".into())
    }

    /// Everything the board knows about one task, by id: what the board messages and handoffs only
    /// summarise. Its upstream and downstream tasks are named, so a caller can follow the chain
    /// one task at a time.
    // @project-doc docs/interfaces/board_mcp.md#task_detail
    pub(super) fn task_detail(&self, run: &Run, task: &str) -> std::result::Result<String, String> {
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        // The usage ledger is read-only and never a reason to fail: a task without usage still
        // answers, saying 「未上报」.
        let usage = crate::usage::ledger(&events);
        let st = run.st.lock().unwrap();
        let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
            return Err(format!("看板上没有任务 {task}"));
        };
        let name = |agent: &str| run.member(agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.to_string());
        let label = |id: &str| {
            format!(
                "{id}（{}）",
                st.tasks.iter().find(|x| x.def.id == id).map(|x| x.status.label()).unwrap_or("不在看板上")
            )
        };
        let mine = |e: &&Event| e.task.as_deref() == Some(task);
        let mut out = vec![
            format!(
                "任务 {task}「{}」· 执行者 {}（{}）· {} · 已尝试 {} 次",
                t.def.title,
                name(&t.agent),
                t.agent,
                t.status.label(),
                t.attempts
            ),
            format!("说明：{}", t.def.desc),
        ];
        out.extend(usage.task_lines(task, &Self::usage_names(run)));
        out.push(format!(
            "验收标准：\n{}",
            t.def.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n")
        ));
        if !t.def.owns.is_empty() {
            out.push(format!("负责的文件：{}", t.def.owns.join("、")));
        }
        if let Some(v) = &t.def.verifies {
            out.push(format!("独立验证：{v}"));
        }
        let downstream: Vec<String> =
            st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).map(|x| label(&x.def.id)).collect();
        if !t.def.deps.is_empty() {
            out.push(format!("上游：{}", t.def.deps.iter().map(|d| label(d)).collect::<Vec<_>>().join("、")));
        }
        if !downstream.is_empty() {
            out.push(format!("下游：{}", downstream.join("、")));
        }
        if let Some(r) = &t.result {
            out.push(format!("结果：{}", r.summary));
            for c in &r.checks {
                out.push(format!("  {}", c.line()));
            }
            let evidence = Summary::of(&t.def.accept, &r.checks);
            out.push(format!(
                "证据汇总：\n{}",
                evidence.lines().iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")
            ));
            if !r.evidence.is_empty() {
                out.push(format!(
                    "证据：\n{}",
                    r.evidence.iter().map(|x| format!("- {x}")).collect::<Vec<_>>().join("\n")
                ));
            }
            if !r.risks.is_empty() {
                out.push(format!(
                    "风险：\n{}",
                    r.risks.iter().map(|x| format!("- {x}")).collect::<Vec<_>>().join("\n")
                ));
            }
        }
        // Those who verified this task independently: their own conclusion, kept apart from the
        // evidence the executor reported.
        for v in st.tasks.iter().filter(|v| v.def.verifies.as_deref() == Some(task)) {
            out.push(format!("独立验证 {}：{}", v.def.id, v.status.label()));
            if let Some(r) = &v.result {
                out.push(format!("  {}", r.summary));
                for c in &r.checks {
                    out.push(format!("  {}", c.line()));
                }
                if !r.evidence.is_empty() {
                    out.push(format!(
                        "  独立证据：\n{}",
                        r.evidence.iter().map(|x| format!("  - {x}")).collect::<Vec<_>>().join("\n")
                    ));
                }
            }
            if let Some(f) = st.last_verification.get(&v.def.id).filter(|_| v.status != TaskStatus::Succeeded) {
                out.push(format!("  没通过核对：\n{f}"));
            }
        }
        if !t.amendments.is_empty() {
            out.push(format!("主控的修订（以此为准）：{}", t.amendments.join("；")));
        }
        if let Some(v) = st.last_verification.get(task).filter(|_| t.status != TaskStatus::Succeeded) {
            out.push(format!("最近一次没通过验收核对：\n{v}"));
        }
        if let Some(e) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind == "attempt.failed")
            .filter(|_| t.status != TaskStatus::Succeeded)
        {
            out.push(format!(
                "最近一次尝试失败：{}",
                e.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or(&e.text)
            ));
        }
        let files = st.traces.files(task);
        if !files.is_empty() {
            out.push(format!("改过的文件：{}", files.into_iter().collect::<Vec<_>>().join("、")));
        }
        if let Some(s) = t.succeeded_seq.filter(|_| t.status == TaskStatus::Succeeded) {
            let note = crate::traces::stale_note(&st.traces.edited_after(task, s));
            if !note.is_empty() {
                out.push(note);
            }
        }
        if let Some(gaps) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind == "task.succeeded")
            .filter(|_| t.status == TaskStatus::Succeeded)
            .and_then(|e| e.data.as_ref()?["traceGaps"].as_array().cloned())
            .filter(|g| !g.is_empty())
        {
            out.push(format!(
                "痕迹核对（结果和工作目录对不上的地方）：\n{}",
                gaps.iter().filter_map(|g| g.as_str()).map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n")
            ));
        }
        let arts: Vec<String> = t.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
        if !arts.is_empty() {
            out.push(format!("产物：{}", arts.join("、")));
        }
        // A conflict is recorded on the task whose edit found it; both sides see it here.
        let theirs = |e: &&Event| e.kind == "task.conflict" && e.data.as_ref().is_some_and(|d| d["other"] == task);
        let talk: Vec<String> = events
            .iter()
            .filter(|e| mine(e) || theirs(e))
            .filter_map(|e| {
                let mut d = e.data.clone().unwrap_or_default();
                if theirs(&e) {
                    d["other"] = json!(e.task.clone().unwrap_or_default());
                }
                match e.kind.as_str() {
                    "board.posted" => Some(format!(
                        "- [{}] {}",
                        match d["kind"].as_str() {
                            Some("blocker") => "阻塞",
                            Some("note") => "备注",
                            _ => "进展",
                        },
                        d["text"].as_str().unwrap_or(&e.text)
                    )),
                    "question.asked" => Some(format!("- [提问] {}", d["question"].as_str().unwrap_or(&e.text))),
                    "question.answered" => Some(format!("- [回答] {}", d["answer"].as_str().unwrap_or(&e.text))),
                    "task.conflict" if d["owned"] == true && theirs(&e) => Some(format!(
                        "- [越界] {} 改了这个任务负责的 {}",
                        d["other"].as_str().unwrap_or("?"),
                        d["paths"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
                    "task.conflict" if d["owned"] == true => Some(format!(
                        "- [越界] 改了 {} 负责的 {}",
                        d["other"].as_str().unwrap_or("?"),
                        d["paths"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
                    "task.conflict" => Some(format!(
                        "- [冲突] 和 {} 同时改了 {}",
                        d["other"].as_str().unwrap_or("?"),
                        d["paths"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
                    "task.claimed" => Some(format!(
                        "- [认领] {}",
                        d["paths"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    )),
                    _ => None,
                }
            })
            .collect();
        if !talk.is_empty() {
            out.push(format!("看板记录：\n{}", talk.join("\n")));
        }
        Ok(out.join("\n"))
    }

    /// A collaborator's view of the rest of the board: the other tasks, how they relate to its
    /// own (upstream, downstream, the same files) and what they wrote back lately. With `task`,
    /// the full detail of that one.
    // @project-doc docs/interfaces/board_mcp.md#peers
    pub(super) fn board_peers(&self, run: &Run, me: &str, args: &Value) -> std::result::Result<String, String> {
        if let Some(t) = args["task"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            return self.task_detail(run, t);
        }
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
                    tags.push("你的上游".to_string());
                }
                if t.def.deps.iter().any(|d| d == me) {
                    tags.push("依赖你".to_string());
                }
                let theirs = st.traces.files(&t.def.id);
                let common: Vec<String> = theirs.intersection(&my_files).cloned().collect();
                if !t.def.owns.is_empty() && !t.status.terminal() {
                    tags.push(format!("负责 {}", t.def.owns.join("、")));
                }
                if !common.is_empty() {
                    tags.push(format!("和你都改了 {}", common.join("、")));
                } else if t.status == TaskStatus::Running && !theirs.is_empty() {
                    tags.push(format!("改过 {}", theirs.iter().take(5).cloned().collect::<Vec<_>>().join("、")));
                }
                if let Some(s) = t.succeeded_seq.filter(|_| t.status == TaskStatus::Succeeded) {
                    let stale = st.traces.edited_after(&t.def.id, s);
                    if !stale.is_empty() {
                        tags.push(format!(
                            "完成后 {} 又被改过，结论可能已过期",
                            stale.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join("、")
                        ));
                    }
                }
                let who = run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent);
                format!(
                    "- {}「{}」· {who} · {}{}",
                    t.def.id,
                    t.def.title,
                    t.status.label(),
                    if tags.is_empty() { String::new() } else { format!(" · {}", tags.join(" · ")) }
                )
            })
            .collect();
        let posts: Vec<String> = events
            .iter()
            .filter(|e| e.kind == "board.posted" && e.task.as_deref() != Some(me))
            .rev()
            .take(5)
            .map(|e| {
                format!(
                    "- {}：{}",
                    e.task.as_deref().unwrap_or("?"),
                    short(e.data.as_ref().and_then(|d| d["text"].as_str()).unwrap_or(&e.text), INLINE_ITEM)
                )
            })
            .collect();
        Ok(format!(
            "看板上的其他任务：\n{}\n\n最近写回看板的内容：\n{}\n\n要看某个任务的完整说明、结果和修订，传 task 再调用一次。",
            if lines.is_empty() { "（没有）".into() } else { lines.join("\n") },
            if posts.is_empty() { "（没有）".into() } else { posts.join("\n") }
        ))
    }

    /// A collaborator writes progress, a blocker or a note to the board while it works.
    pub(super) fn board_post(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err("text 不能为空".into());
        }
        let kind = match args["kind"].as_str().unwrap_or("progress") {
            k @ ("progress" | "blocker" | "note") => k,
            other => return Err(format!("kind 只能是 progress、blocker 或 note，不能是 {other}")),
        };
        let agent = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .map(|t| t.agent.clone())
            .ok_or_else(|| format!("看板上没有任务 {task}"))?;
        let label = match kind {
            "blocker" => "阻塞",
            "note" => "备注",
            _ => "进展",
        };
        self.emit(
            run,
            ev("board.posted", &agent, format!("{label}：{}", short(text, 80)))
                .task(task)
                .to("lead")
                .data(json!({ "kind": kind, "text": text })),
        );
        Ok("已写到共享看板，主控会看到。".into())
    }

    /// A collaborator claims files it is about to change (`teamwork::claim`); the lead sees it on
    /// the board, and the claim counts as `owns` until the task ends.
    // @project-doc docs/interfaces/board_mcp.md#claim
    pub(super) fn board_claim(&self, run: &Run, task: &str, args: &Value) -> std::result::Result<String, String> {
        let mut errors = vec![];
        let paths = crate::teamwork::read_owns(&args["paths"], "paths", &mut errors);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        if paths.is_empty() {
            return Err("paths 不能为空：写相对工作目录的文件或目录".into());
        }
        let (agent, fresh) = {
            let mut guard = run.st.lock().unwrap();
            let st = &mut *guard;
            let me = st.tasks.iter().find(|t| t.def.id == task).ok_or_else(|| format!("看板上没有任务 {task}"))?;
            let board: Vec<(&TaskDef, TaskStatus)> = st.tasks.iter().map(|t| (&t.def, t.status)).collect();
            let fresh = crate::teamwork::claim(&me.def, &paths, &board)?;
            let agent = me.agent.clone();
            if let Some(t) = Run::task_mut(st, task) {
                t.def.owns.extend(fresh.iter().cloned());
            }
            (agent, fresh)
        };
        if fresh.is_empty() {
            return Ok("这些文件已经由你的任务负责。".into());
        }
        self.emit(
            run,
            ev("task.claimed", &agent, format!("认领 {}", short(&fresh.join("、"), 80)))
                .task(task)
                .to("lead")
                .data(json!({ "paths": fresh })),
        );
        Ok(format!(
            "已认领 {}，到任务结束为止归你负责。其他同时进行的任务改它们时，Janus 会提醒对方和主控。",
            fresh.join("、")
        ))
    }
}

/// Where this Janus listens, for the MCP URLs handed to agents.
pub(super) fn janus_addr() -> String {
    crate::config::settings().addr.clone()
}

// @project-doc docs/interfaces/board_mcp.md#transport
/// The token in one session's MCP URL: derived from the run's secret and the session's `who`, so
/// it only opens the board as that member.
pub(super) fn session_token(run_token: &str, who: &str) -> String {
    Sha256::digest(format!("{run_token}\0{who}").as_bytes())[..16].iter().map(|x| format!("{x:02x}")).collect()
}

pub(super) fn random_token() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        b.copy_from_slice(&Sha256::digest(format!("{}-{}", now_ms(), std::process::id()).as_bytes())[..16]);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
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
        match e.kind.as_str() {
            "task.assigned" => t.agent = e.to.clone().or(t.agent.take()),
            "attempt.started" => {
                t.attempts += 1;
                t.started.get_or_insert(e.ts);
                if t.agent.is_none() {
                    t.agent = e.to.clone();
                }
            }
            "verification.failed" => t.verification_failures += 1,
            "task.succeeded" => {
                t.outcome = Some(true);
                t.ended = Some(e.ts);
            }
            "task.failed" => t.outcome = Some(false),
            // A retry starts the count again: what matters is how the task finally went.
            "task.reopened" => t.outcome = None,
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
