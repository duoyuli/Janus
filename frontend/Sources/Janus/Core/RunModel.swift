import Foundation
import Observation

/// Pages inside an open run.
enum RunPage: Hashable {
    case board, audit, team, task(String), agent(String)
    /// A task or agent page, which goes back to the workbench.
    var isDetail: Bool { switch self { case .task, .agent: return true; default: return false } }
}

/// One open run: its events, live stream, the state the server works out from them, and the
/// person's drafts.
// @project-doc docs/architecture/20261031-frontend.md#server_sync
@Observable @MainActor
final class RunModel {
    let id: String
    unowned let store: AppStore
    /// For a chat: the digital employees its digital employee proposed that the person has not
    /// acted on yet (`GET /api/runs/{id}/proposal`), as the server tells it; `nil` for none.
    var proposal: Proposal?
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
    var replay: Int? {
        didSet {
            if replay == nil { replayState = nil }
            loadServed()
        }
    }
    private(set) var replayState: RunState?
    var shown: RunState { replayState ?? state }
    /// The usage ledger at the shown position, the live run's or the replayed prefix's, as the
    /// server computes it (`loadServed`).
    var usage = UsageLedger.empty
    var isReplaying: Bool { replay != nil }
    /// Actions are possible: a live run, not replaying.
    var canAct: Bool { live && replay == nil }
    /// The delivery card offers its buttons ("验收", "让负责人改…") at a stage that waits for the
    /// person's review. Not only while the run is live: a run delivered before the service
    /// restarted is accepted from its record, and a message to its lead reopens the session.
    func offersReview(_ stage: ReviewStage) -> Bool { stage.reviews && !isReplaying }
    /// Nothing can be done in it any more: a stopped run the service no longer holds. Any other run
    /// takes an action without being live (a delivered one is accepted or messaged, an interrupted
    /// one resumed, a queued one started).
    var readOnly: Bool { state.readOnly }

    /// Whether the run card's wall-clock time must advance every second: the server says this run
    /// is live, it is not a replayed position, and the run is still going. A finished, replayed or
    /// read-only run shows the ledger's recorded value and does not redraw every second.
    var elapsedTicks: Bool { canAct && shown.run.status.isActive && shown.run.start > 0 }

    /// The ledger the run card's summary line shows at `now`
    /// (docs/architecture/20261031-frontend.md#usage_ledger): the card and the snapshot's
    /// `elapsedMs` ask here, so the number they show and the live judgement cannot drift apart.
    /// The expanded usage detail keeps the recorded ledger and does not follow the clock.
    // @project-doc docs/architecture/20261031-frontend.md#usage_ledger
    func usageDisplayed(at now: Double) -> UsageLedger {
        usage.displayed(at: now, since: shown.run.start, ticking: canAct && shown.run.status.isActive)
    }

    /// The seq of the last event at the replayed position; nil while live.
    private var replayedSeq: Int? { replay.map { n in events.prefix(n).last?.seq ?? 0 } }

    /// Read what the server works out at the shown position: the run's view
    /// (`GET /api/runs/{id}/view`, in the app's language, its conversations at the display level
    /// chosen) and its usage ledger. Asked when the run is read, after each burst of new events,
    /// when the replay position moves, when the language or the display level changes; an answer
    /// for a position no longer shown is dropped.
    // @project-doc docs/architecture/20261031-frontend.md#event_derivation
    func loadServed() {
        let until = replayedSeq
        let last = until ?? events.last?.seq ?? 0
        viewTask?.cancel()
        viewTask = Task { [weak self] in
            guard let self else { return }
            let at = until.map { "&until=\($0)" } ?? ""
            let detail = ChatDensity.current == .detail ? "&detail=1" : ""
            guard let j = try? await self.api.get("/api/runs/\(self.id)/view?lang=\(L.serverLanguage)\(at)\(detail)"),
                  !Task.isCancelled, self.replayedSeq == until else { return }
            if let until {
                self.replayState = RunState(j, events: self.events.filter { $0.seq <= until })
                return
            }
            let before = self.state.run.status
            self.state = RunState(j, events: self.events)
            // The sidebar lists run statuses; refresh it when this one's changes.
            if self.state.run.status != before { await self.store.loadRuns() }
            // The server holds the run now but this page read it when it did not (a run that left
            // the queue, a chat or a Loop whose session opened): read it again to follow it live.
            if !self.live, j["live"].bool == true { await self.read() }
        }
        usageTask?.cancel()
        usageTask = Task { [weak self] in
            guard let self, last > 0 else { return }
            guard let j = try? await self.api.get("/api/runs/\(self.id)/usage?until=\(last)"),
                  !Task.isCancelled, self.replayedSeq == until else { return }
            self.usage = UsageLedger.server(j)
        }
    }

