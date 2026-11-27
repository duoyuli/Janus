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


/// A workspace's 「待处理」: one row per goal or Loop of that workspace, newest first, saying what
/// it waits on; other workspaces' entries are left out.
@Test func aWorkspaceGroupsWhatWaitsByGoalAndLoop() {
    let labels = ["question": "等你拍板", "delivered": "等你验收", "approval": "等你审批", "interrupted": "已中断"]
    func e(_ key: String, _ run: String, _ kind: String, wait: Bool = true) -> InboxEntry {
        InboxEntry(["key": JSON("\(run)/\(key)"), "run": JSON(run), "title": JSON(key), "ts": 0, "canWait": JSON(wait), "kind": JSON(kind), "label": JSON(labels[kind] ?? "")])
    }
    let open = [e("q", "loop-run", "question"), e("d", "g1", "delivered"), e("a", "g2", "approval", wait: false),
                e("q2", "g2", "question"), e("x", "elsewhere", "question")]
    let rows = Inbox.waiting(open, goals: [["id": "g1", "goal": "上线"], ["id": "g2", "goal": "修复"]],
                             loops: [["id": "l1", "run": "loop-run", "title": "issue 分拣"]])
    #expect(rows.map(\.id) == ["loop-run", "g1", "g2"])
    #expect(rows[0].loop?["id"] == "l1" && rows[0].label == "等你拍板" && rows[0].tone == .amber)
    #expect(rows[1].loop == nil && rows[1].label == "等你验收")
    #expect(rows[2].entries.count == 2 && rows[2].label == expectedCopy("2 件事", "2 Items") && rows[2].tone == .red, "an approval holds an agent up")
    // The leading glyph says what it waits on, not who: a Loop's question looks like a question.
    #expect(rows.map(\.symbol) == ["questionmark.bubble.fill", "checkmark.seal.fill", "tray.full.fill"])
    #expect(Inbox.waiting([e("i", "g1", "interrupted")], goals: [["id": "g1"]], loops: []).first?.symbol == "pause.circle.fill")
}

/// Where a note came from, as the workspace page and its sheet say it.
@Test func aNoteSaysWhoWroteItWhere() {
    let byLead: JSON = ["who": "lead", "author": "PMO", "runGoal": "修复导出", "runStatus": "delivered", "updatedAt": 0]
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
