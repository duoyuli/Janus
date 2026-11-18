import Foundation

// A usage bill as the receipt shows it (`UsageReceipt`): a period's bill across runs, read from
// `GET /api/usage` (docs/architecture/20261031-frontend.md#usage_bill). It lists who paid (Agents)
// with their models. The cost the agents reported stays apart from the estimate at models.dev list
// prices, which is never an invoice.

/// What a bill's tokens would cost at list prices, and what could not be priced.
struct UsageEstimate: Equatable {
    var amount = 0.0
    var unpricedTurns = 0
    var unpricedModels: [String] = []
    var fetchedAt: Date?
}

struct UsageBill: Equatable {
    var title: String
    /// What the bill covers and when it was made.
    var meta: [UsageLine] = []
    var tokens = UsageTokens()
    var cost = UsageCost()
    var stats: [UsageLine] = []
    /// The heading over the payers: 「AGENT / 模型」.
    var groupsTitle: String
    var groups: [UsageBillGroup] = []
    var notes: [String] = []
    /// At models.dev list prices; nil before prices are known.
    var estimate: UsageEstimate?

    /// Whether the headline is money: some of the tokens have a price.
    var priced: Bool { (estimate?.amount ?? 0) > 0 }

    /// `$77.04` when priced, else the tokens: `15.4K`, `21.4M`, `6.45B`.
    var headline: String { priced ? usageUSD(estimate?.amount ?? 0) : tokens.total.map(usageCompact) ?? L.text("未上报") }

    /// What the headline is: an estimate at list prices, or the token total.
    var headlineCaption: String { priced ? L.text("按 models.dev API 价格折算 · 非账单") : L.text("TOKEN 总量") }

    /// `3,111,893 tokens · Agent 上报费用 0.25 USD`, under the headline.
    var headlineDetail: String {
        let cost = UsageScope(cost: cost).costDetail
        return [tokens.total.map { "\(usageNum($0)) tokens" }, self.cost.lines.isEmpty ? L.text("Agent 未上报费用") : L.text("Agent 上报费用 \(cost)")].compactMap { $0 }.joined(separator: " · ")
    }

    /// What the estimate rests on, first among the notes.
    var priceNotes: [String] {
        guard let e = estimate else { return [] }
        var out = [L.text("估算按 models.dev 的 API 价格（不含长上下文加价），不是账单。")]
        if let date = e.fetchedAt { out[0] += L.text("价格更新于 \(L.datedTime(date))。") }
        if e.unpricedTurns > 0 { out.append(L.text("\(e.unpricedTurns) 轮没有价格（\(e.unpricedModels.joined(separator: L.text("、")))），没有计入估算。")) }
        return out
    }

    /// The bill as plain text, for copying.
    var text: String {
        var out = [title] + meta.map { L.text("\($0.label)：\($0.value)") } + ["", L.text("Token 总量：\(headline)（\(headlineDetail)）"), ""]
        out += stats.map { L.text("\($0.label)：\($0.value)") }
        if !groups.isEmpty {
            out += ["", groupsTitle]
            for g in groups {
                out.append(L.text("\(g.name)：\(g.tokens.total.map(usageNum) ?? L.text("未上报"))") + (g.cost.reported ? L.text("（\(UsageScope(cost: g.cost).costDetail)）") : ""))
                out += g.models.map { "  · \($0.model)：\($0.tokens.total.map(usageNum) ?? L.text("未上报"))" }
            }
        }
        out += [""] + totalRows.map { L.text("\($0.label)：\($0.value)") }
        return (out + (notes.isEmpty ? [] : [""] + notes)).joined(separator: "\n")
    }

    /// Token total, its parts and the cache hit rate, at the foot of the bill.
    var totalRows: [UsageLine] {
        var rows = [UsageLine(label: L.text("Token 总量"), value: tokens.total.map(usageNum) ?? L.text("未上报"))]
        for (label, n) in [(L.text("输入"), tokens.input), (L.text("缓存读取"), tokens.cachedRead), (L.text("缓存写入"), tokens.cachedWrite), (L.text("输出"), tokens.output)] {
            if let n, n > 0 { rows.append(UsageLine(label: label, value: usageNum(n))) }
        }
        if let rate = tokens.cacheHitRate { rows.append(UsageLine(label: L.text("缓存命中率"), value: usagePercent(rate))) }
        return rows
    }
}

extension UsageBill {
    /// A period's bill as `GET /api/usage` returns it.
    static func period(_ j: JSON, issued: Date) -> UsageBill {
        let from = Date(timeIntervalSince1970: (j["from"].double ?? 0) / 1000)
        let last = Date(timeIntervalSince1970: ((j["to"].double ?? 0) - 1) / 1000)
        let tokens = UsageTokens.bill(j["tokens"])
        let active = j["activeDays"].int ?? 0
        var stats = [UsageLine(label: L.text("任务"), value: L.text("\(j["runs"].int ?? 0) 个")),
                     UsageLine(label: L.text("请求"), value: L.text("\(usageNum(j["turns"].int ?? 0)) 轮")),
                     UsageLine(label: L.text("会话"), value: L.text("\(j["sessions"].int ?? 0) 个")),
                     UsageLine(label: L.text("活跃天数"), value: "\(active) / \(j["days"].int ?? 0)")]
        if let total = tokens.total, active > 0 { stats.append(UsageLine(label: L.text("活跃日均"), value: "\(usageCompact(total / active)) tokens")) }
        var notes = [L.text("Token 总量含缓存读取。")]
        if let n = j["unreportedSessions"].int, n > 0 { notes.insert(L.text("\(n) 个会话没有上报 token，没有计入。"), at: 0) }
        let e = j["estimate"]
        var bill = UsageBill(
            title: L.text("用量账单"),
            meta: [UsageLine(label: L.text("区间"), value: "\(L.calendarDate(from)) – \(L.calendarDate(last))"),
                   UsageLine(label: L.text("开具"), value: L.datedTime(issued))],
            tokens: tokens, cost: UsageCost.bill(j["cost"]), stats: stats,
            groupsTitle: L.text("AGENT / 模型"),
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
        case .week: return L.text("近 7 天")
        case .days30: return L.text("近 30 天")
        case .thisMonth: return L.text("本月")
        case .lastMonth: return L.text("上月")
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
