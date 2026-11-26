import Foundation
import SwiftUI
import Testing
@testable import Janus

/// A task session's events as the orchestrator records them, one second apart.
private struct Log {
    var events: [Event] = []
    mutating func add(_ type: String, actor: String = "m1", to: String? = nil, task: String? = "T1", text: String = "", data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(1_000_000 + events.count * 1000), "type": JSON(type), "actor": JSON(actor), "text": JSON(text), "data": data]
        if let to { j["to"] = JSON(to) }
        if let task { j["task"] = JSON(task) }
        events.append(Event(j))
    }

    /// An attempt with Janus's system layer, then one turn: thinking, two commands, an edit, a
    /// remark between them, and the reply.
    static func turn() -> Log {
        var l = Log()
        l.add("attempt.started", actor: "janus", to: "m1", data: ["runtime": "Claude Code", "prompt": "做 T1"])
        l.add("session.prompt", actor: "janus", to: "m1", data: ["channel": "system", "via": "system", "prompt": "协作约定"])
        l.add("session.prompt", actor: "janus", to: "m1", data: ["channel": "instructions", "via": "system", "prompt": "指令"])
        l.add("agent.thought", data: ["text": "先跑测试"])
        l.add("tool.call", data: ["callId": "c1", "tool": "execute", "input": "pytest -q", "output": "3 passed", "status": "completed"])
        l.add("agent.message", data: ["text": "测试都过了，"])
        l.add("agent.message", data: ["text": "接着改导出。"])
        l.add("tool.call", data: ["callId": "c2", "tool": "edit", "input": "export.py", "status": "completed",
                                  "diffs": [["path": "/w/export.py", "old": "a\nb", "new": "a\nc\nd"]]])
        l.add("tool.call", data: ["callId": "c3", "tool": "execute", "input": "pytest -q", "output": "4 passed", "status": "completed"])
        l.add("agent.message", data: ["text": "导出加好了，"])
        l.add("agent.message", data: ["text": "测试 4 个都过。"])
        return l
    }
}

private func kinds(_ rows: [ChatTurns.Row]) -> [String] {
    rows.map {
        switch $0 {
        case .section(let e, let injected): return "section:\(e.type.rawValue)+\(injected.count)"
        case .item(let e): return "item:\(e.type.rawValue)"
        case .head(_, let open): return "head:\(open.map { $0 ? "open" : "folded" } ?? "-")"
        case .activity(_, let es, let open): return "activity:\(es.count):\(open.map { $0 ? "open" : "folded" } ?? "-")"
        case .text(let es, let reply): return "text:\(es.count)\(reply ? ":reply" : "")"
        case .edits(_, let files): return "edits:\(files.count)"
        }
    }
}

/// A finished turn folds to the agent's line: the reply and the files it changed stay, the work
/// opens on a click. Janus's injected layer stays in the flow under its attempt.
@Test func aFinishedTurnFoldsToItsReply() {
    let rows = ChatTurns.rows(Log.turn().events, .init())
    #expect(kinds(rows) == ["section:attempt.started+2", "head:folded", "text:2:reply", "edits:1"])
    guard case .head(let t, _) = rows[1] else { Issue.record("no head"); return }
    #expect(t.stats == ChatTurns.Stats(commands: 2, reads: 0, searches: 0, edited: 1, other: 0, board: 0))
    #expect(t.end - t.start == 7000, "from its first event to its last")
    guard case .edits(_, let files) = rows[3] else { return }
    #expect((files[0].added, files[0].removed) == (2, 1))
}

/// Opened (by the person, or the detail level), the work shows in order: calls grouped, the
/// remark between them quieter than the reply.
@Test func anOpenTurnShowsItsWorkInOrder() {
    let evs = Log.turn().events
    let rows = ChatTurns.rows(evs, .init(detail: true))
    #expect(kinds(rows) == ["section:attempt.started+2", "head:open", "activity:2:open", "text:2", "activity:2:open", "text:2:reply", "edits:1"])
    // The person's own click wins over the level.
    let head = rows[1].id
    let folded = ChatTurns.rows(evs, .init(detail: true, open: [head: false]))
    #expect(kinds(folded) == ["section:attempt.started+2", "head:folded", "text:2:reply", "edits:1"])
}

