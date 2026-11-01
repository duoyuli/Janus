import SwiftUI

/// Something on the run's timeline that happened at one moment, backed by one event.
struct TimelineMark: Identifiable {
    enum Kind { case result, artifact, decision, handoff, open }
    let id: String
    let seq: Int
    let ts: Double
    /// The row (agent) it happened on; for a handoff, the agent that handed over.
    let row: String
    let kind: Kind
    let title: String
    /// A handoff's receiving agent.
    var to: String?

    var symbol: String {
        switch kind {
        case .result: return "checkmark.circle.fill"
        case .artifact: return "doc.fill"
        case .decision: return "diamond.fill"
        case .handoff: return "arrow.right.circle.fill"
        case .open: return "exclamationmark.triangle.fill"
        }
    }
    var tone: Tone {
        switch kind {
        case .result: return .green
        case .artifact: return .blue
        case .decision: return .violet
        case .handoff: return .gray
        case .open: return .amber
        }
    }
    var kindName: String {
        switch kind {
        case .result: return "结论"
        case .artifact: return "产物"
        case .decision: return "决定"
        case .handoff: return "交接"
        case .open: return "待处理"
        }
    }
}

/// One attempt of a task, from when it started to when it stopped; or, with attempt 0, a task
/// that has not started, with what it waits for in `note`.
struct TimelineBar: Identifiable {
    let id: String
    let task: String
    let title: String
    let row: String
    let start: Double
    let end: Double
    let attempt: Int
    let status: String
    let running: Bool
    let note: String
}

/// The run as a Gantt chart: one row per agent, time left to right, a line at "now". Attempts are
/// bars; marks sit at the moment their event happened; a handoff is a dashed arrow from the agent
/// that handed over to the one that received it. Tasks not started yet wait after "now".
// @project-doc docs/architecture/frontend.md#run_timeline
struct RunTimeline: View {
    let run: RunModel
    let rows: [String]
    let bars: [TimelineBar]
    let queued: [TimelineBar]
    let marks: [TimelineMark]
    let t0: Double
    let now: Double
    /// Opens a mark's event (in the audit log).
    let onOpen: (TimelineMark) -> Void

    @Local private var zoom: CGFloat = 1

    private let labelW: CGFloat = 116
    private let rowH: CGFloat = 46
    private let axisH: CGFloat = 24

