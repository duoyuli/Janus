import Foundation
import SwiftUI
import Testing
@testable import Janus

// A conversation's rows, what sits beside it, the composer's suggestions and the live previews
// are the server's (`backend/src/run_view/turns.rs`, `aside.rs`, `suggest.rs`, `preview.rs`); they
// are tested there. These tests read what it serves and apply the person's own clicks.

private func event(_ seq: Int, _ type: String, data: JSON = [:]) -> Event {
    Event(["seq": JSON(seq), "ts": JSON(seq * 1000), "type": JSON(type), "actor": "m1", "task": "T1", "text": "", "data": data])
}

private let events = [
    event(1, "attempt.started"), event(2, "session.prompt", data: ["channel": "system"]), event(3, "agent.thought"),
    event(4, "tool.call"), event(5, "agent.message", data: ["text": "好了"]), event(6, "tool.started"),
]

/// A turn the server laid out folded at the summary level, with its running group split off at
/// the step the agent is on.
private let served: JSON = [
    ["kind": "section", "id": 1, "seq": 1, "injected": [2], "gap": "none"],
    ["kind": "head", "id": -25, "turn": 3, "agent": "m1", "running": true, "open": false, "start": 3000, "end": 6000,
     "stats": "运行 1 条命令", "summary": "", "gap": "block"],
    ["kind": "activity", "id": -26, "events": [3, 4], "open": false, "summary": "思考", "turn": 3, "fold": true,
     "whole": [3, 4, 6], "sole": false, "gap": "line"],
    ["kind": "activity", "id": -50, "events": [6], "open": .null, "summary": "", "turn": 3, "fold": true, "partOf": -26, "sole": false, "gap": "line"],
    ["kind": "text", "id": 5, "events": [5], "reply": true, "turn": 3, "fold": false, "gap": "block"],
    ["kind": "edits", "id": -27, "turn": 3, "files": [["path": "/w/a.js", "added": 2, "removed": 1, "changes": [["seq": 4, "index": 0]]]], "gap": "block"],
]

private func kinds(_ rows: [Conversation.Row]) -> [String] {
    rows.map {
        switch $0 {
        case .section(let e, let injected, _): return "section:\(e.seq)+\(injected.count)"
        case .item(let e, _): return "item:\(e.seq)"
        case .head(_, _, let open, _): return "head:\(open.map { $0 ? "open" : "folded" } ?? "-")"
        case .activity(_, let es, let open, _, _): return "activity:\(es.count):\(open.map { $0 ? "open" : "folded" } ?? "-")"
        case .text(_, let es, let reply, _): return "text:\(es.count)\(reply ? ":reply" : "")"
        case .edits(_, let files, _): return "edits:\(files.count)"
        }
    }
}

/// A folded turn leaves out what folds with it; opening it shows its work; a click on the group
/// split off at the running step shows it whole.
@Test func thePersonsClicksApplyToTheServedRows() {
    let find = { (seq: Int) in events.first { $0.seq == seq } }
    #expect(kinds(Conversation.rows(served, events: find, open: [:])) == ["section:1+1", "head:folded", "text:1:reply", "edits:1"])
    #expect(kinds(Conversation.rows(served, events: find, open: [-25: true])) == ["section:1+1", "head:open", "activity:2:folded", "activity:1:-", "text:1:reply", "edits:1"])
    #expect(kinds(Conversation.rows(served, events: find, open: [-25: true, -26: true])) == ["section:1+1", "head:open", "activity:3:open", "text:1:reply", "edits:1"])
    let rows = Conversation.rows(served, events: find, open: [:])
    #expect(rows.map(\.gap) == [.none, .block, .block, .block])
    guard case .head(_, let head, _, _) = rows[1], case .edits(_, let files, _) = rows[3] else { Issue.record("rows"); return }
    #expect(head.running && head.stats == "运行 1 条命令")
    #expect(files[0].changes == [.init(seq: 4, index: 0)])
}

