import SwiftUI

/// Event types a session shows.
let sessionTypes: Set<EventKind> = [.agentThought, .agentPlan, .sessionControls, .sessionControl, .sessionCommands, .sessionUsage, .attemptStarted, .sessionPrompt, .contextHandoff, .boardPosted, .taskConflict, .agentMessage, .userMessage, .messageRouted, .messageDelivered, .toolCall, .toolStarted, .toolCompleted, .questionAsked, .questionAnswered, .approvalRequested, .approvalGranted, .approvalDenied, .inputRequested, .inputAnswered, .controlTaken, .controlReleased, .sessionInterrupted, .resultSubmitted, .attemptFailed, .leadEdited, .leadRouted]

/// Janus's own events that belong in the lead's session: the run it works on and a Loop's wakes.
let leadMarks: Set<EventKind> = [.runCreated, .loopWoke, .loopSlept, .loopFailed]

func isSessionEvent(_ e: Event) -> Bool { sessionTypes.contains(e.type) || (e.type == .agentStatus && e.data["turn"].str == "idle") }

/// Who a session view is about: a task's attempts, or everything one agent did in the run. Task
/// ids and agent ids are separate names (a task may be called `lead`), so the case is part of
/// every key built from it: drafts, pasted images, folds and inbox entries.
enum SessionTarget: Hashable {
    case task(String), agent(String)
    /// How the end-to-end driver names it, as its `page` command does: `task:T1`, `agent:lead`.
    var label: String { switch self { case .task(let t): return "task:" + t; case .agent(let a): return "agent:" + a } }
    init?(label: String) {
        let p = label.split(separator: ":", maxSplits: 1).map(String.init)
        guard p.count == 2 else { return nil }
        switch p[0] {
        case "task": self = .task(p[1])
        case "agent": self = .agent(p[1])
        default: return nil
        }
    }
    var body: JSON { switch self { case .task(let t): return ["task": JSON(t), "agent": ""]; case .agent(let a): return ["task": "", "agent": JSON(a)] } }
}

struct SessionInfo {
    var evs: [Event]
    var open: Bool
    var human: Bool
    var idle: Bool
    var resume: Bool
    var leave: Bool
    /// The lead's first goal is done, but its session is still open for a regular conversation;
    /// or a chat, which is always a conversation.
    var converse: Bool
    /// A chat with one digital employee: sending reopens it whenever its session is gone.
    var chat = false
}

func taskSession(_ st: RunState, _ t: TaskState) -> SessionInfo {
    // A conflict is recorded on the task whose edit found it; the other task's session shows it too.
    SessionInfo(evs: st.events.filter { ($0.task == t.id || ($0.type == .taskConflict && $0.data["other"].str == t.id)) && isSessionEvent($0) }, open: t.live, human: t.human, idle: t.idle, resume: st.run.status.canResume, leave: false, converse: false)
}

/// What a session's composer offers. Replaying shows a past position, so nothing can be sent
/// whatever the session allowed at the time.
enum SessionComposer: Equatable {
    case replay
    /// The person has taken over the open session; `idle` when it waits for their message.
    case human(idle: Bool)
    /// The open session runs on its own; a message cuts in.
    case managed
    /// The run stopped part-way; sending resumes it.
    case interrupted
    /// The lead's goal is done and its session takes ordinary prompts (reopened first when not live).
    case converse
    /// No open session; a message waits for the agent's next one.
    case leave
    case ended

    init(_ info: SessionInfo, live: Bool, replaying: Bool) {
        if replaying { self = .replay }
        // A conversation's open session that waits for the person takes a plain message.
        else if info.open && live && info.converse && info.idle && !info.human { self = .converse }
        else if info.open && live { self = info.human ? .human(idle: info.idle) : .managed }
        else if info.resume { self = .interrupted }
        else if info.converse { self = .converse }
        else if info.leave && live { self = .leave }
        else { self = .ended }
    }
}

/// Everything one agent did in this run, and where a message to it goes right now.
func agentSession(_ st: RunState, _ id: String) -> SessionInfo {
    // The lead's session also opens with the run and, in a Loop's, marks where each wake starts and ends.
    let evs = st.events.filter { e in ((e.actor == id || e.to == id) && isSessionEvent(e)) || (id == "lead" && leadMarks.contains(e.type)) }
    let open = st.orderedTasks.contains { $0.agent == id && $0.live } || (id == "lead" && (st.run.status == .planning || st.run.status == .running))
    // The same turn rule as each task's session (`SessionTurn.after`), over this agent's events.
    let turn = evs.reduce(SessionTurn.opened) { $0.after($1) }
    let human = turn.human, idle = turn.idle
    let active = !(st.run.status.isDone || st.run.status == .cancelled)
    let converse = id == "lead" && st.run.status.isDone
    return SessionInfo(evs: evs, open: open, human: human, idle: idle, resume: st.run.status.canResume, leave: active, converse: converse)
}

/// A chat's one session: the employee's events; open while its session runs. Any message
/// reopens it otherwise, so it is never interrupted or ended for the person.
// @project-doc docs/domains/20261120-chats.md#chat_app
func chatSession(_ st: RunState, live: Bool) -> SessionInfo {
    var info = agentSession(st, "lead")
    // What the person wrote is the prompt itself: it shows once, as their message.
    info.evs.removeAll { $0.type == .sessionPrompt && PromptChannel(rawValue: $0.data["channel"].str) == .followup }
    info.open = live && (st.run.status == .running || st.run.status == .planning)
    // While its session is still starting a message waits for it, sent the plain way.
    if st.run.status == .planning { info.idle = true }
    info.resume = false
    info.leave = false
    info.converse = true
    info.chat = true
    // A chat has no takeover: stopping a turn leaves it waiting for the next message.
    info.human = false
    return info
}

