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
    /// The audit filter's choices; which one each event kind shows under is the server's
    /// (`auditCategories` of `GET /api/meta`).
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
    /// Paths inside the workspace written relative to it, as the agent's own interface shows them.
    static func inWorkspace(_ text: String, _ cwd: String) -> String {
        guard !cwd.isEmpty else { return text }
        let root = cwd.hasSuffix("/") ? String(cwd.dropLast()) : cwd
        return text.replacingOccurrences(of: root + "/", with: "")
    }

    static func short(_ t: String, _ n: Int = 26) -> String { t.count > n ? String(t.prefix(n)) + "…" : t }
}
