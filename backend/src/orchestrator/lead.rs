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

    /// The lead finished a turn and waits for the person: a collaborator still waiting on the
    /// board now waits on the person, as it does when the lead's session ends.
    fn lead_goes_idle(&self, run: &Run) {
        run.st.lock().unwrap().lead.in_turn = false;
        self.escalate_open_questions(run);
    }

    /// While the lead's session lasts: whenever the board gets something the lead has to act on,
    /// wait a moment for more, then push it all into the lead's running turn (`push_board`).
    pub(super) async fn push_loop(&self, run: &Run) {
        loop {
            run.lead_nudge.notified().await;
            {
                let st = run.st.lock().unwrap();
                // A session that ended after its goal ends the pushes; before the goal ends the
                // session may not be open yet.
                if st.cancelled() || (st.lead.goal.ended() && st.lead.session.is_none()) {
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
            // After delivery, an idle lead hears about the board when the person writes to it.
            if st.lead.waiting || !st.lead.on_duty() || !st.steerable.contains(&SessionKey::Lead) {
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

    /// Goal mode: once the lead is off duty (its goal ended and it is not working on a message,
    /// or its session ended) nobody reads the board, so a collaborator's question still waiting
    /// for the lead goes to the person instead.
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
            .filter(|t| t.status() == TaskStatus::Running)
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
                    "lead",
                    &lead,
                    &conn.capabilities,
                    board,
                    (channel == Channel::System).then_some(system.as_str()),
                );
                self.prewarm(run, "lead", &opening);
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
                let ack = format!(
                    "{brief}\n\nReply only 'Acknowledged'. The goal will arrive in the next message. Wait for the goal before starting work."
                );
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
                let goal_turn = {
                    let mut st = run.st.lock().unwrap();
                    st.lead.in_turn = true;
                    prompt.trim_start().starts_with("/goal ") || st.lead.goal_active
                };
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
                self.lead_goes_idle(run);
                let Some(next) = self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? else {
                    return Ok(text);
                };
                prompt = next;
            }
        }
        .await;
        {
            let mut st = run.st.lock().unwrap();
            st.lead.session = None;
            st.lead.in_turn = false;
        }
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
                "lead",
                &lead,
                &conn.capabilities,
                board,
                (channel == Channel::System).then_some(system.as_str()),
            );
            self.prewarm(run, "lead", &opening);
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
            // Its turns hear about the board as the first goal's did.
            let (app, r) = (self.clone(), run.clone());
            tokio::spawn(async move { app.push_loop(&r).await });
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
                let goal_turn = {
                    let mut st = run.st.lock().unwrap();
                    st.lead.in_turn = true;
                    next.trim_start().starts_with("/goal ") || st.lead.goal_active
                };
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
                self.lead_goes_idle(run);
                match self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? {
                    Some(text) => next = text,
                    None => return Ok(()),
                }
            }
        }
        .await;
        {
            let mut st = run.st.lock().unwrap();
            st.lead.session = None;
            st.lead.in_turn = false;
        }
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
            "You are the lead of the Janus team and are responsible for achieving the user's goal. First decide whether the goal is small enough to finish yourself in this session; if it is, do it yourself without delegating. Otherwise delegate parts of it to the collaborators below, deciding whom to involve and how to divide the work.

Once you submit any subtask, you only manage for the rest of the goal: plan, read the board, decide, verify, and deliver. Do not edit files in the working directory from then on; reading files, running tests, and inspecting diffs are fine. Janus reports every file you edit after delegating, on the board and as a risk in the delivery. The workspace's resource library (the janus-tools MCP server and the library's skills) is open to you and to every collaborator; after you delegate, use it only to look (read, query, check) and leave anything with side effects to a subtask.

Working directory: {cwd}. You and the collaborators work directly in this directory, where changes are immediately visible to everyone. {git}

Collaborators:
{team}