/// One conversation view for every session.
// @project-doc docs/architecture/20261031-frontend.md#session_messages
struct ChatCard: View {
    let run: RunModel
    let st: RunState
    let title: String
    let info: SessionInfo
    let target: SessionTarget
    /// How much of the agent's work to show (`ChatDensity`). One choice for all conversations,
    /// remembered on this Mac.
    @AppStorage("janus.chatDensity", store: .janus) private var density = ChatDensity.summary
    private var sendKey: SendKey { SendKeySetting.shared.value }
    /// Counts the person's messages: sending one brings the conversation to its newest item.
    @Local private var sent = 0
    /// The slash commands or files offered above the composer, and the one the keys are on.
    @Local private var picker = SuggestionPicker()

    /// Requests of this session still waiting for the person, oldest first.
    private var requests: [InboxItem] {
        info.evs.filter { [.approvalRequested, .inputRequested].contains($0.type) }.compactMap { e in st.inbox.first { $0.key == e.data["key"].str } }
    }

    /// Requests sit above the composer while the person can act on them here.
    private var docked: Bool { run.canAct }

    var body: some View {
        let live = run.canAct
        let (state, tone, buttons, placeholder, enabled) = mode(live)
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                if info.open && live && !(info.human && info.idle) { QuietNote(last: info.evs.last?.ts ?? 0) }
                Picker(L.text("显示"), selection: $density) {
                    Text(L.text("详细")).tag(ChatDensity.detail)
                    Text(L.text("摘要")).tag(ChatDensity.summary)
                }
                .pickerStyle(.segmented).labelsHidden().controlSize(.small).fixedSize()
                .help(L.text("详细：每次工具调用都显示；摘要：只看对话、请求和结果"))
                Pill(state, tone)
            }
            .padding(12)
            Divider()
            PlanStrip(steps: SessionView.plan(info.evs))
            ChatScroll(run: run, st: st, rows: rows(live: live), target: target, empty: info.evs.isEmpty, last: info.evs.last?.seq ?? 0, sent: sent, density: density, docked: docked) { id, open in
                run.chatOpen[target, default: [:]][id] = open
            }
                // Each conversation keeps its own place; another task's session opens at its newest.
                .id(target)
            PendingTray(st: st, target: target)
            Divider()
            VStack(alignment: .trailing, spacing: 8) {
                // What the agent waits on sits right above where the person answers, as in Lody;
                // the conversation keeps one line where it was asked.
                if docked, let first = requests.first {
                    VStack(alignment: .leading, spacing: 4) {
                        if requests.count > 1 { Text(verbatim: L.text("1 / \(requests.count) 个请求")).font(.caption).foregroundStyle(.secondary) }
                        RequestCard(run: run, st: st, item: first)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                let commands = SessionView.commands(info.evs)
                let draft = Binding(get: { run.sessionDrafts[target] ?? "" }, set: { run.sessionDrafts[target] = $0 })
                if enabled { Suggestions(run: run, draft: draft, commands: commands, picker: picker) }
                Composer(text: draft, images: Binding(get: { run.sessionImages[target] ?? [] }, set: { run.sessionImages[target] = $0 }), placeholder: placeholder, disabled: !enabled, minHeight: 44, focus: target == .agent("lead") ? run.leadFocus : 0,
                         onNavigate: { picker.handle($0, draft: draft) }, ghost: SessionView.argumentHint(draft.wrappedValue, commands)) { send(.message) }
                HStack(spacing: 10) {
                    if live && (info.open || info.converse) { SessionControls(run: run, target: target, controls: SessionView.controls(info.evs)) }
                    if let u = SessionView.usage(info.evs) { ContextMeter(usage: u) }
                    Spacer()
                    buttons
                }
            }
            .padding(12)
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
        // A new level applies to every turn: what the person opened or folded by hand gives way to it.
        .onChange(of: density) { run.chatOpen = [:] }
    }

    private func mode(_ live: Bool) -> (String, Tone, AnyView, String, Bool) {
        let ph = L.text("发消息给 Agent · ⌘V 贴截图 · \(sendKey.symbol) 发送")
        let composer = SessionComposer(info, live: live, replaying: run.isReplaying)
        // A consolidation is one turn: once it ends there is nothing to send it.
        if run.isDream && !(info.open && live) && composer != .replay {
            return (L.text("整理结束"), .gray, AnyView(EmptyView()), L.text("整理只有一轮；要再整理，在工作区页点「现在整理」"), false)
        }
        switch composer {
        case .replay:
            return (L.text("回放中"), .gray, AnyView(EmptyView()), L.text("回放中只能看，回到实时再发消息"), false)
        case .human(idle: true):
            return (L.text("人工接管 · 等你回话"), .amber, AnyView(HStack { btn(.finish, L.text("以当前结果完成")); btn(.release, L.text("交还托管")); btn(.message, L.text("发送"), true) }), ph, true)
        case .human(idle: false):
            return (L.text("人工接管 · 正在处理"), .blue, AnyView(HStack { btn(.interrupt, L.text("打断这一轮"), danger: true); btn(.release, L.text("交还托管")); btn(.message, L.text("发送"), true) }), ph, true)
        case .managed where info.chat:
            // A chat at work: say so (a model may think for minutes before it writes anything),
            // and offer only to stop the turn or add to it.
            return (L.text("正在处理"), .blue, AnyView(HStack { btn(.interrupt, L.text("停止这一轮"), danger: true); btn(.message, L.text("插话"), true) }), L.text("补充或更正，会插入正在进行的这一轮"), true)
        case .managed:
            return (L.text("托管中 · Agent 自主推进"), .blue, AnyView(HStack { btn(.interrupt, L.text("打断并接管"), danger: true); btn(.takeover, L.text("接管")); btn(.message, L.text("插话"), true) }), ph, true)
        case .interrupted:
            return (L.text("已中断"), .gray, AnyView(HStack { Button(L.text("从中断处继续")) { Task { await run.act(.resumeRun) } }; btn(.message, L.text("发送并继续"), true) }), L.text("补充信息或直接写「继续」，发送后从中断处继续"), true)
        case .converse where info.chat:
            // A chat waits for the next message; a chat whose session is gone reopens with it.
            return (
                run.isLoop ? (info.open && live ? L.text("摸鱼中，等下次开工或你的消息") : L.text("会话已结束，下次开工时再接上"))
                    : info.open && live ? L.text("等你回话") : L.text("会话已结束"),
                .gray,
                AnyView(btn(.message, L.text("发送"), true)),
                info.open && live ? L.text("发消息给 \(title) · ⌘V 贴截图 · \(sendKey.symbol) 发送") : L.text("发送后重新打开和 \(title) 的会话"),
                true
            )
        case .converse:
            // The first goal is done; the same session now takes ordinary prompts. A slash command
            // starts a new goal and Janus follows it again. A historical run has no session yet,
            // so sending the first message reopens it.
            return (
                live ? L.text("可以继续对话") : L.text("可以继续对话"),
                .gray,
                AnyView(btn(.message, L.text("发送"), true)),
                live ? L.text("接下来做什么？也可以用 /goal 换个目标") : L.text("写下任务，发送后恢复负责人会话"),
                true
            )
        case .leave:
            return (L.text("当前没有会话"), .gray, AnyView(btn(.message, L.text("留言"), true)), L.text("留言 · 下次会话带给它"), true)
        case .ended:
            return (L.text("会话已结束"), .gray, AnyView(EmptyView()), live ? L.text("会话已结束") : L.text("只读查看"), false)
        }
    }

    /// What a composer button does to its session: send the draft, or one of the session actions.
    enum Verb { case message, interrupt, takeover, release, finish }

    private func btn(_ act: Verb, _ label: String, _ primary: Bool = false, danger: Bool = false) -> some View {
        Group {
            if primary {
                Button(label) { send(act) }.buttonStyle(.primary)
                    .disabled(draft.isEmpty && images.isEmpty).help(draft.isEmpty && images.isEmpty ? L.text("写点什么，或者贴张图") : sendKey.symbol)
            }
            // ⌘. stops the turn, as it stops a running command elsewhere on the Mac.
            else if danger { Button(label, role: .destructive) { send(act) }.keyboardShortcut(".", modifiers: .command).help("⌘.") }
            else { Button(label) { send(act) } }
        }
    }

    private var draft: String { (run.sessionDrafts[target] ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
    private var images: [PastedImage] { run.sessionImages[target] ?? [] }

    private func send(_ verb: Verb) {
        switch verb {
        case .interrupt: Task { await run.act(.interrupt(target)) }
        case .takeover: Task { await run.act(.takeover(target)) }
        case .release: Task { await run.act(.release(target)) }
        case .finish:
            guard confirm(L.text("用当前产出完成这个子任务？"), L.text("子任务里已有的改动会照常记录成 diff。"), ok: L.text("完成"), destructive: false) else { return }
            Task { await run.act(.finish(target)) }
        case .message:
            let text = draft
            let images = images
            guard !text.isEmpty || !images.isEmpty else { return }
            let message = RunAction.message(target, text: text, images: images.map(\.json))
            let key = target
            run.sessionDrafts[key] = ""
            run.sessionImages[key] = []
            sent += 1
            // A message that did not go out comes back into the composer, unless the person has
            // started another one meanwhile.
            Task {
                if await run.act(message) { return }
                if (run.sessionDrafts[key] ?? "").isEmpty && (run.sessionImages[key] ?? []).isEmpty {
                    run.sessionDrafts[key] = text
                    run.sessionImages[key] = images
                }
            }
        }
    }

    /// The conversation in turns (`ChatTurns`): the events it shows, folded as the display level
    /// and the person's own clicks say.
    private func rows(live: Bool) -> [ChatTurns.Row] {
        Self.rows(run: run, st: st, info: info, target: target, detail: density == .detail, live: live)
    }

    static func rows(run: RunModel, st: RunState, info: SessionInfo, target: SessionTarget, detail: Bool, live: Bool) -> [ChatTurns.Row] {
        // Calls that came back: Janus's board tools (`tool.completed`) and the agent's own
        // (`tool.call` with the id its running row had).
        let done = Set(info.evs.filter { $0.type == .toolCompleted || ($0.type == .toolCall && $0.data["callId"].truthy) }.map { "\($0.actor):\($0.data["callId"].str)" })
        let shown = info.evs.filter { e in
            [.toolCall, .toolStarted, .toolCompleted].contains(e.type) ? !ToolRow.hidden(e, done: done) : ChatItem.visible(e, st: st)
        }
        // The newest turn runs while the session works on its own or on the person's message.
        let busy = info.open && live && !info.idle && !run.isReplaying
        return ChatTurns.rows(shown, ChatTurns.Options(detail: detail, busy: busy, cwd: st.run.cwd, open: run.chatOpen[target] ?? [:]))
    }
}

/// How much of an agent's work a conversation shows: every tool call, or only what was said,
/// asked and delivered.
enum ChatDensity: String {
    case detail, summary
}

/// The conversation itself, newest at the bottom. While the person is at the bottom it follows what
/// arrives; once they scroll up it stays where they are, and a button takes them back to the
/// newest. Older rows are built lazily (a lead's session runs to thousands of events); the newest
/// few are always built, so the bottom edge is measured, not estimated.
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct ChatScroll: View {
    let run: RunModel
    let st: RunState
    let rows: [ChatTurns.Row]
    let target: SessionTarget
    let empty: Bool
    /// The session's last event: it changes whenever anything arrives.
    let last: Int
    /// Bumped when the person sends a message, which brings them to the newest.
    let sent: Int
    let density: ChatDensity
    /// Waiting requests are answered above the composer, not in the conversation.
    let docked: Bool
    /// The person opened or folded a turn or a group of calls (row id, open).
    let toggle: (Int, Bool) -> Void
    @Local private var follow = ChatFollow()
    @Local private var position = ScrollPosition(edge: .bottom)
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
                // The newest rows and what the agent is writing right now are always built: their
                // heights are known, not estimated, so the bottom edge is where the end really is.
                // A lazy list's estimate of a tall reply it had not built left the view short of the
                // end, or past it on nothing.
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(all[split...]) { line($0) }
                }
            }
        }
        // A margin of the scroll view, not padding around the rows, so that scrolling to the newest
        // row leaves the margin below it in view.
        .contentMargins(12, for: .scrollContent)
        .modifier(ChatProbe.Scroll(following: follow.following))
        .scrollPosition($position)
        // Opens at the newest item; later changes of content leave the place to `follow`.
        .defaultScrollAnchor(.bottom, for: .initialOffset)
        .onScrollPhaseChange { _, phase in
            let now = phase == .tracking || phase == .interacting || phase == .decelerating
            if now != person { person = now }
        }
        // Where the end is, as an offset of the content: what the scroll to the end goes to.
        .onScrollGeometryChange(for: CGFloat.self) { g in
            g.contentOffset.y + g.contentSize.height - g.visibleRect.maxY
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
    }

