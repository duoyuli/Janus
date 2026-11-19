import Foundation
import Testing
@testable import Janus

/// The store the tests' run models point at. `RunModel.store` is unowned, as the app's store lives as
/// long as the app; a store made inside a test would be freed while a view could still reach it, so
/// the tests share one that is never freed. Its API points at a closed port, so nothing leaves the
/// machine.
@MainActor let testStore = AppStore(api: API(base: URL(string: "http://127.0.0.1:9")!))

/// Builds an event log the way the orchestrator records one.
private struct Log {
    var events: [Event] = []
    mutating func add(_ type: String, actor: String = "janus", to: String? = nil, task: String? = nil, attempt: Int? = nil, at ts: Int? = nil, data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(ts ?? 1_000_000 + events.count * 1000), "type": JSON(type), "actor": JSON(actor), "text": "", "data": data]
        if let to { j["to"] = JSON(to) }
        if let task { j["task"] = JSON(task) }
        if let attempt { j["attempt"] = JSON(attempt) }
        events.append(Event(j))
    }

    /// Janus starts an attempt for `agent` in a session of its own.
    mutating func start(_ task: String, _ agent: String, attempt: Int = 1, session: String) {
        add("attempt.started", to: agent, task: task, attempt: attempt, data: ["session": JSON(session)])
    }

    mutating func turn(_ agent: String, _ task: String?, attempt: Int? = 1, _ data: JSON) {
        add("session.usage", actor: agent, task: task, attempt: task == nil ? nil : attempt, data: data)
    }
}

/// The cases the backend's ledger is tested on (`backend/testdata/usage_cases.json`): the app must
/// give the same numbers and the same summary line for the same events.
@Test func usageSharedCasesMatchTheBackend() throws {
    let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let cases = try JSON.parse(Data(contentsOf: repo.appendingPathComponent("backend/testdata/usage_cases.json"))).array
    #expect(cases.count >= 6)
    for c in cases {
        let name = c["name"].str
        let l = usageLedger(c["events"].array.map(Event.init))
        let want = c["expect"]
        #expect(l.run.tokens.total == want["runTotal"].int, "\(name): run total")
        #expect(l.run.completeness.label == L.catalogText(want["completeness"].str), "\(name): completeness")
        if L.locale.identifier == "zh-Hans" {
            #expect("用量：\(l.runLine)" == want["summary"].str, "\(name): \(l.runLine)")
        }
        for (task, total) in want["tasks"].object {
            #expect(l.task(task)?.tokens.total == total.int, "\(name): task \(task)")
        }
        for (task, usd) in want["taskUsd"].object {
            #expect(l.task(task)?.cost.lines.first?.amount == usd.double)
        }
        for (id, usd) in want["attemptUsd"].object {
            let attempt = try #require(l.attempts.first { $0.id == id })
            let slices = l.sessions.filter { $0.task == attempt.task && $0.attempt == attempt.n }
            #expect(UsageScope.over(id, slices).cost.lines.first?.amount == usd.double)
        }
        for (key, state) in want["attempts"].object {
            #expect(l.attempts.first { $0.id == key }?.state.label == L.catalogText(state.str), "\(name): attempt \(key)")
        }
        #expect(l.reminders.count == (want["reminders"].int ?? 0), "\(name): \(l.reminders)")
    }
}

/// Real per-turn numbers grow with the conversation; every turn is its own work and all of them add up.
@Test func usageAddsEveryTurnEvenWhenTheTurnsGrow() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    for (i, o, t) in [(600, 100, 700), (1200, 200, 1400), (1800, 300, 2100), (300, 50, 350)] {
        log.turn("m1", "T1", ["inputTokens": JSON(i), "outputTokens": JSON(o), "totalTokens": JSON(t)])
    }
    let l = usageLedger(log.events)
    #expect(l.run.tokens.total == 4550 && l.run.tokens.input == 3900 && l.run.tokens.output == 650)
    #expect(l.run.turns == 4 && l.run.completeness == .complete)
    #expect(l.run.tokensText == expectedCopy("4,550 tokens（完整）", "4,550 tokens (Complete)"))
}

