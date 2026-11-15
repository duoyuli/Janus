import SwiftUI

// How acceptance evidence is shown: the method as text first, then the whole evidence, then the
// independent verification, the trace gaps and the risks apart from it. Colour and glyphs repeat
// what the text already says, so dark mode, VoiceOver and a black-and-white screenshot keep the
// meaning (docs/architecture/frontend.md#evidence_methods).

/// One check's method, written out: 「测试」「命令」「检查」「推理判断」「未核实」, 「未说明」 for a
/// check recorded without one, or the recorded value when it is not an allowed method.
struct MethodTag: View {
    /// nil for a criterion that nothing answered yet.
    let check: Check?

    var body: some View {
        let style = self.style
        HStack(spacing: 4) {
            Image(systemName: style.symbol).font(.system(size: 9, weight: .bold)).accessibilityHidden(true)
            Text(style.text)
        }
        .font(.system(size: 11, weight: .medium))
        .foregroundStyle(style.tone.color)
        .padding(.horizontal, 6).padding(.vertical, 1.5)
        .background(style.tone.color.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
        .fixedSize()
        .help(style.help.isEmpty ? "证据方法：\(style.text)" : "证据方法：\(style.text) · \(style.help)")
        // The text carries the meaning; VoiceOver reads it without the glyph or the tone.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("证据方法：\(style.text)")
    }

    private var style: (text: String, tone: Tone, symbol: String, help: String) {
        guard let check else { return ("未核对", .gray, "minus.circle", "还没有核对这条标准") }
        if let method = check.method {
            switch method {
            case .test: return ("测试", .green, "checkmark.seal", "运行自动化测试得到的结果")
            case .command: return ("命令", .blue, "terminal", "运行命令、脚本或检查工具得到的结果")
            case .inspection: return ("检查", .blue, "doc.text.magnifyingglass", "检查代码、文件、界面或实际状态得到的结果")
            case .reasoning: return ("推理判断", .amber, "brain", "根据推理或自我判断得到的结果，不是运行得到的结果")
            case .unverified: return ("未核实", .amber, "questionmark.circle", "证据缺失、无法复现或暂时无法核实")
            }
        }
        if check.badMethod != nil {
            return (check.methodLabel, .red, "exclamationmark.triangle", "不是允许的取证方法，由核对报出")
        }
        return ("未说明", .gray, "minus.circle", "这次结果没有说明证据方法")
    }
}

/// One acceptance criterion with the check that answered it: pass mark, method as text, the
/// criterion and the whole evidence, selectable. `n` is the criterion's place in the result; the
/// session shows a check without one.
struct CheckLine: View {
    let n: Int?
    let criterion: String
    let check: Check?
    var dense = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(check.map { $0.passed ? "✓" : "✗" } ?? "○")
                .fontWeight(.bold)
                .foregroundStyle(check.map { $0.passed ? Tone.green.color : Tone.red.color } ?? Color.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    MethodTag(check: check)
                    Text(criterion.isEmpty ? "（没有写明标准）" : criterion)
                        .font(dense ? .footnote.weight(.medium) : .callout.weight(.medium))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let check {
                    if check.evidence.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        Text("没有给出证据").font(.caption).foregroundStyle(Tone.red.color)
                    } else {
                        // The whole evidence, never shortened: this is what the person checks.
                        Text(MD.inline(check.evidence))
                            .font(.caption).foregroundStyle(.secondary)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(voice)
    }

    /// What VoiceOver reads: whether it passed, the method as words, the criterion and evidence.
    private var voice: String {
        let head = n.map { "第 \($0) 条，" } ?? ""
        guard let check else { return "\(head)未核对，\(criterion)" }
        let state = check.passed ? "通过" : "没通过"
        let method = "证据方法：\(check.methodLabel)"
        let evidence = check.evidence.trimmingCharacters(in: .whitespacesAndNewlines)
        return "\(head)\(state)，\(method)，\(criterion)\(evidence.isEmpty ? "，没有给出证据" : "，证据：\(evidence)")"
    }
}

/// The reminders under a task's checks: weaker evidence that the lead should read but that is not a
/// failure by itself (reasoning, unverified, no method at all).
struct EvidenceReminders: View {
    let reminders: [String]
    var body: some View {
        if !reminders.isEmpty {
            Note(text: "提醒（不影响是否通过）：\n\(reminders.map { "· \($0)" }.joined(separator: "\n"))", tone: .amber)
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
                    Text(v.passed ? "已确认这个任务的结论" : "结论没有得到确认")
                        .font(.caption).foregroundStyle(v.passed ? .secondary : Tone.amber.color)
                }
                if !v.summary.isEmpty { Text(MD.inline(v.summary)).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true) }
                ForEach(CriterionCheck.rows(accept: [], checks: v.checks)) { row in
                    CheckLine(n: row.n, criterion: row.criterion, check: row.check, dense: true)
                }
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

/// One task's acceptance in the delivery: totals and method counts, every check, then — apart from
/// the executor's evidence — the independent verification, the trace gaps and the risks.
struct DeliveryTaskView: View {
    let record: DeliveryAcceptance

    var body: some View {
        let s = record.summary
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                TaskTag(id: record.task)
                Text(record.title).font(.callout.weight(.semibold)).textSelection(.enabled)
                let status = L.task(record.status)
                Pill(status.0, status.1)
                Text(verbatim: "通过 \(s.passed) / \(s.criteria) 条")
                    .font(.caption).foregroundStyle(.secondary)
                    .help("验收标准 \(s.criteria) 条，通过 \(s.passed) 条")
            }
            Text(verbatim: "证据：\(s.countsText)")
                .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            ForEach(CriterionCheck.rows(accept: [], checks: record.checks)) { row in
                CheckLine(n: row.n, criterion: row.criterion, check: row.check)
            }
            EvidenceReminders(reminders: s.reminders)
            if !record.verifications.isEmpty {
                Divider()
                Text("独立验证").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                VerificationList(records: record.verifications).padding(.leading, 2)
            }
            if !record.traceGaps.isEmpty {
                Note(text: "痕迹核对（结果和工作目录对不上）：\n\(record.traceGaps.map { "· \($0)" }.joined(separator: "\n"))", tone: .amber)
            }
            if !record.risks.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    Text("风险").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    RiskList(risks: record.risks, font: .caption)
                }
            }
        }
    }
}