    /// The live preview's row, always last.
    static let liveId = Int.min / 2
    /// How many of the newest lines are always built (`body`).
    static let tail = 8

    /// One line of the list: a conversation row at its place, or the live preview after them all.
    struct Line: Identifiable {
        let index: Int
        let row: ChatTurns.Row?
        var id: Int { row?.id ?? ChatScroll.liveId }
    }

    private var lines: [Line] { rows.enumerated().map { Line(index: $0.offset, row: $0.element) } + [Line(index: rows.count, row: nil)] }

    @ViewBuilder private func line(_ l: Line) -> some View {
        if let r = l.row {
            let gap: CGFloat = l.index == 0 ? 0 : Self.gap(r, after: rows[l.index - 1])
            row(r).padding(.top, gap).modifier(ChatProbe.Row(id: r.id, order: l.index))
        } else {
            LiveSlot(run: run, target: target)
        }
    }

    /// Turns and sections stand apart; the blocks of one turn sit close.
    static func gap(_ r: ChatTurns.Row, after prev: ChatTurns.Row) -> CGFloat {
        switch r {
        case .section: return ChatRhythm.round
        case .item(let e):
            // A prompt right under the message it carries sits with it.
            if case .item(let p) = prev, ChatTurns.role(p) == .opener { return ChatRhythm.block / 2 }
            return ChatTurns.role(e) == .opener ? ChatRhythm.round : ChatRhythm.block / 2
        case .head: return ChatRhythm.block
        case .activity: return ChatRhythm.line
        case .text(_, let reply): return reply ? ChatRhythm.block : ChatRhythm.line + 4
        case .edits: return ChatRhythm.block
        }
    }

