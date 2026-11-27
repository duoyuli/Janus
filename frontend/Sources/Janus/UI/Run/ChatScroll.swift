import SwiftUI

/// How much of an agent's work a conversation shows: every tool call, or only what was said,
/// asked and delivered. One choice for all conversations, remembered on this Mac; the server lays
/// the conversations out at it.
enum ChatDensity: String {
    case detail, summary
    static let key = "janus.chatDensity"
    static var current: ChatDensity { UserDefaults.janus.string(forKey: key).flatMap(ChatDensity.init(rawValue:)) ?? .summary }
}

/// The conversation itself, newest at the bottom. While the person is at the bottom it follows what
/// arrives; once they scroll up it stays where they are, and a button takes them back to the
/// newest. Older rows are built lazily (a lead's session runs to thousands of events); the newest
/// few are always built, so the bottom edge is measured, not estimated.
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct ChatScroll: View {
    let run: RunModel
    let st: RunState
    let rows: [Conversation.Row]
    let target: SessionTarget
    let empty: Bool
    /// The session's last event: it changes whenever anything arrives.
    let last: Int
    /// Bumped when the person sends a message, which brings them to the newest.
    let sent: Int
    let density: ChatDensity
    /// Waiting requests are answered above the composer, not in the conversation.
    let docked: Bool
    /// A row to bring into view, once per new value.
    var jump: RunModel.Jump?
    /// The person opened or folded a turn or a group of calls (row id, open).
    let toggle: (Int, Bool) -> Void
    @Local private var follow = ChatFollow()
    @Local private var position = ScrollPosition(idType: Int.self, edge: .bottom)
    /// Flips with each scroll to the end, so the next one is a new position (`toNewest`).
    @Local private var nudge = false
    /// The content offset that shows the end, from the last measurement.
    @Local private var endOffset: CGFloat = 0
    /// The person is scrolling: dragging, swiping, or the swipe running on.
    @Local private var person = false

    var body: some View {
        let all = lines
        let split = max(0, all.count - Self.tail)
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                // Older rows are built lazily: a lead's session runs to thousands of events.
                LazyVStack(alignment: .leading, spacing: 0) {
                    if empty { Text(L.text("开跑后，对话和工具调用会实时出现在这里")).foregroundStyle(.secondary) }
                    ForEach(all[..<split]) { line($0) }
                }
                // Each row is a target a jump (「@你」) can go to by its id.
                .scrollTargetLayout()
                // The newest rows and what the agent is writing right now are always built: their
                // heights are known, not estimated, so the bottom edge is where the end really is.
                // A lazy list's estimate of a tall reply it had not built left the view short of the
                // end, or past it on nothing.
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(all[split...]) { line($0) }
                }
                .scrollTargetLayout()
            }
        }
        // A margin of the scroll view, not padding around the rows, so that scrolling to the newest
        // row leaves the margin below it in view.
        .contentMargins(12, for: .scrollContent)
        .modifier(ChatProbe.Scroll(following: follow.following))
        .scrollPosition($position)
        // Opens at the newest item. While following, content that grows or shrinks keeps the
        // bottom where it is, so a streaming reply needs no scroll of its own; scrolled up, the
        // person's place stays put.
        .defaultScrollAnchor(.bottom, for: .initialOffset)
        .defaultScrollAnchor(follow.following ? .bottom : nil, for: .sizeChanges)
        .onScrollPhaseChange { _, phase in
            let now = phase == .tracking || phase == .interacting || phase == .decelerating
            if now != person { person = now }
        }
        // Where the end is, as an offset of the content: what the scroll to the end goes to. The
        // largest offset there is, margin included, which is where the bottom edge puts the view
        // too: two ends a few points apart had the view bounce between them as a reply streamed.
        .onScrollGeometryChange(for: CGFloat.self) { g in
            g.contentSize.height + g.contentInsets.bottom - g.containerSize.height
        } action: { _, end in endOffset = end }
        // How much content lies below the visible area, reported on every scroll and every change
        // of the content's size; the state changes only when the answer does.
        .onScrollGeometryChange(for: CGFloat.self) { g in
            g.contentSize.height - g.visibleRect.maxY
        } action: { _, below in
            var again = false
            update { again = $0.measured(below: below, last: last, person: person) }
            // To the newest row again, on the next turn of the main loop.
            if again { DispatchQueue.main.async { toNewest() } }
        }
        // A new reply while following: go to it at once; the measurements above finish the landing.
        .onChange(of: last) { if follow.following && !person { toEnd() } }
        .onChange(of: sent) { toEnd() }
        .onChange(of: density) { if follow.following { toEnd() } }
        // A scroll that stopped short of the end moves nothing more, so nothing above asks again:
        // look once more a moment later while following.
        .task(id: follow) {
            guard follow.following, !follow.landed, !person else { return }
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard !Task.isCancelled, !person else { return }
            var again = false
            update { again = $0.retry() }
            if again { toNewest() }
        }
        .overlay(alignment: .bottom) { if !follow.following { back } }
        // Taken to a row further up (「@你」): the person reads there, so the view lets go of the end.
        .onChange(of: jump) {
            guard let jump else { return }
            update { $0.left() }
            position.scrollTo(id: jump.row, anchor: .top)
        }
    }

    /// The live preview's row, always last.
    static let liveId = Int.min / 2
    /// How many of the newest lines are always built (`body`): more than a screen of them, so no
    /// lazily estimated row is in view at the end. With fewer, macOS 15 measured the rows in view
    /// after it last reported the content's size, and the view stopped about a screen short of the
    /// end, sure it was there.
    static let tail = 30

    /// One line of the list: a conversation row at its place, or the live preview after them all.
    struct Line: Identifiable {
        let index: Int
        let row: Conversation.Row?
        var id: Int { row?.id ?? ChatScroll.liveId }
    }

    private var lines: [Line] { rows.enumerated().map { Line(index: $0.offset, row: $0.element) } + [Line(index: rows.count, row: nil)] }

    @ViewBuilder private func line(_ l: Line) -> some View {
        if let r = l.row {
            let gap: CGFloat = l.index == 0 ? 0 : Self.points(r.gap)
            row(r).padding(.top, gap).modifier(ChatProbe.Row(id: r.id, order: l.index))
        } else {
            LiveSlot(run: run, target: target)
        }
    }

    /// The space the server puts above a row (turns and sections stand apart, the blocks of one
    /// turn sit close), in points.
    static func points(_ gap: Conversation.Gap) -> CGFloat {
        switch gap {
        case .none: return 0
        case .round: return ChatRhythm.round
        case .block: return ChatRhythm.block
        case .half: return ChatRhythm.block / 2
        case .line: return ChatRhythm.line
        case .loose: return ChatRhythm.line + 4
        }
    }

    @ViewBuilder private func row(_ r: Conversation.Row) -> some View {
        let cwd = st.run.cwd
        switch r {
        case .section(let e, let injected, _):
            VStack(alignment: .leading, spacing: 6) {
                ChatItem(run: run, st: st, e: e, docked: docked)
                ForEach(injected) { ChatItem(run: run, st: st, e: $0, docked: docked) }
            }
        case .item(let e, _): ChatItem(run: run, st: st, e: e, docked: docked)
        case .head(_, let t, let open, _): TurnHead(run: run, turn: t, open: open) { toggle(r.id, !(open ?? false)) }
        case .activity(_, let es, let open, let summary, _): ActivityRow(run: run, events: es, open: open, summary: summary, st: st) { toggle(r.id, !(open ?? true)) }
        case .text(_, let es, let reply, _): TextRow(events: es, reply: reply)
        case .edits(_, let files, _): EditsCard(files: files, diffs: st.diffs, cwd: cwd)
        case .post(_, let p, _): PostRow(run: run, st: st, post: p)
        }
    }

    /// Back to the newest item, following again.
    private var back: some View {
        Button { toEnd() } label: {
            Label(follow.unseen ? L.text("有新消息") : L.text("回到最新"), systemImage: "arrow.down")
                .font(.caption.weight(.medium))
                .foregroundStyle(follow.unseen ? Color.accent : Color.secondary)
                .padding(.horizontal, 10).padding(.vertical, 5)
                .background(Color.card, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.line))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .padding(.bottom, 10)
        .help(L.text("回到最新的消息，之后的新消息会继续显示在底部"))
        .accessibilityLabel(follow.unseen ? L.text("有新消息，回到最新") : L.text("回到最新"))
    }

    private func toEnd() {
        update { $0.jumped() }
        toNewest()
    }

    /// To the end: the bottom edge. The newest rows are always built (`tail`), so the edge is
    /// measured, not estimated; scrolling to a row of a lazy list by its id landed where its
    /// estimate put it, short of the end, and the same position again did nothing.
    private func toNewest() {
        // The same position again does nothing: alternate two ways of saying the end, so a try
        // after rows above were measured (which moves the end) is a scroll too. The offset is the
        // measured end, never an out-of-range number: macOS 15 does not clamp one to the end.
        nudge.toggle()
        if nudge { position.scrollTo(edge: .bottom) } else { position.scrollTo(y: max(0, endOffset)) }
    }

    private func update(_ change: (inout ChatFollow) -> Void) {
        var f = follow
        change(&f)
        if f != follow { follow = f }
    }
}

