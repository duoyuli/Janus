import Foundation
import Observation
import SwiftUI

/// Where the main window is.
enum Screen: Hashable {
    case home(String)          // workspace id
    case run(String)           // run id
    case newWorkspace
}

/// Global state: setup objects (Agents, roles, workspaces), runs, adapters and the Skill library.
/// Everything is read from the orchestrator; nothing here is the source of truth.
@Observable @MainActor
final class AppStore {
    let api: API
    var meta: JSON = .null
    var connections: [JSON] = []
    var roles: [JSON] = []
    var workspaces: [JSON] = []
    var runs: [JSON] = []
    var roleStats: JSON = .null
    var adapters: JSON = .null
    var skills: [JSON]?
    var screen: Screen?
    var toast: String?
    /// Why the server is not on its configured address (a taken port), if it is not.
    var serverNotice: String?
    var goalDraft = ""
    /// Workspace edited in the sheet (nil id for a new one).
    var editingWorkspace: JSON?
    /// Role edited in the sheet (empty id for a new one).
    var editingRole: JSON?
    private(set) var run: RunModel?

    @ObservationIgnored private var polls: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var toastTask: Task<Void, Never>?

    init(api: API) {
        self.api = api
    }

    // MARK: Loading

    func boot() async {
        async let m: Void = loadMeta()
        async let r: Void = loadRuns()
        _ = await (m, r)
        await loadSetup()
        watchChecks()
        startRunPolling()
        if screen == nil {
            if let live = runs.first(where: { ["planning", "running"].contains($0["status"].str) }) { open(.run(live["id"].str)) }
            else if let ws = currentWorkspace { open(.home(ws["id"].str)) }
            else { open(.newWorkspace) }
        }
    }

    func loadMeta() async {
        do { meta = try await api.get("/api/meta") } catch { show("连不上 Janus 后端：\(error.localizedDescription)") }
    }

    func loadRuns() async {
        if let r = try? await api.get("/api/runs") { runs = r["runs"].array }
    }

    func loadSetup() async {
        do {
            let s = try await api.get("/api/setup")
            connections = s["connections"].array
            roles = s["roles"].array
            workspaces = s["workspaces"].array
        } catch { show("读取设置失败：\(error.localizedDescription)") }
        // How each role's tasks went in recent runs; the list works without it.
        roleStats = (try? await api.get("/api/stats/roles")) ?? .null
    }

    func loadSkills() async {
        skills = (try? await api.get("/api/skills"))?["skills"].array ?? []
    }

    /// The sidebar keeps run statuses fresh while the window is open.
    private func startRunPolling() {
        polls["runs"]?.cancel()
        polls["runs"] = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                await self?.loadRuns()
            }
        }
    }

    // MARK: Navigation

    func open(_ s: Screen) {
        if case .run(let id) = s {
            if run?.id != id {
                run?.close()
                let m = RunModel(id: id, store: self)
                run = m
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

    /// The settings window's pane: Agents, roles or ACP adapters. The last one opened comes back.
    var settingsTab: String = UserDefaults.standard.string(forKey: "janus.settingsTab") ?? "agents" {
        didSet { UserDefaults.standard.set(settingsTab, forKey: "janus.settingsTab") }
    }
    /// Shows the settings window; set by the main window, which has SwiftUI's openSettings action.
    @ObservationIgnored var presentSettings: () -> Void = {}

    func openSettings(_ tab: String) {
        settingsTab = tab
        presentSettings()
    }
    var currentWorkspace: JSON? { workspace(currentWorkspaceId) ?? workspaces.first }

    // MARK: Lookups

    func kind(_ id: String) -> JSON? { meta["kinds"].array.first { $0["id"].str == id } }
    func connection(_ id: String) -> JSON? { connections.first { $0["id"].str == id } }
    func role(_ id: String) -> JSON? { roles.first { $0["id"].str == id } }
    func workspace(_ id: String) -> JSON? { workspaces.first { $0["id"].str == id } }
    var okConnections: [JSON] { connections.filter { $0["status"].str == "ok" } }
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
        [("接入一个 Agent", !okConnections.isEmpty, "agents"),
         ("定义角色（含一个主控）", boundRoles.contains { $0["lead"].truthy } && boundRoles.count >= 2, "roles"),
         ("新建工作区", !workspaces.isEmpty, "workspace")]
    }

    // MARK: Setup objects

    static let roleColors = ["#c96442", "#7b6cc2", "#3f7d5b", "#b5852b", "#2f7f8c", "#b04d6d", "#5f6b7a", "#8a6a4f"]

    func newRole(connection: String = "") {
        editingRole = ["id": "", "name": "新角色", "role": "", "desc": "", "connection": JSON(connection.isEmpty ? okConnections.first?["id"].str ?? "" : connection), "model": "", "effort": "", "session": [:], "lead": false, "color": JSON(Self.roleColors[roles.count % Self.roleColors.count]), "init": "", "prompt": "", "skills": [], "mcp": []]
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

    func createRun(goal: String, workspace: String) async {
        do {
            let r = try await api.post("/api/runs", ["goal": JSON(goal), "workspace": JSON(workspace)])
            goalDraft = ""
            await loadRuns()
            open(.run(r["id"].str))
        } catch { show("创建失败：\(error.localizedDescription)") }
    }

    // MARK: Agent checks and logins run on the server; the app only polls.

    func testConnection(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/test") } catch { show("无法开始检查：\(error.localizedDescription)"); return }
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
                    if let c = self.connection(id) { self.show(c["status"].str == "ok" ? "\(c["name"].str) 检查通过" : "\(c["name"].str) 检查未通过：\(c["error"].str)") }
                }
                before = now
                if now.isEmpty { self.polls["checks"] = nil; return }
            }
        }
    }

    func login(_ id: String) async {
        do { _ = try await api.post("/api/connections/\(id)/login") } catch { show(error.localizedDescription); return }
        show("已开始登录：按卡片上的提示完成，完成后会自动测试。")
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
        do { adapters = try await api.get("/api/adapters") } catch { adapters = ["adapters": [], "dir": ""]; show("读取适配器失败：\(error.localizedDescription)"); return }
        let busy = adapters["adapters"].array.contains { $0["job"]["state"].str == "running" }
        if busy && polls["adapters"] == nil {
            polls["adapters"] = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(nanoseconds: 1_500_000_000)
                    guard let self else { return }
                    guard let a = try? await self.api.get("/api/adapters") else { continue }
                    self.adapters = a
                    if !a["adapters"].array.contains(where: { $0["job"]["state"].str == "running" }) {
                        for x in a["adapters"].array where (x["job"]["ended"].double ?? 0) > L.nowMs() - 2500 { self.show("\(x["name"].str)：\(x["job"]["detail"].str)") }
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
