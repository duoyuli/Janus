import AppKit
import SwiftUI

/// The MCP library: GitHub repositories added by link, whose `mcp.json` / `.mcp.json` server
/// definitions Janus finds automatically. Roles mount individual servers from here, and their
/// environment values and HTTP headers live only in Janus home.
// @project-doc docs/domains/agents_and_roles.md#mcp_library
struct McpView: View {
    let store: AppStore
    @Local private var link = ""
    @Local private var adding = false
    @Local private var checking = false
    @Local private var open: Set<String> = []
    @Local private var query = ""
    @Local private var editingSecret: JSON?
    @Local private var editingSource = ""

    var body: some View {
        let sources = store.mcpSources ?? []
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("粘贴 GitHub 仓库链接，Janus 会拉取仓库并读取其中的 mcp.json / .mcp.json（Claude 和 VS Code 的写法都支持）。之后在角色里选择，角色启动的 Agent 就会带上这些 MCP 服务器。环境变量和 HTTP header 的值只保存在 Janus home 的 secrets 中，不会出现在设置、数据库或 Run 记录里。")
                    .font(.callout).foregroundStyle(.secondary)
                HStack {
                    TextField("https://github.com/owner/repo 或 …/tree/main/servers", text: $link)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit(add)
                        .disabled(adding)
                    Button(adding ? "正在拉取…" : "导入") { add() }
                        .buttonStyle(.borderedProminent)
                        .disabled(adding || link.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if adding { Note(text: "正在克隆仓库并查找 MCP 定义，较大的仓库可能需要一分钟。") }
                if !sources.isEmpty { updateBar }
                if store.mcpSources == nil { ProgressView() }
                else if sources.isEmpty { Note(text: "还没有 MCP 来源。导入一个仓库；角色的手工 MCP 不受影响，仍可在角色编辑器里添加。") }
                if !sources.isEmpty {
                    TextField("搜索 MCP", text: $query).textFieldStyle(.roundedBorder)
                }
                ForEach(sources, id: \.self) { s in source(s) }
            }
            .padding(20)
        }
        .task { await store.loadMcpSources() }
        .sheet(isPresented: Binding(get: { editingSecret != nil }, set: { if !$0 { editingSecret = nil } })) {
            if let server = editingSecret {
                McpSecretsEditor(store: store, sourceId: editingSource, server: server)
            }
        }
    }

    // MARK: Sources

