import Foundation
import Observation

/// One thing waiting on the person, from any run in any workspace, as the server words it
/// (`GET /api/inbox`, `backend/src/run_view/inbox.rs`).
struct InboxEntry: Identifiable, Equatable {
    /// `<run id>/<what>`: stable while the request is open, so a snooze sticks to it.
    let key: String
    let run: String
    /// What its run is called in the lists.
    let runTitle: String
    let workspace: String
    let title: String
    let text: String
    let sub: String
    /// When it arrived (ms).
    let ts: Double
    /// Nothing is held up while it waits (the lead's own decision, a delivered run to accept, a
    /// lead that may be stuck), so it may be put off. An approval, a form or a collaborator's
    /// question keeps an agent waiting, and cannot. Any entry can still be ignored.
    let canWait: Bool
    /// Answers in one click, right in the inbox.
    var actions: [QuickAction] = []
    /// What it is (an open item's kind, or `delivered` / `interrupted` / `failed` … for a run's own row).
    var kind = ""
    /// The kind in a word or two.
    var label = ""
    /// Put off until then (ms).
    var snoozedUntil: Double?
    var id: String { key }

    init(_ j: JSON) {
        key = j["key"].str
        run = j["run"].str
        runTitle = j["runTitle"].str
        workspace = j["workspaceName"].str
        title = j["title"].str
        text = j["text"].str
        sub = j["sub"].str
        ts = j["ts"].double ?? 0
        canWait = j["canWait"].bool ?? false
        actions = j["actions"].array.map(QuickAction.init)
        kind = j["kind"].str
        label = j["label"].str
        snoozedUntil = j["snoozedUntil"].double
    }
}

/// The global inbox as the server keeps it: what every run waits on the person for, from the same
/// facts the run page shows, and what the person put off or ignored, the same on every client.
// @project-doc docs/architecture/20261031-frontend.md#inbox
@Observable @MainActor
final class InboxModel {
    /// What waits now, newest first.
    private(set) var open: [InboxEntry] = []
    /// What the person put off, the soonest back first.
    private(set) var later: [InboxEntry] = []
    /// What waits in each workspace, by its id, as the server folds `open` (`byWorkspace`).
    private(set) var byWorkspace: [String: JSON] = [:]
    @ObservationIgnored private var refreshGeneration = 0
    @ObservationIgnored private var wakeTask: Task<Void, Never>?
    /// The folded sections of the inbox page; runs at work start folded every time.
    var showLater = false
    var showWorking = false

    /// Read the inbox again: on news of any run, a mark from another client, and when the next
    /// entry put off comes back.
    func refresh(_ store: AppStore) async {
        refreshGeneration += 1
        let generation = refreshGeneration
        guard let j = try? await store.api.get("/api/inbox?lang=\(L.serverLanguage)"), generation == refreshGeneration else { return }
        let open = j["open"].array.map(InboxEntry.init), later = j["later"].array.map(InboxEntry.init)
        // Unchanged entries leave the views that show them alone.
        if open != self.open { self.open = open }
        if later != self.later { self.later = later }
        let byWorkspace = j["byWorkspace"].object
        if byWorkspace != self.byWorkspace { self.byWorkspace = byWorkspace }
        wakeTask?.cancel()
        if let next = j["nextWake"].double {
            let wait = max(0, next - L.nowMs()) / 1000
            wakeTask = Task { [weak self, weak store] in
                try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000) + 500_000_000)
                guard !Task.isCancelled, let self, let store else { return }
                await self.refresh(store)
            }
        }
        Notifier.shared.update(open)
    }

    /// Requests answered from the inbox, until the next refresh shows them gone.
    private(set) var answering: Set<String> = []

    /// Answer a request with one of its quick actions, as the run page would.
    func act(_ e: InboxEntry, _ a: QuickAction, store: AppStore) async {
        answering.insert(e.key)
        var body = a.body
        body["commandId"] = JSON(UUID().uuidString)
        do {
            _ = try await store.api.post("/api/runs/\(e.run)/actions", body)
        } catch {
            store.show(error.localizedDescription)
        }
        await refresh(store)
        answering.remove(e.key)
    }

    func snooze(_ key: String, until: Date, store: AppStore) async {
        await mark(key, ["snoozedUntil": .number((until.timeIntervalSince1970 * 1000).rounded())], store: store)
    }
    func wake(_ key: String, store: AppStore) async { await mark(key, [:], store: store) }
    /// Stop showing it: nothing is answered and the run stays as it is.
    func ignore(_ key: String, store: AppStore) async { await mark(key, ["ignored": true], store: store) }

    private func mark(_ key: String, _ body: JSON, store: AppStore) async {
        var b = body
        b["key"] = JSON(key)
        do {
            _ = try await store.api.post("/api/inbox/marks", b)
        } catch {
            store.show(error.localizedDescription)
        }
        await refresh(store)
    }
}