/// While the agent is at it, the turn stays open with its newest calls, and nothing is called
/// its reply yet.
@Test func aRunningTurnStaysOpen() {
    var l = Log.turn()
    l.events.removeLast(2)
    let rows = ChatTurns.rows(l.events, .init(busy: true))
    #expect(kinds(rows) == ["section:attempt.started+2", "head:open", "activity:2:folded", "text:2", "activity:2:open", "edits:1"])
}

/// What must not be missed stays when the work folds: a request, a message cut into the turn,
/// a result handed in; a person's message opens the next turn.
@Test func pinnedItemsStayWhenTheTurnFolds() {
    var l = Log.turn()
    l.events.removeLast(2)
    l.add("approval.requested", data: ["key": "k1"])
    l.add("tool.call", data: ["callId": "c4", "tool": "execute", "input": "git push", "status": "completed"])
    l.add("agent.message", data: ["text": "推上去了。"])
    l.add("result.submitted", data: ["summary": "完成"])
    l.add("user.message", actor: "user", to: "m1", task: nil, data: ["text": "再看看", "delivery": "prompt"])
    l.add("agent.message", data: ["text": "好的。"])
    let rows = ChatTurns.rows(l.events, .init())
    #expect(kinds(rows) == ["section:attempt.started+2", "head:folded", "item:approval.requested", "text:1:reply", "item:result.submitted", "edits:1",
                            "item:user.message", "head:-", "text:1:reply"])
}

/// Calls that are all a turn did need no line of their own under the turn's.
@Test func callsAloneSitUnderTheTurnsLine() {
    var l = Log()
    l.add("tool.call", data: ["callId": "c1", "tool": "read", "input": "a.py", "status": "completed"])
    l.add("tool.started", data: ["callId": "c2", "tool": "execute", "input": "pytest", "live": true])
    #expect(kinds(ChatTurns.rows(l.events, .init(busy: true))) == ["head:open", "activity:2:-"])
}

/// A preview shows only what the records do not hold yet: a frame older than the newest
/// recorded text is dropped, a newer record clears it, a finished call drops its frame, and the
/// end of the turn drops the rest.
@Test func livePreviewsGiveWayToTheRecord() {
    var live = ChatLive.State()
    var log = Log()
    log.add("agent.message", data: ["text": "前半句"])
    live.recorded(log.events[0])
    live.apply(["kind": "say", "actor": "m1", "task": "T1", "seq": 0, "text": "前半", "thought": ""])
    #expect(live.of(task: "T1", agent: nil) == nil, "made before the record")
    live.apply(["kind": "say", "actor": "m1", "task": "T1", "seq": 1, "text": "后半句", "thought": ""])
    #expect(live.of(task: "T1", agent: nil)?.text == "后半句")
    live.apply(["kind": "tool", "actor": "m1", "task": "T1", "seq": 1, "callId": "c9", "title": "pytest", "tool": "execute", "input": "pytest", "output": "1 passed", "status": "in_progress"])
    #expect(live.tool(actor: "m1", task: "T1", id: "c9")?.output == "1 passed")
    log.add("agent.message", data: ["text": "后半句"])
    live.recorded(log.events[1])
    #expect(live.of(task: "T1", agent: nil)?.text == "", "the record took over")
    log.add("tool.call", data: ["callId": "c9", "tool": "execute", "status": "completed"])
    live.recorded(log.events[2])
    #expect(live.tool(actor: "m1", task: "T1", id: "c9") == nil)
    live.apply(["kind": "tool", "actor": "m1", "task": "T1", "seq": 1, "callId": "c9", "title": "pytest", "tool": "execute", "input": "", "output": "", "status": "in_progress"])
    #expect(live.tool(actor: "m1", task: "T1", id: "c9") == nil, "a late frame of a finished call")
    live.apply(["kind": "say", "actor": "m1", "task": "T1", "seq": 3, "text": "还在写", "thought": ""])
    live.apply(["kind": "end", "actor": "m1", "task": "T1", "seq": 3])
    #expect(live.of(task: "T1", agent: nil) == nil)
}

