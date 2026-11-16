import Foundation

// Acceptance evidence as the events record it: each check, the method behind it, the numbers the
// task page and the delivery show, and the delivery's per-task record. The app only groups what
// was recorded; whether a task passed stays with `task.succeeded` and `verification.*`
// (docs/architecture/frontend.md#evidence_methods). The backend side is `backend/src/evidence.rs`.

/// How a check says it was verified. Only these five values are methods; any other value is kept as
/// recorded and shown as such instead of being read as a method.
enum EvidenceMethod: String, CaseIterable {
    case test, command, inspection, reasoning, unverified

    var label: String {
        switch self {
        case .test: return "测试"
        case .command: return "命令"
        case .inspection: return "检查"
        case .reasoning: return "推理判断"
        case .unverified: return "未核实"
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
        if let badMethod { return "方法「\(badMethod)」不合法（未说明）" }
        return "未说明"
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
            let name = trimmed.isEmpty ? "（没有写明标准）" : trimmed
            switch (c.method, c.badMethod) {
            case (.reasoning?, _):
                reminders.append("第 \(n) 条「\(name)」是推理判断，不是运行得到的结果")
            case (.unverified?, _):
                let why = c.evidence.trimmingCharacters(in: .whitespacesAndNewlines)
                reminders.append("第 \(n) 条「\(name)」未核实：\(why.isEmpty ? "没有说明原因" : why)")
            case (.some, _):
                break
            case (nil, .some):
                invalid += 1
            case (nil, nil):
                unspecified += 1
                reminders.append("第 \(n) 条「\(name)」没有说明证据方法")
            }
        }
    }

    /// The summary the backend recorded (`verification.*`, `task.succeeded`, `run.delivered`).
    init(recorded j: JSON) {
        criteria = j["criteria"].int ?? 0
        passed = j["passed"].int ?? 0
        methods = j["methods"].array.map { m in
            let method = m["method"].str
            return EvidenceMethodCount(method: method, label: m["label"].string ?? EvidenceMethod(rawValue: method)?.label ?? method, count: m["count"].int ?? 0)
        }
        unspecified = j["unspecified"].int ?? 0
        invalid = j["invalid"].array.count
        reminders = j["reminders"].strings
    }

    /// 「测试 2 条、命令 1 条、未说明 1 条」, as the board, the task page and the delivery read it.
    var countsText: String {
        var parts = methods.map { "\($0.label) \($0.count) 条" }
        if unspecified > 0 { parts.append("未说明 \(unspecified) 条") }
        if invalid > 0 { parts.append("方法不合法 \(invalid) 条") }
        return parts.isEmpty ? "没有记录" : parts.joined(separator: "、")
    }
}

/// One independent verification of a task: a task whose `verifies` names it, with its own result.
struct VerificationRecord: Identifiable {
    let task: String
    let label: String
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
        label = j["label"].str
        passed = j["passed"].bool ?? false
        summary = j["summary"].str
        checks = Check.list(j["checks"])
        evidence = j["evidence"].strings
        risks = j["risks"].strings
    }

    /// As the event log has it now.
    init(_ v: TaskState) {
        task = v.id
        label = v.status.label.0
        passed = v.status == .succeeded
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
                  traceGaps: j["traceGaps"].strings, risks: j["risks"].strings)
    }
}

/// The per-task acceptance record `run.delivered` carries.
func deliveryAcceptance(_ st: RunState) -> [DeliveryAcceptance] {
    st.result?.d["acceptance"].array.compactMap(DeliveryAcceptance.init) ?? []
}
