import AppKit
import SwiftUI

/// Reviewing a task's changes, or the whole run's, inside Janus: the diff file by file, a comment on any line, and the
/// comments sent back as one message, to the lead (which decides: reply, redo, or accept) or to
/// the agent that did the task.
// @project-doc docs/architecture/20261031-frontend.md#diff_review
enum DiffWindow {
    @MainActor private static var windows: [String: NSWindow] = [:]

    @MainActor
    static func open(run: RunModel, artifact: Artifact) {
        let view = DiffReviewView(run: run, artifact: artifact)
        if let w = windows[artifact.path] {
            w.contentView = NSHostingView(rootView: view)
            w.makeKeyAndOrderFront(nil)
            return
        }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1040, height: 760), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        w.title = artifact.ofRun ? L.text("审阅整个任务的改动 · \(artifact.name)") : L.text("审阅 \(artifact.task) 的改动 · \(artifact.name)")
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: view)
        w.setFrameAutosaveName("JanusDiffReview")
        if windows.isEmpty { w.center() }
        windows[artifact.path] = w
        w.makeKeyAndOrderFront(nil)
    }
}

struct DiffReviewView: View {
    let run: RunModel
    let artifact: Artifact
    @Local private var files: [DiffFile] = []
    @Local private var error: String?
    @Local private var selected: Int?
    @Local private var hovered: Int?
    @Local private var editing: Int?
    @Local private var draft = ""
    /// The person's comments, by the id of the line each is on.
    @Local private var comments: [Int: String] = [:]
    @Local private var summary = ""
    @Local private var sent: String?

