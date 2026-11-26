//! What the person does to a run: actions, accepting an archived delivery, resuming and recovering
//! after a restart.

use super::*;

/// A person's request to act on a run (`POST /api/runs/{id}/actions`): the action, and the
/// optional `commandId` and `expectedSequence` that make it safe to send twice.
#[derive(Deserialize, Debug, Clone)]
pub struct ActionRequest {
    #[serde(rename = "commandId", default)]
    pub command_id: Option<String>,
    #[serde(rename = "expectedSequence", default)]
    pub expected_sequence: Option<i64>,
    #[serde(flatten)]
    pub action: RunAction,
}

/// What a person can do to a run, by `type`.
// @project-doc docs/interfaces/20261031-http_api.md#run_actions
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunAction {
    /// Answer a question a task's agent, or the lead (`ask_user`, no task), is waiting on.
    Answer {
        key: String,
        #[serde(default)]
        task: String,
        #[serde(default)]
        answer: String,
        #[serde(default)]
        note: String,
    },
    /// Decide an agent's approval request; `option` is the agent's own `optionId`.
    Approve {
        key: String,
        #[serde(default)]
        task: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        allow: bool,
        #[serde(default)]
        option: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    /// Answer an agent's form.
    Input {
        key: String,
        #[serde(default)]
        task: String,
        #[serde(default)]
        agent: String,
        action: InputAction,
        #[serde(default)]
        content: Value,
    },
    /// A message for a session: an agent's (`agent`) or a task's (`task`).
    Message {
        #[serde(default)]
        text: String,
        #[serde(default)]
        images: Vec<Value>,
        #[serde(flatten)]
        to: SessionTarget,
    },
    /// Switch one of an open session's controls.
    Control {
        #[serde(default)]
        id: String,
        #[serde(default)]
        value: Value,
        #[serde(default)]
        via: Option<ControlVia>,
        #[serde(flatten)]
        to: SessionTarget,
    },
    Interrupt(SessionTarget),
    Takeover(SessionTarget),
    Release(SessionTarget),
    Finish(SessionTarget),
    Pause,
    Resume,
    /// Start a queued run now, without waiting for a slot.
    Start,
    Cancel,
    Accept,
    /// Continue an interrupted, failed or finished run where it was.
    ResumeRun,
}

/// The session an action is for: an agent's open session, or a task's.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct SessionTarget {
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub task: String,
}

wire_enum! {
    /// How the person answered an agent's form.
    pub enum InputAction {
        Accept = "accept",
        Decline = "decline",
        Cancel = "cancel",
    }
}

/// What the person (or the lead) decided on a pending request, for the session waiting on it.
#[derive(Debug, Clone)]
pub(super) enum Decision {
    Answer { answer: String, note: String },
    Approval { allow: bool, option: Option<String> },
    Input { action: InputAction, content: Value },
}

impl RunAction {
    /// The action's name, as `type` spells it, for messages and the command record.
    fn name(&self) -> &'static str {
        match self {
            RunAction::Answer { .. } => "answer",
            RunAction::Approve { .. } => "approve",
            RunAction::Input { .. } => "input",
            RunAction::Message { .. } => "message",
            RunAction::Control { .. } => "control",
            RunAction::Interrupt(_) => "interrupt",
            RunAction::Takeover(_) => "takeover",
            RunAction::Release(_) => "release",
            RunAction::Finish(_) => "finish",
            RunAction::Pause => "pause",
            RunAction::Resume => "resume",
            RunAction::Start => "start",
            RunAction::Cancel => "cancel",
            RunAction::Accept => "accept",
            RunAction::ResumeRun => "resumeRun",
        }
    }
}

