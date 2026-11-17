import SwiftUI

/// Event types a session shows.
let sessionTypes: Set<String> = ["agent.thought", "agent.plan", "session.controls", "session.control", "session.commands", "session.usage", "attempt.started", "session.prompt", "context.handoff", "board.posted", "task.conflict", "agent.message", "user.message", "message.routed", "message.delivered", "tool.call", "tool.started", "tool.completed", "question.asked", "question.answered", "approval.requested", "approval.granted", "approval.denied", "input.requested", "input.answered", "control.taken", "control.released", "session.interrupted", "result.submitted", "attempt.failed"]

func isSessionEvent(_ e: Event) -> Bool { sessionTypes.contains(e.type) || (e.type == "agent.status" && e.data["turn"].str == "idle") }

/// Who a session view is about: a task's attempts, or everything one agent did in the run.
enum SessionTarget: Hashable {
    case task(String), agent(String)
    var key: String { switch self { case .task(let t): return "sess-" + t; case .agent(let a): return "sess-" + a } }
    var body: JSON { switch self { case .task(let t): return ["task": JSON(t), "agent": ""]; case .agent(let a): return ["task": "", "agent": JSON(a)] } }
}

struct SessionInfo {
    var evs: [Event]
    var open: Bool
    var human: Bool
    var idle: Bool
    var resume: Bool
    var leave: Bool
    /// The lead's first goal is done, but its session is still open for a regular conversation.
    var converse: Bool
}

func taskSession(_ st: RunState, _ t: TaskState) -> SessionInfo {
    // A conflict is recorded on the task whose edit found it; the other task's session shows it too.
    SessionInfo(evs: st.events.filter { ($0.task == t.id || ($0.type == "task.conflict" && $0.data["other"].str == t.id)) && isSessionEvent($0) }, open: t.live, human: t.human, idle: t.idle, resume: st.run.status.canResume, leave: false, converse: false)
}

/// Everything one agent did in this run, and where a message to it goes right now.
func agentSession(_ st: RunState, _ id: String) -> SessionInfo {
    let evs = st.events.filter { e in ((e.actor == id || e.to == id) && isSessionEvent(e)) || (id == "lead" && e.type == "run.created") }
    let open = st.orderedTasks.contains { $0.agent == id && $0.live } || (id == "lead" && (st.run.status == .planning || st.run.status == .running))
    var human = false, idle = false
    for e in evs {
        if e.type == "control.taken" || (e.type == "session.interrupted" && !e.data["finish"].truthy) { human = true }
        if ["control.released", "attempt.started", "run.created"].contains(e.type) { human = false }
        if e.type == "agent.status" && e.data["turn"].str == "idle" { idle = true }
        if ((e.type == "user.message" || e.type == "message.routed") && e.data["delivery"].str == "prompt") || ["control.released", "message.delivered", "attempt.started"].contains(e.type) { idle = false }
    }
    let active = !(st.run.status.isDone || st.run.status == .cancelled)
    let converse = id == "lead" && st.run.status.isDone
    return SessionInfo(evs: evs, open: open, human: human, idle: idle, resume: st.run.status.canResume, leave: active, converse: converse)
}

/// One conversation view for every session.
// @project-doc docs/architecture/20261031-frontend.md#session_messages
struct ChatCard: View {
    let run: RunModel
    let st: RunState
    let title: String
    let info: SessionInfo
    let target: SessionTarget
    /// How much of the agent's work to show: every tool call (`detail`), long runs of them folded
    /// (`normal`), or only what was said, asked and delivered (`summary`). One choice for all
    /// conversations, remembered on this Mac.
    @AppStorage("janus.chatDensity") private var density = "normal"
    /// Counts the person's messages: sending one brings the conversation to its newest item.
    @Local private var sent = 0