    var body: some View {
        let height = axisH + CGFloat(rows.count) * rowH
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 0) {
                // Row labels stay put while the chart scrolls.
                VStack(alignment: .leading, spacing: 0) {
                    Color.clear.frame(height: axisH)
                    ForEach(rows, id: \.self) { id in
                        let w = run.who(id)
                        HStack(spacing: 6) {
                            if id == "user" { Image(systemName: "person.crop.circle").foregroundStyle(.secondary) }
                            else { Avatar(initials: w.initials, color: w.color, size: 20) }
                            Text(id == "user" ? "你" : w.name).font(.callout).lineLimit(1)
                        }
                        .frame(width: labelW, height: rowH, alignment: .leading)
                        .help([w.role, w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · "))
                    }
                }
                GeometryReader { g in
                    let plotW = max(g.size.width, 320) * zoom
                    ScrollView(.horizontal) {
                        chart(width: plotW, height: height)
                    }
                    .scrollIndicators(zoom > 1 ? .visible : .hidden)
                }
                .frame(height: height + (zoom > 1 ? 12 : 0))
            }
            HStack(spacing: 14) {
                legend("rectangle.fill", "执行", .blue)
                legend("checkmark.circle.fill", "结论", .green)
                legend("doc.fill", "产物", .blue)
                legend("diamond.fill", "决定", .violet)
                legend("arrow.right.circle.fill", "交接", .gray)
                Spacer()
                Text("点按标记在审计日志中查看").foregroundStyle(.tertiary)
                Button { zoom = max(1, zoom / 2) } label: { Label("缩小", systemImage: "minus.magnifyingglass") }
                    .disabled(zoom <= 1).help("缩小时间轴")
                Button { zoom = min(16, zoom * 2) } label: { Label("放大", systemImage: "plus.magnifyingglass") }
                    .disabled(zoom >= 16).help("放大时间轴，横向滚动查看")
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .font(.caption)
            .foregroundStyle(.secondary)
        }
    }

    private func legend(_ symbol: String, _ text: String, _ tone: Tone) -> some View {
        HStack(spacing: 4) { Image(systemName: symbol).foregroundStyle(tone.color.opacity(0.8)); Text(text) }
    }

    // MARK: Geometry

    /// The part of the width after "now" kept for tasks still waiting.
    private var tail: CGFloat { queued.isEmpty ? 0 : 0.24 }

    private var t1: Double { max(now, t0 + 60_000) }

    private func x(_ ts: Double, _ w: CGFloat) -> CGFloat {
        let span = max(t1 - t0, 1)
        return 10 + CGFloat((ts - t0) / span) * (w * (1 - tail) - 20)
    }

    private func rowTop(_ id: String) -> CGFloat {
        axisH + CGFloat(rows.firstIndex(of: id) ?? 0) * rowH
    }

    @ViewBuilder private func chart(width w: CGFloat, height: CGFloat) -> some View {
        ZStack(alignment: .topLeading) {
            // Row bands and the time axis.
            ForEach(Array(rows.enumerated()), id: \.offset) { i, _ in
                Rectangle().fill(i % 2 == 0 ? Color.secondary.opacity(0.05) : .clear)
                    .frame(width: w, height: rowH)
                    .offset(y: axisH + CGFloat(i) * rowH)
            }
            ForEach(ticks(w), id: \.self) { ts in
                let px = x(ts, w)
                Rectangle().fill(Color.line.opacity(0.6)).frame(width: 1, height: height - axisH).offset(x: px, y: axisH)
                Text(L.clock(ts).dropLast(3)).font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
                    .fixedSize().offset(x: px + 3, y: 4)
            }
            // Handoff arrows, under the marks.
            Canvas { ctx, _ in
                for m in marks where m.kind == .handoff {
                    guard let to = m.to, rows.contains(to), to != m.row else { continue }
                    let px = x(m.ts, w)
                    let y0 = rowTop(m.row) + 14
                    let down = rowTop(to) > rowTop(m.row)
                    let y1 = rowTop(to) + (down ? 4 : 24)
                    var p = Path()
                    p.move(to: CGPoint(x: px, y: y0)); p.addLine(to: CGPoint(x: px, y: y1))
                    let dir: CGFloat = down ? 1 : -1
                    var head = Path()
                    head.move(to: CGPoint(x: px - 4, y: y1 - 6 * dir)); head.addLine(to: CGPoint(x: px, y: y1)); head.addLine(to: CGPoint(x: px + 4, y: y1 - 6 * dir))
                    let c = Color.secondary.opacity(0.75)
                    ctx.stroke(p, with: .color(c), style: StrokeStyle(lineWidth: 1.2, dash: [3, 2]))
                    ctx.stroke(head, with: .color(c), style: StrokeStyle(lineWidth: 1.4, lineCap: .round, lineJoin: .round))
                }
            }
            .frame(width: w, height: height)
            .allowsHitTesting(false)
            ForEach(bars) { b in bar(b, w) }
            ForEach(queuedPlaced(w), id: \.0.id) { b, x0, width in waiting(b, x0, width) }
            ForEach(placed(w), id: \.0.id) { m, px in mark(m, px) }
            // Now, for a run still going.
            if run.state.active, !run.isReplaying {
                Rectangle().fill(Color.accent.opacity(0.8)).frame(width: 1.5, height: height - axisH).offset(x: x(now, w), y: axisH)
                Text("现在").font(.caption2.weight(.semibold)).foregroundStyle(Color.accent).fixedSize().offset(x: x(now, w) + 3, y: 4)
            }
        }
        .frame(width: w, height: height, alignment: .topLeading)
    }

    /// Clock times at a round step, about one per 140 points.
    private func ticks(_ w: CGFloat) -> [Double] {
        let span = t1 - t0
        guard span > 0 else { return [t0] }
        let n = max(2, Int(w * (1 - tail) / 140))
        let steps: [Double] = [5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200].map { $0 * 1000 }
        let step = steps.first { span / $0 <= Double(n) } ?? span / Double(n)
        var out: [Double] = []
        var t = (t0 / step).rounded(.up) * step
        while t <= t1 { out.append(t); t += step }
        return out
    }

    // MARK: Bars

    private func bar(_ b: TimelineBar, _ w: CGFloat) -> some View {
        let (label, tone) = L.task(b.status)
        let x0 = x(b.start, w), x1 = max(x(b.end, w), x0 + 6)
        return barBody(b, width: x1 - x0, tone: tone, dashed: b.running, filled: true)
            .offset(x: x0, y: rowTop(b.row) + rowH - 22)
            .help(barHelp(b, label))
            .accessibilityLabel("\(b.task) \(b.title)，尝试 \(b.attempt)，\(label)")
    }

    /// Waiting tasks share the space after "now" on their row.
    private func queuedPlaced(_ w: CGFloat) -> [(TimelineBar, CGFloat, CGFloat)] {
        let start = x(now, w) + 8
        let room = w - start - 8
        var out: [(TimelineBar, CGFloat, CGFloat)] = []
        for r in rows {
            let mine = queued.filter { $0.row == r }
            guard !mine.isEmpty else { continue }
            let each = (room - CGFloat(mine.count - 1) * 4) / CGFloat(mine.count)
            for (i, b) in mine.enumerated() { out.append((b, start + CGFloat(i) * (each + 4), max(each, 24))) }
        }
        return out
    }

    private func waiting(_ b: TimelineBar, _ x0: CGFloat, _ width: CGFloat) -> some View {
        barBody(b, width: width, tone: .gray, dashed: true, filled: false)
            .offset(x: x0, y: rowTop(b.row) + rowH - 22)
            .help("\(b.task) \(b.title)\n还没开始 · \(b.note)\n点按查看任务")
            .accessibilityLabel("\(b.task) \(b.title)，还没开始，\(b.note)")
    }

    private func barBody(_ b: TimelineBar, width: CGFloat, tone: Tone, dashed: Bool, filled: Bool) -> some View {
        Button { run.show(.task(b.task)) } label: {
            Text(verbatim: "\(b.task) \(b.title)")
                .font(.caption.weight(.medium))
                .lineLimit(1)
                .foregroundStyle(width > 40 ? (filled ? Color.primary : Color.secondary) : .clear)
                .padding(.horizontal, 5)
                .frame(width: width, height: 18, alignment: .leading)
                .background(tone.color.opacity(filled ? 0.22 : 0.06), in: RoundedRectangle(cornerRadius: 4))
                .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(tone.color.opacity(dashed ? 0.9 : 0.55), style: StrokeStyle(lineWidth: 1, dash: dashed ? [3, 2] : [])))
                .contentShape(Rectangle())
                .hoverable(4)
        }
        .buttonStyle(.plain)
    }

