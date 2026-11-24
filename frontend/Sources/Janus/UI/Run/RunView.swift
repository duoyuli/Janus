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
        .navigationTitle(run.isChat ? run.chatTitle : run.isDream ? L.text("整理团队记忆") : run.isLoop ? (run.store.loop(ofRun: run.id)?["name"].str ?? run.runJSON["goal"].str) : L.goalTitle(run.state.run.goal.isEmpty ? run.runJSON["goal"].str : run.state.run.goal))
        .navigationSubtitle(run.isChat || run.isLoop || run.isDream ? "" : run.live ? "" : L.text("只读 · 这个任务不在当前运行的 Janus 服务里"))
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
                        Button(L.text("复制交付摘要")) { run.copySummary() }.disabled(run.state.result == nil)
                        Button(L.text("导出事件日志…")) { run.exportLog() }
                    } label: { Label(L.text("分享"), systemImage: "square.and.arrow.up") }
                    .help(L.text("复制交付摘要或导出事件日志"))
                }
                ToolbarItem {
                    // One click removes a finished run; a run still working has to be stopped first.
                    let active = run.state.run.status.isActive
                    Button { Task { await run.store.deleteRun(run.id) } } label: {
                        Label(L.text("删除任务"), systemImage: "trash")
                    }
                    .disabled(!run.loaded || active)
                    .help(!run.loaded ? L.text("正在读取任务") : active ? L.text("任务还在进行，先停止任务再删除") : L.text("删掉这个任务和它的全部记录、产物，删了就找不回来"))
                    .accessibilityLabel(L.text("删除任务"))
                }
            }
        }
        // Links to local Markdown files in agent prose open in Janus.
        .environment(\.openURL, OpenURLAction { DocLinks.handle($0) })
        .onExitCommand {
            // Esc leaves replay first, then a task or agent page.
            if run.isChat { return }
            if run.isReplaying { run.replay = nil } else if run.page.isDetail { run.show(.board) }
        }
    }

    private var inboxCount: Int { run.state.inbox.filter { RequestCard.kinds.contains($0.kind) }.count }

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
        case .board: BoardView(run: run)
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
                    Text(L.goalTitle(run.state.run.goal)).font(.display(.title2)).textSelection(.enabled)
                    if run.state.run.goal.contains("\n") {
                        Text(run.state.run.goal).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    StoredImages(paths: run.state.run.images, size: 64)
                    Text(L.text("并行任务已满，排队中。前面的任务结束或暂停后自动开始。"))
                        .font(.callout).foregroundStyle(.secondary)
                    HStack {
                        Button(L.text("取消任务"), role: .destructive) { Task { await run.act(["type": "cancel"]) } }
                        Button(L.text("马上开工")) { Task { await run.act(["type": "start"]) } }
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
            ForEach([RunDocs.Source.deliverable, .artifact, .mentioned], id: \.self) { source in
                let group = docs.filter { $0.source == source }
                if !group.isEmpty {
                    Section(L.catalogText(Self.title[source] ?? "")) {
                        ForEach(group, id: \.path) { d in
                            Button(Self.label(d.path, cwd: run.state.run.cwd)) { ArtifactWindow.open(run: run, path: d.path, title: (d.path as NSString).lastPathComponent) }
                        }
                    }
                }
            }
            Divider()
            Button(L.text("打开其他 Markdown 文件…")) { ArtifactWindow.choose() }
        } label: { Label(L.text("文档"), systemImage: "doc.richtext") }
        .help(L.text("查看这个任务交付、保存或提到的 Markdown 文档"))
    }

    static let title: [RunDocs.Source: String] = [.deliverable: "交付物", .artifact: "子任务产物", .mentioned: "Agent 写过或提到的文档"]

    /// Inside the run's directory, the path from there; elsewhere, the path with ~.
    static func label(_ path: String, cwd: String) -> String {
        let dir = cwd.hasSuffix("/") ? cwd : cwd + "/"
        return !cwd.isEmpty && path.hasPrefix(dir) ? String(path.dropFirst(dir.count)) : L.tilde(path)
    }
}

