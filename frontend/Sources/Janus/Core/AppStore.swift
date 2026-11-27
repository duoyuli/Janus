import AppKit
import Foundation
import Observation
import SwiftUI

/// Where the main window is.
enum Screen: Hashable {
    case inbox                 // what every run waits on the person for
    case chats                 // talk with one digital employee: start a chat, the recent ones
    case home(String)          // workspace id
    case run(String)           // run id
    case newWorkspace
}

/// The sections of the general settings pane that other places link to directly.
enum GeneralSection: Hashable {
    case updates
    /// 权限: the macOS permissions Janus needs, and what of the home folder agents may use.
    case permissions
    /// 本机环境, the second part of 权限.
    case hostEnv
}

/// Global state: setup objects (Agents, roles, workspaces), runs, adapters and the resource library.
/// Everything is read from the orchestrator; nothing here is the source of truth.
@Observable @MainActor
final class AppStore {
    let api: API
    let permissions: MacPermissions
    var meta: JSON = .null
    var connections: [JSON] = []
    var roles: [Role] = []
    var workspaces: [Workspace] = []
    var runs: [JSON] = []
    /// Chats with one digital employee, newest first, read a page at a time (`GET /api/runs?kind=chat`);
    /// not part of any workspace.
    let chatList: RunPager
    var chats: [JSON] { chatList.runs }
    /// Each workspace's task records, newest first, read a page at a time once its page has shown
    /// (`loadHistory`); refreshed with the other lists.
    private(set) var histories: [String: RunPager] = [:]
    /// Every run and chat that may need the person (working, delivered, stopped part-way), however
    /// many newer ones there are: what the inbox and the quit check read (`GET /api/runs?attention=1`).
    var attention: [JSON] = []
    var chatAttention: [JSON] = []
    /// Every workspace's Loops with what each is doing now (`GET /api/loops`), read again when the
    /// global stream says one changed.
    var loops: [JSON] = []
    /// Loop sessions that may ask the person something (`GET /api/runs?kind=loop&attention=1`).
    var loopAttention: [JSON] = []
    /// Consolidations of a workspace's team memory that are running or that the person has not
    /// looked at yet (`GET /api/runs?kind=dream&attention=1`).
    var dreamAttention: [JSON] = []
    /// Each workspace's team memory whose page has shown (`loadMemories`): its notes and its
    /// consolidation, read again when the global stream says it changed.
    var memories: [String: JSON] = [:]
    /// Every workspace goal runs were made in, as their snapshots name it (`GET /api/runs/workspaces`).
    var runWorkspaces: [JSON] = []
    /// Workspaces deleted since, whose runs stay (`deleted` as the server marks them): listed after
    /// the others, so their runs are found.
    var pastWorkspaces: [JSON] { runWorkspaces.filter { $0["deleted"].bool == true } }
    /// The workspaces with something at work now (`working` of `GET /api/runs/workspaces`).
    var workingWorkspaces: Set<String> = []
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
    /// The digital employee a new chat is with. The last one picked comes back.
    var chatRole: String = UserDefaults.janus.string(forKey: "janus.chatRole") ?? "" {
        didSet { UserDefaults.janus.set(chatRole, forKey: "janus.chatRole") }
    }
    /// Workspace edited in the sheet (nil id for a new one): its folder, name and team.
    var editingWorkspace: JSON?
    /// Workspace whose shared material is edited in its own sheet; its `brief` is what the sheet
    /// opens with (the saved text, or that with a lead's suggested lines added).
    var editingBrief: JSON?
    /// Loop made or edited in the sheet (empty id for a new one).
    var editingLoop: JSON?
    /// Team memory note read or changed in the sheet.
    var editingMemory: JSON?
    /// What of the person's home agents do not see, as the settings page last read it.
    var hostEnv: JSON?
    /// The phone access switch and the paired devices, while the settings page shows them
    /// (`AppStore+Devices.swift`).
    var lanAccess: JSON?
    var devices: [JSON] = []
    /// The last command tried as the person and as an agent (`tryOnHost`).
    var hostTry: JSON?
    /// The sheet that creates the digital employees a chat proposed, while it is open.
    var proposalSheet: ProposalSheet?
    /// Automatic updates already shown: leaving the settings page and coming back does not show
    /// "已自动更新" again (docs/architecture/20261031-adapters.md#update_check).
    var autoUpdateSeen: Set<String> = []
    /// Connections whose login the person started here: when the server says it ended logged in,
    /// the app checks the connection.
    @ObservationIgnored var loggingIn: Set<String> = []
    /// Role edited in the sheet (empty id for a new one). The sheet belongs to the settings panel,
    /// so it opens only through `editRole`, which opens the panel too.
    private(set) var editingRole: JSON?
    private(set) var run: RunModel?
    /// What every run is waiting on the person for, across workspaces.
    let inbox = InboxModel()

