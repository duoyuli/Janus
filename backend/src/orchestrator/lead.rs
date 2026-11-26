//! The lead: its goal session, its brief, the board pushed into it, how a round after delivery
//! ends, and recording what goes into a session.

use super::*;
use crate::lang::{Lang, tr};

wire_enum! {
    /// What Janus puts in a session's system layer before the person's standing instructions.
    pub(super) enum SystemPart {
        /// How to work with Janus and the team: the lead's brief, a collaborator's rules. Only an
        /// Agent Team session gets it; a chat has the digital employee's instructions alone.
        Collaboration = "system",
        /// A chat reopened: its recent exchanges, so the new session knows the conversation.
        History = "history",
        /// The digital employee's own standing instructions.
        Instructions = "instructions",
    }
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
        /// A collaborator whose turn ended without handing anything in, reminded to hand in.
        Fix = "fix",
    }
}

impl PromptChannel {
    /// A prompt in the lead's session: a slash command starts a goal, anything else follows up.
    pub(super) fn of_lead_prompt(prompt: &str) -> PromptChannel {
        if starts_goal(prompt) { PromptChannel::Goal } else { PromptChannel::Followup }
    }

    fn label(self) -> &'static str {
        match self {
            PromptChannel::Goal => "向负责人发送目标提示",
            PromptChannel::Followup => "向负责人发送继续提示",
            PromptChannel::Board => "把看板上需要处理的消息推给负责人",
            PromptChannel::GoalCheck => "问负责人目标是否已经完成",
            PromptChannel::Resume => "告诉负责人编排器重启过，接着推进目标",
            PromptChannel::Fix => "提醒 Agent 交结果",
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
    pub(super) fn run_changes_after_turn(&self, run: &Run) {
        if let Err(err) = self.record_run_changes(run) {
            crate::config::log(format!("janus: run {}: could not save the run's changes: {err:#}", run.id));
        }
    }

    /// The lead finished a turn and waits for the person: a collaborator still waiting on the
    /// board now waits on the person, as it does when the lead's session ends.
    pub(super) fn lead_goes_idle(&self, run: &Run) {
        let off_duty = {
            let mut st = run.st.lock().unwrap();
            st.lead.in_turn = false;
            !st.lead.on_duty()
        };
        if off_duty {
            self.withdraw_lead_questions(run);
        }
        self.escalate_open_questions(run);
    }

    /// The lead left the board with questions it handed the person (`ask_user`) unanswered: it
    /// went on without them and reads no answer now, so their cards go. Anything the person
    /// still wants it to know is a message. A Loop's questions wait for their answer.
    // @project-doc docs/domains/20261031-goal_mode.md#question_routing
    fn withdraw_lead_questions(&self, run: &Run) {
        if run.kind.questions_outlive_session() {
            return;
        }
        let keys: Vec<String> = run
            .st
            .lock()
            .unwrap()
            .pending
            .iter()
            .filter(|(_, p)| p.owner.session == SessionKey::Lead && p.ask == Ask::Question)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            self.withdraw(run, &key, "", 0, "负责人已经结束这一轮，不再等这个回答；要补充什么，直接给负责人发消息");
        }
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
        let id = crate::util::new_id("push");
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
                    "key": key, "reason": "负责人的会话已经结束，没有人在看板上回答这个问题。",
                    "recommend": e.data.as_ref().map_or(&Value::Null, |d| &d["recommend"]),
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
        let brief = format!("{}{resume}{}{notes}", goal_brief(run), self.memory_section(run, MemoryAccess::Write));
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
            let (sid, applied, controls) = main_session_ready(&conn, &session, &lead, &resources).await?;
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
                self.record_system(run, &ctx, &sid, &channel, SystemPart::Instructions, &standing);
                (format!("/goal {}", goal_condition(&run.goal)), None)
            };
            let mut delivered = false;
            loop {
                // A slash command starts a new goal; a message sent while one is active belongs
                // to that goal's turn. Both complete through the same branch below.
                let channel = opening.take().unwrap_or_else(|| PromptChannel::of_lead_prompt(&prompt));
                let session = &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                let (text, goal_turn) = self.main_turn(run, session, &mut ctl, &ctx, channel, &prompt).await?;
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
        self.end_main_session(run, &owner, &conn).await;
        result
    }