impl App {
    /// A person's action on a run, as the HTTP API receives it. With a `commandId` it acts at most
    /// once: the id is claimed before anything happens and marked done once the action took
    /// effect. A later request with the id is a duplicate when the action is done, and refused
    /// while it is still running or when a restart cut it off, because then nobody knows yet
    /// whether it took effect. An action that fails gives the id back so it can be retried.
    /// `expectedSequence`, when given, must still be the run's last event. A run that is not live
    /// can be accepted, resumed, or talked to (which resumes it).
    // @project-doc docs/interfaces/20261031-http_api.md#run_actions
    pub async fn request_action(self: &Arc<Self>, run_id: &str, req: &ActionRequest) -> Result<ActionReply, Refused> {
        if self.db.run(run_id)?.is_none() {
            return Err(Refused::NotFound);
        }
        let command = req.command_id.as_deref().filter(|s| !s.trim().is_empty());
        if let Some(id) = command {
            match self.db.claim_command(run_id, id, &json!({ "type": req.action.name() }), now_ms())? {
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
        let done = match self.dispatch_action(run_id, req).await {
            // Accepting stopped on something in the run's worktree (the branch's new commits
            // conflict, an operation left unfinished there): the lead resolves that, not the person.
            Err(Refused::Invalid(err)) => match merge_handback(&err, MergeAt::Acceptance, self.run_lang(run_id)) {
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

    async fn dispatch_action(self: &Arc<Self>, run_id: &str, req: &ActionRequest) -> Result<(), Refused> {
        if let Some(expected) = req.expected_sequence {
            let actual = self.db.max_seq(run_id)?;
            if expected != actual {
                return Err(Refused::Conflict(format!("状态已变化：expectedSequence={expected}，当前为 {actual}")));
            }
        }
        let action = &req.action;
        let run = match self.run(run_id) {
            // The run is live (a failed driver removes its run, see `launch`): nothing to resume.
            Some(_) if matches!(action, RunAction::ResumeRun) => {
                return Err(Refused::Conflict("这个任务正在进行，不需要从中断处继续".into()));
            }
            Some(run) => run,
            None => {
                let row = self.db.run(run_id)?;
                let stored = row.as_ref().map(|r| r.status);
                let run_kind = row.as_ref().map(|r| r.kind).unwrap_or_default();
                match stored {
                    // A queued run has nothing live yet: it can only start now or be cancelled.
                    Some(RunStatus::Queued) => {
                        return match action {
                            RunAction::Start => self.start_now(run_id),
                            RunAction::Cancel => self.cancel_queued(run_id),
                            _ => Err(Refused::Conflict("这个任务还在排队：可以立即开始，或者取消".into())),
                        };
                    }
                    // One that could not start has no snapshot to continue from: it queues again.
                    Some(RunStatus::Failed) if self.never_started(run_id)? => {
                        return match action {
                            RunAction::ResumeRun => {
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
                    _ => match action {
                        // Acceptance needs no live agents, so it also works for runs delivered before a restart.
                        RunAction::Accept => match self.accept_archived(run_id).await? {
                            None => return Ok(()),
                            // A message brought it back meanwhile: it is accepted as a live run.
                            Some(run) => run,
                        },
                        RunAction::ResumeRun => return Ok(self.resume_run(run_id).await.map(|_| ())?),
                        // Talking to an agent brings an interrupted run back first, and so does
                        // answering a Loop's question: they outlive its session.
                        RunAction::Message { .. } => self.resume_run(run_id).await?,
                        RunAction::Answer { .. } if run_kind.questions_outlive_session() => {
                            self.resume_run(run_id).await?
                        }
                        _ => {
                            return Err(Refused::Conflict(match stored {
                                Some(RunStatus::Interrupted | RunStatus::Failed) => {
                                    "这个任务已经中断：先点「从中断处继续」，或者给某个 Agent 发消息".into()
                                }
                                _ => "这个任务已经不在进行，没有人在等这个操作：要补充什么，直接给某个 Agent 发消息"
                                    .into(),
                            }));
                        }
                    },
                }
            }
        };
        Ok(self.action(&run, action)?)
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
    pub fn action(self: &Arc<Self>, run: &Arc<Run>, action: &RunAction) -> Result<()> {
        match action {
            RunAction::Answer { key, task, answer, note } => self.answer(run, key, task, answer, note),
            RunAction::Approve { key, task, agent, allow, option, label } => {
                let text = match (label.as_deref().filter(|l| !l.is_empty()), allow) {
                    (Some(l), _) => format!("选择：{l}"),
                    (None, true) => "批准执行这一次".into(),
                    (None, false) => "拒绝执行".into(),
                };
                let kind = if *allow { EventKind::ApprovalGranted } else { EventKind::ApprovalDenied };
                let data = json!({ "key": key, "option": option });
                self.decide(
                    run,
                    key,
                    task,
                    agent,
                    ev(kind, "user", text).data(data),
                    Decision::Approval { allow: *allow, option: option.clone() },
                )
            }
            RunAction::Input { key, task, agent, action, content } => {
                let text = match action {
                    InputAction::Accept => "提交了填写的内容",
                    InputAction::Decline => "拒绝填写",
                    InputAction::Cancel => "取消了这个请求",
                };
                let content = if content.is_object() { content.clone() } else { json!({}) };
                let data = json!({ "key": key, "action": action, "content": content });
                self.decide(
                    run,
                    key,
                    task,
                    agent,
                    ev(EventKind::InputAnswered, "user", text).data(data),
                    Decision::Input { action: *action, content },
                )
            }
            // A Loop's clock is paused on the Loop, and its session is stopped by deleting the Loop.
            RunAction::Pause | RunAction::Resume | RunAction::Cancel if !run.kind.stopped_on_run_page() => {
                bail!("值班在值班页暂停或删除，不能当作任务暂停或停止");
            }
            RunAction::Pause | RunAction::Resume => self.pause(run, matches!(action, RunAction::Pause)),
            RunAction::Cancel => self.cancel(run),
            RunAction::Message { text, images, to } => {
                let text = text.trim();
                if text.is_empty() && images.is_empty() {
                    bail!("消息不能为空");
                }
                // Pasted images are stored with the run; the text names each one. A message
                // refused here (an image that is not one) does not title a chat.
                let images = crate::attachments::save(&self.dirs(run), images)?;
                self.title_chat(run, text)?;
                let text = crate::attachments::with_lines(text, &images);
                // Shown the moment it is sent; its session routes it once it can.
                let recipient = if to.agent.is_empty() { self.assignee(run, &to.task) } else { to.agent.clone() };
                let m = self.post_message(run, "user", &recipient, &to.task, &text, &images);
                self.send_steer(run, &to.agent, &to.task, Steer::Message(m))
            }
            RunAction::Control { id, value, via, to } => {
                let id = id.trim();
                if id.is_empty() || value.is_null() || value.as_str().is_some_and(|v| v.trim().is_empty()) {
                    bail!("切换会话选项要给出 id 和 value");
                }
                let control =
                    SessionControl { id: id.to_string(), value: value.clone(), via: via.unwrap_or(ControlVia::Config) };
                self.send_steer(run, &to.agent, &to.task, Steer::Control(control))
            }
            RunAction::Interrupt(to) => self.send_steer(run, &to.agent, &to.task, Steer::Interrupt),
            RunAction::Takeover(to) => self.send_steer(run, &to.agent, &to.task, Steer::TakeOver),
            RunAction::Release(to) => self.send_steer(run, &to.agent, &to.task, Steer::Release),
            RunAction::Finish(to) => self.send_steer(run, &to.agent, &to.task, Steer::Finish),
            RunAction::Accept => self.accept_live(run),
            RunAction::Start => bail!("只有排队中的任务可以立即开始"),
            RunAction::ResumeRun => bail!("这个任务正在进行，不需要从中断处继续"),
        }
    }

    /// The agent that works on `task`; empty when the board has no such task.
    fn assignee(&self, run: &Run, task: &str) -> String {
        run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == task).map(|t| t.agent.clone()).unwrap_or_default()
    }

    /// The person answers a question: a task's agent's, or the lead's (`ask_user`, no task).
    fn answer(&self, run: &Run, key: &str, task: &str, answer: &str, note: &str) -> Result<()> {
        let agent = match self.pending_owner(run, key)?.session {
            SessionKey::Lead => "lead".to_string(),
            SessionKey::Task(t) => self.assignee(run, &t),
        };
        self.emit(
            run,
            ev(EventKind::QuestionAnswered, "user", format!("回答：{answer}"))
                .task(task)
                .to(&agent)
                .data(json!({ "key": key, "answer": answer, "note": note })),
        );
        self.resolve(run, key, Decision::Answer { answer: answer.to_string(), note: note.to_string() })?;
        if task.is_empty() {
            // A Loop does not wait for the answer to its question: the answer wakes it.
            if run.kind == RunKind::Loop
                && let Some(id) = self.db.run(&run.id)?.and_then(|r| r.loop_id.clone())
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
        Ok(())
    }

    /// The person decides an agent's approval or form: recorded as `e`, then handed to the
    /// session waiting on `key`.
    fn decide(&self, run: &Run, key: &str, task: &str, agent: &str, e: Ev, decision: Decision) -> Result<()> {
        self.pending_owner(run, key)?;
        let assignee = self.assignee(run, task);
        let to = if !assignee.is_empty() {
            assignee
        } else if agent.is_empty() {
            "lead".to_string()
        } else {
            agent.to_string()
        };
        self.emit(run, e.to(&to).task(task));
        self.resolve(run, key, decision)
    }

    fn pause(self: &Arc<Self>, run: &Arc<Run>, pause: bool) -> Result<()> {
        run.st.lock().unwrap().set_paused(pause)?;
        let (kind, text) = if pause {
            (EventKind::RunPaused, "暂停任务：进行中的尝试会继续完成，不再启动新子任务")
        } else {
            (EventKind::RunResumed, "继续任务")
        };
        self.emit(run, ev(kind, "user", text));
        self.ensure_driver(run);
        run.wake.notify_one();
        // A paused run gives its slot to the queue; resuming takes one again.
        self.pump();
        Ok(())
    }

    fn cancel(&self, run: &Run) -> Result<()> {
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
        Ok(())
    }

    /// The person accepts a run that is live (its lead's conversation open).
    fn accept_live(&self, run: &Run) -> Result<()> {
        // Only a delivered run has results to accept; accepting earlier would be undone by the
        // scheduler the moment it next ran.
        let (status, busy, asking) = {
            let st = run.st.lock().unwrap();
            (st.status, st.lead.in_turn, !st.pending.is_empty())
        };
        if status != RunStatus::Delivered {
            bail!("任务现在是「{status}」，只有已交付的任务 可以验收");
        }
        // A delivery the lead is still working on, or one that waits on the person's answer, is
        // not the result to accept yet.
        // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
        if busy {
            bail!("负责人还在处理你交代的事，等它这一轮结束后再验收");
        }
        if asking {
            bail!("还有等你回答的请求，先处理它们再验收");
        }
        // The lead's conversation stays open and works in the worktree, so it stays; the next
        // start removes it once nothing works there (`recover`).
        let (text, data) = self.accept_worktree(&run.id, run.worktree.as_ref(), &run.base_ref, &run.goal, false)?;
        self.emit(run, ev(EventKind::RiskAccepted, "user", text).data(data));
        self.set_status(run, RunStatus::Accepted);
        Ok(())
    }

    pub async fn accept_archived(&self, run_id: &str) -> Result<Option<Arc<Run>>> {
        let _turn = self.gates.revival.lock().await;
        if let Some(run) = self.run(run_id) {
            return Ok(Some(run));
        }
        let status = self.db.run(run_id)?.map(|r| r.status).context("run not found")?;
        if status != RunStatus::Delivered {
            bail!("只有已交付的任务 可以验收");
        }
        let row = self.db.run(run_id)?.context("run not found")?;
        let worktree = WorktreeOf::of_config(&row.config);
        let base_ref = row.config["baseRef"].as_str().unwrap_or_default();
        let (text, data) = self.accept_worktree(run_id, worktree.as_ref(), base_ref, row.goal.as_str(), true)?;
        self.record(run_id, ev(EventKind::RiskAccepted, "user", text).data(data))?;
        self.set_stored_status(run_id, RunStatus::Accepted)?;
        Ok(None)
    }

    /// A goal run that left the queue but never got its snapshot (`config.team`).
    fn never_started(&self, run_id: &str) -> Result<bool> {
        Ok(self.db.run(run_id)?.is_some_and(|r| r.kind == RunKind::Goal && r.config["team"].is_null()))
    }

    /// After a restart nothing is running: close orphaned attempts and mark their runs interrupted.
    /// Bring an interrupted run back from its event log: finished tasks stay finished, unfinished
    /// ones start again (reopening their old session when possible), and the lead goes on in its own
    /// session when its agent can load it, or in a new one told where the board stands.
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
        let _turn = self.gates.revival.lock().await;
        if let Some(run) = self.run(run_id) {
            return Ok(run);
        }
        let row = self.db.run(run_id)?.context("run not found")?;
        let previous = row.status;
        let kind = Some(row.kind).context("这个任务的类型无法读取")?;
        // Delivered and accepted runs reopen as a conversation; interrupted and failed runs
        // rebuild the board and continue the goal. A chat (and a Loop's session) is always a
        // conversation: whatever ended its session (a restart, the agent exiting), the next
        // message (or wake) reopens it.
        // @project-doc docs/domains/20261120-chats.md#chat_session
        let conversation = kind.single_session() || matches!(previous, RunStatus::Delivered | RunStatus::Accepted);
        if !kind.resumable(previous).map_err(|why| anyhow!(why))? {
            bail!("只有中断、失败、已交付或已验收的子任务可以继续");
        }
        let events = self.db.events(run_id, 0)?;
        let config = &row.config;
        let mut team: Vec<Member> =
            serde_json::from_value(config["team"].clone()).context("这个子任务的团队记录无法读取")?;
        for m in team.iter_mut() {
            // A conversation only needs the lead. A collaborator's adapter may have changed or
            // been removed since the run; its task will report that if new work uses it.
            if conversation && !m.lead {
                continue;
            }
            self.spec_for(&m.connection)?;
        }
        let base_ref = config["baseRef"].as_str().unwrap_or("").to_string();
        let cwd = PathBuf::from(row.cwd.as_str());
        // An accepted run's worktree was removed; working again brings it back where the last
        // merge-back left the result (or where the run started).
        let worktree = WorktreeOf::of_config(config);
        if let Some(w) = &worktree {
            let merged = self.last_merged(run_id)?;
            let _writing = self.gates.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
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
            goal: row.goal.as_str().to_string(),
            cwd,
            worktree,
            team,
            git: config["git"] == true && !base_ref.is_empty(),
            base_ref,
            brief: config["workspace"]["brief"].as_str().unwrap_or("").trim().to_string(),
            goal_images: config["goalImages"].as_array().cloned().unwrap_or_default(),
            lang: crate::lang::Lang::of_config(config),
            last_seq: self.db.max_seq(run_id)?,
            status: if conversation { previous } else { RunStatus::Running },
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
                .map(result_from_json);
            let trace_gaps: Vec<String> = concluded
                .as_ref()
                .and_then(|d| d["traceGaps"].as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let artifacts: Vec<Artifact> =
                mine(EventKind::ArtifactCreated).iter().filter_map(|e| Artifact::of_event(e)).collect();
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
                    run.st
                        .lock()
                        .unwrap()
                        .resume_session
                        .insert(id.clone(), Reload { session: sid, broke: BrokeOff::Restart });
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
        // Only a restart continues a goal on a schedule's behalf; the person continues it by hand.
        let auto = asker == Asker::Schedule;
        let (actor, who) = if auto { ("janus", "服务重启后自动") } else { ("user", "") };
        self.emit(
            &run,
            ev(
                EventKind::RunRecovered,
                actor,
                format!("{who}从中断处继续：看板上 {done} 个子任务已完成，{open} 个子任务接着做，负责人接着推进目标"),
            )
            .data(json!({ "phase": "running", "done": done, "open": open, "tasks": restored, "auto": auto })),
        );
        self.launch(run.clone(), Phase::Goal { resumed: true });
        Ok(run)
    }

    /// What happens to unfinished run `run_id` once the restarted service listens. A goal goes on
    /// by itself: nobody may be watching (a Loop dispatched it, the person is away), and work left
    /// interrupted would wait forever. A run the person paused waits for them. So does one that
    /// went on by itself after the last restart and was cut off again before any agent did
    /// anything: continuing it once more might only repeat whatever stopped the service.
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    fn goes_on_after_restart(&self, run_id: &str) -> Result<AfterRestart> {
        let Some(row) = self.db.run(run_id)? else { return Ok(AfterRestart::Person) };
        if row.kind != RunKind::Goal {
            return Ok(AfterRestart::Person);
        }
        let seq = |kind| self.db.latest_event(run_id, kind).map(|e| e.map(|e| e.seq).unwrap_or(0));
        let recovered = self.db.latest_event(run_id, EventKind::RunRecovered)?;
        let since = recovered.as_ref().map(|e| e.seq).unwrap_or(0);
        // A pause does not outlive a continue: only one after the last continue still holds.
        if seq(EventKind::RunPaused)? > seq(EventKind::RunResumed)?.max(since) {
            return Ok(AfterRestart::Paused);
        }
        if recovered.is_some_and(|e| e.data.is_some_and(|d| d["auto"] == true)) {
            let worked = self.db.events(run_id, since)?.iter().any(|e| e.kind().is_agent_progress());
            if !worked {
                return Ok(AfterRestart::NoProgress);
            }
        }
        Ok(AfterRestart::GoesOn)
    }

    /// Continue the goal runs `recover` set aside (`continuing`), one after another, each as if a
    /// schedule started it: they take their slots back before the queue hands out new ones.
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    pub fn continue_after_restart(self: &Arc<Self>) {
        let mut ids: Vec<String> = self.continuing.lock().unwrap().iter().cloned().collect();
        ids.sort();
        let app = self.clone();
        tokio::spawn(async move {
            for id in ids {
                let resumed = app.resume_run_by(&id, Asker::Schedule).await;
                // Working again (it holds its slot as a running run now), or waiting for the person.
                app.continuing.lock().unwrap().remove(&id);
                if let Err(err) = resumed {
                    let _ = app.record(
                        &id,
                        ev(
                            EventKind::RunInterrupted,
                            "janus",
                            format!("服务重启后没能自动继续：{err:#}。可以从中断处继续"),
                        )
                        .data(json!({ "after": AfterRestart::Person, "error": format!("{err:#}") })),
                    );
                    crate::config::log(format!("janus: could not continue run {id} after the restart: {err:#}"));
                }
                app.pump();
            }
        });
    }

    /// Whoever waited on a decision waited in the process that just stopped, so a card still open
    /// in a run's log can no longer be acted on (a delivered run's included, which has no
    /// `run.interrupted` to clear it): it is withdrawn. A Loop's questions wait for their answer
    /// across restarts (`restore_loop_questions`).
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    fn withdraw_stale_requests(&self) -> Result<()> {
        for asked in self.db.open_requests()? {
            let kind = self.db.run(&asked.run_id)?.map(|r| r.kind).unwrap_or_default();
            if asked.kind() == EventKind::QuestionEscalated && kind.questions_outlive_session() {
                continue;
            }
            let Some(key) = asked.data.as_ref().and_then(|d| d["key"].as_str()) else { continue };
            let mut e = ev(EventKind::DecisionWithdrawn, "janus", "服务重启了，没有人再等这个回答，撤回这个请求")
                .data(json!({ "key": key }));
            if let Some(task) = asked.task.as_deref().filter(|t| !t.is_empty()) {
                e = e.task(task).attempt(asked.attempt.unwrap_or_default());
            }
            self.record(&asked.run_id, e)?;
        }
        Ok(())
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
            let goes_on = self.goes_on_after_restart(&run_id)?;
            let text = match goes_on {
                AfterRestart::GoesOn => "编排器重启，进行中的 Agent 会话已关闭。服务启动后自动从中断处继续",
                AfterRestart::Paused => "编排器重启，进行中的 Agent 会话已关闭。重启前已暂停，等你从中断处继续",
                AfterRestart::NoProgress => {
                    "编排器重启，进行中的 Agent 会话已关闭。上次重启后自动继续，还没有任何进展又中断了，等你决定是否继续"
                }
                AfterRestart::Person => {
                    "编排器重启，进行中的 Agent 会话已关闭。可以从中断处继续，或者直接给某个 Agent 发消息"
                }
            };
            self.record(&run_id, ev(EventKind::RunInterrupted, "janus", text).data(json!({ "after": goes_on })))?;
            self.db.set_run_status(&run_id, RunStatus::Interrupted)?;
            if goes_on == AfterRestart::GoesOn {
                self.continuing.lock().unwrap().insert(run_id);
            }
        }
        self.withdraw_stale_requests()?;
        // An accepted run kept its worktree while its conversation was open; nothing works there
        // after a restart, and the result is back where the run started. One whose conversation
        // worked on files after the acceptance keeps that work and is delivered again, which
        // merges it. Any other worktree git cannot read (Janus home moved and its repair failed
        // then) is repaired.
        for row in self.db.worktree_runs()? {
            let Some(repo) = row.config["worktree"]["repo"].as_str() else { continue };
            let id = row.id.as_str();
            let base_ref = row.config["baseRef"].as_str().unwrap_or_default();
            if row.status == RunStatus::Accepted && self.moved_on_since_merge(id, base_ref) {
                if let Err(err) = self.redeliver_after_restart(id) {
                    crate::config::log(format!("janus: could not deliver accepted run {id} again: {err:#}"));
                }
                continue;
            }
            let _writing = self.gates.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
            if row.status != RunStatus::Accepted {
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

wire_enum! {
    /// What a restart leaves an unfinished run to (`run.interrupted`'s `data.after`).
    pub enum AfterRestart {
        /// A goal: it continues by itself once the service listens.
        GoesOn = "goes_on",
        /// The person paused it before the restart: it waits for them.
        Paused = "paused",
        /// It continued by itself after the last restart and no agent did anything before this
        /// one: it waits for the person.
        NoProgress = "no_progress",
        /// A chat, a Loop's session, a consolidation, or a run that could not be continued: the
        /// next message or wake opens it, or the person continues it.
        Person = "person",
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
