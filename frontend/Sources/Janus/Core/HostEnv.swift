import Foundation

/// What of the person's own home folder is shared with every agent and MCP server
/// (`GET/PUT /api/host-env`): sign-ins and tool settings, linked into each one's own HOME.
// @project-doc docs/architecture/20261031-frontend.md#host_env
enum HostEnv {
    /// The keychain folder; GitHub CLI, Lark CLI and Git keep their sign-ins there.
    static let keychain = "Library/Keychains"

    /// The shared paths after a tool or a path is switched on or off. A tool switched on brings all
    /// its paths found here, and the keychain when its sign-in is kept there (and the keychain is
    /// on this Mac); switched off, it takes back its own paths only, the keychain stays.
    static func toggled(_ shared: [String], paths: [String], on: Bool, keychain: Bool = false) -> [String] {
        if !on { return shared.filter { !paths.contains($0) } }
        var out = shared
        for p in paths + (keychain ? [Self.keychain] : []) where !out.contains(p) { out.append(p) }
        return out
    }

    /// Whether every path of a tool is shared.
    static func isOn(_ shared: [String], paths: [String]) -> Bool { !paths.isEmpty && paths.allSatisfy(shared.contains) }
}

extension AppStore {
    func loadHostEnv() async {
        do { hostEnv = try await api.get("/api/host-env") } catch { show(L.text("读取本机环境失败：\(error.localizedDescription)")) }
    }

    /// Share exactly `paths` from now on; the server refuses what may not be shared.
    @discardableResult
    func shareHost(_ paths: [String]) async -> Bool {
        do {
            hostEnv = try await api.put("/api/host-env", ["paths": JSON.array(paths.map(JSON.init))])
            return true
        } catch {
            show(error.localizedDescription)
            return false
        }
    }

    /// Switch a tool of the survey on or off, as its row on the settings page does.
    func shareTool(_ tool: JSON, on: Bool) async {
        let env = hostEnv ?? [:]
        let keychainHere = env["tools"].array.contains { $0["id"].str == "keychain" }
        let shared = env["shared"].strings
        let next = HostEnv.toggled(shared, paths: tool["paths"].strings, on: on, keychain: tool["keychain"].bool == true && keychainHere)
        if await shareHost(next), on, !shared.contains(HostEnv.keychain), next.contains(HostEnv.keychain) {
            show(L.text("\(tool["name"].str) 的登录存在钥匙串里，同时共享了 macOS 钥匙串"))
        }
    }
}
