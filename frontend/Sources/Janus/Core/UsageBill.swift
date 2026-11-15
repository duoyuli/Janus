import Foundation

// A usage bill as the receipt shows it (`UsageReceipt`): a task's bill, built from the run's ledger,
// or a period's bill across runs, read from `GET /api/usage` (docs/architecture/frontend.md#usage_bill).
// Both list who paid (members of the task, or Agents) with their models. Costs are only what the
// agents reported; nothing is priced from tokens.

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

    /// `15.4K`, `21.4M`, `6.45B`: the headline and the payer lines; the full number is in the help.
    var headline: String { tokens.total.map(usageCompact) ?? "未上报" }

    /// `3,111,893 tokens · 费用 0.25 USD`, under the headline.
    var headlineDetail: String {
        let cost = UsageScope(cost: cost).costDetail
        return [tokens.total.map { "\(usageNum($0)) tokens" }, cost == "未上报" ? "费用未上报" : "费用 \(cost)"].compactMap { $0 }.joined(separator: " · ")
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
        var notes = ["费用只计 Agent 上报的金额，不按 token 估算。"]
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
        let clock = DateFormatter()
        clock.locale = day.locale
        clock.dateFormat = "yyyy年M月d日 HH:mm"
        let from = Date(timeIntervalSince1970: (j["from"].double ?? 0) / 1000)
        let last = Date(timeIntervalSince1970: ((j["to"].double ?? 0) - 1) / 1000)
        let tokens = UsageTokens.bill(j["tokens"])
        let active = j["activeDays"].int ?? 0
        var stats = [UsageLine(label: "Run", value: "\(j["runs"].int ?? 0) 个"),
                     UsageLine(label: "请求", value: "\(usageNum(j["turns"].int ?? 0)) 轮"),
                     UsageLine(label: "会话", value: "\(j["sessions"].int ?? 0) 个"),
                     UsageLine(label: "活跃天数", value: "\(active) / \(j["days"].int ?? 0)")]
        if let total = tokens.total, active > 0 { stats.append(UsageLine(label: "活跃日均", value: "\(usageCompact(total / active)) tokens")) }
        var notes = ["费用只计 Agent 上报的金额，不按 token 估算；Token 总量含缓存读取。"]
        if let n = j["unreportedSessions"].int, n > 0 { notes.insert("\(n) 个会话没有上报 token，没有计入。", at: 0) }
        return UsageBill(
            title: "用量账单",
            meta: [UsageLine(label: "区间", value: "\(day.string(from: from)) – \(day.string(from: last))"),
                   UsageLine(label: "开具", value: clock.string(from: issued))],
            tokens: tokens, cost: UsageCost.bill(j["cost"]), stats: stats,
            groupsTitle: "AGENT / 模型",
            groups: j["agents"].array.map { g in
                UsageBillGroup(id: g["id"].str, name: g["name"].str, tokens: .bill(g["tokens"]), turns: g["turns"].int ?? 0, cost: .bill(g["cost"]),
                               models: g["models"].array.map { UsageModelLine(model: $0["model"].str, tokens: .bill($0["tokens"]), turns: $0["turns"].int ?? 0) })
            },
            notes: notes)
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
