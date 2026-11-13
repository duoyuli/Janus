//! When a lead's goal does not settle by itself. A lead runs `/goal`, and Janus follows its
//! session until the agent reports the goal no longer active. Some agents never do: Claude Code
//! clears a goal only when its own check confirms the condition, and that check may not run (a
//! custom endpoint without the small model it uses). The lead then says it is done and stops,
//! while its goal stays "active" and the run never delivers. So once the lead's turn has
//! returned and its session has been quiet for a while with no tool running, Janus asks it
//! directly; a lead that confirms gets its goal cleared through `_lody/session/goal`, and the run
//! goes on to delivery. A lead that is not done goes on working; after a few asks the watchdog
//! is left to tell the person.
// @project-doc docs/domains/goal_mode.md#goal_settle

use std::time::Duration;

/// How long a lead's session must be quiet, after its turn returned, before Janus asks.
pub const SETTLE: Duration = Duration::from_secs(90);
/// How many times Janus asks in one stretch of following.
pub const ASKS: u8 = 3;
/// The word a lead starts its reply with to confirm the goal is done.
pub const DONE: &str = "GOAL_DONE";

pub fn ask() -> String {
    format!(
        "[Janus] 你的这一轮已经结束，但目标仍然标记为进行中，会话也已经安静了一段时间。\n\
         如果目标已经全部完成并且你核对过结果，请在回复的第一行只写 {DONE}，然后给出最终总结（它会作为这个 Run 的交付摘要）。\n\
         如果还没有完成，说明还差什么，然后继续推进。"
    )
}

/// The lead's reply to `ask`, when it confirms: the summary without the marker.
pub fn confirmed(reply: &str) -> Option<String> {
    let t = reply.trim_start();
    let rest = t.strip_prefix(DONE)?;
    Some(rest.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_reply_that_starts_with_the_marker_confirms() {
        assert_eq!(confirmed("GOAL_DONE\n已完成：divide 已加上。").as_deref(), Some("已完成：divide 已加上。"));
        assert_eq!(confirmed("  GOAL_DONE").as_deref(), Some(""));
        assert!(confirmed("还差测试没跑，GOAL_DONE 之后再说").is_none());
        assert!(confirmed("继续推进").is_none());
        assert!(ask().contains(DONE));
    }
}