    var body: some View {
        let live = run.canAct
        let (state, tone, buttons, placeholder, enabled) = mode(live)
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                if info.open && live && !(info.human && info.idle) { QuietNote(last: info.evs.last?.ts ?? 0) }
                if let u = SessionView.usage(info.evs) {
                    // This session's own numbers, labelled: `本会话 上下文 26% · 上一轮 1,500 tokens`.
                    // They stay apart from the run's cumulative total on the run card.
                    Text(verbatim: u.labels ?? u.percent.map { "上下文 \($0)%" } ?? "\(SessionView.tokens(u.tokens ?? 0)) tokens")
                        .font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
                        .help([u.used.map { "上下文已用 \(SessionView.tokens($0))" }, u.size.map { "共 \(SessionView.tokens($0))" }, u.tokens.map { "上一轮 \(SessionView.tokens($0)) tokens" }].compactMap { $0 }.joined(separator: " · ") + "\n本会话的数字不计入任务累计用量")
                }
                Picker("显示", selection: $density) {
                    Text("详细").tag("detail")
                    Text("普通").tag("normal")
                    Text("摘要").tag("summary")
                }
                .pickerStyle(.segmented).labelsHidden().controlSize(.small).fixedSize()
                .help("详细：每次工具调用都显示；普通：连续的工具调用折叠；摘要：只看对话、请求和结果")
                Pill(state, tone)
            }
            .padding(12)
            Divider()
            PlanStrip(steps: SessionView.plan(info.evs))
            ChatScroll(run: run, st: st, groups: groups, density: density, empty: info.evs.isEmpty, last: info.evs.last?.seq ?? 0, sent: sent)
                // Each conversation keeps its own place; another task's session opens at its newest.
                .id(target)
            PendingTray(st: st, target: target)
            Divider()
            VStack(alignment: .trailing, spacing: 8) {
                if enabled { Suggestions(run: run, draft: Binding(get: { run.sessionDrafts[target.key] ?? "" }, set: { run.sessionDrafts[target.key] = $0 }), commands: SessionView.commands(info.evs)) }
                Composer(text: Binding(get: { run.sessionDrafts[target.key] ?? "" }, set: { run.sessionDrafts[target.key] = $0 }), images: Binding(get: { run.sessionImages[target.key] ?? [] }, set: { run.sessionImages[target.key] = $0 }), placeholder: placeholder, disabled: !enabled, minHeight: 44) { send("message") }
                HStack {
                    if live && (info.open || info.converse) { SessionControls(run: run, target: target, controls: SessionView.controls(info.evs)) }
                    Spacer()
                    buttons
                }
            }
            .padding(12)
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
    }

    private func mode(_ live: Bool) -> (String, Tone, AnyView, String, Bool) {
        let ph = "给 Agent 发消息，可以 ⌘V 粘贴截图，⌘↩ 发送"
        if info.open && live {
            if info.human {
                if info.idle {
                    return ("人工接管 · 等你的消息", .amber, AnyView(HStack { btn("finish", "以当前结果完成"); btn("release", "交还托管"); btn("sessSend", "发送", true) }), ph, true)
                }
                return ("人工接管 · 正在处理", .blue, AnyView(HStack { btn("interrupt", "打断这一轮", danger: true); btn("release", "交还托管"); btn("sessSend", "发送", true) }), ph, true)
            }
            return ("托管中 · 自主执行", .blue, AnyView(HStack { btn("interrupt", "打断并接管", danger: true); btn("takeover", "接管"); btn("sessSend", "插话", true) }), ph, true)
        }
        if info.resume {
            return ("已中断", .gray, AnyView(HStack { Button("从中断处继续") { Task { await run.act(["type": "resumeRun"]) } }; btn("sessSend", "发送并继续", true) }), "补充信息或直接写「继续」，发送后从中断处恢复", true)
        }
        if info.converse {
            // The first goal is done; the same session now takes ordinary prompts. A slash command
            // starts a new goal and Janus follows it again. A historical run has no session yet,
            // so sending the first message reopens it.
            return (
                live ? "可以继续对话" : "可以恢复对话",
                .gray,
                AnyView(btn("sessSend", "发送", true)),
                live ? "写下接下来要做的，或用 /goal 设置新的目标" : "写下任务，发送后恢复负责人会话",
                true
            )
        }
        if info.leave && live {
            return ("当前没有会话", .gray, AnyView(btn("sessSend", "留言", true)), "留言会在它下一次会话开始时带给它", true)
        }
        return ("会话已结束", .gray, AnyView(EmptyView()), live ? "这个会话已经结束" : "只读查看", false)
    }

    private func btn(_ act: String, _ label: String, _ primary: Bool = false, danger: Bool = false) -> some View {
        Group {
            if primary {
                Button(label) { send(act == "sessSend" ? "message" : act) }.buttonStyle(.primary)
                    .disabled(draft.isEmpty && images.isEmpty).help(draft.isEmpty && images.isEmpty ? "先写下要发送的内容，或者粘贴图片" : "⌘↩")
            }
            // ⌘. stops the turn, as it stops a running command elsewhere on the Mac.
            else if danger { Button(label, role: .destructive) { send(act) }.keyboardShortcut(".", modifiers: .command).help("⌘.") }
            else { Button(label) { send(act) } }
        }
    }

    private var draft: String { (run.sessionDrafts[target.key] ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
    private var images: [PastedImage] { run.sessionImages[target.key] ?? [] }

    private func send(_ type: String) {
        if type == "finish" && !confirm("用当前产出完成这个子任务？", "子任务里已有的改动会照常记录成 diff。", ok: "完成", destructive: false) { return }
        var body = target.body
        body["type"] = JSON(type)
        if type == "message" {
            let text = draft
            let images = images
            guard !text.isEmpty || !images.isEmpty else { return }
            body["text"] = JSON(text)
            if !images.isEmpty { body["images"] = .array(images.map(\.json)) }
            run.sessionDrafts[target.key] = ""
            run.sessionImages[target.key] = []
            sent += 1
        }
        Task { await run.act(body) }
    }

    // Long runs of tool calls fold into one line, keeping the latest few in view.
    struct Group1: Identifiable { enum Kind { case item(Event), messages([Event]), tools([Event]) }; let id: Int; let kind: Kind }

    private var groups: [Group1] {
        var out: [Group1] = []
        var run: [Event] = []
        // Calls that came back: Janus's board tools (`tool.completed`) and the agent's own
        // (`tool.call` with the id its running row had).
        let done = Set(info.evs.filter { $0.type == "tool.completed" || ($0.type == "tool.call" && $0.data["callId"].truthy) }.map { "\($0.actor):\($0.data["callId"].str)" })
        func flush() { if let f = run.first { out.append(Group1(id: -f.seq, kind: .tools(run))) }; run = [] }
        for e in info.evs {
            if ["tool.call", "tool.started", "tool.completed"].contains(e.type) {
                if ToolRow.hidden(e, done: done) { continue }
                run.append(e)
            } else {
                if !ChatItem.visible(e, st: st) { continue }
                // The summary keeps what was said, asked and delivered; Janus's own prompts,
                // status lines and the agent's thinking stay in the other views.
                if density == "summary" && ["session.prompt", "agent.status", "agent.thought"].contains(e.type) { continue }
                flush()
                // Text streams in pieces; pieces in a row from one agent read as one message.
                if e.type == "agent.message", let last = out.last, case .messages(let es) = last.kind, es[0].actor == e.actor, es[0].attempt == e.attempt {
                    out[out.count - 1] = Group1(id: last.id, kind: .messages(es + [e]))
                    continue
                }
                out.append(Group1(id: e.seq, kind: e.type == "agent.message" ? .messages([e]) : .item(e)))
            }
        }
        flush()
        return out
    }
}

/// The conversation itself, newest at the bottom. While the person is at the bottom it follows what
/// arrives; once they scroll up it stays where they are, and a button takes them back to the
/// newest. Rows are built lazily: a lead's session runs to thousands of events.
// @project-doc docs/architecture/20261031-frontend.md#session_view
struct ChatScroll: View {
    let run: RunModel
    let st: RunState
    let groups: [ChatCard.Group1]
    let density: String
    let empty: Bool
    /// The session's last event: it changes whenever anything arrives.
    let last: Int
    /// Bumped when the person sends a message, which brings them to the newest.
    let sent: Int
    @Local private var follow = ChatFollow()
    @Local private var position = ScrollPosition(edge: .bottom)
    /// The person is scrolling: dragging, swiping, or the swipe running on.
    @Local private var person = false

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 10) {
                if empty { Text("开始执行后，这里会实时显示对话、工具调用和你的消息。").foregroundStyle(.secondary) }
                ForEach(groups) { row($0) }
            }
            .padding(12)
        }
        .scrollPosition($position)
        // Opens at the newest item; later changes of content leave the place to `follow`.
        .defaultScrollAnchor(.bottom, for: .initialOffset)
        .onScrollPhaseChange { _, phase in
            let now = phase == .tracking || phase == .interacting || phase == .decelerating
            if now != person { person = now }
        }
        // How much content lies below the visible area, reported on every scroll and every change
        // of the content's size; the state changes only when the answer does.
        .onScrollGeometryChange(for: ChatGeometry.self) { g in
            let below = g.contentSize.height - g.visibleRect.maxY
            return ChatGeometry(below: below, end: g.contentOffset.y + below)
        } action: { _, g in
            var again = false
            update { again = $0.measured(below: g.below, last: last, person: person) }
            // To the offset that shows the end now, on the next turn of the main loop. Asking for
            // the bottom edge again while the position already says "bottom edge" (as it does from
            // opening) did not move the view on macOS 15.
            if again { DispatchQueue.main.async { position.scrollTo(y: g.end) } }
        }
        // A new reply while following: go to it at once; the measurements above finish the landing.
        .onChange(of: last) { if follow.following && !person { toEnd() } }
        .onChange(of: sent) { toEnd() }
        .onChange(of: density) { if follow.following { toEnd() } }
        .overlay(alignment: .bottom) { if !follow.following { back } }
    }

    @ViewBuilder private func row(_ g: ChatCard.Group1) -> some View {
        let cwd = st.run.cwd
        switch g.kind {
        case .item(let e): ChatItem(run: run, st: st, e: e)
        case .messages(let es): ChatItem(run: run, st: st, e: es[0], more: Array(es.dropFirst()))
        case .tools(let es):
            if density == "summary" {
                Text(verbatim: "· \(es.count) 个操作").font(.caption).foregroundStyle(.tertiary)
            } else if density == "normal" && es.count > 4 {
                DisclosureGroup(String("\(es.count - 2) 个操作")) {
                    VStack(alignment: .leading, spacing: 6) { ForEach(es.dropLast(2)) { ToolRow(run: run, e: $0, cwd: cwd) } }
                }
                .font(.callout).foregroundStyle(.secondary)
                ForEach(es.suffix(2)) { ToolRow(run: run, e: $0, cwd: cwd) }
            } else {
                ForEach(es) { ToolRow(run: run, e: $0, cwd: cwd) }
            }
        }
    }

    /// Back to the newest item, following again.
    private var back: some View {
        Button { toEnd() } label: {
            Label(follow.unseen ? "有新消息" : "回到最新", systemImage: "arrow.down")
                .font(.caption.weight(.medium))
                .foregroundStyle(follow.unseen ? Color.accent : Color.secondary)
                .padding(.horizontal, 10).padding(.vertical, 5)
                .background(Color.card, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.line))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .padding(.bottom, 10)
        .help("回到最新的消息，之后的新消息会继续显示在底部")
        .accessibilityLabel(follow.unseen ? "有新消息，回到最新" : "回到最新")
    }

    private func toEnd() {
        update { $0.jumped() }
        position.scrollTo(edge: .bottom)
    }

    private func update(_ change: (inout ChatFollow) -> Void) {
        var f = follow
        change(&f)
        if f != follow { follow = f }
    }
}

