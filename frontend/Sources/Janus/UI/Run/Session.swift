import SwiftUI

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

/// One conversation as the server works it out (`backend/src/run_view/sessions.rs`): the events it
/// shows, whether its session is open, whose turn it is, what its composer offers, its rows in
/// turns and what sits beside it (`Conversation`).
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct SessionInfo {
    var evs: [Event]
    var open: Bool
    var human: Bool
    var idle: Bool
    /// The lead's first goal is done, but its session is still open for a regular conversation;
    /// or a chat, which is always a conversation.
    var converse: Bool
    /// A chat with one digital employee: sending reopens it whenever its session is gone.
    var chat: Bool
    var composer: SessionComposer
    /// Its newest turn runs: the session works on its own or on the person's message.
    var busy: Bool
    /// The rows as served (`Conversation.rows` applies the person's clicks).
    var rows: JSON
    var plan: [Conversation.Step]
    var controls: [Conversation.Control]
    var usage: Conversation.Usage?
    /// A task's latest attempt's Git facts in one line.
    var git: String?
    /// What each section's opening event says about its session, by seq.
    var notes: [Int: Conversation.Note]
    var pending: [Conversation.Pending]
    /// Keys of its approvals and forms waiting on the person, oldest first.
    var requests: [String]

    init(_ j: JSON, events: [Event]) {
        let seqs = Set(j["events"].array.compactMap(\.int))
        evs = events.filter { seqs.contains($0.seq) }
        open = j["open"].bool ?? false
        human = j["human"].bool ?? false
        idle = j["idle"].bool ?? false
        converse = j["converse"].bool ?? false
        chat = j["chat"].bool ?? false
        composer = SessionComposer(served: j["composer"].str, idle: idle)
        busy = j["busy"].bool ?? false
        rows = j["rows"]
        plan = j["plan"].array.map { Conversation.Step(content: $0["content"].str, status: $0["status"].str) }
        controls = j["controls"].array.map(Conversation.Control.init)
        usage = j["usage"].isNull ? nil : Conversation.Usage(j["usage"])
        git = j["git"].string
        notes = Dictionary(j["notes"].object.compactMap { k, v in Int(k).map { ($0, Conversation.Note(note: v["note"].str, git: v["git"].string)) } }, uniquingKeysWith: { a, _ in a })
        pending = j["pending"].array.map { Conversation.Pending(id: $0["id"].str, text: $0["text"].str, delivery: MessageDelivery($0["delivery"].str) ?? .pending, label: $0["label"].str) }
        requests = j["requests"].strings
    }
}

extension RunState {
    /// The conversation of `target`: a task's attempts, or everything an agent did in the run (the
    /// lead's is a chat's or a Loop's one session).
    func session(_ target: SessionTarget) -> SessionInfo {
        SessionInfo(sessions[target.label] ?? .null, events: events)
    }
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

    init(served: String, idle: Bool) {
        switch served {
        case "replay": self = .replay
        case "human": self = .human(idle: idle)
        case "managed": self = .managed
        case "interrupted": self = .interrupted
        case "converse": self = .converse
        case "leave": self = .leave
        default: self = .ended
        }
    }
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
        info.requests.compactMap { k in st.inbox.first { $0.key == k } }
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
            PlanStrip(steps: info.plan)
            ChatScroll(run: run, st: st, rows: Self.rows(run: run, st: st, info: info, target: target), notes: info.notes, target: target, empty: info.evs.isEmpty, last: info.evs.last?.seq ?? 0, sent: sent, density: density, docked: docked) { id, open in
                run.chatOpen[target, default: [:]][id] = open
            }
                // Each conversation keeps its own place; another task's session opens at its newest.
                .id(target)
            PendingTray(pending: info.pending)
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
                let draft = Binding(get: { run.sessionDrafts[target] ?? "" }, set: { run.sessionDrafts[target] = $0 })
                if enabled { Suggestions(run: run, target: target, draft: draft, picker: picker) }
                Composer(text: draft, images: Binding(get: { run.sessionImages[target] ?? [] }, set: { run.sessionImages[target] = $0 }), placeholder: placeholder, disabled: !enabled, minHeight: 44, focus: target == .agent("lead") ? run.leadFocus : 0,
                         onNavigate: { picker.handle($0, draft: draft) }, ghost: run.suggestions[target].flatMap { $0.draft == draft.wrappedValue ? $0.ghost : nil }) { send(.message) }
                HStack(spacing: 10) {
                    if live && (info.open || info.converse) { SessionControls(run: run, target: target, controls: info.controls) }
                    if let u = info.usage { ContextMeter(usage: u) }
                    Spacer()
                    buttons
                }
            }
            .padding(12)
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
        // A new level applies to every turn: what the person opened or folded by hand gives way to
        // it, and the server lays the conversations out again at that level.
        .onChange(of: density) {
            run.chatOpen = [:]
            run.loadServed()
        }
    }

    private func mode(_ live: Bool) -> (String, Tone, AnyView, String, Bool) {
        let ph = L.text("发消息给 Agent · ⌘V 贴截图 · \(sendKey.symbol) 发送")
        let composer = info.composer
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

    /// The conversation in turns as the server laid it out at the display level, with what the
    /// person opened or folded by hand.
    static func rows(run: RunModel, st: RunState, info: SessionInfo, target: SessionTarget) -> [Conversation.Row] {
        Conversation.rows(info.rows, events: st.event, open: run.chatOpen[target] ?? [:])
    }
}

/// Messages sent to this session that have not reached the agent yet, newest last.
struct PendingTray: View {
    let pending: [Conversation.Pending]

    var body: some View {
        let list = pending
        if !list.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text(verbatim: L.text("待发送 \(list.count)")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                ForEach(list, id: \.id) { m in
                    HStack(spacing: 8) {
                        if m.delivery == .pending { ProgressView().controlSize(.mini) }
                        Pill(m.label, m.delivery.trayTone)
                        Text(m.text).font(.callout).lineLimit(2)
                    }
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.05))
        }
    }
}

// MARK: - Agent page
