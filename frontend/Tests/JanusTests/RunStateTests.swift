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
                                           "rows": [["n": 1, "criterion": "测试通过", "check": ["criterion": "测试通过", "passed": true, "method": "test"],
                                                     "line": ["mark": "passed", "method": ["text": "测试", "tone": "green", "icon": "checkmark.seal", "help": "h"],
                                                              "criterion": "测试通过", "evidence": "", "noEvidence": "没有给出证据", "folds": false, "voice": "v"]],
                                                    ["n": 2, "criterion": "有保护", "check": .null,
                                                     "line": ["mark": "unchecked", "method": ["text": "未核对", "tone": "gray", "icon": "minus.circle", "help": "h"],
                                                              "criterion": "有保护", "evidence": "", "folds": false, "voice": "v"]]],
                                           "verifications": [["task": "V1", "status": "succeeded", "verdict": "rejected", "passed": false,
                                                              "checks": [["criterion": "边界通过", "passed": false, "method": "command", "evidence": "panic"]],
                                                              "lines": [["mark": "failed", "method": ["text": "命令", "tone": "blue", "icon": "terminal", "help": "h"],
                                                                         "criterion": "边界通过", "evidence": "panic", "folds": false, "voice": "v"]]]]]]],
                      events: [])
    #expect(st.tasks["T2"]?.checks.first?["method"].str == "test" && st.tasks["T2"]?.risks == ["没有覆盖负数"])
    let e = try #require(st.evidence["T2"])
    #expect(e.rows.map(\.n) == [1, 2] && e.rows[1].line.mark == .unchecked, "a criterion nothing answered keeps its row")
    #expect(e.rows[0].line.method == CheckView.Method(text: "测试", tone: .green, icon: "checkmark.seal", help: "h") && e.rows[0].line.noEvidence == "没有给出证据")
    let v = try #require(e.verifications.first)
    #expect(v.task == "V1" && v.verdict == .rejected && !v.passed, "completed negative review is not an approval")
    #expect(v.checks.first?.method.text == "命令" && v.checks.first?.mark == .failed, "a verifier's checks are the lines the server wrote")
}

/// A run's row shows the label the server wrote; a queued run is neither at work nor finished.
@Test func aRunRowShowsTheServedLabel() {
    #expect(!RunStatus.queued.isActive && !RunStatus.queued.isFinished)
    #expect(runLabel(["label": ["text": "排队中 · 第 2 位", "tone": "gray"]]) == ("排队中 · 第 2 位", .gray))
    #expect(runLabel([:]).1 == .gray)
}

/// A Loop's schedule and phase read as the server words them, with times in this Mac's words.
@Test func aLoopsScheduleAndPhaseReadAsShown() {
    let loop: JSON = ["scheduleText": "每 2 小时", "pill": ["text": "已暂停", "tone": "gray"]]
    #expect(LoopText.schedule(loop) == "每 2 小时")
    #expect(LoopText.phase(loop) == ("已暂停", .gray))
    // A time it asked for says so, with why.
    let at = JSON(floatLiteral: L.nowMs() + 600_000)
    let asked: JSON = ["pill": ["text": "摸鱼中", "tone": "gray"], "wakesAt": at, "asked": ["at": at, "why": "等 CI"]]
    #expect(LoopText.asked(asked)?.hasSuffix(expectedCopy("再看一次：等 CI", ": 等 CI")) == true)
    #expect(LoopText.asked(loop) == nil)
    // The narrow column's pill keeps to the state; when it wakes goes on the line below.
    #expect(LoopText.phase(asked).0.hasPrefix(expectedCopy("摸鱼中 · ", "Idle · ")))
    #expect(LoopText.phase(asked, short: true).0 == "摸鱼中")
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

@Test func homePathsReadWithATilde() {
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

/// An installed app does not use a Janus service of another release (its data upgrade has not run);
/// an app run from source uses whatever runs.
@Test func aServiceOfAnotherReleaseIsStale() {
    #expect(!Backend.isStale(server: "2026.11.19", app: "2026.11.19"))
    #expect(Backend.isStale(server: "2026.11.18.3", app: "2026.11.19"))
    #expect(Backend.isStale(server: "0.1.0", app: "2026.11.19"), "a service from source")
    #expect(Backend.isStale(server: nil, app: "2026.11.19"), "a service too old to say its version")
    #expect(!Backend.isStale(server: "0.1.0", app: "0"), "the app from source")
}

/// The form writes its common schedules as cron expressions (the server reads them back into the
/// same shape, `loops/words.rs`).
@Test func aLoopsFormWritesItsScheduleAsCron() {
    #expect(LoopSchedule.minutes(15).cron == "*/15 * * * *")
    #expect(LoopSchedule.hours(1).cron == "0 * * * *" && LoopSchedule.hours(6).cron == "0 */6 * * *")
    #expect(LoopSchedule.daily(hour: 9, minute: 0).cron == "0 9 * * *")
    #expect(LoopSchedule.weekdays(hour: 9, minute: 0).cron == "0 9 * * 1-5")
    #expect(LoopSchedule.weekly(day: 0, hour: 20, minute: 5).cron == "5 20 * * 0")
    #expect(LoopSchedule.monthly(day: 1, hour: 8, minute: 0).cron == "0 8 1 * *")
    // The time menu offers every five minutes and keeps a minute a Loop already has.
    #expect(LoopSchedule.minuteMarks(keeping: 0) == Array(stride(from: 0, to: 60, by: 5)))
    #expect(LoopSchedule.minuteMarks(keeping: 7) == [0, 5, 7, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55])
}
