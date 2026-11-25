//! The lead: its goal session, the conversation after delivery, its instructions and the board
//! pushed into it.

use super::actions::reached_agent;
use super::chat::chat_brief;
use super::*;
use crate::lang::{Lang, tr};

/// A heading Janus asks the lead for in its run's language and reads back in either.
pub(super) struct Heading {
    pub zh: &'static str,
    pub en: &'static str,
}

impl Heading {
    pub(super) fn in_(&self, lang: Lang) -> &'static str {
        lang.pick(self.zh, self.en)
    }
}

/// The heading the lead puts its suggested additions to the shared material under (see `goal_brief`).
pub(super) const BRIEF_HEADING: Heading =
    Heading { zh: "建议更新共享资料", en: "Suggested Updates to Shared Context" };

/// The heading of the lead's conclusion in its final reply, a sentence or two (see `goal_brief`).
pub(super) const HEADLINE_HEADING: Heading = Heading { zh: "结论", en: "Conclusion" };

/// The heading of the questions the lead's final reply leaves for the person (see `goal_brief`).
pub(super) const DECISIONS_HEADING: Heading = Heading { zh: "需要你决定", en: "Needs Your Decision" };

wire_enum! {
    /// What Janus puts in a session's system layer before the person's standing instructions.
    pub(super) enum SystemPart {
        /// How to work with Janus and the team: the lead's brief, a collaborator's rules. Only an
        /// Agent Team session gets it; a chat has the digital employee's instructions alone.
        Collaboration = "system",
        /// A chat reopened: its recent exchanges, so the new session knows the conversation.
        History = "history",
    }
}

/// A conversation's main session once it is open: loaded (the old session, with the system layer
/// it had) or new.
struct Conversation {
    conn: Arc<AcpConn>,
    rx: mpsc::UnboundedReceiver<Incoming>,
    ctl: Control,
    session: Value,
    loaded: bool,
    resources: Vec<crate::library::Delivery>,
}

wire_enum! {
    /// Why Janus sent a prompt into a session (`session.prompt`, `data.channel`).
    pub(super) enum PromptChannel {
        /// `/goal <goal>`.
        Goal = "goal",
        /// What comes next in the lead's session: the person's message, or messages that waited.
        Followup = "followup",
        /// Board entries pushed into the lead's running turn.
        Board = "board",
        /// Janus asks a lead whose goal does not settle whether it is done.
        GoalCheck = "goal_check",
        /// The lead's own session loaded again after a restart: what happened meanwhile.
        Resume = "resume",
        /// A result that did not pass its check, sent back to be fixed.
        Fix = "fix",
    }
}

impl PromptChannel {
    /// A prompt in the lead's session: a slash command starts a goal, anything else follows up.
    pub(super) fn of_lead_prompt(prompt: &str) -> PromptChannel {
        if prompt.starts_with("/goal ") { PromptChannel::Goal } else { PromptChannel::Followup }
    }

    fn label(self) -> &'static str {
        match self {
            PromptChannel::Goal => "向负责人发送目标提示",
            PromptChannel::Followup => "向负责人发送继续提示",
            PromptChannel::Board => "把看板上需要处理的消息推给负责人",
            PromptChannel::GoalCheck => "问负责人目标是否已经完成",
            PromptChannel::Resume => "告诉负责人编排器重启过，接着推进目标",
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
        let token = random_token()?;
        {
            let mut st = run.st.lock().unwrap();
            st.lead.goal = LeadGoal::Working;
            // Changes already in the directory are not this run's: a task's diff starts from here.
            if run.git && st.last_diff.is_empty() {
                st.last_diff = working_diff(&run.cwd, "HEAD", &[]);
            }
            // Continuing: the restarted lead is told the board so far; `wait` brings only what is new.
            if resumed {
                st.lead.cursor = run.seq.load(Ordering::SeqCst);
            }
            if st.mcp_token.is_empty() {
                st.mcp_token = token;
            }
        }
        self.set_status(run, RunStatus::Running);
        // Where the board stood when Janus stopped, taken before the scheduler starts tasks again.
        let board = resumed.then(|| LeadResume {
            board: board_on_resume(run, &self.db.events(&run.id, 0).unwrap_or_default()),
            broke: BrokeOff::Restart,
        });
        let (app, r) = (self.clone(), run.clone());
        tokio::spawn(async move { app.push_loop(&r).await });
        let (app, r) = (self.clone(), run.clone());
        tokio::spawn(async move {
            let mut outcome = app.lead_goal(&r, board).await;
            // A lead whose session broke off before the goal was done goes back into it, as after a
            // restart, after a pause for whatever made it fail (a dropped connection, an error from
            // the provider). It gives up when it fails again without having done anything since.
            // @project-doc docs/domains/20261031-goal_mode.md#failures_in_goal_mode
            let mut since = None;
            while let Err(err) = &outcome {
                if r.st.lock().unwrap().cancelled() || err.downcast_ref::<Cancelled>().is_some() {
                    return;
                }
                let worked = since.is_none_or(|seq| app.lead_worked_since(&r, seq));
                let again =
                    worked && err.downcast_ref::<GoalRefused>().is_none() && !r.st.lock().unwrap().lead.goal.ended();
                let error = format!("{err:#}");
                let text = if again {
                    format!(
                        "负责人的会话异常结束：{}。{} 秒后回到它的会话继续推进目标",
                        clip(&error, 120),
                        app.cfg.retry_after.as_secs()
                    )
                } else {
                    format!("负责人的会话异常结束：{}", clip(&error, 120))
                };
                app.emit(
                    &r,
                    ev(EventKind::LeadFailed, "janus", text)
                        .data(json!({ "error": error, "next": if again { "reopen" } else { "stop" } })),
                );
                if !again {
                    break;
                }
                tokio::time::sleep(app.cfg.retry_after).await;
                if r.st.lock().unwrap().cancelled() {
                    return;
                }
                since = Some(r.seq.load(Ordering::SeqCst));
                // It is told where the board stands now; `wait` brings only what happens next.
                r.st.lock().unwrap().lead.cursor = r.seq.load(Ordering::SeqCst);
                let board = board_on_resume(&r, &app.db.events(&r.id, 0).unwrap_or_default());
                outcome = app.lead_goal(&r, Some(LeadResume { board, broke: BrokeOff::Failure(error) })).await;
            }
            if r.st.lock().unwrap().cancelled() {
                return;
            }
            let done = match outcome {
                Ok(text) => LeadGoal::Done(app.lead_report(&r, &text)),
                Err(err) if err.downcast_ref::<Cancelled>().is_some() => return,
                Err(err) => LeadGoal::Failed(format!("{err:#}")),
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
            } else {
                // The conversation after delivery ended (the agent exited): nothing listens in
                // this run any more, so it leaves memory once no driver is left, and the next
                // message reopens it as a conversation (`resume_run`) with what waited for it.
                r.driving.idle().await;
                app.runs.lock().unwrap().remove(&r.id);
            }
            r.lead_nudge.notify_one();
            r.wake.notify_one();
        });
        self.execute(run).await
    }

