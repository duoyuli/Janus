//! The lead: its goal session, the conversation after delivery, its instructions and the board
//! pushed into it.

use super::*;

/// Why Janus sent a prompt into a session (`session.prompt`, `data.channel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromptChannel {
    /// The lead's brief as a first prompt, for agents without a slot in their system layer.
    Context,
    /// `/goal <goal>`.
    Goal,
    /// What comes next in the lead's session: the person's message, or messages that waited.
    Followup,
    /// Board entries pushed into the lead's running turn.
    Board,
    /// Janus asks a lead whose goal does not settle whether it is done.
    GoalCheck,
    /// A result that did not pass its check, sent back to be fixed.
    Fix,
}

impl PromptChannel {
    /// A prompt in the lead's session: a slash command starts a goal, anything else follows up.
    pub(super) fn of_lead_prompt(prompt: &str) -> PromptChannel {
        if prompt.starts_with("/goal ") { PromptChannel::Goal } else { PromptChannel::Followup }
    }

    fn as_str(self) -> &'static str {
        match self {
            PromptChannel::Context => "context",
            PromptChannel::Goal => "goal",
            PromptChannel::Followup => "followup",
            PromptChannel::Board => "board",
            PromptChannel::GoalCheck => "goal_check",
            PromptChannel::Fix => "fix",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PromptChannel::Context => "向负责人发送团队上下文",
            PromptChannel::Goal => "向负责人发送目标提示",
            PromptChannel::Followup => "向负责人发送继续提示",
            PromptChannel::Board => "把看板上需要处理的消息推给负责人",
            PromptChannel::GoalCheck => "问负责人目标是否已经完成",
            PromptChannel::Fix => "验证没有通过，把结果发回 Agent 修复",
        }
    }
}

impl App {
    /// Goal mode: the lead pursues the goal in its own session (Claude Code and Codex `/goal`) and
    /// does it alone or brings in the team. Bringing them in means submitting an orchestration
    /// (tasks, assignees, dependencies) through Janus's MCP tools; Janus validates it, runs it
    /// with the same scheduler as a plan, and the shared board carries everything in between.
    // @project-doc docs/domains/20261031-goal_mode.md#lead_session
    pub(super) async fn goal(self: &Arc<Self>, run: &Arc<Run>, resumed: bool) -> Result<()> {
        {
            let mut st = run.st.lock().unwrap();
            st.lead.goal = LeadGoal::Working;
            // Changes already in the directory are not this run's: a task's diff starts from here.
            if run.git && st.last_diff.is_empty() {
                st.last_diff = working_diff(&run.cwd, &[]);
            }
            // Continuing: the restarted lead is told the board so far; `wait` brings only what is new.
            if resumed {
                st.lead.cursor = run.seq.load(Ordering::SeqCst);
            }
            if st.mcp_token.is_empty() {
                st.mcp_token = random_token();
            }
        }
        self.set_status(run, RunStatus::Running);
        let (app, r) = (self.clone(), run.clone());
        tokio::spawn(async move { app.push_loop(&r).await });
        let (app, r) = (self.clone(), run.clone());
        tokio::spawn(async move {
            let outcome = app.lead_goal(&r, resumed).await;
            if r.st.lock().unwrap().cancelled() {
                return;
            }
            let done = match outcome {
                // The lead's last message is its report; earlier ones narrated the work.
                Ok(text) => LeadGoal::Done(
                    last_reply(&app.db.events(&r.id, 0).unwrap_or_default())
                        // A lead that confirmed its goal to Janus starts with the marker; the summary does not.
                        .map(|t| crate::goal_settle::confirmed(&t).filter(|s| !s.is_empty()).unwrap_or(t))
                        .unwrap_or(text),
                ),
                Err(err) if err.downcast_ref::<Cancelled>().is_some() => return,
                Err(err) => {
                    app.emit(
                        &r,
                        ev(
                            "lead.failed",
                            "janus",
                            format!("负责人的会话异常结束：{}", short(&format!("{err:#}"), 120)),
                        )
                        .data(json!({ "error": format!("{err:#}") })),
                    );
                    LeadGoal::Failed(format!("{err:#}"))
                }
            };
            // The lead sets this when its first goal completes, while the session stays open for
            // conversation. Do not replace the delivered summary with a later chat reply.
            let first = {
                let mut st = r.st.lock().unwrap();
                if !st.lead.goal.ended() {
                    st.lead.goal = done;
                    true
                } else {
                    false
                }
            };
            if first {
                app.escalate_open_questions(&r);
            }
            r.lead_nudge.notify_one();
            r.wake.notify_one();
        });
        self.execute(run).await
    }

