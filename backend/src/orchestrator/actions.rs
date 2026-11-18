//! What the person does to a run: actions, accepting an archived delivery, resuming and recovering
//! after a restart.

use super::*;

impl App {
    /// A person's action on a run, as the HTTP API receives it. With a `commandId` it acts at most
    /// once: the id is claimed before anything happens, a second request with it is a duplicate,
    /// and an action that fails gives the id back so it can be retried. `expectedSequence`, when
    /// given, must still be the run's last event. A run that is not live can be accepted,
    /// resumed, or talked to (which resumes it).
    // @project-doc docs/interfaces/20261031-http_api.md#run_actions
    pub async fn request_action(self: &Arc<Self>, run_id: &str, body: &Value) -> Result<ActionReply, ActionRefused> {
        if self.db.run(run_id).map_err(ActionRefused::Invalid)?.is_none() {
            return Err(ActionRefused::NotFound);
        }
        let command = body["commandId"].as_str().filter(|s| !s.trim().is_empty());
        if let Some(id) = command
            && !self
                .db
                .claim_command(run_id, id, &json!({ "type": body["type"] }), now_ms())
                .map_err(ActionRefused::Invalid)?
        {
            return Ok(ActionReply::Duplicate);
        }
        let done = self.dispatch_action(run_id, body).await;
        if done.is_err()
            && let Some(id) = command
            && let Err(err) = self.db.release_command(run_id, id)
        {
            crate::config::log(format!("janus: could not release command {id} of {run_id}: {err:#}"));
        }
        done.map(|()| ActionReply::Done)
    }

    async fn dispatch_action(self: &Arc<Self>, run_id: &str, body: &Value) -> Result<(), ActionRefused> {
        if let Some(expected) = body["expectedSequence"].as_i64() {
            let actual = self.db.max_seq(run_id).map_err(ActionRefused::Invalid)?;
            if expected != actual {
                return Err(ActionRefused::Conflict(format!(
                    "状态已变化：expectedSequence={expected}，当前为 {actual}"
                )));
            }
        }
        let kind = body["type"].as_str().unwrap_or("");
        let run = match self.run(run_id) {
            // The run is live (a failed driver removes its run, see `launch`): nothing to resume.
            Some(_) if kind == "resumeRun" => {
                return Err(ActionRefused::Conflict("这个任务正在进行，不需要从中断处继续".into()));
            }
            Some(run) => run,
            None => match kind {
                // Acceptance needs no live agents, so it also works for runs delivered before a restart.
                "accept" => return self.accept_archived(run_id).map_err(ActionRefused::Invalid),
                "resumeRun" => return self.resume_run(run_id).await.map(|_| ()).map_err(ActionRefused::Invalid),
                // Talking to an agent brings an interrupted run back first.
                "message" => self.resume_run(run_id).await.map_err(ActionRefused::Invalid)?,
                _ => {
                    return Err(ActionRefused::Conflict(
                        "这个子任务已经中断：先点「从中断处继续」，或者给某个 Agent 发消息".into(),
                    ));
                }
            },
        };
        self.action(&run, body).map_err(ActionRefused::Invalid)
    }