    /// A delivered or accepted run in a worktree that has work since its last merge-back: a turn of
    /// the lead's conversation changed files or made commits (the person asked for more) that are
    /// not on the branch, so it is delivered again, which merges them. Questions about the result
    /// leave the delivery as it was.
    // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
    fn changed_since_delivery(&self, run: &Run) -> bool {
        if !matches!(run.st.lock().unwrap().status, RunStatus::Delivered | RunStatus::Accepted) {
            return false;
        }
        match &run.worktree {
            Some(_) => self.moved_on_since_merge(&run.id, &run.base_ref),
            // No Git to ask: the lead's own edits since the delivery, from its tool calls.
            None => {
                let since = self.last_delivery_seq(&run.id);
                !run.st.lock().unwrap().traces.lead_edited_since(since).is_empty()
            }
        }
    }

    /// The seq of the run's last delivery; 0 before the first one.
    pub(super) fn last_delivery_seq(&self, run_id: &str) -> i64 {
        self.db.latest_event(run_id, EventKind::RunDelivered).ok().flatten().map_or(0, |e| e.seq)
    }

    /// A turn that is not a new goal delivers the run again when it worked on files after a
    /// delivery, or resolved what stopped bringing its result back.
    // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
    fn delivers_again(&self, run: &Run) -> bool {
        let resolved = std::mem::take(&mut run.st.lock().unwrap().lead.redeliver);
        resolved || self.changed_since_delivery(run)
    }

    /// A turn of the lead ended: keep what the whole run changed so far, before a delivery lists
    /// it. Failing to read it only costs the Run page that diff.
    fn run_changes_after_turn(&self, run: &Run) {
        if let Err(err) = self.record_run_changes(run) {
            crate::config::log(format!("janus: run {}: could not save the run's changes: {err:#}", run.id));
        }
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
        let entries = entries.join("\n");
        let text = tr!(
            run.lang,
            "[Janus 看板推送] 看板上有需要你处理的新消息。它们已经算作读过，不会再出现在 janus.wait 里：\n{entries}",
            "[Janus board push] New board messages need you. They count as read and will not come back in janus.wait:\n{entries}"
        );
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("push{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        if tx.send(Steer::Message(Msg { id, text: text.clone() })).is_err() {
            // The session ended in between: the entries were not seen, so `wait` still brings them (AT-03).
            let mut st = run.st.lock().unwrap();
            if st.lead.cursor == last {
                st.lead.cursor = cursor;
            }
            return;
        }
        self.record_session_prompt(
            run,
            &Ctx { agent: "lead", task: None, attempt: None, goal: false },
            &sid,
            PromptChannel::Board,
            &text,
        );
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
            .filter(|e| matches!(e.kind(), EventKind::QuestionAnswered | EventKind::QuestionEscalated))
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
        for e in events.iter().filter(|e| e.kind() == EventKind::QuestionAsked) {
            let Some(key) = key_of(e).filter(|k| !settled.contains(k)) else { continue };
            // A question from an attempt that has since ended is no longer waiting for anyone.
            let live = e.task.as_ref().is_some_and(|t| {
                running.contains(t)
                    && !events
                        .iter()
                        .any(|x| x.seq > e.seq && x.kind() == EventKind::AttemptStarted && x.task.as_ref() == Some(t))
            });
            if !live {
                continue;
            }
            let esc = ev(EventKind::QuestionEscalated, "janus", "负责人已经结束，协作者的提问转交给你")
                .to("user")
                .data(json!({
                    "key": key, "reason": "负责人的会话已经结束，没有人在看板上回答这个问题。"
                }));
            self.emit(run, esc.task(e.task.as_deref().unwrap_or_default()));
        }
    }

