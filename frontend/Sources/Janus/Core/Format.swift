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
        "idle": ("摸鱼中", .gray), "thinking": ("思考中", .blue), "coordinating": ("协调中", .blue), "reading_files": ("翻文件", .blue),
        "editing": ("改文件", .blue), "running_tool": ("用工具", .blue), "running_test": ("跑测试", .blue),
        "waiting_for_input": ("等你输入", .amber), "waiting_for_agent": ("等队友", .amber), "waiting_approval": ("等你审批", .amber), "waiting_for_seat": ("等空位", .amber), "error": ("出错了", .red),
    ]
    static func phase(_ s: String) -> (String, Tone) {
        let (key, tone) = agentPhase[s] ?? agentPhase["idle"]!
        return (catalogText(key), tone)
    }

    static let icon: [EventKind: String] = [
        .runCreated: "✦", .runCancelled: "■", .runPaused: "Ⅱ", .runResumed: "▶",
        .taskReady: "▸", .taskAssigned: "↗", .attemptStarted: "▶", .attemptFailed: "✕", .taskRetryScheduled: "↻", .taskReopened: "↺",
        .resultSubmitted: "⇪", .taskSucceeded: "✓", .artifactCreated: "▤", .contextHandoff: "⇄",
        .questionAsked: "?", .questionEscalated: "⇡", .questionAnswered: "✎", .answerDelivered: "↩",
        .approvalRequested: "⚑", .inputRequested: "✎", .inputAnswered: "↩", .approvalGranted: "✓", .approvalDenied: "✕",
        .runDelivered: "★", .riskAccepted: "✓", .runMerged: "✓", .taskFailed: "✕",
        .taskBlocked: "⊘", .taskSkippedByDecision: "→", .verificationStarted: "◌", .verificationFailed: "✕",
        .verificationPassed: "✓", .runFailed: "✕", .runInterrupted: "■", .agentStatus: "·", .toolCall: "⌘", .toolStarted: "⌘", .toolCompleted: "✓",
        .sessionPrompt: "→", .resultAmended: "✎", .runRecovered: "↻", .messageRouted: "↪", .decisionWithdrawn: "↩",
        .agentMessage: "❝", .userMessage: "✎",
        .messageDelivered: "↪", .sessionParked: "◌", .controlTaken: "✋", .controlReleased: "↻", .sessionInterrupted: "‖",
        .taskConflict: "⚠", .leadStalled: "⏳", .taskStalled: "⏳", .taskSplitRequested: "✂",
    ]

    private static let toneTable: [Tone: [EventKind]] = [
        .green: [.taskSucceeded, .approvalGranted, .inputAnswered, .runDelivered, .riskAccepted, .runMerged],
        .amber: [.questionAsked, .questionEscalated, .approvalRequested, .inputRequested, .runPaused, .controlTaken, .sessionInterrupted, .userMessage, .taskConflict, .leadStalled, .taskStalled, .taskSplitRequested],
        .red: [.attemptFailed, .approvalDenied, .runCancelled, .taskFailed, .runFailed, .runInterrupted],
        .gray: [.agentStatus, .toolCall, .toolCompleted, .taskReady, .sessionParked],
        .blue: [.toolStarted, .sessionPrompt],
    ]
    static func tone(of type: EventKind) -> Tone? { toneTable.first { $0.value.contains(type) }?.key }

    /// Event categories for the feed and the audit filter.
    static func category(_ type: EventKind) -> String {
        let family = type.family
        if [.questionAnswered, .inputAnswered, .approvalGranted, .approvalDenied, .riskAccepted, .runMerged, .runPaused, .runResumed, .runCancelled].contains(type) { return "decisions" }
        if ["question", "answer", "control", "session"].contains(family) || [.approvalRequested, .inputRequested, .userMessage, .messageDelivered].contains(type) { return "messages" }
        if type == .agentMessage { return "progress" }
        if family == "artifact" || type == .contextHandoff { return "artifacts" }
        if family == "tool" || type == .agentStatus { return "progress" }
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
}
