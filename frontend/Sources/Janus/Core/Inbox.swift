import Foundation
import Observation

/// One thing waiting on the person, from any run in any workspace.
struct InboxEntry: Identifiable, Equatable {
    /// `<run id>/<what>`: stable while the request is open, so a snooze sticks to it.
    let key: String
    let run: String
    let goal: String
    let workspace: String
    let title: String
    let text: String
    let sub: String
    /// When it arrived (ms).
    let ts: Double
    /// Nothing is held up while it waits (the lead's own decision, a delivered run to accept, a
    /// lead that may be stuck), so it may be put off. An approval, a form or a collaborator's
    /// question keeps an agent waiting, and cannot.
    let canWait: Bool
    /// A run that stopped (interrupted or failed): the person may decide to leave it.
    let canSettle: Bool
    /// Answers in one click, right in the inbox (`OpenFact.actions`).
    var actions: [QuickAction] = []
    /// What it is (`OpenFact.kind`, or `delivered` / `interrupted` / `failed` for a run's own row).
    var kind = ""
    var id: String { key }
}

/// The global inbox: what every run is waiting on the person for, gathered from the same
/// derivation the run page uses (`derive`, `boardFacts`), so the two never disagree.
// @project-doc docs/architecture/20261031-frontend.md#inbox
enum Inbox {
    /// The person's open items of one live run.
    static func entries(run: JSON, state: RunState, workspace: String) -> [InboxEntry] {
        let id = run["id"].str
        let names = Dictionary(run["config"]["team"].array.map { ($0["id"].str, $0["name"].str) }, uniquingKeysWith: { a, _ in a })
        let facts = boardFacts(state, name: { names[$0] ?? $0 })
        return facts.open.filter(\.you).map { f in
            InboxEntry(
                key: "\(id)/\(f.key)", run: id, goal: run["goal"].str, workspace: workspace,
                title: f.title, text: f.text, sub: f.sub,
                ts: f.seq.flatMap { state.event($0)?.ts } ?? run["createdAt"].double ?? 0,
                canWait: ["lead_stall", "loop_touched", "loop_failed"].contains(f.kind) || (f.kind == "question" && f.tasks.isEmpty),
                canSettle: false,
                actions: f.actions,
                kind: f.kind
            )
        }
    }

    /// What a finished run asks of the person, from its row alone: accept a delivery, or decide
    /// about a run that stopped.
    static func entries(row: JSON, workspace: String) -> [InboxEntry] {
        let id = row["id"].str
        let at = row["createdAt"].double ?? 0
        func entry(_ what: String, _ title: String, _ sub: String, wait: Bool, settle: Bool) -> InboxEntry {
            InboxEntry(key: "\(id)/\(what)", run: id, goal: row["goal"].str, workspace: workspace, title: title, text: "", sub: sub, ts: at, canWait: wait, canSettle: settle, kind: what)
        }
        switch RunStatus(row["status"].str) {
        case .delivered: return [entry("delivered", L.text("已交付，等你验收"), L.text("核对证据和风险后验收"), wait: true, settle: false)]
        case .interrupted: return [entry("interrupted", L.text("已中断，可以从中断处继续"), L.text("继续，或者标记为已处理"), wait: true, settle: true)]
        case .failed: return [entry("failed", L.text("失败了，可以从中断处继续"), L.text("查看原因后继续，或者标记为已处理"), wait: true, settle: true)]
        default: return []
        }
    }

    /// Split into what to do now and what was put off, dropping what the person settled.
    static func split(_ all: [InboxEntry], snoozed: [String: Double], settled: Set<String>, now: Double) -> (open: [InboxEntry], later: [InboxEntry]) {
        let kept = all.filter { !settled.contains($0.key) }
        let later = kept.filter { e in e.canWait && (snoozed[e.key] ?? 0) > now }
        let open = kept.filter { e in !later.contains(e) }
        return (open.sorted { $0.ts > $1.ts }, later.sorted { (snoozed[$0.key] ?? 0) < (snoozed[$1.key] ?? 0) })
    }
}

/// Keeps the inbox current: each live run's new events are read (`GET /api/runs/{id}?after=`)
/// and derived again; finished runs come from the run list. Snoozes and settled runs are this
/// person's own view and live in `UserDefaults`.
@Observable @MainActor
final class InboxModel {
    private(set) var all: [InboxEntry] = []
    private(set) var snoozed: [String: Double] = InboxModel.load("janus.inbox.snoozed") {
        didSet { UserDefaults.janus.set(snoozed, forKey: "janus.inbox.snoozed") }
    }
    private(set) var settled: Set<String> = Set(UserDefaults.janus.stringArray(forKey: "janus.inbox.settled") ?? []) {
        didSet { UserDefaults.janus.set(Array(settled), forKey: "janus.inbox.settled") }
    }
    private(set) var now = Date().timeIntervalSince1970 * 1000
    struct CachedRun {
        var events: [Event]
        var row: JSON
        var workspace: String
        var locale: Locale
        var entries: [InboxEntry]

