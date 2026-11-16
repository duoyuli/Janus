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
        ["id": "T1", "title": "调研", "agent": "researcher", "deps": [], "accept": ["列出调用方"]],
        ["id": "T2", "title": "实现", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过", "有除零保护"]],
    ]])
    log.add("task.ready", task: "T1")
    log.add("attempt.started", to: "researcher", task: "T1", attempt: 1, data: ["session": "s1", "workspace": "工作目录 /tmp/x"])
    return log
}

@Test func goalRunTracksTasksAndAgents() {
    var log = goalRun()
    let s1 = derive(log.events)
    #expect(s1.run.status == .running)
    #expect(s1.order == ["T1", "T2"])
    #expect(s1.tasks["T1"]?.status == .running)
    #expect(s1.tasks["T1"]?.live == true)
    #expect(s1.tasks["T2"]?.status == .pending)
    #expect(s1.agents["researcher"]?.phase == "thinking")

    log.add("result.submitted", actor: "researcher", task: "T1", attempt: 1, data: ["summary": "三处调用", "evidence": ["calc.py:3"], "checks": [["criterion": "列出调用方", "passed": true, "evidence": "calc.py:3"]]])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "三处调用", "evidence": ["calc.py:3"], "risks": [], "checks": [["criterion": "列出调用方", "passed": true, "evidence": "calc.py:3"]]])
    let s2 = derive(log.events)
    #expect(s2.tasks["T1"]?.status == .succeeded)
    #expect(s2.tasks["T1"]?.checks.first?.passed == true)
    #expect(s2.tasks["T1"]?.live == false)
    #expect(s2.tasks["T1"]?.attempts.last?.status == .succeeded)
    #expect(s2.agents["researcher"]?.phase == "idle")
}

@Test func requestsEnterAndLeaveTheInbox() {
    var log = goalRun()
    log.add("approval.requested", actor: "researcher", task: "T1", attempt: 1, text: "运行命令", data: ["key": "a-1", "command": "rm -rf build", "options": [["optionId": "ok", "name": "允许", "kind": "allow_once"]]])
    log.add("question.escalated", actor: "lead", data: ["key": "q-lead-1", "question": "用哪个框架？", "options": ["A", "B"], "recommend": "A", "reason": "影响整体"])
    let s = derive(log.events)
    #expect(s.inbox.map(\.key) == ["a-1", "q-lead-1"])
    #expect(s.tasks["T1"]?.status == .waitingApproval)
    #expect(s.inbox[1].task == nil)
    #expect(s.inbox[1].d["options"].strings == ["A", "B"])
    #expect(s.agents["researcher"]?.phase == "waiting_approval")

    log.add("approval.granted", actor: "user", to: "researcher", task: "T1", data: ["key": "a-1"])
    log.add("question.answered", actor: "user", to: "lead", data: ["key": "q-lead-1", "answer": "A"])
    let s2 = derive(log.events)
    #expect(s2.inbox.isEmpty)
    #expect(s2.tasks["T1"]?.status == .running)
    #expect(s2.answers["a-1"] == "granted")
}

@Test func failuresStayWithTheLead() {
    var log = goalRun()
    log.add("verification.failed", task: "T1", data: ["failures": ["漏了验收标准"]])
    log.add("task.failed", task: "T1", data: ["error": "用完重试次数"])
    let s = derive(log.events)
    #expect(s.tasks["T1"]?.status == .failed)
    // The lead deals with failures: the person is not asked.
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
    log.add("run.delivered", data: ["summary": "完成", "briefSuggestions": ["定价页在 docs/pricing.md"], "risks": [], "evidence": [], "deliverables": []])
    var s = derive(log.events)
    #expect(s.run.status == .delivered)
    #expect(s.result?.d["briefSuggestions"].strings == ["定价页在 docs/pricing.md"])
    log.add("risk.accepted", actor: "user")
    s = derive(log.events)
    #expect(s.run.status == .accepted)
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
    #expect(s.tasks["T1"]?.status == .ready)
}

