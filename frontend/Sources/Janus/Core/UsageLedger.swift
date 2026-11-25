import Foundation

// The run's read-only usage ledger, as the server computes it from the event log
// (`GET /api/runs/{id}/usage`, `backend/src/usage.rs`, docs/architecture/20261031-frontend.md#usage_ledger).
// The app reads it for the live run and for each replayed position and words it: the numbers,
// the reminders and their thresholds have one source, the server's. Missing numbers are words,
// never 0.

/// Whether a range's token numbers are all there.
enum UsageCompleteness: String {
    case missing, complete, partial

    var label: String {
        switch self {
        case .missing: return L.text("未上报")
        case .complete: return L.text("完整")
        case .partial: return L.text("部分上报")
        }
    }

    var tone: Tone {
        switch self {
        case .missing: return .gray
        case .complete: return .green
        case .partial: return .amber
        }
    }

    static func of(reported: Int, missing: Int) -> UsageCompleteness {
        reported == 0 ? .missing : missing == 0 ? .complete : .partial
    }
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

/// Tokens and turns on one model, in a bill.
struct UsageModelLine: Equatable, Identifiable {
    var model: String
    var tokens = UsageTokens()
    var turns = 0
    /// USD at the model's list price; nil when it has none.
    var estimate: Double?
    var id: String { model }
}

/// Who paid in a period's bill: an Agent, with its models (from the backend).
struct UsageBillGroup: Equatable, Identifiable {
    var id: String
    var name: String
    var tokens = UsageTokens()
    var turns = 0
    var cost = UsageCost()
    var models: [UsageModelLine] = []
    /// The priced models' estimates added up; nil when none has a price.
    var estimate: Double?
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

/// A read-only reminder about the run's usage, with the numbers behind it (`Reminder` in the server).
enum UsageReminder: Equatable {
    case tokens(total: Int, threshold: Int)
    case oneTask(task: String, total: Int, share: Double)
    case idleTeam(collaborators: Int, minutes: Int)
    case costUnknown

    static func server(_ j: JSON) -> UsageReminder? {
        switch j["kind"].str {
        case "tokens": return .tokens(total: j["total"].int ?? 0, threshold: j["threshold"].int ?? 0)
        case "oneTask": return .oneTask(task: j["task"].str, total: j["total"].int ?? 0, share: j["share"].double ?? 0)
        case "idleTeam": return .idleTeam(collaborators: j["collaborators"].int ?? 0, minutes: j["minutes"].int ?? 0)
        case "costUnknown": return .costUnknown
        default: return nil
        }
    }

    var text: String {
        switch self {
        case .tokens(let total, let threshold): return L.text("Token 用量偏高：已上报 \(usageNum(total)) tokens，达到默认提醒阈值 \(usageNum(threshold))")
        case .oneTask(let task, let total, let share): return L.text("大部分用量集中在一个子任务：\(task) 占 \(usageNum(total)) 的 \(usagePercent(share))")
        case .idleTeam(let collaborators, let minutes): return L.text("协作者多但没有有效产出：\(collaborators) 个协作者，\(minutes) 分钟内没有任何子任务成功")
        case .costUnknown: return L.text("费用算不出来：有 Token 用量，但没有会话上报费用")
        }
    }
}

/// Where the collaborators' time went (`TimeSplit` in the server): each attempt's wall-clock time
/// as work, verification or rework, and the span they covered.
struct UsageTime: Equatable {
    var workMs = 0
    var verifyMs = 0
    var reworkMs = 0
    var spanMs = 0

    var totalMs: Int { workMs + verifyMs + reworkMs }

    static func server(_ j: JSON) -> UsageTime {
        UsageTime(workMs: j["workMs"].int ?? 0, verifyMs: j["verifyMs"].int ?? 0, reworkMs: j["reworkMs"].int ?? 0,
                  spanMs: j["spanMs"].int ?? 0)
    }

    /// 「42 分：执行 20 分 · 验证 15 分 · 返工 7 分」, the usage card's 「子任务」 row; nothing before any attempt.
    var value: String? {
        guard totalMs > 0 else { return nil }
        var parts = [L.text("执行 \(L.dur(Double(workMs)))")]
        if verifyMs > 0 { parts.append(L.text("验证 \(L.dur(Double(verifyMs)))")) }
        if reworkMs > 0 { parts.append(L.text("返工 \(L.dur(Double(reworkMs)))")) }
        return L.text("\(L.dur(Double(totalMs)))：\(parts.joined(separator: " · "))")
    }
}

/// The run's ledger: every range over the same events, plus reminders that change nothing.
struct UsageLedger: Equatable {
    var run = UsageScope(id: "run")
    var lead = UsageScope(id: "lead")
    var collaborators = UsageScope(id: "collaborators")
    var tasks: [UsageScope] = []
    var members: [UsageScope] = []
    var attempts: [UsageAttempt] = []
    var time = UsageTime()
    var sessions: [UsageSession] = []
    var reminders: [UsageReminder] = []

