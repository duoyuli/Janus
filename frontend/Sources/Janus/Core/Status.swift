import Foundation

/// A run's lifecycle state, as the server records it (`backend/src/status.rs`, `RunStatus`). The
/// groups the app acts on (still running, finished, can be continued, reopens as a conversation)
/// are stated here once instead of as string sets in each view.
enum RunStatus: String {
    /// Waiting in the dispatch queue for a free slot; nothing of it has started.
    case queued
    case planning, running
    case delivered, accepted, cancelled, failed, interrupted
    /// Nothing read yet (before `run.created`), or a value this app does not know.
    case unknown

    init(_ raw: String) { self = RunStatus(rawValue: raw) ?? .unknown }

    /// The run is live on the server: work can still start.
    var isActive: Bool { self == .planning || self == .running }
    /// Nothing more happens unless the person acts.
    var isFinished: Bool { [.delivered, .accepted, .cancelled, .failed, .interrupted].contains(self) }
    /// The run stopped short and can be continued where it was.
    var canResume: Bool { self == .interrupted || self == .failed }
    /// The goal is done; the lead's session reopens as a conversation.
    var isDone: Bool { self == .delivered || self == .accepted }

    /// How the run lists name it.
    var listLabel: (String, Tone) {
        switch self {
        case .queued: return (L.text("排队中"), .gray)
        case .planning: return (L.text("负责人上岗中"), .blue)
        case .running: return (L.text("干活中"), .blue)
        case .delivered: return (L.text("已交付"), .green)
        case .accepted: return (L.text("已验收"), .green)
        case .failed: return (L.text("失败"), .red)
        case .cancelled: return (L.text("已停止"), .gray)
        case .interrupted: return (L.text("已中断"), .gray)
        case .unknown: return (L.text("未知"), .gray)
        }
    }
}

/// A task as the app shows it: the scheduler's states (`backend/src/status.rs`, `TaskStatus`)
/// and the steps the app reads from events on the way (ready, assigned, verifying, waiting on
/// the person).
enum TaskStatus: String {
    case pending, ready, assigned, running, verifying
    case verificationFailed = "verification_failed"
    case waitingInput = "waiting_input"
    case waitingApproval = "waiting_approval"
    case blocked, succeeded, failed, cancelled
    case skippedByDecision = "skipped_by_decision"

    /// A status the server sent (a delivery's acceptance record); an unknown one reads as pending.
    init(_ raw: String) { self = TaskStatus(rawValue: raw) ?? .pending }

    /// No more work happens on it without a retry.
    var isEnded: Bool { [.succeeded, .failed, .cancelled, .skippedByDecision].contains(self) }
    /// Its attempt waits for the person to answer or approve.
    var waitsOnPerson: Bool { self == .waitingInput || self == .waitingApproval }
    /// Not started yet: waiting for its dependencies or the scheduler.
    var isWaitingToStart: Bool { [.pending, .ready, .assigned, .blocked].contains(self) }
    /// Its agent is on it: assigned, working, or waiting on the person in its session.
    var isBusy: Bool { [.assigned, .running, .waitingInput, .waitingApproval].contains(self) }

    var label: (String, Tone) {
        switch self {
        case .pending: return (L.text("等上游"), .gray)
        case .ready: return (L.text("待开工"), .gray)
        case .assigned: return (L.text("已派活"), .blue)
        case .running: return (L.text("干活中"), .blue)
        case .waitingInput: return (L.text("等你回答"), .amber)
        case .waitingApproval: return (L.text("等你审批"), .amber)
        case .blocked: return (L.text("上游出错"), .amber)
        case .skippedByDecision: return (L.text("负责人取消"), .gray)
        case .verifying: return (L.text("核对中"), .violet)
        case .verificationFailed: return (L.text("验收没通过"), .red)
        case .succeeded: return (L.text("已完成"), .green)
        case .failed: return (L.text("失败"), .red)
        case .cancelled: return (L.text("已取消"), .gray)
        }
    }
}

/// One attempt as the app follows it: running until it fails or its task succeeds.
enum AttemptStatus: String {
    case running, succeeded, failed

    var label: (String, Tone) {
        switch self {
        case .running: return (L.text("进行中"), .blue)
        case .succeeded: return (L.text("成功"), .green)
        case .failed: return (L.text("失败"), .red)
        }
    }

    /// What an ended attempt shows as on the timeline, in the task's terms.
    var asTask: TaskStatus {
        switch self {
        case .running: return .running
        case .succeeded: return .succeeded
        case .failed: return .failed
        }
    }
}

