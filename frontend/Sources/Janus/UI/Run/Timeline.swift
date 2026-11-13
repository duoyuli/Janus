import SwiftUI

/// Something that happened at one moment, backed by one event: a decision or an open item on the
/// timeline's top lane, or a task's conclusion or artifact at the end of its bar.
struct TimelineMark: Identifiable {
    enum Kind { case result, artifact, decision, open }
    let id: String
    let seq: Int
    let ts: Double
    let kind: Kind
    let title: String
    /// Who made it ("user", "lead" or an agent id).
    var by = ""
    /// For an open item: what it is (`OpenFact.kind`), so each kind has its own glyph.
    var what = ""
    /// For an open item: it waits on the person, not the lead.
    var you = false

    /// Open items, by kind: glyph and name. The person's are red, the lead's amber.
    static let openKinds: [String: (String, String)] = [
        "approval": ("hand.raised.fill", "等你审批"), "input": ("square.and.pencil", "等你填写"),
        "question": ("questionmark.bubble.fill", "等你决定"), "session": ("text.bubble.fill", "等你的消息"),
        "budget": ("hourglass.bottomhalf.filled", "预算用完"), "lead_stall": ("hourglass", "主控可能卡住"),
        "conflict": ("arrow.triangle.merge", "同时改了一个文件"), "overstep": ("arrow.right.to.line", "越界"),
        "trace": ("doc.badge.ellipsis", "结果和工作目录对不上"), "stall": ("hourglass", "可能卡住"),
        "blocker": ("exclamationmark.octagon.fill", "阻塞"), "lead_question": ("questionmark.circle.fill", "问主控"),
        "failed": ("xmark.octagon.fill", "失败"), "blocked": ("pause.circle.fill", "等依赖"),
    ]

    var symbol: String {
        switch kind {
        case .result: return "checkmark.seal.fill"
        case .artifact: return "doc.fill"
        case .decision: return by == "user" ? "person.crop.circle.fill" : "diamond.fill"
        case .open: return Self.openKinds[what]?.0 ?? "exclamationmark.triangle.fill"
        }
    }
    var tone: Tone {
        switch kind {
        case .result: return .green
        case .artifact: return .blue
        case .decision: return .violet
        case .open: return you ? .red : .amber
        }
    }
    var kindName: String {
        switch kind {
        case .result: return "结论"
        case .artifact: return "产物"
        case .decision: return "决定"
        case .open: return Self.openKinds[what]?.1 ?? "待处理"
        }
    }
}

/// One attempt of a task, from when it started to when it stopped (or now, while it runs).
struct TimelineBar: Identifiable {
    let id: String
    let start: Double
    let end: Double
    let attempt: Int
    let status: String
    let running: Bool
}

/// One task on the timeline: who does it, what it waits for, its attempts and what it produced.
struct TimelineTask: Identifiable {
    let id: String
    let title: String
    let agent: String
    let status: String
    let deps: [String]
    var bars: [TimelineBar] = []
    /// Not started yet: what it waits for.
    var waiting: String?
    var result: TimelineMark?
    var artifacts: [TimelineMark] = []
}

/// Maps time to x with long idle stretches cut out: active spans share the width at one scale,
/// and each gap longer than a few minutes becomes a fixed-width break labeled with its length.
struct TimeScale {
    struct Seg { let t0: Double; let t1: Double; let x0: CGFloat }
    private(set) var segs: [Seg] = []
    private(set) var k: CGFloat = 0
    static let breakW: CGFloat = 36

    /// `untilLast`: the last span ends where it ends (a live run's "now"), not padded out to a
    /// minimum width, so nothing but empty space follows "now".
    init(spans: [(Double, Double)], width: CGFloat, lead: CGFloat, trail: CGFloat, untilLast: Bool = false) {
        let iv = spans.map { ($0.0, Swift.max($0.0, $0.1)) }.sorted { $0.0 < $1.0 }
        guard let lo = iv.first?.0, let hi = iv.map(\.1).max() else { return }
        let threshold = Swift.max(180_000, (hi - lo) * 0.08)
        var merged: [(Double, Double)] = []
        for (a, b) in iv {
            if let last = merged.last, a - last.1 <= threshold { merged[merged.count - 1].1 = Swift.max(last.1, b) }
            else { merged.append((a, b)) }
        }
        // A lone moment still gets some room.
        let active0 = merged.reduce(0) { $0 + ($1.1 - $1.0) }
        let minDur = Swift.max(20_000, active0 * 0.06)
        for i in merged.indices where merged[i].1 - merged[i].0 < minDur && !(untilLast && i == merged.count - 1 && merged[i].1 > merged[i].0) {
            let room = i + 1 < merged.count ? merged[i + 1].0 - merged[i].0 : .infinity
            merged[i].1 = merged[i].0 + Swift.min(minDur, room)
        }
        let active = merged.reduce(0) { $0 + ($1.1 - $1.0) }
        let avail = Swift.max(width - lead - trail - CGFloat(merged.count - 1) * Self.breakW, 40)
        k = avail / CGFloat(Swift.max(active, 1))
        var x = lead
        for (a, b) in merged {
            segs.append(Seg(t0: a, t1: b, x0: x))
            x += CGFloat(b - a) * k + Self.breakW
        }
    }