    // @project-doc docs/interfaces/20261031-http_api.md#run_actions
    pub fn action(self: &Arc<Self>, run: &Arc<Run>, a: &Value) -> Result<()> {
        let kind = a["type"].as_str().unwrap_or("");
        let key = a["key"].as_str().unwrap_or("");
        let task = a["task"].as_str().unwrap_or("");
        match kind {
            "answer" => {
                // The answer goes to whoever asked: a task's agent, or the lead (`ask_user`).
                let agent = match self.pending_owner(run, key)?.session {
                    SessionKey::Lead => "lead".to_string(),
                    SessionKey::Task(t) => run
                        .st
                        .lock()
                        .unwrap()
                        .tasks
                        .iter()
                        .find(|x| x.def.id == t)
                        .map(|x| x.agent.clone())
                        .unwrap_or_default(),
                };
                let answer = a["answer"].as_str().unwrap_or("").to_string();
                self.emit(
                    run,
                    ev("question.answered", "user", format!("回答：{answer}"))
                        .task(task)
                        .to(&agent)
                        .data(json!({ "key": key, "answer": answer, "note": a["note"] })),
                );
                self.resolve(run, key, a.clone())?;
                if task.is_empty() {
                    return Ok(());
                }
                self.emit(
                    run,
                    ev("answer.delivered", "janus", format!("通过 ACP 将回答发回 {task} 的会话"))
                        .task(task)
                        .to(&agent)
                        .data(json!({ "key": key })),
                );
            }
            "approve" | "input" => {
                self.pending_owner(run, key)?;
                let agent = run
                    .st
                    .lock()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|t| t.def.id == task)
                    .map(|t| t.agent.clone())
                    .unwrap_or_else(|| a["agent"].as_str().unwrap_or("lead").to_string());
                let (ty, text, value) = if kind == "approve" {
                    let allow = a["allow"].as_bool().unwrap_or(false);
                    let label = a["label"].as_str().filter(|l| !l.is_empty());
                    (
                        if allow { "approval.granted" } else { "approval.denied" },
                        match (label, allow) {
                            (Some(l), _) => format!("选择：{l}"),
                            (None, true) => "批准执行这一次".into(),
                            (None, false) => "拒绝执行".into(),
                        },
                        json!({ "allow": allow, "option": a["option"] }),
                    )
                } else {
                    // Anything but accept or decline is a cancel, for the agent and the record alike.
                    let action = a["action"].as_str().filter(|x| ["accept", "decline"].contains(x)).unwrap_or("cancel");
                    let content = if a["content"].is_object() { a["content"].clone() } else { json!({}) };
                    (
                        "input.answered",
                        match action {
                            "accept" => "提交了填写的内容".into(),
                            "decline" => "拒绝填写".into(),
                            _ => "取消了这个请求".into(),
                        },
                        json!({ "action": action, "content": content }),
                    )
                };
                let mut e = ev(ty, "user", text).to(&agent).data(json!({ "key": key, "option": a["option"], "action": value["action"], "content": value["content"] }));
                if !task.is_empty() {
                    e = e.task(task);
                }
                self.emit(run, e);
                self.resolve(run, key, value)?;
            }
            "pause" | "resume" => {
                let pause = kind == "pause";
                {
                    let mut st = run.st.lock().unwrap();
                    // Pausing or resuming a finished Run would leave `paused` set with nothing to drive.
                    if !st.status.is_active() {
                        bail!("任务现在是「{}」，不能{}", st.status, if pause { "暂停" } else { "继续" });
                    }
                    st.paused = pause;
                }
                self.emit(
                    run,
                    ev(
                        if pause { "run.paused" } else { "run.resumed" },
                        "user",
                        if pause {
                            "暂停任务：进行中的尝试会继续完成，不再启动新子任务"
                        } else {
                            "继续任务"
                        },
                    ),
                );
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "extendBudget" => {
                let status = run.st.lock().unwrap().status;
                if !status.is_active() {
                    bail!("任务现在是「{status}」，不能追加预算");
                }
                let hit = self.check_budget(run);
                let explicit_tokens = a["tokens"].as_i64();
                let explicit_cost = a["costUsd"].as_f64();
                let explicit_minutes = a["minutes"].as_i64();
                let default_hit = if explicit_tokens.is_none() && explicit_cost.is_none() && explicit_minutes.is_none()
                {
                    hit
                } else {
                    None
                };
                if let Some(tokens) =
                    explicit_tokens.or_else(|| matches!(default_hit, Some(BudgetHit::Tokens { .. })).then_some(500_000))
                {
                    let added = tokens.clamp(1_000, 100_000_000);
                    let used = self.usage(run).run.tokens.total.unwrap_or(0);
                    let new_limit = run.token_limit.load(Ordering::SeqCst).max(used) + added;
                    run.token_limit.store(new_limit, Ordering::SeqCst);
                    self.emit(
                        run,
                        ev("run.budget_extended", "user", format!("追加 Token 预算 {}", crate::usage::num(added)))
                            .data(json!({ "kind": "tokens", "tokens": added, "tokenLimit": new_limit })),
                    );
                } else if let Some(cost_usd) =
                    explicit_cost.or_else(|| matches!(default_hit, Some(BudgetHit::CostUsd { .. })).then_some(5.0))
                {
                    let added = cost_usd.clamp(0.01, 10_000.0);
                    let added_micros = (added * 1_000_000.0).round() as i64;
                    let used_micros = self
                        .usage(run)
                        .run
                        .cost
                        .lines
                        .iter()
                        .find(|l| l.currency.eq_ignore_ascii_case("USD"))
                        .map(|l| (l.amount * 1_000_000.0).round() as i64)
                        .unwrap_or(0);
                    let new_micros = run.cost_limit_micros.load(Ordering::SeqCst).max(used_micros) + added_micros;
                    run.cost_limit_micros.store(new_micros, Ordering::SeqCst);
                    let new_usd = new_micros as f64 / 1_000_000.0;
                    self.emit(
                        run,
                        ev("run.budget_extended", "user", format!("追加费用预算 {added:.2} USD"))
                            .data(json!({ "kind": "cost_usd", "costUsd": added, "costLimitUsd": new_usd })),
                    );
                } else {
                    let minutes = explicit_minutes.unwrap_or(60).clamp(1, 24 * 60);
                    let added = minutes * 60 * 1000;
                    // Counted from now when the budget already ran out, so one extension is enough.
                    let deadline = run.deadline_ms.load(Ordering::SeqCst).max(now_ms()) + added;
                    run.deadline_ms.store(deadline, Ordering::SeqCst);
                    self.emit(
                        run,
                        ev("run.budget_extended", "user", format!("追加任务预算 {minutes} 分钟"))
                            .data(json!({ "kind": "wall_clock", "minutes": minutes, "addedMs": added })),
                    );
                }
                if status == RunStatus::WaitingInput {
                    self.set_status(run, RunStatus::Running);
                }
                self.ensure_driver(run);
                run.wake.notify_one();
            }
            "cancel" => {
                let status = run.st.lock().unwrap().status;
                if !matches!(
                    status,
                    RunStatus::Planning | RunStatus::Running | RunStatus::WaitingInput | RunStatus::Delivered
                ) {
                    bail!("任务现在是「{status}」，不能停止");
                }
                let conns: Vec<Arc<AcpConn>> = {
                    let mut st = run.st.lock().unwrap();
                    st.pending.clear();
                    st.live.clear();
                    st.conns.drain().map(|(_, c)| c).collect()
                };
                for c in conns {
                    tokio::spawn(async move { c.kill().await });
                }
                self.emit(run, ev("run.cancelled", "user", "取消这个目标"));
                self.set_status(run, RunStatus::Cancelled);
                run.board_wake.notify_waiters();
                run.lead_nudge.notify_one();
                run.wake.notify_one();
            }
            "message" | "interrupt" | "takeover" | "release" | "finish" | "control" => {
                let agent = a["agent"].as_str().unwrap_or("");
                let cmd = match kind {
                    "message" => {
                        let text = a["text"].as_str().unwrap_or("").trim().to_string();
                        let images = a["images"].as_array().cloned().unwrap_or_default();
                        if text.is_empty() && images.is_empty() {
                            bail!("消息不能为空");
                        }
                        // Pasted images are stored with the run; the text names each one.
                        let images = crate::attachments::save(&self.cfg.data_dir, &run.id, &images)?;
                        let text = crate::attachments::with_lines(&text, &images);
                        // Shown the moment it is sent; its session routes it once it can.
                        let to = if agent.is_empty() {
                            run.st
                                .lock()
                                .unwrap()
                                .tasks
                                .iter()
                                .find(|t| t.def.id == task)
                                .map(|t| t.agent.clone())
                                .unwrap_or_default()
                        } else {
                            agent.to_string()
                        };
                        Steer::Message(self.post_message(run, "user", &to, task, &text, &images))
                    }
                    "interrupt" => Steer::Interrupt,
                    "control" => {
                        let id = a["id"].as_str().unwrap_or("").trim();
                        if id.is_empty() || a["value"].is_null() {
                            bail!("切换会话选项要给出 id 和 value");
                        }
                        Steer::Control(SessionControl {
                            id: id.to_string(),
                            value: a["value"].clone(),
                            via: ControlVia::parse(a["via"].as_str().unwrap_or_default()),
                        })
                    }
                    "takeover" => Steer::TakeOver,
                    "release" => Steer::Release,
                    _ => Steer::Finish,
                };
                let tx = {
                    let st = run.st.lock().unwrap();
                    let key =
                        if task.is_empty() { st.live_agent.get(agent).cloned() } else { Some(SessionKey::task(task)) };
                    key.and_then(|k| st.live.get(&k).cloned())
                };
                match (tx, cmd) {
                    (Some(tx), cmd) => tx.send(cmd).map_err(|_| anyhow!("这个会话已经结束"))?,
                    // No open session: keep the message for this agent's next one.
                    (None, Steer::Message(m)) if !agent.is_empty() => {
                        self.route_message(run, agent, "", 0, &m.id, Delivery::Next);
                        run.st.lock().unwrap().notes.entry(agent.to_string()).or_default().push(m);
                    }
                    (None, _) => bail!("{} 现在没有打开的会话", if agent.is_empty() { task } else { agent }),
                }
            }
            "accept" => {
                // Only a delivered run has results to accept; accepting earlier would be undone
                // by the scheduler the moment it next ran.
                let status = run.st.lock().unwrap().status;
                if status != RunStatus::Delivered {
                    bail!("任务现在是「{status}」，只有已交付的任务 可以验收");
                }
                self.emit(run, ev("risk.accepted", "user", "核对证据后验收通过"));
                self.set_status(run, RunStatus::Accepted);
            }
            _ => bail!("未知操作：{kind}"),
        }
        Ok(())
    }

