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
        // Digital employees a chat proposes (`propose_employees`) and what the person did with them
        RolesProposed = "roles.proposed",
        RolesCreated = "roles.created",
        RolesDeclined = "roles.declined",
    }
}

wire_enum! {
    /// The audit log's filters (besides 「全部」): which one an event shows under.
    pub enum AuditCategory {
        Tasks = "tasks",
        Messages = "messages",
        Artifacts = "artifacts",
        Decisions = "decisions",
        Progress = "progress",
    }
}

impl EventKind {
    /// What answers or withdraws a request (`data.key`): a question, an approval or a form.
    pub const SETTLES_REQUEST: [EventKind; 5] = [
        EventKind::QuestionAnswered,
        EventKind::ApprovalGranted,
        EventKind::ApprovalDenied,
        EventKind::InputAnswered,
        EventKind::DecisionWithdrawn,
    ];

    /// The audit log's filter that shows this kind (`GET /api/meta` gives every kind's, so the
    /// clients filter the log the same way).
    // @project-doc docs/architecture/20261031-frontend.md#run_page
    pub fn audit_category(self) -> AuditCategory {
        use EventKind::*;
        let family = self.as_str().split('.').next().unwrap_or("");
        if matches!(
            self,
            QuestionAnswered
                | InputAnswered
                | ApprovalGranted
                | ApprovalDenied
                | RiskAccepted
                | RunMerged
                | RunPaused
                | RunResumed
                | RunCancelled
        ) {
            AuditCategory::Decisions
        } else if matches!(family, "question" | "answer" | "control" | "session")
            || matches!(self, ApprovalRequested | InputRequested | UserMessage | MessageDelivered)
        {
            AuditCategory::Messages
        } else if self == AgentMessage {
            AuditCategory::Progress
        } else if family == "artifact" || self == ContextHandoff {
            AuditCategory::Artifacts
        } else if family == "tool" || self == AgentStatus {
            AuditCategory::Progress
        } else {
            AuditCategory::Tasks
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kind shows under the filter the audit log has always put it under.
    #[test]
    fn audit_categories() {
        let cases = [
            ("question.answered", "decisions"),
            ("input.answered", "decisions"),
            ("approval.granted", "decisions"),
            ("approval.denied", "decisions"),
            ("risk.accepted", "decisions"),
            ("run.merged", "decisions"),
            ("run.paused", "decisions"),
            ("run.resumed", "decisions"),
            ("run.cancelled", "decisions"),
            ("question.asked", "messages"),
            ("question.escalated", "messages"),
            ("approval.requested", "messages"),
            ("input.requested", "messages"),
            ("answer.delivered", "messages"),
            ("user.message", "messages"),
            ("message.delivered", "messages"),
            ("control.taken", "messages"),
            ("session.prompt", "messages"),
            ("session.usage", "messages"),
            ("agent.message", "progress"),
            ("agent.status", "progress"),
            ("tool.call", "progress"),
            ("tool.completed", "progress"),
            ("artifact.created", "artifacts"),
            ("context.handoff", "artifacts"),
            ("task.succeeded", "tasks"),
            ("attempt.started", "tasks"),
            ("run.created", "tasks"),
            ("message.routed", "tasks"),
            ("run.delivered", "tasks"),
        ];
        for (kind, category) in cases {
            let k = EventKind::parse(kind).unwrap_or_else(|| panic!("{kind} is a kind"));
            assert_eq!(k.audit_category().as_str(), category, "{kind}");
        }
    }
}
