import Foundation
import Testing
@testable import Janus

private func event(_ seq: Int, _ type: String, actor: String = "janus", task: String? = nil, data: JSON = [:]) -> Event {
    var j: JSON = ["seq": JSON(seq), "ts": JSON(1_000_000 + seq * 1000), "type": JSON(type), "actor": JSON(actor), "text": "", "data": data]
    if let task { j["task"] = JSON(task) }
    return Event(j)
}

private let run: JSON = ["id": "r1", "goal": "上线支付页", "createdAt": 1_000_000, "config": ["team": [["id": "m1", "name": "实现者"]]]]

/// The inbox shows exactly the run page's 「需要你处理」, and only what nothing waits on can be put off.
@Test func inboxTakesThePersonsItemsFromALiveRun() {
    let events = [
        event(1, "run.created", actor: "user", data: ["mode": "goal", "goal": "上线支付页"]),
        event(2, "orchestration.accepted", actor: "lead", data: ["tasks": [["id": "T1", "title": "实现", "agent": "m1", "deps": [], "accept": ["能付款"]]]]),
        event(3, "attempt.started", task: "T1"),
        event(4, "approval.requested", actor: "m1", task: "T1", data: ["key": "a-1", "command": "npm publish"]),
        event(5, "question.escalated", actor: "lead", data: ["key": "q-lead-1", "question": "先上哪个地区？", "options": ["国内", "海外"]]),
        // The lead's own blocker: the lead settles it, it is not the person's.
        event(6, "board.posted", actor: "m1", task: "T1", data: ["kind": "blocker", "text": "缺少测试账号"]),
    ]
    let entries = Inbox.entries(run: run, state: derive(events), workspace: "商城", live: true)
    #expect(entries.count == 2)
    let approval = entries.first { $0.kind == "approval" }
    let decision = entries.first { $0.kind == "question" }
    #expect(approval?.canWait == false && approval?.text == "npm publish" && approval?.workspace == "商城")
    #expect(decision?.canWait == true && decision?.ts == 1_005_000)
    #expect(entries.allSatisfy { $0.key.hasPrefix("r1/") && $0.goal == "上线支付页" })
}

@Test func finishedRunsAskForAFinalWord() {
    // A delivered run is read like a live one: its delivery speaks for it (`Inbox.review`).
    #expect(Inbox.entries(row: ["id": "r2", "status": "delivered", "goal": "g"], workspace: "").isEmpty)
    let stopped = Inbox.entries(row: ["id": "r3", "status": "interrupted", "goal": "g"], workspace: "")
    #expect(stopped.count == 1 && stopped[0].canWait)
    #expect(Inbox.entries(row: ["id": "r4", "status": "accepted"], workspace: "").isEmpty)
    #expect(Inbox.entries(row: ["id": "r5", "status": "cancelled"], workspace: "").isEmpty)
}

/// A Loop's session closes between wakes (the run reads `interrupted`), and a chat's between
/// messages: that is how they rest, not a stopped run to continue.
@Test func restingLoopsAndChatsAskNothing() {
    #expect(Inbox.entries(row: ["id": "l1", "kind": "loop", "status": "interrupted", "goal": "昨日精选"], workspace: "W").isEmpty)
    #expect(Inbox.entries(row: ["id": "c1", "kind": "chat", "status": "interrupted", "goal": "g"], workspace: "").isEmpty)
    #expect(Inbox.entries(row: ["id": "r6", "kind": "goal", "status": "interrupted", "goal": "g"], workspace: "").count == 1)
}

@Test func snoozedItemsComeBackOnTimeAndBlockingOnesCannotBeSnoozed() {
    func e(_ key: String, wait: Bool, ts: Double) -> InboxEntry {
        InboxEntry(key: key, run: "r", goal: "", workspace: "", title: key, text: "", sub: "", ts: ts, canWait: wait)
    }
    let all = [e("a", wait: true, ts: 1), e("b", wait: false, ts: 2), e("c", wait: true, ts: 3)]
    // b keeps an agent waiting: a snooze on it has no effect. c was ignored.
    let (open, later) = Inbox.split(all, snoozed: ["a": 100, "b": 100], ignored: ["c"], now: 50)
    #expect(open.map(\.key) == ["b"])
    #expect(later.map(\.key) == ["a"])
    let (open2, later2) = Inbox.split(all, snoozed: ["a": 100], ignored: [], now: 150)
    #expect(open2.map(\.key) == ["c", "b", "a"], "newest first; a is back once its time came")
    #expect(later2.isEmpty)
}