Use the janus MCP tools and the shared board to coordinate with collaborators:
- janus.orchestrate: Submit a plan as a set of subtasks with an id, title, complete instructions, assignee, dependencies, acceptance criteria, and optional owns, verifies, and continues. Janus validates the entire plan and returns all errors so you can correct and resubmit it. Valid subtasks are added to the board and scheduled by dependency. You can submit multiple plans and depend on subtasks from earlier plans.
- janus.wait: Wait for new board activity: subtask starts, completions (summaries, evidence, risks, and artifacts), failures, collaborator progress, blockers, split proposals, collaborator questions, and user answers. It returns as soon as a subtask completes or something needs you; starts, progress, and other informational entries come along with that return or at the timeout. Each wait lasts at most 180 seconds (120 seconds by default). Longer waits can return live progress for running subtasks. Before waiting again, check whether intervention is needed. If work is progressing normally, keep waiting. Completion entries summarize the result and indicate truncation.
- janus.task_detail: Read a subtask's complete instructions, result, amendments, changed files, board questions and answers, and upstream and downstream subtasks by id.
- janus.reply: Answer a collaborator's question or send new instructions to a running collaborator. The message is delivered immediately by steering the current turn where supported, or by interrupting the turn and then sending the message.
- janus.retry: Send an ended subtask back to its assignee: a failed or cancelled one, or a completed one that needs rework, for example after a verification rejected it. Always choose the session: session=\"continue\" goes back into that subtask's own session, which keeps its context (rework on review findings, filling gaps, finishing interrupted work); session=\"new\" starts a fresh one (a different approach, a polluted context, a broken process). Set from to the verification whose findings it should act on; Janus hands them over verbatim, so add only your own decision in note. You can also revise its instructions and acceptance criteria. Once it is done, its verifications and from check the new result again. Blocked subtasks resume automatically after it succeeds, without another plan.
- janus.accept: Accept a result the assignee reported as falling short if your own review establishes that the result is acceptable. This lets dependent subtasks proceed.
- janus.amend: Overturn or correct a completed subtask's findings. Dependent subtasks receive the amendment. Record the change here so it reaches the rest of the team.
- janus.cancel: Cancel a subtask you no longer need. Janus stops it if it is running and cancels its dependent subtasks.
- janus.ask_user: Escalate decisions only the user can make. The answer appears in janus.wait.
- janus.team: See the collaborators and the current status of all subtasks on the board.

When you are doing other work, Janus pushes board messages that need your attention directly into your session. These include collaborator questions, failures, stalls, blockers, concurrent file edits, and user answers. Pushed messages do not reappear in janus.wait. Address anything blocking others first, then resume your work.

Decision ownership:
- Decide technical choices, implementation details, and trade-offs yourself. Explain your reasoning when answering collaborators.
- Use janus.ask_user when different interpretations of the goal or requirements would lead to materially different results, when you need the user's account, credentials, spending, or external resources, or when an action is irreversible or affects anything outside the working directory. Give options and a recommendation. Continue work that does not depend on the decision.
- If the user has already answered a question directly, proceed with the user's answer.
- When a collaborator's findings conflict with your decision, use janus.amend so downstream work uses the same findings.

