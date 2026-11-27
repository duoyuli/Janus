import Foundation

// A run as the server works it out from its event log (`GET /api/runs/{id}/view`,
// `backend/src/run_view.rs`): its tasks, agents, requests, messages, deliveries, the shared board
// and where its delivery's review stands, with the board's words already written in the app's
// language. The app decodes it and computes none of it
// (docs/decisions/20261126-backend_owns_derived_state.md).

struct Event: Identifiable, Hashable {
    let seq: Int
    let ts: Double
    let type: EventKind
    let actor: String
    let to: String?
    let task: String?
    let attempt: Int?
    let text: String
    let data: JSON
    let raw: JSON
    var id: Int { seq }

    init(_ j: JSON) {
        raw = j
        seq = j["seq"].int ?? 0
        ts = j["ts"].double ?? 0
        type = EventKind(wire: j["type"].str)
        actor = j["actor"].str
        to = j["to"].string
        task = j["task"].string
        attempt = j["attempt"].int
        text = j["text"].str
        data = j["data"]
    }
}

struct Attempt {
    let n: Int
    let session: String
    let workspace: String
    let start: Double
    let status: AttemptStatus
    let seq: Int
    let error: String?
    /// When the attempt stopped running (succeeded, failed, cancelled, interrupted); nil while it runs.
    let end: Double?

    init(_ j: JSON) {
        n = j["n"].int ?? 0
        session = j["session"].str
        workspace = j["workspace"].str
        start = j["start"].double ?? 0
        status = AttemptStatus(rawValue: j["status"].str) ?? .running
        seq = j["seq"].int ?? 0
        error = j["error"].string
        end = j["end"].double
    }
}

struct Artifact: Hashable {
    let name, kind, path, desc, hash: String
    let bytes: Int?
    /// The task it belongs to; empty for the whole run's diff (`run_diff`).
    let task: String
    let attempt: Int
    let seq: Int
    let by: String

    init(_ j: JSON) {
        name = j["name"].str; kind = j["kind"].str; path = j["path"].str; desc = j["desc"].str; hash = j["hash"].str
        bytes = j["bytes"].int
        task = j["task"].str
        attempt = j["attempt"].int ?? 0
        seq = j["seq"].int ?? 0
        by = j["by"].str
    }

    /// A diff to review: a task's (`git_diff`) or the whole run's against where it started (`run_diff`).
    var isDiff: Bool { kind == "git_diff" || ofRun }
    /// The whole run's diff: everything accepting brings back, the lead's own edits included.
    var ofRun: Bool { kind == "run_diff" }
}

struct Question {
    let key: String
    let question: String
    let options: [String]
    let blocking: Bool
    let seq: Int
    let from: String
    let answer: String?

    init(_ j: JSON) {
        key = j["key"].str; question = j["question"].str; options = j["options"].strings
        blocking = j["blocking"].bool ?? false
        seq = j["seq"].int ?? 0
        from = j["from"].str
        answer = j["answer"].string
    }
}

/// Whose turn an open session is in, and whether that turn has ended (the session waits for a
/// message). A closed session is no `SessionTurn` at all (`TaskState.session` is nil).
enum SessionTurn: Equatable {
    case agent(idle: Bool)
    case person(idle: Bool)

    /// A session that just opened: the agent at work.
    static let opened = SessionTurn.agent(idle: false)

    init(_ j: JSON) {
        let idle = j["idle"].bool ?? false
        self = j["person"].bool == true ? .person(idle: idle) : .agent(idle: idle)
    }

    var human: Bool { if case .person = self { return true } else { return false } }
    var idle: Bool { switch self { case .agent(let i), .person(let i): return i } }

}

struct TaskState {
    let id: String
    let title, desc, agent: String
    let deps: [String]
    let accept: [String]
    let status: TaskStatus
    let attempts: [Attempt]
    let progress: Double
    let artifacts: [Artifact]
    /// Its open session's turn; nil while it has none.
    let session: SessionTurn?
    var live: Bool { session != nil }
    /// The person holds the session (takeover or interrupt).
    var human: Bool { session?.human ?? false }
    /// The session's turn has ended and it waits for a message.
    var idle: Bool { session?.idle ?? false }
    let blockedBy: String
    /// The checks of its result, as the agent handed them in.
    let checks: [JSON]
    let verificationFailures: [String]
    let error: String
    let question: Question?
    let amendments: [String]
    let summary: String
    let evidence: [String]
    /// The risks the executor reported with its result.
    let risks: [String]
    /// Files the task owns, as its orchestration gave them.
    let owns: [String]
    let verifies: String
    let continues: String
    let handoff: String
    let verdict: Verdict?
    /// Where the finished result disagrees with what the task left in the workspace.
    let traceGaps: [String]

