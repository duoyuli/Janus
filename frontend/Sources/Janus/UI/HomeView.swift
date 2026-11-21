import AppKit
import SwiftUI

/// A workspace: start a goal; the team in one line and only the runs that wait on the person.
struct HomeView: View {
    let store: AppStore
    let ws: JSON

    private let suggestions = [
        (L.text("新功能上线"), L.text("为这个产品规划并上线一个新功能：先确认目标用户和成功标准，写 PRD，实现并测试，准备发布说明")),
        (L.text("竞品与定位"), L.text("调研这个产品的主要竞品和目标用户，整理定位差异和三个最值得做的机会，给出依据")),
        (L.text("发布前检查"), L.text("检查这个产品是否可以发布：核对主要用户流程、已知缺陷、回滚方式和发布说明，给出质量结论")),
    ]

    var body: some View {
        let members = ws["members"].array.map { m in (m, store.role(m["role"].str)) }
        let problems = Self.problems(members.map(\.1), connection: store.connection)
        let runs = store.runs(of: ws)
        // Runs waiting on the person, the same as the inbox: any run with something waiting on them.
        // Below them, every task record of this workspace, newest first (`WorkspaceHistory`).
        let waiting = runs.filter { r in store.inbox.open.contains { $0.run == r["id"].str } }
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // The workspace's own problems below say the same, more precisely.
                if problems.isEmpty { OnboardingBar(store: store) }
                HStack(alignment: .firstTextBaseline) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(ws["name"].str).font(.display(.largeTitle))
                        Text(L.tilde(ws["path"].str)).font(.callout).foregroundStyle(.secondary).help(ws["path"].str).textSelection(.enabled)
                    }
                    Spacer()
                    Menu {
                        Button(L.text("在 Finder 中显示")) { NSWorkspace.shared.open(URL(fileURLWithPath: ws["path"].str)) }
                        Button(L.text("工作区设置…")) { store.editingWorkspace = ws }
                    } label: { Label(L.text("工作区操作"), systemImage: "ellipsis.circle") }
                    .labelStyle(.iconOnly).menuStyle(.borderlessButton).fixedSize().help(L.text("工作区操作"))
                }

                VStack(alignment: .leading, spacing: 10) {
                    Composer(text: Binding(get: { store.goalDraft }, set: { store.goalDraft = $0 }), images: Binding(get: { store.goalImages }, set: { store.goalImages = $0 }), placeholder: L.text("说出你想要的结果，可以 ⌘V 粘贴截图，⌘↩ 交给负责人"), minHeight: 96) { start(problems) }
                    HStack {
                        // Examples only help before the first run.
                        if runs.isEmpty {
                            ForEach(suggestions, id: \.0) { s in
                                Button(s.0) { store.goalDraft = s.1 }.buttonStyle(.bordered).controlSize(.small).help(s.1)
                            }
                        }
                        Spacer()
                        let empty = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && store.goalImages.isEmpty
                        Button(L.text("交给负责人")) { start(problems) }
                            .buttonStyle(.primary)
                            .disabled(!problems.isEmpty || empty)
                            .help(!problems.isEmpty ? L.text("还有数字员工不能运行") : empty ? L.text("先写下你想要的结果") : "⌘↩")
                    }
                }
                // One line per cause, with the place that fixes it.
                ForEach(problems, id: \.text) { p in
                    HStack {
                        Note(text: p.text, tone: .amber)
                        Button(p.action) { store.openSettings(p.tab) }
                    }
                }

                team(members)

                sharedMaterial

                if !waiting.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(L.text("等你处理")).font(.headline)
                        RunRows(store: store, runs: waiting)
                    }
                }

                WorkspaceHistory(store: store, workspace: ws["id"].str)
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    /// The team in one line; models, permissions and tools are in the workspace settings.
    private func team(_ members: [(JSON, JSON?)]) -> some View {
        let lead = store.role(ws["lead"].str)
        let totalSlots = members.reduce(0) { acc, pair in
            acc + (pair.0["role"].str == ws["lead"].str ? 1 : max(1, pair.0["slots"].int ?? 1))
        }
        let countLabel = totalSlots > members.count
            ? L.text("\(members.count) 个数字员工（共 \(totalSlots) 个并发实例）")
            : L.text("\(members.count) 个数字员工")
        return HStack(spacing: 10) {
            Text(L.text("团队")).foregroundStyle(.secondary)
            HStack(spacing: -4) {
                ForEach(members.indices, id: \.self) { i in
                    let r = members[i].1
                    Avatar(initials: r?["init"].str ?? "?", color: r?["color"].str ?? "#64748b", size: 22, role: r?["id"].str ?? "")
                        .overlay(RoundedRectangle(cornerRadius: 22 * 0.28).strokeBorder(Color.page, lineWidth: 1.5))
                        .help(r?["name"].str ?? L.text("已删除的数字员工"))
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(members.compactMap { $0.1?["name"].str }.joined(separator: L.text("、")))
            Text(verbatim: "\(countLabel)\(lead.map { $0["name"].str == L.text("负责人") ? L.text(" · 有负责人") : L.text(" · 负责人：\($0["name"].str)") } ?? "")").font(.callout).foregroundStyle(.secondary)
            Spacer()
            Button(L.text("管理团队…")) { store.editingWorkspace = ws }.buttonStyle(.link)
        }
    }

    /// The workspace's shared material, readable where goals start; editing stays in the workspace settings.
    // @project-doc docs/domains/20261031-agents_and_roles.md#workspace_brief
    @ViewBuilder private var sharedMaterial: some View {
        let brief = ws["brief"].str.trimmingCharacters(in: .whitespacesAndNewlines)
        let long = brief.split(separator: "\n", omittingEmptySubsequences: false).count > Self.previewLines || brief.count > 400
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(L.text("共享资料")).font(.headline)
                Text(L.text("每次任务开始时交给负责人和协作者")).font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button(brief.isEmpty ? L.text("添加…") : L.text("编辑…")) { store.editingWorkspace = ws }.buttonStyle(.link)
                    .help(L.text("产品定位、目标用户、约定、重要文件的位置：写下后，之后的任务不必重新调研或再问你"))
            }
            if !brief.isEmpty {
                Card {
                    Text(brief)
                        .lineLimit(briefExpanded || !long ? nil : Self.previewLines)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if long {
                        Button(briefExpanded ? L.text("收起") : L.text("展开全部")) { briefExpanded.toggle() }.buttonStyle(.link)
                    }
                }
            }
        }
    }

    @Local private var briefExpanded = false
    private static let previewLines = 6

    struct Problem { let text, action: String; let tab: AppStore.SettingsTab }

    /// Why the workspace's roles cannot run, grouped by cause: an Agent that has not passed its
    /// test is one problem however many roles use it.
    static func problems(_ roles: [JSON?], connection: (String) -> JSON?) -> [Problem] {
        var out: [Problem] = []
        let deleted = roles.filter { $0 == nil }.count
        if deleted > 0 { out.append(Problem(text: L.text("工作区里有 \(deleted) 个数字员工已被删除，请在工作区设置中移除或换成别的数字员工。"), action: L.text("打开数字员工设置"), tab: .agents)) }
        let unbound = roles.compactMap { $0 }.filter { connection($0["connection"].str) == nil }.map { $0["name"].str }
        if !unbound.isEmpty {
            let names = unbound.joined(separator: L.text("、"))
            out.append(Problem(text: L.text("尚未绑定 Agent 的数字员工：\(names)。"), action: L.text("打开数字员工设置"), tab: .agents))
        }
        var untested: [(String, [String])] = []
        for r in roles.compactMap({ $0 }) {
            guard let c = connection(r["connection"].str), c["phase"].str != "ok" else { continue }
            let name = c["name"].str
            if let i = untested.firstIndex(where: { $0.0 == name }) { untested[i].1.append(r["name"].str) } else { untested.append((name, [r["name"].str])) }
        }
        for (agent, names) in untested {
            let affected = names.joined(separator: L.text("、"))
            out.append(Problem(text: L.text("Agent「\(agent)」还没有测试通过或未登录，以下数字员工无法运行：\(affected)。"), action: L.text("去测试"), tab: .agents))
        }
        return out
    }

    private func start(_ problems: [Problem]) {
        let goal = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        let images = store.goalImages
        guard !goal.isEmpty || !images.isEmpty, problems.isEmpty else { return }
        Task { await store.createRun(goal: goal, workspace: ws["id"].str, images: images) }
    }
}

