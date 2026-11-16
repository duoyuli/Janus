import AppKit
import SwiftUI

/// The resource library (Settings › 资源库): skills and MCP servers kept once and picked per role,
/// and the GitHub sources they can come from. A session gets its role's resources when it starts;
/// MCP values live only in Janus home and are never shown again.
// @project-doc docs/domains/resource_library.md#model
struct LibraryView: View {
    let store: AppStore
    @Local private var tab = "skill"
    @Local private var query = ""
    @Local private var skillDraft: JSON?
    @Local private var mcpDraft: JSON?
    @Local private var paste: String?
    @Local private var link = ""
    @Local private var adding = false
    @Local private var checking = false
    @Local private var picks: [String: Set<String>] = [:]

    var body: some View {
        let items = store.libraryItems
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Picker("类型", selection: $tab) {
                    Text("Skill \(items.filter { $0["kind"].str == "skill" }.count)").tag("skill")
                    Text("MCP \(items.filter { $0["kind"].str == "mcp" }.count)").tag("mcp")
                    Text(store.sourceUpdates.isEmpty ? "来源 \(store.librarySources.count)" : "来源 · \(store.sourceUpdates.count) 个有更新").tag("sources")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 420)
                Spacer()
                if tab != "sources" { TextField("搜索", text: $query).textFieldStyle(.roundedBorder).frame(width: 180) }
            }
            .padding(.horizontal, 20).padding(.top, 18).padding(.bottom, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if store.library == nil { ProgressView() }
                    else {
                        switch tab {
                        case "mcp": servers
                        case "sources": sources
                        default: skills
                        }
                    }
                }
                .padding(.horizontal, 20).padding(.bottom, 20)
            }
        }
        .task { await store.loadLibrary() }
        .sheet(isPresented: Binding(get: { skillDraft != nil }, set: { if !$0 { skillDraft = nil } })) {
            if let d = skillDraft { SkillEditor(store: store, draft: d) }
        }
        .sheet(isPresented: Binding(get: { mcpDraft != nil }, set: { if !$0 { mcpDraft = nil } })) {
            if let d = mcpDraft { McpEditor(store: store, draft: d) }
        }
        .sheet(isPresented: Binding(get: { paste != nil }, set: { if !$0 { paste = nil } })) { pasteSheet }
    }

    private func shown(_ kind: String) -> [JSON] {
        let q = query.lowercased()
        return store.libraryItems.filter { i in
            i["kind"].str == kind && (q.isEmpty || "\(i["name"].str) \(i["description"].str)".lowercased().contains(q))
        }
    }

    // MARK: Skills

    @ViewBuilder private var skills: some View {
        Text("Skill 在会话启动时交给 Agent：Claude Code 作为本地插件加载（显示为 janus:<名字>），Codex 作为额外的 Skill 目录读取（显示为 $<名字>）。不写进任何 Agent 的配置。自定义命令的 Agent 没有这样的入口，用不了 Skill。")
            .font(.callout).foregroundStyle(.secondary)
        HStack {
            Button("新建 Skill") { skillDraft = ["name": "", "description": "", "content": ""] }.buttonStyle(.primary)
            Button("从文件夹复制…") { importFolder() }
            Button("从 GitHub 导入…") { tab = "sources" }
        }
        let list = shown("skill")
        if list.isEmpty { Note(text: store.libraryItems.contains { $0["kind"].str == "skill" } ? "没有匹配的 Skill" : "还没有 Skill。新建一个，或从文件夹、GitHub 导入。") }
        else { rows(list) { skillRow($0) } }
    }

    private func skillRow(_ i: JSON) -> some View {
        row(i, icon: "sparkles", lines: [i["description"].str, Library.origin(i)]) {
            if i["origin"]["type"].str == "created" { Button("编辑") { edit(i) }.buttonStyle(.link) }
            if i["origin"]["type"].str == "folder" { Button("刷新") { refresh(i) }.buttonStyle(.link).help("从原文件夹重新复制") }
        }
    }

    // MARK: MCP servers

    @ViewBuilder private var servers: some View {
        Text("MCP 服务器在会话启动时放进 session/new 的 mcpServers。环境变量和 HTTP header 的值只保存在 Janus home，设置后不再显示，也不会进入数字员工、数据库或 Run 记录。")
            .font(.callout).foregroundStyle(.secondary)
        HStack {
            Button("添加服务器") { mcpDraft = ["name": "", "description": "", "transport": "stdio", "command": "", "url": "", "vars": []] }.buttonStyle(.primary)
            Button("粘贴配置…") { paste = "" }.help("Claude 的 mcpServers、VS Code 的 servers，或一个带 name 的服务器")
            Button("从 GitHub 导入…") { tab = "sources" }
        }
        let list = shown("mcp")
        if list.isEmpty { Note(text: store.libraryItems.contains { $0["kind"].str == "mcp" } ? "没有匹配的服务器" : "还没有 MCP 服务器。添加一个、粘贴配置，或从 GitHub 导入。") }
        else { rows(list) { serverRow($0) } }
    }

    private func serverRow(_ i: JSON) -> some View {
        let vars = Library.vars(i)
        let codex = Library.support(i, runtime: "codex")
        return row(i, icon: "wrench.and.screwdriver", lines: [Library.endpoint(i), i["description"].str, Library.origin(i)], warning: [vars.complete ? "" : vars.text, codex.state == "no" ? codex.why : ""].filter { !$0.isEmpty }.joined(separator: " · "), note: vars.complete ? vars.text : "") {
            Button("编辑") { edit(i) }.buttonStyle(.link)
        }
    }

    private var pasteSheet: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("粘贴 MCP 配置").font(.display(.title3))
            Text("支持 Claude 的 {\"mcpServers\": {…}}、VS Code 的 {\"servers\": {…}}，或一个带 name 的服务器对象。配置中写的值存进 Janus home，名字被占用时自动加后缀。").font(.caption).foregroundStyle(.secondary)
            TextEditor(text: Binding(get: { paste ?? "" }, set: { paste = $0 }))
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 220)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
            HStack {
                Spacer()
                Button("取消") { paste = nil }.keyboardShortcut(.cancelAction)
                Button("导入") { importPasted() }.buttonStyle(.primary).disabled((paste ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(18)
        .frame(width: 560)
    }

    // MARK: Sources

    @ViewBuilder private var sources: some View {
        Text("粘贴 GitHub 仓库或其中一个目录的链接，Janus 拉取仓库，列出其中的 Skill（SKILL.md）和 MCP 服务器（mcp.json / .mcp.json），由你选择导入哪些。只读取，不执行仓库中的任何文件。")
            .font(.callout).foregroundStyle(.secondary)
        HStack {
            TextField("https://github.com/owner/repo 或 …/tree/main/skills", text: $link)
                .textFieldStyle(.roundedBorder)
                .onSubmit(addSource)
                .disabled(adding)
            Button(adding ? "正在拉取…" : "添加来源") { addSource() }
                .buttonStyle(.primary)
                .disabled(adding || link.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        if adding { Note(text: "正在克隆仓库并查找 Skill 和 MCP 服务器，较大的仓库可能需要一分钟。") }
        if !store.librarySources.isEmpty {
            HStack {
                Button(checking ? "正在检查…" : "检查更新") {
                    checking = true
                    Task { await store.checkSources(); checking = false }
                }
                .disabled(checking)
                if store.sourceUpdates.count > 1 { Button("全部更新") { Task { await store.updateSources() } } }
                Spacer()
                Text("服务每 6 小时检查一次").font(.caption).foregroundStyle(.secondary)
            }
        }
        ForEach(store.librarySources, id: \.self) { source($0) }
    }

    private func source(_ s: JSON) -> some View {
        let id = s["id"].str
        let imported = Dictionary(s["imported"].array.map { ($0["path"].str, $0["name"].str) }, uniquingKeysWith: { a, _ in a })
        let chosen = picks[id] ?? []
        return Card {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(s["url"].str).fontWeight(.semibold).textSelection(.enabled)
                    Text("提交 \(String(s["commit"].str.prefix(7))) · \(s["offers"].array.count) 项可导入 · 已导入 \(imported.count) 项").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if store.updatingSources.contains(id) { ProgressView().controlSize(.small) }
                else if s["updateAvailable"].truthy { Pill("有更新", .blue); Button("更新") { Task { await store.updateSources([id]) } } }
                Button("删除…") { removeSource(s) }.buttonStyle(.link)
            }
            if s["checkError"].truthy { Note(text: "检查更新失败：\(s["checkError"].str)", tone: .amber) }
            ForEach(s["offers"].array, id: \.self) { o in
                let path = o["path"].str
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    if let name = imported[path] {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(Tone.green.color).frame(width: 16)
                        offerText(o, extra: name == o["name"].str ? "已导入" : "已导入为 \(name)")
                    } else {
                        Toggle(isOn: Binding(get: { chosen.contains(path) }, set: { on in
                            var set = chosen
                            if on { set.insert(path) } else { set.remove(path) }
                            picks[id] = set
                        })) { offerText(o, extra: "") }
                        .toggleStyle(.checkbox)
                    }
                }
            }
            ForEach(s["warnings"].strings, id: \.self) { w in Text("⚠ \(w)").font(.caption).foregroundStyle(Tone.amber.color) }
            let left = s["offers"].array.map { $0["path"].str }.filter { imported[$0] == nil }
            if !left.isEmpty {
                HStack {
                    Button(chosen.count == left.count ? "全不选" : "全选") { picks[id] = chosen.count == left.count ? [] : Set(left) }.buttonStyle(.link)
                    Spacer()
                    Button("导入所选（\(chosen.count)）") { importOffers(id, Array(chosen)) }.buttonStyle(.primary).disabled(chosen.isEmpty)
                }
            }
        }
    }

    private func offerText(_ o: JSON, extra: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: 6) {
                Image(systemName: o["kind"].str == "skill" ? "sparkles" : "wrench.and.screwdriver").font(.caption).foregroundStyle(.secondary)
                Text(o["name"].str).fontWeight(.medium)
                if !extra.isEmpty { Text(extra).font(.caption).foregroundStyle(.secondary) }
            }
            Text([o["description"].str, o["path"].str].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(2)
        }
    }

    // MARK: Rows

    private func rows(_ list: [JSON], @ViewBuilder _ content: @escaping (JSON) -> some View) -> some View {
        VStack(spacing: 0) {
            ForEach(list, id: \.self) { i in content(i); Divider() }
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
    }

    private func row(_ i: JSON, icon: String, lines: [String], warning: String = "", note: String = "", @ViewBuilder actions: () -> some View) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).foregroundStyle(.secondary).frame(width: 18).padding(.top, 2)
            VStack(alignment: .leading, spacing: 3) {
                Text(i["name"].str).font(.system(.body, design: .monospaced).weight(.medium)).textSelection(.enabled)
                ForEach(lines.filter { !$0.isEmpty }, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                HStack(spacing: 8) {
                    Text(Library.usedBy(i)).font(.caption).foregroundStyle(.tertiary)
                    if !note.isEmpty { Text(note).font(.caption).foregroundStyle(.tertiary) }
                    if !warning.isEmpty { Text(warning).font(.caption).foregroundStyle(Tone.amber.color) }
                }
            }
            Spacer()
            HStack(spacing: 10) {
                actions()
                Button("删除") { remove(i) }.buttonStyle(.link)
            }
        }
        .padding(12)
    }

    // MARK: Actions

    private func edit(_ i: JSON) {
        Task {
            do {
                let full = try await store.api.get("/api/library/items/\(i["id"].str)")
                if full["kind"].str == "skill" {
                    skillDraft = ["id": full["id"], "name": full["name"], "content": full["content"]]
                } else {
                    let s = full["server"]
                    let command = ([s["command"].str] + s["args"].strings).joined(separator: " ")
                    let vars: [JSON] = full["vars"].array.map { ["name": $0["name"], "value": "", "set": $0["set"]] }
                    mcpDraft = ["id": full["id"], "name": full["name"], "description": full["description"], "transport": s["transport"], "command": JSON(command), "url": s["url"], "vars": .array(vars)]
                }
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func refresh(_ i: JSON) {
        Task {
            do {
                let r = try await store.api.post("/api/library/items/\(i["id"].str)/refresh")
                await store.loadLibrary()
                store.show(r["revision"].str == i["revision"].str ? "\(i["name"].str) 没有变化" : "已从文件夹刷新 \(i["name"].str)，新的 Run 使用新内容")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func remove(_ i: JSON) {
        guard confirm("删除 \(i["name"].str)？", "已经开始的 Run 有自己的副本，不受影响。MCP 服务器的值会一起删除。", ok: "删除") else { return }
        Task {
            do { try await store.api.delete("/api/library/items/\(i["id"].str)"); await store.loadLibrary() }
            catch { store.show(error.localizedDescription) }
        }
    }

    private func importFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.prompt = "复制到资源库"
        p.message = "选择包含 SKILL.md 的文件夹"
        guard p.runModal() == .OK, let u = p.url else { return }
        Task {
            do {
                let item = try await store.api.post("/api/library/skills/import", ["path": JSON(u.path)])
                await store.loadLibrary()
                store.show("已复制 Skill \(item["name"].str)")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importPasted() {
        let text = paste ?? ""
        Task {
            do {
                let r = try await store.api.post("/api/library/mcp/import", ["text": JSON(text)])
                paste = nil
                await store.loadLibrary()
                let names = r["items"].array.map { $0["name"].str }.joined(separator: "、")
                store.show(r["warnings"].strings.isEmpty ? "已添加 \(names)" : "已添加 \(names)；跳过：\(r["warnings"].strings.joined(separator: "；"))")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func addSource() {
        let url = link.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty, !adding else { return }
        adding = true
        Task {
            defer { adding = false }
            do {
                let s = try await store.api.post("/api/library/sources", ["url": JSON(url)], timeout: 240)
                link = ""
                picks[s["id"].str] = Set(s["offers"].array.map { $0["path"].str })
                await store.loadLibrary()
                store.show("找到 \(s["offers"].array.count) 项，选择要导入的")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importOffers(_ id: String, _ paths: [String]) {
        Task {
            do {
                let r = try await store.api.post("/api/library/sources/\(id)/import", ["paths": JSON(paths)])
                picks[id] = []
                await store.loadLibrary()
                store.show("已导入 \(r.array.map { $0["name"].str }.joined(separator: "、"))")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func removeSource(_ s: JSON) {
        guard confirm("删除来源 \(s["url"].str)？", "本机的仓库副本会删除。还有从它导入的资源时不能删除。", ok: "删除") else { return }
        Task {
            do { try await store.api.delete("/api/library/sources/\(s["id"].str)"); await store.loadLibrary() }
            catch { store.show(error.localizedDescription) }
        }
    }
}

/// A skill written in Janus: name, description and body when new; the whole SKILL.md when edited.
struct SkillEditor: View {
    let store: AppStore
    let draft: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var d: JSON = [:]
    @Local private var saving = false

    var body: some View {
        let editing = d["id"].truthy
        VStack(alignment: .leading, spacing: 10) {
            Text(editing ? "编辑 Skill" : "新建 Skill").font(.display(.title3))
            TextField("名称：小写字母、数字和 -，例如 release-notes", text: field("name")).textFieldStyle(.roundedBorder)
            if !editing {
                TextField("什么时候用，例如：写发布说明时使用", text: field("description")).textFieldStyle(.roundedBorder)
            }
            Text(editing ? "SKILL.md（front matter 中的 name 以上面的名称为准）" : "正文：Agent 使用这个 Skill 时读到的说明").font(.caption).foregroundStyle(.secondary)
            TextEditor(text: field("content"))
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 260)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
            HStack {
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(saving ? "正在保存…" : "保存") { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(saving)
            }
        }
        .padding(18)
        .frame(width: 620)
        .onAppear { d = draft }
    }

    private func field(_ k: String) -> Binding<String> { Binding(get: { d[k].str }, set: { d[k] = JSON($0) }) }

    private func save() {
        saving = true
        Task {
            defer { saving = false }
            do {
                let body: JSON = ["name": d["name"], "description": d["description"], "content": d["content"]]
                let item = d["id"].truthy
                    ? try await store.api.put("/api/library/skills/\(d["id"].str)", body)
                    : try await store.api.post("/api/library/skills", body)
                await store.loadLibrary()
                store.show("已保存 Skill \(item["name"].str)")
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}

/// An MCP server: how to start or reach it, and the names of its variables (stdio) or headers
/// (http, sse). A value left empty keeps the one already saved; saved values are never shown.
struct McpEditor: View {
    let store: AppStore
    let draft: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var d: JSON = [:]
    @Local private var saving = false

    var body: some View {
        let stdio = d["transport"].str == "stdio"
        VStack(alignment: .leading, spacing: 10) {
            Text(d["id"].truthy ? "编辑 MCP 服务器" : "添加 MCP 服务器").font(.display(.title3))
            HStack {
                TextField("名称，例如 github", text: field("name")).textFieldStyle(.roundedBorder).frame(width: 200)
                Picker("传输方式", selection: field("transport")) {
                    Text("stdio（命令）").tag("stdio")
                    Text("HTTP").tag("http")
                    Text("SSE").tag("sse")
                }
                .labelsHidden()
                .frame(width: 150)
            }
            TextField("说明（可选）", text: field("description")).textFieldStyle(.roundedBorder)
            if stdio {
                TextField("启动命令，例如 npx -y @modelcontextprotocol/server-github", text: field("command"))
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
            } else {
                TextField("地址，例如 https://example.com/mcp", text: field("url"))
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                if d["transport"].str == "sse" { Text("Codex 不支持 SSE 传输，绑定 Codex 的数字员工选择它时会被跳过。").font(.caption).foregroundStyle(Tone.amber.color) }
            }
            HStack {
                Text(stdio ? "环境变量" : "HTTP header").font(.callout.weight(.semibold))
                Spacer()
                Button("添加") { d["vars"] = .array(d["vars"].array + [["name": "", "value": "", "set": false]]) }.buttonStyle(.link)
            }
            ForEach(d["vars"].array.indices, id: \.self) { i in
                HStack {
                    TextField(stdio ? "名称，例如 GITHUB_TOKEN" : "名称，例如 Authorization", text: varField(i, "name"))
                        .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)).frame(width: 220)
                    SecureField(d["vars"][i]["set"].truthy ? "已设置，留空保留" : "值", text: varField(i, "value")).textFieldStyle(.roundedBorder)
                    Button { var v = d["vars"].array; v.remove(at: i); d["vars"] = .array(v) } label: { Image(systemName: "minus.circle") }
                        .buttonStyle(.borderless).help("移除这个变量和它的值")
                }
            }
            Text("值只保存在 Janus home（0600），会话启动时交给 Agent，不会再显示。").font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(saving ? "正在保存…" : "保存") { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(saving)
            }
        }
        .padding(18)
        .frame(width: 620)
        .onAppear { d = draft }
    }

    private func field(_ k: String) -> Binding<String> { Binding(get: { d[k].str }, set: { d[k] = JSON($0) }) }

    private func varField(_ i: Int, _ k: String) -> Binding<String> {
        Binding(get: { d["vars"][i][k].str }, set: { v in
            var list = d["vars"].array
            guard i < list.count else { return }
            list[i][k] = JSON(v)
            d["vars"] = .array(list)
        })
    }

    private func save() {
        saving = true
        Task {
            defer { saving = false }
            do {
                let vars: [JSON] = d["vars"].array.filter { !$0["name"].str.isEmpty }.map { ["name": $0["name"], "value": $0["value"]] }
                let body: JSON = ["name": d["name"], "description": d["description"], "transport": d["transport"], "command": d["command"], "url": d["url"], "vars": .array(vars)]
                let item = d["id"].truthy
                    ? try await store.api.put("/api/library/mcp/\(d["id"].str)", body)
                    : try await store.api.post("/api/library/mcp", body)
                await store.loadLibrary()
                store.show("已保存 \(item["name"].str)")
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}