extension RunModel {
    /// Switches the run's page. The audit and team pages show the live run, so they end a replay;
    /// task and agent pages show the replayed moment.
    func show(_ p: RunPage) {
        if p == .audit || p == .team { replay = nil }
        page = p
    }

    func togglePause() async {
        await act(["type": JSON(state.run.paused ? "resume" : "pause")])
    }

    /// The delivery under review as text: the conclusion, what needs looking at, where the result
    /// is, each task's numbers, and the report.
    func copySummary() {
        guard let r = state.result else { return }
        let d = Delivery(r)
        var lines: [String] = [L.text("已完成：\(L.goalTitle(state.run.goal))")]
        if state.deliveries.count > 1 { lines.append(L.text("第 \(d.n) 次交付")) }
        if !d.headline.isEmpty { lines.append(d.headline) }
        let signals = reviewSignals(d)
        if !signals.isEmpty {
            lines += ["", L.text("需要你看的：")]
            lines += signals.map { "- " + $0.text + ($0.detail.isEmpty ? "" : L.text("（\($0.detail)）")) }
        }
        if !d.mergeNote.isEmpty { lines += ["", d.mergeNote] }
        let acceptance = d.acceptance
        lines += ["", ReviewBody.factsLine(acceptance, changed: d.changed.count)]
        if !acceptance.isEmpty {
            lines += ["", L.text("逐条验收：")]
            for a in acceptance {
                let verified: [String] = a.verifications.map { v in v.passed ? L.text("\(v.task) 通过") : L.text("\(v.task) 未确认（\(v.label)）") }
                var line = L.text("- \(a.task)：通过 \(a.summary.passed)/\(a.summary.criteria) · 证据：\(a.summary.countsText)")
                if !verified.isEmpty { line += L.text(" · 独立验证：") + verified.joined(separator: L.text("、")) }
                if a.unverified { line += L.text(" · 没有独立验证") }
                lines.append(line)
            }
        }
        if !d.summary.isEmpty { lines += ["", d.summary] }
        copyText(lines.joined(separator: "\n"))
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

// MARK: - Board

struct BoardView: View {
    let run: RunModel

    var body: some View {
        let st = run.shown
        GeometryReader { geo in
            // The card column's width: the scroll view minus the 20-point padding on both sides.
            let contentWidth = max(0, geo.size.width - 40)
            let lead = agentSession(st, "lead")
            // A run with nothing to say yet: no tasks, and the lead has only been created.
            let empty = st.order.isEmpty && !lead.evs.contains { $0.type != .runCreated }
            let heights = BoardLayout.cardHeights(
                contentHeight: geo.size.height, twoColumn: BoardLayout.isTwoColumn(contentWidth), empty: empty)
            ScrollView {
                // The lead's conversation is the run: the goal, how it is being pursued and what it
                // asks. What the team is doing sits beside it; what waits on the person, above it.
                VStack(alignment: .leading, spacing: 18) {
                    RunCard(run: run, st: st, width: contentWidth)
                    InboxSection(run: run, st: st)
                    if st.result != nil { ResultCard(run: run, st: st, width: contentWidth) }
                    // The progress timeline answers "what is the team doing", so it sits above the
                    // conversation, folded to one line until asked for (W1 U3/F8).
                    ProgressBoard(run: run, st: st)
                    AdaptiveColumns(side: 340, minMain: 480, spacing: 18) {
                        ChatCard(run: run, st: st, title: L.text("负责人"), info: lead, target: .agent("lead"))
                            .frame(height: heights.chat)
                        RunInspector(run: run, st: st)
                            .frame(height: heights.inspector)
                    }
                }
                .padding(20)
            }
        }
    }
}

struct RunCard: View {
    let run: RunModel
    let st: RunState
    /// The card's content width: under 640 the actions move below the title (W1 F9).
    var width: CGFloat

    var body: some View {
        let r = st.run
        Card(padding: 18) {
            if BoardLayout.runCardStacks(width) {
                VStack(alignment: .leading, spacing: 10) {
                    heading
                    actions
                }
            } else {
                HStack(alignment: .top) {
                    heading
                    Spacer()
                    actions
                }
            }
            if r.status == .failed { Note(text: r.error, tone: .red) }
            if r.status.canResume && !run.isReplaying {
                HStack {
                    Text(verbatim: L.text("\(r.status == .interrupted ? L.text("Janus 服务重启，任务中断了。做完的保留，没做完的接着做。") : L.text("可以从失败处继续。")) 也可以点右侧的 Agent，补充信息后再继续。")).font(.callout).foregroundStyle(.secondary)
                    Spacer()
                    Button(L.text("从中断处继续")) { Task { await run.act(["type": "resumeRun"]) } }.buttonStyle(.primary)
                }
                .padding(10)
                .background(Color.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            }
            summaryRow
            if let reference = run.runJSON["config"]["tokenLimit"].double, reference > 0 {
                Text(verbatim: L.text("Token 用量参考值：\(reference.formatted(.number.locale(L.locale)))"))
                    .font(.callout).foregroundStyle(.secondary)
            }
            // Reminders stay directly under the summary whether or not the ledger is open; the
            // ledger never repeats them (W1 F4). Text only: nothing here pauses, stops or budgets
            // the run.
            if let line = run.usage.remindersLine { Note(text: line, tone: .amber) }
            if run.usageOpen { UsageSection(run: run, st: st) }
        }
    }

    /// The status capsule, the goal's title (with the full goal behind a link when it is longer),
    /// and any images attached to the goal.
    @ViewBuilder private var heading: some View {
        let r = st.run
        let title = L.goalTitle(r.goal)
        let long = r.goal.trimmingCharacters(in: .whitespacesAndNewlines) != title
        let s = status
        VStack(alignment: .leading, spacing: 6) {
            Pill(s.0, s.1)
            Text(title.isEmpty ? L.text("正在连接 Janus 服务…") : title).font(.display(.title2)).textSelection(.enabled)
            if long {
                Button(run.goalOpen ? L.text("收起完整目标") : L.text("查看完整目标")) { run.goalOpen.toggle() }.buttonStyle(.link)
                if run.goalOpen { MarkdownText(text: r.goal).padding(.top, 4) }
            }
            StoredImages(paths: r.images, size: 64)
        }
    }

    /// Pause and stop, only while the run is live and still going. Team, workspace and start time
    /// are on the 团队与权限 page; the wall-clock time is in the summary line.
    @ViewBuilder private var actions: some View {
        let r = st.run
        if run.canAct && !r.status.isFinished {
            HStack(spacing: 8) {
                if r.status == .running || r.paused {
                    Button { Task { await run.togglePause() } } label: {
                        Label(r.paused ? L.text("继续") : L.text("暂停"), systemImage: r.paused ? "play.fill" : "pause.fill")
                    }
                    .help(r.paused ? L.text("继续调度子任务（⇧⌘P）") : L.text("暂停：不再派新活，已开工的照常跑（⇧⌘P）"))
                }
                // A delivery's result is out already: stopping cannot take it back, so after one
                // the lead's turn is interrupted from its conversation instead.
                if st.result == nil {
                    Button(L.text("停止…"), role: .destructive) { cancelRun(run) }.help(L.text("结束这个任务的所有会话"))
                }
            }
        }
    }

    /// One line of numbers, with the ledger's disclosure at its end when anything was reported
    /// (W1 F2). The line is one VoiceOver element with full digits; the control is its own. While
    /// the run is live and still going, a one-second timeline keeps the wall-clock time counting;
    /// a finished, replayed or read-only run is drawn once with the ledger's value.
    private var summaryRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if run.elapsedTicks {
                TimelineView(.periodic(from: .now, by: 1)) { ctx in
                    summaryText(now: ctx.date.timeIntervalSince1970 * 1000)
                }
            } else {
                summaryText(now: L.nowMs())
            }
            Spacer(minLength: 4)
            if run.usage.showsLedgerControl(open: run.usageOpen) { usageToggle }
        }
    }

    /// The summary line at `now`: the visible text and the full-digit VoiceOver and hover text come
    /// from the same displayed ledger, so they never disagree.
    private func summaryText(now: Double) -> some View {
        let u = run.usageDisplayed(at: now)
        let tasks = st.order.count
        let done = st.orderedTasks.filter { $0.status == .succeeded }.count
        let voice = u.runCardVoice(tasks: tasks, done: done)
        return Text(verbatim: u.runCardLine(tasks: tasks, done: done))
            .font(.callout).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
            .help(voice)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(voice)
    }

    /// A disclosure, not a link: it opens the ledger inside this card.
    private var usageToggle: some View {
        Button { run.usageOpen.toggle() } label: {
            HStack(spacing: 6) {
                DisclosureChevron(open: run.usageOpen)
                Text(L.text("用量明细")).font(.callout)
            }
            .foregroundStyle(Color.accent)
            .padding(.vertical, 2).padding(.trailing, 6)
            .contentShape(Rectangle())
            .hoverable(6)
        }
        .buttonStyle(.plain)
        .help(L.text("展开用量：Token、费用、耗时、上下文"))
        .accessibilityLabel(run.usageOpen ? L.text("收起用量明细") : L.text("展开用量明细"))
    }

    private var status: (String, Tone) {
        let r = st.run
        switch r.status {
        case .cancelled: return (L.text("已停止"), .gray)
        case .failed: return (L.text("任务失败"), .red)
        case .interrupted: return (L.text("已中断"), .gray)
        default: break
        }
        if r.paused { return (L.text("已暂停"), .gray) }
        switch reviewStage(st, live: run.live && !run.isReplaying) {
        case .awaiting?: return (L.text("已交付 · 等你验收"), .green)
        case .blocked?: return (L.text("结果没进你的分支"), .amber)
        case .resolving?: return (L.text("负责人在解决合并冲突"), .blue)
        case .leadWorking?: return (L.text("负责人在处理你的消息"), .blue)
        case .held?: return (L.text("你打断了负责人"), .amber)
        case .accepted?: return (L.text("已验收"), .green)
        default: break
        }
        if r.status == .planning { return (L.text("负责人正在上岗"), .blue) }
        if !st.inbox.isEmpty { return (L.text("\(st.inbox.count) 件待处理"), .amber) }
        return (L.text("团队干活中"), .blue)
    }
}

struct InboxSection: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let open = st.inbox.filter { RequestCard.kinds.contains($0.kind) }
        // Nothing to do shows nothing; the toolbar's 工作台 count says the same.
        if !open.isEmpty {
            SectionHeader(title: L.text("待处理"), count: open.count)
            ForEach(open) { item in RequestCard(run: run, st: st, item: item) }
        }
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

struct AgentsPanel: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let members = run.team
        let active = members.filter { phase($0["id"].str, lead: $0["lead"].truthy) != "idle" }.count
        VStack(alignment: .leading, spacing: 8) {
            if active > 0 { Text(verbatim: L.text("\(active) 个在干活")).font(.caption).foregroundStyle(.secondary) }
            // Rows divided by hairlines: no boxes inside the box.
            VStack(spacing: 0) {
                ForEach(Array(members.enumerated()), id: \.offset) { i, m in
                    if i > 0 { Divider().padding(.leading, 42) }
                    row(m["id"].str, lead: m["lead"].truthy)
                }
            }
        }
    }

    private func row(_ id: String, lead: Bool) -> some View {
        let w = run.who(id)
        let a = st.agents[id]
        let mine = st.orderedTasks.filter { $0.agent == id }
        let cur = mine.first { $0.status.isBusy }
        let next = mine.first { $0.status.isWaitingToStart }
        let phase = self.phase(id, lead: lead)
        let (pl, pt) = L.phase(phase)
        var line: String
        if lead {
            line = !st.inbox.isEmpty ? L.text("\(st.inbox.count) 件待处理，其他子任务照常推进") : cur.map { "\($0.id) \($0.title)" } ?? (!(a?.note ?? "").isEmpty ? a!.note : st.run.status == .delivered ? L.text("已完成最终验收") : L.text("按依赖调度子任务"))
        } else if let cur {
            line = "\(cur.id) \(cur.title)"
            if cur.status == .waitingInput { line += L.text(" · 等你回答") } else if cur.status == .waitingApproval { line += L.text(" · 等你审批") } else if let n = a?.note, !n.isEmpty { line += " · \(n)" }
            if cur.human { line += L.text(" · 人工接管中") }
        } else if let next {
            let waiting = next.deps.filter { st.tasks[$0]?.status != .succeeded }.joined(separator: L.text("、"))
            line = next.status == .blocked ? L.text("\(next.id) 等待 \(next.blockedBy) 修复") : L.text("下一步 \(next.id) · 等待 \(waiting.isEmpty ? L.text("调度") : waiting)")
        } else {
            line = !mine.isEmpty ? L.text("派给它的子任务都做完了") : !st.order.isEmpty ? L.text("负责人没有给它派子任务") : L.text("待命")
        }
        var meta: [String] = []
        if phase != "idle", let beat = a?.beat, beat > 0 { meta.append(L.text("最近活动 \(L.ago(beat, st.run.now))")) }
        if !lead, let rs = runStats(id, mine) { meta.append(rs) }
        return Button { run.page = .agent(id) } label: {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 10) {
                    Avatar(initials: w.initials, color: w.color, size: 30, role: w.role)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Text(w.name).font(.callout.weight(.semibold))
                            Spacer()
                            Pill(pl, pt)
                        }
                        Text(line).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
                if let cur, cur.status == .running, let p = a?.progress, p > 0 { Bar(value: p).padding(.leading, 40) }
            }
            .padding(.vertical, 9).padding(.horizontal, 4)
            .contentShape(Rectangle())
            .hoverable(8)
        }
        .buttonStyle(.plain)
        .help(([w.runtime, w.model].filter { !$0.isEmpty } + meta).joined(separator: " · ") + L.text("\n点按查看 \(w.name) 的对话"))
    }

    /// A collaborator without a task in progress is idle, whatever its last status said.
    private func phase(_ id: String, lead: Bool) -> String {
        let p = st.run.status.isFinished ? "idle" : st.agents[id]?.phase ?? "idle"
        let busy = st.orderedTasks.contains { $0.agent == id && $0.status.isBusy }
        return !lead && !busy && p != "error" ? "idle" : p
    }

    /// "本次 2/3 首次通过 · 验证没通过 1"
    private func runStats(_ id: String, _ mine: [TaskState]) -> String? {
        let ended = mine.filter { $0.status == .succeeded || $0.status == .failed }
        guard !ended.isEmpty else { return nil }
        func vf(_ t: String) -> Int { st.events.filter { $0.type == .verificationFailed && $0.task == t }.count }
        let first = ended.filter { $0.status == .succeeded && $0.attempts.count == 1 && vf($0.id) == 0 }.count
        let vfail = ended.filter { vf($0.id) > 0 }.count
        return L.text("本次 \(first)/\(ended.count) 首次通过\(vfail > 0 ? L.text(" · 验证没通过 \(vfail)") : "")")
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
                    Button(L.text("加入共享资料…")) { run.store.editingBrief = Self.adding(lines, to: ws.json) }
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

    /// The workspace as the shared material sheet opens it: its shared material with the lines appended.
    static func adding(_ lines: [String], to ws: JSON) -> JSON {
        var ws = ws
        let current = ws["brief"].str.trimmingCharacters(in: .whitespacesAndNewlines)
        let added = lines.map { "- " + $0 }.joined(separator: "\n")
        ws["brief"] = JSON(current.isEmpty ? added : current + "\n" + added)
        return ws
    }
}

