//! A run's main session as a conversation: a chat, a Loop's session, a consolidation, and the
//! lead's session after delivery. It opens (loading the old session when it can), takes the
//! person's messages turn by turn and closes when the run leaves memory.
// @project-doc docs/domains/20261120-chats.md#chat_session

use super::actions::reached_agent;
use super::chat::chat_brief;
use super::*;
use crate::lang::{Lang, tr};

impl App {
    /// Run a conversation in the background (`lead_conversation`). When it cannot open or its
    /// session breaks off, the run leaves the live map, so the next message reopens it; a chat
    /// also records why, since nobody else would tell the person.
    pub(super) fn spawn_conversation(self: &Arc<Self>, run: Arc<Run>) {
        let app = self.clone();
        tokio::spawn(async move {
            let Err(err) = app.lead_conversation(&run).await else { return };
            if err.downcast_ref::<Cancelled>().is_some() || run.st.lock().unwrap().cancelled() {
                return;
            }
            // A Loop's session ending ends its wake (nothing for another kind: it has no Loop).
            app.loop_session_ended(&run, &format!("{err:#}"));
            if run.kind.single_session() && !run.driving.active() {
                app.runs.lock().unwrap().remove(&run.id);
                app.emit(
                    &run,
                    ev(EventKind::RunFailed, "janus", format!("对话的会话结束了：{err:#}。再发一条消息会重新打开会话"))
                        .data(json!({ "error": format!("{err:#}") })),
                );
                app.set_status(&run, RunStatus::Failed);
            }
            crate::config::log(format!("janus: conversation {} ended: {err:#}", run.id));
        });
    }