    var body: some View {
        VStack(spacing: 0) {
            if let error {
                Text(error).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if files.isEmpty {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                HSplitView {
                    fileList.frame(minWidth: 200, idealWidth: 240, maxWidth: 320)
                    ScrollView { hunks.padding(16) }.frame(minWidth: 420).background(Color.page)
                }
            }
            Divider()
            footer
        }
        .frame(minWidth: 720, minHeight: 480)
        .background(Color.page)
        .task { await load() }
    }

    private var fileList: some View {
        List(selection: $selected) {
            Section(L.text("\(files.count) 个文件")) {
                ForEach(files) { f in
                    HStack {
                        Text((f.path as NSString).lastPathComponent).lineLimit(1)
                        Spacer()
                        if f.untracked { Text(L.text("新文件")).font(.caption).foregroundStyle(.secondary) }
                        else { Text(verbatim: "+\(f.added) −\(f.removed)").font(.caption.monospacedDigit()).foregroundStyle(.secondary) }
                    }
                    .help(f.path)
                    .tag(f.id)
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(Color.sidebar)
    }

    @ViewBuilder private var hunks: some View {
        let shown = selected.map { id in files.filter { $0.id == id } } ?? files
        LazyVStack(alignment: .leading, spacing: 18) {
            ForEach(shown) { f in
                VStack(alignment: .leading, spacing: 0) {
                    HStack {
                        Text(f.path).font(.system(.callout, design: .monospaced).weight(.semibold)).textSelection(.enabled)
                        Spacer()
                        if !f.untracked { Text(verbatim: "+\(f.added) −\(f.removed)").font(.caption.monospacedDigit()).foregroundStyle(.secondary) }
                    }
                    .padding(.horizontal, 12).padding(.vertical, 9)
                    Divider()
                    if f.untracked && f.hunks.isEmpty {
                        Text(L.text("新文件，没有可显示的文本改动；内容在工作目录里查看。")).font(.callout).foregroundStyle(.secondary).padding(12)
                    }
                    ForEach(f.hunks) { h in
                        Text(h.header).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
                            .padding(.horizontal, 12).padding(.vertical, 4)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(Color.secondary.opacity(0.06))
                        ForEach(h.lines) { l in row(f, l) }
                    }
                }
                .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line))
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
        }
    }

    @ViewBuilder private func row(_ f: DiffFile, _ l: DiffFile.Line) -> some View {
        let tint: Color = l.kind == .added ? Tone.green.color : l.kind == .removed ? Tone.red.color : .clear
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: l.old.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
                Text(verbatim: l.new.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
                // Hover a line to comment on it.
                Button { editing = l.id; draft = comments[l.id] ?? "" } label: {
                    Image(systemName: "plus.bubble.fill").foregroundStyle(Color.accent)
                }
                .buttonStyle(.plain).frame(width: 24)
                .opacity(hovered == l.id && run.canAct ? 1 : 0)
                .help(L.text("对这一行写审阅意见"))
                .accessibilityLabel(L.text("对这一行写审阅意见"))
                Text(verbatim: (l.kind == .added ? "+" : l.kind == .removed ? "−" : " ") + l.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(.system(size: 12, design: .monospaced))
            .foregroundStyle(l.kind == .context ? Color.primary.opacity(0.8) : Color.primary)
            .padding(.vertical, 1.5)
            .background(tint.opacity(0.1))
            .contentShape(Rectangle())
            .onHover { hovered = $0 ? l.id : (hovered == l.id ? nil : hovered) }
            if editing == l.id {
                VStack(alignment: .trailing, spacing: 6) {
                    TextField(L.text("这一行要怎么改"), text: $draft, axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(1...5)
                    HStack {
                        if comments[l.id] != nil { Button(L.text("删除"), role: .destructive) { comments[l.id] = nil; editing = nil } }
                        Button(L.text("取消")) { editing = nil }.keyboardShortcut(.cancelAction)
                        Button(L.text("添加")) {
                            let t = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                            comments[l.id] = t.isEmpty ? nil : t
                            editing = nil
                        }
                        .buttonStyle(.primary).keyboardShortcut(.defaultAction)
                    }
                }
                .padding(10)
                .background(Color.soft)
            } else if let c = comments[l.id] {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "text.bubble").foregroundStyle(Color.accent)
                    Text(c).font(.callout).frame(maxWidth: .infinity, alignment: .leading)
                    IconButton(symbol: "square.and.pencil", label: L.text("编辑这条意见"), size: 11) { editing = l.id; draft = c }
                }
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(Color.soft)
            }
        }
    }

    private var footer: some View {
        let agent = run.state.tasks[artifact.task]?.agent ?? ""
        let name = run.who(agent).name
        let ready = !comments.isEmpty || !summary.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        return HStack(spacing: 10) {
            TextField(L.text("总体意见（可选）"), text: $summary).textFieldStyle(.roundedBorder)
            Text(verbatim: L.text("\(comments.count) 条意见")).font(.callout).foregroundStyle(.secondary).fixedSize()
            if let sent { Text(sent).font(.callout).foregroundStyle(Tone.green.color).fixedSize() }
            // The whole run's diff has no one agent behind it: its review goes to the lead.
            if !artifact.ofRun {
                Button(L.text("发给\(name)")) { send(to: agent, label: name) }
                    .disabled(!run.canAct || !ready || agent.isEmpty)
                    .help(L.text("直接发给做这个子任务的 Agent：在线就插进这一轮，不在线就下次会话带给它"))
            }
            Button(L.text("交给负责人")) { send(to: "lead", label: L.text("负责人")) }
                .buttonStyle(.primary)
                .disabled(!run.canAct || !ready)
                .help(run.canAct ? L.text("负责人读完决定：让协作者修改、重做，或者自己处理") : L.text("这个任务已经结束，不能再发消息"))
        }
        .padding(12)
        .background(Color.card)
    }

    /// The comments as one message, written by the server in the run's language
    /// (`POST /api/runs/{id}/diff/message`), sent as any message.
    private func send(to agent: String, label: String) {
        let body: JSON = ["path": JSON(artifact.path), "summary": JSON(summary),
                          "comments": JSON.array(comments.map { ["line": JSON($0.key), "text": JSON($0.value)] })]
        Task {
            do {
                let text = try await run.api.post("/api/runs/\(run.id)/diff/message", body)["text"].str
                if await run.act(.message(.agent(agent), text: text, images: [])) {
                    comments = [:]; summary = ""; sent = L.text("已发给\(label)")
                }
            } catch {
                run.store.show(error.localizedDescription)
            }
        }
    }

    /// The diff file by file, as the server reads it (`GET /api/runs/{id}/diff`).
    private func load() async {
        do {
            let parsed = try await run.api.get("/api/runs/\(run.id)/diff?path=\(API.q(artifact.path))")["files"].array.map(DiffFile.init)
            if parsed.isEmpty { error = L.text("这个 diff 是空的") } else { files = parsed }
        } catch {
            self.error = L.text("读不了这个 diff")
        }
    }
}

/// One file of a saved diff as the server parsed it (`backend/src/diff.rs`): its hunks and lines,
/// each line numbered on both sides and by its id in the whole diff.
struct DiffFile: Identifiable, Equatable {
    struct Line: Identifiable, Equatable {
        enum Kind: String { case context, added, removed }
        let id: Int
        let kind: Kind
        let text: String
        let old: Int?
        let new: Int?
    }
    struct Hunk: Identifiable, Equatable {
        let id: Int
        let header: String
        let lines: [Line]
    }
    let id: Int
    let path: String
    let hunks: [Hunk]
    let added: Int
    let removed: Int
    /// A new file Git does not track yet; its diff is included when Git could read it.
    let untracked: Bool

    init(_ j: JSON) {
        id = j["id"].int ?? 0
        path = j["path"].str
        added = j["added"].int ?? 0
        removed = j["removed"].int ?? 0
        untracked = j["untracked"].bool ?? false
        hunks = j["hunks"].array.map { h in
            Hunk(id: h["id"].int ?? 0, header: h["header"].str, lines: h["lines"].array.map {
                Line(id: $0["id"].int ?? 0, kind: Line.Kind(rawValue: $0["kind"].str) ?? .context, text: $0["text"].str, old: $0["old"].int, new: $0["new"].int)
            })
        }
    }
}
