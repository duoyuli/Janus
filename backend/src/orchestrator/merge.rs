//! Bringing a run's result back to where it started (`crate::worktrees::accept`): each time the run
//! is delivered, so what the run did is on the person's branch when they look at it, the way it
//! would be without a worktree; and when the person accepts a delivery that could not bring it.
//! Each records the commit it left (`data.result` of `run.merged` or `risk.accepted`), where the
//! next one starts from.

use super::*;

/// What one merge-back came to.
struct MergedBack {
    /// Where it started: the run's start, or the result the one before left.
    since: String,
    accepted: crate::worktrees::Accepted,
}

/// What bringing the result back at a delivery came to.
pub(super) enum DeliveryMerge {
    /// Nothing new since the last merge-back (or since the run started).
    Nothing,
    /// The result is on the branch now: what `run.merged` records.
    Merged { text: String, data: Value },
    /// Stopped on something in the worktree the lead resolves (`merge_handback`); nothing changed
    /// on the person's side.
    HandBack(String),
    /// Stopped on something on the person's side (their uncommitted work in the way, the branch
    /// gone or rewritten): theirs to sort out, then accepting brings it back.
    Failed(String),
}

wire_enum! {
    /// How a delivery's result reached the person's branch, for the app (`run.delivered`
    /// `merge.state`).
    // @project-doc docs/interfaces/20261031-events.md#event_types
    pub(super) enum MergeState {
        /// On the branch now.
        Merged = "merged",
        /// Nothing new since the last merge-back.
        Nothing = "nothing",
        /// A conflict with the branch's new commits that the lead is resolving now.
        Resolving = "resolving",
        /// One still there after the lead tried, which the person hands back to it by accepting.
        Conflict = "conflict",
        /// Something on the person's side is in the way (their uncommitted work, a rewritten branch).
        Blocked = "blocked",
        /// Not a Git repository: the run changed the person's folder directly.
        InPlace = "inPlace",
    }
}

impl DeliveryMerge {
    /// How the delivery reached the person's branch; `again` when the lead already tried to
    /// resolve the same stop.
    pub(super) fn state(&self, again: bool) -> MergeState {
        match self {
            DeliveryMerge::Merged { .. } => MergeState::Merged,
            DeliveryMerge::Nothing => MergeState::Nothing,
            DeliveryMerge::HandBack(_) if again => MergeState::Conflict,
            DeliveryMerge::HandBack(_) => MergeState::Resolving,
            DeliveryMerge::Failed(_) => MergeState::Blocked,
        }
    }
}

impl App {
    /// Bring the result of the run in worktree `w` back to its source, committing what is not
    /// committed yet as `message`; with `remove` the worktree goes afterwards.
    fn merge_back(
        &self,
        run_id: &str,
        w: &WorktreeOf,
        base_ref: &str,
        message: &str,
        remove: bool,
    ) -> Result<MergedBack> {
        // The source must still be where the run started, or where its last merge-back left it,
        // or have moved on from there.
        let since = self.last_merged(run_id)?.unwrap_or_else(|| base_ref.to_string());
        let _writing = self.gates.worktree_writes.lock().unwrap_or_else(|p| p.into_inner());
        let accepted = crate::worktrees::accept(
            &w.repo,
            &self.worktree_root(run_id),
            w.branch.as_deref(),
            &since,
            message,
            remove,
        )?;
        Ok(MergedBack { since, accepted })
    }

    /// The run is delivered: its result goes back to its branch now (see `DeliveryMerge`).
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
    pub(super) fn merge_at_delivery(&self, run_id: &str, w: &WorktreeOf, base_ref: &str, goal: &str) -> DeliveryMerge {
        match self.merge_back(run_id, w, base_ref, &commit_message(goal, run_id), false) {
            Ok(m) if m.accepted.result == m.since => DeliveryMerge::Nothing,
            Ok(m) => {
                let a = &m.accepted;
                let text = if a.brought {
                    format!("交付时把结果合并到 {}（提交 {}）", w.source(), short_sha(&a.result))
                } else {
                    format!("{} 已经包含这个任务的结果（提交 {}）", w.source(), short_sha(&a.result))
                };
                DeliveryMerge::Merged { text, data: merged_data(w, a) }
            }
            Err(err) => match merge_handback(&err, MergeAt::Delivery) {
                Some(prompt) => DeliveryMerge::HandBack(prompt),
                None => DeliveryMerge::Failed(format!("{err:#}")),
            },
        }
    }