    /// The background tasks in flight, by name, so a second start replaces the first: the global
    /// stream and the inbox's clock.
    @ObservationIgnored private var polls: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var toastTask: Task<Void, Never>?

    init(api: API, permissions: MacPermissions? = nil) {
        self.api = api
        self.permissions = permissions ?? .unavailable()
        self.chatList = RunPager(query: "kind=\(RunKind.chat.rawValue)", api: api)
    }

    // MARK: Loading

    func boot() async {
        async let m: Void = loadMeta()
        async let r: Void = loadRuns()
        _ = await (m, r)
        await loadSetup()
        await loadLibrary()
        await inbox.refresh(self)
        followEvents()
        if screen == nil {
            // Start from what needs the person, not from watching a run work.
            if !inbox.open.isEmpty || attention.contains(where: { RunStatus($0["status"].str).isActive }) { open(.inbox) }
            else if let ws = currentWorkspace { open(.home(ws.id)) }
            else { open(.newWorkspace) }
        }
    }

    func loadMeta() async {
        do { meta = try await api.get("/api/meta") } catch { show(L.text("连不上 Janus 服务：\(error.localizedDescription)")) }
    }

    /// Read a workspace's task records the first time its page shows; later reads come with `loadRuns`.
    func loadHistory(_ workspace: String) async {
        guard histories[workspace] == nil else { return }
        let pager = RunPager(query: "workspace=\(workspace.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? "")", api: api)
        histories[workspace] = pager
        await pager.refresh()
    }

    func loadRuns() async {
        // Read again when the global stream says a run changed: an unchanged list leaves the sidebar alone.
        if let r = try? await api.get("/api/runs"), r["runs"].array != runs { runs = r["runs"].array }
        await chatList.refresh()
        for pager in histories.values { await pager.refresh() }
        if let r = try? await api.get("/api/runs?attention=1&limit=500&lang=\(L.serverLanguage)"), r["runs"].array != attention { attention = r["runs"].array }
        if let r = try? await api.get("/api/runs?kind=\(RunKind.chat.rawValue)&attention=1&limit=500&lang=\(L.serverLanguage)"), r["runs"].array != chatAttention { chatAttention = r["runs"].array }
        if let r = try? await api.get("/api/runs?kind=\(RunKind.loop.rawValue)&attention=1&limit=500&lang=\(L.serverLanguage)"), r["runs"].array != loopAttention { loopAttention = r["runs"].array }
        if let r = try? await api.get("/api/runs?kind=\(RunKind.dream.rawValue)&attention=1&limit=500&lang=\(L.serverLanguage)"), r["runs"].array != dreamAttention { dreamAttention = r["runs"].array }
        if let r = try? await api.get("/api/runs/workspaces") {
            if r["workspaces"].array != runWorkspaces { runWorkspaces = r["workspaces"].array }
            let working = Set(r["working"].strings)
            if working != workingWorkspaces { workingWorkspaces = working }
        }
        // An open run waiting in the queue moves up when a run ahead of it starts or goes, which
        // writes nothing to its own log: read its place again.
        if let r = run, r.state.run.status == .queued { await r.read() }
    }

    func isChat(_ id: String) -> Bool { chats.contains { $0["id"].str == id } }

    /// Whether the server has a run at work now (`atWork` of `GET /api/meta`), asked afresh rather
    /// than read from the lists, which may lag; no answer counts as at work.
    // @project-doc docs/architecture/20261031-frontend.md#auto_update
    func runsAtWork() async -> Bool {
        guard let m = try? await api.get("/api/meta") else { return true }
        return !m["atWork"].array.isEmpty
    }

