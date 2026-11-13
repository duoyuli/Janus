import AppKit
import SwiftUI

/// Reviewing a task's changes inside Janus: the diff file by file, a comment on any line, and the
/// comments sent back as one message, to the lead (which decides: reply, redo, or accept) or to
/// the agent that did the task.
// @project-doc docs/architecture/frontend.md#diff_review
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
        w.title = "审阅 \(artifact.task) 的改动 · \(artifact.name)"
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
    @Local private var files: [Diff.File] = []
    @Local private var error: String?
    @Local private var selected: Int?
    @Local private var hovered: Int?
    @Local private var editing: Int?
    @Local private var draft = ""
    @Local private var comments: [Int: (file: String, line: Diff.Line, text: String)] = [:]
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
            Section("\(files.count) 个文件") {
                ForEach(files) { f in
                    HStack {
                        Text((f.path as NSString).lastPathComponent).lineLimit(1)
                        Spacer()
                        if f.untracked { Text("新文件").font(.caption).foregroundStyle(.secondary) }
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
                    if f.untracked {
                        Text("新文件，还没有被 Git 跟踪；内容在工作目录里查看。").font(.callout).foregroundStyle(.secondary).padding(12)
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

    @ViewBuilder private func row(_ f: Diff.File, _ l: Diff.Line) -> some View {
        let tint: Color = l.kind == .added ? Tone.green.color : l.kind == .removed ? Tone.red.color : .clear
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: l.old.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
                Text(verbatim: l.new.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
                // Hover a line to comment on it.
                Button { editing = l.id; draft = comments[l.id]?.text ?? "" } label: {
                    Image(systemName: "plus.bubble.fill").foregroundStyle(Color.accent)
                }
                .buttonStyle(.plain).frame(width: 24)
                .opacity(hovered == l.id && run.canAct ? 1 : 0)
                .help("对这一行写审阅意见")
                .accessibilityLabel("对这一行写审阅意见")
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
                    TextField("这一行要怎么改", text: $draft, axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(1...5)
                    HStack {
                        if comments[l.id] != nil { Button("删除", role: .destructive) { comments[l.id] = nil; editing = nil } }
                        Button("取消") { editing = nil }.keyboardShortcut(.cancelAction)
                        Button("添加") {
                            let t = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                            comments[l.id] = t.isEmpty ? nil : (f.path, l, t)
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
                    Text(c.text).font(.callout).frame(maxWidth: .infinity, alignment: .leading)
                    Button("编辑") { editing = l.id; draft = c.text }.buttonStyle(.link).font(.caption)
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
            TextField("总体意见（可选）", text: $summary).textFieldStyle(.roundedBorder)
            Text(verbatim: "\(comments.count) 条意见").font(.callout).foregroundStyle(.secondary).fixedSize()
            if let sent { Text(sent).font(.callout).foregroundStyle(Tone.green.color).fixedSize() }
            Button("发给\(name)") { send(to: agent, label: name) }
                .disabled(!run.canAct || !ready || agent.isEmpty)
                .help("直接发给做这个任务的 Agent：它在会话里时插入这一轮，否则在它下次会话开始时带给它")
            Button("交给主控") { send(to: "lead", label: "主控") }
                .buttonStyle(.primary)
                .disabled(!run.canAct || !ready)
                .help(run.canAct ? "主控读到意见后决定：让执行者修改、重做这个任务，或者自己处理" : "这个 Run 已经结束，不能再发消息")
        }
        .padding(12)
        .background(Color.card)
    }

    private func send(to agent: String, label: String) {
        let list = comments.values.sorted { ($0.file, $0.line.id) < ($1.file, $1.line.id) }.map { Diff.Comment(file: $0.file, line: $0.line, text: $0.text) }
        let text = Diff.message(task: artifact.task, artifact: artifact.name, comments: list, summary: summary)
        Task {
            if await run.act(["type": "message", "agent": JSON(agent), "text": JSON(text)]) {
                comments = [:]; summary = ""; sent = "已发给\(label)"
            }
        }
    }

    private func load() async {
        let text: String?
        if let t = try? String(contentsOfFile: artifact.path, encoding: .utf8) { text = t }
        else { text = try? await run.api.text("/api/file?path=\(API.q(artifact.path))") }
        guard let text else { error = "读不了这个 diff"; return }
        let parsed = Diff.parse(text)
        if parsed.isEmpty { error = "这个 diff 是空的" } else { files = parsed }
    }
}
