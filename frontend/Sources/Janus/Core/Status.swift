import Foundation

/// A run's lifecycle state, as the server records it (`backend/src/status.rs`, `RunStatus`). The
/// groups the app acts on (still running, finished, can be continued, reopens as a conversation)
/// are stated here once instead of as string sets in each view.
enum RunStatus: String {
    case planning, running
    case waitingInput = "waiting_input"
    case delivered, accepted, cancelled, failed, interrupted
    /// Nothing read yet (before `run.created`), or a value this app does not know.
    case unknown

    init(_ raw: String) { self = RunStatus(rawValue: raw) ?? .unknown }

    /// The run is live on the server: work can still start.
    var isActive: Bool { [.planning, .running, .waitingInput].contains(self) }
    /// Nothing more happens unless the person acts.
    var isFinished: Bool { [.delivered, .accepted, .cancelled, .failed, .interrupted].contains(self) }
    /// The run stopped short and can be continued where it was.
    var canResume: Bool { self == .interrupted || self == .failed }
    /// The goal is done; the lead's session reopens as a conversation.
    var isDone: Bool { self == .delivered || self == .accepted }

    /// How the run lists name it.
    var listLabel: (String, Tone) {
        switch self {
        case .planning: return ("主控启动中", .blue)
        case .waitingInput: return ("等你追加预算", .amber)
        case .running: return ("执行中", .blue)
        case .delivered: return ("已交付", .green)
        case .accepted: return ("已验收", .green)
        case .failed: return ("失败", .red)
        case .cancelled: return ("已停止", .gray)
        case .interrupted: return ("已中断", .gray)
        case .unknown: return ("未知", .gray)
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
        case .pending: return ("等待依赖", .gray)
        case .ready: return ("就绪", .gray)
        case .assigned: return ("已分配", .blue)
        case .running: return ("执行中", .blue)
        case .waitingInput: return ("等你回答", .amber)
        case .waitingApproval: return ("等你审批", .amber)
        case .blocked: return ("依赖失败", .amber)
        case .skippedByDecision: return ("主控取消", .gray)
        case .verifying: return ("核对中", .violet)
        case .verificationFailed: return ("没通过验收", .red)
        case .succeeded: return ("已完成", .green)
        case .failed: return ("失败", .red)
        case .cancelled: return ("已取消", .gray)
        }
    }
}

/// One attempt as the app follows it: running until it fails or its task succeeds.
enum AttemptStatus: String {
    case running, succeeded, failed

    var label: (String, Tone) {
        switch self {
        case .running: return ("进行中", .blue)
        case .succeeded: return ("成功", .green)
        case .failed: return ("失败", .red)
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