/// The quota rows by model also count subagents, so they replace the main-loop `usage` for that turn.
@Test func usageQuotaRowsReplaceTheMainLoop() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    log.turn("m1", "T1", ["inputTokens": 1200, "outputTokens": 300, "cachedReadTokens": 5000, "cachedWriteTokens": 100, "totalTokens": 6600, "models": [
        ["model": "opus", "inputTokens": 1200, "outputTokens": 300, "cachedReadTokens": 5000, "cachedWriteTokens": 100, "totalTokens": 6600],
        ["model": "haiku", "inputTokens": 800, "outputTokens": 100, "totalTokens": 900],
    ]])
    log.turn("m1", "T1", ["inputTokens": 50, "outputTokens": 10, "totalTokens": 60])
    // A turn without a total adds its parts.
    log.turn("m1", "T1", ["inputTokens": 500, "outputTokens": 100, "cachedReadTokens": 2000, "totalTokens": JSON.null])
    let l = usageLedger(log.events)
    #expect(l.run.tokens.total == 7560 + 2600)
    #expect(l.run.breakdown == expectedCopy("输入 2,550 · 缓存读取 7,000 · 缓存写入 100 · 输出 510", "Input 2,550 · Cache Read 7,000 · Cache Write 100 · Output 510"))
    #expect(l.run.summaryRows.map(\.label) == ["Token", expectedCopy("构成", "Breakdown"), expectedCopy("费用", "Cost"), expectedCopy("耗时", "Elapsed")])
}

/// Context snapshots are the window right now: never tokens, shown as the latest and the peak.
@Test func usageContextSnapshotsAreNeverTokens() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    for used in [52000, 52000, 61000, 61000, 52000] { log.turn("m1", "T1", ["used": JSON(used), "size": 200000]) }
    let l = usageLedger(log.events)
    #expect(l.run.tokens.total == nil && l.run.completeness == .missing)
    #expect(l.run.context == UsageContext(used: 52000, peak: 61000, size: 200000, reports: 5))
    #expect(l.run.contextDetail == expectedCopy("52,000 / 200,000（26%） · 峰值 61,000", "52,000 / 200,000 (26%) · Peak: 61,000"))
    #expect(l.run.tokensText == expectedCopy("未上报", "Not reported"), "never 0 tokens")
}

/// Missing numbers are words, a session with no tokens makes the run partial, a reported 0 is 0,
/// and a task without a session is 「未开始」.
@Test func usageMissingIsWordsAndZeroIsANumber() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.add("orchestration.accepted", actor: "lead", data: ["tasks": [["id": "T1", "agent": "m1"], ["id": "T2", "agent": "m2"], ["id": "T3", "agent": "m1"]]])
    log.start("T1", "m1", session: "s1")
    log.turn("m1", "T1", ["totalTokens": 0])
    log.start("T2", "m2", session: "s2")
    let l = usageLedger(log.events)
    #expect(l.task("T1")?.tokensText == expectedCopy("0 tokens（完整）", "0 tokens (Complete)"))
    #expect(l.task("T2")?.tokensText == expectedCopy("未上报", "Not reported"))
    #expect(l.task("T3")?.notStarted == true && l.taskDetail(l.task("T3")!) == expectedCopy("未开始", "Not started"))
    #expect(l.run.completeness == .partial)
    #expect(l.subtotalLine.value == expectedCopy("负责人 未上报 · 协作者 0 tokens（部分上报）", "Lead: Not reported · Collaborators: 0 tokens (Partially reported)"))
}