    static let empty = UsageLedger()

    /// The ledger as `GET /api/runs/{id}/usage` returns it.
    static func server(_ j: JSON) -> UsageLedger {
        UsageLedger(run: .server(j["run"]), lead: .server(j["lead"]), collaborators: .server(j["collaborators"]),
                    tasks: j["tasks"].array.map(UsageScope.server), members: j["members"].array.map(UsageScope.server),
                    attempts: j["attempts"].array.map(UsageAttempt.server), time: .server(j["time"]),
                    sessions: j["sessions"].array.map(UsageSession.server),
                    reminders: j["reminders"].array.compactMap(UsageReminder.server))
    }

    func task(_ id: String) -> UsageScope? { tasks.first { $0.id == id } }

    /// The run's tokens, cost and wall-clock time in one line.
    var runLine: String { L.text("\(run.tokensText) · \(run.costText) · 耗时 \(run.durationText)") }

    /// Whether any session reported usage (tokens, a context snapshot or a cost): only then is
    /// there a ledger to open. A session that opened and reported nothing is not one.
    var hasUsage: Bool {
        sessions.contains { $0.turns > 0 || $0.context.reports > 0 || !$0.cost.isEmpty }
    }

    /// Whether the run card offers the ledger's disclosure. A position with reports offers it;
    /// once the ledger is open the control stays, so a replayed position that reported nothing
    /// can still fold it again.
    func showsLedgerControl(open: Bool) -> Bool { hasUsage || open }

    /// The run card's one line of numbers: task progress, tokens as compactly reported, cost,
    /// wall-clock time and the context ratio. Items nothing reported are left out; the time is
    /// always written. `tasks` is the run's task count and `done` the succeeded ones.
    func runCardLine(tasks: Int, done: Int) -> String {
        cardParts(tasks: tasks, done: done, compact: true).joined(separator: " · ")
    }

    /// The same summary with every token and context digit, for VoiceOver and the hover tip.
    func runCardVoice(tasks: Int, done: Int) -> String {
        cardParts(tasks: tasks, done: done, compact: false).joined(separator: L.text("，"))
    }

    private func cardParts(tasks: Int, done: Int, compact: Bool) -> [String] {
        var parts: [String] = []
        if tasks > 0 { parts.append(L.text("子任务 \(done) / \(tasks)")) }
        if let total = run.tokens.total {
            let n = compact ? usageCompact(total) : usageNum(total)
            parts.append(L.text("用量 \(n) tokens（\(run.completeness.label)）"))
        }
        if !run.cost.lines.isEmpty { parts.append(L.text("费用 \(run.costAmountsText ?? run.costDetail)")) }
        parts.append(L.text("耗时 \(run.durationText)"))
        if run.context.reports > 0 {
            if compact {
                parts.append(L.text("上下文 \(run.context.ratio.map(usagePercent) ?? L.text("没有比例"))"))
            } else if let detail = run.contextDetail {
                parts.append(L.text("上下文 \(detail)"))
            }
        }
        return parts
    }

    /// The reminders in one line, prefixed when the numbers are not complete.
    var remindersLine: String? {
        guard !reminders.isEmpty else { return nil }
        return (run.completeness == .complete ? L.text("提醒") : L.text("提醒（按已上报数据）")) + L.text("：") + reminders.map(\.text).joined(separator: L.text("；"))
    }
}

// MARK: Display text

extension UsageScope {
    /// `15,400 tokens（部分上报）`, or `未上报`: a number is only written when it was reported.
    var tokensText: String { tokens.total.map { L.text("\(usageNum($0)) tokens（\(completeness.label)）") } ?? L.text("未上报") }

    /// `输入 1,200 · 缓存读取 5,000 · 输出 300`: what the tokens are made of, parts of 0 left out.
    var breakdown: String? {
        let parts = [(L.text("输入"), tokens.input), (L.text("缓存读取"), tokens.cachedRead), (L.text("缓存写入"), tokens.cachedWrite), (L.text("输出"), tokens.output)]
            .compactMap { label, n in n.flatMap { $0 > 0 ? "\(label) \(usageNum($0))" : nil } }
        guard !parts.isEmpty else { return nil }
        let thought = tokens.thought.flatMap { $0 > 0 ? L.text("（输出中推理 \(usageNum($0))）") : nil } ?? ""
        return parts.joined(separator: " · ") + thought
    }

    /// `费用：未上报`, `费用：0.25 USD`, `费用：0.25 USD + 40 credits（币种或口径不同，未合并）`.
    var costText: String { L.text("费用：\(costDetail)") }

