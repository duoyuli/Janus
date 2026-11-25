//! The dispatch queue: every goal run (one the person writes on a workspace's home, one a Loop
//! dispatches) waits here until fewer than `max_running_runs` goal runs are working. A queued run
//! is only its record and its `run.queued` event; its team, library, base commit and worktree are
//! taken when it starts. The queue is the queued rows in creation order, so it survives a restart.
// @project-doc docs/domains/20261031-run_lifecycle.md#dispatch_queue

use super::*;

/// Who asked for a goal run (`runs.origin` and what goes with it).
pub(crate) enum Origin {
    /// The person, on a workspace's home.
    User,
    /// A Loop's `dispatch`: which Loop, its key for what the run is about, where that came from.
    Loop { id: String, key: String, source: Option<String> },
}

impl Origin {
    fn row(&self) -> crate::db::OriginRow<'_> {
        match self {
            Origin::User => crate::db::OriginRow { origin: RunOrigin::User, loop_id: None, key: None, source: None },
            Origin::Loop { id, key, source } => crate::db::OriginRow {
                origin: RunOrigin::Loop,
                loop_id: Some(id),
                key: Some(key),
                source: source.as_deref(),
            },
        }
    }

    /// Who the queue entry is from in the log.
    fn actor(&self) -> &'static str {
        match self {
            Origin::User => "user",
            Origin::Loop { .. } => "lead",
        }
    }

    /// The origin as `run.queued` records it.
    fn view(&self) -> Value {
        match self {
            Origin::User => json!({ "kind": RunOrigin::User }),
            Origin::Loop { id, key, source } => {
                json!({ "kind": RunOrigin::Loop, "loop": id, "key": key, "sourceUrl": source })
            }
        }
    }
}

impl App {
    /// Put the person's goal on the queue of its workspace.
    // @project-doc docs/domains/20261031-run_lifecycle.md#create_run
    pub async fn create_run(self: &Arc<Self>, req: NewRun) -> Result<String> {
        if req.goal.trim().is_empty() && req.images.is_empty() {
            bail!("目标不能为空");
        }
        let goal = match req.goal.trim() {
            "" => "按附图完成",
            g => g,
        };
        self.enqueue(&req.workspace, goal, &req.images, req.max_tokens, Origin::User).await
    }

    /// Put a goal on the queue of `workspace`. What can be checked now is checked now (the
    /// workspace, its team, their agents), so whoever asked hears at once; the run starts when a
    /// slot is free, which with a free slot is right away.
    // @project-doc docs/domains/20261031-run_lifecycle.md#create_run
    pub(super) async fn enqueue(
        self: &Arc<Self>,
        workspace: &str,
        goal: &str,
        images: &[Value],
        max_tokens: Option<i64>,
        origin: Origin,
    ) -> Result<String> {
        let ws = self.workspace(workspace)?;
        let (cwd, _) = self.startable_team(&ws)?;
        // Unique even for runs created in the same millisecond (side by side, as the e2e test does).
        let id = crate::agents::new_id("r");
        let dirs = self.cfg.layout.run(RunKind::Goal, &id);
        let goal_images = crate::attachments::save(&dirs, images).inspect_err(|_| {
            crate::layout::remove(&dirs.data);
        })?;
        let token_limit = max_tokens.filter(|&n| n > 0).or(self.cfg.run_token_budget).unwrap_or(0);
        let mut config = json!({ "workspace": ws, "goalImages": goal_images });
        if token_limit > 0 {
            config["tokenLimit"] = json!(token_limit);
        }
        let first = ev(EventKind::RunQueued, origin.actor(), format!("排队：{goal}"))
            .data(json!({
                "goal": goal, "workspace": { "id": ws.id, "name": ws.name }, "images": goal_images,
                "origin": origin.view(),
            }))
            .into_event(&id, 1);
        // Until this request has had its own turn at the slots, no other pump hands this run out,
        // so with a free slot it is set up before whoever asked is answered.
        self.fresh.lock().unwrap().insert(id.clone());
        let queued = self.db.insert_queued_run(goal, &cwd.to_string_lossy(), &config, &origin.row(), &first);
        if let Err(err) = queued {
            self.fresh.lock().unwrap().remove(&id);
            crate::layout::remove(&dirs.data);
            return Err(err);
        }
        let _ = self.tx.send(first);
        self.run_changed(&id, RunStatus::Queued);
        let mine = self.take_slots(Some(&id)).into_iter().fold(false, |mine, other| {
            if other == id {
                return true;
            }
            self.spawn_start(other);
            mine
        });
        self.fresh.lock().unwrap().remove(&id);
        if mine {
            self.start_queued(&id).await;
        } else {
            // A slot freed while it was held back goes to it (or an older run) now.
            self.pump();
        }
        Ok(id)
    }