/// A delivery is the lead's turn ending: the run asks for acceptance once, not also for a reply,
/// whichever of the two events comes first. Before the delivery, a turn that ends still asks.
@Test func aDeliveredRunAsksOnceNotAlsoForAReply() {
    let idle: JSON = ["phase": "waiting_for_input", "turn": "idle"]
    let start = [event(1, "run.created", actor: "user", data: ["mode": "goal", "goal": "上线支付页"])]
    let asking = Inbox.entries(run: run, state: derive(start + [event(2, "agent.status", actor: "lead", data: idle)]), workspace: "W", live: true)
    #expect(asking.map(\.kind) == ["session"])
    for tail in [[event(2, "agent.status", actor: "lead", data: idle), event(3, "run.delivered", actor: "lead", data: ["summary": "好了"])],
                 [event(2, "run.delivered", actor: "lead", data: ["summary": "好了"]), event(3, "agent.status", actor: "lead", data: idle)]] {
        #expect(Inbox.entries(run: run, state: derive(start + tail), workspace: "W", live: true).map(\.kind) == ["delivered"])
    }
}

/// Any entry can be ignored, one that holds an agent up too: it leaves both 「待处理」 and
/// 「稍后处理」, and the rest stay as they were.
@Test func anyEntryCanBeIgnored() {
    func e(_ key: String, wait: Bool) -> InboxEntry {
        InboxEntry(key: key, run: "r", goal: "", workspace: "", title: key, text: "", sub: "", ts: 0, canWait: wait)
    }
    let all = [e("delivered", wait: true), e("session", wait: false), e("snoozed", wait: true)]
    let (open, later) = Inbox.split(all, snoozed: ["snoozed": 100], ignored: ["session", "snoozed"], now: 50)
    #expect(open.map(\.key) == ["delivered"])
    #expect(later.isEmpty)
}

/// A plain choice is answered in the inbox: an approval's options in the app's words, and a
/// decision's options with the lead's pick first; each click is the run page's own action.
@Test func plainChoicesCanBeAnsweredFromTheInbox() throws {
    let events = [
        event(1, "run.created", actor: "user", data: ["mode": "goal", "goal": "上线支付页"]),
        event(2, "orchestration.accepted", actor: "lead", data: ["tasks": [["id": "T1", "title": "清理旧构建", "agent": "m1", "deps": [], "accept": ["干净"]]]]),
        event(3, "attempt.started", task: "T1"),
        event(4, "approval.requested", actor: "m1", task: "T1", data: ["key": "a-1", "command": "rm -rf build", "options": [
            ["optionId": "o1", "name": "Allow once", "kind": "allow_once"], ["optionId": "o2", "name": "Always allow", "kind": "allow_always"], ["optionId": "o3", "name": "Reject", "kind": "reject_once"]]]),
        event(5, "question.escalated", actor: "lead", data: ["key": "q-lead-1", "question": "先上哪个地区？", "options": ["国内", "海外"], "recommend": "海外"]),
        event(6, "input.requested", actor: "m1", task: "T1", data: ["key": "i-1", "message": "账号", "schema": ["type": "object"]]),
    ]
    let entries = Inbox.entries(run: run, state: derive(events), workspace: "商城", live: true)
    let approval = try #require(entries.first { $0.kind == "approval" })
    #expect(approval.title == expectedCopy("「实现者」等你拍板", "实现者 needs your call") && approval.sub == "T1 · 清理旧构建", "named like the request card")
    #expect(approval.actions.map(\.label) == [expectedCopy("允许一次", "Allow Once"), expectedCopy("始终允许", "Always Allow"), expectedCopy("拒绝", "Deny")])
    #expect(approval.actions[0].prominent && approval.actions[0].body["option"].str == "o1" && approval.actions[0].body["allow"] == true)
    #expect(approval.actions[2].body["allow"] == false && approval.actions[2].body["label"].str == "Reject", "the agent's own words go back")
    #expect(approval.actions[1].help.contains(expectedCopy("不再询问", "will be allowed automatically")) && approval.actions[1].help.contains("Always allow"))
    let decision = try #require(entries.first { $0.kind == "question" })
    #expect(decision.actions.map(\.label) == ["国内", "海外"] && decision.actions[1].prominent)
    #expect(decision.actions[0].body["type"].str == "answer" && decision.actions[0].body["key"].str == "q-lead-1")
    #expect(entries.first { $0.key.hasSuffix("input") }?.actions.isEmpty == true, "a form opens the run")
}