    var costDetail: String {
        let parts = cost.lines.map { l -> String in
            if l.currency.isEmpty { return L.text("\(usageMoney(l.amount))（未标注币种）") }
            return l.sessions > 1 ? L.text("\(usageMoney(l.amount)) \(l.currency)（\(l.sessions) 个会话）") : "\(usageMoney(l.amount)) \(l.currency)"
        }
        if parts.isEmpty { return L.text("未上报") }
        if parts.count == 1, !cost.lines[0].currency.isEmpty { return parts[0] }
        return parts.joined(separator: " + ") + L.text("（币种或口径不同，未合并）")
    }

    /// The reported amounts without the 「N 个会话」 note `costText` adds: the run card's compact
    /// line leaves the session count to the expanded ledger (W1 F2).
    var costAmountsText: String? {
        let parts = cost.lines.map { l in
            l.currency.isEmpty ? L.text("\(usageMoney(l.amount))（未标注币种）") : "\(usageMoney(l.amount)) \(l.currency)"
        }
        guard !parts.isEmpty else { return nil }
        if parts.count == 1 { return parts[0] }
        return parts.joined(separator: " + ") + L.text("（币种或口径不同，未合并）")
    }

    /// Wall-clock time; it never stands in for agent execution time.
    var durationText: String { durationMs.map { L.text("\(L.dur(Double($0)))（总时长）") } ?? L.text("没有时长记录") }

    /// `其中等待你 1 分 20 秒` when the range waited on a person.
    var waitText: String? { waitMs.map { L.text("其中等待你 \(L.dur(Double($0)))") } }

    /// `52,000 / 200,000（26%） · 峰值 61,000`; snapshots are never added to the tokens.
    var contextDetail: String? {
        guard context.reports > 0 else { return nil }
        let used = context.used.map(usageNum) ?? L.text("未上报")
        var line = context.size.map { L.text("\(used) / \(usageNum($0))（\(context.ratio.map(usagePercent) ?? L.text("没有比例"))）") } ?? used
        if let peak = context.peak, let u = context.used, peak > u { line += L.text(" · 峰值 \(usageNum(peak))") }
        return line
    }

    /// A task or member with no session at all is "not started", not "unreported".
    var notStarted: Bool { sessions == 0 }

    /// Token, makeup, cost, wall-clock time and context: the rows of the run card and the task card.
    var summaryRows: [UsageLine] {
        var rows = [UsageLine(label: "Token", value: tokensText)]
        if let breakdown { rows.append(UsageLine(label: L.text("构成"), value: breakdown)) }
        rows.append(UsageLine(label: L.text("费用"), value: costDetail))
        rows.append(UsageLine(label: L.text("耗时"), value: [durationText, waitText].compactMap { $0 }.joined(separator: " · ")))
        if let contextDetail { rows.append(UsageLine(label: L.text("上下文"), value: contextDetail)) }
        return rows
    }
}

extension UsageLedger {
    /// The ledger the run card's summary line shows at `now`. While the run is `ticking` (live,
    /// not a replayed position, still going) and has a start, the run's wall-clock time counts
    /// from `start` to `now`, never below the recorded value, so a clock that moves back does not
    /// make it shrink. Otherwise, and for everything but the run's own time, the recorded values.
    // @project-doc docs/architecture/20261031-frontend.md#usage_ledger
    func displayed(at now: Double, since start: Double, ticking: Bool) -> UsageLedger {
        guard ticking, start > 0 else { return self }
        var shown = self
        shown.run.durationMs = max(run.durationMs ?? 0, Int(max(0, now - start).rounded()))
        return shown
    }
}

/// One line a usage card renders: a label and the value beside it. The views build their rows from
/// these, so a test reads exactly what the card shows.
struct UsageLine: Equatable, Identifiable {
    let label: String
    let value: String
    var id: String { label }
}

extension UsageLedger {
    /// The run card's subtotal row: the lead is part of the run total and shown apart.
    var subtotalLine: UsageLine { UsageLine(label: L.text("小计"), value: L.text("负责人 \(lead.tokensText) · 协作者 \(collaborators.tokensText)")) }

    /// One task in the expanded detail: its tokens and wall-clock time, or 「未开始」.
    func taskDetail(_ scope: UsageScope) -> String {
        guard !scope.notStarted else { return L.text("未开始") }
        return ([scope.tokensText] + [scope.durationMs.map { L.dur(Double($0)) }].compactMap { $0 }).joined(separator: " · ")
    }

    /// One member in the expanded detail: its tokens, or 「没有会话」, and how many sessions it used.
    func memberDetail(_ scope: UsageScope) -> String { L.text("\(scope.notStarted ? L.text("没有会话") : scope.tokensText) · \(scope.sessions) 个会话") }
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