    /// While the lead's session lasts: whenever the board gets something the lead has to act on,
    /// wait a moment for more, then push it all into the lead's running turn (`push_board`).
    pub(super) async fn push_loop(&self, run: &Run) {
        loop {
            run.lead_nudge.notified().await;
            {
                let st = run.st.lock().unwrap();
                // Delivery ends automatic board pushes; the session then only answers the person.
                if st.cancelled() || st.lead.goal.ended() {
                    return;
                }
            }
            tokio::time::sleep(PUSH_SETTLE).await;
            self.push_board(run);
        }
    }

    /// The lead hears about questions, failures, stalls, blockers, conflicts and the person's
    /// answers while it works on something else, instead of only on its next `wait`. Everything
    /// new on the board since it last looked goes in as one message steered into its turn, and
    /// counts as seen. Only for a lead whose agent takes steering: queued until its turn ends,
    /// the message would hide those entries from `wait` in the meantime. Nothing is pushed while
    /// the lead is inside `wait`, which hands it the same entries.
    // @project-doc docs/domains/20261031-goal_mode.md#board_push
    pub(super) fn push_board(&self, run: &Run) {
        let (tx, cursor, sid) = {
            let st = run.st.lock().unwrap();
            if st.lead.waiting || !st.steerable.contains(&SessionKey::Lead) {
                return;
            }
            (st.live.get(&SessionKey::Lead).cloned(), st.lead.cursor, st.lead.session.clone())
        };
        let (Some(tx), Some(sid)) = (tx, sid) else { return };
        let events = self.db.events(&run.id, cursor).unwrap_or_default();
        if !events.iter().any(needs_lead) {
            return;
        }
        let entries: Vec<String> = events.iter().filter_map(|e| board_entry(run, e)).collect();
        let last = events.last().map(|e| e.seq).unwrap_or(cursor);
        {
            let mut st = run.st.lock().unwrap();
            if st.lead.waiting || st.lead.cursor != cursor {
                return;
            }
            st.lead.cursor = last;
        }
        if entries.is_empty() {
            return;
        }
        let text = format!(
            "[Janus 看板推送] 看板上有需要你处理的新消息。它们已经算作读过，不会再出现在 janus.wait 里：\n{}",
            entries.join("\n")
        );
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("push{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        self.record_session_prompt(
            run,
            &Ctx { agent: "lead", task: None, attempt: None, goal: false },
            &sid,
            PromptChannel::Board,
            &text,
        );
        let _ = tx.send(Steer::Message(Msg { id, text }));
    }

    /// Goal mode: once the lead's session has ended nobody reads the board, so a collaborator's
    /// question still waiting for the lead goes to the person instead.
    // @project-doc docs/domains/20261031-goal_mode.md#question_routing
    pub(super) fn escalate_open_questions(&self, run: &Run) {
        let events = self.db.events(&run.id, 0).unwrap_or_default();
        let key_of = |e: &Event| e.data.as_ref().and_then(|d| d["key"].as_str().map(str::to_owned));
        let settled: HashSet<String> = events
            .iter()
            .filter(|e| matches!(e.kind.as_str(), "question.answered" | "question.escalated"))
            .filter_map(key_of)
            .collect();
        let running: HashSet<String> = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .map(|t| t.def.id.clone())
            .collect();
        for e in events.iter().filter(|e| e.kind == "question.asked") {
            let Some(key) = key_of(e).filter(|k| !settled.contains(k)) else { continue };
            // A question from an attempt that has since ended is no longer waiting for anyone.
            let live = e.task.as_ref().is_some_and(|t| {
                running.contains(t)
                    && !events
                        .iter()
                        .any(|x| x.seq > e.seq && x.kind == "attempt.started" && x.task.as_ref() == Some(t))
            });
            if !live {
                continue;
            }
            let esc =
                ev("question.escalated", "janus", "负责人已经结束，协作者的提问转交给你").to("user").data(json!({
                    "key": key, "reason": "负责人的会话已经结束，没有人在看板上回答这个问题。"
                }));
            self.emit(run, esc.task(e.task.as_deref().unwrap_or_default()));
        }
    }

