import SwiftUI

/// What of the person's own home folder agents and MCP servers may use: each works in a HOME of
/// its own and sees none of the person's sign-ins or tool settings until they are switched on here.
/// The tools Janus knows are offered when found on this Mac; everything else in the home folder is
/// offered by its path, and any other path can be added. A switch takes effect at once, sessions
/// already running included.
// @project-doc docs/architecture/20261031-frontend.md#host_env
struct HostEnvView: View {
    let store: AppStore
    @Local private var custom = ""
    @Local private var showAll = false

    var body: some View {
        let env = store.hostEnv ?? [:]
        let shared = env["shared"].strings
        let tools = env["tools"].array
        let others = env["others"].strings
        // Shared paths that are neither a found tool's nor in the list of others: added by hand.
        let toolPaths = Set(tools.flatMap { $0["paths"].strings })
        let added = shared.filter { !toolPaths.contains($0) && !others.contains($0) }
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text(L.text("每个 Agent 和 MCP 服务器都在自己独立的家目录里工作，看不到你本机的登录和工具配置。在这里打开的项会链接进它们的家目录，用的就是你本机的同一份登录：在 Agent 里刷新的令牌也写回你这里。没打开的一律看不到；Agent 自己的配置（~/.codex、~/.claude、~/.agents 等）不能共享。改动立即生效，正在运行的会话也一样。"))
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                if store.hostEnv == nil {
                    ProgressView().frame(maxWidth: .infinity)
                } else {
                    group(L.text("登录与账号"), tools.filter { $0["group"].str == "login" }, shared)
                    group(L.text("工具链"), tools.filter { $0["group"].str == "toolchain" }, shared)
                    if let k = tools.first(where: { $0["group"].str == "system" }) {
                        section(L.text("macOS 钥匙串"), note: L.text("GitHub CLI、飞书 CLI 和 Git 的登录存在这里。打开后，Agent 也能读到钥匙串里的其他条目。")) {
                            toolRow(k, shared, name: L.text("登录钥匙串"))
                        }
                    }
                    section(L.text("家目录里的其他内容"), note: L.text("上面没有的工具，按它在家目录里的位置打开。")) {
                        let list = showAll ? others : Array(others.prefix(12))
                        ForEach(list, id: \.self) { p in pathRow(p, shared) }
                        ForEach(added, id: \.self) { p in pathRow(p, shared, missing: env["missing"].strings.contains(p)) }
                        if others.count > list.count {
                            Button(L.text("显示全部 \(others.count) 项")) { showAll = true }.buttonStyle(.link).padding(.horizontal, 10).padding(.vertical, 6)
                        }
                        HStack {
                            TextField(L.text("家目录下的路径，例如 .config/mytool"), text: $custom)
                                .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                                .onSubmit(addCustom)
                            Button(L.text("添加")) { addCustom() }
                                .disabled(custom.trimmingCharacters(in: .whitespaces).isEmpty)
                        }
                        .padding(10)
                    }
                }
            }
            .padding(20)
        }
        .task { await store.loadHostEnv() }
    }

    @ViewBuilder private func group(_ title: String, _ tools: [JSON], _ shared: [String]) -> some View {
        if !tools.isEmpty {
            section(title) { ForEach(tools, id: \.self) { t in toolRow(t, shared) } }
        }
    }

    private func section<Content: View>(_ title: String, note: String = "", @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            if !note.isEmpty { Text(note).font(.caption).foregroundStyle(.secondary) }
            Card(padding: 4) { VStack(alignment: .leading, spacing: 0) { content() } }
        }
    }

    private func toolRow(_ t: JSON, _ shared: [String], name: String? = nil) -> some View {
        let paths = t["paths"].strings
        return Toggle(isOn: Binding(get: { HostEnv.isOn(shared, paths: paths) }, set: { on in Task { await store.shareTool(t, on: on) } })) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name ?? t["name"].str)
                Text(paths.map { "~/" + $0 }.joined(separator: "  ")).font(.caption.monospaced()).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .toggleStyle(.switch)
        .padding(.horizontal, 10).padding(.vertical, 6)
        .help(t["keychain"].bool == true ? L.text("它的登录存在 macOS 钥匙串里，打开时一起共享钥匙串") : "")
    }

    private func pathRow(_ p: String, _ shared: [String], missing: Bool = false) -> some View {
        Toggle(isOn: Binding(get: { shared.contains(p) }, set: { on in Task { await store.shareHost(HostEnv.toggled(shared, paths: [p], on: on)) } })) {
            HStack {
                Text(verbatim: "~/" + p).font(.callout.monospaced())
                if missing { Pill(L.text("本机没有"), .gray) }
                Spacer()
            }
        }
        .toggleStyle(.switch)
        .padding(.horizontal, 10).padding(.vertical, 5)
    }

    private func addCustom() {
        let p = custom.trimmingCharacters(in: .whitespaces)
        guard !p.isEmpty else { return }
        let shared = store.hostEnv?["shared"].strings ?? []
        Task { if await store.shareHost(shared + [p]) { custom = "" } }
    }
}
