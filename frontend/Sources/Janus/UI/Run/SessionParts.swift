import SwiftUI

/// The agent's plan, as its own interface shows it: a checklist above the conversation, folded
/// to one line once every step is done.
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct PlanStrip: View {
    let steps: [Conversation.Step]
    @Local private var open = true

    var body: some View {
        if !steps.isEmpty {
            let done = steps.filter { $0.status == "completed" }.count
            VStack(alignment: .leading, spacing: 4) {
                Button { open.toggle() } label: {
                    HStack(spacing: 6) {
                        DisclosureChevron(open: open)
                        Text(L.text("计划")).font(.caption.weight(.semibold))
                        Text(verbatim: "\(done)/\(steps.count)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                        if !open, let now = steps.first(where: { $0.status == "in_progress" }) { Text(now.content).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                        Spacer()
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
                if open {
                    ForEach(Array(steps.enumerated()), id: \.offset) { _, s in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Image(systemName: s.status == "completed" ? "checkmark.circle.fill" : s.status == "in_progress" ? "circle.dotted" : "circle")
                                .font(.caption).foregroundStyle(s.status == "completed" ? Color.secondary : s.status == "in_progress" ? Color.accent : Color.secondary.opacity(0.6))
                                .accessibilityLabel(s.status == "completed" ? L.text("已完成") : s.status == "in_progress" ? L.text("进行中") : L.text("未开始"))
                            Text(s.content).font(.callout)
                                .strikethrough(s.status == "completed", color: .secondary)
                                .foregroundStyle(s.status == "completed" ? Color.secondary : Color.primary)
                        }
                    }
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.soft)
            .onAppear { open = done < steps.count }
            Divider()
        }
    }
}

/// What the suggestions above a composer offer now and which one the keys are on. The composer
/// hands it the arrows, Tab, ↩ and Esc while it is open (`ComposerNav`).
@Observable final class SuggestionPicker {
    var rows: [Conversation.Suggestions.Row] = []
    var index = 0
    /// The draft the person closed the list on: it stays closed until the draft changes.
    var closedAt: String?

    func open(for draft: String) -> Bool { !rows.isEmpty && closedAt != draft }

    /// A key from the composer; true when the list took it. Taking a row writes it into the draft.
    func handle(_ nav: ComposerNav, draft: Binding<String>) -> Bool {
        guard open(for: draft.wrappedValue) else { return false }
        switch nav {
        case .up: index = (index + rows.count - 1) % rows.count
        case .down: index = (index + 1) % rows.count
        case .accept: draft.wrappedValue = rows[min(index, rows.count - 1)].completion
        case .dismiss: closedAt = draft.wrappedValue
        }
        return true
    }
}

/// Above the composer while typing: the agent's slash commands after `/`, workspace files after `@`,
/// as the server offers them for the draft (`GET /api/runs/{id}/suggest`). ↑ ↓ move, Tab or ↩
/// takes one, Esc closes the list; a click takes one too.
struct Suggestions: View {
    let run: RunModel
    let target: SessionTarget
    @Binding var draft: String
    let picker: SuggestionPicker

    var body: some View {
        // Only what was offered for this very draft: an answer for an earlier one is stale.
        let rows = run.suggestions[target].flatMap { $0.draft == draft ? $0.rows : nil } ?? []
        let open = !rows.isEmpty && picker.closedAt != draft
        VStack(alignment: .leading, spacing: 0) {
            if open {
                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(rows.enumerated()), id: \.offset) { i, r in
                                if let g = r.group {
                                    Text(g).font(.caption2.weight(.semibold)).foregroundStyle(.secondary).padding(.horizontal, 10).padding(.top, i == 0 ? 6 : 10).padding(.bottom, 2)
                                }
                                Button { draft = r.completion } label: {
                                    HStack(spacing: 10) {
                                        Text(r.label).font(.system(.callout, design: .monospaced)).lineLimit(1)
                                        Text(r.detail).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                        Spacer()
                                    }
                                    .padding(.horizontal, 10).padding(.vertical, 6)
                                    .background(i == picker.index ? Color.accentColor.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 6))
                                    .contentShape(Rectangle())
                                    .hoverable(6)
                                }
                                .buttonStyle(.plain)
                                .id(i)
                                .accessibilityAddTraits(i == picker.index ? .isSelected : [])
                            }
                        }
                        .padding(4)
                    }
                    .frame(maxHeight: 260)
                    .onChange(of: picker.index) { proxy.scrollTo(picker.index) }
                }
            }
        }
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(open ? Color.line : .clear))
        .onChange(of: rows, initial: true) {
            picker.rows = rows
            picker.index = 0
        }
        .onChange(of: draft) { if picker.closedAt != nil && picker.closedAt != draft { picker.closedAt = nil } }
    }
}

/// The controls the agent offers for its session (model, effort, mode …), switched while it runs,
/// in the agent's own terms.
struct SessionControls: View {
    let run: RunModel
    let target: SessionTarget
    let controls: [Conversation.Control]

    var body: some View {
        HStack(spacing: 8) {
            ForEach(controls) { c in
                Menu {
                    ForEach(c.options, id: \.value) { o in
                        Button { set(c, o.value) } label: {
                            if o.value == c.current { Label(o.name, systemImage: "checkmark") } else { Text(o.name) }
                        }
                    }
                } label: {
                    Text(verbatim: c.label).font(.caption)
                }
                .menuStyle(.borderlessButton).fixedSize()
                .help(L.text("切换这个会话的\(c.name)，立即生效"))
            }
        }
    }

    private func set(_ c: Conversation.Control, _ value: String) {
        guard value != c.current else { return }
        Task { await run.act(.control(target, id: c.id, value: value, via: c.via)) }
    }
}

/// A file change an event carries, as removed and added lines with a little context, as the
/// server worked them out (`ServedDiff`).
struct InlineDiff: View {
    let path: String
    let lines: [(mark: String, text: String)]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(path).font(.system(size: 11, design: .monospaced).weight(.semibold)).padding(.horizontal, 8).padding(.vertical, 4)
            ForEach(Array(lines.enumerated()), id: \.offset) { _, l in
                Text(verbatim: "\(l.mark)\(l.text)")
                    .font(.system(size: 11, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 8)
                    .background((l.mark == "+" ? Tone.green.color : l.mark == "-" ? Tone.red.color : .clear).opacity(0.1))
            }
        }
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 6))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .textSelection(.enabled)
    }
}

/// This session's context in the composer's row, as a ring and a number: how full it is, or the
/// last turn's tokens when the agent does not say. Its own numbers, apart from the run's total.
struct ContextMeter: View {
    let usage: Conversation.Usage

    var body: some View {
        let u = usage
        HStack(spacing: 5) {
            if let p = u.percent {
                ZStack {
                    Circle().stroke(Color.secondary.opacity(0.25), lineWidth: 2)
                    Circle().trim(from: 0, to: CGFloat(p) / 100).stroke(p >= 80 ? Tone.amber.color : Color.secondary, style: StrokeStyle(lineWidth: 2, lineCap: .round)).rotationEffect(.degrees(-90))
                }
                .frame(width: 12, height: 12)
            }
            Text(verbatim: u.meter)
        }
        .font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
        .help(u.help)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(u.labels)
    }
}
