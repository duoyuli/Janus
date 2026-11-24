import Foundation

/// What happened, as the event log names it: the same kinds as the server's `EventKind`
/// (`backend/src/events.rs`, docs/interfaces/20261031-events.md#event_types). A run recorded by an
/// earlier Janus may hold kinds this one no longer knows; they read as `.unknown`.
enum EventKind: String {
    // Run
    case runQueued = "run.queued"
    case runStarting = "run.starting"
    case runRequeued = "run.requeued"
    case runCreated = "run.created"
    case runDelivered = "run.delivered"
    case riskAccepted = "risk.accepted"
    case runMerged = "run.merged"
    case runFailed = "run.failed"
    case runCancelled = "run.cancelled"
    case runInterrupted = "run.interrupted"
    case runRecovered = "run.recovered"
    case runPaused = "run.paused"
    case runResumed = "run.resumed"
    // Workspace Loops
    case loopWoke = "loop.woke"
    case loopSlept = "loop.slept"
    case loopScheduled = "loop.scheduled"
    case loopNextWake = "loop.next_wake"
    case loopDispatchEnded = "loop.dispatch_ended"
    case loopMoved = "loop.moved"
    case loopNamed = "loop.named"
    case loopStateSaved = "loop.state_saved"
    case loopTouched = "loop.touched"
    case loopFailed = "loop.failed"
    case loopPaused = "loop.paused"
    case loopResumed = "loop.resumed"
    // Team memory
    case memorySaved = "memory.saved"
    case memoryForgotten = "memory.forgotten"
    case memoryConsolidated = "memory.consolidated"
    // The lead and its goal
    case goalStatus = "goal.status"
    case leadFailed = "lead.failed"
    case skillUsed = "skill.used"
    case mcpRouted = "mcp.routed"
    case leadRouted = "lead.routed"
    case leadEdited = "lead.edited"
    case leadStalled = "lead.stalled"
    case orchestrationAccepted = "orchestration.accepted"
    case orchestrationRejected = "orchestration.rejected"
    case boardPosted = "board.posted"
    // Tasks and attempts
    case taskAssigned = "task.assigned"
    case taskReady = "task.ready"
    case attemptLaunchRequested = "attempt.launch_requested"
    case attemptStarted = "attempt.started"
    case contextHandoff = "context.handoff"
    case resultAmended = "result.amended"
    case resultSubmitted = "result.submitted"
    case artifactCreated = "artifact.created"
    case attemptFailed = "attempt.failed"
    case attemptLateResult = "attempt.late_result"
    case taskRetryScheduled = "task.retry_scheduled"
    case taskSucceeded = "task.succeeded"
    case taskFailed = "task.failed"
    case taskBlocked = "task.blocked"
    case taskStalled = "task.stalled"
    case taskConflict = "task.conflict"
    case taskClaimed = "task.claimed"
    case taskSplitRequested = "task.split_requested"
    case taskSkippedByDecision = "task.skipped_by_decision"
    case taskReopened = "task.reopened"
    // Verification
    case verificationStarted = "verification.started"
    case verificationPassed = "verification.passed"
    case verificationFailed = "verification.failed"
    // Sessions
    case agentStatus = "agent.status"
    case agentMessage = "agent.message"
    case sessionPrompt = "session.prompt"
    case toolCall = "tool.call"
    case toolStarted = "tool.started"
    case agentThought = "agent.thought"
    case agentPlan = "agent.plan"
    case sessionControls = "session.controls"
    case sessionControl = "session.control"
    case sessionCommands = "session.commands"
    case sessionUsage = "session.usage"
    case toolCompleted = "tool.completed"
    case userMessage = "user.message"
    case messageRouted = "message.routed"
    case messageDelivered = "message.delivered"
    case sessionParked = "session.parked"
    case sessionInterrupted = "session.interrupted"
    case controlTaken = "control.taken"
    case controlReleased = "control.released"
    // Requests that wait on someone
    case questionAsked = "question.asked"
    case questionEscalated = "question.escalated"
    case questionAnswered = "question.answered"
    case answerDelivered = "answer.delivered"
    case approvalRequested = "approval.requested"
    case approvalGranted = "approval.granted"
    case approvalDenied = "approval.denied"
    case inputRequested = "input.requested"
    case inputAnswered = "input.answered"
    case decisionWithdrawn = "decision.withdrawn"
    /// A kind this build does not know.
    case unknown

    init(wire: String) { self = EventKind(rawValue: wire) ?? .unknown }

    /// What the event is about, the part of its name before the dot (`question` of `question.asked`).
    var family: Substring { rawValue.prefix { $0 != "." } }
}
