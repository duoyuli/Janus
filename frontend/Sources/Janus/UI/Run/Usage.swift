import SwiftUI

// The usage ledger on the run page and the task page (docs/architecture/frontend.md#usage_ledger).
// The numbers come from `RunState.usage`; these views only lay them out.

/// The run's usage ledger on the workbench: tokens, cost state, wall-clock time, completeness, the
/// lead and collaborator subtotals, context snapshots, task and member detail, and the read-only
/// reminders. Every number comes from the same `session.usage` events the session cards read; a
/// missing number is written in words and never as 0. The rows come from `UsageScope.summaryRows`,
/// so a test reads exactly what this card shows.
// @project-doc docs/architecture/frontend.md#usage_ledger
struct UsageSection: View {
    let run: RunModel
    let st: RunState
    @Local private var detail: Bool

    init(run: RunModel, st: RunState, initiallyOpen: Bool = false) {
        self.run = run
        self.st = st
        _detail = Local(initialValue: initiallyOpen)
    }

    /// What each row means, shown on hover.
    static let help: [String: String] = [
        "Token": "每一轮 Agent 上报的 token 相加（含子 Agent，Agent 按模型上报时以它为准）；缺失写「未上报」，不写 0；上下文快照不计入",
        "构成": "Agent 上报的输入、缓存读取、缓存写入和输出；为 0 或没有上报的部分不列出",
        "费用": "Agent 上报的原值，不同币种或口径不合并；没有上报时写「未上报」，不根据 token 估算",
        "耗时": "墙钟时长：从 Run 创建到最后一条事件。等待你的时间单独写出，不当作 Agent 执行时间",
        "上下文": "上下文快照只显示最近值、峰值和比例，不计入 token 总量",
        "小计": "Run 总计包含主控；主控和协作者分开列出，方便对比",
    ]

    var body: some View {
        let u = st.usage
        let subtotal = u.subtotalLine
        VStack(alignment: .leading, spacing: 9) {
            Divider()
            HStack(spacing: 8) {
                Text("用量").font(.callout.weight(.semibold))
                Pill(u.run.completeness.label, u.run.completeness.tone)
                if u.run.sessions > 0 { Text(verbatim: "\(u.run.sessions) 个会话").font(.caption).foregroundStyle(.secondary) }
                Spacer()
                // A disclosure, not a link: it opens detail inside this card.
                Button { detail.toggle() } label: {
                    HStack(spacing: 4) {
                        Image(systemName: detail ? "chevron.down" : "chevron.right")
                            .font(.caption2.weight(.semibold))
                        Text(detail ? "收起明细" : "任务与成员明细").font(.callout)
                    }
                    .foregroundStyle(Color.accent)
                }
                .buttonStyle(.plain)
                .help("按任务和成员展开这次 Run 的用量，每个数字都来自 session.usage 事件")
                .accessibilityLabel(detail ? "收起任务与成员明细" : "展开任务与成员明细")
            }
            ForEach(u.run.summaryRows) { row in
                UsageRow(label: row.label, value: row.value).help(Self.help[row.label] ?? "")
            }
            UsageRow(label: subtotal.label, value: subtotal.value).help(Self.help[subtotal.label] ?? "")
            if detail { details(u) }
            // Reminders live in this card only: nothing is paused, cancelled, retried or budgeted
            // from here, and nothing is added to the inbox.
            if let line = u.remindersLine { Note(text: line, tone: .amber) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("这次 Run 的用量")
    }

    @ViewBuilder private func details(_ u: UsageLedger) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            if u.tasks.isEmpty && u.members.isEmpty {
                Text("还没有任务和会话。").font(.caption).foregroundStyle(.secondary)
            }
            ForEach(u.tasks, id: \.id) { scope in
                Button { run.show(.task(scope.id)) } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        TaskTag(id: scope.id)
                        Text(st.tasks[scope.id]?.title ?? "").font(.callout).lineLimit(1)
                        Spacer(minLength: 8)
                        Text(verbatim: u.taskDetail(scope)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help("\(u.taskDetail(scope)) · \(scope.costDetail)，点按打开任务页")
            }
            if !u.tasks.isEmpty && !u.members.isEmpty { Divider() }
            ForEach(u.members, id: \.id) { member in
                Button { run.page = .agent(member.id) } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        let w = run.who(member.id)
                        Avatar(initials: w.initials, color: w.color, size: 14)
                        Text(w.name).font(.callout).lineLimit(1)
                        if member.id == "lead", w.name != "主控" { Text("主控").font(.caption2).foregroundStyle(.secondary) }
                        Spacer(minLength: 8)
                        Text(verbatim: u.memberDetail(member)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(u.memberDetail(member) + (member.contextDetail.map { " · 上下文 \($0)" } ?? ""))
            }
        }
        .padding(.leading, 4)
    }
}

/// A usage row: a short label and its value. The value says 「未上报」 / 「口径未知」 in words, so a
/// missing number never reads as 0.
struct UsageRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(label).foregroundStyle(.secondary).font(.callout).frame(width: 56, alignment: .leading)
            Text(value).font(.callout).textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(label)：\(value)")
    }
}

/// A task's usage as a bill: the total, who spent it on which model, each attempt, and the parts of
/// the tokens; a task with no session yet says so instead of showing 0.
// @project-doc docs/architecture/frontend.md#usage_bill
struct TaskUsageCard: View {
    let run: RunModel
    let st: RunState
    let task: TaskState
    /// List prices of the bill's models, from the backend's models.dev table.
    @Local private var prices: [String: UsagePrice] = [:]
    @Local private var pricesAt: Date?

