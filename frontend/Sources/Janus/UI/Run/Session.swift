import SwiftUI

/// Event types a session shows.
let sessionTypes: Set<String> = ["attempt.started", "session.prompt", "context.handoff", "board.posted", "agent.message", "user.message", "message.routed", "message.delivered", "tool.call", "tool.started", "tool.completed", "question.asked", "question.answered", "approval.requested", "approval.granted", "approval.denied", "input.requested", "input.answered", "control.taken", "control.released", "session.interrupted", "result.submitted", "attempt.failed"]

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
}

func taskSession(_ st: RunState, _ t: TaskState) -> SessionInfo {
    SessionInfo(evs: st.events.filter { $0.task == t.id && isSessionEvent($0) }, open: t.live, human: t.control == "human", idle: t.turn == "idle", resume: ["interrupted", "failed"].contains(st.run.status), leave: false)
}

/// Everything one agent did in this run, and where a message to it goes right now.
func agentSession(_ st: RunState, _ id: String) -> SessionInfo {
    let evs = st.events.filter { e in ((e.actor == id || e.to == id) && isSessionEvent(e)) || (id == "lead" && e.type == "run.created") }
    let open = st.orderedTasks.contains { $0.agent == id && $0.live } || (id == "lead" && ["planning", "running"].contains(st.run.status))
    var human = false, idle = false
    for e in evs {
        if e.type == "control.taken" || (e.type == "session.interrupted" && !e.data["finish"].truthy) { human = true }
        if ["control.released", "attempt.started", "run.created"].contains(e.type) { human = false }
        if e.type == "agent.status" && e.data["turn"].str == "idle" { idle = true }
        if ((e.type == "user.message" || e.type == "message.routed") && e.data["delivery"].str == "prompt") || ["control.released", "message.delivered", "attempt.started"].contains(e.type) { idle = false }
    }
    let active = !["delivered", "accepted", "cancelled"].contains(st.run.status)
    return SessionInfo(evs: evs, open: open, human: human, idle: idle, resume: ["interrupted", "failed"].contains(st.run.status), leave: active)
}

/// One conversation view for every session.
// @project-doc docs/architecture/frontend.md#session_messages
struct ChatCard: View {
    let run: RunModel
    let st: RunState
    let title: String
    let info: SessionInfo
    let target: SessionTarget