    /// What a delivery says about where its result is, after `merge_at_delivery`; `again` when the
    /// lead was already handed this conflict once and its turn did not resolve it.
    pub(super) fn delivery_note(&self, run_id: &str, w: &WorktreeOf, merge: &DeliveryMerge, again: bool) -> String {
        let source = w.source();
        match merge {
            DeliveryMerge::Merged { data, .. } => {
                format!("结果已合并到 {source}（提交 {}）。", short_sha(data["result"].as_str().unwrap_or_default()))
            }
            DeliveryMerge::Nothing => match self.last_merged(run_id).ok().flatten() {
                Some(result) => format!("结果已在 {source}（提交 {}），没有新的改动。", short_sha(&result)),
                None => format!("这个任务没有改动，{source} 没有变化。"),
            },
            DeliveryMerge::HandBack(_) if again => format!(
                "合并到 {source} 时仍然冲突，你的目录和分支都没有动：可以继续和负责人处理，或者点验收让负责人再解决一次。"
            ),
            DeliveryMerge::HandBack(_) => format!(
                "合并到 {source} 时和分支上的新提交冲突，你的目录和分支都没有动：负责人正在任务的副本里解决，解决后再次交付并合并。"
            ),
            // It says what is in the way and that accepting merges once it is out of the way.
            DeliveryMerge::Failed(err) => err.clone(),
        }
    }

    /// The person accepts a run: what a delivery could not bring back is committed and brought
    /// back now, and with `remove` (no session works there) the worktree goes (see
    /// `crate::worktrees::accept`). The acceptance's text and data.
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
    pub(super) fn accept_worktree(
        &self,
        run_id: &str,
        worktree: Option<&WorktreeOf>,
        base_ref: &str,
        goal: &str,
        remove: bool,
    ) -> Result<(String, Value)> {
        let Some(w) = worktree else {
            return Ok(("核对证据后验收通过".into(), json!({})));
        };
        let m = self.merge_back(run_id, w, base_ref, &commit_message(goal, run_id), remove)?;
        let a = &m.accepted;
        let text = if a.brought {
            format!("核对证据后验收通过，结果已合并到 {}（提交 {}）", w.source(), short_sha(&a.result))
        } else if a.result == base_ref {
            "核对证据后验收通过，这个任务没有改动，没有提交".into()
        } else {
            format!("核对证据后验收通过，结果已在 {}（提交 {}）", w.source(), short_sha(&a.result))
        };
        Ok((text, merged_data(w, a)))
    }

    /// Whether the worktree of run `run_id` has work its last merge-back (or, before any, the
    /// run's start `base_ref`) does not (`crate::worktrees::moved_on`): files changed, or commits
    /// made after it.
    pub(super) fn moved_on_since_merge(&self, run_id: &str, base_ref: &str) -> bool {
        let last = self.last_merged(run_id).unwrap_or_else(|err| {
            crate::config::log(format!("janus: run {run_id}: could not read its last merge-back: {err:#}"));
            None
        });
        crate::worktrees::moved_on(&self.worktree_root(run_id), last.as_deref().unwrap_or(base_ref))
    }

    /// The commit holding the run's result as last brought back, by a delivery (`run.merged`) or
    /// the person accepting (`risk.accepted`): its `data.result`.
    pub(super) fn last_merged(&self, run_id: &str) -> Result<Option<String>> {
        let latest = [
            self.db.latest_event(run_id, EventKind::RiskAccepted)?,
            self.db.latest_event(run_id, EventKind::RunMerged)?,
        ]
        .into_iter()
        .flatten()
        .max_by_key(|e| e.seq);
        Ok(latest.and_then(|e| {
            let d = e.data?;
            d["result"].as_str().map(str::to_owned)
        }))
    }