    pub fn accept_archived(&self, run_id: &str) -> Result<()> {
        let status = self.db.run(run_id)?.map(|r| r["status"].clone()).context("run not found")?;
        if status.as_str() != Some("delivered") {
            bail!("只有已交付的任务 可以验收");
        }
        let seq = self.db.max_seq(run_id)? + 1;
        self.store(Event {
            run_id: run_id.into(),
            seq,
            ts: now_ms(),
            kind: "risk.accepted".into(),
            actor: "user".into(),
            to: None,
            task: None,
            attempt: None,
            text: "核对证据后验收通过".into(),
            data: None,
        });
        self.db.set_run_status(run_id, RunStatus::Accepted)
    }

    /// After a restart nothing is running: close orphaned attempts and mark their runs interrupted.
    /// Bring an interrupted run back from its event log: finished tasks stay finished, unfinished
    /// ones start again (reopening their old session when possible), and planning resumes where it was.
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    pub async fn resume_run(self: &Arc<Self>, run_id: &str) -> Result<Arc<Run>> {
        if let Some(run) = self.run(run_id) {
            return Ok(run);
        }
        let row = self.db.run(run_id)?.context("run not found")?;
        let previous = row["status"].as_str().and_then(RunStatus::parse);
        // Delivered and accepted runs reopen as a conversation; interrupted and failed runs
        // rebuild the board and continue the goal.
        let conversation = matches!(previous, Some(RunStatus::Delivered | RunStatus::Accepted));
        if !matches!(
            previous,
            Some(RunStatus::Interrupted | RunStatus::Failed | RunStatus::Delivered | RunStatus::Accepted)
        ) {
            bail!("只有中断、失败、已交付或已验收的子任务可以继续");
        }
        let events = self.db.events(run_id, 0)?;
        let config = &row["config"];
        let mut team: Vec<Member> =
            serde_json::from_value(config["team"].clone()).context("这个子任务的团队记录无法读取")?;
        for m in team.iter_mut() {
            // A conversation only needs the lead. A collaborator's adapter may have changed or
            // been removed since the run; its task will report that if new work uses it.
            if conversation && !m.lead {
                continue;
            }
            self.spec_for(&m.runtime)?;
        }
        let base_ref = config["baseRef"].as_str().unwrap_or("").to_string();
        let mut token_limit = config["tokenLimit"].as_i64().unwrap_or(0);
        let mut cost_limit_micros =
            config["costLimitUsd"].as_f64().map(|c| (c * 1_000_000.0).round() as i64).unwrap_or(0);
        for e in events.iter().filter(|e| e.kind == "run.budget_extended") {
            let Some(d) = e.data.as_ref() else { continue };
            if let Some(tl) = d["tokenLimit"].as_i64() {
                token_limit = tl;
            }
            if let Some(cl) = d["costLimitUsd"].as_f64() {
                cost_limit_micros = (cl * 1_000_000.0).round() as i64;
            }
        }
        let owner = format!("janus-{}", std::process::id());
        if self.db.acquire_lease(run_id, "run", &owner, now_ms(), 10 * 60 * 1000)?.is_none() {
            bail!("任务已被另一个 Janus 实例接管");
        }
        let run = Arc::new(Run::new(RunRecord {
            id: run_id.to_string(),
            goal: row["goal"].as_str().unwrap_or("").to_string(),
            cwd: PathBuf::from(row["cwd"].as_str().unwrap_or("")),
            team,
            git: config["git"] == true && !base_ref.is_empty(),
            base_ref,
            brief: config["workspace"]["brief"].as_str().unwrap_or("").trim().to_string(),
            goal_images: config["goalImages"].as_array().cloned().unwrap_or_default(),
            last_seq: self.db.max_seq(run_id)?,
            status: if conversation { previous.unwrap_or(RunStatus::Delivered) } else { RunStatus::Running },
            token_limit,
            cost_limit_micros,
        }));

        // The board's tasks are every orchestration the lead got accepted.
        // Each with the last event before its orchestration: what the lead knew when it planned it.
        let board: Vec<(i64, Value)> = events
            .iter()
            .filter(|e| e.kind == "orchestration.accepted")
            .filter_map(|e| e.data.as_ref().and_then(|d| d["tasks"].as_array()).map(|t| (e.seq - 1, t.clone())))
            .flat_map(|(planned, tasks)| tasks.into_iter().map(move |t| (planned, t)))
            .collect();
        let (mut done, mut open) = (0, 0);
        let mut tasks = vec![];
        for (planned, item) in board {
            let mut def: TaskDef = serde_json::from_value(item.clone())?;
            def.assignee = item["agent"].as_str().unwrap_or("").to_string();
            let id = def.id.clone();
            // The lead may have retried it with new instructions, acceptance criteria, or session continuation.
            let mut last_reopen_seq = 0i64;
            let mut continue_session = false;
            for e in events.iter().filter(|e| e.kind == "task.reopened" && e.task.as_deref() == Some(id.as_str())) {
                let d = e.data.clone().unwrap_or_default();
                if let Some(i) = d["instructions"].as_str() {
                    def.desc = i.to_string();
                }
                if let Some(a) = d["accept"].as_array() {
                    def.accept = a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                }
                last_reopen_seq = e.seq;
                continue_session = d["continueSession"] == true;
            }
            let agent = def.assignee.clone();
            let mine = |kind: &str| -> Vec<&Event> {
                events.iter().filter(|e| e.kind == kind && e.task.as_deref() == Some(id.as_str())).collect()
            };
            if mine("attempt.started").iter().any(|e| e.seq > last_reopen_seq) {
                continue_session = false;
            }
            let attempts = mine("attempt.started").len() as i64;
            // The current retry budget starts at the last user retry.
            let retry_base = events
                .iter()
                .rposition(|e| e.kind == "task.reopened" && e.task.as_deref() == Some(id.as_str()))
                .map(|j| {
                    events[..j]
                        .iter()
                        .filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id.as_str()))
                        .count() as i64
                })
                .unwrap_or(0);
            // Only an attempt that was still open when the orchestrator stopped is reloaded;
            // a finished one (failed or succeeded) starts a fresh session.
            let interrupted_attempt = events
                .iter()
                .rposition(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(id.as_str()))
                .is_some_and(|j| {
                    !events[j + 1..].iter().any(|e| {
                        e.task.as_deref() == Some(id.as_str())
                            && matches!(
                                e.kind.as_str(),
                                "attempt.failed" | "task.succeeded" | "task.reopened" | "attempt.late_result"
                            )
                    })
                });
            let succeeded = events
                .iter()
                .rfind(|e| {
                    e.task.as_deref() == Some(id.as_str())
                        && matches!(e.kind.as_str(), "task.succeeded" | "task.reopened")
                })
                .filter(|e| e.kind == "task.succeeded");
            // A finished task's conclusion carries its whole result; an unfinished one has the last
            // result it submitted.
            let concluded = succeeded.and_then(|e| e.data.clone());
            let result = concluded
                .as_ref()
                .or_else(|| mine("result.submitted").last().and_then(|e| e.data.as_ref()))
                .map(result_from_event);
            let trace_gaps: Vec<String> = concluded
                .as_ref()
                .and_then(|d| d["traceGaps"].as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let artifacts: Vec<Value> = mine("artifact.created").iter().filter_map(|e| e.data.clone()).collect();
            let last_session = mine("attempt.started")
                .last()
                .and_then(|e| e.data.as_ref().and_then(|d| d["session"].as_str().map(str::to_owned)));
            let last_task_event = events
                .iter()
                .filter(|e| e.task.as_deref() == Some(id.as_str()))
                .rfind(|e| {
                    matches!(
                        e.kind.as_str(),
                        "task.succeeded"
                            | "task.failed"
                            | "task.blocked"
                            | "task.skipped_by_decision"
                            | "task.reopened"
                    )
                })
                .map(|e| e.kind.as_str());
            // Everything unfinished restarts from `pending`: the sessions and open requests of a
            // restart are gone, so a task that was waiting on a person cannot stay waiting.
            let status = match last_task_event {
                Some("task.succeeded") => TaskStatus::Succeeded,
                Some("task.failed") => TaskStatus::Failed,
                Some("task.blocked") => TaskStatus::Blocked,
                Some("task.skipped_by_decision") => TaskStatus::SkippedByDecision,
                _ => TaskStatus::Pending,
            };
            // What a blocked task waits on: its failed dependency.
            let blocked_by = events
                .iter()
                .rev()
                .find(|e| e.task.as_deref() == Some(id.as_str()) && e.kind == "task.blocked")
                .and_then(|e| e.data.as_ref())
                .and_then(|d| d["dependency"].as_str())
                .map(str::to_owned);
            if status == TaskStatus::Succeeded {
                done += 1;
            } else {
                open += 1;
                if status == TaskStatus::Pending
                    && interrupted_attempt
                    && let Some(sid) = last_session
                {
                    run.st.lock().unwrap().resume_session.insert(id.clone(), sid);
                }
            }
            let mut task = Task::new(def, agent, planned);
            // Rebuilding the board, not a transition: the event log already decided this state.
            task.status = status;
            task.attempts = attempts;
            task.result = result;
            task.blocked_by = blocked_by;
            task.skipped_because = (status == TaskStatus::SkippedByDecision)
                .then(|| {
                    mine("task.skipped_by_decision")
                        .last()
                        .and_then(|e| e.data.as_ref()?["dependency"].as_str().map(str::to_owned))
                })
                .flatten();
            task.succeeded_seq = succeeded.map(|e| e.seq);
            task.trace_gaps = trace_gaps;
            task.continue_session = continue_session;
            task.artifacts = artifacts;
            task.retry_base = retry_base;
            let since = events
                .iter()
                .rposition(|e| e.kind == "task.reopened" && e.task.as_deref() == Some(id.as_str()))
                .map(|j| events[j].seq)
                .unwrap_or(0);
            task.amendments = mine("result.amended")
                .iter()
                .filter(|e| e.seq > since)
                .filter_map(|e| e.data.as_ref().and_then(|d| d["text"].as_str().map(str::to_owned)))
                .collect();
            for p in mine("task.claimed")
                .iter()
                .filter_map(|e| e.data.as_ref()?["paths"].as_array().cloned())
                .flatten()
                .filter_map(|p| p.as_str().map(str::to_owned))
            {
                if !task.def.owns.contains(&p) {
                    task.def.owns.push(p);
                }
            }
            tasks.push(task);
        }
        run.st.lock().unwrap().tasks = tasks;
        // What the tasks left in the workspace and what was already reported about it, so peers,
        // conflicts and the out-of-date marks carry on where they were.
        {
            let mut st = run.st.lock().unwrap();
            for e in &events {
                let Some(d) = e.data.as_ref() else { continue };
                let paths: Vec<String> =
                    d["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
                match (e.kind.as_str(), e.task.as_deref()) {
                    ("tool.call", Some(task)) if !paths.is_empty() => st.traces.record(task, &paths, e.seq),
                    ("tool.call", None) if e.actor == "lead" && !paths.is_empty() => {
                        st.traces.record_lead(&paths, e.seq);
                    }
                    ("task.conflict", Some(task)) => {
                        let kind = if d["owned"] == true { Overlap::Owned } else { Overlap::Same };
                        if d["byLead"] == true {
                            for p in &paths {
                                st.traces.first_actor_report(
                                    kind,
                                    crate::traces::Actor::Lead,
                                    crate::traces::Actor::task(task),
                                    p,
                                );
                            }
                        } else {
                            let other = d["other"].as_str().unwrap_or_default();
                            for p in &paths {
                                st.traces.first_report(kind, task, other, p);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // Messages that were sent, queued or left for an agent but never reached it survive the
        // restart: every message not delivered and not joined into a turn (steer / prompt).
        {
            let mut st = run.st.lock().unwrap();
            for (i, e) in events.iter().enumerate() {
                let Some(agent) = e.to.as_deref() else {
                    continue;
                };
                let d = e.data.clone().unwrap_or_default();
                if e.kind != "user.message" {
                    continue;
                }
                let Some(id) = d["id"].as_str() else { continue };
                let later = &events[i + 1..];
                let routed = later
                    .iter()
                    .rev()
                    .find(|x| x.kind == "message.routed" && x.data.as_ref().is_some_and(|x| x["id"] == id));
                let joined = routed
                    .is_some_and(|x| x.data.as_ref().unwrap()["delivery"].as_str().is_some_and(Delivery::reached_turn));
                let delivered = later.iter().any(|x| {
                    x.kind == "message.delivered"
                        && x.data
                            .as_ref()
                            .is_some_and(|x| x["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id)))
                });
                if !joined && !delivered {
                    let text = d["text"].as_str().unwrap_or_default().to_string();
                    st.notes.entry(agent.to_string()).or_default().push(Msg { id: id.to_string(), text });
                }
            }
        }
        self.runs.lock().unwrap().insert(run_id.to_string(), run.clone());
        if conversation {
            // No scheduler and no automatic goal: the lead session comes back for a regular
            // conversation, and the message that asked for it is queued as a note.
            let app = self.clone();
            let r = run.clone();
            tokio::spawn(async move {
                if let Err(err) = app.lead_conversation(&r).await {
                    crate::config::log(format!("janus: chat {} ended: {err:#}", r.id));
                }
            });
            return Ok(run);
        }
        self.set_status(&run, RunStatus::Running);
        // What each task came back as, so the app rebuilds the same state.
        let restored: Vec<Value> = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .map(|t| json!({ "id": t.def.id, "status": t.status, "blockedBy": t.blocked_by }))
            .collect();
        self.emit(
            &run,
            ev(
                "run.recovered",
                "user",
                format!("从中断处继续：看板上 {done} 个子任务已完成，{open} 个子任务重新开始，负责人重新推进目标"),
            )
            .data(json!({ "phase": "running", "done": done, "open": open, "tasks": restored })),
        );
        self.launch(run.clone(), Phase::Goal { resumed: true });
        Ok(run)
    }

    pub fn recover(&self) -> Result<()> {
        for (run_id, task, n) in self.db.orphaned_attempts()? {
            self.db.update_attempt(
                &run_id,
                &task,
                n,
                AttemptStatus::Interrupted,
                AttemptUpdate { error: Some("编排器重启"), ended: Some(now_ms()), ..Default::default() },
            )?;
        }
        for run_id in self.db.unfinished_runs()? {
            let seq = self.db.max_seq(&run_id)? + 1;
            self.store(Event {
                run_id: run_id.clone(),
                seq,
                ts: now_ms(),
                kind: "run.interrupted".into(),
                actor: "janus".into(),
                to: None,
                task: None,
                attempt: None,
                text: "编排器重启，进行中的 Agent 会话已关闭。可以从中断处继续，或者直接给某个 Agent 发消息".into(),
                data: None,
            });
            self.db.set_run_status(&run_id, RunStatus::Interrupted)?;
        }
        Ok(())
    }
}

/// What a request to act on a run came to.
pub enum ActionReply {
    Done,
    /// A request with a `commandId` that was already claimed: nothing was done again.
    Duplicate,
}

/// Why a request to act on a run was turned down.
pub enum ActionRefused {
    NotFound,
    /// The run's state does not allow it now.
    Conflict(String),
    Invalid(anyhow::Error),
}

/// Why a request to delete a run was turned down.
#[derive(Debug)]
pub enum DeleteRefused {
    NotFound,
    /// The run is still working; stop it first.
    Conflict(String),
    Invalid(anyhow::Error),
}
