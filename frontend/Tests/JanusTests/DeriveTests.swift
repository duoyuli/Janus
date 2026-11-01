import Foundation
import Testing
@testable import Janus

/// Builds an event log the way the orchestrator records one.
private struct Log {
    var events: [Event] = []
    mutating func add(_ type: String, actor: String = "janus", to: String? = nil, task: String? = nil, attempt: Int? = nil, text: String = "", data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(1_000_000 + events.count * 1000), "type": JSON(type), "actor": JSON(actor), "text": JSON(text), "data": data]
        if let to { j["to"] = JSON(to) }
        if let task { j["task"] = JSON(task) }
        if let attempt { j["attempt"] = JSON(attempt) }
        events.append(Event(j))
    }
}

private func goalRun() -> Log {
    var log = Log()
    log.add("run.created", actor: "user", data: ["mode": "goal", "goal": "给 divide 增加除零保护\n细节……", "cwd": "/tmp/x", "git": true])
    log.add("orchestration.accepted", actor: "lead", data: ["tasks": [
        ["id": "T1", "title": "调研", "agent": "researcher", "deps": [], "accept": ["列出调用方"], "delegated": true],
        ["id": "T2", "title": "实现", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过", "有除零保护"], "delegated": true],
    ]])
    log.add("task.ready", task: "T1")
    log.add("attempt.started", to: "researcher", task: "T1", attempt: 1, data: ["session": "s1", "workspace": "工作目录 /tmp/x"])
    return log
}

@Test func goalRunTracksTasksAndAgents() {
    var log = goalRun()
    let s1 = derive(log.events)
    #expect(s1.run.status == "running")
    #expect(s1.run.mode == "goal")
    #expect(s1.order == ["T1", "T2"])
    #expect(s1.tasks["T1"]?.status == "running")
    #expect(s1.tasks["T1"]?.live == true)
    #expect(s1.tasks["T2"]?.status == "pending")
    #expect(s1.agents["researcher"]?.phase == "thinking")
    #expect(s1.run.inPlace == true)

    log.add("result.submitted", actor: "researcher", task: "T1", attempt: 1, data: ["summary": "三处调用", "evidence": ["calc.py:3"], "checks": [["criterion": "列出调用方", "passed": true, "evidence": "calc.py:3"]]])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "三处调用", "evidence": ["calc.py:3"]])
    let s2 = derive(log.events)
    #expect(s2.tasks["T1"]?.status == "succeeded")
    #expect(s2.tasks["T1"]?.checks.first?.passed == true)
    #expect(s2.tasks["T1"]?.live == false)
    #expect(s2.tasks["T1"]?.attempts.last?.status == "succeeded")
    #expect(s2.agents["researcher"]?.phase == "idle")
}

@Test func requestsEnterAndLeaveTheInbox() {
    var log = goalRun()
    log.add("approval.requested", actor: "researcher", task: "T1", attempt: 1, text: "运行命令", data: ["key": "a-1", "command": "rm -rf build", "options": [["optionId": "ok", "name": "允许", "kind": "allow_once"]]])
    log.add("question.escalated", actor: "lead", data: ["key": "q-lead-1", "question": "用哪个框架？", "options": ["A", "B"], "recommend": "A", "reason": "影响整体"])
    let s = derive(log.events)
    #expect(s.inbox.map(\.key) == ["a-1", "q-lead-1"])
    #expect(s.tasks["T1"]?.status == "waiting_approval")
    #expect(s.inbox[1].task == nil)
    #expect(s.inbox[1].d["options"].strings == ["A", "B"])
    #expect(s.agents["researcher"]?.phase == "waiting_approval")

    log.add("approval.granted", actor: "user", to: "researcher", task: "T1", data: ["key": "a-1"])
    log.add("question.answered", actor: "user", to: "lead", data: ["key": "q-lead-1", "answer": "A"])
    let s2 = derive(log.events)
    #expect(s2.inbox.isEmpty)
    #expect(s2.tasks["T1"]?.status == "running")
    #expect(s2.answers["a-1"] == "granted")
}

@Test func delegatedFailuresStayWithTheLead() {
    var log = goalRun()
    log.add("verification.failed", task: "T1", data: ["failures": ["漏了验收标准"]])
    log.add("task.failed", task: "T1", data: ["error": "用完重试次数"])
    let s = derive(log.events)
    #expect(s.tasks["T1"]?.status == "failed")
    // The lead orchestrated T1: the person is not asked.
    #expect(s.inbox.isEmpty)
    let f = boardFacts(s) { $0 }
    #expect(f.open.contains { $0.key == "ofT1" && !$0.you })
}

