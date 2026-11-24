//! What the person does to a run: actions, accepting an archived delivery, resuming and recovering
//! after a restart.

use super::*;

impl App {
    /// A person's action on a run, as the HTTP API receives it. With a `commandId` it acts at most
    /// once: the id is claimed before anything happens and marked done once the action took
    /// effect. A later request with the id is a duplicate when the action is done, and refused
    /// while it is still running or when a restart cut it off, because then nobody knows yet
    /// whether it took effect. An action that fails gives the id back so it can be retried.
    /// `expectedSequence`, when given, must still be the run's last event. A run that is not live
    /// can be accepted, resumed, or talked to (which resumes it).
    // @project-doc docs/interfaces/20261031-http_api.md#run_actions
    pub async fn request_action(self: &Arc<Self>, run_id: &str, body: &Value) -> Result<ActionReply, Refused> {
        if self.db.run(run_id)?.is_none() {
            return Err(Refused::NotFound);
        }
        let command = body["commandId"].as_str().filter(|s| !s.trim().is_empty());
        if let Some(id) = command {
            match self.db.claim_command(run_id, id, &json!({ "type": body["type"] }), now_ms())? {
                None => {}
                Some(CommandState::Done) => return Ok(ActionReply::Duplicate),
                Some(CommandState::Running) => {
                    return Err(Refused::Conflict(format!("commandId {id} 的操作还在执行，结束前无法确认是否生效")));
                }
                Some(CommandState::Interrupted) => {
                    return Err(Refused::Conflict(format!(
                        "commandId {id} 的操作在服务重启时中断，无法确认是否生效：先查看任务的事件，需要时换一个 commandId 重试"
                    )));
                }
            }
        }
        let done = match self.dispatch_action(run_id, body).await {
            // Accepting stopped on something in the run's worktree (the branch's new commits
            // conflict, an operation left unfinished there): the lead resolves that, not the person.
            Err(Refused::Invalid(err)) => match merge_handback(&err, MergeAt::Acceptance) {
                Some(prompt) => self.hand_back_to_lead(run_id, &prompt).await,
                None => Err(Refused::Invalid(err)),
            },
            done => done,
        };
        if let Some(id) = command {
            let recorded =
                if done.is_ok() { self.db.finish_command(run_id, id) } else { self.db.release_command(run_id, id) };
            if let Err(err) = recorded {
                crate::config::log(format!("janus: could not record command {id} of {run_id}: {err:#}"));
            }
        }
        done.map(|()| ActionReply::Done)
    }

    /// Accepting stopped on something in the run's worktree: the branch the run goes back to moved
    /// on with commits that conflict with the run's, or an operation is left unfinished there.
    /// Nothing changed on the person's side, so the run stays delivered and its lead resolves it
    /// (`hand_merge_to_lead`).
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
    async fn hand_back_to_lead(self: &Arc<Self>, run_id: &str, prompt: &str) -> Result<(), Refused> {
        let run = self.resume_run(run_id).await?;
        self.hand_merge_to_lead(&run, prompt)?;
        Ok(())
    }

    /// Bringing the result back stopped on something in the worktree: the lead resolves it there
    /// (`prompt` says how) and tells the person how, and that turn delivers the run again, which
    /// merges. `lead.merge_handed_back` keeps one that did not resolve it from being handed the
    /// same thing again by its own delivery.
    pub(super) fn hand_merge_to_lead(&self, run: &Run, prompt: &str) -> Result<()> {
        {
            let mut st = run.st.lock().unwrap();
            st.lead.redeliver = true;
            st.lead.merge_handed_back = true;
        }
        let m = self.post_message(run, "janus", "lead", "", prompt, &[]);
        self.send_steer(run, "lead", "", Steer::Message(m))
    }