    func x(_ ts: Double) -> CGFloat {
        guard var s = segs.first else { return 0 }
        for g in segs where g.t0 <= ts { s = g }
        return s.x0 + CGFloat(Swift.min(Swift.max(ts, s.t0), s.t1) - s.t0) * k
    }

    func end(_ s: Seg) -> CGFloat { s.x0 + CGFloat(s.t1 - s.t0) * k }

    /// The breaks: where each one starts and how long the idle stretch was.
    var breaks: [(x: CGFloat, ms: Double)] {
        zip(segs, segs.dropFirst()).map { (end($0), $1.t0 - $0.t1) }
    }

    /// Clock labels at a round step, about one per 90 points, never closer than 46 points; each
    /// span after a break is labeled where it starts.
    func ticks() -> [(x: CGFloat, ts: Double, seconds: Bool)] {
        guard k > 0 else { return [] }
        let steps: [Double] = [10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400].map { $0 * 1000 }
        let step = steps.first { CGFloat($0) * k >= 90 } ?? steps.last!
        let seconds = step < 60_000
        var out: [(x: CGFloat, ts: Double, seconds: Bool)] = []
        for (i, s) in segs.enumerated() {
            if i > 0 { out.append((s.x0, s.t0, seconds)) }
            var t = (s.t0 / step).rounded(.up) * step
            while t <= s.t1 {
                let px = x(t)
                if out.last.map({ px - $0.x >= 46 }) ?? true { out.append((px, t, seconds)) }
                t += step
            }
        }
        return out
    }
}

/// The run as a Gantt chart: one row per task, time left to right. A bar is an attempt, colored by
/// who did it; a connector runs from each upstream task to the task it handed over to. Decisions
/// and open items sit on a lane above. Idle stretches are cut down to a labeled break, and tasks
/// not started yet wait after "now".
// @project-doc docs/architecture/frontend.md#run_timeline
struct RunTimeline: View {
    let run: RunModel
    let tasks: [TimelineTask]
    let milestones: [TimelineMark]
    let now: Double
    let live: Bool
    /// Opens a mark's event (in the audit log).
    let onOpen: (TimelineMark) -> Void

    @Local private var hovered: String?

    private let labelW: CGFloat = 220
    private let rowH: CGFloat = 40
    private let axisH: CGFloat = 22
    private var laneH: CGFloat { milestones.isEmpty ? 0 : 32 }
    private var queued: Bool { tasks.contains { $0.waiting != nil } }
    private var height: CGFloat { axisH + laneH + CGFloat(tasks.count) * rowH }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            labels.frame(width: labelW)
            GeometryReader { g in chart(scale(g.size.width), width: g.size.width) }
                .frame(height: height)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func scale(_ w: CGFloat) -> TimeScale {
        var spans: [(Double, Double)] = []
        for t in tasks { for b in t.bars { spans.append((b.start, b.end)) } }
        for t in tasks { for m in [t.result].compactMap({ $0 }) + t.artifacts { spans.append((m.ts, m.ts)) } }
        for m in milestones { spans.append((m.ts, m.ts)) }
        if live || queued { spans.append((now, now)) }
        return TimeScale(spans: spans, width: w, lead: 12, trail: queued ? 132 : 56, untilLast: live)
    }

    private func rowTop(_ i: Int) -> CGFloat { axisH + laneH + CGFloat(i) * rowH }

    // MARK: Labels

    private var labels: some View {
        VStack(alignment: .leading, spacing: 0) {
            Color.clear.frame(height: axisH)
            if !milestones.isEmpty {
                Label("决定与待处理", systemImage: "diamond")
                    .help("◆ 决定 · 红色：等你处理 · 橙色：主控在处理。悬停图标看是什么，点按打开")
                    .font(.caption).foregroundStyle(.secondary)
                    .frame(height: laneH, alignment: .center)
            }
            ForEach(tasks) { t in
                Button { run.show(.task(t.id)) } label: { taskLabel(t) }
                    .buttonStyle(.plain)
                    .help(labelHelp(t))
            }
        }
    }

