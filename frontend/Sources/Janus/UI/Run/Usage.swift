import SwiftUI

// The usage ledger on the run page and the receipt the settings' usage bill draws
// (docs/architecture/20261031-frontend.md#usage_ledger). The numbers come from `RunState.usage`;
// these views only lay them out.

/// The run's usage ledger, opened from the run card's summary line (W1 F3). It is the expanded
/// body only: the summary line already names the totals and their completeness, so the body does
/// not repeat the card title, the completeness capsule or the reminders. Tokens, makeup, cost,
/// wall-clock time, context snapshots, the lead and collaborator subtotals and the task and member
/// detail all come from `UsageScope.summaryRows`, so a test reads exactly what this card shows. A
/// missing number is written in words and never as 0.
// @project-doc docs/architecture/20261031-frontend.md#usage_ledger
struct UsageSection: View {
    let run: RunModel
    let st: RunState
    /// The second-level task and member list, closed until asked for.
    @Local private var detail = false

    /// What each row means, shown on hover.
    private var help: [String: String] { [
        "Token": L.text("每一轮 Agent 上报的 token 相加（含子 Agent，Agent 按模型上报时以它为准）；缺失写「未上报」，不写 0；上下文快照不计入"),
        L.text("构成"): L.text("Agent 上报的输入、缓存读取、缓存写入和输出；为 0 或没有上报的部分不列出"),
        L.text("费用"): L.text("Agent 上报的原值，不同币种或口径不合并；没有上报时写「未上报」，不根据 token 估算"),
        L.text("耗时"): L.text("墙钟时长：从任务创建到最后一条事件。等待你的时间单独写出，不当作 Agent 执行时间"),
        L.text("上下文"): L.text("上下文快照只显示最近值、峰值和比例，不计入 token 总量"),
        L.text("小计"): L.text("任务总计包含负责人；负责人和协作者分开列出，方便对比"),
    ] }

    var body: some View {
        let u = st.usage
        let subtotal = u.subtotalLine
        let rowHelp = help
        VStack(alignment: .leading, spacing: 9) {
            Divider()
            ForEach(u.run.summaryRows) { row in
                UsageRow(label: row.label, value: row.value).help(rowHelp[row.label] ?? "")
            }
            UsageRow(label: subtotal.label, value: subtotal.value).help(rowHelp[subtotal.label] ?? "")
            HStack(spacing: 8) {
                Text(verbatim: L.text("\(u.run.sessions) 个会话")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                // A disclosure, not a link: it opens the task and member list at this level.
                Button { detail.toggle() } label: {
                    HStack(spacing: 4) {
                        Image(systemName: detail ? "chevron.down" : "chevron.right")
                            .font(.caption2.weight(.semibold))
                        Text(detail ? L.text("收起明细") : L.text("子任务与成员明细")).font(.callout)
                    }
                    .foregroundStyle(Color.accent)
                }
                .buttonStyle(.plain)
                .help(L.text("按子任务和成员展开这次任务的用量，每个数字都来自 session.usage 事件"))
                .accessibilityLabel(detail ? L.text("收起子任务与成员明细") : L.text("展开子任务与成员明细"))
            }
            if detail { details(u) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(L.text("这次任务的用量明细"))
    }

    @ViewBuilder private func details(_ u: UsageLedger) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            if u.tasks.isEmpty && u.members.isEmpty {
                Text(L.text("还没有子任务和会话。")).font(.caption).foregroundStyle(.secondary)
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
                .help(L.text("\(u.taskDetail(scope)) · \(scope.costDetail)，点按打开子任务页"))
            }
            if !u.tasks.isEmpty && !u.members.isEmpty { Divider() }
            ForEach(u.members, id: \.id) { member in
                Button { run.page = .agent(member.id) } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        let w = run.who(member.id)
                        Avatar(initials: w.initials, color: w.color, size: 14, role: w.role)
                        Text(w.name).font(.callout).lineLimit(1)
                        if member.id == "lead", w.name != L.text("负责人") { Text(L.text("负责人")).font(.caption2).foregroundStyle(.secondary) }
                        Spacer(minLength: 8)
                        Text(verbatim: u.memberDetail(member)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(u.memberDetail(member) + (member.contextDetail.map { L.text(" · 上下文 \($0)") } ?? ""))
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
        .accessibilityLabel(L.text("\(label)：\(value)"))
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
                    .help(bill.tokens.total.map { "\(usageNum($0)) tokens" } ?? L.text("没有会话上报 token"))
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
                    Text(bill.priced ? L.text("估算 · TOKEN") : "TOKEN")
                }
                .font(.caption2.weight(.semibold)).tracking(1).foregroundStyle(.secondary)
                ForEach(bill.groups) { g in ReceiptGroup(group: g) }
            }
            ReceiptRule()
            ForEach(bill.totalRows) { ReceiptRow(line: $0) }
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
        let compact = tokens.total.map(usageCompact) ?? L.text("未上报")
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            if let estimate {
                Text(compact).font(.caption2).foregroundStyle(.secondary)
                Text(usageUSD(estimate)).font(bold ? .callout.weight(.bold) : .caption)
            } else {
                Text(compact).font(bold ? .callout.weight(.bold) : .caption)
            }
        }
        .monospacedDigit()
        .help(tokens.total.map { L.text("\(usageNum($0)) tokens · \(turns) 轮") } ?? L.text("没有上报 token"))
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