    async fn dispatch_action(self: &Arc<Self>, run_id: &str, body: &Value) -> Result<(), Refused> {
        if let Some(expected) = body["expectedSequence"].as_i64() {
            let actual = self.db.max_seq(run_id)?;
            if expected != actual {
                return Err(Refused::Conflict(format!("状态已变化：expectedSequence={expected}，当前为 {actual}")));
            }
        }
        let kind = body["type"].as_str().unwrap_or("");
        let run = match self.run(run_id) {
            // The run is live (a failed driver removes its run, see `launch`): nothing to resume.
            Some(_) if kind == "resumeRun" => {
                return Err(Refused::Conflict("这个任务正在进行，不需要从中断处继续".into()));
            }
            Some(run) => run,
            None => {
                let row = self.db.run(run_id)?;
                let stored = row.as_ref().and_then(|r| RunStatus::parse(r["status"].as_str()?));
                let run_kind = row.as_ref().and_then(|r| RunKind::parse(r["kind"].as_str()?)).unwrap_or_default();
                match stored {
                    // A queued run has nothing live yet: it can only start now or be cancelled.
                    Some(RunStatus::Queued) => {
                        return match kind {
                            "start" => self.start_now(run_id),
                            "cancel" => self.cancel_queued(run_id),
                            _ => Err(Refused::Conflict("这个任务还在排队：可以立即开始，或者取消".into())),
                        };
                    }
                    // One that could not start has no snapshot to continue from: it queues again.
                    Some(RunStatus::Failed) if self.never_started(run_id)? => {
                        return match kind {
                            "resumeRun" => {
                                self.requeue(run_id, "重新排队：上次没能开始")?;
                                self.pump();
                                Ok(())
                            }
                            _ => Err(Refused::Conflict("这个任务没能开始：点「从中断处继续」重新排队".into())),
                        };
                    }
                    // It just left the queue and is being set up.
                    Some(RunStatus::Planning) if run_kind == RunKind::Goal => {
                        return Err(Refused::Conflict("这个任务正在开始，稍后再试".into()));
                    }
                    _ => match kind {
                        // Acceptance needs no live agents, so it also works for runs delivered before a restart.
                        "accept" => match self.accept_archived(run_id).await? {
                            None => return Ok(()),
                            // A message brought it back meanwhile: it is accepted as a live run.
                            Some(run) => run,
                        },
                        "resumeRun" => return Ok(self.resume_run(run_id).await.map(|_| ())?),
                        // Talking to an agent brings an interrupted run back first, and so does
                        // answering a Loop's question: they outlive its session.
                        "message" => self.resume_run(run_id).await?,
                        "answer" if run_kind == RunKind::Loop => self.resume_run(run_id).await?,
                        _ => {
                            return Err(Refused::Conflict(
                                "这个子任务已经中断：先点「从中断处继续」，或者给某个 Agent 发消息".into(),
                            ));
                        }
                    },
                }
            }
        };
        Ok(self.action(&run, body)?)
    }

