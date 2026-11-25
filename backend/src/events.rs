//! What happened, as the event log names it (`events.type`). Every event Janus writes has one of
//! these kinds, and code that looks for an event asks by kind, never by spelling it out. The log
//! keeps the kind as text: a run recorded by an earlier Janus may hold kinds this one no longer
//! writes, and those read back as `EventKind::Unknown` from `Event::kind`.
// @project-doc docs/interfaces/20261031-events.md#event_types

use crate::wire::wire_enum;

wire_enum! {
    /// The kind of an event, grouped as the events document lists them.
    pub enum EventKind {
        /// A kind an earlier Janus wrote and this one no longer does, as `Event::kind` reads it
        /// back. Janus never writes it.
        Unknown = "unknown",
        // Run
        RunQueued = "run.queued",
        RunStarting = "run.starting",
        RunRequeued = "run.requeued",
        RunCreated = "run.created",
        RunDelivered = "run.delivered",
        RiskAccepted = "risk.accepted",
        RunMerged = "run.merged",
        RunFailed = "run.failed",
        RunCancelled = "run.cancelled",
        RunInterrupted = "run.interrupted",
        RunRecovered = "run.recovered",
        RunPaused = "run.paused",
        RunResumed = "run.resumed",
        // Workspace Loops
        LoopWoke = "loop.woke",
        LoopSlept = "loop.slept",
        LoopScheduled = "loop.scheduled",
        LoopNextWake = "loop.next_wake",
        LoopDispatchEnded = "loop.dispatch_ended",
        LoopMoved = "loop.moved",
        LoopNamed = "loop.named",
        LoopStateSaved = "loop.state_saved",
        LoopTouched = "loop.touched",
        LoopFailed = "loop.failed",
        LoopPaused = "loop.paused",
        LoopResumed = "loop.resumed",
        // Team memory
        MemorySaved = "memory.saved",
        MemoryForgotten = "memory.forgotten",
        MemoryConsolidated = "memory.consolidated",
        // The lead and its goal
        GoalStatus = "goal.status",
        LeadFailed = "lead.failed",
        SkillUsed = "skill.used",
        McpRouted = "mcp.routed",
        LeadRouted = "lead.routed",
        LeadEdited = "lead.edited",
        LeadStalled = "lead.stalled",
        OrchestrationAccepted = "orchestration.accepted",
        OrchestrationRejected = "orchestration.rejected",
        BoardPosted = "board.posted",
        // Tasks and attempts
        TaskAssigned = "task.assigned",
        TaskReady = "task.ready",
        AttemptLaunchRequested = "attempt.launch_requested",
        AttemptStarted = "attempt.started",
        ContextHandoff = "context.handoff",
        ResultAmended = "result.amended",
        ResultSubmitted = "result.submitted",
        ArtifactCreated = "artifact.created",
        AttemptFailed = "attempt.failed",
        AttemptLateResult = "attempt.late_result",
        TaskRetryScheduled = "task.retry_scheduled",
        TaskSucceeded = "task.succeeded",
        TaskFailed = "task.failed",
        TaskBlocked = "task.blocked",
        TaskStalled = "task.stalled",
        TaskConflict = "task.conflict",
        TaskClaimed = "task.claimed",
        TaskSplitRequested = "task.split_requested",
        TaskSkippedByDecision = "task.skipped_by_decision",
        TaskReopened = "task.reopened",
        // Verification
        VerificationStarted = "verification.started",
        VerificationPassed = "verification.passed",
        VerificationFailed = "verification.failed",
        // Sessions
        AgentStatus = "agent.status",
        AgentMessage = "agent.message",
        SessionPrompt = "session.prompt",
        ToolCall = "tool.call",
        ToolStarted = "tool.started",
        AgentThought = "agent.thought",
        AgentPlan = "agent.plan",
        SessionControls = "session.controls",
        SessionControl = "session.control",
        SessionCommands = "session.commands",
        SessionUsage = "session.usage",
        ToolCompleted = "tool.completed",
        UserMessage = "user.message",
        MessageRouted = "message.routed",
        MessageDelivered = "message.delivered",
        SessionParked = "session.parked",
        SessionInterrupted = "session.interrupted",
        ControlTaken = "control.taken",
        ControlReleased = "control.released",
        // Requests that wait on someone
        QuestionAsked = "question.asked",
        QuestionEscalated = "question.escalated",
        QuestionAnswered = "question.answered",
        AnswerDelivered = "answer.delivered",
        ApprovalRequested = "approval.requested",
        ApprovalGranted = "approval.granted",
        ApprovalDenied = "approval.denied",
        InputRequested = "input.requested",
        InputAnswered = "input.answered",
        DecisionWithdrawn = "decision.withdrawn",
    }
}

impl EventKind {
    /// An agent did something in the run: spoke, called a tool, handed in a result or put
    /// tasks on the board. Opening a session, Janus's own notes and usage readings are not work.
    pub fn is_agent_progress(self) -> bool {
        matches!(
            self,
            EventKind::AgentMessage
                | EventKind::ToolCall
                | EventKind::ResultSubmitted
                | EventKind::OrchestrationAccepted
                | EventKind::TaskSucceeded
        )
    }
}