    @ViewBuilder private func row(_ r: ChatTurns.Row) -> some View {
        let cwd = st.run.cwd
        switch r {
        case .section(let e, let injected):
            VStack(alignment: .leading, spacing: 6) {
                ChatItem(run: run, st: st, e: e, docked: docked)
                ForEach(injected) { ChatItem(run: run, st: st, e: $0, docked: docked) }
            }
        case .item(let e): ChatItem(run: run, st: st, e: e, docked: docked)
        case .head(let t, let open): TurnHead(run: run, turn: t, open: open) { toggle(r.id, !(open ?? false)) }
        case .activity(_, let es, let open): ActivityRow(run: run, events: es, open: open, cwd: cwd) { toggle(r.id, !(open ?? true)) }
        case .text(let es, let reply): TextRow(events: es, reply: reply)
        case .edits(_, let files): EditsCard(files: files, cwd: cwd)
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
        if !run.isReplaying, let s = session, !(s.text.isEmpty && s.thought.isEmpty) {
            LiveRow(session: s).padding(.top, ChatRhythm.block)
        } else {
            Color.clear.frame(height: 1)
        }
    }

    private var session: ChatLive.Session? {
        switch target {
        case .task(let t): return run.preview.of(task: t, agent: nil)
        case .agent(let a): return run.preview.of(task: nil, agent: a)
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

struct ChatItem: View {
    let run: RunModel
    let st: RunState
    let e: Event
    /// Further pieces of the same agent message, streamed after `e`.
    var more: [Event] = []
    /// A waiting request is answered above the composer: here it is one line.
    var docked = false

    /// Items that render nothing (messages shown where they were routed, unknown messages).
    static func visible(_ e: Event, st: RunState) -> Bool {
        switch e.type {
        case .userMessage: return !e.data["id"].exists
        // A Loop has no goal to open its session with; each wake is marked where it starts.
        case .runCreated: return RunKind(e.data["kind"].str) != .loop
        // Shown beside the conversation (plan, controls, commands, usage), not in it.
        case .agentPlan, .sessionControls, .sessionCommands, .sessionUsage: return false
        // Stepping on another task's files says more than "both edited it": show only that.
        case .taskConflict:
            guard !e.data["owned"].truthy else { return true }
            let pair: Set<String> = [e.task ?? "", e.data["other"].str]
            let paths = Set(e.data["paths"].strings)
            return !st.events.contains { x in
                x.type == .taskConflict && x.data["owned"].truthy && [x.task ?? "", x.data["other"].str] == pair && paths.isSubset(of: Set(x.data["paths"].strings))
            }
        case .messageRouted:
            guard st.msgs[e.data["id"].str] != nil else { return false }
            return [.steer, .prompt].contains(MessageDelivery(e.data["delivery"].str))
        // A steered message is shown where it was inserted; applying it only updates that bubble.
        case .messageDelivered:
            guard e.data["ids"].exists else { return true }
            return e.data["ids"].strings.contains { st.msgs[$0].map { $0.delivery != .steer } ?? false }
        default: return true
        }
    }

    var body: some View {
        let d = e.data
        switch e.type {
        case .attemptStarted:
            VStack(alignment: .leading, spacing: 4) {
                Divider()
                Text(verbatim: L.text("\(e.task.map { "\($0) · " } ?? "")尝试 \(e.attempt ?? 0) · \(d["runtime"].str)\(d["config"]["model"].truthy ? " · \(d["config"]["model"].str)" : "")\(SessionView.attemptNote(d, task: e.task))"))
                    .font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                if let git = SessionView.gitNote(d["git"]) {
                    Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                }
                DisclosureGroup(L.text("Janus 发出的子任务说明")) { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
        case .runCreated where RunKind(d["kind"].str) == .chat:
            divider(L.text("对话 · \(d["role"]["name"].str) · \(L.dateTime(e.ts))"))
            Text(verbatim: L.text("工作目录 \(L.tilde(d["cwd"].str)) · 对话专用，删对话时一起删")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        case .runCreated where RunKind(d["kind"].str) == .dream:
            divider(L.text("整理团队记忆 · \(L.dateTime(e.ts))"))
        case .runCreated:
            divider(L.text("目标 · \(L.goalTitle(d["goal"].str))"))
            if let git = SessionView.runGitNote(d) {
                Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            StoredImages(paths: Attach.paths(d), size: 56)
        case .agentMessage:
            TextRow(events: [e] + more, reply: true)
        case .agentThought:
            ThoughtLine(text: d["text"].str)
        case .sessionControl:
            note("\(d["failed"].truthy ? "⚠" : "↺") \(e.text)", d["failed"].truthy ? .amber : nil)
        case .sessionPrompt:
            let via = d["via"].str == "developer" ? L.text("开发者指令") : L.text("系统提示")
            // The system layer holds up to two sets of instructions, named by where they come from.
            let channel = PromptChannel(rawValue: d["channel"].str)
            let label: String = switch channel {
            case .instructions: L.text("数字员工的指令 · \(via)")
            case .system: run.isLoop ? L.text("值班的工作方式 · \(via)") : run.isDream ? L.text("怎么整理团队记忆 · \(via)") : L.text("团队协作指令 · \(via)")
            case .history: L.text("之前的对话 · \(via)")
            case .goal: L.text("目标提示")
            case .followup: L.text("继续提示")
            case .fix: L.text("提醒交结果 · 这一轮结束了还没交")
            case .board: L.text("看板推送 · 插入正在进行的这一轮")
            case .goalCheck: L.text("目标确认 · 负责人停了，但目标还没完成")
            case .resume: L.text("恢复会话 · 从中断处接着推进目标")
            case nil: L.text("Janus 提示")
            }
            let hint: String? = switch channel {
            case .instructions: L.text("数字员工的指令 + Agent 的自定义提示词。一对一对话只带这一份。")
            case .system: run.isLoop ? L.text("值班守则：不亲自改文件，活儿派成任务，拿不准才问你；附带共享资料。")
                : run.isDream ? L.text("只动笔记：合并重复的、改正过时的、删掉没用的；附带全部笔记的索引。")
                : L.text("团队任务专属：分工、看板、派工和验证规则。")
            case .history: L.text("重开对话时带上最近的聊天，接着聊。")
            default: nil
            }
            // One line in the flow, as the rest of Janus's own notes: what it sent, a click from its text.
            DisclosureGroup {
                CodeBlock(text: d["prompt"].str, maxHeight: 360).padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: "→ \(label)").font(.callout.weight(.medium)).foregroundStyle(.secondary)
                    Text(hint ?? L.text("Janus 发给 Agent")).font(.caption).foregroundStyle(.tertiary).lineLimit(1)
                }
                .help(L.text("Janus 发给 Agent"))
            }
        // Goal mode: a collaborator asks the lead, which answers on the board.
        case .questionAsked: note(L.text("? 问负责人：\(d["question"].string ?? e.text)\(d["options"].array.isEmpty ? "" : L.text("（\(d["options"].strings.joined(separator: " / "))）")) · 负责人会回答，不需要你处理"), .amber)
        // The lead edited files after handing out subtasks: it only manages from then on.
        case .leadEdited: note(L.text("⚠ 派出子任务后改了文件：\(e.data["paths"].strings.joined(separator: L.text("、"))) · 交付时会单独列出"), .amber)
        // The lead called a library tool after handing out subtasks: noted like an edit.
        case .leadRouted: note(L.text("⚠ 派出子任务后调用了资源库工具 \(e.data["tool"].str) · 会写进交付风险"), .amber)
        // A handoff with a verdict is a verification's findings (for a rework), not an upstream result.
        case .contextHandoff: note(d["verdict"].exists ? L.text("⇄ 独立验证 \(d["from"].string ?? e.text) 的意见已带入本次子任务") : L.text("⇄ 上游子任务 \(d["from"].string ?? e.text) 的结果已带入本次子任务"), nil)
        case .boardPosted: note(L.text("\(d["kind"].str == "blocker" ? "⚠" : d["kind"].str == "note" ? "·" : "↗") 协作者写回看板：\(d["text"].string ?? e.text)"), nil)
        case .taskConflict where d["owned"].truthy:
            note(L.text("⚠ 越界：\(e.task ?? "") 改了 \(d["other"].str) 负责的 \(d["paths"].strings.joined(separator: L.text("、")))。已提醒 \(e.task ?? "") 不要再改，并告诉负责人"), .amber)
        case .taskConflict: note(L.text("⚠ \(e.task ?? "") 和 \(d["other"].str) 同时在改 \(d["paths"].strings.joined(separator: L.text("、")))。已提醒双方先读最新内容，并告诉负责人"), .amber)
        case .userMessage:
            let via: String = switch MessageDelivery(d["delivery"].str) {
            case .steer: L.text("插话 · 已插进这一轮")
            case .queued: L.text("排队 · 这一轮结束后发送")
            case .prompt: L.text("作为新一轮发送")
            case .next: L.text("留言 · 下次会话带给它")
            default: ""
            }
            userBubble(d["text"].str, images: Attach.paths(d), via: via, insert: false)
        case .messageRouted:
            if let m = st.msgs[d["id"].str] {
                if MessageDelivery(d["delivery"].str) == .steer { userBubble(m.text, images: m.images, via: "\(m.by == "lead" ? L.text("负责人插话") : L.text("插话")) · \(m.deliveredSeq != nil ? L.text("Agent 已读到") : L.text("插话 · 已插进这一轮，做完手头这步就会读到"))", insert: true) }
                else { userBubble(m.text, images: m.images, via: (m.by == "lead" ? L.text("负责人 · ") : "") + L.text("作为新一轮发送"), insert: false) }
            }
        case .messageDelivered:
            if d["ids"].exists {
                ForEach(d["ids"].strings.compactMap { st.msgs[$0] }.filter { $0.delivery != .steer }, id: \.id) { m in
                    userBubble(m.text, images: m.images, via: (m.by == "lead" ? L.text("负责人 · ") : "") + (m.delivery == .next ? L.text("留言 · 这次会话开始时已带上") : m.delivery == .interrupt ? L.text("插话 · 打断上一轮后立刻送达") : L.text("排队 · 这一轮结束后发送")), insert: false)
                }
            } else { note("\(L.icon[e.type] ?? "·") \(e.text)", L.tone(of: e.type)) }
        // One line in the conversation; each criterion is a click away (and on the task page).
        case .resultSubmitted:
            let checks = Check.list(d["checks"])
            let passed = checks.filter(\.passed).count
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(checks.indices, id: \.self) { i in CheckRow(c: checks[i]) }
                    if !d["evidence"].array.isEmpty { Text(verbatim: L.text("证据：\(d["evidence"].strings.joined(separator: L.text("；")))")).font(.caption).foregroundStyle(.secondary) }
                    if !d["risks"].array.isEmpty { Text(verbatim: L.text("风险：\(d["risks"].strings.joined(separator: L.text("；")))")).font(.caption).foregroundStyle(Tone.amber.color) }
                }
                .padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: L.text("⇪ 提交结果：\(d["summary"].string ?? e.text)")).font(.callout.weight(.semibold)).lineLimit(2)
                    if !checks.isEmpty { Pill(L.text("\(passed)/\(checks.count) 通过"), passed == checks.count ? .green : .amber) }
                    if !d["risks"].array.isEmpty { Pill(L.text("\(d["risks"].array.count) 个风险"), .amber) }
                }
            }
            .padding(8)
            .background(Tone.green.color.opacity(0.07), in: RoundedRectangle(cornerRadius: 8))
        case .agentStatus: note("◌ \(e.text)", .amber)
        // A Loop's wake: when it started and why, then when it ended, how long it took and when the next one is.
        case .loopWoke: divider(LoopText.woke(e))
        case .loopSlept: note("○ " + LoopText.slept(e, woke: LoopText.wake(before: e, in: st.events)), nil)
        case .loopFailed: note("⚠ \(L.dateTime(e.ts)) · \(e.text)", .amber)
        // A request still waiting is answered right here, with the same card as the inbox.
        case .approvalRequested, .inputRequested:
            if let item = st.inbox.first(where: { $0.key == d["key"].str }) {
                if docked { note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text) · 等你处理，在输入框上方"), .amber) } else { RequestCard(run: run, st: st, item: item) }
            }
            else { note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)"), L.tone(of: e.type)) }
        default: note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)"), L.tone(of: e.type))
        }
    }

    private func divider(_ t: String) -> some View {
        HStack { VStack { Divider() }; Text(t).font(.caption).foregroundStyle(.secondary).lineLimit(1); VStack { Divider() } }
    }

    private func note(_ t: String, _ tone: Tone?) -> some View {
        Text(t).font(.callout).foregroundStyle(tone?.color ?? .secondary).textSelection(.enabled)
            .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
    }

    private func userBubble(_ text: String, images: [String] = [], via: String, insert: Bool) -> some View {
        let words = images.isEmpty ? text : Attach.visibleText(text)
        return HStack {
            Spacer(minLength: 60)
            VStack(alignment: .trailing, spacing: 4) {
                if insert { Text("↪ " + via).font(.caption.weight(.semibold)).foregroundStyle(Color.accent) }
                StoredImages(paths: images)
                if !words.isEmpty { Text(words).textSelection(.enabled) }
                if !insert && !via.isEmpty { Text(via).font(.caption).foregroundStyle(.secondary) }
            }
            .padding(10)
            .background(insert ? Color.accent.opacity(0.06) : Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(insert ? Color.accent.opacity(0.4) : .clear, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        }
    }
}

struct CheckRow: View {
    let c: Check
    var body: some View {
        // The method is shown as text here too; the conversation only shows it, the task page
        // groups it and the delivery repeats it per task.
        CheckLine(n: nil, criterion: c.criterion, check: c, dense: true)
    }
}

/// One row per tool call, readable at a glance, with the full input and output a click away.
/// Janus's board tools are recorded twice (by Janus and by the agent): only Janus's record is
/// shown, and a call that came back replaces its "waiting" row.
struct ToolRow: View {
    let run: RunModel
    let e: Event
    /// The run's directory, so paths inside it read as relative ones.
    let cwd: String

    static func hidden(_ e: Event, done: Set<String>) -> Bool {
        let d = e.data
        // Codex names them `mcp.janus.*`, Claude Code `mcp__janus__*`; Janus's own record is the one shown.
        let mcp = { (s: String) in s.hasPrefix("mcp.janus.") || s.hasPrefix("mcp__janus__") }
        let echo = [.toolCall, .toolStarted].contains(e.type) && !d["tool"].str.hasPrefix("janus.") && (mcp(e.text) || mcp(d["tool"].str) || d["input"].str.hasPrefix("{\"arguments\""))
        return echo || (e.type == .toolStarted && done.contains("\(e.actor):\(d["callId"].str)"))
    }

    var body: some View {
        let d = e.data
        let board = d["tool"].str.hasPrefix("janus.")
        let running = e.type == .toolStarted
        // A call still running shows its newest frame: a title the agent filled in later, its output so far.
        let live = running ? run.preview.tool(actor: e.actor, task: e.task, id: d["callId"].str) : nil
        let output = L.inWorkspace(live?.output ?? d["output"].str, cwd)
        let input = live.map { $0.input.isEmpty ? $0.title : $0.input } ?? (d["input"].truthy ? d["input"].str : e.text)
        let failed = d["status"].str == "failed"
        // The first line that says something: not a Markdown fence around the output.
        let firstLine = output.split(separator: "\n").map(String.init).first { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("```") && !$0.trimmingCharacters(in: .whitespaces).isEmpty } ?? ""
        let full = board ? d["args"].or(d["input"]).pretty() : input
        VStack(alignment: .leading, spacing: 4) {
            DisclosureGroup {
                CodeBlock(text: full + (output.isEmpty ? "" : "\n\n→ \(output)"), maxHeight: 300)
                ForEach(Array(d["diffs"].array.enumerated()), id: \.offset) { _, x in
                    InlineDiff(path: L.tilde(x["path"].str), old: x["old"].str, new: x["new"].str)
                }
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        if board {
                            Text(run.isLoop ? L.text("值班") : L.text("看板")).font(.system(size: 11, design: .monospaced)).padding(.horizontal, 4).background(Color.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 3))
                            Text(janusLabel(d["tool"].str, d["args"], output).0).font(.callout).lineLimit(1).truncationMode(.tail)
                        } else {
                            let (verb, what, mono) = Self.label(kind: live?.tool ?? d["tool"].str, input: input, title: live?.title ?? e.text, running: running, cwd: cwd)
                            if running {
                                Shimmer(text: [verb, what].filter { !$0.isEmpty }.joined(separator: " "))
                            } else {
                                if !verb.isEmpty { Text(verb).font(.callout).foregroundStyle(failed ? Tone.red.color : .secondary) }
                                Text(verbatim: what).font(mono ? .system(.callout, design: .monospaced) : .callout)
                                    .foregroundStyle(failed ? Tone.red.color : .primary).lineLimit(1).truncationMode(.tail)
                            }
                        }
                        if running && !d["live"].truthy { Pill(L.text("等待返回"), .blue) }
                        if failed { Pill(L.text("失败"), .red) }
                        if !d["diffs"].array.isEmpty { Text(verbatim: L.text("改了 \(d["diffs"].array.count) 个文件")).font(.caption).foregroundStyle(.secondary) }
                    }
                    .help(input)
                    if !running, !firstLine.isEmpty { Text("→ " + L.short(board ? janusLabel(d["tool"].str, d["args"], output).1 : firstLine, 160)).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
            }
            // A running command's output so far: its last lines, as its own terminal ends.
            if running, !output.isEmpty {
                let (tail, cut) = ChatTurns.tail(output)
                VStack(alignment: .leading, spacing: 2) {
                    if cut { Text(L.text("更早的输出没有显示")).font(.caption2).foregroundStyle(.tertiary) }
                    Text(verbatim: tail).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).lineLimit(16)
                }
                .padding(8)
                .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
                .background(Color.soft, in: RoundedRectangle(cornerRadius: 6))
                .textSelection(.enabled)
            }
        }
    }

    /// A call in one line: a verb for its kind and what it worked on, the command itself for a
    /// command and the file's name for a file. Unknown kinds read as their own title.
    static func label(kind: String, input: String, title: String, running: Bool, cwd: String) -> (verb: String, what: String, mono: Bool) {
        let verb = ChatTurns.verb(kind, running: running)
        let path = L.inWorkspace(input, cwd)
        switch kind {
        case "execute": return (verb, input.components(separatedBy: "\n").first ?? input, true)
        case "read", "edit", "delete", "move":
            // A plain path names the file; anything else (a range, a pattern) reads as the agent titled it.
            if !path.contains(" "), !path.isEmpty { return (verb, (path as NSString).lastPathComponent, true) }
            return (verb, title, false)
        case "search", "fetch": return (verb, input.isEmpty ? title : input, false)
        case "think": return (verb, "", false)
        default: return ("", title.isEmpty ? input : title, false)
        }
    }

    private func janusLabel(_ tool: String, _ a: JSON, _ out: String) -> (String, String) {
        let first = out.split(separator: "\n").first.map(String.init) ?? ""
        switch JanusTool(call: tool) {
        case .orchestrate: return (L.text("派出子任务：\(a["tasks"].array.map { "\($0["id"].str) \($0["title"].str) → \(run.who($0["assignee"].str).name)" }.joined(separator: L.text("；")))"), first)
        case .wait: return (L.text("等待看板上的进展"), first)
        case .reply: return (L.text("回复 \(a["task"].str)：\(a["text"].str)"), first)
        case .retry: return (L.text("重新执行 \(a["task"].str)\(a["instructions"].truthy ? L.text("（改了说明）") : "")\(a["accept"].truthy ? L.text("（改了验收标准）") : "")"), first)
        case .amend: return (L.text("修订 \(a["task"].str)：\(a["text"].str)"), first)
        case .cancel: return (L.text("取消 \(a["tasks"].strings.joined(separator: L.text("、")))\(a["reason"].truthy ? L.text("：\(a["reason"].str)") : "")"), first)
        case .askUser: return (L.text("等你拍板：\(a["question"].str)"), first)
        case .board: return (a["task"].truthy ? L.text("查看 \(a["task"].str) 的全部信息") : L.text("查看团队和子任务状态"), "")
        case .boardPost: return (L.text("写回看板：\(a["text"].str)"), "")
        default: return (tool, first)
        }
    }
}

/// Messages sent to this session that have not reached the agent yet, newest last.
struct PendingTray: View {
    let st: RunState
    let target: SessionTarget

