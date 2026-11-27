import AppKit
import Foundation
import SwiftUI
import Testing
@testable import Janus

/// The usage card and the period bill really draw: the same rows the data tests assert are rendered
/// into PNGs (light and dark), so a person can look at the result instead of trusting the strings
/// alone.
@Test @MainActor func usageCardsRenderInBothAppearances() throws {
    var events: [Event] = []
    func add(_ type: String, actor: String = "janus", task: String? = nil, attempt: Int? = nil, data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(1_000_000 + events.count * 1000), "type": JSON(type),
                       "actor": JSON(actor), "text": "", "data": data]
        if let task { j["task"] = JSON(task) }
        if let attempt { j["attempt"] = JSON(attempt) }
        events.append(Event(j))
    }
    add("run.created", actor: "user", data: ["goal": "统计这次 Run 的用量", "cwd": "/tmp/x"])
    add("orchestration.accepted", actor: "lead", data: ["tasks": [
        ["id": "T1", "title": "调研调用方", "agent": "researcher", "deps": [], "accept": ["列出调用方"]],
        ["id": "T2", "title": "补充测试", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过"]],
    ]])
    add("attempt.started", actor: "researcher", task: "T1", attempt: 1, data: ["session": "s1"])
    add("session.controls", actor: "researcher", task: "T1", attempt: 1, data: ["model": ["id": "model", "current": "claude/claude-opus-5-5"]])
    add("session.usage", actor: "researcher", task: "T1", attempt: 1, data: ["used": 61000, "size": 200000])
    add("session.usage", actor: "researcher", task: "T1", attempt: 1, data: ["used": 52000, "size": 200000])
    add("session.usage", actor: "researcher", task: "T1", attempt: 1, data: ["inputTokens": 12000, "outputTokens": 3400, "totalTokens": 15400])
    add("question.asked", actor: "researcher", task: "T1", attempt: 1, data: ["key": "q1", "question": "用哪个？", "options": ["A"]])
    add("question.answered", actor: "lead", task: "T1", data: ["key": "q1", "answer": "A"])
    add("task.succeeded", actor: "researcher", task: "T1", attempt: 1, data: ["summary": "三处调用方"])
    add("session.usage", actor: "lead", data: ["totalTokens": 1500, "cost": ["amount": 0.25, "currency": "USD"]])
    // T2 opened a session and reported nothing: the run is partial and says so.
    add("attempt.started", actor: "implementer", task: "T2", attempt: 1, data: ["session": "s2"])

    let store = testStore
    let run = RunModel(id: "render", store: store)
    run.runJSON = ["config": ["team": [
        ["id": "lead", "name": "负责人", "lead": true],
        ["id": "researcher", "name": "调研者", "color": "#6a42d6"],
        ["id": "implementer", "name": "实现者"],
    ]]]
    run.events = events
    run.replay = events.count
    // The ledger the server would compute for these events (`serverLedger`, a partial run).
    run.usage = UsageLedger.server(serverLedger)

    // The view the server would serve for them (`GET /api/runs/{id}/view`).
    let t1 = events.filter { $0.task == "T1" }.map { JSON($0.seq) }
    let st = RunState([
        "run": ["status": "running", "goal": "统计这次 Run 的用量", "cwd": "/tmp/x", "start": 1_000_000, "now": .number(events.last!.ts)],
        "tasks": [["id": "T1", "title": "调研调用方", "agent": "researcher", "status": "succeeded", "accept": ["列出调用方"], "summary": "三处调用方"],
                  ["id": "T2", "title": "补充测试", "agent": "implementer", "status": "running", "deps": ["T1"], "accept": ["测试通过"], "session": ["person": false, "idle": false]]],
        "sessions": ["task:T1": ["events": .array(t1), "composer": "replay"]],
    ], events: events)
    #expect(run.usage.run.completeness == .partial, "the rendered card says partial")
    #expect(run.usage.words.rows.first?.value.contains("部分上报") == true)

    let dir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("janus-usage", isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    func render(_ name: String, dark: Bool, _ view: some View) throws -> Int {
        let lit = view.environment(\.colorScheme, dark ? .dark : .light)
            .padding(16)
            .frame(width: 620, alignment: .leading)
            .background(Color.page)
        let renderer = ImageRenderer(content: lit)
        renderer.scale = 2
        let image = try #require(renderer.nsImage, "SwiftUI drew the view")
        let tiff = try #require(image.tiffRepresentation)
        let rep = try #require(NSBitmapImageRep(data: tiff))
        let data = try #require(rep.representation(using: .png, properties: [:]))
        try data.write(to: dir.appendingPathComponent(name))
        return data.count
    }
    let runCard = try render("run-card-light.png", dark: false, RunCard(run: run, st: st, width: 620))
    let runCardDark = try render("run-card-dark.png", dark: true, RunCard(run: run, st: st, width: 620))
    let runCardNarrow = try render("run-card-narrow-light.png", dark: false, RunCard(run: run, st: st, width: 560).frame(width: 560))
    let detail = try render("run-usage-detail-light.png", dark: false, Card { UsageSection(run: run, st: st) })
    // The session card's header is the other place the numbers appear: it labels them 本会话/上一轮.
    let chat = ChatCard(run: run, st: st, title: "调研者", info: st.session(.task("T1")), target: .task("T1"))
        .frame(width: 560, height: 260)
    let session = try render("session-header-light.png", dark: false, chat)
    #expect(detail > 10_000 && session > 10_000)
    // The period bill of the settings pane, from a receipt shaped like `GET /api/usage/receipt`.
    let receipt: JSON = [
        "title": "用量账单", "meta": [["label": "区间", "value": "2025年10月9日 – 2025年11月7日"]], "headline": "$6,389.62",
        "headlineCaption": "按 models.dev API 价格折算 · 非账单", "headlineDetail": "6,450,000,000 tokens · Agent 上报费用 12.5 USD（4 个会话）",
        "stats": [["label": "任务", "value": "12 个"]], "groupsTitle": "Agent / 模型", "amountTitle": "估算 · Token",
        "groups": [["name": "Codex", "amount": ["tokens": "5.9B", "estimate": "$5,213.22", "help": "5,900,000,000 tokens · 2900 轮"],
                    "models": [["model": "gpt-6-astra", "amount": ["tokens": "5.1B", "estimate": "$4,418.29", "help": ""]]]]],
        "totals": [["label": "Token 总量", "value": "6,450,000,000"], ["label": "缓存命中率", "value": "97%"]],
        "notes": ["估算按 models.dev 的 API 价格（不含长上下文加价），不是账单。"], "text": "用量账单",
    ]
    let bill = UsageBill(receipt)
    #expect(bill.headline == "$6,389.62" && bill.totalRows.last?.value == "97%" && bill.groups[0].models[0].name == "gpt-6-astra")
    let periodCard = try render("period-bill-light.png", dark: false, Card(padding: 22) { UsageReceipt(bill: bill) }.frame(width: 520))
    #expect(periodCard > 10_000)
    #expect(runCard > 10_000 && runCardDark > 10_000 && runCardNarrow > 10_000, "the cards drew: \(runCard)/\(runCardDark)/\(runCardNarrow) bytes")
    print("usage card screenshots: \(dir.path)")
}

/// A conversation follows new items only while its end is in view: only the person's own scrolling
/// lets go of the end. Scrolled up, they keep their place and what arrived is noted for the way back.
@Test func aConversationFollowsOnlyFromTheBottom() {
    var f = ChatFollow()
    // Opening with the end out of view: the view goes there.
    var again = f.measured(below: 2000, last: 10, person: false)
    #expect(again && f.following)
    again = f.measured(below: 0, last: 10, person: false)
    #expect(!again && f.following && f.landing == 0)
    // At the end, a new reply below the view is followed.
    again = f.measured(below: 300, last: 11, person: false)
    #expect(again && f.following && !f.unseen)
    _ = f.measured(below: 0, last: 11, person: false)
    // SwiftUI moving the content (rows measured, the offset clamped) does not let go either.
    again = f.measured(below: 400, last: 11, person: false)
    #expect(again && f.following)
    _ = f.measured(below: 0, last: 11, person: false)
    // The person scrolls up: the view stays, and replies are only noted.
    again = f.measured(below: 800, last: 11, person: true)
    #expect(!again && !f.following && !f.unseen)
    again = f.measured(below: 1100, last: 12, person: false)
    #expect(!again && !f.following && f.unseen)
    again = f.measured(below: 1300, last: 12, person: false)
    #expect(!again && !f.following, "rows measured further up do not move the view")
    // Back at the end by scrolling: following again, nothing left unseen.
    again = f.measured(below: 10, last: 12, person: true)
    #expect(!again && f.following && !f.unseen)
}

/// Sent to the newest item, the view keeps going while the lazy list measures the rows on the
/// way; an end it never reaches is let go after a few scrolls and seconds, and the button takes the
/// person back. Many quick passes (a busy Mac measuring a tall reply) do not use the time up.
@Test func aJumpToTheNewestLandsBeforeLettingGo() {
    var f = ChatFollow()
    _ = f.measured(below: 0, last: 5, person: false, now: 0)
    _ = f.measured(below: 3000, last: 5, person: true, now: 0)
    #expect(!f.following)
    f.jumped()
    var again = f.measured(below: 70, last: 5, person: false, now: 1)
    #expect(again && f.following, "the end moved while the rows were measured: go again")
    again = f.measured(below: 0, last: 5, person: false, now: 1)
    #expect(!again && f.following && f.landing == 0)
    // Forty passes within a second: still going.
    var going = true
    for i in 0..<40 { going = f.measured(below: 5000, last: 5, person: false, now: 2 + Double(i) / 40) && going }
    #expect(going && f.following)
    // Then the end is reached: done, following.
    again = f.measured(below: 0, last: 5, person: false, now: 3)
    #expect(!again && f.following)
    // An end it never reaches is let go once both the scrolls and the seconds are spent.
    var t = 10.0, tries = 0
    while f.measured(below: 5000, last: 5, person: false, now: t) { tries += 1; t += 0.5 }
    #expect(!f.following && tries == ChatFollow.landingTries && t - 10 > ChatFollow.landingTime)
    // A scroll that stopped short reports nothing more: a look a moment later scrolls again.
    var g = ChatFollow()
    _ = g.measured(below: 600, last: 7, person: false, now: 0)
    var more = g.retry(now: 0.2)
    #expect(more && g.following)
    // Within the slack but not at the very end: the view still goes all the way.
    more = g.measured(below: 13, last: 7, person: false, now: 0.3)
    #expect(more && g.atEnd)
    _ = g.measured(below: 0, last: 7, person: false, now: 0.4)
    more = g.retry(now: 0.6)
    #expect(!more, "at the end: nothing to do")
}