/// Cost is a running total per session: the latest reading, plus what came before a reset; a
/// crashed zero is ignored, and currencies are never added together.
@Test func usageCostIsARunningTotalPerSessionAndCurrency() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    for amount in [0.1, 0.3, 0, 0.05, 0.2] { log.turn("m1", "T1", ["cost": ["amount": .number(amount), "currency": "USD"]]) }
    log.start("T2", "m2", session: "s2")
    log.turn("m2", "T2", ["cost": ["amount": 40, "currency": "credits"]])
    log.start("T3", "m3", session: "s3")
    log.turn("m3", "T3", ["cost": ["amount": 5]])
    let l = usageLedger(log.events)
    #expect(l.task("T1")?.costText == expectedCopy("费用：0.5 USD", "Cost: 0.5 USD"))
    #expect(l.run.costText == expectedCopy("费用：0.5 USD + 40 credits + 5（未标注币种）（币种或口径不同，未合并）", "Cost: 0.5 USD + 40 credits + 5 (currency unspecified) (reported separately due to different currencies or billing methods)"))
    #expect(l.run.cost.reportedSessions == 3)
}

/// The lead is part of the run total and shown apart from the collaborators.
@Test func usageCountsTheLeadApart() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.add("agent.status", actor: "lead", data: ["phase": "thinking", "session": "lead-1"])
    log.turn("lead", nil, ["totalTokens": 1500])
    log.start("T1", "m1", session: "s1")
    log.turn("m1", "T1", ["totalTokens": 800])
    let l = usageLedger(log.events)
    #expect((l.lead.tokens.total, l.collaborators.tokens.total, l.run.tokens.total) == (1500, 800, 2300))
    #expect(l.members.map(\.id) == ["lead", "m1"], "Janus, which wrote attempt.started, is not a member")
}

/// Every attempt keeps its own numbers and outcome; a cancel ends what was still running and says
/// who cancelled; a failure before the cancel stays a failure.
@Test func usageAttemptsKeepTheirOutcome() {
    var log = Log()
    log.add("run.created", actor: "user", at: 0, data: ["goal": "g"])
    log.add("attempt.started", to: "m1", task: "T1", attempt: 1, at: 1000, data: ["session": "s1"])
    log.add("session.usage", actor: "m1", task: "T1", attempt: 1, at: 2000, data: ["totalTokens": 700])
    log.add("attempt.failed", actor: "m1", task: "T1", attempt: 1, at: 3000, data: ["error": "脚本失败"])
    log.add("attempt.started", to: "m1", task: "T1", attempt: 2, at: 4000, data: ["session": "s1b"])
    log.add("session.usage", actor: "m1", task: "T1", attempt: 2, at: 5000, data: ["totalTokens": 900])
    log.add("attempt.started", to: "m2", task: "T2", attempt: 1, at: 5000, data: ["session": "s2"])
    log.add("task.skipped_by_decision", actor: "lead", task: "T1", at: 6000, data: ["reason": "不需要了"])
    log.add("run.cancelled", actor: "user", at: 9000)
    log.add("attempt.failed", actor: "m2", task: "T2", attempt: 1, at: 9100, data: ["error": "连接已关闭"])
    let l = usageLedger(log.events)
    let a = Dictionary(uniqueKeysWithValues: l.attempts.map { ($0.id, $0) })
    #expect(a["T1#1"]?.state == .failed && a["T1#1"]?.note == "脚本失败" && a["T1#1"]?.durationMs == 2000)
    #expect(a["T1#2"]?.state == .cancelled && a["T1#2"]?.note == expectedCopy("负责人取消了这个子任务", "The lead cancelled this subtask"))
    #expect(a["T2#1"]?.state == .cancelled && a["T2#1"]?.note == expectedCopy("任务被取消", "Run cancelled") && a["T2#1"]?.durationMs == 4100)
    #expect(l.task("T1")?.tokens.total == 1600, "a failed attempt's usage still counts")
}