    var page: RunPage = .board
    // Drafts: answers chosen per request key, notes, form fields, session messages.
    var picks: [String: String] = [:]
    var notes: [String: String] = [:]
    var forms: [String: JSON] = [:]
    /// Each composer's draft; a change asks the server what the composer offers for it.
    var sessionDrafts: [SessionTarget: String] = [:] {
        didSet {
            for (target, draft) in sessionDrafts where oldValue[target] != draft { suggest(target, draft: draft) }
        }
    }
    /// Images pasted into each session's composer, by the same key as the drafts.
    var sessionImages: [SessionTarget: [PastedImage]] = [:]
    // Board view state
    var goalOpen = false
    var resultOpen = false
    /// The usage ledger's detail, and the progress timeline, are folded in the run card and above
    /// the conversation until opened here (W1 F3, F8). They last for this opening of the run.
    var usageOpen = false
    var progressOpen = false
    /// Each acceptance row's own switch; a task without an entry uses its default.
    var taskOpen: [String: Bool] = [:]
    /// The delivery folded to one line (the lead at work, the team at work…) is opened to read.
    var reviewOpen = false
    /// Bumped to put the cursor in the lead's composer (「让负责人改…」).
    var leadFocus = 0
    /// Turns and groups of calls the person opened or folded in each conversation (by session,
    /// then row id), for this opening of the run.
    var chatOpen: [SessionTarget: [Int: Bool]] = [:]
    /// The inspector tab chosen for this opening; nil until the board first appears.
    var inspectorTab: String?
    // Audit view state
    var invariantsOpen = false
    var auditCat = "all"
    var auditTask = ""
    var auditActor = ""
    var highlight: Int?

    /// The run's Markdown files that exist, for the 文档 menu, as the server lists them.
    var documents: [RunDoc] { state.docs }

    /// What each conversation's session is writing and running right now, before it is on record,
    /// by `SessionTarget.label`, as the run's stream sends it.
    var preview: [String: LivePreview] = [:]

    /// What each composer offers for its draft, as the server answered.
    var suggestions: [SessionTarget: Conversation.Suggestions] = [:]

    @ObservationIgnored private var streamTask: Task<Void, Never>?
    @ObservationIgnored private var suggestTasks: [SessionTarget: Task<Void, Never>] = [:]
    @ObservationIgnored private var batchTask: Task<Void, Never>?
    @ObservationIgnored private var viewTask: Task<Void, Never>?
    @ObservationIgnored private var usageTask: Task<Void, Never>?

    init(id: String, store: AppStore) {
        self.id = id
        self.store = store
    }

    var team: [JSON] { runJSON["config"]["team"].array }
    var workspaceId: String { runJSON["config"]["workspace"]["id"].str }
    /// Its place in the dispatch queue (1 is next), while it waits: an open queued run is read
    /// again whenever any run's status changes (`AppStore.loadRuns`), so it follows runs ahead of it.
    var queuePosition: Int? { runJSON["queuePosition"].int }
    /// A conversation with one digital employee (`runs.kind`), shown as a chat page instead of the workbench.
    var isChat: Bool { kind == .chat }
    /// A Loop's session (`runs.kind` `loop`), shown as the Loop's page.
    var isLoop: Bool { kind == .loop }
    /// A consolidation of a workspace's team memory (`runs.kind` `dream`), shown as its own page.
    var isDream: Bool { kind == .dream }
    var kind: RunKind { RunKind(runJSON["kind"].str) }
    /// A Loop's page: the runs it dispatched that the page has read, newest first, and where the
    /// next page of them starts (nil when all are read).
    var loopDispatched: [JSON] = []
    var loopMore: String?

