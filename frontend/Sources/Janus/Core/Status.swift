import Foundation

/// A run's lifecycle state, as the server records it (`backend/src/status.rs`, `RunStatus`). The
/// groups the app acts on (still running, finished, can be continued, reopens as a conversation)
/// are stated here once instead of as string sets in each view.
enum RunStatus: String {
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
        case .planning: return (L.text("负责人启动中"), .blue)
        case .running: return (L.text("执行中"), .blue)
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
        case .pending: return (L.text("等待依赖"), .gray)
        case .ready: return (L.text("就绪"), .gray)
        case .assigned: return (L.text("已分配"), .blue)
        case .running: return (L.text("执行中"), .blue)
        case .waitingInput: return (L.text("等你回答"), .amber)
        case .waitingApproval: return (L.text("等你审批"), .amber)
        case .blocked: return (L.text("依赖失败"), .amber)
        case .skippedByDecision: return (L.text("负责人取消"), .gray)
        case .verifying: return (L.text("核对中"), .violet)
        case .verificationFailed: return (L.text("没通过验收"), .red)
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