    /// What went into the system layer is invisible in the transcript: record each part, with its
    /// channel.
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
            SystemPart::Collaboration if ctx.task.is_none() => "向负责人注入 Agent Team 协作指令",
            SystemPart::Collaboration => "向协作者注入 Agent Team 协作约定",
            SystemPart::History => "向 Agent 注入之前的对话",
            SystemPart::Instructions => "向 Agent 注入数字员工的指令",
        };
        self.emit(
            run,
            ev(EventKind::SessionPrompt, "janus", label)
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "channel": part.as_str(), "via": via, "session": sid, "prompt": text })),
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
}

/// What the lead knows about its team and how to work with it. Everything it learns about
/// the collaborators comes from here (and `janus.board`, which repeats it).
// @project-doc docs/domains/20261031-goal_mode.md#lead_brief
pub(super) fn goal_brief(run: &Run) -> String {
    let lang = run.lang;
    let cwd = run.cwd.display();
    let team = team_lines(run);
    let (workdir, commit) = (workdir_brief(run), commit_rule(run));
    let (heading, headline, decisions) =
        (BRIEF_HEADING.in_(lang), HEADLINE_HEADING.in_(lang), DECISIONS_HEADING.in_(lang));
    let brief = brief_section(&run.brief, Wins::Goal, lang);
    let mut p = tr!(
        lang,
        "你是 Janus 团队的负责人，负责达成用户的目标。先判断：目标小到你在这个会话里能自己做完，就自己做；否则把工作派给下面的协作者。

协作者：
{team}

工作目录：{cwd}。你和协作者都直接在这里工作，改动彼此立即可见。{workdir}

派活：
- 协作者看不到你的对话。子任务说明里写全背景、要做的事、负责什么、不能碰什么。
- 验收标准写预期的结果和怎样认出它，不写做法的步骤；需要特定的核对方式时写进标准。
- 紧密耦合的步骤放进一个子任务，其余按文件、模块或问题切开。互不依赖的子任务同时做，需要上一步的结果时才用依赖排先后；并行的子任务共用的定义（接口、格式、提纲、术语）先定下来。
- 一个协作者实例一次只做一个子任务，多派的会排队。会改同一批文件的子任务用依赖排开。

派出之后：
- 你只做管理：不再改工作目录里的文件（读和检查可以），资源库的工具和 Skill 只用来查看。你改的文件 Janus 会写到看板上，并在交付中列为风险。
- 派出的工作绝不自己接手，哪怕只改一行：返工、换人、谁都做不了时问用户，或者取消并在交付里写明缺口。
- 读看板跟进。需要你处理的消息（提问、失败、停滞、阻塞、冲突、用户的回答）Janus 也会直接推进你的会话，先处理会卡住别人的。
- 协作者提问后会等你回答；要改正在执行的子任务，直接告诉它。
- 执行者交的结果有标准没达到时，Janus 会提醒它修好再交一次；仍然没达到的，由你决定接受、返工还是取消。执行者的证据是它自己的说法。

交付（目标达成才结束）：
- 最终回复以「## {headline}」开头，一两句话说目标是否达成、最重要的结果是什么。
- 用户要对结果做决定时，加「## {decisions}」，最多三个用户能直接回答的问题，一行一个；协作者的风险里要用户处理的放在这里。
- 然后写改了什么、怎样核对的、还有什么没做，写结果不写过程。交付会标出改了文件却没有独立验证的子任务，逐个说明是否核对过、怎样核对的。
- 这次确定或修正了值得长期保留的事实时，在末尾加「## {heading}」，一行一条完整的陈述，只写新增或修正的；保留哪些由用户决定。
- 交付后用户再找你、你又改了东西时，那一轮的回复也这样结尾。
- {commit}

谁来做决定：
- 目标范围内的做法和取舍由你决定；协作者的结论和你的决定冲突时，修订它的结论，让下游用同一份。
- 理解不同会导致明显不同的结果、需要用户的账号、凭据、花费或外部资源，或者操作不可逆、会影响工作目录之外时，交给用户决定，给出选项和建议；不依赖它的工作继续做。用户已经回答过的，照用户的回答做。{brief}",
        "You are the lead of the Janus team, responsible for achieving the user's goal. Decide first: if the goal is small enough to finish yourself in this session, do it yourself; otherwise hand the work to the collaborators below.

Collaborators:
{team}

Working directory: {cwd}. You and the collaborators all work directly in it, and changes are visible to everyone at once. {workdir}

Handing out work:
- Collaborators cannot see your conversation. Give each subtask's instructions the full context, the work to do, and what it owns and must leave alone.
- Acceptance criteria state the expected outcome and how to recognize it, not the steps to get there; if a particular way of checking is required, put it in the criteria.
- Keep tightly coupled steps in one subtask and divide the rest by file, module, or question. Subtasks that do not depend on each other run at the same time; chain them with dependencies only when one needs the other's result. Settle the definitions parallel subtasks share (an interface, a format, an outline, terms) first.
- A collaborator instance works on one subtask at a time; more queue. Use dependencies to keep subtasks that change the same files apart.

Once work is handed out:
- You only manage: you no longer change files in the working directory (reading and checking are fine), and you use the resource library's tools and skills only to look. Janus reports every file you change on the board and lists it as a risk in the delivery.
- Never take over delegated work, not even a one-line fix: send it back for rework, reassign it, ask the user when no one can do it, or cancel it and report the gap in the delivery.
- Follow the board. Janus also pushes the messages that need you (questions, failures, stalls, blockers, conflicts, user answers) straight into your session; deal first with what blocks others.
- A collaborator who asks a question waits for your answer; to change the work of a running subtask, tell it.
- When an assignee hands in a result with a criterion unmet, Janus tells it once that it may fix it and hand in again; if it still falls short, you decide whether to accept, rework, or cancel. An assignee's evidence is its own account.

Delivery (finish only when the goal is achieved):
- Begin your final reply with '## {headline}' and one or two sentences: whether the goal is achieved and the result that matters most.
- If the user has to decide something about the result, add '## {decisions}' with at most three questions they can answer, one per line; collaborators' risks the user must act on go here.
- Then report what changed, how it was checked, and what is not done: results, not the steps you took. The delivery marks each subtask that changed files without an independent verification; say for each whether and how it was checked.
- If you established or corrected facts worth keeping, end with '## {heading}' and one complete statement per line, new or corrected ones only; the user decides what to keep.
- When the user writes to you after a delivery and you change something, end that turn's reply the same way.
- {commit}

Decisions:
- Decide how the work is done and the trade-offs within the goal yourself; when a collaborator's findings conflict with your decision, correct its findings so downstream work uses the same ones.
- When different readings would lead to materially different results, when you need the user's account, credentials, spending, or outside resources, or when an action is irreversible or reaches outside the working directory, leave the decision to the user, giving options and a recommendation; carry on with work that does not depend on it. Follow answers the user has already given.{brief}"
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

/// Whether a prompt to the lead is a `/goal` command, which starts a goal.
pub(super) fn starts_goal(prompt: &str) -> bool {
    prompt.trim_start().starts_with("/goal ")
}

/// The lead's agent turned `/goal` down: going back into the session would only hear it again.
#[derive(Debug)]
struct GoalRefused(String);

impl std::fmt::Display for GoalRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GoalRefused {}

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
    pub(super) fn turn_after_delivery(&self, run: &Run, text: &str, goal_turn: bool) {
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
    pub(super) fn lead_turn_idle(&self, run: &Run) {
        self.emit(
            run,
            ev(EventKind::AgentStatus, "lead", "这一轮已结束，等待你的消息")
                .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
        );
    }
}

/// The workspace's shared material as a section of a prompt; nothing when the workspace has none.
/// `wins` names what takes precedence when the two disagree.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
pub(super) fn brief_section(brief: &str, wins: Wins, lang: Lang) -> String {
    let brief = brief.trim();
    if brief.is_empty() {
        return String::new();
    }
    let head = match (wins, lang) {
        (Wins::Goal, Lang::Zh) => {
            "工作区共享资料（用户维护，所有协作者都能看到；已有的事实直接用，不再调研或问用户；和本次目标冲突时以目标为准）："
        }
        (Wins::Goal, Lang::En) => {
            "Shared workspace context (kept by the user and given to every collaborator; use the facts in it without researching or asking again; the current goal wins where they conflict):"
        }
        (Wins::Task, Lang::Zh) => "工作区共享资料（用户维护；和子任务说明冲突时以说明为准）：",
        (Wins::Task, Lang::En) => {
            "Shared workspace context (kept by the user; your subtask's instructions win where they conflict):"
        }
        (Wins::Loop, Lang::Zh) => "工作区共享资料（用户维护；和值班的提示冲突时以提示为准）：",
        (Wins::Loop, Lang::En) => {
            "Shared workspace context (kept by the user; this Loop's prompt wins where they conflict):"
        }
    };
    format!("\n\n{head}\n{brief}")
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