    /// The workspace's directory and team, when a run could start there now: the directory exists
    /// and every member's agent can run.
    fn startable_team(&self, ws: &Workspace) -> Result<(PathBuf, Vec<Member>)> {
        let (cwd, team) = self.team_for(ws)?;
        if !cwd.is_dir() {
            bail!("工作目录不存在：{}", cwd.display());
        }
        for m in &team {
            self.spec_for(&m.connection)?;
        }
        Ok((cwd, team))
    }

    /// Hand free slots to the queued runs that have waited longest. Called whenever a run's status
    /// changes and a run is paused or resumed: only planning and running goal runs that are not
    /// paused hold a slot.
    pub(crate) fn pump(&self) {
        for id in self.take_slots(None) {
            self.spawn_start(id);
        }
    }

    /// The queued runs that get the free slots, oldest first, each taken off the queue (`planning`,
    /// `run.starting`) so no other pump hands it out again. A run still being queued by its own
    /// request is left to that request (`own`). Nothing is handed out when the slots cannot be
    /// counted.
    fn take_slots(&self, own: Option<&str>) -> Vec<String> {
        let _queue = self.gates.queue.lock().unwrap_or_else(|p| p.into_inner());
        let counted = self.db.queued_runs().and_then(|q| Ok((q, self.db.working_goal_runs()?)));
        let (queued, working) = match counted {
            Ok(c) => c,
            Err(err) => {
                crate::config::log(format!("janus: could not read the dispatch queue: {err:#}"));
                return vec![];
            }
        };
        if queued.is_empty() {
            return vec![];
        }
        // Runs continuing after a restart hold their slots while they are set up again.
        let continuing = self.continuing.lock().unwrap().clone();
        let working = working
            .iter()
            .filter(|id| !continuing.contains(*id) && !self.run(id).is_some_and(|r| r.st.lock().unwrap().paused()))
            .count()
            + continuing.len();
        let mut free = self.cfg.max_running_runs.max(1).saturating_sub(working);
        let fresh = self.fresh.lock().unwrap().clone();
        let mut starting = vec![];
        for id in queued {
            if free == 0 {
                break;
            }
            if fresh.contains(&id) && own != Some(id.as_str()) {
                continue;
            }
            match self.claim(&id, RunStatus::Planning, ev(EventKind::RunStarting, "janus", "有了空位，开始准备"))
            {
                Ok(true) => {
                    free -= 1;
                    starting.push(id);
                }
                Ok(false) => {}
                Err(err) => crate::config::log(format!("janus: could not take {id} off the queue: {err:#}")),
            }
        }
        starting
    }

    /// Take queued run `id` off the queue into `to`, recording `e` with it: `false` when it was no
    /// longer queued. The event and the new status reach the app.
    fn claim(&self, id: &str, to: RunStatus, e: Ev) -> Result<bool> {
        let Some(event) = self.db.leave_queue(id, to, e.into_event(id, 0))? else { return Ok(false) };
        let _ = self.tx.send(event);
        self.run_changed(id, to);
        // A queued run cancelled before it started also ends a Loop's dispatch.
        self.dispatch_ended(id, to);
        Ok(true)
    }

    /// Start a run that left the queue, in the background.
    fn spawn_start(&self, id: String) {
        let Some(app) = self.me.upgrade() else { return };
        tokio::spawn(async move { app.start_queued(&id).await });
    }

    /// Start queued run `id` now, past the slots: the person decided it should not wait.
    pub(super) fn start_now(&self, id: &str) -> Result<(), Refused> {
        let _queue = self.gates.queue.lock().unwrap_or_else(|p| p.into_inner());
        if !self.claim(id, RunStatus::Planning, ev(EventKind::RunStarting, "user", "立即开始，不等空位"))? {
            return Err(Refused::Conflict("这个任务已经不在排队了".into()));
        }
        self.spawn_start(id.to_string());
        Ok(())
    }

    /// Cancel queued run `id`: it never starts.
    pub(super) fn cancel_queued(&self, id: &str) -> Result<(), Refused> {
        let _queue = self.gates.queue.lock().unwrap_or_else(|p| p.into_inner());
        if !self.claim(id, RunStatus::Cancelled, ev(EventKind::RunCancelled, "user", "取消这个排队中的任务"))?
        {
            return Err(Refused::Conflict("这个任务已经不在排队了".into()));
        }
        Ok(())
    }

    /// Take a queued run off the queue before it is deleted, so it cannot start meanwhile:
    /// `false` when it just started.
    pub(super) fn leave_queue_for_delete(&self, id: &str) -> Result<bool> {
        let _queue = self.gates.queue.lock().unwrap_or_else(|p| p.into_inner());
        self.claim(id, RunStatus::Cancelled, ev(EventKind::RunCancelled, "user", "删除这个排队中的任务"))
    }

