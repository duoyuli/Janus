//! The states the orchestrator moves through, as types instead of strings.
//!
//! Every value here is stored and sent as the same snake_case string the database, the events
//! and the app already use (`RunStatus::Running.as_str() == "running"`), so this module changes
//! where a state is allowed to change, not the wire format. The allowed moves are stated once,
//! in `can_transition_to`; a call that does not fit is a bug in the caller.

use serde::{Deserialize, Serialize};

/// The lifecycle of one Run, as `runs.status` and the app's run list see it.
///
/// `paused` and `cancelled` are not separate values: a paused Run keeps `Running` (running
/// attempts finish), and cancelling sets `Cancelled`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Created, the lead's goal session is starting.
    #[default]
    Planning,
    Running,
    /// Over budget, waiting for the person to extend it or cancel.
    WaitingInput,
    /// Delivered, waiting for the person to accept it.
    Delivered,
    Accepted,
    Cancelled,
    Failed,
    /// The orchestrator restarted while the Run was unfinished.
    Interrupted,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RunStatus {
    /// Read a status the database or an older record wrote.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "planning" => RunStatus::Planning,
            "running" => RunStatus::Running,
            "waiting_input" => RunStatus::WaitingInput,
            "delivered" => RunStatus::Delivered,
            "accepted" => RunStatus::Accepted,
            "cancelled" => RunStatus::Cancelled,
            "failed" => RunStatus::Failed,
            "interrupted" => RunStatus::Interrupted,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Planning => "planning",
            RunStatus::Running => "running",
            RunStatus::WaitingInput => "waiting_input",
            RunStatus::Delivered => "delivered",
            RunStatus::Accepted => "accepted",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Failed => "failed",
            RunStatus::Interrupted => "interrupted",
        }
    }

    /// Whether the Run may move from `self` to `to`. Repeating a state is always allowed: the
    /// scheduling loop recomputes the status on every pass.
    pub fn can_transition_to(self, to: RunStatus) -> bool {
        use RunStatus::*;
        if self == to {
            return true;
        }
        matches!(
            (self, to),
            (Planning, Running | WaitingInput | Delivered | Failed | Cancelled | Interrupted)
                | (Running, WaitingInput | Delivered | Failed | Cancelled | Interrupted)
                | (WaitingInput, Running | Delivered | Failed | Cancelled | Interrupted)
                | (Delivered, Accepted | Running | Failed | Cancelled | Interrupted)
                // Resuming rebuilds the run from its event log; the new driver starts running.
                | (Failed, Running)
                | (Interrupted, Running)
        )
    }
}

/// A task on the shared board. These are the states the scheduler writes; the app additionally
/// derives display states (`ready`, `assigned`, `verifying`, `verification_failed`,
/// `waiting_input`, `waiting_approval`) from events, see `docs/domains/run_lifecycle.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    /// A dependency failed; returns to `Pending` once that dependency succeeds.
    Blocked,
    Succeeded,
    Failed,
    Cancelled,
    /// The lead dropped the task with `cancel`.
    SkippedByDecision,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::SkippedByDecision => "skipped_by_decision",
        }
    }

    /// What the lead and the person read.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Pending => "等待开始",
            TaskStatus::Running => "执行中",
            TaskStatus::Succeeded => "已完成",
            TaskStatus::Failed => "失败",
            TaskStatus::Blocked => "依赖失败",
            TaskStatus::Cancelled => "已取消",
            TaskStatus::SkippedByDecision => "主控已取消",
        }
    }

    /// No further work starts from this state without an explicit retry or acceptance.
    pub fn terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::SkippedByDecision
        )
    }

    /// A failed dependency blocks its downstream task.
    pub fn failed_dependency(self) -> bool {
        matches!(
            self,
            TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Blocked | TaskStatus::SkippedByDecision
        )
    }

    pub fn can_transition_to(self, to: TaskStatus) -> bool {
        use TaskStatus::*;
        if self == to {
            return true;
        }
        matches!(
            (self, to),
            (Pending, Running | Blocked | Cancelled | SkippedByDecision)
                | (Running, Pending | Succeeded | Failed | Cancelled | SkippedByDecision)
                | (Blocked, Pending | Cancelled | SkippedByDecision)
                // A user retry (task.reopened) reopens failed and cancelled tasks.
                | (Failed, Pending | Succeeded | SkippedByDecision)
                | (Cancelled, Pending | SkippedByDecision)
                | (SkippedByDecision, Pending)
        )
    }
}

/// One ACP attempt, as the `attempts` table records it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    #[default]
    Starting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// The orchestrator restarted while the attempt was open.
    Interrupted,
}

