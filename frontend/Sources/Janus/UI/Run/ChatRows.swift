import SwiftUI

// The rows of a conversation in turns (`Conversation`): the agent's line for each turn, its calls
// grouped, its text, the files it changed, and the preview of what it is writing right now.
// @project-doc docs/architecture/20261031-frontend.md#session_turns

/// The vertical rhythm of a conversation: a turn's blocks sit close, turns stand apart.
enum ChatRhythm {
    /// Before what opens a turn or a section.
    static let round: CGFloat = 20
    /// Between the blocks of one turn: the agent's line, its text, its changed files.
    static let block: CGFloat = 12
    /// Between the lines of a turn's work.
    static let line: CGFloat = 4
    /// The width prose reads well at; wider windows leave the rest empty.
    static let reading: CGFloat = 780
}

/// The agent's line at the top of its turn: who, and what the turn did. A turn with work folds
/// and opens here; while it runs, the line says what the agent is at.
struct TurnHead: View {
    let run: RunModel
    let turn: Conversation.Head
    /// Nil when the turn has no work to fold, only its reply.
    let open: Bool?
    let toggle: () -> Void

    var body: some View {
        let w = run.who(turn.agent)
        VStack(alignment: .leading, spacing: 6) {
        HStack(spacing: 8) {
            Avatar(initials: w.initials, color: w.color, size: 20, role: w.role)
            Text([w.name, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            Spacer(minLength: 0)
        }
        if let open {
            Button(action: toggle) {
                HStack(spacing: 6) {
                    DisclosureChevron(open: open)
                    if turn.running {
                        TimelineView(.periodic(from: .now, by: 1)) { ctx in
                            Shimmer(text: summary(now: ctx.date.timeIntervalSince1970 * 1000))
                        }
                    } else {
                        Text(verbatim: summary(now: 0)).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
                .hoverable(6)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(open ? L.text("收起这一轮的工作过程") : L.text("展开这一轮的工作过程"))
            .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
        }
        }
    }

    /// `工作了 2 分 13 秒 · 运行 3 条命令 · 改 1 个文件` as the server wrote it, or
    /// `正在工作 · 35 秒 · …` with the clock running while it runs.
    private func summary(now: Double) -> String {
        guard turn.running else { return turn.summary }
        return ([L.text("正在工作 · \(L.dur(max(0, now - turn.start)))")] + (turn.stats.isEmpty ? [] : [turn.stats])).joined(separator: " · ")
    }
}

/// Text that is in progress: a light passing over it, as Lody marks what is still happening,
/// instead of a spinner.
struct Shimmer: View {
    let text: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let label = Text(verbatim: text).font(.callout).lineLimit(1)
        if reduceMotion {
            label.foregroundStyle(.secondary)
        } else {
            TimelineView(.animation(minimumInterval: 1 / 20)) { ctx in
                let phase = ctx.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.6) / 1.6
                label.foregroundStyle(
                    LinearGradient(
                        stops: [
                            .init(color: .secondary, location: max(0, phase - 0.25)),
                            .init(color: .primary, location: phase),
                            .init(color: .secondary, location: min(1, phase + 0.25)),
                        ],
                        startPoint: .leading, endPoint: .trailing))
            }
        }
    }
}

/// Calls and thinking in a row. More than one shows as a line of what they did that opens to
/// every call; one call is just its own line.
struct ActivityRow: View {
    let run: RunModel
    let events: [Event]
    /// Nil: every call shows, under no line of its own.
    let open: Bool?
    /// `运行 2 条命令 · 读 1 个文件 · 思考`, as the server wrote it.
    let summary: String
    let st: RunState
    let toggle: () -> Void

    var body: some View {
        if events.count == 1, let e = events.first {
            line(e)
        } else if open == nil {
            VStack(alignment: .leading, spacing: ChatRhythm.line) { ForEach(events) { line($0) } }
        } else {
            let open = open ?? true
            VStack(alignment: .leading, spacing: ChatRhythm.line) {
                Button(action: toggle) {
                    HStack(spacing: 6) {
                        DisclosureChevron(open: open)
                        Text(verbatim: summary).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
                if open {
                    VStack(alignment: .leading, spacing: ChatRhythm.line) {
                        ForEach(events) { line($0) }
                    }
                    .padding(.leading, 12)
                    .overlay(alignment: .leading) { Rectangle().fill(Color.line).frame(width: 1).padding(.leading, 8) }
                }
            }
        }
    }

    @ViewBuilder private func line(_ e: Event) -> some View {
        if e.type == .agentThought { ThoughtLine(text: e.data["text"].str) } else { ToolRow(run: run, st: st, e: e) }
    }

}

/// The agent's thinking, quiet: its first lines, all of it a click away.
struct ThoughtLine: View {
    let text: String
    @Local private var open = false

    var body: some View {
        Button { open.toggle() } label: {
            Text(verbatim: text.trimmingCharacters(in: .whitespacesAndNewlines))
                .font(.callout).italic().foregroundStyle(.secondary)
                .lineLimit(open ? nil : 2)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .textSelection(.enabled)
        .help(open ? L.text("收起思考") : L.text("展开思考"))
        .accessibilityLabel(L.text("思考"))
    }
}

/// The agent's own words: the reply that ended a turn in full colour, what it said along the
/// way quieter. Full width like a document, not a chat bubble.
struct TextRow: View {
    let events: [Event]
    let reply: Bool

    var body: some View {
        // Pieces keep their own whitespace (code blocks may be cut anywhere): join them as they came.
        let words = events.map { $0.data["text"].str }.joined().trimmingCharacters(in: .whitespacesAndNewlines)
        if !words.isEmpty {
            MarkdownText(text: words)
                .foregroundStyle(reply ? Color.primary : Color.secondary)
                .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
        }
    }
}

/// What a session is writing or thinking right now, before it is on record.
struct LiveRow: View {
    let session: LivePreview

    var body: some View {
        VStack(alignment: .leading, spacing: ChatRhythm.line) {
            if !session.thought.isEmpty && session.text.isEmpty {
                let lines = session.thought.trimmingCharacters(in: .whitespacesAndNewlines).components(separatedBy: "\n").suffix(3).joined(separator: "\n")
                Text(verbatim: lines).font(.callout).italic().foregroundStyle(.secondary).lineLimit(3)
                    .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
            }
            if !session.text.isEmpty {
                MarkdownText(text: session.text)
                    .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
            }
        }
    }
}

/// The files a turn changed, under its reply: each with its lines added and removed, the change
/// itself a click away.
struct EditsCard: View {
    let files: [Conversation.EditedFile]
    /// The lines of each change, by event seq (`RunState.diffs`).
    let diffs: [Int: [ServedDiff]]
    let cwd: String
    @Local private var shown: Set<String> = []
    @Local private var all = false

    var body: some View {
        let added = files.reduce(0) { $0 + $1.added }, removed = files.reduce(0) { $0 + $1.removed }
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(verbatim: L.text("改了 \(files.count) 个文件")).font(.callout.weight(.medium))
                Spacer()
                Counts(added: added, removed: removed)
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            ForEach(Array((all ? files : Array(files.prefix(4))).enumerated()), id: \.offset) { _, f in
                Divider()
                Button { if shown.contains(f.path) { shown.remove(f.path) } else { shown.insert(f.path) } } label: {
                    HStack(spacing: 8) {
                        let rel = L.inWorkspace(f.path, cwd)
                        Text(verbatim: (rel as NSString).lastPathComponent).font(.system(.callout, design: .monospaced)).lineLimit(1)
                        Text(verbatim: (rel as NSString).deletingLastPathComponent).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                        Spacer(minLength: 8)
                        Counts(added: f.added, removed: f.removed)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .contentShape(Rectangle())
                    .hoverable(0)
                }
                .buttonStyle(.plain)
                .help(L.tilde(f.path))
                if shown.contains(f.path) {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(Array(f.changes.enumerated()), id: \.offset) { _, c in
                            if let list = diffs[c.seq], c.index < list.count { InlineDiff(path: L.tilde(f.path), lines: list[c.index].lines) }
                        }
                    }
                    .padding(.horizontal, 12).padding(.bottom, 8)
                }
            }
            if files.count > 4 && !all {
                Divider()
                Button { all = true } label: {
                    Label(L.text("还有 \(files.count - 4) 个"), systemImage: "chevron.down").font(.caption).frame(maxWidth: .infinity).padding(.vertical, 6).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .background(Color.soft.opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line))
        .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
    }
}

/// `+42 −10` in the colours of a diff.
struct Counts: View {
    let added: Int
    let removed: Int
    var body: some View {
        HStack(spacing: 4) {
            Text(verbatim: "+\(added)").foregroundStyle(Tone.green.color)
            Text(verbatim: "−\(removed)").foregroundStyle(Tone.red.color)
        }
        .font(.caption.monospacedDigit())
    }
}
