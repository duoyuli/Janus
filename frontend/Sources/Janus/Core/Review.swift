import Foundation

// Reviewing a delivery: where the run stands after one (`ReviewStage`), what the latest delivery
// says (`Delivery`), what in it needs the person (`ReviewSignal`) and how strong each task's
// evidence is (the run view's `delivery`). The stage and the signals are the server's
// (docs/architecture/20261031-frontend.md#review).

/// How a delivery's result reached the person's branch (`run.delivered` `merge.state`).
enum MergeState: String {
    /// On the branch now.
    case merged
    /// Nothing new since the last merge-back.
    case nothing
    /// It conflicted with new commits on the branch; the lead resolves it in the run's worktree.
    case resolving
    /// Still conflicting after the lead tried once: the person hands it back by accepting.
    case conflict
    /// Something on the person's side is in the way (their uncommitted work, a rewritten branch).
    case blocked
    /// Not a Git repository: the run changed the person's folder directly.
    case inPlace

    /// The result is not on the branch, and the person decides what happens next.
    var waitsOnPerson: Bool { self == .conflict || self == .blocked }
}

/// The latest delivery as the review reads it.
struct Delivery {
    let result: RunResult
    var d: JSON { result.d }
    var n: Int { result.n }
    /// The lead's conclusion; empty when it left none (it is then the start of the report).
    var headline: String { d["headline"].str }
    var summary: String { d["summary"].str }
    /// Questions the lead leaves for the person.
    var decisions: [String] { d["decisions"].strings }
    var merge: MergeState? { MergeState(rawValue: d["merge"]["state"].str) }
    var mergeNote: String { d["merge"]["note"].str }
    /// The files this round changed.
    var changed: [String] { d["changed"].strings }
    /// The files the lead changed itself this round: no subtask's acceptance covers them.
    var leadEdits: [String] { d["leadEdits"].strings }
    /// What Janus found: unfinished tasks, independent verifications that did not confirm a task,
    /// library tools the lead called after delegating; each with the task it is about, if any.
    var risks: [(task: String?, text: String)] { d["risks"].array.map { ($0["task"].string, $0["text"].str) }.filter { !$0.text.isEmpty } }
    var evidence: [JSON] { d["evidence"].array }
    var deliverables: [JSON] { d["deliverables"].array }

    init(_ result: RunResult) { self.result = result }
}

/// Where a run stands after a delivery: the delivery is reviewed in full only while it waits for
/// the person (`awaiting`, `blocked`); otherwise the page says who it waits on now.
// @project-doc docs/architecture/20261031-frontend.md#review
enum ReviewStage: Equatable {
    /// S1: the latest delivery waits for acceptance.
    case awaiting
    /// S2: the merge conflicted; the lead resolves it, then delivers again.
    case resolving
    /// S3: the result is not on the branch and the person decides: hand the conflict back to the
    /// lead (`conflict`), or clear what is in the way on their side.
    case blocked(conflict: Bool)
    /// S4: the lead works on what the person asked after the delivery.
    case leadWorking
    /// S5: subtasks the lead handed out after the delivery are at work.
    case teamWorking
    /// S6: accepted.
    case accepted
    /// S7: something waits for the person's answer (an approval, a form, a question).
    case asking
    /// S8: the person interrupted the lead and holds it: go on, or let the turn's work stand.
    case held
    /// The run stopped after its delivery (interrupted, failed).
    case stopped

    /// The full review with its actions; the other stages fold the delivery to one line.
    var reviews: Bool { if case .awaiting = self { return true }; if case .blocked = self { return true }; return false }
}

/// The latest delivery's review as the server works it out (`backend/src/run_view/review.rs`):
/// its stage, what the person should look at, and what they asked for in its round.
struct ServedReview {
    let stage: ReviewStage
    let signals: [ReviewSignal]
    /// What the person asked the lead for in this round; nothing for the first delivery.
    let requests: [(seq: Int, text: String)]

    init(_ j: JSON) {
        stage = ReviewStage(served: j["stage"].str, conflict: j["conflict"].bool ?? false)
        signals = j["signals"].array.map(ReviewSignal.init)
        requests = j["requests"].array.map { (seq: $0["seq"].int ?? 0, text: $0["text"].str) }
    }
}

extension ReviewStage {
    init(served: String, conflict: Bool) {
        switch served {
        case "awaiting": self = .awaiting
        case "resolving": self = .resolving
        case "blocked": self = .blocked(conflict: conflict)
        case "lead_working": self = .leadWorking
        case "team_working": self = .teamWorking
        case "accepted": self = .accepted
        case "asking": self = .asking
        case "held": self = .held
        default: self = .stopped
        }
    }
}

/// One thing in a delivery the person should look at before accepting, and who says so.
struct ReviewSignal: Equatable, Identifiable {
    enum Source: Equatable { case lead, janus }
    let source: Source
    let text: String
    /// Where to look, or what the lead says about it.
    var detail = ""
    /// The task it is about, to open.
    var task: String?
    var id: String { "\(source)-\(text)" }

    init(_ j: JSON) {
        source = j["source"].str == "lead" ? .lead : .janus
        text = j["text"].str
        detail = j["detail"].str
        task = j["task"].string
    }
}
