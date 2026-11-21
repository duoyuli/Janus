//! The states the orchestrator moves through, as types instead of strings.
//!
//! Every value here is stored and sent as the same snake_case string the database, the events
//! and the app already use (`RunStatus::Running.as_str() == "running"`), written once in its
//! `wire_enum!`. This module changes where a state is allowed to change, not the wire format. The
//! allowed moves are stated once, in `can_transition_to`; a call that does not fit is a bug in the
//! caller.

use crate::wire::wire_enum;

wire_enum! {
    /// The lifecycle of one Run, as `runs.status` and the app's run list see it.
    ///
    /// `paused` and `cancelled` are not separate values: a paused Run keeps `Running` (running
    /// attempts finish), and cancelling sets `Cancelled`.
    #[derive(Default)]
    pub enum RunStatus {
        /// Created, the lead's goal session is starting.
        #[default]
        Planning = "planning",
        Running = "running",
        /// Delivered, waiting for the person to accept it.
        Delivered = "delivered",
        Accepted = "accepted",
        Cancelled = "cancelled",
        Failed = "failed",
        /// The orchestrator restarted while the Run was unfinished.
        Interrupted = "interrupted",
    }
}

wire_enum! {
    /// What a Run is: a goal the lead pursues with its team, or a conversation with one digital
    /// employee (`runs.kind`). A chat has one member, no board, no goal and no delivery; it works in
    /// its own scratch directory under Janus home.
    // @project-doc docs/domains/20261120-chats.md#chat_model
    #[derive(Default)]
    pub enum RunKind {
        #[default]
        Goal = "goal",
        Chat = "chat",
    }
}

impl RunStatus {
    /// Work can still start or is in progress: the states in which a run's sessions may still
    /// write, so its data cannot be deleted out from under them.
    pub fn is_active(self) -> bool {
        matches!(self, RunStatus::Planning | RunStatus::Running)
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
            (Planning, Running | Delivered | Failed | Cancelled | Interrupted)
                | (Running, Delivered | Failed | Cancelled | Interrupted)
                | (Delivered, Accepted | Running | Failed | Cancelled | Interrupted)
                // Continuing an accepted run (a new task or goal in its session) reopens it.
                | (Accepted, Running | Delivered)
                // Resuming rebuilds the run from its event log; the new driver starts running.
                | (Failed, Running)
                | (Interrupted, Running)
        )
    }
}

wire_enum! {
    /// A task on the shared board. These are the states the scheduler writes; the app additionally
    /// derives display states (`ready`, `assigned`, `verifying`, `verification_failed`,
    /// `waiting_input`, `waiting_approval`) from events, see `docs/domains/20261031-run_lifecycle.md`.
    #[derive(Default)]
    pub enum TaskStatus {
        #[default]
        Pending = "pending",
        Running = "running",
        /// A dependency failed; returns to `Pending` once that dependency succeeds.
        Blocked = "blocked",
        Succeeded = "succeeded",
        Failed = "failed",
        Cancelled = "cancelled",
        /// The lead dropped the task with `cancel`.
        SkippedByDecision = "skipped_by_decision",
    }
}

impl TaskStatus {
    /// What the lead and the person read.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Pending => "等待开始",
            TaskStatus::Running => "执行中",
            TaskStatus::Succeeded => "已完成",
            TaskStatus::Failed => "失败",
            TaskStatus::Blocked => "依赖失败",
            TaskStatus::Cancelled => "已取消",
            TaskStatus::SkippedByDecision => "负责人已取消",
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
        matches!(self, TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Blocked | TaskStatus::SkippedByDecision)
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
                // The lead's retry (task.reopened) reopens failed and cancelled tasks, and a
                // finished one it sends back for rework, together with the verifications of it.
                | (Succeeded, Pending)
                | (Failed, Pending | Succeeded | SkippedByDecision)
                | (Cancelled, Pending | SkippedByDecision)
                | (SkippedByDecision, Pending)
        )
    }
}