    init(_ j: JSON) {
        id = j["id"].str; title = j["title"].str; desc = j["desc"].str; agent = j["agent"].str
        deps = j["deps"].strings
        accept = j["accept"].strings
        status = TaskStatus(j["status"].str)
        attempts = j["attempts"].array.map(Attempt.init)
        progress = j["progress"].double ?? 0
        artifacts = j["artifacts"].array.map(Artifact.init)
        session = j["session"].isNull ? nil : SessionTurn(j["session"])
        blockedBy = j["blockedBy"].str
        checks = j["checks"].array
        verificationFailures = j["verificationFailures"].strings
        error = j["error"].str
        question = j["question"].isNull ? nil : Question(j["question"])
        amendments = j["amendments"].strings
        summary = j["summary"].str
        evidence = j["evidence"].strings
        risks = j["risks"].strings
        owns = j["owns"].strings
        verifies = j["verifies"].str
        continues = j["continues"].str
        handoff = j["handoff"].str
        verdict = j["verdict"].string.flatMap(Verdict.init(rawValue:))
        traceGaps = j["traceGaps"].strings
    }
}

struct AgentState {
    var phase = AgentPhase.idle
    var note = ""
    var progress: Double?
    var beat: Double = 0
    var session: String?

    init() {}

    init(_ j: JSON) {
        phase = AgentPhase(j["phase"].str)
        note = j["note"].str
        progress = j["progress"].double
        beat = j["beat"].double ?? 0
        session = j["session"].string
    }
}

/// A message sent to a session, followed from being sent to its delivery.
struct Msg {
    let id: String
    let text: String
    let by: String
    let to: String
    let task: String?
    let seq: Int
    /// Stored paths of the images sent with it; `text` still names them (see `Attach.visibleText`).
    let images: [String]
    let delivery: MessageDelivery
    let routedTask: String?
    let routedSeq: Int?
    let deliveredSeq: Int?

    init(_ j: JSON) {
        id = j["id"].str; text = j["text"].str; by = j["by"].str; to = j["to"].str
        task = j["task"].string
        seq = j["seq"].int ?? 0
        images = j["images"].strings
        delivery = MessageDelivery(j["delivery"].str) ?? .pending
        routedTask = j["routedTask"].string
        routedSeq = j["routedSeq"].int
        deliveredSeq = j["deliveredSeq"].int
    }
}

/// Something in front of the person.
struct InboxItem: Identifiable {
    /// What it belongs to: a decision the backend waits on (by its key), or a session whose turn
    /// ended and waits for the person to reply. Never read from the shape of a string.
    enum Ref: Hashable {
        case decision(String)
        case session(SessionTarget)
    }
    let kind: String
    let ref: Ref
    let seq: Int
    var task: String?
    var agent: String?
    /// The rest of the request: reason, recommend, question, options, schema, url, command …
    var d: JSON = [:]
    var id: Ref { ref }
    /// The backend's key of the decision it answers; empty for a session's turn.
    var key: String { if case .decision(let k) = ref { return k } else { return "" } }

    init(_ j: JSON) {
        kind = j["kind"].str
        seq = j["seq"].int ?? 0
        task = j["task"].string
        agent = j["agent"].string
        d = j["d"]
        if kind == "session" {
            ref = .session(task.map(SessionTarget.task) ?? .agent(agent ?? ""))
        } else {
            ref = .decision(j["key"].str)
        }
    }
}

struct RunInfo {
    var status = RunStatus.unknown
    var goal = ""
    /// Stored paths of the images pasted with the goal.
    var images: [String] = []
    var start: Double = 0
    var now: Double = 0
    var paused = false
    var cwd = ""
    /// The repository this run's own worktree was made from; empty for a run working in place.
    var repo = ""
    /// The branch a run in a worktree started on, where accepting merges its result back; empty
    /// when it started on a detached HEAD.
    var branch = ""
    /// Set once an acceptance merged a result back.
    var merged = false
    var git = false
    var baseRef = ""
    var workspace = ""
    var error = ""
    /// A conversation with one digital employee rather than a goal: no board, no delivery, and
    /// its one member is not a lead.
    // @project-doc docs/domains/20261120-chats.md#chat_app
    var chat: Bool { kind == .chat }
    /// A workspace Loop's session (`kind` `loop`): one member like a chat, woken on a rhythm.
    var loop: Bool { kind == .loop }
    var kind = RunKind.goal
    /// One session and no board: a chat or a Loop. Waiting for the next message is its normal
    /// state, and its session opening again means it is going.
    var singleSession: Bool { chat || loop }

