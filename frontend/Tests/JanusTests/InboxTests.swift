import Foundation
import Testing
@testable import Janus

// What waits on the person is the server's (`GET /api/inbox`, `backend/src/run_view/inbox.rs`),
// in the app's language, with what they put off or ignored; it is tested there. These tests read
// the served entries and the app's own grouping.

/// An entry reads back as the server words it: its kind, whether it can wait, when it comes back,
/// and its one-click answers with the request body to send.
@Test func anEntryReadsBackAsServed() {
    let e = InboxEntry([
        "key": "r1/o5question", "run": "r1", "runKind": "goal", "goal": "上线支付页", "workspace": "w1", "workspaceName": "商城",
        "kind": "question", "label": "等你拍板", "title": "负责人等你拍板：先上哪个地区？", "text": "", "sub": "", "ts": 1_005_000,
        "canWait": true, "snoozedUntil": 2_000_000,
        "actions": [["label": "海外", "help": "负责人建议", "prominent": true, "body": ["type": "answer", "key": "q-lead-1", "task": "", "answer": "海外", "note": ""]]],
    ])
    #expect(e.workspace == "商城" && e.kind == "question" && e.canWait && e.ts == 1_005_000 && e.snoozedUntil == 2_000_000)
    #expect(e.actions.first?.prominent == true && e.actions.first?.body["key"] == "q-lead-1")
}


/// A workspace's 「待处理」 row draws what it waits on: a glyph for the one thing's kind or a stack
/// for several, in the colour the server gives the row.
@Test func aWaitingRowShowsWhatItWaitsOn() {
    func e(_ key: String, _ kind: String, wait: Bool = true) -> InboxEntry {
        InboxEntry(["key": JSON(key), "run": "g", "title": JSON(key), "ts": 0, "canWait": JSON(wait), "kind": JSON(kind)])
    }
    func row(_ entries: [InboxEntry], tone: Tone = .amber) -> WaitingRow {
        WaitingRow(id: "g", run: .null, loop: nil, title: "上线", label: "", tone: tone, entries: entries)
    }
    #expect(row([e("q", "question")]).symbol == "questionmark.bubble.fill")
    #expect(row([e("d", "delivered")]).symbol == "checkmark.seal.fill")
    #expect(row([e("i", "interrupted")]).symbol == "pause.circle.fill")
    let two = row([e("a", "approval", wait: false), e("q", "question")], tone: .red)
    #expect(two.symbol == "tray.full.fill" && two.tone == .red)
}

/// Where a note came from, as the workspace page and its sheet say it.
@Test func aNoteSaysWhoWroteItWhere() {
    let byLead: JSON = ["who": "lead", "author": "PMO", "runTitle": "修复导出", "runStatus": "delivered", "updatedAt": 0]
    #expect(MemoryText.source(byLead).hasPrefix("PMO · "))
    #expect(MemoryText.source(byLead).contains("修复导出"))
    let cancelled: JSON = ["who": "lead", "author": "PMO", "runGoal": "试试", "runStatus": "cancelled", "updatedAt": 0]
    #expect(MemoryText.source(cancelled).contains(expectedCopy("来自已取消的任务", "from a cancelled run")))
    let mine: JSON = ["who": "user", "author": "你", "updatedAt": 0]
    #expect(MemoryText.source(mine).hasPrefix(expectedCopy("你 · ", "You · ")))
    #expect(MemoryText.briefLine(["title": "测试命令", "body": "先装依赖\n\n再跑 just test"]) == "测试命令：先装依赖 再跑 just test")
}

/// Only what is new since the last look is announced, and nothing on the first look.
@Test func notificationsAreForWhatIsNew() {
    func e(_ k: String) -> InboxEntry { InboxEntry(["key": JSON(k), "run": "r", "title": JSON(k), "canWait": true]) }
    #expect(Notifier.fresh([e("a"), e("b")], seen: nil).isEmpty)
    #expect(Notifier.fresh([e("a"), e("b")], seen: ["a"]).map(\.key) == ["b"])
}