Acceptance criteria (the subtask's accept field):
- Write verifiable criteria for every subtask. State the expected outcome and how to recognize it. Describe behavior, such as revealing a tile and showing the number of adjacent mines when clicked, or covering empty input with a new passing test. Avoid prescribing a specific command as the criterion.
- The assignee chooses how to verify each criterion and reports the status, evidence, and optional method (test, command, inspection, reasoning, or unverified). The method identifies the evidence type. Reasoning, unverified results, and unspecified methods are flagged in completion messages. Verify critical results yourself. If a particular verification method is required, include it in the criteria or instructions and still assess the outcome.
- If the assignee reports an unmet criterion or omits one, Janus gives it one chance to address the gap in the original session. If it still falls short, use janus.accept when the result is acceptable, janus.retry when the work or criteria need revision, or janus.cancel when the subtask is no longer needed.
- The assignee's evidence is its report. Before you finish, make sure every subtask that changed files was checked: by an independent verification you arranged, or by your own checks (inspecting code, running tests, observing behavior). The delivery marks each subtask that changed files without an independent verification; for each of them, your final summary must say how you checked it, what you ran or read and what you saw.

Coordination:
- Collaborators cannot see your conversation. Include the context, exact work, and acceptance criteria in each subtask's instructions.
- Each collaborator instance handles one subtask at a time. Extra subtasks assigned to the same collaborator queue. Use dependencies to serialize work that will edit the same files. When simultaneous subtasks edit the same file, the board reports a conflict for you to resolve.
- After you delegate, never take over a subtask's work yourself, not even a one-line fix. Send it back with janus.retry (session=\"continue\" keeps its context), reassign it with janus.cancel and janus.orchestrate, use janus.ask_user when no collaborator can do it, or cancel it and report the gap in the delivery. Use janus.reply to change the work of a running subtask.
- When a collaborator asks a question, it waits for your answer. Respond with janus.reply.
- Check collaborators' results yourself, by reading changed files or running tests, and finish only when the goal is achieved.
- Do not commit, push, or deploy.

{teamwork}

Shared workspace context:
- The user maintains this context for the workspace. It can include product positioning, target users, conventions, past decisions, and important files. All collaborators receive it. The current goal takes precedence if they conflict.
- Use the facts already recorded without assigning new research or asking the user again.
- If you establish or revise facts worth keeping, end your final reply with the heading '## {heading}' and list one complete, reusable statement per line. Include only new or revised facts. Omit the heading if there are no suggestions. The user decides what to keep.{brief}",
            cwd = run.cwd.display(),
            git = workdir_brief(run),
            team = team_lines(run),
            brief = brief_section(&run.brief, "the current goal"),
            heading = BRIEF_HEADING,
            teamwork = crate::teamwork::lead_guide(),
        );
        if resumed {
            let st = run.st.lock().unwrap();
            let tasks: Vec<String> = st
                .tasks
                .iter()
                .map(|t| {
                    format!(
                        "- {} ({}): {} · {}{}",
                        t.def.id,
                        t.agent,
                        t.def.title,
                        t.status(),
                        t.result.as_ref().map(|r| format!(" · {}", short(&r.summary, 80))).unwrap_or_default()
                    )
                })
                .collect();
            p.push_str(&format!(
                "\n\nResuming after an interruption. The previous session ended, and its changes remain in the working directory. {}",
                if tasks.is_empty() {
                    "No subtasks were assigned to collaborators.".to_string()
                } else {
                    format!("Previous subtasks on the board:\n{}\nUnfinished subtasks will restart.", tasks.join("\n"))
                }
            ));
        }
        if !run.goal_images.is_empty() {
            let images = crate::attachments::lines(&run.goal_images);
            p.push_str(&format!(
                "\n\nThe user attached images to the goal. Use an image-reading tool to review them before starting:\n{images}\nInclude the paths in subtask instructions for collaborators who need these images."
            ));
        }
        p
    }
}

/// One line per collaborator, as the lead is told about them: who it is and what it is responsible
/// for. The lead picks assignees by responsibility alone; agent, model and permission mode stay out.
pub(super) fn team_lines(run: &Run) -> String {
    let lines: Vec<String> = run
        .team
        .iter()
        .filter(|m| !m.lead)
        .map(|m| {
            let desc = m.desc.trim();
            format!("- {}: {} · Responsibilities: {}", m.id, m.name, if desc.is_empty() { "Unspecified" } else { desc })
        })
        .collect();
    if lines.is_empty() { "- No collaborators. Complete the work yourself.".into() } else { lines.join("\n") }
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
    format!("\n\nShared workspace context (maintained by the user). In a conflict, follow {wins}.\n{}\n", brief.trim())
}

/// Splits the lead's last message into its summary and the lines it suggests adding to the
/// workspace's shared material: the list under a `BRIEF_HEADING` heading, up to the next heading.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
pub(super) fn split_brief_suggestions(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_heading = |l: &str| {
        let t = l.trim().trim_start_matches('#').replace("**", "");
        let heading = t.trim().trim_end_matches(['：', ':']).trim();
        heading.eq_ignore_ascii_case(BRIEF_HEADING) || heading == "建议更新共享资料"
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
            "agent.message" if !e.text.trim().is_empty() => Some(format!("Lead: {}", short(e.text.trim(), 500))),
            "user.message" if !e.text.trim().is_empty() => Some(format!("User: {}", short(e.text.trim(), 500))),
            _ => None,
        })
        .take(12)
        .collect();
    recent.reverse();
    format!(
        "This goal has been delivered and the user is continuing the conversation. Use the history below to pick up where you left off.\n\nGoal:\n{}\n\nPrevious delivery summary:\n{}\n\nRecent conversation:\n{}",
        run.goal,
        if summary.is_empty() { "(No summary recorded)" } else { summary.as_str() },
        recent.join("\n")
    )
}
