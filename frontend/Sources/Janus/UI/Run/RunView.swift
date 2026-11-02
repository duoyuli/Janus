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
                    Text("打不开这个任务").font(.title2.weight(.semibold))
                    Text(err).foregroundStyle(.secondary)
                    Button("重试") { Task { await run.load() } }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if !run.loaded {
                ProgressView("正在读取…").frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                VStack(spacing: 0) {
                    if run.isReplaying && (run.page == .board || run.page.isDetail) { ReplayBar(run: run) }
                    page
                }
            }
        }
        .navigationTitle(L.goalTitle(run.state.run.goal.isEmpty ? run.runJSON["goal"].str : run.state.run.goal))
        .navigationSubtitle(run.live ? "" : "只读 · 这个任务不在当前编排器进程里")
        .toolbar {
            if run.page.isDetail {
                ToolbarItem(placement: .navigation) {
                    Button { run.show(.board) } label: { Label("返回工作台", systemImage: "chevron.backward") }
                        .help("返回工作台（⌘[）")
                }
            }
            ToolbarItem(placement: .principal) {
                // On a task or agent page no segment is selected, so choosing 工作台 goes back.
                Picker("视图", selection: Binding(get: { tab }, set: { v in run.show(v == "audit" ? .audit : v == "team" ? .team : .board) })) {
                    Text(inboxCount == 0 ? "工作台" : "工作台 · \(inboxCount)").tag("board")
                    Text("审计与回放").tag("audit")
                    Text("团队与权限").tag("team")
                }
                .pickerStyle(.segmented)
                .frame(width: 330)
                .help("切换视图（⌘1 / ⌘2 / ⌘3）")
                .accessibilityValue(inboxCount == 0 ? "" : "\(inboxCount) 项需要你处理")
            }
            ToolbarItem {
                Menu {
                    Button("复制交付摘要") { run.copySummary() }.disabled(run.state.result == nil)
                    Button("导出事件日志…") { run.exportLog() }
                } label: { Label("分享", systemImage: "square.and.arrow.up") }
                .help("复制交付摘要或导出事件日志")
            }
        }
        .onExitCommand {
            // Esc leaves replay first, then a task or agent page.
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

    func copySummary() {
        guard let r = state.result else { return }
        let d = r.d
        let risks = d["risks"].strings
        let text = (["已完成：\(L.goalTitle(state.run.goal))", d["summary"].str, "", "证据："]
            + d["evidence"].array.map { "- \($0["text"].str)（事件 #\($0["seq"].str)）" }
            + ["", "风险："] + (risks.isEmpty ? ["- 无"] : risks.map { "- " + $0 })).joined(separator: "\n")
        copyText(text)
        store.show("已复制交付摘要")
    }

    func exportLog() {
        let p = NSSavePanel()
        p.nameFieldStringValue = "janus-\(id).jsonl"
        guard p.runModal() == .OK, let url = p.url else { return }
        Task {
            do {
                let text = try await api.text("/api/runs/\(id)/export")
                try text.write(to: url, atomically: true, encoding: .utf8)
                store.show("已导出 \(url.lastPathComponent)")
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
            Text("回放模式").font(.headline)
            Text("只读 · 界面显示的是当时的状态").font(.caption).foregroundStyle(.secondary)
            Button { run.replay = max(1, pos - 1) } label: { Label("上一条事件", systemImage: "chevron.left") }
                .labelStyle(.iconOnly).help("上一条事件（←）").keyboardShortcut(.leftArrow, modifiers: [])
            Slider(value: Binding(get: { Double(pos) }, set: { run.replay = max(1, min(n, Int($0.rounded()))) }), in: 1...Double(max(n, 2)))
                .accessibilityLabel("回放位置")
                .accessibilityValue("第 \(pos) 条，共 \(n) 条")
            Button { run.replay = min(n, pos + 1) } label: { Label("下一条事件", systemImage: "chevron.right") }
                .labelStyle(.iconOnly).help("下一条事件（→）").keyboardShortcut(.rightArrow, modifiers: [])
            Text(e.map { "第 \(pos) / \(n) 条 · \(L.clock($0.ts)) · \(run.who($0.actor).name)：\(L.short($0.text, 40))" } ?? "").font(.caption).lineLimit(1).frame(width: 340, alignment: .leading)
            Button("回到实时") { run.replay = nil }.buttonStyle(.borderedProminent).help("退出回放（Esc）")
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
        ScrollView {
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 14) {
                    RunCard(run: run, st: st)
                    InboxSection(run: run, st: st)
                    if st.result != nil { ResultCard(run: run, st: st) }
                    ProgressBoard(run: run, st: st)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                AgentsPanel(run: run, st: st)
                    .frame(width: 300)
            }
            .padding(20)
        }
    }
}

struct RunCard: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let r = st.run
        let (label, tone) = status
        let title = L.goalTitle(r.goal)
        let long = r.goal.trimmingCharacters(in: .whitespacesAndNewlines) != title
        let done = st.orderedTasks.filter { $0.status == "succeeded" }.count
        let stages = ["推进目标", "协作", "交付", "验收"]
        let idx = stageIndex
        let active = !RunState.finished.contains(r.status)
        Card(padding: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    Pill(label, tone)
                    Text(title.isEmpty ? "正在连接编排器…" : title).font(.title2.weight(.semibold)).textSelection(.enabled)
                    if long {
                        Button(run.goalOpen ? "收起完整目标" : "查看完整目标") { run.goalOpen.toggle() }.buttonStyle(.link)
                        if run.goalOpen { MarkdownText(text: r.goal).padding(.top, 4) }
                    }
                }
                Spacer()
                if run.canAct && active {
                    if r.status == "running" || r.paused {
                        Button { Task { await run.togglePause() } } label: {
                            Label(r.paused ? "继续" : "暂停", systemImage: r.paused ? "play.fill" : "pause.fill")
                        }
                        .help(r.paused ? "继续调度任务（⇧⌘P）" : "暂停后不再启动新任务，进行中的尝试会继续（⇧⌘P）")
                    }
                    Button("停止…", role: .destructive) { cancelRun(run) }.help("结束这个 Run 的所有会话")
                }
            }
            if r.status == "failed" { Note(text: r.error, tone: .red) }
            if ["interrupted", "failed"].contains(r.status) && !run.isReplaying {
                HStack {
                    Text(verbatim: "\(r.status == "interrupted" ? "编排器重启时中断了。已完成的任务会保留，未完成的从原处继续。" : "可以从失败处继续。")也可以点右侧的 Agent 进入对话，补充信息后再继续。").font(.callout).foregroundStyle(.secondary)
                    Spacer()
                    Button("从中断处继续") { Task { await run.act(["type": "resumeRun"]) } }.buttonStyle(.borderedProminent)
                }
                .padding(10)
                .background(Color.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            }
            // Team, workspace and start time are on the 团队与权限 page.
            HStack(spacing: 16) {
                meta("用时", L.dur(r.now - r.start)).help("开始于 \(L.clock(r.start))")
                if !st.order.isEmpty { meta("任务", "\(done) / \(st.order.count)") }
            }
            HStack(spacing: 6) {
                ForEach(Array(stages.enumerated()), id: \.offset) { i, s in
                    Text(i < idx ? "✓ \(s)" : s)
                        .font(.callout.weight(i == idx ? .semibold : .regular))
                        .foregroundStyle(i < idx ? Tone.green.color : i == idx ? Color.accent : .secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                        .background(i == idx ? Color.accent.opacity(0.1) : i < idx ? Tone.green.color.opacity(0.08) : Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                        .accessibilityLabel("\(s)，\(i < idx ? "已完成" : i == idx ? "当前阶段" : "未开始")")
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("阶段")
        }
    }

    private func meta(_ k: String, _ v: String) -> some View {
        HStack(spacing: 4) { Text(k).foregroundStyle(.secondary); Text(v).fontWeight(.medium) }.font(.callout)
    }

    private var status: (String, Tone) {
        let r = st.run
        switch r.status {
        case "cancelled": return ("已停止", .gray)
        case "failed": return ("Run 失败", .red)
        case "interrupted": return ("已中断", .gray)
        default: break
        }
        if r.paused { return ("已暂停", .gray) }
        switch r.status {
        case "planning": return ("主控正在启动", .blue)
        case "accepted": return ("已验收", .green)
        case "waiting_input": return ("等你追加预算", .amber)
        case "delivered": return ("已交付 · 等你验收", .green)
        default: break
        }
        if !st.inbox.isEmpty { return ("等你处理 \(st.inbox.count) 项", .amber) }
        return ("团队执行中", .blue)
    }

    private var stageIndex: Int {
        let r = st.run
        if r.status == "accepted" { return 4 }
        if r.status == "delivered" { return 3 }
        if st.orderedTasks.contains(where: { !["succeeded", "failed", "cancelled", "skipped_by_decision"].contains($0.status) }) { return 1 }
        return 0
    }
}

struct InboxSection: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let open = st.inbox.filter { RequestCard.kinds.contains($0.kind) }
        // Nothing to do shows nothing; the toolbar's 工作台 count says the same.
        if !open.isEmpty {
            SectionHeader(title: "需要你处理", count: open.count)
            ForEach(open) { item in RequestCard(run: run, st: st, item: item) }
        }
    }
}