        func unchangedEntries(newEvents: [JSON], row: JSON, workspace: String, locale: Locale) -> [InboxEntry]? {
            guard newEvents.isEmpty, self.row == row, self.workspace == workspace, self.locale == locale else { return nil }
            return entries
        }
    }
    @ObservationIgnored private var cache: [String: CachedRun] = [:]
    @ObservationIgnored private var refreshGeneration = 0
    /// The folded sections of the inbox page; runs at work start folded every time.
    var showLater = false
    var showWorking = false

    var open: [InboxEntry] { Inbox.split(all, snoozed: snoozed, settled: settled, now: now).open }
    var later: [InboxEntry] { Inbox.split(all, snoozed: snoozed, settled: settled, now: now).later }

    /// Bring the inbox up to date. `changed` names the runs with news on the global stream: only
    /// they are read; the others keep their entries unless their status, workspace or the language
    /// changed. Without it every candidate is read.
    func refresh(_ store: AppStore, changed: Set<String>? = nil) async {
        refreshGeneration += 1
        let generation = refreshGeneration
        let locale = L.locale
        var out: [InboxEntry] = []
        // A chat's approvals and forms wait on the person like a run's; a chat with no open
        // session asks nothing of them (the next message reopens it), so only open chats count.
        // Candidates come from the attention list, so an old run that waits is never left out
        // by newer ones (FL-06). A delivered run is read too: after delivery the lead may still
        // ask something in its conversation, and that waits on the person like any request.
        // @project-doc docs/domains/20261120-chats.md#chat_app
        // @project-doc docs/architecture/20261031-frontend.md#inbox
        // A Loop asks of the person whether or not its session is open: its questions outlive the
        // session, and a wake that failed (or changed files) is told between wakes.
        let chats = store.chatAttention.filter { RunStatus($0["status"].str).isActive } + store.loopAttention
        let read = { (row: JSON) in RunStatus(row["status"].str).isActive || RunStatus(row["status"].str) == .delivered || row["kind"].str == "loop" }
        let live = store.attention.filter(read) + chats
        for row in store.attention + chats {
            let chat = row["kind"].str == "chat" || row["kind"].str == "loop"
            let ws = row["kind"].str == "chat" ? L.text("对话") : store.workspace(row["config"]["workspace"]["id"].str)?["name"].string ?? row["config"]["workspace"]["name"].str
            guard read(row) else {
                out += Inbox.entries(row: row, workspace: ws)
                continue
            }
            let id = row["id"].str
            let known = cache[id]
            if let changed, !changed.contains(id), let known, known.row["status"] == row["status"], known.workspace == ws, known.locale == locale {
                out += known.entries
                continue
            }
            guard let data = try? await store.api.get("/api/runs/\(id)?after=\(known?.events.last?.seq ?? 0)") else {
                out += known?.entries ?? []
                continue
            }
            guard generation == refreshGeneration, locale == L.locale else { return }
            let row = data["run"]
            let new = data["events"].array
            guard read(row) else {
                cache[id] = nil
                if !chat { out += Inbox.entries(row: row, workspace: ws) }
                continue
            }
            if let unchanged = known?.unchangedEntries(newEvents: new, row: row, workspace: ws, locale: locale) {
                out += unchanged
                continue
            }
            let updated = await Task.detached(priority: .utility) {
                let events = (known?.events ?? []) + new.map(Event.init)
                let entries = Inbox.entries(run: row, state: derive(events), workspace: ws) + Inbox.entries(row: row, workspace: ws)
                return CachedRun(events: events, row: row, workspace: ws, locale: locale, entries: entries)
            }.value
            guard generation == refreshGeneration, locale == L.locale else { return }
            cache[id] = updated
            out += updated.entries
        }
        guard generation == refreshGeneration, locale == L.locale else { return }
        let liveIds = Set(live.map { $0["id"].str })
        cache = cache.filter { liveIds.contains($0.key) }
        // Unchanged entries leave the views that show them alone.
        if out != all { all = out }
        now = Date().timeIntervalSince1970 * 1000
        // A request that was answered elsewhere, or a run that moved on, is settled by itself.
        let keys = Set(out.map(\.key))
        if snoozed.keys.contains(where: { !keys.contains($0) }) { snoozed = snoozed.filter { keys.contains($0.key) } }
        if settled.contains(where: { !keys.contains($0) }) { settled = settled.filter { keys.contains($0) } }
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

    func snooze(_ key: String, until: Date) { snoozed[key] = until.timeIntervalSince1970 * 1000 }
    func wake(_ key: String) { snoozed[key] = nil }
    func settle(_ key: String) { settled.insert(key) }

    private static func load(_ key: String) -> [String: Double] {
        (UserDefaults.janus.dictionary(forKey: key) as? [String: Double]) ?? [:]
    }
}