    private func barHelp(_ b: TimelineBar, _ label: String) -> String {
        let until = b.running ? "进行中" : L.clock(b.end)
        return "\(b.task) \(b.title)\n尝试 \(b.attempt) · \(label) · \(L.clock(b.start)) – \(until)（\(L.dur(b.end - b.start))）\n点按查看任务"
    }

    // MARK: Marks

    /// Marks on one row never sit on top of each other: each is nudged right of the one before.
    private func placed(_ w: CGFloat) -> [(TimelineMark, CGFloat)] {
        var last: [String: CGFloat] = [:]
        return marks.sorted { $0.ts < $1.ts }.map { m in
            var px = x(m.ts, w)
            if let l = last[m.row], px < l + 15 { px = l + 15 }
            last[m.row] = px
            return (m, px)
        }
    }

    private func mark(_ m: TimelineMark, _ px: CGFloat) -> some View {
        Button { onOpen(m) } label: {
            Image(systemName: m.symbol)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(m.tone.color)
                .frame(width: 18, height: 18)
                .background(Circle().fill(Color.card))
                .contentShape(Circle())
                .hoverable(9)
        }
        .buttonStyle(.plain)
        .offset(x: px - 9, y: rowTop(m.row) + 5)
        .help("\(m.kindName) · \(L.clock(m.ts))\(m.to.map { " → \(run.who($0).name)" } ?? "")\n\(m.title)\n点按在审计日志中查看")
        .accessibilityLabel("\(m.kindName)，\(L.clock(m.ts))，\(m.title)")
    }
}
