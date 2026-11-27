import Foundation

/// The settings page's Agents: configuration checks, account logins and the ACP adapters they
/// run on. The server does the work and says on the global stream when it moved on.
extension AppStore {
    func testConnection(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/test") } catch { show(L.text("没法开始检查：\(error.localizedDescription)")); return }
        await loadSetup()
    }

    func login(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/login") } catch { show(error.localizedDescription); return }
        show(L.text("已开始登录：按卡片上的提示操作，完成后自动检查。"))
        loggingIn.insert(id)
        await loadSetup()
    }

    /// The server says a connection moved on (its check or login, `connection` on the global
    /// stream): read the setup again, say how a check ended, and check an account just logged in.
    func connectionChanged(_ id: String) async {
        let before = connection(id)
        await loadSetup()
        guard let c = connection(id) else { loggingIn.remove(id); return }
        if before?["status"].str == "testing" && c["status"].str != "testing" {
            switch c["status"].str {
            case "ok": show(L.text("\(c["name"].str) 检查通过"))
            case "error": show(L.text("\(c["name"].str) 检查未通过：\(c["error"].str)"))
            // Replaced by a save of the connection, or cut off by a restart: no result.
            default: show(L.text("\(c["name"].str) 的检查没有完成，需要时重新检查"))
            }
        }
        if loggingIn.contains(id) && c["login"]["state"].str != "running" {
            loggingIn.remove(id)
            if c["loggedIn"].truthy { await testConnection(id) }
        }
    }

    /// Called when an Agent card's runtime line goes away: the person has seen this update's word.
    func acknowledgeAutoUpdate(_ a: JSON) {
        guard a["job"]["action"].str == "auto", a["job"]["state"].str == "ok", a["job"]["ended"].truthy else { return }
        autoUpdateSeen.insert(a["line"]["autoUpdated"]["key"].str)
    }

    /// Read the adapters: when the settings page opens, and again whenever the server says one
    /// moved on (`adapters` on the global stream). An install the person asked for confirms itself
    /// with a toast when it ends; an automatic update finishes silently. `quiet` for the
    /// background read: a failed read keeps what was shown and says nothing.
    func loadAdapters(quiet: Bool = false) async {
        let running = Set(adapters["adapters"].array.filter { $0["job"]["state"].str == "running" }.map { $0["id"].str })
        do {
            let a = try await api.get("/api/adapters?lang=\(L.serverLanguage)")
            // Read in the background: an unchanged list leaves the sidebar alone.
            if a != adapters { adapters = a }
        } catch {
            if quiet { return }
            adapters = ["adapters": [], "dir": ""]
            show(L.text("读取运行组件失败：\(error.localizedDescription)"))
            return
        }
        for x in adapters["adapters"].array where running.contains(x["id"].str) && x["job"]["state"].str != "running" && x["job"]["action"].str != "auto" {
            show(L.text("\(x["name"].str)：\(x["job"]["detail"].str)"))
        }
    }

    /// What `/api/adapters/{id}/{action}` does with an adapter: install (the newest, or a named
    /// version; on a pinned adapter this resumes automatic updates) or switch to an installed one
    /// and pin it.
    enum AdapterAction: String { case install, use }

    func adapterAction(_ id: String, _ action: AdapterAction, version: String? = nil) async {
        do { adapters = try await api.post("/api/adapters/\(API.q(id))/\(action.rawValue)?lang=\(L.serverLanguage)", version.map { ["version": JSON($0)] } ?? [:]) } catch { show(error.localizedDescription); return }
        if action == .use, let version { show(L.text("已切到 \(version)，下个会话开始用")) }
        await loadAdapters()
    }
}