struct ResultCard: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        if let r = st.result {
            let d = r.d
            let risks = d["risks"].strings
            let suggestions = d["briefSuggestions"].strings
            let deliverables = d["deliverables"].array
            let evidence = d["evidence"].array
            let long = d["summary"].str.count > 360
            let canAccept = r.accepted == nil && !run.isReplaying
            SectionHeader(title: "交付结果")
            Card(padding: 18, tint: Tone.green.color) {
                HStack(spacing: 8) {
                    Pill("已交付", .green)
                    if let a = r.accepted { Pill("✓ 你已于 \(L.clock(a)) 验收", .green) }
                    SeqButton(seq: r.seq, run: run)
                    Spacer()
                }
                Text("目标已完成").font(.title3.weight(.semibold))
                MarkdownText(text: d["summary"].str, lineLimit: long && !run.resultOpen ? 8 : nil)
                if long { Button(run.resultOpen ? "收起" : "展开全文") { run.resultOpen.toggle() }.buttonStyle(.link) }
                Grid(alignment: .topLeading, horizontalSpacing: 16, verticalSpacing: 12) {
                    if !deliverables.isEmpty { GridRow {
                        section("交付物", deliverables.count)
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(deliverables.indices, id: \.self) { i in
                                let a = deliverables[i]
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack { Text(a["name"].str).fontWeight(.semibold); SeqButton(seq: a["seq"].int, run: run); Button("查看") { ArtifactWindow.open(run: run, path: a["path"].str, title: a["name"].str) }.buttonStyle(.link) }
                                    if a["desc"].truthy { Text(a["desc"].str).font(.callout).foregroundStyle(.secondary) }
                                    Text(L.tilde(a["path"].str)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).help(a["path"].str)
                                }
                            }
                        }
                    } }
                    if !evidence.isEmpty { GridRow {
                        section("证据", evidence.count)
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(evidence.indices, id: \.self) { i in
                                HStack(alignment: .firstTextBaseline) { tagged(evidence[i]["text"].str); SeqButton(seq: evidence[i]["seq"].int, run: run) }
                            }
                        }
                    } }
                    if !risks.isEmpty { GridRow {
                        section("风险与限制", risks.count)
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(risks.indices, id: \.self) { i in HStack(alignment: .firstTextBaseline) { Text("⚠").foregroundStyle(Tone.amber.color).accessibilityLabel("风险"); tagged(risks[i]) } }
                        }
                    } }
                }
                if !suggestions.isEmpty { BriefSuggestions(run: run, lines: suggestions) }
                if canAccept {
                    HStack {
                        Text(risks.isEmpty ? "核对证据后验收。" : "有 \(risks.count) 条风险由 Agent 自己报告，或在核对结果时发现。请逐条核对证据后再验收。").font(.callout)
                        Spacer()
                        Button(risks.isEmpty ? "验收通过" : "核对后接受风险并验收") { Task { await run.act(["type": "accept"]) } }.buttonStyle(.borderedProminent)
                    }
                    .padding(12)
                    .background((risks.isEmpty ? Color.secondary : Tone.amber.color).opacity(0.08), in: RoundedRectangle(cornerRadius: 9))
                }
                HStack {
                    Button("在审计日志中逐条核对") { run.show(.audit) }.buttonStyle(.link)
                }
            }
        }
    }

    private func section(_ t: String, _ n: Int) -> some View {
        HStack(spacing: 5) { Text(t).font(.callout.weight(.semibold)); if n > 0 { Text(verbatim: "\(n)").font(.caption).foregroundStyle(.secondary) } }.frame(width: 90, alignment: .leading)
    }

    /// "T1：text" from the orchestrator: the task id as a tag.
    private func tagged(_ x: String) -> some View {
        let parts = x.split(separator: "：", maxSplits: 1).map(String.init)
        let isTag = parts.count == 2 && parts[0].range(of: #"^[A-Za-z][\w-]*$"#, options: .regularExpression) != nil
        return HStack(alignment: .firstTextBaseline, spacing: 6) {
            if isTag { TaskTag(id: parts[0]) }
            Text(MD.inline(isTag ? parts[1] : x)).font(.callout).textSelection(.enabled)
        }
    }
}

