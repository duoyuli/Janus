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
                    if run.isReplaying, case .board = run.page { ReplayBar(run: run) }
                    page
                }
            }
        }
        .navigationTitle(L.goalTitle(run.state.run.goal.isEmpty ? run.runJSON["goal"].str : run.state.run.goal))
        .navigationSubtitle(run.live ? "" : "只读 · 这个任务不在当前编排器进程里")
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("视图", selection: Binding(get: { tab }, set: { v in
                    run.page = v == "audit" ? .audit : v == "team" ? .team : .board
                    if v != "board" { run.replay = nil }
                })) {
                    Text(run.state.inbox.isEmpty ? "工作台" : "工作台 · \(run.state.inbox.filter { RequestCard.kinds.contains($0.kind) }.count)").tag("board")
                    Text("审计与回放").tag("audit")
                    Text("团队与权限").tag("team")
                }
                .pickerStyle(.segmented)
                .frame(width: 330)
            }
            ToolbarItem {
                Menu {
                    Button("复制交付摘要") { copySummary() }.disabled(run.state.result == nil)
                    Button("导出事件日志（JSONL）…") { export() }
                    Button("在浏览器中打开") { openURL(run.api.base.absoluteString + "/?run=\(run.id)") }
                } label: { Image(systemName: "square.and.arrow.up") }
            }
        }
    }

    private var tab: String {
        switch run.page {
        case .audit: return "audit"
        case .team: return "team"
        default: return "board"
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

    private func copySummary() {
        guard let r = run.state.result else { return }
        let d = r.d
        let risks = d["risks"].strings
        let text = (["\(d["partial"].truthy ? "部分交付" : "已完成")：\(L.goalTitle(run.state.run.goal))", d["summary"].str, "", "证据："]
            + d["evidence"].array.map { "- \($0["text"].str)（事件 #\($0["seq"].str)）" }
            + ["", "风险："] + (risks.isEmpty ? ["- 无"] : risks.map { "- " + $0 })).joined(separator: "\n")
        copyText(text)
        run.store.show("已复制交付摘要。")
    }

    private func export() {
        let p = NSSavePanel()
        p.nameFieldStringValue = "janus-\(run.id).jsonl"
        guard p.runModal() == .OK, let url = p.url else { return }
        Task {
            do {
                let text = try await run.api.text("/api/runs/\(run.id)/export")
                try text.write(to: url, atomically: true, encoding: .utf8)
                run.store.show("已导出 \(url.lastPathComponent)")
            } catch { run.store.show(error.localizedDescription) }
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
            Button { run.replay = max(1, pos - 1) } label: { Image(systemName: "chevron.left") }
            Slider(value: Binding(get: { Double(pos) }, set: { run.replay = max(1, min(n, Int($0.rounded()))) }), in: 1...Double(max(n, 2)))
            Button { run.replay = min(n, pos + 1) } label: { Image(systemName: "chevron.right") }
            Text(e.map { "第 \(pos) / \(n) 条 · \(L.clock($0.ts)) · \(run.who($0.actor).name)：\(L.short($0.text, 40))" } ?? "").font(.caption).lineLimit(1).frame(width: 340, alignment: .leading)
            Button("回到实时 →") { run.replay = nil }.buttonStyle(.borderedProminent)
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
                    TaskGraph(run: run, st: st)
                    SharedBoard(run: run, st: st)
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
        let stages = r.mode == "goal" ? ["推进目标", "协作", "交付", "验收"] : ["确认计划", "执行", "评审", "交付", "验收"]
        let idx = r.mode == "goal" ? goalStageIndex : stageIndex
        let active = !RunState.finished.contains(r.status)
        Card(padding: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    Pill("● \(label)", tone)
                    Text(title.isEmpty ? "正在连接编排器…" : title).font(.title2.weight(.semibold)).textSelection(.enabled)
                    if long {
                        Button(run.goalOpen ? "收起完整目标" : "查看完整目标") { run.goalOpen.toggle() }.buttonStyle(.link)
                        if run.goalOpen { MarkdownText(text: r.goal).padding(.top, 4) }
                    }
                }
                Spacer()
                if run.canAct && active {
                    if r.status == "running" || r.paused {
                        Button(r.paused ? "▶ 继续" : "Ⅱ 暂停") { Task { await run.act(["type": JSON(r.paused ? "resume" : "pause")]) } }
                            .help("暂停后不再启动新任务，进行中的尝试会继续")
                    }
                    Button("取消", role: .destructive) { cancelRun(run) }
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
            FlowLayout(spacing: 16) {
                meta("主控", run.agentName("lead"))
                meta("工作区", wsNote)
                meta("团队", "\(run.team.count) 人")
                meta("开始", L.clock(r.start))
                meta("用时", L.dur(r.now - r.start))
                meta("任务", "\(done) / \(st.order.isEmpty ? "—" : String(st.order.count))")
            }
            HStack(spacing: 6) {
                ForEach(Array(stages.enumerated()), id: \.offset) { i, s in
                    Text(i < idx ? "✓ \(s)" : s)
                        .font(.callout.weight(i == idx ? .semibold : .regular))
                        .foregroundStyle(i < idx ? Tone.green.color : i == idx ? Color.accent : .secondary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                        .background(i == idx ? Color.accent.opacity(0.1) : i < idx ? Tone.green.color.opacity(0.08) : Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                }
            }
        }
    }

    private func meta(_ k: String, _ v: String) -> some View {
        HStack(spacing: 4) { Text(k).foregroundStyle(.secondary); Text(v).fontWeight(.medium) }.font(.callout)
    }

    /// What the run does to its directory, from facts rather than the sentence recorded at creation.
    private var wsNote: String {
        let r = st.run
        if r.git { return r.workspace.isEmpty ? "Git 仓库，基线 \(String(r.baseRef.prefix(7)))" : r.workspace }
        if r.inPlace == true { return "非 Git 目录：任务直接在这个目录中执行，没有 diff" }
        return r.workspace
    }

    private var status: (String, Tone) {
        let r = st.run
        switch r.status {
        case "cancelled": return ("已取消", .gray)
        case "failed": return ("Run 失败", .red)
        case "interrupted": return ("已中断", .gray)
        default: break
        }
        if r.paused { return ("已暂停", .gray) }
        switch r.status {
        case "planning": return ("主控正在启动", .blue)
        case "awaiting_plan": return ("等你确认计划", .amber)
        case "accepted": return ("已验收", .green)
        case "accepted_with_risk": return ("已接受风险", .amber)
        case "waiting_input": return ("等你决策", .amber)
        case "delivered": return ("已交付 · 等你验收", .green)
        default: break
        }
        if !st.inbox.isEmpty { return ("等你处理 \(st.inbox.count) 项", .amber) }
        return ("团队执行中", .blue)
    }

    private var goalStageIndex: Int {
        let r = st.run
        if r.status == "accepted" { return 4 }
        if ["delivered", "accepted_with_risk"].contains(r.status) { return 3 }
        if st.orderedTasks.contains(where: { !["succeeded", "failed", "cancelled"].contains($0.status) }) { return 1 }
        return 0
    }

    private var stageIndex: Int {
        let r = st.run.status
        if ["idle", "planning", "awaiting_plan"].contains(r) { return 0 }
        if r == "delivered" { return 4 }
        if r == "accepted" { return 5 }
        if st.orderedTasks.first(where: { $0.kind == "deliver" })?.attempts.isEmpty == false { return 3 }
        if st.orderedTasks.first(where: { $0.kind == "review" })?.attempts.isEmpty == false { return 2 }
        return 1
    }
}

struct InboxSection: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let open = st.inbox.filter { RequestCard.kinds.contains($0.kind) }
        if open.isEmpty {
            if st.run.status == "planning" { Note(text: "◌ 主控 Agent 正在启动，马上开始推进目标。") }
            else if st.run.status == "running" { Note(text: "✓ 目前没有需要你处理的事项。Agent 的问题和权限请求会出现在这里。") }
        } else {
            SectionHeader(title: "需要你处理", sub: "相关任务会等待你的决定，其他任务继续", count: open.count)
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
            let diag = d["diagnosis"]
            let risks = d["risks"].strings
            let deliverables = d["deliverables"].array
            let evidence = d["evidence"].array
            let long = d["summary"].str.count > 360
            let canAccept = r.accepted == nil && !run.isReplaying
            SectionHeader(title: "交付结果", sub: "每条结论都能追溯到事件")
            Card(padding: 18, tint: Tone.green.color) {
                HStack(spacing: 8) {
                    Pill(d["partial"].truthy ? "部分交付" : "已交付", .green)
                    Pill("置信度 \(d["confidence"].str)", d["confidence"].str == "高" ? .green : .amber)
                    if let a = r.accepted { Pill("✓ 你已于 \(L.clock(a)) 验收", .green) }
                    SeqButton(seq: r.seq, run: run)
                    Spacer()
                }
                Text(d["partial"].truthy ? "目标部分完成" : "目标已完成").font(.title3.weight(.semibold))
                MarkdownText(text: d["summary"].str, lineLimit: long && !run.resultOpen ? 8 : nil)
                if long { Button(run.resultOpen ? "收起" : "展开全文") { run.resultOpen.toggle() }.buttonStyle(.link) }
                Grid(alignment: .topLeading, horizontalSpacing: 16, verticalSpacing: 12) {
                    GridRow {
                        section("交付物", deliverables.count)
                        VStack(alignment: .leading, spacing: 6) {
                            if deliverables.isEmpty { none("没有记录交付物") }
                            ForEach(deliverables.indices, id: \.self) { i in
                                let a = deliverables[i]
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack { Text(a["name"].str).fontWeight(.semibold); SeqButton(seq: a["seq"].int, run: run); Button("查看") { ArtifactWindow.open(run: run, path: a["path"].str, title: a["name"].str) }.buttonStyle(.link) }
                                    if a["desc"].truthy { Text(a["desc"].str).font(.callout).foregroundStyle(.secondary) }
                                    Text(L.tilde(a["path"].str)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).help(a["path"].str)
                                }
                            }
                        }
                    }
                    GridRow {
                        section("证据", evidence.count)
                        VStack(alignment: .leading, spacing: 4) {
                            if evidence.isEmpty { none("没有结构化证据") }
                            ForEach(evidence.indices, id: \.self) { i in
                                HStack(alignment: .firstTextBaseline) { tagged(evidence[i]["text"].str); SeqButton(seq: evidence[i]["seq"].int, run: run) }
                            }
                            if d["review"].truthy { Text(d["review"].str).font(.callout).foregroundStyle(.secondary) }
                        }
                    }
                    GridRow {
                        section("风险与限制", risks.count)
                        VStack(alignment: .leading, spacing: 4) {
                            if risks.isEmpty { none("没有已知风险") }
                            ForEach(risks.indices, id: \.self) { i in HStack(alignment: .firstTextBaseline) { Text("⚠").foregroundStyle(Tone.amber.color); tagged(risks[i]) } }
                        }
                    }
                }
                if diag["cat"].truthy || canAccept {
                    VStack(alignment: .leading, spacing: 8) {
                        if diag["cat"].truthy {
                            Text("偏差诊断 · 问题最可能来自哪里").font(.callout.weight(.semibold))
                            FlowLayout(spacing: 6) { ForEach(L.diagCats, id: \.0) { c in Pill(c.1, diag["cat"].str == c.0 ? .red : .gray) } }
                        }
                        Text(diag["title"].str).fontWeight(.semibold)
                        HStack(alignment: .firstTextBaseline) {
                            Text(diag["text"].str).font(.callout)
                            ForEach(diag["refs"].array.compactMap(\.int), id: \.self) { SeqButton(seq: $0, run: run) }
                        }
                        if canAccept {
                            HStack {
                                ForEach(diag["actions"].array.indices, id: \.self) { i in
                                    Button(diag["actions"][i][1].str) { Task { await run.act(["type": "accept"]) } }
                                }
                                Button(risks.isEmpty ? "验收通过" : "核对后接受风险并验收") { Task { await run.act(["type": "accept"]) } }.buttonStyle(.borderedProminent)
                            }
                        }
                    }
                    .padding(12)
                    .background((diag["cat"].truthy || !risks.isEmpty ? Tone.amber.color : Color.secondary).opacity(0.08), in: RoundedRectangle(cornerRadius: 9))
                }
                HStack {
                    Button("在审计日志中逐条核对 →") { run.page = .audit }.buttonStyle(.link)
                }
            }
        }
    }

    private func section(_ t: String, _ n: Int) -> some View {
        HStack(spacing: 5) { Text(t).font(.callout.weight(.semibold)); if n > 0 { Text(verbatim: "\(n)").font(.caption).foregroundStyle(.secondary) } }.frame(width: 90, alignment: .leading)
    }
    private func none(_ t: String) -> some View { Text(t).font(.callout).foregroundStyle(.secondary) }

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

struct TaskGraph: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        SectionHeader(title: "任务图", sub: st.order.isEmpty ? "" : "从左到右按依赖排列 · 点击任务查看执行详情")
        if st.order.isEmpty {
            if !st.run.goal.isEmpty {
                Note(text: st.run.mode == "goal" ? "主控正在自己推进目标，还没有交给协作者的任务。需要协作时，它提交的编排会出现在这里。" : "主控 Agent 正在理解目标、匹配成员…")
            }
        } else {
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 12) {
                    ForEach(columns.indices, id: \.self) { c in
                        VStack(spacing: 10) {
                            ForEach(columns[c], id: \.id) { t in node(t) }
                        }
                        .frame(width: 220)
                        if c < columns.count - 1 { Image(systemName: "arrow.right").foregroundStyle(.tertiary).padding(.top, 30) }
                    }
                }
                .padding(12)
            }
            .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
        }
    }

    private var columns: [[TaskState]] {
        var depth: [String: Int] = [:]
        func dep(_ id: String, _ seen: Set<String> = []) -> Int {
            if let d = depth[id] { return d }
            guard let t = st.tasks[id], !seen.contains(id) else { return 0 }
            let d = t.deps.isEmpty ? 0 : 1 + (t.deps.map { dep($0, seen.union([id])) }.max() ?? 0)
            depth[id] = d
            return d
        }
        var cols: [[TaskState]] = []
        for id in st.order {
            let d = dep(id)
            while cols.count <= d { cols.append([]) }
            if let t = st.tasks[id] { cols[d].append(t) }
        }
        return cols.filter { !$0.isEmpty }
    }

    private func node(_ t: TaskState) -> some View {
        let (label, tone) = L.task(t.status)
        let w = run.who(t.agent)
        let dim = run.boardSel.map { !$0.tasks.contains(t.id) } ?? false
        return Button { run.page = .task(t.id) } label: {
            VStack(alignment: .leading, spacing: 6) {
                HStack { TaskTag(id: t.id); Spacer(); Pill(label, tone) }
                Text(t.title).font(.callout.weight(.semibold)).lineLimit(3).multilineTextAlignment(.leading)
                HStack(spacing: 5) {
                    Avatar(initials: w.initials, color: w.color, size: 16)
                    Text(w.name + (t.attempts.count > 1 ? " · 第 \(t.attempts.count) 次尝试" : "")).font(.caption).foregroundStyle(.secondary)
                }
                if t.status == "running" { Bar(value: t.progress) }
                if t.status == "blocked" { Text(verbatim: "等待 \(t.blockedBy) 修复后复审").font(.caption).foregroundStyle(.secondary) }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(tone.color.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
            .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(tone.color.opacity(0.35)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .opacity(dim ? 0.35 : 1)
    }
}

struct AgentsPanel: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let members = run.team
        let active = members.filter { phase($0["id"].str, lead: $0["lead"].truthy) != "idle" }.count
        Card(padding: 12) {
            HStack { Text("Agent 状态").font(.headline); Spacer(); Text(verbatim: "\(members.count) 个成员 · \(active) 个活跃").font(.caption).foregroundStyle(.secondary) }
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
            line = !mine.isEmpty ? "分配的任务都已完成" : !st.order.isEmpty ? (st.run.mode == "goal" ? "主控没有给它派任务" : "本次计划没有分配任务") : "待命"
        }
        var meta: [String] = []
        if phase != "idle", let beat = a?.beat, beat > 0 { meta.append("心跳 \(L.ago(beat, st.run.now))") }
        if !lead, let rs = runStats(id, mine) { meta.append(rs) }
        let dim = run.boardSel.map { !$0.agents.contains(id) } ?? false
        return Button { run.page = .agent(id) } label: {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 8) {
                    Avatar(initials: w.initials, color: w.color, size: 26)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(w.name).font(.callout.weight(.semibold))
                        Text([w.role, w.runtime, w.model].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    Pill(pl, pt)
                }
                Text(line).font(.caption).foregroundStyle(.primary.opacity(0.85)).lineLimit(3)
                if let cur, cur.status == "running", let p = a?.progress, p > 0 { Bar(value: p) }
                if !meta.isEmpty { Text(meta.joined(separator: " · ")).font(.caption2).foregroundStyle(.secondary) }
            }
            .padding(8)
            .background(Color.secondary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .opacity(dim ? 0.4 : 1)
        .help("查看 \(w.name) 的对话")
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
