import SwiftUI

// How acceptance evidence is shown: the method as text first, then the whole evidence, then the
// independent verification, the trace gaps and the risks apart from it. Colour and glyphs repeat
// what the text already says, so dark mode, VoiceOver and a black-and-white screenshot keep the
// meaning (docs/architecture/20261031-frontend.md#evidence_methods).

/// One check's method, written out: 「测试」「命令」「检查」「推理判断」「未核实」, 「未说明」 for a
/// check recorded without one, or the recorded value when it is not an allowed method.
struct MethodTag: View {
    let method: CheckView.Method

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: method.icon).font(.system(size: 9, weight: .bold)).accessibilityHidden(true)
            Text(method.text)
        }
        .font(.system(size: 11, weight: .medium))
        .foregroundStyle(method.tone.color)
        .padding(.horizontal, 6).padding(.vertical, 1.5)
        .background(method.tone.color.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
        .fixedSize()
        .help(L.text("证据方法：\(method.text) · \(method.help)"))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(L.text("证据方法：\(method.text)"))
    }
}

/// One criterion with the check that answered it, as the server writes it: ✓/✗/○, how it was
/// checked, and the evidence (two lines until asked for the rest).
struct CheckLine: View {
    let line: CheckView
    var dense = false
    @Local private var whole = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(line.mark == .passed ? "✓" : line.mark == .failed ? "✗" : "○")
                .fontWeight(.bold)
                .foregroundStyle(line.mark == .passed ? Tone.green.color : line.mark == .failed ? Tone.red.color : Color.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    MethodTag(method: line.method)
                    Text(line.criterion)
                        .font(dense ? .footnote.weight(.medium) : .callout.weight(.medium))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !line.evidence.isEmpty {
                    Text(MD.inline(line.evidence))
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(whole ? nil : 2)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    if line.folds {
                        Button(whole ? L.text("收起") : L.text("全文")) { whole.toggle() }
                            .buttonStyle(.link).font(.caption)
                            .accessibilityLabel(whole ? L.text("收起证据") : L.text("展开证据全文"))
                    }
                } else if let none = line.noEvidence {
                    Text(none).font(.caption).foregroundStyle(Tone.red.color)
                }
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(line.voice)
    }
}

/// A record's checks, each as a line.
struct CheckLines: View {
    let lines: [CheckView]
    var dense = false

    var body: some View {
        ForEach(Array(lines.enumerated()), id: \.offset) { _, l in CheckLine(line: l, dense: dense) }
    }
}

struct EvidenceReminders: View {
    let reminders: [String]
    var body: some View {
        if !reminders.isEmpty {
            Note(text: L.text("提醒（不影响是否通过）：\n\(reminders.map { "· \($0)" }.joined(separator: "\n"))"), tone: .amber)
        }
    }
}

/// The tasks that verified a task independently, each with its own conclusion, checks, evidence and
/// risks, kept apart from what the executor reported.
struct VerificationList: View {
    let records: [VerificationRecord]

    var body: some View {
        ForEach(records) { v in
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(v.task).font(.system(size: 11, weight: .semibold, design: .monospaced))
                    Pill(v.label, v.passed ? .green : .amber)
                    if let vd = v.verdict {
                        Pill(L.text("判定：\(vd.label)"), vd == .approved ? .green : (vd == .rejected ? .red : .amber))
                    }
                    Text(v.passed ? L.text("已确认这个子任务的结论") : L.text("结论没有得到确认"))
                        .font(.caption).foregroundStyle(v.passed ? .secondary : Tone.amber.color)
                }
                if !v.summary.isEmpty { Text(MD.inline(v.summary)).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true) }
                CheckLines(lines: v.checks, dense: true)
                ForEach(v.evidence.indices, id: \.self) { i in
                    Text(MD.inline(v.evidence[i])).font(.caption).foregroundStyle(.secondary)
                        .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
                RiskList(risks: v.risks, font: .caption)
            }
        }
    }
}

/// Risks, one per line with a warning mark; the mark is decoration, the text carries the meaning.
struct RiskList: View {
    let risks: [String]
    var font: Font = .callout

    var body: some View {
        ForEach(risks.indices, id: \.self) { i in
            HStack(alignment: .firstTextBaseline, spacing: 5) {
                Text("⚠").foregroundStyle(Tone.amber.color).accessibilityHidden(true)
                Text(MD.inline(risks[i])).font(font).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// One task's acceptance in the delivery: one line by default, opened on demand (W1 F6/A5). The
/// collapsed row carries the id, a one-line title, the passed count, the method counts and text
/// markers; the body repeats nothing of the row except the full title, then — exactly as before —
/// every check with its method and whole evidence, the reminders, the independent verification,
/// the trace gaps and the risks.
struct DeliveryTaskView: View {
    let run: RunModel
    let record: DeliveryAcceptance
    /// The content width; under 838 the numbers take a second line instead of truncating (F9).
    var width: CGFloat

    var body: some View {
        let row = record
        // The person's switch wins for this opening of the run; otherwise the row's own default.
        let open = run.taskOpen[record.task] ?? row.needsAttention
        VStack(alignment: .leading, spacing: 6) {
            if BoardLayout.deliveryRowStacks(width) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        header(row, open: open, titleLimit: 1)
                        Spacer(minLength: 4)
                        disclosure(open)
                    }
                    numbers(row)
                }
            } else {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    header(row, open: open, titleLimit: 1)
                    Spacer(minLength: 4)
                    disclosure(open)
                }
            }
            if open { expanded }
        }
    }