struct TaskTag: View {
    let id: String
    var body: some View {
        Text(id).font(.system(size: 11, weight: .semibold, design: .monospaced))
            .lineLimit(1).truncationMode(.middle)
            .padding(.horizontal, 5).padding(.vertical, 1)
            .background(Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 4))
    }
}

struct AgentsPanel: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let members = run.team
        let active = members.filter { phase($0["id"].str, lead: $0["lead"].truthy) != "idle" }.count
        Card(padding: 12) {
            HStack { Text("Agent").font(.headline); Spacer(); if active > 0 { Text(verbatim: "\(active) 个在工作").font(.caption).foregroundStyle(.secondary) } }
            ForEach(members, id: \.self) { m in row(m["id"].str, lead: m["lead"].truthy) }
        }
    }

    private func row(_ id: String, lead: Bool) -> some View {
        let w = run.who(id)
        let a = st.agents[id]
        let mine = st.orderedTasks.filter { $0.agent == id }
        let cur = mine.first { ["running", "waiting_input", "waiting_approval", "assigned"].contains($0.status) }
        let next = mine.first { ["created", "pending", "ready", "blocked"].contains($0.status) }
        let phase = self.phase(id, lead: lead)
        let (pl, pt) = L.phase(phase)
        var line: String
        if lead {
            line = !st.inbox.isEmpty ? "等你处理 \(st.inbox.count) 项，其余任务继续调度" : cur.map { "\($0.id) \($0.title)" } ?? (!(a?.note ?? "").isEmpty ? a!.note : st.run.status == "delivered" ? "已完成最终验收" : "按依赖调度任务")
        } else if let cur {
            line = "\(cur.id) \(cur.title)"
            if cur.status == "waiting_input" { line += " · 等你回答" } else if cur.status == "waiting_approval" { line += " · 等你审批" } else if let n = a?.note, !n.isEmpty { line += " · \(n)" }
            if cur.control == "human" { line += " · 人工接管中" }
        } else if let next {
            line = next.status == "blocked" ? "\(next.id) 等待 \(next.blockedBy) 修复" : "下一步 \(next.id) · 等待 \(next.deps.filter { st.tasks[$0]?.status != "succeeded" }.joined(separator: "、").isEmpty ? "调度" : next.deps.filter { st.tasks[$0]?.status != "succeeded" }.joined(separator: "、"))"
        } else {
            line = !mine.isEmpty ? "分配的任务都已完成" : !st.order.isEmpty ? "主控没有给它派任务" : "待命"
        }
        var meta: [String] = []
        if phase != "idle", let beat = a?.beat, beat > 0 { meta.append("心跳 \(L.ago(beat, st.run.now))") }
        if !lead, let rs = runStats(id, mine) { meta.append(rs) }
        return Button { run.page = .agent(id) } label: {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 8) {
                    Avatar(initials: w.initials, color: w.color, size: 26)
                    Text(w.name).font(.callout.weight(.semibold))
                    Spacer()
                    Pill(pl, pt)
                }
                Text(line).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                if let cur, cur.status == "running", let p = a?.progress, p > 0 { Bar(value: p) }
            }
            .padding(8)
            .background(Color.secondary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
            .hoverable(8)
        }
        .buttonStyle(.plain)
        .help(([w.role, w.runtime, w.model].filter { !$0.isEmpty } + meta).joined(separator: " · ") + "\n点按查看 \(w.name) 的对话")
    }

    /// A collaborator without a task in progress is idle, whatever its last status said.
    private func phase(_ id: String, lead: Bool) -> String {
        let p = RunState.finished.contains(st.run.status) ? "idle" : st.agents[id]?.phase ?? "idle"
        let busy = st.orderedTasks.contains { $0.agent == id && ["running", "waiting_input", "waiting_approval", "assigned"].contains($0.status) }
        return !lead && !busy && p != "error" ? "idle" : p
    }

    /// "本次 2/3 首次通过 · 验证没通过 1"
    private func runStats(_ id: String, _ mine: [TaskState]) -> String? {
        let ended = mine.filter { ["succeeded", "failed"].contains($0.status) }
        guard !ended.isEmpty else { return nil }
        func vf(_ t: String) -> Int { st.events.filter { $0.type == "verification.failed" && $0.task == t }.count }
        let first = ended.filter { $0.status == "succeeded" && $0.attempts.count == 1 && vf($0.id) == 0 }.count
        let vfail = ended.filter { vf($0.id) > 0 }.count
        return "本次 \(first)/\(ended.count) 首次通过\(vfail > 0 ? " · 验证没通过 \(vfail)" : "")"
    }
}