    var body: some View {
        let live = run.canAct
        let (state, tone, buttons, placeholder, enabled) = mode(live)
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                if info.open && live && !(info.human && info.idle) { QuietNote(last: info.evs.last?.ts ?? 0) }
                Pill(state, tone)
            }
            .padding(12)
            Divider()
            ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if info.evs.isEmpty { Text("开始执行后，这里会实时显示对话、工具调用和你的消息。").foregroundStyle(.secondary) }
                    ForEach(groups) { g in
                        switch g.kind {
                        case .item(let e): ChatItem(run: run, st: st, e: e, done: done)
                        case .tools(let es):
                            if es.count > 4 {
                                DisclosureGroup(String("\(es.count - 2) 个操作")) {
                                    VStack(alignment: .leading, spacing: 6) { ForEach(es.dropLast(2)) { ToolRow(run: run, e: $0) } }
                                }
                                .font(.callout).foregroundStyle(.secondary)
                                ForEach(es.suffix(2)) { ToolRow(run: run, e: $0) }
                            } else {
                                ForEach(es) { ToolRow(run: run, e: $0) }
                            }
                        }
                    }
                    Color.clear.frame(height: 1).id("chat-end")
                }
                .padding(12)
            }
            .onAppear { proxy.scrollTo("chat-end", anchor: .bottom) }
            .onChange(of: info.evs.count) { proxy.scrollTo("chat-end", anchor: .bottom) }
            }
            PendingTray(st: st, target: target)
            Divider()
            VStack(alignment: .trailing, spacing: 8) {
                Composer(text: Binding(get: { run.sessionDrafts[target.key] ?? "" }, set: { run.sessionDrafts[target.key] = $0 }), images: Binding(get: { run.sessionImages[target.key] ?? [] }, set: { run.sessionImages[target.key] = $0 }), placeholder: placeholder, disabled: !enabled, minHeight: 44) { send("message") }
                HStack { Spacer(); buttons }
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
        if info.leave && live {
            return ("当前没有会话", .gray, AnyView(btn("sessSend", "留言", true)), "留言会在它下一次会话开始时带给它", true)
        }
        return ("会话已结束", .gray, AnyView(EmptyView()), live ? "这个会话已经结束" : "只读查看", false)
    }

    private func btn(_ act: String, _ label: String, _ primary: Bool = false, danger: Bool = false) -> some View {
        Group {
            if primary {
                Button(label) { send(act == "sessSend" ? "message" : act) }.buttonStyle(.borderedProminent)
                    .disabled(draft.isEmpty && images.isEmpty).help(draft.isEmpty && images.isEmpty ? "先写下要发送的内容，或者粘贴图片" : "⌘↩")
            }
            else if danger { Button(label, role: .destructive) { send(act) } }
            else { Button(label) { send(act) } }
        }
    }

    private var draft: String { (run.sessionDrafts[target.key] ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
    private var images: [PastedImage] { run.sessionImages[target.key] ?? [] }

    private func send(_ type: String) {
        if type == "finish" && !confirm("用当前产出完成这个任务？", "任务里已有的改动会照常记录成 diff。", ok: "完成", destructive: false) { return }
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
        }
        Task { await run.act(body) }
    }

    // Long runs of tool calls fold into one line, keeping the latest few in view.
    struct Group1: Identifiable { enum Kind { case item(Event), tools([Event]) }; let id: Int; let kind: Kind }

    private var done: Set<String> { Set(info.evs.filter { $0.type == "tool.completed" }.map { "\($0.actor):\($0.data["callId"].str)" }) }

    private var groups: [Group1] {
        var out: [Group1] = []
        var run: [Event] = []
        let done = done
        func flush() { if let f = run.first { out.append(Group1(id: -f.seq, kind: .tools(run))) }; run = [] }
        for e in info.evs {
            if ["tool.call", "tool.started", "tool.completed"].contains(e.type) {
                if ToolRow.hidden(e, done: done) { continue }
                run.append(e)
            } else {
                if !ChatItem.visible(e, st: st) { continue }
                flush()
                out.append(Group1(id: e.seq, kind: .item(e)))
            }
        }
        flush()
        return out
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
    let done: Set<String>

    /// Items that render nothing (messages shown where they were routed, unknown messages).
    static func visible(_ e: Event, st: RunState) -> Bool {
        switch e.type {
        case "user.message": return !e.data["id"].exists
        case "message.routed":
            guard st.msgs[e.data["id"].str] != nil else { return false }
            return ["steer", "prompt"].contains(e.data["delivery"].str)
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
                DisclosureGroup("编排器发出的任务说明") { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
        case "run.created":
            divider("目标 · \(L.goalTitle(d["goal"].str))")
            StoredImages(paths: Attach.paths(d), size: 56)
        case "agent.message":
            // The structured result block is for the orchestrator; in the chat it collapses to a label.
            let text = d["text"].str
            let r = text.range(of: #"\s*```json\s*[\s\S]*?```\s*$"#, options: .regularExpression)
            bubble(agent: e.actor) {
                MarkdownText(text: r.map { String(text[..<$0.lowerBound]) } ?? text)
                if r != nil { Text("附结构化结果").font(.caption).foregroundStyle(.secondary) }
            }
        case "session.prompt":
            let label = d["channel"].str == "instructions"
                ? "用户的长期要求 · \(d["via"].str == "developer" ? "开发者指令" : d["via"].str == "extension" ? "系统提示（Pi 扩展）" : "系统提示")"
                : ["system": "主控上下文", "context": "团队上下文", "goal": "目标提示", "followup": "继续提示", "fix": "验证没有通过 · 发回给它修复"][d["channel"].str] ?? "Janus 提示"
            VStack(alignment: .leading, spacing: 4) {
                HStack { Text(verbatim: "→ \(label)").font(.callout.weight(.semibold)); Text("Janus 发给 Agent").font(.caption).foregroundStyle(.secondary) }
                DisclosureGroup("查看内容") { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
            .padding(8)
            .background(Tone.blue.color.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
        // Goal mode: a collaborator asks the lead, which answers on the board.
        case "question.asked": note("? 问主控：\(d["question"].string ?? e.text)\(d["options"].array.isEmpty ? "" : "（\(d["options"].strings.joined(separator: " / "))）") · 主控会回答，不需要你处理", .amber)
        case "context.handoff": note("⇄ 上游任务 \(d["from"].string ?? e.text) 的结果已带入本次任务", nil)
        case "board.posted": note("\(d["kind"].str == "blocker" ? "⚠" : d["kind"].str == "note" ? "·" : "↗") 协作者写回看板：\(d["text"].string ?? e.text)", nil)
        case "user.message":
            let via = ["steer": "已插入正在进行的这一轮", "queued": "这一轮结束后发送", "prompt": "作为新一轮发送", "next": "下次会话时带给它"][d["delivery"].str] ?? ""
            userBubble(d["text"].str, images: Attach.paths(d), via: via, insert: false)
        case "message.routed":
            if let m = st.msgs[d["id"].str] {
                if d["delivery"].str == "steer" { userBubble(m.text, images: m.images, via: "\(m.by == "lead" ? "主控插话" : "插话") · 已插入正在进行的这一轮", insert: true) }
                else { userBubble(m.text, images: m.images, via: (m.by == "lead" ? "主控 · " : "") + "作为新的一轮发送", insert: false) }
            }
        case "message.delivered":
            if d["ids"].exists {
                ForEach(d["ids"].strings.compactMap { st.msgs[$0] }, id: \.id) { m in
                    userBubble(m.text, images: m.images, via: (m.by == "lead" ? "主控 · " : "") + (m.delivery == "next" ? "留言 · 在这次会话开始时带入" : m.delivery == "interrupt" ? "打断上一轮后立即发送" : "排队消息 · 上一轮结束后发送"), insert: false)
                }
            } else { note("\(L.icon[e.type] ?? "·") \(e.text)", L.tone(of: e.type)) }
        case "result.submitted":
            let checks = Check.list(d["checks"])
            VStack(alignment: .leading, spacing: 6) {
                Text(verbatim: "结构化结果：\(d["summary"].string ?? e.text)").font(.callout.weight(.semibold))
                ForEach(checks.indices, id: \.self) { i in CheckRow(c: checks[i]) }
                if !d["evidence"].array.isEmpty { Text(verbatim: "证据：\(d["evidence"].strings.joined(separator: "；"))").font(.caption).foregroundStyle(.secondary) }
                if !d["risks"].array.isEmpty { Text(verbatim: "风险：\(d["risks"].strings.joined(separator: "；"))").font(.caption).foregroundStyle(Tone.amber.color) }
            }
            .padding(10)
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
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(c.passed ? "✓" : "✗").foregroundStyle(c.passed ? Tone.green.color : Tone.red.color).fontWeight(.bold)
            VStack(alignment: .leading, spacing: 2) {
                Text(c.criterion).font(.callout.weight(.medium))
                if !c.evidence.isEmpty { Text(MD.inline(c.evidence)).font(.caption).foregroundStyle(.secondary) }
            }
        }
    }
}

/// One row per tool call, readable at a glance, with the full input and output a click away.
/// Janus's board tools are recorded twice (by Janus and by the agent): only Janus's record is
/// shown, and a call that came back replaces its "waiting" row.
struct ToolRow: View {
    let run: RunModel
    let e: Event

    static func hidden(_ e: Event, done: Set<String>) -> Bool {
        let d = e.data
        let echo = e.type == "tool.call" && (e.text.hasPrefix("mcp.janus.") || d["tool"].str.hasPrefix("mcp.janus.") || d["input"].str.hasPrefix("{\"arguments\""))
        return echo || (e.type == "tool.started" && done.contains("\(e.actor):\(d["callId"].str)"))
    }

    var body: some View {
        let d = e.data
        let board = d["tool"].str.hasPrefix("janus.")
        let (label, result) = board ? janusLabel(d["tool"].str, d["args"], d["output"].str) : (d["input"].truthy ? d["input"].str : e.text, d["output"].str.split(separator: "\n").first.map(String.init) ?? "")
        let full = board ? d["args"].or(d["input"]).pretty() : (d["input"].truthy ? d["input"].str : e.text)
        DisclosureGroup {
            CodeBlock(text: full + (d["output"].truthy ? "\n\n→ \(d["output"].str)" : ""), maxHeight: 300)
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(board ? "看板" : (d["tool"].truthy ? d["tool"].str : "工具")).font(.system(size: 11, design: .monospaced)).padding(.horizontal, 4).background((board ? Color.accent : Color.secondary).opacity(0.12), in: RoundedRectangle(cornerRadius: 3))
                    Text(label).font(.callout).lineLimit(1).truncationMode(.tail)
                    if e.type == "tool.started" { Pill("等待返回", .blue) } else if d["status"].str == "failed" { Pill("失败", .red) }
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
        case "janus.team": return ("查看团队和任务状态", "")
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
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Avatar(initials: w.initials, color: w.color, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(w.name).font(.title2.weight(.semibold))
                    Text([w.role, w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).foregroundStyle(.secondary)
                }
                Spacer()
            }
            HStack(alignment: .top, spacing: 16) {
                ChatCard(run: run, st: st, title: "对话", info: agentSession(st, agentId), target: .agent(agentId))
                Card(padding: 12) {
                    HStack { Text("负责的任务").font(.headline); Spacer(); Text(verbatim: "\(tasks.count) 个").font(.caption).foregroundStyle(.secondary) }
                    if tasks.isEmpty { Text(agentId == "lead" ? "主控负责规划和协调。" : "还没有分配任务。").font(.callout).foregroundStyle(.secondary) }
                    ForEach(tasks, id: \.id) { t in
                        let (l, tone) = L.task(t.status)
                        HStack {
                            Button(String("\(t.id) \(t.title)")) { run.show(.task(t.id)) }.buttonStyle(.link).lineLimit(2)
                            Spacer()
                            Pill(l, tone)
                        }
                    }
                }
                .frame(width: 300)
            }
        }
        .padding(20)
    }
}
