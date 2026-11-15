import AppKit
import Foundation
import SwiftUI
import Testing
@testable import Janus

private func ev(_ seq: Int, _ type: String, _ data: JSON) -> Event {
    Event(["seq": JSON(seq), "ts": JSON(seq * 1000), "type": JSON(type), "actor": "m1", "task": "T1", "text": "", "data": data])
}

/// The plan, controls, commands and usage beside a conversation come from its latest events.
@Test func sessionStateComesFromTheLatestEvents() {
    let evs = [
        ev(1, "session.controls", ["model": ["id": "model", "current": "a", "options": [["value": "a", "name": "A"], ["value": "b", "name": "B"]]],
                                   "controls": [["id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "mode", "current": "ask", "options": [["value": "ask"], ["value": "full"]]],
                                                ["id": "solo", "type": "select", "current": "x", "options": [["value": "x"]]]]]),
        ev(2, "agent.plan", ["entries": [["content": "读", "status": "completed"], ["content": "改", "status": "in_progress"]]]),
        ev(3, "session.control", ["id": "mode", "value": "full", "via": "mode"]),
        ev(4, "session.control", ["id": "model", "failed": true]),
        ev(5, "session.commands", ["commands": [["name": "compact", "description": "压缩"]]]),
        ev(6, "session.usage", ["used": 50000, "size": 200000]),
        ev(7, "session.usage", ["totalTokens": 1500]),
    ]
    #expect(SessionView.plan(evs).map(\.status) == ["completed", "in_progress"])
    let c = SessionView.controls(evs)
    #expect(c.map(\.id) == ["model", "mode"], "a control with one choice is not offered")
    #expect(c[0].current == "a" && c[1].current == "full" && c[1].name == "模式" && c[1].via == "mode")
    #expect(SessionView.commands(evs).first?.name == "compact")
    let u = SessionView.usage(evs)
    #expect(u?.percent == 25 && u?.tokens == 1500)
    #expect(SessionView.usage([]) == nil)
}

/// Typing `/` at the start asks for a command, `@` as the last word for a file; picking one
/// completes that word.
@Test func composerSuggestionsCompleteTheWordBeingTyped() {
    #expect(SessionView.asking("/re") == .command("re"))
    #expect(SessionView.asking("/review now") == nil)
    #expect(SessionView.asking("看看 @src/ma") == .file("src/ma"))
    #expect(SessionView.asking("看看 src") == nil)
    #expect(SessionView.complete("/re", with: .command("review")) == "/review ")
    #expect(SessionView.complete("看看 @src/ma", with: .file("src/main.rs")) == "看看 @src/main.rs ")
}

/// A reported file change shows its changed lines with a little context.
@Test func inlineDiffsShowChangedLines() {
    let lines = InlineDiff.lines(old: "a\nb\nc", new: "a\nB\nc\nd")
    #expect(lines.map(\.0) == [" ", "-", "+", " ", "+"])
    #expect(lines.map(\.1) == ["a", "b", "B", "c", "d"])
}

/// Janus's own board tools show once: an agent's echo of them is hidden, whichever way the agent
/// names them (Codex `mcp.janus.*`, Claude Code `mcp__janus__*`).
@Test func boardToolEchoesAreHidden() {
    func call(_ type: String, _ text: String, _ tool: String) -> Event {
        Event(["seq": 1, "ts": 1, "type": JSON(type), "actor": "lead", "text": JSON(text), "data": ["tool": JSON(tool), "input": "{\"timeout_sec\":45}", "callId": "c1"]])
    }
    #expect(ToolRow.hidden(call("tool.call", "mcp__janus__wait", "other"), done: []))
    #expect(ToolRow.hidden(call("tool.started", "mcp__janus__wait", "other"), done: []))
    #expect(ToolRow.hidden(call("tool.call", "mcp.janus.wait", "other"), done: []))
    #expect(!ToolRow.hidden(call("tool.started", "调用 janus.wait，等待返回", "janus.wait"), done: []), "Janus's own record stays")
    #expect(!ToolRow.hidden(call("tool.call", "Read calc.py", "read"), done: []))
}

/// After delivery the lead's session is a regular conversation, not a closed one; other agents stay
/// closed.
@Test func aDeliveredLeadStaysInConversation() {
    var st = RunState()
    st.run.goal = "目标"
    st.run.status = "delivered"
    #expect(agentSession(st, "lead").converse)
    #expect(!agentSession(st, "m1").converse)
    st.run.status = "accepted"
    #expect(agentSession(st, "lead").converse)
    st.run.status = "cancelled"
    #expect(!agentSession(st, "lead").converse)
}

/// The session card keeps its own numbers: this session's context and the last turn, labelled apart
/// from the run's cumulative total.
@Test func sessionUsageIsLabelledApartFromTheRunTotal() {
    let evs = [
        ev(1, "session.usage", ["used": 50000, "size": 200000]),
        ev(2, "session.usage", ["totalTokens": 1500]),
    ]
    let u = SessionView.usage(evs)
    // The header keeps reading the same two numbers it did before.
    #expect(u?.percent == 25 && u?.tokens == 1500)
    #expect(u?.contextLabel == "本会话 上下文 25%")
    #expect(u?.turnLabel == "上一轮 1,500 tokens")
    #expect(u?.labels == "本会话 上下文 25% · 上一轮 1,500 tokens")

    // An agent that only reports context has no last-turn number; a 0 before it read anything is
    // not shown, as before.
    let contextOnly = [ev(1, "session.usage", ["used": 0, "size": 200000]), ev(2, "session.usage", ["used": 21000, "size": 200000])]
    let c = SessionView.usage(contextOnly)
    #expect(c?.tokens == nil && c?.turnLabel == nil)
    #expect(c?.contextLabel == "本会话 上下文 10%", "\(c?.contextLabel ?? "nil")")
    #expect(c?.percent == 10)

    // Only a last-turn number: the context label stays absent instead of showing 0.
    let turnOnly = [ev(1, "session.usage", ["totalTokens": 700])]
    let t = SessionView.usage(turnOnly)
    #expect(t?.contextLabel == nil)
    #expect(t?.turnLabel == "上一轮 700 tokens")
    #expect(t?.labels == "上一轮 700 tokens")
    #expect(SessionView.usage([]) == nil)
}

/// The usage card and the task usage card really draw: the same rows the data tests assert are
/// rendered into PNGs (light and dark), so a person can look at the result instead of trusting the
/// strings alone.
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
        ["id": "lead", "name": "主控", "lead": true],
        ["id": "researcher", "name": "调研者", "color": "#6a42d6"],
        ["id": "implementer", "name": "实现者"],
    ]]]
    run.events = events
    run.replay = events.count

    let st = run.shown
    #expect(st.usage.run.completeness == .partial, "the rendered card says partial")
    #expect(st.usage.run.summaryRows.first?.value.contains("部分上报") == true)

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
    let runCard = try render("run-usage-light.png", dark: false, Card { UsageSection(run: run, st: st) })
    let runCardDark = try render("run-usage-dark.png", dark: true, Card { UsageSection(run: run, st: st) })
    let detail = try render("run-usage-detail-light.png", dark: false, Card { UsageSection(run: run, st: st, initiallyOpen: true) })
    // The session card's header is the other place the numbers appear: it labels them 本会话/上一轮.
    let chat = ChatCard(run: run, st: st, title: "调研者", info: taskSession(st, st.tasks["T1"]!), target: .task("T1"))
        .frame(width: 560, height: 260)
    let session = try render("session-header-light.png", dark: false, chat)
    #expect(detail > 10_000 && session > 10_000)
    let taskCard = try render("task-usage-light.png", dark: false, Card { TaskUsageCard(run: run, st: st, task: st.tasks["T1"]!) })
    // The period bill of the settings pane, from a response shaped like `GET /api/usage`.
    let period: JSON = [
        "from": 1_760_000_000_000, "to": 1_762_592_000_000, "runs": 12, "turns": 3192, "sessions": 40, "unreportedSessions": 3,
        "activeDays": 21, "days": 30,
        "tokens": ["total": 6_450_000_000, "input": 200_000_000, "output": 21_400_000, "cachedRead": 6_228_600_000],
        "cost": ["lines": [["currency": "USD", "amount": 12.5, "sessions": 4]], "reportedSessions": 4, "missingSessions": 36],
        "estimate": ["amount": 6389.62, "unpricedTurns": 12, "unpricedModels": ["qwen3.8-27b-mlx"], "fetchedAt": 1_762_580_000_000],
        "agents": [
            ["id": "codex", "name": "Codex", "turns": 2900, "tokens": ["total": 5_900_000_000], "cost": ["lines": [], "reportedSessions": 0, "missingSessions": 30], "estimate": 5213.22,
             "models": [["model": "gpt-6-astra", "turns": 2500, "tokens": ["total": 5_100_000_000], "estimate": 4418.29], ["model": "gpt-6.1-sol", "turns": 400, "tokens": ["total": 800_000_000], "estimate": 794.93]]],
            ["id": "claude", "name": "Claude Code", "turns": 292, "tokens": ["total": 550_000_000], "cost": ["lines": [["currency": "USD", "amount": 12.5, "sessions": 4]], "reportedSessions": 4, "missingSessions": 6],
             "models": [["model": "claude-opus-5-5", "turns": 250, "tokens": ["total": 500_000_000]], ["model": "claude-haiku-5-5", "turns": 42, "tokens": ["total": 50_000_000]]]],
        ],
    ]
    let bill = UsageBill.period(period, issued: Date(timeIntervalSince1970: 1_762_590_000))
    #expect(bill.headline == "$6,389.62" && bill.totalRows.last?.value == "97%", "\(bill.headline) \(bill.totalRows)")
    #expect(bill.notes.first?.contains("models.dev") == true && bill.notes.contains { $0.contains("12 轮没有价格") })
    let periodCard = try render("period-bill-light.png", dark: false, Card(padding: 22) { UsageReceipt(bill: bill) }.frame(width: 520))
    #expect(periodCard > 10_000)
    #expect(runCard > 10_000 && runCardDark > 10_000 && taskCard > 10_000, "the cards drew: \(runCard)/\(runCardDark)/\(taskCard) bytes")
    print("usage card screenshots: \(dir.path)")
}