    /// Where the run's own changes in its worktree start, for the diff of the whole run: past the
    /// last merge-back, and past the commits of the branch the worktree took in
    /// (`crate::worktrees::fork_point`). Outside a worktree, the run's start.
    pub(super) fn run_changes_from(&self, run: &Run) -> String {
        match &run.worktree {
            Some(w) => self.changes_from(&run.id, w, &run.base_ref),
            None => run.base_ref.clone(),
        }
    }

    fn changes_from(&self, run_id: &str, w: &WorktreeOf, base_ref: &str) -> String {
        let since = self.last_merged(run_id).ok().flatten().unwrap_or_else(|| base_ref.to_string());
        crate::worktrees::fork_point(&self.worktree_root(run_id), w.branch.as_deref(), &since)
    }

    /// The files the run in worktree `w` changed that are not back on its source yet, committed or
    /// not: what the next delivery brings, listed with it as this round's changes.
    pub(super) fn unmerged_files(&self, run_id: &str, w: &WorktreeOf, base_ref: &str) -> Vec<String> {
        let dir = self.worktree_root(run_id);
        let from = self.changes_from(run_id, w, base_ref);
        crate::traces::changed_files(
            &crate::git::output(&dir, &["diff-index", "--name-only", &from]),
            &crate::git::output(&dir, &["ls-files", "--others", "--exclude-standard"]),
        )
        .into_iter()
        .collect()
    }

    /// After a restart: the lead's conversation worked on files after the run was accepted and was
    /// cut off before its turn ended, which would have delivered it again. It is delivered now,
    /// with what it delivered last, and like any delivery brings the work back to its branch; what
    /// stops that waits in the worktree for the person to accept.
    // @project-doc docs/domains/20261031-run_lifecycle.md#recovery
    pub(super) fn redeliver_after_restart(&self, run_id: &str) -> Result<()> {
        let row = self.db.run(run_id)?.context("run not found")?;
        let w = WorktreeOf::of_config(&row["config"]).context("the run has no worktree")?;
        let base_ref = row["config"]["baseRef"].as_str().unwrap_or_default();
        let changed = self.unmerged_files(run_id, &w, base_ref);
        let merge = self.merge_at_delivery(run_id, &w, base_ref, row["goal"].as_str().unwrap_or(""));
        if let DeliveryMerge::Merged { text, data } = &merge {
            self.record(run_id, ev(EventKind::RunMerged, "janus", text.clone()).data(data.clone()))?;
        }
        let (state, note) = match &merge {
            // The lead is not there to resolve it now: accepting hands it over.
            DeliveryMerge::HandBack(_) => (
                MergeState::Conflict,
                format!(
                    "合并到 {} 时和分支上的新提交冲突，你的目录和分支都没有动：让负责人解决，它会在任务的副本里处理。",
                    w.source()
                ),
            ),
            merge => (merge.state(false), self.delivery_note(run_id, &w, merge, false)),
        };
        // This round is the lead's unfinished turn alone: no subtasks, no report of its own.
        let headline = "验收之后负责人又做了改动，服务重启时这一轮还没结束。";
        let data = json!({
            "headline": headline,
            "summary": headline,
            "decisions": [],
            "briefSuggestions": [],
            "deliverables": [],
            "evidence": [],
            "acceptance": [],
            "risks": [],
            "changed": changed,
            "leadEdits": [],
            "merge": { "state": state, "note": note },
            "usage": crate::usage::ledger(&self.db.events(run_id, 0)?).delivery_line(),
        });
        self.record(
            run_id,
            ev(EventKind::RunDelivered, "janus", "验收之后负责人又做了改动，重启后再次交付").data(data),
        )?;
        self.set_stored_status(run_id, RunStatus::Delivered)
    }
}

/// The commit message for what the run left uncommitted: the goal's first line, and the run.
fn commit_message(goal: &str, run_id: &str) -> String {
    let title: String = goal.lines().next().unwrap_or_default().chars().take(72).collect();
    format!("{title}\n\nJanus 任务 {run_id}")
}

/// What a merge-back records: where it went, what it brought (`commit`, `null` when the source had
/// it already) and the commit holding the result (`result`).
fn merged_data(w: &WorktreeOf, a: &crate::worktrees::Accepted) -> Value {
    json!({ "repo": w.repo, "branch": w.branch, "commit": a.brought.then_some(&a.result), "result": a.result })
}
