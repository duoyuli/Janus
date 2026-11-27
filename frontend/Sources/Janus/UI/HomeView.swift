import AppKit
import SwiftUI

/// A workspace, all of it on one page (the sidebar only lists workspaces). The goal to start is
/// what the page is for: a wide box right under who the workspace is, with the keyboard in it.
/// Beside the name, what the workspace's runs know (the shared material every run is given, the
/// team memory its agents search); under the box, what waits on the person and the task records,
/// with the Loops in a column of their own.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct HomeView: View {
    let store: AppStore
    let ws: Workspace
    private var sendKey: SendKey { SendKeySetting.shared.value }

    private let suggestions = [
        (L.text("新功能上线"), L.text("为这个产品规划并上线一个新功能：先确认目标用户和成功标准，写 PRD，实现并测试，准备发布说明")),
        (L.text("竞品与定位"), L.text("调研这个产品的主要竞品和目标用户，整理定位差异和三个最值得做的机会，给出依据")),
        (L.text("发布前检查"), L.text("检查这个产品是否可以发布：核对主要用户流程、已知缺陷、回滚方式和发布说明，给出质量结论")),
    ]

    var body: some View {
        let members = ws.members.map { m in (m, store.role(m.role)) }
        let problems = ws.problems
        let runs = store.runs(of: ws)
        // What waits on the person, the same as the inbox and the count in the sidebar; those runs
        // are listed there only, not again in the task records.
        let waiting = store.waiting(in: ws)
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // The workspace's own problems below say the same, more precisely.
                if problems.isEmpty { OnboardingBar(store: store) }
                // Who the workspace is, and what its runs know beside it; under it when there is no room.
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .center, spacing: 24) {
                        identity(members).frame(minWidth: 0, idealWidth: 320, maxWidth: .infinity, alignment: .leading)
                        knowledge
                    }
                    VStack(alignment: .leading, spacing: 14) {
                        identity(members)
                        knowledge
                    }
                }
                composer(problems, runs: runs)
                // One line per cause, with the place that fixes it.
                ForEach(problems, id: \.text) { p in
                    HStack {
                        Note(text: p.text, tone: .amber)
                        Button(p.action) { store.openSettings(p.tab) }
                    }
                }
                // The Loops in a column of their own beside what waits and the records; under what
                // waits, above the records (which read on as they scroll), when there is no room.
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: 24) {
                        VStack(alignment: .leading, spacing: 20) {
                            waitingBlock(waiting)
                            WorkspaceHistory(store: store, workspace: ws.id, hiding: Set(waiting.map(\.id)))
                        }
                        .frame(minWidth: 0, idealWidth: 480, maxWidth: .infinity, alignment: .leading)
                        LoopSection(store: store, ws: ws).frame(width: 270)
                    }
                    VStack(alignment: .leading, spacing: 20) {
                        waitingBlock(waiting)
                        LoopSection(store: store, ws: ws)
                        WorkspaceHistory(store: store, workspace: ws.id, hiding: Set(waiting.map(\.id)))
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    /// What waits on the person here, the same as the inbox and the count in the sidebar; those
    /// runs are listed here only, not again in the task records.
    @ViewBuilder private func waitingBlock(_ waiting: [WaitingRow]) -> some View {
        if !waiting.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                let n = waiting.reduce(0) { $0 + $1.entries.count }
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(L.text("待处理")).font(.headline)
                    Text(verbatim: "\(n)").font(.headline).foregroundStyle(Tone.amber.color).accessibilityLabel(L.text("\(n) 项"))
                }
                WaitingRows(store: store, rows: waiting)
            }
        }
    }

    /// The goal to start: what the page is for, wide, with the keyboard in it as the page opens.
    /// The send button is in the box, at the end of its bar.
    private func composer(_ problems: [(text: String, action: String, tab: AppStore.SettingsTab)], runs: [JSON]) -> some View {
        let empty = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && store.goalImages.isEmpty
        return VStack(alignment: .leading, spacing: 10) {
            Composer(text: Binding(get: { store.goalDraft }, set: { store.goalDraft = $0 }), images: Binding(get: { store.goalImages }, set: { store.goalImages = $0 }),
                     placeholder: L.text("想要什么结果？"), minHeight: 110, focusOnAppear: true) { start(problems) } footer: {
                Text(L.text("可以 ⌘V 贴截图")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text(verbatim: sendKey.symbol).font(.caption).foregroundStyle(.secondary)
                Button(L.text("交给负责人")) { start(problems) }
                    .buttonStyle(.primary)
                    .disabled(!problems.isEmpty || empty)
                    .help(!problems.isEmpty ? L.text("还有数字员工不能运行") : empty ? L.text("先写下你想要的结果") : sendKey.symbol)
            }
            // Examples only help before the first run.
            if runs.isEmpty {
                HStack {
                    ForEach(suggestions, id: \.0) { s in
                        Button(s.0) { store.goalDraft = s.1 }.buttonStyle(.bordered).controlSize(.small).help(s.1)
                    }
                }
            }
        }
    }

    /// The shared material and the team memory, a tile each: what the workspace's runs know. The
    /// person writes the one, which every run is given; the agents write the other and search it
    /// when they need it. Each says whether there is anything and how it reaches the agents; a
    /// click shows it.
    private var knowledge: some View {
        let id = ws.id
        let status = Self.knowledge(ws, store: store)
        return HStack(spacing: 10) {
            KnowledgeTile(symbol: "doc.text", tint: .accent, title: L.text("共享资料"), status: status.brief, way: L.text("每个任务都带上")) { leave in
                sharedMaterial(ws.brief.trimmingCharacters(in: .whitespacesAndNewlines), leave: leave)
            }
            KnowledgeTile(symbol: "brain", tint: .memory, title: L.text("团队记忆"), status: status.memory, way: L.text("需要时检索")) { leave in
                MemoryPanel(store: store, ws: ws, leave: leave)
            }
        }
        .fixedSize()
        .task(id: id) { await store.loadMemories(id) }
    }

    /// What the two tiles say there is: how long the shared material is, how many notes the team
    /// memory holds (or that it is being consolidated).
    static func knowledge(_ ws: Workspace, store: AppStore) -> (brief: String, memory: String) {
        let brief = ws.brief.trimmingCharacters(in: .whitespacesAndNewlines)
        let notes = store.notes(of: ws.id).count
        let consolidating = store.memories[ws.id]?["consolidation"]["running"].string != nil
        return (brief.isEmpty ? L.text("还没写") : L.text("\(brief.count) 字"),
                consolidating ? L.text("整理中") : notes == 0 ? L.text("还没有笔记") : L.text("\(notes) 条笔记"))
    }

    /// The workspace's shared material, in the popover of its tile. It is edited in its own sheet
    /// (`BriefEditor`), apart from the team.
    // @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
    private func sharedMaterial(_ brief: String, leave: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text(L.text("共享资料")).font(.headline)
                HowItWorks(title: L.text("共享资料"), lines: [
                    L.text("你写的，只有你能改；Agent 不能直接写进来。"),
                    L.text("每个任务开始时原样交给负责人和每个协作者，值班开工时也带上；和本次目标冲突时以目标为准。"),
                    L.text("任务开始时就固定下来，之后的修改从下一个任务起生效。"),
                    L.text("负责人交付时会建议值得长期保留的新事实，你点「加入共享资料…」、确认保存后才写入。"),
                ])
                Spacer()
                IconButton(symbol: brief.isEmpty ? "plus" : "square.and.pencil",
                           label: brief.isEmpty ? L.text("添加共享资料") : L.text("编辑共享资料"),
                           help: L.text("产品定位、目标用户、约定、重要文件位置……每次任务都交给团队，写过的不再重复调研或问你。")) { leave(); store.editingBrief = ws.json }
            }
            if brief.isEmpty {
                Text(L.text("产品定位、目标用户、约定、重要文件位置……每次任务都交给团队，写过的不再重复调研或问你。")).font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                ScrollView {
                    Text(brief).font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 360)
            }
        }
    }

    /// The name, the folder and the team, one under the other; models, permissions and tools are in the
    /// workspace settings, reached from the 「工作区操作」 menu beside the name (one way in, not two).
    private func identity(_ members: [(Workspace.Member, Role?)]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(ws.name).font(.display(.largeTitle))
                    Menu {
                        Button(L.text("在 Finder 中显示")) { NSWorkspace.shared.open(URL(fileURLWithPath: ws.path)) }
                        Button(L.text("工作区设置…")) { store.editingWorkspace = ws.json }
                    } label: { Label(L.text("工作区操作"), systemImage: "ellipsis.circle") }
                    .labelStyle(.iconOnly).menuStyle(.borderlessButton).fixedSize().help(L.text("工作区操作"))
                }
                Text(L.tilde(ws.path)).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).help(ws.path).textSelection(.enabled)
            }
            team(members)
        }
    }

    private func team(_ members: [(Workspace.Member, Role?)]) -> some View {
        let lead = store.role(ws.lead)
        let totalSlots = members.reduce(0) { acc, pair in
            acc + (pair.0.role == ws.lead ? 1 : max(1, pair.0.slots))
        }
        let countLabel = totalSlots > members.count
            ? L.text("\(members.count) 个数字员工（共 \(totalSlots) 个分身）")
            : L.text("\(members.count) 个数字员工")
        return HStack(spacing: 10) {
            HStack(spacing: -4) {
                ForEach(members.indices, id: \.self) { i in
                    let r = members[i].1
                    Avatar(initials: r?.initials ?? "?", color: r?.color ?? "#64748b", size: 22, role: r?.id ?? "")
                        .overlay(RoundedRectangle(cornerRadius: 22 * 0.28).strokeBorder(Color.page, lineWidth: 1.5))
                        .help(r?.name ?? L.text("已删除的数字员工"))
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(members.compactMap { $0.1?.name }.joined(separator: L.text("、")))
            Text(verbatim: "\(countLabel)\(lead.map { $0.name == L.text("负责人") ? L.text(" · 有负责人") : L.text(" · 负责人：\($0.name)") } ?? "")")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func start(_ problems: [(text: String, action: String, tab: AppStore.SettingsTab)]) {
        let goal = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        let images = store.goalImages
        guard !goal.isEmpty || !images.isEmpty, problems.isEmpty else { return }
        Task { await store.createRun(goal: goal, workspace: ws.id, images: images) }
    }
}

/// One of the two things a workspace's runs know, beside its name: a tinted glyph that tells it
/// from the other, its name, and a line saying whether there is anything and how it reaches the
/// agents. A click shows it in a popover; `content` gets the way to close it before a sheet or a
/// page opens.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct KnowledgeTile<Content: View>: View {
    let symbol: String
    let tint: Color
    let title: String
    let status: String
    let way: String
    @ViewBuilder let content: (_ leave: @escaping () -> Void) -> Content
    @Local private var shown = false
    @Local private var hovered = false

    var body: some View {
        Button { shown.toggle() } label: {
            HStack(spacing: 10) {
                Image(systemName: symbol)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(tint)
                    .frame(width: 30, height: 30)
                    .background(tint.opacity(0.15), in: RoundedRectangle(cornerRadius: 8))
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.callout.weight(.semibold))
                    Text(verbatim: "\(status) · \(way)").font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(.leading, 9).padding(.trailing, 14).padding(.vertical, 8)
            .background(tint.opacity(hovered || shown ? 0.12 : 0.06), in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(tint.opacity(hovered || shown ? 0.45 : 0.28)))
            .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title)，\(status)，\(way)")
        .accessibilityAddTraits(.isButton)
        .popover(isPresented: $shown, arrowEdge: .bottom) {
            content { shown = false }
                .padding(14)
                .frame(width: 380, alignment: .leading)
        }
    }
}

/// Every task record of a workspace, newest first, read a page at a time as the list scrolls; it
/// follows the run list, which the global stream keeps current. Also the page of a deleted
/// workspace, whose runs stay.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct WorkspaceHistory: View {
    let store: AppStore
    let workspace: String
    /// Runs listed elsewhere on the page (those waiting on the person), left out here.
    var hiding: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let history = store.histories[workspace], !history.runs.isEmpty {
                let shown = history.runs.filter { !hiding.contains($0["id"].str) }
                if !shown.isEmpty {
                    Text(L.text("任务记录")).font(.headline)
                    RunRows(store: store, runs: shown, dated: true) { Task { await history.loadMore() } }
                }
                // Shown at the end while there is more: also reads on when every row of the page
                // read so far is listed elsewhere.
                if history.more { ProgressView().controlSize(.small).frame(maxWidth: .infinity).onAppear { Task { await history.loadMore() } } }
            } else {
                // Something on screen before the first read: an empty container is never shown,
                // and its `task` would never run, so the records would never be read.
                Color.clear.frame(height: 0)
            }
        }
        .task(id: workspace) { await store.loadHistory(workspace) }
    }
}

