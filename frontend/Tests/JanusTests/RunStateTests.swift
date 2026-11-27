import Foundation
import Testing
@testable import Janus

// The run's state is the server's (`GET /api/runs/{id}/view`, `backend/src/run_view.rs`); what it
// works out is tested there. These tests read the served shape and the app's own display code.

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

/// A task as the view serves it.
private func servedTask(_ id: String, agent: String = "m1", status: String = "running", _ more: [String: JSON] = [:]) -> JSON {
    var t: JSON = ["id": JSON(id), "title": JSON(id), "agent": JSON(agent), "status": JSON(status), "deps": [], "accept": [], "attempts": [], "checks": []]
    for (k, v) in more { t[k] = v }
    return t
}

/// The served view reads back as the run page uses it: tasks in board order, each session's turn,
/// the requests by what they belong to, the deliveries, and each conversation's events and composer.
@Test func theServedViewReadsBack() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.add("agent.message", actor: "m1", task: "T2", data: ["text": "在做"])
    let view: JSON = [
        "run": ["status": "delivered", "goal": "g", "kind": "goal", "start": 1_000_000, "merged": true],
        "tasks": [servedTask("T2", ["session": ["person": true, "idle": true]]), servedTask("T1", status: "succeeded")],
        "agents": ["m1": ["phase": "waiting_for_input", "note": "等你", "beat": 5]],
        "inbox": [["kind": "approval", "key": "a-1", "seq": 9, "task": "T2", "agent": "m1", "d": ["command": "rm"]],
                  ["kind": "session", "key": "", "seq": 10, "agent": "lead", "d": [:]]],
        "msgs": [["id": "m9", "text": "顺便", "by": "user", "to": "lead", "seq": 7, "images": [], "delivery": "steer", "deliveredSeq": 8]],
        "deliveries": [["d": ["summary": "完成"], "seq": 11, "n": 1, "since": 0, "accepted": .null]],
        "leadTurn": ["person": false, "idle": true],
        "board": ["open": [["key": "o9approval", "kind": "approval", "seq": 9, "you": true, "title": "「m1」等你拍板", "text": "rm", "sub": "T2 · T2", "label": "等你审批",
                            "tasks": ["T2"], "agents": ["m1"], "actions": [["label": "允许一次", "help": "", "prominent": true, "body": ["type": "approve", "key": "a-1"]]]]]],
        "review": ["stage": "blocked", "conflict": true, "signals": [["source": "lead", "kind": "decision", "text": "要不要发版？", "task": .null]], "requests": [["seq": 5, "text": "再改"]]],
        "sessions": ["task:T2": ["events": [2], "open": true, "human": true, "idle": true, "converse": false, "chat": false, "composer": "human"]],
        "seq": 11,
    ]
    let st = RunState(view, events: log.events)
    #expect(st.order == ["T2", "T1"] && st.tasks["T2"]?.human == true && st.tasks["T2"]?.idle == true && st.tasks["T1"]?.live == false)
    #expect(st.run.status == .delivered && st.run.merged && st.result?.d["summary"] == "完成")
    #expect(st.inbox.map(\.ref) == [.decision("a-1"), .session(.agent("lead"))])
    #expect(st.agents["m1"]?.phase == .waitingForInput && st.msgs["m9"]?.delivery == .steer)
    #expect(st.board.open.first?.actions.first?.body["key"] == "a-1")
    #expect(st.review?.stage == .blocked(conflict: true) && st.review?.requests.map(\.text) == ["再改"])
    let s = st.session(.task("T2"))
    #expect(s.evs.map(\.seq) == [2] && s.composer == .human(idle: true))
    #expect(st.session(.agent("nobody")).composer == .ended, "a conversation the server did not serve takes nothing")
}

/// The independent verifier is kept apart from the task it checks, with its own methods; a
/// completed negative review is not an approval.
@Test func aVerifierIsKeptApartAndARejectionIsNoApproval() {
    let check: JSON = ["criterion": "复现行为", "passed": true, "method": "command", "evidence": "python3 calc.py"]
    let st = RunState(["tasks": [servedTask("T2", status: "succeeded", ["checks": [["criterion": "测试通过", "passed": true, "method": "test", "evidence": "ok"]], "risks": ["没有覆盖负数"]]),
                                 servedTask("V1", agent: "qa", status: "succeeded", ["verifies": "T2", "checks": [check], "accept": ["复现行为"]])]], events: [])
    #expect(st.tasks["T2"]?.checks.first?.method == .test && st.tasks["T2"]?.risks == ["没有覆盖负数"])
    #expect(st.verifiers(of: "T2").map(\.id) == ["V1"])
    #expect(st.verifiers(of: "T2").first?.checks.first?.methodLabel == expectedCopy("命令", "Command"))
    #expect(st.verifiers(of: "T1").isEmpty)
    let rejected = TaskState(servedTask("V1", agent: "qa", status: "succeeded", ["verdict": "rejected", "accept": ["边界通过"],
                                                                                    "checks": [["criterion": "边界通过", "passed": false, "method": "test", "evidence": "panic"]]]))
    let vr = VerificationRecord(rejected)
    #expect(vr.verdict == .rejected && vr.passed == false, "completed negative review is not an approval")
}