/// Wall-clock time and the time spent waiting for a person are kept apart.
@Test func usageTimeIsWallClockAndWaitingIsApart() {
    var log = Log()
    log.add("run.created", actor: "user", at: 0, data: ["goal": "g"])
    log.add("attempt.started", to: "m1", task: "T1", attempt: 1, at: 1000, data: ["session": "s1"])
    log.add("approval.requested", actor: "m1", task: "T1", attempt: 1, at: 2000, data: ["key": "req-1"])
    log.add("approval.granted", actor: "user", task: "T1", attempt: 1, at: 32000, data: ["key": "req-1"])
    log.add("task.succeeded", actor: "m1", task: "T1", attempt: 1, at: 59000, data: ["summary": "完成"])
    let l = usageLedger(log.events)
    #expect(l.run.durationMs == 59000 && l.run.waitMs == 30000)
    #expect(l.task("T1")?.summaryRows.first { $0.label == expectedCopy("耗时", "Elapsed") }?.value == expectedCopy("58 秒（墙钟） · 其中等待你 30 秒", "58 sec (wall time) · Waiting for you: 30 sec"))
}

/// The ledger follows the replay position: each prefix of the log has its own numbers.
@Test @MainActor func usageFollowsTheReplayPosition() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    for n in [700, 1400, 2100] { log.turn("m1", "T1", ["totalTokens": JSON(n)]) }
    let run = RunModel(id: "usage-replay", store: testStore)
    run.events = log.events
    run.replay = log.events.count
    #expect(run.usage.run.tokens.total == 4200)
    run.replay = 3
    #expect(run.usage.run.tokens.total == 700)
    run.replay = log.events.count
    #expect(run.usage == usageLedger(log.events), "the same events give the same ledger")
}

/// Reminders are text with the default thresholds, prefixed when the numbers are not complete.
@Test func usageRemindersAreTextOnly() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.add("agent.status", actor: "lead", data: ["session": "lead-1"])
    log.turn("lead", nil, ["used": 4000, "size": 200000])
    log.start("T1", "m1", session: "s1")
    log.turn("m1", "T1", ["totalTokens": 520_000])
    let l = usageLedger(log.events)
    #expect(l.reminders.count == 3, "\(l.reminders)")
    #expect(l.remindersLine?.hasPrefix(expectedCopy("提醒（按已上报数据）：", "Notice (reported data only): ")) == true)
    #expect(usageLedger(Array(log.events.prefix(4))).remindersLine == nil)
}

/// Estimates are written as US dollars, to the cent; a tiny amount is not rounded to zero.
@Test func usageUSDIsWrittenInDollars() {
    #expect(usageUSD(6389.624) == "$6,389.62" && usageUSD(0.004) == "<$0.01" && usageUSD(0) == "$0.00")
}

/// The run card's wall-clock time counts from `run.created` while a live run is still going, and
/// freezes at the ledger's recorded value everywhere else: a read-only run, a finished run, and a
/// clock that reads earlier than the records. Only the run's own time moves.
@Test func usageDisplayCountsUpOnlyWhileTheRunIsLive() {
    var log = Log()
    log.add("run.created", actor: "user", at: 1_000_000, data: ["goal": "g"])
    log.add("agent.status", actor: "lead", at: 1_001_000, data: ["phase": "thinking", "session": "lead-1"])
    log.add("attempt.started", to: "m1", task: "T1", attempt: 1, at: 1_001_000, data: ["session": "s1"])
    log.add("approval.requested", actor: "m1", task: "T1", attempt: 1, at: 1_001_000, data: ["key": "req-1"])

    let running = derive(log.events)
    #expect(running.run.status == .running)
    #expect(running.usage.run.durationMs == 1_000, "the ledger still records run.created to the latest event")

    let live = running.usageForDisplay(now: 1_061_000, live: true)
    #expect(live.run.durationMs == 61_000, "live and unfinished: now minus run.created")
    #expect(live.runCardLine(tasks: 0, done: 0) == expectedCopy("耗时 1 分 01 秒（墙钟）", "Elapsed: 1 min 01 sec (wall time)"))
    // Everything but the run's own wall-clock time stays what the events recorded.
    #expect(live.run.waitMs == running.usage.run.waitMs && live.reminders == running.usage.reminders)
    #expect(live.tasks.map(\.durationMs) == running.usage.tasks.map(\.durationMs), "task durations stay recorded")
    #expect(live.attempts.map(\.durationMs) == running.usage.attempts.map(\.durationMs), "attempt durations stay recorded")

    #expect(running.usageForDisplay(now: 1_061_000, live: false).run.durationMs == 1_000, "a read-only run keeps the recorded value")
    #expect(running.usageForDisplay(now: 1_000_500, live: true).run.durationMs == 1_000, "a clock behind the records does not shrink it")

    var deliveredLog = log
    deliveredLog.add("run.delivered", actor: "lead", at: 1_002_000, data: ["summary": "完成"])
    let delivered = derive(deliveredLog.events)
    #expect(delivered.run.status == .delivered)
    #expect(delivered.usageForDisplay(now: 1_061_000, live: true).run.durationMs == 2_000, "a finished run freezes at the recorded value")
}