/// What sits beside a conversation reads back as served.
@Test func whatSitsBesideAConversationReadsBack() {
    let j: JSON = ["events": [1, 5], "composer": "managed", "open": true, "busy": true,
                   "plan": [["content": "读", "status": "completed"]],
                   "controls": [["id": "mode", "name": "模式", "via": "mode", "current": "full", "label": "模式：full", "options": [["value": "ask", "name": "ask"], ["value": "full", "name": "full"]]]],
                   "usage": ["percent": 25, "meter": "25%", "labels": "本会话 上下文 25%", "help": "h"],
                   "git": "Git 仓库 /a", "notes": ["1": ["note": " · 接上原会话", "git": "Git 仓库 /a"]],
                   "pending": [["id": "m1", "text": "先别改", "delivery": "queued", "label": "排队"]], "requests": ["a-1"], "rows": served]
    let s = SessionInfo(j, events: events)
    #expect(s.evs.map(\.seq) == [1, 5] && s.busy && s.composer == .managed)
    #expect(s.plan.map(\.content) == ["读"] && s.controls[0].label == "模式：full" && s.controls[0].options.count == 2)
    #expect(s.usage?.meter == "25%" && s.git == "Git 仓库 /a" && s.notes[1]?.note == " · 接上原会话")
    #expect(s.pending.map(\.delivery) == [.queued] && s.requests == ["a-1"])
}

/// The composer's suggestions: the arrows wrap, ↩ takes the row's completion, Esc closes the list
/// until the draft changes.
@Test func theSuggestionKeysMoveTakeAndClose() {
    let picker = SuggestionPicker()
    var draft = "/re"
    let binding = Binding(get: { draft }, set: { draft = $0 })
    #expect(!picker.handle(.down, draft: binding), "nothing open")
    let offered = Conversation.Suggestions(["rows": [["label": "/review", "detail": "", "completion": "/review "],
                                                    ["label": "/rename", "detail": "", "group": "Agent 命令", "completion": "/rename "]],
                                           "ghost": .null], draft: "/re")
    picker.rows = offered.rows
    #expect(offered.rows[1].group == "Agent 命令" && offered.ghost == nil)
    #expect(picker.handle(.up, draft: binding) && picker.index == 1)
    #expect(picker.handle(.accept, draft: binding) && draft == "/rename ")
    #expect(picker.handle(.dismiss, draft: binding) && !picker.open(for: draft))
    #expect(picker.open(for: "/ren"))
}

/// The run's diff, documents and live previews read back as served; a diff artifact says whose
/// changes it holds.
@Test func diffsDocumentsAndPreviewsReadBack() {
    let file = DiffFile(["id": 0, "path": "pay.js", "added": 1, "removed": 1, "untracked": false, "hunks": [
        ["id": 0, "header": "@@ -1 +1 @@", "lines": [["id": 1, "kind": "removed", "text": "a", "old": 1, "new": .null],
                                                    ["id": 2, "kind": "added", "text": "b", "old": .null, "new": 1]]]]])
    #expect(file.hunks[0].lines.map(\.kind) == [.removed, .added] && file.hunks[0].lines[1].new == 1 && file.hunks[0].lines[0].new == nil)
    let st = RunState(["diffs": ["4": [["path": "/w/a.js", "added": 1, "removed": 0, "lines": [["mark": "+", "text": "x"]]]]],
                       "docs": [["path": "/w/PRD.md", "source": "deliverable", "seq": 6, "label": "PRD.md", "name": "PRD.md", "dir": "~/w"]]], events: [])
    #expect(st.diffs[4]?[0].lines.map(\.mark) == ["+"])
    #expect(st.docs[0].source == .deliverable && st.docs[0].label == "PRD.md")
    let p = LivePreview(["text": "正在写", "thought": "", "tools": ["c1": ["title": "pytest", "tool": "execute", "input": "", "output": "1 passed", "status": "in_progress"]]])
    #expect(p.text == "正在写" && p.tools["c1"]?.output == "1 passed")
    let run = Artifact(["name": "run-1.diff", "kind": "run_diff", "seq": 1, "by": "janus"])
    #expect(run.isDiff && run.ofRun)
}
