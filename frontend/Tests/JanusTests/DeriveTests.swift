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
    #expect(f.open.contains { $0.title == expectedCopy("T1 可能卡住了", "T1 may be stalled") && !$0.you })
    log.add("tool.call", actor: "researcher", task: "T1", attempt: 1, data: ["tool": "read", "status": "completed"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("T1 可能卡住了", "T1 may be stalled") })
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "idle", "idleMs": 600000])
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "idle", "resolved": true])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("T1 可能卡住了", "T1 may be stalled") }, "the backend saw the agent move again")
    log.add("task.stalled", to: "lead", task: "T1", attempt: 1, data: ["reason": "loop", "repeats": 5])
    log.add("attempt.started", to: "researcher", task: "T1", attempt: 2, data: ["session": "s2"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("T1 可能卡住了", "T1 may be stalled") }, "a new attempt starts clean")
    log.add("task.stalled", to: "lead", task: "T1", attempt: 2, data: ["reason": "loop", "repeats": 5])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("T1 可能卡住了", "T1 may be stalled") })
}

@Test func leadStallsAndConflictsOpenOnTheBoard() {
    var log = goalRun()
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "idle", "idleMs": 600000])
    log.add("task.conflict", actor: "janus", to: "lead", task: "T1", data: ["other": "T2", "paths": ["calc.py"]])
    var f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.contains { $0.title == expectedCopy("负责人可能卡住了", "The lead may be stalled") && $0.you }, "nobody but the person watches the lead")
    #expect(f.open.contains { $0.title == expectedCopy("T1 和 T2 改了同一个文件", "T1 and T2 edited the same file") && $0.text == "calc.py" && !$0.you })
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "idle", "resolved": true])
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "x"])
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("负责人可能卡住了", "The lead may be stalled") })
    #expect(!f.open.contains { $0.title.contains(expectedCopy("改了同一个文件", "edited the same file")) }, "neither task is running any more")
}

/// A repeated-call report has no explicit resolution: the lead doing anything again closes it.
@Test func aLeadStallClearsWhenTheLeadMovesAgain() {
    var log = goalRun()
    log.add("lead.stalled", actor: "janus", to: "user", data: ["reason": "loop", "tool": "Read", "repeats": 5])
    var f = boardFacts(derive(log.events)) { $0 }
    #expect(f.open.contains { $0.title == expectedCopy("负责人可能卡住了", "The lead may be stalled") && $0.you })
    log.add("agent.message", actor: "lead", text: "换个办法")
    f = boardFacts(derive(log.events)) { $0 }
    #expect(!f.open.contains { $0.title == expectedCopy("负责人可能卡住了", "The lead may be stalled") })
}

