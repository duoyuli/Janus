import SwiftUI

/// The agent's plan, as its own interface shows it: a checklist above the conversation, folded
/// to one line once every step is done.
// @project-doc docs/architecture/frontend.md#session_view
struct PlanStrip: View {
    let steps: [SessionView.Step]
    @Local private var open = true

    var body: some View {
        if !steps.isEmpty {
            let done = steps.filter { $0.status == "completed" }.count
            VStack(alignment: .leading, spacing: 4) {
                Button { open.toggle() } label: {
                    HStack(spacing: 6) {
                        Image(systemName: open ? "chevron.down" : "chevron.right").font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                        Text("计划").font(.caption.weight(.semibold))
                        Text(verbatim: "\(done)/\(steps.count)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                        if !open, let now = steps.first(where: { $0.status == "in_progress" }) { Text(now.content).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                        Spacer()
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if open {
                    ForEach(Array(steps.enumerated()), id: \.offset) { _, s in
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            Image(systemName: s.status == "completed" ? "checkmark.circle.fill" : s.status == "in_progress" ? "circle.dotted" : "circle")
                                .font(.caption).foregroundStyle(s.status == "completed" ? Color.secondary : s.status == "in_progress" ? Color.accent : Color.secondary.opacity(0.6))
                                .accessibilityLabel(s.status == "completed" ? "已完成" : s.status == "in_progress" ? "进行中" : "未开始")
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

/// Above the composer while typing: the agent's slash commands after `/`, workspace files after `@`.
/// Picking one completes the word being typed.
struct Suggestions: View {
    let run: RunModel
    @Binding var draft: String
    let commands: [SessionView.Command]
    @Local private var files: [String] = []

    var body: some View {
        let asking = SessionView.asking(draft)
        let rows: [(SessionView.Asking, String, String)] = {
            switch asking {
            case .command(let q): return commands.filter { q.isEmpty || $0.name.lowercased().hasPrefix(q.lowercased()) }.prefix(8).map { (.command($0.name), "/\($0.name)", [$0.description, $0.hint.isEmpty ? "" : "参数：\($0.hint)"].filter { !$0.isEmpty }.joined(separator: " · ")) }
            case .file: return files.prefix(8).map { (.file($0), "@\($0)", "") }
            case nil: return []
            }
        }()
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { i, r in
                if i > 0 { Divider() }
                Button { draft = SessionView.complete(draft, with: r.0) } label: {
                    HStack(spacing: 10) {
                        Text(r.1).font(.system(.callout, design: .monospaced)).lineLimit(1)
                        Text(r.2).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                    }
                    .padding(.horizontal, 10).padding(.vertical, 6)
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
            }
        }
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(rows.isEmpty ? .clear : Color.line))
        .task(id: asking) {
            guard case .file(let q) = asking else { files = []; return }
            try? await Task.sleep(nanoseconds: 150_000_000)
            let r = try? await run.api.get("/api/runs/\(run.id)/files?q=\(API.q(q))")
            files = r?["files"].strings ?? []
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
                    Text(verbatim: "\(c.name)：\(c.options.first { $0.value == c.current }?.name ?? c.current)").font(.caption)
                }
                .menuStyle(.borderlessButton).fixedSize()
                .help("切换这个会话的\(c.name)，立即生效")
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