wire_enum! {
    /// One ACP attempt, as the `attempts` table records it.
    #[derive(Default)]
    pub enum AttemptStatus {
        #[default]
        Starting = "starting",
        Running = "running",
        Succeeded = "succeeded",
        Failed = "failed",
        Cancelled = "cancelled",
        /// The orchestrator restarted while the attempt was open.
        Interrupted = "interrupted",
    }
}

wire_enum! {
    /// A person's action claimed by its `commandId`, as the `commands` table records it.
    pub enum CommandState {
        /// Claimed; the action is being carried out.
        Running = "running",
        /// The action took effect.
        Done = "done",
        /// The service stopped while the action was running: whether it took effect is not known.
        Interrupted = "interrupted",
    }
}

wire_enum! {
    /// What an agent's session is doing, as `agent.status` events report it. The app maps unknown
    /// values to `idle`, but this is the complete set Janus emits.
    #[derive(Default)]
    pub enum AgentPhase {
        #[default]
        Idle = "idle",
        Thinking = "thinking",
        Coordinating = "coordinating",
        ReadingFiles = "reading_files",
        Editing = "editing",
        RunningTool = "running_tool",
        RunningTest = "running_test",
        WaitingForInput = "waiting_for_input",
        WaitingForAgent = "waiting_for_agent",
        WaitingApproval = "waiting_approval",
        Error = "error",
    }
}

impl AgentPhase {
    pub fn label(self) -> &'static str {
        match self {
            AgentPhase::Idle => "空闲",
            AgentPhase::Thinking => "思考中",
            AgentPhase::Coordinating => "协调中",
            AgentPhase::ReadingFiles => "读取文件",
            AgentPhase::Editing => "修改文件",
            AgentPhase::RunningTool => "调用工具",
            AgentPhase::RunningTest => "运行测试",
            AgentPhase::WaitingForInput => "等待输入",
            AgentPhase::WaitingForAgent => "等待协作者",
            AgentPhase::WaitingApproval => "等待用户审批",
            AgentPhase::Error => "出错",
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
        assert!(RunStatus::Running.can_transition_to(RunStatus::Delivered));
        assert!(RunStatus::Delivered.can_transition_to(RunStatus::Accepted));
        assert!(RunStatus::Delivered.can_transition_to(RunStatus::Running), "a retry reopens delivery");
        assert!(!RunStatus::Cancelled.can_transition_to(RunStatus::Running));
        assert!(RunStatus::Accepted.can_transition_to(RunStatus::Delivered), "a new goal delivers again");
        assert!(RunStatus::Accepted.can_transition_to(RunStatus::Running), "new work reopens it");
        assert!(!RunStatus::Planning.can_transition_to(RunStatus::Accepted));
        // Only a delivered run is accepted.
        assert!(!RunStatus::Running.can_transition_to(RunStatus::Accepted));
        assert!(
            [RunStatus::Cancelled, RunStatus::Interrupted, RunStatus::Failed]
                .iter()
                .all(|to| !RunStatus::Accepted.can_transition_to(*to))
        );
        assert!(RunStatus::Running.can_transition_to(RunStatus::Running));
        // Only these states still have work that could write to the run.
        assert!(RunStatus::Planning.is_active() && RunStatus::Running.is_active());
        assert!(
            ![
                RunStatus::Delivered,
                RunStatus::Accepted,
                RunStatus::Cancelled,
                RunStatus::Failed,
                RunStatus::Interrupted,
            ]
            .iter()
            .any(|s| s.is_active())
        );
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
        assert!(TaskStatus::Succeeded.can_transition_to(TaskStatus::Pending), "the lead sends it back for rework");
        assert!(!TaskStatus::Succeeded.can_transition_to(TaskStatus::Running));
        assert!(!TaskStatus::Succeeded.can_transition_to(TaskStatus::Failed));
        assert!(!TaskStatus::Pending.can_transition_to(TaskStatus::Succeeded));
        assert!(!TaskStatus::Blocked.terminal());
        assert!(TaskStatus::Failed.terminal());
        assert!(TaskStatus::Failed.failed_dependency());
    }
}
