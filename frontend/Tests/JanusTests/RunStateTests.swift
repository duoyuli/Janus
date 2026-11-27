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

/// A task's evidence reads as the server pairs and counts it, its independent verifier apart with
/// its own conclusion; a completed negative review is not an approval.
@Test func aTasksEvidenceAndItsVerifierReadAsServed() throws {
    let st = RunState(["tasks": [servedTask("T2", status: "succeeded", ["checks": [["criterion": "测试通过", "passed": true, "method": "test", "evidence": "ok"]], "risks": ["没有覆盖负数"]])],
                       "evidence": ["T2": ["criteria": 2, "passed": 1, "counts": "测试 1 条", "reminders": [],
                                           "rows": [["n": 1, "criterion": "测试通过", "check": ["criterion": "测试通过", "passed": true, "method": "test"]],
                                                    ["n": 2, "criterion": "有保护", "check": .null]],
                                           "verifications": [["task": "V1", "status": "succeeded", "verdict": "rejected", "passed": false,
                                                              "checks": [["criterion": "边界通过", "passed": false, "method": "command", "evidence": "panic"]]]]]]],
                      events: [])
    #expect(st.tasks["T2"]?.checks.first?.method == .test && st.tasks["T2"]?.risks == ["没有覆盖负数"])
    let e = try #require(st.evidence["T2"])
    #expect(e.rows.map(\.n) == [1, 2] && e.rows[1].check == nil, "a criterion nothing answered keeps its row")
    let v = try #require(e.verifications.first)
    #expect(v.task == "V1" && v.verdict == .rejected && !v.passed, "completed negative review is not an approval")
    #expect(v.checks.first?.methodLabel == expectedCopy("命令", "Command"))
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


/// An installed app does not use a Janus service of another release (its data upgrade has not run);
/// an app run from source uses whatever runs.
@Test func aServiceOfAnotherReleaseIsStale() {
    #expect(!Backend.isStale(server: "2026.11.19", app: "2026.11.19"))
    #expect(Backend.isStale(server: "2026.11.18.3", app: "2026.11.19"))
    #expect(Backend.isStale(server: "0.1.0", app: "2026.11.19"), "a service from source")
    #expect(Backend.isStale(server: nil, app: "2026.11.19"), "a service too old to say its version")
    #expect(!Backend.isStale(server: "0.1.0", app: "0"), "the app from source")
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