    func read(keepPage: Bool = true) async {
        streamTask?.cancel()
        do {
            let data = try await api.get("/api/runs/\(id)")
            runJSON = data["run"]
            events = data["events"].array.map(Event.init)
            // The stream starts again from here and sends each preview anew.
            preview = [:]
            // A batch scheduled before this read would land on the events it replaced.
            batchTask?.cancel()
            batchTask = nil
            load = .loaded(live: data["live"].bool ?? false)
            state.events = events
            if let r = replay, r > events.count { replay = nil } else { loadServed() }
            if !workspaceId.isEmpty, store.workspace(workspaceId) != nil { store.rememberWorkspace(workspaceId) }
            subscribe()
        } catch {
            if !loaded { load = .failed(error.localizedDescription) }
        }
    }

    /// The language changed: the server writes the view in the new one.
    func refreshLocalizedState() {
        loadServed()
    }

    func close() {
        streamTask?.cancel()
        batchTask?.cancel()
        viewTask?.cancel()
        usageTask?.cancel()
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
                    case .named("live", let f):
                        for (key, p) in f["previews"].object { self.preview[key] = p.isNull ? nil : LivePreview(p) }
                    case .named: continue
                    case .event(let j):
                        let e = Event(j)
                        let last = self.events.last?.seq ?? 0
                        if e.seq <= last { continue }
                        // A gap means something was missed: read the whole run again.
                        if e.seq != last + 1 { self.reload(); return }
                        self.events.append(e)
                        self.scheduleLoad()
                    }
                }
            } catch {}
            // The stream ended (server restarted, network): try again shortly while the run is open.
            guard let self, !Task.isCancelled else { return }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            if !Task.isCancelled, self.store.run === self { self.reload() }
        }
    }

    /// Ask what the composer of `target` offers for `draft` (`GET /api/runs/{id}/suggest`), a
    /// moment after the person stops typing; a newer draft cancels the ask before.
    private func suggest(_ target: SessionTarget, draft: String) {
        suggestTasks[target]?.cancel()
        guard !draft.isEmpty else { suggestions[target] = nil; return }
        suggestTasks[target] = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 120_000_000)
            guard let self, !Task.isCancelled,
                  let j = try? await self.api.get("/api/runs/\(self.id)/suggest?key=\(API.q(target.label))&draft=\(API.q(draft))&lang=\(L.serverLanguage)"),
                  !Task.isCancelled else { return }
            self.suggestions[target] = Conversation.Suggestions(j, draft: draft)
        }
    }

    /// Read the whole run again from outside the stream task: `read` cancels `streamTask`, so
    /// awaiting it from inside that task would cancel its own request and never resubscribe.
    private func reload() {
        Task { [weak self] in await self?.read() }
    }

    /// Events arrive in bursts; ask the server once per burst. The events themselves show at once.
    private func scheduleLoad() {
        guard batchTask == nil else { return }
        batchTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 60_000_000)
            guard let self, !Task.isCancelled else { return }
            self.batchTask = nil
            self.state.events = self.events
            self.loadServed()
        }
    }

    /// A Run action (docs/interfaces/20261031-http_api.md#run_actions): de-duplicated by `commandId`,
    /// guarded by `expectedSequence`; a 409 refreshes once and tries again.
    @discardableResult
    func act(_ action: RunAction) async -> Bool {
        guard replay == nil else { return false }
        var body = action.body
        body["commandId"] = JSON(UUID().uuidString)
        for attempt in 0..<2 {
            body["expectedSequence"] = JSON(events.last?.seq ?? 0)
            do {
                _ = try await api.post("/api/runs/\(id)/actions", body)
                // Resuming (or messaging an interrupted run) makes it live again: reconnect.
                if !live, action.reopens {
                    await read()
                    store.show(action == .resumeRun ? L.text("已从中断处继续") : L.text("正在接上会话"))
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
        let initials = Role.initials(m["init"].str, name: name)
        // A role without a color of its own is slate; the lead's is the one colored tile.
        let own = m["color"].str
        let color = Avatar.neutral.contains(own.lowercased()) ? (lead ? Avatar.lead : Avatar.slate) : own
        let rt = m["connection_name"].str
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
