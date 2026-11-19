import Foundation
import Testing
@testable import Janus

func expectedCopy(_ chinese: String, _ english: String) -> String {
    L.locale.identifier == "en" ? english : chinese
}

/// Dates and durations follow the app language while using the same time zone.
@Test func timesAreInTheAppsOwnWords() {
    var c = DateComponents(); c.year = 2026; c.month = 11; c.day = 12; c.hour = 20; c.minute = 16
    let cal = Calendar(identifier: .gregorian)
    let now = cal.date(from: c)!
    let ms = { (d: Date) in d.timeIntervalSince1970 * 1000 }
    let t = ms(now)
    let dates = [-60, -86400, 86400, -5 * 86400].map {
        L.dateTime(ms(now.addingTimeInterval(Double($0))), now: now)
    }
    let recent = [3000.0, 30_000, 600_000, 7_200_000].map { L.ago(t - $0, t) }
    let durations = [400.0, 5000, 125_000, 3_900_000].map(L.dur)
    if L.locale.identifier == "en" {
        #expect(dates == ["Today at 20:15", "Yesterday at 20:16", "Tomorrow at 20:16", "Nov 7, 20:16"])
        #expect(recent == ["Just now", "30 sec ago", "10 min ago", "2 hr ago"])
        #expect(durations == ["Under 1 sec", "5 sec", "2 min 05 sec", "1 hr 5 min"])
    } else {
        #expect(dates == ["今天 20:15", "昨天 20:16", "明天 20:16", "11月7日 20:16"])
        #expect(recent == ["刚刚", "30 秒前", "10 分钟前", "2 小时前"])
        #expect(durations == ["不到 1 秒", "5 秒", "2 分 05 秒", "1 小时 5 分"])
    }
    #expect(L.dateTime(ms(now.addingTimeInterval(-400 * 86400)), now: now).contains("2025"))
}

/// One problem per cause: an Agent that failed its test is named once with all its roles.
@Test func workspaceProblemsAreGroupedByCause() {
    let conns: [String: JSON] = ["c1": ["name": "脚本 Agent", "phase": "failed"], "c2": ["name": "Codex", "phase": "ok"]]
    let roles: [JSON?] = [["name": "负责人", "connection": "c1"], ["name": "实现者", "connection": "c1"], ["name": "审查者", "connection": "c2"], ["name": "文档", "connection": ""], nil]
    let p = HomeView.problems(roles, connection: { conns[$0] })
    #expect(p.count == 3, "\(p.map(\.text))")
    #expect(p.contains { $0.text.contains("脚本 Agent") && $0.text.contains("负责人") && $0.text.contains("实现者") && $0.tab == .agents })
    #expect(p.contains { $0.text.contains("文档") && $0.tab == .agents })
    #expect(p.contains { $0.text.contains("1") && $0.tab == .agents })
    #expect(HomeView.problems([["name": "审查者", "connection": "c2"]], connection: { conns[$0] }).isEmpty)
}

@Test func appLanguageSwitchesResourcesAndPersistsTheChoice() throws {
    let suite = "JanusTests.language." + UUID().uuidString
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    let settings = LanguageSettings(defaults: defaults)
    #expect(settings.selection == .system)
    for (choice, label, pattern) in [(AppLanguage.english, "Settings", "MMM d, HH:mm"), (.chinese, "设置", "M月d日 HH:mm")] {
        settings.selection = choice
        let current = L.resources(for: settings.selection)
        #expect(L.text("设置", bundle: current.bundle) == label)
        #expect(current.day.dateFormat == pattern)
        #expect(LanguageSettings(defaults: defaults).selection == choice)
    }
    settings.selection = .system
    #expect(LanguageSettings(defaults: defaults).selection == .system)
}

