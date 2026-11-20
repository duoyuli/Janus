//! Chats: a conversation with one digital employee, as a run of its own kind. The employee is the
//! run's one member and holds its main session; there is no board, no goal and no delivery. The
//! conversation works in a scratch directory under Janus home made for it, and everything else
//! (messages, steering, takeover, approvals and forms, images, `@` files, controls, usage, reopening
//! after a restart) is the same as in the lead's conversation.
// @project-doc docs/domains/20261120-chats.md#chat_model

use super::*;

impl App {
    /// Start a chat with `role`: snapshot it as the run's one member, make the chat's directory,
    /// record the first message and open the session that takes it.
    // @project-doc docs/domains/20261120-chats.md#chat_model
    pub async fn create_chat(self: &Arc<Self>, req: NewChat) -> Result<String> {
        if req.text.trim().is_empty() && req.images.is_empty() {
            bail!("消息不能为空");
        }
        let role = self
            .db
            .get_config(crate::agents::ROLE, &req.role)?
            .ok_or_else(|| anyhow!("数字员工 {} 不存在", req.role))?;
        // The library as it is now, pinned for the chat like for a run's team.
        // @project-doc docs/domains/20261116-resource_library.md#delivery
        let member = self.role_member(&role, &self.items()?, true)?;
        self.spec_for(&member.runtime)?;
        let id = crate::agents::new_id("r");
        let dir = self.chat_dir(&id);
        std::fs::create_dir_all(&dir).with_context(|| format!("创建对话目录 {} 失败", dir.display()))?;
        // The path the agent's own tools report, so the files it names resolve against it.
        let cwd = dir.canonicalize().unwrap_or(dir);
        let team = vec![member];
        self.bundle_team(&id, &team)?;
        let images = crate::attachments::save(&self.cfg.data_dir, &id, &req.images)?;
        let text = crate::attachments::with_lines(req.text.trim(), &images);
        let title = match req.text.trim() {
            "" => "看看这些图片".to_string(),
            t => t.to_string(),
        };
        let config = json!({ "team": team, "git": false, "baseRef": "" });
        self.db.insert_run(&id, RunKind::Chat, &title, &cwd.to_string_lossy(), &config, now_ms())?;
        let run = Arc::new(Run::new(RunRecord {
            id: id.clone(),
            kind: RunKind::Chat,
            goal: title,
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
        // The first message waits for the session like any message left for it, and opens it.
        let msg = self.post_message(&run, "user", "lead", "", &text, &images);
        run.st.lock().unwrap().notes.entry("lead".into()).or_default().push(msg);
        self.runs.lock().unwrap().insert(id.clone(), run.clone());
        self.spawn_conversation(run);
        Ok(id)
    }

    /// Where a chat works: its own directory under Janus home, removed with the chat.
    pub(super) fn chat_dir(&self, run_id: &str) -> PathBuf {
        self.cfg.data_dir.join("chats").join(run_id)
    }
}

/// What Janus tells a chat's agent in its system layer: who it is talking to and where it works,
/// and, when the chat goes on in a new session, what was said so far.
// @project-doc docs/domains/20261120-chats.md#chat_session
pub(super) fn chat_brief(run: &Run, events: &[Event]) -> String {
    let me = &run.team[0];
    let mut p = format!(
        "You are {}, a digital employee talking with the user directly, one to one. There is no team and no shared board in this conversation.",
        me.name
    );
    if !me.desc.trim().is_empty() {
        p += &format!("\nYour responsibilities: {}", me.desc.trim());
    }
    p += &format!(
        "\n\nYour working directory {} is a scratch directory Janus made for this conversation. It started empty and is deleted together with the conversation, so keep nothing there that the user needs afterwards. Work on files elsewhere only where the user points you.",
        run.cwd.display()
    );
    let recent = recent_exchanges(events, "You");
    if !recent.is_empty() {
        p += &format!(
            "\n\nThis conversation continues in a new session. Use the history below to pick up where you left off.\n\nRecent conversation:\n{recent}"
        );
    }
    p
}
