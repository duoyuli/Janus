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
        let f = boardFacts(st) { run.who($0).name }
        let now = nowMs
        let tasks = timelineTasks(f, now)
        let milestones = milestoneMarks(f)
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

    /// Tasks in the order they started, then the ones still waiting in the order the lead gave them. A task that
    /// neither ran nor waits (cancelled or skipped before it started) stays as a row without a bar.
    private func timelineTasks(_ f: BoardFacts, _ now: Double) -> [TimelineTask] {
        let waits = run.state.active || run.isReplaying
        var out: [TimelineTask] = []
        for t in st.orderedTasks {
            var x = TimelineTask(id: t.id, title: t.title, agent: t.agent, status: t.status, deps: t.deps)
            for a in t.attempts {
                // A finished attempt keeps its own outcome; the running one shows the task's status.
                let ended = a.status != .running
                x.bars.append(TimelineBar(id: "\(t.id)#\(a.n)", start: a.start, end: a.end ?? now, attempt: a.n,
                                          status: a.end != nil && ended ? a.status.asTask : t.status, running: a.end == nil))
            }
            if t.attempts.isEmpty, waits, t.status.isWaitingToStart {
                let waiting = t.deps.filter { st.tasks[$0]?.status != .succeeded }
                x.waiting = t.status == .blocked ? L.text("等待 \(t.blockedBy) 修复") : waiting.isEmpty ? L.text("等待调度") : L.text("等待 \(waiting.joined(separator: L.text("、")))")
            }
            if let r = f.results.last(where: { $0.task == t.id }) {
                // The mark itself stays small; its tooltip and VoiceOver label carry the evidence
                // methods. Each check with its evidence is on the task page, the delivery and the
                // audit log.
                let evidence = EvidenceSummary(accept: t.accept, checks: r.checks)
                let counts = r.checks.isEmpty ? "" : L.text(" · 证据 \(evidence.countsText)")
                x.result = TimelineMark(id: r.key, seq: r.seq, ts: r.ts, kind: .result, title: L.short(L.plain(r.summary), 120) + counts, by: r.by)
            }
            x.artifacts = f.arts.filter { $0.a.task == t.id && !$0.old }.map { a in
                TimelineMark(id: "a\(a.a.seq)", seq: a.a.seq, ts: st.event(a.a.seq)?.ts ?? st.run.start, kind: .artifact, title: a.a.name, by: a.a.by)
            }
            out.append(x)
        }
        let started = out.filter { !$0.bars.isEmpty }.sorted { $0.bars[0].start < $1.bars[0].start }
        return started + out.filter { $0.bars.isEmpty }
    }

    /// Decisions (the person's, the lead's) and open items, at the moment they happened.
    private func milestoneMarks(_ f: BoardFacts) -> [TimelineMark] {
        var out = f.decisions.map { x in
            TimelineMark(id: x.key, seq: x.seq, ts: x.ts, kind: .decision, title: L.text("\(x.title)：\(L.short(L.plain(x.text), 120))"), by: x.by)
        }
        for x in f.open {
            guard let seq = x.seq, let e = st.event(seq) else { continue }
            out.append(TimelineMark(id: x.id, seq: seq, ts: e.ts, kind: .open, title: x.title, by: x.agents.first ?? e.actor, what: x.kind, you: x.you))
        }
        return out
    }
}

struct EventIcon: View {
    let type: EventKind
    var body: some View {
        let tone = L.tone(of: type)
        Text(L.icon[type] ?? "·")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(tone?.color ?? .secondary)
            .background((tone?.color ?? .secondary).opacity(0.12), in: Circle())
    }
}