    /// Goals and chats still at work: quitting the server Janus started would interrupt them.
    var working: [JSON] { (attention + chatAttention + loopAttention).filter { RunStatus($0["status"].str).isActive } }

    /// The team a new workspace starts with, and each group's (`team` of `GET /api/setup`).
    var defaultTeam: JSON = .null
    /// The steps a first goal still needs (`onboarding`) and the digital employees by group
    /// (`roleGroups`), as `GET /api/setup` says.
    var setupSteps: [JSON] = []
    var servedRoleGroups: [JSON] = []

    func loadSetup() async {
        do {
            let s = try await api.get("/api/setup?lang=\(L.serverLanguage)")
            connections = s["connections"].array
            roles = s["roles"].array.map(Role.init)
            workspaces = s["workspaces"].array.map(Workspace.init)
            defaultTeam = s["team"]
            setupSteps = s["onboarding"].array
            servedRoleGroups = s["roleGroups"].array
        } catch { show(L.text("读取设置失败：\(error.localizedDescription)")) }
        await AvatarImages.shared.sync(roles, api: api)
        // How each role's tasks went in recent runs; the list works without it.
        roleStats = (try? await api.get("/api/stats/roles")) ?? .null
    }

    // MARK: Resource library

    var libraryItems: [JSON] { library?["items"].array ?? [] }
    var librarySources: [JSON] { library?["sources"].array ?? [] }

    func loadLibrary() async {
        if let l = try? await api.get("/api/library") { library = l }
        else if library == nil { library = ["items": [], "sources": []] }
    }

    /// Sources whose remote moved on since the last fetch (the server checks every few hours).
    /// Prepares a server again: a command started once in its sandbox, a URL reached; its tools counted.
    func warmMcp(_ id: String) async {
        do { _ = try await api.post("/api/library/mcp/\(id)/warm") } catch { show(error.localizedDescription) }
        await loadLibrary()
    }

    var sourceUpdates: [JSON] { librarySources.filter { $0["updateAvailable"].truthy } }

    /// Whether the MCP router gives agents `run_code` (code mode); on unless turned off.
    var codeMode: Bool { library?["mcpRouter"]["code"].bool ?? true }

