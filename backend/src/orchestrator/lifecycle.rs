//! Creating and driving a run: the scheduler loop, the watchdog and the delivery.

use super::*;

impl App {
    // @project-doc docs/domains/run_lifecycle.md#create_run
    // @project-doc docs/domains/run_lifecycle.md#workspace_model
    pub async fn create_run(self: &Arc<Self>, req: NewRun) -> Result<String> {
        if req.goal.trim().is_empty() && req.images.is_empty() {
            bail!("目标不能为空");
        }
        let ws = self.workspace(&req.workspace)?;
        let (cwd, team) = self.team_for(&ws)?;
        if !cwd.is_dir() {
            bail!("工作目录不存在：{}", cwd.display());
        }
        for m in &team {
            self.spec_for(&m.runtime)?;
        }
        if let Some(lead) = team.iter().find(|m| m.lead).filter(|m| !lead_capable(&m.kind)) {
            bail!("主控「{}」用的是 {}，只有 Claude Code 或 Codex 能做主控（需要 /goal），请在工作区里换一个主控", lead.name, lead.runtime_name);
        }
        let is_git = git(&cwd, &["rev-parse", "--is-inside-work-tree"])
            .await
            .is_ok();
        let mut team = team;
        let base_ref = if is_git {
            git(&cwd, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };
        let id = format!("r{}", to_base36(now_ms() as u64));
        self.snapshot_skills(&id, &mut team)?;
        let goal_images = crate::attachments::save(&self.cfg.data_dir, &id, &req.images)?;
        let goal = match req.goal.trim() {
            "" => "按附图完成".to_string(),
            g => g.to_string(),
        };
        let config = json!({ "team": team, "git": is_git, "baseRef": base_ref, "workspace": ws, "goalImages": goal_images });
        self.db.insert_run(
            &id,
            &goal,
            &cwd.to_string_lossy(),
            &config,
            now_ms(),
        )?;
        let owner = format!("janus-{}", std::process::id());
        if self
            .db
            .acquire_lease(&id, "run", &owner, now_ms(), 10 * 60 * 1000)?
            .is_none()
        {
            bail!("Run 已被另一个 Janus 实例接管");
        }
        let run = Arc::new(Run {
            id: id.clone(),
            goal,
            cwd,
            team,
            git: is_git && !base_ref.is_empty(),
            base_ref,
            brief: ws["brief"].as_str().unwrap_or("").trim().to_string(),
            goal_images,
            seq: AtomicI64::new(0),
            emitting: StdMutex::new(()),
            deadline_ms: AtomicI64::new(now_ms() + RUN_BUDGET_MS),
            st: StdMutex::new(RunState {
                status: RunStatus::Planning,
                ..Default::default()
            }),
            wake: Notify::new(),
            driving: Default::default(),
            lead_nudge: Notify::new(),
            watch: Default::default(),
        });
        self.runs.lock().unwrap().insert(id.clone(), run.clone());
        self.launch(run, Phase::Start);
        Ok(id)
    }

    pub(super) fn launch(self: &Arc<Self>, run: Arc<Run>, phase: Phase) {
        run.driving.store(true, Ordering::SeqCst);
        let app = self.clone();
        tokio::spawn(async move {
            let result = match phase {
                Phase::Start => app.drive(&run).await,
                Phase::Execute => app.execute(&run).await,
                Phase::Goal(resumed) => app.goal(&run, resumed).await,
            };
            run.driving.store(false, Ordering::SeqCst);
            if let Err(err) = result
                && err.downcast_ref::<Cancelled>().is_none() && !run.st.lock().unwrap().cancelled {
                    app.emit(
                        &run,
                        ev("run.failed", "janus", format!("Run 失败：{err:#}"))
                            .data(json!({ "error": format!("{err:#}") })),
                    );
                    app.set_status(&run, RunStatus::Failed);
                    // A failed run is rebuilt from its event log when the person continues it.
                    app.runs.lock().unwrap().remove(&run.id);
                }
        });
    }

    /// Restart execution when a decision arrives after the driver has stopped, e.g. a retry
    /// after delivery. Only runs whose plan is frozen have anything to execute.
    pub(super) fn ensure_driver(self: &Arc<Self>, run: &Arc<Run>) {
        let executable = {
            let st = run.st.lock().unwrap();
            !st.cancelled && !st.tasks.is_empty()
        };
        if executable
            && run
                .driving
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            self.launch(run.clone(), Phase::Execute);
        }
    }

