//! Messages from the person and the lead to a session: posting, routing into a live turn or
//! queueing.

use super::*;

wire_enum! {
    /// How a sent message went into a session (`message.routed`, `data.delivery`).
    pub(super) enum Delivery {
        /// Joined the running turn.
        Steer = "steer",
        /// Waits for the running turn to end.
        Queued = "queued",
        /// Stopped the running turn and goes in right after it.
        Interrupt = "interrupt",
        /// A turn of its own.
        Prompt = "prompt",
        /// Left for the agent's next session.
        Next = "next",
    }
}

impl Delivery {
    fn label(self) -> &'static str {
        match self {
            Delivery::Steer => "插入到正在进行的这一轮",
            Delivery::Queued => "排队：这一轮结束后发送",
            Delivery::Interrupt => "打断正在进行的这一轮，立即发送",
            Delivery::Prompt => "作为新的一轮发送",
            Delivery::Next => "留言：下次会话开始时带给它",
        }
    }

    /// Whether a recorded `delivery` means the message reached the agent in that turn, so a
    /// resumed run does not send it again.
    pub(super) fn reached_turn(recorded: &str) -> bool {
        recorded == Delivery::Steer.as_str() || recorded == Delivery::Prompt.as_str()
    }
}

impl App {
    /// Messages left for an agent while it had no open session, handed over with its next prompt.
    /// Record a message the moment it is sent, before any session has picked it up.
    // @project-doc docs/interfaces/20261031-events.md#message_flow
    pub(super) fn post_message(
        &self,
        run: &Run,
        actor: &str,
        to: &str,
        task: &str,
        text: &str,
        images: &[Value],
    ) -> Msg {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("m{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        let mut data = json!({ "id": id, "text": text, "delivery": "pending" });
        if !images.is_empty() {
            data["images"] = json!(images);
        }
        let label = if images.is_empty() {
            short(text, 80)
        } else {
            format!("{}（附 {} 张图片）", short(text.lines().next().unwrap_or(""), 60), images.len())
        };
        let mut e = ev("user.message", actor, label).to(to).data(data);
        if !task.is_empty() {
            e = e.task(task);
        }
        self.emit(run, e);
        Msg { id, text: text.to_string() }
    }

    /// How a sent message went in.
    pub(super) fn route_message(&self, run: &Run, agent: &str, task: &str, n: i64, id: &str, delivery: Delivery) {
        let mut e = ev("message.routed", "janus", delivery.label())
            .to(agent)
            .data(json!({ "id": id, "delivery": delivery.as_str() }));
        if !task.is_empty() {
            e = e.task(task);
            if n > 0 {
                e = e.attempt(n);
            }
        }
        self.emit(run, e);
    }

    /// The messages that waited for the turn to end, as the text of the next prompt. Records that
    /// they went in (`message.delivered`), saying why in the words `why` gives for their count.
    pub(super) fn take_queued(
        &self,
        run: &Run,
        ctl: &mut Control,
        ctx: &Ctx<'_>,
        why: impl FnOnce(usize) -> String,
    ) -> String {
        let msgs: Vec<Msg> = std::mem::take(&mut ctl.queued);
        let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
        self.emit(
            run,
            ev("message.delivered", "janus", why(msgs.len())).to(ctx.agent).scope(ctx).data(json!({ "ids": ids })),
        );
        msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n")
    }

    /// The person ends the task with the result so far while no turn is running (it holds the
    /// session, or the agent waits on a question): on record like ending a running turn, so the
    /// audit log and the lead's board show it.
    pub(super) fn record_finish(&self, run: &Run, ctx: &Ctx<'_>) {
        let name = run.member(ctx.agent).map(|m| m.name.as_str()).unwrap_or(ctx.agent);
        self.emit(
            run,
            ev("session.interrupted", "user", format!("以当前结果完成 {name} 的子任务"))
                .scope(ctx)
                .to(ctx.agent)
                .data(json!({ "finish": true })),
        );
    }

    /// The person holds the session and the turn has ended: wait for them. Their message is the
    /// next prompt; handing back resumes the agent with `resume` (and what queued meanwhile);
    /// finishing ends the session with the result so far (`None`). Controls switch on the way.
    pub(super) async fn wait_for_person(
        &self,
        run: &Run,
        session: &PromptSession<'_>,
        ctl: &mut Control,
        ctx: &Ctx<'_>,
        resume: &str,
    ) -> Result<Option<String>> {
        let (agent, task, n) = (ctx.agent, ctx.task.unwrap_or_default(), ctx.attempt.unwrap_or(0));
        self.emit(
            run,
            ev("agent.status", agent, "这一轮已结束，等待你的消息（人工接管中）")
                .scope(ctx)
                .data(json!({ "phase": AgentPhase::WaitingForInput, "turn": "idle" })),
        );
        loop {
            match ctl.rx.recv().await {
                None => return Err(Cancelled.into()),
                Some(Steer::Message(m)) => {
                    self.route_message(run, agent, task, n, &m.id, Delivery::Prompt);
                    return Ok(Some(m.text));
                }
                // The lead's word while the person holds the session waits for the release.
                Some(Steer::Redirect(m)) => {
                    self.route_message(run, agent, task, n, &m.id, Delivery::Queued);
                    ctl.queued.push(m);
                }
                Some(Steer::Release) => {
                    ctl.set_hold(run, Hold::Agent);
                    let name = run.member(agent).map(|m| m.name.as_str()).unwrap_or(agent);
                    let text = if ctx.task.is_some() {
                        format!("交还托管：{name} 继续自主完成子任务")
                    } else {
                        "交还托管：负责人继续推进目标".to_string()
                    };
                    self.emit(run, ev("control.released", "user", text).scope(ctx).to(agent));
                    if ctl.queued.is_empty() {
                        return Ok(Some(resume.to_string()));
                    }
                    let waiting =
                        self.take_queued(run, ctl, ctx, |n| format!("交还托管，把排队的 {n} 条消息发给 {agent}"));
                    return Ok(Some(format!("{resume}\n\n{waiting}")));
                }
                Some(Steer::Finish) => {
                    ctl.set_hold(run, Hold::Finish);
                    self.record_finish(run, ctx);
                    return Ok(None);
                }
                Some(Steer::Control(c)) => self.apply_control(run, session.conn, session.sid, c, ctx).await,
                // The person holds this session: it is not idle.
                Some(Steer::TakeOver | Steer::Interrupt | Steer::Park) => {}
            }
        }
    }

    /// Take the notes for an agent's session working on `task`; notes about its other tasks stay.
    pub(super) fn take_notes(&self, run: &Run, agent: &str, task: Option<&str>) -> String {
        let notes: Vec<Msg> = {
            let mut st = run.st.lock().unwrap();
            let Some(kept) = st.notes.get_mut(agent) else { return String::new() };
            let (taken, left): (Vec<Note>, Vec<Note>) =
                std::mem::take(kept).into_iter().partition(|n| n.goes_with(task));
            *kept = left;
            if kept.is_empty() {
                st.notes.remove(agent);
            }
            taken.into_iter().map(|n| n.msg).collect()
        };
        self.notes_delivered(run, agent, &notes);
        notes_text(&notes)
    }

    /// The notes left for an agent, without taking them: a session that has to know them before
    /// it starts (Codex's system layer is in its environment) takes them once it is open.
    pub(super) fn peek_notes(&self, run: &Run, agent: &str) -> (String, Vec<String>) {
        let st = run.st.lock().unwrap();
        let notes: Vec<Msg> = st.notes.get(agent).into_iter().flatten().map(|n| n.msg.clone()).collect();
        (notes_text(&notes), notes.iter().map(|m| m.id.clone()).collect())
    }

    /// Take the notes `peek_notes` saw; any left since stay for the next prompt.
    pub(super) fn take_peeked_notes(&self, run: &Run, agent: &str, ids: &[String]) {
        let taken: Vec<Msg> = {
            let mut st = run.st.lock().unwrap();
            let Some(notes) = st.notes.get_mut(agent) else { return };
            let (taken, left): (Vec<Note>, Vec<Note>) =
                std::mem::take(notes).into_iter().partition(|n| ids.contains(&n.msg.id));
            *notes = left;
            if notes.is_empty() {
                st.notes.remove(agent);
            }
            taken.into_iter().map(|n| n.msg).collect()
        };
        self.notes_delivered(run, agent, &taken);
    }

    fn notes_delivered(&self, run: &Run, agent: &str, notes: &[Msg]) {
        if notes.is_empty() {
            return;
        }
        let ids: Vec<&str> = notes.iter().map(|m| m.id.as_str()).collect();
        self.emit(
            run,
            ev("message.delivered", "janus", format!("把你留给 {agent} 的 {} 条消息带进这次会话", notes.len()))
                .to(agent)
                .data(json!({ "ids": ids })),
        );
    }
}

fn notes_text(notes: &[Msg]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    format!(
        "\n\nAdditional information from the user. Give it priority:\n{}",
        notes.iter().map(|n| format!("- {}", n.text)).collect::<Vec<_>>().join("\n")
    )
}