@Test func bothLocalizationResourcesPreserveInterpolatedValues() throws {
    let englishURL = try #require(Bundle.module.url(forResource: "en", withExtension: "lproj"))
    let chineseURL = try #require(Bundle.module.url(forResource: "zh-Hans", withExtension: "lproj"))
    let en = try #require(Bundle(url: englishURL))
    let zh = try #require(Bundle(url: chineseURL))
    #expect(L.text("收件箱", bundle: en) == "Inbox")
    #expect(L.text("收件箱", bundle: zh) == "收件箱")
    let name = "实现者"
    #expect(L.text("「\(name)」请求你决定", bundle: en) == "实现者 asks you to decide")
    #expect(L.text("「\(name)」请求你决定", bundle: zh) == "「实现者」请求你决定")
    let first = "T1", second = "T2", percent = 25
    #expect(L.text("依赖的 \(first) 没有完成，\(second) 还不能开始。", bundle: en) == "T2 cannot start until its dependency T1 is complete.")
    #expect(L.text("本会话 上下文 \(percent)%", bundle: en) == "Session context: 25%")
}

/// Usage numbers keep their state: a reported 0 is written as 0, a missing number as words, and
/// amounts are shown as reported.
@Test func usageNumbersKeepTheirState() {
    #expect(usageNum(15400) == "15,400")
    #expect(usageNum(999) == "999" && usageNum(0) == "0")
    #expect(usageMoney(0.25) == "0.25" && usageMoney(5) == "5" && usageMoney(0.12345) == "0.1235")
    #expect(usagePercent(0.26) == "26%")

    var zero = UsageScope(id: "run")
    zero.tokens = UsageTokens(total: 0)
    zero.completeness = .complete
    #expect(zero.tokensText == expectedCopy("0 tokens（完整）", "0 tokens (Complete)"))
    #expect(zero.breakdown == nil, "parts of 0 are not listed")

    let missing = UsageScope(id: "run")
    #expect(missing.tokensText == expectedCopy("未上报", "Not reported"))
    #expect(missing.costText == expectedCopy("费用：未上报", "Cost: Not reported"))
    #expect(missing.costDetail == expectedCopy("未上报", "Not reported"))
    #expect(missing.durationText == expectedCopy("没有时长记录", "No duration recorded"))
    #expect(missing.contextDetail == nil)
    // An empty ledger keeps the words, so the run card cannot show "0 tokens".
    #expect(UsageLedger.empty.runLine == expectedCopy("未上报 · 费用：未上报 · 耗时 没有时长记录", "Not reported · Cost: Not reported · Elapsed: No duration recorded"))
    #expect(UsageLedger.empty.remindersLine == nil)
}

/// The audit filter's categories, compared by prefix (each event is asked on every draw).
@Test func eventCategoriesFollowTheirPrefixes() {
    let cases: [(String, String)] = [
        ("question.answered", "decisions"), ("input.answered", "decisions"), ("approval.granted", "decisions"), ("approval.denied", "decisions"),
        ("risk.accepted", "decisions"), ("run.paused", "decisions"), ("run.resumed", "decisions"), ("run.cancelled", "decisions"),
        ("question.asked", "messages"), ("question.escalated", "messages"), ("approval.requested", "messages"), ("input.requested", "messages"),
        ("answer.delivered", "messages"), ("user.message", "messages"), ("message.delivered", "messages"), ("control.taken", "messages"),
        ("session.prompt", "messages"), ("session.usage", "messages"),
        ("agent.message", "progress"), ("agent.status", "progress"), ("tool.call", "progress"), ("tool.completed", "progress"),
        ("artifact.created", "artifacts"), ("context.handoff", "artifacts"),
        ("task.succeeded", "tasks"), ("attempt.started", "tasks"), ("run.created", "tasks"), ("message.routed", "tasks"), ("run.delivered", "tasks"),
    ]
    for (type, category) in cases { #expect(L.category(type) == category, "\(type)") }
}

/// Patterns compiled once behave like the string methods they replace.
@Test func compiledPatternsMatchTheStringMethods() {
    #expect(L.tilde("/Users/someone/work/a.md") == "~/work/a.md" && L.tilde("/opt/a.md") == "/opt/a.md")
    #expect(L.goalTitle("## 做 **一件** [事](/tmp/x.md)\n第二行") == "做 一件 事")
    #expect(MD.inline("看 `a.md` 和 **重点**").characters.count == "看 a.md 和 重点".count)
}
