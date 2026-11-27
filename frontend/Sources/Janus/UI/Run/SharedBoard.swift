import AppKit
import SwiftUI

/// The run's progress board: one timeline of how this run moved forward, rebuilt from the event
/// log. Rows are tasks, bars are their attempts, connectors are handoffs from upstream tasks, and
/// decisions and open items sit on a lane above. A bar opens its task; a mark opens its event in
/// the audit log, which is where details are read. Above the lead's conversation it stays folded
/// to one line until opened (W1 F8/U3): it answers what the team is doing, not how every event
/// looked.
// @project-doc docs/architecture/20261031-frontend.md#run_timeline
struct ProgressBoard: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let now = nowMs
        let tasks = st.timeline["tasks"].array.map { TimelineTask($0, now: now) }
        let milestones = st.timeline["milestones"].array.map(TimelineMark.init)
        if !tasks.isEmpty || !milestones.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .center, spacing: 8) {
                    Text(L.text("进展")).font(.display(.title3))
                    Button(run.progressOpen ? L.text("收起进展") : L.text("展开进展")) { run.progressOpen.toggle() }
                        .buttonStyle(.link).font(.callout)
                        .help(L.text("按时间看谁做了什么、交给了谁"))
                        .accessibilityLabel(run.progressOpen ? L.text("收起进展") : L.text("展开进展"))
                    Spacer()
                    Button(L.text("在审计日志中查看全部")) { run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""; run.highlight = nil; run.show(.audit) }
                        .buttonStyle(.link).font(.callout)
                }
                .padding(.top, 6)
                if run.progressOpen {
                    Card(padding: 12) {
                        RunTimeline(run: run, tasks: tasks, milestones: milestones, now: now, live: run.state.active && !run.isReplaying) { m in
                            run.replay = nil
                            run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""
                            run.highlight = m.seq
                            run.show(.audit)
                        }
                    }
                }
            }
        }
    }

    private var nowMs: Double {
        run.state.active && !run.isReplaying ? Swift.max(st.run.now, L.nowMs()) : st.run.now
    }
}

/// An event kind's symbol, as the server names it (`EventMark`), drawn as the app's glyph.
struct EventIcon: View {
    let mark: EventMark

    static let glyph: [String: String] = [
        "created": "✦", "stop": "■", "pause": "Ⅱ", "start": "▶", "next": "▸", "assign": "↗", "fail": "✕", "retry": "↻",
        "reopen": "↺", "submit": "⇪", "done": "✓", "artifact": "▤", "handoff": "⇄", "question": "?", "escalate": "⇡",
        "write": "✎", "reply": "↩", "flag": "⚑", "delivered": "★", "blocked": "⊘", "forward": "→", "waiting": "◌",
        "tool": "⌘", "route": "↪", "quote": "❝", "hand": "✋", "interrupt": "‖", "warning": "⚠", "stall": "⏳", "split": "✂",
    ]

    var body: some View {
        let tone = mark.tone
        Text(Self.glyph[mark.icon] ?? "·")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(tone?.color ?? .secondary)
            .background((tone?.color ?? .secondary).opacity(0.12), in: Circle())
    }
}
