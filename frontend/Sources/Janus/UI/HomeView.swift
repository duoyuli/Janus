import AppKit
import SwiftUI

/// A workspace: start a goal; the team in one line and only the runs that wait on the person.
struct HomeView: View {
    let store: AppStore
    let ws: JSON

    private let suggestions = [
        ("新功能上线", "为这个产品规划并上线一个新功能：先确认目标用户和成功标准，写 PRD，实现并测试，准备发布说明"),
        ("竞品与定位", "调研这个产品的主要竞品和目标用户，整理定位差异和三个最值得做的机会，给出依据"),
        ("发布前检查", "检查这个产品是否可以发布：核对主要用户流程、已知缺陷、回滚方式和发布说明，给出质量结论"),
    ]

    var body: some View {
        let members = ws["members"].array.map { m in (m, store.role(m["role"].str)) }
        let problems = members.compactMap { m, r -> String? in
            guard let r else { return "已删除的角色" }
            let c = store.connection(r["connection"].str)
            if c == nil { return "\(r["name"].str)（未绑定 Agent）" }
            if c?["status"].str != "ok" { return "\(r["name"].str)（Agent 未测试通过）" }
            return nil
        }
        let runs = store.runs(of: ws)
        // Runs waiting on the person; the full list is in the sidebar.
        let waiting = runs.filter { ["waiting_input", "delivered"].contains($0["status"].str) }
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                OnboardingBar(store: store)
                HStack(alignment: .firstTextBaseline) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(ws["name"].str).font(.largeTitle.weight(.semibold))
                        Text(L.tilde(ws["path"].str)).font(.callout).foregroundStyle(.secondary).help(ws["path"].str).textSelection(.enabled)
                    }
                    Spacer()
                    Menu {
                        Button("在 Finder 中显示") { NSWorkspace.shared.open(URL(fileURLWithPath: ws["path"].str)) }
                        Button("工作区设置…") { store.editingWorkspace = ws }
                    } label: { Label("工作区操作", systemImage: "ellipsis.circle") }
                    .labelStyle(.iconOnly).menuStyle(.borderlessButton).fixedSize().help("工作区操作")
                }

                VStack(alignment: .leading, spacing: 10) {
                    Composer(text: Binding(get: { store.goalDraft }, set: { store.goalDraft = $0 }), placeholder: "说出你想要的结果，⌘↩ 交给主控", minHeight: 96) { start(problems) }
                    HStack {
                        // Examples only help before the first run.
                        if runs.isEmpty {
                            ForEach(suggestions, id: \.0) { s in
                                Button(s.0) { store.goalDraft = s.1 }.buttonStyle(.bordered).controlSize(.small).help(s.1)
                            }
                        }
                        Spacer()
                        let empty = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        Button("交给主控") { start(problems) }
                            .buttonStyle(.borderedProminent)
                            .disabled(!problems.isEmpty || empty)
                            .help(!problems.isEmpty ? "还有角色不能运行" : empty ? "先写下你想要的结果" : "⌘↩")
                    }
                }
                if !problems.isEmpty {
                    HStack {
                        Note(text: "这些角色还不能运行：\(problems.joined(separator: "、"))。", tone: .amber)
                        Button("打开角色设置") { store.openSettings("roles") }
                    }
                }

                team(members)

                sharedMaterial

                if !waiting.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("等你处理").font(.headline)
                        Card(padding: 4) {
                            ForEach(waiting, id: \.self) { r in
                                let (l, t) = L.runListStatus[r["status"].str] ?? (r["status"].str, .gray)
                                Button { store.open(.run(r["id"].str)) } label: {
                                    HStack {
                                        Text(L.goalTitle(r["goal"].str)).lineLimit(1)
                                        Spacer()
                                        Pill(l, t)
                                    }
                                    .padding(.horizontal, 10).padding(.vertical, 8)
                                    .contentShape(Rectangle())
                                    .hoverable(6)
                                }
                                .buttonStyle(.plain)
                                if r != waiting.last { Divider() }
                            }
                        }
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    /// The team in one line; models, permissions and tools are in the workspace settings.
    private func team(_ members: [(JSON, JSON?)]) -> some View {
        let lead = store.role(ws["lead"].str)
        return HStack(spacing: 10) {
            Text("团队").foregroundStyle(.secondary)
            HStack(spacing: -4) {
                ForEach(members.indices, id: \.self) { i in
                    let r = members[i].1
                    Avatar(initials: r?["init"].str ?? "?", color: r?["color"].str ?? "#64748b", size: 22)
                        .overlay(RoundedRectangle(cornerRadius: 22 * 0.28).strokeBorder(Color(nsColor: .windowBackgroundColor), lineWidth: 1.5))
                        .help(r?["name"].str ?? "已删除的角色")
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(members.compactMap { $0.1?["name"].str }.joined(separator: "、"))
            Text(verbatim: "\(members.count) 个角色\(lead.map { " · 主控 \($0["name"].str)" } ?? "")").font(.callout).foregroundStyle(.secondary)
            Spacer()
            Button("管理团队…") { store.editingWorkspace = ws }.buttonStyle(.link)
        }
    }

    /// The workspace's shared material, readable where goals start; editing stays in the workspace settings.
    // @project-doc docs/domains/agents_and_roles.md#workspace_brief
    @ViewBuilder private var sharedMaterial: some View {
        let brief = ws["brief"].str.trimmingCharacters(in: .whitespacesAndNewlines)
        let long = brief.split(separator: "\n", omittingEmptySubsequences: false).count > Self.previewLines || brief.count > 400
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text("共享资料").font(.headline)
                Text("每个 Run 开始时交给主控和协作者").font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button(brief.isEmpty ? "添加…" : "编辑…") { store.editingWorkspace = ws }.buttonStyle(.link)
            }
            if brief.isEmpty {
                Text("还没有共享资料。写下产品定位、目标用户、约定和重要文件的位置，之后的 Run 不必重新调研或再问你。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            } else {
                Card {
                    Text(brief)
                        .lineLimit(briefExpanded || !long ? nil : Self.previewLines)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if long {
                        Button(briefExpanded ? "收起" : "展开全部") { briefExpanded.toggle() }.buttonStyle(.link)
                    }
                }
            }
        }
    }

    @Local private var briefExpanded = false
    private static let previewLines = 6

    private func start(_ problems: [String]) {
        let goal = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !goal.isEmpty, problems.isEmpty else { return }
        Task { await store.createRun(goal: goal, workspace: ws["id"].str) }
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
    @Local private var brief = ""
    @Local private var inspect: JSON?
    @Local private var saving = false

    var body: some View {
        let isNew = original["id"].str.isEmpty
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(isNew ? "新建工作区" : "工作区设置 · \(original["name"].str)").font(.title2.weight(.semibold))
                Spacer()
            }
            .padding(20)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text("项目目录").font(.headline)
                    HStack {
                        TextField("/path/to/your/repo", text: $path).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                        Button("选择文件夹…") { pickFolder() }
                    }
                    if let def = store.meta["defaultCwd"].string, !def.isEmpty {
                        Button("示例项目 examples/calc") { path = def }.buttonStyle(.link)
                    }
                    Text(inspectLine).font(.callout).foregroundStyle(inspect?["exists"].bool == false ? Tone.red.color : .secondary)
                    TextField("名称（可选，默认用目录名）", text: $name).textFieldStyle(.roundedBorder)

                    Text("共享资料").font(.headline).padding(.top, 6)
                    Text("产品定位、目标用户、约定、过去的决定、重要文件的位置等长期背景。每次 Run 开始时交给主控和所有协作者，已经写在这里的事实不会再被重新调研或再问你。").font(.callout).foregroundStyle(.secondary)
                    TextEditor(text: $brief)
                        .font(.body)
                        .frame(height: 140)
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(brief.count > Self.briefLimit ? Tone.red.color : Color.line))
                        .accessibilityLabel("共享资料")
                    HStack {
                        if let saved = initial?.last, saved != brief, !isNew { Text("还没保存：保存后，之后的 Run 才会用到新的共享资料。").font(.caption).foregroundStyle(Tone.amber.color) }
                        Spacer()
                        Text(verbatim: "\(brief.count) / \(Self.briefLimit)").font(.caption).foregroundStyle(brief.count > Self.briefLimit ? Tone.red.color : .secondary)
                    }

                    Text("启用的角色与主控").font(.headline).padding(.top, 6)
                    if store.roles.isEmpty { Note(text: "还没有角色，先去「设置 → 角色」创建。") }
                    VStack(spacing: 0) {
                        ForEach(store.roles, id: \.self) { r in
                            let id = r["id"].str
                            let c = store.connection(r["connection"].str)
                            HStack(spacing: 10) {
                                Toggle("启用 \(r["name"].str)", isOn: Binding(get: { on.contains(id) }, set: { v in if v { on.insert(id) } else { on.remove(id); if lead == id { lead = "" } } }))
                                    .labelsHidden().disabled(c == nil)
                                Avatar(initials: r["init"].str.isEmpty ? String(r["name"].str.prefix(2)).uppercased() : r["init"].str, color: r["color"].str, size: 24)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(r["name"].str).font(.body.weight(.medium))
                                    Text(c.map { "\($0["name"].str) · \(r["model"].truthy ? r["model"].str : "默认模型")" } ?? "未绑定 Agent").font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(Setup.permLabel(r["session"], c).isEmpty ? "—" : Setup.permLabel(r["session"], c)).font(.caption).foregroundStyle(.secondary)
                                Button { lead = id } label: {
                                    Label("主控", systemImage: lead == id ? "largecircle.fill.circle" : "circle")
                                }
                                .buttonStyle(.borderless)
                                .disabled(!on.contains(id) || !Setup.leadCapable(c))
                                .help(!Setup.leadCapable(c) ? "只有 Claude Code 或 Codex 能做主控" : !on.contains(id) ? "先启用这个角色" : "设为主控")
                                .accessibilityLabel("\(r["name"].str) 担任主控")
                                .accessibilityAddTraits(lead == id ? .isSelected : [])
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
                    Button("删除工作区…", role: .destructive) { remove() }
                }
                Spacer()
                Button("取消") { if !dirty || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                Button(saving ? "正在保存…" : isNew ? "创建工作区" : "保存") { save() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
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
        if path.trimmingCharacters(in: .whitespaces).isEmpty { return "请填写一个目录" }
        guard let i = inspect else { return "检查中…" }
        if i["exists"].bool == false { return "目录不存在" }
        if !i["git"].truthy { return "不是 Git 仓库：任务直接在这个目录中执行，Janus 不会生成改动的 diff" }
        let dirty = i["dirty"].int ?? 0
        return "✓ Git 仓库 · 当前分支 \(i["branch"].str) · \(dirty > 0 ? "⚠ 有 \(dirty) 个未提交改动（任务会直接在这些改动之上工作）" : "工作区干净")"
    }

    /// Edits not saved yet, so Cancel and Esc ask before throwing them away.
    private var dirty: Bool { initial.map { $0 != snapshot } ?? false }
    private var snapshot: [String] { [name, path, lead, on.sorted().joined(separator: ","), brief] }
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
        if isNew && store.workspaces.isEmpty, let d = store.meta["defaultCwd"].string { path = d }
        let members = original["members"].array.map { $0["role"].str }
        on = Set(isNew ? store.roles.filter { $0["connection"].truthy }.map { $0["id"].str } : members)
        lead = original["lead"].truthy ? original["lead"].str : store.roles.first { $0["lead"].truthy && Setup.leadCapable(store.connection($0["connection"].str)) }?["id"].str ?? ""
    }

    private func pickFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.allowsMultipleSelection = false
        p.prompt = "使用这个文件夹"
        if !path.isEmpty { p.directoryURL = URL(fileURLWithPath: path) }
        if p.runModal() == .OK, let u = p.url { path = u.path }
    }

    private func save() {
        saving = true
        let members = store.roles.map { $0["id"].str }.filter { on.contains($0) }.map { JSON.object(["role": JSON($0)]) }
        let body: JSON = ["id": original["id"], "name": JSON(name), "path": JSON(path), "lead": JSON(lead), "members": .array(members), "brief": JSON(brief)]
        Task {
            do {
                let ws = try await store.save("workspaces", body)
                store.show("已保存工作区 \(ws["name"].str)")
                dismiss()
                store.open(.home(ws["id"].str))
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func remove() {
        guard confirm("删除工作区「\(original["name"].str)」？", "已有的任务记录会保留。", ok: "删除") else { return }
        Task {
            do {
                try await store.delete("workspaces", original["id"].str)
                dismiss()
                if let w = store.workspaces.first { store.open(.home(w["id"].str)) } else { store.open(.newWorkspace) }
            } catch { store.show(error.localizedDescription) }
        }
    }
}