/// The delivery's per-task record keeps every check with its method, the independent verification,
/// the trace gaps and the risks.
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
    let r = RunResult(["d": ["summary": "完成", "acceptance": [record]], "seq": 5, "n": 1, "since": 0])
    let records = Delivery(r).acceptance
    #expect(records.count == 1)
    let a = try #require(records.first)
    #expect((a.summary.passed, a.summary.criteria) == (2, 2))
    #expect(a.checks.count == 2 && a.checks[1].methodLabel == expectedCopy("未核实", "Unverified"))
    #expect(a.summary.countsText == expectedCopy("测试 1 条、未核实 1 条", "Test: 1, Unverified: 1"))
    #expect(a.verifications.first?.passed == false && a.verifications.first?.label == expectedCopy("失败", "Failed"), "a failed verification is shown apart from the executor's checks")
    #expect(a.traceGaps.count == 1 && a.risks == ["旧版本没有回归测试"])
    #expect(a.summary.reminders.count == 1)
}

/// A queued run's row names its place in the queue; a queued run is neither at work nor finished.
@Test func aQueuedRunIsListedByItsPlace() {
    #expect(!RunStatus.queued.isActive && !RunStatus.queued.isFinished)
    #expect(runLabel(["status": "queued", "queuePosition": 2]).0 == expectedCopy("排队中 · 第 2 位", "Queued · #2"))
    #expect(runLabel(["status": "queued"]).0 == expectedCopy("排队中", "Queued"))
}

/// A Loop's schedule and phase read as the app shows them.
@Test func aLoopsScheduleAndPhaseReadAsShown() {
    let loop: JSON = ["schedule": ["cron": "0 */2 * * *", "tz": "UTC"], "status": ["phase": "paused"]]
    #expect(LoopText.schedule(loop) == expectedCopy("每 2 小时", "Every 2 hours"))
    #expect(LoopText.schedule(["schedule": .null]) == expectedCopy("只在你叫它时", "Only when you call it"))
    #expect(LoopText.phase(loop).0 == expectedCopy("已暂停", "Paused"))
    // A time it asked for says so, with why.
    let asked: JSON = ["status": ["phase": "sleeping", "nextWakeAt": JSON(floatLiteral: L.nowMs() + 600_000), "nextWakeReason": "self", "nextWakeWhy": "等 CI"]]
    #expect(LoopText.asked(asked)?.hasSuffix(expectedCopy("再看一次：等 CI", ": 等 CI")) == true)
    #expect(LoopText.asked(loop) == nil)
    // The narrow column's pill keeps to the state; when it wakes goes on the line below.
    #expect(LoopText.phase(asked).0.hasPrefix(expectedCopy("摸鱼中 · ", "Idle · ")))
    #expect(LoopText.phase(asked, short: true).0 == expectedCopy("摸鱼中", "Idle"))
    #expect(LoopText.phase(["status": ["phase": "awake"]]).0 == expectedCopy("干活中", "Working"))
}

/// A Loop's session marks each wake: which one, when and why; then when it ended, how long it took
/// and when the next one is. It has no goal: the session opens with its first wake.
@Test func aLoopsSessionMarksWhenEachWakeStartsAndEnds() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["kind": "loop", "cwd": "/tmp/ws", "git": false])
    log.add("loop.woke", data: ["reason": "timer", "n": 3])
    log.add("agent.message", actor: "lead", data: ["text": "看过了"])
    log.add("loop.slept", data: ["until": JSON(floatLiteral: 9_000_000), "reason": "self", "summary": "看过了"])
    log.add("loop.woke", data: ["reason": "manual", "n": 4])
    log.add("loop.slept", data: ["until": .null, "reason": .null, "summary": ""])
    var st = RunState()
    st.events = log.events
    let (woke, slept) = (log.events[1], log.events[3])
    #expect(LoopText.woke(woke).hasPrefix(expectedCopy("第 3 次开工 · ", "Wake 3 · ")))
    #expect(LoopText.woke(woke).hasSuffix(expectedCopy(" · 按时间表", " · on schedule")))
    #expect(LoopText.wake(before: slept, in: st.events)?.seq == woke.seq)
    #expect(LoopText.slept(slept, woke: woke).contains(expectedCopy("收工（2 秒）· 下次开工：", " (2 sec) · next wake: ")))
    #expect(LoopText.slept(slept, woke: woke).hasSuffix(expectedCopy("（它约的）", " (it asked)")))
    // No time ahead: it waits to be called.
    let last = log.events[5]
    #expect(LoopText.wake(before: last, in: st.events)?.seq == log.events[4].seq)
    #expect(LoopText.slept(last, woke: log.events[4]).hasSuffix(expectedCopy("收工（1 秒）· 下次等你叫它", " (1 sec) · next when you call it")))
}

@testable import Janus

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