/// The run card and the snapshot share one displayed ledger and one live judgement: a replayed
/// position of a still-running prefix keeps that prefix's value and never ticks with the clock.
@Test @MainActor func usageDisplayKeepsAReplayedPrefixFrozen() {
    var log = Log()
    log.add("run.created", actor: "user", at: 1_000_000, data: ["goal": "g"])
    log.add("agent.status", actor: "lead", at: 1_001_000, data: ["phase": "thinking", "session": "lead-1"])
    let run = RunModel(id: "usage-display", store: testStore)
    run.events = log.events
    run.load = .loaded(live: true)
    run.replay = log.events.count
    #expect(run.shown.run.status == .running)
    #expect(!run.elapsedTicks, "a replayed position does not ask for a per-second timeline")
    #expect(run.usageDisplayed(at: 1_061_000).run.durationMs == 1_000, "a replay position uses its prefix's ledger value")
}

/// The summary line's visible text and its full-digit VoiceOver and hover text are built from the
/// same displayed ledger (`RunCard.summaryText`), so at one moment they carry the same wall-clock
/// value; the expanded usage detail keeps the recorded ledger and does not follow the clock.
@Test func usageDisplayKeepsTheSummaryLineAndItsVoiceInStep() {
    var log = Log()
    log.add("run.created", actor: "user", at: 1_000_000, data: ["goal": "g"])
    log.add("agent.status", actor: "lead", at: 1_001_000, data: ["phase": "thinking", "session": "lead-1"])
    let running = derive(log.events)
    let u = running.usageForDisplay(now: 1_061_000, live: true)
    let elapsed = L.dur(61_000)
    #expect(u.runCardLine(tasks: 0, done: 0).contains(elapsed), "the visible line counts up")
    #expect(u.runCardVoice(tasks: 0, done: 0).contains(elapsed), "VoiceOver and the hover tip read the same value")
    #expect(running.usage.run.durationMs == 1_000, "the expanded detail's recorded ledger does not tick")
}

/// When `T2` continues `T1`'s ACP session (`continues`), each task keeps its own turn tokens.
@Test func usageContinuedSessionAttributesEachTasksTurns() {
    var log = Log()
    log.add("run.created", actor: "user", data: ["goal": "g"])
    log.start("T1", "m1", session: "s1")
    log.turn("m1", "T1", ["totalTokens": 600])
    log.add("task.succeeded", actor: "m1", task: "T1", attempt: 1, data: ["summary": "初版"])
    log.start("T2", "m1", session: "s1")
    log.turn("m1", "T2", ["totalTokens": 250])
    log.add("task.succeeded", actor: "m1", task: "T2", attempt: 1, data: ["summary": "修复"])
    let l = usageLedger(log.events)
    #expect(l.task("T1")?.tokens.total == 600)
    #expect(l.task("T2")?.tokens.total == 250)
    #expect(l.run.tokens.total == 850)
}