@Test func stalledTasksStayOpenUntilTheAgentMoves() {
    var log = goalRun()
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "idle", "idleMs": 600000])
    var f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.contains { $0.title == "T1 可能卡住了" && !$0.you })
    log.add("tool.call", actor: "researcher", task: "T1", attempt: 1, data: ["tool": "read", "status": "completed"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "T1 可能卡住了" })
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "idle", "idleMs": 600000])
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "idle", "resolved": true])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "T1 可能卡住了" }, "the backend saw the agent move again")
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "loop", "repeats": 5])
    log.add("attempt.started", to: "researcher", task: "T1", attempt: 2, data: ["session": "s2"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "T1 可能卡住了" }, "a new attempt starts clean")
    log.add("task.stalled", to: "lead", task: "T1", attempt: 2, data: ["reason": "loop", "repeats": 5])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "T1 可能卡住了" })
}

@Test func leadStallsAndConflictsOpenOnTheBoard() {
    var log = goalRun()
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "idle", "idleMs": 600000])
    log.add("task.conflict", actor: "janus", to: "lead", task: "T1", data: ["other": "T2", "paths": ["calc.py"]])
    var f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.contains { $0.title == "负责人可能卡住了" && $0.you }, "nobody but the person watches the lead")
    #expect(f.open.contains { $0.title == "T1 和 T2 改了同一个文件" && $0.text == "calc.py" && !$0.you })
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "idle", "resolved": true])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "负责人可能卡住了" })
    #expect(!f.open.contains { $0.title.contains("改了同一个文件") }, "neither task is running any more")
}

/// A repeated-call report has no explicit resolution: the lead doing anything again closes it.
@Test func aLeadStallClearsWhenTheLeadMovesAgain() {
    var log = goalRun()
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "loop", "tool": "Read", "repeats": 5])
    var f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.contains { $0.title == "负责人可能卡住了" && $0.you })
    log.add("agent.message", actor: "lead", text: "换个办法")
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == "负责人可能卡住了" })
}

@Test func tracesOpenOnTheBoardUntilTheLeadActs() {
    var log = goalRun()
    log.add("task.claimed", actor: "researcher", to: "lead", task: "T1", data: ["paths": ["notes.md"]])
    log.add("task.conflict", actor: "janus", to: "lead", task: "T1", data: ["other": "T2", "paths": ["calc.py"], "owned": true])
    var st = derive(log.events)
    #expect(st.tasks["T1"]?.owns == ["notes.md"], "a claim adds to what the task owns")
    var f = boardFacts(st) { $0 }
    #expect(f.open.contains { $0.title == "T1 改了 T2 负责的文件" && $0.text == "calc.py" && !$0.you })
    #expect(f.handoffs.contains { $0.summary == "认领：notes.md" })
    // The same edit also seen as both tasks editing one file: the overstep says it all.
    log.add("task.conflict", actor: "janus", to: "lead", task: "T1", data: ["other": "T2", "paths": ["calc.py"]])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.filter { $0.tasks.contains("T2") && $0.text == "calc.py" }.count == 1)
    #expect(f.open.first { $0.text == "calc.py" }?.kind == "overstep")
    let shared = derive(log.events).events.last!
    #expect(!ChatItem.visible(shared, st: derive(log.events)), "the conversation shows one line, not two")
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x", "traceGaps": ["改了负责范围（notes.md）以外的文件：calc.py"]])
    st = derive(log.events)
    f = boardFacts(st) { $0 }
    #expect(!f.open.contains { $0.title.contains("负责的文件") }, "the task that stepped over stopped")
    #expect(st.tasks["T1"]?.traceGaps.count == 1)
    #expect(f.open.contains { $0.title == "T1 的结果和工作目录对不上" && $0.text.contains("calc.py") })
    log.add("result.amended", actor: "lead", task: "T1", data: ["text": "calc.py 的改动由 T2 负责"])
    #expect(!boardFacts(derive(log.events)) { $0 }.open.contains { $0.title.contains("对不上") }, "the lead corrected it")
    log.add("task.reopened", actor: "lead", task: "T1", data: ["reason": "lead.retry"])
    #expect(derive(log.events).tasks["T1"]?.traceGaps.isEmpty == true)
}

