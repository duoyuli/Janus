import Foundation
import Observation
import SwiftUI

/// Where the main window is.
enum Screen: Hashable {
    case inbox                 // what every run waits on the person for
    case home(String)          // workspace id
    case run(String)           // run id
    case newWorkspace
}

/// Global state: setup objects (Agents, roles, workspaces), runs, adapters and the resource library.
/// Everything is read from the orchestrator; nothing here is the source of truth.
@Observable @MainActor
final class AppStore {
    let api: API
    let permissions: MacPermissions
    var meta: JSON = .null
    var connections: [JSON] = []
    var roles: [JSON] = []
    var workspaces: [JSON] = []
    var runs: [JSON] = []
    var roleStats: JSON = .null
    var adapters: JSON = .null
    /// The resource library (Settings › 资源库): `items` (skills and MCP servers) and `sources`.
    var library: JSON?
    var screen: Screen?
    var toast: String?
    /// Why the server is not on its configured address (a taken port), if it is not.
    var serverNotice: String?
    var goalDraft = ""
    /// Images pasted into the goal, sent with it.
    var goalImages: [PastedImage] = []
    /// Workspace edited in the sheet (nil id for a new one).
    var editingWorkspace: JSON?
    /// Role edited in the sheet (empty id for a new one).
    var editingRole: JSON?
    private(set) var run: RunModel?
    /// What every run is waiting on the person for, across workspaces.
    let inbox = InboxModel()

    @ObservationIgnored private var polls: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var toastTask: Task<Void, Never>?

    init(api: API, permissions: MacPermissions? = nil) {
        self.api = api
        self.permissions = permissions ?? .unavailable()
    }

    // MARK: Loading

    func boot() async {
        async let m: Void = loadMeta()
        async let r: Void = loadRuns()
        _ = await (m, r)
        await loadSetup()
        await loadLibrary()
        watchLibrary()
        watchChecks()
        await inbox.refresh(self)
        startRunPolling()
        if screen == nil {
            // Start from what needs the person, not from watching a run work.
            if !inbox.open.isEmpty || runs.contains(where: { RunStatus($0["status"].str).isActive }) { open(.inbox) }
            else if let ws = currentWorkspace { open(.home(ws["id"].str)) }
            else { open(.newWorkspace) }
        }
    }

    func loadMeta() async {
        do { meta = try await api.get("/api/meta") } catch { show(L.text("连不上 Janus 后端：\(error.localizedDescription)")) }
    }

    func loadRuns() async {
        // Polled every few seconds: an unchanged list leaves the sidebar alone.
        if let r = try? await api.get("/api/runs"), r["runs"].array != runs { runs = r["runs"].array }
    }

    func loadSetup() async {
        do {
            let s = try await api.get("/api/setup")
            connections = s["connections"].array
            roles = s["roles"].array
            workspaces = s["workspaces"].array
        } catch { show(L.text("读取设置失败：\(error.localizedDescription)")) }
        // How each role's tasks went in recent runs; the list works without it.
        roleStats = (try? await api.get("/api/stats/roles")) ?? .null
    }

    // MARK: Resource library

    var libraryItems: [JSON] { library?["items"].array ?? [] }
    var librarySources: [JSON] { library?["sources"].array ?? [] }
    func libraryItem(_ id: String) -> JSON? { libraryItems.first { $0["id"].str == id } }

    func loadLibrary() async {
        if let l = try? await api.get("/api/library") { library = l }
        else if library == nil { library = ["items": [], "sources": []] }
    }

    /// Sources whose remote moved on since the last fetch (the server checks every few hours).
    var sourceUpdates: [JSON] { librarySources.filter { $0["updateAvailable"].truthy } }

    /// Sources being fetched, so every screen shows the same progress.
    var updatingSources: Set<String> = []

