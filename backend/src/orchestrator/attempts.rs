//! One attempt of a task: its session, its result check, the changes it made and its artifacts.

use super::*;

/// Why an attempt tries `session/load` instead of opening a fresh ACP session.
enum SessionResume {
    /// Resuming an attempt that was still open when the orchestrator restarted.
    Interrupted(String),
    /// Continuing a finished upstream task's session (`continues`), the task's own last session
    /// (a retry with `session: "continue"`, or a verification checking reworked work).
    Followup { from: String, session: String },
    /// A continuation was asked for, but the task it names never opened a session (for example
    /// one cancelled before it started): a new session, with the reason on record.
    Missing { from: String },
}

impl App {
    pub(super) async fn run_attempt(self: &Arc<Self>, run: &Arc<Run>, id: &str, n: i64, attempt_id: &str) {
        let result = self.attempt(run, id, n, attempt_id).await;
        if let Err(err) = result {
            let msg = format!("{err:#}");
            let verification = err.downcast_ref::<VerificationFailed>().is_some();
            // An attempt that will be tried again waits a moment first, still holding its task: a
            // cancel or retry by the lead meanwhile is seen below as usual (AT-10).
            let retrying = !verification && {
                let st = run.st.lock().unwrap();
                !st.cancelled()
                    && st.tasks.iter().any(|t| {
                        t.def.id == id && t.on_attempt(attempt_id) && t.attempts - t.retry_base < t.def.max_attempts
                    })
            };
            if retrying && !self.cfg.retry_after.is_zero() {
                tokio::time::sleep(self.cfg.retry_after).await;
            }
            // Decide under one lock whether this attempt still owns its task, and settle the task
            // in the same step: a cancel or retry by the lead in between must not be overwritten.
            enum Next {
                Late,
                Cancelled,
                Retry,
                Failed,
            }
            let (next, max_attempts) = {
                let mut st = run.st.lock().unwrap();
                let cancelled = st.cancelled();
                match Run::task_mut(&mut st, id).filter(|t| t.on_attempt(attempt_id)) {
                    None => (Next::Late, 0),
                    Some(t) => {
                        let next = if cancelled {
                            Next::Cancelled
                        } else if !verification && t.attempts - t.retry_base < t.def.max_attempts {
                            Next::Retry
                        } else {
                            Next::Failed
                        };
                        match next {
                            Next::Cancelled if t.status() == TaskStatus::Running => t.set_phase(TaskPhase::Cancelled),
                            Next::Retry => t.set_phase(TaskPhase::Pending),
                            Next::Failed => t.set_phase(TaskPhase::Failed),
                            _ => {}
                        }
                        (next, t.def.max_attempts)
                    }
                }
            };
            if matches!(next, Next::Late) {
                // The lead cancelled or retried the task while this attempt was ending.
                let _ = self.db.update_attempt(
                    &run.id,
                    id,
                    n,
                    AttemptStatus::Cancelled,
                    AttemptUpdate { error: Some(&msg), ended: Some(now_ms()), ..Default::default() },
                );
                self.emit(
                    run,
                    ev("attempt.late_result", "janus", format!("忽略已过期 Attempt {attempt_id} 的结果"))
                        .task(id)
                        .attempt(n)
                        .data(json!({ "attemptId": attempt_id, "error": msg })),
                );
                return;
            }
            // Only a cancelled run counts as cancellation. Anything else, including an agent
            // ending its own turn as cancelled, is a failed attempt.
            let cancelled = matches!(next, Next::Cancelled);
            let _ = self.db.update_attempt(
                &run.id,
                id,
                n,
                if cancelled { AttemptStatus::Cancelled } else { AttemptStatus::Failed },
                AttemptUpdate { error: Some(&msg), ended: Some(now_ms()), ..Default::default() },
            );
            if !cancelled {
                self.emit(
                    run,
                    ev("attempt.failed", "janus", format!("{id} 第 {n} 次尝试失败"))
                        .task(id)
                        .attempt(n)
                        .data(json!({ "error": msg })),
                );
                if matches!(next, Next::Retry) {
                    self.emit(
                        run,
                        ev("task.retry_scheduled", "janus", format!("按重试策略为 {id} 创建第 {} 次尝试", n + 1))
                            .task(id)
                            .data(json!({
                                "next": n + 1, "strategy": "新建 ACP 会话，注入子任务说明、上游结果和上次失败原因"
                            })),
                    );
                } else {
                    self.emit(
                        run,
                        ev(
                            "task.failed",
                            "janus",
                            if verification { format!("{id} 验证没有通过，交给负责人决定") } else { format!("{id} 已用完尝试次数，交给负责人处理") },
                        )
                        .task(id)
                        .data(json!({ "error": msg, "maxAttempts": max_attempts, "reason": if verification { "verification" } else { "attempts" } })),
                    );
                }
            }
        }
        run.wake.notify_one();
    }

