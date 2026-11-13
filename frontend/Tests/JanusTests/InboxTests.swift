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
    let entries = Inbox.entries(run: run, state: derive(events), workspace: "商城")
    #expect(entries.count == 2)
    let approval = entries.first { $0.title.contains("请求你决定") }
    let decision = entries.first { $0.title.contains("主控请你决定") }
    #expect(approval?.canWait == false && approval?.text == "npm publish" && approval?.workspace == "商城")
    #expect(decision?.canWait == true && decision?.ts == 1_005_000)
    #expect(entries.allSatisfy { $0.key.hasPrefix("r1/") && $0.goal == "上线支付页" && !$0.canSettle })
}

@Test func finishedRunsAskForAcceptanceOrAFinalWord() {
    let delivered = Inbox.entries(row: ["id": "r2", "status": "delivered", "goal": "g"], workspace: "")
    #expect(delivered.count == 1 && delivered[0].canWait && !delivered[0].canSettle)
    let stopped = Inbox.entries(row: ["id": "r3", "status": "interrupted", "goal": "g"], workspace: "")
    #expect(stopped.count == 1 && stopped[0].canSettle)
    #expect(Inbox.entries(row: ["id": "r4", "status": "accepted"], workspace: "").isEmpty)
    #expect(Inbox.entries(row: ["id": "r5", "status": "cancelled"], workspace: "").isEmpty)
}

@Test func snoozedItemsComeBackOnTimeAndBlockingOnesCannotBeSnoozed() {
    func e(_ key: String, wait: Bool, settle: Bool = false, ts: Double) -> InboxEntry {
        InboxEntry(key: key, run: "r", goal: "", workspace: "", title: key, text: "", sub: "", ts: ts, canWait: wait, canSettle: settle)
    }
    let all = [e("a", wait: true, ts: 1), e("b", wait: false, ts: 2), e("c", wait: true, settle: true, ts: 3)]
    // b keeps an agent waiting: a snooze on it is ignored. c was settled.
    let (open, later) = Inbox.split(all, snoozed: ["a": 100, "b": 100], settled: ["c"], now: 50)
    #expect(open.map(\.key) == ["b"])
    #expect(later.map(\.key) == ["a"])
    let (open2, later2) = Inbox.split(all, snoozed: ["a": 100], settled: [], now: 150)
    #expect(open2.map(\.key) == ["c", "b", "a"], "newest first; a is back once its time came")
    #expect(later2.isEmpty)
}

/// A plain choice is answered in the inbox: an approval's options in the app's words, and a
/// decision's options with the lead's pick first; each click is the run page's own action.
@Test func plainChoicesCanBeAnsweredFromTheInbox() {
    let events = [
        event(1, "run.created", actor: "user", data: ["mode": "goal", "goal": "上线支付页"]),
        event(2, "orchestration.accepted", actor: "lead", data: ["tasks": [["id": "T1", "title": "清理旧构建", "agent": "m1", "deps": [], "accept": ["干净"]]]]),
        event(3, "attempt.started", task: "T1"),
        event(4, "approval.requested", actor: "m1", task: "T1", data: ["key": "a-1", "command": "rm -rf build", "options": [
            ["optionId": "o1", "name": "Allow once", "kind": "allow_once"], ["optionId": "o2", "name": "Always allow", "kind": "allow_always"], ["optionId": "o3", "name": "Reject", "kind": "reject_once"]]]),
        event(5, "question.escalated", actor: "lead", data: ["key": "q-lead-1", "question": "先上哪个地区？", "options": ["国内", "海外"], "recommend": "海外"]),
        event(6, "input.requested", actor: "m1", task: "T1", data: ["key": "i-1", "message": "账号", "schema": ["type": "object"]]),
    ]
    let entries = Inbox.entries(run: run, state: derive(events), workspace: "商城")
    let approval = entries.first { $0.key.hasSuffix("approval") }!
    #expect(approval.title == "实现者请求你决定" && approval.sub == "T1 · 清理旧构建", "named like the request card")
    #expect(approval.actions.map(\.label) == ["允许一次", "始终允许", "拒绝"])
    #expect(approval.actions[0].prominent && approval.actions[0].body["option"].str == "o1" && approval.actions[0].body["allow"] == true)
    #expect(approval.actions[2].body["allow"] == false && approval.actions[2].body["label"].str == "Reject", "the agent's own words go back")
    #expect(approval.actions[1].help.contains("不再询问") && approval.actions[1].help.contains("Always allow"))
    let decision = entries.first { $0.title.contains("主控请你决定") }!
    #expect(decision.actions.map(\.label) == ["国内", "海外"] && decision.actions[1].prominent)
    #expect(decision.actions[0].body["type"].str == "answer" && decision.actions[0].body["key"].str == "q-lead-1")
    #expect(entries.first { $0.key.hasSuffix("input") }?.actions.isEmpty == true, "a form opens the run")
}

@Test func permissionOptionsWithoutOptionsAllowOrRejectOnce() {
    let opts = L.permissionOptions(.null)
    #expect(opts.map(\.label) == ["允许一次", "拒绝"] && opts.allSatisfy { $0.id.isEmpty })
    #expect(L.permissionOptions([["optionId": "x", "name": "Proceed", "kind": "other"]]).first?.label == "Proceed", "an unknown kind keeps the agent's words")
}
