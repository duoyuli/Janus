import SwiftUI

// The usage ledger on the run page and the receipt the settings' usage bill draws
// (docs/architecture/20261031-frontend.md#usage_ledger). The numbers come from `RunState.usage`;
// these views only lay them out.

/// The run's usage ledger, opened from the run card's summary line (W1 F3). It is the expanded
/// body only: the summary line already names the totals and their completeness, so the body does
/// not repeat the card title, the completeness capsule or the reminders. Its rows and the task and
/// member detail are the server's words (`UsageWords`). A missing number is written in words and
/// never as 0.
// @project-doc docs/architecture/20261031-frontend.md#usage_ledger
struct UsageSection: View {
    let run: RunModel
    let st: RunState
    /// The second-level task and member list, closed until asked for.
    @Local private var detail = false

    var body: some View {
        let u = run.usage
        VStack(alignment: .leading, spacing: 9) {
            Divider()
            ForEach(u.words.rows) { row in
                UsageRow(label: row.label, value: row.value).help(row.help)
            }
            HStack(spacing: 8) {
                Text(verbatim: u.words.sessions).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button { detail.toggle() } label: {
                    HStack(spacing: 6) {
                        DisclosureChevron(open: detail)
                        Text(detail ? L.text("收起明细") : L.text("子任务与成员明细")).font(.callout)
                    }
                    .foregroundStyle(Color.accent)
                    .padding(.vertical, 2).padding(.trailing, 6)
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(L.text("按子任务和成员拆开看用量"))
                .accessibilityLabel(detail ? L.text("收起子任务与成员明细") : L.text("展开子任务与成员明细"))
            }
            if detail { details(u.words) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(L.text("这次任务的用量明细"))
    }

    @ViewBuilder private func details(_ u: UsageWords) -> some View {
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
                        Text(verbatim: scope.text).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(scope.help)
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
                        Text(verbatim: member.text).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(member.help)
            }
        }
        .padding(.leading, 4)
    }
}

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
                    .help(bill.headlineHelp)
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
                    Text(bill.amountTitle)
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
    let group: UsageBillLine

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(group.name).font(.callout.weight(.bold))
                if let cost = group.cost {
                    Text(verbatim: cost).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                ReceiptAmount(line: group, bold: true)
            }
            ForEach(group.models) { m in
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text("•").foregroundStyle(.secondary).accessibilityHidden(true)
                    Text(m.name).font(.system(.caption, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 8)
                    ReceiptAmount(line: m, bold: false)
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
    let line: UsageBillLine
    let bold: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            if let estimate = line.estimate {
                Text(line.tokens).font(.caption2).foregroundStyle(.secondary)
                Text(estimate).font(bold ? .callout.weight(.bold) : .caption)
            } else {
                Text(line.tokens).font(bold ? .callout.weight(.bold) : .caption)
            }
        }
        .monospacedDigit()
        .help(line.help)
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
