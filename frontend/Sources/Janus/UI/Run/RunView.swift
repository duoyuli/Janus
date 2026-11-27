import AppKit
import SwiftUI

/// An open run: the workbench (board), the audit log with replay, and the team snapshot, plus a
/// task's detail and an agent's conversation.
struct RunView: View {
    let run: RunModel

    var body: some View {
        Group {
            if let err = run.loadError, !run.loaded {
                VStack(spacing: 12) {
                    Text(L.text("打不开这个任务")).font(.display(.title2))
                    Text(err).foregroundStyle(.secondary)
                    Button(L.text("重试")) { Task { await run.read() } }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if !run.loaded {
                ProgressView(L.text("正在读取…")).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if run.isChat {
                // A chat is one conversation: no workbench, audit or team pages.
                ChatView(run: run)
            } else if run.isLoop {
                // A Loop's session: the Loop's page.
                LoopView(run: run)
            } else if run.isDream {
                // A consolidation of the team memory: one turn of the lead over the notes.
                DreamView(run: run)
            } else if run.state.run.status == .queued {
                // Nothing of it has started: what it will do, where it waits, and the two choices.
                QueuedView(run: run)
            } else {
                VStack(spacing: 0) {
                    if run.isReplaying && (run.page == .board || run.page.isDetail) { ReplayBar(run: run) }
                    page
                }
            }
        }
        .navigationTitle(run.isChat ? run.chatTitle : run.isDream ? L.text("整理团队记忆") : run.isLoop ? (run.store.loop(ofRun: run.id)?["name"].str ?? run.runJSON["goal"].str) : run.state.title)
        .navigationSubtitle(run.isChat || run.isLoop || run.isDream || !run.readOnly ? "" : L.text("只读 · 这个任务已停止"))
        .toolbar {
            if run.isLoop || run.isDream {
                ToolbarItem { DocumentsMenu(run: run) }
            } else if run.isChat {
                ToolbarItem { DocumentsMenu(run: run) }
                ToolbarItem {
                    Button { run.exportLog() } label: { Label(L.text("导出事件日志…"), systemImage: "square.and.arrow.up") }
                        .help(L.text("导出这次对话的事件日志"))
                }
                ToolbarItem {
                    Button { Task { await run.store.deleteRun(run.id) } } label: { Label(L.text("删除对话"), systemImage: "trash") }
                        .disabled(!run.loaded)
                        .help(L.text("删掉这次对话、记录和工作目录，删了就找不回来"))
                        .accessibilityLabel(L.text("删除对话"))
                }
            } else {
                if run.page.isDetail {
                    ToolbarItem(placement: .navigation) {
                        Button { run.show(.board) } label: { Label(L.text("返回工作台"), systemImage: "chevron.backward") }
                            .help(L.text("返回工作台（⌘[）"))
                    }
                }
                ToolbarItem(placement: .principal) {
                    // On a task or agent page no segment is selected, so choosing 工作台 goes back.
                    Picker(L.text("视图"), selection: Binding(get: { tab }, set: { v in run.show(v == "audit" ? .audit : v == "team" ? .team : .board) })) {
                        Text(inboxCount == 0 ? L.text("工作台") : L.text("工作台 · \(inboxCount)")).tag("board")
                        Text(L.text("审计与回放")).tag("audit")
                        Text(L.text("团队与权限")).tag("team")
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 330)
                    .help(L.text("切换视图（⌘1 / ⌘2 / ⌘3）"))
                    .accessibilityValue(inboxCount == 0 ? "" : L.text("\(inboxCount) 件待处理"))
                }
                ToolbarItem {
                    DocumentsMenu(run: run)
                }
                ToolbarItem {
                    Menu {
                        Button(L.text("复制交付摘要")) { run.copySummary() }.disabled(run.state.page.summary == nil)
                        Button(L.text("导出事件日志…")) { run.exportLog() }
                    } label: { Label(L.text("分享"), systemImage: "square.and.arrow.up") }
                    .help(L.text("复制交付摘要或导出事件日志"))
                }
                ToolbarItem {
                    // One click removes a finished run; a run still working has to be stopped first.
                    let allowed = run.state.allowed
                    Button { Task { await run.store.deleteRun(run.id) } } label: {
                        Label(L.text("删除任务"), systemImage: "trash")
                    }
                    .disabled(!run.loaded || !allowed.ok("delete"))
                    .help(!run.loaded ? L.text("正在读取任务") : !allowed.ok("delete") ? allowed.why("delete") : L.text("删掉这个任务和它的全部记录、产物，删了就找不回来"))
                    .accessibilityLabel(L.text("删除任务"))
                }
            }
        }
        // Links to local Markdown files in agent prose open in Janus.
        .environment(\.openURL, OpenURLAction { DocLinks.handle($0, source: .run(run.api, run: run.id)) })
        .onExitCommand {
            // Esc leaves replay first, then a task or agent page.
            if run.isChat { return }
            if run.isReplaying { run.replay = nil } else if run.page.isDetail { run.show(.board) }
        }
    }

    private var inboxCount: Int { run.state.cards.count }

    private var tab: String {
        switch run.page {
        case .audit: return "audit"
        case .team: return "team"
        case .board: return "board"
        default: return ""
        }
    }

    @ViewBuilder private var page: some View {
        switch run.page {
        case .board: GroupView(run: run)
        case .audit: AuditView(run: run)
        case .team: TeamView(run: run)
        case .task(let id): TaskView(run: run, taskId: id)
        case .agent(let id): AgentView(run: run, agentId: id)
        }
    }

}

/// A run waiting in the dispatch queue: it starts by itself once a slot is free; the person may
/// start it now (past the slots) or cancel it.
// @project-doc docs/architecture/20261031-frontend.md#queued_run
struct QueuedView: View {
    let run: RunModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Card {
                    HStack {
                        Pill(run.queuePosition.map { L.text("排队中 · 第 \($0) 位") } ?? L.text("排队中"), .gray)
                        Spacer()
                    }
                    Text(run.state.title).font(.display(.title2)).textSelection(.enabled)
                    if run.state.run.goal.contains("\n") {
                        Text(run.state.run.goal).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    StoredImages(run: run, paths: run.state.run.images, size: 64)
                    Text(L.text("并行任务已满，排队中。前面的任务结束或暂停后自动开始。"))
                        .font(.callout).foregroundStyle(.secondary)
                    HStack {
                        Button(L.text("取消任务"), role: .destructive) { Task { await run.act(.cancel) } }
                        Button(L.text("马上开工")) { Task { await run.act(.start) } }
                            .buttonStyle(.primary)
                            .help(L.text("跳过排队，马上开工"))
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: 860, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}

/// The run's Markdown files, one click from anywhere in the run: deliverables, artifacts, and
/// documents the agents wrote or mentioned.
// @project-doc docs/architecture/20261031-frontend.md#markdown_viewer
struct DocumentsMenu: View {
    let run: RunModel

    var body: some View {
        Menu {
            let docs = run.documents
            if docs.isEmpty { Text(L.text("还没有 Markdown 文档")) }
            ForEach([RunDoc.Source.deliverable, .artifact, .mentioned], id: \.self) { source in
                let group = docs.filter { $0.source == source }
                if !group.isEmpty {
                    Section(L.catalogText(Self.title[source] ?? "")) {
                        ForEach(group, id: \.path) { d in
                            Button(d.label) { ArtifactWindow.open(run: run, path: d.path, title: d.name) }
                        }
                    }
                }
            }
            Divider()
            Button(L.text("打开其他 Markdown 文件…")) { ArtifactWindow.choose() }
        } label: { Label(L.text("文档"), systemImage: "doc.richtext") }
        .help(L.text("查看这个任务交付、保存或提到的 Markdown 文档"))
    }

    static let title: [RunDoc.Source: String] = [.deliverable: "交付物", .artifact: "子任务产物", .mentioned: "Agent 写过或提到的文档"]
}

extension RunModel {
    /// Switches the run's page. The audit and team pages show the live run, so they end a replay;
    /// task and agent pages show the replayed moment.
    func show(_ p: RunPage) {
        if p == .audit || p == .team { replay = nil }
        page = p
    }

    func togglePause() async {
        await act(state.run.paused ? .resume : .pause)
    }

    /// The delivery under review as text, as the server writes it: the conclusion, what needs
    /// looking at, where the result is, each task's numbers, and the report.
    func copySummary() {
        guard let text = state.page.summary else { return }
        copyText(text)
        store.show(L.text("已复制交付摘要"))
    }

    func exportLog() {
        let p = NSSavePanel()
        p.nameFieldStringValue = "janus-\(id).jsonl"
        guard p.runModal() == .OK, let url = p.url else { return }
        Task {
            do {
                let text = try await api.text("/api/runs/\(id)/export")
                try text.write(to: url, atomically: true, encoding: .utf8)
                store.show(L.text("已导出 \(url.lastPathComponent)"))
            } catch { store.show(error.localizedDescription) }
        }
    }
}

struct ReplayBar: View {
    let run: RunModel
    var body: some View {
        let n = run.events.count
        let pos = run.replay ?? n
        let e = pos >= 1 && pos <= n ? run.events[pos - 1] : nil
        HStack(spacing: 10) {
            Text(L.text("回放模式")).font(.headline)
            Text(L.text("只读 · 当时的状态")).font(.caption).foregroundStyle(.secondary)
            Button { run.replay = max(1, pos - 1) } label: { Label(L.text("上一条事件"), systemImage: "chevron.left") }
                .labelStyle(.iconOnly).help(L.text("上一条事件（←）")).keyboardShortcut(.leftArrow, modifiers: [])
            Slider(value: Binding(get: { Double(pos) }, set: { run.replay = max(1, min(n, Int($0.rounded()))) }), in: 1...Double(max(n, 2)))
                .accessibilityLabel(L.text("回放位置"))
                .accessibilityValue(L.text("第 \(pos) 条，共 \(n) 条"))
            Button { run.replay = min(n, pos + 1) } label: { Label(L.text("下一条事件"), systemImage: "chevron.right") }
                .labelStyle(.iconOnly).help(L.text("下一条事件（→）")).keyboardShortcut(.rightArrow, modifiers: [])
            Text(e.map { L.text("第 \(pos) / \(n) 条 · \(L.clock($0.ts)) · \(run.who($0.actor).name)：\(L.short($0.text, 40))") } ?? "").font(.caption).lineLimit(1).frame(width: 340, alignment: .leading)
            Button(L.text("回到实时")) { run.replay = nil }.buttonStyle(.primary).help(L.text("退出回放（Esc）"))
        }
        .padding(.horizontal, 16).padding(.vertical, 8)
        .background(Tone.violet.color.opacity(0.1))
    }
}

struct TaskTag: View {
    let id: String
    var body: some View {
        Text(id).font(.system(size: 11, weight: .semibold, design: .monospaced))
            .lineLimit(1).truncationMode(.middle)
            .padding(.horizontal, 5).padding(.vertical, 1)
            .foregroundStyle(Color.accent)
            .background(Color.accent.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color.accent.opacity(0.25), lineWidth: 0.5))
    }
}

/// What the lead suggests keeping in the workspace's shared material. Nothing is written until the
/// person opens the shared material with the lines added and saves there.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
struct BriefSuggestions: View {
    let run: RunModel
    let lines: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L.text("负责人建议写入共享资料")).font(.callout.weight(.semibold))
            ForEach(lines.indices, id: \.self) { i in Text("· \(lines[i])").font(.callout).textSelection(.enabled) }
            HStack {
                Text(L.text("采纳后，这个工作区之后的任务都能看到。")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                if let ws = run.store.workspace(run.workspaceId) {
                    Button(L.text("加入共享资料…")) { Task { await run.store.editBrief(ws.json, adding: lines) } }
                        .disabled(run.isReplaying)
                        .help(L.text("打开共享资料，把这些内容加到末尾，确认后保存"))
                } else {
                    Text(L.text("这个工作区已经删除")).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .padding(12)
        .background(Color.accent.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
    }
}
