//! The states the orchestrator moves through, as types instead of strings.
//!
//! Every value here is stored and sent as the same snake_case string the database, the events
//! and the app already use (`RunStatus::Running.as_str() == "running"`), written once in its
//! `wire_enum!`. This module changes where a state is allowed to change, not the wire format. The
//! allowed moves are stated once, in `can_transition_to`; a call that does not fit is a bug in the
//! caller.

use crate::lang::Lang;
use crate::wire::wire_enum;

wire_enum! {
    /// The lifecycle of one Run, as `runs.status` and the app's run list see it.
    ///
    /// `paused` and `cancelled` are not separate values: a paused Run keeps `Running` (running
    /// attempts finish), and cancelling sets `Cancelled`.
    #[derive(Default)]
    pub enum RunStatus {
        /// Waiting in the dispatch queue for a free slot (`orchestrator/queue.rs`): nothing of it
        /// has started, no worktree, no team snapshot, no session.
        Queued = "queued",
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
    /// What a Run is: a goal the lead pursues with its team, a conversation with one digital
    /// employee, a workspace Loop's session, or a consolidation of a workspace's team memory
    /// (`runs.kind`). A chat has one member, no board, no goal and no delivery; it works in its own
    /// scratch directory under Janus home. A Loop's session is the workspace lead alone, in the
    /// workspace's directory, waking on a rhythm. A consolidation is the lead alone too, in its
    /// own directory, with the memory tools only.
    // @project-doc docs/domains/20261120-chats.md#chat_model
    // @project-doc docs/domains/20261121-loops.md#loop_model
    #[derive(Default)]
    pub enum RunKind {
        #[default]
        Goal = "goal",
        Chat = "chat",
        Loop = "loop",
        /// The workspace lead consolidating the workspace's team memory (`orchestrator/memory.rs`).
        Dream = "dream",
    }
}

wire_enum! {
    /// Who asked for a goal run (`runs.origin`): the person on a workspace's home, or a Loop that
    /// dispatched it (then `runs.loop_id` names the Loop).
    #[derive(Default)]
    pub enum RunOrigin {
        #[default]
        User = "user",
        Loop = "loop",
    }
}

/// What differs between the kinds of run. Code that would branch on a run's kind asks here, so a
/// new kind is one more arm in each of these, not another check in every module.
impl RunKind {
    /// One member in the run's main session, no board and no delivery: a chat, a Loop's session
    /// or a consolidation.
    pub fn single_session(self) -> bool {
        self != RunKind::Goal
    }

    /// The folder its files sit in, in every category of Janus home (`data/<dir>/<id>`, ...).
    pub fn dir(self) -> &'static str {
        match self {
            RunKind::Goal => "runs",
            RunKind::Chat => "chats",
            RunKind::Loop => "loops",
            RunKind::Dream => "dreams",
        }
    }

    /// The log of its main session's agent process.
    pub fn conversation_log(self) -> &'static str {
        match self {
            RunKind::Goal => "lead-chat.log",
            RunKind::Chat => "chat.log",
            RunKind::Loop => "loop.log",
            RunKind::Dream => "dream.log",
        }
    }

    /// Whether a question its session asked stays open after the session ends: a Loop does not
    /// wait for the answer, which wakes it whenever it comes.
    pub fn questions_outlive_session(self) -> bool {
        self == RunKind::Loop
    }

    /// Whether the person stops and pauses it on the run page. A Loop's session is paused on its
    /// Loop and goes when the Loop is deleted.
    pub fn stopped_on_run_page(self) -> bool {
        self != RunKind::Loop
    }

    /// Whether images can be pasted into it (a consolidation takes no messages).
    pub fn takes_images(self) -> bool {
        self != RunKind::Dream
    }

    /// Whether its main session's agent may close while nothing happens in it, given whether a
    /// goal run still needs its lead (its goal not ended, or its team at work).
    pub fn parks_when_idle(self, lead_needed: bool) -> bool {
        self.single_session() || !lead_needed
    }

    /// Whether a run that is not live can be brought back after it ended `previous`, or why not.
    pub fn resumable(self, previous: RunStatus) -> Result<bool, &'static str> {
        match self {
            RunKind::Chat | RunKind::Loop => Ok(previous != RunStatus::Cancelled),
            // A consolidation is one turn; a new one starts from the workspace's page.
            RunKind::Dream => Err("整理团队记忆只有一轮，结束后不能继续；可以在工作区页重新整理"),
            RunKind::Goal => Ok(matches!(
                previous,
                RunStatus::Interrupted | RunStatus::Failed | RunStatus::Delivered | RunStatus::Accepted
            )),
        }
    }

    /// How the record names what Janus tells its main session about working with it, when that
    /// differs from a goal run's (`None`: the Agent Team's rules).
    pub fn collaboration_label(self) -> Option<&'static str> {
        match self {
            RunKind::Loop => Some("向值班注入值班的工作方式"),
            RunKind::Dream => Some("向负责人注入整理团队记忆的做法"),
            RunKind::Goal | RunKind::Chat => None,
        }
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
            // A queued run starts, is cancelled, or cannot start (its workspace is gone).
            (Queued, Planning | Cancelled | Failed)
                // One that never got set up goes back on the queue: the service stopped while it
                // was being set up, or it could not start and the person continues it.
                | (Planning | Failed, Queued)
                | (Planning, Running | Delivered | Failed | Cancelled | Interrupted)
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
    /// What the person reads.
    pub fn label(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// What an agent reads, in the run's language.
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            TaskStatus::Pending => lang.pick("等待开始", "pending"),
            TaskStatus::Running => lang.pick("执行中", "running"),
            TaskStatus::Succeeded => lang.pick("已完成", "done"),
            TaskStatus::Failed => lang.pick("失败", "failed"),
            TaskStatus::Blocked => lang.pick("依赖失败", "blocked by a failed dependency"),
            TaskStatus::Cancelled => lang.pick("已取消", "cancelled"),
            TaskStatus::SkippedByDecision => lang.pick("负责人已取消", "cancelled by the lead"),
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
        /// Every seat for an agent process is taken (`seats`).
        WaitingForSeat = "waiting_for_seat",
        Error = "error",
    }
}

impl AgentPhase {
    /// What an agent reads, in the run's language.
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self {
            AgentPhase::Idle => lang.pick("空闲", "idle"),
            AgentPhase::Thinking => lang.pick("思考中", "thinking"),
            AgentPhase::Coordinating => lang.pick("协调中", "coordinating"),
            AgentPhase::ReadingFiles => lang.pick("读取文件", "reading files"),
            AgentPhase::Editing => lang.pick("修改文件", "editing files"),
            AgentPhase::RunningTool => lang.pick("调用工具", "calling a tool"),
            AgentPhase::RunningTest => lang.pick("运行测试", "running tests"),
            AgentPhase::WaitingForInput => lang.pick("等待输入", "waiting for input"),
            AgentPhase::WaitingForAgent => lang.pick("等待协作者", "waiting for a collaborator"),
            AgentPhase::WaitingApproval => lang.pick("等待用户审批", "waiting for the user's approval"),
            AgentPhase::WaitingForSeat => lang.pick("等空位", "waiting for a free seat"),
            AgentPhase::Error => lang.pick("出错", "error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_keeps_its_wire_name() {
        for (s, name) in [
            (RunStatus::Queued, "queued"),
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
        assert!(RunStatus::Queued.can_transition_to(RunStatus::Planning));
        assert!(!RunStatus::Queued.can_transition_to(RunStatus::Running), "a queued run starts by planning");
        assert!(!RunStatus::Queued.is_active(), "nothing of a queued run is working yet");
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