/// Every task record of a workspace, newest first, read a page at a time as the list scrolls; it
/// follows the run list, which the global stream keeps current. Also the page of a deleted
/// workspace, whose runs stay.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct WorkspaceHistory: View {
    let store: AppStore
    let workspace: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let history = store.histories[workspace], !history.runs.isEmpty {
                Text(L.text("任务记录")).font(.headline)
                RunRows(store: store, runs: history.runs, dated: true) { Task { await history.loadMore() } }
                if history.more { ProgressView().controlSize(.small).frame(maxWidth: .infinity) }
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
                Note(text: L.text("这个工作区已经删除，下面是它留下的任务记录，可以打开查看、继续对话或删除。要在这个目录里提交新目标，新建一个工作区。"))
                WorkspaceHistory(store: store, workspace: ws["id"].str)
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}

/// Runs of a workspace in one card, one row each: the goal, when it started (`dated`), how it stands
/// (「等你处理」 when something waits on the person), a click to open it and a right click to delete it.
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
                let (l, t) = waiting > 0 ? (L.text("等你处理"), Tone.amber) : RunStatus(r["status"].str).listLabel
                Button { store.open(.run(r["id"].str)) } label: {
                    HStack(spacing: 10) {
                        Text(L.goalTitle(r["goal"].str)).lineLimit(1)
                        Spacer()
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
                        .disabled(RunStatus(r["status"].str).isActive)
                }
                .onAppear { if r == runs.last { atEnd?() } }
                if r != runs.last { Divider() }
            }
        }
    }
}