    /// Turn code mode on or off for the sessions that start from now on.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    func setCodeMode(_ on: Bool) async {
        do {
            let r = try await api.put("/api/library/router", ["code": JSON(on)])
            library?["mcpRouter"] = ["code": r["code"]]
        } catch { show(error.localizedDescription) }
    }

    /// Sources being fetched, so every screen shows the same progress.
    var updatingSources: Set<String> = []

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

    /// Whether the lists changed since the last batch of news from the global stream, gathered
    /// for one read a moment later.
    @ObservationIgnored private var listsChanged = false
    @ObservationIgnored private var batch: Task<Void, Never>?

    /// Follows every run through the server's global stream (`GET /api/stream`) instead of polling:
    /// news of any run reads the inbox again (the server brings only that run up to date), and the
    /// lists are read again when a run is created, changes status or a chat gets its title. When
    /// the stream opens, lags or ends, everything is read once; then it reconnects. The inbox asks
    /// again when the next entry put off comes back (`InboxModel.refresh`).
    // @project-doc docs/architecture/20261031-frontend.md#server_sync
    private func followEvents() {
        polls["events"]?.cancel()
        polls["events"] = Task { [weak self] in
            while !Task.isCancelled {
                guard let stream = self?.api.streamAll() else { return }
                do {
                    for try await item in stream {
                        guard let self else { return }
                        switch item {
                        case .named("ready", _):
                            await self.sendLanguage()
                            await self.readAll()
                        case .lagged: await self.readAll()
                        case .named("run", let c):
                            self.note(lists: true)
                            // A run a Loop dispatched changed: that Loop's counts and list did too.
                            if let loop = c["loop"].string, !loop.isEmpty { await self.loadLoops() }
                        case .named("loop", _): await self.loadLoops()
                        // A workspace's team memory changed: read it again if its page has shown.
                        case .named("memory", let c): if self.memories[c["workspace"].str] != nil { await self.loadMemories(c["workspace"].str) }
                        // The library moved on by itself (a skill an agent saved, a server prepared,
                        // the sources checked): read it again if it has shown.
                        case .named("library", _): if self.library != nil { await self.loadLibrary() }
                        case .named("connection", let c): await self.connectionChanged(c["connection"].str)
                        case .named("adapters", _): if self.adapters != .null { await self.loadAdapters(quiet: true) }
                        // Another client put an inbox entry off, brought it back or ignored it.
                        case .named("inbox", _): await self.inbox.refresh(self)
                        case .named("devices", _): if self.lanAccess != nil { _ = await self.loadDevices() }
                        case .named("event", let n): self.note(lists: Self.listChanges(EventKind(wire: n["type"].str), run: n["run"].str, store: self))
                        default: break
                        }
                    }
                } catch {}
                // The stream ended (the server restarted, or is gone for a moment): read once, retry.
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                await self?.readAll()
            }
        }
    }

    /// Whether an event changes what the lists show: a run they do not have yet, or a chat's first
    /// message (its title). Status changes come as their own notices.
    static func listChanges(_ type: EventKind, run: String, store: AppStore) -> Bool {
        if type == .runCreated { return true }
        if type == .userMessage, let chat = store.chats.first(where: { $0["id"].str == run }) { return chat["goal"].str.isEmpty }
        return !store.attention.contains { $0["id"].str == run } && !store.chats.contains { $0["id"].str == run } && !store.runs.contains { $0["id"].str == run }
            && !store.loops.contains { $0["run"].str == run }
    }

    /// Tell the service the language the app is used in (`PUT /api/language`): what it tells agents
    /// in the runs created from now on is in this language. Sent whenever the stream opens (the
    /// service may have restarted or be new) and when the person picks another language.
    // @project-doc docs/domains/20261031-goal_mode.md#prompt_language
    func sendLanguage() async {
        _ = try? await api.put("/api/language", ["language": .string(L.serverLanguage)])
    }

    private func readAll() async {
        await loadLoops()
        await loadRuns()
        await inbox.refresh(self)
    }

    private func note(lists: Bool) {
        listsChanged = listsChanged || lists
        guard batch == nil else { return }
        batch = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 300_000_000)
            guard let self else { return }
            let lists = self.listsChanged
            self.listsChanged = false
            self.batch = nil
            if lists { await self.loadRuns() }
            await self.inbox.refresh(self)
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
                Task { await m.read() }
            } else if screen != s, let m = run {
                // Coming back to the open run: read it again rather than trust a stream that may have stopped.
                Task { await m.read() }
            }
        } else if case .home(let ws) = s {
            rememberWorkspace(ws)
        }
        screen = s
    }

    var currentWorkspaceId: String {
        get { UserDefaults.janus.string(forKey: "janus.ws") ?? "" }
    }
    func rememberWorkspace(_ id: String) { UserDefaults.janus.set(id, forKey: "janus.ws") }

    /// The settings panel's panes.
    enum SettingsTab: String, CaseIterable {
        case general, agents, library, usage, devices
    }

    /// The settings page's pane. The last one opened comes back.
    var settingsTab: SettingsTab = SettingsTab(rawValue: UserDefaults.janus.string(forKey: "janus.settingsTab") ?? "") ?? .general {
        didSet { UserDefaults.janus.set(settingsTab.rawValue, forKey: "janus.settingsTab") }
    }
    /// The resource library's tabs: skills and MCP servers, each with its own router.
    enum LibraryKind: String, CaseIterable {
        case skill, mcp
    }

    /// The library tab shown. The last one opened comes back.
    var libraryKind: LibraryKind = LibraryKind(rawValue: UserDefaults.janus.string(forKey: "janus.libraryKind") ?? "") ?? .skill {
        didSet { UserDefaults.janus.set(libraryKind.rawValue, forKey: "janus.libraryKind") }
    }

    enum AgentEditorRequest: Equatable {
        /// A new agent of `kind` on `auth`; `template` brings that auth's environment template in.
        case new(kind: String, auth: String?, template: Bool)
        case existing(String)
    }
    var agentEditorRequest: AgentEditorRequest?
    /// The environment text of the agent editor while it is open, for the end-to-end state.
    var agentEditorEnv: String?

    func editAgent(_ request: AgentEditorRequest) {
        agentEditorRequest = request
        openSettings(.agents)
    }

    func openLibrary(_ kind: LibraryKind) {
        libraryKind = kind
        openSettings(.library)
    }

    /// An MCP server open in the library's editor: what the editor starts from, the server as the
    /// library lists it (nil for a new one), whether it opens on the form or the JSON, and what to
    /// fill in (a pick from the catalog that needs a key).
    struct McpEditing: Identifiable {
        let draft: McpDraft
        let item: JSON?
        var json = false
        var note = ""
        var id: String { draft.id }
    }

    var editingMcp: McpEditing?

    /// Opens the MCP editor on a new server, or on a saved one read whole first, values included.
    func editMcp(_ item: JSON?, json: Bool = false) async {
        guard let item else {
            editingMcp = McpEditing(draft: McpDraft(), item: nil, json: json)
            return
        }
        do {
            let full = try await api.get("/api/library/items/\(item["id"].str)")
            let values = try await api.get("/api/library/mcp/\(item["id"].str)/values")
            editingMcp = McpEditing(draft: McpDraft(item: full, values: values["values"]), item: item, json: json)
        } catch { show(error.localizedDescription) }
    }

    /// The settings page covers the main window; closing it shows the screen underneath again.
    var showingSettings = false
    /// A section of the general pane to scroll to once it shows; the pane clears it after scrolling.
    var generalSection: GeneralSection?

    func openSettings(_ tab: SettingsTab) {
        settingsTab = tab
        showingSettings = true
    }

    /// Opens a role in the editor, over the settings pane it belongs to.
    func editRole(_ role: JSON) {
        openSettings(.agents)
        editingRole = role
    }

    func closeRoleEditor() {
        editingRole = nil
    }

    /// Opens the general pane scrolled to one of its sections.
    func openSettings(_ section: GeneralSection) {
        generalSection = section
        openSettings(.general)
    }
    var currentWorkspace: Workspace? { workspace(currentWorkspaceId) ?? workspaces.first }

    // MARK: Lookups

    func kind(_ id: String) -> JSON? { meta["kinds"].array.first { $0["id"].str == id } }
    /// One way to sign in of an agent kind, as the catalog describes it.
    func auth(_ kind: String, _ id: String) -> JSON? { self.kind(kind)?["auths"].array.first { $0["id"].str == id } }
    /// Whether the agent is set up by its environment lines (Claude's custom API).
    func envAuth(_ kind: String, _ auth: String) -> Bool { self.auth(kind, auth)?["env"].truthy == true }
    func connection(_ id: String) -> JSON? { connections.first { $0["id"].str == id } }
    func role(_ id: String) -> Role? { roles.first { $0.id == id } }
    func workspace(_ id: String) -> Workspace? { workspaces.first { $0.id == id } }
    var okConnections: [JSON] { connections.filter { $0["phase"].str == "ok" } }
    /// The newest goals of workspace `ws`, by the workspace the server says each is one of.
    func runs(of ws: Workspace) -> [JSON] { runs.filter { $0["workspace"].str == ws.id } }

    /// What is still missing before a first task can run, in order.
    /// Each step names where it is done: a settings pane, or "workspace" for the new-workspace sheet.
    /// Where an onboarding step takes the person: a settings pane, or a new workspace.
    enum Destination: Equatable {
        case settings(SettingsTab)
        case newWorkspace
    }

    var onboarding: [(String, Bool, Destination)] {
        setupSteps.map { ($0["text"].str, $0["done"].bool == true, $0["go"].str == "workspace" ? .newWorkspace : .settings(.agents)) }
    }

    // MARK: Setup objects

    static let roleColors = ["#c96442", "#7b6cc2", "#3f7d5b", "#b5852b", "#2f7f8c", "#b04d6d", "#5f6b7a", "#8a6a4f"]

    func newRole(connection: String = "") {
        editRole(["id": "", "name": JSON(L.text("新数字员工")), "desc": "", "group": "", "connection": JSON(connection.isEmpty ? okConnections.first?["id"].str ?? "" : connection), "model": "", "effort": "", "session": [:], "lead": false, "color": JSON(Self.roleColors[roles.count % Self.roleColors.count]), "init": "", "prompt": "", "resources": []])
    }

    /// The kinds of setup object `/api/setup/{kind}` saves and deletes.
    enum SetupKind: String { case connections, roles, workspaces }

    @discardableResult
    func save(_ kind: SetupKind, _ body: JSON) async throws -> JSON {
        let saved = try await api.post("/api/setup/\(kind.rawValue)", body)
        await loadSetup()
        return saved
    }

    /// Give a digital employee its own picture (a square PNG), or take it back to its initials.
    func setAvatar(_ role: String, png: Data?) async throws {
        let path = "/api/roles/\(API.q(role))/avatar"
        if let png { _ = try await api.put(path, ["data": JSON(png.base64EncodedString())]) } else { try await api.delete(path) }
        await loadSetup()
    }

    /// Take (`adopt`) or decline the newer default text of a field the person edited on a default
    /// digital employee. Returns the role as saved.
    func seedChoice(_ role: String, _ field: String, adopt: Bool) async throws -> JSON {
        let saved = try await api.post("/api/roles/\(role)/seed", ["field": JSON(field), "adopt": JSON(adopt)])
        await loadSetup()
        return saved
    }

    func delete(_ kind: SetupKind, _ id: String) async throws {
        try await api.delete("/api/setup/\(kind.rawValue)/\(API.q(id))")
        await loadSetup()
    }

    func createRun(goal: String, workspace: String, images: [PastedImage] = []) async {
        do {
            // What the person should know before it starts (changes a worktree leaves out, another
            // run in a directory outside git); they may go ahead anyway.
            let warnings = try await api.get("/api/runs/preflight?workspace=\(workspace.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? workspace)")["warnings"].strings
            if !warnings.isEmpty, !confirm(L.text("继续执行？"), warnings.joined(separator: "\n\n"), ok: L.text("继续"), destructive: false) { return }
            let r = try await api.post("/api/runs", ["goal": JSON(goal), "workspace": JSON(workspace), "images": .array(images.map(\.json))], timeout: images.isEmpty ? nil : 120)
            goalDraft = ""
            goalImages = []
            await loadRuns()
            open(.run(r["id"].str))
        } catch { show(L.text("创建失败：\(error.localizedDescription)")) }
    }

    /// Start a chat with a digital employee and open it; its session starts right away and waits
    /// for the person's first message.
    // @project-doc docs/domains/20261120-chats.md#chat_app
    func createChat(role: String) async {
        do {
            let r = try await api.post("/api/chats", ["role": JSON(role)])
            await loadRuns()
            open(.run(r["id"].str))
        } catch { show(L.text("开始对话失败：\(error.localizedDescription)")) }
    }

    /// Delete a run with everything Janus kept for it. A run that is still working is refused by
    /// the server as well: stop it first. Asks before deleting unless the person chose not to be
    /// asked again; the workspace's own files stay either way.
    func deleteRun(_ id: String) async {
        if let chat = chats.first(where: { $0["id"].str == id }) ?? (run?.id == id && run?.isChat == true ? run?.runJSON : nil) {
            await deleteChat(chat)
            return
        }
        let row = runs.first { $0["id"].str == id }
        // Whether the server lets it go: the open run's view is fresher than the listed row.
        let delete = run?.id == id && run?.loaded == true ? run?.state.allowed.answers["delete"] : row?["delete"]
        if let delete, delete["ok"].bool == false {
            show(delete["why"].str)
            return
        }
        let title = row?["title"].string ?? (run?.id == id ? run?.state.title ?? "" : "")
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
            if let ws = currentWorkspace { open(.home(ws.id)) } else { open(.inbox) }
        }
        show(L.text("已删除任务"))
    }

    /// A chat goes with its directory; one still talking is stopped, there is nothing to stop first.
    private func deleteChat(_ row: JSON) async {
        let id = row["id"].str
        guard confirmDeleteChat(row["title"].str, role: row["config"]["team"][0]["name"].str) else { return }
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
        if case .run(let shown) = screen, shown == id { open(.chats) }
        show(L.text("已删除对话"))
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