/// The newest row of a conversation: what its session is writing or thinking right now. Reads
/// the run's live previews itself, so a frame redraws this row and nothing else.
struct LiveSlot: View {
    let run: RunModel
    let target: SessionTarget

    var body: some View {
        if !run.isReplaying, let s = run.preview[target.label], !(s.text.isEmpty && s.thought.isEmpty) {
            LiveRow(session: s).padding(.top, ChatRhythm.block)
        } else {
            Color.clear.frame(height: 1)
        }
    }
}

/// Whether a conversation follows what arrives: only while its end is in view. The person's own
/// scrolling lets go of the end or takes it up again, and so does a way to the end that never
/// lands (`landingTries`, `landingTime`); scrolled up, the person keeps their place, and what
/// arrived meanwhile is noted for the way back.
struct ChatFollow: Equatable {
    /// The newest item is in view, so what moves the end is followed.
    var following = true
    /// Something arrived while the person was reading further up.
    var unseen = false
    /// Scrolls to the end made since the view set out for it. A lazy list only estimates the rows
    /// it has not built, and on macOS 15 SwiftUI moves the offset while it measures them, so the end
    /// moves while the view goes there.
    var landing = 0
    /// When the view set out for the end (seconds), while it has not got there.
    var landingSince: Double?
    /// The session's newest event at the last measurement.
    var seen = 0
    /// The last measurement found the end in view.
    var atEnd = true
    /// How far the content went on below the view at the last measurement.
    var below: CGFloat = 0
    /// How close to the bottom still counts as being there.
    static let slack: CGFloat = 32
    /// Giving up takes both this many scrolls and `landingTime`: a busy Mac measures a tall row
    /// (a long reply with a table) over many more passes than an idle one, and a count alone ran out
    /// before the row had its height, leaving the view short of the end.
    static let landingTries = 8
    static let landingTime: Double = 3

