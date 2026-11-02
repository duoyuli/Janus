import AppKit
import SwiftUI

/// The run's progress board: one timeline of how this run moved forward, rebuilt from the event
/// log. Rows are tasks, bars are their attempts, connectors are handoffs from upstream tasks, and
/// decisions and open items sit on a lane above. A bar opens its task; a mark opens its event in
/// the audit log, which is where details are read.
// @project-doc docs/architecture/frontend.md#run_timeline
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
                HStack(alignment: .firstTextBaseline) {
                    Text("进展").font(.title3.weight(.semibold))
                    Text(verbatim: summary).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("在审计日志中查看全部") { run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""; run.highlight = nil; run.show(.audit) }
                        .buttonStyle(.link).font(.callout)
                }
                .padding(.top, 6)
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

    /// "3 个任务 · 完成 2"
    private var summary: String {
        guard !st.order.isEmpty else { return "" }
        let done = st.orderedTasks.filter { $0.status == "succeeded" }.count
        return "\(st.order.count) 个任务 · 完成 \(done)"
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
                let ended = ["succeeded", "failed"].contains(a.status)
                x.bars.append(TimelineBar(id: "\(t.id)#\(a.n)", start: a.start, end: a.end ?? now, attempt: a.n,
                                          status: a.end != nil && ended ? a.status : t.status, running: a.end == nil))
            }
            if t.attempts.isEmpty, waits, ["pending", "ready", "assigned", "blocked"].contains(t.status) {
                let waiting = t.deps.filter { st.tasks[$0]?.status != "succeeded" }
                x.waiting = t.status == "blocked" ? "等待 \(t.blockedBy) 修复" : waiting.isEmpty ? "等待调度" : "等待 \(waiting.joined(separator: "、"))"
            }
            if let r = f.results.last(where: { $0.task == t.id }) {
                x.result = TimelineMark(id: r.key, seq: r.seq, ts: r.ts, kind: .result, title: L.short(L.plain(r.summary), 120), by: r.by)
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
            TimelineMark(id: x.key, seq: x.seq, ts: x.ts, kind: .decision, title: "\(x.title)：\(L.short(L.plain(x.text), 120))", by: x.by)
        }
        for x in f.open {
            guard let seq = x.seq, let e = st.event(seq) else { continue }
            out.append(TimelineMark(id: x.key, seq: seq, ts: e.ts, kind: .open, title: x.title, by: x.agents.first ?? e.actor))
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
