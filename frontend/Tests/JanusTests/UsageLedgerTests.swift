import Foundation
import Testing
@testable import Janus

/// The store the tests' run models point at. `RunModel.store` is unowned, as the app's store lives as
/// long as the app; a store made inside a test would be freed while a view could still reach it, so
/// the tests share one that is never freed. Its API points at a closed port, so nothing leaves the
/// machine.
@MainActor let testStore = AppStore(api: API(base: URL(string: "http://127.0.0.1:9")!))

/// A ledger shaped as `GET /api/runs/{id}/usage` returns it (`backend/src/usage.rs`): a partial run
/// with one task, the lead, a cost and a reminder. The numbers are the server's; the app only reads
/// and words them.
let serverLedger: JSON = [
    "run": ["id": "run", "tokens": ["total": 16900, "input": 12000, "output": 3400], "turns": 2, "context": ["used": 52000, "peak": 61000, "size": 200000, "reports": 2],
            "cost": ["lines": [["currency": "USD", "amount": 0.25, "sessions": 1]], "reportedSessions": 1, "missingSessions": 2],
            "durationMs": 59000, "waitMs": 30000, "sessions": 3, "reportedSessions": 2, "missingSessions": 1, "completeness": "partial"],
    "lead": ["id": "lead", "tokens": ["total": 1500], "turns": 1, "context": ["reports": 0], "cost": ["lines": [], "reportedSessions": 1, "missingSessions": 0],
             "sessions": 1, "reportedSessions": 1, "missingSessions": 0, "completeness": "complete"],
    "collaborators": ["id": "collaborators", "tokens": ["total": 15400], "turns": 1, "context": ["reports": 2], "cost": ["lines": [], "reportedSessions": 0, "missingSessions": 2],
                      "sessions": 2, "reportedSessions": 1, "missingSessions": 1, "completeness": "partial"],
    "tasks": [["id": "T1", "tokens": ["total": 15400], "turns": 1, "context": ["reports": 2], "cost": ["lines": []], "durationMs": 58000, "sessions": 1, "reportedSessions": 1, "missingSessions": 0, "completeness": "complete"],
              ["id": "T2", "tokens": [:], "turns": 0, "context": ["reports": 0], "cost": ["lines": []], "sessions": 0, "reportedSessions": 0, "missingSessions": 0, "completeness": "missing"]],
    "members": [],
    "attempts": [["task": "T1", "n": 1, "actor": "researcher", "state": "succeeded", "startedMs": 1000, "endedMs": 59000, "durationMs": 58000],
                 ["task": "T2", "n": 1, "actor": "implementer", "state": "verificationFailed", "note": "没有测试", "startedMs": 2000]],
    "sessions": [["key": "s1", "actor": "researcher", "task": "T1", "attempt": 1, "tokens": ["total": 15400], "turns": 1, "context": ["used": 52000, "size": 200000, "reports": 2], "cost": [], "firstMs": 1000, "lastMs": 59000]],
    "reminders": [["kind": "oneTask", "task": "T1", "total": 16900, "share": 0.91], ["kind": "costUnknown"], ["kind": "somethingNew"]],
]

/// The server's ledger reads into every range, with missing numbers kept missing.
@Test func theServersLedgerReadsIntoEveryRange() {
    let l = UsageLedger.server(serverLedger)
    #expect(l.run.tokens.total == 16900 && l.run.tokens.cachedRead == nil, "a count nobody reported stays unreported")
    #expect(l.run.completeness == .partial && l.run.waitMs == 30000 && l.run.context.peak == 61000)
    #expect(l.run.cost.lines == [UsageCostLine(currency: "USD", amount: 0.25, sessions: 1)])
    #expect(l.lead.tokens.total == 1500 && l.collaborators.tokens.total == 15400)
    #expect(l.task("T2")?.notStarted == true && l.task("T2")?.tokens.total == nil)
    #expect(l.attempts.map(\.state) == [.succeeded, .verificationFailed] && l.attempts[1].note == "没有测试" && l.attempts[1].durationMs == nil)
    #expect(l.sessions.first?.context.ratio == 0.26)
    #expect(l.hasUsage)
    #expect(l.reminders == [.oneTask(task: "T1", total: 16900, share: 0.91), .costUnknown], "a reminder this build does not know is left out")
}