    /// Put a goal run that never got its snapshot back on the queue, in its old place: the service
    /// stopped while it was being set up, or it could not start and the person continues it. Slots
    /// are handed out by whoever asked, when it is ready to (not during recovery).
    pub(super) fn requeue(&self, id: &str, why: &str) -> Result<()> {
        self.record(id, ev(EventKind::RunRequeued, "janus", why))?;
        self.db.set_run_status(id, RunStatus::Queued)?;
        self.run_changed(id, RunStatus::Queued);
        Ok(())
    }

    /// A run left the queue to start: set it up as it is now, or record why it cannot start.
    async fn start_queued(self: &Arc<Self>, id: &str) {
        if let Err(err) = self.start_run(id).await {
            let _ = self.record(
                id,
                ev(EventKind::RunFailed, "janus", format!("任务没能开始：{err:#}"))
                    .data(json!({ "error": format!("{err:#}") })),
            );
            if let Err(err) = self.set_stored_status(id, RunStatus::Failed) {
                crate::config::log(format!("janus: run {id} could not start nor be marked failed: {err:#}"));
            }
        }
    }

    /// Take the snapshot a queued run works with, from its workspace as it is now (team, library,
    /// base commit), make its worktree and start its lead.
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
    async fn start_run(self: &Arc<Self>, id: &str) -> Result<()> {
        let row = self.db.run(id)?.context("任务不存在")?;
        let queued = &row.config;
        let ws = self.workspace(queued["workspace"]["id"].as_str().unwrap_or(""))?;
        let (cwd, team) = self.startable_team(&ws)?;
        let checkout = crate::worktrees::Checkout::of(&cwd).await?;
        let is_git = checkout.is_some();
        let base_ref = checkout.as_ref().map(|c| c.head.clone()).unwrap_or_default();
        // A run on a git repository works in its own worktree, so runs on one repository do not
        // write over each other; one without commits yet, or outside git, works in place. The
        // worktree is of the whole repository; a workspace that is a folder inside it
        // (`apps/client`) has its sessions start in that folder of the worktree.
        let (cwd, worktree) = if let Some(checkout) = checkout.filter(|c| !c.head.is_empty()) {
            let top = checkout.top.clone();
            let path = self.worktree_dir(id);
            let branch = crate::worktrees::current_branch(&top);
            let included = {
                let _writing = self.gates.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
                // A start the service did not finish (it stopped) left its worktree: begin again
                // from a clean one.
                if path.exists() {
                    crate::worktrees::remove(&top, &self.worktree_root(id), &self.cfg.layout.deleted_runs(), id)?;
                }
                crate::worktrees::create(&top, &path, &base_ref, id)?
            };
            // The path the agents' own tools report (`/private/var/…` for `/var/…` on macOS), so
            // the files they edit resolve against it.
            let start = checkout.start_in(path.canonicalize().unwrap_or(path));
            (start, Some(WorktreeOf { repo: top, branch, included }))
        } else {
            (cwd, None)
        };
        // A run that cannot start leaves no worktree behind.
        let dirs = self.cfg.layout.run(RunKind::Goal, id);
        if let Err(err) = self.bundle_team(&dirs, &team) {
            if let Some(w) = &worktree {
                let kept = self.cfg.layout.deleted_runs();
                let _writing = self.gates.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
                if let Err(e) = crate::worktrees::remove(&w.repo, &self.worktree_root(id), &kept, id) {
                    crate::config::log(format!("janus: could not take back the worktree of {id}: {e:#}"));
                }
            }
            return Err(err);
        }
        let goal_images = queued["goalImages"].as_array().cloned().unwrap_or_default();
        let mut config =
            json!({ "team": team, "git": is_git, "baseRef": base_ref, "workspace": ws, "goalImages": goal_images });
        if let Some(limit) = queued.get("tokenLimit") {
            config["tokenLimit"] = limit.clone();
        }
        if let Some(w) = &worktree {
            config["worktree"] = json!({ "repo": w.repo, "branch": w.branch });
        }
        self.db.start_run(id, &cwd.to_string_lossy(), &config)?;
        let run = Arc::new(Run::new(RunRecord {
            id: id.to_string(),
            kind: RunKind::Goal,
            goal: row.goal.as_str().to_string(),
            cwd,
            worktree,
            team,
            git: is_git && !base_ref.is_empty(),
            base_ref,
            brief: ws.brief.trim().to_string(),
            goal_images,
            last_seq: self.db.max_seq(id)?,
            status: RunStatus::Planning,
            asker: Asker::Schedule,
        }));
        self.runs.lock().unwrap().insert(id.to_string(), run.clone());
        self.launch(run, Phase::Start);
        Ok(())
    }
}
