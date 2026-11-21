import Foundation
import Observation

/// Pages inside an open run.
enum RunPage: Hashable {
    case board, audit, team, task(String), agent(String)
    /// A task or agent page, which goes back to the workbench.
    var isDetail: Bool { switch self { case .task, .agent: return true; default: return false } }
}

/// One open run: its events, live stream, the state derived from them, and the person's drafts.
// @project-doc docs/architecture/20261031-frontend.md#server_sync
@Observable @MainActor
final class RunModel {
    let id: String
    unowned let store: AppStore
    var api: API { store.api }

    var runJSON: JSON = .null
    var events: [Event] = []
    /// Reading the run: still reading, failed the first read (a later failed reload keeps what was
    /// read), or read, and then whether it runs in this orchestrator (live) or is read-only.
    enum Load: Equatable {
        case loading
        case failed(String)
        case loaded(live: Bool)
    }
    var load: Load = .loading
    var loaded: Bool { if case .loaded = load { return true } else { return false } }
    var live: Bool { load == .loaded(live: true) }
    var loadError: String? { if case .failed(let why) = load { return why } else { return nil } }
    private(set) var state = RunState()
    /// Replay position (number of events shown), nil while live.
    var replay: Int? { didSet { replayState = replay.map { n in derive(Array(events.prefix(n))) } } }
    private(set) var replayState: RunState?
    var shown: RunState { replayState ?? state }
    /// The usage ledger at the shown position: the live run, or the replayed prefix.
    var usage: UsageLedger { shown.usage }
    var isReplaying: Bool { replay != nil }
    /// Actions are possible: a live run, not replaying.
    var canAct: Bool { live && replay == nil }

    /// Whether the run card's wall-clock time must advance every second: the server says this run
    /// is live, it is not a replayed position, and the run is still going. A finished, replayed or
    /// read-only run shows the ledger's recorded value and does not redraw every second.
    var elapsedTicks: Bool { canAct && shown.run.status.isActive && shown.run.start > 0 }

    /// The ledger the run card's summary line shows at `now`
    /// (docs/architecture/20261031-frontend.md#usage_ledger): the card and the snapshot's
    /// `elapsedMs` ask here, so the number they show and the live judgement cannot drift apart.
    /// The expanded usage detail keeps the recorded ledger and does not follow the clock.
    func usageDisplayed(at now: Double) -> UsageLedger { shown.usageForDisplay(now: now, live: canAct) }

    var page: RunPage = .board
    // Drafts: answers chosen per request key, notes, form fields, session messages.
    var picks: [String: String] = [:]
    var notes: [String: String] = [:]
    var forms: [String: JSON] = [:]
    var sessionDrafts: [String: String] = [:]
    /// Images pasted into each session's composer, by the same key as the drafts.
    var sessionImages: [String: [PastedImage]] = [:]
    // Board view state
    var goalOpen = false
    var resultOpen = false
    /// The usage ledger's detail, and the progress timeline, are folded in the run card and above
    /// the conversation until opened here (W1 F3, F8). They last for this opening of the run.
    var usageOpen = false
    var progressOpen = false
    /// The run-level risk list past its preview, opened in place (W1 A14).
    var risksOpen = false
    /// Each acceptance row's own switch; a task without an entry uses its default.
    var taskOpen: [String: Bool] = [:]
    /// The inspector tab chosen for this opening; nil until the board first appears.
    var inspectorTab: String?
    // Audit view state
    var invariantsOpen = false
    var auditCat = "all"
    var auditTask = ""
    var auditActor = ""
    var highlight: Int?

    /// The run's Markdown files that exist, for the 文档 menu. Found again with each derive, not
    /// on every event: checking the files on each draw of the toolbar slowed busy runs down.
    private(set) var documents: [RunDocs.Doc] = []

    @ObservationIgnored private var streamTask: Task<Void, Never>?
    /// Markdown files found in the events so far, and how many events were scanned.
    @ObservationIgnored private var docScan: [String: RunDocs.Doc] = [:]
    @ObservationIgnored private var docScanned = 0
    @ObservationIgnored private var deriveTask: Task<Void, Never>?

    init(id: String, store: AppStore) {
        self.id = id
        self.store = store
    }

    var team: [JSON] { runJSON["config"]["team"].array }
    var workspaceId: String { runJSON["config"]["workspace"]["id"].str }
    /// A conversation with one digital employee (`runs.kind`), shown as a chat page instead of the workbench.
    var isChat: Bool { runJSON["kind"].str == "chat" }

    func read(keepPage: Bool = true) async {
        streamTask?.cancel()
        do {
            let data = try await api.get("/api/runs/\(id)")
            runJSON = data["run"]
            events = data["events"].array.map(Event.init)
            docScan = [:]
            docScanned = 0
            // A derive scheduled before this read would land on the events it replaced.
            deriveTask?.cancel()
            deriveTask = nil
            state = derive(events)
            findDocuments()
            if let r = replay, r > events.count { replay = nil }
            load = .loaded(live: data["live"].bool ?? false)
            if !workspaceId.isEmpty, store.workspace(workspaceId) != nil { store.rememberWorkspace(workspaceId) }
            subscribe()
        } catch {
            if !loaded { load = .failed(error.localizedDescription) }
        }
    }

    func refreshLocalizedState() {
        deriveTask?.cancel()
        deriveTask = nil
        scheduleDerive()
        replayState = replay.map { derive(Array(events.prefix($0))) }
    }

    func close() {
        streamTask?.cancel()
        deriveTask?.cancel()
    }

