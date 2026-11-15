import Foundation

// The run's read-only usage ledger, derived from the event log like the rest of the run page
// (`derive` stores it in `RunState.usage`, so a replayed prefix gets its own numbers). The rules are
// the backend's (`backend/src/usage.rs`, docs/architecture/frontend.md#usage_ledger): every
// `session/prompt` report is one turn and turns add up, the quota rows by model replace the
// main-loop usage, context snapshots are never tokens, cost is a running total per session, and
// missing numbers are words, never 0. `UsageLedgerTests` reads `backend/testdata/usage_cases.json`,
// the cases the backend is tested on too.

/// Whether a range's token numbers are all there.
enum UsageCompleteness: String {
    case missing, complete, partial

    var label: String {
        switch self {
        case .missing: return "未上报"
        case .complete: return "完整"
        case .partial: return "部分上报"
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

    /// One count block; without `totalTokens` but with input or output, the sum of its parts.
    static func read(_ j: JSON) -> UsageTokens {
        var t = UsageTokens(total: j["totalTokens"].int, input: j["inputTokens"].int, output: j["outputTokens"].int,
                            cachedRead: j["cachedReadTokens"].int, cachedWrite: j["cachedWriteTokens"].int, thought: j["thoughtTokens"].int)
        if t.total == nil, t.input != nil || t.output != nil {
            t.total = [t.input, t.output, t.cachedRead, t.cachedWrite].compactMap { $0 }.reduce(0, +)
        }
        return t
    }

    /// One turn by model: the quota rows when the agent sent them (they name their model), else
    /// `usage` on the session's current model (nil as model); empty for a context snapshot.
    static func ofTurn(_ d: JSON) -> [(model: String?, tokens: UsageTokens)] {
        let rows = d["models"].array.map { (model: $0["model"].string.flatMap { $0.isEmpty ? nil : $0 }, tokens: read($0)) }.filter { $0.tokens.total != nil }
        if !rows.isEmpty { return rows }
        let usage = read(d)
        return usage.total == nil ? [] : [(nil, usage)]
    }

    /// Cache reads over input plus cache reads and writes; nil unless input and cache reads were reported.
    var cacheHitRate: Double? {
        guard let input, let cachedRead else { return nil }
        let prompt = input + cachedRead + (cachedWrite ?? 0)
        return prompt > 0 ? Double(cachedRead) / Double(prompt) : nil
    }

    /// Add another range's counts; a count neither side reported stays unreported.
    mutating func add(_ o: UsageTokens) {
        total = total.plus(o.total)
        input = input.plus(o.input)
        output = output.plus(o.output)
        cachedRead = cachedRead.plus(o.cachedRead)
        cachedWrite = cachedWrite.plus(o.cachedWrite)
        thought = thought.plus(o.thought)
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

    /// One snapshot. A `used` of 0 or less says nothing yet, as in the session card.
    mutating func snapshot(used u: Int?, size s: Int?) {
        reports += 1
        if let u, u > 0 {
            used = u
            peak = max(peak ?? u, u)
        }
        if let s, s > 0 { size = s }
    }

    /// Merge a newer range: the peak is the larger one, the latest value the newer one.
    mutating func merge(_ newer: UsageContext) {
        reports += newer.reports
        peak = [peak, newer.peak].compactMap { $0 }.max()
        used = newer.used ?? used
        size = newer.size ?? size
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

    /// One session's lines. Amounts without a currency may be in different units and stay apart;
    /// named currencies come first, in order.
    mutating func add(_ session: [UsageCostLine]) {
        guard !session.isEmpty else { missingSessions += 1; return }
        reportedSessions += 1
        for l in session {
            if !l.currency.isEmpty, let i = lines.firstIndex(where: { $0.currency == l.currency }) {
                lines[i].amount += l.amount
                lines[i].sessions += l.sessions
            } else if l.currency.isEmpty {
                lines.append(l)
            } else {
                lines.insert(l, at: lines.firstIndex { $0.currency.isEmpty || $0.currency > l.currency } ?? lines.endIndex)
            }
        }
    }
}

/// One session's running cost in one currency: what earlier segments reached before a reset, and
/// the latest reading.
private struct RunningCost {
    var beforeReset = 0.0
    var latest = 0.0

    mutating func reading(_ amount: Double) {
        if amount <= 0, latest > 0 { return } // a crashed result reports zero
        if amount < latest { beforeReset += latest } // the running total restarted (`/clear`)
        latest = amount
    }

    var total: Double { beforeReset + latest }
}

/// What one turn spent on one model; a turn reported by model has a row per model.
struct UsageSpend: Equatable {
    let ts: Int
    let model: String
    let tokens: UsageTokens
    /// The first row of its turn, so a bill counts turns once.
    let first: Bool
}

/// The model name of a turn whose agent named no model and whose session reported none.
let unknownModel = "未知模型"

/// Tokens and turns on one model, in a bill.
struct UsageModelLine: Equatable, Identifiable {
    var model: String
    var tokens = UsageTokens()
    var turns = 0
    var id: String { model }
}

/// Who paid in a bill: a member, in a task's bill, or an Agent, in a period's bill (from the backend).
struct UsageBillGroup: Equatable, Identifiable {
    var id: String
    var name: String
    var tokens = UsageTokens()
    var turns = 0
    var cost = UsageCost()
    var models: [UsageModelLine] = []

    mutating func spend(_ s: UsageSpend) {
        tokens.add(s.tokens)
        if s.first { turns += 1 }
        if let i = models.firstIndex(where: { $0.model == s.model }) {
            models[i].tokens.add(s.tokens)
            models[i].turns += 1
        } else {
            models.append(UsageModelLine(model: s.model, tokens: s.tokens, turns: 1))
        }
    }

    /// Most tokens first, for the groups of a bill and the models of each.
    static func sorted(_ groups: [UsageBillGroup]) -> [UsageBillGroup] {
        groups.map { g in
            var g = g
            g.models.sort { ($0.tokens.total ?? -1, $1.model) > ($1.tokens.total ?? -1, $0.model) }
            return g
        }
        .sorted { ($0.tokens.total ?? -1, $1.name) > ($1.tokens.total ?? -1, $0.name) }
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
    /// Each turn's tokens by model, for the task's bill.
    var spend: [UsageSpend] = []
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

    /// The range over `sessions`, oldest first, so the latest context snapshot wins.
    static func over(_ id: String, _ sessions: [UsageSession], durationMs: Int? = nil, waitMs: Int? = nil) -> UsageScope {
        var s = UsageScope(id: id, durationMs: durationMs, waitMs: waitMs)
        for session in sessions {
            s.sessions += 1
            if session.turns > 0 { s.reportedSessions += 1 } else { s.missingSessions += 1 }
            s.tokens.add(session.tokens)
            s.turns += session.turns
            s.context.merge(session.context)
            s.cost.add(session.cost)
        }
        s.completeness = .of(reported: s.reportedSessions, missing: s.missingSessions)
        return s
    }
}

/// One attempt of a task, with the usage it recorded and how it ended.
struct UsageAttempt: Identifiable, Equatable {
    enum State: String {
        case unfinished, succeeded, failed, verificationFailed, expired, cancelled

        var label: String {
            switch self {
            case .unfinished: return "未完成"
            case .succeeded: return "成功"
            case .failed: return "失败"
            case .verificationFailed: return "验证没通过"
            case .expired: return "已过期但仍计入"
            case .cancelled: return "已取消"
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
    var usage = UsageScope()
    var id: String { "\(task)#\(n)" }
}

/// The run's ledger: every range over the same events, plus reminders that are only text.
struct UsageLedger: Equatable {
    var run = UsageScope(id: "run")
    var lead = UsageScope(id: "lead")
    var collaborators = UsageScope(id: "collaborators")
    var tasks: [UsageScope] = []
    var members: [UsageScope] = []
    var attempts: [UsageAttempt] = []
    var sessions: [UsageSession] = []
    var reminders: [String] = []

    static let empty = UsageLedger()

    func task(_ id: String) -> UsageScope? { tasks.first { $0.id == id } }
    func attempts(of task: String) -> [UsageAttempt] { attempts.filter { $0.task == task } }

    /// `用量：… · 费用：… · 耗时 …`, the line `team` reads.
    var summaryLine: String { "用量：\(runLine)" }
    /// `用量 … · 费用：… · 耗时 …`, the delivery's line.
    var deliveryLine: String { "用量 \(runLine)" }
    private var runLine: String { "\(run.tokensText) · \(run.costText) · 耗时 \(run.durationText)" }

    /// The reminders in one line, prefixed when the numbers are not complete.
    var remindersLine: String? {
        guard !reminders.isEmpty else { return nil }
        return (run.completeness == .complete ? "提醒" : "提醒（按已上报数据）") + "：" + reminders.joined(separator: "；")
    }
}

// MARK: Display text

extension UsageScope {
    /// `15,400 tokens（部分上报）`, or `未上报`: a number is only written when it was reported.
    var tokensText: String { tokens.total.map { "\(usageNum($0)) tokens（\(completeness.label)）" } ?? "未上报" }

    /// `输入 1,200 · 缓存读取 5,000 · 输出 300`: what the tokens are made of, parts of 0 left out.
    var breakdown: String? {
        let parts = [("输入", tokens.input), ("缓存读取", tokens.cachedRead), ("缓存写入", tokens.cachedWrite), ("输出", tokens.output)]
            .compactMap { label, n in n.flatMap { $0 > 0 ? "\(label) \(usageNum($0))" : nil } }
        guard !parts.isEmpty else { return nil }
        let thought = tokens.thought.flatMap { $0 > 0 ? "（输出中推理 \(usageNum($0))）" : nil } ?? ""
        return parts.joined(separator: " · ") + thought
    }

    /// `费用：未上报`, `费用：0.25 USD`, `费用：0.25 USD + 40 credits（币种或口径不同，未合并）`.
    var costText: String {
        let parts = cost.lines.map { l -> String in
            if l.currency.isEmpty { return "\(usageMoney(l.amount))（未标注币种）" }
            return l.sessions > 1 ? "\(usageMoney(l.amount)) \(l.currency)（\(l.sessions) 个会话）" : "\(usageMoney(l.amount)) \(l.currency)"
        }
        if parts.isEmpty { return "费用：未上报" }
        if parts.count == 1, !cost.lines[0].currency.isEmpty { return "费用：\(parts[0])" }
        return "费用：\(parts.joined(separator: " + "))（币种或口径不同，未合并）"
    }

    /// The cost without its `费用：` label, for a row that already names it.
    var costDetail: String { String(costText.dropFirst("费用：".count)) }

    /// Wall-clock time; it never stands in for agent execution time.
    var durationText: String { durationMs.map { "\(L.dur(Double($0)))（墙钟）" } ?? "没有时长记录" }

    /// `其中等待你 1 分 20 秒` when the range waited on a person.
    var waitText: String? { waitMs.map { "其中等待你 \(L.dur(Double($0)))" } }

    /// `52,000 / 200,000（26%） · 峰值 61,000`; snapshots are never added to the tokens.
    var contextDetail: String? {
        guard context.reports > 0 else { return nil }
        let used = context.used.map(usageNum) ?? "未上报"
        var line = context.size.map { "\(used) / \(usageNum($0))（\(context.ratio.map(usagePercent) ?? "没有比例")）" } ?? used
        if let peak = context.peak, let u = context.used, peak > u { line += " · 峰值 \(usageNum(peak))" }
        return line
    }

    /// A task or member with no session at all is "not started", not "unreported".
    var notStarted: Bool { sessions == 0 }

    /// Token, makeup, cost, wall-clock time and context: the rows of the run card and the task card.
    var summaryRows: [UsageLine] {
        var rows = [UsageLine(label: "Token", value: tokensText)]
        if let breakdown { rows.append(UsageLine(label: "构成", value: breakdown)) }
        rows.append(UsageLine(label: "费用", value: costDetail))
        rows.append(UsageLine(label: "耗时", value: [durationText, waitText].compactMap { $0 }.joined(separator: " · ")))
        if let contextDetail { rows.append(UsageLine(label: "上下文", value: contextDetail)) }
        return rows
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
    var subtotalLine: UsageLine { UsageLine(label: "小计", value: "主控 \(lead.tokensText) · 协作者 \(collaborators.tokensText)") }

    /// One task in the expanded detail: its tokens and wall-clock time, or 「未开始」.
    func taskDetail(_ scope: UsageScope) -> String {
        guard !scope.notStarted else { return "未开始" }
        return ([scope.tokensText] + [scope.durationMs.map { L.dur(Double($0)) }].compactMap { $0 }).joined(separator: " · ")
    }

    /// One member in the expanded detail: its tokens, or 「没有会话」, and how many sessions it used.
    func memberDetail(_ scope: UsageScope) -> String { "\(scope.notStarted ? "没有会话" : scope.tokensText) · \(scope.sessions) 个会话" }

    /// A task's bill: each member that worked on it, with its tokens by model and its reported cost,
    /// most tokens first (`task_bill` in the backend). A member that reported nothing has no models.
    func bill(ofTask task: String, name: (String) -> String) -> [UsageBillGroup] {
        var groups: [UsageBillGroup] = []
        for s in sessions where s.task == task {
            let i = groups.firstIndex { $0.id == s.actor } ?? {
                groups.append(UsageBillGroup(id: s.actor, name: name(s.actor)))
                return groups.count - 1
            }()
            for x in s.spend { groups[i].spend(x) }
            groups[i].cost.add(s.cost)
        }
        return UsageBillGroup.sorted(groups)
    }
}

extension UsageAttempt {
    /// One attempt as the task card writes it: number, outcome, executor, tokens, time.
    func line(name: String) -> String {
        "#\(n) · \(state.label) · \(name) · \(usage.tokensText) · \(durationMs.map { L.dur(Double($0)) } ?? "没有时长记录")"
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

private extension Int? {
    /// Two optional counts added: no data on both sides stays no data.
    func plus(_ other: Int?) -> Int? {
        switch (self, other) {
        case let (a?, b?): return a + b
        case let (a, b): return a ?? b
        }
    }
}

// MARK: Derivation

/// Who owns the session an event speaks about: `attempt.started` is written by Janus for the agent
/// in `to`; every other event's actor owns its session (`session_actor` in the backend).
private func usageOwner(_ e: Event) -> String {
    guard e.type == "attempt.started", let to = e.to, !to.isEmpty else { return e.actor }
    return to
}

/// The ledger of one slice of the event log. Pure: the same events always give the same numbers.
// @project-doc docs/architecture/frontend.md#usage_ledger
func usageLedger(_ events: [Event]) -> UsageLedger {
    struct SessionBuild {
        var actor: String, task: String?, attempt: Int?
        var tokens = UsageTokens(), turns = 0, context = UsageContext()
        var cost: [String: RunningCost] = [:]
        var spend: [UsageSpend] = []
        var firstMs: Int, lastMs: Int
    }
    struct AttemptBuild {
        var task: String, n: Int, actor: String
        var state = UsageAttempt.State.unfinished, note: String?
        var startedMs: Int, endedMs: Int?, lastMs: Int
    }
    var created: Int?
    var last: Int?
    var current: [String: String] = [:]
    var model: [String: String] = [:]
    var builds: [String: SessionBuild] = [:]
    var order: [String] = []
    var attempts: [String: AttemptBuild] = [:]
    var attemptOrder: [String] = []
    var tasks: [String] = []
    var spans: [String: (Int, Int)] = [:]
    var waits: [(ms: Int, task: String?, actor: String)] = []
    var open: [String: (start: Int, task: String?, actor: String)] = [:]
    var declared: Set<String> = []
    var runCancelled: Int?
    var taskCancelled: [String: Int] = [:]

    func noteTask(_ id: String) { if !tasks.contains(id) { tasks.append(id) } }

    /// The actor is in its current session, even if it never reports usage; returns the key.
    @discardableResult func touch(_ e: Event, _ owner: String) -> String {
        let ts = Int(e.ts)
        let key = current[owner].map { "\(owner):\($0)" } ?? "\(owner):\(e.task ?? "-")/\(e.attempt.map(String.init) ?? "-")"
        if var s = builds[key] {
            s.firstMs = min(s.firstMs, ts)
            s.lastMs = max(s.lastMs, ts)
            builds[key] = s
        } else {
            order.append(key)
            builds[key] = SessionBuild(actor: owner, task: e.task, attempt: e.attempt, firstMs: ts, lastMs: ts)
        }
        if owner != "lead" { declared.insert(owner) }
        return key
    }

    func attempt(_ task: String, _ n: Int, _ actor: String, _ ts: Int) {
        let key = "\(task)#\(n)"
        var a = attempts[key] ?? {
            attemptOrder.append(key)
            return AttemptBuild(task: task, n: n, actor: actor, startedMs: ts, endedMs: nil, lastMs: ts)
        }()
        a.lastMs = max(a.lastMs, ts)
        attempts[key] = a
        if actor != "lead" { declared.insert(actor) }
    }

    func ended(_ e: Event, _ state: UsageAttempt.State, _ note: String?) {
        guard let task = e.task, let n = e.attempt else { return }
        let ts = Int(e.ts)
        attempt(task, n, e.actor, ts)
        let key = "\(task)#\(n)"
        guard var a = attempts[key] else { return }
        a.state = state
        if let note { a.note = note }
        a.endedMs = max(a.endedMs ?? ts, ts)
        attempts[key] = a
    }

    for e in events {
        let ts = Int(e.ts)
        last = max(last ?? ts, ts)
        if let task = e.task {
            noteTask(task)
            let span = spans[task] ?? (ts, ts)
            spans[task] = (min(span.0, ts), max(span.1, ts))
        }
        let d = e.data
        switch e.type {
        case "run.created":
            created = ts
        case "attempt.started", "agent.status":
            let owner = usageOwner(e)
            if let sid = d["session"].string, !sid.isEmpty {
                current[owner] = sid
            } else if e.type == "agent.status" {
                break
            }
            // An opened session can be one whose usage never arrives: it counts as unreported.
            touch(e, owner)
            if e.type == "attempt.started", let task = e.task, let n = e.attempt { attempt(task, n, owner, ts) }
        case "session.usage":
            let rows = UsageTokens.ofTurn(d)
            let used = d["used"].int, size = d["size"].int
            let cost = d["cost"]["amount"].double.map { (currency: d["cost"]["currency"].str, amount: $0) }
            if rows.isEmpty, used == nil, size == nil, cost == nil { break }
            let owner = usageOwner(e)
            let key = touch(e, owner)
            guard var s = builds[key] else { break }
            if !rows.isEmpty { s.turns += 1 }
            for (i, row) in rows.enumerated() {
                s.tokens.add(row.tokens)
                s.spend.append(UsageSpend(ts: ts, model: row.model ?? model[owner] ?? unknownModel, tokens: row.tokens, first: i == 0))
            }
            if used != nil || size != nil { s.context.snapshot(used: used, size: size) }
            if let cost { s.cost[cost.currency, default: RunningCost()].reading(cost.amount) }
            builds[key] = s
        // The model a session runs on: as it opened, and when someone switched it.
        case "session.controls":
            if let m = d["model"]["current"].string, !m.isEmpty { model[e.actor] = m }
        case "session.control" where d["id"].str == "model" && d["failed"].bool != true:
            if let m = d["value"].string, !m.isEmpty { model[e.actor] = m }
        case "attempt.failed":
            ended(e, .failed, d["error"].string)
        case "attempt.late_result":
            ended(e, .expired, "过期尝试的结果被忽略，用量照常计入")
        case "verification.failed":
            let failures = d["failures"].strings
            ended(e, .verificationFailed, failures.isEmpty ? nil : failures.joined(separator: "；"))
        case "task.succeeded":
            ended(e, .succeeded, nil)
        case "task.failed":
            ended(e, .failed, ["verification": "验收没通过", "attempts": "用完尝试次数"][d["reason"].str])
        case "task.skipped_by_decision":
            // The lead cancelled one task; Janus writes no attempt event for it (see below).
            if let task = e.task { taskCancelled[task] = min(taskCancelled[task] ?? ts, ts) }
        case "run.cancelled":
            runCancelled = min(runCancelled ?? ts, ts)
        case "orchestration.accepted":
            for x in d["tasks"].array {
                if let id = x["id"].string { noteTask(id) }
                if let agent = x["agent"].string { declared.insert(agent) }
            }
        // Requests that hold a session until a person or the lead answers.
        case "question.asked", "approval.requested", "input.requested":
            if let key = d["key"].string, !key.isEmpty { open[key] = (ts, e.task, e.actor) }
        case "question.answered", "approval.granted", "approval.denied", "input.answered", "decision.withdrawn":
            if let key = d["key"].string, let w = open.removeValue(forKey: key) {
                waits.append((ms: max(0, ts - w.start), task: w.task, actor: w.actor))
            }
        default:
            break
        }
    }

    let end = last ?? 0
    // A request nobody answered yet waits until the last event.
    for w in open.values { waits.append((ms: max(0, end - w.start), task: w.task, actor: w.actor)) }
    func wait(_ match: (String?, String) -> Bool) -> Int? {
        let matched = waits.filter { match($0.task, $0.actor) }.map(\.ms)
        return matched.isEmpty ? nil : matched.reduce(0, +)
    }

    // Oldest first, so a range's latest context snapshot is the one that shows.
    let sessions: [UsageSession] = order.enumerated().compactMap { i, key in
        builds[key].map { b in
            (i, UsageSession(key: key, actor: b.actor, task: b.task, attempt: b.attempt, tokens: b.tokens, turns: b.turns, context: b.context,
                             cost: b.cost.keys.sorted().map { UsageCostLine(currency: $0, amount: b.cost[$0]?.total ?? 0) },
                             firstMs: b.firstMs, lastMs: b.lastMs, spend: b.spend))
        }
    }
    .sorted { ($0.1.lastMs, $0.0) < ($1.1.lastMs, $1.0) }
    .map(\.1)

    var ledger = UsageLedger()
    ledger.sessions = sessions
    ledger.run = .over("run", sessions, durationMs: created.map { max(0, end - $0) }, waitMs: wait { _, _ in true })
    ledger.lead = .over("lead", sessions.filter { $0.actor == "lead" }, waitMs: wait { _, a in a == "lead" })
    ledger.collaborators = .over("collaborators", sessions.filter { $0.actor != "lead" }, waitMs: wait { _, a in a != "lead" })
    ledger.tasks = tasks.map { task in
        .over(task, sessions.filter { $0.task == task }, durationMs: spans[task].map { max(0, $0.1 - $0.0) }, waitMs: wait { t, _ in t == task })
    }
    ledger.members = Set(sessions.map(\.actor)).sorted().map { actor in
        .over(actor, sessions.filter { $0.actor == actor }, waitMs: wait { _, a in a == actor })
    }
    ledger.attempts = attemptOrder.compactMap { key in
        guard var b = attempts[key] else { return nil }
        // Janus kills the sessions of a cancelled run (or of a task the lead cancels) just before it
        // records the cancel, so a failure event can still follow it. An attempt still running when
        // its cancel arrived was cancelled; what it reported stays counted.
        let cancel: (at: Int, note: String)? = switch (runCancelled, taskCancelled[b.task]) {
        case let (run?, task?) where task <= run: (task, "主控取消了这个任务")
        case let (run?, _): (run, "Run 被取消")
        case let (nil, task?): (task, "主控取消了这个任务")
        case (nil, nil): nil
        }
        if let cancel, b.state != .succeeded, !(b.endedMs.map { $0 < cancel.at } ?? false) {
            b.state = .cancelled
            b.note = cancel.note
            b.endedMs = max(b.endedMs ?? cancel.at, cancel.at)
        }
        let usage = UsageScope.over(key, sessions.filter { $0.task == b.task && $0.attempt == b.n })
        let duration = b.endedMs.map { max(0, $0 - b.startedMs) } ?? (b.lastMs > b.startedMs ? b.lastMs - b.startedMs : nil)
        return UsageAttempt(task: b.task, n: b.n, actor: b.actor, state: b.state, note: b.note, startedMs: b.startedMs,
                            endedMs: b.endedMs, durationMs: duration, usage: usage)
    }
    ledger.reminders = usageReminders(ledger, declared: declared.count)
    return ledger
}

/// The read-only reminders, with the backend's default thresholds. Text only: nothing here changes
/// the run, its budget, its attempts or the inbox.
func usageReminders(_ ledger: UsageLedger, declared: Int) -> [String] {
    var out: [String] = []
    let total = ledger.run.tokens.total
    if let n = total, n >= 500_000 {
        out.append("Token 用量偏高：已上报 \(usageNum(n)) tokens，达到默认提醒阈值 500,000")
    }
    if let n = total, n >= 50_000, let (task, used) = ledger.tasks.compactMap({ t in t.tokens.total.map { (t, $0) } }).max(by: { $0.1 < $1.1 }),
       Double(used) >= Double(n) * 0.6 {
        out.append("大部分用量集中在一个任务：\(task.id) 占 \(usageNum(n)) 的 \(usagePercent(Double(used) / Double(n)))")
    }
    let collaborators = max(declared, ledger.members.filter { $0.id != "lead" }.count)
    if collaborators >= 5, !ledger.attempts.contains(where: { $0.state == .succeeded }), (ledger.run.durationMs ?? 0) >= 30 * 60 * 1000 {
        out.append("协作者多但没有有效产出：\(collaborators) 个协作者，30 分钟内没有任何任务成功")
    }
    if let n = total, n >= 100_000, !ledger.run.cost.reported {
        out.append("费用无法判断：有 token 用量，但没有任何会话上报费用")
    }
    return out
}