    /// The lead's goal session: `/goal <goal>` in the workspace directory, with the team and the
    /// board tools in reach. Open to the person like any session: steering, takeover, messages.
    /// `resume` is set when the lead goes back into its goal after a restart or after its session
    /// broke off: where its subtasks stand (`board_on_resume`) and what broke it off.
    pub(super) async fn lead_goal(self: &Arc<Self>, run: &Arc<Run>, resume: Option<LeadResume>) -> Result<String> {
        let resumed = resume.is_some();
        let (board, broke) = match resume {
            Some(LeadResume { board, broke }) => (board, broke),
            None => (String::new(), BrokeOff::Restart),
        };
        let lead = run.team.iter().find(|m| m.lead).context("no lead")?.clone();
        let standing = standing_instructions(&lead, run.lang);
        let channel = lead.runtime.instruction_channel();
        // Continuing after a restart: the lead's own session comes back when its agent can load it,
        // with everything it did and was told; otherwise a new one is told where things stand.
        // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
        let events = if resumed { self.db.events(&run.id, 0).unwrap_or_default() } else { vec![] };
        // The brief, with the messages left for the lead, goes in the system layer before the
        // standing rules. Codex reads it from its environment, so it is known before the adapter
        // starts; the notes are taken only once the session is open.
        // @project-doc docs/domains/20261031-goal_mode.md#lead_brief
        let (notes, note_ids) = self.peek_notes(run, "lead");
        let resume = if resumed { resume_brief(&board, &broke, &events, run.lang) } else { String::new() };
        let brief = format!("{}{resume}{}{notes}", self.goal_brief(run), self.memory_section(run, MemoryAccess::Write));
        let system = system_layer(&[&brief, &standing]);
        let old_session = if resumed { last_lead_session(&events) } else { None };
        let old = old_session.as_deref().map(|sid| {
            let kept = recorded_system(&events, sid).unwrap_or_else(|| standing.clone());
            (sid, kept)
        });
        let owner = Owner { session: SessionKey::Lead, attempt: 0 };
        let Conversation { conn, mut rx, mut ctl, session, loaded, resources } = self
            .open_conversation(
                run,
                &lead,
                &channel,
                old.as_ref().map(|(s, k)| (*s, k.as_str())),
                &system,
                "lead-goal.log",
            )
            .await?;
        let result = async {
            self.take_peeked_notes(run, "lead", &note_ids);
            let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
            if sid.is_empty() {
                bail!("负责人的会话没有返回 sessionId");
            }
            let mut applied = configure_session(&conn, &session, &lead).await?;
            applied["resources"] = json!(resources);
            let controls = crate::acp_view::controls(&session, &applied);
            let opened = match (resumed, loaded) {
                (false, _) => "开始推进目标",
                (true, true) => "恢复了原来的会话，从中断处继续推进目标",
                (true, false) => "新开会话，从中断处继续推进目标",
            };
            self.emit(
                run,
                ev(EventKind::AgentStatus, "lead", opened)
                    .data(json!({ "phase": AgentPhase::Thinking, "session": sid, "config": applied })),
            );
            {
                let mut st = run.st.lock().unwrap();
                st.lead.session = Some(sid.clone());
                // The goal was active when Janus stopped and the agent keeps it with the session
                // (Claude Code restores it from the transcript): follow it as before.
                if loaded {
                    st.lead.goal_active = true;
                }
            }
            self.watch_session(run, &owner.session, 0);
            let ctx = Ctx { agent: "lead", task: None, attempt: None, goal: true };
            // A loaded session may not return its options: keep the ones on record then.
            if !loaded || has_controls(&controls) {
                self.show(run, (EventKind::SessionControls, "会话选项".into(), controls), &ctx);
            }
            // A loaded session has its system layer already and got it again unchanged: nothing new
            // went in. It is told what happened instead of being given the goal again.
            let (mut prompt, mut opening) = if loaded {
                (resume_prompt(&board, &broke, &notes, run.lang), Some(PromptChannel::Resume))
            } else {
                self.record_system(run, &ctx, &sid, &channel, SystemPart::Collaboration, &brief);
                self.record_instructions(run, &ctx, &sid, &channel, &standing);
                (format!("/goal {}", goal_condition(&run.goal)), None)
            };
            let mut delivered = false;
            loop {
                // A slash command starts a new goal; a message sent while one is active belongs
                // to that goal's turn. Both complete through the same branch below.
                let goal_turn = {
                    let mut st = run.st.lock().unwrap();
                    st.lead.in_turn = true;
                    prompt.trim_start().starts_with("/goal ") || st.lead.goal_active
                };
                let channel = opening.take().unwrap_or_else(|| PromptChannel::of_lead_prompt(&prompt));
                self.record_session_prompt(run, &ctx, &sid, channel, &prompt);
                let text = self
                    .stream_prompt(
                        run,
                        &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
                        &prompt,
                        &ctx,
                        Some(&mut ctl),
                    )
                    .await?;
                self.run_changes_after_turn(run);
                if let Some(reason) = goal_refused(&text) {
                    return Err(GoalRefused(format!("{} 拒绝了 /goal：{reason}", lead.connection_name)).into());
                }
                if !ctl.held_by_person() && !ctl.queued.is_empty() {
                    let next =
                        self.take_queued(run, &mut ctl, &ctx, |n| format!("这一轮结束，把 {n} 条消息发给负责人"));
                    prompt = if delivered {
                        next
                    } else {
                        let goal = &run.goal;
                        tr!(
                            run.lang,
                            "{next}\n\n请结合这些信息继续推进目标：{goal}",
                            "{next}\n\nTake this into account and go on with the goal: {goal}"
                        )
                    };
                    continue;
                }
                if ctl.held_by_person() {
                    let session = PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                    if let Some(next) =
                        self.wait_for_person(run, &session, &mut ctl, &ctx, &resume_after_hold(run, delivered)).await?
                    {
                        prompt = next;
                        continue;
                    }
                }
                if !delivered {
                    // The goal is complete. Delivery can happen while this session stays open:
                    // the person may write the next task or set a new goal with a slash command.
                    delivered = true;
                    run.watch.lock().unwrap().remove(&owner.session);
                    let report = self.lead_report(run, &text);
                    {
                        let mut st = run.st.lock().unwrap();
                        // A goal that was ended by hand must not make the next chat turn follow it.
                        st.lead.goal_active = false;
                        st.lead.goal = LeadGoal::Done(report);
                    }
                    run.lead_nudge.notify_one();
                    run.wake.notify_one();
                    self.emit(
                        run,
                        ev(EventKind::AgentStatus, "lead", "目标已完成，可以继续对话或设置新的目标")
                            .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
                    );
                } else {
                    self.turn_after_delivery(run, &text, goal_turn);
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
            if run.kind == RunKind::Loop {
                app.loop_session_ended(&run, &format!("{err:#}"));
            }
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
            RunKind::Goal => {
                format!("{}{}", conversation_brief(run, &events), self.memory_section(run, MemoryAccess::Write))
            }
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
            let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
            if sid.is_empty() {
                bail!("恢复会话没有返回 sessionId");
            }
            let mut applied = configure_session(&conn, &session, &lead).await?;
            applied["resources"] = json!(resources);
            let controls = crate::acp_view::controls(&session, &applied);
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
                self.record_instructions(run, &ctx, &sid, &channel, &standing);
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
                    )
                    .await?;
                self.run_changes_after_turn(run);
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

    /// Open a run's main session: a conversation's, or the lead's goal session (new, or continued
    /// after a restart). `old` is the session to load and the system layer it already has: it is
    /// loaded with exactly that layer. When it cannot be loaded, a new session starts with
    /// `fresh`, which carries what the agent needs to know. Codex reads its layer from the
    /// environment its adapter starts with, so that new session needs a new adapter process.
    /// The agent's output goes to `log` in the run's log directory.
    async fn open_conversation(
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
                if let Ok(Ok(r)) = tokio::time::timeout(STARTUP_TIMEOUT, conn.request("session/load", params)).await {
                    drain_replay(&conn, &mut rx).await;
                    let mut session = if r.is_object() { r } else { json!({}) };
                    session["sessionId"] = json!(sid);
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
    async fn start_conversation_agent(
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
    /// member has no team to lead.
    fn conversation_opening(
        &self,
        run: &Run,
        lead: &Member,
        conn: &AcpConn,
        channel: &InstructionChannel,
        system: &str,
    ) -> Opening {
        let board = if run.kind.has_janus_server() && conn.capabilities["mcpCapabilities"]["http"] == true {
            vec![self.mcp_server(run, "lead")]
        } else {
            vec![]
        };
        self.opening(
            run,
            "lead",
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

    /// Janus's part of the system layer is invisible in the transcript: record it, with its
    /// channel, the way `record_instructions` records the person's part.
    pub(super) fn record_system(
        &self,
        run: &Run,
        ctx: &Ctx<'_>,
        sid: &str,
        channel: &InstructionChannel,
        part: SystemPart,
        text: &str,
    ) {
        if text.is_empty() {
            return;
        }
        let via = channel.as_str();
        let label = match part {
            SystemPart::Collaboration if let Some(label) = run.kind.collaboration_label() => label,
            SystemPart::Collaboration if ctx.agent == "lead" => "向负责人注入 Agent Team 协作指令",
            SystemPart::Collaboration => "向协作者注入 Agent Team 协作约定",
            SystemPart::History => "向 Agent 注入之前的对话",
        };
        self.emit(
            run,
            ev(EventKind::SessionPrompt, "janus", label)
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": part.as_str(), "via": via, "session": sid, "prompt": text })),
        );
    }

    /// What went into the system layer is invisible in the transcript: record it, with its channel.
    pub(super) fn record_instructions(
        &self,
        run: &Run,
        ctx: &Ctx<'_>,
        sid: &str,
        channel: &InstructionChannel,
        text: &str,
    ) {
        let via = channel.as_str();
        if text.is_empty() {
            return;
        }
        self.emit(
            run,
            ev(EventKind::SessionPrompt, "janus", "向 Agent 注入数字员工的指令")
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
            ev(EventKind::SessionPrompt, "janus", channel.label())
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": channel.as_str(), "session": sid, "prompt": prompt })),
        );
    }

    /// What the lead knows about its team and how to work with it. Everything it learns about
    /// the collaborators comes from here (and the `team` tool, which repeats it).
    // @project-doc docs/domains/20261031-goal_mode.md#lead_brief
    pub(super) fn goal_brief(&self, run: &Run) -> String {
        let lang = run.lang;
        let cwd = run.cwd.display();
        let git = workdir_brief(run);
        let team = team_lines(run);
        let brief = brief_section(&run.brief, Wins::Goal, lang);
        let (heading, headline, decisions) =
            (BRIEF_HEADING.in_(lang), HEADLINE_HEADING.in_(lang), DECISIONS_HEADING.in_(lang));
        let teamwork = crate::teamwork::lead_guide(lang);
        let mut p = tr!(
            lang,
            "你是 Janus 团队的负责人，负责达成用户的目标。先判断目标是否小到你在这个会话里自己就能做完；是的话自己做，不派子任务。否则把其中的部分交给下面的协作者，由你决定让谁参与、怎样分工。

一旦派出任何子任务，这个目标余下的部分你只做管理：计划、看看板、做决定、核对和交付。从那时起不再修改工作目录里的文件，读和检查可以。派出子任务后你改的每个文件，Janus 都会写到看板上，并在交付中列为风险。派出子任务后，资源库的工具和 Skill 只用来查看（读、查询、检查），有副作用的事交给子任务。

工作目录：{cwd}。你和协作者都直接在这个目录里工作，改动对所有人立即可见。{git}

协作者：
{team}

你在做别的事时，需要你处理的看板消息会由 Janus 直接推进你的会话：协作者的提问、失败、停滞、阻塞、同时改了同一个文件、用户的回答。先处理会卡住别人的事，再接着做。

谁来做决定：
- 目标范围内的做法和取舍由你自己决定。
- 对目标或需求的不同理解会带来明显不同的结果、需要用户的账号、凭据、花费或外部资源，或者操作不可逆、会影响工作目录之外而用户没有要求时，用 janus.ask_user 交给用户，给出选项和建议。不依赖这个决定的工作继续做。
- 用户已经直接回答过的问题，按用户的回答做。
- 协作者的结论和你的决定冲突时，用 janus.amend 修订，让下游用同一份结论。

验收标准（子任务的 accept）：
- 每个子任务都写可核对的验收标准：预期的结果和怎样认出它，不写做法的步骤。
- 执行者自己决定怎样核对每条标准，并回报是否达到、证据和可选的证据方法；推理、未核实和没写方法的条目会在完成消息里标出。需要特定的核对方式时，写进标准或说明。
- 执行者回报某条没达到或漏了某条时，Janus 在原会话里给它一次补的机会。仍然不够时，你决定接受、返工或取消。
- 执行者的证据是它自己的说法。交付会标出改了文件却没有独立验证的子任务；对每一个，你的最终回复要说明是否核对过、怎样核对的。

最终回复（用户凭它验收）：
- 以标题「## {headline}」开头，写一两句话：目标是否达成，以及最重要的结果。
- 用户需要对结果做决定时，加标题「## {decisions}」，最多列三个用户能直接回答的问题，一行一个。协作者的风险里需要用户处理的放在这里，其余留在他们的报告里。没有要决定的事就不写这个标题。
- 然后写改了什么、怎样核对的、还有什么没做。写结果，不写过程。
- 交付后用户再找你、你又改了东西时，那一轮的回复也这样结尾：下一次交付只显示那一轮的工作。

协作：
- 协作者看不到你的对话。每个子任务的说明里写全背景、具体要做的事和验收标准。
- 一个协作者实例同一时间只做一个子任务，多派给它的会排队。会改同一批文件的工作用依赖排成先后；同时进行的子任务改了同一个文件时，看板会报冲突，由你处理。
- 派出子任务后，绝不自己接手它的工作，哪怕只改一行：发回原执行者返工、换人重新编排、谁都做不了时问用户，或者取消并在交付里写明缺口。要改正在执行的子任务，用 janus.reply 告诉它。
- 协作者提问后会等你回答，用 janus.reply 回答。
- 目标达成才结束。
- 只有用户要求时（在目标里、回答里或之后的消息里）才 commit、push、部署或发布，并且只做到要求的程度：要求提交不等于要求推送。这是用户的决定，照做时不再询问；没有要求就都不做。子任务要做其中一件时，在它的说明里写明。

{teamwork}

工作区共享资料：
- 共享资料由用户为这个工作区维护，可能包括产品定位、目标用户、约定、过去的决定和重要文件的位置。所有协作者都会收到。和本次目标冲突时以本次目标为准。
- 已经记下的事实直接用，不再安排调研，也不再问用户。
- 这次确定或修正了值得长期保留的事实时，在最终回复末尾加标题「## {heading}」，一行一条完整、可复用的陈述，只写新增或修正的。没有就不写这个标题。保留哪些由用户决定。{brief}",
            "You are the lead of the Janus team and are responsible for achieving the user's goal. First decide whether the goal is small enough to finish yourself in this session; if it is, do it yourself without delegating. Otherwise delegate parts of it to the collaborators below, deciding whom to involve and how to divide the work.

Once you submit any subtask, you only manage for the rest of the goal: plan, read the board, decide, verify, and deliver. Do not edit files in the working directory from then on; reading and checking are fine. Janus reports every file you edit after delegating, on the board and as a risk in the delivery. After you delegate, use the resource library's tools and skills only to look (read, query, check) and leave anything with side effects to a subtask.

Working directory: {cwd}. You and the collaborators work directly in this directory, where changes are immediately visible to everyone. {git}

Collaborators:
{team}

While you do other work, Janus pushes board messages that need you straight into your session: collaborator questions, failures, stalls, blockers, concurrent edits of one file, and user answers. Deal first with anything that blocks others, then carry on.

Decision ownership:
- Decide how the work is done and the trade-offs within the goal yourself.
- Use janus.ask_user when different interpretations of the goal or requirements would lead to materially different results, when you need the user's account, credentials, spending, or external resources, or when an action is irreversible or affects anything outside the working directory and the user has not asked for it. Give options and a recommendation. Continue work that does not depend on the decision.
- If the user has already answered a question directly, proceed with the user's answer.
- When a collaborator's findings conflict with your decision, use janus.amend so downstream work uses the same findings.

Acceptance criteria (the subtask's accept field):
- Write verifiable criteria for every subtask: the expected outcome and how to recognize it, not the steps to get there.
- The assignee chooses how to verify each criterion and reports the status, evidence, and optional method; reasoning, unverified results, and unspecified methods are flagged in completion messages. If a particular verification method is required, include it in the criteria or instructions.
- If the assignee reports an unmet criterion or omits one, Janus gives it one chance to address the gap in the original session. If it still falls short, you decide whether to accept, rework or cancel.
- The assignee's evidence is its own report. The delivery marks each subtask that changed files without an independent verification; for each of them, your final reply says whether and how it was checked.

Final reply (the user reviews the delivery from it):
- Begin with the heading '## {headline}' and one or two sentences: whether the goal is achieved, and the result that matters most.
- If the user has to decide something about the result, add the heading '## {decisions}' and list at most three questions they can answer, one per line. Include the collaborators' risks the user must act on; leave the rest in their reports. Omit the heading when there is nothing to decide.
- Then report what changed, how it was checked, and what is not done. Report results, not the steps you took.
- When the user writes to you after a delivery and you change anything, end that turn's reply the same way: the next delivery shows only that round of work.

Coordination:
- Collaborators cannot see your conversation. Include the context, exact work, and acceptance criteria in each subtask's instructions.
- Each collaborator instance handles one subtask at a time; extra subtasks assigned to it queue. Use dependencies to serialize work that will edit the same files. When simultaneous subtasks edit the same file, the board reports a conflict for you to resolve.
- After you delegate, never take over a subtask's work yourself, not even a one-line fix: send it back for rework, reassign it, ask the user when no collaborator can do it, or cancel it and report the gap in the delivery. Use janus.reply to change the work of a running subtask.
- When a collaborator asks a question, it waits for your answer. Respond with janus.reply.
- Finish only when the goal is achieved.
- Commit, push, deploy, or release only when the user asks for it, in the goal, in an answer, or in a later message, and only as far as asked: a request to commit is not one to push. That request is the user's decision, so carry it out without asking again; otherwise do none of them. When a subtask is to do one of them, say so in its instructions.

{teamwork}

Shared workspace context:
- The user maintains this context for the workspace. It can include product positioning, target users, conventions, past decisions, and important files. All collaborators receive it. The current goal takes precedence if they conflict.
- Use the facts already recorded without assigning new research or asking the user again.
- If you establish or revise facts worth keeping, end your final reply with the heading '## {heading}' and list one complete, reusable statement per line. Include only new or revised facts. Omit the heading if there are no suggestions. The user decides what to keep.{brief}"
        );
        if !run.goal_images.is_empty() {
            let images = crate::attachments::lines(&run.goal_images);
            p.push_str(&tr!(
                lang,
                "\n\n用户给目标附了图片。开始前用读图的工具看一遍：\n{images}\n需要这些图片的协作者，在子任务说明里写上路径。",
                "\n\nThe user attached images to the goal. Use an image-reading tool to review them before starting:\n{images}\nInclude the paths in subtask instructions for collaborators who need these images."
            ));
        }
        p
    }
}

/// Whether a session's options (`acp_view::controls`) name anything; a loaded session may return none.
fn has_controls(controls: &Value) -> bool {
    controls["model"].is_object()
        || controls["mode"].is_object()
        || controls["controls"].as_array().is_some_and(|a| !a.is_empty())
}

/// Where each subtask on the board stands as a run continues after a restart, as the lead reads
/// it: what is done, what was at work and carries on in its own session, what the lead had sent
/// back and with what note, and the last thing the lead told the ones still open. A result from an
/// earlier attempt is marked as such, so it is not taken for the current state.
// @project-doc docs/domains/20261031-run_lifecycle.md#recovery
/// The lead's agent turned `/goal` down: going back into the session would only hear it again.
#[derive(Debug)]
struct GoalRefused(String);

impl std::fmt::Display for GoalRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GoalRefused {}

impl App {
    /// Whether the lead did anything in run `run` after event `seq`: spoke, called a tool, put
    /// tasks on the board.
    fn lead_worked_since(&self, run: &Run, seq: i64) -> bool {
        self.db
            .events(&run.id, seq)
            .is_ok_and(|evs| evs.iter().any(|e| e.actor == "lead" && e.kind().is_agent_progress()))
    }
}

fn board_on_resume(run: &Run, events: &[Event]) -> String {
    let lang = run.lang;
    let st = run.st.lock().unwrap();
    if st.tasks.is_empty() {
        return lang.pick("还没有给协作者派过子任务。", "No subtasks were assigned to collaborators.").into();
    }
    let lines: Vec<String> = st
        .tasks
        .iter()
        .map(|t| {
            let id = t.def.id.as_str();
            let mine = |e: &&Event| e.task.as_deref() == Some(id);
            let mut parts: Vec<String> = vec![];
            match t.status() {
                TaskStatus::Succeeded => {
                    let summary = t.result.as_ref().map(|r| clip(&r.summary, 200)).unwrap_or_default();
                    parts.push(match t.verdict() {
                        Some(vd) => {
                            let vd = vd.as_str();
                            tr!(lang, "已完成，判定 {vd}：{summary}", "done, verdict {vd}: {summary}")
                        }
                        None => tr!(lang, "已完成：{summary}", "done: {summary}"),
                    });
                }
                TaskStatus::Pending => {
                    // Why it runs again: the lead's retry or a re-verification, unless it already
                    // succeeded since.
                    let reopened = events
                        .iter()
                        .filter(mine)
                        .rfind(|e| matches!(e.kind(), EventKind::TaskReopened | EventKind::TaskSucceeded))
                        .filter(|e| e.kind() == EventKind::TaskReopened)
                        .and_then(|e| e.data.as_ref());
                    match reopened.and_then(Reopen::of_event) {
                        Some(Reopen::Retry { session, from, note, .. }) => {
                            let session = session.as_str();
                            let mut why = tr!(
                                lang,
                                "你用 janus.retry 把它发回返工（session={session}）",
                                "you sent it back with janus.retry (session={session})"
                            );
                            if let Some(v) = from {
                                why.push_str(&tr!(lang, "，依据 {v} 的意见", " on {v}'s findings"));
                            }
                            if let Some(n) = note {
                                let n = clip(&n, 300);
                                why.push_str(&tr!(lang, "；你的说明：{n}", "; your note: {n}"));
                            }
                            parts.push(why);
                        }
                        Some(Reopen::Reverify { target }) => {
                            parts.push(tr!(lang, "在 {target} 返工后重新验证", "checks {target} again after its rework"))
                        }
                        None => {}
                    }
                    parts.push(if let Some(reload) = st.resume_session.get(id) {
                        match &reload.broke {
                            BrokeOff::Restart => lang.pick(
                                "Janus 停下时它正在工作，会打开自己原来的会话接着做（Agent 加载不了时新开会话）",
                                "it was at work when Janus stopped and reopens its own session to carry on (a new session if its agent cannot load it)",
                            ).into(),
                            BrokeOff::Failure(err) => {
                                let err = clip(err, 160);
                                tr!(
                                    lang,
                                    "它上一次尝试中断了（{err}），会回到自己原来的会话接着做（加载不了或快满时新开会话）",
                                    "its last attempt broke off ({err}) and goes back into its own session to carry on (a new session if that cannot be loaded or is nearly full)"
                                )
                            }
                        }
                    } else if t.attempts == 0 {
                        lang.pick("还没开始", "not started yet").into()
                    } else {
                        lang.pick("会重新开始", "starts again").into()
                    });
                    if let Some(r) = &t.result {
                        let summary = clip(&r.summary, 160);
                        parts.push(match t.last_verdict() {
                            Some(v) => {
                                let v = v.as_str();
                                tr!(lang, "之前一次尝试的结果（判定 {v}）：{summary}", "result of an earlier attempt (verdict {v}): {summary}")
                            }
                            None => tr!(lang, "之前一次尝试的结果：{summary}", "result of an earlier attempt: {summary}"),
                        });
                    }
                }
                TaskStatus::Blocked => {
                    let by = t.blocked_by().unwrap_or(lang.pick("某个依赖", "a dependency"));
                    parts.push(tr!(lang, "被 {by} 挡住", "blocked by {by}"))
                }
                TaskStatus::Failed => parts.push(lang.pick("失败", "failed").into()),
                TaskStatus::SkippedByDecision => parts.push(lang.pick("被你取消", "cancelled by you").into()),
                other => parts.push(other.as_str().into()),
            }
            if !t.status().terminal()
                && let Some(told) = events
                    .iter()
                    .filter(mine)
                    .rfind(|e| e.kind() == EventKind::UserMessage && e.actor == "lead")
            {
                let told = clip(told.text.trim(), 200);
                parts.push(tr!(lang, "你最后对它说：{told}", "your last message to it: {told}"));
            }
            format!("- {id} ({}): {} · {}", t.agent, t.def.title, parts.join(" · "))
        })
        .collect();
    lines.join("\n")
}

/// What a lead in a new session after a restart or a failure is told, in its brief: its old session is gone,
/// so where the board stands and what it and the person last said.
fn resume_brief(board: &str, broke: &BrokeOff, events: &[Event], lang: Lang) -> String {
    let recent = recent_exchanges(events, Speakers::You, lang);
    let recent = if recent.is_empty() { lang.pick("（无）", "(none)").to_string() } else { recent };
    match (broke, lang) {
        (BrokeOff::Restart, Lang::Zh) => format!(
            "\n\n从中断处继续。你推进这个目标时 Janus 重启了，这是一个新会话：之前做过、读过的都不在里面。已经做的改动还在工作目录里。从现状接着做，不要重做已经完成的工作。\n\nJanus 停下时看板上的子任务：\n{board}\n\n你和用户最近的对话，按时间先后：\n{recent}"
        ),
        (BrokeOff::Restart, Lang::En) => format!(
            "\n\nResuming after an interruption. Janus restarted while you were working on this goal, and this is a new session: what you did and read before is not in it. The changes made so far remain in the working directory. Continue from where things stand; do not redo finished work.\n\nSubtasks on the board when Janus stopped:\n{board}\n\nYour recent conversation with the user, oldest first:\n{recent}"
        ),
        (BrokeOff::Failure(err), Lang::Zh) => format!(
            "\n\n从中断处继续。你推进这个目标时会话中断了（{}），这是一个新会话：之前做过、读过的都不在里面。已经做的改动还在工作目录里。从现状接着做，不要重做已经完成的工作。\n\n现在看板上的子任务：\n{board}\n\n你和用户最近的对话，按时间先后：\n{recent}",
            clip(err, 300)
        ),
        (BrokeOff::Failure(err), Lang::En) => format!(
            "\n\nResuming after an interruption. Your session broke off while you were working on this goal ({}), and this is a new session: what you did and read before is not in it. The changes made so far remain in the working directory. Continue from where things stand; do not redo finished work.\n\nSubtasks on the board now:\n{board}\n\nYour recent conversation with the user, oldest first:\n{recent}",
            clip(err, 300)
        ),
    }
}

/// What a lead whose own session was loaded again is told first: it remembers its work, so only
/// what broke it off, where the board stands and the messages left for it meanwhile.
fn resume_prompt(board: &str, broke: &BrokeOff, notes: &str, lang: Lang) -> String {
    match (broke, lang) {
        (BrokeOff::Restart, Lang::Zh) => format!(
            "[Janus] 你推进这个目标时 Janus 重启了。你的会话已经恢复，之前做过、读过的都还在；协作者的会话都已关闭。从停下的地方接着推进目标，不要重做已经完成的工作。\n\nJanus 停下时看板上的子任务：\n{board}{notes}\n\n从现在起，janus.wait 只带来之后发生的事。"
        ),
        (BrokeOff::Restart, Lang::En) => format!(
            "[Janus] Janus restarted while you were working on this goal. Your session was restored, so what you did and read before is still here; the collaborators' sessions were closed. Continue the goal from where you stopped; do not redo finished work.\n\nSubtasks on the board when Janus stopped:\n{board}{notes}\n\njanus.wait brings only what happens from here on."
        ),
        (BrokeOff::Failure(err), Lang::Zh) => format!(
            "[Janus] 你推进这个目标时会话中断了（{}）。会话已经恢复，之前做过、读过的都还在；协作者一直在工作。从停下的地方接着推进目标，不要重做已经完成的工作。\n\n现在看板上的子任务：\n{board}{notes}\n\n从现在起，janus.wait 只带来之后发生的事。",
            clip(err, 300)
        ),
        (BrokeOff::Failure(err), Lang::En) => format!(
            "[Janus] Your session broke off while you were working on this goal ({}). It was restored, so what you did and read before is still here; the collaborators kept working. Continue the goal from where you stopped; do not redo finished work.\n\nSubtasks on the board now:\n{board}{notes}\n\njanus.wait brings only what happens from here on.",
            clip(err, 300)
        ),
    }
}

/// Where a lead goes back into its goal from: the board as it stands and what broke it off.
pub(super) struct LeadResume {
    pub board: String,
    pub broke: BrokeOff,
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
            let desc = if desc.is_empty() { run.lang.pick("未填写", "Unspecified") } else { desc };
            let (id, name) = (&m.id, &m.name);
            tr!(run.lang, "- {id}：{name} · 职责：{desc}", "- {id}: {name} · Responsibilities: {desc}")
        })
        .collect();
    if lines.is_empty() {
        run.lang.pick("- 没有协作者，自己完成全部工作。", "- No collaborators. Complete the work yourself.").into()
    } else {
        lines.join("\n")
    }
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
    .map(|_| clip(text.trim(), 200))
}

impl App {
    /// A turn of the lead's after a delivery ended. A new goal is a new result, and so is work on
    /// files after a delivery or a conflict resolved for one: deliver it like the first one.
    /// Otherwise the delivery stands, and the lead waits for the next message.
    // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
    fn turn_after_delivery(&self, run: &Run, text: &str, goal_turn: bool) {
        if self.delivers_again(run) || goal_turn {
            let report = self.lead_report(run, text);
            run.st.lock().unwrap().lead.goal = LeadGoal::Done(report);
            run.wake.notify_one();
            if !run.driving.active() {
                self.deliver(run);
            }
        } else if !run.driving.active() {
            self.lead_turn_idle(run);
        }
    }

    /// The lead's turn is over and it waits for the person's next message.
    fn lead_turn_idle(&self, run: &Run) {
        self.emit(
            run,
            ev(EventKind::AgentStatus, "lead", "这一轮已结束，等待你的消息")
                .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
        );
    }

    /// What the lead reports when a turn of it delivers: its last reply, since the earlier ones
    /// narrated the work, without the marker a lead that confirmed its goal to Janus starts with.
    /// The whole turn's text only when no reply was recorded.
    // @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode
    pub(super) fn lead_report(&self, run: &Run, text: &str) -> String {
        last_reply(&self.db.events(&run.id, 0).unwrap_or_default())
            .map(|t| crate::goal_settle::confirmed(&t).filter(|s| !s.is_empty()).unwrap_or(t))
            .unwrap_or_else(|| text.to_string())
    }
}

/// What the lead hears when the person who interrupted it hands the turn back: before the goal is
/// delivered, to go on with it; after, to go on with what the person asked.
fn resume_after_hold(run: &Run, delivered: bool) -> String {
    let goal = &run.goal;
    match delivered {
        true => run
            .lang
            .pick(
                "人工介入结束。请结合上面的对话，继续处理用户的要求。",
                "The person has handed the session back. Taking the conversation above into account, go on with what the user asked.",
            )
            .into(),
        false => tr!(
            run.lang,
            "人工介入结束。请结合上面的对话，继续推进目标直到完成：{goal}",
            "The person has handed the session back. Taking the conversation above into account, carry the goal on to completion: {goal}"
        ),
    }
}

/// The lead's last reply: its trailing run of `agent.message` pieces (nothing else of the lead's
/// between them), joined back together.
pub(super) fn last_reply(events: &[Event]) -> Option<String> {
    // The lead's own steps, and what reached it (a prompt from Janus, a message): each ends a reply.
    let lead = |e: &&Event| {
        e.task.is_none()
            && (e.actor == "lead" || e.to.as_deref() == Some("lead"))
            && !matches!(
                e.kind(),
                EventKind::AgentStatus | EventKind::SessionUsage | EventKind::AgentThought | EventKind::MessageRouted
            )
    };
    let mine: Vec<&Event> = events.iter().filter(lead).collect();
    let tail: Vec<&str> = mine
        .iter()
        .rev()
        .skip_while(|e| e.kind() != EventKind::AgentMessage)
        .take_while(|e| e.kind() == EventKind::AgentMessage)
        .filter_map(|e| e.data.as_ref()?["text"].as_str())
        .collect();
    (!tail.is_empty()).then(|| tail.into_iter().rev().collect::<String>().trim().to_string())
}

/// The lead's last message without a trailing JSON block if it left one. Kept whole: the app folds
/// a long one, and the conclusion the person reads first is its own section (`LeadReport`).
pub(super) fn lead_summary(text: &str) -> String {
    let text = match text.rfind("```json") {
        Some(i) => &text[..i],
        None => text,
    };
    text.trim().to_string()
}

/// The workspace's shared material as a section of a prompt; nothing when the workspace has none.
/// `wins` names what takes precedence when the two disagree.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
pub(super) fn brief_section(brief: &str, wins: Wins, lang: Lang) -> String {
    let brief = brief.trim();
    if brief.is_empty() {
        return String::new();
    }
    let wins = wins.label(lang);
    tr!(
        lang,
        "\n\n工作区共享资料（由用户维护）。有冲突时以{wins}为准。\n{brief}\n",
        "\n\nShared workspace context (maintained by the user). In a conflict, follow {wins}.\n{brief}\n"
    )
}

/// What takes precedence over the shared material where they disagree.
#[derive(Clone, Copy)]
pub(super) enum Wins {
    /// The lead's goal.
    Goal,
    /// A collaborator's subtask instructions.
    Task,
    /// A Loop's prompt.
    Loop,
}

impl Wins {
    fn label(self, lang: Lang) -> &'static str {
        match self {
            Wins::Goal => lang.pick("本次目标", "the current goal"),
            Wins::Task => lang.pick("子任务说明", "the subtask instructions"),
            Wins::Loop => lang.pick("值班的提示", "this Loop's prompt"),
        }
    }
}

/// The lead's last message taken apart: the sections it ends with by the lead's brief
/// (`goal_brief`), each under its own heading, and the rest.
// @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode
#[derive(Debug, Default, PartialEq)]
pub(super) struct LeadReport {
    /// The message without the sections below.
    pub summary: String,
    /// The conclusion in a sentence or two (`HEADLINE_HEADING`); empty when the lead left none.
    pub headline: String,
    /// Questions only the person can answer (`DECISIONS_HEADING`).
    pub decisions: Vec<String>,
    /// Lines to add to the workspace's shared material (`BRIEF_HEADING`).
    pub brief: Vec<String>,
}

impl LeadReport {
    pub(super) fn of(text: &str) -> LeadReport {
        let names = |h: &Heading| [h.zh, h.en];
        let (rest, headline) = take_section(text, &names(&HEADLINE_HEADING));
        let (rest, decisions) = take_section(&rest, &names(&DECISIONS_HEADING));
        let (rest, brief) = take_section(&rest, &names(&BRIEF_HEADING));
        LeadReport { summary: lead_summary(&rest), headline: headline.join("\n"), decisions, brief }
    }
}

/// The first section of `text` under a heading spelled one of `names` (any level, bold or not, a
/// trailing colon or not), up to the next heading: its lines without list markers, and `text`
/// without it. No such heading leaves `text` as it is.
pub(super) fn take_section(text: &str, names: &[&str]) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_heading = |l: &str| {
        let t = l.trim().trim_start_matches('#').replace("**", "");
        let heading = t.trim().trim_end_matches(['：', ':']).trim();
        names.iter().any(|n| heading.eq_ignore_ascii_case(n))
    };
    let Some(start) = lines.iter().position(|l| is_heading(l)) else {
        return (text.to_string(), vec![]);
    };
    let end =
        lines[start + 1..].iter().position(|l| l.trim_start().starts_with('#')).map_or(lines.len(), |i| start + 1 + i);
    let items = lines[start + 1..end]
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
    (rest.trim().to_string(), items)
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

/// The last delivery in the event log as one text (`delivery_text`).
pub(super) fn delivery_summary(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find(|e| e.kind() == EventKind::RunDelivered)
        .and_then(|e| e.data.as_ref())
        .map(delivery_text)
        .unwrap_or_default()
}

/// A delivery (`run.delivered` `data`) as one text for an agent that reads it later (the lead
/// reopened, a Loop, the team memory's consolidation): the conclusion, the report and where the
/// result is.
pub(super) fn delivery_text(d: &Value) -> String {
    let mut parts: Vec<&str> = vec![];
    for key in ["headline", "summary"] {
        let t = d[key].as_str().unwrap_or("").trim();
        if !t.is_empty() && !parts.contains(&t) {
            parts.push(t);
        }
    }
    if let Some(note) = d["merge"]["note"].as_str().filter(|n| !n.trim().is_empty()) {
        parts.push(note.trim());
    }
    parts.join("\n\n")
}

impl App {
    /// The run's last delivery as one text (`delivery_text`); nothing before it was delivered.
    pub(super) fn last_delivery_text(&self, run_id: &str) -> Option<String> {
        self.db
            .latest_event(run_id, EventKind::RunDelivered)
            .ok()
            .flatten()
            .and_then(|e| e.data)
            .map(|d| delivery_text(&d))
    }
}

/// Context for a conversation in a new lead session: the goal, the delivery, and the last things
/// said. Used when the adapter cannot load the original session.
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
        "这个目标已经交付，用户在继续对话。用下面的记录接上之前的进度。\n\n目标：\n{goal}\n\n上次交付的摘要：\n{summary}\n\n最近的对话：\n{recent}",
        "This goal has been delivered and the user is continuing the conversation. Use the history below to pick up where you left off.\n\nGoal:\n{goal}\n\nPrevious delivery summary:\n{summary}\n\nRecent conversation:\n{recent}"
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