/// A workspace that was deleted but whose runs stay: its name and folder as the runs recorded them,
/// and every one of its task records, to open, review or delete.
struct PastWorkspaceView: View {
    let store: AppStore
    let ws: JSON

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(ws["name"].str.isEmpty ? L.text("已删除的工作区") : ws["name"].str).font(.display(.largeTitle))
                    Text(L.tilde(ws["path"].str)).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                }
                Note(text: L.text("工作区已删除。留下的任务记录还能查看、继续对话或删除；想在这里提新目标，新建一个工作区。"))
                WorkspaceHistory(store: store, workspace: ws["id"].str)
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}

/// How a run's row names its state, as the server words it (a chat in session or ended, a queued
/// run with its place in the queue).
func runLabel(_ r: JSON) -> (String, Tone) {
    (r["label"]["text"].str, Tone(rawValue: r["label"]["tone"].str) ?? .gray)
}

/// A Loop's own mark, wherever the Loop itself is shown (its row, its page): a tinted tile, so
/// it reads apart from what waits on the person and from the goals it dispatched.
struct LoopBadge: View {
    var size: CGFloat = 24

    var body: some View {
        Image(systemName: "clock.arrow.circlepath")
            .font(.system(size: size * 0.55, weight: .semibold))
            .foregroundStyle(Tone.violet.color)
            .frame(width: size, height: size)
            .background(Tone.violet.color.opacity(0.14), in: RoundedRectangle(cornerRadius: size * 0.28))
            .accessibilityHidden(true)
    }
}