    /// Hand `cmd` to the open session of `agent` (or of `task`); a message for an agent without an
    /// open session waits for its next one.
    pub(super) fn send_steer(&self, run: &Run, agent: &str, task: &str, cmd: Steer) -> Result<()> {
        // Choosing the open session and leaving the message for the next one happen under one
        // lock, the one a session registers under: a session that opens meanwhile either is
        // chosen, or finds the message among the notes it takes when it starts.
        let left = {
            let mut st = run.st.lock().unwrap();
            let key = if task.is_empty() { st.live_agent.get(agent).cloned() } else { Some(SessionKey::task(task)) };
            match (key.and_then(|k| st.live.get(&k).cloned()), cmd) {
                (Some(tx), cmd) => {
                    tx.send(cmd).map_err(|_| anyhow!("这个会话已经结束"))?;
                    None
                }
                // No open session: keep the message for this agent's next one.
                (None, Steer::Message(m)) if !agent.is_empty() => {
                    let id = m.id.clone();
                    st.notes.entry(agent.to_string()).or_default().push(Note::new(m, task));
                    Some(id)
                }
                (None, _) => bail!("{} 现在没有打开的会话", if agent.is_empty() { task } else { agent }),
            }
        };
        if let Some(id) = left {
            self.route_message(run, agent, "", 0, &id, Delivery::Next);
        }
        Ok(())
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
                    ev(EventKind::QuestionAnswered, "user", format!("回答：{answer}"))
                        .task(task)
                        .to(&agent)
                        .data(json!({ "key": key, "answer": answer, "note": a["note"] })),
                );
                self.resolve(run, key, a.clone())?;
                if task.is_empty() {
                    // A Loop does not wait for the answer to its question: the answer wakes it.
                    if run.kind == RunKind::Loop
                        && let Some(id) = self.db.run(&run.id)?.and_then(|r| r["loopId"].as_str().map(str::to_owned))
                    {
                        self.wake_loop_later(&id, WakeReason::Answer);
                    }
                    return Ok(());
                }
                self.emit(
                    run,
                    ev(EventKind::AnswerDelivered, "janus", format!("通过 ACP 将回答发回 {task} 的会话"))
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
                        if allow { EventKind::ApprovalGranted } else { EventKind::ApprovalDenied },
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
                        EventKind::InputAnswered,
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
            // A Loop's clock is paused on the Loop, and its session is stopped by deleting the Loop.
            "pause" | "resume" | "cancel" if run.kind == RunKind::Loop => {
                bail!("值班在值班页暂停或删除，不能当作任务暂停或停止");
            }
            "pause" | "resume" => {
                let pause = kind == "pause";
                {
                    run.st.lock().unwrap().set_paused(pause)?;
                }
                self.emit(
                    run,
                    ev(
                        if pause { EventKind::RunPaused } else { EventKind::RunResumed },
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
                // A paused run gives its slot to the queue; resuming takes one again.
                self.pump();
            }
            "cancel" => {
                // A delivery already put its result on the person's branch (or in their folder):
                // stopping would only end the lead's conversation and call the run cancelled.
                // @project-doc docs/domains/20261031-run_lifecycle.md#user_actions
                if self.last_delivery_seq(&run.id) > 0 {
                    bail!(
                        "这个任务已经交付过，结果已经在你的分支或目录里，停止撤不回它。要停下负责人这一轮，就打断它；不想要这个结果，让负责人回退。"
                    );
                }
                let status = run.st.lock().unwrap().status;
                if !matches!(status, RunStatus::Planning | RunStatus::Running) {
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
                self.emit(run, ev(EventKind::RunCancelled, "user", "取消这个目标"));
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
                        // Pasted images are stored with the run; the text names each one. A message
                        // refused here (an image that is not one) does not title a chat.
                        let images = crate::attachments::save(&self.dirs(run), &images)?;
                        self.title_chat(run, &text)?;
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
                        let value = &a["value"];
                        if id.is_empty() || value.is_null() || value.as_str().is_some_and(|v| v.trim().is_empty()) {
                            bail!("切换会话选项要给出 id 和 value");
                        }
                        Steer::Control(SessionControl {
                            id: id.to_string(),
                            value: a["value"].clone(),
                            via: ControlVia::of(a["via"].as_str().unwrap_or_default()),
                        })
                    }
                    "takeover" => Steer::TakeOver,
                    "release" => Steer::Release,
                    _ => Steer::Finish,
                };
                self.send_steer(run, agent, task, cmd)?;
            }
            "accept" => {
                // Only a delivered run has results to accept; accepting earlier would be undone
                // by the scheduler the moment it next ran.
                let (status, busy, asking) = {
                    let st = run.st.lock().unwrap();
                    (st.status, st.lead.in_turn, !st.pending.is_empty())
                };
                if status != RunStatus::Delivered {
                    bail!("任务现在是「{status}」，只有已交付的任务 可以验收");
                }
                // A delivery the lead is still working on, or one that waits on the person's
                // answer, is not the result to accept yet.
                // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
                if busy {
                    bail!("负责人还在处理你交代的事，等它这一轮结束后再验收");
                }
                if asking {
                    bail!("还有等你回答的请求，先处理它们再验收");
                }
                // The lead's conversation stays open and works in the worktree, so it stays; the
                // next start removes it once nothing works there (`recover`).
                let (text, data) =
                    self.accept_worktree(&run.id, run.worktree.as_ref(), &run.base_ref, &run.goal, false)?;
                self.emit(run, ev(EventKind::RiskAccepted, "user", text).data(data));
                self.set_status(run, RunStatus::Accepted);
            }
            _ => bail!("未知操作：{kind}"),
        }
        Ok(())
    }

    pub async fn accept_archived(&self, run_id: &str) -> Result<Option<Arc<Run>>> {
        let _turn = self.revival.lock().await;
        if let Some(run) = self.run(run_id) {
            return Ok(Some(run));
        }
        let status = self.db.run(run_id)?.map(|r| r["status"].clone()).context("run not found")?;
        if status.as_str() != Some("delivered") {
            bail!("只有已交付的任务 可以验收");
        }
        let row = self.db.run(run_id)?.context("run not found")?;
        let worktree = WorktreeOf::of_config(&row["config"]);
        let base_ref = row["config"]["baseRef"].as_str().unwrap_or_default();
        let (text, data) =
            self.accept_worktree(run_id, worktree.as_ref(), base_ref, row["goal"].as_str().unwrap_or(""), true)?;
        self.record(run_id, ev(EventKind::RiskAccepted, "user", text).data(data))?;
        self.set_stored_status(run_id, RunStatus::Accepted)?;
        Ok(None)
    }