    private func source(_ s: JSON) -> some View {
        let id = s["id"].str
        let busy = store.updatingMcpSources.contains(id)
        let servers = filtered(s["servers"].array)
        let expanded = open.contains(id) || !query.isEmpty
        let where_ = [s["ref"].truthy ? s["ref"].str : "", s["path"].str].filter { !$0.isEmpty }.joined(separator: " · ")
        return Card {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "shippingbox").foregroundStyle(Color.accent).padding(.top, 2)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Button(s["repo"].str) { if let u = URL(string: s["url"].str) { NSWorkspace.shared.open(u) } }
                            .buttonStyle(.link).fontWeight(.semibold).help(s["url"].str)
                        Pill("\(s["servers"].array.count) MCP")
                        // A failed check wins: it also clears `updateAvailable`, but an old reply
                        // must never hide why the last check did not finish.
                        if s["checkError"].truthy { Pill("检查失败", .amber).help(s["checkError"].str) }
                        else if s["updateAvailable"].truthy { Pill("有更新", .blue).help("GitHub 上的最新提交是 \(String(s["latest"].str.prefix(7)))") }
                    }
                    Text(([where_, s["commit"].truthy ? String(s["commit"].str.prefix(7)) : "", s["updatedAt"].truthy ? "\(L.dateTime(s["updatedAt"].double ?? 0)) 拉取" : "", s["checkedAt"].truthy ? "\(L.dateTime(s["checkedAt"].double ?? 0)) 检查" : ""].filter { !$0.isEmpty }).joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary)
                    if let show = s["warnings"].array.first, show["error"].truthy {
                        Text("部分定义未导入：\(show["manifest"].str) \(show["error"].str)").font(.caption).foregroundStyle(Tone.amber.color).lineLimit(2)
                    }
                }
                Spacer()
                if busy { ProgressView().controlSize(.small) }
                if s["updateAvailable"].truthy {
                    Button("更新") { update(id) }.buttonStyle(.borderedProminent).disabled(busy).help("拉取最新定义；已保存的凭据保留")
                } else {
                    Button("重新拉取") { update(id) }.disabled(busy).help("重新拉取仓库")
                }
                Button { remove(s) } label: { Label("删除来源", systemImage: "trash") }
                    .labelStyle(.iconOnly).buttonStyle(.borderless).disabled(busy).help("删除来源")
            }
            Button {
                if open.contains(id) { open.remove(id) } else { open.insert(id) }
            } label: {
                Label(expanded ? "收起" : "查看 MCP", systemImage: expanded ? "chevron.down" : "chevron.right").font(.callout)
            }
            .buttonStyle(.plain).foregroundStyle(.secondary)
            if expanded {
                if servers.isEmpty { Text("没有匹配的 MCP").font(.callout).foregroundStyle(.secondary) }
                ForEach(servers, id: \.self) { server in serverRow(server, sourceId: id) }
            }
        }
    }

    private func serverRow(_ s: JSON, sourceId: String) -> some View {
        let key = s["key"].str
        let name = s["title"].truthy ? s["title"].str : key
        let http = s["transport"].str == "http"
        let what = http ? s["url"].str : ([s["command"].str] + s["args"].strings).joined(separator: " ")
        let envKeys = s["envKeys"].strings
        let headerKeys = s["headerKeys"].strings
        let envSet = s["envSet"].strings
        let headerSet = s["headerSet"].strings
        let missing = envKeys.count - envSet.count + headerKeys.count - headerSet.count
        let users = store.roles.filter { $0["mcpRefs"].strings.contains(s["id"].str) }.map { $0["name"].str }
        let creds = [envKeys.isEmpty ? "" : "环境变量 \(envSet.count)/\(envKeys.count)",
                     headerKeys.isEmpty ? "" : "Header \(headerSet.count)/\(headerKeys.count)"].filter { !$0.isEmpty }.joined(separator: " · ")
        return HStack(alignment: .top, spacing: 8) {
            Image(systemName: http ? "network" : "terminal").foregroundStyle(.secondary).padding(.top, 2).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(name).fontWeight(.medium)
                    Pill(http ? "HTTP" : "stdio", http ? .blue : .gray)
                    if name != key { Text(key).font(.system(.caption, design: .monospaced)).foregroundStyle(.tertiary) }
                    if !users.isEmpty { Pill(users.joined(separator: "、"), .violet).help("挂载这个 MCP 的角色") }
                }
                if s["description"].truthy { Text(s["description"].str).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                Text(what).font(.system(.caption2, design: .monospaced)).foregroundStyle(.tertiary).lineLimit(1)
                HStack(spacing: 6) {
                    Text(s["manifest"].str).font(.system(.caption2, design: .monospaced)).foregroundStyle(.tertiary)
                    if !creds.isEmpty {
                        Text(creds).font(.caption2)
                            .foregroundStyle(missing > 0 ? Tone.amber.color : Tone.green.color)
                            .help(missing > 0 ? "有 \(missing) 个值还没有设置，启动会话时不会传给 Agent" : "所有声明的值都已设置")
                    }
                }
            }
            Spacer()
            if !envKeys.isEmpty || !headerKeys.isEmpty {
                Button("凭据…") { editingSource = sourceId; editingSecret = s }
                    .controlSize(.small)
                    .help("设置环境变量和 HTTP header 的值；值只写入 Janus home 的 secrets")
            }
        }
        .padding(.vertical, 4)
    }

    /// Reminder and one-click update for every source with news, or a manual check.
    private var updateBar: some View {
        let updates = store.mcpSourceUpdates
        let busy = !store.updatingMcpSources.isEmpty
        return HStack(spacing: 10) {
            Image(systemName: updates.isEmpty ? "checkmark.circle" : "arrow.down.circle.fill")
                .foregroundStyle(updates.isEmpty ? Color.secondary : Tone.blue.color)
            Text(updates.isEmpty ? "Janus 每 6 小时检查一次 GitHub 上的更新。" : "\(updates.map { $0["repo"].str }.joined(separator: "、")) 有新的提交。")
                .font(.callout).foregroundStyle(updates.isEmpty ? .secondary : .primary)
            Spacer()
            if checking || busy { ProgressView().controlSize(.small) }
            Button(checking ? "正在检查…" : "检查更新") {
                checking = true
                Task { await store.checkMcpSources(); checking = false }
            }
            .disabled(checking || busy)
            if !updates.isEmpty {
                Button(updates.count == 1 ? "更新" : "全部更新（\(updates.count)）") { Task { await store.updateMcpSources() } }
                    .buttonStyle(.borderedProminent).disabled(busy)
            }
        }
        .padding(10)
        .background((updates.isEmpty ? Color.secondary : Tone.blue.color).opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }

    private func filtered(_ list: [JSON]) -> [JSON] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? list : list.filter { "\($0["title"].str) \($0["description"].str) \($0["manifest"].str)".lowercased().contains(q) }
    }

    // MARK: Actions

    private func reload() async {
        async let a: Void = store.loadMcpSources()
        async let b: Void = store.loadSetup()
        _ = await (a, b)
    }

    private func add() {
        let url = link.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty, !adding else { return }
        adding = true
        Task {
            defer { adding = false }
            do {
                let s = try await store.api.post("/api/mcp-sources", ["url": JSON(url)], timeout: 240)
                link = ""
                open.insert(s["id"].str)
                await reload()
                store.show("已导入 \(s["repo"].str)：找到 \(s["servers"].array.count) 个 MCP")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func update(_ id: String) {
        Task { await store.updateMcpSources([id]) }
    }

    private func remove(_ s: JSON) {
        guard confirm("删除来源 \(s["repo"].str)？", "本机的仓库副本和这个来源的凭据会删除。仍挂载其中 MCP 的角色，或还在运行的 Run，会阻止删除；已经交付的 Run 不受影响。", ok: "删除") else { return }
        Task {
            do {
                try await store.api.delete("/api/mcp-sources/\(s["id"].str)")
                await reload()
            } catch { store.show(error.localizedDescription) }
        }
    }
}

/// Which credential slots the editor shows as set. It starts from the library entry the sheet was
/// opened with, then follows each successful save locally. The entry it started from is stale
/// after a save, so it is never consulted again: a cleared slot cannot fall back to it.
struct McpSecretState: Equatable {
    private(set) var set: Set<String> = []

    init(envSet: [String] = [], headerSet: [String] = []) {
        set.formUnion(envSet.map { Self.slot("env", $0) })
        set.formUnion(headerSet.map { Self.slot("headers", $0) })
    }

    static func slot(_ field: String, _ key: String) -> String { "\(field):\(key)" }

    func isSet(_ field: String, _ key: String) -> Bool {
        set.contains(Self.slot(field, key))
    }

    mutating func markSaved(_ slots: Set<String>) { set.formUnion(slots) }
    mutating func markCleared(_ slots: Set<String>) { set.subtract(slots) }
}

/// Sets and clears the values one library MCP declares. Values are write-only: the library only
/// reports which keys are set, so every field starts empty.
struct McpSecretsEditor: View {
    let store: AppStore
    let sourceId: String
    let server: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var values: [String: String] = [:]
    @Local private var secretState = McpSecretState()
    @Local private var saving = false

    private var name: String { server["title"].truthy ? server["title"].str : server["key"].str }
    private var envKeys: [String] { server["envKeys"].strings }
    private var headerKeys: [String] { server["headerKeys"].strings }
    private var path: String { "/api/mcp-sources/\(sourceId)/servers/\(server["key"].str)/secrets" }
    private func slot(_ field: String, _ key: String) -> String { McpSecretState.slot(field, key) }
    private func isSaved(_ field: String, _ key: String) -> Bool { secretState.isSet(field, key) }
    private func value(_ field: String, _ key: String) -> Binding<String> {
        let id = slot(field, key)
        return Binding(get: { values[id] ?? "" }, set: { values[id] = $0 })
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("MCP 凭据 · \(name)").font(.title3.weight(.semibold))
                    Text("值只写入 Janus home 的 secrets，接口和 Run 记录只看到变量名。留空表示不修改。")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(16)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if !envKeys.isEmpty {
                        Text("环境变量").font(.headline)
                        ForEach(envKeys, id: \.self) { key in row("env", key) }
                    }
                    if !headerKeys.isEmpty {
                        Text("HTTP Header").font(.headline)
                        ForEach(headerKeys, id: \.self) { key in row("headers", key) }
                    }
                    Text("清单里的默认值在导入时写入 secrets；这里保存的值优先，更新来源不会覆盖。").font(.caption).foregroundStyle(.secondary)
                }
                .padding(16)
            }
            Divider()
            HStack {
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(saving ? "正在保存…" : "保存") { save() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(saving)
            }
            .padding(14)
        }
        .frame(minWidth: 520, idealWidth: 560, minHeight: 320, idealHeight: 480)
        .onAppear {
            // Keep the editor's status in step with the library read a moment earlier.
            secretState = McpSecretState(envSet: server["envSet"].strings, headerSet: server["headerSet"].strings)
        }
    }

    private func row(_ field: String, _ key: String) -> some View {
        HStack(spacing: 10) {
            Text(key).font(.system(.callout, design: .monospaced)).frame(width: 190, alignment: .leading)
            SecureField(isSaved(field, key) ? "已保存，留空表示不修改" : "输入值", text: value(field, key))
                .textFieldStyle(.roundedBorder)
            if isSaved(field, key) {
                Pill("已设置", .green)
                Button("清除") { clear(field, key) }.buttonStyle(.link).font(.caption).disabled(saving)
            } else {
                Pill("未设置", .amber)
            }
        }
    }

    private func save() {
        save(clear: [])
    }

    /// Post the typed values. `clear` names `(field, key)` slots to remove in the same request.
    private func save(clear: [(String, String)]) {
        let clearing = Set(clear.map { slot($0.0, $0.1) })
        let sent = values.filter { !$0.value.isEmpty && !clearing.contains($0.key) }
        var body: [String: JSON] = [:]
        for (id, v) in sent {
            let parts = id.split(separator: ":", maxSplits: 1).map(String.init)
            guard parts.count == 2 else { continue }
            var group = body[parts[0]]?.object ?? [:]
            group[parts[1]] = JSON(v)
            body[parts[0]] = .object(group)
        }
        for (field, key) in clear {
            var group = body[field]?.object ?? [:]
            group[key] = ""
            body[field] = .object(group)
        }
        // Nothing typed and nothing to clear: 「保存」 has nothing to send, so just close.
        guard !body.isEmpty else { dismiss(); return }
        saving = true
        Task {
            do {
                let updated = try await store.api.post(path, .object(body))
                values = [:]
                // The reply is the resource as it is now; the local update covers a reply without it.
                if updated["envSet"].exists || updated["headerSet"].exists {
                    secretState = McpSecretState(envSet: updated["envSet"].strings, headerSet: updated["headerSet"].strings)
                } else {
                    secretState.markSaved(Set(sent.keys))
                    secretState.markCleared(Set(clear.map { slot($0.0, $0.1) }))
                }
                if clear.isEmpty { dismiss() }
                await store.loadMcpSources()
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func clear(_ field: String, _ key: String) {
        save(clear: [(field, key)])
    }
}