/// A small tag after the title of a goal a Loop dispatched: which Loop, and (on hover) its key
/// and source. The goal stays the row's subject; the tag only says where it came from.
struct LoopMark: View {
    let store: AppStore
    let run: JSON

    var body: some View {
        if RunOrigin(run["origin"].str) == .loop {
            let name = store.loops.first { $0["id"] == run["loopId"] }?["title"].string ?? L.text("已删除的值班")
            HStack(spacing: 3) {
                Image(systemName: "clock.arrow.circlepath").accessibilityHidden(true)
                Text(name).lineLimit(1)
            }
            .font(.caption).foregroundStyle(.secondary)
            .fixedSize()
            .help(L.text("来自值班：\(name)") + (run["dispatchKey"].string.map { " · \($0)" } ?? "") + (run["sourceUrl"].string.map { " · \($0)" } ?? ""))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(L.text("来自值班：\(name)"))
        }
    }
}

/// What waits on the person in a workspace, one row per goal or Loop: a glyph and pill for what it
/// waits on (「等你验收」「等你拍板」「2 件事」…), then what it is; a click opens it.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct WaitingRows: View {
    let store: AppStore
    let rows: [WaitingRow]

    var body: some View {
        Card(padding: 4) {
            ForEach(rows) { w in
                Button { store.open(.run(w.id)) } label: {
                    HStack(spacing: 8) {
                        Image(systemName: w.symbol).foregroundStyle(w.tone.color).frame(width: 18).accessibilityHidden(true)
                        if w.loop != nil {
                            Text(w.title).lineLimit(1)
                            Text(L.text("值班")).font(.caption).foregroundStyle(.secondary)
                        } else {
                            Text(w.title).lineLimit(1)
                            LoopMark(store: store, run: w.run)
                        }
                        Spacer(minLength: 12)
                        Pill(w.label, w.tone)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .contentShape(Rectangle())
                    .hoverable(6)
                    .help(w.entries.map(\.title).joined(separator: "\n"))
                }
                .buttonStyle(.plain)
                if w != rows.last { Divider() }
            }
        }
    }
}

