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
    /// question keeps an agent waiting, and cannot. Any entry can still be ignored.
    let canWait: Bool
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
    /// The person's open items of one live run, and what its latest delivery asks of them.
    static func entries(run: JSON, state: RunState, workspace: String, live: Bool) -> [InboxEntry] {
        let id = run["id"].str
        let names = Dictionary(run["config"]["team"].array.map { ($0["id"].str, $0["name"].str) }, uniquingKeysWith: { a, _ in a })
        let facts = boardFacts(state, name: { names[$0] ?? $0 })
        return facts.open.filter(\.you).map { f in
            InboxEntry(
                key: "\(id)/\(f.id)", run: id, goal: run["goal"].str, workspace: workspace,
                title: f.title, text: f.text, sub: f.sub,
                ts: f.seq.flatMap { state.event($0)?.ts } ?? run["createdAt"].double ?? 0,
                canWait: ["lead_stall", "loop_touched", "loop_failed"].contains(f.kind) || (f.kind == "question" && f.tasks.isEmpty),
                actions: f.actions,
                kind: f.kind
            )
        } + review(run: run, state: state, workspace: workspace, live: live)
    }

    /// What a delivery asks of the person, by its stage (`reviewStage`): to review it, to decide
    /// about a result that did not reach their branch, or to let the lead they interrupted go on.
    /// Nothing while the lead or the team works on it. Each delivery is its own entry, so one
    /// ignored does not hide the next.
    // @project-doc docs/architecture/20261031-frontend.md#inbox
    static func review(run: JSON, state: RunState, workspace: String, live: Bool) -> [InboxEntry] {
        guard let r = state.result, let stage = reviewStage(state, live: live) else { return [] }
        let d = Delivery(r)
        let id = run["id"].str
        func entry(_ kind: String, _ title: String, _ text: String, _ sub: String) -> InboxEntry {
            InboxEntry(key: "\(id)/\(kind)-\(r.n)", run: id, goal: run["goal"].str, workspace: workspace, title: title, text: text, sub: sub,
                       ts: state.event(r.seq)?.ts ?? run["createdAt"].double ?? 0, canWait: true, kind: kind)
        }
        switch stage {
        case .awaiting:
            let signals = reviewSignals(d)
            return [entry("delivered", r.n > 1 ? L.text("第 \(r.n) 次交付，等你验收") : L.text("已交付，等你验收"), ReviewBody.firstLine(d),
                          signals.isEmpty ? L.text("没有需要你看的，核对后验收") : L.text("需要你看的 \(signals.count) 件"))]
        case .blocked(let conflict):
            return [entry("merge_blocked", L.text("结果没进你的分支"), d.mergeNote,
                          conflict ? L.text("让负责人再解决冲突；忽略的话这一版不会进你的分支") : L.text("处理好挡路的东西再合并；忽略的话这一版不会进你的分支"))]
        case .held:
            return [entry("held", L.text("你打断了负责人"), L.text("这一轮改的东西还没有交付"), L.text("让它接着做，或按现在的样子交付"))]
        default:
            return []
        }
    }

    /// What a finished run asks of the person, from its row alone: decide about a run that
    /// stopped. A delivered run is read like a live one, and its delivery speaks (`review`). A chat or a Loop asks none of this: its session closing between
    /// messages or wakes (`interrupted`) is how it rests, and the next message or wake reopens it.
    static func entries(row: JSON, workspace: String) -> [InboxEntry] {
        let kind = RunKind(row["kind"].str)
        guard kind != .chat && kind != .loop else { return [] }
        let id = row["id"].str
        let at = row["createdAt"].double ?? 0
        func entry(_ what: String, _ title: String, _ sub: String) -> InboxEntry {
            InboxEntry(key: "\(id)/\(what)", run: id, goal: row["goal"].str, workspace: workspace, title: title, text: "", sub: sub, ts: at, canWait: true, kind: what)
        }
        // A consolidation of the team memory asks only to be looked at, and is ignored once seen.
        // @project-doc docs/domains/20261123-team_memory.md#memory_review
        if kind == .dream {
            switch RunStatus(row["status"].str) {
            case .delivered: return [entry("dream", L.text("团队记忆整理好了"), L.text("看看改了什么，看过就忽略"))]
            case .interrupted, .failed: return [entry("dream_failed", L.text("团队记忆没整理完"), L.text("已经改的保留；可以在工作区页重新整理"))]
            default: return []
            }
        }
        switch RunStatus(row["status"].str) {
        case .interrupted: return [entry("interrupted", L.text("已中断，可以从中断处继续"), L.text("继续，或者忽略"))]
        case .failed: return [entry("failed", L.text("失败了，可以从中断处继续"), L.text("查看原因后继续，或者忽略"))]
        default: return []
        }
    }

    /// Split into what to do now and what was put off, dropping what the person ignored.
    static func split(_ all: [InboxEntry], snoozed: [String: Double], ignored: Set<String>, now: Double) -> (open: [InboxEntry], later: [InboxEntry]) {
        let kept = all.filter { !ignored.contains($0.key) }
        let later = kept.filter { e in e.canWait && (snoozed[e.key] ?? 0) > now }
        let open = kept.filter { e in !later.contains(e) }
        return (open.sorted { $0.ts > $1.ts }, later.sorted { (snoozed[$0.key] ?? 0) < (snoozed[$1.key] ?? 0) })
    }
}