    /// Copy each member's skills into this run's own directory. Agents get the snapshot path,
    /// so later edits to the library do not change a run, and no agent reads the library itself.
    pub(super) fn snapshot_skills(&self, run_id: &str, team: &mut [Member]) -> Result<()> {
        let root = self.cfg.data_dir.join("runs").join(run_id).join("skills");
        for m in team.iter_mut() {
            for s in m.skills.iter_mut() {
                let (Some(id), Some(src)) = (
                    s["id"].as_str().map(str::to_owned),
                    s["path"].as_str().map(PathBuf::from),
                ) else {
                    continue;
                };
                let dest = root.join(&id);
                if !dest.exists() {
                    copy_dir(&src, &dest)?;
                }
                s["path"] = json!(dest);
            }
        }
        Ok(())
    }

    pub(super) async fn drive(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        let workspace_note = if run.git {
            format!("Git 仓库，基线 {}", short_sha(&run.base_ref))
        } else {
            "非 Git 目录：任务直接在这个目录中执行，没有 diff".to_string()
        };
        self.emit(run, ev("run.created", "user", format!("创建目标：{}", run.goal)).data(json!({
            "goal": run.goal, "cwd": run.cwd, "git": run.git, "baseRef": run.base_ref, "workspace": workspace_note,
            "images": run.goal_images
        })));
        self.goal(run, false).await
    }