    pub(super) async fn attempt(self: &Arc<Self>, run: &Arc<Run>, id: &str, n: i64, attempt_id: &str) -> Result<()> {
        let (def, agent, upstream, handoffs, last_error, team_note, reopen, review) = {
            let mut st = run.st.lock().unwrap();
            // How this attempt was asked to start, once: a later automatic retry starts afresh.
            let reopen = Run::task_mut(&mut st, id).and_then(|t| t.reopen.take());
            let t = st.tasks.iter().find(|t| t.def.id == id).context("task missing")?;
            let v_task = t.def.verifies.as_deref().and_then(|v| st.tasks.iter().find(|x| x.def.id == v));
            let team_note = match v_task {
                Some(vt) => {
                    let diff_path =
                        vt.artifacts.iter().rev().find(|a| a["kind"] == "git_diff").and_then(|a| a["path"].as_str());
                    let edited_files: Vec<String> = st.traces.files(&vt.def.id).into_iter().collect();
                    let vr = vt.result.as_ref();
                    let weak_checks = vr.map(|r| Summary::of(&vt.def.accept, &r.checks).reminders).unwrap_or_default();
                    let risks = vr.map(|r| r.risks.as_slice()).unwrap_or(&[]);
                    let vc = crate::teamwork::VerifiedContext {
                        def: &vt.def,
                        diff_path,
                        edited_files: &edited_files,
                        trace_gaps: &vt.trace_gaps,
                        weak_checks: &weak_checks,
                        risks,
                    };
                    crate::teamwork::task_section(&t.def, Some(&vc))
                }
                None => crate::teamwork::task_section(&t.def, None),
            };
            // What an upstream task hands this one: a line for the prompt and the handoff record.
            // A verification whose findings the lead's retry hands over (`from`) is described the
            // same way.
            let describe = |u: &Task| -> (String, Value) {
                let r = u.result.clone().unwrap_or_default();
                let files: Vec<String> =
                    u.artifacts.iter().filter_map(|a| a["path"].as_str().map(str::to_owned)).collect();
                let names: Vec<&str> = u.artifacts.iter().filter_map(|a| a["name"].as_str()).collect();
                let amended = if u.amendments.is_empty() {
                    String::new()
                } else {
                    format!("\n  负责人修订了这个结论，以修订为准：{}", u.amendments.join("；"))
                };
                let handoff_note = r.handoff.as_deref().map(|h| format!("\n  交接说明：{h}")).unwrap_or_default();
                let risk_note = if r.risks.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n  上游风险：{}",
                        r.risks.iter().take(5).map(|x| short(x, INLINE_ITEM)).collect::<Vec<_>>().join("；")
                    )
                };
                // Where to look, not only what it said: the files it changed, and which of them
                // others changed after it finished.
                let edited: Vec<String> = st.traces.files(&u.def.id).into_iter().collect();
                let stale = u.last_success_seq.map(|s| st.traces.edited_after(&u.def.id, s)).unwrap_or_default();
                let traces = format!(
                    "{}{}",
                    if edited.is_empty() {
                        String::new()
                    } else {
                        format!("\n  改过的文件：{}（以工作目录里的现状为准）", edited.join("、"))
                    },
                    if stale.is_empty() { String::new() } else { format!("\n  {}", crate::traces::stale_note(&stale)) }
                );
                let verdict = u.last_verdict();
                let review = verdict
                    .map(|v| {
                        let mut note = format!("\n  独立验证判定：{}（{}）", v.label(), v.as_str());
                        for c in r.checks.iter().filter(|c| !c.passed) {
                            note.push_str(&format!(
                                "\n  未通过：[{}] {}：{}",
                                c.method_label(),
                                short(&c.criterion, INLINE_ITEM),
                                short(&c.evidence, INLINE_SUMMARY)
                            ));
                        }
                        note
                    })
                    .unwrap_or_default();
                let line = format!(
                    "- {} {}（{}，状态 {}）：{}{review}{handoff_note}{risk_note}{amended}{traces}\n  产物：{}",
                    u.def.id,
                    u.def.title,
                    u.agent,
                    u.status(),
                    short(&r.summary, INLINE_SUMMARY),
                    if files.is_empty() { "无".into() } else { files.join("，") }
                );
                let mut handoff = json!({
                    "from": u.def.id, "fromAgent": u.agent, "status": u.status(), "summary": r.summary,
                    "handoff": r.handoff, "artifacts": names, "risks": r.risks.len(), "riskItems": r.risks,
                    "amendments": u.amendments, "files": edited,
                    "stale": stale.iter().map(|(p, by)| json!({ "path": p, "by": by })).collect::<Vec<_>>()
                });
                if let Some(v) = verdict {
                    handoff["verdict"] = json!(v.as_str());
                    handoff["checks"] = json!(r.checks);
                }
                (line, handoff)
            };
            let from = match &reopen {
                Some(Reopen::Retry { from: Some(v), .. }) => st.tasks.iter().find(|x| &x.def.id == v),
                _ => None,
            };
            let (upstream, mut handoffs): (Vec<String>, Vec<Value>) =
                t.def.deps.iter().filter_map(|d| st.tasks.iter().find(|x| &x.def.id == d)).map(describe).unzip();
            // The findings reach this session too: the record of who used whose result says so.
            // `retry` refuses a `from` this task depends on, so it is never among the upstream.
            handoffs.extend(from.map(|v| describe(v).1));
            let review = match &reopen {
                Some(Reopen::Retry { .. }) => from.map(|v| describe(v).0).map(Review::Findings),
                Some(Reopen::Reverify { target }) => {
                    st.tasks.iter().find(|x| &x.def.id == target).map(|vt| Review::Recheck {
                        own: previous_verdict(&t.def, t.result.as_ref()),
                        target: describe(vt).0,
                        diffs: vt
                            .artifacts
                            .iter()
                            .rev()
                            .filter(|a| a["kind"] == "git_diff")
                            .filter_map(|a| a["path"].as_str().map(str::to_owned))
                            .take(2)
                            .collect(),
                    })
                }
                None => None,
            };
            (
                t.def.clone(),
                t.agent.clone(),
                upstream,
                handoffs,
                self.db_last_error(&run.id, id),
                team_note,
                reopen,
                review,
            )
        };
        let peers: Vec<String> = {
            let st = run.st.lock().unwrap();
            st.tasks
                .iter()
                .filter(|t| t.status() == TaskStatus::Running && t.def.id != id)
                .map(|t| {
                    format!(
                        "- {}「{}」· {}",
                        t.def.id,
                        t.def.title,
                        run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent)
                    )
                })
                .collect()
        };
        let member = run.member(&agent).context("member missing")?.clone();
        let standing = standing_instructions(&member);
        let channel = member.kind.instruction_channel();
        // The collaborator rules, then the person's standing rules, in the system layer. Codex and
        // Claude Code both take HTTP MCP servers, so their sessions always have the board.
        // @project-doc docs/domains/20261031-goal_mode.md#collaborator_rules
        let rules = format!("{}{}", collaborator_rules(true), self.memory_section(run, MemoryAccess::Write));
        let skills = skills_note(self.skill_router_md(run, &member));
        let system_text = system_layer(&[&rules, &skills, &standing]);
        let spec = member_spec(self.spec_for(&member.runtime)?, &member, &system_text);
        let system = (channel == InstructionChannel::System).then_some(system_text.as_str());
        self.db.start_attempt(&run.id, id, n, &agent, &spec.id, now_ms())?;

        let workspace = run.cwd.clone();
        let log = self.dirs(run).log(&format!("{id}-a{n}.log"));
        let owner = Owner { session: SessionKey::task(id), attempt: n };
        let (conn, mut rx, mut ctl) = self.open_session(run, &owner, &agent, &spec, &log, "Agent ").await?;
        let mut native_turn = None;
        let outcome = async {
        // A task interrupted by a restart reopens its old session; a follow-up task (`continues`),
        // a retry the lead sent back into its own session (`session: "continue"`) and a
        // verification checking reworked work load the earlier session when context headroom allows.
        let resume = {
            let mut st = run.st.lock().unwrap();
            if let Some(old) = st.resume_session.remove(id) {
                Some(SessionResume::Interrupted(old))
            } else {
                drop(st);
                let follow = |from: &str| match self.last_task_session(&run.id, from) {
                    Some(session) => SessionResume::Followup { from: from.to_string(), session },
                    None => SessionResume::Missing { from: from.to_string() },
                };
                if reopen.as_ref().is_some_and(Reopen::continues_own_session) {
                    Some(follow(id))
                } else if n == 1 {
                    def.continues.as_deref().map(follow)
                } else {
                    None
                }
            }
        };
        let opened = tokio::time::timeout(STARTUP_TIMEOUT, async {
            let mut resumed = false;
            let mut followup_from: Option<String> = None;
            let mut continuation: Option<Value> = None;
            // Collaborators write progress and blockers to the shared board. The board is an HTTP
            // MCP server; an agent that declares no HTTP MCP works without it and reports through
            // its result and questions.
            let board: Vec<Value> = if conn.capabilities["mcpCapabilities"]["http"] == true {
                vec![self.mcp_server(run, id)]
            } else {
                vec![]
            };
            let opening = self.opening(run, id, &member, &conn.capabilities, board, system);
            let session = match resume {
                Some(SessionResume::Interrupted(old)) if conn.capabilities["loadSession"] == true => {
                    match conn.request("session/load", opening.load_params(&old, &workspace)).await {
                        Ok(r) => {
                            // The reload replays history as updates; it is already in the event log.
                            // Updates can trail the response, so wait until the agent goes quiet.
                            drain_replay(&conn, &mut rx).await;
                            resumed = true;
                            let mut r = if r.is_object() { r } else { json!({}) };
                            r["sessionId"] = json!(old);
                            r
                        }
                        Err(_) => new_session(&conn, &workspace, &opening).await?,
                    }
                }
                Some(SessionResume::Followup { from, session: old }) => {
                    let over_budget = self.session_context_ratio(run, &old).is_some_and(|r| r > 0.75);
                    if over_budget {
                        continuation = Some(
                            json!({ "from": from, "session": old, "loaded": false, "fallback": "context_over_75pct" }),
                        );
                        new_session(&conn, &workspace, &opening).await?
                    } else if conn.capabilities["loadSession"] != true {
                        continuation =
                            Some(json!({ "from": from, "session": old, "loaded": false, "fallback": "unsupported" }));
                        new_session(&conn, &workspace, &opening).await?
                    } else {
                        match conn.request("session/load", opening.load_params(&old, &workspace)).await {
                            Ok(r) => {
                                drain_replay(&conn, &mut rx).await;
                                followup_from = Some(from.clone());
                                continuation = Some(json!({ "from": from, "session": old, "loaded": true }));
                                let mut r = if r.is_object() { r } else { json!({}) };
                                r["sessionId"] = json!(old);
                                r
                            }
                            Err(_) => {
                                continuation = Some(
                                    json!({ "from": from, "session": old, "loaded": false, "fallback": "load_failed" }),
                                );
                                new_session(&conn, &workspace, &opening).await?
                            }
                        }
                    }
                }
                Some(SessionResume::Missing { from }) => {
                    continuation = Some(json!({ "from": from, "loaded": false, "fallback": "no_session" }));
                    new_session(&conn, &workspace, &opening).await?
                }
                _ => new_session(&conn, &workspace, &opening).await?,
            };
            let mut applied = configure_session(&conn, &session, &member).await?;
            applied["resources"] = json!(opening.resources);
            Ok::<_, anyhow::Error>((session, applied, resumed, followup_from, continuation))
        })
        .await
        .map_err(|_| anyhow!("打开 ACP 会话超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
        let (session, applied, resumed, followup_from, continuation) = opened;
        let sid = session["sessionId"].as_str().unwrap_or_default().to_string();
        let prompt = if resumed {
            format!(
                "编排器中断后，这个会话已经恢复。请从中断的地方继续完成原来的子任务，完成后按最初的要求以 ```json 结构化结果结束。{}",
                self.take_notes(run, &agent, Some(id))
            )
        } else {
            let notes = self.take_notes(run, &agent, Some(id));
            match (&followup_from, &reopen) {
                // Back in its own session, the agent has the task and what it did: only what is
                // new goes in.
                (Some(from), Some(reopen)) if from == id => {
                    continued_prompt(&def, n, reopen, review.as_ref(), last_error.as_deref(), &notes)
                }
                (Some(from), _) => format!(
                    "你在子任务 {from} 的原会话中继续工作，之前的上下文和文件阅读记录仍在。请按下面新的子任务要求和验收标准执行，完成后以 ```json 结构化结果结束。\n\n{}{team_note}{notes}",
                    task_prompt(run, &member, &def, &upstream, &peers, last_error.as_deref()),
                ),
                (None, _) => format!(
                    "{}{team_note}{}{notes}",
                    task_prompt(run, &member, &def, &upstream, &peers, last_error.as_deref()),
                    reopen.as_ref().map(|r| reopen_section(r, review.as_ref())).unwrap_or_default(),
                ),
            }
        };
        self.db.update_attempt(
            &run.id,
            id,
            n,
            AttemptStatus::Running,
            AttemptUpdate { session: Some(&sid), workspace: Some(&workspace.to_string_lossy()), ..Default::default() },
        )?;
        let mut started_data = json!({
            "attemptId": attempt_id, "session": sid, "workspace": format!("工作目录 {}", workspace.display()), "path": workspace, "runtime": spec.name, "kind": spec.kind, "agentInfo": conn.agent_info,
            "config": applied, "enforcement": enforcement(&applied), "prompt": prompt, "resumed": resumed
        });
        if let Some(cont) = continuation {
            started_data["continuation"] = cont;
        }
        if let Some(r) = &reopen {
            started_data["reopen"] = r.record();
        }
        if let Some(g) = run.git_facts().await {
            started_data["git"] = g;
        }
        self.emit(
            run,
            ev("attempt.started", "janus", format!("启动 {id} 第 {n} 次尝试 · {}", spec.name))
                .task(id)
                .attempt(n)
                .to(&agent)
                .data(started_data),
        );
        // What each upstream task handed to this session: the fact behind "who used whose result".
        if !resumed {
            for h in handoffs {
                let from = h["from"].as_str().unwrap_or_default().to_string();
                let actor = h["fromAgent"].as_str().unwrap_or("janus").to_string();
                self.emit(
                    run,
                    ev("context.handoff", &actor, format!("{from} 的结果交给 {id}"))
                        .task(id)
                        .attempt(n)
                        .to(&agent)
                        .data(h),
                );
            }
        }

        let ctx = Ctx { agent: &agent, task: Some(id), attempt: Some(n), goal: false };
        self.show(run, ("session.controls", "会话选项".into(), crate::acp_view::controls(&session, &applied)), &ctx);
        // A loaded session is the same member's, so its system layer is the same as when it opened:
        // nothing new went in, so nothing new is recorded.
        if !resumed && followup_from.is_none() {
            self.record_system(run, &ctx, &sid, &channel, SystemPart::Collaboration, &rules);
            self.record_system(run, &ctx, &sid, &channel, SystemPart::Skills, &skills);
            self.record_instructions(run, &ctx, &sid, &channel, &standing);
        }
        self.watch_session(run, &owner.session, n);
        let mut next = Some(prompt);
        let mut text = String::new();
        let mut round = 0;
        let mut asked = 0;
        let mut total_pauses = 0;
        let mut last_question_seq = 0i64;
        // A failed verification goes back to the same session once: the agent still has the context
        // to fix its work, or to say why the command does not fit. A second failure goes to the lead.
        let mut fixes = 0;
        let result = 'session: loop {
            let mut result = loop {
                if let Some(p) = next.take() {
                    native_turn = self.native_turn(run, &member, &sid, native_turn.take().and_then(|t: NativeTurn| t.path));
                    text = self
                        .stream_prompt(
                            run,
                            &mut PromptSession { conn: &conn, rx: &mut rx, sid: &sid },
                            &p,
                            &ctx,
                            Some(&mut ctl),
                        )
                        .await?;
                    let suffix = if round == 0 { String::new() } else { format!("-r{round}") };
                    round += 1;
                    // A turn stopped before it said anything (an interrupt) leaves no report.
                    if !text.trim().is_empty() {
                        self.save_artifact(
                            run,
                            NewArtifact {
                                task: Some(id),
                                attempt: n,
                                agent: &agent,
                                name: &format!("{id}-a{n}-report{suffix}.md"),
                                kind: "report",
                                bytes: text.as_bytes(),
                                desc: "Agent 这一轮的最终回复",
                            },
                        )?;
                    }
                }
                if ctl.finishing() {
                    break finished_by_user(&text);
                }
                // Under takeover the agent waits for the person; queued messages wait with it.
                if !ctl.held_by_person() && !ctl.queued.is_empty() {
                    next = Some(self.take_queued(run, &mut ctl, &ctx, |k| format!("把排队的 {k} 条消息作为新一轮发给 {agent}")));
                    continue;
                }
                if ctl.held_by_person() {
                    let session = PromptSession { conn: &conn, rx: &mut rx, sid: &sid };
                    next = self.wait_for_person(run, &session, &mut ctl, &ctx, RELEASE_PROMPT).await?;
                    continue;
                }
                let parsed = parse_result(&text);
                let Some(q) = parsed.question.clone() else {
                    break parsed;
                };
                total_pauses += 1;
                if !self.has_split_request_since(&run.id, id, last_question_seq) {
                    asked += 1;
                }
                if asked > 4 || total_pauses > 8 {
                    bail!("连续 4 轮仍在提问，没有完成子任务");
                }
                let key = format!("q-{id}-{n}-{round}");
                let question = q["text"].as_str().or(q.as_str()).unwrap_or("需要你的决定").to_string();
                let options: Vec<String> = q["options"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                last_question_seq = self.emit(
                    run,
                    ev("question.asked", &agent, short(&question, 60)).task(id).attempt(n).to("lead").data(json!({
                        "key": key, "question": question, "options": options, "blocking": true
                    })),
                );
                // The lead answers questions on the board (it reads them with `wait` or has them
                // pushed); while it is not there, they go to the person.
                if !run.st.lock().unwrap().lead.on_duty() {
                    self.escalate_open_questions(run);
                }
                // The question card and the session both reach the same agent; whichever you use first wins.
                let _held = run.held(&owner.session, n);
                tokio::select! {
                    answer = self.wait_for(run, &key, owner.clone(), Ask::Question) => {
                        let answer = answer?;
                        next = Some(format!(
                            "你的问题已得到回答：{}\n补充说明：{}\n请据此继续完成子任务。完成后同样以 ```json 结构化结果结束。",
                            answer["answer"].as_str().unwrap_or(""),
                            answer["note"].as_str().filter(|s| !s.is_empty()).unwrap_or("无")
                        ));
                    }
                    cmd = ctl.rx.recv() => {
                        run.st.lock().unwrap().pending.remove(&key);
                        let reply = match cmd {
                            None => return Err(Cancelled.into()),
                            Some(Steer::Message(m) | Steer::Redirect(m)) => {
                                self.route_message(run, &agent, id, n, &m.id, Delivery::Prompt);
                                next = Some(m.text.clone());
                                m.text
                            }
                            Some(Steer::Finish) => {
                                ctl.set_hold(run, Hold::Finish);
                                self.record_finish(run, &ctx);
                                "（直接以当前结果完成）".into()
                            }
                            Some(_) => {
                                ctl.set_hold(run, Hold::Person);
                                self.emit(run, ev("control.taken", "user", format!("接管 {} 的会话", run.member(&agent).map(|m| m.name.as_str()).unwrap_or(&agent))).task(id).attempt(n).to(&agent));
                                "（转为人工接管，在会话中继续）".into()
                            }
                        };
                        self.emit(run, ev("question.answered", "user", format!("在会话中回答：{}", short(&reply, 40))).task(id).to(&agent).data(json!({ "key": key, "answer": reply, "via": "session" })));
                    }
                }
            };
            // An abort that arrived between turns (or a cancel) must not end up as a success.
            self.check_aborted(run, id, &mut ctl)?;
            if result.evidence.is_empty() {
                result.evidence.push(format!("最终回复：{id}-a{n}-report.md"));
            }
            let mut submitted_data = json!({
                "attemptId": attempt_id,
                "summary": result.summary, "evidence": result.evidence, "risks": result.risks, "checks": result.checks
            });
            if let Some(h) = &result.handoff {
                submitted_data["handoff"] = json!(h);
            }
            if let Some(vd) = result.verdict.map(Verdict::as_str).or(result.verdict_error.as_deref()) {
                submitted_data["verdict"] = json!(vd);
            }
            self.emit(
                run,
                ev(
                    "result.submitted",
                    &agent,
                    format!("提交结构化结果：{}", short(&result.summary, 60)),
                )
                .task(id)
                .attempt(n)
                .data(submitted_data),
            );
            match self.verify_result(run, id, n, attempt_id, &def, &mut result).await {
                Ok(()) => break 'session result,
                Err(err) if fixes == 0 && !ctl.finishing() && err.is::<VerificationFailed>() => {
                    fixes += 1;
                    let prompt = format!(
                        "你的结果还没有满足验收标准：\n{}\n\n请继续：没有达到的标准，修复后重新验证；没有核对或缺少证据的，补上验证和证据。如果某条标准在当前条件下确实无法达到，如实写 passed: false 并说明原因和你能提供的证据。完成后同样以 ```json 结构化结果结束，checks 逐条更新。",
                        verification_output(run, id)
                    );
                    self.record_session_prompt(run, &ctx, &sid, PromptChannel::Fix, &prompt);
                    next = Some(prompt);
                }
                Err(err) => return Err(err),
            }
        };
        Ok::<_, anyhow::Error>(result)
        }
        .await;
        // The agent stops before its changes are recorded, so nothing moves under the record.
        self.close_session(run, &owner, Some(&conn), &format!("{id} 的这次尝试已经结束，撤回待处理的请求")).await;
        if outcome.is_err()
            && let Some(turn) = native_turn
        {
            let ctx = Ctx { agent: &agent, task: Some(id), attempt: Some(n), goal: false };
            self.recover_native_usage(run, &turn, &ctx);
        }
        let result = outcome?;
        self.check_aborted(run, id, &mut ctl)?;
        self.record_changes(run, id, n, &agent, &workspace)?;
        // The result against what the task left in the workspace: the lead sees where they differ.
        let gaps = {
            let changed = run.git.then(|| {
                crate::traces::changed_files(
                    &git_out(&workspace, &["diff-index", "--name-only", "HEAD"]),
                    &git_out(&workspace, &["ls-files", "--others", "--exclude-standard"]),
                )
            });
            let st = run.st.lock().unwrap();
            st.tasks
                .iter()
                .find(|t| t.def.id == id)
                .map(|t| crate::traces::gaps(&t.def, &st.traces.files(id), changed.as_ref()))
                .unwrap_or_default()
        };

        // Settle the task in the same step as checking that this attempt still owns it, so a cancel
        // by the lead in between is not overwritten by a success.
        let current = {
            let mut st = run.st.lock().unwrap();
            match Run::task_mut(&mut st, id).filter(|t| t.on_attempt(attempt_id)) {
                Some(t) => {
                    t.set_phase(TaskPhase::Succeeded);
                    t.result = Some(result.clone());
                    t.trace_gaps = gaps.clone();
                    true
                }
                None => false,
            }
        };
        if !current {
            let _ = self.db.update_attempt(
                &run.id,
                id,
                n,
                AttemptStatus::Cancelled,
                AttemptUpdate { ended: Some(now_ms()), ..Default::default() },
            );
            self.emit(
                run,
                ev("attempt.late_result", "janus", format!("忽略已过期 Attempt {attempt_id} 的成功结果"))
                    .task(id)
                    .attempt(n),
            );
            return Ok(());
        }
        let mut succeeded_data = json!({
            "attemptId": attempt_id,
            // The conclusion carries the whole result, so the app, the delivery and a resumed
            // run read it from this one event.
            "summary": result.summary, "evidence": result.evidence, "risks": result.risks, "traceGaps": gaps,
            "checks": result.checks, "acceptance": Summary::of(&def.accept, &result.checks)
        });
        if let Some(h) = &result.handoff {
            succeeded_data["handoff"] = json!(h);
        }
        if def.verifies.is_some() {
            let vd = crate::evidence::effective_verdict(result.verdict, def.accept.len(), &result.checks);
            succeeded_data["verdict"] = json!(vd.as_str());
        }
        let seq = self.emit(
            run,
            ev("task.succeeded", "janus", format!("校验结构化结果与证据后，将 {id} 标记为完成"))
                .task(id)
                .attempt(n)
                .data(succeeded_data),
        );
        if let Some(t) = Run::task_mut(&mut run.st.lock().unwrap(), id) {
            t.last_success_seq = Some(seq);
        }
        self.recheck_verifications(run, id, n, seq);
        // The event above is the fact; a failed bookkeeping write must not turn it into a retry.
        if let Err(err) = self.db.update_attempt(
            &run.id,
            id,
            n,
            AttemptStatus::Succeeded,
            AttemptUpdate { ended: Some(now_ms()), ..Default::default() },
        ) {
            crate::config::log(format!("janus: failed to record attempt {id}#{n}: {err:#}"));
        }
        Ok(())
    }

    /// Stop an attempt whose run was cancelled, even if the cancel reached it while it was not
    /// listening (between turns or during verification).
    pub(super) fn check_aborted(&self, run: &Run, id: &str, ctl: &mut Control) -> Result<()> {
        // The lead's replies are about this task; the person's messages are for the agent.
        let mut late = vec![];
        while let Ok(cmd) = ctl.rx.try_recv() {
            match cmd {
                Steer::Redirect(m) => late.push(Note::new(m, id)),
                Steer::Message(m) => late.push(Note::new(m, "")),
                _ => {}
            }
        }
        if let Some(agent) = run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == id).map(|t| t.agent.clone()) {
            for n in &late {
                self.route_message(run, &agent, id, 0, &n.msg.id, Delivery::Next);
            }
        }
        let mut st = run.st.lock().unwrap();
        // Messages sent after the last turn are kept for this agent's next session.
        if let Some(agent) = st.tasks.iter().find(|t| t.def.id == id && !late.is_empty()).map(|t| t.agent.clone()) {
            st.notes.entry(agent).or_default().extend(late);
        }
        if st.cancelled() {
            return Err(Cancelled.into());
        }
        Ok(())
    }

    /// Janus does not run commands of its own to verify a task. The agent verifies each acceptance
    /// criterion in whatever way it judges right and reports it, with the method it used; Janus
    /// checks that every criterion was answered, with evidence, that the method is allowed, and
    /// that none was reported unmet.
    // @project-doc docs/domains/20261031-run_lifecycle.md#verification
    pub(super) async fn verify_result(
        &self,
        run: &Run,
        id: &str,
        n: i64,
        attempt_id: &str,
        def: &TaskDef,
        result: &mut TaskResult,
    ) -> Result<()> {
        self.emit(
            run,
            ev("verification.started", "janus", format!("核对 {id} 第 {n} 次尝试的结果"))
                .task(id)
                .attempt(n)
                .data(json!({ "attemptId": attempt_id, "accept": def.accept })),
        );
        let failures = crate::evidence::gaps_for(
            &def.accept,
            &result.summary,
            &result.checks,
            def.verifies.is_some(),
            result.verdict,
            result.verdict_error.as_deref(),
        );
        // The numbers and the weaker evidence that only warrants a reminder, recorded with the
        // verdict so the app and a resumed run do not have to rebuild them differently.
        let summary = Summary::of(&def.accept, &result.checks);
        if !failures.is_empty() {
            let message = failures.join("；");
            run.st
                .lock()
                .unwrap()
                .last_verification
                .insert(id.to_string(), failures.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"));
            self.db.insert_verification(&VerificationRow {
                run_id: &run.id,
                task: id,
                attempt: n,
                attempt_id,
                status: "failed",
                details: &json!({ "attemptId": attempt_id, "failures": failures }),
                created_at: now_ms(),
            })?;
            self.emit(
                run,
                ev("verification.failed", "janus", format!("{id} 没有通过验收核对：{}", short(&message, 120)))
                    .task(id)
                    .attempt(n)
                    .data(json!({ "attemptId": attempt_id, "failures": failures, "checks": result.checks, "acceptance": summary })),
            );
            return Err(VerificationFailed(message).into());
        }
        self.db.insert_verification(&VerificationRow {
            run_id: &run.id,
            task: id,
            attempt: n,
            attempt_id,
            status: "passed",
            details: &json!({ "attemptId": attempt_id, "checks": result.checks }),
            created_at: now_ms(),
        })?;
        self.emit(
            run,
            ev("verification.passed", "janus", format!("{id} 通过验收核对"))
                .task(id)
                .attempt(n)
                .data(json!({ "attemptId": attempt_id, "checks": result.checks, "acceptance": summary })),
        );
        Ok(())
    }

    /// The failure of the previous attempt, if the previous attempt failed. A task that succeeded
    /// since (and was retried by the person) starts fresh, not from an old error.
    pub(super) fn db_last_error(&self, run_id: &str, task: &str) -> Option<String> {
        self.db
            .events(run_id, 0)
            .ok()?
            .into_iter()
            .rev()
            .find(|e| matches!(e.kind.as_str(), "attempt.failed" | "task.succeeded") && e.task.as_deref() == Some(task))
            .filter(|e| e.kind == "attempt.failed")
            .and_then(|e| e.data?["error"].as_str().map(str::to_owned))
    }

    /// Last non-empty ACP `sessionId` opened for `task` in this run.
    pub(super) fn last_task_session(&self, run_id: &str, task: &str) -> Option<String> {
        self.db
            .events(run_id, 0)
            .ok()?
            .into_iter()
            .rev()
            .filter(|e| e.kind == "attempt.started" && e.task.as_deref() == Some(task))
            .find_map(|e| e.data?["session"].as_str().filter(|s| !s.is_empty()).map(str::to_owned))
    }

    /// Latest `used / size` context ratio reported for ACP session `sid` in the run, if any.
    pub(super) fn session_context_ratio(&self, run: &Run, sid: &str) -> Option<f64> {
        self.usage(run).sessions_of(sid).rev().find_map(|s| s.context.ratio())
    }

    /// Whether `task` submitted a `task.split_requested` event after `since_seq`.
    pub(super) fn has_split_request_since(&self, run_id: &str, task: &str, since_seq: i64) -> bool {
        self.db
            .events(run_id, since_seq)
            .is_ok_and(|evts| evts.iter().any(|e| e.kind == "task.split_requested" && e.task.as_deref() == Some(task)))
    }

    /// Mark the transcript and event stream immediately before sending this prompt. A follow-up
    /// or retry can reuse the same native session and must only recover its own turn.
    pub(super) fn native_turn(
        &self,
        run: &Run,
        member: &Member,
        sid: &str,
        known_path: Option<PathBuf>,
    ) -> Option<NativeTurn> {
        if !member.kind.native_transcript() || sid.is_empty() || Path::new(sid).file_name()?.to_str()? != sid {
            return None;
        }
        let projects = self.cfg.layout.agent_config(&member.runtime).join("projects");
        let path = known_path.or_else(|| find_session_jsonl(&projects, &format!("{sid}.jsonl"), 3));
        let offset = match &path {
            Some(p) => p.metadata().ok()?.len(),
            None => 0,
        };
        Some(NativeTurn { projects, sid: sid.into(), path, offset, after_seq: run.seq.load(Ordering::SeqCst) })
    }

    pub(super) fn recover_native_usage(&self, run: &Run, turn: &NativeTurn, ctx: &Ctx<'_>) {
        use std::io::{Read, Seek, SeekFrom};
        let Some(path) =
            turn.path.clone().or_else(|| find_session_jsonl(&turn.projects, &format!("{}.jsonl", turn.sid), 3))
        else {
            return;
        };
        let Ok(mut file) = std::fs::File::open(path) else { return };
        if !file.metadata().is_ok_and(|m| m.len() > turn.offset) || file.seek(SeekFrom::Start(turn.offset)).is_err() {
            return;
        }
        // A cancelled prompt may still have returned its usage. A previous fallback for this
        // turn is also a report. Context-only snapshots do not count.
        let Ok(events) = self.db.events(&run.id, turn.after_seq) else { return };
        if events.iter().any(|e| {
            e.kind == "session.usage"
                && e.actor == ctx.agent
                && e.task.as_deref() == ctx.task
                && e.attempt == ctx.attempt
                && e.data.as_ref().is_some_and(|d| !crate::usage::Tokens::of_turn(d).is_empty())
        }) {
            return;
        }
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok()
            && let Some(mut data) = parse_claude_session_jsonl(&content)
        {
            data["session"] = json!(turn.sid);
            self.show(run, ("session.usage", "从隔离会话日志补齐本轮用量".into(), data), ctx);
        }
    }

    /// Working in place there is nothing to commit. In a Git directory, keep what the working
    /// tree now differs from HEAD by whenever a task leaves it different, as that task's diff.
    /// Tasks running side by side share the directory, so the diff can include their changes too.
    /// Keep what the attempt changed as a diff artifact. Tasks share the working tree, so the diff
    /// is limited to the files the task owns and edited (`traces::scope`); only without a record
    /// of either does it cover the whole tree, and then nothing is saved when the tree is as the
    /// last task left it.
    // @project-doc docs/domains/20261031-goal_mode.md#traces
    pub(super) fn record_changes(&self, run: &Run, id: &str, n: i64, agent: &str, workspace: &Path) -> Result<()> {
        if !run.git {
            return Ok(());
        }
        let scope = {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == id).context("task missing")?;
            crate::traces::scope(&t.def, &st.traces.files(id))
        };
        let whole = working_diff(workspace, "HEAD", &[]);
        let unchanged = std::mem::replace(&mut run.st.lock().unwrap().last_diff, whole.clone()) == whole;
        let diff = if scope.is_empty() { whole } else { working_diff(workspace, "HEAD", &scope) };
        if diff.trim().is_empty() || (scope.is_empty() && unchanged) {
            return Ok(());
        }
        let stat = git_scoped(workspace, &["diff-index", "--shortstat", "HEAD"], &scope);
        let new_files = git_scoped(workspace, &["ls-files", "--others", "--exclude-standard"], &scope).lines().count();
        let desc = [
            Some(crate::traces::diff_summary(&stat, new_files)).filter(|s| !s.is_empty()),
            Some(if scope.is_empty() {
                "工作目录相对 HEAD 的全部改动（没有它改了哪些文件的记录，可能含其他子任务的改动），未提交".to_string()
            } else {
                format!("{id} 负责和改过的文件相对 HEAD 的改动，未提交")
            }),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        self.save_artifact(
            run,
            NewArtifact {
                task: Some(id),
                attempt: n,
                agent,
                name: &format!("{id}-a{n}.diff"),
                kind: "git_diff",
                bytes: diff.as_bytes(),
                desc: &desc,
            },
        )?;
        Ok(())
    }

    /// What the whole run changed so far against where it started (`base_ref`): the lead's own
    /// edits, every task's, and commits made in the run, which is what accepting brings back. A
    /// task's diff only covers that task, so a lead that did the work itself left none. Saved as a
    /// `run_diff` artifact after each of the lead's turns when it differs from the last one.
    // @project-doc docs/domains/20261031-goal_mode.md#traces
    pub(super) fn record_run_changes(&self, run: &Run) -> Result<()> {
        if run.kind != RunKind::Goal || !run.git || run.base_ref.is_empty() {
            return Ok(());
        }
        let diff = working_diff(&run.cwd, &run.base_ref, &[]);
        let hash: String = Sha256::digest(diff.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("run-{}.diff", &hash[..12]);
        // After a restart nothing is remembered: the same diff saved before is already there.
        let saved = match &run.st.lock().unwrap().run_diff {
            Some((last, _)) => *last == hash,
            None => self.dirs(run).changes().join(&name).exists(),
        };
        if diff.trim().is_empty() || saved {
            return Ok(());
        }
        let stat = git_out(&run.cwd, &["diff-index", "--shortstat", &run.base_ref]);
        let new_files = git_out(&run.cwd, &["ls-files", "--others", "--exclude-standard"])
            .lines()
            .filter(|l| !l.is_empty())
            .count();
        let desc = [
            crate::traces::diff_summary(&stat, new_files),
            "整个任务相对开始时的改动，验收时合并回去的就是这些".into(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let artifact = self.save_artifact(
            run,
            NewArtifact {
                task: None,
                attempt: 0,
                agent: "janus",
                name: &name,
                kind: "run_diff",
                bytes: diff.as_bytes(),
                desc: &desc,
            },
        )?;
        run.st.lock().unwrap().run_diff = Some((hash, artifact));
        Ok(())
    }

    /// Store an artifact and announce it; returns it as the run's state and the delivery list it.
    pub(super) fn save_artifact(&self, run: &Run, artifact: NewArtifact<'_>) -> Result<Value> {
        let NewArtifact { task, attempt: n, agent, name, kind, bytes, desc } = artifact;
        let dirs = self.dirs(run);
        let dir = task.map_or_else(|| dirs.changes(), |t| dirs.artifacts(t));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        let sha: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        self.db.insert_artifact(&ArtifactRow {
            run_id: &run.id,
            // The table's column has no empty value of its own: the whole run's has kind `run_diff`.
            task: task.unwrap_or_default(),
            attempt: n,
            name,
            kind,
            path: &path.to_string_lossy(),
            sha256: &sha,
            bytes: bytes.len() as i64,
            created_at: now_ms(),
        })?;
        let mut e = ev("artifact.created", agent, format!("提交产物 {name}")).data(json!({
            "name": name, "kind": kind, "desc": desc, "path": path, "hash": &sha[..12], "bytes": bytes.len()
        }));
        if let Some(t) = task {
            e = e.task(t).attempt(n);
        }
        let seq = self.emit(run, e);
        let artifact = json!({ "name": name, "kind": kind, "desc": desc, "path": path, "seq": seq });
        let mut st = run.st.lock().unwrap();
        if let Some(t) = task.and_then(|t| Run::task_mut(&mut st, t)) {
            t.artifacts.retain(|a| a["kind"] != kind || kind == "report");
            t.artifacts.push(artifact.clone());
        }
        Ok(artifact)
    }
}

pub(super) fn git_out(cwd: &Path, args: &[&str]) -> String {
    git_probe::read_only(cwd, args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// A git query limited to `scope` (paths taken literally, not as patterns); all of the working
/// tree when `scope` is empty.
pub(super) fn git_scoped(cwd: &Path, args: &[&str], scope: &[String]) -> String {
    let mut all: Vec<&str> = vec!["--literal-pathspecs"];
    all.extend(args);
    if !scope.is_empty() {
        all.push("--");
        all.extend(scope.iter().map(String::as_str));
    }
    git_out(cwd, &all)
}

/// What the working tree differs from the commit `against` by within `scope` (everything when
/// empty), untracked files included as diffs against an empty file.
pub(super) fn working_diff(cwd: &Path, against: &str, scope: &[String]) -> String {
    // `diff-index`, not `diff`: porcelain `git diff` refreshes the index even with optional locks
    // off (see `git_probe`).
    let mut diff = git_scoped(cwd, &["diff-index", "-p", against], scope);
    // Untracked files have no `diff-index` entry. Diff each against /dev/null so the artifact
    // carries their content, then list them so a review can mark them as new files.
    let untracked = git_scoped(cwd, &["ls-files", "--others", "--exclude-standard", "-z"], scope);
    for path in untracked.split('\0').filter(|p| !p.is_empty()) {
        diff.push_str(&git_out(
            cwd,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-index",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "/dev/null",
                path,
            ],
        ));
    }
    if !untracked.trim().is_empty() {
        diff.push_str(&format!("\n# 未跟踪的新文件\n{}", untracked.replace('\0', "\n")));
    }
    diff
}

pub(super) struct NativeTurn {
    projects: PathBuf,
    sid: String,
    path: Option<PathBuf>,
    offset: u64,
    after_seq: i64,
}

fn find_session_jsonl(dir: &Path, filename: &str, depth: u8) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let direct = dir.join(filename);
    if std::fs::symlink_metadata(&direct).is_ok_and(|m| m.is_file()) {
        return Some(direct);
    }
    if depth == 1 {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir())
            && let Some(found) = find_session_jsonl(&path, filename, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

/// Parse an isolated Claude Code `<sid>.jsonl` transcript into a single `session.usage` payload,
/// deduplicating repeated streaming entries that share the same `message.id` (keeping the final
/// usage snapshot of each message).
pub(super) fn parse_claude_session_jsonl(content: &str) -> Option<Value> {
    #[derive(Clone, Default)]
    struct MsgUsage {
        model: String,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
    }
    let mut order: Vec<String> = vec![];
    let mut by_id: HashMap<String, MsgUsage> = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let msg = &v["message"];
        let usage = &msg["usage"];
        if !usage.is_object() {
            continue;
        }
        let input = usage["input_tokens"].as_i64().unwrap_or(0);
        let output = usage["output_tokens"].as_i64().unwrap_or(0);
        let cache_read = usage["cache_read_input_tokens"].as_i64().unwrap_or(0);
        let cache_write = usage["cache_creation_input_tokens"].as_i64().unwrap_or(0);
        if input + output + cache_read + cache_write <= 0 {
            continue;
        }
        let key =
            msg["id"].as_str().filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_else(|| format!("anon-{idx}"));
        let model = msg["model"].as_str().unwrap_or("").to_string();
        if !by_id.contains_key(&key) {
            order.push(key.clone());
        }
        by_id.insert(key, MsgUsage { model, input, output, cache_read, cache_write });
    }
    if order.is_empty() {
        return None;
    }
    let (mut tot_in, mut tot_out, mut tot_read, mut tot_write) = (0i64, 0i64, 0i64, 0i64);
    let mut per_model: BTreeMap<String, (i64, i64, i64, i64)> = BTreeMap::new();
    for key in &order {
        let m = &by_id[key];
        tot_in += m.input;
        tot_out += m.output;
        tot_read += m.cache_read;
        tot_write += m.cache_write;
        let e = per_model.entry(m.model.clone()).or_default();
        e.0 += m.input;
        e.1 += m.output;
        e.2 += m.cache_read;
        e.3 += m.cache_write;
    }
    let total = tot_in + tot_out + tot_read + tot_write;
    let mut data = json!({
        "inputTokens": tot_in,
        "outputTokens": tot_out,
        "cachedReadTokens": tot_read,
        "cachedWriteTokens": tot_write,
        "totalTokens": total,
        "source": "native_log",
    });
    if per_model.keys().any(|model| !model.is_empty()) {
        let models: Vec<Value> = per_model
            .into_iter()
            .map(|(model, (i, o, cr, cw))| {
                json!({
                    "model": model,
                    "inputTokens": i,
                    "outputTokens": o,
                    "cachedReadTokens": cr,
                    "cachedWriteTokens": cw,
                    "totalTokens": i + o + cr + cw,
                })
            })
            .collect();
        data["models"] = json!(models);
    }
    Some(data)
}

#[cfg(test)]
mod native_usage_tests {
    use super::*;

    #[test]
    fn claude_jsonl_deduplicates_message_ids_and_groups_by_model() {
        let log = [
            json!({ "type": "assistant", "message": { "id": "msg_1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 100, "output_tokens": 10, "cache_read_input_tokens": 500, "cache_creation_input_tokens": 50 } } }).to_string(),
            json!({ "type": "assistant", "message": { "id": "msg_1", "model": "claude-sonnet-4-5", "usage": { "input_tokens": 100, "output_tokens": 45, "cache_read_input_tokens": 500, "cache_creation_input_tokens": 50 } } }).to_string(),
            json!({ "type": "assistant", "message": { "id": "msg_2", "model": "claude-haiku-4-5", "usage": { "input_tokens": 80, "output_tokens": 20, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0 } } }).to_string(),
        ]
        .join("\n");
        let parsed = parse_claude_session_jsonl(&log).unwrap();
        assert_eq!(parsed["inputTokens"], 180);
        assert_eq!(parsed["outputTokens"], 65);
        assert_eq!(parsed["cachedReadTokens"], 500);
        assert_eq!(parsed["cachedWriteTokens"], 50);
        assert_eq!(parsed["totalTokens"], 795);
        assert_eq!(parsed["source"], "native_log");
        assert_eq!(parsed["models"].as_array().unwrap().len(), 2);
    }
}
