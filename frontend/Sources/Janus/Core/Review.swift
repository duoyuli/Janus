import Foundation

// Reviewing a delivery: where the run stands after one (`ReviewStage`), what the latest delivery
// says (`Delivery`), what in it needs the person (`ReviewSignal`) and how strong each task's
// evidence is (`EvidenceStrength`). All of it is read from the events; nothing here is stored
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
    /// library tools the lead called after delegating.
    var risks: [String] { d["risks"].strings }
    var evidence: [JSON] { d["evidence"].array }
    var deliverables: [JSON] { d["deliverables"].array }
    var acceptance: [DeliveryAcceptance] { d["acceptance"].array.compactMap(DeliveryAcceptance.init) }

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

/// The run's stage after its latest delivery; nil before the first one, and for a chat or a Loop.
/// `live`: the run is in the service's memory. Without it no lead session is there to work or to
/// be held, whatever the last events said (a restart cut its turn off).
func reviewStage(_ s: RunState, live: Bool) -> ReviewStage? {
    guard let r = s.result, !s.run.singleSession else { return nil }
    let asking = s.inbox.contains { ["question", "approval", "input"].contains($0.kind) || ($0.kind == "session" && $0.task != nil) }
    switch s.run.status {
    case .running, .planning: return asking ? .asking : .teamWorking
    case .delivered, .accepted: break
    default: return .stopped
    }
    if asking { return .asking }
    let merge = Delivery(r).merge
    if merge == .resolving { return .resolving }
    let turn = live ? s.leadTurn : .agent(idle: true)
    if turn.human { return turn.idle ? .held : .leadWorking }
    if !turn.idle { return .leadWorking }
    if s.run.status == .accepted { return .accepted }
    if let merge, merge.waitsOnPerson { return .blocked(conflict: merge == .conflict) }
    return .awaiting
}

/// What the person asked the lead for between the delivery before `r` and `r`: the requests
/// this round answers. Nothing for the first delivery, which answers the goal.
func roundRequests(_ s: RunState, _ r: RunResult) -> [(seq: Int, text: String)] {
    guard r.n > 1 else { return [] }
    return s.events.filter { e in
        e.type == "user.message" && e.actor == "user" && e.to == "lead" && e.task == nil
            && e.seq > r.since && e.seq < r.seq && e.data["id"].exists
    }.map { (seq: $0.seq, text: Attach.visibleText($0.data["text"].str)) }
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
}

/// What the person should look at, in order: the lead's questions first, then what Janus found.
/// The executors' own risks are not here: there is no bound on how many they report; the lead
/// picks the ones that matter into its questions, and each stays with its task.
func reviewSignals(_ d: Delivery) -> [ReviewSignal] {
    var out = d.decisions.map { ReviewSignal(source: .lead, text: $0) }
    out += d.risks.map { r in
        // Janus writes 「T1：…」 whatever the app's language.
        let parts = r.split(separator: "：", maxSplits: 1).map(String.init)
        return ReviewSignal(source: .janus, text: r, task: parts.count == 2 && !parts[0].contains(" ") ? parts[0] : nil)
    }
    let records = d.acceptance
    for a in records {
        let failed = a.summary.criteria - a.summary.passed
        if failed > 0 || a.checks.contains(where: { !$0.passed }) {
            out.append(ReviewSignal(source: .janus, text: L.text("\(a.task) 有 \(max(failed, 1)) 条验收标准没通过"), task: a.task))
        }
        if !a.traceGaps.isEmpty {
            out.append(ReviewSignal(source: .janus, text: L.text("\(a.task) 的结果和工作目录对不上 \(a.traceGaps.count) 处"), detail: a.traceGaps.joined(separator: L.text("；")), task: a.task))
        }
    }
    let unverified = records.filter(\.unverified).map(\.task)
    if !unverified.isEmpty {
        out.append(ReviewSignal(source: .janus, text: L.text("\(unverified.joined(separator: L.text("、"))) 改了文件，没有独立验证"),
                                detail: L.text("负责人要在总结里写明怎么核对的"), task: unverified.count == 1 ? unverified[0] : nil))
    }
    if !d.leadEdits.isEmpty {
        out.append(ReviewSignal(source: .janus, text: L.text("负责人自己改了 \(d.leadEdits.count) 个文件，没有子任务验收和独立验证"),
                                detail: d.leadEdits.joined(separator: L.text("、"))))
    }
    return out
}

/// How a task's checks were verified, in three bands: run (tests, commands), read (inspection),
/// or only said (reasoning, unverified, no method or an illegal one).
struct EvidenceStrength: Equatable {
    var ran = 0
    var read = 0
    var said = 0
    var total: Int { ran + read + said }

    init(_ s: EvidenceSummary) {
        for m in s.methods {
            switch EvidenceMethod(rawValue: m.method) {
            case .test?, .command?: ran += m.count
            case .inspection?: read += m.count
            default: said += m.count
            }
        }
        said += s.unspecified
    }

    /// 「运行 2 · 检查 1 · 只是说法 1」
    var text: String {
        [(ran, L.text("运行 \(ran)")), (read, L.text("检查 \(read)")), (said, L.text("只是说法 \(said)"))]
            .filter { $0.0 > 0 }.map(\.1).joined(separator: " · ")
    }
}