    /// The scheduling loop. It recomputes the run's status from the task graph on every pass and
    /// only returns once the run is delivered or cancelled, so every later decision still has a driver to act on it.
    // @project-doc docs/domains/run_lifecycle.md#scheduling
    // @project-doc docs/domains/run_lifecycle.md#delivery_conditions
    pub(super) async fn execute(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        enum Outcome {
            Deliver,
            /// Goal mode: the lead's session broke off; the person continues the run to restart it.
            LeadFailed(String),
            Budget,
            Paused,
            Running,
        }
        loop {
            if run.st.lock().unwrap().cancelled {
                return Err(Cancelled.into());
            }
            let over_budget = now_ms() > run.deadline_ms.load(Ordering::SeqCst);
            let (starts, blocked, outcome) = {
                let mut st = run.st.lock().unwrap();
                let statuses: HashMap<String, TaskStatus> = st
                    .tasks
                    .iter()
                    .map(|t| (t.def.id.clone(), t.status))
                    .collect();
                let succeeded: HashSet<String> = statuses
                    .iter()
                    .filter_map(|(id, status)| (*status == TaskStatus::Succeeded).then_some(id.clone()))
                    .collect();
                let mut blocked = vec![];
                for t in st.tasks.iter_mut().filter(|t| t.status == TaskStatus::Pending) {
                    if let Some(dep) = t.def.dep_failed(&statuses) {
                        t.set_status(TaskStatus::Blocked);
                        t.blocked_by = Some(dep.clone());
                        blocked.push((t.def.id.clone(), dep));
                    }
                }
                // A blocked task waits again once none of its dependencies is failed any more (the
                // lead retried or accepted them); one still failed keeps it blocked, and is named.
                for t in st.tasks.iter_mut().filter(|t| t.status == TaskStatus::Blocked) {
                    match t.def.dep_failed(&statuses) {
                        None => {
                            t.set_status(TaskStatus::Pending);
                            t.blocked_by = None;
                        }
                        Some(dep) if t.blocked_by.as_ref() != Some(&dep) => {
                            t.blocked_by = Some(dep.clone());
                            blocked.push((t.def.id.clone(), dep));
                        }
                        Some(_) => {}
                    }
                }
                let mut starts = vec![];
                if !st.paused && !over_budget {
                    let mut busy: HashSet<String> = st
                        .tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::Running)
                        .map(|t| t.agent.clone())
                        .collect();
                    for t in st.tasks.iter_mut().filter(|t| t.status == TaskStatus::Pending) {
                        if t.def.deps.iter().all(|d| succeeded.contains(d)) && !busy.contains(&t.agent) {
                            busy.insert(t.agent.clone());
                            t.set_status(TaskStatus::Running);
                            t.attempts += 1;
                            let attempt_id =
                                format!("{}-{}-{}-{}", run.id, t.def.id, t.attempts, now_ms());
                            t.active_attempt = Some(attempt_id.clone());
                            starts.push((
                                t.def.id.clone(),
                                t.attempts,
                                t.agent.clone(),
                                attempt_id,
                            ));
                        }
                    }
                }
                let active = st.tasks.iter().any(|t| t.status == TaskStatus::Running);
                // A task that could start but for a pause or the budget is still work to do: the
                // run waits for the person instead of delivering without it.
                let runnable = st.tasks.iter().any(|t| t.status == TaskStatus::Pending && t.def.deps.iter().all(|d| succeeded.contains(d)));
                let finished = !active && !runnable;
                let all_terminal = st.tasks.iter().all(|t| t.status.terminal());
                // The lead decides what a failure means (it reads it on the board), so nothing
                // waits for the person; the run ends once the lead is done and no task is running
                // or can still start.
                let outcome = if let (true, Some(Err(err))) = (finished, &st.lead_done) {
                    Outcome::LeadFailed(err.clone())
                } else if finished && st.lead_done.is_some() {
                    Outcome::Deliver
                } else if over_budget && !all_terminal {
                    Outcome::Budget
                } else if !active && starts.is_empty() && st.paused {
                    Outcome::Paused
                } else {
                    Outcome::Running
                };
                (starts, blocked, outcome)
            };
            for (id, dep) in blocked {
                self.emit(
                    run,
                    ev(
                        "task.blocked",
                        "janus",
                        format!("{id} 被失败的硬依赖 {dep} 阻塞"),
                    )
                    .task(&id)
                    .data(json!({ "dependency": dep, "kind": "hard" })),
                );
            }
            for (id, n, agent, attempt_id) in starts {
                if n == 1 {
                    self.emit(
                        run,
                        ev(
                            "task.ready",
                            "janus",
                            format!("{id} 的依赖已满足，进入 ready"),
                        )
                        .task(&id)
                        .data(json!({ "tasks": [id] })),
                    );
                    self.emit(
                        run,
                        ev(
                            "task.assigned",
                            "janus",
                            format!(
                                "{id} 分配给 {}",
                                run.member(&agent)
                                    .map(|m| m.name.as_str())
                                    .unwrap_or(&agent)
                            ),
                        )
                        .task(&id)
                        .to(&agent)
                        .data(json!({ "agent": agent })),
                    );
                }
                self.emit(
                    run,
                    ev(
                        "attempt.launch_requested",
                        "janus",
                        format!("准备启动 {id} 第 {n} 次尝试"),
                    )
                    .task(&id)
                    .attempt(n)
                    .data(json!({ "attemptId": attempt_id })),
                );
                let (app, run, id) = (self.clone(), run.clone(), id.clone());
                tokio::spawn(async move { app.run_attempt(&run, &id, n, &attempt_id).await });
            }
            match outcome {
                Outcome::Deliver => {
                    self.deliver(run);
                    return Ok(());
                }
                Outcome::LeadFailed(err) => {
                    // Like any failed run: continuing it rebuilds the board and restarts the lead.
                    bail!("主控的会话异常结束：{err}");
                }
                Outcome::Budget => {
                    // Announced each time the run comes to wait on the budget, including when the
                    // lead adds work after the run went back to running.
                    if run.st.lock().unwrap().status != RunStatus::WaitingInput {
                        self.set_status(run, RunStatus::WaitingInput);
                        self.emit(
                            run,
                            ev(
                                "run.budget_exhausted",
                                "janus",
                                "Run 达到墙钟预算，等待你追加预算、接受部分结果或取消",
                            )
                            .data(json!({ "budgetMs": RUN_BUDGET_MS })),
                        );
                    }
                }
                Outcome::Running | Outcome::Paused => {
                    let status = run.st.lock().unwrap().status;
                    if status != RunStatus::Running {
                        self.set_status(run, RunStatus::Running);
                    }
                }
            }
            self.watchdog(run);
            let _ = tokio::time::timeout(Duration::from_secs(2), run.wake.notified()).await;
        }
    }

    /// Tell the lead about running attempts that show no sign of work: nothing from the session
    /// for `stall_after`, while the agent is not waiting on a person (a question, an approval, a
    /// form, a takeover). Each quiet spell is reported once. The watchdog only reports; the lead
    /// decides whether to wait, redirect, retry or cancel. The lead's own session is watched the
    /// same way and reported to the person (`lead.stalled`): nobody above the lead would notice.
    // @project-doc docs/domains/run_lifecycle.md#watchdog
    pub(super) fn watchdog(&self, run: &Run) {
        let now = now_ms();
        let limit = self.cfg.stall_after.as_millis() as i64;
        let due: Vec<(String, i64, i64)> = run
            .watch
            .lock()
            .unwrap()
            .iter_mut()
            .filter_map(|(task, w)| {
                if w.human || w.waits > 0 {
                    // Waiting on a person is not the agent's silence; the clock starts once they are done.
                    w.last_ms = now;
                    return None;
                }
                if w.idle_noticed || now - w.last_ms < limit {
                    return None;
                }
                w.idle_noticed = true;
                Some((task.clone(), w.attempt, now - w.last_ms))
            })
            .collect();
        for (task, n, idle) in due {
            if task == LEAD_WATCH {
                self.emit(run, ev("lead.stalled", "janus", format!("主控已有 {} 没有任何动静", span(idle))).to("user").data(json!({ "reason": "idle", "idleMs": idle })));
                continue;
            }
            self.emit(
                run,
                ev("task.stalled", "janus", format!("{task} 已有 {} 没有任何动静", span(idle)))
                    .task(&task)
                    .attempt(n)
                    .to("lead")
                    .data(json!({ "reason": "idle", "idleMs": idle })),
            );
        }
    }

    /// Count a finished tool call; every `LOOP_REPEAT` identical calls in a row, tell the lead.
    // @project-doc docs/domains/run_lifecycle.md#watchdog
    pub(super) fn watch_call(&self, run: &Run, task: &str, kind: &str, title: &str, input: &str) {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        (kind, title, input).hash(&mut h);
        let call = h.finish();
        let (n, repeats) = {
            let mut watch = run.watch.lock().unwrap();
            let Some(w) = watch.get_mut(task) else { return };
            if w.call == call {
                w.repeats += 1;
            } else {
                w.call = call;
                w.repeats = 1;
            }
            if w.repeats % LOOP_REPEAT != 0 {
                return;
            }
            (w.attempt, w.repeats)
        };
        if task == LEAD_WATCH {
            self.emit(run, ev("lead.stalled", "janus", format!("主控连续 {repeats} 次做同样的调用：{}", short(title, 60))).to("user").data(json!({ "reason": "loop", "tool": title, "repeats": repeats })));
            return;
        }
        self.emit(
            run,
            ev("task.stalled", "janus", format!("{task} 连续 {repeats} 次做同样的调用：{}", short(title, 60)))
                .task(task)
                .attempt(n)
                .to("lead")
                .data(json!({ "reason": "loop", "tool": title, "repeats": repeats })),
        );
    }

    /// The lead's goal ended and nothing can still run: hand over its summary, with every task's
    /// evidence, acceptance record, independent verification, trace gaps and risks, and unfinished
    /// tasks listed as risks.
    pub(super) fn deliver(&self, run: &Run) {
        let result = {
            // Where a finished task's result disagreed with the workspace, from its own event.
            let events = self.db.events(&run.id, 0).unwrap_or_default();
            // The same ledger the Run page and the board read; usage is never required to deliver.
            let usage = crate::usage::ledger(&events).delivery_line();
            let trace_gaps = |id: &str| -> Vec<String> {
                events
                    .iter()
                    .rev()
                    .find(|e| e.kind == "task.succeeded" && e.task.as_deref() == Some(id))
                    .and_then(|e| e.data.as_ref()?["traceGaps"].as_array().cloned())
                    .into_iter()
                    .flatten()
                    .filter_map(|g| g.as_str().map(str::to_owned))
                    .collect()
            };
            let st = run.st.lock().unwrap();
            let mut deliverables = vec![];
            let mut evidence = vec![];
            let mut risks = vec![];
            let mut acceptance = vec![];
            for t in &st.tasks {
                let unfinished = match t.status {
                    TaskStatus::Pending | TaskStatus::Running => Some("没有执行"),
                    TaskStatus::Blocked => Some("依赖失败，没有执行"),
                    TaskStatus::Failed => Some("失败"),
                    TaskStatus::Cancelled => Some("已取消"),
                    TaskStatus::SkippedByDecision => Some("被主控取消"),
                    _ => None,
                };
                if let Some(state) = unfinished {
                    risks.push(format!("{}：{state}，没有交付结果", t.def.id));
                }
                let r = t.result.clone().unwrap_or_default();
                for a in &t.artifacts {
                    if a["kind"] == "git_diff" {
                        deliverables.push(a.clone());
                    }
                }
                // Every piece of evidence the task reported, not only the first.
                for e in &r.evidence {
                    evidence.push(json!({ "text": format!("{}：{}", t.def.id, e), "seq": t.succeeded_seq }));
                }
                risks.extend(r.risks.iter().map(|x| format!("{}：{}", t.def.id, x)));
                // Who checked this task independently, and how that went. A verifier that did not
                // pass stays visible as a risk of the task it was meant to confirm.
                let verifications: Vec<Verification> = st
                    .tasks
                    .iter()
                    .filter(|v| v.def.verifies.as_deref() == Some(t.def.id.as_str()))
                    .map(|v| {
                        let passed = v.status == TaskStatus::Succeeded;
                        if !passed {
                            risks.push(format!("{}：独立验证 {} 的状态是 {}，结论没有得到确认", t.def.id, v.def.id, v.status.label()));
                        }
                        let vr = v.result.clone().unwrap_or_default();
                        Verification { task: v.def.id.clone(), status: v.status.as_str(), label: v.status.label(), passed, summary: vr.summary, checks: vr.checks, evidence: vr.evidence, risks: vr.risks }
                    })
                    .collect();
                acceptance.push(TaskAcceptance {
                    task: t.def.id.clone(),
                    title: t.def.title.clone(),
                    status: t.status.as_str(),
                    label: t.status.label(),
                    summary: Summary::of(&t.def.accept, &r.checks),
                    checks: r.checks.clone(),
                    verifications,
                    trace_gaps: trace_gaps(&t.def.id),
                    risks: r.risks.clone(),
                });
            }
            if let Some(last) = st.tasks.last()
                && let Some(report) = last.artifacts.iter().rev().find(|a| a["kind"] == "report") {
                    deliverables.push(report.clone());
                }
            // The lead reports how the goal went, in its last message.
            let (text, suggestions) = match &st.lead_done {
                Some(Ok(text)) => split_brief_suggestions(text),
                _ => (String::new(), vec![]),
            };
            json!({
                "summary": format!("{} 改动直接在工作目录 {} 中，Janus 没有提交。", lead_summary(&text), run.cwd.display()),
                "briefSuggestions": suggestions,
                "deliverables": deliverables,
                "evidence": evidence,
                "acceptance": acceptance,
                "usage": usage,
                "risks": risks,
            })
        };
        self.emit(
            run,
            ev("run.delivered", "janus", "主控完成了目标，交付结果已准备好").data(result),
        );
        self.set_status(run, RunStatus::Delivered);
    }
}

/// Where the watchdog keeps a session: its task, or the lead's own entry.
pub(super) fn watch_key<'a>(ctx: &Ctx<'a>) -> &'a str {
    match ctx.task {
        Some(t) => t,
        None if ctx.agent == "lead" => LEAD_WATCH,
        None => "",
    }
}

pub(super) fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    // Symbolic links are skipped: a skill from a cloned repository could point anywhere.
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            if e.file_name() != ".git" {
                copy_dir(&src, &dst)?;
            }
        } else if t.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}
