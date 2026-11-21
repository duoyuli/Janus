//! Creating and driving a run: the scheduler loop, the watchdog and the delivery.

use super::*;

impl App {
    // @project-doc docs/domains/20261031-run_lifecycle.md#create_run
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
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
        let is_git = git(&cwd, &["rev-parse", "--is-inside-work-tree"]).await.is_ok();
        let base_ref = if is_git {
            git(&cwd, &["rev-parse", "HEAD"]).await.unwrap_or_default().trim().to_string()
        } else {
            String::new()
        };
        // Unique even for runs created in the same millisecond (side by side, as the e2e test does).
        let id = crate::agents::new_id("r");
        // A run on a git repository works in its own worktree, so runs on one repository do not
        // write over each other; one without commits yet, or outside git, works in place.
        // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
        let (cwd, worktree) = if is_git && !base_ref.is_empty() {
            // The worktree is of the whole repository; a workspace that is a folder inside it
            // (`apps/client`) has its sessions start in that folder of the worktree (FL-09).
            let top = PathBuf::from(git(&cwd, &["rev-parse", "--show-toplevel"]).await?.trim());
            let prefix = git(&cwd, &["rev-parse", "--show-prefix"]).await.unwrap_or_default().trim().to_string();
            let path = self.worktree_dir(&id);
            let branch = crate::worktrees::current_branch(&top);
            let included = {
                let _adding = self.worktree_adds.lock().unwrap_or_else(|p| p.into_inner());
                crate::worktrees::create(&top, &path, &base_ref, &id)?
            };
            // The path the agents' own tools report (`/private/var/…` for `/var/…` on macOS), so
            // the files they edit resolve against it.
            let root = path.canonicalize().unwrap_or(path);
            let start = match prefix.trim_end_matches('/') {
                "" => root,
                p => root.join(p),
            };
            (start, Some(WorktreeOf { repo: top, branch, included }))
        } else {
            (cwd, None)
        };
        // Nothing made for a run that is never recorded stays behind: a step that fails from here
        // takes back the worktree and the run's own directory.
        let dirs = self.cfg.layout.run(RunKind::Goal, &id);
        let prepared = self.bundle_team(&dirs, &team).and_then(|()| crate::attachments::save(&dirs, &req.images));
        let goal_images = match prepared {
            Ok(images) => images,
            Err(err) => {
                if let Some(w) = &worktree {
                    let kept = self.cfg.layout.deleted_runs();
                    if let Err(e) = crate::worktrees::remove(&w.repo, &self.worktree_root(&id), &kept, &id) {
                        crate::config::log(format!("janus: could not take back the worktree of {id}: {e:#}"));
                    }
                }
                crate::layout::remove(&dirs.data);
                return Err(err);
            }
        };
        let goal = match req.goal.trim() {
            "" => "按附图完成".to_string(),
            g => g.to_string(),
        };
        let token_limit = req.max_tokens.filter(|&n| n > 0).or(self.cfg.run_token_budget).unwrap_or(0);
        let cost_limit_usd = req.max_cost_usd.filter(|n| n.is_finite() && *n > 0.0).or(self.cfg.run_cost_budget_usd);
        let cost_limit_micros = cost_limit_usd.map(|c| (c * 1_000_000.0).round() as i64).unwrap_or(0);
        let mut config =
            json!({ "team": team, "git": is_git, "baseRef": base_ref, "workspace": ws, "goalImages": goal_images });
        if token_limit > 0 {
            config["tokenLimit"] = json!(token_limit);
        }
        if let Some(c) = cost_limit_usd {
            config["costLimitUsd"] = json!(c);
        }
        if let Some(w) = &worktree {
            config["worktree"] = json!({ "repo": w.repo, "branch": w.branch });
        }
        self.db.insert_run(&id, RunKind::Goal, &goal, &cwd.to_string_lossy(), &config, now_ms())?;
        let run = Arc::new(Run::new(RunRecord {
            id: id.clone(),
            kind: RunKind::Goal,
            goal,
            cwd,
            worktree,
            team,
            git: is_git && !base_ref.is_empty(),
            base_ref,
            brief: ws["brief"].as_str().unwrap_or("").trim().to_string(),
            goal_images,
            last_seq: 0,
            status: RunStatus::Planning,
            cost_limit_micros,
        }));
        self.runs.lock().unwrap().insert(id.clone(), run.clone());
        self.launch(run, Phase::Start);
        Ok(id)
    }

    /// What the person is reminded of before a run starts in `workspace`; they may go ahead
    /// anyway. A git repository's run starts from `HEAD` in its own worktree, without the changes
    /// not committed yet; a directory outside git has no worktree, so another run still working
    /// there would edit the same files.
    // @project-doc docs/interfaces/20261031-http_api.md#run_preflight
    pub async fn preflight(&self, workspace: &str) -> Result<Vec<String>> {
        let ws = self.workspace(workspace)?;
        let (cwd, _) = self.team_for(&ws)?;
        if git(&cwd, &["rev-parse", "--verify", "HEAD"]).await.is_ok() {
            let changed = crate::worktrees::uncommitted(&cwd);
            if changed.is_empty() {
                return Ok(vec![]);
            }
            let shown: Vec<&str> = changed.iter().take(5).map(String::as_str).collect();
            let more =
                if changed.len() > shown.len() { format!(" 等 {} 个文件", changed.len()) } else { String::new() };
            return Ok(vec![format!(
                "工作目录里有还没提交的改动（{}{more}）。这些改动不会带进这个任务：任务从最近一次提交开始，在单独的 worktree 中进行。",
                shown.join("、")
            )]);
        }
        Ok(self
            .db
            .active_runs_in(&cwd.to_string_lossy())?
            .iter()
            .map(|r| {
                format!(
                    "这个目录上有一个正在运行的任务「{}」。这个目录不是 Git 仓库，两个任务会直接改同一批文件。",
                    r["goal"].as_str().unwrap_or("").lines().next().unwrap_or("")
                )
            })
            .collect())
    }

    pub(super) fn launch(self: &Arc<Self>, run: Arc<Run>, phase: Phase) {
        run.driving.set(true);
        let app = self.clone();
        tokio::spawn(async move {
            let result = match phase {
                Phase::Start => app.drive(&run).await,
                Phase::Execute => app.execute(&run).await,
                Phase::Goal { resumed } => app.goal(&run, resumed).await,
            };
            run.driving.set(false);
            if let Err(err) = result
                && err.downcast_ref::<Cancelled>().is_none()
                && !run.st.lock().unwrap().cancelled()
            {
                app.emit(
                    &run,
                    ev("run.failed", "janus", format!("任务失败：{err:#}"))
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
            !st.cancelled() && !st.tasks.is_empty()
        };
        if executable && run.driving.claim() {
            self.launch(run.clone(), Phase::Execute);
        }
    }

    /// Where a run's worktree lives (see `crate::worktrees`): its `work` directory.
    pub(super) fn worktree_dir(&self, run_id: &str) -> PathBuf {
        self.cfg.layout.run(RunKind::Goal, run_id).work
    }

    /// The top of a run's worktree as the agents' tools report it. The run's `cwd` is a folder in
    /// it when its workspace is a folder inside the repository; git works on the whole worktree.
    pub(super) fn worktree_root(&self, run_id: &str) -> PathBuf {
        let dir = self.worktree_dir(run_id);
        dir.canonicalize().unwrap_or(dir)
    }

    /// The places of a run or chat (`Layout::run`).
    pub(crate) fn dirs(&self, run: &Run) -> crate::layout::RunDirs {
        self.cfg.layout.run(run.kind, &run.id)
    }

    /// The SKILL.md of the member's skill router in its bundle, when the member has skills.
    pub(super) fn skill_router_md(&self, run: &Run, m: &Member) -> Option<PathBuf> {
        crate::library::skill_router_md(m.kind, &m.resources, &self.dirs(run).bundle(&m.id))
    }

    /// Copy each member's skills into its bundle, so later edits to the library do not change
    /// the run and no agent reads the library itself.
    // @project-doc docs/domains/20261116-resource_library.md#delivery
    pub(super) fn bundle_team(&self, dirs: &crate::layout::RunDirs, team: &[Member]) -> Result<()> {
        let uses = self.db.skill_uses()?.into_iter().map(|(id, (n, _))| (id, n)).collect();
        for m in team {
            crate::library::bundle(&self.store_dir(), m.kind, &m.resources, &uses, &dirs.bundle(&m.id))?;
        }
        Ok(())
    }

    pub(super) async fn drive(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        let workspace_note = match &run.worktree {
            Some(w) => format!(
                "Git 仓库 {} 的独立 worktree，从{} {} 开始",
                w.repo.display(),
                w.branch.as_deref().map(|b| format!("分支 {b} 的")).unwrap_or_default(),
                short_sha(&run.base_ref)
            ),
            None => "非 Git 目录：子任务直接在这个目录中执行，没有 diff".to_string(),
        };
        let mut data = json!({
            "goal": run.goal, "cwd": run.cwd, "git": run.git, "baseRef": run.base_ref, "workspace": workspace_note,
            "images": run.goal_images
        });
        if let Some(w) = &run.worktree {
            data["worktree"] = json!({ "repo": w.repo, "included": w.included, "branch": w.branch });
        }
        self.emit(run, ev("run.created", "user", format!("创建目标：{}", run.goal)).data(data));
        self.goal(run, false).await
    }

    /// Check whether wall-clock or reported USD cost ceiling has been reached. Unreported
    /// usage (`None`) never triggers a pause.
    pub(super) fn check_budget(&self, run: &Run) -> Option<BudgetHit> {
        if now_ms() > run.deadline_ms.load(Ordering::SeqCst) {
            return Some(BudgetHit::WallClock);
        }
        let cost_limit_micros = run.cost_limit_micros.load(Ordering::SeqCst);
        if cost_limit_micros > 0
            && let Some(usd) = run.costs.lock().unwrap().usd
            && (usd * 1_000_000.0).round() as i64 >= cost_limit_micros
        {
            return Some(BudgetHit::CostUsd { used: usd, limit: cost_limit_micros as f64 / 1_000_000.0 });
        }
        None
    }

    /// The scheduling loop. It recomputes the run's status from the task graph on every pass and
    /// only returns once the run is delivered or cancelled, so every later decision still has a driver to act on it.
    // @project-doc docs/domains/20261031-run_lifecycle.md#scheduling
    // @project-doc docs/domains/20261031-run_lifecycle.md#delivery_conditions
    pub(super) async fn execute(self: &Arc<Self>, run: &Arc<Run>) -> Result<()> {
        enum Outcome {
            Deliver,
            /// Goal mode: the lead's session broke off; the person continues the run to restart it.
            LeadFailed(String),
            Budget(BudgetHit),
            Paused,
            Running,
        }
        loop {
            if run.st.lock().unwrap().cancelled() {
                return Err(Cancelled.into());
            }
            let budget_hit = self.check_budget(run);
            let over_budget = budget_hit.is_some();
            let cap = self.cfg.max_concurrent_attempts.max(1);
            let (starts, blocked, outcome) = {
                let mut st = run.st.lock().unwrap();
                let gate = DepGate::of(&st.tasks);
                let succeeded: HashSet<String> =
                    st.tasks.iter().filter(|t| t.status() == TaskStatus::Succeeded).map(|t| t.def.id.clone()).collect();
                let mut blocked = vec![];
                for t in st.tasks.iter_mut().filter(|t| t.status() == TaskStatus::Pending) {
                    if let Some((dep, vd)) = gate.holds(t) {
                        t.set_phase(TaskPhase::Blocked { by: dep.clone() });
                        blocked.push((t.def.id.clone(), dep, vd));
                    }
                }
                // A blocked task waits again once none of its dependencies is failed any more (the
                // lead retried or accepted them); one still failed keeps it blocked, and is named.
                for t in st.tasks.iter_mut().filter(|t| t.status() == TaskStatus::Blocked) {
                    match gate.holds(t) {
                        None => t.set_phase(TaskPhase::Pending),
                        Some((dep, vd)) if t.blocked_by() != Some(dep.as_str()) => {
                            // Still blocked, now on another failed dependency.
                            t.restore(TaskPhase::Blocked { by: dep.clone() });
                            blocked.push((t.def.id.clone(), dep, vd));
                        }
                        Some(_) => {}
                    }
                }
                let mut starts = vec![];
                if !st.paused() && !over_budget {
                    let mut running_count = st.tasks.iter().filter(|t| t.status() == TaskStatus::Running).count();
                    let mut busy: HashSet<String> = st
                        .tasks
                        .iter()
                        .filter(|t| t.status() == TaskStatus::Running)
                        .map(|t| t.agent.clone())
                        .collect();
                    for t in st.tasks.iter_mut().filter(|t| t.status() == TaskStatus::Pending) {
                        if running_count >= cap {
                            break;
                        }
                        if t.def.deps.iter().all(|d| succeeded.contains(d)) && !busy.contains(&t.agent) {
                            busy.insert(t.agent.clone());
                            running_count += 1;
                            t.attempts += 1;
                            let attempt_id = format!("{}-{}-{}-{}", run.id, t.def.id, t.attempts, now_ms());
                            t.set_phase(TaskPhase::Running { attempt: attempt_id.clone() });
                            starts.push((t.def.id.clone(), t.attempts, t.agent.clone(), attempt_id));
                        }
                    }
                }
                let active = st.tasks.iter().any(|t| t.status() == TaskStatus::Running);
                // A task that could start but for a pause, concurrency cap or budget is still work
                // to do: the run waits instead of delivering without it.
                let runnable = st
                    .tasks
                    .iter()
                    .any(|t| t.status() == TaskStatus::Pending && t.def.deps.iter().all(|d| succeeded.contains(d)));
                let finished = !active && !runnable;
                let all_terminal = st.tasks.iter().all(|t| t.status().terminal());
                // The lead decides what a failure means (it reads it on the board), so nothing
                // waits for the person; the run ends once the lead is done and no task is running
                // or can still start.
                let outcome = if let (true, LeadGoal::Failed(err)) = (finished, &st.lead.goal) {
                    Outcome::LeadFailed(err.clone())
                } else if finished && st.lead.goal.ended() {
                    Outcome::Deliver
                } else if let (Some(hit), false) = (budget_hit, all_terminal) {
                    Outcome::Budget(hit)
                } else if !active && starts.is_empty() && st.paused() {
                    Outcome::Paused
                } else {
                    Outcome::Running
                };
                (starts, blocked, outcome)
            };
            for (id, dep, vd) in blocked {
                let mut data = json!({ "dependency": dep, "kind": "hard" });
                let text = match vd {
                    Some(vd) => {
                        data["verdict"] = json!(vd);
                        format!("{id} 被独立验证 {dep} 的判定「{}」阻塞", vd.label())
                    }
                    None => format!("{id} 被失败的硬依赖 {dep} 阻塞"),
                };
                self.emit(run, ev("task.blocked", "janus", text).task(&id).data(data));
            }
            for (id, n, agent, attempt_id) in starts {
                if n == 1 {
                    self.emit(
                        run,
                        ev("task.ready", "janus", format!("{id} 的依赖已满足，进入 ready"))
                            .task(&id)
                            .data(json!({ "tasks": [id] })),
                    );
                    self.emit(
                        run,
                        ev(
                            "task.assigned",
                            "janus",
                            format!("{id} 分配给 {}", run.member(&agent).map(|m| m.name.as_str()).unwrap_or(&agent)),
                        )
                        .task(&id)
                        .to(&agent)
                        .data(json!({ "agent": agent })),
                    );
                }
                self.emit(
                    run,
                    ev("attempt.launch_requested", "janus", format!("准备启动 {id} 第 {n} 次尝试"))
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
                    bail!("负责人的会话异常结束：{err}");
                }
                Outcome::Budget(hit) => {
                    // Announced each time the run comes to wait on a budget ceiling, including when
                    // the lead adds work after the run went back to running.
                    if run.st.lock().unwrap().status != RunStatus::WaitingInput {
                        self.set_status(run, RunStatus::WaitingInput);
                        let (text, data) = match hit {
                            BudgetHit::WallClock => (
                                "任务达到墙钟预算，等待你追加预算或停止".to_string(),
                                json!({ "kind": "wall_clock", "budgetMs": RUN_BUDGET_MS }),
                            ),
                            BudgetHit::CostUsd { used, limit } => (
                                format!("任务已上报费用 ${used:.2}（达到上限 ${limit:.2}），等待你追加预算或停止"),
                                json!({ "kind": "cost_usd", "usedCostUsd": used, "costLimitUsd": limit }),
                            ),
                        };
                        self.emit(run, ev("run.budget_exhausted", "janus", text).data(data));
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
    // @project-doc docs/domains/20261031-run_lifecycle.md#watchdog
    pub(super) fn watchdog(&self, run: &Run) {
        let now = now_ms();
        let limit = self.cfg.stall_after.as_millis() as i64;
        let due: Vec<(SessionKey, i64, i64)> = run
            .watch
            .lock()
            .unwrap()
            .iter_mut()
            .filter_map(|(session, w)| {
                if w.human || w.waits > 0 {
                    // Waiting on a person is not the agent's silence; the clock starts once they are done.
                    w.last_ms = now;
                    return None;
                }
                if w.idle_noticed || now - w.last_ms < limit {
                    return None;
                }
                w.idle_noticed = true;
                Some((session.clone(), w.attempt, now - w.last_ms))
            })
            .collect();
        for (session, n, idle) in due {
            let SessionKey::Task(task) = session else {
                self.emit(
                    run,
                    ev("lead.stalled", "janus", format!("负责人已有 {} 没有任何动静", span(idle)))
                        .to("user")
                        .data(json!({ "reason": "idle", "idleMs": idle })),
                );
                continue;
            };
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

    /// Record a finished tool call (its ACP record, see `call_fingerprint`) and detect period-1
    /// (`AAAAA`), period-2 (`ABABABAB`) and period-3 (`ABCABCABC`) loops, alerting the lead (or
    /// the person, for the lead itself).
    // @project-doc docs/domains/20261031-run_lifecycle.md#watchdog
    pub(super) fn watch_call(&self, run: &Run, session: &SessionKey, entry: &Value) {
        let call = call_fingerprint(entry);
        let title = entry["title"].as_str().unwrap_or("工具调用");
        let hit = {
            let mut watch = run.watch.lock().unwrap();
            let Some(w) = watch.get_mut(session) else { return };
            w.calls.record(call, title).map(|(p, r, pat)| (w.attempt, p, r, pat))
        };
        let Some((n, period, repeats, pattern)) = hit else { return };
        let text_body = if period == 1 {
            format!("连续 {repeats} 次做同样的调用：{pattern}")
        } else {
            format!("连续 {repeats} 轮重复调用序列：{pattern}")
        };
        let SessionKey::Task(task) = session else {
            self.emit(
                run,
                ev("lead.stalled", "janus", format!("负责人{text_body}"))
                    .to("user")
                    .data(json!({ "reason": "loop", "tool": pattern, "repeats": repeats, "period": period })),
            );
            return;
        };
        self.emit(
            run,
            ev("task.stalled", "janus", format!("{task} {text_body}"))
                .task(task)
                .attempt(n)
                .to("lead")
                .data(json!({ "reason": "loop", "tool": pattern, "repeats": repeats, "period": period })),
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
                let unfinished = match t.status() {
                    TaskStatus::Pending | TaskStatus::Running => Some("没有执行"),
                    TaskStatus::Blocked => Some("依赖失败，没有执行"),
                    TaskStatus::Failed => Some("失败"),
                    TaskStatus::Cancelled => Some("已取消"),
                    TaskStatus::SkippedByDecision => Some("被负责人取消"),
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
                    evidence.push(json!({ "text": format!("{}：{}", t.def.id, e), "seq": t.last_success_seq }));
                }
                risks.extend(r.risks.iter().map(|x| format!("{}：{}", t.def.id, x)));
                // Who checked this task independently, and how that went. Completing a review
                // (`Succeeded`) and confirming the work (`Verdict::Approved`) are distinct: a
                // verifier that rejects the work has `passed == false`. Every verifier that did not
                // confirm the task leaves a risk on the delivery: independent verifiers check
                // different things, so one's approval does not cancel another's rejection.
                let verifications: Vec<Verification> = st
                    .tasks
                    .iter()
                    .filter(|v| v.def.verifies.as_deref() == Some(t.def.id.as_str()))
                    .map(|v| {
                        let vr = v.result.clone().unwrap_or_default();
                        let verdict = v.verdict();
                        Verification {
                            task: v.def.id.clone(),
                            status: v.status().as_str(),
                            label: v.status().label(),
                            verdict,
                            passed: verdict == Some(Verdict::Approved),
                            summary: vr.summary,
                            checks: vr.checks,
                            evidence: vr.evidence,
                            risks: vr.risks,
                        }
                    })
                    .collect();
                for v in verifications.iter().filter(|v| !v.passed) {
                    let why = match v.verdict {
                        Some(vd) => format!("判定为「{}」", vd.label()),
                        None => format!("状态是 {}", v.label),
                    };
                    risks.push(format!("{}：独立验证 {} {why}，结论没有得到确认", t.def.id, v.task));
                }
                let gaps = if t.trace_gaps.is_empty() { trace_gaps(&t.def.id) } else { t.trace_gaps.clone() };
                // A finished task that changed files and that no verification checks: the lead may
                // have checked it itself, which its summary has to say.
                // @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode
                let changed =
                    !st.traces.files(&t.def.id).is_empty() || t.artifacts.iter().any(|a| a["kind"] == "git_diff");
                let unverified = t.def.verifies.is_none()
                    && t.status() == TaskStatus::Succeeded
                    && changed
                    && verifications.is_empty();
                acceptance.push(TaskAcceptance {
                    task: t.def.id.clone(),
                    title: t.def.title.clone(),
                    status: t.status().as_str(),
                    label: t.status().label(),
                    summary: Summary::of(&t.def.accept, &r.checks),
                    checks: r.checks.clone(),
                    verifications,
                    unverified,
                    trace_gaps: gaps,
                    risks: r.risks.clone(),
                });
            }
            let lead_edits = st.traces.lead_after_plan();
            if !lead_edits.is_empty() {
                risks.push(format!(
                    "负责人在派出子任务后自己改了文件：{}。这些改动没有经过子任务的验收和独立验证",
                    lead_edits.iter().cloned().collect::<Vec<_>>().join("、")
                ));
            }
            let lead_tools = st.traces.lead_tools_after_plan();
            if !lead_tools.is_empty() {
                risks.push(format!(
                    "负责人在派出子任务后自己调用了资源库工具：{}。它们做的事没有经过子任务的验收和独立验证",
                    lead_tools.iter().cloned().collect::<Vec<_>>().join("、")
                ));
            }
            if let Some(last) = st.tasks.last()
                && let Some(report) = last.artifacts.iter().rev().find(|a| a["kind"] == "report")
            {
                deliverables.push(report.clone());
            }
            // The lead reports how the goal went, in its last message.
            let (text, suggestions) = match &st.lead.goal {
                LeadGoal::Done(text) => split_brief_suggestions(text),
                _ => (String::new(), vec![]),
            };
            json!({
                "summary": match &run.worktree {
                    Some(w) => format!(
                        "{} 改动在这个任务单独的 worktree {} 中，还没有提交；验收时 Janus 把它们提交并合并回 {}。",
                        lead_summary(&text),
                        run.cwd.display(),
                        w.source()
                    ),
                    None => format!("{} 改动直接在工作目录 {} 中，Janus 没有提交。", lead_summary(&text), run.cwd.display()),
                },
                "briefSuggestions": suggestions,
                "deliverables": deliverables,
                "evidence": evidence,
                "acceptance": acceptance,
                "usage": usage,
                "risks": risks,
            })
        };
        self.emit(run, ev("run.delivered", "janus", "负责人完成了目标，交付结果已准备好").data(result));
        self.set_status(run, RunStatus::Delivered);
    }

    /// Delete a run and everything Janus kept for it: its rows in SQLite, its places in Janus home
    /// (`RunDirs`: what it recorded, the folder it worked in, its logs) and the transcripts its
    /// sessions left in the agents' own configuration directories. A run that is still working is refused, because its sessions and attempts would keep writing;
    /// stop it first. A chat has only its own session, which is stopped here. The workspace's own
    /// files are not touched.
    // @project-doc docs/interfaces/20261031-http_api.md#run_delete
    pub async fn delete_run(self: &Arc<Self>, run_id: &str) -> Result<(), Refused> {
        let _turn = self.revival.lock().await;
        let row = self.db.run(run_id)?;
        let Some(row) = row else { return Err(Refused::NotFound) };
        let status = RunStatus::parse(row["status"].as_str().unwrap_or(""));
        let kind = RunKind::parse(row["kind"].as_str().unwrap_or("")).unwrap_or_default();
        if kind == RunKind::Goal && status.is_some_and(RunStatus::is_active) {
            return Err(Refused::Conflict("任务还在进行，先停止任务再删除".into()));
        }
        // A delivered run keeps its lead conversation open for more work; take the run out of the
        // live map first, then stop those sessions before the rows they could still write to are gone.
        let live = self.runs.lock().unwrap().remove(run_id);
        if let Some(run) = live {
            *run.emitting.lock().unwrap_or_else(|p| p.into_inner()) = true;
            let conns: Vec<Arc<AcpConn>> = {
                let mut st = run.st.lock().unwrap();
                st.pending.clear();
                st.live.clear();
                st.conns.drain().map(|(_, c)| c).collect()
            };
            for c in conns {
                c.kill().await;
            }
        }
        // The run's worktree goes too; changes never committed are kept as a patch first.
        if let Some(repo) = row["config"]["worktree"]["repo"].as_str() {
            let kept = self.cfg.layout.deleted_runs();
            match crate::worktrees::remove(Path::new(repo), &self.worktree_root(run_id), &kept, run_id) {
                Ok(Some(patch)) => crate::config::log(format!(
                    "janus: run {run_id} deleted; its uncommitted changes are kept in {}",
                    patch.display()
                )),
                Ok(None) => {}
                Err(err) => return Err(Refused::Invalid(err)),
            }
        }
        // The files go before the rows: the row is what lets a second request find the run, so a
        // file that could not be removed is removed by trying again.
        let agents: HashSet<&str> =
            row["config"]["team"].as_array().into_iter().flatten().filter_map(|m| m["runtime"].as_str()).collect();
        let sessions = self.db.run_sessions(run_id)?;
        for agent in agents {
            self.forget_sessions(agent, &sessions)?;
        }
        let dirs = self.cfg.layout.run(kind, run_id);
        for path in [&dirs.data, &dirs.work, &dirs.logs] {
            if let Err(err) = std::fs::remove_dir_all(path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                return Err(Refused::Invalid(anyhow!("删除 {} 失败：{err}", path.display())));
            }
        }
        Ok(self.db.delete_run(run_id)?)
    }
}

/// What makes two finished tool calls the same call for loop detection: the tool, its whole
/// input and what came back (output, content such as an edit's diff, status). Editing one file
/// again with different text, or a test run whose output changed, is progress, not a repeat.
/// Durations and times of day in what came back do not count (`steady`): the same failing test
/// run again reports a new duration every time and is still the same call. Only the hash is kept.
// @project-doc docs/domains/20261031-run_lifecycle.md#watchdog
pub(super) fn call_fingerprint(entry: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for key in ["kind", "title", "rawInput", "status"] {
        entry[key].to_string().hash(&mut h);
    }
    for key in ["rawOutput", "content"] {
        steady(&entry[key]).to_string().hash(&mut h);
    }
    h.finish()
}

/// A tool result with every string's durations and times of day taken out (`steady_text`).
fn steady(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(steady_text(s)),
        Value::Array(items) => Value::Array(items.iter().map(steady).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), steady(v))).collect()),
        other => other.clone(),
    }
}

/// Text with what changes on every run of the same command replaced: a number followed by a
/// time unit (`0.23s`, `120 ms`) becomes `#s` / `# ms`, a time of day (`12:34:56.789`) becomes
/// `#:#`. Other numbers, such as how many tests failed, stay.
fn steady_text(text: &str) -> String {
    const UNITS: [&str; 15] =
        ["ns", "us", "µs", "ms", "s", "sec", "secs", "second", "seconds", "m", "min", "mins", "minute", "minutes", "h"];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let starts_number =
            chars[i].is_ascii_digit() && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'));
        if !starts_number {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len()
            && (chars[i].is_ascii_digit()
                || (matches!(chars[i], '.' | ':') && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
        {
            i += 1;
        }
        if chars[start..i].contains(&':') {
            // After a colon it is a position, `parser.rs:42:15`, not a time of day (AT-13).
            if start > 0 && chars[start - 1] == ':' {
                out.extend(&chars[start..i]);
            } else {
                out.push_str("#:#");
            }
            continue;
        }
        let gap = usize::from(chars.get(i) == Some(&' '));
        let unit_end = (i + gap..chars.len()).find(|&k| !chars[k].is_alphabetic()).unwrap_or(chars.len());
        let unit: String = chars[i + gap..unit_end].iter().collect();
        if UNITS.contains(&unit.as_str()) {
            out.push('#');
            out.extend(&chars[i..unit_end]);
            i = unit_end;
        } else {
            out.extend(&chars[start..i]);
        }
    }
    out
}

/// Recent finished tool calls of one session, for loop detection: a ring buffer of the last
/// `WINDOW` calls `(fingerprint, short_title)` for multi-step cycles, and the length of the
/// current run of identical calls, which keeps counting past the window.
#[derive(Default)]
pub(super) struct CallLoop {
    recent: VecDeque<(u64, String)>,
    /// Calls remaining before another multi-step loop report may fire.
    cooldown: u32,
    /// How many times in a row the latest call was made.
    streak: u32,
}

impl CallLoop {
    const WINDOW: usize = 12;

    /// Record a call and detect period-1 (`AAAAA`, reported at every 5th repeat), period-2
    /// (`ABABABAB`, 4 cycles = 8 calls) or period-3 (`ABCABCABC`, 3 cycles = 9 calls) loops.
    pub(super) fn record(&mut self, call: u64, title: &str) -> Option<(u32, u32, String)> {
        let recent = &mut self.recent;
        self.streak = if recent.back().is_some_and(|(h, _)| *h == call) { self.streak + 1 } else { 1 };
        recent.push_back((call, short(title, 36)));
        if recent.len() > Self::WINDOW {
            recent.pop_front();
        }
        let same = self.streak;
        if same >= LOOP_REPEAT && same.is_multiple_of(LOOP_REPEAT) {
            self.cooldown = 0;
            return Some((1, same, short(title, 60)));
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return None;
        }
        if recent.len() >= 8 {
            let s = recent.len() - 8;
            let (h0, t0) = &recent[s];
            let (h1, t1) = &recent[s + 1];
            if h0 != h1 && (0..8).all(|i| recent[s + i].0 == if i % 2 == 0 { *h0 } else { *h1 }) {
                self.cooldown = 7;
                return Some((2, 4, format!("{t0} → {t1}")));
            }
        }
        if recent.len() >= 9 {
            let s = recent.len() - 9;
            let (h0, t0) = &recent[s];
            let (h1, t1) = &recent[s + 1];
            let (h2, t2) = &recent[s + 2];
            if !(h0 == h1 && h1 == h2) && (0..9).all(|i| recent[s + i].0 == recent[s + (i % 3)].0) {
                self.cooldown = 8;
                return Some((3, 3, format!("{t0} → {t1} → {t2}")));
            }
        }
        None
    }
}

#[cfg(test)]
mod loop_tests {
    use super::*;

    #[test]
    fn loop_detector_catches_single_and_alternating_cycles() {
        let mut calls = CallLoop::default();
        for i in 1..=4 {
            assert!(calls.record(10, "cat x").is_none(), "step {i}");
        }
        assert_eq!(calls.record(10, "cat x"), Some((1, 5, "cat x".into())));

        let mut calls = CallLoop::default();
        for i in 0..7 {
            let (h, t) = if i % 2 == 0 { (1, "Edit foo.rs") } else { (2, "cargo test") };
            assert!(calls.record(h, t).is_none());
        }
        assert_eq!(calls.record(2, "cargo test"), Some((2, 4, "Edit foo.rs → cargo test".into())));
        // Cooldown prevents firing again on the very next step.
        assert!(calls.record(1, "Edit foo.rs").is_none());

        let mut calls = CallLoop::default();
        let steps = [(1, "read"), (2, "edit"), (3, "test")];
        for i in 0..8 {
            let (h, t) = steps[i % 3];
            assert!(calls.record(h, t).is_none());
        }
        assert_eq!(calls.record(3, "test"), Some((3, 3, "read → edit → test".into())));
    }

    fn edit(text: &str) -> Value {
        json!({
            "kind": "edit", "title": "Edit api.rs", "status": "completed",
            "rawInput": { "file_path": "api.rs", "old_string": "x", "new_string": text },
            "content": [{ "type": "diff", "path": "api.rs", "oldText": "x", "newText": text }]
        })
    }

    fn test_run(output: &str) -> Value {
        json!({
            "kind": "execute", "title": "cargo test", "status": "completed",
            "rawInput": { "command": ["cargo", "test"] }, "rawOutput": output
        })
    }

    fn reports(calls: &[Value]) -> Vec<(u32, u32, String)> {
        let mut seen = CallLoop::default();
        calls.iter().filter_map(|c| seen.record(call_fingerprint(c), c["title"].as_str().unwrap())).collect()
    }

    /// Editing one file over and over with different text is progress, not a loop.
    #[test]
    fn edits_of_one_file_with_different_text_are_not_a_loop() {
        let edits: Vec<Value> = (0..10).map(|i| edit(&format!("fn v{i}() {{}}"))).collect();
        assert!(reports(&edits).is_empty());
    }

    /// Edit and test in turn while the test output changes is progress, not an `ABAB` loop.
    #[test]
    fn edit_and_test_in_turn_with_changing_output_is_not_a_loop() {
        let calls: Vec<Value> = (0..12)
            .map(|i| if i % 2 == 0 { edit(&format!("v{i}")) } else { test_run(&format!("{i} failed")) })
            .collect();
        assert!(reports(&calls).is_empty());
    }

    /// The very same call with the very same result is still a loop, alone or in alternation.
    #[test]
    fn identical_calls_and_results_are_still_a_loop() {
        let same: Vec<Value> = (0..5).map(|_| edit("fn a() {}")).collect();
        assert_eq!(reports(&same), vec![(1, 5, "Edit api.rs".into())]);
        let calls: Vec<Value> = (0..8).map(|i| if i % 2 == 0 { edit("v") } else { test_run("1 failed") }).collect();
        assert_eq!(reports(&calls), vec![(2, 4, "Edit api.rs → cargo test".into())]);
    }

    /// The same failing test run again, with a new duration and timestamp each time, is a loop;
    /// a different number of failures is not.
    #[test]
    fn durations_and_times_in_the_output_do_not_make_a_new_call() {
        let runs: Vec<Value> = (0..5)
            .map(|i| {
                test_run(&format!("[10:0{i}:1{i}.5{i}] test a ... FAILED\n1 failed; finished in 0.{i}3s ({i}00 ms)"))
            })
            .collect();
        assert_eq!(reports(&runs), vec![(1, 5, "cargo test".into())]);
        assert_eq!(
            steady_text("2 failed in 1.25s, 300 ms, at 09:15:02, 3 runs, v12 at 5m"),
            "2 failed in #s, # ms, at #:#, 3 runs, v12 at #m"
        );
        let progress: Vec<Value> = (0..5).map(|i| test_run(&format!("{} failed; finished in 0.2s", 5 - i))).collect();
        assert!(reports(&progress).is_empty());
        // A compiler error moving through the file is progress, not a loop (AT-13).
        assert_eq!(steady_text("error at src/parser.rs:42:15"), "error at src/parser.rs:42:15");
        let fixing: Vec<Value> = (0..5)
            .map(|i| test_run(&format!("error[E0308]: mismatched types\n --> src/parser.rs:{}:15", 40 + i)))
            .collect();
        assert!(reports(&fixing).is_empty());
    }

    #[test]
    fn a_repeated_call_is_reported_every_fifth_time_past_the_window() {
        let mut calls = CallLoop::default();
        let reports: Vec<u32> = (0..50).filter_map(|_| calls.record(10, "cat x")).map(|(_, n, _)| n).collect();
        assert_eq!(reports, vec![5, 10, 15, 20, 25, 30, 35, 40, 45, 50]);
        // A different call ends the streak; the count starts over.
        assert!(calls.record(11, "ls").is_none());
        let again: Vec<u32> = (0..5).filter_map(|_| calls.record(10, "cat x")).map(|(_, n, _)| n).collect();
        assert_eq!(again, vec![5]);
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    fn finished_app(name: &str) -> (Arc<App>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("janus-delete-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = test_app(&dir);
        app.db.insert_run("r1", RunKind::Goal, "g", "/tmp", &json!({}), 1).unwrap();
        app.db.set_run_status("r1", RunStatus::Cancelled).unwrap();
        let event = |seq, kind: &str| Event {
            run_id: "r1".into(),
            seq,
            ts: 1,
            kind: kind.into(),
            actor: "user".into(),
            to: None,
            task: None,
            attempt: None,
            text: kind.into(),
            data: None,
        };
        app.db.insert_event(&event(1, "run.created")).unwrap();
        app.db.insert_event(&event(2, "run.cancelled")).unwrap();
        let dirs = app.cfg.layout.run(RunKind::Goal, "r1");
        for path in [dirs.artifacts("T1"), dirs.bundle("lead"), dirs.attachments(), dirs.logs.clone()] {
            std::fs::create_dir_all(path).unwrap();
        }
        (app, dir)
    }

    /// A run that can still write is refused; nothing is removed.
    #[tokio::test]
    async fn a_run_that_is_still_working_cannot_be_deleted() {
        let (app, dir) = finished_app("active");
        app.db.set_run_status("r1", RunStatus::Running).unwrap();
        assert!(matches!(app.delete_run("r1").await, Err(Refused::Conflict(_))));
        assert!(app.db.run("r1").unwrap().is_some());
        assert_eq!(app.db.events("r1", 0).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A finished run takes its rows and every directory Janus kept for it; a second request
    /// finds nothing to delete.
    #[tokio::test]
    async fn deleting_a_finished_run_removes_its_rows_and_directories() {
        let (app, dir) = finished_app("finished");
        app.delete_run("r1").await.unwrap();
        assert!(app.db.run("r1").unwrap().is_none());
        assert!(app.db.events("r1", 0).unwrap().is_empty());
        let dirs = app.cfg.layout.run(RunKind::Goal, "r1");
        for path in [&dirs.data, &dirs.work, &dirs.logs] {
            assert!(!path.exists(), "{} should be gone", path.display());
        }
        assert!(matches!(app.delete_run("r1").await, Err(Refused::NotFound)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A directory that cannot be removed keeps the run, so the same request removes the rest
    /// once the file system lets it.
    #[tokio::test]
    async fn a_delete_that_could_not_remove_a_directory_can_be_repeated() {
        use std::os::unix::fs::PermissionsExt;
        let (app, dir) = finished_app("retry");
        let locked = app.cfg.layout.run(RunKind::Goal, "r1").log("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("lead.log"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();

        assert!(matches!(app.delete_run("r1").await, Err(Refused::Invalid(_))));
        assert!(app.db.run("r1").unwrap().is_some(), "the run is still there to delete again");

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        app.delete_run("r1").await.unwrap();
        assert!(app.db.run("r1").unwrap().is_none());
        let dirs = app.cfg.layout.run(RunKind::Goal, "r1");
        for path in [&dirs.data, &dirs.work, &dirs.logs] {
            assert!(!path.exists(), "{} should be gone", path.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