/// Beside the lead's conversation: the team's work, one view at a time. Tasks (click one to open
/// it), changes (each task's latest diff, to review), files the run produced, and the team.
// @project-doc docs/architecture/20261031-frontend.md#run_page
struct RunInspector: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        // The whole run's diff first: it is what accepting brings back, the lead's own edits included.
        let diffs = latest(st.artifacts.filter(\.isDiff)).sorted { $0.ofRun && !$1.ofRun }
        let files = latest(st.artifacts.filter { !$0.isDiff })
        let tab = self.tab
        Card(padding: 12) {
            Picker(L.text("查看"), selection: tabBinding) {
                Text(verbatim: L.text("子任务 \(st.order.count)")).tag("tasks")
                Text(verbatim: L.text("改动 \(diffs.count)")).tag("changes")
                Text(verbatim: L.text("产物 \(files.count)")).tag("files")
                Text(L.text("团队")).tag("team")
            }
            .pickerStyle(.segmented).labelsHidden()
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch tab {
                    case "changes": artifacts(diffs, empty: L.text("还没有改动。"))
                    case "files": artifacts(files, empty: L.text("还没有产物。"))
                    case "team": AgentsPanel(run: run, st: st)
                    default: tasks
                    }
                }
            }
        }
        .onAppear {
            // Chosen once per opening: the last tab the person picked, otherwise team before any
            // task exists and tasks after. Tasks appearing later do not switch it under them
            // (W1 U2/A15).
            if run.inspectorTab == nil {
                run.inspectorTab = RunModel.initialInspectorTab(
                    saved: UserDefaults.janus.string(forKey: "janus.inspectorTab"),
                    hasTasks: !st.orderedTasks.isEmpty)
            }
        }
    }

    /// The tab for this opening; before `onAppear` resolves it, the same rule applies.
    private var tab: String {
        run.inspectorTab ?? RunModel.initialInspectorTab(
            saved: UserDefaults.janus.string(forKey: "janus.inspectorTab"),
            hasTasks: !st.orderedTasks.isEmpty)
    }

    private var tabBinding: Binding<String> {
        Binding(get: { tab }, set: { v in
            run.inspectorTab = v
            // A manual choice is the one that outlasts this run.
            UserDefaults.janus.set(v, forKey: "janus.inspectorTab")
        })
    }

    @ViewBuilder private var tasks: some View {
        if st.orderedTasks.isEmpty { Text(L.text("负责人还没有派出子任务。")).font(.callout).foregroundStyle(.secondary).padding(.vertical, 8) }
        ForEach(Array(st.orderedTasks.enumerated()), id: \.element.id) { i, t in
            if i > 0 { Divider() }
            let (label, tone) = t.status.label
            let w = run.who(t.agent)
            Button { run.show(.task(t.id)) } label: {
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: RunTimeline.statusSymbol(tone)).foregroundStyle(tone.color).font(.system(size: 13)).frame(width: 16).padding(.top, 2)
                        .accessibilityLabel(label)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(t.title).font(.callout.weight(.medium)).lineLimit(2).multilineTextAlignment(.leading)
                        HStack(spacing: 5) {
                            TaskTag(id: t.id)
                            Avatar(initials: w.initials, color: w.color, size: 14, role: w.role)
                            Text(verbatim: "\(w.name) · \(label)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 8)
                .contentShape(Rectangle())
                .hoverable(6)
            }
            .buttonStyle(.plain)
        }
    }

    @ViewBuilder private func artifacts(_ list: [Artifact], empty: String) -> some View {
        if list.isEmpty { Text(empty).font(.callout).foregroundStyle(.secondary).padding(.vertical, 8) }
        ForEach(Array(list.enumerated()), id: \.element.seq) { i, a in
            if i > 0 { Divider() }
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        if a.ofRun { Text(L.text("整个任务")).font(.callout.weight(.medium)) } else { TaskTag(id: a.task) }
                        Text(a.name).font(.callout.weight(a.ofRun ? .regular : .medium)).foregroundStyle(a.ofRun ? .secondary : .primary).lineLimit(1)
                    }
                    if !a.desc.isEmpty { Text(a.desc).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                }
                Spacer()
                Button(a.isDiff ? L.text("审阅") : L.text("查看")) {
                    if a.isDiff { DiffWindow.open(run: run, artifact: a) } else { ArtifactWindow.open(run: run, path: a.path, title: a.name) }
                }
                .buttonStyle(.link)
            }
            .padding(.vertical, 8)
        }
    }

    /// The newest artifact of each task and name kind: an older attempt's diff is history.
    private func latest(_ xs: [Artifact]) -> [Artifact] {
        var seen = Set<String>()
        return xs.sorted { $0.seq > $1.seq }.filter { seen.insert("\($0.task)/\($0.kind)/\($0.isDiff ? "" : $0.name)").inserted }
    }
}