/// What a conversation's scroll view reports: how much content lies below the visible area, and
/// the offset at which the end would be in view.
struct ChatGeometry: Equatable {
    var below: CGFloat
    var end: CGFloat
}

/// Whether a conversation follows what arrives: only while its end is in view. Only the person's
/// own scrolling lets go of the end or takes it up again; scrolled up, they keep their place, and
/// what arrived meanwhile is noted for the way back.
struct ChatFollow: Equatable {
    /// The newest item is in view, so what moves the end is followed.
    var following = true
    /// Something arrived while the person was reading further up.
    var unseen = false
    /// Scrolls to the end still allowed before giving up. A lazy list only estimates the rows it
    /// has not built, and on macOS 15 SwiftUI moves the offset while it measures them, so the end
    /// moves while the view goes there.
    var landing = 0
    /// The session's newest event at the last measurement.
    var seen = 0
    /// The last measurement found the end in view.
    var atEnd = true
    /// How close to the bottom still counts as being there.
    static let slack: CGFloat = 32
    static let landingTries = 8

    /// `below` points of content lie under the visible area, the session's newest event is
    /// `last`, and `person` says whether the person is scrolling. True when the view should
    /// scroll to the end.
    mutating func measured(below: CGFloat, last: Int, person: Bool) -> Bool {
        let atEnd = below <= Self.slack
        let new = last != seen
        self.atEnd = atEnd
        seen = last
        if person {
            following = atEnd
            if atEnd { unseen = false } else if new { unseen = true }
            landing = 0
            return false
        }
        if !following {
            if new { unseen = true }
            return false
        }
        // Following, and not the person: new content, rows measured or the offset adjusted moved
        // the end. Go after it, a few times at most.
        if atEnd { landing = 0; return false }
        if landing == 0 { landing = Self.landingTries; return true }
        landing -= 1
        // Never got there: let go, and the button takes the person back.
        if landing == 0 { following = false; return false }
        return true
    }

