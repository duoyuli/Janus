import Foundation

// The run's read-only usage ledger, as the server computes it from the event log
// (`GET /api/runs/{id}/usage`, `backend/src/usage.rs`, docs/architecture/20261031-frontend.md#usage_ledger).
// The app reads it for the live run and for each replayed position, with the words the server
// writes for it (`words`, `backend/src/usage/words.rs`): the numbers, the reminders and their
// thresholds have one source, the server's. Missing numbers are words, never 0.

/// Whether a range's token numbers are all there.
enum UsageCompleteness: String {
    case missing, complete, partial
}

/// Token counts under the ACP names. `nil` is "not reported", never 0. `thought` is part of
/// `output`; the cache counts are part of `total`.
struct UsageTokens: Equatable {
    var total: Int?
    var input: Int?
    var output: Int?
    var cachedRead: Int?
    var cachedWrite: Int?
    var thought: Int?

    /// Token counts as the server writes them, in a ledger or a bill.
    static func server(_ j: JSON) -> UsageTokens {
        UsageTokens(total: j["total"].int, input: j["input"].int, output: j["output"].int,
                    cachedRead: j["cachedRead"].int, cachedWrite: j["cachedWrite"].int, thought: j["thought"].int)
    }

    /// Cache reads over input plus cache reads and writes; nil unless input and cache reads were reported.
    var cacheHitRate: Double? {
        guard let input, let cachedRead else { return nil }
        let prompt = input + cachedRead + (cachedWrite ?? 0)
        return prompt > 0 ? Double(cachedRead) / Double(prompt) : nil
    }
}

/// Context window snapshots (`used` / `size`): only the latest value, the peak and the ratio.
struct UsageContext: Equatable {
    var used: Int?
    var peak: Int?
    var size: Int?
    var reports = 0

    var ratio: Double? {
        guard let used, let size, size > 0 else { return nil }
        return Double(used) / Double(size)
    }

    static func server(_ j: JSON) -> UsageContext {
        UsageContext(used: j["used"].int, peak: j["peak"].int, size: j["size"].int, reports: j["reports"].int ?? 0)
    }
}

/// A reported cost in one currency, as the agent gave it; an empty currency means none was named.
struct UsageCostLine: Equatable {
    let currency: String
    var amount: Double
    var sessions = 1
}

/// A range's cost: one line per currency, added only within the same currency.
struct UsageCost: Equatable {
    var lines: [UsageCostLine] = []
    var reportedSessions = 0
    var missingSessions = 0

    var reported: Bool { reportedSessions > 0 }

    /// A cost as the server writes it, in a ledger or a bill.
    static func server(_ j: JSON) -> UsageCost {
        UsageCost(lines: j["lines"].array.map(UsageCostLine.server),
                  reportedSessions: j["reportedSessions"].int ?? 0, missingSessions: j["missingSessions"].int ?? 0)
    }
}

extension UsageCostLine {
    static func server(_ j: JSON) -> UsageCostLine {
        UsageCostLine(currency: j["currency"].str, amount: j["amount"].double ?? 0, sessions: j["sessions"].int ?? 1)
    }
}

/// One ACP session (or one lead session): the unit the numbers are counted in.
struct UsageSession: Identifiable, Equatable {
    var key: String
    var actor: String
    var task: String?
    var attempt: Int?
    var tokens = UsageTokens()
    /// Turns that reported tokens.
    var turns = 0
    var context = UsageContext()
    var cost: [UsageCostLine] = []
    var firstMs = 0
    var lastMs = 0
    var id: String { key }
}

/// One range: the run, the lead, the collaborators, a task, a member or an attempt.
struct UsageScope: Equatable {
    var id = ""
    var tokens = UsageTokens()
    var turns = 0
    var context = UsageContext()
    var cost = UsageCost()
    /// Wall-clock span; nil when the range has no records, so it is not shown as 0.
    var durationMs: Int?
    /// Time the range waited for a person or the lead; nil when nothing was waited on.
    var waitMs: Int?
    var sessions = 0
    var reportedSessions = 0
    var missingSessions = 0
    var completeness: UsageCompleteness = .missing