/// Keeps the inbox current: each live run's new events are read (`GET /api/runs/{id}?after=`)
/// and derived again; finished runs come from the run list. Snoozed and ignored entries are this
/// person's own view and live in `UserDefaults`.
@Observable @MainActor
final class InboxModel {
    private(set) var all: [InboxEntry] = []
    private(set) var snoozed: [String: Double] = InboxModel.load("janus.inbox.snoozed") {
        didSet { UserDefaults.janus.set(snoozed, forKey: "janus.inbox.snoozed") }
    }
    private(set) var ignored: Set<String> = Set(UserDefaults.janus.stringArray(forKey: "janus.inbox.ignored") ?? []) {
        didSet { UserDefaults.janus.set(Array(ignored), forKey: "janus.inbox.ignored") }
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

    var open: [InboxEntry] { Inbox.split(all, snoozed: snoozed, ignored: ignored, now: now).open }
    var later: [InboxEntry] { Inbox.split(all, snoozed: snoozed, ignored: ignored, now: now).later }

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
        // A consolidation's approvals and forms wait while it works; once it ends, its row says it all.
        let chats = store.chatAttention.filter { RunStatus($0["status"].str).isActive } + store.loopAttention + store.dreamAttention
        let read = { (row: JSON) in
            RunStatus(row["status"].str).isActive || (RunStatus(row["status"].str) == .delivered && RunKind(row["kind"].str) != .dream) || RunKind(row["kind"].str) == .loop
        }
        let live = store.attention.filter(read) + chats
        for row in store.attention + chats {
            let ws = RunKind(row["kind"].str) == .chat ? L.text("对话") : store.workspace(row["config"]["workspace"]["id"].str)?.name ?? row["config"]["workspace"]["name"].str
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
                out += Inbox.entries(row: row, workspace: ws)
                continue
            }
            if let unchanged = known?.unchangedEntries(newEvents: new, row: row, workspace: ws, locale: locale) {
                out += unchanged
                continue
            }
            let live = data["live"].bool ?? false
            let updated = await Task.detached(priority: .utility) {
                let events = (known?.events ?? []) + new.map(Event.init)
                let entries = Inbox.entries(run: row, state: derive(events), workspace: ws, live: live) + Inbox.entries(row: row, workspace: ws)
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
        // A request that was answered elsewhere, or a run that moved on, is gone by itself.
        let keys = Set(out.map(\.key))
        if snoozed.keys.contains(where: { !keys.contains($0) }) { snoozed = snoozed.filter { keys.contains($0.key) } }
        if ignored.contains(where: { !keys.contains($0) }) { ignored = ignored.filter { keys.contains($0) } }
        Notifier.shared.update(open)
    }

    /// Requests answered from the inbox, until the next refresh shows them gone.
    private(set) var answering: Set<String> = []

    /// Answer a request with one of its quick actions, as the run page would.
    func act(_ e: InboxEntry, _ a: QuickAction, store: AppStore) async {
        answering.insert(e.key)
        var body = a.action.body
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
    /// Stop showing it: nothing is answered and the run stays as it is.
    func ignore(_ key: String) { ignored.insert(key) }

    private static func load(_ key: String) -> [String: Double] {
        (UserDefaults.janus.dictionary(forKey: key) as? [String: Double]) ?? [:]
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
    var entries: [InboxEntry]

    /// What it waits on, at a glance: the one thing by name, or how many.
    var label: String {
        guard entries.count == 1, let e = entries.first else { return L.text("\(entries.count) 件事") }
        switch e.kind {
        case "delivered": return L.text("等你验收")
        case "merge_blocked": return L.text("结果没进分支")
        case "held": return L.text("等你决定")
        case "interrupted": return L.text("已中断")
        case "failed": return L.text("失败了")
        case "dream": return L.text("整理好了")
        case "dream_failed": return L.text("没整理完")
        default: return L.catalogText(TimelineMark.openKinds[e.kind]?.1 ?? "待处理")
        }
    }

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
        default: return TimelineMark.openKinds[e.kind]?.0 ?? "exclamationmark.circle.fill"
        }
    }

    /// Red when an agent is held up until the person answers; amber when it can wait.
    var tone: Tone { entries.allSatisfy(\.canWait) ? .amber : .red }
}

extension AppStore {
    /// What waits on the person in one workspace, one row per goal or Loop, newest first: the same
    /// entries as the inbox, so the workspace's count in the sidebar, its page and the inbox agree.
    /// Goals come from the attention list, so an old run that waits is never left out by newer ones.
    // @project-doc docs/architecture/20261031-frontend.md#screens
    func waiting(in ws: Workspace) -> [WaitingRow] {
        Inbox.waiting(inbox.open, goals: attention.filter { Self.belongs($0, id: ws.id, path: ws.path) }, loops: loops(of: ws.id))
    }
}

extension Inbox {
    /// Open entries grouped into rows, one per goal or Loop session among `goals` and `loops`, in
    /// the entries' order (newest first); entries of other runs are left out.
    static func waiting(_ open: [InboxEntry], goals: [JSON], loops: [JSON]) -> [WaitingRow] {
        var rows: [WaitingRow] = []
        for e in open {
            if let i = rows.firstIndex(where: { $0.id == e.run }) {
                rows[i].entries.append(e)
            } else if let r = goals.first(where: { $0["id"].str == e.run }) {
                rows.append(WaitingRow(id: e.run, run: r, loop: nil, entries: [e]))
            } else if let l = loops.first(where: { $0["run"].str == e.run }) {
                rows.append(WaitingRow(id: e.run, run: .null, loop: l, entries: [e]))
            }
        }
        return rows
    }
}