impl std::fmt::Display for AttemptStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptStatus::Starting => "starting",
            AttemptStatus::Running => "running",
            AttemptStatus::Succeeded => "succeeded",
            AttemptStatus::Failed => "failed",
            AttemptStatus::Cancelled => "cancelled",
            AttemptStatus::Interrupted => "interrupted",
        }
    }
}

/// What an agent's session is doing, as `agent.status` events report it. The app maps unknown
/// values to `idle`, but this is the complete set Janus emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    Idle,
    Thinking,
    Coordinating,
    ReadingFiles,
    Editing,
    RunningTool,
    RunningTest,
    WaitingForInput,
    WaitingForAgent,
    WaitingApproval,
    Error,
}

impl std::fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AgentPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentPhase::Idle => "idle",
            AgentPhase::Thinking => "thinking",
            AgentPhase::Coordinating => "coordinating",
            AgentPhase::ReadingFiles => "reading_files",
            AgentPhase::Editing => "editing",
            AgentPhase::RunningTool => "running_tool",
            AgentPhase::RunningTest => "running_test",
            AgentPhase::WaitingForInput => "waiting_for_input",
            AgentPhase::WaitingForAgent => "waiting_for_agent",
            AgentPhase::WaitingApproval => "waiting_approval",
            AgentPhase::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_keeps_its_wire_name() {
        for (s, name) in [
            (RunStatus::Planning, "planning"),
            (RunStatus::Running, "running"),
            (RunStatus::WaitingInput, "waiting_input"),
            (RunStatus::Delivered, "delivered"),
            (RunStatus::Accepted, "accepted"),
            (RunStatus::Cancelled, "cancelled"),
            (RunStatus::Failed, "failed"),
            (RunStatus::Interrupted, "interrupted"),
        ] {
            assert_eq!(s.as_str(), name);
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<RunStatus>(&format!("\"{name}\"")).unwrap(), s);
        }
        for (s, name) in [
            (TaskStatus::Pending, "pending"),
            (TaskStatus::Running, "running"),
            (TaskStatus::Blocked, "blocked"),
            (TaskStatus::Succeeded, "succeeded"),
            (TaskStatus::Failed, "failed"),
            (TaskStatus::Cancelled, "cancelled"),
            (TaskStatus::SkippedByDecision, "skipped_by_decision"),
        ] {
            assert_eq!(s.as_str(), name);
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{name}\""));
        }
    }

    #[test]
    fn run_states_only_move_where_the_lifecycle_allows() {
        assert!(RunStatus::Planning.can_transition_to(RunStatus::Running));
        assert!(RunStatus::Running.can_transition_to(RunStatus::WaitingInput));
        assert!(RunStatus::WaitingInput.can_transition_to(RunStatus::Running));
        assert!(RunStatus::Running.can_transition_to(RunStatus::Delivered));
        assert!(RunStatus::Delivered.can_transition_to(RunStatus::Accepted));
        assert!(RunStatus::Delivered.can_transition_to(RunStatus::Running), "a retry reopens delivery");
        assert!(!RunStatus::Cancelled.can_transition_to(RunStatus::Running));
        assert!(!RunStatus::Accepted.can_transition_to(RunStatus::Delivered));
        assert!(!RunStatus::Planning.can_transition_to(RunStatus::Accepted));
        // Only a delivered run is accepted, and nothing follows acceptance.
        assert!(!RunStatus::Running.can_transition_to(RunStatus::Accepted));
        assert!(!RunStatus::WaitingInput.can_transition_to(RunStatus::Accepted));
        assert!([RunStatus::Running, RunStatus::Cancelled, RunStatus::Interrupted, RunStatus::Failed].iter().all(|to| !RunStatus::Accepted.can_transition_to(*to)));
        assert!(RunStatus::Running.can_transition_to(RunStatus::Running));
    }

    #[test]
    fn task_states_only_move_where_the_scheduler_allows() {
        assert!(TaskStatus::Pending.can_transition_to(TaskStatus::Running));
        assert!(TaskStatus::Pending.can_transition_to(TaskStatus::Blocked));
        assert!(TaskStatus::Blocked.can_transition_to(TaskStatus::Pending));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Succeeded));
        assert!(TaskStatus::Running.can_transition_to(TaskStatus::Failed));
        assert!(TaskStatus::Failed.can_transition_to(TaskStatus::Pending), "the lead retries it");
        assert!(TaskStatus::Failed.can_transition_to(TaskStatus::Succeeded), "the lead accepts it");
        assert!(!TaskStatus::Succeeded.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Pending.can_transition_to(TaskStatus::Succeeded));
        assert!(!TaskStatus::Blocked.terminal());
        assert!(TaskStatus::Failed.terminal());
        assert!(TaskStatus::Failed.failed_dependency());
    }
}