/// One row of a workspace's 「等你处理」: a goal, or one of its Loops, with what waits on the person there.
struct WaitingRow: Identifiable, Equatable {
    /// The run that waits: a goal, or the Loop's session.
    let id: String
    /// The goal's run row; null for a Loop.
    let run: JSON
    /// The Loop whose session waits; nil for a goal.
    let loop: JSON?
    /// The goal's title, or the Loop's.
    let title: String
    /// What it waits on, at a glance: the one thing by name, or how many, as the server words it.
    let label: String
    /// Red when an agent is held up until the person answers; amber when it can wait.
    let tone: Tone
    var entries: [InboxEntry]

    /// Open items, by kind (`OpenFact.kind`): their glyph.
    static let openSymbols: [String: String] = [
        "approval": "hand.raised.fill", "input": "square.and.pencil",
        "question": "questionmark.bubble.fill", "session": "text.bubble.fill",
        "lead_stall": "hourglass", "loop_touched": "doc.badge.ellipsis", "loop_failed": "exclamationmark.triangle",
        "conflict": "arrow.triangle.merge", "overstep": "arrow.right.to.line",
        "trace": "doc.badge.ellipsis", "stall": "hourglass",
        "blocker": "exclamationmark.octagon.fill", "lead_question": "questionmark.circle.fill",
        "failed": "xmark.octagon.fill", "blocked": "pause.circle.fill",
    ]

    /// What it waits on, as a glyph: the one thing's kind, or a stack for several. It leads the
    /// row, so a Loop that waits does not look like the Loop itself or a goal it dispatched.
    var symbol: String {
        guard entries.count == 1, let e = entries.first else { return "tray.full.fill" }
        switch e.kind {
        case "delivered": return "checkmark.seal.fill"
        case "merge_blocked": return "arrow.triangle.merge"
        case "held": return "hand.raised.fill"
        case "interrupted": return "pause.circle.fill"
        case "failed": return "xmark.octagon.fill"
        case "dream": return "brain.head.profile"
        case "dream_failed": return "exclamationmark.triangle.fill"
        default: return WaitingRow.openSymbols[e.kind] ?? "exclamationmark.circle.fill"
        }
    }
}

extension AppStore {
    /// What waits on the person in one workspace, one row per goal or Loop, newest first, as the
    /// server folds the inbox's entries (`byWorkspace` of `GET /api/inbox`): the same entries as the
    /// inbox, so the workspace's count in the sidebar, its page and the inbox agree.
    // @project-doc docs/architecture/20261031-frontend.md#screens
    func waiting(in ws: Workspace) -> [WaitingRow] {
        (inbox.byWorkspace[ws.id]?.array ?? []).map { w in
            let run = w["run"].str
            return WaitingRow(id: run, run: attention.first { $0["id"].str == run } ?? .null,
                              loop: w["loop"].string.flatMap { id in loops.first { $0["id"].str == id } },
                              title: w["title"].str, label: w["label"].str, tone: Tone(rawValue: w["tone"].str) ?? .amber,
                              entries: w["entries"].strings.compactMap { k in inbox.open.first { $0.key == k } })
        }
    }
}