    /// `below` points of content lie under the visible area, the session's newest event is
    /// `last`, and `person` says whether the person is scrolling. True when the view should
    /// scroll to the end.
    mutating func measured(below: CGFloat, last: Int, person: Bool, now: Double = Date.timeIntervalSinceReferenceDate) -> Bool {
        let atEnd = below <= Self.slack
        let new = last != seen
        self.atEnd = atEnd
        self.below = below
        seen = last
        if person {
            following = atEnd
            if atEnd { unseen = false } else if new { unseen = true }
            landing = 0
            landingSince = nil
            return false
        }
        if !following {
            if new { unseen = true }
            return false
        }
        // Following, and not the person: new content, rows measured or the offset adjusted moved
        // the end. Go after it until it is reached, or for a few scrolls and seconds at most. The
        // slack says whether the person is at the end; going there, the view goes all the way.
        if landed { landing = 0; landingSince = nil; return false }
        return land(now: now)
    }

    /// At the very end, not only within the slack.
    var landed: Bool { below <= 1 }

    /// Still short of the end a moment after the last scroll, with nothing moved since (a scroll
    /// that stopped short reports no further change): true to scroll again.
    mutating func retry(now: Double = Date.timeIntervalSinceReferenceDate) -> Bool {
        guard following, !landed else { return false }
        return land(now: now)
    }

    private mutating func land(now: Double) -> Bool {
        let since = landingSince ?? now
        landingSince = since
        landing += 1
        // Never got there: let go, and the button takes the person back.
        if landing > Self.landingTries && now - since > Self.landingTime {
            following = false
            landing = 0
            landingSince = nil
            return false
        }
        return true
    }

    /// The view is taken to a row further up: it no longer follows the end.
    mutating func left() {
        following = false
        landing = 0
        landingSince = nil
    }

    /// The view is sent to the newest item: sending a message, the button, a new display level.
    mutating func jumped() {
        following = true
        unseen = false
        landing = 0
        landingSince = nil
    }
}

/// "N 秒没有新输出": while an agent works on its own, how long since it did something visible.
struct QuietNote: View {
    let last: Double
    var body: some View {
        TimelineView(.periodic(from: .now, by: 5)) { ctx in
            let s = Int((ctx.date.timeIntervalSince1970 * 1000 - last) / 1000)
            if last > 0 && s >= 20 {
                Text(verbatim: L.text("\(s < 120 ? L.text("\(s) 秒") : L.text("\(s / 60) 分钟"))没有新输出")).font(.caption).foregroundStyle(.secondary)
                    .help(L.text("Agent 在深度思考或写大段内容，暂时没有新输出"))
            }
        }
    }
}
