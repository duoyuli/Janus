//! Live frames: what an agent is writing, thinking or running right now, before it becomes an
//! event. The run's event stream carries them beside its events (`event: live`) so the app can
//! show text as it streams and a command's output while it runs. They are never stored: the
//! events stay the record, written as before (a message once the agent pauses or calls a tool,
//! a tool call once it is done), and replays, the ledger and the board read only those.
// @project-doc docs/interfaces/20261031-acp_integration.md#live_frames

use super::*;

/// A preview of one session's output not yet on record.
#[derive(Clone, Debug)]
pub struct Live {
    pub run: String,
    pub data: Value,
}

/// How often a session's preview is sent while it streams.
pub(super) const LIVE_EVERY: Duration = Duration::from_millis(100);

/// A running command's output in a preview: its last lines, as the agent's own terminal ends.
const TAIL_LINES: usize = 16;
const TAIL_CHARS: usize = 4000;

impl App {
    /// Send one frame of a session's preview. `seq` is the run's newest event when the frame is
    /// made: the app shows a frame only while no newer record of the same output has reached it.
    pub(super) fn live(&self, run: &Run, ctx: &Ctx<'_>, mut data: Value) {
        if self.live.receiver_count() == 0 {
            return;
        }
        data["actor"] = json!(ctx.agent);
        data["task"] = json!(ctx.task);
        data["attempt"] = json!(ctx.attempt);
        data["seq"] = json!(run.last_seq.load(Ordering::SeqCst));
        let _ = self.live.send(Live { run: run.id.clone(), data });
    }

    /// The preview of what changed since the last frame: the text and thought not yet recorded,
    /// and each tool call still running that moved.
    pub(super) fn live_frame(&self, run: &Run, ctx: &Ctx<'_>, unshown: &str, turn: &mut crate::acp_view::Turn) {
        turn.live_at = now_ms();
        // Text or thought already on record leaves nothing to preview: the app drops it by the event.
        if std::mem::take(&mut turn.live_said) && !(unshown.trim().is_empty() && turn.thought.is_empty()) {
            self.live(
                run,
                ctx,
                json!({ "kind": "say", "text": clip(unshown, 8000), "thought": crate::text::tail(&turn.thought, TAIL_CHARS) }),
            );
        }
        for id in std::mem::take(&mut turn.live_tools) {
            let Some(entry) = turn.tools.get(&id) else { continue };
            let title = entry["title"].as_str().unwrap_or("工具调用");
            self.live(
                run,
                ctx,
                json!({
                    "kind": "tool",
                    "callId": id,
                    "title": clip(title, 80),
                    "tool": entry["kind"].as_str().unwrap_or("tool"),
                    "input": super::turn::summarize_input(&entry["rawInput"], title),
                    "output": tool_tail(entry),
                    "status": entry["status"].as_str().unwrap_or("pending"),
                }),
            );
        }
    }

    /// The session's turn is over: whatever it previewed is now on record or gone.
    pub(super) fn live_end(&self, run: &Run, ctx: &Ctx<'_>) {
        self.live(run, ctx, json!({ "kind": "end" }));
    }
}

/// When the next frame is due, if anything waits for one.
pub(super) fn live_due(turn: &crate::acp_view::Turn) -> Option<tokio::time::Instant> {
    if !turn.live_said && turn.live_tools.is_empty() {
        return None;
    }
    let wait = (turn.live_at + LIVE_EVERY.as_millis() as i64 - now_ms()).max(0) as u64;
    Some(tokio::time::Instant::now() + Duration::from_millis(wait))
}

/// A running tool's output so far, from its content or raw output, as its last lines.
fn tool_tail(entry: &Value) -> String {
    let mut text = String::new();
    if let Some(items) = entry["content"].as_array() {
        for c in items {
            if let Some(t) = c["content"]["text"].as_str() {
                text.push_str(t);
            }
        }
    }
    if text.is_empty() {
        text = match &entry["rawOutput"] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            v => v["output"].as_str().or(v["stdout"].as_str()).map(str::to_owned).unwrap_or_default(),
        };
    }
    if let Some(t) = entry["_meta"]["terminal_output"]["data"].as_str() {
        text.push_str(t);
    }
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    crate::text::tail(&lines[start..].join("\n"), TAIL_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_preview_keeps_the_last_lines() {
        let out: String = (1..=40).map(|i| format!("line {i}\n")).collect();
        let entry = json!({ "content": [{ "type": "content", "content": { "type": "text", "text": out } }] });
        let t = tool_tail(&entry);
        assert!(t.starts_with("line 25\n") && t.ends_with("line 40"), "{t}");
        assert_eq!(tool_tail(&json!({ "rawOutput": { "stdout": "ok\n" } })), "ok");
        assert_eq!(tool_tail(&json!({})), "");
    }

    #[test]
    fn a_frame_waits_for_the_interval() {
        let mut turn = crate::acp_view::Turn::default();
        assert!(live_due(&turn).is_none(), "nothing to preview");
        turn.live_said = true;
        turn.live_at = now_ms();
        let due = live_due(&turn).unwrap();
        assert!(due > tokio::time::Instant::now() + Duration::from_millis(50));
        turn.live_at = 0;
        assert!(live_due(&turn).unwrap() <= tokio::time::Instant::now());
    }
}