/// What the lead suggests keeping in the workspace's shared material. Nothing is written until the
/// person opens the workspace settings with the lines added and saves there.
// @project-doc docs/domains/agents_and_roles.md#workspace_brief
struct BriefSuggestions: View {
    let run: RunModel
    let lines: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("主控建议写入共享资料").font(.callout.weight(.semibold))
            ForEach(lines.indices, id: \.self) { i in Text("· \(lines[i])").font(.callout).textSelection(.enabled) }
            HStack {
                Text("采纳后，之后在这个工作区的 Run 都会看到这些内容。").font(.caption).foregroundStyle(.secondary)
                Spacer()
                if let ws = run.store.workspace(run.workspaceId) {
                    Button("加入共享资料…") { run.store.editingWorkspace = Self.adding(lines, to: ws) }
                        .disabled(run.isReplaying)
                        .help("打开工作区设置，把这些内容加到共享资料末尾，确认后保存")
                } else {
                    Text("这个工作区已经删除").font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .padding(12)
        .background(Color.accent.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
    }

    /// The workspace as the settings sheet opens it: its shared material with the lines appended.
    static func adding(_ lines: [String], to ws: JSON) -> JSON {
        var ws = ws
        let current = ws["brief"].str.trimmingCharacters(in: .whitespacesAndNewlines)
        let added = lines.map { "- " + $0 }.joined(separator: "\n")
        ws["brief"] = JSON(current.isEmpty ? added : current + "\n" + added)
        return ws
    }
}