/// Runs of a workspace in one card, one row each: the goal (marked when a Loop dispatched it), when
/// it started (`dated`), how it stands, a click to open it and a right click to delete it.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct RunRows: View {
    let store: AppStore
    let runs: [JSON]
    var dated = false
    /// Called when the last row shows, to read the next page.
    var atEnd: (() -> Void)?

    var body: some View {
        Card(padding: 4) {
            ForEach(runs, id: \.self) { r in
                let waiting = store.inbox.open.filter { $0.run == r["id"].str }.count
                let (l, t) = waiting > 0 ? (L.text("待处理"), Tone.amber) : runLabel(r)
                Button { store.open(.run(r["id"].str)) } label: {
                    HStack(spacing: 8) {
                        Text(r["title"].str).lineLimit(1)
                        LoopMark(store: store, run: r)
                        Spacer(minLength: 12)
                        if dated { Text(L.dateTime(r["createdAt"].double ?? 0)).font(.caption).foregroundStyle(.secondary) }
                        Pill(l, t)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button(L.text("删除任务…"), role: .destructive) { Task { await store.deleteRun(r["id"].str) } }
                        .disabled(r["delete"]["ok"].bool != true)
                }
                .onAppear { if r == runs.last { atEnd?() } }
                if r != runs.last { Divider() }
            }
        }
    }
}