    var body: some View {
        let bill = UsageBill.task(task, ledger: st.usage, name: { run.who($0).name })
        let models = Set(bill?.groups.flatMap { $0.models.map(\.model) } ?? []).sorted()
        Card {
            if let bill {
                UsageReceipt(bill: pricesAt == nil && prices.isEmpty ? bill : bill.priced(with: prices, fetchedAt: pricesAt))
            } else {
                Text("用量").font(.headline)
                Text("未开始（这个任务还没有会话）").font(.callout).foregroundStyle(.secondary)
                    .accessibilityLabel("用量未开始，这个任务还没有会话")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(task.id) 的用量账单")
        .task(id: models) { await loadPrices(models) }
    }

    /// Ask the backend for the models' list prices; without them the bill shows tokens only.
    private func loadPrices(_ models: [String]) async {
        guard !models.isEmpty else { return }
        let query = models.joined(separator: ",").addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? ""
        guard let j = try? await run.store.api.get("/api/prices?models=\(query)") else { return }
        prices = j["prices"].object.compactMapValues(UsagePrice.init)
        pricesAt = j["fetchedAt"].double.map { Date(timeIntervalSince1970: $0 / 1000) }
    }
}

/// A bill laid out like a receipt: what it covers, the headline total, the numbers behind it, who
/// paid on which model, the parts of the tokens and the notes. Every value is text, selectable,
/// and read by VoiceOver with its label; the dashed rules are decoration.
struct UsageReceipt: View {
    let bill: UsageBill

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(bill.title).font(.display(.title3)).frame(maxWidth: .infinity)
            ForEach(bill.meta) { ReceiptRow(line: $0) }
            ReceiptRule()
            VStack(spacing: 3) {
                Text(bill.headlineCaption).font(.caption2.weight(.semibold)).tracking(1.5).foregroundStyle(.secondary)
                Text(bill.headline).font(.system(size: 34, weight: .bold, design: .rounded)).monospacedDigit()
                    .help(bill.tokens.total.map { "\(usageNum($0)) tokens" } ?? "没有会话上报 token")
                Text(bill.headlineDetail).font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .textSelection(.enabled)
            .accessibilityElement(children: .combine)
            ReceiptRule()
            ForEach(bill.stats) { ReceiptRow(line: $0) }
            if !bill.groups.isEmpty {
                ReceiptRule()
                HStack {
                    Text(bill.groupsTitle)
                    Spacer()
                    Text(bill.priced ? "估算 · TOKEN" : "TOKEN")
                }
                .font(.caption2.weight(.semibold)).tracking(1).foregroundStyle(.secondary)
                ForEach(bill.groups) { g in ReceiptGroup(group: g) }
            }
            ReceiptRule()
            ForEach(bill.totalRows) { ReceiptRow(line: $0) }
            ForEach(bill.sections) { section in
                ReceiptRule()
                Text(section.title).font(.caption2.weight(.semibold)).tracking(1).foregroundStyle(.secondary)
                ForEach(section.rows) { ReceiptRow(line: $0) }
            }
            if !bill.notes.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(bill.notes, id: \.self) { Text($0) }
                }
                .font(.caption2).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// One payer of a bill and its models, most tokens first.
private struct ReceiptGroup: View {
    let group: UsageBillGroup

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(group.name).font(.callout.weight(.bold))
                if group.cost.reported {
                    Text(verbatim: UsageScope(cost: group.cost).costDetail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                ReceiptAmount(estimate: group.estimate, tokens: group.tokens, turns: group.turns, bold: true)
            }
            ForEach(group.models) { m in
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text("•").foregroundStyle(.secondary).accessibilityHidden(true)
                    Text(m.model).font(.system(.caption, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 8)
                    ReceiptAmount(estimate: m.estimate, tokens: m.tokens, turns: m.turns, bold: false)
                }
                .padding(.leading, 6)
                .accessibilityElement(children: .combine)
            }
        }
        .textSelection(.enabled)
    }
}

/// A payer's or a model's amount: the estimate with the tokens beside it when priced, else the tokens.
private struct ReceiptAmount: View {
    let estimate: Double?
    let tokens: UsageTokens
    let turns: Int
    let bold: Bool

    var body: some View {
        let compact = tokens.total.map(usageCompact) ?? "未上报"
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            if let estimate {
                Text(compact).font(.caption2).foregroundStyle(.secondary)
                Text(usageUSD(estimate)).font(bold ? .callout.weight(.bold) : .caption)
            } else {
                Text(compact).font(bold ? .callout.weight(.bold) : .caption)
            }
        }
        .monospacedDigit()
        .help(tokens.total.map { "\(usageNum($0)) tokens · \(turns) 轮" } ?? "没有上报 token")
    }
}

/// A label and its value on one line of the receipt.
private struct ReceiptRow: View {
    let line: UsageLine

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(line.label).foregroundStyle(.secondary)
            Spacer(minLength: 8)
            Text(line.value).monospacedDigit().multilineTextAlignment(.trailing).textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
        .font(.callout)
        .accessibilityElement(children: .combine)
    }
}

/// The receipt's dashed rule.
private struct ReceiptRule: View {
    var body: some View {
        GeometryReader { g in
            Path { p in
                p.move(to: CGPoint(x: 0, y: 0.5))
                p.addLine(to: CGPoint(x: g.size.width, y: 0.5))
            }
            .stroke(Color.secondary.opacity(0.45), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
        }
        .frame(height: 1)
        .accessibilityHidden(true)
    }
}
