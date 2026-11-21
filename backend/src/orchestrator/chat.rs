//! Chats: a conversation with one digital employee, as a run of its own kind. The employee is the
//! run's one member and holds its main session; there is no board, no goal and no delivery. The
//! conversation works in a scratch directory under Janus home made for it, and everything else
//! (messages, steering, takeover, approvals and forms, images, `@` files, controls, usage, reopening
//! after a restart) is the same as in the lead's conversation.
// @project-doc docs/domains/20261120-chats.md#chat_model

use super::*;

impl App {
    /// Start a chat with `role`: snapshot it as the run's one member, make the chat's directory and
    /// open its session, which waits for the person's first message.
    // @project-doc docs/domains/20261120-chats.md#chat_model
    pub async fn create_chat(self: &Arc<Self>, req: NewChat) -> Result<String> {
        let role = self
            .db
            .get_config(crate::agents::ROLE, &req.role)?
            .ok_or_else(|| anyhow!("数字员工 {} 不存在", req.role))?;
        // The library as it is now, pinned for the chat like for a run's team.
        // @project-doc docs/domains/20261116-resource_library.md#delivery
        let member = self.role_member(&role, &self.items()?, true)?;
        self.spec_for(&member.runtime)?;
        let id = crate::agents::new_id("r");
        let dirs = self.cfg.layout.run(RunKind::Chat, &id);
        let dir = dirs.work.clone();
        std::fs::create_dir_all(&dir).with_context(|| format!("创建对话目录 {} 失败", dir.display()))?;
        // The path the agent's own tools report, so the files it names resolve against it.
        let cwd = dir.canonicalize().unwrap_or(dir);
        let team = vec![member];
        self.bundle_team(&dirs, &team)?;
        let config = json!({ "team": team, "git": false, "baseRef": "" });
        // Titled by its first message, once there is one (`title_chat`).
        self.db.insert_run(&id, RunKind::Chat, "", &cwd.to_string_lossy(), &config, now_ms())?;
        let run = Arc::new(Run::new(RunRecord {
            id: id.clone(),
            kind: RunKind::Chat,
            goal: String::new(),
            cwd,
            worktree: None,
            team,
            git: false,
            base_ref: String::new(),
            brief: String::new(),
            goal_images: vec![],
            last_seq: 0,
            status: RunStatus::Planning,
            cost_limit_micros: 0,
        }));
        let name = run.team[0].name.clone();
        self.emit(
            &run,
            ev("run.created", "user", format!("和 {name} 开始对话")).data(json!({
                "kind": RunKind::Chat, "cwd": run.cwd, "git": false, "role": { "id": role["id"], "name": name },
            })),
        );
        self.runs.lock().unwrap().insert(id.clone(), run.clone());
        self.spawn_conversation(run);
        Ok(id)
    }

    /// A chat is listed by its first message: the first one the person sends titles it.
    pub(super) fn title_chat(&self, run: &Run, text: &str) -> Result<()> {
        if run.kind != RunKind::Chat || self.db.run(&run.id)?.is_some_and(|r| r["goal"] != "") {
            return Ok(());
        }
        let title = match text.trim() {
            "" => "看看这些图片",
            t => t,
        };
        self.db.set_run_goal(&run.id, title)
    }
}

/// What Janus adds to a chat's system layer: nothing of its own, since a chat's agent works only by
/// the digital employee's instructions (the standing ones that follow), and not by Janus's teamwork
/// rules, which are for an Agent Team. Only a chat that goes on in a new session (its old one could
/// not be loaded) gets what was said so far, so it knows where it left off.
// @project-doc docs/domains/20261120-chats.md#chat_session
pub(super) fn chat_brief(events: &[Event]) -> String {
    let recent = recent_exchanges(events, "You");
    if recent.is_empty() {
        return String::new();
    }
    format!(
        "This conversation with the user continues in a new session. Use the history below to pick up where you left off.\n\nRecent conversation:\n{recent}"
    )
}