    /// The id, title, passed count and method counts; in the wide layout they share the row, in the
    /// narrow one the numbers sit below. One VoiceOver element with the row's full reading.
    @ViewBuilder private func header(_ row: DeliveryAcceptance, open: Bool, titleLimit: Int) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            // The id stays whole; the title gives way first.
            TaskTag(id: record.task).fixedSize()
            Text(record.title).font(.callout.weight(.semibold)).textSelection(.enabled)
                .lineLimit(titleLimit).truncationMode(.tail)
            if !BoardLayout.deliveryRowStacks(width) {
                Text(verbatim: row.metrics).font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).fixedSize(horizontal: true, vertical: false)
                StrengthBar(strength: record.strength)
                if !row.markers.isEmpty {
                    Text(verbatim: row.markers.joined(separator: " · "))
                        .font(.caption).foregroundStyle(Tone.amber.color)
                        .lineLimit(1).fixedSize(horizontal: true, vertical: false)
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(row.voice(open: open))
    }

    @ViewBuilder private func numbers(_ row: DeliveryAcceptance) -> some View {
        HStack(spacing: 6) {
            Text(verbatim: row.metrics).font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            StrengthBar(strength: record.strength)
        }
        if !row.markers.isEmpty {
            Text(verbatim: row.markers.joined(separator: " · "))
                .font(.caption).foregroundStyle(Tone.amber.color)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func disclosure(_ open: Bool) -> some View {
        Button { run.taskOpen[record.task] = !open } label: {
            HStack(spacing: 6) {
                DisclosureChevron(open: open)
                Text(open ? L.text("收起") : L.text("展开")).font(.caption)
            }
            .foregroundStyle(Color.accent)
            .padding(.vertical, 2).padding(.trailing, 6)
            .contentShape(Rectangle())
            .hoverable(6)
        }
        .buttonStyle(.plain)
        .help(open ? L.text("收起这个子任务的验收证据") : L.text("展开这个子任务的验收标准、证据、独立验证、痕迹和风险"))
        .accessibilityLabel(open ? L.text("收起 \(record.task) 的验收证据") : L.text("展开 \(record.task) 的验收证据"))
    }

    @ViewBuilder private var expanded: some View {
        VStack(alignment: .leading, spacing: 6) {
            // The row title was shortened to one line; the body starts with the whole one.
            Text(record.title).font(.callout.weight(.semibold)).textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            Text(verbatim: L.text("证据：\(record.counts)"))
                .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            CheckLines(lines: record.checks)
            EvidenceReminders(reminders: record.reminders)
            if !record.verifications.isEmpty {
                Divider()
                Text(L.text("独立验证")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                VerificationList(records: record.verifications).padding(.leading, 2)
            }
            if !record.traceGaps.isEmpty {
                Note(text: L.text("改动核对（结果和工作目录对不上）：\n\(record.traceGaps.map { "· \($0)" }.joined(separator: "\n"))"), tone: .amber)
            }
            if !record.risks.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text(L.text("执行者自述的风险")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    RiskList(risks: record.risks, font: .caption)
                }
            }
        }
        .padding(.leading, 2)
    }
}

/// How a task's checks were verified, as one bar: run (tests, commands), read (inspection), only
/// said (reasoning, unverified, none). It repeats the method counts beside it; the help and
/// VoiceOver read them as words.
struct StrengthBar: View {
    let strength: EvidenceStrength

    var body: some View {
        if strength.total > 0 {
            let w: CGFloat = 64
            HStack(spacing: 0) {
                Rectangle().fill(Tone.green.color).frame(width: w * CGFloat(strength.ran) / CGFloat(strength.total))
                Rectangle().fill(Tone.blue.color).frame(width: w * CGFloat(strength.read) / CGFloat(strength.total))
                Rectangle().fill(Color.secondary.opacity(0.35)).frame(width: w * CGFloat(strength.said) / CGFloat(strength.total))
            }
            .frame(width: w, height: 6)
            .clipShape(RoundedRectangle(cornerRadius: 3))
            .help(L.text("证据：\(strength.text)"))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(L.text("证据：\(strength.text)"))
        }
    }
}
