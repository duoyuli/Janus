import Foundation

/// A pattern written in the code, kept in a `static let` so it is compiled once. Views are drawn
/// again for every event; `String.range(of:options: .regularExpression)` compiles its pattern on
/// each call, which made long conversations and the audit log stutter.
func regex(_ pattern: String) -> NSRegularExpression {
    // The patterns are constants in the source: one that does not compile is a programming error.
    try! NSRegularExpression(pattern: pattern)
}

extension NSRegularExpression {
    private func whole(_ s: String) -> NSRange { NSRange(s.startIndex..., in: s) }
    func found(in s: String) -> Bool { firstMatch(in: s, range: whole(s)) != nil }
    /// The text of the first match's first group.
    func firstGroup(in s: String) -> String? {
        guard let m = firstMatch(in: s, range: whole(s)), m.numberOfRanges > 1, let r = Range(m.range(at: 1), in: s) else { return nil }
        return String(s[r])
    }
    /// Every match, whole.
    func all(in s: String) -> [String] {
        matches(in: s, range: whole(s)).compactMap { Range($0.range, in: s).map { String(s[$0]) } }
    }
    /// Like `replacingOccurrences(of:with:options: .regularExpression)`: `$1` names a group.
    func replacing(in s: String, with template: String) -> String {
        stringByReplacingMatches(in: s, range: whole(s), withTemplate: template)
    }
}

/// Display tones shared by pills, dots and event icons.
enum Tone: String { case gray, blue, green, amber, red, violet }

/// Labels and small formatting helpers.
enum L {
    static let agentPhase: [String: (String, Tone)] = [
        "idle": ("空闲", .gray), "thinking": ("规划中", .blue), "coordinating": ("协调中", .blue), "reading_files": ("读取文件", .blue),
        "editing": ("修改文件", .blue), "running_tool": ("调用工具", .blue), "running_test": ("运行测试", .blue),
        "waiting_for_input": ("等待输入", .amber), "waiting_for_agent": ("等待协作者", .amber), "waiting_approval": ("等待审批", .amber), "error": ("异常", .red),
    ]
    static func phase(_ s: String) -> (String, Tone) {
        let (key, tone) = agentPhase[s] ?? agentPhase["idle"]!
        return (catalogText(key), tone)
    }

    static let icon: [String: String] = [
        "run.created": "✦", "run.cancelled": "■", "run.paused": "Ⅱ", "run.resumed": "▶",
        "task.ready": "▸", "task.assigned": "↗", "attempt.started": "▶", "attempt.failed": "✕", "task.retry_scheduled": "↻", "task.reopened": "↺",
        "result.submitted": "⇪", "task.succeeded": "✓", "artifact.created": "▤", "context.handoff": "⇄",
        "question.asked": "?", "question.escalated": "⇡", "question.answered": "✎", "answer.delivered": "↩",
        "approval.requested": "⚑", "input.requested": "✎", "input.answered": "↩", "approval.granted": "✓", "approval.denied": "✕",
        "run.delivered": "★", "run.budget_exhausted": "⌛", "risk.accepted": "✓", "task.failed": "✕",
        "task.blocked": "⊘", "task.skipped_by_decision": "→", "verification.started": "◌", "verification.failed": "✕",
        "verification.passed": "✓", "run.failed": "✕", "run.interrupted": "■", "agent.status": "·", "tool.call": "⌘", "tool.started": "⌘", "tool.completed": "✓",
        "session.prompt": "→", "result.amended": "✎", "run.recovered": "↻", "message.routed": "↪", "decision.withdrawn": "↩",
        "run.budget_extended": "⌛", "agent.message": "❝", "user.message": "✎",
        "message.delivered": "↪", "control.taken": "✋", "control.released": "↻", "session.interrupted": "‖",
        "task.conflict": "⚠", "lead.stalled": "⏳", "task.stalled": "⏳", "task.claimed": "⚐", "task.split_requested": "✂",
    ]

    private static let toneTable: [Tone: [String]] = [
        .green: ["task.succeeded", "approval.granted", "input.answered", "run.delivered", "risk.accepted"],
        .amber: ["question.asked", "question.escalated", "approval.requested", "input.requested", "run.paused", "control.taken", "session.interrupted", "user.message", "task.conflict", "lead.stalled", "task.stalled", "task.split_requested"],
        .red: ["attempt.failed", "approval.denied", "run.cancelled", "task.failed", "run.failed", "run.interrupted"],
        .gray: ["agent.status", "tool.call", "tool.completed", "task.ready"],
        .blue: ["tool.started", "session.prompt"],
    ]
    static func tone(of type: String) -> Tone? { toneTable.first { $0.value.contains(type) }?.key }

    /// Event categories for the feed and the audit filter.
    /// The audit filter asks this for every event on each draw, so it compares prefixes instead of
    /// matching patterns.
    static func category(_ type: String) -> String {
        func m(_ prefixes: [String]) -> Bool { prefixes.contains { type.hasPrefix($0) } }
        if m(["question.answered", "input.answered", "approval.granted", "approval.denied", "risk.", "run.paused", "run.resumed", "run.cancelled"]) { return "decisions" }
        if m(["question.", "approval.requested", "input.requested", "answer.", "user.message", "message.delivered", "control.", "session."]) { return "messages" }
        if type == "agent.message" { return "progress" }
        if type.hasPrefix("artifact.") || type == "context.handoff" { return "artifacts" }
        if m(["agent.status", "tool.", "session.prompt"]) { return "progress" }
        return "tasks"
    }
    static var categoryLabel: [(String, String)] { [("all", L.text("全部")), ("tasks", L.text("子任务")), ("messages", L.text("消息")), ("artifacts", L.text("产物")), ("decisions", L.text("决策")), ("progress", L.text("进度与工具"))] }

