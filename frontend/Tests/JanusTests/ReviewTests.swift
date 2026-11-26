import Foundation
import Testing
@testable import Janus

// Reviewing a delivery (docs/architecture/20261031-frontend.md#review): where the run stands after
// one, what the inbox asks for it, and what the review shows first.

private func event(_ seq: Int, _ type: String, actor: String = "janus", to: String? = nil, task: String? = nil, data: JSON = [:]) -> Event {
    var j: JSON = ["seq": JSON(seq), "ts": JSON(1_000_000 + seq * 1000), "type": JSON(type), "actor": JSON(actor), "text": "", "data": data]
    if let task { j["task"] = JSON(task) }
    if let to { j["to"] = JSON(to) }
    return Event(j)
}

private let run: JSON = ["id": "r1", "goal": "上线支付页", "createdAt": 1_000_000, "config": ["team": [["id": "m1", "name": "实现者"]]]]
private let idle: JSON = ["phase": "waiting_for_input", "turn": "idle"]

/// The person writes to the lead after a delivery: the message, then its routing as a prompt.
private func ask(_ seq: Int, _ text: String) -> [Event] {
    [event(seq, "user.message", actor: "user", to: "lead", data: ["id": JSON("m\(seq)"), "text": JSON(text)]),
     event(seq + 1, "message.routed", to: "lead", data: ["id": JSON("m\(seq)"), "delivery": "prompt"])]
}

private func delivered(_ seq: Int, _ d: JSON = ["headline": "做完了", "summary": "改了 a.rs"]) -> Event {
    event(seq, "run.delivered", data: d)
}

private let start = [event(1, "run.created", actor: "user", data: ["mode": "goal", "goal": "上线支付页"])]

@Test func aDeliveryIsReviewedOnlyWhileItWaitsForThePerson() {
    let first = start + [delivered(2)]
    #expect(reviewStage(derive(start), live: true) == nil, "nothing to review before a delivery")
    #expect(reviewStage(derive(first), live: true) == .awaiting)
    // The person asks for more: the lead works on it, and the delivery waits.
    let working = first + ask(3, "顺便改一下文案")
    #expect(reviewStage(derive(working), live: true) == .leadWorking)
    #expect(reviewStage(derive(working), live: false) == .awaiting, "no lead session is there after a restart")
    // It answered without changing anything: the same delivery waits again.
    #expect(reviewStage(derive(working + [event(5, "agent.status", actor: "lead", data: idle)]), live: true) == .awaiting)
    // It changed something: a second delivery, of this round.
    let second = working + [delivered(5)]
    let s = derive(second)
    #expect(reviewStage(s, live: true) == .awaiting && s.result?.n == 2 && s.result?.since == 2 && s.deliveries.count == 2)
    #expect(roundRequests(s, s.result!).map(\.text) == ["顺便改一下文案"])
    #expect(roundRequests(s, s.deliveries[0]).isEmpty, "the first delivery answers the goal")
    #expect(reviewStage(derive(second + [event(6, "risk.accepted", actor: "user")]), live: true) == .accepted)
}

@Test func afterADeliveryThePageSaysWhoItWaitsOn() {
    let first = start + [delivered(2)] + ask(3, "再查一遍")
    // The person interrupts the lead and holds it, until they hand it back.
    let held = first + [event(5, "session.interrupted", actor: "user", to: "lead"), event(6, "agent.status", actor: "lead", data: idle)]
    #expect(reviewStage(derive(held), live: true) == .held)
    #expect(reviewStage(derive(held + [event(7, "control.released", actor: "user", to: "lead")]), live: true) == .leadWorking)
    // The lead hands out subtasks: the team works, the run is running again.
    let team = first + [event(5, "orchestration.accepted", actor: "lead", data: ["tasks": [["id": "T2", "title": "改", "agent": "m1", "deps": [], "accept": ["好"]]]])]
    #expect(reviewStage(derive(team), live: true) == .teamWorking)
    let approval = team + [event(6, "attempt.started", task: "T2"), event(7, "approval.requested", actor: "m1", task: "T2", data: ["key": "a-1", "command": "rm"])]
    #expect(reviewStage(derive(approval), live: true) == .asking)
    // The merge: the lead resolves a conflict; then it is the person's call.
    #expect(reviewStage(derive(start + [delivered(2, ["merge": ["state": "resolving", "note": "冲突"]])]), live: true) == .resolving)
    #expect(reviewStage(derive(start + [delivered(2, ["merge": ["state": "conflict", "note": "仍然冲突"]])]), live: true) == .blocked(conflict: true))
    #expect(reviewStage(derive(start + [delivered(2, ["merge": ["state": "blocked", "note": "先移走"]])]), live: true) == .blocked(conflict: false))
}