    private func subscribe() {
        let after = events.last?.seq ?? 0
        let stream = api.stream(run: id, after: after)
        streamTask = Task { [weak self] in
            do {
                for try await item in stream {
                    guard let self else { return }
                    switch item {
                    case .lagged:
                        self.reload()
                        return
                    case .named: continue
                    case .event(let j):
                        let e = Event(j)
                        let last = self.events.last?.seq ?? 0
                        if e.seq <= last { continue }
                        // A gap means something was missed: read the whole run again.
                        if e.seq != last + 1 { self.reload(); return }
                        self.events.append(e)
                        self.scheduleDerive()
                    }
                }
            } catch {}
            // The stream ended (server restarted, network): try again shortly while the run is open.
            guard let self, !Task.isCancelled else { return }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            if !Task.isCancelled, self.store.run === self { self.reload() }
        }
    }

    /// Scans only the events not seen yet, then keeps the candidates that exist now.
    private func findDocuments() {
        if docScanned < events.count {
            let moved = RunDocs.Moved.of(cwd: state.run.cwd, repo: state.run.repo, run: id)
            RunDocs.scan(events[docScanned...], cwd: state.run.cwd, moved: moved, into: &docScan)
            docScanned = events.count
        }
        let found = RunDocs.existing(docScan)
        if found != documents { documents = found }
    }

    /// Read the whole run again from outside the stream task: `read` cancels `streamTask`, so
    /// awaiting it from inside that task would cancel its own request and never resubscribe.
    private func reload() {
        Task { [weak self] in await self?.read() }
    }

    /// Events arrive in bursts; derive once per burst, off the main thread: a long run takes
    /// milliseconds that scrolling needs.
    private func scheduleDerive() {
        guard deriveTask == nil else { return }
        deriveTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 60_000_000)
            guard let self, !Task.isCancelled else { return }
            let events = self.events
            let derived = await Task.detached(priority: .userInitiated) { derive(events) }.value
            // Closed, or read again by `load`, whose own derive is newer.
            guard !Task.isCancelled else { return }
            let before = self.state.run.status
            self.state = derived
            self.findDocuments()
            self.deriveTask = nil
            // Events that came in while deriving get a round of their own.
            if self.events.count > events.count { self.scheduleDerive() }
            // The sidebar lists run statuses; refresh it when this one's changes.
            if derived.run.status != before { await self.store.loadRuns() }
        }
    }

    /// A Run action (docs/interfaces/20261031-http_api.md#run_actions): de-duplicated by `commandId`,
    /// guarded by `expectedSequence`; a 409 refreshes once and tries again.
    @discardableResult
    func act(_ body: JSON) async -> Bool {
        guard replay == nil else { return false }
        var body = body
        body["commandId"] = JSON(UUID().uuidString)
        for attempt in 0..<2 {
            body["expectedSequence"] = JSON(events.last?.seq ?? 0)
            do {
                _ = try await api.post("/api/runs/\(id)/actions", body)
                // Resuming (or messaging an interrupted run) makes it live again: reconnect.
                if !live, ["resumeRun", "message"].contains(body["type"].str) {
                    await read()
                    store.show(body["type"].str == "resumeRun" ? L.text("已从中断处继续") : L.text("正在恢复会话"))
                }
                return true
            } catch let err as APIError where err.status == 409 && attempt == 0 {
                await read()
                continue
            } catch {
                store.show(error.localizedDescription)
                return false
            }
        }
        return false
    }

    // MARK: Team

    struct Who {
        let id, name, initials, color, runtime, model: String; let lead: Bool; let m: JSON
        /// The digital employee it was made from, whose picture it shows; none for the person and Janus.
        var role = ""
    }

    func who(_ id: String) -> Who {
        if id == "user" {
            let name = L.text("你")
            return Who(id: id, name: name, initials: String(name.prefix(1)), color: "#0f766e", runtime: "", model: "", lead: false, m: .null)
        }
        if id == "janus" { return Who(id: id, name: "Janus", initials: "JN", color: "#8a94a6", runtime: "", model: "", lead: false, m: .null) }
        guard let m = team.first(where: { $0["id"].str == id }) else {
            return Who(id: id, name: id, initials: "?", color: "#94a3b8", runtime: "", model: "", lead: false, m: .null)
        }
        let name = m["name"].str
        // A chat's one member holds the main session but leads nobody: it looks like its role.
        let lead = m["lead"].truthy && !isChat
        let initials = m["init"].truthy ? m["init"].str : lead ? "LD" : String(name.prefix(2)).uppercased()
        // A role without a color of its own is slate; the lead's is the one colored tile.
        let own = m["color"].str
        let color = Avatar.neutral.contains(own.lowercased()) ? (lead ? Avatar.lead : Avatar.slate) : own
        let rt = m["runtime_name"].truthy ? m["runtime_name"].str : store.connection(m["runtime"].str)?["name"].str ?? m["runtime"].str
        return Who(id: id, name: name, initials: initials, color: color, runtime: rt, model: m["model"].str, lead: lead, m: m, role: m["role_id"].str)
    }

    func agentName(_ id: String) -> String {
        let w = who(id)
        return w.runtime.isEmpty ? w.name : "\(w.name) · \(w.runtime)"
    }

    /// The inspector tab a run opens on: the last one the person chose, otherwise by state —
    /// team before any task exists, tasks afterwards. It is resolved once per opening, so tasks
    /// appearing later do not switch the tab under the person (W1 U2/A15).
    static func initialInspectorTab(saved: String?, hasTasks: Bool) -> String {
        guard let saved, !saved.isEmpty else { return hasTasks ? "tasks" : "team" }
        return saved
    }
}