    init() {}

    init(_ j: JSON) {
        status = RunStatus(j["status"].str)
        goal = j["goal"].str
        images = j["images"].strings
        start = j["start"].double ?? 0
        now = j["now"].double ?? 0
        paused = j["paused"].bool ?? false
        cwd = j["cwd"].str
        repo = j["repo"].str
        branch = j["branch"].str
        merged = j["merged"].bool ?? false
        git = j["git"].bool ?? false
        baseRef = j["baseRef"].str
        workspace = j["workspace"].str
        error = j["error"].str
        kind = RunKind(j["kind"].str)
    }
}

/// One delivery (`run.delivered`): what the lead handed over at the end of a round.
struct RunResult {
    let d: JSON
    let seq: Int
    let accepted: Double?
    /// Which delivery of the run it is, from 1.
    let n: Int
    /// The seq of the delivery before it (0 for the first): its round is what happened after that.
    let since: Int

    init(_ j: JSON) {
        d = j["d"]
        seq = j["seq"].int ?? 0
        accepted = j["accepted"].double
        n = j["n"].int ?? 1
        since = j["since"].int ?? 0
    }
}

struct RunState {
    var run = RunInfo()
    var tasks: [String: TaskState] = [:]
    var order: [String] = []
    var agents: [String: AgentState] = [:]
    var inbox: [InboxItem] = []
    var artifacts: [Artifact] = []
    var answers: [String: JSON] = [:]
    var msgs: [String: Msg] = [:]
    /// Every delivery, oldest first; the last is `result`.
    var deliveries: [RunResult] = []
    /// The latest delivery, the one under review.
    var result: RunResult? { deliveries.last }
    /// The lead's own turn in its conversation, outside any task.
    var leadTurn = SessionTurn.opened
    var board = BoardFacts()
    /// Where the latest delivery's review stands; nil before the first one, and for a chat or a Loop.
    var review: ServedReview?
    /// Each conversation as the server serves it, by `SessionTarget.label` (`RunState.session`).
    var sessions: [String: JSON] = [:]
    /// The file changes the conversations' events carry, by event seq, in their order there.
    var diffs: [Int: [ServedDiff]] = [:]
    /// Each event of the conversations in one line, by seq, as the server words it.
    var lines: [Int: ServedLine] = [:]
    /// The card of each request waiting on the person, in the order they wait.
    var cards: [ServedCard] = []
    /// The run's Markdown documents that exist.
    var docs: [RunDoc] = []
    /// The run card's status, as the server words it.
    var capsule = (text: "", tone: Tone.gray)
    /// The team panel's rows, in the team's order.
    var members: [MemberLine] = []
    /// The audit page's system rules over the log: what each says, whether it is kept, and what it
    /// was checked over.
    var audit: [(String, Bool, String)] = []
    /// Each task's acceptance evidence, by its id, as the server groups it.
    var evidence: [String: TaskEvidence] = [:]
    /// The latest delivery's evidence: the round's numbers and each task's row.
    var delivery: DeliveryEvidence?
    /// What can be done to the whole run now, as the server answers it.
    var allowed = RunActions()
    /// What the run is called, as the lists call it.
    var title = ""
    /// Nothing can be done in it any more: a stopped run the service no longer holds.
    var readOnly = false
    /// At work at this position (its lead starting, or running), as the server says (`active`).
    var atWork = false
    /// A finished consolidation of the team memory, as the server says it (`saved`, `forgotten`, `text`); `null` before.
    var consolidated: JSON = .null
    /// Above the run card while it stopped short and can go on.
    var resumeNote: String?
    /// The run page's own lines and lists around the board, as the server writes them.
    var page = ServedPage()
    /// The checks events carry (a result, a verification), by seq, as the server writes them.
    var checks: [Int: [CheckView]] = [:]
    /// Each task's topic, who is at work and what waits on the person, beside the group.
    var feed = Feed()
    /// The newest event the server folded in.
    var seq = 0
    /// The events shown with it, the run's log up to the shown position (the app's own copy).
    var events: [Event] = []

    init() {}

