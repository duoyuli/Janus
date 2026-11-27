import Foundation

// Acceptance evidence as the server groups it (`evidence` and `delivery` of the run's view,
// `backend/src/run_view/evidence.rs`): each task's checks against its criteria with the method
// counts and reminders, its independent verifications, and the delivery's per-task rows. The app
// decodes and draws them; whether a task passed stays with `task.succeeded` and `verification.*`
// (docs/architecture/20261031-frontend.md#evidence_methods).

/// A criterion with the check that answered it, as the server writes it (`CheckLine`,
/// `backend/src/run_view/checks.rs`): how it came out, how it was checked (a word, a colour, a
/// symbol and what it means), the evidence and whether it folds, and the whole line read out.
struct CheckView: Equatable {
    enum Mark: String { case passed, failed, unchecked }
    struct Method: Equatable { let text: String; let tone: Tone; let icon: String; let help: String }

    let mark: Mark
    let method: Method
    let criterion: String
    let evidence: String
    let noEvidence: String?
    let folds: Bool
    let voice: String

    init(_ j: JSON) {
        mark = Mark(rawValue: j["mark"].str) ?? .unchecked
        let m = j["method"]
        method = Method(text: m["text"].str, tone: Tone(rawValue: m["tone"].str) ?? .gray, icon: m["icon"].str, help: m["help"].str)
        criterion = j["criterion"].str
        evidence = j["evidence"].str
        noEvidence = j["noEvidence"].string
        folds = j["folds"].bool ?? false
        voice = j["voice"].str
    }

    static func list(_ j: JSON) -> [CheckView] { j.array.map(CheckView.init) }
}

/// A criterion with the check that answered it, as the server pairs them: every criterion has a
/// row and every check is kept.
struct CriterionCheck: Identifiable {
    let n: Int
    let line: CheckView
    var id: Int { n }

    init(_ j: JSON) {
        n = j["n"].int ?? 0
        line = CheckView(j["line"])
    }
}

/// An independent verifier's judgment on the target task (`verdict` in `evidence.rs`).
enum Verdict: String, CaseIterable {
    case approved, rejected, inconclusive

    var label: String {
        switch self {
        case .approved: return L.text("通过")
        case .rejected: return L.text("需要修改")
        case .inconclusive: return L.text("证据不足")
        }
    }
}

/// One independent verification of a task: a task whose `verifies` names it, with its own result,
/// as the server gives it (now, or as the delivery recorded it).
struct VerificationRecord: Identifiable {
    let task: String
    let label: String
    let verdict: Verdict?
    let passed: Bool
    let summary: String
    let checks: [CheckView]
    let evidence: [String]
    let risks: [String]
    var id: String { task }

    init(_ j: JSON) {
        task = j["task"].str
        label = TaskStatus(rawValue: j["status"].str)?.label.0 ?? j["label"].str
        verdict = j["verdict"].string.flatMap(Verdict.init(rawValue:))
        passed = j["passed"].bool ?? false
        summary = j["summary"].str
        checks = CheckView.list(j["lines"])
        evidence = j["evidence"].strings
        risks = j["risks"].strings
    }
}

/// One task's evidence, as its page shows it.
struct TaskEvidence {
    var criteria = 0
    var passed = 0
    /// 「测试 2 条、命令 1 条、未说明 1 条」.
    var counts = ""
    var reminders: [String] = []
    var rows: [CriterionCheck] = []
    var verifications: [VerificationRecord] = []

    init() {}

    init(_ j: JSON) {
        criteria = j["criteria"].int ?? 0
        passed = j["passed"].int ?? 0
        counts = j["counts"].str
        reminders = j["reminders"].strings
        rows = j["rows"].array.map(CriterionCheck.init)
        verifications = j["verifications"].array.map(VerificationRecord.init)
    }
}

/// How a task's checks were verified, in three bands: run (tests, commands), read (inspection),
/// or only said; and the same in words.
struct EvidenceStrength: Equatable {
    var ran = 0
    var read = 0
    var said = 0
    var text = ""
    var total: Int { ran + read + said }

    init(_ j: JSON) {
        ran = j["ran"].int ?? 0
        read = j["read"].int ?? 0
        said = j["said"].int ?? 0
        text = j["text"].str
    }
}

/// One task's acceptance in the delivery under review (W1 F6): what the record says, its numbers
/// in words, the markers that need attention and whether the row starts open, as the server works
/// them out.
struct DeliveryAcceptance: Identifiable {
    let task: String
    let title: String
    let status: TaskStatus
    let passed: Int
    let criteria: Int
    /// 「测试 1 条、命令 2 条、推理判断 1 条」.
    let counts: String
    let reminders: [String]
    let strength: EvidenceStrength
    /// 「通过 4 / 5 条 · 测试 1 条、命令 2 条、推理判断 1 条」.
    let metrics: String
    /// Inline text, not colour: the status when it is not success, and risks, trace gaps and
    /// reminders when there are any.
    let markers: [String]
    /// Starts open when a criterion failed, a trace gap exists, or an independent verification did
    /// not pass.
    let needsAttention: Bool
    let checks: [CheckView]
    let verifications: [VerificationRecord]
    /// It changed files and no independent verification checks it.
    let unverified: Bool
    let traceGaps: [String]
    let risks: [String]
    var id: String { task }

    init(_ j: JSON) {
        task = j["task"].str
        title = j["title"].str
        status = TaskStatus(j["status"].str)
        passed = j["passed"].int ?? 0
        criteria = j["criteria"].int ?? 0
        counts = j["counts"].str
        reminders = j["reminders"].strings
        strength = EvidenceStrength(j["strength"])
        metrics = j["metrics"].str
        markers = j["markers"].strings
        needsAttention = j["needsAttention"].bool ?? false
        checks = CheckView.list(j["lines"])
        verifications = j["verifications"].array.map(VerificationRecord.init)
        unverified = j["unverified"].truthy
        traceGaps = j["traceGaps"].strings
        risks = j["risks"].strings
    }

    /// What VoiceOver reads: the id, title, passed count, method counts, markers and whether the
    /// row is open.
    func voice(open: Bool) -> String {
        var parts = [task, title, L.text("通过 \(passed) / \(criteria) 条"), counts, open ? L.text("已展开") : L.text("已收起")]
        parts += markers
        return parts.joined(separator: L.text("，"))
    }
}

/// The delivery under review as its card shows it: the round's line of numbers and each task's row.
struct DeliveryEvidence {
    var facts = ""
    var tasks: [DeliveryAcceptance] = []

    init() {}

    init(_ j: JSON) {
        facts = j["facts"].str
        tasks = j["tasks"].array.map(DeliveryAcceptance.init)
    }
}