@Test func aSteeredMessageIsShownOnceAndMarkedRead() {
    var log = goalRun()
    log.add("user.message", actor: "user", to: "lead", data: ["id": "m1", "text": "先看 T1", "delivery": "pending"])
    log.add("message.routed", actor: "janus", to: "lead", data: ["id": "m1", "delivery": "steer"])
    var st = derive(log.events)
    #expect(st.msgs["m1"]?.deliveredSeq == nil)
    log.add("message.delivered", actor: "janus", to: "lead", data: ["ids": ["m1"], "applied": true])
    st = derive(log.events)
    #expect(st.msgs["m1"]?.deliveredSeq != nil)
    #expect(!ChatItem.visible(log.events.last!, st: st), "applying it only updates the inserted bubble")
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
    #expect(f.decisions.map(\.title).contains("负责人提交编排"))
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

/// The progress timeline draws each attempt from its start to its end.
@Test func attemptsRecordWhenTheyEnd() {
    var log = goalRun()
    let start = derive(log.events).tasks["T1"]?.attempts.last
    #expect(start?.end == nil)
    log.add("attempt.failed", task: "T1", attempt: 1, data: ["error": "超时"])
    let failed = derive(log.events)
    #expect(failed.tasks["T1"]?.attempts.last?.end == log.events.last?.ts)
    log.add("attempt.started", to: "researcher", task: "T1", attempt: 2, data: ["session": "s2"])
    log.add("run.interrupted")
    let s = derive(log.events)
    #expect(s.tasks["T1"]?.attempts.count == 2)
    #expect(s.tasks["T1"]?.attempts.last?.end == log.events.last?.ts)
}

/// The scheduler's seven states plus the six the app derives for display are each a case of
/// `TaskStatus` (a new one needs a case, its label and a line in docs/domains/run_lifecycle.md);
/// the run states are each a `RunStatus`, and nothing else is.
@Test func everyStatusTheServerSendsIsACase() {
    let scheduler = ["pending", "running", "blocked", "succeeded", "failed", "cancelled", "skipped_by_decision"]
    let derived = ["ready", "assigned", "verifying", "verification_failed", "waiting_input", "waiting_approval"]
    for s in scheduler + derived { #expect(TaskStatus(rawValue: s) != nil, "\(s)") }
    let runs = ["planning", "running", "waiting_input", "delivered", "accepted", "cancelled", "failed", "interrupted"]
    for s in runs { #expect(RunStatus(s) != .unknown, "\(s)") }
    #expect(RunStatus("idle") == .unknown)
    #expect(Set(runs.map(RunStatus.init).filter(\.isActive)) == [.planning, .running, .waitingInput])
    #expect(Set(runs.map(RunStatus.init).filter(\.canResume)) == [.interrupted, .failed])
}

@Test func suggestedLinesAreAppendedToTheSharedMaterial() {
    let ws: JSON = ["id": "w", "brief": "目标用户是独立开发者\n"]
    #expect(BriefSuggestions.adding(["定价页在 docs/pricing.md"], to: ws)["brief"].str == "目标用户是独立开发者\n- 定价页在 docs/pricing.md")
    #expect(BriefSuggestions.adding(["A", "B"], to: ["id": "w"])["brief"].str == "- A\n- B")
}

/// The five methods read back with the words the task page shows; a value outside the set stays as
/// written instead of becoming one of them, and a check with none is 「未说明」, not 「推理判断」.
@Test func evidenceMethodsLabelThemselves() {
    for (raw, label) in [("test", "测试"), ("command", "命令"), ("inspection", "检查"), ("reasoning", "推理判断"), ("unverified", "未核实")] {
        let c = Check(["criterion": "标准1", "passed": true, "method": JSON(raw), "evidence": "证据"])
        #expect(c.method?.rawValue == raw)
        #expect(c.methodLabel == label)
    }
    // A check that names no method: shown as 未说明, never as reasoning.
    let none = Check(["criterion": "标准1", "passed": true, "evidence": "看起来没问题"])
    #expect(none.method == nil && none.badMethod == nil)
    #expect(none.methodLabel == "未说明")
    // An illegal value is kept as recorded and never mapped onto an allowed method.
    let bad = Check(["criterion": "标准1", "passed": true, "method": "vibes", "evidence": "看着没问题"])
    #expect(bad.method == nil)
    #expect(bad.badMethod == "vibes")
    #expect(bad.methodLabel == "方法「vibes」不合法（未说明）")
}

/// Every criterion gets a row even with no check, and every check is kept even when the result
/// names more checks than criteria: nothing is truncated to the first one.
@Test func everyCriterionAndEveryCheckGetsARow() {
    let checks = [
        Check(["criterion": "多给的一条", "passed": true, "method": "inspection", "evidence": "读了代码"]),
        Check(["criterion": "旧结果", "passed": true, "evidence": "第一条证据"]),
        Check(["criterion": "旧结果第二条", "passed": false, "evidence": "第二条证据"]),
    ]
    let rows = CriterionCheck.rows(accept: ["标准1", "标准2"], checks: checks)
    #expect(rows.count == 3, "the extra check is not dropped")
    #expect(rows[0].criterion == "标准1" && rows[0].check?.method == .inspection)
    #expect(rows[2].check?.evidence == "第二条证据", "extra checks keep their own criterion")
    let unanswered = CriterionCheck.rows(accept: ["标准1", "标准2"], checks: [checks[1]])
    #expect(unanswered.count == 2 && unanswered[1].check == nil, "a criterion with no check still gets a row")
    #expect(CriterionCheck.rows(accept: [], checks: checks).map(\.criterion) == ["多给的一条", "旧结果", "旧结果第二条"])
}

/// Reasoning, unverified and a missing method only remind; they neither pass nor fail a task, and
/// an illegal method is counted apart instead of being turned into one of the five.
@Test func weakerEvidenceOnlyReminds() {
    let checks = [
        Check(["criterion": "跑过测试", "passed": true, "method": "test", "evidence": "cargo test"]),
        Check(["criterion": "推断不影响", "passed": true, "method": "reasoning", "evidence": "调用方只有这一处"]),
        Check(["criterion": "本机没有 Xcode", "passed": true, "method": "unverified", "evidence": "本机没有 Xcode"]),
        Check(["criterion": "没写方法", "passed": true, "evidence": "看起来没问题"]),
        Check(["criterion": "写错方法", "passed": true, "method": "vibes", "evidence": "看着没问题"]),
    ]
    let s = EvidenceSummary(accept: ["跑过测试", "推断不影响", "本机没有 Xcode", "没写方法", "写错方法"], checks: checks)
    #expect(s.criteria == 5 && s.passed == 5, "reminders do not change how many passed")
    #expect(s.unspecified == 1)
    #expect(s.invalid == 1)
    #expect(s.reminders.count == 3, "\(s.reminders)")
    #expect(s.reminders.contains { $0.contains("推理判断") && $0.contains("不是运行得到的结果") })
    #expect(s.reminders.contains { $0.contains("未核实") && $0.contains("本机没有 Xcode") })
    #expect(s.reminders.contains { $0.contains("没有说明证据方法") })
    #expect(s.methods.map(\.method) == ["test", "reasoning", "unverified"], "counts keep the five methods' order: \(s.methods)")
    #expect(s.countsText == "测试 1 条、推理判断 1 条、未核实 1 条、未说明 1 条、方法不合法 1 条", "\(s.countsText)")
    #expect(EvidenceSummary(accept: [], checks: []).countsText == "没有记录")
}

/// The task page reads the checks, methods, evidence and risks from the conclusion event too, so a
/// replayed or resumed run shows the same evidence; the independent verifier is kept apart.
@Test func aConclusionKeepsMethodsEvidenceRisksAndItsVerifier() {
    var log = goalRun()
    // A verifier that the orchestration declared with `verifies`.
    log.add("orchestration.accepted", actor: "lead", data: ["tasks": [
        ["id": "T1", "title": "调研", "agent": "researcher", "deps": [], "accept": ["列出调用方"]],
        ["id": "T2", "title": "实现", "agent": "implementer", "deps": ["T1"], "accept": ["测试通过", "有除零保护"]],
        ["id": "V1", "title": "验证", "agent": "reviewer", "deps": ["T2"], "accept": ["复现行为"], "verifies": "T2"],
    ]])
    log.add("result.submitted", actor: "implementer", task: "T2", attempt: 1, data: [
        "summary": "加了保护", "evidence": ["calc.py:12", "docs/calc.md"], "risks": ["没有覆盖负数"],
        "checks": [
            ["criterion": "测试通过", "passed": true, "method": "test", "evidence": "cargo test 通过"],
            ["criterion": "有除零保护", "passed": true, "method": "reasoning", "evidence": "调用方只有这一处"],
        ],
    ])
    log.add("task.succeeded", task: "T2", attempt: 1, data: [
        "summary": "加了保护", "evidence": ["calc.py:12", "docs/calc.md"], "risks": ["没有覆盖负数"], "traceGaps": [],
        "checks": [["criterion": "测试通过", "passed": true, "method": "test", "evidence": "cargo test 通过"]],
    ])
    log.add("result.submitted", actor: "reviewer", task: "V1", attempt: 1, data: [
        "summary": "复现了", "evidence": ["python3 calc.py"],
        "checks": [["criterion": "复现行为", "passed": true, "method": "command", "evidence": "python3 calc.py → 除零提示"]],
    ])
    log.add("task.succeeded", task: "V1", attempt: 1, data: [
        "summary": "复现了", "evidence": ["python3 calc.py"], "risks": [],
        "checks": [["criterion": "复现行为", "passed": true, "method": "command", "evidence": "python3 calc.py → 除零提示"]],
    ])
    let st = derive(log.events)
    let t2 = st.tasks["T2"]
    #expect(t2?.checks.count == 1 && t2?.checks[0].method == .test, "the conclusion's checks and methods are kept")
    #expect(t2?.risks == ["没有覆盖负数"], "the executor's risks stay with the task")
    #expect(t2?.evidence.count == 2, "both pieces of evidence are kept")
    #expect(st.verifiers(of: "T2").map(\.id) == ["V1"])
    #expect(st.verifiers(of: "T2").first?.checks.first?.methodLabel == "命令")
    #expect(st.verifiers(of: "T1").isEmpty)
}

/// The delivery's per-task record keeps every check with its method, the independent verification,
/// the trace gaps and the risks; a delivery written before the record existed is read from the tasks.
@Test func deliveryReadsEveryTaskAcceptanceApart() throws {
    let record: JSON = [
        "task": "T1", "title": "实现", "status": "succeeded", "label": "已完成",
        "criteria": 2, "passed": 2,
        "methods": [["method": "test", "label": "测试", "count": 1], ["method": "unverified", "label": "未核实", "count": 1]],
        "unspecified": 0, "invalid": [],
        "reminders": ["第 2 条「界面正常」未核实：本机没有 Xcode"],
        "checks": [
            ["criterion": "测试通过", "passed": true, "method": "test", "evidence": "cargo test 96 passed"],
            ["criterion": "界面正常", "passed": true, "method": "unverified", "evidence": "本机没有 Xcode"],
        ],
        "verifications": [[
            "task": "V1", "status": "failed", "label": "失败", "passed": false, "summary": "没能复现",
            "checks": [["criterion": "复现行为", "passed": false, "method": "command", "evidence": "运行后没有提示"]],
            "evidence": ["输出见上"], "risks": ["只在 macOS 上测过"],
        ]],
        "traceGaps": ["改了负责范围以外的文件：notes.md"],
        "risks": ["旧版本没有回归测试"],
    ]
    var log = goalRun()
    log.add("run.delivered", data: ["summary": "完成", "evidence": [["text": "T1：cargo test 96 passed", "seq": 3]], "acceptance": [record], "risks": ["T1：旧版本没有回归测试"]])
    let st = derive(log.events)
    let records = deliveryAcceptance(st)
    #expect(records.count == 1)
    let a = try #require(records.first)
    #expect((a.summary.passed, a.summary.criteria) == (2, 2))
    #expect(a.checks.count == 2 && a.checks[1].methodLabel == "未核实")
    #expect(a.summary.countsText == "测试 1 条、未核实 1 条")
    #expect(a.verifications.first?.passed == false && a.verifications.first?.label == "失败", "a failed verification is shown apart from the executor's checks")
    #expect(a.traceGaps.count == 1 && a.risks == ["旧版本没有回归测试"])
    #expect(a.summary.reminders.count == 1)
}