/// Reminders are worded in the app's language from their numbers, prefixed when the numbers are
/// not complete; nothing reported, no line.
@Test func usageRemindersAreWordedFromTheirNumbers() {
    let l = UsageLedger.server(serverLedger)
    #expect(l.remindersLine?.hasPrefix(expectedCopy("提醒（按已上报数据）：", "Notice (reported data only): ")) == true)
    #expect(l.remindersLine?.contains("T1") == true && l.remindersLine?.contains("16,900") == true && l.remindersLine?.contains("91%") == true)
    #expect(UsageReminder.tokens(total: 520_000, threshold: 500_000).text.contains("520,000"))
    #expect(UsageReminder.idleTeam(collaborators: 6, minutes: 30).text.contains("30"))
    #expect(UsageLedger.empty.remindersLine == nil)
}

/// Estimates are written as US dollars, to the cent; a tiny amount is not rounded to zero.
@Test func usageUSDIsWrittenInDollars() {
    #expect(usageUSD(6389.624) == "$6,389.62" && usageUSD(0.004) == "<$0.01" && usageUSD(0) == "$0.00")
}

/// The run card's wall-clock time counts from the run's start while the run is ticking (live,
/// not replayed, still going), and keeps the recorded value everywhere else: a read-only or
/// finished run, and a clock that reads earlier than the records. Only the run's own time moves.
@Test func usageDisplayCountsUpOnlyWhileTheRunTicks() {
    let l = UsageLedger.server(serverLedger)
    let start = 1_000_000.0
    let live = l.displayed(at: start + 61_000, since: start, ticking: true)
    #expect(live.run.durationMs == 61_000, "ticking: now minus the start")
    #expect(live.runCardLine(tasks: 0, done: 0).contains(L.dur(61_000)) && live.runCardVoice(tasks: 0, done: 0).contains(L.dur(61_000)),
            "the visible line and its VoiceOver text carry the same value")
    #expect(live.run.waitMs == l.run.waitMs && live.reminders == l.reminders && live.tasks == l.tasks && live.attempts == l.attempts,
            "everything but the run's own time stays recorded")
    #expect(l.displayed(at: start + 61_000, since: start, ticking: false).run.durationMs == 59_000, "not ticking: the recorded value")
    #expect(l.displayed(at: start + 500, since: start, ticking: true).run.durationMs == 59_000, "a clock behind the records does not shrink it")
    #expect(l.displayed(at: start + 61_000, since: 0, ticking: true).run.durationMs == 59_000, "no start, nothing to count from")
}

/// A replayed position of a running prefix never ticks with the clock.
@Test @MainActor func usageDisplayKeepsAReplayedPrefixFrozen() {
    let events = [
        Event(["seq": 1, "ts": 1_000_000, "type": "run.created", "actor": "user", "text": "", "data": ["goal": "g"]]),
        Event(["seq": 2, "ts": 1_001_000, "type": "agent.status", "actor": "lead", "text": "", "data": ["phase": "thinking", "session": "lead-1"]]),
    ]
    let run = RunModel(id: "usage-display", store: testStore)
    run.events = events
    run.load = .loaded(live: true)
    run.replay = events.count
    run.usage = UsageLedger.server(serverLedger)
    #expect(!run.elapsedTicks, "a replayed position does not ask for a per-second timeline")
    #expect(run.usageDisplayed(at: 1_061_000).run.durationMs == 59_000, "a replay position keeps its ledger's value")
}