@Test func tracesOpenOnTheBoardUntilTheLeadActs() {
    var log = goalRun()
    log.add("task.claimed", actor: "researcher", to: "lead", task: "T1", data: ["paths": ["notes.md"]])
    log.add("task.conflict", actor: "janus", to: "lead", task: "T1", data: ["other": "T2", "paths": ["calc.py"], "owned": true])
    var st = derive(log.events)
    #expect(st.tasks["T1"]?.owns == ["notes.md"], "a claim adds to what the task owns")
    var f = boardFacts(st) { $0 }
    #expect(f.open.contains { $0.title == expectedCopy("T1 改了 T2 负责的文件", "T1 edited files owned by T2") && $0.text == "calc.py" && !$0.you })
    #expect(f.handoffs.contains { $0.summary == expectedCopy("认领：notes.md", "Claimed: notes.md") })
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
    #expect(!f.open.contains { $0.title.contains(expectedCopy("负责的文件", "edited files owned by")) }, "the task that stepped over stopped")
    #expect(st.tasks["T1"]?.traceGaps.count == 1)
    #expect(f.open.contains { $0.title == expectedCopy("T1 的结果和工作目录对不上", "The result for T1 differs from the workspace") && $0.text.contains("calc.py") })
    log.add("result.amended", actor: "lead", task: "T1", data: ["text": "calc.py 的改动由 T2 负责"])
    #expect(!boardFacts(derive(log.events)) { $0 }.open.contains { $0.title.contains(expectedCopy("对不上", "differs from the workspace")) }, "the lead corrected it")
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
    #expect(f.decisions.map(\.title).contains(expectedCopy("负责人提交编排", "Lead submitted a plan")))
    #expect(f.decisions.map(\.title).contains(expectedCopy("给 lead 的补充", "Follow-up for lead")))
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

@Test func markdownProse() {
    func item(_ s: String) -> MDDoc.Item { .init(checked: nil, blocks: [.para(s)]) }
    #expect(MD.blocks("结论：\n\n- 甲\n- 乙\n# 标题") == [.para("结论："), .list(ordered: false, start: 0, items: [item("甲"), item("乙")]), .heading(level: 1, text: "标题")])
    // A list sent on one line is split; a message with line breaks is left as written.
    #expect(MD.blocks("改了： - 甲 - 乙") == [.para("改了："), .list(ordered: false, start: 0, items: [item("甲 - 乙")])])
    #expect(MD.prose("改了： - 甲\n下一行") == "改了： - 甲\n下一行")
    // Parsed once, then served from the cache.
    #expect(MD.blocks("改了： - 甲 - 乙") == MDDoc.parse("改了：\n- 甲 - 乙"))
    // An agent's reply: a table right under a heading and a fenced command render as blocks,
    // not as one paragraph of pipes and backticks.
    let reply = """
    已按你的要求处理。
    ## 1. 分支与提交
    | 仓库 | 分支 |
    |---|---|
    | biz-knowledge | `feature/route-p0` |
    这些分支都从各自 master 创建。
    ```bash
    cd biz-knowledge
    git push origin feature/route-p0
    ```
    """
    #expect(MD.blocks(reply) == [
        .para("已按你的要求处理。"),
        .heading(level: 2, text: "1. 分支与提交"),
        .table(header: ["仓库", "分支"], align: [.leading, .leading], rows: [["biz-knowledge", "`feature/route-p0`"]]),
        .para("这些分支都从各自 master 创建。"),
        .code(lang: "bash", text: "cd biz-knowledge\ngit push origin feature/route-p0"),
    ])
}

@Test func planControlsAreHidden() {
    let c: JSON = ["options": ["controls": [
        ["id": "plan-mode", "type": "select", "options": []],
        ["id": "mode", "type": "select", "permission": true, "current": "ask", "default": "ask", "options": [["value": "plan", "name": "Plan"], ["value": "ask", "name": "Ask"]]],
    ]]]
    let ctrls = Setup.controls(c)
    #expect(ctrls.map { $0["id"].str } == ["mode"])
    #expect(ctrls[0]["options"].array.map { $0["value"].str } == ["ask"])
    #expect(Setup.permLabel([:], c) == "Ask")
}

