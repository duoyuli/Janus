import Foundation

// A usage bill as the receipt shows it (`UsageReceipt`): a period's bill across runs, worded by the
// service (`GET /api/usage/receipt`, `backend/src/setup_view/bill.rs`;
// docs/architecture/20261031-frontend.md#usage_bill). It lists who paid (Agents) with their models.
// The cost the agents reported stays apart from the estimate at models.dev list prices, which is
// never an invoice.

/// One payer of a bill, or one of its models: its name, the amount drawn beside it (the estimate
/// with the tokens, or the tokens) and what hovering the amount says.
struct UsageBillLine: Equatable, Identifiable {
    let name: String
    /// The reported cost beside an Agent's name, when it reported any.
    let cost: String?
    let tokens: String
    let estimate: String?
    let help: String
    var models: [UsageBillLine] = []
    var id: String { name }

    init(_ j: JSON, name: String) {
        self.name = name
        cost = j["cost"].string
        tokens = j["amount"]["tokens"].str
        estimate = j["amount"]["estimate"].string
        help = j["amount"]["help"].str
        models = j["models"].array.map { UsageBillLine($0, name: $0["model"].str) }
    }
}

struct UsageBill: Equatable {
    var title: String
    /// What the bill covers and when it was made.
    var meta: [UsageLine]
    var headline: String
    var headlineCaption: String
    var headlineDetail: String
    var headlineHelp: String
    var stats: [UsageLine]
    /// The heading over the payers: 「AGENT / 模型」, and over their amounts.
    var groupsTitle: String
    var amountTitle: String
    var groups: [UsageBillLine]
    /// Token total, its parts and the cache hit rate, at the foot of the bill.
    var totalRows: [UsageLine]
    var notes: [String]
    /// The bill as plain text, for copying.
    var text: String

    /// The receipt as the service words it.
    init(_ j: JSON) {
        let lines = { (k: String) in j[k].array.map { UsageLine(label: $0["label"].str, value: $0["value"].str) } }
        title = j["title"].str
        meta = lines("meta")
        headline = j["headline"].str
        headlineCaption = j["headlineCaption"].str
        headlineDetail = j["headlineDetail"].str
        headlineHelp = j["headlineHelp"].str
        stats = lines("stats")
        groupsTitle = j["groupsTitle"].str
        amountTitle = j["amountTitle"].str
        groups = j["groups"].array.map { UsageBillLine($0, name: $0["name"].str) }
        totalRows = lines("totals")
        notes = j["notes"].strings
        text = j["text"].str
    }
}

/// A period the bill can cover, in the person's calendar (the service works out its days).
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
}

/// `15.4K`, `21.4M`, `6.45B`: three significant digits, for the run card.
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