    // MARK: Time

    static func clock(_ ms: Double) -> String { language.clock.string(from: Date(timeIntervalSince1970: ms / 1000)) }
    static func calendarDate(_ date: Date) -> String { language.calendarDate.string(from: date) }
    static func datedTime(_ date: Date) -> String { language.year.string(from: date) }

    /// A moment in the app language, with the year when it is not this year.
    static func dateTime(_ ms: Double, now: Date = Date()) -> String {
        let date = Date(timeIntervalSince1970: ms / 1000)
        let cal = Calendar(identifier: .gregorian)
        let time = language.time.string(from: date)
        if cal.isDate(date, inSameDayAs: now) { return L.text("今天 \(time)") }
        if let y = cal.date(byAdding: .day, value: -1, to: now), cal.isDate(date, inSameDayAs: y) { return L.text("昨天 \(time)") }
        if let t = cal.date(byAdding: .day, value: 1, to: now), cal.isDate(date, inSameDayAs: t) { return L.text("明天 \(time)") }
        return (cal.component(.year, from: date) == cal.component(.year, from: now) ? language.day : language.year).string(from: date)
    }
    static func dur(_ ms: Double) -> String {
        let s = max(0, Int((ms / 1000).rounded()))
        if ms < 1000 { return L.text("不到 1 秒") }
        if s < 60 { return L.text("\(s) 秒") }
        if s < 3600 { return L.text("\(s / 60) 分 \(String(format: "%02d", s % 60)) 秒") }
        return L.text("\(s / 3600) 小时 \(s % 3600 / 60) 分")
    }
    static func ago(_ ts: Double, _ now: Double) -> String {
        let s = max(0, Int(((now - ts) / 1000).rounded()))
        if s < 10 { return L.text("刚刚") }
        if s < 60 { return L.text("\(s) 秒前") }
        if s < 3600 { return L.text("\(s / 60) 分钟前") }
        if s < 86400 { return L.text("\(s / 3600) 小时前") }
        return dateTime(ts, now: Date(timeIntervalSince1970: now / 1000))
    }
    static func nowMs() -> Double { Date().timeIntervalSince1970 * 1000 }

    // MARK: Text

    /// Home-relative path for display.
    static func tilde(_ p: String) -> String { homeDir.replacing(in: p, with: "~/") }
    private static let homeDir = regex(#"^/(?:Users|home)/[^/]+/"#)
    /// Markdown marks dropped, for previews cut to a length.
    static func plain(_ t: String) -> String {
        headingMarks.replacing(in: emphasis.replacing(in: link.replacing(in: t, with: "$1"), with: ""), with: "")
    }
    private static let link = regex(#"\[([^\]\n]+)\]\([^)\s]+\)"#)
    private static let emphasis = regex(#"\*\*|`"#)
    private static let headingMarks = regex(#"(?m)^#+\s*"#)
    /// A goal can be a long pasted brief; lists and headers show its first line.
    static func goalTitle(_ g: String) -> String {
        let first = g.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty } ?? ""
        return plain(first).trimmingCharacters(in: .whitespaces)
    }
    /// An agent's permission options in the app's words, by their ACP `kind`; the agent's own
    /// wording stays in the help. Without options, approve or reject this once.
    struct PermissionOption { let id, name, kind, label, help: String }
    static func permissionOptions(_ options: JSON) -> [PermissionOption] {
        let list = options.array.isEmpty ? [["optionId": "", "name": "", "kind": "allow_once"], ["optionId": "", "name": "", "kind": "reject_once"]] as [JSON] : options.array
        let words = ["allow_once": L.text("允许一次"), "allow_always": L.text("始终允许"), "reject_once": L.text("拒绝"), "reject_always": L.text("始终拒绝")]
        let notes = ["allow_always": L.text("之后同类操作不再询问"), "reject_always": L.text("之后同类操作直接拒绝")]
        return list.map { o in
            let kind = o["kind"].str
            let name = o["name"].truthy ? o["name"].str : o["optionId"].str
            let label = words[kind] ?? name
            let help = [notes[kind], name.isEmpty || name == label ? nil : L.text("Agent 的原话：\(name)")].compactMap { $0 }.joined(separator: L.text("。"))
            return PermissionOption(id: o["optionId"].str, name: name.isEmpty ? label : name, kind: kind, label: label, help: help)
        }
    }

    /// Paths inside the workspace written relative to it, as the agent's own interface shows them.
    static func inWorkspace(_ text: String, _ cwd: String) -> String {
        guard !cwd.isEmpty else { return text }
        let root = cwd.hasSuffix("/") ? String(cwd.dropLast()) : cwd
        return text.replacingOccurrences(of: root + "/", with: "")
    }

    static func short(_ t: String, _ n: Int = 26) -> String { t.count > n ? String(t.prefix(n)) + "…" : t }
    static func kb(_ n: Int?) -> String {
        guard let n else { return "" }
        return n < 1024 ? "\(n) B" : String(format: n < 10240 ? "%.1f KB" : "%.0f KB", Double(n) / 1024)
    }
}
