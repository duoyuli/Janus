import Foundation
import Observation

/// Pages inside an open run.
enum RunPage: Hashable {
    case board, audit, team, task(String), agent(String)
    /// A task or agent page, which goes back to the workbench.
    var isDetail: Bool { switch self { case .task, .agent: return true; default: return false } }
}

/// One open run: its events, live stream, the state derived from them, and the person's drafts.
// @project-doc docs/architecture/frontend.md#server_sync
@Observable @MainActor
final class RunModel {
    let id: String
    unowned let store: AppStore
    var api: API { store.api }

    var runJSON: JSON = .null
    var events: [Event] = []
    var live = false
    var loaded = false
    var loadError: String?
    private(set) var state = RunState()
    /// Replay position (number of events shown), nil while live.
    var replay: Int? { didSet { replayState = replay.map { n in derive(Array(events.prefix(n))) } } }
    private(set) var replayState: RunState?
    var shown: RunState { replayState ?? state }
    var isReplaying: Bool { replay != nil }
    /// Actions are possible: a live run, not replaying.
    var canAct: Bool { live && replay == nil }

    var page: RunPage = .board
    // Drafts: answers chosen per request key, notes, form fields, session messages.
    var picks: [String: String] = [:]
    var notes: [String: String] = [:]
    var forms: [String: JSON] = [:]
    var sessionDrafts: [String: String] = [:]
    // Board view state
    var goalOpen = false
    var resultOpen = false
    // Audit view state
    var invariantsOpen = false
    var auditCat = "all"
    var auditTask = ""
    var auditActor = ""
    var highlight: Int?

    @ObservationIgnored private var streamTask: Task<Void, Never>?
    @ObservationIgnored private var deriveTask: Task<Void, Never>?

    init(id: String, store: AppStore) {
        self.id = id
        self.store = store
    }

    var team: [JSON] { runJSON["config"]["team"].array }
    var workspaceId: String { runJSON["config"]["workspace"]["id"].str }

    func load(keepPage: Bool = true) async {
        streamTask?.cancel()
        do {
            let data = try await api.get("/api/runs/\(id)")
            runJSON = data["run"]
            events = data["events"].array.map(Event.init)
            live = data["live"].bool ?? false
            state = derive(events)
            if let r = replay, r > events.count { replay = nil }
            loaded = true
            loadError = nil
            if !workspaceId.isEmpty, store.workspace(workspaceId) != nil { store.rememberWorkspace(workspaceId) }
            subscribe()
        } catch {
            loadError = error.localizedDescription
        }
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

    /// Read the whole run again from outside the stream task: `load` cancels `streamTask`, so
    /// awaiting it from inside that task would cancel its own request and never resubscribe.
    private func reload() {
        Task { [weak self] in await self?.load() }
    }

    /// Events arrive in bursts; derive once per burst.
    private func scheduleDerive() {
        guard deriveTask == nil else { return }
        deriveTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 60_000_000)
            guard let self else { return }
            let before = self.state.run.status
            self.state = derive(self.events)
            self.deriveTask = nil
            // The sidebar lists run statuses; refresh it when this one's changes.
            if self.state.run.status != before { await self.store.loadRuns() }
        }
    }

    /// A Run action (docs/interfaces/http_api.md#run_actions): de-duplicated by `commandId`,
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
                    await load()
                    store.show("已从中断处继续")
                }
                return true
            } catch let err as APIError where err.status == 409 && attempt == 0 {
                await load()
                continue
            } catch {
                store.show(error.localizedDescription)
                return false
            }
        }
        return false
    }

    // MARK: Team

    struct Who { let id, name, initials, color, role, runtime, model: String; let lead: Bool; let m: JSON }

    func who(_ id: String) -> Who {
        if id == "user" { return Who(id: id, name: "你", initials: "你", color: "#0f766e", role: "", runtime: "", model: "", lead: false, m: .null) }
        if id == "janus" { return Who(id: id, name: "Janus", initials: "JN", color: "#8a94a6", role: "", runtime: "", model: "", lead: false, m: .null) }
        guard let m = team.first(where: { $0["id"].str == id }) else {
            return Who(id: id, name: id, initials: "?", color: "#94a3b8", role: "", runtime: "", model: "", lead: false, m: .null)
        }
        let name = m["name"].str
        let initials = m["init"].truthy ? m["init"].str : m["lead"].truthy ? "LD" : String(name.prefix(2)).uppercased()
        let color = m["color"].truthy ? m["color"].str : "#64748b"
        let rt = m["runtime_name"].truthy ? m["runtime_name"].str : store.connection(m["runtime"].str)?["name"].str ?? m["runtime"].str
        return Who(id: id, name: name, initials: initials, color: color, role: m["role"].str, runtime: rt, model: m["model"].str, lead: m["lead"].truthy, m: m)
    }

    func agentName(_ id: String) -> String {
        let w = who(id)
        return w.runtime.isEmpty ? w.name : "\(w.name) · \(w.runtime)"
    }
}
