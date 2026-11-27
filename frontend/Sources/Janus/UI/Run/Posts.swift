import SwiftUI

// One post of the group or of a task's topic, as the server wrote it (`Post`): who says it and
// when, then what it is. Janus's own lines about a task starting are one quiet line; what waits on
// the person is drawn as a mention with its card in place
// (docs/architecture/20261031-frontend.md#run_group).

struct PostRow: View {
    let run: RunModel
    let st: RunState
    let post: Post
    /// In a task's topic: the topic it is from goes without saying.
    var inTopic = false

    var body: some View {
        if post.kind == .start {
            startLine
        } else {
            message
        }
    }

    /// 「研发工程师 开始 T2」: a line between the messages, a click from the task's topic.
    private var startLine: some View {
        Button { open(post.task) } label: {
            HStack(spacing: 6) {
                Image(systemName: "play.fill").font(.system(size: 8)).foregroundStyle(.tertiary).accessibilityHidden(true)
                Text(verbatim: post.text).font(.caption).foregroundStyle(.secondary)
                Text(verbatim: L.clock(post.ts)).font(.caption2.monospacedDigit()).foregroundStyle(.tertiary)
                Spacer(minLength: 0)
            }
            .padding(.leading, inTopic ? 0 : 38)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(inTopic || post.task == nil)
        .help(post.task.map { L.text("打开 \($0) 的话题") } ?? "")
    }

    private var message: some View {
        let w = run.who(post.by)
        let size: CGFloat = inTopic ? 24 : 28
        return HStack(alignment: .top, spacing: 10) {
            Avatar(initials: w.initials, color: w.color, size: size, role: w.role)
            VStack(alignment: .leading, spacing: 5) {
                head(w)
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, post.you ? 8 : 0)
        .padding(.horizontal, post.you ? 8 : 0)
        .background(post.you ? Tone.amber.color.opacity(0.07) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
        .overlay(alignment: .leading) {
            if post.you { Rectangle().fill(Tone.amber.color).frame(width: 3).clipShape(RoundedRectangle(cornerRadius: 1.5)) }
        }
        .frame(maxWidth: ChatRhythm.reading + 40, alignment: .leading)
    }

    /// Who, their role in the run, when, what kind of post, and the topic it came from.
    private func head(_ w: RunModel.Who) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(w.name).font(.callout.weight(.semibold))
            if w.lead { Tag(text: L.text("负责人"), tone: .blue) }
            if post.by == "janus" { Tag(text: L.text("系统"), tone: .violet) }
            Text(verbatim: L.clock(post.ts)).font(.caption.monospacedDigit()).foregroundStyle(.tertiary)
            if !post.title.isEmpty && ![.dispatch, .result, .delivery].contains(post.kind) {
                Tag(text: post.title, tone: post.tone ?? .gray)
            }
            if !inTopic, let task = post.task {
                Button { open(task) } label: {
                    Text(verbatim: L.text("来自话题 \(task)")).font(.caption)
                }
                .buttonStyle(.link)
                .help(L.text("打开 \(task) 的话题"))
            }
        }
    }

    @ViewBuilder private var content: some View {
        switch post.kind {
        case .dispatch: dispatch
        case .result: result
        case .ask: ask
        case .delivery: delivery
        default: words
        }
    }

    /// Plain words: the agent's own, or Janus's line, and the lines under them.
    @ViewBuilder private var words: some View {
        if !post.text.isEmpty {
            MarkdownText(text: post.text)
                .foregroundStyle(post.kind == .failed && post.tone == .red ? Tone.red.color : Color.primary)
        }
        lines
    }

    @ViewBuilder private var lines: some View {
        if !post.lines.isEmpty {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(post.lines.enumerated()), id: \.offset) { _, l in
                    Text(MD.inline("· " + l)).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    /// The tasks the lead put on the board, each as it stands now; a click opens its topic.
    private var dispatch: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(post.title).font(.callout.weight(.semibold))
                Spacer()
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            ForEach(post.tasks, id: \.id) { item in
                Divider()
                dispatched(item.id, scope: item.scope)
            }
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line))
    }