@Test func messagesAreTrackedUntilDelivered() {
    var log = goalRun()
    log.add("user.message", actor: "user", to: "researcher", task: "T1", data: ["id": "m1", "text": "顺便看看测试"])
    var s = derive(log.events)
    #expect(s.msgs["m1"]?.delivery == "pending")
    log.add("message.routed", to: "researcher", task: "T1", data: ["id": "m1", "delivery": "queued"])
    s = derive(log.events)
    #expect(s.msgs["m1"]?.delivery == "queued")
    #expect(s.msgs["m1"]?.deliveredSeq == nil)
    log.add("message.delivered", to: "researcher", task: "T1", data: ["ids": ["m1"]])
    s = derive(log.events)
    #expect(s.msgs["m1"]?.deliveredSeq == log.events.count)
}

@Test func deliveryAndAcceptance() {
    var log = goalRun()
    log.add("run.delivered", data: ["summary": "完成", "partial": false, "confidence": "高", "risks": [], "evidence": [], "deliverables": [], "diagnosis": ["actions": [], "refs": []]])
    var s = derive(log.events)
    #expect(s.run.status == "delivered")
    #expect(s.result?.d["confidence"] == "高")
    log.add("risk.accepted", actor: "user")
    s = derive(log.events)
    #expect(s.run.status == "accepted")
    #expect(s.result?.accepted != nil)
}

@Test func reopenedTasksDropTheirAmendments() {
    var log = goalRun()
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x"])
    log.add("result.amended", actor: "lead", task: "T1", data: ["text": "以修订为准"])
    #expect(derive(log.events).tasks["T1"]?.amendments == ["以修订为准"])
    log.add("task.reopened", actor: "lead", task: "T1", data: ["instructions": "再查一遍"])
    let s = derive(log.events)
    #expect(s.tasks["T1"]?.amendments.isEmpty == true)
    #expect(s.tasks["T1"]?.desc == "再查一遍")
    #expect(s.tasks["T1"]?.status == "ready")
}

@Test func boardFactsLinkResultsToHandoffs() {
    var log = goalRun()
    log.add("result.submitted", actor: "researcher", task: "T1", attempt: 1, data: ["summary": "三处调用", "evidence": ["a"], "risks": ["b"]])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "三处调用"])
    log.add("context.handoff", actor: "janus", to: "implementer", task: "T2", data: ["from": "T1", "summary": "三处调用", "artifacts": []])
    log.add("user.message", actor: "user", to: "lead", data: ["text": "注意兼容", "delivery": "steer"])
    let f = boardFacts(derive(log.events)) { $0 }
    #expect(f.results.count == 1)
    #expect(f.results[0].usedBy == ["T2"])
    #expect(f.handoffs.map(\.to) == ["T2"])
    #expect(f.decisions.map(\.title).contains("主控提交编排"))
    #expect(f.decisions.map(\.title).contains("给 lead 的补充"))
}

@Test func jsonParsesBooleansAndNumbersApart() throws {
    let j = try JSON.parse(Data(#"{"a":true,"b":1,"c":1.5,"d":"x","e":null,"f":[1,"y"]}"#.utf8))
    #expect(j["a"] == .bool(true))
    #expect(j["b"].int == 1)
    #expect(j["b"].str == "1")
    #expect(j["c"].double == 1.5)
    #expect(j["d"].str == "x")
    #expect(j["e"].isNull)
    #expect(j["f"].array.count == 2)
    #expect(!JSON.string("").truthy && JSON.string("x").truthy)
}

@Test func goalTitleAndPlainText() {
    #expect(L.goalTitle("\n  **修复** `divide`\n第二行") == "修复 divide")
    #expect(L.plain("见 [calc.py](/tmp/calc.py)") == "见 calc.py")
    #expect(L.tilde("/Users/someone/repo") == "~/repo")
}

@Test func markdownBlocks() {
    #expect(MD.blocks("结论：\n\n- 甲\n- 乙\n# 标题") == [.para("结论："), .list(["甲", "乙"]), .heading("标题")])
    // A list sent on one line is split.
    #expect(MD.blocks("改了： - 甲 - 乙") == [.para("改了："), .list(["甲 - 乙"])])
}

@Test func envLinesKeepTheUsersOwnLines() {
    let text = "A=1\nMINE=keep\nB=2"
    let vars: [String: String?] = ["A": String?.none, "B": "3", "C": "has space"]
    #expect(Setup.setEnvLines(text, vars) == "MINE=keep\nB=3\nC=\"has space\"")
}

@Test func planControlsAreHidden() {
    let c: JSON = ["options": ["controls": [
        ["id": "plan-mode", "type": "select", "options": []],
        ["id": "mode", "type": "select", "permission": true, "current": "ask", "options": [["value": "plan", "name": "Plan"], ["value": "ask", "name": "Ask"]]],
    ]]]
    let ctrls = Setup.controls(c)
    #expect(ctrls.map { $0["id"].str } == ["mode"])
    #expect(ctrls[0]["options"].array.map { $0["value"].str } == ["ask"])
    #expect(Setup.permLabel([:], c) == "Ask")
}