    /// The view is sent to the newest item: sending a message, the button, a new display level.
    mutating func jumped() {
        following = true
        unseen = false
        landing = 0
    }
}

/// "N 秒没有新输出": while an agent works on its own, how long since it did something visible.
struct QuietNote: View {
    let last: Double
    var body: some View {
        TimelineView(.periodic(from: .now, by: 5)) { ctx in
            let s = Int((ctx.date.timeIntervalSince1970 * 1000 - last) / 1000)
            if last > 0 && s >= 20 {
                Text(verbatim: "\(s < 120 ? "\(s) 秒" : "\(s / 60) 分钟")没有新输出").font(.caption).foregroundStyle(.secondary)
                    .help("Agent 长时间思考或写较长的内容时不会有更新")
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

    /// The structured result at the end of an agent's reply.
    private static let resultBlock = regex(#"\s*```json\s*[\s\S]*?```\s*$"#)

    /// Items that render nothing (messages shown where they were routed, unknown messages).
    static func visible(_ e: Event, st: RunState) -> Bool {
        switch e.type {
        case "user.message": return !e.data["id"].exists
        // Shown beside the conversation (plan, controls, commands, usage), not in it.
        case "agent.plan", "session.controls", "session.commands", "session.usage": return false
        // Stepping on another task's files says more than "both edited it": show only that.
        case "task.conflict":
            guard !e.data["owned"].truthy else { return true }
            let pair: Set<String> = [e.task ?? "", e.data["other"].str]
            let paths = Set(e.data["paths"].strings)
            return !st.events.contains { x in
                x.type == "task.conflict" && x.data["owned"].truthy && [x.task ?? "", x.data["other"].str] == pair && paths.isSubset(of: Set(x.data["paths"].strings))
            }
        case "message.routed":
            guard st.msgs[e.data["id"].str] != nil else { return false }
            return ["steer", "prompt"].contains(e.data["delivery"].str)
        // A steered message is shown where it was inserted; applying it only updates that bubble.
        case "message.delivered":
            guard e.data["ids"].exists else { return true }
            return e.data["ids"].strings.contains { st.msgs[$0].map { $0.delivery != "steer" } ?? false }
        default: return true
        }
    }

    var body: some View {
        let d = e.data
        switch e.type {
        case "attempt.started":
            VStack(alignment: .leading, spacing: 4) {
                Divider()
                Text(verbatim: "\(e.task.map { "\($0) · " } ?? "")尝试 \(e.attempt ?? 0) · \(d["runtime"].str)\(d["config"]["model"].truthy ? " · \(d["config"]["model"].str)" : "")\(d["resumed"].truthy ? " · 恢复了原会话" : "")")
                    .font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                DisclosureGroup("编排器发出的子任务说明") { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
        case "run.created":
            divider("目标 · \(L.goalTitle(d["goal"].str))")
            StoredImages(paths: Attach.paths(d), size: 56)
        case "agent.message":
            // The structured result block is for the orchestrator; in the chat it collapses to a label.
            // The result itself shows as its own line (`result.submitted`).
            // Pieces keep their own whitespace (code blocks may be cut anywhere): join them as they came.
            let text = ([e] + more).map { $0.data["text"].str }.joined()
            let words = Self.resultBlock.replacing(in: text, with: "").trimmingCharacters(in: .whitespacesAndNewlines)
            if !words.isEmpty { bubble(agent: e.actor) { MarkdownText(text: words) } }
        case "agent.thought":
            DisclosureGroup {
                Text(d["text"].str).font(.callout).italic().foregroundStyle(.secondary).textSelection(.enabled).padding(.top, 2)
            } label: {
                Text(verbatim: "思考 · \(L.short(d["text"].str.replacingOccurrences(of: "\n", with: " "), 60))").font(.caption).foregroundStyle(.secondary)
            }
        case "session.control":
            note("\(d["failed"].truthy ? "⚠" : "↺") \(e.text)", d["failed"].truthy ? .amber : nil)
        case "session.prompt":
            let via = d["via"].str == "developer" ? "开发者指令" : "系统提示"
            let label = d["channel"].str == "instructions" ? "用户的长期要求 · \(via)"
                : d["channel"].str == "system" ? "Janus 的说明 · \(via)"
                : ["context": "团队上下文", "goal": "目标提示", "followup": "继续提示", "fix": "验证没有通过 · 发回给它修复", "board": "看板推送 · 插入正在进行的这一轮", "goal_check": "目标确认 · 负责人停下了，目标还标着进行中"][d["channel"].str] ?? "Janus 提示"
            VStack(alignment: .leading, spacing: 4) {
                HStack { Text(verbatim: "→ \(label)").font(.callout.weight(.semibold)); Text("Janus 发给 Agent").font(.caption).foregroundStyle(.secondary) }
                DisclosureGroup("查看内容") { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
            .padding(8)
            .background(Tone.blue.color.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
        // Goal mode: a collaborator asks the lead, which answers on the board.
        case "question.asked": note("? 问负责人：\(d["question"].string ?? e.text)\(d["options"].array.isEmpty ? "" : "（\(d["options"].strings.joined(separator: " / "))）") · 负责人会回答，不需要你处理", .amber)
        case "context.handoff": note("⇄ 上游子任务 \(d["from"].string ?? e.text) 的结果已带入本次子任务", nil)
        case "board.posted": note("\(d["kind"].str == "blocker" ? "⚠" : d["kind"].str == "note" ? "·" : "↗") 协作者写回看板：\(d["text"].string ?? e.text)", nil)
        case "task.conflict" where d["owned"].truthy:
            note("⚠ 越界：\(e.task ?? "") 改了 \(d["other"].str) 负责的 \(d["paths"].strings.joined(separator: "、"))。已提醒 \(e.task ?? "") 不要再改，并告诉负责人", .amber)
        case "task.conflict": note("⚠ \(e.task ?? "") 和 \(d["other"].str) 同时在改 \(d["paths"].strings.joined(separator: "、"))。已提醒双方先读最新内容，并告诉负责人", .amber)
        case "user.message":
            let via = ["steer": "已插入正在进行的这一轮", "queued": "这一轮结束后发送", "prompt": "作为新一轮发送", "next": "下次会话时带给它"][d["delivery"].str] ?? ""
            userBubble(d["text"].str, images: Attach.paths(d), via: via, insert: false)
        case "message.routed":
            if let m = st.msgs[d["id"].str] {
                if d["delivery"].str == "steer" { userBubble(m.text, images: m.images, via: "\(m.by == "lead" ? "负责人插话" : "插话") · \(m.deliveredSeq != nil ? "Agent 已读到" : "已插入这一轮，当前操作结束后读到")", insert: true) }
                else { userBubble(m.text, images: m.images, via: (m.by == "lead" ? "负责人 · " : "") + "作为新的一轮发送", insert: false) }
            }
        case "message.delivered":
            if d["ids"].exists {
                ForEach(d["ids"].strings.compactMap { st.msgs[$0] }.filter { $0.delivery != "steer" }, id: \.id) { m in
                    userBubble(m.text, images: m.images, via: (m.by == "lead" ? "负责人 · " : "") + (m.delivery == "next" ? "留言 · 在这次会话开始时带入" : m.delivery == "interrupt" ? "打断上一轮后立即发送" : "排队消息 · 上一轮结束后发送"), insert: false)
                }
            } else { note("\(L.icon[e.type] ?? "·") \(e.text)", L.tone(of: e.type)) }
        // One line in the conversation; each criterion is a click away (and on the task page).
        case "result.submitted":
            let checks = Check.list(d["checks"])
            let passed = checks.filter(\.passed).count
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(checks.indices, id: \.self) { i in CheckRow(c: checks[i]) }
                    if !d["evidence"].array.isEmpty { Text(verbatim: "证据：\(d["evidence"].strings.joined(separator: "；"))").font(.caption).foregroundStyle(.secondary) }
                    if !d["risks"].array.isEmpty { Text(verbatim: "风险：\(d["risks"].strings.joined(separator: "；"))").font(.caption).foregroundStyle(Tone.amber.color) }
                }
                .padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: "⇪ 提交结果：\(d["summary"].string ?? e.text)").font(.callout.weight(.semibold)).lineLimit(2)
                    if !checks.isEmpty { Pill("\(passed)/\(checks.count) 通过", passed == checks.count ? .green : .amber) }
                    if !d["risks"].array.isEmpty { Pill("\(d["risks"].array.count) 个风险", .amber) }
                }
            }
            .padding(8)
            .background(Tone.green.color.opacity(0.07), in: RoundedRectangle(cornerRadius: 8))
        case "agent.status": note("◌ \(e.text)", .amber)
        // A request still waiting is answered right here, with the same card as the inbox.
        case "approval.requested", "input.requested":
            if let item = st.inbox.first(where: { $0.key == d["key"].str }) { RequestCard(run: run, st: st, item: item) }
            else { note("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)", L.tone(of: e.type)) }
        default: note("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)", L.tone(of: e.type))
        }
    }

    private func divider(_ t: String) -> some View {
        HStack { VStack { Divider() }; Text(t).font(.caption).foregroundStyle(.secondary).lineLimit(1); VStack { Divider() } }
    }

    private func note(_ t: String, _ tone: Tone?) -> some View {
        Text(t).font(.caption).foregroundStyle(tone?.color ?? .secondary).textSelection(.enabled)
    }

    private func bubble<C: View>(agent: String, @ViewBuilder content: () -> C) -> some View {
        let w = run.who(agent)
        return HStack(alignment: .top, spacing: 8) {
            Avatar(initials: w.initials, color: w.color, size: 22)
            VStack(alignment: .leading, spacing: 4) { content() }
                .padding(10)
                .background(Color.secondary.opacity(0.07), in: RoundedRectangle(cornerRadius: 10))
            Spacer(minLength: 40)
        }
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
            .background(Color.accent.opacity(insert ? 0.06 : 0.12), in: RoundedRectangle(cornerRadius: 10))
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
        let echo = ["tool.call", "tool.started"].contains(e.type) && !d["tool"].str.hasPrefix("janus.") && (mcp(e.text) || mcp(d["tool"].str) || d["input"].str.hasPrefix("{\"arguments\""))
        return echo || (e.type == "tool.started" && done.contains("\(e.actor):\(d["callId"].str)"))
    }

    var body: some View {
        let d = e.data
        let board = d["tool"].str.hasPrefix("janus.")
        let output = L.inWorkspace(d["output"].str, cwd)
        // The first line that says something: not a Markdown fence around the output.
        let firstLine = output.split(separator: "\n").map(String.init).first { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("```") && !$0.trimmingCharacters(in: .whitespaces).isEmpty } ?? ""
        let (label, result) = board ? janusLabel(d["tool"].str, d["args"], output) : (d["input"].truthy ? d["input"].str : e.text, firstLine)
        let full = board ? d["args"].or(d["input"]).pretty() : (d["input"].truthy ? d["input"].str : e.text)
        DisclosureGroup {
            CodeBlock(text: full + (output.isEmpty ? "" : "\n\n→ \(output)"), maxHeight: 300)
            ForEach(Array(d["diffs"].array.enumerated()), id: \.offset) { _, x in
                InlineDiff(path: L.tilde(x["path"].str), old: x["old"].str, new: x["new"].str)
            }
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(board ? "看板" : (d["tool"].truthy ? d["tool"].str : "工具")).font(.system(size: 11, design: .monospaced)).padding(.horizontal, 4).background((board ? Color.accent : Color.secondary).opacity(0.12), in: RoundedRectangle(cornerRadius: 3))
                    Text(label).font(.callout).lineLimit(1).truncationMode(.tail)
                    if e.type == "tool.started" { Pill(d["live"].truthy ? "进行中" : "等待返回", .blue) } else if d["status"].str == "failed" { Pill("失败", .red) }
                    if !d["diffs"].array.isEmpty { Text(verbatim: "改了 \(d["diffs"].array.count) 个文件").font(.caption).foregroundStyle(.secondary) }
                }
                if !result.isEmpty { Text("→ " + L.short(result, 160)).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
            }
        }
    }

    private func janusLabel(_ tool: String, _ a: JSON, _ out: String) -> (String, String) {
        let first = out.split(separator: "\n").first.map(String.init) ?? ""
        switch tool {
        case "janus.orchestrate": return ("提交编排：\(a["tasks"].array.map { "\($0["id"].str) \($0["title"].str) → \(run.who($0["assignee"].str).name)" }.joined(separator: "；"))", first)
        case "janus.wait": return ("等待看板上的进展", first)
        case "janus.reply": return ("回复 \(a["task"].str)：\(a["text"].str)", first)
        case "janus.retry": return ("重新执行 \(a["task"].str)\(a["instructions"].truthy ? "（改了说明）" : "")\(a["accept"].truthy ? "（改了验收标准）" : "")", first)
        case "janus.accept": return ("接受 \(a["task"].str) 的结果：\(a["reason"].str)", first)
        case "janus.cancel": return ("取消 \(a["tasks"].strings.joined(separator: "、"))\(a["reason"].truthy ? "：\(a["reason"].str)" : "")", first)
        case "janus.ask_user": return ("请你决定：\(a["question"].str)", first)
        case "janus.team": return ("查看团队和子任务状态", "")
        case "janus.board_post": return ("写回看板：\(a["text"].str)", "")
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
                Text(verbatim: "待发送 \(list.count)").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                ForEach(list, id: \.id) { m in
                    let (l, tone) = Self.labels[m.delivery] ?? ("", .gray)
                    HStack(spacing: 8) {
                        if m.delivery == "pending" { ProgressView().controlSize(.mini) }
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

    static let labels: [String: (String, Tone)] = ["pending": ("发送中…", .blue), "queued": ("排队 · 这一轮结束后发送", .amber), "next": ("留言 · 下次会话开始时带给它", .gray), "interrupt": ("正在打断这一轮，马上送达", .blue)]

    private var pending: [Msg] {
        st.msgs.values.filter { m in
            guard m.deliveredSeq == nil, Self.labels[m.delivery] != nil else { return false }
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
                Avatar(initials: w.initials, color: w.color, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(w.name).font(.display(.title2))
                    Text([w.role, w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).foregroundStyle(.secondary)
                }
                Spacer()
            }
            AdaptiveColumns(side: 300) {
                // A fixed height like the task page: the conversation scrolls inside its card,
                // not the page around it.
                ChatCard(run: run, st: st, title: "对话", info: agentSession(st, agentId), target: .agent(agentId))
                    .frame(height: 640)
                Card(padding: 12) {
                    HStack { Text("负责的子任务").font(.headline); Spacer(); Text(verbatim: "\(tasks.count) 个").font(.caption).foregroundStyle(.secondary) }
                    if tasks.isEmpty { Text(agentId == "lead" ? "负责人负责规划和协调。" : "还没有分配子任务。").font(.callout).foregroundStyle(.secondary) }
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