/// New or existing workspace: which directory, which roles, who leads. The shared material is
/// edited apart from it (`BriefEditor`); saving here keeps what is saved there.
struct WorkspaceEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var name = ""
    @Local private var path = ""
    @Local private var lead = ""
    @Local private var on: Set<String> = []
    @Local private var slots: [String: Int] = [:]
    @Local private var inspect: JSON?
    @Local private var saving = false

    var body: some View {
        let isNew = original["id"].str.isEmpty
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(isNew ? L.text("新建工作区") : L.text("工作区设置 · \(original["name"].str)")).font(.display(.title2))
                Spacer()
            }
            .padding(20)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text(L.text("项目目录")).font(.headline)
                    HStack {
                        TextField("/path/to/your/repo", text: $path).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                        Button(L.text("选择文件夹…")) { pickFolder() }
                    }
                    Text(inspectLine).font(.callout).foregroundStyle(inspect?["exists"].bool == false ? Tone.red.color : .secondary)
                    TextField(L.text("名称（可选，默认用目录名）"), text: $name).textFieldStyle(.roundedBorder)


                    HStack {
                        Text(L.text("启用的数字员工")).font(.headline)
                        Spacer()
                        if store.roleGroups.count > 1 {
                            Menu(L.text("按分组选…")) {
                                ForEach(store.roleGroups, id: \.self) { g in Button(AppStore.groupTitle(g)) { useGroup(g) } }
                            }
                            .fixedSize()
                            .help(L.text("只启用这个分组里绑定了 Agent 的数字员工，其中能担任负责人的当负责人"))
                        }
                    }
                    .padding(.top, 6)
                    if store.roles.isEmpty { Note(text: L.text("还没有数字员工，先去「设置 › 数字员工」建一个。")) }
                    // By group, each headed by its name when there is more than one.
                    let ordered = store.roleSections.flatMap(\.roles)
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(ordered.enumerated()), id: \.element) { i, r in
                            if store.roleGroups.count > 1, i == 0 || ordered[i - 1].group != r.group {
                                Text(AppStore.groupTitle(r.group)).font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.top, 10).padding(.bottom, 2)
                            }
                            let id = r.id
                            let c = store.connection(r.connection)
                            HStack(spacing: 10) {
                                Toggle(L.text("启用 \(r.name)"), isOn: Binding(get: { on.contains(id) }, set: { v in if v { on.insert(id) } else { on.remove(id); if lead == id { lead = "" } } }))
                                    .labelsHidden().disabled(c == nil)
                                Avatar(initials: r.displayInitials, color: r.color, size: 24, role: r.id)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(r.name).font(.body.weight(.medium))
                                    Text(c.map { "\($0["name"].str) · \(r.model.isEmpty ? L.text("默认模型") : r.model)" } ?? L.text("未绑定 Agent")).font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(Setup.permLabel(r.session, c).isEmpty ? "—" : Setup.permLabel(r.session, c)).font(.caption).foregroundStyle(.secondary)
                                if on.contains(id) && lead != id {
                                    let count = min(Role.maxSlots, max(1, slots[id] ?? 1))
                                    Stepper(value: Binding(get: { count }, set: { slots[id] = min(Role.maxSlots, max(1, $0)) }), in: 1...Role.maxSlots) {
                                        Text(verbatim: L.text("\(count) 个分身")).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                                    }
                                    .controlSize(.small)
                                    .help(L.text("分身数：大于 1 时可以同时接多个子任务"))
                                }
                                if r.lead && Setup.leadCapable(c) {
                                    Button { lead = id } label: {
                                        Label(L.text("负责人"), systemImage: lead == id ? "largecircle.fill.circle" : "circle")
                                    }
                                    .buttonStyle(.borderless)
                                    .disabled(!on.contains(id))
                                    .help(!on.contains(id) ? L.text("先启用这个数字员工") : L.text("设为负责人"))
                                    .accessibilityLabel(L.text("\(r.name) 担任负责人"))
                                    .accessibilityAddTraits(lead == id ? .isSelected : [])
                                }
                            }
                            .padding(.vertical, 7)
                            .opacity(on.contains(id) ? 1 : 0.6)
                            Divider()
                        }
                    }
                }
                .padding(20)
            }
            Divider()
            HStack {
                if !isNew {
                    Button(L.text("删除工作区…"), role: .destructive) { remove() }
                }
                Spacer()
                Button(L.text("取消")) { if !dirty || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                Button(saving ? L.text("正在保存…") : isNew ? L.text("创建工作区") : L.text("保存")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction)
                    .disabled(saving || path.trimmingCharacters(in: .whitespaces).isEmpty || inspect?["exists"].bool == false)
            }
            .padding(16)
        }
        .frame(minWidth: 600, idealWidth: 640, minHeight: 440, idealHeight: 640)
        .onAppear(perform: setup)
        .task(id: path) {
            try? await Task.sleep(nanoseconds: 250_000_000)
            // Not `empty ? nil : …`: JSON takes nil literals, so that `nil` would be `JSON.null`.
            if path.trimmingCharacters(in: .whitespaces).isEmpty { inspect = nil } else { inspect = try? await store.api.get("/api/inspect?path=\(API.q(path))") }
        }
    }

    private var inspectLine: String {
        if path.trimmingCharacters(in: .whitespaces).isEmpty { return L.text("先选一个目录") }
        guard let i = inspect else { return L.text("检查中…") }
        if i["exists"].bool == false { return L.text("目录不存在") }
        if !i["git"].truthy { return L.text("不是 Git 仓库：直接改这个目录，没有 diff") }
        let dirty = i["dirty"].int ?? 0
        return L.text("✓ Git 仓库 · 当前分支 \(i["branch"].str) · \(dirty > 0 ? L.text("⚠ 有 \(dirty) 个未提交改动（不会带进任务）") : L.text("工作区干净"))")
    }

    /// Edits not saved yet, so Cancel and Esc ask before throwing them away.
    private var dirty: Bool { initial.map { $0 != snapshot } ?? false }
    private var snapshot: [String] {
        let slotSpec = on.sorted().map { id in "\(id):\(lead == id ? 1 : min(Role.maxSlots, max(1, slots[id] ?? 1)))" }.joined(separator: ",")
        return [name, path, lead, slotSpec]
    }
    @Local private var initial: [String]?

    private func setup() {
        defer { initial = snapshot }
        let isNew = original["id"].str.isEmpty
        name = original["name"].str
        path = original["path"].str
        let rawMembers = original["members"].array
        let members = rawMembers.map { $0["role"].str }
        var loadedSlots: [String: Int] = [:]
        for m in rawMembers {
            loadedSlots[m["role"].str] = min(Role.maxSlots, max(1, m["slots"].int ?? 1))
        }
        slots = loadedSlots
        // A new workspace starts with the team the server proposes (the lead's group).
        lead = original["lead"].truthy ? original["lead"].str : store.defaultTeam["lead"].str
        on = Set(isNew ? store.defaultTeam["members"].strings : members)
    }

    /// Take group `g` as the team: its members bound to an Agent, one of them that can lead as
    /// the lead (the current lead when it is in the group).
    private func useGroup(_ g: String) {
        let group = store.defaultTeam["groups"][g]
        on = Set(group["members"].strings)
        let leads = group["leads"].strings
        lead = leads.contains(lead) ? lead : leads.first ?? ""
    }

    private func pickFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.allowsMultipleSelection = false
        // A new project starts in a folder that does not exist yet.
        p.canCreateDirectories = true
        p.prompt = L.text("使用这个文件夹")
        if !path.isEmpty { p.directoryURL = URL(fileURLWithPath: path) }
        if p.runModal() == .OK, let u = p.url { path = u.path }
    }

    private func save() {
        saving = true
        let members = store.roles.map(\.id).filter { on.contains($0) }.map { id -> JSON in
            let n = lead == id ? 1 : min(Role.maxSlots, max(1, slots[id] ?? 1))
            return n > 1 ? JSON.object(["role": JSON(id), "slots": JSON(n)]) : JSON.object(["role": JSON(id)])
        }
        // The workspace is saved whole: keep its shared material as saved now.
        let brief = store.workspace(original["id"].str).map { JSON($0.brief) } ?? original["brief"]
        let body: JSON = ["id": original["id"], "name": JSON(name), "path": JSON(path), "lead": JSON(lead), "members": .array(members), "brief": brief]
        Task {
            do {
                let ws = try await store.save(.workspaces, body)
                store.show(L.text("已保存工作区 \(ws["name"].str)"))
                dismiss()
                store.open(.home(ws["id"].str))
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func remove() {
        guard confirm(L.text("删除工作区「\(original["name"].str)」？"), L.text("任务记录会保留，在侧栏的「团队」里还能找到。"), ok: L.text("删除")) else { return }
        Task {
            do {
                try await store.delete(.workspaces, original["id"].str)
                dismiss()
                if let w = store.workspaces.first { store.open(.home(w.id)) } else { store.open(.newWorkspace) }
            } catch { store.show(error.localizedDescription) }
        }
    }
}

/// A workspace's shared material, on its own: what every goal and Loop of the workspace is given.
/// Opened from the workspace page, or from a delivered run with the lead's suggested lines added
/// (those count as unsaved). The team stays in the workspace settings.
// @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
struct BriefEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var brief = ""
    @Local private var saving = false

    /// The backend's `BRIEF_LIMIT`: the shared material goes into every session's prompt.
    static let limit = 8000

    /// What is saved now; the sheet may have opened with lines added to it.
    private var saved: String { store.workspace(original["id"].str)?.brief ?? original["brief"].str }
    private var dirty: Bool { brief != saved }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(L.text("共享资料 · \(original["name"].str)")).font(.display(.title2)).padding(20)
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                Text(L.text("产品定位、目标用户、约定、重要文件位置……每次任务都交给团队，写过的不再重复调研或问你。")).font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                TextEditor(text: $brief)
                    .font(.body)
                    .frame(minHeight: 240)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(brief.count > Self.limit ? Tone.red.color : Color.line))
                    .accessibilityLabel(L.text("共享资料"))
                HStack {
                    if dirty { Text(L.text("还没保存 · 保存后下个任务起生效")).font(.caption).foregroundStyle(Tone.amber.color) }
                    Spacer()
                    Text(verbatim: "\(brief.count) / \(Self.limit)").font(.caption).foregroundStyle(brief.count > Self.limit ? Tone.red.color : .secondary)
                }
            }
            .padding(20)
            Divider()
            HStack {
                Spacer()
                Button(L.text("取消")) { if !dirty || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                Button(saving ? L.text("正在保存…") : L.text("保存")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction)
                    .disabled(saving || !dirty || brief.count > Self.limit)
            }
            .padding(16)
        }
        .frame(minWidth: 560, idealWidth: 600, minHeight: 420, idealHeight: 480)
        .onAppear { brief = original["brief"].str }
    }

    /// The workspace is saved whole: everything but the shared material as it is saved now.
    static func body(_ ws: JSON, brief: String) -> JSON {
        ["id": ws["id"], "name": ws["name"], "path": ws["path"], "lead": ws["lead"], "members": ws["members"], "brief": JSON(brief)]
    }

    private func save() {
        saving = true
        let body = Self.body(store.workspace(original["id"].str)?.json ?? original, brief: brief)
        Task {
            do {
                _ = try await store.save(.workspaces, body)
                store.show(L.text("已保存共享资料"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }
}
