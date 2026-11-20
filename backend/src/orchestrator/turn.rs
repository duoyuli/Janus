//! One prompt turn in an ACP session: streaming its updates into events, steering, controls and the
//! agent's requests.

use super::*;

impl App {
    /// Stream one prompt turn: bridge ACP updates into events, route permission requests to the user,
    /// and apply what the person does in the session (steer, interrupt, take over) while the turn runs.
    pub(super) async fn stream_prompt(
        &self,
        run: &Run,
        session: &mut PromptSession<'_>,
        text: &str,
        ctx: &Ctx<'_>,
        mut ctl: Option<&mut Control>,
        limit: Option<Duration>,
    ) -> Result<String> {
        let PromptSession { conn, rx, sid } = session;
        let (conn, sid) = (*conn, *sid);
        // The turn's own deadline replaces the ACP request timeout, so that time spent waiting
        // for a person to decide on a permission request does not count against the agent.
        let send = |text: &str| {
            let params = json!({ "sessionId": sid, "prompt": self.prompt_blocks(run, conn, text) });
            let conn = conn.clone();
            Box::pin(async move { conn.request_timeout("session/prompt", params, None).await })
        };
        let mut prompt = send(text);
        let mut deadline = limit.map(|l| tokio::time::Instant::now() + l);
        let mut out = String::new();
        let mut flushed = 0usize;
        let mut turn = crate::acp_view::Turn::default();
        // Goal mode: the prompt has returned but the agent's goal is still active, so the agent
        // keeps working in further turns of its own; the session is followed until it settles.
        let mut following = false;
        let goal_active = || ctx.goal && run.st.lock().unwrap().lead.goal_active;
        // Following a goal that does not settle by itself: when the session last moved, how many
        // times Janus has asked the lead, and where the reply to the last ask starts in `out`.
        let mut heard = tokio::time::Instant::now();
        let mut asks = 0u8;
        let mut asked_at: Option<usize> = None;
        loop {
            let steer = async {
                match ctl.as_mut() {
                    Some(c) => c.rx.recv().await,
                    None => std::future::pending().await,
                }
            };
            let expire = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            };
            // Text the agent has said is shown once it stops talking for a moment, not only at its
            // next tool call: an agent may say what it is about to do, then write a long file for minutes.
            let unshown = out.len() > flushed || !turn.thought.is_empty();
            let quiet = async move {
                if unshown { tokio::time::sleep(MESSAGE_QUIET).await } else { std::future::pending().await }
            };
            let settle_wait = following && turn.tools.is_empty() && asks < crate::goal_settle::ASKS;
            let settle_at = heard + self.cfg.goal_settle;
            let settle = async move {
                if settle_wait { tokio::time::sleep_until(settle_at).await } else { std::future::pending().await }
            };
            tokio::select! {
                _ = settle => {
                    asks += 1;
                    let text = crate::goal_settle::ask();
                    self.record_session_prompt(run, ctx, sid, PromptChannel::GoalCheck, &text);
                    self.flush_thought(run, &mut turn.thought, ctx);
                    self.flush_message(run, &out, &mut flushed, ctx);
                    asked_at = Some(out.len());
                    prompt = send(&text);
                    following = false;
                }
                _ = quiet => {
                    self.flush_thought(run, &mut turn.thought, ctx);
                    self.flush_message(run, &out, &mut flushed, ctx)
                }
                res = &mut prompt, if !following => {
                    let res = res?;
                    // Updates that arrived with the response; a request among them still gets an answer.
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            Incoming::Notification { method, params } => {
                                if method == "session/update" {
                                    self.on_update(run, &params["update"], &mut out, &mut flushed, &mut turn, ctx);
                                }
                            }
                            Incoming::Request { id, method, .. } => {
                                if is_user_request(&method) {
                                    conn.respond(id, cancelled_reply(&method)).await?;
                                } else {
                                    conn.respond_error(id, "janus does not implement this client method").await?;
                                }
                            }
                            Incoming::Closed => break,
                        }
                    }
                    self.flush_thought(run, &mut turn.thought, ctx);
                    self.flush_message(run, &out, &mut flushed, ctx);
                    if let Some(u) = crate::acp_view::prompt_usage(&res) {
                        self.show(run, u, ctx);
                    }
                    if run.st.lock().unwrap().cancelled() {
                        return Err(Cancelled.into());
                    }
                    let interrupted = ctl.as_mut().is_some_and(|c| std::mem::take(&mut c.interrupted));
                    heard = tokio::time::Instant::now();
                    // The lead answered Janus's ask: a confirmation ends the goal here.
                    if let Some(from) = asked_at.take()
                        && let Some(summary) = crate::goal_settle::confirmed(&out[from..])
                    {
                        let _ = conn.request_timeout("_session/goal", json!({ "sessionId": sid, "action": "clear" }), Some(STEER_TIMEOUT)).await;
                        run.st.lock().unwrap().lead.goal_active = false;
                        self.emit(run, ev("goal.status", "janus", "负责人确认目标已完成，Janus 结束了目标").data(json!({ "status": null, "by": "janus" })));
                        return Ok(if summary.is_empty() { out[..from].to_string() } else { summary });
                    }
                    if res["stopReason"] == "cancelled" && !interrupted {
                        // Nobody asked for this: the agent stopped on its own. Retry it like any failure.
                        bail!("Agent 自行以 cancelled 结束了这一轮");
                    }
                    if goal_active() && !interrupted {
                        following = true;
                        continue;
                    }
                    return Ok(out);
                }
                _ = expire => {
                    let _ = conn.notify("session/cancel", json!({ "sessionId": sid })).await;
                    if let Ok(Ok(res)) = tokio::time::timeout(Duration::from_secs(2), &mut prompt).await
                        && let Some(u) = crate::acp_view::prompt_usage(&res)
                    {
                        self.show(run, u, ctx);
                    }
                    self.flush_thought(run, &mut turn.thought, ctx);
                    self.flush_message(run, &out, &mut flushed, ctx);
                    bail!("超过 {} 分钟没有完成", limit.unwrap_or_default().as_secs() / 60);
                }
                cmd = steer => {
                    let Some(c) = ctl.as_mut() else { continue };
                    match cmd {
                        // The attempt's sender only goes away when the run is cancelled.
                        None => return Err(Cancelled.into()),
                        Some(cmd) => self.steer_turn(run, conn, sid, c, cmd, ctx).await?,
                    }
                    // Following an active goal there is no prompt in flight to wait for: what
                    // could not join the agent's own turn goes in now as a prompt of its own,
                    // instead of waiting until the goal settles.
                    if following && !c.held_by_person() && !c.queued.is_empty() {
                        let next = self.take_queued(run, c, ctx, |n| format!("目标仍在进行，把 {n} 条消息立即发给 {}", ctx.agent));
                        self.flush_message(run, &out, &mut flushed, ctx);
                        prompt = send(&next);
                        following = false;
                    }
                }
                msg = rx.recv() => match msg {
                    None | Some(Incoming::Closed) => bail!("Agent 进程意外退出"),
                    Some(Incoming::Notification { method, params }) => {
                        heard = tokio::time::Instant::now();
                        if method == "session/update" {
                            self.on_update(run, &params["update"], &mut out, &mut flushed, &mut turn, ctx);
                        }
                        if following && !goal_active() {
                            self.flush_thought(run, &mut turn.thought, ctx);
                            self.flush_message(run, &out, &mut flushed, ctx);
                            return Ok(out);
                        }
                    }
                    Some(Incoming::Request { id, method, params }) => {
                        if is_user_request(&method) {
                            let waited = tokio::time::Instant::now();
                            let outcome = self.user_request(run, conn, sid, AgentRequest { method: &method, params: &params }, ctx, ctl.as_deref_mut()).await?;
                            if let Some(d) = deadline.as_mut() {
                                *d += waited.elapsed();
                            }
                            conn.respond(id, outcome).await?;
                        } else {
                            conn.respond_error(id, "janus does not implement this client method").await?;
                        }
                    }
                }
            }
        }
    }

    /// Put a message into the turn that is already running, through the agreed
    /// `_session/steering` extension. `promptRequired` keeps an idle session's input here, so the
    /// normal prompt flow still owns the next turn.
    pub(super) async fn steer_into_turn(&self, run: &Run, conn: &AcpConn, sid: &str, text: &str) -> bool {
        conn.request_timeout(
            "_session/steering",
            json!({
                "sessionId": sid,
                "prompt": self.prompt_blocks(run, conn, text),
                "_meta": { "steering": { "idleBehavior": "promptRequired" } }
            }),
            Some(STEER_TIMEOUT),
        )
        .await
        .is_ok_and(|r| r["outcome"] == "injected")
    }

    /// The agent took a steered message into the running turn.
    pub(super) fn delivered_steer(&self, run: &Run, ctx: &Ctx<'_>, id: &str) {
        self.emit(
            run,
            ev("message.delivered", "janus", format!("{} 已经读到插入的消息", ctx.agent))
                .to(ctx.agent)
                .scope(ctx)
                .data(json!({ "ids": [id], "applied": true })),
        );
    }

    /// Apply one session command while a turn is running.
    // @project-doc docs/interfaces/20261031-acp_integration.md#steering
    pub(super) async fn steer_turn(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        c: &mut Control,
        cmd: Steer,
        ctx: &Ctx<'_>,
    ) -> Result<()> {
        let (agent, task, n) = (ctx.agent, ctx.task.unwrap_or_default(), ctx.attempt.unwrap_or(0));
        match cmd {
            Steer::Message(m) => {
                // Mid-turn steering joins the running turn; without it the message waits for the
                // turn to end. A short timeout: while this waits, the session loop answers nothing
                // else, and the agent may itself be waiting for us (for example on a permission
                // request).
                let injected = c.steering && self.steer_into_turn(run, conn, sid, &m.text).await;
                self.route_message(
                    run,
                    agent,
                    task,
                    n,
                    &m.id,
                    if injected { Delivery::Steer } else { Delivery::Queued },
                );
                if injected {
                    self.delivered_steer(run, ctx, &m.id);
                } else {
                    c.queued.push(m);
                }
            }
            Steer::Redirect(m) => {
                let injected = c.steering && self.steer_into_turn(run, conn, sid, &m.text).await;
                if injected {
                    self.route_message(run, agent, task, n, &m.id, Delivery::Steer);
                    self.delivered_steer(run, ctx, &m.id);
                } else {
                    // Stop the turn; the message is the next turn, sent as soon as this one returns.
                    // Under takeover the person decides when the agent goes on, so it waits there.
                    let held = c.held_by_person();
                    if !held {
                        conn.notify("session/cancel", json!({ "sessionId": sid })).await?;
                        c.interrupted = true;
                    }
                    self.route_message(
                        run,
                        agent,
                        task,
                        n,
                        &m.id,
                        if held { Delivery::Queued } else { Delivery::Interrupt },
                    );
                    c.queued.push(m);
                }
            }
            Steer::Interrupt | Steer::Finish => {
                let finish = matches!(cmd, Steer::Finish);
                conn.notify("session/cancel", json!({ "sessionId": sid })).await?;
                c.interrupted = true;
                // A chat has no takeover: stopping its turn leaves it waiting for the next message.
                let chat = run.kind == RunKind::Chat;
                if finish {
                    c.set_hold(run, Hold::Finish);
                } else if !chat && !c.held_by_person() {
                    c.set_hold(run, Hold::Person);
                }
                let name = run.member(agent).map(|m| m.name.as_str()).unwrap_or(agent);
                let text = if finish {
                    format!("停止 {name} 这一轮，并以当前结果完成子任务")
                } else if chat {
                    format!("停止 {name} 这一轮")
                } else {
                    format!("打断 {name} 这一轮，转为人工接管")
                };
                self.emit(
                    run,
                    ev("session.interrupted", "user", text)
                        .task(task)
                        .attempt(n)
                        .to(agent)
                        .data(json!({ "finish": finish })),
                );
            }
            Steer::TakeOver => {
                if !c.held_by_person() {
                    c.set_hold(run, Hold::Person);
                    self.emit(
                        run,
                        ev(
                            "control.taken",
                            "user",
                            format!(
                                "接管 {} 的会话：这一轮结束后等你的消息",
                                run.member(agent).map(|m| m.name.as_str()).unwrap_or(agent)
                            ),
                        )
                        .task(task)
                        .attempt(n)
                        .to(agent),
                    );
                }
            }
            Steer::Control(control) => self.apply_control(run, conn, sid, control, ctx).await,
            Steer::Release => {
                if c.held_by_person() {
                    c.set_hold(run, Hold::Agent);
                    self.emit(
                        run,
                        ev(
                            "control.released",
                            "user",
                            format!(
                                "交还托管：{} 继续自主完成子任务",
                                run.member(agent).map(|m| m.name.as_str()).unwrap_or(agent)
                            ),
                        )
                        .task(task)
                        .attempt(n)
                        .to(agent),
                    );
                }
            }
        }
        Ok(())
    }

    /// Agent text since the last tool call becomes one `agent.message` event, so the session reads like a chat.
    /// Text streams in pieces; each piece keeps its own whitespace, so pieces joined back
    /// together (the app's bubble, the lead's summary) are the text the agent wrote, code blocks
    /// and line breaks included.
    pub(super) fn flush_message(&self, run: &Run, out: &str, flushed: &mut usize, ctx: &Ctx<'_>) {
        let raw = &out[*flushed..];
        *flushed = out.len();
        if raw.trim().is_empty() {
            return;
        }
        let e =
            ev("agent.message", ctx.agent, short(raw.trim(), 80)).data(json!({ "text": short(raw, 8000) })).scope(ctx);
        self.emit(run, e);
    }

    /// A collaborator's tool call edited these files. Two kinds of trace are reported to the lead
    /// on the board (`task.conflict`), each once per pair and file:
    /// - another task running at the same time edited one of them: both work in the same
    ///   directory and may overwrite each other; both sides whose agent takes steering get a
    ///   notice in their running turn.
    /// - one of them belongs to another open task that may run alongside (`owns`, or a claim):
    ///   `data.owned` is true, and only the task that stepped over hears about it.
    // @project-doc docs/domains/20261031-goal_mode.md#conflicts
    pub(super) fn record_edits(&self, run: &Run, task: &str, paths: &[String], seq: i64) {
        let (shared, owned) = {
            let mut guard = run.st.lock().unwrap();
            let st = &mut *guard;
            st.traces.record(task, paths, seq);
            let shared: Vec<(String, Vec<String>)> = st
                .tasks
                .iter()
                .filter(|t| t.status() == TaskStatus::Running && t.def.id != task)
                .filter_map(|t| {
                    let theirs = st.traces.files(&t.def.id);
                    let common: Vec<String> = paths.iter().filter(|p| theirs.contains(*p)).cloned().collect();
                    (!common.is_empty()).then(|| (t.def.id.clone(), common))
                })
                .collect();
            let board: Vec<(&TaskDef, TaskStatus)> = st.tasks.iter().map(|t| (&t.def, t.status())).collect();
            let mut owned: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for p in paths {
                if let Some(o) = crate::teamwork::owner_of(p, task, &board) {
                    owned.entry(o.to_string()).or_default().push(p.clone());
                }
            }
            let shared: Vec<(String, Vec<String>)> = shared
                .into_iter()
                .filter_map(|(other, common)| {
                    let fresh: Vec<String> =
                        common.into_iter().filter(|p| st.traces.first_report(Overlap::Same, task, &other, p)).collect();
                    (!fresh.is_empty()).then_some((other, fresh))
                })
                .collect();
            let owned: Vec<(String, Vec<String>)> = owned
                .into_iter()
                .filter_map(|(other, files)| {
                    let fresh: Vec<String> =
                        files.into_iter().filter(|p| st.traces.first_report(Overlap::Owned, task, &other, p)).collect();
                    (!fresh.is_empty()).then_some((other, fresh))
                })
                .collect();
            (shared, owned)
        };
        let name = |id: &str| {
            run.st
                .lock()
                .unwrap()
                .tasks
                .iter()
                .find(|t| t.def.id == id)
                .and_then(|t| run.member(&t.agent))
                .map(|m| m.name.clone())
                .unwrap_or_default()
        };
        for (other, files) in shared {
            let list = files.join("、");
            self.emit(
                run,
                ev("task.conflict", "janus", format!("{task} 和 {other} 都改了 {}", short(&list, 80)))
                    .task(task)
                    .to("lead")
                    .data(json!({ "other": other, "paths": files })),
            );
            for (me, them) in [(task, other.as_str()), (other.as_str(), task)] {
                self.notify(run, me, &format!("[Janus 提醒] 子任务 {them}（{}）也在修改 {list}。你们都直接在同一个工作目录里改文件：再改这些文件前先重新读取最新内容，不要覆盖对方的改动；分工不清楚时用 janus.board_post 写一条 blocker 告诉负责人。", name(them)));
            }
        }
        for (other, files) in owned {
            let list = files.join("、");
            self.emit(
                run,
                ev("task.conflict", "janus", format!("{task} 改了 {other} 负责的 {}", short(&list, 80)))
                    .task(task)
                    .to("lead")
                    .data(json!({ "other": other, "paths": files, "owned": true })),
            );
            self.notify(run, task, &format!("[Janus 提醒] {list} 归子任务 {other}（{}）负责，它可能正在改。不要再改这些文件，也不要覆盖对方的改动；确实需要改时，用 janus.board_post 写一条 blocker 说明原因，由负责人协调。", name(&other)));
        }
    }

    /// The lead's own session edited these files: record them in `traces` so finished tasks'
    /// conclusions are marked stale (`负责人 改`), and warn both the lead and any running
    /// collaborator task whose owned or concurrently edited files were touched. A task that has
    /// not started yet will read the lead's change when it does (the lead settling a shared
    /// contract before fanning out), so it is not a conflict.
    pub(super) fn record_lead_edits(&self, run: &Run, paths: &[String], seq: i64) {
        let (shared, owned, after_plan) = {
            let mut guard = run.st.lock().unwrap();
            let st = &mut *guard;
            st.traces.record_lead(paths, seq);
            // Once a subtask is on the board the lead only manages: an edit of its own is
            // reported to it and goes on the delivery as a risk. Janus does not stop it; what the
            // lead may do is up to its agent's own permission mode.
            // @project-doc docs/domains/20261031-goal_mode.md#lead_role
            let after_plan = if st.tasks.is_empty() { vec![] } else { st.traces.record_lead_after_plan(paths) };
            let mut shared: Vec<(String, Vec<String>)> = vec![];
            let mut owned: Vec<(String, Vec<String>)> = vec![];
            for t in st.tasks.iter().filter(|t| t.status() == TaskStatus::Running) {
                let id = &t.def.id;
                let owned_files: Vec<String> = paths
                    .iter()
                    .filter(|p| t.def.owns.iter().any(|o| crate::teamwork::overlaps(o, p)))
                    .filter(|p| {
                        st.traces.first_actor_report(
                            Overlap::Owned,
                            crate::traces::Actor::Lead,
                            crate::traces::Actor::task(id),
                            p,
                        )
                    })
                    .cloned()
                    .collect();
                if !owned_files.is_empty() {
                    owned.push((id.clone(), owned_files));
                } else {
                    let theirs = st.traces.files(id);
                    let common: Vec<String> = paths
                        .iter()
                        .filter(|p| theirs.contains(*p))
                        .filter(|p| {
                            st.traces.first_actor_report(
                                Overlap::Same,
                                crate::traces::Actor::Lead,
                                crate::traces::Actor::task(id),
                                p,
                            )
                        })
                        .cloned()
                        .collect();
                    if !common.is_empty() {
                        shared.push((id.clone(), common));
                    }
                }
            }
            (shared, owned, after_plan)
        };
        if !after_plan.is_empty() {
            self.emit(
                run,
                ev("lead.edited", "janus", format!("负责人派出子任务后改了 {}", short(&after_plan.join("、"), 80)))
                    .to("lead")
                    .data(json!({ "paths": after_plan })),
            );
        }
        for (other, files) in owned {
            let list = files.join("、");
            self.emit(
                run,
                ev("task.conflict", "janus", format!("负责人改了 {other} 负责的 {}", short(&list, 80)))
                    .task(&other)
                    .to("lead")
                    .data(json!({ "other": "lead", "byLead": true, "owned": true, "paths": files })),
            );
            self.notify(
                run,
                &other,
                &format!(
                    "[Janus 提醒] 负责人刚刚修改了归你负责的 {list}。继续修改前先重新读取最新内容，以负责人的改动为准。"
                ),
            );
        }
        for (other, files) in shared {
            let list = files.join("、");
            self.emit(
                run,
                ev("task.conflict", "janus", format!("负责人和 {other} 都改了 {}", short(&list, 80)))
                    .task(&other)
                    .to("lead")
                    .data(json!({ "other": "lead", "byLead": true, "paths": files })),
            );
            self.notify(
                run,
                &other,
                &format!(
                    "[Janus 提醒] 负责人刚刚也修改了 {list}。再改这些文件前先重新读取最新内容，不要覆盖负责人的改动。"
                ),
            );
        }
    }

    /// A notice from Janus into a task's running turn, for an agent that takes steering: queued
    /// until the turn ends, it would arrive after the fact, so other agents do not get it.
    pub(super) fn notify(&self, run: &Run, task: &str, text: &str) {
        let tx = {
            let st = run.st.lock().unwrap();
            let key = SessionKey::task(task);
            st.steerable.contains(&key).then(|| st.live.get(&key).cloned()).flatten()
        };
        let Some(tx) = tx else { return };
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("notice{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        let _ = tx.send(Steer::Message(Msg { id, text: text.to_string() }));
    }

    pub(super) fn on_update(
        &self,
        run: &Run,
        u: &Value,
        out: &mut String,
        flushed: &mut usize,
        turn: &mut crate::acp_view::Turn,
        ctx: &Ctx<'_>,
    ) {
        // Any update from the session (a chunk of text or thought, a tool's progress) is a sign of work.
        let key = ctx.session();
        let n = ctx.attempt.unwrap_or(0);
        let quiet = run.with_watch(&key, n, |w| {
            w.last_ms = now_ms();
            std::mem::take(&mut w.idle_noticed)
        });
        // The board (or, for the lead, the person) stops seeing the stall once the agent moves again.
        if quiet == Some(true) {
            match ctx.task {
                Some(task) => self.emit(
                    run,
                    ev("task.stalled", "janus", format!("{task} 又有动静了"))
                        .task(task)
                        .attempt(n)
                        .to("lead")
                        .data(json!({ "reason": "idle", "resolved": true })),
                ),
                None => self.emit(
                    run,
                    ev("lead.stalled", "janus", "负责人又有动静了")
                        .to("user")
                        .data(json!({ "reason": "idle", "resolved": true })),
                ),
            };
        }
        // The /goal state comes as session info, { goal: { objective, status } } or null once
        // cleared, under the AIR metadata both official adapters use.
        let goal_meta = u["_meta"]["jetbrains"]["air"].get("goal");
        match u["sessionUpdate"].as_str().unwrap_or("") {
            "session_info_update" if ctx.goal && goal_meta.is_some() => {
                let goal = goal_meta.unwrap();
                let status = goal["status"].as_str().map(str::to_owned);
                run.st.lock().unwrap().lead.goal_active = status.as_deref() == Some("active");
                self.emit(
                    run,
                    ev(
                        "goal.status",
                        ctx.agent,
                        match status.as_deref() {
                            Some("active") => "目标进行中".to_string(),
                            Some(other) => format!("目标状态：{other}"),
                            None => "目标已清除".to_string(),
                        },
                    )
                    .data(json!({ "status": status, "objective": goal["objective"] })),
                );
            }
            "agent_message_chunk" => {
                if let Some(t) = u["content"]["text"].as_str() {
                    out.push_str(t);
                }
                run.with_watch(&key, n, |w| {
                    w.phase = AgentPhase::Thinking;
                    w.last_tool.clear();
                });
            }
            "tool_call" | "tool_call_update" => {
                let first = u["sessionUpdate"] == "tool_call";
                if first {
                    self.flush_thought(run, &mut turn.thought, ctx);
                    self.flush_message(run, out, flushed, ctx);
                }
                let id = u["toolCallId"].as_str().unwrap_or("").to_string();
                let entry = turn.tools.entry(id.clone()).or_insert_with(|| json!({}));
                for key in ["title", "kind", "rawInput", "rawOutput", "content", "status", "locations"] {
                    if !u[key].is_null() {
                        entry[key] = u[key].clone();
                    }
                }
                let status = entry["status"].as_str().unwrap_or("pending").to_string();
                let title = entry["title"].as_str().unwrap_or("工具调用").to_string();
                let phase = tool_phase(entry["kind"].as_str().unwrap_or(""), &title);
                // A call still running is shown at once, like in the agent's own interface; its
                // finished record (`tool.call`, same `callId`) replaces it.
                if first && !matches!(status.as_str(), "completed" | "failed") {
                    let data = crate::acp_view::tool_started(&id, entry, &summarize_input(&entry["rawInput"], &title));
                    let e = ev("tool.started", ctx.agent, short(&title, 80)).data(data).scope(ctx);
                    self.emit(run, e);
                }
                if matches!(status.as_str(), "completed" | "failed") {
                    let entry = turn.tools.remove(&id).unwrap();
                    if turn.tools.is_empty() {
                        run.with_watch(&key, n, |w| {
                            w.phase = AgentPhase::Thinking;
                            w.last_tool = short(&title, 60);
                        });
                    }
                    let kind = entry["kind"].as_str().unwrap_or("tool").to_string();
                    let input = summarize_input(&entry["rawInput"], &title);
                    let edited = if status == "completed" { edited_paths(&entry, &run.cwd) } else { vec![] };
                    let mut data = json!({
                        "callId": id,
                        "tool": kind,
                        "input": input,
                        "output": summarize_output(&entry),
                        "status": status,
                    });
                    if !edited.is_empty() {
                        data["paths"] = json!(edited);
                    }
                    let diffs = crate::acp_view::diffs(&entry);
                    if !diffs.is_empty() {
                        data["diffs"] = json!(diffs);
                    }
                    let e = ev("tool.call", ctx.agent, short(&title, 80)).data(data).scope(ctx);
                    let seq = self.emit(run, e);
                    // The lead polls the board on purpose; only its other calls can go in circles.
                    if ctx.task.is_some() || !title.contains("janus") {
                        self.watch_call(run, &key, &entry);
                    }
                    if !edited.is_empty() {
                        match ctx.task {
                            Some(task) => self.record_edits(run, task, &edited, seq),
                            None if ctx.agent == "lead" => self.record_lead_edits(run, &edited, seq),
                            None => {}
                        }
                    }
                } else {
                    run.with_watch(&key, n, |w| {
                        w.phase = phase;
                        w.last_tool = short(&title, 60);
                    });
                    if now_ms() - turn.last_status > 1500 {
                        turn.last_status = now_ms();
                        self.emit(
                            run,
                            ev("agent.status", ctx.agent, short(&title, 80)).scope(ctx).data(json!({ "phase": phase })),
                        );
                    }
                }
            }
            _ => {
                if let Some(shown) = crate::acp_view::event_for(u, turn) {
                    self.show(run, shown, ctx);
                }
            }
        }
    }

    /// Record what `acp_view` made of an update, in the session it came from.
    pub(super) fn show(&self, run: &Run, (kind, text, data): crate::acp_view::Shown, ctx: &Ctx<'_>) {
        let e = ev(kind, ctx.agent, text).data(data).scope(ctx);
        self.emit(run, e);
    }

    /// The agent's thinking since it last spoke or acted, as one `agent.thought`.
    pub(super) fn flush_thought(&self, run: &Run, thought: &mut String, ctx: &Ctx<'_>) {
        let text = std::mem::take(thought);
        let text = text.trim();
        if !text.is_empty() {
            self.show(run, ("agent.thought", short(text, 80), json!({ "text": short(text, 8000) })), ctx);
        }
    }

    /// A message's ACP prompt: its text, the images pasted with it, and the workspace files it
    /// mentions as `@path` (`acp_view::resource_links`).
    pub(super) fn prompt_blocks(&self, run: &Run, conn: &AcpConn, text: &str) -> Vec<Value> {
        let mut blocks = crate::attachments::prompt_blocks(&self.cfg.data_dir, text, takes_images(conn));
        blocks.extend(crate::acp_view::resource_links(text, &run.cwd));
        blocks
    }

    /// Switch one of the session's controls while it is open: an ACP session mode, or a config
    /// option (the model, the effort, a permission mode). The agent's answer is the new state.
    // @project-doc docs/interfaces/20261031-acp_integration.md#session_controls
    pub(super) async fn apply_control(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        control: SessionControl,
        ctx: &Ctx<'_>,
    ) {
        if let Err(err) = self.set_control(run, conn, sid, &control, ctx).await {
            let id = &control.id;
            self.show(
                run,
                (
                    "session.control",
                    format!("切换 {id} 没有成功：{}", short(&format!("{err:#}"), 120)),
                    json!({ "id": id, "failed": true }),
                ),
                ctx,
            );
        }
    }

    async fn set_control(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        control: &SessionControl,
        ctx: &Ctx<'_>,
    ) -> Result<()> {
        let SessionControl { id, value, via } = control;
        if *via == ControlVia::Mode {
            conn.request("session/set_mode", json!({ "sessionId": sid, "modeId": value })).await?;
            self.show(
                run,
                (
                    "session.control",
                    format!("你把模式切换为 {}", value.as_str().unwrap_or_default()),
                    json!({ "id": id, "value": value, "via": via.as_str(), "by": "user" }),
                ),
                ctx,
            );
        } else {
            let r = conn
                .request("session/set_config_option", json!({ "sessionId": sid, "configId": id, "value": value }))
                .await?;
            self.show(
                run,
                (
                    "session.control",
                    format!(
                        "你把 {id} 切换为 {}",
                        value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())
                    ),
                    json!({ "id": id, "value": value, "via": via.as_str(), "by": "user" }),
                ),
                ctx,
            );
            if r["configOptions"].is_array() {
                self.show(run, ("session.controls", "会话选项已更新".into(), crate::agents::session_options(&r)), ctx);
            }
        }
        Ok(())
    }

    /// Put an agent's request in front of the person and answer it in the request's own terms:
    /// a permission request with the option they pick (every option the agent offered, not just
    /// allow/deny), an elicitation with the form they fill in or the link they finish. What an
    /// agent may do without asking is its permission mode, chosen on the role; whatever it does
    /// ask lands here. While waiting, the session stays responsive: an interrupt or finish
    /// withdraws the request, an abort ends the attempt, and messages queue for the end of the turn.
    // @project-doc docs/interfaces/20261031-acp_integration.md#user_request_replies
    pub(super) async fn user_request(
        &self,
        run: &Run,
        conn: &AcpConn,
        sid: &str,
        request: AgentRequest<'_>,
        ctx: &Ctx<'_>,
        mut ctl: Option<&mut Control>,
    ) -> Result<Value> {
        let AgentRequest { method, params } = request;
        let task = ctx.task.unwrap_or_default();
        let n = ctx.attempt.unwrap_or(0);
        let elicit = method == "elicitation/create";
        let options = params["options"].as_array().cloned().unwrap_or_default();
        let pick = |prefix: &str| {
            options
                .iter()
                .find(|o| o["kind"].as_str().is_some_and(|k| k.starts_with(prefix)))
                .and_then(|o| o["optionId"].as_str())
                .map(str::to_owned)
        };
        let allow = pick("allow_once")
            .or_else(|| pick("allow"))
            .or_else(|| options.first().and_then(|o| o["optionId"].as_str().map(str::to_owned)))
            .unwrap_or_default();
        let reject = pick("reject").unwrap_or_else(|| allow.clone());
        let tool = &params["toolCall"];
        let title = if elicit {
            params["message"].as_str().unwrap_or("Agent 需要你填写信息").to_string()
        } else {
            tool["title"].as_str().unwrap_or("执行操作").to_string()
        };
        let command = summarize_input(&tool["rawInput"], &title);
        let selected = |id: &str| json!({ "outcome": { "outcome": "selected", "optionId": id } });

        let key = format!("req-{}-{}-{}", if task.is_empty() { ctx.agent } else { task }, n, now_ms());
        let mode = params["mode"].as_str().unwrap_or("form");
        let mut e = if elicit {
            ev("input.requested", ctx.agent, format!("请你填写：{}", short(&title, 50))).data(json!({
                "key": key, "agent": ctx.agent, "mode": mode, "message": title,
                "schema": params["requestedSchema"], "url": params["url"],
                "toolCallId": params["toolCallId"], "meta": params["_meta"],
            }))
        } else {
            ev("approval.requested", ctx.agent, format!("请求批准：{}", short(&title, 50))).data(json!({
                "key": key, "agent": ctx.agent, "command": command, "title": title,
                "toolKind": tool["kind"], "content": tool["content"], "rawInput": tool["rawInput"],
                "policy": "这个数字员工的权限模式要求这一步由你决定",
                "impact": "Agent 在等你选择；选项由 Agent 提供，按它自己的含义生效。",
                "options": options,
            }))
        };
        if !task.is_empty() {
            e = e.task(task).attempt(n);
        }
        self.emit(run, e);
        let _held = run.held(&ctx.session(), n);
        let wait = self.wait_for(run, &key, ctx.owner(), Ask::Request);
        tokio::pin!(wait);
        loop {
            let steer = async {
                match ctl.as_mut() {
                    Some(c) => c.rx.recv().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                decision = &mut wait => {
                    let decision = decision?;
                    if elicit {
                        return Ok(match decision["action"].as_str().unwrap_or("cancel") {
                            "accept" => json!({ "action": "accept", "content": decision["content"].clone() }),
                            "decline" => json!({ "action": "decline" }),
                            _ => json!({ "action": "cancel" }),
                        });
                    }
                    if let Some(o) = decision["option"].as_str().filter(|o| options.iter().any(|x| x["optionId"] == *o)) {
                        return Ok(selected(o));
                    }
                    return Ok(selected(if decision["allow"].as_bool().unwrap_or(false) {
                        &allow
                    } else {
                        &reject
                    }));
                }
                cmd = steer => {
                    let Some(c) = ctl.as_mut() else { continue };
                    match cmd {
                        None => {
                            self.withdraw(run, &key, task, n, "会话已结束，撤回这个请求");
                            return Err(Cancelled.into());
                        }
                        Some(cmd @ (Steer::Interrupt | Steer::Finish)) => {
                            self.withdraw(run, &key, task, n, "这一轮被打断，撤回这个请求");
                            self.steer_turn(run, conn, sid, c, cmd, ctx).await?;
                            return Ok(cancelled_reply(method));
                        }
                        Some(Steer::Message(m) | Steer::Redirect(m)) => {
                            // The agent is blocked on this request; the message goes in after the turn.
                            self.route_message(run, ctx.agent, task, n, &m.id, Delivery::Queued);
                            c.queued.push(m);
                        }
                        Some(cmd) => self.steer_turn(run, conn, sid, c, cmd, ctx).await?,
                    }
                }
            }
        }
    }

    /// Drop a pending decision nobody can act on any more, and tell the app to remove its card.
    pub(super) fn withdraw(&self, run: &Run, key: &str, task: &str, n: i64, text: &str) {
        run.st.lock().unwrap().pending.remove(key);
        let mut e = ev("decision.withdrawn", "janus", text).data(json!({ "key": key }));
        if !task.is_empty() {
            e = e.task(task).attempt(n);
        }
        self.emit(run, e);
    }
}