    static func server(_ j: JSON) -> UsageScope {
        UsageScope(id: j["id"].str, tokens: .server(j["tokens"]), turns: j["turns"].int ?? 0, context: .server(j["context"]),
                   cost: .server(j["cost"]), durationMs: j["durationMs"].int, waitMs: j["waitMs"].int, sessions: j["sessions"].int ?? 0,
                   reportedSessions: j["reportedSessions"].int ?? 0, missingSessions: j["missingSessions"].int ?? 0,
                   completeness: UsageCompleteness(rawValue: j["completeness"].str) ?? .missing)
    }
}

/// One attempt of a task: how it ended and how long it took.
struct UsageAttempt: Identifiable, Equatable {
    enum State: String {
        case unfinished, succeeded, failed, verificationFailed, expired, cancelled

        var label: String {
            switch self {
            case .unfinished: return L.text("未完成")
            case .succeeded: return L.text("成功")
            case .failed: return L.text("失败")
            case .verificationFailed: return L.text("验证没通过")
            case .expired: return L.text("已过期但仍计入")
            case .cancelled: return L.text("已取消")
            }
        }
    }

    var task: String
    var n: Int
    var actor: String
    var state: State = .unfinished
    var note: String?
    var startedMs = 0
    var endedMs: Int?
    var durationMs: Int?
    var id: String { "\(task)#\(n)" }

    static func server(_ j: JSON) -> UsageAttempt {
        UsageAttempt(task: j["task"].str, n: j["n"].int ?? 0, actor: j["actor"].str, state: State(rawValue: j["state"].str) ?? .unfinished,
                     note: j["note"].string, startedMs: j["startedMs"].int ?? 0, endedMs: j["endedMs"].int, durationMs: j["durationMs"].int)
    }
}

extension UsageSession {
    static func server(_ j: JSON) -> UsageSession {
        UsageSession(key: j["key"].str, actor: j["actor"].str, task: j["task"].string, attempt: j["attempt"].int, tokens: .server(j["tokens"]),
                     turns: j["turns"].int ?? 0, context: .server(j["context"]), cost: j["cost"].array.map(UsageCostLine.server),
                     firstMs: j["firstMs"].int ?? 0, lastMs: j["lastMs"].int ?? 0)
    }
}

/// Where the collaborators' time went (`TimeSplit` in the server): each attempt's wall-clock time
/// as work, verification or rework, and the span they covered.
struct UsageTime: Equatable {
    var workMs = 0
    var verifyMs = 0
    var reworkMs = 0
    var spanMs = 0


    static func server(_ j: JSON) -> UsageTime {
        UsageTime(workMs: j["workMs"].int ?? 0, verifyMs: j["verifyMs"].int ?? 0, reworkMs: j["reworkMs"].int ?? 0,
                  spanMs: j["spanMs"].int ?? 0)
    }

}

/// The ledger in the person's language, as the server writes it (`words`,
/// `backend/src/usage/words.rs`): the run card's numbers, the reminders, the expanded rows and
/// each task's and member's detail.
struct UsageWords: Equatable {
    struct Row: Equatable, Identifiable {
        let label: String
        let value: String
        let help: String
        var id: String { label }
    }

    struct Detail: Equatable {
        let id: String
        let text: String
        let help: String
    }

    var hasUsage = false
    /// The run card's items after the subtasks' progress; `elapsed` is the wall-clock one.
    var card: [String] = []
    var voice: [String] = []
    var elapsed = 0
    var reminders: String?
    var rows: [Row] = []
    var sessions = ""
    var tasks: [Detail] = []
    var members: [Detail] = []

    static func server(_ j: JSON) -> UsageWords {
        let detail = { (d: JSON) in Detail(id: d["id"].str, text: d["text"].str, help: d["help"].str) }
        return UsageWords(hasUsage: j["hasUsage"].bool ?? false, card: j["card"].strings, voice: j["voice"].strings,
                          elapsed: j["elapsed"].int ?? 0, reminders: j["reminders"].string,
                          rows: j["rows"].array.map { Row(label: $0["label"].str, value: $0["value"].str, help: $0["help"].str) },
                          sessions: j["sessions"].str, tasks: j["tasks"].array.map(detail), members: j["members"].array.map(detail))
    }
}

/// The run's ledger: every range over the same events, and its words.
struct UsageLedger: Equatable {
    var run = UsageScope(id: "run")
    var lead = UsageScope(id: "lead")
    var collaborators = UsageScope(id: "collaborators")
    var tasks: [UsageScope] = []
    var members: [UsageScope] = []
    var attempts: [UsageAttempt] = []
    var time = UsageTime()
    var sessions: [UsageSession] = []
    var words = UsageWords()