/// The inbox asks for a delivery only where the person has to act, one entry per delivery.
@Test func theInboxAsksForADeliveryByItsStage() {
    func kinds(_ evs: [Event]) -> [String] { Inbox.review(run: run, state: derive(evs), workspace: "W", live: true).map(\.kind) }
    let first = start + [delivered(2)]
    let entry = Inbox.review(run: run, state: derive(first), workspace: "W", live: true)
    #expect(entry.map(\.key) == ["r1/delivered-1"] && entry[0].text == "做完了")
    #expect(kinds(first + ask(3, "再改")) == [], "the lead works on it")
    let second = Inbox.review(run: run, state: derive(first + ask(3, "再改") + [delivered(5)]), workspace: "W", live: true)
    #expect(second.map(\.key) == ["r1/delivered-2"], "a new delivery is a new entry: ignoring the first does not hide it")
    #expect(kinds(first + ask(3, "再查") + [event(5, "session.interrupted", actor: "user", to: "lead"), event(6, "agent.status", actor: "lead", data: idle)]) == ["held"])
    #expect(kinds(start + [delivered(2, ["merge": ["state": "conflict", "note": "仍然冲突"]])]) == ["merge_blocked"])
    #expect(kinds(start + [delivered(2), event(3, "risk.accepted", actor: "user")]) == [])
}

/// The lead's questions come first, then what Janus found; the executors' own risks stay with
/// their tasks.
@Test func whatNeedsLookingAtIsTheLeadsQuestionsThenJanussFindings() {
    let d = Delivery(RunResult(d: [
        "decisions": ["被忽略的文件要带回吗？"],
        "risks": [["task": "T3", "text": "T3：失败，没有交付结果"]],
        "leadEdits": ["a.rs", "b.rs"],
        "acceptance": [
            ["task": "T1", "title": "改", "status": "succeeded", "criteria": 2, "passed": 1, "checks": [["criterion": "一", "passed": true], ["criterion": "二", "passed": false]], "risks": ["没跑全量"]],
            ["task": "T2", "title": "写", "status": "succeeded", "criteria": 1, "passed": 1, "unverified": true, "traceGaps": ["改了 c.rs"], "risks": ["推断"]],
            ["task": "T4", "title": "测", "status": "succeeded", "criteria": 1, "passed": 1, "unverified": true],
        ],
    ], seq: 9))
    let s = reviewSignals(d)
    #expect(s.first == ReviewSignal(source: .lead, text: "被忽略的文件要带回吗？"))
    #expect(s.contains { $0.source == .janus && $0.text == "T3：失败，没有交付结果" && $0.task == "T3" })
    #expect(s.contains { $0.text.contains("T1") && $0.task == "T1" }, "a criterion that did not pass")
    #expect(s.contains { $0.text.contains("T2") && $0.detail == "改了 c.rs" }, "the trace gap")
    #expect(s.contains { $0.text.hasPrefix(expectedCopy("T2、T4", "T2, T4")) }, "tasks without an independent verification, in one line")
    #expect(s.contains { $0.detail == expectedCopy("a.rs、b.rs", "a.rs, b.rs") }, "the lead's own edits")
    #expect(!s.contains { $0.text.contains("没跑全量") || $0.text.contains("推断") }, "the executors' own risks are not here")
}

@Test func evidenceStrengthBandsTheMethods() {
    let checks = ["test", "command", "inspection", "reasoning", "unverified"].map { Check(["criterion": "x", "passed": true, "method": JSON($0)]) }
        + [Check(["criterion": "y", "passed": true])]
    let e = EvidenceStrength(EvidenceSummary(accept: [], checks: checks))
    #expect((e.ran, e.read, e.said) == (2, 1, 3))
    #expect(e.text == expectedCopy("运行 2 · 检查 1 · 只是说法 3", "Ran 2 · Read 1 · Only said 3"))
    let line = ReviewBody.factsLine([], changed: 0)
    #expect(line == expectedCopy("没有子任务，负责人自己完成 · 没有改动文件", "No subtasks, the lead did it itself · No files changed"))
}

/// A subtask's own risks mark its row but do not open it; a failed criterion, a trace gap or an
/// unconfirmed verification does.
@Test func onlyWhatJanusFindsOpensASubtaskRow() {
    let risky = DeliveryAcceptance(["task": "T1", "title": "改", "status": "succeeded", "criteria": 1, "passed": 1, "checks": [["criterion": "一", "passed": true]], "risks": ["没跑全量"]])!
    #expect(!risky.row.needsAttention)
    #expect(risky.row.markers == [expectedCopy("自述风险 1 条", "Self-reported risks: 1")])
    let gap = DeliveryAcceptance(["task": "T2", "title": "写", "status": "succeeded", "criteria": 1, "passed": 1, "traceGaps": ["改了 c.rs"]])!
    #expect(gap.row.needsAttention)
}
