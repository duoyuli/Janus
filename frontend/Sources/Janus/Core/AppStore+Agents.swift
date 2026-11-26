import Foundation

/// The settings page's Agents: configuration checks, account logins and the ACP adapters they
/// run on. The server does the work; the app reads it back until it is done.
extension AppStore {
    func testConnection(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/test") } catch { show(L.text("没法开始检查：\(error.localizedDescription)")); return }
        await loadSetup()
        watchChecks()
    }

    func watchChecks() {
        guard polls["checks"] == nil else { return }
        var before = Set(connections.filter { $0["status"].str == "testing" }.map { $0["id"].str })
        guard !before.isEmpty else { return }
        polls["checks"] = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 1_500_000_000)
                guard let self else { return }
                await self.loadSetup()
                let now = Set(self.connections.filter { $0["status"].str == "testing" }.map { $0["id"].str })
                for id in before.subtracting(now) {
                    guard let c = self.connection(id) else { continue }
                    switch c["status"].str {
                    case "ok": self.show(L.text("\(c["name"].str) 检查通过"))
                    case "error": self.show(L.text("\(c["name"].str) 检查未通过：\(c["error"].str)"))
                    // Replaced by a save of the connection, or cut off by a restart: no result.
                    default: self.show(L.text("\(c["name"].str) 的检查没有完成，需要时重新检查"))
                    }
                }
                before = now
                if now.isEmpty { self.polls["checks"] = nil; return }
            }
        }
    }

    func login(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/login") } catch { show(error.localizedDescription); return }
        show(L.text("已开始登录：按卡片上的提示操作，完成后自动检查。"))
        polls["login"]?.cancel()
        polls["login"] = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                guard let self else { return }
                await self.loadSetup()
                let c = self.connection(id)
                if c == nil || c?["login"]["state"].str != "running" {
                    if c?["loggedIn"].truthy == true { await self.testConnection(id) }
                    self.polls["login"] = nil
                    return
                }
            }
        }
    }

    /// Called when an Agent card's runtime line goes away: the person has seen this update's word.
    func acknowledgeAutoUpdate(_ a: JSON) {
        guard a["job"]["action"].str == "auto", a["job"]["state"].str == "ok", a["job"]["ended"].truthy else { return }
        autoUpdateSeen.insert(AdapterText.autoUpdatedKey(a))
    }

    /// Keep the runtime lines current while 「设置 › 数字员工」 is open: the server checks and
    /// updates adapters on its own, and nothing else wakes this page when it does. Runs only while
    /// the page is on screen — the view's `.task` cancels it when the page or settings close — so
    /// there is no app-wide polling and nothing grows with the number of Agents
    /// (docs/architecture/20261031-adapters.md#update_check).
    func watchAdapters() async {
        while !Task.isCancelled {
            try? await Task.sleep(nanoseconds: 2_500_000_000)
            guard !Task.isCancelled else { return }
            await loadAdapters(quiet: true)
        }
    }

    /// `quiet` for the background refresh: a failed read keeps what was shown and says nothing.
    func loadAdapters(quiet: Bool = false) async {
        do {
            let a = try await api.get("/api/adapters")
            // Polled in the background: an unchanged list leaves the sidebar alone.
            if a != adapters { adapters = a }
        } catch {
            if quiet { return }
            adapters = ["adapters": [], "dir": ""]
            show(L.text("读取运行组件失败：\(error.localizedDescription)"))
            return
        }
        let busy = adapters["adapters"].array.contains { $0["job"]["state"].str == "running" }
        if busy && polls["adapters"] == nil {
            polls["adapters"] = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(nanoseconds: 1_500_000_000)
                    guard let self else { return }
                    guard let a = try? await self.api.get("/api/adapters") else { continue }
                    self.adapters = a
                    if !a["adapters"].array.contains(where: { $0["job"]["state"].str == "running" }) {
                        // An automatic update finishes silently; only what the person asked for
                        // confirms itself with a toast.
                        for x in a["adapters"].array where x["job"]["action"].str != "auto" && (x["job"]["ended"].double ?? 0) > L.nowMs() - 2500 {
                            self.show(L.text("\(x["name"].str)：\(x["job"]["detail"].str)"))
                        }
                        self.polls["adapters"] = nil
                        return
                    }
                }
            }
        }
    }

    /// What `/api/adapters/{id}/{action}` does with an adapter: install (the newest, or a named
    /// version; on a pinned adapter this resumes automatic updates) or switch to an installed one
    /// and pin it.
    enum AdapterAction: String { case install, use }

    func adapterAction(_ id: String, _ action: AdapterAction, version: String? = nil) async {
        do { adapters = try await api.post("/api/adapters/\(API.q(id))/\(action.rawValue)", version.map { ["version": JSON($0)] } ?? [:]) } catch { show(error.localizedDescription); return }
        if action == .use, let version { show(L.text("已切到 \(version)，下个会话开始用")) }
        await loadAdapters()
    }
}