    /// The server records its own checks; re-read the library now and then so reminders show up.
    private func watchLibrary() {
        polls["library"]?.cancel()
        polls["library"] = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 600_000_000_000)
                await self?.loadLibrary()
            }
        }
    }

    /// Ask every remote now, instead of waiting for the server's next round.
    func checkSources() async {
        do {
            _ = try await api.post("/api/library/sources/check", timeout: 240)
            await loadLibrary()
            let n = sourceUpdates.count
            show(n == 0 ? L.text("所有来源都是最新的") : L.text("\(n) 个来源有更新"))
        } catch { show(error.localizedDescription) }
    }

    /// Fetch the given sources again (all with updates when nil); items imported from them follow.
    func updateSources(_ ids: [String]? = nil) async {
        let ids = ids ?? sourceUpdates.map { $0["id"].str }
        guard !ids.isEmpty else { return }
        updatingSources.formUnion(ids)
        var updated: [String] = [], missing: [String] = [], failed: [String] = []
        for id in ids {
            do {
                let r = try await api.post("/api/library/sources/\(id)/update", timeout: 240)
                updated += r["updated"].strings
                missing += r["missing"].strings
            } catch { failed.append(error.localizedDescription) }
            updatingSources.remove(id)
        }
        await loadLibrary()
        if !failed.isEmpty { show(L.text("更新失败：\(failed.joined(separator: L.text("；")))")); return }
        var msg = updated.isEmpty ? L.text("已更新来源，导入的资源没有变化") : L.text("已更新 \(updated.joined(separator: L.text("、")))")
        if !missing.isEmpty { msg += L.text("；\(missing.joined(separator: L.text("、"))) 已不在来源中，保持原样") }
        show(msg)
    }

    /// The sidebar keeps run statuses fresh while the window is open.
    private func startRunPolling() {
        polls["runs"]?.cancel()
        polls["runs"] = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                await self?.loadRuns()
                if let self { await self.inbox.refresh(self) }
            }
        }
    }

    // MARK: Navigation

    func open(_ s: Screen) {
        if case .run(let id) = s {
            // The sidebar follows: a run opened from the inbox, a notification or another
            // workspace's page shows its own workspace's runs around it.
            if let ws = runs.first(where: { $0["id"].str == id })?["config"]["workspace"]["id"].string, !ws.isEmpty, ws != currentWorkspaceId {
                rememberWorkspace(ws)
            }
            if run?.id != id {
                run?.close()
                let m = RunModel(id: id, store: self)
                run = m
                Task { await m.load() }
            } else if screen != s, let m = run {
                // Coming back to the open run: read it again rather than trust a stream that may have stopped.
                Task { await m.load() }
            }
        } else if case .home(let ws) = s {
            rememberWorkspace(ws)
        }
        screen = s
    }

    var currentWorkspaceId: String {
        get { UserDefaults.standard.string(forKey: "janus.ws") ?? "" }
    }
    func rememberWorkspace(_ id: String) { UserDefaults.standard.set(id, forKey: "janus.ws") }

    /// The settings page's pane: Agents, roles, libraries or ACP adapters. The last one opened comes back.
    var settingsTab: String = UserDefaults.standard.string(forKey: "janus.settingsTab") ?? "agents" {
        didSet { UserDefaults.standard.set(settingsTab, forKey: "janus.settingsTab") }
    }
    /// The settings page covers the main window; closing it shows the screen underneath again.
    var showingSettings = false

    func openSettings(_ tab: String) {
        settingsTab = tab
        showingSettings = true
    }
    var currentWorkspace: JSON? { workspace(currentWorkspaceId) ?? workspaces.first }

    // MARK: Lookups

    func kind(_ id: String) -> JSON? { meta["kinds"].array.first { $0["id"].str == id } }
    func connection(_ id: String) -> JSON? { connections.first { $0["id"].str == id } }
    func role(_ id: String) -> JSON? { roles.first { $0["id"].str == id } }
    func workspace(_ id: String) -> JSON? { workspaces.first { $0["id"].str == id } }
    var okConnections: [JSON] { connections.filter { $0["phase"].str == "ok" } }
    var boundRoles: [JSON] { roles.filter { $0["connection"].truthy && connection($0["connection"].str) != nil } }
    func runs(of ws: JSON) -> [JSON] {
        runs.filter { r in
            let wid = r["config"]["workspace"]["id"]
            return wid.truthy ? wid.str == ws["id"].str : r["cwd"].str == ws["path"].str
        }
    }

    /// What is still missing before a first task can run, in order.
    /// Each step names where it is done: a settings pane, or "workspace" for the new-workspace sheet.
    var onboarding: [(String, Bool, String)] {
        [(L.text("接入一个 Agent"), !okConnections.isEmpty, "agents"),
         (L.text("定义数字员工（含一个负责人）"), boundRoles.contains { $0["lead"].truthy } && boundRoles.count >= 2, "roles"),
         (L.text("新建工作区"), !workspaces.isEmpty, "workspace")]
    }

    // MARK: Setup objects

    static let roleColors = ["#c96442", "#7b6cc2", "#3f7d5b", "#b5852b", "#2f7f8c", "#b04d6d", "#5f6b7a", "#8a6a4f"]

    func newRole(connection: String = "") {
        editingRole = ["id": "", "name": JSON(L.text("新数字员工")), "role": "", "desc": "", "connection": JSON(connection.isEmpty ? okConnections.first?["id"].str ?? "" : connection), "model": "", "effort": "", "session": [:], "lead": false, "color": JSON(Self.roleColors[roles.count % Self.roleColors.count]), "init": "", "prompt": "", "resources": []]
    }

    @discardableResult
    func save(_ kind: String, _ body: JSON) async throws -> JSON {
        let saved = try await api.post("/api/setup/\(kind)", body)
        await loadSetup()
        return saved
    }

    func delete(_ kind: String, _ id: String) async throws {
        try await api.delete("/api/setup/\(kind)/\(id)")
        await loadSetup()
    }

    func createRun(goal: String, workspace: String, images: [PastedImage] = []) async {
        do {
            let r = try await api.post("/api/runs", ["goal": JSON(goal), "workspace": JSON(workspace), "images": .array(images.map(\.json))], timeout: images.isEmpty ? nil : 120)
            goalDraft = ""
            goalImages = []
            await loadRuns()
            open(.run(r["id"].str))
        } catch { show(L.text("创建失败：\(error.localizedDescription)")) }
    }

    /// Delete a run with everything Janus kept for it. A run that is still working is refused by
    /// the server as well: stop it first. Asks before deleting unless the person chose not to be
    /// asked again; the workspace's own files stay either way.
    func deleteRun(_ id: String) async {
        let row = runs.first { $0["id"].str == id }
        // The open run's status comes from its event stream, which is fresher than the polled list.
        let status = run?.id == id && run?.loaded == true
            ? run?.state.run.status ?? .unknown
            : row.map { RunStatus($0["status"].str) } ?? .unknown
        if status.isActive {
            show(L.text("任务还在进行，先停止任务再删除"))
            return
        }
        let title = L.goalTitle(row?["goal"].str ?? (run?.id == id ? run?.state.run.goal ?? "" : ""))
        guard confirmDeleteRun(title) else { return }
        do {
            try await api.delete("/api/runs/\(id)")
        } catch {
            show(L.text("删除失败：\(error.localizedDescription)"))
            return
        }
        if run?.id == id {
            run?.close()
            run = nil
        }
        await loadRuns()
        await inbox.refresh(self)
        if case .run(let shown) = screen, shown == id {
            if let ws = currentWorkspace { open(.home(ws["id"].str)) } else { open(.inbox) }
        }
        show(L.text("已删除任务"))
    }

    // MARK: Agent checks and logins run on the server; the app only polls.

    func testConnection(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/test") } catch { show(L.text("无法开始检查：\(error.localizedDescription)")); return }
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
                    if let c = self.connection(id) { self.show(c["status"].str == "ok" ? L.text("\(c["name"].str) 检查通过") : L.text("\(c["name"].str) 检查未通过：\(c["error"].str)")) }
                }
                before = now
                if now.isEmpty { self.polls["checks"] = nil; return }
            }
        }
    }

    func login(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/login") } catch { show(error.localizedDescription); return }
        show(L.text("已开始登录：按卡片上的提示完成，完成后会自动测试。"))
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

    // MARK: Adapters: installs and checks run in the background; poll until they end.

    func loadAdapters() async {
        do { adapters = try await api.get("/api/adapters") } catch { adapters = ["adapters": [], "dir": ""]; show(L.text("读取适配器失败：\(error.localizedDescription)")); return }
        let busy = adapters["adapters"].array.contains { $0["job"]["state"].str == "running" }
        if busy && polls["adapters"] == nil {
            polls["adapters"] = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(nanoseconds: 1_500_000_000)
                    guard let self else { return }
                    guard let a = try? await self.api.get("/api/adapters") else { continue }
                    self.adapters = a
                    if !a["adapters"].array.contains(where: { $0["job"]["state"].str == "running" }) {
                        for x in a["adapters"].array where (x["job"]["ended"].double ?? 0) > L.nowMs() - 2500 { self.show(L.text("\(x["name"].str)：\(x["job"]["detail"].str)")) }
                        self.polls["adapters"] = nil
                        return
                    }
                }
            }
        }
    }

    func adapterAction(_ id: String, _ action: String, version: String? = nil) async {
        do { adapters = try await api.post("/api/adapters/\(id)/\(action)", version.map { ["version": JSON($0)] } ?? [:]) } catch { show(error.localizedDescription); return }
        await loadAdapters()
    }

    // MARK: Toast

    /// The draft a new-workspace sheet starts from.
    static let blankWorkspace: JSON = ["id": "", "name": "", "path": "", "lead": "", "members": [], "brief": ""]

    func show(_ msg: String) {
        toast = msg
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 3_400_000_000)
            if !Task.isCancelled { self?.toast = nil }
        }
    }
}