    private func dispatched(_ id: String, scope: String) -> some View {
        let t = st.tasks[id]
        let (label, tone) = t?.status.label ?? (L.text("等上游"), .gray)
        let w = run.who(t?.agent ?? "")
        let said = st.feed.topics[id]?.count ?? 0
        return Button { open(id) } label: {
            HStack(alignment: .center, spacing: 10) {
                TaskTag(id: id)
                VStack(alignment: .leading, spacing: 2) {
                    Text(t?.title ?? id).font(.callout).lineLimit(1)
                    if !scope.isEmpty { Text(verbatim: scope).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
                Spacer(minLength: 8)
                HStack(spacing: 5) {
                    Avatar(initials: w.initials, color: w.color, size: 16, role: w.role)
                    Text(w.name).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Pill(label, tone)
                Text(verbatim: said > 0 ? L.text("\(said) 条 ›") : L.text("话题 ›"))
                    .font(.caption).foregroundStyle(Color.accent).frame(minWidth: 44, alignment: .trailing)
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .contentShape(Rectangle())
            .hoverable(0)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(id) \(t?.title ?? "") · \(w.name) · \(label)")
    }

    /// A result or a verdict: the agent's summary, its numbers, the checks a verification ran and
    /// what it hands on.
    private var result: some View {
        let checks = st.checks[post.seq] ?? []
        let verifies = post.task.flatMap { st.tasks[$0] }.map { !$0.verifies.isEmpty } ?? false
        return Card(padding: 12, tint: (post.tone ?? .green).color) {
            HStack(spacing: 6) {
                if let task = post.task { TaskTag(id: task) }
                Image(systemName: post.tone == .red ? "xmark" : post.tone == .amber ? "exclamationmark" : "checkmark")
                    .font(.caption.weight(.bold)).foregroundStyle((post.tone ?? .green).color)
                    .accessibilityHidden(true)
                Text(post.title).font(.callout.weight(.semibold))
                Spacer(minLength: 8)
                if let t = post.task.flatMap({ st.tasks[$0] }) {
                    Text(t.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            if !post.text.isEmpty { MarkdownText(text: post.text) }
            if !post.meta.isEmpty { Text(verbatim: post.meta).font(.caption).foregroundStyle(.secondary) }
            if verifies && !checks.isEmpty { CheckLines(lines: checks, dense: true) }
            lines
        }
    }

    /// What waits on the person: its card while open, else how it was settled.
    @ViewBuilder private var ask: some View {
        if !post.text.isEmpty && !post.you { Text(MD.inline(post.text)).font(.callout).foregroundStyle(.secondary).textSelection(.enabled) }
        if post.you, let card = st.cards.first(where: { $0.key == post.key && !post.key.isEmpty }) {
            RequestCard(run: run, st: st, card: card)
        } else if !post.settled.isEmpty {
            HStack(spacing: 6) {
                Image(systemName: "checkmark.circle").foregroundStyle(.secondary).accessibilityHidden(true)
                Text(verbatim: post.settled).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
            }
        }
    }

    /// The latest delivery is the review itself; one a later delivery took up is a line.
    @ViewBuilder private var delivery: some View {
        if post.n == st.deliveries.count {
            ResultCard(run: run, st: st, width: ChatRhythm.reading)
        } else {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                RoundTag(n: post.n)
                Text(verbatim: post.text).font(.callout).foregroundStyle(.secondary).lineLimit(2)
            }
        }
    }

    private func open(_ task: String?) {
        guard let task, st.tasks[task] != nil else { return }
        run.side = .topic(task, work: false)
        run.sideShown = true
    }
}

/// A small label beside a name: 「负责人」, 「系统」, a post's kind.
struct Tag: View {
    let text: String
    var tone: Tone = .gray

    var body: some View {
        Text(text)
            .font(.system(size: 10.5, weight: .medium))
            .foregroundStyle(tone == .gray ? Color.secondary : tone.color)
            .padding(.horizontal, 5).padding(.vertical, 1)
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder((tone == .gray ? Color.secondary : tone.color).opacity(0.35)))
            .fixedSize()
    }
}
