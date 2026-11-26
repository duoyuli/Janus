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