    /// The lead's goal session: `/goal <goal>` in the workspace directory, with the team and the
    /// board tools in reach. Open to the person like any session: steering, takeover, messages.
    pub(super) async fn lead_goal(self: &Arc<Self>, run: &Arc<Run>, resumed: bool) -> Result<String> {
        let lead = run.team.iter().find(|m| m.lead).context("no lead")?.clone();
        if !lead.kind.leads() {
            bail!("负责人只能是 Claude Code 或 Codex：只有它们支持 /goal");
        }
        let standing = standing_instructions(&lead);
        let channel = instruction_channel(lead.kind);
        // The brief, with the messages left for the lead, goes in the system layer before the
        // standing rules. Codex reads it from its environment, so it is known before the adapter
        // starts; the notes are taken only once the session is open.
        // @project-doc docs/domains/20261031-goal_mode.md#lead_brief
        let (notes, note_ids) = self.peek_notes(run, "lead");
        let brief = format!("{}{notes}", self.goal_brief(run, resumed));
        let system = system_layer(&brief, &standing);
        let spec = with_system(self.spec_for(&lead.runtime)?, &channel, &system);
        let workspace = run.cwd.clone();
        if lead.kind == Runtime::Claude {
            // Claude Code only runs /goal in a workspace it trusts; its isolated config has no record yet.
            self.trust_claude_workspace(&lead.runtime, &workspace)?;
        }
        let log = self.cfg.data_dir.join("logs").join(&run.id).join("lead-goal.log");
        let owner = Owner { session: SessionKey::Lead, attempt: 0 };
        let (conn, mut rx, mut ctl) = self.open_session(run, &owner, "lead", &spec, &log, "负责人").await?;
        let result = async {
            self.take_peeked_notes(run, "lead", &note_ids);
            let (sid, applied, controls) = tokio::time::timeout(STARTUP_TIMEOUT, async {
                let board = vec![self.mcp_server(run, "lead")];
                let opening = self.opening(
                    run,
                    &lead,
                    &conn.capabilities,
                    board,
                    (channel == Channel::System).then_some(system.as_str()),
                );
                let session = new_session(&conn, &workspace, &opening).await?;
                let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
                let mut applied = configure_session(&conn, &session, &lead).await?;
                applied["resources"] = json!(opening.resources);
                let controls = crate::acp_view::controls(&session, &applied);
                Ok::<_, anyhow::Error>((sid, applied, controls))
            })
            .await
            .map_err(|_| anyhow!("负责人打开会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
            self.emit(
                run,
                ev("agent.status", "lead", if resumed { "从中断处继续推进目标" } else { "开始推进目标" })
                    .data(json!({ "phase": AgentPhase::Thinking, "session": sid, "config": applied })),
            );
            run.st.lock().unwrap().lead.session = Some(sid.clone());
            self.watch_session(run, &owner.session, 0);
            let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
            self.show(run, ("session.controls", "会话选项".into(), controls), &ctx);
            self.record_system(run, &ctx, &sid, &channel, "向负责人注入负责人说明", &brief);
            self.record_instructions(run, &ctx, &sid, &channel, &standing);
            if channel == Channel::Prompt {
                // An agent without a system layer gets the brief first, as context.
                let ack =
                    format!("{brief}\n\n以上是背景信息。现在只回复“已了解”，不要开始工作，目标会在下一条消息里给出。");
                self.record_session_prompt(run, &ctx, &sid, PromptChannel::Context, &ack);
                self.stream_prompt(
                    run,
                    &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
                    &ack,
                    &Ctx { goal: false, ..ctx },
                    Some(&mut ctl),
                    Some(STARTUP_TIMEOUT),
                )
                .await?;
            }
            let mut prompt = format!("/goal {}", goal_condition(&run.goal));
            let mut delivered = false;
            loop {
                // A slash command starts a new goal; a message sent while one is active belongs
                // to that goal's turn. Both complete through the same branch below.
                let goal_turn = prompt.trim_start().starts_with("/goal ") || run.st.lock().unwrap().lead.goal_active;
                self.record_session_prompt(run, &ctx, &sid, PromptChannel::of_lead_prompt(&prompt), &prompt);
                let text = self
                    .stream_prompt(
                        run,
                        &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
                        &prompt,
                        &ctx,
                        Some(&mut ctl),
                        None,
                    )
                    .await?;
                if let Some(reason) = goal_refused(&text) {
                    bail!("{} 拒绝了 /goal：{reason}", lead.runtime_name);
                }
                if !ctl.held_by_person() && !ctl.queued.is_empty() {
                    let next =
                        self.take_queued(run, &mut ctl, &ctx, |n| format!("这一轮结束，把 {n} 条消息发给负责人"));
                    prompt = if delivered {
                        next
                    } else {
                        format!("{next}\n\n请结合这些信息继续推进目标：{}", run.goal)
                    };
                    continue;
                }
                if ctl.held_by_person() {
                    let session = PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                    let resume = format!("人工介入结束。请结合上面的对话，继续推进目标直到完成：{}", run.goal);
                    if let Some(next) = self.wait_for_person(run, &session, &mut ctl, &ctx, &resume).await? {
                        prompt = next;
                        continue;
                    }
                }
                if !delivered {
                    // The goal is complete. Delivery can happen while this session stays open:
                    // the person may write the next task or set a new goal with a slash command.
                    delivered = true;
                    run.watch.lock().unwrap().remove(&owner.session);
                    {
                        let mut st = run.st.lock().unwrap();
                        // A goal that was ended by hand must not make the next chat turn follow it.
                        st.lead.goal_active = false;
                        st.lead.goal = LeadGoal::Done(text.clone());
                    }
                    run.lead_nudge.notify_one();
                    run.wake.notify_one();
                    // The run is delivered; a collaborator still waiting on the board now waits on
                    // the person, as it did when the lead's session ended.
                    self.escalate_open_questions(run);
                    self.emit(
                        run,
                        ev("agent.status", "lead", "目标已完成，可以继续对话或设置新的目标")
                            .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
                    );
                } else if goal_turn {
                    // A new goal in the conversation is a new result: deliver it like the first one.
                    run.st.lock().unwrap().lead.goal = LeadGoal::Done(text.clone());
                    run.wake.notify_one();
                    if !run.driving.active() {
                        self.deliver(run);
                    }
                }
                ctl.next_turn(run);
                let Some(next) = self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? else {
                    return Ok(text);
                };
                prompt = next;
            }
        }
        .await;
        run.st.lock().unwrap().lead.session = None;
        self.close_session(run, &owner, Some(&conn), "负责人的会话已结束，撤回这个请求").await;
        run.lead_nudge.notify_one();
        result
    }

    /// Bring a delivered or accepted run back as a regular conversation. The lead session is
    /// restored with `session/load` when the adapter still has it; otherwise a new session starts
    /// with a compact brief of the goal, the delivery and the last exchanges.
    pub(super) async fn lead_conversation(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        let lead = run.team.iter().find(|m| m.lead).context("no lead")?.clone();
        let standing = standing_instructions(&lead);
        let channel = instruction_channel(lead.kind);
        let events = self.db.events(&run.id, 0).unwrap_or_default();
        let old_session = last_lead_session(&events);
        let brief = conversation_brief(run, &events);
        let system = system_layer(&brief, &standing);
        let spec = with_system(self.spec_for(&lead.runtime)?, &channel, &system);
        let workspace = run.cwd.clone();
        if lead.kind == Runtime::Claude {
            self.trust_claude_workspace(&lead.runtime, &workspace)?;
        }
        let log = self.cfg.data_dir.join("logs").join(&run.id).join("lead-chat.log");
        let owner = Owner { session: SessionKey::Lead, attempt: 0 };
        let opened = self.open_session(run, &owner, "lead", &spec, &log, "负责人").await;
        let (conn, mut rx, mut ctl) = match opened {
            Ok(opened) => opened,
            Err(err) => {
                self.runs.lock().unwrap().remove(&run.id);
                return Err(err);
            }
        };
        let result = async {
            let board = if conn.capabilities["mcpCapabilities"]["http"] == true {
                vec![self.mcp_server(run, "lead")]
            } else {
                vec![]
            };
            let opening = self.opening(
                run,
                &lead,
                &conn.capabilities,
                board,
                (channel == Channel::System).then_some(system.as_str()),
            );
            let mut loaded = false;
            let mut session = Value::Null;
            if conn.capabilities["loadSession"] == true
                && let Some(old) = old_session
            {
                let params = opening.load_params(&old, &workspace);
                if let Ok(Ok(r)) = tokio::time::timeout(STARTUP_TIMEOUT, conn.request("session/load", params)).await {
                    drain_replay(&conn, &mut rx).await;
                    let mut r = if r.is_object() { r } else { json!({}) };
                    r["sessionId"] = json!(old);
                    session = r;
                    loaded = true;
                }
            }
            if !loaded {
                session = tokio::time::timeout(STARTUP_TIMEOUT, new_session(&conn, &workspace, &opening))
                    .await
                    .map_err(|_| anyhow!("负责人打开会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
            }
            let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
            if sid.is_empty() {
                bail!("恢复会话没有返回 sessionId");
            }
            let mut applied = configure_session(&conn, &session, &lead).await?;
            applied["resources"] = json!(opening.resources);
            let controls = crate::acp_view::controls(&session, &applied);
            self.emit(
                run,
                ev("agent.status", "lead", "继续之前的对话")
                    .data(json!({ "phase": AgentPhase::Thinking, "session": sid, "config": applied, "mode": "chat" })),
            );
            {
                let mut st = run.st.lock().unwrap();
                st.lead.session = Some(sid.clone());
                if !st.lead.goal.ended() {
                    st.lead.goal = LeadGoal::Done(delivery_summary(&events));
                }
            }
            let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
            // A loaded session may not return its options. Keep the controls already in the
            // event log instead of replacing them with an empty list.
            let has_controls = controls["model"].is_object()
                || controls["mode"].is_object()
                || controls["controls"].as_array().is_some_and(|a| !a.is_empty());
            if has_controls {
                self.show(run, ("session.controls", "会话选项".into(), controls), &ctx);
            }
            self.record_system(run, &ctx, &sid, &channel, "向负责人注入对话背景", &brief);
            self.record_instructions(run, &ctx, &sid, &channel, &standing);

            // The message that reopened the run was queued as a note; otherwise wait for one.
            let mut prompt = {
                let mut st = run.st.lock().unwrap();
                st.notes.remove("lead").unwrap_or_default()
            };
            if prompt.is_empty() {
                match self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? {
                    Some(text) => prompt.push(Msg { id: "chat-open".into(), text }),
                    None => return Ok(()),
                }
            } else {
                let ids: Vec<&str> = prompt.iter().map(|m| m.id.as_str()).collect();
                self.emit(
                    run,
                    ev(
                        "message.delivered",
                        "janus",
                        format!("恢复会话，把你留给负责人的 {} 条消息发出去", prompt.len()),
                    )
                    .to("lead")
                    .data(json!({ "ids": ids })),
                );
            }
            let mut next = prompt.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n");
            if !loaded && channel == Channel::Prompt {
                next = format!("{brief}\n\n用户接着说：\n{next}");
            }
            loop {
                let goal_turn = next.trim_start().starts_with("/goal ") || run.st.lock().unwrap().lead.goal_active;
                self.record_session_prompt(run, &ctx, &sid, PromptChannel::of_lead_prompt(&next), &next);
                let text = self
                    .stream_prompt(
                        run,
                        &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
                        &next,
                        &ctx,
                        Some(&mut ctl),
                        None,
                    )
                    .await?;
                if let Some(reason) = goal_refused(&text) {
                    self.emit(
                        run,
                        ev("session.prompt", "janus", format!("负责人没有把它当作新的目标：{reason}")).to("lead"),
                    );
                }
                if !ctl.held_by_person() && !ctl.queued.is_empty() {
                    next = self.take_queued(run, &mut ctl, &ctx, |n| format!("这一轮结束，把 {n} 条消息发给负责人"));
                    continue;
                }
                if goal_turn {
                    run.st.lock().unwrap().lead.goal = LeadGoal::Done(text.clone());
                    run.wake.notify_one();
                    if !run.driving.active() {
                        self.deliver(run);
                    }
                }
                ctl.next_turn(run);
                match self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? {
                    Some(text) => next = text,
                    None => return Ok(()),
                }
            }
        }
        .await;
        run.st.lock().unwrap().lead.session = None;
        self.close_session(run, &owner, Some(&conn), "负责人的会话已结束，撤回这个请求").await;
        // Without a live session the run is read-only again; the next message reopens it. If the
        // scheduler is still running tasks, leave the run reachable until they finish.
        if run.driving.active() {
            let app = self.clone();
            let r = run.clone();
            tokio::spawn(async move {
                r.driving.idle().await;
                app.runs.lock().unwrap().remove(&r.id);
            });
        } else {
            self.runs.lock().unwrap().remove(&run.id);
        }
        run.lead_nudge.notify_one();
        result
    }

    /// After the first goal, the lead session is a regular conversation: wait for the person's
    /// next task (or `/goal`), answer the agent's requests on the way, and give up if it exits.
    pub(super) async fn wait_for_lead_message(
        &self,
        run: &Run,
        conn: &AcpConn,
        rx: &mut mpsc::UnboundedReceiver<Incoming>,
        sid: &str,
        ctl: &mut Control,
        ctx: &Ctx<'_>,
    ) -> Result<Option<String>> {
        loop {
            tokio::select! {
                cmd = ctl.rx.recv() => match cmd {
                    None => return Err(Cancelled.into()),
                    Some(Steer::Message(m)) => {
                        self.route_message(run, "lead", "", 0, &m.id, Delivery::Prompt);
                        return Ok(Some(m.text));
                    }
                    Some(Steer::Control(c)) => self.apply_control(run, conn, sid, c, ctx).await,
                    Some(Steer::Finish) => return Ok(None),
                    Some(Steer::TakeOver | Steer::Interrupt | Steer::Release | Steer::Redirect(_)) => {}
                },
                msg = rx.recv() => match msg {
                    None | Some(Incoming::Closed) => return Err(anyhow!("Agent 进程已退出")),
                    Some(Incoming::Request { id, method, .. }) => {
                        if is_user_request(&method) {
                            conn.respond(id, cancelled_reply(&method)).await?;
                        } else {
                            conn.respond_error(id, "janus does not implement this client method").await?;
                        }
                    }
                    Some(Incoming::Notification { .. }) => {}
                }
            }
        }
    }

    /// Janus's part of the system layer is invisible in the transcript: record it, with its
    /// channel, the way `record_instructions` records the person's part.
    pub(super) fn record_system(
        &self,
        run: &Run,
        ctx: &Ctx<'_>,
        sid: &str,
        channel: &Channel,
        label: &str,
        text: &str,
    ) {
        let via = match channel {
            Channel::System => "system",
            Channel::Developer => "developer",
            Channel::Prompt => return,
        };
        self.emit(
            run,
            ev("session.prompt", "janus", label)
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": "system", "via": via, "session": sid, "prompt": text })),
        );
    }

    /// What went into the system layer is invisible in the transcript: record it, with its channel.
    pub(super) fn record_instructions(&self, run: &Run, ctx: &Ctx<'_>, sid: &str, channel: &Channel, text: &str) {
        let via = match channel {
            Channel::System => "system",
            Channel::Developer => "developer",
            Channel::Prompt => return,
        };
        if text.is_empty() {
            return;
        }
        self.emit(
            run,
            ev("session.prompt", "janus", "向 Agent 注入用户的长期要求")
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": "instructions", "via": via, "session": sid, "prompt": text })),
        );
    }

    pub(super) fn record_session_prompt(
        &self,
        run: &Run,
        ctx: &Ctx<'_>,
        sid: &str,
        channel: PromptChannel,
        prompt: &str,
    ) {
        self.emit(
            run,
            ev("session.prompt", "janus", channel.label())
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": channel.as_str(), "session": sid, "prompt": prompt })),
        );
    }

    /// What the lead knows about its team and how to work with it. Everything it learns about
    /// the collaborators comes from here (and the `team` tool, which repeats it).
    // @project-doc docs/domains/20261031-goal_mode.md#lead_brief
    pub(super) fn goal_brief(&self, run: &Run, resumed: bool) -> String {
        let mut p = format!(
            "你是 Janus 团队的负责人，负责完成用户交给你的目标。你可以自己从头做到尾；也可以在需要时把一部分工作交给下面的协作者。是否需要协作、交给谁、怎么拆，都由你判断，不要为了协作而协作。

工作目录：{cwd}（你和协作者都直接在这里工作，改动立即彼此可见）。{git}

协作者：
{team}

和协作者协作，都通过 janus MCP 工具和共享看板：
- janus.orchestrate：提交一份编排（一组子任务：id、标题、给执行者的完整说明、执行者、依赖、验收标准，以及可选的负责文件 owns 和独立验证 verifies）。Janus 先校验，不通过会返回全部错误，改好再提交；通过后子任务写入看板并按依赖执行。可以多次提交，后提交的子任务可以依赖之前的子任务。
- janus.wait：取回看板上的新消息并等待进展：子任务开始、完成（摘要、证据、风险、产物）、失败，协作者写回的进展和阻塞，协作者的提问，以及用户的回答。每次最多等 45 秒，没有新消息就再调用。完成消息只有摘要，结果太长时会注明有删节。
- janus.task_detail：按 id 查看一个子任务的全部信息：说明、完整结果、修订、改过的文件、看板上的问答，以及它的上游和下游子任务。
- janus.reply：回答协作者的提问，或给正在执行的协作者补充指示。消息会立即送达：协作者支持插话时插入当前这一轮，否则打断它当前这一轮后发送。
- janus.retry：重新执行失败或已取消的子任务，可以同时修改说明和验收标准。被它阻塞的子任务会在它成功后自动继续，不需要重新编排。
- janus.accept：执行者自认没有完全达到标准、但你核对后认为结果可以接受时，接受这个结果，让依赖它的子任务继续。
- janus.amend：推翻或修正一个已完成子任务的结论时使用，依赖它的子任务会收到修订。不要只在自己的对话里改主意。
- janus.cancel：取消不再需要的子任务（执行中的会被停止），依赖它的子任务一并取消。
- janus.ask_user：把只有用户能做的决定交给用户，答案会出现在 janus.wait 里。
- janus.team：查看协作者和看板上所有子任务的当前状态。

你在做别的事情时，看板上需要你处理的消息（协作者的提问、失败、停滞、阻塞、两个子任务改了同一个文件，以及用户的回答）会由 Janus 以「[Janus 看板推送]」开头直接插进你的会话，不需要等下一次 janus.wait；推送过的消息不会再出现在 janus.wait 里。收到后先处理会卡住别人的事（回答提问、处理失败），再继续手上的工作。

谁来做决定：
- 技术选型、实现方式、方案取舍、协作者提出的这类问题，由你直接决定并说明理由，不要问用户。
- 只有以下情况用 janus.ask_user 交给用户：目标或需求有多种理解、会得到明显不同的结果；需要用户的账号、凭据、付费或外部资源；不可逆或影响工作目录之外的操作。问的时候给出选项和你的建议，并继续推进不依赖这个决定的工作。
- 用户已经直接回答过的问题（看板上显示「用户已回答」），按用户的回答推进，不要再给出不同的答案。
- 协作者的结论和你的决定冲突时，用 janus.amend 修订它的结论，让下游以同一个结论为准。

验收标准（子任务的 accept）：
- 每个子任务都要写可核对的验收标准：做到什么、怎样算做到。写结果和行为（「点击格子能翻开并显示周围雷数」「新增测试覆盖空输入并通过」），不要写成某条具体命令。
- 怎么验证由执行者自己判断，它会逐条给出是否达到、证据和可选的证据方法（test、command、inspection、reasoning、unverified）。方法只说明依据的类型，不代表结果是真实的：推理判断、未核实和没有说明方法的项会作为提醒出现在完成消息里，关键结果由你自己核对。需要特定的验证方式时，写进标准或说明里，作为要求而不是唯一的检查。
- 执行者自认没有达到、或漏了某条标准时，Janus 会让它在原会话里补一次；仍然不行才交给你：结果其实可以接受 → janus.accept；需要返工或标准要调整 → janus.retry（可改说明和标准）；不需要了 → janus.cancel。
- 执行者的证据是它的说法。关键的结果由你自己核对（读代码、运行、看实际表现），改了代码的目标在结束前安排独立评审。

协作要点：
- 协作者看不到你的对话，子任务说明要写清背景、具体要做什么和验收标准。
- 每个协作者同一时间只做一个子任务，派给同一人的子任务会排队；会改同一批文件的子任务要用依赖串行。两个同时执行的子任务改了同一个文件时，看板上会出现「[冲突]」，你来确认分工。
- 子任务派出去后，不要自己去改它负责的文件：要调整就用 janus.reply 告诉执行者；决定自己做，就先用 janus.cancel 取消它。
- 协作者提问时会停下来等你，用 janus.reply 回答。
- 协作者完成后，你要自己核对结果（读文件、运行测试），确认目标真正达成后再结束。
- 不要 git commit、push 或部署。

{teamwork}

工作区共享资料：
- 共享资料是用户为这个工作区维护的长期背景（产品定位、目标用户、约定、过去的决定、重要文件的位置），你和每个协作者都能看到。它和本次目标冲突时，以本次目标为准。
- 已经写在共享资料里的事实，不要再派子任务重新调研，也不要再问用户。
- 结束时，如果这次确定了值得长期保留的新事实或改变了已有的事实，在最后一条回复的末尾加一个标题「## 建议更新共享资料」，下面每行一条，写成可以直接放进共享资料的完整陈述。只写新增或需要修改的内容，不要重复已有的；没有就不要写这个标题。是否采纳由用户决定。{brief}",
            cwd = run.cwd.display(),
            git = if run.git { "这是一个 Git 仓库。" } else { "这不是 Git 仓库，git 命令在这里不可用。" },
            team = team_lines(run),
            brief = brief_section(&run.brief, "本次目标"),
            teamwork = crate::teamwork::lead_guide(),
        );
        if resumed {
            let st = run.st.lock().unwrap();
            let tasks: Vec<String> = st
                .tasks
                .iter()
                .map(|t| {
                    format!(
                        "- {}（{}）：{} · {}{}",
                        t.def.id,
                        t.agent,
                        t.def.title,
                        t.status,
                        t.result.as_ref().map(|r| format!(" · {}", short(&r.summary, 80))).unwrap_or_default()
                    )
                })
                .collect();
            p.push_str(&format!(
                "\n\n这是从中断处继续：上一次会话已经结束，工作目录里保留了之前的改动。{}",
                if tasks.is_empty() {
                    "之前没有交给协作者的子任务。".to_string()
                } else {
                    format!("看板上之前的子任务：\n{}\n未完成的子任务会重新开始。", tasks.join("\n"))
                }
            ));
        }
        if !run.goal_images.is_empty() {
            p.push_str(&format!(
                "\n\n用户随目标附了 {} 张图片，它们是目标的一部分，开始前先用读取图片的工具查看：\n{}\n需要协作者参考这些图片时，把路径写进子任务说明。",
                run.goal_images.len(),
                crate::attachments::lines(&run.goal_images)
            ));
        }
        p
    }
}

/// One line per collaborator, as the lead is told about them: duty, agent, model, permission mode.
pub(super) fn team_lines_vec(run: &Run) -> Vec<String> {
    run.team
        .iter()
        .filter(|m| !m.lead)
        .map(|m| {
            format!(
                "- {}：{}（{}）· 职责：{} · Agent {} · 模型 {} · 权限模式 {}",
                m.id,
                m.name,
                m.role,
                if m.desc.trim().is_empty() { "未填写" } else { m.desc.trim() },
                if m.runtime_name.is_empty() { m.kind.as_str() } else { &m.runtime_name },
                if m.model_label.is_empty() { "Agent 默认" } else { &m.model_label },
                if m.permission.is_empty() { "未声明，工具调用直接执行" } else { &m.permission },
            )
        })
        .collect()
}

pub(super) fn team_lines(run: &Run) -> String {
    let lines = team_lines_vec(run);
    if lines.is_empty() { "- （没有协作者，所有工作由你自己完成）".into() } else { lines.join("\n") }
}

/// The `/goal` text: the user's goal alone (Claude Code judges completion against it), within
/// the agents' length limits.
pub(super) fn goal_condition(goal: &str) -> String {
    let goal = goal.trim().replace('\n', " ");
    goal.chars().take(3800).collect()
}

/// The agent turned `/goal` down instead of starting on it.
pub(super) fn goal_refused(text: &str) -> Option<String> {
    [
        "only available in trusted workspaces",
        "can't run while hooks are restricted",
        "Goal condition is limited to",
        "requires goal text of at most",
    ]
    .iter()
    .find(|m| text.contains(**m))
    .map(|_| short(text.trim(), 200))
}

/// The lead's last reply: its trailing run of `agent.message` pieces (nothing else of the lead's
/// between them), joined back together.
pub(super) fn last_reply(events: &[Event]) -> Option<String> {
    // The lead's own steps, and what reached it (a prompt from Janus, a message): each ends a reply.
    let lead = |e: &&Event| {
        e.task.is_none()
            && (e.actor == "lead" || e.to.as_deref() == Some("lead"))
            && !matches!(e.kind.as_str(), "agent.status" | "session.usage" | "agent.thought" | "message.routed")
    };
    let mine: Vec<&Event> = events.iter().filter(lead).collect();
    let tail: Vec<&str> = mine
        .iter()
        .rev()
        .skip_while(|e| e.kind != "agent.message")
        .take_while(|e| e.kind == "agent.message")
        .filter_map(|e| e.data.as_ref()?["text"].as_str())
        .collect();
    (!tail.is_empty()).then(|| tail.into_iter().rev().collect::<String>().trim().to_string())
}

/// The lead's last message as the run's summary, without a trailing JSON block if it left one.
pub(super) fn lead_summary(text: &str) -> String {
    let text = match text.rfind("```json") {
        Some(i) => &text[..i],
        None => text,
    };
    short(text.trim(), 2000)
}

/// The workspace's shared material as a section of a prompt; nothing when the workspace has none.
/// `wins` names what takes precedence when the two disagree.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
pub(super) fn brief_section(brief: &str, wins: &str) -> String {
    if brief.trim().is_empty() {
        return String::new();
    }
    format!("\n\n工作区共享资料（用户维护的长期背景，与{wins}冲突时以{wins}为准）：\n{}\n", brief.trim())
}

/// Splits the lead's last message into its summary and the lines it suggests adding to the
/// workspace's shared material: the list under a `BRIEF_HEADING` heading, up to the next heading.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
pub(super) fn split_brief_suggestions(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_heading = |l: &str| {
        let t = l.trim().trim_start_matches('#').trim().trim_end_matches(['：', ':']).replace("**", "");
        t == BRIEF_HEADING
    };
    let Some(start) = lines.iter().position(|l| is_heading(l)) else {
        return (text.to_string(), vec![]);
    };
    let end =
        lines[start + 1..].iter().position(|l| l.trim_start().starts_with('#')).map_or(lines.len(), |i| start + 1 + i);
    let suggestions = lines[start + 1..end]
        .iter()
        .map(|l| {
            let t = l.trim();
            let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
            let t = t
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                .map_or(t, |(_, rest)| rest);
            t.trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    let rest = [&lines[..start], &lines[end..]].concat().join("\n");
    (rest, suggestions)
}

/// The ACP session id of the lead's last session, from the event log.
pub(super) fn last_lead_session(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|e| {
        if e.task.is_some() || e.attempt.is_some() {
            return None;
        }
        let lead = e.actor == "lead" || e.to.as_deref() == Some("lead");
        lead.then(|| e.data.as_ref()?.get("session")?.as_str().map(str::to_owned)).flatten()
    })
}

/// The summary of the last delivery in the event log.
pub(super) fn delivery_summary(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "run.delivered")
        .and_then(|e| e.data.as_ref()?.get("summary")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Context for a conversation in a new lead session: the goal, the delivery, and the last things
/// said. Used when the adapter cannot load the original session.
pub(super) fn conversation_brief(run: &Run, events: &[Event]) -> String {
    let summary = delivery_summary(events);
    let mut recent: Vec<String> = events
        .iter()
        .rev()
        .filter(|e| e.task.is_none() && e.attempt.is_none())
        .filter_map(|e| match e.kind.as_str() {
            "agent.message" if !e.text.trim().is_empty() => Some(format!("负责人：{}", short(e.text.trim(), 500))),
            "user.message" if !e.text.trim().is_empty() => Some(format!("用户：{}", short(e.text.trim(), 500))),
            _ => None,
        })
        .take(12)
        .collect();
    recent.reverse();
    format!(
        "这个目标已经交付，用户现在要继续对话。请根据下面的历史接着做，不要重新开始整个目标。\n\n目标：\n{}\n\n上一轮交付摘要：\n{}\n\n最近的对话：\n{}",
        run.goal,
        if summary.is_empty() { "（没有记录）" } else { summary.as_str() },
        recent.join("\n")
    )
}
