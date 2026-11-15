//! Messages from the person and the lead to a session: posting, routing into a live turn or
//! queueing.

use super::*;

impl App {
    /// Messages left for an agent while it had no open session, handed over with its next prompt.
    /// Record a message the moment it is sent, before any session has picked it up.
    // @project-doc docs/interfaces/events.md#message_flow
    pub(super) fn post_message(&self, run: &Run, actor: &str, to: &str, task: &str, text: &str, images: &[Value]) -> Msg {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        let id = format!("m{}-{}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst));
        let mut data = json!({ "id": id, "text": text, "delivery": "pending" });
        if !images.is_empty() {
            data["images"] = json!(images);
        }
        let label = if images.is_empty() { short(text, 80) } else { format!("{}（附 {} 张图片）", short(text.lines().next().unwrap_or(""), 60), images.len()) };
        let mut e = ev("user.message", actor, label).to(to).data(data);
        if !task.is_empty() {
            e = e.task(task);
        }
        self.emit(run, e);
        Msg { id, text: text.to_string() }
    }

    /// How a sent message went in: `steer` (joined the running turn), `queued` (after this turn),
    /// `prompt` (a turn of its own) or `next` (with the agent's next session).
    pub(super) fn route_message(&self, run: &Run, agent: &str, task: &str, n: i64, id: &str, delivery: &str) {
        let text = match delivery {
            "steer" => "插入到正在进行的这一轮",
            "queued" => "排队：这一轮结束后发送",
            "interrupt" => "打断正在进行的这一轮，立即发送",
            "prompt" => "作为新的一轮发送",
            _ => "留言：下次会话开始时带给它",
        };
        let mut e = ev("message.routed", "janus", text).to(agent).data(json!({ "id": id, "delivery": delivery }));
        if !task.is_empty() {
            e = e.task(task);
            if n > 0 {
                e = e.attempt(n);
            }
        }
        self.emit(run, e);
    }

    /// Queued messages become the next turn of an attempt; returns its text.
    pub(super) fn deliver_queued(&self, run: &Run, ctl: &mut Control, agent: &str, id: &str, n: i64) -> String {
        let msgs: Vec<Msg> = std::mem::take(&mut ctl.queued);
        let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
        self.emit(
            run,
            ev("message.delivered", "janus", format!("把排队的 {} 条消息作为新一轮发给 {agent}", msgs.len()))
                .task(id)
                .attempt(n)
                .to(agent)
                .data(json!({ "ids": ids })),
        );
        msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n")
    }

    pub(super) fn take_notes(&self, run: &Run, agent: &str) -> String {
        let notes = run
            .st
            .lock()
            .unwrap()
            .notes
            .remove(agent)
            .unwrap_or_default();
        if notes.is_empty() {
            return String::new();
        }
        let ids: Vec<&str> = notes.iter().map(|m| m.id.as_str()).collect();
        self.emit(
            run,
            ev(
                "message.delivered",
                "janus",
                format!("把你留给 {agent} 的 {} 条消息带进这次会话", notes.len()),
            )
            .to(agent)
            .data(json!({ "ids": ids })),
        );
        format!(
            "\n\n用户补充的信息（请优先考虑）：\n{}",
            notes
                .iter()
                .map(|n| format!("- {}", n.text))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}
