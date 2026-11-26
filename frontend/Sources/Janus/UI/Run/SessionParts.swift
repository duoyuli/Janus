import SwiftUI

/// The agent's plan, as its own interface shows it: a checklist above the conversation, folded
/// to one line once every step is done.
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct PlanStrip: View {
    let steps: [SessionView.Step]
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
    struct Row: Equatable {
        let pick: SessionView.Asking
        let label: String
        let detail: String
        /// The heading above this row, when it starts a group.
        var group: String?
    }

    var rows: [Row] = []
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
        case .accept: draft.wrappedValue = SessionView.complete(draft.wrappedValue, with: rows[min(index, rows.count - 1)].pick)
        case .dismiss: closedAt = draft.wrappedValue
        }
        return true
    }
}

/// Above the composer while typing: the agent's slash commands after `/`, workspace files after `@`.
/// ↑ ↓ move, Tab or ↩ takes one, Esc closes the list; a click takes one too.
struct Suggestions: View {
    let run: RunModel
    @Binding var draft: String
    let commands: [SessionView.Command]
    let picker: SuggestionPicker
    @Local private var files: [String] = []

    var body: some View {
        let asking = SessionView.asking(draft)
        let rows = Self.rows(asking, commands: commands, files: files)
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
                                Button { draft = SessionView.complete(draft, with: r.pick) } label: {
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
        .task(id: asking) {
            guard case .file(let q) = asking else { files = []; return }
            try? await Task.sleep(nanoseconds: 150_000_000)
            let r = try? await run.api.get("/api/runs/\(run.id)/files?q=\(API.q(q))")
            files = r?["files"].strings ?? []
        }
    }

    /// The rows for what is being typed: commands ranked (`SessionView.rank`), grouped by where
    /// they come from while nothing follows `/` yet; files as the service found them.
    static func rows(_ asking: SessionView.Asking?, commands: [SessionView.Command], files: [String]) -> [SuggestionPicker.Row] {
        switch asking {
        case .command(let q):
            let ranked = SessionView.rank(commands, q).prefix(50)
            var rows: [SuggestionPicker.Row] = []
            var last: Bool?
            for c in ranked {
                let detail = [c.description, c.hint.isEmpty ? "" : L.text("参数：\(c.hint)")].filter { !$0.isEmpty }.joined(separator: " · ")
                // Headings only for the whole list: once something is typed, the best match leads.
                let group = q.isEmpty && commands.contains(where: \.skill) && c.skill != last ? (c.skill ? L.text("Skill") : L.text("Agent 命令")) : nil
                last = c.skill
                rows.append(.init(pick: .command(c.name), label: "/\(c.name)", detail: detail, group: group))
            }
            return rows
        case .file: return files.prefix(8).map { .init(pick: .file($0), label: "@\($0)", detail: "") }
        case nil: return []
        }
    }
}

/// The controls the agent offers for its session (model, effort, mode …), switched while it runs,
/// in the agent's own terms.
struct SessionControls: View {
    let run: RunModel
    let target: SessionTarget
    let controls: [SessionView.Control]

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
                    Text(verbatim: L.text("\(c.name)：\(c.options.first { $0.value == c.current }?.name ?? c.current)")).font(.caption)
                }
                .menuStyle(.borderlessButton).fixedSize()
                .help(L.text("切换这个会话的\(c.name)，立即生效"))
            }
        }
    }

    private func set(_ c: SessionView.Control, _ value: String) {
        guard value != c.current else { return }
        var body = target.body
        body["type"] = "control"
        body["id"] = JSON(c.id)
        body["value"] = JSON(value)
        body["via"] = JSON(c.via)
        Task { await run.act(body) }
    }
}

/// A file change a tool call reported, as removed and added lines.
struct InlineDiff: View {
    let path: String
    let old: String
    let new: String

    var body: some View {
        let lines = Self.lines(old: old, new: new)
        VStack(alignment: .leading, spacing: 0) {
            Text(path).font(.system(size: 11, design: .monospaced).weight(.semibold)).padding(.horizontal, 8).padding(.vertical, 4)
            ForEach(Array(lines.prefix(400).enumerated()), id: \.offset) { _, l in
                Text(verbatim: "\(l.0)\(l.1)")
                    .font(.system(size: 11, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 8)
                    .background((l.0 == "+" ? Tone.green.color : l.0 == "-" ? Tone.red.color : .clear).opacity(0.1))
            }
        }
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 6))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .textSelection(.enabled)
    }

    /// Changed lines with three lines of context around them, the rest left out.
    static func lines(old: String, new: String) -> [(String, String)] {
        let a = old.components(separatedBy: "\n"), b = new.components(separatedBy: "\n")
        let diff = b.difference(from: a)
        var removed = Set<Int>(), inserted = Set<Int>()
        for c in diff { switch c { case .remove(let o, _, _): removed.insert(o); case .insert(let o, _, _): inserted.insert(o) } }
        var out: [(String, String)] = []
        var i = 0, j = 0
        while i < a.count || j < b.count {
            if i < a.count, removed.contains(i) { out.append(("-", a[i])); i += 1 }
            else if j < b.count, inserted.contains(j) { out.append(("+", b[j])); j += 1 }
            else { if j < b.count { out.append((" ", b[j])) }; i += 1; j += 1 }
        }
        let changed = out.indices.filter { out[$0].0 != " " }
        return out.indices.filter { k in changed.contains { abs($0 - k) <= 3 } }.map { out[$0] }
    }
}

/// This session's context in the composer's row, as a ring and a number: how full it is, or the
/// last turn's tokens when the agent does not say. Its own numbers, apart from the run's total.
struct ContextMeter: View {
    let usage: SessionView.Usage

    var body: some View {
        let u = usage
        HStack(spacing: 5) {
            if let p = u.percent {
                ZStack {
                    Circle().stroke(Color.secondary.opacity(0.25), lineWidth: 2)
                    Circle().trim(from: 0, to: CGFloat(p) / 100).stroke(p >= 80 ? Tone.amber.color : Color.secondary, style: StrokeStyle(lineWidth: 2, lineCap: .round)).rotationEffect(.degrees(-90))
                }
                .frame(width: 12, height: 12)
                Text(verbatim: "\(p)%")
            } else {
                Text(verbatim: "\(SessionView.tokens(u.tokens ?? u.used ?? 0)) tokens")
            }
        }
        .font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
        .help(([u.labels].compactMap { $0 } + [u.used.map { L.text("上下文已用 \(SessionView.tokens($0))") }, u.size.map { L.text("共 \(SessionView.tokens($0))") }].compactMap { $0 }).joined(separator: " · ") + L.text("\n本会话的数字不计入任务累计用量"))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(u.labels ?? "")
    }
}