    var body: some View {
        let list = pending
        if !list.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text(verbatim: L.text("待发送 \(list.count)")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                ForEach(list, id: \.id) { m in
                    let (l, tone) = m.delivery.trayLabel ?? ("", .gray)
                    HStack(spacing: 8) {
                        if m.delivery == .pending { ProgressView().controlSize(.mini) }
                        Pill(l, tone)
                        Text(m.text).font(.callout).lineLimit(2)
                    }
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.05))
        }
    }

    private var pending: [Msg] {
        st.msgs.values.filter { m in
            guard m.deliveredSeq == nil, m.delivery.trayLabel != nil else { return false }
            switch target {
            case .task(let t):
                let agent = st.tasks[t]?.agent
                return (m.task ?? m.routedTask) == t || (m.task == nil && m.routedTask == nil && m.to == agent)
            case .agent(let a): return m.to == a
            }
        }
        .sorted { $0.seq < $1.seq }
    }
}

// MARK: - Agent page

struct AgentView: View {
    let run: RunModel
    let agentId: String

    var body: some View {
        let st = run.shown
        let w = run.who(agentId)
        let tasks = st.orderedTasks.filter { $0.agent == agentId }
        // A page of its own that scrolls, like the task page: content taller than the window
        // must not push the window's layout around.
        ScrollView {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Avatar(initials: w.initials, color: w.color, size: 34, role: w.role)
                VStack(alignment: .leading, spacing: 2) {
                    Text(w.name).font(.display(.title2))
                    Text([w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).foregroundStyle(.secondary)
                }
                Spacer()
            }
            AdaptiveColumns(side: 300) {
                // A fixed height like the task page: the conversation scrolls inside its card,
                // not the page around it.
                ChatCard(run: run, st: st, title: L.text("对话"), info: agentSession(st, agentId), target: .agent(agentId))
                    .frame(height: 640)
                Card(padding: 12) {
                    HStack { Text(L.text("负责的子任务")).font(.headline); Spacer(); Text(verbatim: L.text("\(tasks.count) 个")).font(.caption).foregroundStyle(.secondary) }
                    if tasks.isEmpty { Text(agentId == "lead" ? L.text("负责人管规划和协调。") : L.text("还没有分配子任务。")).font(.callout).foregroundStyle(.secondary) }
                    ForEach(tasks, id: \.id) { t in
                        let (l, tone) = t.status.label
                        HStack {
                            Button(String("\(t.id) \(t.title)")) { run.show(.task(t.id)) }.buttonStyle(.link).lineLimit(2)
                            Spacer()
                            Pill(l, tone)
                        }
                    }
                }
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }
}
