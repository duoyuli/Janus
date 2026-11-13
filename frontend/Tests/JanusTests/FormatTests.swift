import Foundation
import Testing
@testable import Janus

/// Times read the same whatever the system language.
@Test func timesAreInTheAppsOwnWords() {
    var c = DateComponents(); c.year = 2026; c.month = 11; c.day = 12; c.hour = 20; c.minute = 16
    let cal = Calendar(identifier: .gregorian)
    let now = cal.date(from: c)!
    let ms = { (d: Date) in d.timeIntervalSince1970 * 1000 }
    #expect(L.dateTime(ms(now.addingTimeInterval(-60)), now: now) == "今天 20:15")
    #expect(L.dateTime(ms(now.addingTimeInterval(-86400)), now: now) == "昨天 20:16")
    #expect(L.dateTime(ms(now.addingTimeInterval(86400)), now: now) == "明天 20:16")
    #expect(L.dateTime(ms(now.addingTimeInterval(-5 * 86400)), now: now) == "11月7日 20:16")
    #expect(L.dateTime(ms(now.addingTimeInterval(-400 * 86400)), now: now).hasPrefix("2025年"))
    let t = ms(now)
    #expect(L.ago(t - 3000, t) == "刚刚" && L.ago(t - 30_000, t) == "30 秒前" && L.ago(t - 600_000, t) == "10 分钟前" && L.ago(t - 7_200_000, t) == "2 小时前")
    #expect(L.dur(400) == "不到 1 秒" && L.dur(5000) == "5 秒" && L.dur(125_000) == "2 分 05 秒" && L.dur(3_900_000) == "1 小时 5 分")
}

/// One problem per cause: an Agent that failed its test is named once with all its roles.
@Test func workspaceProblemsAreGroupedByCause() {
    let conns: [String: JSON] = ["c1": ["name": "脚本 Agent", "phase": "failed"], "c2": ["name": "Codex", "phase": "ok"]]
    let roles: [JSON?] = [["name": "主控", "connection": "c1"], ["name": "实现者", "connection": "c1"], ["name": "审查者", "connection": "c2"], ["name": "文档", "connection": ""], nil]
    let p = HomeView.problems(roles, connection: { conns[$0] })
    #expect(p.count == 3, "\(p.map(\.text))")
    #expect(p.contains { $0.text.contains("「脚本 Agent」") && $0.text.contains("主控、实现者") && $0.tab == "agents" })
    #expect(p.contains { $0.text.hasPrefix("文档还没有选择 Agent") && $0.tab == "roles" })
    #expect(p.contains { $0.text.contains("1 个角色已被删除") })
    #expect(HomeView.problems([["name": "审查者", "connection": "c2"]], connection: { conns[$0] }).isEmpty)
}

/// A space only where Chinese meets Latin letters or digits.
@Test func chineseJoinsWithoutStraySpaces() {
    #expect(L.cn("实现者", "请求你决定") == "实现者请求你决定")
    #expect(L.cn("Codex", "请求你决定") == "Codex 请求你决定")
    #expect(L.cn("主控、实现者", "还没有选择 Agent。") == "主控、实现者还没有选择 Agent。")
}
