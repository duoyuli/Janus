import AppKit
import SwiftUI

/// A workspace: start a goal, see who will work on it, find earlier runs.
struct HomeView: View {
    let store: AppStore
    let ws: JSON

    private let suggestions = [
        ("除零保护", "给 divide 增加除零保护：除数为 0 时抛出 ValueError，并补充对应测试"),
        ("新增函数", "为 calc 增加 subtract 和 multiply 两个函数，补测试，并在 README 中说明用法"),
        ("代码调研", "阅读这个项目，整理代码结构和测试覆盖情况，给出三条改进建议"),
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
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                OnboardingBar(store: store)
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(ws["name"].str).font(.largeTitle.weight(.semibold))
                        Text(ws["path"].str).font(.system(.callout, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Spacer()
                    Button("在 Finder 中显示") { NSWorkspace.shared.open(URL(fileURLWithPath: ws["path"].str)) }
                    Button("工作区设置") { store.editingWorkspace = ws }
                }

                Card {
                    Composer(text: Binding(get: { store.goalDraft }, set: { store.goalDraft = $0 }), placeholder: "说出你想要的结果，例如：给 divide 增加除零保护，并补充测试", minHeight: 96) { start(problems) }
                    HStack {
                        ForEach(suggestions, id: \.0) { s in
                            Button(s.0) { store.goalDraft = s.1 }.buttonStyle(.bordered).controlSize(.small)
                        }
                        Spacer()
                        Text("⌘↩ 发送").font(.caption).foregroundStyle(.secondary)
                        Button("交给主控 →") { start(problems) }
                            .buttonStyle(.borderedProminent)
                            .disabled(!problems.isEmpty)
                    }
                }
                if !problems.isEmpty {
                    HStack {
                        Note(text: "这些角色还不能运行：\(problems.joined(separator: "、"))。", tone: .amber)
                        Button("去设置 →") { store.open(.settings("roles")) }
                    }
                }

                SectionHeader(title: "这个工作区的团队")
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 240), spacing: 10)], spacing: 10) {
                    ForEach(members.indices, id: \.self) { i in
                        let (m, r) = members[i]
                        let c = store.connection(r?["connection"].str ?? "")
                        Card(padding: 12) {
                            HStack(alignment: .top, spacing: 10) {
                                Avatar(initials: r?["init"].str ?? "?", color: r?["color"].str ?? "#64748b", size: 32)
                                VStack(alignment: .leading, spacing: 3) {
                                    HStack(spacing: 6) {
                                        Text(r?["name"].str ?? "已删除").font(.headline)
                                        if m["role"].str == ws["lead"].str { Pill("主控", .violet) }
                                    }
                                    Text(verbatim: "\(c?["name"].str ?? "未绑定") · \(r?["model"].truthy == true ? r!["model"].str : c?["options"]["model"]["current"].str ?? "默认模型")")
                                        .font(.caption).foregroundStyle(.secondary)
                                    let facts = [Setup.permLabel(r?["session"] ?? .null, c), (r?["skills"].array.count ?? 0) > 0 ? "\(r!["skills"].array.count) Skill" : "", (r?["mcp"].array.count ?? 0) > 0 ? "\(r!["mcp"].array.count) MCP" : ""].filter { !$0.isEmpty }
                                    Text(facts.isEmpty ? "Agent 默认权限" : facts.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }

                SectionHeader(title: "任务记录", sub: "\(runs.count) 个")
                Card(padding: 4) {
                    if runs.isEmpty {
                        Text("还没有任务。在上面写下第一个目标。").foregroundStyle(.secondary).padding(12)
                    }
                    ForEach(runs, id: \.self) { r in
                        let (l, t) = L.runListStatus[r["status"].str] ?? (r["status"].str, .gray)
                        Button { store.open(.run(r["id"].str)) } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(L.goalTitle(r["goal"].str)).font(.body.weight(.medium)).lineLimit(1)
                                    Text(L.dateTime(r["createdAt"].double ?? 0)).font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Pill(l, t)
                            }
                            .padding(.horizontal, 10).padding(.vertical, 8)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        if r != runs.last { Divider() }
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 980, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    private func start(_ problems: [String]) {
        let goal = store.goalDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !goal.isEmpty else { store.show("先告诉 Janus 你想完成什么。"); return }
        guard problems.isEmpty else { store.show("还有角色不能运行，先到设置里处理。"); return }
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

                    Text("启用的角色与主控").font(.headline).padding(.top, 6)
                    if store.roles.isEmpty { Note(text: "还没有角色，先去「设置 → 角色」创建。") }
                    VStack(spacing: 0) {
                        ForEach(store.roles, id: \.self) { r in
                            let id = r["id"].str
                            let c = store.connection(r["connection"].str)
                            HStack(spacing: 10) {
                                Toggle("", isOn: Binding(get: { on.contains(id) }, set: { v in if v { on.insert(id) } else { on.remove(id); if lead == id { lead = "" } } }))
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
                                .help(Setup.leadCapable(c) ? "设为主控" : "只有 Claude Code 或 Codex 能做主控")
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
                    Button("删除工作区", role: .destructive) { remove() }
                }
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(isNew ? "创建工作区" : "保存") { save() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(saving)
            }
            .padding(16)
        }
        .frame(width: 640, height: 640)
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

    private func setup() {
        let isNew = original["id"].str.isEmpty
        name = original["name"].str
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
        let body: JSON = ["id": original["id"], "name": JSON(name), "path": JSON(path), "lead": JSON(lead), "members": .array(members)]
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