@Test func permissionOptionsWithoutOptionsAllowOrRejectOnce() {
    let opts = L.permissionOptions(.null)
    #expect(opts.map(\.label) == [expectedCopy("允许一次", "Allow Once"), expectedCopy("拒绝", "Deny")] && opts.allSatisfy { $0.id.isEmpty })
    #expect(L.permissionOptions([["optionId": "x", "name": "Proceed", "kind": "other"]]).first?.label == "Proceed", "an unknown kind keeps the agent's words")
}

@Test func idleRefreshReusesEntriesForLongRunsUntilAnInputChanges() {
    let zh = Locale(identifier: "zh-Hans")
    for id in ["long-a", "long-b"] {
        let row: JSON = ["id": JSON(id), "status": "running", "goal": "目标"]
        let events = (1...20000).map { event($0, "agent.message") }
        let entries = Inbox.entries(row: ["id": JSON(id), "status": "delivered"], workspace: "W")
        let cached = InboxModel.CachedRun(events: events, row: row, workspace: "W", locale: zh, entries: entries)
        #expect(cached.unchangedEntries(newEvents: [], row: row, workspace: "W", locale: zh) == entries)
        #expect(cached.unchangedEntries(newEvents: [["seq": 20001]], row: row, workspace: "W", locale: zh) == nil)
        #expect(cached.unchangedEntries(newEvents: [], row: row, workspace: "W", locale: Locale(identifier: "en")) == nil)
        #expect(cached.unchangedEntries(newEvents: [], row: row, workspace: "Renamed", locale: zh) == nil)
        var renamed = row
        renamed["goal"] = "新目标"
        #expect(cached.unchangedEntries(newEvents: [], row: renamed, workspace: "W", locale: zh) == nil)
    }
}

/// A workspace's 「待处理」: one row per goal or Loop of that workspace, newest first, saying what
/// it waits on; other workspaces' entries are left out.
@Test func aWorkspaceGroupsWhatWaitsByGoalAndLoop() {
    func e(_ key: String, _ run: String, _ kind: String, wait: Bool = true) -> InboxEntry {
        InboxEntry(key: "\(run)/\(key)", run: run, goal: "", workspace: "", title: key, text: "", sub: "", ts: 0, canWait: wait, kind: kind)
    }
    let open = [e("q", "loop-run", "question"), e("d", "g1", "delivered"), e("a", "g2", "approval", wait: false),
                e("q2", "g2", "question"), e("x", "elsewhere", "question")]
    let rows = Inbox.waiting(open, goals: [["id": "g1", "goal": "上线"], ["id": "g2", "goal": "修复"]],
                             loops: [["id": "l1", "run": "loop-run", "title": "issue 分拣"]])
    #expect(rows.map(\.id) == ["loop-run", "g1", "g2"])
    #expect(rows[0].loop?["id"] == "l1" && rows[0].label == expectedCopy("等你拍板", "Your Call") && rows[0].tone == .amber)
    #expect(rows[1].loop == nil && rows[1].label == expectedCopy("等你验收", "Ready to Accept"))
    #expect(rows[2].entries.count == 2 && rows[2].label == expectedCopy("2 件事", "2 Items") && rows[2].tone == .red, "an approval holds an agent up")
    // The leading glyph says what it waits on, not who: a Loop's question looks like a question.
    #expect(rows.map(\.symbol) == ["questionmark.bubble.fill", "checkmark.seal.fill", "tray.full.fill"])
    #expect(Inbox.waiting([e("i", "g1", "interrupted")], goals: [["id": "g1"]], loops: []).first?.symbol == "pause.circle.fill")
}

/// A consolidation of the team memory asks only to be looked at: once it ends, ignored once seen;
/// one that did not finish says so; while it works its row asks nothing.
@Test func aConsolidationAsksToBeLookedAt() {
    let done = Inbox.entries(row: ["id": "d1", "kind": "dream", "status": "delivered", "goal": "整理团队记忆"], workspace: "W")
    #expect(done.count == 1 && done[0].kind == "dream" && done[0].canWait)
    #expect(done[0].title == expectedCopy("团队记忆整理好了", "Team memory consolidated"))
    let broke = Inbox.entries(row: ["id": "d2", "kind": "dream", "status": "failed"], workspace: "W")
    #expect(broke.count == 1 && broke[0].kind == "dream_failed" && broke[0].canWait)
    #expect(Inbox.entries(row: ["id": "d3", "kind": "dream", "status": "running"], workspace: "W").isEmpty)
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
