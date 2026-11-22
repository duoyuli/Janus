import SwiftUI

/// How a library router works, at the top of its tab: a short loop that plays the router's steps
/// beside the steps in words, the one playing highlighted. Clicking a step jumps to it; with
/// reduced motion the picture stands still and the steps read as a list. It can fold down to the
/// one line that says what the router does with the library as it is now.
// @project-doc docs/domains/20261116-resource_library.md#skill_router
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
struct RouterGuide: View {
    let kind: AppStore.LibraryKind
    /// Names of what goes through the router: every skill (in the catalog's order), or every MCP server.
    let names: [String]
    /// What the router does with the library as it is now, in one line.
    let now: String
    /// The skill router's catalog is searched by keyword rather than listed in full.
    var searched = false

    @AppStorage("janus.routerGuide", store: .janus) private var open = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Local private var start = Date()

    /// Seconds each step plays.
    static let beat = 2.6

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: kind == .skill ? "sparkles" : "point.3.connected.trianglepath.dotted").foregroundStyle(Color.accent)
                Text(kind == .skill ? L.text("Skill 路由怎样工作") : L.text("MCP 路由怎样工作")).font(.headline)
                Spacer()
                Button(open ? L.text("收起说明") : L.text("展开说明")) { open.toggle() }.buttonStyle(.link).font(.callout)
            }
            if open {
                TimelineView(.animation(minimumInterval: 1.0 / 30, paused: reduceMotion)) { ctx in
                    let (step, p) = clock(ctx.date)
                    HStack(alignment: .top, spacing: 18) {
                        Group {
                            switch kind {
                            case .skill: SkillRouteScene(names: names, searched: searched, step: step, p: p)
                            case .mcp: McpRouteScene(names: names, step: step, p: p)
                            }
                        }
                        .frame(width: 360, height: 150)
                        .background(Color.soft, in: RoundedRectangle(cornerRadius: 10))
                        .overlay(alignment: .bottom) {
                            if names.isEmpty { Text(L.text("示例")).font(.system(size: 9.5)).foregroundStyle(.tertiary).padding(6) }
                        }
                        .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(steps.indices, id: \.self) { i in stepRow(i, current: reduceMotion ? nil : step) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            Text(now).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
    }

    /// The step playing at `date` and how far into it (0…1). Reduced motion stays on the last step, finished.
    private func clock(_ date: Date) -> (Int, Double) {
        guard !reduceMotion else { return (steps.count - 1, 1) }
        let t = max(0, date.timeIntervalSince(start))
        let step = Int(t / Self.beat) % steps.count
        return (step, (t / Self.beat).truncatingRemainder(dividingBy: 1))
    }

    private func stepRow(_ i: Int, current: Int?) -> some View {
        let on = current == i
        return Button {
            start = Date().addingTimeInterval(-Double(i) * Self.beat)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(i + 1)")
                    .font(.caption.weight(.bold).monospacedDigit())
                    .foregroundStyle(on ? Color.onInk : Color.secondary)
                    .frame(width: 18, height: 18)
                    .background(on ? Color.accent : Color.secondary.opacity(0.12), in: Circle())
                VStack(alignment: .leading, spacing: 1) {
                    Text(steps[i].title).font(.callout.weight(.semibold)).foregroundStyle(on || current == nil ? Color.primary : Color.secondary)
                    Text(steps[i].detail).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.horizontal, 8).padding(.vertical, 5)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(on ? Color.accent.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .animation(.easeOut(duration: 0.25), value: on)
    }

    private var steps: [(title: String, detail: String)] {
        switch kind {
        case .skill: return [
            (L.text("收进路由"), L.text("资源库的 Skill 统一收进 janus-skills，Agent 的 Skill 列表里只多这一项。")),
            (L.text("只占一条描述"), L.text("janus-skills 的描述提醒 Agent 先读它，并列出常用 Skill。")),
            (L.text("先查目录"), searched
                ? L.text("Skill 多时目录只列名字，Agent 按关键词搜。")
                : L.text("Agent 读 janus-skills 的目录：每个 Skill 一行，写着完整描述和什么时候用。")),
            (L.text("用到再读"), L.text("找到合适的 Skill 再读 SKILL.md 照做；每次读取都会计数。")),
        ]
        case .mcp: return [
            (L.text("只多一个入口"), L.text("会话只拿到 janus-tools 一个入口，不预先启动服务器；工具名带服务器前缀。")),
            (L.text("用到才启动"), L.text("第一次调用时才启动：命令型在隔离环境里跑，HTTP / SSE 由 Janus 连接。")),
            (L.text("同一个任务共用"), L.text("负责人和子任务共用进程；浏览器这类有状态的服务器可以每个会话一份。")),
            (L.text("闲置就停"), L.text("共用的闲置 10 分钟自动停；单独的随会话结束；任务结束时全停。")),
        ]
        }
    }
}

// MARK: Scenes

/// Smooth in and out, 0…1.
private func ease(_ p: Double) -> Double {
    let p = min(max(p, 0), 1)
    return p < 0.5 ? 2 * p * p : 1 - pow(-2 * p + 2, 2) / 2
}

/// How far a part that plays from `from` to `to` of the step has got.
private func span(_ p: Double, _ from: Double, _ to: Double) -> Double { ease((p - from) / (to - from)) }

private func mix(_ a: CGPoint, _ b: CGPoint, _ t: Double) -> CGPoint {
    CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
}

/// A point on the link from `a` to `b`, `t` of the way along the part between the two nodes'
/// edges: `ra` and `rb` are their half widths, so a packet leaves and arrives at a node's side
/// instead of crossing its name.
private func along(_ a: CGPoint, _ b: CGPoint, _ ra: CGFloat, _ rb: CGFloat, _ t: Double) -> CGPoint {
    let dx = max(abs(b.x - a.x), 1)
    return mix(mix(a, b, ra / dx), mix(a, b, 1 - rb / dx), t)
}

/// A node in a scene: a name in monospace over a quiet caption.
private struct Node: View {
    let title: String
    var caption = ""
    var lit = false
    var tone: Color = .accent
    var width: CGFloat = 96

    var body: some View {
        VStack(spacing: 1) {
            Text(title).font(.system(size: 11, weight: .semibold, design: .monospaced)).lineLimit(1).truncationMode(.middle)
            if !caption.isEmpty { Text(caption).font(.system(size: 9.5)).foregroundStyle(.secondary).lineLimit(1) }
        }
        .padding(.horizontal, 6).padding(.vertical, 4)
        .frame(width: width)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 7))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(lit ? tone : Color.line, lineWidth: lit ? 1.5 : 1))
        .shadow(color: lit ? tone.opacity(0.35) : .clear, radius: 6)
    }
}

/// A packet travelling along a link: a dot, or a small label.
private struct Packet: View {
    var label = ""
    var tone: Color = .accent
    var body: some View {
        if label.isEmpty {
            Circle().fill(tone).frame(width: 9, height: 9).shadow(color: tone.opacity(0.6), radius: 4)
        } else {
            Text(label).font(.system(size: 9.5, weight: .semibold, design: .monospaced)).foregroundStyle(Color.onInk)
                .padding(.horizontal, 5).padding(.vertical, 2)
                .background(tone, in: Capsule())
        }
    }
}

/// The skill router: the skills go into a hidden folder behind one `janus-skills`, the session's
/// prompt gets its one description, the Agent looks the task up in its catalog (listed, or
/// searched by keyword), and reads the SKILL.md of the skill that fits.
private struct SkillRouteScene: View {
    let names: [String]
    let searched: Bool
    let step: Int
    let p: Double

    var body: some View {
        let shown = Array((names.isEmpty ? ["pdf", "docx", "xlsx", "pptx"] : names).prefix(4))
        let more = max(names.count - shown.count, 0)
        GeometryReader { g in
            let w = g.size.width, h = g.size.height
            let chips = shown.indices.map { CGPoint(x: w * 0.18, y: h * (0.16 + 0.2 * Double($0))) }
            let hub = CGPoint(x: w * 0.5, y: h * 0.42)
            let agent = CGPoint(x: w * 0.84, y: h * 0.42)
            let pick = 0
            // The read arcs over the top, from the Agent's top edge to the skill's.
            let readFrom = CGPoint(x: agent.x, y: agent.y - 22), readTo = CGPoint(x: chips[pick].x + 20, y: chips[pick].y - 13)
            let arc = CGPoint(x: w * 0.5, y: h * 0.02)
            let filled = step > 0 ? 1 : span(p, 0.2, 0.95)
            let prompt = step == 1 ? span(p, 0.1, 0.75) : step >= 2 ? 1 : 0
            ZStack {
                Canvas { ctx, _ in
                    for c in chips {
                        var link = Path(); link.move(to: c); link.addLine(to: hub)
                        ctx.stroke(link, with: .color(.secondary.opacity(0.25)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                    }
                    var out = Path(); out.move(to: hub); out.addLine(to: agent)
                    ctx.stroke(out, with: .color(Color.accent.opacity(0.25 + 0.5 * prompt)), lineWidth: 1.5)
                    if step == 3 {
                        var read = Path(); read.move(to: readFrom)
                        read.addQuadCurve(to: readTo, control: arc)
                        ctx.stroke(read, with: .color(Color.accent.opacity(0.6 * span(p, 0, 0.3))), style: StrokeStyle(lineWidth: 1.2, dash: [4, 3]))
                    }
                }
                ForEach(chips.indices, id: \.self) { i in
                    Node(title: shown[i], lit: step == 3 && i == pick && p > 0.3, width: 108).position(chips[i])
                }
                if more > 0 {
                    Text(L.text("还有 \(more) 个")).font(.system(size: 9.5)).foregroundStyle(.secondary)
                        .position(x: w * 0.18, y: h * 0.93)
                }
                Node(title: "janus-skills", caption: searched ? L.text("目录 · 按关键词搜") : L.text("目录 · \(names.count) 行"),
                     lit: filled >= 1 && step != 3, width: 104)
                    .opacity(0.45 + 0.55 * filled).position(hub)
                Node(title: "Agent", caption: prompt > 0.99 ? L.text("提示词 +1 条") : L.text("一个会话"), lit: step >= 2, width: 82).position(agent)
                // 1: copies of the skills slide in behind janus-skills, one after another.
                if step == 0 {
                    ForEach(chips.indices, id: \.self) { i in
                        let t = span(p, 0.08 * Double(i), 0.55 + 0.08 * Double(i))
                        if t > 0 && t < 1 { Packet().position(along(chips[i], hub, 54, 52, t)).opacity(1 - t * 0.4) }
                    }
                }
                // 2: one description reaches the session's prompt.
                if step == 1, prompt > 0, prompt < 1 {
                    Packet(label: L.text("1 条描述")).position(along(hub, agent, 52, 41, prompt))
                }
                // 3: the Agent looks the task up in the catalog: reads it whole, or greps it and
                // gets back the matching lines.
                if step == 2 {
                    if searched {
                        let ask = span(p, 0.05, 0.45), back = span(p, 0.5, 0.9)
                        if ask > 0 && ask < 1 { Packet(label: "grep").position(along(agent, hub, 41, 52, ask)) }
                        if back > 0 && back < 1 { Packet(label: L.text("相符的 2 行")).position(along(hub, agent, 52, 41, back)) }
                    } else {
                        let read = span(p, 0.1, 0.8)
                        if read > 0 && read < 1 { Packet(label: L.text("目录")).position(along(hub, agent, 52, 41, read)) }
                    }
                }
                // 4: the Agent reads the one SKILL.md that fits.
                if step == 3 {
                    let back = span(p, 0.35, 0.9)
                    if back > 0 && back < 1 {
                        Packet(label: "SKILL.md").position(curve(readTo, readFrom, control: arc, back))
                    }
                    Text(L.text("任务用得上 \(shown[pick])")).font(.system(size: 9.5)).foregroundStyle(Color.accent)
                        .position(x: agent.x, y: agent.y + 34).opacity(span(p, 0, 0.25))
                }
            }
        }
    }

    private func curve(_ a: CGPoint, _ b: CGPoint, control c: CGPoint, _ t: Double) -> CGPoint {
        let u = 1 - t
        return CGPoint(x: u * u * a.x + 2 * u * t * c.x + t * t * b.x, y: u * u * a.y + 2 * u * t * c.y + t * t * b.y)
    }
}

/// The MCP router: sessions see one `janus-tools`; a server starts on its first call, the run's
/// sessions share it, and it stops once idle.
private struct McpRouteScene: View {
    let names: [String]
    let step: Int
    let p: Double

    var body: some View {
        let servers = Array((names.isEmpty ? ["github", "browser", "sqlite"] : names).prefix(3))
        GeometryReader { g in
            let w = g.size.width, h = g.size.height
            let ys = [0.2, 0.5, 0.8].map { h * $0 }
            let agents = ys.map { CGPoint(x: w * 0.14, y: $0) }
            let hub = CGPoint(x: w * 0.48, y: h * 0.5)
            let boxes = servers.indices.map { CGPoint(x: w * 0.85, y: ys[$0 + (servers.count == 1 ? 1 : 0)]) }
            let wire = step == 0 ? span(p, 0.05, 0.6) : 1
            let running = runningLevel
            let green = Tone.green.color
            ZStack {
                Canvas { ctx, _ in
                    for a in agents {
                        var link = Path(); link.move(to: a); link.addLine(to: mix(a, hub, wire))
                        ctx.stroke(link, with: .color(Color.accent.opacity(0.55)), lineWidth: 1.5)
                    }
                    for (i, b) in boxes.enumerated() {
                        var link = Path(); link.move(to: hub); link.addLine(to: b)
                        let on = i == 0 ? running : 0
                        ctx.stroke(link, with: .color(on > 0 ? green.opacity(0.3 + 0.5 * on) : .secondary.opacity(0.25)),
                                   style: StrokeStyle(lineWidth: on > 0 ? 1.5 : 1, dash: on > 0 ? [] : [3, 3]))
                    }
                }
                ForEach(agents.indices, id: \.self) { i in
                    Node(title: i == 0 ? L.text("负责人") : L.text("子任务 T\(i)"), width: 76).position(agents[i])
                }
                Node(title: "janus-tools", caption: L.text("每个会话一个"), lit: true, width: 92).position(hub)
                ForEach(boxes.indices, id: \.self) { i in
                    let on = i == 0 ? running : 0
                    Node(title: servers[i], caption: i == 0 ? serverState : L.text("未启动"), lit: on > 0.5, tone: green, width: 84)
                        .opacity(0.5 + 0.5 * max(on, i == 0 ? 0.3 : 0))
                        .position(boxes[i])
                }
                switch step {
                case 1: call(from: agents[0], hub: hub, to: boxes[0], p)
                case 2:
                    call(from: agents[1], hub: hub, to: boxes[0], p * 1.25)
                    call(from: agents[2], hub: hub, to: boxes[0], p * 1.25 - 0.3)
                default: EmptyView()
                }
            }
        }
    }

    /// How lit the first server is: off until the first call reaches it, on while in use, fading when idle.
    private var runningLevel: Double {
        switch step {
        case 1: return span(p, 0.75, 0.95)
        case 2: return 1
        case 3: return 1 - span(p, 0.35, 0.8)
        default: return 0
        }
    }

    private var serverState: String {
        switch step {
        case 1: return p < 0.75 ? L.text("未启动") : L.text("启动")
        case 2: return L.text("运行中 · 共用")
        case 3: return p < 0.35 ? L.text("闲置") : L.text("已停止")
        default: return L.text("未启动")
        }
    }

    /// A call: from a session to `janus-tools`, then on to the server.
    @ViewBuilder private func call(from a: CGPoint, hub: CGPoint, to b: CGPoint, _ p: Double) -> some View {
        if p > 0 && p < 0.8 {
            let at = p < 0.4 ? along(a, hub, 38, 46, ease(p / 0.4)) : along(hub, b, 46, 42, ease((p - 0.4) / 0.4))
            Packet().position(at)
        }
    }
}