    init(_ j: JSON, events: [Event]) {
        run = RunInfo(j["run"])
        let ts = j["tasks"].array.map(TaskState.init)
        order = ts.map(\.id)
        tasks = Dictionary(ts.map { ($0.id, $0) }, uniquingKeysWith: { _, b in b })
        agents = j["agents"].object.mapValues(AgentState.init)
        inbox = j["inbox"].array.map(InboxItem.init)
        artifacts = j["artifacts"].array.map(Artifact.init)
        answers = j["answers"].object
        msgs = Dictionary(j["msgs"].array.map(Msg.init).map { ($0.id, $0) }, uniquingKeysWith: { _, b in b })
        deliveries = j["deliveries"].array.map(RunResult.init)
        leadTurn = SessionTurn(j["leadTurn"])
        board = BoardFacts(j["board"])
        review = j["review"].isNull ? nil : ServedReview(j["review"])
        sessions = j["sessions"].object
        diffs = Dictionary(j["diffs"].object.compactMap { k, v in Int(k).map { ($0, v.array.map(ServedDiff.init)) } }, uniquingKeysWith: { a, _ in a })
        docs = j["docs"].array.map(RunDoc.init)
        lines = Dictionary(j["lines"].object.compactMap { k, v in Int(k).flatMap { s in ServedLine(v).map { (s, $0) } } }, uniquingKeysWith: { a, _ in a })
        cards = j["cards"].array.map(ServedCard.init)
        capsule = (j["capsule"]["text"].str, Tone(rawValue: j["capsule"]["tone"].str) ?? .gray)
        members = j["members"].array.map(MemberLine.init)
        audit = j["audit"].array.map { ($0["text"].str, $0["ok"].bool ?? false, $0["note"].str) }
        evidence = j["evidence"].object.mapValues(TaskEvidence.init)
        delivery = j["delivery"].isNull ? nil : DeliveryEvidence(j["delivery"])
        allowed = RunActions(answers: j["actions"].object)
        readOnly = j["readOnly"].bool ?? false
        atWork = j["active"].bool ?? false
        consolidated = j["consolidated"]
        title = j["title"].str
        resumeNote = j["resumeNote"].string
        page = ServedPage(j["page"])
        checks = Dictionary(j["checks"].object.compactMap { k, v in Int(k).map { ($0, CheckView.list(v)) } }, uniquingKeysWith: { a, _ in a })
        feed = Feed(j["feed"])
        seq = j["seq"].int ?? 0
        self.events = events
    }

    var orderedTasks: [TaskState] { order.compactMap { tasks[$0] } }
    /// The one-click answers the server gives for request `item`.
    func actions(of item: InboxItem) -> [QuickAction] {
        board.open.first { $0.you && $0.seq == item.seq && $0.kind == item.kind }?.actions ?? []
    }
    func event(_ seq: Int) -> Event? { seq >= 1 && seq <= events.count && events[seq - 1].seq == seq ? events[seq - 1] : events.first { $0.seq == seq } }

    var active: Bool { !run.goal.isEmpty && !run.status.isFinished }
}

/// One member's row in the team panel, as the server works it out (`backend/src/run_view/card.rs`):
/// what it is doing, what it is on, and how its work in this run went.
struct MemberLine {
    let id: String
    let lead: Bool
    let phase: AgentPhase
    let line: String
    /// Its newest event while it works.
    let beat: Double?
    /// 「本次 2/3 首次通过 · 验证没通过 1」, for a collaborator whose tasks ended.
    let stats: String?
    let progress: Double?

    init(_ j: JSON) {
        id = j["id"].str
        lead = j["lead"].bool ?? false
        phase = AgentPhase(j["phase"].str)
        line = j["line"].str
        beat = j["beat"].double
        stats = j["stats"].string
        progress = j["progress"].double
    }
}

/// What can be done to the whole run now, as the server answers it (`backend/src/run_ops.rs`):
/// each action by its `type` (`pause`, `resume`, `cancel`, `start`, `accept`, `resumeRun`,
/// `delete`), and why not. At a replayed position nothing can be done.
struct RunActions {
    var answers: [String: JSON] = [:]
    func ok(_ op: String) -> Bool { answers[op]?["ok"].bool ?? false }
    /// Why `op` cannot be done now; empty when it can.
    func why(_ op: String) -> String { answers[op]?["why"].str ?? "" }
}

// MARK: - Shared board

/// Something that holds work up until someone acts, as the server words it.
struct OpenFact: Identifiable {
    let id: String
    let seq: Int?
    let title: String
    let text: String
    let tasks: [String]
    let agents: [String]
    let you: Bool
    let sub: String
    /// What it is (`question`, `approval`, `input`, `session`, `lead_stall`, `conflict` …).
    let kind: String
    /// Its kind in a word or two.
    let label: String
    /// Choices that answer the request in one click, where it is a plain choice.
    let actions: [QuickAction]