/// A request the agent sent the client mid-turn: a permission request or an elicitation.
pub(super) struct AgentRequest<'a> {
    method: &'a str,
    params: &'a Value,
}

/// The files a finished tool call changed, relative to the workspace: its ACP `locations`, or the
/// path in its input. Only edits count (`edit`, `delete`, `move`), not reads.
pub(super) fn edited_paths(entry: &Value, cwd: &Path) -> Vec<String> {
    if !matches!(entry["kind"].as_str(), Some("edit" | "delete" | "move")) {
        return vec![];
    }
    let mut raw: Vec<String> = entry["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["path"].as_str().map(str::to_owned))
        .collect();
    if raw.is_empty() {
        let input = &entry["rawInput"];
        raw.extend(
            ["file_path", "filePath", "path", "notebook_path"]
                .iter()
                .filter_map(|k| input[*k].as_str().map(str::to_owned)),
        );
        if let Some(changes) = input["changes"].as_object() {
            raw.extend(changes.keys().cloned());
        }
    }
    let mut out: Vec<String> = vec![];
    for p in raw {
        let path = Path::new(&p);
        let rel = path.strip_prefix(cwd).map(|r| r.to_string_lossy().to_string()).unwrap_or(p.clone());
        if !rel.is_empty() && !out.contains(&rel) {
            out.push(rel);
        }
    }
    out
}

/// The reply to an agent request Janus cannot put in front of anyone: a session that is closing,
/// or a connection check.
pub(super) fn is_user_request(method: &str) -> bool {
    matches!(method, "session/request_permission" | "elicitation/create")
}

pub fn cancelled_reply(method: &str) -> Value {
    match method {
        "elicitation/create" => json!({ "action": "cancel" }),
        _ => json!({ "outcome": { "outcome": "cancelled" } }),
    }
}

pub(super) fn summarize_input(raw: &Value, title: &str) -> String {
    let s = match raw {
        Value::Null => title.to_string(),
        Value::String(s) => s.clone(),
        v => match &v["command"] {
            Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
            Value::String(s) => s.clone(),
            _ => v
                .get("path")
                .or(v.get("file_path"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string()),
        },
    };
    short(&s, 240)
}

/// Codex reports command output as a JSON envelope; show the text a person would read.
pub(super) fn unwrap_output(s: &str) -> String {
    match serde_json::from_str::<Value>(s) {
        Ok(v) if v.is_object() => {
            let text = v
                .get("formatted_output")
                .or(v.get("output"))
                .or(v.get("stdout"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            match v["exit_code"].as_i64() {
                Some(code) if code != 0 => format!("exit {code} · {text}"),
                _ => text,
            }
        }
        _ => s.to_string(),
    }
}

pub(super) fn summarize_output(entry: &Value) -> String {
    if let Some(items) = entry["content"].as_array() {
        let parts: Vec<String> = items
            .iter()
            .filter_map(|c| match c["type"].as_str() {
                Some("content") => c["content"]["text"].as_str().map(unwrap_output),
                Some("diff") => Some(format!("修改 {}", c["path"].as_str().unwrap_or("文件"))),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            return short(&parts.join(" · "), 300);
        }
    }
    // No output: nothing to show (the call's status is recorded on its own).
    match &entry["rawOutput"] {
        Value::Null => String::new(),
        Value::String(s) => short(&unwrap_output(s), 300),
        v => short(&unwrap_output(&v.to_string()), 300),
    }
}

fn tool_phase(kind: &str, title: &str) -> AgentPhase {
    match kind {
        "read" | "search" | "fetch" => AgentPhase::ReadingFiles,
        "edit" | "delete" | "move" => AgentPhase::Editing,
        _ if title.contains("test") || title.contains("测试") => AgentPhase::RunningTest,
        _ => AgentPhase::RunningTool,
    }
}