/// A role that leaves the mode open shows the default the server marks (Codex: Full access), not the agent's current mode.
@Test func anOpenModeShowsTheDefault() {
    let c: JSON = ["base": "codex", "options": ["controls": [
        ["id": "mode", "type": "select", "permission": true, "current": "agent", "default": "agent-full-access",
         "options": [["value": "agent", "name": "Auto review"], ["value": "agent-full-access", "name": "Full access"]]],
    ]]]
    #expect(Setup.permLabel([:], c) == "Full access")
    #expect(Setup.permLabel(["mode": "agent"], c) == "Auto review")
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
/// `TaskStatus` (a new one needs a case, its label and a line in docs/domains/20261031-run_lifecycle.md);
/// the run states are each a `RunStatus`, and nothing else is.
@Test func everyStatusTheServerSendsIsACase() {
    let scheduler = ["pending", "running", "blocked", "succeeded", "failed", "cancelled", "skipped_by_decision"]
    let derived = ["ready", "assigned", "verifying", "verification_failed", "waiting_input", "waiting_approval"]
    for s in scheduler + derived { #expect(TaskStatus(rawValue: s) != nil, "\(s)") }
    let runs = ["queued", "planning", "running", "delivered", "accepted", "cancelled", "failed", "interrupted"]
    for s in runs { #expect(RunStatus(s) != .unknown, "\(s)") }
    #expect(RunStatus("idle") == .unknown)
    #expect(Set(runs.map(RunStatus.init).filter(\.isActive)) == [.planning, .running])
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
    for (raw, label) in [("test", expectedCopy("测试", "Test")), ("command", expectedCopy("命令", "Command")),
                         ("inspection", expectedCopy("检查", "Inspection")), ("reasoning", expectedCopy("推理判断", "Reasoning")),
                         ("unverified", expectedCopy("未核实", "Unverified"))] {
        let c = Check(["criterion": "标准1", "passed": true, "method": JSON(raw), "evidence": "证据"])
        #expect(c.method?.rawValue == raw)
        #expect(c.methodLabel == label)
    }
    // A check that names no method: shown as 未说明, never as reasoning.
    let none = Check(["criterion": "标准1", "passed": true, "evidence": "看起来没问题"])
    #expect(none.method == nil && none.badMethod == nil)
    #expect(none.methodLabel == expectedCopy("未说明", "Unspecified"))
    // An illegal value is kept as recorded and never mapped onto an allowed method.
    let bad = Check(["criterion": "标准1", "passed": true, "method": "vibes", "evidence": "看着没问题"])
    #expect(bad.method == nil)
    #expect(bad.badMethod == "vibes")
    #expect(bad.methodLabel == expectedCopy("方法「vibes」不合法（未说明）", "Invalid method “vibes” (unspecified)"))
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
    #expect(s.reminders.contains { $0.contains(expectedCopy("推理判断", "assessed by reasoning")) && $0.contains(expectedCopy("不是运行得到的结果", "without an executed check")) })
    #expect(s.reminders.contains { $0.contains(expectedCopy("未核实", "was not verified")) && $0.contains("本机没有 Xcode") })
    #expect(s.reminders.contains { $0.contains(expectedCopy("没有说明证据方法", "has no evidence method")) })
    #expect(s.methods.map(\.method) == ["test", "reasoning", "unverified"], "counts keep the five methods' order: \(s.methods)")
    #expect(s.countsText == expectedCopy("测试 1 条、推理判断 1 条、未核实 1 条、未说明 1 条、方法不合法 1 条", "Test: 1, Reasoning: 1, Unverified: 1, Unspecified: 1, Invalid methods: 1"), "\(s.countsText)")
    #expect(EvidenceSummary(accept: [], checks: []).countsText == expectedCopy("没有记录", "No records"))
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
    #expect(st.verifiers(of: "T2").first?.checks.first?.methodLabel == expectedCopy("命令", "Command"))
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
    #expect(a.checks.count == 2 && a.checks[1].methodLabel == expectedCopy("未核实", "Unverified"))
    #expect(a.summary.countsText == expectedCopy("测试 1 条、未核实 1 条", "Test: 1, Unverified: 1"))
    #expect(a.verifications.first?.passed == false && a.verifications.first?.label == expectedCopy("失败", "Failed"), "a failed verification is shown apart from the executor's checks")
    #expect(a.traceGaps.count == 1 && a.risks == ["旧版本没有回归测试"])
    #expect(a.summary.reminders.count == 1)
}

@Test func verifierVerdictSplitProposalsAndLeadConflictsDeriveCleanly() throws {
    var log = Log()
    log.add("run.created", actor: "user", data: ["mode": "goal", "goal": "核验与协作", "cwd": "/tmp/x", "git": true])
    log.add("orchestration.accepted", actor: "lead", data: ["tasks": [
        ["id": "T1", "title": "实现", "agent": "dev", "deps": [], "owns": ["src/api.rs"], "accept": ["接口可用"]],
        ["id": "V1", "title": "核验", "agent": "qa", "deps": ["T1"], "verifies": "T1", "accept": ["边界通过"]],
        ["id": "T2", "title": "修复", "agent": "dev", "deps": ["V1"], "continues": "T1", "accept": ["修复通过"]],
    ]])
    log.add("attempt.started", to: "dev", task: "T1", attempt: 1, data: ["session": "s1"])
    log.add("task.split_requested", actor: "dev", to: "lead", task: "T1", data: [
        "reason": "模块过大", "subtasks": [["title": "解析器"], ["title": "生成器"]],
    ])
    log.add("task.conflict", to: "lead", task: "T1", data: [
        "other": "lead", "byLead": true, "owned": true, "paths": ["src/api.rs"],
    ])
    let s1 = derive(log.events)
    let f1 = boardFacts(s1) { $0 }
    #expect(s1.tasks["T2"]?.continues == "T1")
    #expect(f1.open.contains { $0.kind == "split_request" && $0.title == expectedCopy("T1 请求拆分子任务", "T1 requested a subtask split") })
    #expect(f1.open.contains { $0.kind == "overstep" && $0.title == expectedCopy("负责人改了 T1 负责的文件", "The lead edited files owned by T1") })

    // Finishing T1 and having V1 submit `verdict: "rejected"` clears the split request and marks V1 unconfirmed.
    log.add("task.succeeded", task: "T1", attempt: 1, data: [
        "summary": "初版", "handoff": "parse() 返回 Result", "checks": [["criterion": "接口可用", "passed": true, "method": "test", "evidence": "ok"]],
    ])
    log.add("attempt.started", to: "qa", task: "V1", attempt: 1, data: ["session": "s2"])
    log.add("task.succeeded", task: "V1", attempt: 1, data: [
        "summary": "空输入崩溃", "verdict": "rejected",
        "checks": [["criterion": "边界通过", "passed": false, "method": "test", "evidence": "cargo test panic"]],
    ])
    let s2 = derive(log.events)
    #expect(s2.tasks["T1"]?.handoff == "parse() 返回 Result")
    #expect(s2.tasks["V1"]?.status == .succeeded)
    #expect(s2.tasks["V1"]?.verdict == .rejected)
    let vr = VerificationRecord(try #require(s2.tasks["V1"]))
    #expect(vr.verdict == .rejected && vr.passed == false, "completed negative review is not an approval")
    #expect(s2.run.status == .running)
    let f2 = boardFacts(s2) { $0 }
    #expect(!f2.open.contains { $0.kind == "split_request" })
}


/// An installed app does not use a Janus service of another release (its data upgrade has not run);
/// an app run from source uses whatever runs.
@Test func aServiceOfAnotherReleaseIsStale() {
    #expect(!Backend.isStale(server: "2026.11.19", app: "2026.11.19"))
    #expect(Backend.isStale(server: "2026.11.18.3", app: "2026.11.19"))
    #expect(Backend.isStale(server: "0.1.0", app: "2026.11.19"), "a service from source")
    #expect(Backend.isStale(server: nil, app: "2026.11.19"), "a service too old to say its version")
    #expect(!Backend.isStale(server: "0.1.0", app: "0"), "the app from source")
}

/// The lead sending a task back on a verification's findings is a decision on the board: which
/// session, whose findings and the lead's note. The verification it brings back loses its old verdict.
@Test func aReworkIsTheLeadsDecisionAndClearsTheOldVerdict() {
    var log = goalRun()
    log.add("task.succeeded", task: "T1", attempt: 1, data: ["summary": "初版"])
    log.add("task.succeeded", task: "T2", attempt: 1, data: ["summary": "空输入崩溃", "verdict": "rejected"])
    #expect(derive(log.events).tasks["T2"]?.verdict == .rejected)
    log.add("task.reopened", actor: "lead", task: "T1", data: ["reason": "lead.retry", "session": "continue", "from": "T2", "note": "空输入按错误处理"])
    log.add("task.reopened", actor: "janus", task: "T2", data: ["reason": "target.reworked", "target": "T1"])
    let st = derive(log.events)
    #expect(st.tasks["T1"]?.status == .ready && st.tasks["T2"]?.verdict == nil)
    let d = boardFacts(st) { $0 }.decisions.first { $0.title == expectedCopy("负责人让 T1 返工", "Lead sent T1 back for rework") }
    #expect(d?.text.contains(expectedCopy("会话：接着原会话", "Session: continue its own")) == true)
    #expect(d?.text.contains(expectedCopy("依据 T2 的意见", "On T2's findings")) == true)
    #expect(d?.text.contains("空输入按错误处理") == true)
    #expect(d?.tasks == ["T1", "T2"])
}

/// A run in its own worktree says where its changes are until accepted, then where they were
/// merged back; a run working in place says nothing about it.
@Test func aWorktreeRunSaysWhereItsResultIs() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g", "cwd": "/h/work/runs/r1", "git": true,
                                                  "worktree": ["repo": "/code/app", "included": [".env"], "branch": "main"]])
    var st = derive(log.events)
    #expect(st.run.repo == "/code/app" && st.run.branch == "main" && !st.run.merged)
    #expect(SessionView.worktreeNote(st.run, accepted: false) == expectedCopy("改动在这个任务单独的副本里，验收时合并回 /code/app 的分支 main。", "The changes are in this task's own copy; accepting merges them back into branch main of /code/app."))
    log.add("risk.accepted", actor: "user", data: ["branch": "main", "commit": "abc", "repo": "/code/app"])
    st = derive(log.events)
    #expect(st.run.merged)
    #expect(SessionView.worktreeNote(st.run, accepted: true) == expectedCopy("结果已合并到 /code/app 的分支 main。", "The result is merged into branch main of /code/app."))
    // Started on a detached HEAD; an acceptance with nothing new merges nothing.
    var detached = Log()
    detached.add("run.created", actor: "user", data: ["goal": "g", "cwd": "/h/work/runs/r2", "git": true,
                                                       "worktree": ["repo": "/code/app", "branch": .null]])
    detached.add("risk.accepted", actor: "user", data: ["branch": .null, "commit": .null, "repo": "/code/app"])
    st = derive(detached.events)
    #expect(st.run.branch.isEmpty && !st.run.merged)
    #expect(SessionView.worktreeNote(st.run, accepted: true) == expectedCopy("这个任务没有新的改动，没有提交。", "This task brought no new changes, so nothing was committed."))
    st.run.merged = true
    #expect(SessionView.worktreeNote(st.run, accepted: true) == expectedCopy("结果已合并到 /code/app 当前的 detached HEAD。", "The result is merged into the detached HEAD of /code/app."))
    #expect(SessionView.runGitNote(log.events[0].data) == expectedCopy("Git 仓库 /code/app · 分支 main", "Git repository /code/app · branch main"))
    log.add("attempt.started", task: "T1", attempt: 1, data: ["git": ["repo": "/code/app", "branch": .null, "worktree": "/h/work/runs/r1", "head": "0123456789abcdef"]])
    #expect(SessionView.taskGitNote(derive(log.events), "T1") == expectedCopy("Git 仓库 /code/app · detached HEAD · 提交 01234567", "Git repository /code/app · detached HEAD · commit 01234567"))
    var inPlace = Log()
    inPlace.add("run.created", actor: "user", data: ["goal": "g", "cwd": "/code/notes", "git": false])
    #expect(SessionView.worktreeNote(derive(inPlace.events).run, accepted: true) == nil)
}

/// One turn rule for every session: who holds it and whether its turn ended move together, and a
/// closed session holds nothing.
@Test func aSessionsTurnFollowsItsEvents() {
    func ev(_ type: String, _ data: JSON = [:]) -> Event { Event(["seq": 1, "type": JSON(type), "actor": "m1", "data": data]) }
    var turn = SessionTurn.opened
    turn = turn.after(ev("agent.status", ["turn": "idle"]))
    #expect(turn == .agent(idle: true))
    turn = turn.after(ev("control.taken"))
    #expect(turn.human && turn.idle, "taking over keeps the turn's state")
    turn = turn.after(ev("user.message", ["delivery": "prompt"]))
    #expect(turn == .person(idle: false))
    #expect(turn.after(ev("session.interrupted", ["finish": true])) == turn, "finishing is not taking over")
    #expect(turn.after(ev("control.released")) == .opened)
}

/// A queued run is only its goal until it starts: `run.queued` names the goal and the queued
/// state, `run.created` starts it; the run list names its place in the queue.
@Test func aQueuedRunShowsItsGoalUntilItStarts() {
    var log = Log()
    log.add("run.queued", actor: "user", data: ["goal": "修复导出乱码", "workspace": ["id": "w1", "name": "W"], "images": []])
    let queued = derive(log.events)
    #expect(queued.run.status == .queued && queued.run.goal == "修复导出乱码")
    #expect(!queued.run.status.isActive && !queued.run.status.isFinished)
    // Taken off the queue: being set up, no longer offered to start or cancel.
    log.add("run.starting", text: "有了空位，开始准备")
    #expect(derive(log.events).run.status == .planning)
    log.add("run.created", actor: "user", data: ["goal": "修复导出乱码", "cwd": "/tmp/x", "git": true])
    #expect(derive(log.events).run.status == .running)
    // One whose setup never finished goes back on the queue.
    var again = Log()
    again.add("run.queued", actor: "user", data: ["goal": "g", "images": []])
    again.add("run.starting")
    again.add("run.requeued")
    #expect(derive(again.events).run.status == .queued)
    #expect(runLabel(["status": "queued", "queuePosition": 2]).0 == expectedCopy("排队中 · 第 2 位", "Queued · #2"))
    #expect(runLabel(["status": "queued"]).0 == expectedCopy("排队中", "Queued"))
}

/// A Loop's last wake reaches the person when it changed workspace files or could not wake; the
/// next wake replaces it. Its schedule and phase read as the app shows them.
@Test func aLoopsWakeNoticesReachTheInboxUntilTheNextWake() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["kind": "loop", "cwd": "/tmp/ws", "git": false])
    log.add("loop.woke", data: ["reason": "timer", "n": 1])
    log.add("loop.touched", text: "Loop 醒来期间工作区多了 1 处改动", data: ["files": ["notes.md"]])
    let f = boardFacts(derive(log.events)) { $0 }
    let touched = f.open.first { $0.kind == "loop_touched" }
    #expect(touched?.you == true && touched?.sub == "notes.md")
    log.add("loop.woke", data: ["reason": "timer", "n": 2])
    #expect(!boardFacts(derive(log.events)) { $0 }.open.contains { $0.kind == "loop_touched" }, "the next wake replaces it")
    log.add("loop.failed", text: "Loop 没能醒来：工作区不存在")
    #expect(boardFacts(derive(log.events)) { $0 }.open.contains { $0.kind == "loop_failed" })

    let loop: JSON = ["rhythm": ["kind": "interval", "minutes": 120], "status": ["phase": "paused"]]
    #expect(LoopText.rhythm(loop) == expectedCopy("每 2 小时", "Every 2 hours"))
    #expect(LoopText.rhythm(["rhythm": ["kind": "interval", "minutes": 30]]) == expectedCopy("每 30 分钟", "Every 30 minutes"))
    #expect(LoopText.rhythm(["rhythm": ["kind": "self_paced"]]) == expectedCopy("自己决定", "Its own call"))
    #expect(LoopText.every(1440) == expectedCopy("每天", "Every day"))
    #expect(LoopText.every(60) == expectedCopy("每小时", "Every hour"))
    #expect(LoopText.phase(loop).0 == expectedCopy("已暂停", "Paused"))
    #expect(LoopText.phase(["status": ["phase": "awake"]]).0 == expectedCopy("醒着", "Awake"))
}
