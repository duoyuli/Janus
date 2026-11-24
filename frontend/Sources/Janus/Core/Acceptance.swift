import Foundation

// Acceptance evidence as the events record it: each check, the method behind it, the numbers the
// task page and the delivery show, and the delivery's per-task record. The app only groups what
// was recorded; whether a task passed stays with `task.succeeded` and `verification.*`
// (docs/architecture/20261031-frontend.md#evidence_methods). The backend side is `backend/src/evidence.rs`.

/// How a check says it was verified. Only these five values are methods; any other value is kept as
/// recorded and shown as such instead of being read as a method.
enum EvidenceMethod: String, CaseIterable {
    case test, command, inspection, reasoning, unverified

    var label: String {
        switch self {
        case .test: return L.text("测试")
        case .command: return L.text("命令")
        case .inspection: return L.text("检查")
        case .reasoning: return L.text("推理判断")
        case .unverified: return L.text("未核实")
        }
    }
}

struct Check: Hashable {
    let criterion: String
    let passed: Bool
    let evidence: String
    /// The method the check named, if it named one.
    let method: EvidenceMethod?
    /// A `method` value outside the allowed set, kept exactly as recorded so it is shown, not guessed.
    let badMethod: String?

    init(_ j: JSON) {
        criterion = j["criterion"].str
        passed = j["passed"].bool ?? false
        evidence = j["evidence"].str
        let raw = j["method"]
        let parsed = raw.string.flatMap(EvidenceMethod.init(rawValue:))
        method = parsed
        badMethod = raw.isNull || parsed != nil ? nil : (raw.string ?? raw.str)
    }

    static func list(_ j: JSON) -> [Check] { j.array.map(Check.init) }

    /// 「测试」/「推理判断」/「未核实」/「未说明」; an illegal value is shown as written.
    var methodLabel: String {
        if let method { return method.label }
        if let badMethod { return L.text("证据方法「\(badMethod)」不认识（没说明）") }
        return L.text("未说明")
    }
}

/// A criterion paired with the check that answered it: every criterion gets a row (even when nothing
/// answered it) and every check is kept (even when it names no criterion), so no evidence is dropped.
struct CriterionCheck: Identifiable {
    let n: Int
    let criterion: String
    let check: Check?
    var id: Int { n }

    static func rows(accept: [String], checks: [Check]) -> [CriterionCheck] {
        var rows = accept.enumerated().map { i, criterion in
            CriterionCheck(n: i + 1, criterion: criterion, check: i < checks.count ? checks[i] : nil)
        }
        if checks.count > accept.count {
            for i in accept.count..<checks.count {
                rows.append(CriterionCheck(n: i + 1, criterion: checks[i].criterion, check: checks[i]))
            }
        }
        return rows
    }
}

/// One method's count, in the order the five methods are listed.
struct EvidenceMethodCount: Hashable {
    let method: String
    let label: String
    let count: Int
}

/// The numbers and reminders shown with a task's checks: built from the checks for the task page,
/// or read from the record the backend wrote (`evidence::Summary`) for the delivery.
struct EvidenceSummary: Hashable {
    var criteria = 0
    var passed = 0
    var methods: [EvidenceMethodCount] = []
    var unspecified = 0
    var invalid = 0
    var reminders: [String] = []

    init(accept: [String], checks: [Check]) {
        criteria = accept.count
        passed = checks.prefix(criteria).filter(\.passed).count
        methods = EvidenceMethod.allCases.compactMap { m in
            let n = checks.filter { $0.method == m }.count
            return n == 0 ? nil : EvidenceMethodCount(method: m.rawValue, label: m.label, count: n)
        }
        for (i, c) in checks.enumerated() {
            let n = i + 1
            let trimmed = c.criterion.trimmingCharacters(in: .whitespacesAndNewlines)
            let name = trimmed.isEmpty ? L.text("（没有写明标准）") : trimmed
            switch (c.method, c.badMethod) {
            case (.reasoning?, _):
                reminders.append(L.text("第 \(n) 条「\(name)」是推理判断，不是运行得到的结果"))
            case (.unverified?, _):
                let why = c.evidence.trimmingCharacters(in: .whitespacesAndNewlines)
                reminders.append(L.text("第 \(n) 条「\(name)」未核实：\(why.isEmpty ? L.text("没有说明原因") : why)"))
            case (.some, _):
                break
            case (nil, .some):
                invalid += 1
            case (nil, nil):
                unspecified += 1
                reminders.append(L.text("第 \(n) 条「\(name)」没有说明证据方法"))
            }
        }
    }

    /// The summary the backend recorded (`verification.*`, `task.succeeded`, `run.delivered`).
    init(recorded j: JSON) {
        criteria = j["criteria"].int ?? 0
        passed = j["passed"].int ?? 0
        methods = j["methods"].array.map { m in
            let method = m["method"].str
            return EvidenceMethodCount(method: method, label: EvidenceMethod(rawValue: method)?.label ?? m["label"].string ?? method, count: m["count"].int ?? 0)
        }
        unspecified = j["unspecified"].int ?? 0
        invalid = j["invalid"].array.count
        let recordedReminders = j["reminders"].strings
        // Current deliveries carry the checks. Older records may only have the original text.
        reminders = !recordedReminders.isEmpty && j["checks"].exists
            ? EvidenceSummary(accept: [], checks: Check.list(j["checks"])).reminders
            : recordedReminders
    }

