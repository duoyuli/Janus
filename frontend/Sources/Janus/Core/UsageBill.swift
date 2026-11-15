import Foundation

// A usage bill as the receipt shows it (`UsageReceipt`): a task's bill, built from the run's ledger,
// or a period's bill across runs, read from `GET /api/usage` (docs/architecture/frontend.md#usage_bill).
// Both list who paid (members of the task, or Agents) with their models. The cost the agents
// reported stays apart from the estimate at models.dev list prices, which is never an invoice.

/// A model's list price from models.dev, USD per million tokens (`backend/src/prices.rs`).
struct UsagePrice: Equatable {
    let input: Double
    let output: Double
    let cacheRead: Double?
    let cacheWrite: Double?

    init?(_ j: JSON) {
        guard let input = j["input"].double, let output = j["output"].double else { return nil }
        self.input = input
        self.output = output
        cacheRead = j["cacheRead"].double
        cacheWrite = j["cacheWrite"].double
    }

    /// What `t` costs at this price; nil without input or output (a total alone cannot be priced).
    /// A cache price the list leaves out is the input price, as in the backend.
    func cost(_ t: UsageTokens) -> Double? {
        guard t.input != nil || t.output != nil else { return nil }
        func at(_ n: Int?, _ usd: Double) -> Double { Double(n ?? 0) * usd / 1_000_000 }
        return at(t.input, input) + at(t.output, output) + at(t.cachedRead, cacheRead ?? input) + at(t.cachedWrite, cacheWrite ?? input)
    }
}

/// What a bill's tokens would cost at list prices, and what could not be priced.
struct UsageEstimate: Equatable {
    var amount = 0.0
    var unpricedTurns = 0
    var unpricedModels: [String] = []
    var fetchedAt: Date?
}

/// A titled list of rows under the bill's totals, such as a task's attempts.
struct UsageBillSection: Equatable, Identifiable {
    let title: String
    let rows: [UsageLine]
    var id: String { title }
}

struct UsageBill: Equatable {
    var title: String
    /// What the bill covers and when it was made.
    var meta: [UsageLine] = []
    var tokens = UsageTokens()
    var cost = UsageCost()
    var stats: [UsageLine] = []
    /// The heading over the payers: 「成员 / 模型」 or 「AGENT / 模型」.
    var groupsTitle: String
    var groups: [UsageBillGroup] = []
    var sections: [UsageBillSection] = []
    var notes: [String] = []
    /// At models.dev list prices; nil before prices are known.
    var estimate: UsageEstimate?

    /// Whether the headline is money: some of the tokens have a price.
    var priced: Bool { (estimate?.amount ?? 0) > 0 }

    /// `$77.04` when priced, else the tokens: `15.4K`, `21.4M`, `6.45B`.
    var headline: String { priced ? usageUSD(estimate?.amount ?? 0) : tokens.total.map(usageCompact) ?? "未上报" }

    /// What the headline is: an estimate at list prices, or the token total.
    var headlineCaption: String { priced ? "按 models.dev API 价格折算 · 非账单" : "TOKEN 总量" }

    /// `3,111,893 tokens · Agent 上报费用 0.25 USD`, under the headline.
    var headlineDetail: String {
        let cost = UsageScope(cost: cost).costDetail
        return [tokens.total.map { "\(usageNum($0)) tokens" }, cost == "未上报" ? "Agent 未上报费用" : "Agent 上报费用 \(cost)"].compactMap { $0 }.joined(separator: " · ")
    }

    /// The bill with its models priced: each model line and group gets its estimate, and the notes
    /// say what could not be priced (`prices::Table::price` in the backend).
    func priced(with prices: [String: UsagePrice], fetchedAt: Date?) -> UsageBill {
        var bill = self
        var e = UsageEstimate(fetchedAt: fetchedAt)
        for gi in bill.groups.indices {
            for mi in bill.groups[gi].models.indices {
                let m = bill.groups[gi].models[mi]
                if let usd = prices[m.model]?.cost(m.tokens) {
                    bill.groups[gi].models[mi].estimate = usd
                    bill.groups[gi].estimate = (bill.groups[gi].estimate ?? 0) + usd
                    e.amount += usd
                } else {
                    e.unpricedTurns += m.turns
                    if !e.unpricedModels.contains(m.model) { e.unpricedModels.append(m.model) }
                }
            }
        }
        bill.estimate = e
        bill.notes = bill.priceNotes + bill.notes
        return bill
    }

    /// What the estimate rests on, first among the notes.
    var priceNotes: [String] {
        guard let e = estimate else { return [] }
        var out = ["估算按 models.dev 的 API 价格（不含长上下文加价），不是账单" + (e.fetchedAt.map { "；价格更新于 \(usageClock($0))" } ?? "") + "。"]
        if e.unpricedTurns > 0 { out.append("\(e.unpricedTurns) 轮没有价格（\(e.unpricedModels.joined(separator: "、"))），没有计入估算。") }
        return out
    }