    init(_ j: JSON) {
        id = j["key"].str
        seq = j["seq"].int
        title = j["title"].str
        text = j["text"].str
        tasks = j["tasks"].strings
        agents = j["agents"].strings
        you = j["you"].bool ?? false
        sub = j["sub"].str
        kind = j["kind"].str
        label = j["label"].str
        actions = j["actions"].array.map(QuickAction.init)
    }
}

/// One click that settles a request: the request body the server prepared, sent as it is.
struct QuickAction: Equatable {
    let label: String
    let help: String
    let prominent: Bool
    let body: JSON
    /// An approval option's kind (`allow_once`, `allow_always` …); empty for an answer.
    let kind: String

    init(_ j: JSON) {
        label = j["label"].str
        help = j["help"].str
        prominent = j["prominent"].bool ?? false
        body = j["body"]
        kind = j["kind"].str
    }
}

/// What the shared board holds up, as the server words it; the board's results, handoffs and
/// decisions reach the app as the group's posts (`Feed`).
struct BoardFacts {
    var open: [OpenFact] = []

    init() {}

    init(_ j: JSON) {
        open = j["open"].array.map(OpenFact.init)
    }
}
/// What the run page shows around the board, as the server writes it (`page` of the view,
/// `backend/src/run_view/page.rs`): the delivery in one line and as text to copy, what the round
/// changed, the deliveries before, the inspector's lists, each task page's trace and setting.
struct ServedPage {
    struct Earlier { let n: Int; let seq: Int; let line: String; let accepted: Bool }
    struct TaskPage {
        var trace: [TraceRow] = []
        var downstream: [String] = []
        var structure = ""
        var env: [(String, String)] = []
    }

    var reviewLine: String?
    var summary: String?
    var progress: String?
    var diff: Artifact?
    var others: [JSON] = []
    var earlier: [Earlier] = []
    var diffs: [Artifact] = []
    var files: [Artifact] = []
    var delivered: [RunDoc] = []
    var tasks: [String: TaskPage] = [:]
    var workspaceNote = ""
    var branchNote: String?

    init() {}

    init(_ j: JSON) {
        reviewLine = j["reviewLine"].string
        summary = j["summary"].string
        progress = j["progress"].string
        diff = j["changes"]["diff"].isNull ? nil : Artifact(j["changes"]["diff"])
        others = j["changes"]["others"].array
        earlier = j["earlier"].array.map { Earlier(n: $0["n"].int ?? 0, seq: $0["seq"].int ?? 0, line: $0["line"].str, accepted: $0["accepted"].bool ?? false) }
        diffs = j["inspector"]["diffs"].array.map(Artifact.init)
        files = j["inspector"]["files"].array.map(Artifact.init)
        delivered = j["inspector"]["delivered"].array.map(RunDoc.init)
        tasks = j["tasks"].object.mapValues { t in
            TaskPage(trace: t["trace"].array.map(TraceRow.init), downstream: t["downstream"].strings, structure: t["structure"].str,
                     env: t["env"].array.map { ($0["label"].str, $0["value"].str) })
        }
        workspaceNote = j["team"]["workspace"].str
        branchNote = j["team"]["branch"].string
    }
}

/// One row of a task's execution trace, as the server writes it (`TraceRow` in
/// `backend/src/run_view/page.rs`): an attempt's header, or an event with its mark, who did it, its
/// text and what it shows under that.
struct TraceRow: Identifiable {
    struct Attempt { let title, runtime, session, place: String }
    struct Detail { let kind, text: String; let tone: Tone; let checks: [CheckView] }

    let seq: Int
    let ts: Double
    let attempt: Attempt?
    let mark: EventMark
    let who: String
    let text: String
    let detail: Detail?
    var id: Int { seq }

    init(_ j: JSON) {
        seq = j["seq"].int ?? 0
        ts = j["ts"].double ?? 0
        let a = j["attempt"]
        attempt = a.isNull ? nil : Attempt(title: a["title"].str, runtime: a["runtime"].str, session: a["session"].str, place: a["place"].str)
        mark = EventMark(j["mark"])
        who = j["who"].str
        text = j["text"].str
        let d = j["detail"]
        detail = d.isNull ? nil : Detail(kind: d["kind"].str, text: d["text"].str, tone: Tone(rawValue: d["tone"].str) ?? .gray, checks: CheckView.list(d["checks"]))
    }
}

/// An event kind's symbol and colour, as the server names them (`event_mark`).
struct EventMark {
    let icon: String
    let tone: Tone?

    init(_ j: JSON) {
        icon = j["icon"].str
        tone = j["tone"].string.flatMap(Tone.init(rawValue:))
    }
}