/// What a run is (`backend/src/status.rs`, `RunKind`, `runs.kind`): a goal the lead pursues
/// with its team, a chat with one digital employee, a workspace Loop's session, or a team memory
/// consolidation. A value this app does not know reads as a goal.
enum RunKind: String, CaseIterable {
    case goal, chat, loop, dream

    init(_ raw: String) { self = RunKind(rawValue: raw) ?? .goal }
}

/// Who asked for a goal run (`backend/src/status.rs`, `RunOrigin`, `runs.origin`): the person,
/// or a Loop that dispatched it.
enum RunOrigin: String, CaseIterable {
    case user, loop

    init(_ raw: String) { self = RunOrigin(rawValue: raw) ?? .user }
}

/// What an agent's session is doing (`backend/src/status.rs`, `AgentPhase`): what its
/// `agent.status` events say, and what the app reads from its tool calls on the way. A value this
/// app does not know reads as idle.
enum AgentPhase: String, CaseIterable {
    case idle, thinking, coordinating
    case readingFiles = "reading_files"
    case editing
    case runningTool = "running_tool"
    case runningTest = "running_test"
    case waitingForInput = "waiting_for_input"
    case waitingForAgent = "waiting_for_agent"
    case waitingApproval = "waiting_approval"
    case waitingForSeat = "waiting_for_seat"
    case error

    init(_ raw: String) { self = AgentPhase(rawValue: raw) ?? .idle }

    var label: (String, Tone) {
        switch self {
        case .idle: return (L.text("摸鱼中"), .gray)
        case .thinking: return (L.text("思考中"), .blue)
        case .coordinating: return (L.text("协调中"), .blue)
        case .readingFiles: return (L.text("翻文件"), .blue)
        case .editing: return (L.text("改文件"), .blue)
        case .runningTool: return (L.text("用工具"), .blue)
        case .runningTest: return (L.text("跑测试"), .blue)
        case .waitingForInput: return (L.text("等你输入"), .amber)
        case .waitingForAgent: return (L.text("等队友"), .amber)
        case .waitingApproval: return (L.text("等你审批"), .amber)
        case .waitingForSeat: return (L.text("等空位"), .amber)
        case .error: return (L.text("出错了"), .red)
        }
    }
}

/// Why Janus put something into a session (`session.prompt`, `data.channel`): one of the system
/// layer's parts (`backend/src/orchestrator/lead.rs`, `SystemPart` and the digital employee's
/// instructions), or a prompt it sent (`PromptChannel`).
enum PromptChannel: String, CaseIterable {
    /// The system layer: how to work with Janus and the team, the digital employee's own
    /// instructions, and a reopened chat's recent exchanges.
    case system, instructions, history
    case goal, followup, board
    case goalCheck = "goal_check"
    case resume, fix

    /// Part of the session's system layer rather than a prompt of a turn.
    var injected: Bool { [.system, .instructions, .history].contains(self) }
}

/// How a sent message went into a session (`message.routed` / `user.message`, `data.delivery`;
/// `backend/src/orchestrator/messages.rs`, `Delivery`), and `pending` before the server said.
enum MessageDelivery: String, CaseIterable {
    case pending, steer, queued, interrupt, prompt, next

    init?(_ raw: String) { self.init(rawValue: raw) }


    /// The tone of the tray's badge for a message still on its way (the server words the badge).
    var trayTone: Tone {
        switch self {
        case .pending, .interrupt: return .blue
        case .queued: return .amber
        case .next, .steer, .prompt: return .gray
        }
    }
}

/// One of the tools Janus's own `janus` server gives a session (`backend/src/orchestrator`: the
/// lead's `LeadTool`, a collaborator's `MemberTool`, a Loop's `LoopTool`, the team memory's
/// `MemoryTool` and a chat's `ChatTool`). Its calls are recorded as `janus.<name>`.
enum JanusTool: String, CaseIterable {
    case board, orchestrate, wait, reply, retry, amend, cancel
    case askUser = "ask_user"
    case boardPost = "board_post"
    case submit, ask
    case dispatch
    case nextWake = "next_wake"
    case recall, remember, forget
    case proposeEmployees = "propose_employees"

    /// The tool a recorded call names (`data.tool`), when it is one of Janus's own.
    init?(call: String) {
        guard call.hasPrefix("janus.") else { return nil }
        self.init(rawValue: String(call.dropFirst("janus.".count)))
    }
}