    /// The bill as plain text, for copying.
    var text: String {
        var out = [title] + meta.map { "\($0.label)：\($0.value)" } + ["", "Token 总量：\(headline)（\(headlineDetail)）", ""]
        out += stats.map { "\($0.label)：\($0.value)" }
        if !groups.isEmpty {
            out += ["", groupsTitle]
            for g in groups {
                out.append("\(g.name)：\(g.tokens.total.map(usageNum) ?? "未上报")" + (g.cost.reported ? "（\(UsageScope(cost: g.cost).costDetail)）" : ""))
                out += g.models.map { "  · \($0.model)：\($0.tokens.total.map(usageNum) ?? "未上报")" }
            }
        }
        out += [""] + totalRows.map { "\($0.label)：\($0.value)" }
        for section in sections { out += ["", section.title] + section.rows.map { "\($0.label)：\($0.value)" } }
        return (out + (notes.isEmpty ? [] : [""] + notes)).joined(separator: "\n")
    }

    /// Token total, its parts and the cache hit rate, at the foot of the bill.
    var totalRows: [UsageLine] {
        var rows = [UsageLine(label: "Token 总量", value: tokens.total.map(usageNum) ?? "未上报")]
        for (label, n) in [("输入", tokens.input), ("缓存读取", tokens.cachedRead), ("缓存写入", tokens.cachedWrite), ("输出", tokens.output)] {
            if let n, n > 0 { rows.append(UsageLine(label: label, value: usageNum(n))) }
        }
        if let rate = tokens.cacheHitRate { rows.append(UsageLine(label: "缓存命中率", value: usagePercent(rate))) }
        return rows
    }
}

extension UsageBill {
    /// A task's bill from the run's ledger: the task's totals, each member by model, each attempt.
    static func task(_ t: TaskState, ledger: UsageLedger, name: (String) -> String) -> UsageBill? {
        guard let scope = ledger.task(t.id), !scope.notStarted else { return nil }
        let attempts = ledger.attempts(of: t.id)
        var stats = [UsageLine(label: "数据", value: scope.completeness.label),
                     UsageLine(label: "尝试", value: "\(attempts.count) 次"),
                     UsageLine(label: "请求", value: "\(scope.turns) 轮"),
                     UsageLine(label: "会话", value: "\(scope.sessions) 个"),
                     UsageLine(label: "耗时", value: [scope.durationText, scope.waitText].compactMap { $0 }.joined(separator: " · "))]
        if let context = scope.contextDetail { stats.append(UsageLine(label: "上下文", value: context)) }
        var notes: [String] = []
        if scope.completeness == .partial { notes.insert("\(scope.missingSessions) 个会话没有上报 token，数字是下界。", at: 0) }
        return UsageBill(
            title: "任务用量账单",
            meta: [UsageLine(label: "任务", value: "\(t.id)「\(t.title)」")],
            tokens: scope.tokens, cost: scope.cost, stats: stats,
            groupsTitle: "成员 / 模型", groups: ledger.bill(ofTask: t.id, name: name),
            sections: attempts.isEmpty ? [] : [UsageBillSection(title: "每次尝试", rows: attempts.map { a in
                UsageLine(label: "#\(a.n) \(a.state.label) · \(name(a.actor))",
                          value: [a.usage.tokens.total.map { "\(usageNum($0)) tokens" } ?? "未上报", a.durationMs.map { L.dur(Double($0)) }].compactMap { $0 }.joined(separator: " · ")
                              + (a.note.map { "（\($0)）" } ?? ""))
            })],
            notes: notes)
    }