    private func taskLabel(_ t: TimelineTask) -> some View {
        let (label, tone) = L.task(t.status)
        let w = run.who(t.agent)
        return HStack(spacing: 8) {
            Image(systemName: Self.statusSymbol(tone))
                .foregroundStyle(tone.color)
                .font(.system(size: 13))
                .frame(width: 16)
                .accessibilityLabel(label)
            VStack(alignment: .leading, spacing: 2) {
                Text(t.title).font(.callout).lineLimit(1).truncationMode(.tail)
                HStack(spacing: 4) {
                    Avatar(initials: w.initials, color: w.color, size: 14)
                    Text(verbatim: "\(w.name) · \(t.id)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 6)
        .frame(height: rowH)
        .background(hovered == t.id ? Color.secondary.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 6))
        .contentShape(Rectangle())
        .onHover { hovered = $0 ? t.id : (hovered == t.id ? nil : hovered) }
    }

    static func statusSymbol(_ tone: Tone) -> String {
        switch tone {
        case .green: return "checkmark.circle.fill"
        case .red: return "xmark.circle.fill"
        case .amber: return "exclamationmark.circle.fill"
        case .blue, .violet: return "circle.lefthalf.filled"
        default: return "circle.dashed"
        }
    }

    private func labelHelp(_ t: TimelineTask) -> String {
        var lines = ["\(t.id) \(t.title)", "执行者：\(run.who(t.agent).name) · \(L.task(t.status).0)"]
        let ups = t.deps.map { d in "\(d)（\(run.who(tasks.first { $0.id == d }?.agent ?? "").name)）" }
        if !ups.isEmpty { lines.append("上游交接：\(ups.joined(separator: "、"))") }
        if let w = t.waiting { lines.append("还没开始 · \(w)") }
        lines.append("点按查看任务")
        return lines.joined(separator: "\n")
    }

    // MARK: Chart

    @ViewBuilder private func chart(_ s: TimeScale, width w: CGFloat) -> some View {
        ZStack(alignment: .topLeading) {
            // Row separators and the hovered row.
            ForEach(Array(tasks.enumerated()), id: \.offset) { i, t in
                Rectangle().fill(hovered == t.id ? Color.secondary.opacity(0.06) : .clear)
                    .frame(width: w, height: rowH).offset(y: rowTop(i))
                Rectangle().fill(Color.line.opacity(0.5)).frame(width: w, height: 1).offset(y: rowTop(i))
            }
            // Clock labels and grid lines.
            ForEach(Array(ticks(s).enumerated()), id: \.offset) { _, tk in
                Rectangle().fill(Color.line.opacity(0.45)).frame(width: 1, height: height - axisH).offset(x: tk.x, y: axisH)
                Text(verbatim: String(L.clock(tk.ts).dropLast(tk.seconds ? 0 : 3)))
                    .font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
                    .fixedSize().offset(x: tk.x + 3, y: 4)
            }
            // Idle stretches, cut short.
            ForEach(Array(s.breaks.enumerated()), id: \.offset) { _, b in breakView(b.x, b.ms) }
            connectors(s, w)
            ForEach(Array(tasks.enumerated()), id: \.offset) { i, t in row(t, i, s, w) }
            ForEach(placedMilestones(s), id: \.0.id) { m, px in milestone(m, px) }
            if live {
                let px = s.x(now)
                Rectangle().fill(Color.accent).frame(width: 1.5, height: height - axisH).offset(x: px, y: axisH)
                Text("现在").font(.caption2.weight(.semibold)).foregroundStyle(Color.accent)
                    .padding(.horizontal, 4).background(Color.card)
                    .fixedSize().offset(x: px - 12, y: 3)
            }
        }
        .frame(width: w, height: height, alignment: .topLeading)
        .clipped()
    }

    /// Clock labels, minus the ones "现在" would cover.
    private func ticks(_ s: TimeScale) -> [(x: CGFloat, ts: Double, seconds: Bool)] {
        let nx = s.x(now)
        return s.ticks().filter { !live || $0.x < nx - 52 || $0.x > nx + 34 }
    }

    private func breakView(_ x: CGFloat, _ ms: Double) -> some View {
        Rectangle().fill(Color.secondary.opacity(0.07))
            .overlay(alignment: .leading) { Rectangle().fill(Color.line).frame(width: 1) }
            .overlay(alignment: .trailing) { Rectangle().fill(Color.line).frame(width: 1) }
            .overlay { Image(systemName: "ellipsis").font(.caption).foregroundStyle(.secondary) }
            .frame(width: TimeScale.breakW - 8, height: height - axisH)
            .offset(x: x + 4, y: axisH)
        .help("这段时间没有进展：\(Self.idle(ms))")
        .accessibilityLabel("空闲 \(Self.idle(ms))")
    }

    static func idle(_ ms: Double) -> String {
        let m = Int((ms / 60_000).rounded())
        return m < 60 ? "\(m) 分钟" : m % 60 == 0 ? "\(m / 60) 小时" : "\(m / 60) 小时 \(m % 60) 分"
    }

    // MARK: Rows

    /// Where a task's waiting bar goes: the space after "now".
    private func waitingSpan(_ s: TimeScale, _ w: CGFloat) -> (CGFloat, CGFloat) {
        let x0 = s.x(now) + 8
        return (x0, Swift.max(40, w - x0 - 12))
    }

    @ViewBuilder private func row(_ t: TimelineTask, _ i: Int, _ s: TimeScale, _ w: CGFloat) -> some View {
        let y = rowTop(i) + (rowH - 22) / 2
        let who = run.who(t.agent)
        let color = Color(hex: Avatar.neutral.contains(who.color.lowercased()) ? Avatar.slate : who.color)
        ForEach(t.bars) { b in
            let x0 = s.x(b.start), x1 = Swift.max(s.x(b.end), x0 + 4)
            attemptBar(t, b, color: color, width: x1 - x0).offset(x: x0, y: y)
        }
        if let note = t.waiting {
            let (x0, width) = waitingSpan(s, w)
            Button { run.show(.task(t.id)) } label: {
                Text(verbatim: note)
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    .padding(.horizontal, 6)
                    .frame(width: width, height: 22, alignment: .leading)
                    .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(Color.secondary.opacity(0.5), style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .offset(x: x0, y: y)
            .help("\(t.id) \(t.title)\n还没开始 · \(note)\n点按查看任务")
            .accessibilityLabel("\(t.id) \(t.title)，还没开始，\(note)")
        }
        if let last = t.bars.last, t.result != nil || !t.artifacts.isEmpty {
            outcome(t).offset(x: Swift.min(s.x(last.end) + 6, w - 52), y: y + 2)
        }
    }

    private func attemptBar(_ t: TimelineTask, _ b: TimelineBar, color: Color, width: CGFloat) -> some View {
        let (label, tone) = L.task(b.status)
        let failed = tone == .red
        // Work in progress is the one thing in signal cyan; finished work is its agent's tone.
        let fill = failed ? Tone.red.color : b.running ? Color.accent : color
        let dur = L.dur(b.end - b.start)
        let text = b.running ? "进行中" : b.attempt == 1 ? dur : width > 130 ? "第 \(b.attempt) 次 · \(dur)" : "第 \(b.attempt) 次"
        return Button { run.show(.task(t.id)) } label: {
            // A finished attempt is a solid bar in its agent's color; one still running is its outline.
            Text(verbatim: text)
                .font(.caption2.weight(.semibold).monospacedDigit())
                .foregroundStyle(width > 64 ? (b.running ? fill : .white) : .clear)
                .lineLimit(1)
                .padding(.horizontal, 7)
                .frame(width: width, height: 22, alignment: .leading)
                .background(b.running ? AnyShapeStyle(fill.opacity(0.14)) : AnyShapeStyle(LinearGradient(colors: [fill, fill.opacity(0.75)], startPoint: .leading, endPoint: .trailing)), in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(b.running ? fill.opacity(0.9) : .white.opacity(0.12), style: StrokeStyle(lineWidth: 1, dash: b.running ? [4, 2] : [])))
                .contentShape(Rectangle())
                .hoverable(5)
        }
        .buttonStyle(.plain)
        .help("\(t.id) \(t.title)\n尝试 \(b.attempt) · \(label) · \(L.clock(b.start)) – \(b.running ? "进行中" : L.clock(b.end))（\(L.dur(b.end - b.start))）\n点按查看任务")
        .accessibilityLabel("\(t.id) \(t.title)，尝试 \(b.attempt)，\(label)，用时 \(L.dur(b.end - b.start))")
    }

    /// What a task produced, right after its last bar: the conclusion and the artifacts.
    private func outcome(_ t: TimelineTask) -> some View {
        HStack(spacing: 2) {
            if let r = t.result { markButton(r, help: "结论 · \(L.clock(r.ts))\n\(r.title)\n点按在审计日志中查看") { Image(systemName: r.symbol) } }
            if let a = t.artifacts.last {
                let names = t.artifacts.map(\.title).joined(separator: "\n")
                markButton(a, help: "产物 \(t.artifacts.count) 个\n\(names)\n点按在审计日志中查看") {
                    HStack(spacing: 1) {
                        Image(systemName: a.symbol)
                        if t.artifacts.count > 1 { Text(verbatim: "\(t.artifacts.count)").font(.caption2.monospacedDigit()) }
                    }
                }
            }
        }
    }

    private func markButton<V: View>(_ m: TimelineMark, help: String, @ViewBuilder _ content: () -> V) -> some View {
        Button { onOpen(m) } label: {
            content()
                .font(.system(size: 12))
                .foregroundStyle(m.tone.color)
                .padding(.horizontal, 3)
                .frame(minWidth: 20, minHeight: 18)
                .contentShape(Rectangle())
                .hoverable(4)
        }
        .buttonStyle(.plain)
        .help(help)
        .accessibilityLabel("\(m.kindName)，\(m.title)")
    }

    // MARK: Handoffs

    /// A connector from each upstream task's last bar to where the task it handed over to starts.
    private func connectors(_ s: TimeScale, _ w: CGFloat) -> some View {
        let index = Dictionary(tasks.enumerated().map { ($1.id, $0) }, uniquingKeysWith: { a, _ in a })
        return Canvas { ctx, _ in
            let c = Color.secondary.opacity(0.55)
            for (i, t) in tasks.enumerated() {
                let xs: CGFloat
                if let b = t.bars.first { xs = s.x(b.start) } else if t.waiting != nil { xs = waitingSpan(s, w).0 } else { continue }
                let ys = rowTop(i) + rowH / 2
                for d in t.deps {
                    guard let j = index[d], let up = tasks[j].bars.last else { continue }
                    let xe = s.x(up.end), ye = rowTop(j) + rowH / 2
                    var p = Path()
                    p.move(to: CGPoint(x: xe, y: ye))
                    let xm = xe + 6
                    if xs - 4 >= xm {
                        p.addLine(to: CGPoint(x: xm, y: ye))
                        p.addLine(to: CGPoint(x: xm, y: ys))
                    } else {
                        // The downstream task starts before the upstream bar ends on screen: go
                        // around through the gap between the rows.
                        let yg = rowTop(i) - 1
                        p.addLine(to: CGPoint(x: xm, y: ye))
                        p.addLine(to: CGPoint(x: xm, y: yg))
                        p.addLine(to: CGPoint(x: xs - 8, y: yg))
                        p.addLine(to: CGPoint(x: xs - 8, y: ys))
                    }
                    p.addLine(to: CGPoint(x: xs - 1, y: ys))
                    ctx.stroke(p, with: .color(c), style: StrokeStyle(lineWidth: 1, lineJoin: .round))
                    var head = Path()
                    head.move(to: CGPoint(x: xs - 5, y: ys - 3.5)); head.addLine(to: CGPoint(x: xs - 1, y: ys)); head.addLine(to: CGPoint(x: xs - 5, y: ys + 3.5))
                    ctx.stroke(head, with: .color(c), style: StrokeStyle(lineWidth: 1.2, lineCap: .round, lineJoin: .round))
                }
            }
        }
        .frame(width: w, height: height)
        .allowsHitTesting(false)
    }

    // MARK: Milestones

    /// Milestones never sit on top of each other: each is nudged right of the one before.
    private func placedMilestones(_ s: TimeScale) -> [(TimelineMark, CGFloat)] {
        var last: CGFloat = -.infinity
        return milestones.sorted { $0.ts < $1.ts }.map { m in
            let px = Swift.max(s.x(m.ts), last + 20)
            last = px
            return (m, px)
        }
    }

    private func milestone(_ m: TimelineMark, _ px: CGFloat) -> some View {
        let who = m.by == "user" ? "你" : run.who(m.by).name
        return Button { onOpen(m) } label: {
            Image(systemName: m.symbol)
                .font(.system(size: 13))
                .foregroundStyle(m.tone.color)
                .frame(width: 20, height: 20)
                .background(Circle().fill(Color.card))
                .contentShape(Circle())
                .hoverable(10)
        }
        .buttonStyle(.plain)
        .offset(x: px - 10, y: axisH + (laneH - 20) / 2)
        .help("\(m.kindName) · \(who) · \(L.clock(m.ts))\n\(m.title)\n点按在审计日志中查看")
        .accessibilityLabel("\(m.kindName)，\(who)，\(L.clock(m.ts))，\(m.title)")
    }
}