    /// A goal run that left the queue but never got its snapshot (`config.team`).
    fn never_started(&self, run_id: &str) -> Result<bool> {
        Ok(self.db.run(run_id)?.is_some_and(|r| r["kind"] == RunKind::Goal.as_str() && r["config"]["team"].is_null()))
    }

    /// After a restart nothing is running: close orphaned attempts and mark their runs interrupted.
    /// Bring an interrupted run back from its event log: finished tasks stay finished, unfinished
    /// ones start again (reopening their old session when possible), and planning resumes where it was.
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    /// Open run `run_id` again for the person (`resume_run_by`).
    pub async fn resume_run(self: &Arc<Self>, run_id: &str) -> Result<Arc<Run>> {
        self.resume_run_by(run_id, Asker::Person).await
    }

    /// Open run `run_id` again, its agents waiting for seats as `asker` (a Loop's timer as a
    /// schedule, the person's message first).
    pub async fn resume_run_by(self: &Arc<Self>, run_id: &str, asker: Asker) -> Result<Arc<Run>> {
        // A session closing to free its seat (`park`) is not reused: wait for its run to leave
        // memory, then open it anew, so nothing sent now goes to the closing session.
        for _ in 0..600 {
            match self.run(run_id) {
                Some(run) if run.st.lock().unwrap().lead.parked => tokio::time::sleep(Duration::from_millis(50)).await,
                _ => break,
            }
        }
        let _turn = self.revival.lock().await;
        if let Some(run) = self.run(run_id) {
            return Ok(run);
        }
        let row = self.db.run(run_id)?.context("run not found")?;
        let previous = row["status"].as_str().and_then(RunStatus::parse);
        let kind = row["kind"].as_str().and_then(RunKind::parse).context("这个任务的类型无法读取")?;
        // Delivered and accepted runs reopen as a conversation; interrupted and failed runs
        // rebuild the board and continue the goal. A chat (and a Loop's session) is always a
        // conversation: whatever ended its session (a restart, the agent exiting), the next
        // message (or wake) reopens it.
        // @project-doc docs/domains/20261120-chats.md#chat_session
        let conversation =
            kind.single_session() || matches!(previous, Some(RunStatus::Delivered | RunStatus::Accepted));
        let resumable = match kind {
            RunKind::Chat | RunKind::Loop => previous.is_some_and(|s| s != RunStatus::Cancelled),
            // A consolidation is one turn; a new one starts from the workspace's page.
            RunKind::Dream => bail!("整理团队记忆只有一轮，结束后不能继续；可以在工作区页重新整理"),
            RunKind::Goal => matches!(
                previous,
                Some(RunStatus::Interrupted | RunStatus::Failed | RunStatus::Delivered | RunStatus::Accepted)
            ),
        };
        if !resumable {
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
        let cwd = PathBuf::from(row["cwd"].as_str().unwrap_or(""));
        // An accepted run's worktree was removed; working again brings it back where the last
        // merge-back left the result (or where the run started).
        let worktree = WorktreeOf::of_config(config);
        if let Some(w) = &worktree {
            let merged = self.last_merged(run_id)?;
            let _writing = self.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
            crate::worktrees::ensure(
                &w.repo,
                &self.worktree_root(run_id),
                merged.as_deref().unwrap_or(&base_ref),
                run_id,
            )?;
        }
        let run = Arc::new(Run::new(RunRecord {
            id: run_id.to_string(),
            kind,
            goal: row["goal"].as_str().unwrap_or("").to_string(),
            cwd,
            worktree,
            team,
            git: config["git"] == true && !base_ref.is_empty(),
            base_ref,
            brief: config["workspace"]["brief"].as_str().unwrap_or("").trim().to_string(),
            goal_images: config["goalImages"].as_array().cloned().unwrap_or_default(),
            last_seq: self.db.max_seq(run_id)?,
            status: if conversation { previous.unwrap_or(RunStatus::Delivered) } else { RunStatus::Running },
            asker,
        }));

        // The board's tasks are every orchestration the lead got accepted.
        // Each with the last event before its orchestration: what the lead knew when it planned it.
        let board: Vec<_> = events
            .iter()
            .filter(|e| e.kind() == EventKind::OrchestrationAccepted)
            .filter_map(|e| {
                let d = e.data.as_ref()?;
                Some((e.seq, d["plannedSeq"].as_i64(), d["tasks"].as_array()?))
            })
            .flat_map(|(seq, planned, tasks)| tasks.iter().map(move |t| (seq, planned, t)))
            .collect();
        let (mut done, mut open) = (0, 0);
        let mut tasks = vec![];
        for (accepted_seq, planned, item) in board {
            let mut def: TaskDef = serde_json::from_value(item.clone())?;
            def.assignee = item["agent"].as_str().unwrap_or("").to_string();
            let id = def.id.clone();
            // The lead may have retried it with new instructions or acceptance criteria, and in a
            // chosen session; a verification of reworked work checks it again after it.
            let mut last_reopen_seq = 0i64;
            let mut reopen = None;
            for e in
                events.iter().filter(|e| e.kind() == EventKind::TaskReopened && e.task.as_deref() == Some(id.as_str()))
            {
                let d = e.data.clone().unwrap_or_default();
                if let Some(i) = d["instructions"].as_str() {
                    def.desc = i.to_string();
                }
                if let Some(a) = d["accept"].as_array() {
                    def.accept = a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                }
                reopen = Reopen::of_event(&d);
                if let Some(Reopen::Reverify { target }) = &reopen
                    && !def.deps.contains(target)
                {
                    def.deps.push(target.clone());
                }
                last_reopen_seq = e.seq;
            }
            let agent = def.assignee.clone();
            let mine = |kind: EventKind| -> Vec<&Event> {
                events.iter().filter(|e| e.kind() == kind && e.task.as_deref() == Some(id.as_str())).collect()
            };
            // An attempt started since took it.
            if mine(EventKind::AttemptStarted).iter().any(|e| e.seq > last_reopen_seq) {
                reopen = None;
            }
            let attempts = mine(EventKind::AttemptStarted).len() as i64;
            // The current retry budget starts at the last user retry.
            let retry_base = events
                .iter()
                .rposition(|e| e.kind() == EventKind::TaskReopened && e.task.as_deref() == Some(id.as_str()))
                .map(|j| {
                    events[..j]
                        .iter()
                        .filter(|e| e.kind() == EventKind::AttemptStarted && e.task.as_deref() == Some(id.as_str()))
                        .count() as i64
                })
                .unwrap_or(0);
            // Only an attempt that was still open when the orchestrator stopped is reloaded;
            // a finished one (failed or succeeded) starts a fresh session.
            let interrupted_attempt = events
                .iter()
                .rposition(|e| e.kind() == EventKind::AttemptStarted && e.task.as_deref() == Some(id.as_str()))
                .is_some_and(|j| {
                    !events[j + 1..].iter().any(|e| {
                        e.task.as_deref() == Some(id.as_str())
                            && matches!(
                                e.kind(),
                                EventKind::AttemptFailed
                                    | EventKind::TaskSucceeded
                                    | EventKind::TaskReopened
                                    | EventKind::AttemptLateResult
                            )
                    })
                });
            let succeeded = events
                .iter()
                .rfind(|e| {
                    e.task.as_deref() == Some(id.as_str())
                        && matches!(e.kind(), EventKind::TaskSucceeded | EventKind::TaskReopened)
                })
                .filter(|e| e.kind() == EventKind::TaskSucceeded);
            // A finished task's conclusion carries its whole result; an unfinished one has the last
            // result it submitted.
            let concluded = succeeded.and_then(|e| e.data.clone());
            let result = concluded
                .as_ref()
                .or_else(|| mine(EventKind::ResultSubmitted).last().and_then(|e| e.data.as_ref()))
                .map(result_from_event);
            let trace_gaps: Vec<String> = concluded
                .as_ref()
                .and_then(|d| d["traceGaps"].as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let artifacts: Vec<Value> =
                mine(EventKind::ArtifactCreated).iter().filter_map(|e| e.data.clone()).collect();
            let last_session = mine(EventKind::AttemptStarted)
                .last()
                .and_then(|e| e.data.as_ref().and_then(|d| d["session"].as_str().map(str::to_owned)));
            let last_task_event = events
                .iter()
                .filter(|e| e.task.as_deref() == Some(id.as_str()))
                .rfind(|e| {
                    matches!(
                        e.kind(),
                        EventKind::TaskSucceeded
                            | EventKind::TaskFailed
                            | EventKind::TaskBlocked
                            | EventKind::TaskSkippedByDecision
                            | EventKind::TaskReopened
                    )
                })
                .map(|e| e.kind());
            // Everything unfinished restarts from `pending`: the sessions and open requests of a
            // restart are gone, so a task that was waiting on a person cannot stay waiting.
            let status = match last_task_event {
                Some(EventKind::TaskSucceeded) => TaskStatus::Succeeded,
                Some(EventKind::TaskFailed) => TaskStatus::Failed,
                Some(EventKind::TaskBlocked) => TaskStatus::Blocked,
                Some(EventKind::TaskSkippedByDecision) => TaskStatus::SkippedByDecision,
                _ => TaskStatus::Pending,
            };
            // What a blocked task waits on: its failed dependency.
            let blocked =
                events.iter().rfind(|e| e.task.as_deref() == Some(id.as_str()) && e.kind() == EventKind::TaskBlocked);
            let blocked_by = blocked.and_then(|e| e.data.as_ref()?["dependency"].as_str()).map(str::to_owned);
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
            // A recorded negative gate is evidence of ordering even in logs written before plannedSeq.
            let planned = planned.unwrap_or_else(|| {
                let mut before = accepted_seq - 1;
                if let (TaskStatus::Blocked, Some(block), Some(dep)) = (status, blocked, blocked_by.as_deref())
                    && block.data.as_ref().is_some_and(|d| d["verdict"].is_string())
                    && let Some(result) = events.iter().rfind(|e| {
                        e.seq < block.seq && e.kind() == EventKind::TaskSucceeded && e.task.as_deref() == Some(dep)
                    })
                {
                    before = before.min(result.seq - 1);
                }
                before
            });
            let mut task = Task::new(def, agent, planned);
            // Rebuilding the board, not a transition: the event log already decided this state.
            task.restore(match status {
                TaskStatus::Succeeded => TaskPhase::Succeeded,
                TaskStatus::Failed => TaskPhase::Failed,
                TaskStatus::Blocked => TaskPhase::Blocked { by: blocked_by.unwrap_or_default() },
                TaskStatus::SkippedByDecision => TaskPhase::SkippedByDecision {
                    because: mine(EventKind::TaskSkippedByDecision)
                        .last()
                        .and_then(|e| e.data.as_ref()?["dependency"].as_str().map(str::to_owned)),
                },
                // Everything unfinished restarts from pending (see above).
                _ => TaskPhase::Pending,
            });
            task.attempts = attempts;
            task.result = result;
            task.last_success_seq = succeeded.map(|e| e.seq);
            task.trace_gaps = trace_gaps;
            task.reopen = reopen;
            task.artifacts = artifacts;
            task.retry_base = retry_base;
            let since = events
                .iter()
                .rposition(|e| e.kind() == EventKind::TaskReopened && e.task.as_deref() == Some(id.as_str()))
                .map(|j| events[j].seq)
                .unwrap_or(0);
            task.amendments = mine(EventKind::ResultAmended)
                .iter()
                .filter(|e| e.seq > since)
                .filter_map(|e| e.data.as_ref().and_then(|d| d["text"].as_str().map(str::to_owned)))
                .collect();
            for p in mine(EventKind::TaskClaimed)
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
                match (e.kind(), e.task.as_deref()) {
                    (EventKind::ToolCall, Some(task)) if !paths.is_empty() => st.traces.record(task, &paths, e.seq),
                    (EventKind::ToolCall, None) if e.actor == "lead" && !paths.is_empty() => {
                        st.traces.record_lead(&paths, e.seq);
                    }
                    (EventKind::LeadEdited, None) => {
                        st.traces.record_lead_after_plan(&paths);
                    }
                    (EventKind::LeadRouted, None) => {
                        st.traces.record_lead_tool_after_plan(d["tool"].as_str().unwrap_or_default());
                    }
                    (EventKind::TaskConflict, Some(task)) => {
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
                if e.kind() != EventKind::UserMessage {
                    continue;
                }
                let Some(id) = d["id"].as_str() else { continue };
                if !reached_agent(id, &events[i + 1..]) {
                    let text = d["text"].as_str().unwrap_or_default().to_string();
                    let msg = Msg { id: id.to_string(), text };
                    st.notes
                        .entry(agent.to_string())
                        .or_default()
                        .push(Note::new(msg, e.task.as_deref().unwrap_or("")));
                }
            }
        }
        if kind == RunKind::Loop {
            self.restore_loop_questions(&run, &events);
        }
        self.runs.lock().unwrap().insert(run_id.to_string(), run.clone());
        if conversation {
            // No scheduler and no automatic goal: the lead session comes back for a regular
            // conversation, and the message that asked for it is queued as a note.
            self.spawn_conversation(run.clone());
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
            .map(|t| json!({ "id": t.def.id, "status": t.status(), "blockedBy": t.blocked_by() }))
            .collect();
        self.emit(
            &run,
            ev(
                EventKind::RunRecovered,
                "user",
                format!("从中断处继续：看板上 {done} 个子任务已完成，{open} 个子任务重新开始，负责人重新推进目标"),
            )
            .data(json!({ "phase": "running", "done": done, "open": open, "tasks": restored })),
        );
        self.launch(run.clone(), Phase::Goal { resumed: true });
        Ok(run)
    }

    pub fn recover(&self) -> Result<()> {
        self.db.interrupt_commands()?;
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
            // Taken off the queue but not set up yet: it goes back in its place.
            if self.never_started(&run_id)? {
                self.requeue(&run_id, "服务重启时这个任务还在准备，重新排队")?;
                continue;
            }
            self.record(
                &run_id,
                ev(
                    EventKind::RunInterrupted,
                    "janus",
                    "编排器重启，进行中的 Agent 会话已关闭。可以从中断处继续，或者直接给某个 Agent 发消息",
                ),
            )?;
            self.db.set_run_status(&run_id, RunStatus::Interrupted)?;
        }
        // An accepted run kept its worktree while its conversation was open; nothing works there
        // after a restart, and the result is back where the run started. One whose conversation
        // worked on files after the acceptance keeps that work and is delivered again, which
        // merges it. Any other worktree git cannot read (Janus home moved and its repair failed
        // then) is repaired.
        for row in self.db.worktree_runs()? {
            let Some(repo) = row["config"]["worktree"]["repo"].as_str() else { continue };
            let id = row["id"].as_str().unwrap_or_default();
            let base_ref = row["config"]["baseRef"].as_str().unwrap_or_default();
            if row["status"] == "accepted" && self.moved_on_since_merge(id, base_ref) {
                if let Err(err) = self.redeliver_after_restart(id) {
                    crate::config::log(format!("janus: could not deliver accepted run {id} again: {err:#}"));
                }
                continue;
            }
            let _writing = self.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
            if row["status"] != "accepted" {
                crate::worktrees::heal(Path::new(repo), &self.worktree_root(id));
                continue;
            }
            let kept = self.cfg.layout.deleted_runs();
            if let Err(err) = crate::worktrees::remove(Path::new(repo), &self.worktree_root(id), &kept, id) {
                crate::config::log(format!("janus: could not remove the worktree of accepted run {id}: {err:#}"));
            }
        }
        Ok(())
    }
}

/// What a request to act on a run came to.
pub enum ActionReply {
    Done,
    /// A request with a `commandId` whose action already took effect: nothing was done again.
    Duplicate,
}

/// Why a request to act on a run, or to delete it, was turned down.
#[derive(Debug)]
pub enum Refused {
    NotFound,
    /// The run's state does not allow it now (a delete: the run is still working; stop it first).
    Conflict(String),
    Invalid(anyhow::Error),
}

impl From<anyhow::Error> for Refused {
    fn from(err: anyhow::Error) -> Refused {
        Refused::Invalid(err)
    }
}

/// Whether the person's message `id` reached its agent according to the events after it: joined
/// into a turn (steer / prompt) or delivered with a session. One that did not is still waiting.
pub(super) fn reached_agent(id: &str, later: &[Event]) -> bool {
    let routed = later
        .iter()
        .rev()
        .find(|x| x.kind() == EventKind::MessageRouted && x.data.as_ref().is_some_and(|x| x["id"] == id));
    let joined =
        routed.is_some_and(|x| x.data.as_ref().unwrap()["delivery"].as_str().is_some_and(Delivery::reached_turn));
    joined
        || later.iter().any(|x| {
            x.kind() == EventKind::MessageDelivered
                && x.data.as_ref().is_some_and(|x| x["ids"].as_array().is_some_and(|ids| ids.iter().any(|v| v == id)))
        })
}