    /// 「测试 2 条、命令 1 条、未说明 1 条」, as the board, the task page and the delivery read it.
    var countsText: String {
        var parts = methods.map { L.text("\($0.label) \($0.count) 条") }
        if unspecified > 0 { parts.append(L.text("未说明 \(unspecified) 条")) }
        if invalid > 0 { parts.append(L.text("方法不合法 \(invalid) 条")) }
        return parts.isEmpty ? L.text("没有记录") : parts.joined(separator: L.text("、"))
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

    static func effective(verdict: Verdict?, acceptCount: Int, checks: [Check]) -> Verdict {
        if let verdict { return verdict }
        if checks.count < acceptCount || checks.prefix(acceptCount).contains(where: { !$0.passed }) {
            return .rejected
        }
        return .approved
    }
}

/// One independent verification of a task: a task whose `verifies` names it, with its own result.
struct VerificationRecord: Identifiable {
    let task: String
    let label: String
    let verdict: Verdict?
    let passed: Bool
    let summary: String
    let checks: [Check]
    let evidence: [String]
    let risks: [String]
    var id: String { task }

    /// As the delivery recorded it.
    init?(_ j: JSON) {
        task = j["task"].str
        if task.isEmpty { return nil }
        label = TaskStatus(rawValue: j["status"].str)?.label.0 ?? j["label"].str
        let vd = j["verdict"].string.flatMap(Verdict.init(rawValue:))
        verdict = vd
        passed = j["passed"].bool ?? (vd == .approved)
        summary = j["summary"].str
        checks = Check.list(j["checks"])
        evidence = j["evidence"].strings
        risks = j["risks"].strings
    }

    /// As the event log has it now.
    init(_ v: TaskState) {
        task = v.id
        label = v.status.label.0
        let eff = v.status == .succeeded ? Verdict.effective(verdict: v.verdict, acceptCount: v.accept.count, checks: v.checks) : nil
        verdict = eff
        passed = eff == .approved
        summary = v.summary
        checks = v.checks
        evidence = v.evidence
        risks = v.risks
    }
}

/// One task's acceptance in `run.delivered`: its numbers and reminders, every check, who verified it
/// independently, where its result disagreed with the workspace, and its risks.
struct DeliveryAcceptance: Identifiable {
    let task: String
    let title: String
    let status: TaskStatus
    let summary: EvidenceSummary
    let checks: [Check]
    let verifications: [VerificationRecord]
    /// It changed files and no independent verification checks it; the lead's summary says how it
    /// checked it.
    let unverified: Bool
    let traceGaps: [String]
    let risks: [String]
    var id: String { task }
}

extension DeliveryAcceptance {
    init?(_ j: JSON) {
        let task = j["task"].str
        if task.isEmpty { return nil }
        self.init(task: task, title: j["title"].str, status: TaskStatus(j["status"].str), summary: EvidenceSummary(recorded: j),
                  checks: Check.list(j["checks"]), verifications: j["verifications"].array.compactMap(VerificationRecord.init),
                  unverified: j["unverified"].truthy, traceGaps: j["traceGaps"].strings, risks: j["risks"].strings)
    }
}

// MARK: Delivery rows

/// One task's row in the delivery's acceptance list (W1 F6): the one-line numbers, the text
/// markers that need attention, and whether the row starts open. The views render these strings;
/// a test reads exactly what they show.
struct DeliveryTaskRow: Equatable {
    let task: String
    let title: String
    let passed: Int
    let criteria: Int
    /// 「测试 1 条、命令 2 条、推理判断 1 条」 (`EvidenceSummary.countsText`).
    let methods: String
    /// Inline text, not colour: the status when it is not success, and risks, trace gaps and
    /// reminders when there are any.
    let markers: [String]
    /// Starts open when a criterion failed, a trace gap exists, or an independent verification did
    /// not pass. The executor's own risks and reminders only mark the row: there is no bound on how
    /// many it reports, and the lead picks the ones the person must act on.
    let needsAttention: Bool

    /// 「通过 4 / 5 条 · 测试 1 条、命令 2 条、推理判断 1 条」
    var metrics: String { L.text("通过 \(passed) / \(criteria) 条 · \(methods)") }

    /// What VoiceOver reads: the id, title, passed count, method counts, markers and whether the
    /// row is open.
    func voice(open: Bool) -> String {
        var parts = [task, title, L.text("通过 \(passed) / \(criteria) 条"), methods, open ? L.text("已展开") : L.text("已收起")]
        parts += markers
        return parts.joined(separator: L.text("，"))
    }
}

extension DeliveryAcceptance {
    /// The collapsed row and the default that decides whether it opens.
    var row: DeliveryTaskRow {
        var markers: [String] = []
        if status != .succeeded { markers.append(status.label.0) }
        if !risks.isEmpty { markers.append(L.text("自述风险 \(risks.count) 条")) }
        if !traceGaps.isEmpty { markers.append(L.text("对不上 \(traceGaps.count) 处")) }
        if !summary.reminders.isEmpty { markers.append(L.text("提醒 \(summary.reminders.count) 条")) }
        if unverified { markers.append(L.text("没有独立验证")) }
        let failed = summary.criteria - summary.passed
        let unconfirmed = verifications.filter { !$0.passed }.count
        return DeliveryTaskRow(
            task: task, title: title, passed: summary.passed, criteria: summary.criteria,
            methods: summary.countsText, markers: markers,
            needsAttention: failed > 0 || checks.contains { !$0.passed } || !traceGaps.isEmpty || unconfirmed > 0)
    }
}
