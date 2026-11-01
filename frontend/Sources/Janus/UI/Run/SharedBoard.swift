import AppKit
import SwiftUI

/// The run's progress board: one timeline of how this run moved forward, rebuilt from the event
/// log. Rows are agents, bars are task attempts (and tasks still waiting), marks are conclusions,
/// artifacts, decisions, open items and handoffs at the moment they happened. A mark opens its
/// event in the audit log, which is where details are read; a bar opens its task.
// @project-doc docs/architecture/frontend.md#run_timeline
struct ProgressBoard: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let f = boardFacts(st) { run.who($0).name }
        let marks = timelineMarks(f)
        let now = nowMs
        let bars = attemptBars(now)
        let queued = queuedBars
        let rows = timelineRows(marks: marks, bars: bars + queued)
        if !bars.isEmpty || !marks.isEmpty || !queued.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline) {
                    Text("进展").font(.title3.weight(.semibold))
                    Text(verbatim: summary).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("在审计日志中查看全部") { run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""; run.highlight = nil; run.show(.audit) }
                        .buttonStyle(.link).font(.callout)
                }
                .padding(.top, 6)
                Card(padding: 12) {
                    RunTimeline(run: run, rows: rows, bars: bars, queued: queued, marks: marks, t0: st.run.start, now: now) { m in
                        run.replay = nil
                        run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""
                        run.highlight = m.seq
                        run.show(.audit)
                    }
                }
            }
        }
    }

    /// "3 个任务 · 完成 2"
    private var summary: String {
        guard !st.order.isEmpty else { return "" }
        let done = st.orderedTasks.filter { $0.status == "succeeded" }.count
        return "\(st.order.count) 个任务 · 完成 \(done)"
    }

    private var nowMs: Double {
        run.state.active && !run.isReplaying ? Swift.max(st.run.now, L.nowMs()) : st.run.now
    }

    /// Lead first, then the team in its order, then "你" when the person decided something; only
    /// rows with something on them.
    private func timelineRows(marks: [TimelineMark], bars: [TimelineBar]) -> [String] {
        var ids = ["lead"] + run.team.map { $0["id"].str }.filter { $0 != "lead" }
        if marks.contains(where: { $0.row == "user" }) { ids.append("user") }
        let used = Set(marks.map(\.row) + marks.compactMap(\.to) + bars.map(\.row) + ["lead"])
        return ids.filter { used.contains($0) }
    }

    /// Where an actor's mark goes: its own row; the lead's for Janus and unknown actors.
    private func row(_ actor: String) -> String {
        if actor == "user" { return "user" }
        return actor == "lead" || run.team.contains { $0["id"].str == actor } ? actor : "lead"
    }

    private func attemptBars(_ now: Double) -> [TimelineBar] {
        var out: [TimelineBar] = []
        for t in st.orderedTasks {
            for a in t.attempts {
                // A finished attempt keeps its own outcome; the running one shows the task's status.
                let ended = ["succeeded", "failed", "rejected"].contains(a.status)
                let status: String = a.end != nil && ended ? a.status : t.status
                out.append(TimelineBar(id: "\(t.id)#\(a.n)", task: t.id, title: t.title, row: row(t.agent), start: a.start, end: a.end ?? now,
                                       attempt: a.n, status: status, running: a.end == nil, note: ""))
            }
        }
        return out
    }

    /// Tasks not started yet, drawn after "now" on their agent's row with what they wait for.
    private var queuedBars: [TimelineBar] {
        guard run.state.active || run.isReplaying else { return [] }
        return st.orderedTasks.filter { $0.attempts.isEmpty && ["created", "pending", "ready", "assigned", "blocked"].contains($0.status) }.map { t in
            let waiting = t.deps.filter { st.tasks[$0]?.status != "succeeded" }
            let note = t.status == "blocked" ? "等待 \(t.blockedBy) 修复" : waiting.isEmpty ? "等待调度" : "等待 \(waiting.joined(separator: "、"))"
            return TimelineBar(id: t.id, task: t.id, title: t.title, row: row(t.agent), start: 0, end: 0, attempt: 0, status: t.status, running: false, note: note)
        }
    }

    private func timelineMarks(_ f: BoardFacts) -> [TimelineMark] {
        var out: [TimelineMark] = []
        for x in f.results {
            out.append(TimelineMark(id: x.key, seq: x.seq, ts: x.ts, row: row(x.by), kind: .result, title: "\(x.task)：\(L.short(L.plain(x.summary), 80))"))
        }
        for x in f.arts {
            let ts = st.event(x.a.seq)?.ts ?? st.run.start
            out.append(TimelineMark(id: "a\(x.a.seq)", seq: x.a.seq, ts: ts, row: row(x.a.by), kind: .artifact, title: "\(x.a.task)：\(x.a.name)"))
        }
        for x in f.handoffs {
            // From the agent that did the upstream work to the one that receives it.
            let from = row(st.tasks[x.from]?.agent ?? x.by)
            out.append(TimelineMark(id: x.key, seq: x.seq, ts: x.ts, row: from, kind: .handoff,
                                    title: "\(x.from) → \(x.to)：\(L.short(L.plain(x.summary.isEmpty ? "上游没有提交结论" : x.summary), 80))", to: row(x.agent)))
        }
        for x in f.decisions {
            out.append(TimelineMark(id: x.key, seq: x.seq, ts: x.ts, row: row(x.by), kind: .decision, title: "\(x.title)：\(L.short(L.plain(x.text), 80))"))
        }
        for x in f.open {
            guard let seq = x.seq, let e = st.event(seq) else { continue }
            out.append(TimelineMark(id: x.key, seq: seq, ts: e.ts, row: row(x.agents.first ?? e.actor), kind: .open, title: x.title))
        }
        return out
    }
}

struct EventIcon: View {
    let type: String
    var body: some View {
        let tone = L.tone(of: type)
        Text(L.icon[type] ?? "·")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(tone?.color ?? .secondary)
            .background((tone?.color ?? .secondary).opacity(0.12), in: Circle())
    }
}

/// A window showing an artifact (a reply, a diff, a test report).
enum ArtifactWindow {
    @MainActor
    static func open(run: RunModel, path: String, title: String) {
        let view = ArtifactView(api: run.api, path: path)
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 820, height: 640), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        w.title = title
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: view)
        w.center()
        w.makeKeyAndOrderFront(nil)
    }
}

struct ArtifactView: View {
    let api: API
    let path: String
    @Local private var text: String?
    @Local private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(L.tilde(path)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("在 Finder 中显示") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
                Button("复制") { copyText(text ?? "") }.disabled(text == nil)
            }
            .padding(10)
            Divider()
            if let text {
                ScrollView([.vertical, .horizontal]) {
                    Text(text).font(.system(size: 12, design: .monospaced)).textSelection(.enabled).padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
            } else if let error {
                Text(error).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            do { text = try await api.text("/api/file?path=\(API.q(path))") } catch {
                // Deliverables can live outside the artifacts directory; this user can read them directly.
                if let t = try? String(contentsOfFile: path, encoding: .utf8) { text = t } else { self.error = error.localizedDescription }
            }
        }
    }
}
