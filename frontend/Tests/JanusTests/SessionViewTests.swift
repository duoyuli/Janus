import Foundation
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