/// A role that leaves the mode open shows the default the server marks (Codex: Full access), not the agent's current mode.
@Test func anOpenModeShowsTheDefault() {
    let c: JSON = ["base": "codex", "options": ["controls": [
        ["id": "mode", "type": "select", "permission": true, "current": "agent", "default": "agent-full-access",
         "options": [["value": "agent", "name": "Auto review"], ["value": "agent-full-access", "name": "Full access"]]],
    ]]]
    #expect(Setup.permLabel([:], c) == "Full access")
    #expect(Setup.permLabel(["mode": "agent"], c) == "Auto review")
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

/// Saving the shared material on its own keeps the workspace's team, lead, name and folder.
@MainActor @Test func savingTheSharedMaterialKeepsTheTeam() {
    let ws: JSON = ["id": "w", "name": "商城", "path": "/tmp/shop", "lead": "pmo", "brief": "旧的",
                    "members": [["role": "pmo"], ["role": "generalist", "slots": 3]]]
    let body = BriefEditor.body(ws, brief: "新的")
    #expect(body["brief"] == "新的")
    for k in ["id", "name", "path", "lead", "members"] { #expect(body[k] == ws[k], "\(k)") }
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
    #expect(bad.methodLabel == expectedCopy("证据方法「vibes」不认识（没说明）", "Unknown evidence method “vibes” (not given)"))
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

/// An installed app does not use a Janus service of another release (its data upgrade has not run);
/// an app run from source uses whatever runs.
@Test func aServiceOfAnotherReleaseIsStale() {
    #expect(!Backend.isStale(server: "2026.11.19", app: "2026.11.19"))
    #expect(Backend.isStale(server: "2026.11.18.3", app: "2026.11.19"))
    #expect(Backend.isStale(server: "0.1.0", app: "2026.11.19"), "a service from source")
    #expect(Backend.isStale(server: nil, app: "2026.11.19"), "a service too old to say its version")
    #expect(!Backend.isStale(server: "0.1.0", app: "0"), "the app from source")
}

/// A workspace's Loops, the ones to look at first on top: failed, at work, sleeping by when they
/// wake (one without a time after those with one), paused last; ties keep their order.
@Test func loopsAreListedByWhatNeedsALook() {
    func l(_ id: String, _ phase: String, wake: Double? = nil) -> JSON {
        ["id": JSON(id), "status": ["phase": JSON(phase), "nextWakeAt": wake.map { JSON(floatLiteral: $0) } ?? .null]]
    }
    let loops = [l("paused", "paused"), l("late", "sleeping", wake: 2_000), l("never", "sleeping"), l("soon", "sleeping", wake: 1_000),
                 l("working", "awake"), l("broken", "failed"), l("paused2", "paused")]
    #expect(LoopText.byAttention(loops).map { $0["id"].str } == ["broken", "working", "soon", "late", "never", "paused", "paused2"])
}

/// The form's common schedules are cron expressions and read back as the same shape; anything else
/// is an expression of its own.
@Test func aLoopsScheduleReadsBackAsTheShapeTheFormMadeIt() {
    let shapes: [LoopSchedule] = [.minutes(15), .hours(1), .hours(6), .daily(hour: 9, minute: 0), .weekdays(hour: 18, minute: 30),
                                  .weekly(day: 1, hour: 9, minute: 5), .weekly(day: 0, hour: 20, minute: 0), .monthly(day: 1, hour: 8, minute: 0)]
    for s in shapes { #expect(LoopSchedule(cron: s.cron) == s, "\(s.cron)") }
    #expect(LoopSchedule.daily(hour: 9, minute: 0).cron == "0 9 * * *")
    #expect(LoopSchedule.weekdays(hour: 9, minute: 0).cron == "0 9 * * 1-5")
    #expect(LoopSchedule(cron: "0  9-18 * *  1-5") == .cron("0 9-18 * * 1-5"))
    #expect(LoopSchedule(cron: "*/7 * * * *") == .cron("*/7 * * * *"))
    #expect(LoopSchedule(cron: "0 9 * 1 *") == .cron("0 9 * 1 *"))
    #expect(LoopSchedule.weekdays(hour: 9, minute: 0).words == expectedCopy("工作日 09:00", "Weekdays at 09:00"))
    #expect(LoopSchedule.weekly(day: 1, hour: 9, minute: 0).words == expectedCopy("每周一 09:00", "Every Monday at 09:00"))
    #expect(LoopSchedule.minutes(30).words == expectedCopy("每 30 分钟", "Every 30 minutes"))
    // The time menu offers every five minutes and keeps a minute a Loop already has.
    #expect(LoopSchedule.minuteMarks(keeping: 0) == Array(stride(from: 0, to: 60, by: 5)))
    #expect(LoopSchedule.minuteMarks(keeping: 7) == [0, 5, 7, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55])
    #expect(LoopSchedule.cron("0 9-18 * * 1-5").words == expectedCopy("cron：0 9-18 * * 1-5", "cron: 0 9-18 * * 1-5"))
}