    static let empty = UsageLedger()

    /// The ledger as `GET /api/runs/{id}/usage` returns it.
    static func server(_ j: JSON) -> UsageLedger {
        UsageLedger(run: .server(j["run"]), lead: .server(j["lead"]), collaborators: .server(j["collaborators"]),
                    tasks: j["tasks"].array.map(UsageScope.server), members: j["members"].array.map(UsageScope.server),
                    attempts: j["attempts"].array.map(UsageAttempt.server), time: .server(j["time"]),
                    sessions: j["sessions"].array.map(UsageSession.server), words: .server(j["words"]))
    }

    func task(_ id: String) -> UsageScope? { tasks.first { $0.id == id } }

    /// Whether the run card offers the ledger's disclosure. A position with reports offers it;
    /// once the ledger is open the control stays, so a replayed position that reported nothing
    /// can still fold it again.
    func showsLedgerControl(open: Bool) -> Bool { words.hasUsage || open }

    /// The run's wall-clock time at `now`. While the run is `ticking` (live, not a replayed
    /// position, still going) and has a start, it counts from `start` to `now`, never below the
    /// recorded value, so a clock that moves back does not make it shrink. Otherwise the recorded value.
    // @project-doc docs/architecture/20261031-frontend.md#usage_ledger
    func elapsedMs(at now: Double, since start: Double, ticking: Bool) -> Int? {
        guard ticking, start > 0 else { return run.durationMs }
        return max(run.durationMs ?? 0, Int(max(0, now - start).rounded()))
    }

    /// The run card's line (`·`) and its full-digit reading (`，`) at `now`: the subtasks'
    /// progress, then the server's items with the wall-clock one counted on while ticking.
    func cardLine(progress: String?, at now: Double, since start: Double, ticking: Bool) -> (line: String, voice: String) {
        func items(_ xs: [String]) -> [String] {
            var xs = xs
            if ticking, start > 0, words.elapsed < xs.count, let ms = elapsedMs(at: now, since: start, ticking: true) {
                xs[words.elapsed] = L.text("耗时 \(L.text("\(L.dur(Double(ms)))（总时长）"))")
            }
            return [progress].compactMap { $0 } + xs
        }
        return (items(words.card).joined(separator: " · "), items(words.voice).joined(separator: L.text("，")))
    }
}

/// One line a usage card renders: a label and the value beside it.
struct UsageLine: Equatable, Identifiable {
    let label: String
    let value: String
    var id: String { label }
}

extension UsageScope {
    /// `0.25 USD`, `0.25 USD + 40 credits（币种或口径不同，未合并）`, or `未上报`, as a bill's groups show it.
    var costDetail: String {
        let parts = cost.lines.map { l -> String in
            if l.currency.isEmpty { return L.text("\(usageMoney(l.amount))（未标注币种）") }
            return l.sessions > 1 ? L.text("\(usageMoney(l.amount)) \(l.currency)（\(l.sessions) 个会话）") : "\(usageMoney(l.amount)) \(l.currency)"
        }
        if parts.isEmpty { return L.text("未上报") }
        if parts.count == 1 { return parts[0] }
        return parts.joined(separator: " + ") + L.text("（币种或口径不同，未合并）")
    }
}

/// Thousands separators for token counts, the same on every page.
func usageNum(_ n: Int) -> String {
    let digits = String(n.magnitude)
    var out = ""
    for (i, c) in digits.enumerated() {
        if i > 0, (digits.count - i) % 3 == 0 { out.append(",") }
        out.append(c)
    }
    return n < 0 ? "-" + out : out
}

/// `26%` for a context ratio.
func usagePercent(_ ratio: Double) -> String { "\(Int((ratio * 100).rounded()))%" }

/// A reported amount exactly as the agent gave it, to four decimals at most: no conversion, no estimate.
func usageMoney(_ v: Double) -> String {
    let rounded = (v * 10_000).rounded() / 10_000
    if rounded == rounded.rounded(), abs(rounded) < 1e15 { return String(Int(rounded)) }
    var s = String(format: "%.4f", rounded)
    while s.hasSuffix("0") { s.removeLast() }
    return s
}