/// New or existing workspace: which directory, which roles, who leads.
struct WorkspaceEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var name = ""
    @Local private var path = ""
    @Local private var lead = ""
    @Local private var on: Set<String> = []
    @Local private var slots: [String: Int] = [:]
    @Local private var brief = ""
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

                    Text(L.text("共享资料")).font(.headline).padding(.top, 6)
                    Text(L.text("产品定位、目标用户、约定、过去的决定、重要文件的位置等长期背景。每次任务开始时交给负责人和所有协作者，已经写在这里的事实不会再被重新调研或再问你。")).font(.callout).foregroundStyle(.secondary)
                    TextEditor(text: $brief)
                        .font(.body)
                        .frame(height: 140)
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(brief.count > Self.briefLimit ? Tone.red.color : Color.line))
                        .accessibilityLabel(L.text("共享资料"))
                    HStack {
                        if let saved = initial?.last, saved != brief, !isNew { Text(L.text("还没保存：保存后，之后的任务才会用到新的共享资料。")).font(.caption).foregroundStyle(Tone.amber.color) }
                        Spacer()
                        Text(verbatim: "\(brief.count) / \(Self.briefLimit)").font(.caption).foregroundStyle(brief.count > Self.briefLimit ? Tone.red.color : .secondary)
                    }

                    Text(L.text("启用的数字员工")).font(.headline).padding(.top, 6)
                    if store.roles.isEmpty { Note(text: L.text("还没有数字员工，先去「设置 → 数字员工」创建。")) }
                    VStack(spacing: 0) {
                        ForEach(store.roles, id: \.self) { r in
                            let id = r["id"].str
                            let c = store.connection(r["connection"].str)
                            HStack(spacing: 10) {
                                Toggle(L.text("启用 \(r["name"].str)"), isOn: Binding(get: { on.contains(id) }, set: { v in if v { on.insert(id) } else { on.remove(id); if lead == id { lead = "" } } }))
                                    .labelsHidden().disabled(c == nil)
                                Avatar(initials: r["init"].str.isEmpty ? String(r["name"].str.prefix(2)).uppercased() : r["init"].str, color: r["color"].str, size: 24, role: r["id"].str)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(r["name"].str).font(.body.weight(.medium))
                                    Text(c.map { "\($0["name"].str) · \(r["model"].truthy ? r["model"].str : L.text("默认模型"))" } ?? L.text("未绑定 Agent")).font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(Setup.permLabel(r["session"], c).isEmpty ? "—" : Setup.permLabel(r["session"], c)).font(.caption).foregroundStyle(.secondary)
                                if on.contains(id) && lead != id {
                                    let count = min(8, max(1, slots[id] ?? 1))
                                    Stepper(value: Binding(get: { count }, set: { slots[id] = min(8, max(1, $0)) }), in: 1...8) {
                                        Text(verbatim: L.text("\(count) 个实例")).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                                    }
                                    .controlSize(.small)
                                    .help(L.text("并发实例数：大于 1 时，这个数字员工可同时执行多个子任务"))
                                }
                                if r["lead"].truthy && Setup.leadCapable(c) {
                                    Button { lead = id } label: {
                                        Label(L.text("负责人"), systemImage: lead == id ? "largecircle.fill.circle" : "circle")
                                    }
                                    .buttonStyle(.borderless)
                                    .disabled(!on.contains(id))
                                    .help(!on.contains(id) ? L.text("先启用这个数字员工") : L.text("设为负责人"))
                                    .accessibilityLabel(L.text("\(r["name"].str) 担任负责人"))
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
                    .disabled(saving || path.trimmingCharacters(in: .whitespaces).isEmpty || inspect?["exists"].bool == false || brief.count > Self.briefLimit)
            }
            .padding(16)
        }
        .frame(minWidth: 600, idealWidth: 640, minHeight: 440, idealHeight: 640)
        .onAppear(perform: setup)
        .task(id: path) {
            try? await Task.sleep(nanoseconds: 250_000_000)
            inspect = path.trimmingCharacters(in: .whitespaces).isEmpty ? nil : try? await store.api.get("/api/inspect?path=\(API.q(path))")
        }
    }

    private var inspectLine: String {
        if path.trimmingCharacters(in: .whitespaces).isEmpty { return L.text("请填写一个目录") }
        guard let i = inspect else { return L.text("检查中…") }
        if i["exists"].bool == false { return L.text("目录不存在") }
        if !i["git"].truthy { return L.text("不是 Git 仓库：任务直接在这个目录中执行，Janus 不会生成改动的 diff") }
        let dirty = i["dirty"].int ?? 0
        return L.text("✓ Git 仓库 · 当前分支 \(i["branch"].str) · \(dirty > 0 ? L.text("⚠ 有 \(dirty) 个未提交改动（不会带进任务）") : L.text("工作区干净"))")
    }

    /// Edits not saved yet, so Cancel and Esc ask before throwing them away.
    private var dirty: Bool { initial.map { $0 != snapshot } ?? false }
    private var snapshot: [String] {
        let slotSpec = on.sorted().map { id in "\(id):\(lead == id ? 1 : min(8, max(1, slots[id] ?? 1)))" }.joined(separator: ",")
        return [name, path, lead, slotSpec, brief]
    }
    /// The backend's `BRIEF_LIMIT`: the shared material goes into every session's prompt.
    static let briefLimit = 8000
    @Local private var initial: [String]?

    private func setup() {
        // Opened with suggested lines already added: those count as unsaved edits.
        defer { initial = Array(snapshot.dropLast()) + [store.workspace(original["id"].str)?["brief"].str ?? original["brief"].str] }
        let isNew = original["id"].str.isEmpty
        name = original["name"].str
        brief = original["brief"].str
        path = original["path"].str
        let rawMembers = original["members"].array
        let members = rawMembers.map { $0["role"].str }
        var loadedSlots: [String: Int] = [:]
        for m in rawMembers {
            loadedSlots[m["role"].str] = min(8, max(1, m["slots"].int ?? 1))
        }
        slots = loadedSlots
        lead = original["lead"].truthy ? original["lead"].str : store.roles.first { $0["lead"].truthy && Setup.leadCapable(store.connection($0["connection"].str)) }?["id"].str ?? ""
        on = Set(isNew ? Self.preset(store.roles, lead: lead) : members)
        if isNew && on.contains(Self.generalist) { slots[Self.generalist] = 3 }
    }

    /// The general digital worker (`GENERALIST` in `backend/src/agents/roles.rs`): with the library open to every worker,
    /// one kind of worker run three at a time handles most subtasks.
    static let generalist = "generalist"

    /// What a new workspace starts with: the lead and three general workers when the general
    /// worker has an Agent; otherwise every role that has one.
    // @project-doc docs/domains/20261031-agents_and_roles.md#product_launch_roles
    static func preset(_ roles: [JSON], lead: String) -> [String] {
        let bound = roles.filter { $0["connection"].truthy }.map { $0["id"].str }
        guard bound.contains(generalist) else { return bound }
        return [lead, generalist].filter { !$0.isEmpty }
    }

    private func pickFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.allowsMultipleSelection = false
        p.prompt = L.text("使用这个文件夹")
        if !path.isEmpty { p.directoryURL = URL(fileURLWithPath: path) }
        if p.runModal() == .OK, let u = p.url { path = u.path }
    }

    private func save() {
        saving = true
        let members = store.roles.map { $0["id"].str }.filter { on.contains($0) }.map { id -> JSON in
            let n = lead == id ? 1 : min(8, max(1, slots[id] ?? 1))
            return n > 1 ? JSON.object(["role": JSON(id), "slots": JSON(n)]) : JSON.object(["role": JSON(id)])
        }
        let body: JSON = ["id": original["id"], "name": JSON(name), "path": JSON(path), "lead": JSON(lead), "members": .array(members), "brief": JSON(brief)]
        Task {
            do {
                let ws = try await store.save("workspaces", body)
                store.show(L.text("已保存工作区 \(ws["name"].str)"))
                dismiss()
                store.open(.home(ws["id"].str))
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func remove() {
        guard confirm(L.text("删除工作区「\(original["name"].str)」？"), L.text("已有的任务记录会保留，在侧栏的 Agent Team 里仍能找到。"), ok: L.text("删除")) else { return }
        Task {
            do {
                try await store.delete("workspaces", original["id"].str)
                dismiss()
                if let w = store.workspaces.first { store.open(.home(w["id"].str)) } else { store.open(.newWorkspace) }
            } catch { store.show(error.localizedDescription) }
        }
    }
}