/// Commands rank as Lody ranks them: the name, its prefix, a word's prefix, anywhere, letters in
/// order, then only the description; nothing typed lists the agent's own before its skills.
@Test func slashCommandsRankByHowTheyMatch() {
    let cmds = ["review-branch", "compact", "pr-review", "rename", "context", "mcp:review"].map { SessionView.Command(name: $0, description: "", hint: "") }
        + [SessionView.Command(name: "simplify", description: "review changed code", hint: "", skill: true)]
    #expect(SessionView.rank(cmds, "review").map(\.name) == ["review-branch", "pr-review", "mcp:review", "simplify"])
    #expect(SessionView.rank(cmds, "cpt").map(\.name) == ["compact"])
    #expect(SessionView.rank(cmds, "co").map(\.name) == ["compact", "context", "simplify"], "the description (\"code\") comes last")
    #expect(SessionView.rank(cmds, "").last?.name == "simplify")
    let rows = Suggestions.rows(.command(""), commands: cmds, files: [])
    #expect(rows.compactMap(\.group) == [L.text("Agent 命令"), L.text("Skill")])
}

/// A command picked from the list shows its arguments grey until the person types on.
@Test func aPickedCommandShowsItsArguments() {
    let cmds = [SessionView.Command(name: "review", description: "", hint: "branch name")]
    #expect(SessionView.argumentHint("/review ", cmds) == "branch name")
    #expect(SessionView.argumentHint("/review m", cmds) == nil)
    #expect(SessionView.argumentHint("/other ", cmds) == nil)
}

/// The keys of the list: arrows wrap, ↩ takes the row, Esc closes it until the draft changes.
@Test func theSuggestionKeysMoveTakeAndClose() {
    let picker = SuggestionPicker()
    var draft = "/re"
    let binding = Binding(get: { draft }, set: { draft = $0 })
    #expect(!picker.handle(.down, draft: binding), "nothing open")
    picker.rows = [.init(pick: .command("review"), label: "/review", detail: ""), .init(pick: .command("rename"), label: "/rename", detail: "")]
    #expect(picker.handle(.up, draft: binding) && picker.index == 1)
    #expect(picker.handle(.accept, draft: binding) && draft == "/rename ")
    #expect(picker.handle(.dismiss, draft: binding) && !picker.open(for: draft))
    #expect(picker.open(for: "/ren"))
}

/// A file named by its path in the working directory and by its full path is one file.
@Test func aFileCountsOnceWhateverItsPath() {
    var l = Log()
    l.add("tool.call", data: ["callId": "c1", "tool": "edit", "status": "completed", "paths": ["pay.js"], "diffs": [["path": "/w/pay.js", "old": "", "new": "x"]]])
    l.add("agent.message", data: ["text": "改好了"])
    guard case .head(let t, _)? = ChatTurns.rows(l.events, .init(cwd: "/w")).first else { Issue.record("no head"); return }
    #expect(t.stats.edited == 1)
}

/// Janus's prompt that carries the person's message is part of the turn the message opened.
@Test func thePromptCarryingAMessageStaysInItsTurn() {
    var l = Log()
    l.add("user.message", actor: "user", to: "lead", task: nil, data: ["text": "接着做", "delivery": "prompt"])
    l.add("session.prompt", actor: "janus", to: "lead", task: nil, data: ["channel": "followup", "prompt": "继续：接着做"])
    l.add("agent.message", actor: "lead", task: nil, data: ["text": "好。"])
    #expect(kinds(ChatTurns.rows(l.events, .init())) == ["item:user.message", "item:session.prompt", "head:-", "text:1:reply"])
}

/// A turn that only says where the session stands has no work to fold: its status line stays.
@Test func aStatusLineIsNotWork() {
    var l = Log()
    l.add("agent.status", actor: "lead", task: nil, text: "会话已打开，等待你的消息", data: ["turn": "idle"])
    #expect(kinds(ChatTurns.rows(l.events, .init())) == ["item:agent.status"])
}
