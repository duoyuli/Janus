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
    "reminders": [["kind": "oneTask", "task": "T1", "total": 16900, "share": 0.91], ["kind": "costUnknown"]],
    "words": ["hasUsage": true, "card": ["用量 16.9K tokens（部分上报）", "费用 0.25 USD", "耗时 59 秒（总时长）", "上下文 26%"],
              "voice": ["用量 16,900 tokens（部分上报）", "费用 0.25 USD", "耗时 59 秒（总时长）", "上下文 52,000 / 200,000（26%） · 峰值 61,000"],
              "elapsed": 2, "reminders": "提醒（按已上报数据）：大部分用量集中在一个子任务：T1 占 16,900 的 91%",
              "rows": [["label": "Token", "value": "16,900 tokens（部分上报）", "help": "h"]], "sessions": "3 个会话",
              "tasks": [["id": "T1", "text": "15,400 tokens（完整） · 58 秒", "help": "t"]], "members": []],
]

/// The server's ledger reads into every range, with missing numbers kept missing.
@Test func theServersLedgerReadsIntoEveryRange() {
    let l = UsageLedger.server(serverLedger)
    #expect(l.run.tokens.total == 16900 && l.run.tokens.cachedRead == nil, "a count nobody reported stays unreported")
    #expect(l.run.completeness == .partial && l.run.waitMs == 30000 && l.run.context.peak == 61000)
    #expect(l.run.cost.lines == [UsageCostLine(currency: "USD", amount: 0.25, sessions: 1)])
    #expect(l.lead.tokens.total == 1500 && l.collaborators.tokens.total == 15400)
    #expect(l.task("T2")?.sessions == 0 && l.task("T2")?.tokens.total == nil)
    #expect(l.attempts.map(\.state) == [.succeeded, .verificationFailed] && l.attempts[1].note == "没有测试" && l.attempts[1].durationMs == nil)
    #expect(l.sessions.first?.context.ratio == 0.26)
    #expect(l.words.hasUsage && l.words.elapsed == 2 && l.words.reminders?.contains("91%") == true)
    #expect(l.words.rows.first?.label == "Token" && l.words.tasks.first?.id == "T1" && l.words.sessions == "3 个会话")
}

/// The run card's wall-clock time counts from the run's start while the run is ticking (live,
/// not replayed, still going), and keeps the recorded value everywhere else: a read-only or
/// finished run, and a clock that reads earlier than the records.
@Test func usageDisplayCountsUpOnlyWhileTheRunTicks() {
    let l = UsageLedger.server(serverLedger)
    let start = 1_000_000.0
    #expect(l.elapsedMs(at: start + 61_000, since: start, ticking: true) == 61_000, "ticking: now minus the start")
    let card = l.cardLine(progress: nil, at: start + 61_000, since: start, ticking: true)
    #expect(card.line.contains(L.dur(61_000)) && card.voice.contains(L.dur(61_000)), "the visible line and its VoiceOver text carry the same value")
    #expect(l.elapsedMs(at: start + 61_000, since: start, ticking: false) == 59_000, "not ticking: the recorded value")
    #expect(l.elapsedMs(at: start + 500, since: start, ticking: true) == 59_000, "a clock behind the records does not shrink it")
    #expect(l.elapsedMs(at: start + 61_000, since: 0, ticking: true) == 59_000, "no start, nothing to count from")
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
    #expect(run.elapsedMs(at: 1_061_000) == 59_000, "a replay position keeps its ledger's value")
}