    /// A conversation in the run's main session. For a delivered or accepted run it is the lead's
    /// regular conversation after the goal; for a chat, the conversation with its one digital
    /// employee, without board, goal or delivery. The session is restored with `session/load`
    /// when the adapter still has it, with the system layer it already has, so it is told nothing
    /// again; otherwise a new session starts with a compact brief of what was said so far (and,
    /// for a goal, the delivery).
    // @project-doc docs/domains/20261120-chats.md#chat_session
    pub(super) async fn lead_conversation(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        // A chat and a Loop's session: one member, no board to run, no delivery.
        let chat = run.kind.single_session();
        let lead = run.team.iter().find(|m| m.lead).context("no lead")?.clone();
        let standing = standing_instructions(&lead, run.lang);
        let channel = lead.runtime.instruction_channel();
        let events = self.db.events(&run.id, 0).unwrap_or_default();
        let old_session = last_lead_session(&events);
        // How to work with Janus (the lead's brief after delivery, a Loop's rules), and for a chat
        // or a Loop opened in a new session, what was said so far.
        // @project-doc docs/domains/20261121-loops.md#loop_session
        let guide = match run.kind {
            RunKind::Goal => format!(
                "{}{}{}",
                goal_brief(run),
                conversation_brief(run, &events),
                self.memory_section(run, MemoryAccess::Write)
            ),
            RunKind::Loop => format!("{}{}", self.loop_brief(run), self.memory_section(run, MemoryAccess::Write)),
            RunKind::Dream => self.dream_brief(run),
            RunKind::Chat => String::new(),
        };
        let history = if chat { chat_brief(&events, run.lang) } else { String::new() };
        let fresh = system_layer(&[&guide, &history, &standing]);
        let old = old_session.as_deref().map(|sid| {
            let kept = recorded_system(&events, sid).unwrap_or_else(|| standing.clone());
            (sid, kept)
        });
        let owner = Owner { session: SessionKey::Lead, attempt: 0 };
        let opened = self
            .open_conversation(
                run,
                &lead,
                &channel,
                old.as_ref().map(|(s, k)| (*s, k.as_str())),
                &fresh,
                run.kind.conversation_log(),
            )
            .await;
        let Conversation { conn, mut rx, mut ctl, session, loaded, resources } = match opened {
            Ok(opened) => opened,
            Err(err) => {
                self.runs.lock().unwrap().remove(&run.id);
                return Err(err);
            }
        };
        let result = async {
            let (sid, applied, controls) = main_session_ready(&conn, &session, &lead, &resources).await?;
            let opened = match (chat, loaded || old_session.is_some()) {
                (true, false) => "开始对话",
                (true, true) if loaded => "恢复了之前的会话",
                (true, true) => "新开会话，带上之前的对话",
                (false, _) => "继续之前的对话",
            };
            self.emit(
                run,
                ev(EventKind::AgentStatus, "lead", opened)
                    .data(json!({ "phase": AgentPhase::Thinking, "session": sid, "config": applied, "mode": "chat" })),
            );
            {
                let mut st = run.st.lock().unwrap();
                st.lead.session = Some(sid.clone());
                if !chat && !st.lead.goal.ended() {
                    st.lead.goal = LeadGoal::Done(delivery_summary(&events));
                }
            }
            if chat {
                self.set_status(run, RunStatus::Running);
            } else {
                // Its turns hear about the board as the first goal's did.
                let (app, r) = (self.clone(), run.clone());
                tokio::spawn(async move { app.push_loop(&r).await });
            }
            let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
            // A loaded session may not return its options. Keep the controls already in the
            // event log instead of replacing them with an empty list.
            if has_controls(&controls) {
                self.show(run, (EventKind::SessionControls, "会话选项".into(), controls), &ctx);
            }
            // A loaded session already has its system layer and got it again unchanged: nothing new
            // went in, so nothing new is recorded.
            if !loaded {
                if !guide.is_empty() {
                    self.record_system(run, &ctx, &sid, &channel, SystemPart::Collaboration, &guide);
                }
                if !history.is_empty() {
                    self.record_system(run, &ctx, &sid, &channel, SystemPart::History, &history);
                }
                self.record_system(run, &ctx, &sid, &channel, SystemPart::Instructions, &standing);
            }

            // The message that reopened the run was queued as a note; otherwise wait for one.
            let mut prompt = {
                let mut st = run.st.lock().unwrap();
                st.notes.remove("lead").unwrap_or_default().into_iter().map(|n| n.msg).collect::<Vec<_>>()
            };
            if prompt.is_empty() {
                if chat {
                    // A new chat, or one reopened without a message waiting: the person writes first.
                    self.emit(
                        run,
                        ev(EventKind::AgentStatus, "lead", "会话已打开，等待你的消息")
                            .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
                    );
                }
                match self.wait_for_lead_message(run, &conn, &mut rx, &sid, &mut ctl, &ctx).await? {
                    Some(text) => prompt.push(Msg { id: "chat-open".into(), text }),
                    None => return Ok(()),
                }
            } else if chat {
                // A chat's messages opened its session: each goes in as this new turn.
                for m in &prompt {
                    self.route_message(run, "lead", "", 0, &m.id, Delivery::Prompt);
                }
            } else {
                let ids: Vec<&str> = prompt.iter().map(|m| m.id.as_str()).collect();
                self.emit(
                    run,
                    ev(
                        EventKind::MessageDelivered,
                        "janus",
                        format!("恢复会话，把你留给负责人的 {} 条消息发出去", prompt.len()),
                    )
                    .to("lead")
                    .data(json!({ "ids": ids })),
                );
            }
            let mut next = prompt.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n");
            loop {
                let session = &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                let channel = PromptChannel::of_lead_prompt(&next);
                let (text, goal_turn) = self.main_turn(run, session, &mut ctl, &ctx, channel, &next).await?;
                match run.kind {
                    RunKind::Loop => self.loop_turn_ended(run),
                    // A consolidation is one turn: on record, delivered, and its session closes.
                    RunKind::Dream => {
                        self.dream_turn_ended(run, &text);
                        return Ok(());
                    }
                    RunKind::Goal | RunKind::Chat => {}
                }
                if let Some(reason) = goal_refused(&text) {
                    self.emit(
                        run,
                        ev(EventKind::SessionPrompt, "janus", format!("负责人没有把它当作新的目标：{reason}"))
                            .to("lead"),
                    );
                }
                if !ctl.held_by_person() && !ctl.queued.is_empty() {
                    next = self.take_queued(run, &mut ctl, &ctx, |n| format!("这一轮结束，把 {n} 条消息发给负责人"));
                    continue;
                }
                // Interrupted: the person holds the lead, and decides whether it goes on or the
                // turn's work stands as it is. Nothing is delivered (and so merged) before that.
                if !chat && ctl.held_by_person() {
                    let session = PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                    if let Some(text) =
                        self.wait_for_person(run, &session, &mut ctl, &ctx, &resume_after_hold(run, true)).await?
                    {
                        next = text;
                        continue;
                    }
                }
                if chat {
                    // Nothing to deliver: the reply is the result. The session waits for the next message.
                    self.lead_turn_idle(run);
                } else {
                    self.turn_after_delivery(run, &text, goal_turn);
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
        self.end_main_session(run, &owner, &conn).await;
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
        result
    }

    /// One turn of a run's main session: `prompt` recorded as sent on `channel`, streamed until
    /// the turn ends, and what the run changed meanwhile recorded. Returns the reply and whether
    /// the turn belonged to a goal (a `/goal` command, or a message while one was active).
    pub(super) async fn main_turn(
        &self,
        run: &Run,
        session: &mut PromptSession<'_>,
        ctl: &mut Control,
        ctx: &Ctx<'_>,
        channel: PromptChannel,
        prompt: &str,
    ) -> Result<(String, bool)> {
        let goal_turn = {
            let mut st = run.st.lock().unwrap();
            st.lead.in_turn = true;
            starts_goal(prompt) || st.lead.goal_active
        };
        self.record_session_prompt(run, ctx, session.sid, channel, prompt);
        let text = self.stream_prompt(run, session, prompt, ctx, Some(ctl)).await?;
        self.run_changes_after_turn(run);
        Ok((text, goal_turn))
    }

    /// A run's main session ended, however it did: nothing is in it any more, what it waited for
    /// is withdrawn, its agent stops, and whoever waits on the lead hears.
    pub(super) async fn end_main_session(&self, run: &Run, owner: &Owner, conn: &Arc<AcpConn>) {
        {
            let mut st = run.st.lock().unwrap();
            st.lead.session = None;
            st.lead.in_turn = false;
        }
        self.close_session(run, owner, Some(conn), "负责人的会话已结束，撤回这个请求").await;
        run.lead_nudge.notify_one();
    }

    /// Open a run's main session: a conversation's, or the lead's goal session (new, or continued
    /// after a restart). `old` is the session to load and the system layer it already has: it is
    /// loaded with exactly that layer. When it cannot be loaded, a new session starts with
    /// `fresh`, which carries what the agent needs to know. Codex reads its layer from the
    /// environment its adapter starts with, so that new session needs a new adapter process.
    /// The agent's output goes to `log` in the run's log directory.
    pub(super) async fn open_conversation(
        &self,
        run: &Arc<Run>,
        lead: &Member,
        channel: &InstructionChannel,
        old: Option<(&str, &str)>,
        fresh: &str,
        log: &str,
    ) -> Result<Conversation> {
        let owner = Owner { session: SessionKey::Lead, attempt: 0 };
        let mut started = None;
        if let Some((sid, kept)) = old {
            let (conn, mut rx, ctl) = self.start_conversation_agent(run, lead, kept, log).await?;
            if conn.capabilities["loadSession"] == true {
                let opening = self.conversation_opening(run, lead, &conn, channel, kept);
                let params = opening.load_params(sid, &run.cwd);
                if let Ok(Ok(session)) =
                    tokio::time::timeout(STARTUP_TIMEOUT, load_session(&conn, &mut rx, params, sid)).await
                {
                    let resources = opening.resources;
                    return Ok(Conversation { conn, rx, ctl, session, loaded: true, resources });
                }
            }
            if *channel == InstructionChannel::Developer && kept != fresh {
                self.close_session(run, &owner, Some(&conn), "重新打开会话，撤回这个请求").await;
            } else {
                started = Some((conn, rx, ctl));
            }
        }
        let (conn, rx, ctl) = match started {
            Some(started) => started,
            None => self.start_conversation_agent(run, lead, fresh, log).await?,
        };
        let opening = self.conversation_opening(run, lead, &conn, channel, fresh);
        let session = tokio::time::timeout(STARTUP_TIMEOUT, new_session(&conn, &run.cwd, &opening))
            .await
            .map_err(|_| anyhow!("负责人打开会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))
            .and_then(|r| r);
        match session {
            Ok(session) => Ok(Conversation { conn, rx, ctl, session, loaded: false, resources: opening.resources }),
            Err(err) => {
                self.close_session(run, &owner, Some(&conn), "负责人的会话已结束，撤回这个请求").await;
                Err(err)
            }
        }
    }

    /// Start the adapter of a conversation's main session, with `system` as its system layer where
    /// the agent reads it from its environment (Codex) and the settings of its permission mode.
    pub(super) async fn start_conversation_agent(
        &self,
        run: &Run,
        lead: &Member,
        system: &str,
        log: &str,
    ) -> Result<(Arc<AcpConn>, mpsc::UnboundedReceiver<Incoming>, Control)> {
        let spec = member_spec(self.spec_for(&lead.connection)?, lead, system);
        if lead.runtime.trusts_workspaces() {
            // Claude Code only runs /goal in a workspace it trusts, in a chat too.
            self.trust_claude_workspace(&lead.connection, &run.cwd)?;
        }
        // The library router's URL carries this secret; a run reopened after a restart has none yet.
        let token = random_token()?;
        {
            let mut st = run.st.lock().unwrap();
            if st.mcp_token.is_empty() {
                st.mcp_token = token;
            }
        }
        let log = self.dirs(run).log(log);
        let who = if run.kind.single_session() { lead.name.as_str() } else { "负责人" };
        self.open_session(run, &Owner { session: SessionKey::Lead, attempt: 0 }, "lead", &spec, &log, who).await
    }

    /// The opening of a conversation's main session, with `system` as its system layer where the
    /// agent takes it in the session's parameters (Claude Code). A chat has no board: its one
    /// member has no team to lead; it reaches the `janus` server only for the tools its digital
    /// employee has (`Caller::of_run`).
    pub(super) fn conversation_opening(
        &self,
        run: &Run,
        lead: &Member,
        conn: &AcpConn,
        channel: &InstructionChannel,
        system: &str,
    ) -> Opening {
        let board = if Caller::of_run(run, &SessionKey::Lead).is_some()
            && conn.capabilities["mcpCapabilities"]["http"] == true
        {
            vec![self.mcp_server(run, &SessionKey::Lead)]
        } else {
            vec![]
        };
        self.opening(
            run,
            &SessionKey::Lead,
            lead,
            &conn.capabilities,
            board,
            (*channel == InstructionChannel::System).then_some(system),
        )
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
        run.st.lock().unwrap().lead.idle_since = now_ms();
        let mut idle = Box::pin(tokio::time::sleep(self.cfg.idle_after));
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
                    Some(Steer::Park) if self.parkable(run) => {
                        self.park(run, parking::Parked::Full);
                        return Ok(None);
                    }
                    Some(Steer::Park | Steer::TakeOver | Steer::Interrupt | Steer::Release | Steer::Redirect(_)) => {}
                },
                () = &mut idle => {
                    if self.parkable(run) {
                        self.park(run, parking::Parked::Idle);
                        return Ok(None);
                    }
                    idle.as_mut().reset(tokio::time::Instant::now() + self.cfg.idle_after);
                }
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
}

/// A conversation's main session once it is open: loaded (the old session, with the system layer
/// it had) or new.
pub(super) struct Conversation {
    pub(super) conn: Arc<AcpConn>,
    pub(super) rx: mpsc::UnboundedReceiver<Incoming>,
    pub(super) ctl: Control,
    pub(super) session: Value,
    pub(super) loaded: bool,
    pub(super) resources: Vec<crate::library::Delivery>,
}

/// A run's main session just opened: its id, the controls applied to it with the resources it
/// was given, and the options it offers.
pub(super) async fn main_session_ready(
    conn: &AcpConn,
    session: &Value,
    lead: &Member,
    resources: &[crate::library::Delivery],
) -> Result<(String, Value, Value)> {
    let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
    if sid.is_empty() {
        bail!("负责人的会话没有返回 sessionId");
    }
    let mut applied = configure_session(conn, session, lead).await?;
    applied["resources"] = json!(resources);
    let controls = crate::acp_view::controls(session, &applied);
    Ok((sid, applied, controls))
}

/// Whether a session's options (`acp_view::controls`) name anything; a loaded session may return none.
pub(super) fn has_controls(controls: &Value) -> bool {
    controls["model"].is_object()
        || controls["mode"].is_object()
        || controls["controls"].as_array().is_some_and(|a| !a.is_empty())
}

/// Where a delivered goal stands, for a lead whose conversation goes on in a new session: after its
/// brief (the same as the goal's), the goal, the delivery and the last things said. Used when the
/// adapter cannot load the original session.
pub(super) fn conversation_brief(run: &Run, events: &[Event]) -> String {
    let lang = run.lang;
    let summary = delivery_summary(events);
    let summary = if summary.is_empty() {
        lang.pick("（没有记录摘要）", "(No summary recorded)").to_string()
    } else {
        summary
    };
    let recent = recent_exchanges(events, Speakers::Lead, lang);
    let recent = if recent.is_empty() { lang.pick("（无）", "(none)").to_string() } else { recent };
    let goal = &run.goal;
    tr!(
        lang,
        "

交付之后：这个目标已经交付，用户在继续和你对话。这是一个新会话，之前的上下文不在里面，按下面的记录接上。

目标：
{goal}

上次交付：
{summary}

最近的对话：
{recent}",
        "

After delivery: this goal has been delivered, and the user is continuing the conversation. This is a new session without the earlier context; pick up from the record below.

Goal:
{goal}

Last delivery:
{summary}

Recent conversation:
{recent}"
    )
}

/// What the agent of the main session is called in its recent exchanges.
#[derive(Clone, Copy)]
pub(super) enum Speakers {
    /// It reads its own words.
    You,
    /// The lead of a delivered goal.
    Lead,
}

/// The last twelve things the person and the main session's agent (called `agent`) said, one
/// line each, for a new session that cannot load the old one.
pub(super) fn recent_exchanges(events: &[Event], speakers: Speakers, lang: Lang) -> String {
    let agent = match speakers {
        Speakers::You => lang.pick("你：", "You: "),
        Speakers::Lead => lang.pick("负责人：", "Lead: "),
    };
    let user = lang.pick("用户：", "User: ");
    // A message still waiting for the agent is not history: it is the prompt the new session gets.
    let waiting = |i: usize, e: &Event| {
        e.kind() == EventKind::UserMessage
            && e.data.as_ref().and_then(|d| d["id"].as_str()).is_some_and(|id| !reached_agent(id, &events[i + 1..]))
    };
    let mut recent: Vec<String> = events
        .iter()
        .enumerate()
        .rev()
        .filter(|&(i, e)| e.task.is_none() && e.attempt.is_none() && !waiting(i, e))
        .filter_map(|(_, e)| match e.kind() {
            EventKind::AgentMessage if !e.text.trim().is_empty() => {
                Some(format!("{agent}{}", clip(e.text.trim(), 500)))
            }
            EventKind::UserMessage if !e.text.trim().is_empty() => Some(format!("{user}{}", clip(e.text.trim(), 500))),
            _ => None,
        })
        .take(12)
        .collect();
    recent.reverse();
    recent.join("\n")
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

/// The system layer session `sid` last opened with, put back together from what Janus recorded of
/// it (`session.prompt` events with a `via`). Loading the session with exactly this layer changes
/// nothing in it: the adapter re-sends instructions only when the layer differs (codex-acp treats
/// a different one as a system prompt change). A session recorded twice keeps its last layer.
// @project-doc docs/interfaces/20261031-acp_integration.md#standing_instructions
pub(super) fn recorded_system(events: &[Event], sid: &str) -> Option<String> {
    let mut parts: Vec<(&str, &str)> = vec![];
    for d in events.iter().rev().filter(|e| e.kind() == EventKind::SessionPrompt).filter_map(|e| e.data.as_ref()) {
        if d["session"].as_str() != Some(sid) || !d["via"].is_string() {
            continue;
        }
        let channel = d["channel"].as_str().unwrap_or_default();
        if parts.iter().any(|(c, _)| *c == channel) {
            break;
        }
        parts.push((channel, d["prompt"].as_str().unwrap_or_default()));
    }
    if parts.is_empty() {
        return None;
    }
    let texts: Vec<&str> = parts.iter().rev().map(|(_, t)| *t).collect();
    Some(system_layer(&texts))
}