    /// A period's bill as `GET /api/usage` returns it.
    static func period(_ j: JSON, issued: Date) -> UsageBill {
        let day = DateFormatter()
        day.locale = Locale(identifier: "zh_CN")
        day.dateFormat = "yyyy年M月d日"
        let from = Date(timeIntervalSince1970: (j["from"].double ?? 0) / 1000)
        let last = Date(timeIntervalSince1970: ((j["to"].double ?? 0) - 1) / 1000)
        let tokens = UsageTokens.bill(j["tokens"])
        let active = j["activeDays"].int ?? 0
        var stats = [UsageLine(label: "Run", value: "\(j["runs"].int ?? 0) 个"),
                     UsageLine(label: "请求", value: "\(usageNum(j["turns"].int ?? 0)) 轮"),
                     UsageLine(label: "会话", value: "\(j["sessions"].int ?? 0) 个"),
                     UsageLine(label: "活跃天数", value: "\(active) / \(j["days"].int ?? 0)")]
        if let total = tokens.total, active > 0 { stats.append(UsageLine(label: "活跃日均", value: "\(usageCompact(total / active)) tokens")) }
        var notes = ["Token 总量含缓存读取。"]
        if let n = j["unreportedSessions"].int, n > 0 { notes.insert("\(n) 个会话没有上报 token，没有计入。", at: 0) }
        let e = j["estimate"]
        var bill = UsageBill(
            title: "用量账单",
            meta: [UsageLine(label: "区间", value: "\(day.string(from: from)) – \(day.string(from: last))"),
                   UsageLine(label: "开具", value: usageClock(issued))],
            tokens: tokens, cost: UsageCost.bill(j["cost"]), stats: stats,
            groupsTitle: "AGENT / 模型",
            groups: j["agents"].array.map { g in
                UsageBillGroup(id: g["id"].str, name: g["name"].str, tokens: .bill(g["tokens"]), turns: g["turns"].int ?? 0, cost: .bill(g["cost"]),
                               models: g["models"].array.map { UsageModelLine(model: $0["model"].str, tokens: .bill($0["tokens"]), turns: $0["turns"].int ?? 0, estimate: $0["estimate"].double) },
                               estimate: g["estimate"].double)
            },
            notes: notes,
            estimate: e.isNull ? nil : UsageEstimate(amount: e["amount"].double ?? 0, unpricedTurns: e["unpricedTurns"].int ?? 0, unpricedModels: e["unpricedModels"].strings,
                                                     fetchedAt: e["fetchedAt"].double.map { Date(timeIntervalSince1970: $0 / 1000) }))
        bill.notes = bill.priceNotes + bill.notes
        return bill
    }
}

extension UsageTokens {
    /// Token counts as the backend's bill writes them.
    static func bill(_ j: JSON) -> UsageTokens {
        UsageTokens(total: j["total"].int, input: j["input"].int, output: j["output"].int,
                    cachedRead: j["cachedRead"].int, cachedWrite: j["cachedWrite"].int, thought: j["thought"].int)
    }
}

extension UsageCost {
    /// A cost as the backend's bill writes it.
    static func bill(_ j: JSON) -> UsageCost {
        UsageCost(lines: j["lines"].array.map { UsageCostLine(currency: $0["currency"].str, amount: $0["amount"].double ?? 0, sessions: $0["sessions"].int ?? 1) },
                  reportedSessions: j["reportedSessions"].int ?? 0, missingSessions: j["missingSessions"].int ?? 0)
    }
}

/// A period the bill can cover, in the person's calendar.
enum UsagePeriod: String, CaseIterable, Identifiable {
    case week, days30, thisMonth, lastMonth
    var id: String { rawValue }

    var title: String {
        switch self {
        case .week: return "近 7 天"
        case .days30: return "近 30 天"
        case .thisMonth: return "本月"
        case .lastMonth: return "上月"
        }
    }

    /// `[from, to)`: whole days, the last of them today (or the last day of last month).
    func range(now: Date, calendar: Calendar = .current) -> (from: Date, to: Date) {
        let tomorrow = calendar.date(byAdding: .day, value: 1, to: calendar.startOfDay(for: now)) ?? now
        let month = calendar.dateInterval(of: .month, for: now)?.start ?? calendar.startOfDay(for: now)
        switch self {
        case .week: return (calendar.date(byAdding: .day, value: -7, to: tomorrow) ?? tomorrow, tomorrow)
        case .days30: return (calendar.date(byAdding: .day, value: -30, to: tomorrow) ?? tomorrow, tomorrow)
        case .thisMonth: return (month, tomorrow)
        case .lastMonth: return (calendar.date(byAdding: .month, value: -1, to: month) ?? month, month)
        }
    }
}

/// `$6,389.62`, `$0.35`, `<$0.01`: an estimate in US dollars.
func usageUSD(_ v: Double) -> String {
    if v > 0, v < 0.005 { return "<$0.01" }
    let cents = Int((v * 100).rounded())
    return "$\(usageNum(cents / 100)).\(String(format: "%02d", cents % 100))"
}

/// `2026年11月15日 08:56`, when a price list or a bill was made.
func usageClock(_ d: Date) -> String {
    let f = DateFormatter()
    f.locale = Locale(identifier: "zh_CN")
    f.dateFormat = "yyyy年M月d日 HH:mm"
    return f.string(from: d)
}

/// `15.4K`, `21.4M`, `6.45B`: three significant digits, for the receipt.
func usageCompact(_ n: Int) -> String {
    let units: [(Double, String)] = [(1e9, "B"), (1e6, "M"), (1e3, "K")]
    for (size, unit) in units where Double(abs(n)) >= size {
        let v = Double(n) / size
        let digits = abs(v) >= 100 ? 0 : abs(v) >= 10 ? 1 : 2
        var s = String(format: "%.\(digits)f", v)
        if s.contains(".") { while s.hasSuffix("0") { s.removeLast() }; if s.hasSuffix(".") { s.removeLast() } }
        return s + unit
    }
    return usageNum(n)
}
