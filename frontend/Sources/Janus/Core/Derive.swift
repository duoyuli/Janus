import Foundation

// The run page keeps no business state of its own: every view is derived from the event log by
// `derive` and `boardFacts`. A new or changed event type must be handled here as well
// (docs/architecture/20261031-frontend.md#event_derivation).

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

final class Attempt {
    let n: Int
    let session: String
    let workspace: String
    let start: Double
    var status: AttemptStatus
    let seq: Int
    var error: String?
    /// When the attempt stopped running (succeeded, failed, cancelled, interrupted); nil while it runs.
    var end: Double?
    init(n: Int, session: String, workspace: String, start: Double, status: AttemptStatus, seq: Int) {
        self.n = n; self.session = session; self.workspace = workspace; self.start = start; self.status = status; self.seq = seq
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

    /// A diff to review: a task's (`git_diff`) or the whole run's against where it started (`run_diff`).
    var isDiff: Bool { kind == "git_diff" || ofRun }
    /// The whole run's diff: everything accepting brings back, the lead's own edits included.
    var ofRun: Bool { kind == "run_diff" }
    /// What its review is about, in the message to the agent (Janus talks to agents in Chinese).
    var subject: String { ofRun ? "整个任务的改动" : "\(task) 的改动" }
}

struct Question {
    let key: String
    let question: String
    let options: [String]
    let blocking: Bool
    let seq: Int
    let from: String
    var answer: String?
}

/// Whose turn an open session is in, and whether that turn has ended (the session waits for a
/// message). One rule for every session, applied event by event (`after`): the task's own and the
/// agent page's are the same state. A closed session is no `SessionTurn` at all (`TaskState.session`
/// is nil), so "the person holds a closed session" cannot be written down.
enum SessionTurn: Equatable {
    case agent(idle: Bool)
    case person(idle: Bool)

    /// A session that just opened: the agent at work.
    static let opened = SessionTurn.agent(idle: false)

    var human: Bool { if case .person = self { return true } else { return false } }
    var idle: Bool { switch self { case .agent(let i), .person(let i): return i } }

    private func with(idle: Bool) -> SessionTurn { human ? .person(idle: idle) : .agent(idle: idle) }

    /// The turn after `e`.
    func after(_ e: Event) -> SessionTurn {
        let d = e.data
        switch e.type {
        case .attemptStarted, .runCreated, .controlReleased: return .opened
        case .controlTaken: return .person(idle: idle)
        case .sessionInterrupted: return d["finish"].truthy ? self : .person(idle: idle)
        case .agentStatus where d["turn"].str == "idle": return with(idle: true)
        case .userMessage, .messageRouted: return MessageDelivery(d["delivery"].str) == .prompt ? with(idle: false) : self
        case .messageDelivered: return with(idle: false)
        default: return self
        }
    }
}

final class TaskState {
    let id: String
    var title, desc, agent: String
    var deps: [String]
    var accept: [String]
    var status = TaskStatus.pending
    var attempts: [Attempt] = []
    var progress: Double = 0
    var artifacts: [Artifact] = []
    /// Its open session's turn; nil while it has none.
    var session: SessionTurn?
    var live: Bool { session != nil }
    /// The person holds the session (takeover or interrupt).
    var human: Bool { session?.human ?? false }
    /// The session's turn has ended and it waits for a message.
    var idle: Bool { session?.idle ?? false }
    var blockedBy = ""
    var checks: [Check] = []
    var verificationFailures: [String] = []
    var error = ""
    var question: Question?
    var amendments: [String] = []
    var summary = ""
    var evidence: [String] = []
    /// The risks the executor reported with its result.
    var risks: [String] = []
    /// Files the task owns: from its orchestration, plus what it claimed while working.
    var owns: [String] = []
    var verifies = ""
    var continues = ""
    var handoff = ""
    var verdict: Verdict?
    /// Where the finished result disagrees with what the task left in the workspace.
    var traceGaps: [String] = []

    init(_ x: JSON) {
        id = x["id"].str
        title = x["title"].str
        desc = x["desc"].str
        agent = x["agent"].str
        deps = x["deps"].strings
        accept = x["accept"].strings
        owns = x["owns"].strings
        verifies = x["verifies"].str
        continues = x["continues"].str
    }
}

final class AgentState {
    var phase = AgentPhase.idle
    var note = ""
    var progress: Double?
    var beat: Double = 0
    var session: String?
}

/// A message sent to a session, tracked by id from `pending` to its delivery.
final class Msg {
    let id: String
    let text: String
    let by: String
    let to: String
    let task: String?
    let seq: Int
    /// Stored paths of the images sent with it; `text` still names them (see `Attach.visibleText`).
    var images: [String] = []
    var delivery = MessageDelivery.pending
    var routedTask: String?
    var routedSeq: Int?
    var deliveredSeq: Int?
    init(id: String, text: String, by: String, to: String, task: String?, seq: Int, images: [String] = []) {
        self.id = id; self.text = text; self.by = by; self.to = to; self.task = task; self.seq = seq; self.images = images
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
    /// A conversation with one digital employee rather than a goal (`run.created` `data.kind`): no
    /// board, no delivery, and its one member is not a lead.
    // @project-doc docs/domains/20261120-chats.md#chat_app
    var chat: Bool { kind == .chat }
    /// A workspace Loop's session (`kind` `loop`): one member like a chat, woken on a rhythm.
    var loop: Bool { kind == .loop }
    var kind = RunKind.goal
    /// One session and no board: a chat or a Loop. Waiting for the next message is its normal
    /// state, and its session opening again means it is going.
    var singleSession: Bool { chat || loop }
}

/// One delivery (`run.delivered`): what the lead handed over at the end of a round.
struct RunResult {
    let d: JSON
    let seq: Int
    var accepted: Double?
    /// Which delivery of the run it is, from 1.
    var n = 1
    /// The seq of the delivery before it (0 for the first): its round is what happened after that.
    var since = 0
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
    /// The latest delivery, the one under review.
    var result: RunResult?
    /// Every delivery, oldest first; the last is `result`.
    var deliveries: [RunResult] = []
    /// The lead's own turn in its conversation (`SessionTurn` over its events outside any task):
    /// after a delivery it tells whether it works on what the person asked, or the person holds it.
    var leadTurn = SessionTurn.opened
    var events: [Event] = []

    var orderedTasks: [TaskState] { order.compactMap { tasks[$0] } }
    func event(_ seq: Int) -> Event? { seq >= 1 && seq <= events.count && events[seq - 1].seq == seq ? events[seq - 1] : events.first { $0.seq == seq } }

    var active: Bool { !run.goal.isEmpty && !run.status.isFinished }

    /// The tasks that verify `task` independently; their conclusion is shown apart from the
    /// evidence the executor reported.
    func verifiers(of task: String) -> [TaskState] { orderedTasks.filter { $0.verifies == task } }
}

// @project-doc docs/architecture/20261031-frontend.md#event_derivation
func derive(_ events: [Event]) -> RunState {
    var s = RunState()
    s.events = events
    func ag(_ id: String) -> AgentState {
        if let a = s.agents[id] { return a }
        let a = AgentState()
        s.agents[id] = a
        return a
    }
    func dropSess(_ target: SessionTarget) { s.inbox.removeAll { $0.ref == .session(target) } }
    func endLive(_ t: TaskState?) {
        guard let t else { return }
        t.session = nil; dropSess(.task(t.id))
        if let at = t.attempts.last, at.end == nil { at.end = s.run.now }
    }
    func dropKey(_ k: String) { s.inbox.removeAll { $0.ref == .decision(k) } }

    for e in events {
        s.run.now = e.ts
        if e.task == nil && (e.actor == "lead" || e.to == "lead") { s.leadTurn = s.leadTurn.after(e) }
        if !e.actor.isEmpty && e.actor != "user" && e.actor != "janus" { ag(e.actor).beat = e.ts }
        let t = e.task.flatMap { s.tasks[$0] }
        let d = e.data
        switch e.type {
        case .runQueued:
            // Waiting for a slot: what it will do is known, nothing of it has started.
            s.run.status = .queued
            s.run.goal = d["goal"].str
            s.run.images = Attach.paths(d)
            s.run.start = e.ts
        case .runStarting: s.run.status = .planning
        case .runRequeued: s.run.status = .queued
        case .runCreated:
            s.run.status = .running
            s.run.goal = d["goal"].str
            s.run.images = Attach.paths(d)
            s.run.start = e.ts
            s.run.cwd = d["cwd"].str
            s.run.repo = d["worktree"]["repo"].str
            s.run.branch = d["worktree"]["branch"].str
            s.run.git = d["git"].truthy
            s.run.baseRef = d["baseRef"].str
            s.run.workspace = d["workspace"].str
            s.run.kind = RunKind(d["kind"].str)
            ag("lead").phase = .thinking
        case .orchestrationAccepted:
            for x in d["tasks"].array {
                let nt = TaskState(x)
                if s.tasks[nt.id] == nil { s.order.append(nt.id) }
                s.tasks[nt.id] = nt
            }
            ag("lead").phase = .coordinating
            // New work after a delivery: the run is running again until it is delivered anew, so
            // the earlier delivery is not offered for acceptance meanwhile (FL-10).
            if s.run.status == .delivered || s.run.status == .accepted { s.run.status = .running }
        case .goalStatus: ag("lead").note = e.text
        case .decisionWithdrawn:
            dropKey(d["key"].str)
            if let t, t.status.waitsOnPerson { t.status = .running }
        case .runCancelled:
            s.run.status = .cancelled; s.inbox = []
            for x in s.orderedTasks {
                x.session = nil; if x.status != .succeeded { x.status = .cancelled }
                if let at = x.attempts.last, at.end == nil { at.end = e.ts }
            }
        case .runPaused: s.run.paused = true
        case .runResumed: s.run.paused = false
        case .resultSubmitted:
            t?.checks = Check.list(d["checks"])
            t?.risks = d["risks"].strings
            t?.handoff = d["handoff"].str
            if let vd = d["verdict"].string.flatMap(Verdict.init(rawValue:)) { t?.verdict = vd }
        case .verificationStarted: t?.status = .verifying
        // The checks and their methods the backend checked; `task.succeeded` repeats them with the
        // conclusion, so a replayed or resumed run still shows them from one place.
        case .verificationPassed, .verificationFailed:
            guard let t else { break }
            if !d["checks"].array.isEmpty { t.checks = Check.list(d["checks"]) }
            if e.type == .verificationFailed {
                t.status = .verificationFailed; t.verificationFailures = d["failures"].strings
            }
        case .taskReady:
            let ids = d["tasks"].exists ? d["tasks"].strings : [e.task].compactMap { $0 }
            for id in ids { s.tasks[id]?.status = .ready }
        case .taskAssigned: t?.status = .assigned
        case .taskBlocked: t?.status = .blocked; t?.blockedBy = d["dependency"].str
        case .taskSkippedByDecision: t?.status = .skippedByDecision; endLive(t)
        case .attemptStarted:
            guard let t else { break }
            if let prev = t.attempts.last, prev.end == nil { prev.end = e.ts }
            t.attempts.append(Attempt(n: e.attempt ?? t.attempts.count + 1, session: d["session"].str, workspace: d["workspace"].str, start: e.ts, status: .running, seq: e.seq))
            t.session = .opened; t.status = .running; t.progress = 5
            let a = ag(t.agent); a.session = d["session"].string; a.phase = .thinking; a.note = ""; a.progress = 5
        case .agentStatus:
            let a = ag(e.actor)
            a.phase = AgentPhase(d["phase"].str); a.note = e.text; a.progress = d["progress"].double
            if d["session"].exists { a.session = d["session"].string }
            if let t, let p = d["progress"].double { t.progress = p }
            // A chat's session opened (again): the conversation is going, whatever ended the last one.
            if s.run.singleSession && d["session"].exists { s.run.status = .running; s.run.error = "" }
            // A chat (or a Loop) waiting for the next message is its normal state, not something to handle.
            if d["turn"].str == "idle" && !s.run.singleSession {
                if let t {
                    t.session = t.session?.after(e)
                    if !s.inbox.contains(where: { $0.ref == .session(.task(t.id)) }) { s.inbox.append(InboxItem(kind: "session", ref: .session(.task(t.id)), seq: e.seq, task: t.id)) }
                } else if !(e.actor == "lead" && s.result != nil), !s.inbox.contains(where: { $0.ref == .session(.agent(e.actor)) }) {
                    // After a delivery the review says where the lead is (`reviewStage`): waiting
                    // for acceptance, or done with what the person asked, so its turn ending is not
                    // a second entry.
                    s.inbox.append(InboxItem(kind: "session", ref: .session(.agent(e.actor)), seq: e.seq, agent: e.actor))
                }
            }
        case .toolStarted:
            let a = ag(e.actor); a.phase = JanusTool(call: d["tool"].str) == .wait ? .waitingForAgent : .runningTool; a.note = e.text
        case .toolCompleted:
            if [.wait, .reply, .orchestrate].contains(JanusTool(call: d["tool"].str)) { let a = ag(e.actor); a.phase = e.actor == "lead" ? .coordinating : .thinking; a.note = e.text }
        case .controlTaken, .sessionInterrupted: t?.session = t?.session?.after(e)
        case .controlReleased:
            if let t { t.session = t.session?.after(e); dropSess(.task(t.id)) } else if let to = e.to { dropSess(.agent(to)) }
        case .userMessage:
            if let id = d["id"].string {
                s.msgs[id] = Msg(id: id, text: d["text"].str, by: e.actor, to: e.to ?? "", task: e.task, seq: e.seq, images: Attach.paths(d))
                break
            }
            if let t, MessageDelivery(d["delivery"].str) == .prompt { t.session = t.session?.after(e); dropSess(.task(t.id)); ag(t.agent).phase = .thinking }
            else if t == nil, MessageDelivery(d["delivery"].str) == .prompt, let to = e.to { dropSess(.agent(to)) }
        case .messageRouted:
            if let m = s.msgs[d["id"].str] { m.delivery = MessageDelivery(d["delivery"].str) ?? m.delivery; m.routedTask = e.task; m.routedSeq = e.seq }
            if MessageDelivery(d["delivery"].str) == .prompt {
                if let t { t.session = t.session?.after(e); dropSess(.task(t.id)); ag(t.agent).phase = .thinking } else if let to = e.to { dropSess(.agent(to)) }
            }
        case .messageDelivered:
            t?.session = t?.session?.after(e)
            for id in d["ids"].strings { s.msgs[id]?.deliveredSeq = e.seq }
        case .questionAsked:
            guard let t else { break }
            t.status = .waitingInput
            t.question = Question(key: d["key"].str, question: d["question"].str, options: d["options"].strings, blocking: d["blocking"].truthy, seq: e.seq, from: e.actor)
            let a = ag(e.actor); a.phase = .waitingForInput; a.note = e.text
        // A collaborator's question passed on to the person, or a decision the lead hands over itself (no task).
        case .questionEscalated:
            dropKey(d["key"].str)
            var item = InboxItem(kind: "question", ref: .decision(d["key"].str), seq: e.seq, task: e.task, d: ["reason": d["reason"], "recommend": d["recommend"]])
            if e.task == nil { item.d["question"] = d["question"]; item.d["options"] = d["options"].or([]); item.d["from"] = JSON(e.actor) }
            s.inbox.append(item)
        case .questionAnswered:
            s.answers[d["key"].str] = d["answer"]; dropKey(d["key"].str)
            guard let t else { break }
            if t.status == .waitingInput { t.status = .running }
            t.question?.answer = d["answer"].str
            ag(t.agent).phase = .thinking
        case .approvalRequested:
            t?.status = .waitingApproval
            s.inbox.append(InboxItem(kind: "approval", ref: .decision(d["key"].str), seq: e.seq, task: e.task, agent: e.actor, d: d))
            let a = ag(e.actor); a.phase = .waitingApproval; a.note = e.text
        case .inputRequested:
            t?.status = .waitingInput
            s.inbox.append(InboxItem(kind: "input", ref: .decision(d["key"].str), seq: e.seq, task: e.task, agent: e.actor, d: d))
            let a = ag(e.actor); a.phase = .waitingForInput; a.note = e.text
        case .inputAnswered:
            s.answers[d["key"].str] = d["action"]; dropKey(d["key"].str)
            if let t, t.status == .waitingInput { t.status = .running }
            if let to = e.to { ag(to).phase = .thinking }
        case .approvalGranted, .approvalDenied:
            s.answers[d["key"].str] = JSON(e.type == .approvalGranted ? "granted" : "denied"); dropKey(d["key"].str)
            if let t, t.status == .waitingApproval { t.status = .running; ag(t.agent).phase = .runningTool }
        case .taskFailed:
            guard let t else { break }
            // The lead deals with a failure (it reads it on the board), so nothing waits for the person.
            t.status = .failed; t.error = d["error"].str
        case .runFailed: s.run.status = .failed; s.run.error = d["error"].string ?? e.text
        case .runInterrupted:
            // A Loop's questions outlive its session: the answer wakes it whenever it comes.
            s.run.status = .interrupted; s.inbox = s.run.loop ? s.inbox.filter { $0.kind == "question" } : []
            for x in s.orderedTasks { x.session = nil; if let at = x.attempts.last, at.end == nil { at.end = e.ts } }
            for a in s.agents.values { a.phase = .idle }
        case .sessionParked:
            // The lead's agent closed while idle to give its seat back: a chat or a Loop's session
            // reads as closed, as after a restart; a goal keeps its status. The next message opens it.
            if s.run.chat || s.run.loop { s.run.status = .interrupted }
            let a = ag("lead"); a.phase = .idle; a.note = e.text
        case .runRecovered:
            s.run.status = RunStatus(d["phase"].str)
            // A resumed run schedules again: whatever pause came before the restart is over (FL-12).
            s.run.paused = false
            for x in d["tasks"].array {
                guard let rt = s.tasks[x["id"].str] else { continue }
                rt.status = TaskStatus(x["status"].str); rt.blockedBy = x["blockedBy"].str; rt.session = nil
            }
        case .artifactCreated:
            let a = Artifact(name: d["name"].str, kind: d["kind"].str, path: d["path"].str, desc: d["desc"].str, hash: d["hash"].str, bytes: d["bytes"].int, task: e.task ?? "", attempt: e.attempt ?? 0, seq: e.seq, by: e.actor)
            s.artifacts.append(a); t?.artifacts.append(a)
        case .attemptFailed:
            guard let t else { break }
            endLive(t)
            if let at = t.attempts.last { at.status = .failed; at.error = d["error"].str }
            t.status = .ready
            let a = ag(t.agent); a.phase = .error; a.note = d["error"].str; a.progress = nil
        case .resultAmended: t?.amendments.append(d["text"].str)
        case .sessionPrompt: if let t, PromptChannel(rawValue: d["channel"].str) == .fix { t.status = .running }
        case .taskReopened:
            guard let t else { break }
            t.amendments = []; t.traceGaps = []; t.verdict = nil; t.handoff = ""
            if let ins = d["instructions"].string, !ins.isEmpty { t.desc = ins }
            // Rework with revised criteria: the new checks are about these, not the old ones (FL-11).
            if d["accept"].array.count > 0 { t.accept = d["accept"].strings }
            // Tasks held by this one's failure wait for it again instead of showing as stuck.
            for x in s.orderedTasks where x.status == .blocked && x.blockedBy == t.id { x.status = .pending }
            t.status = .ready
            if s.run.status.isDone { s.run.status = .running }
        case .taskSucceeded:
            guard let t else { break }
            t.status = .succeeded; t.progress = 100; t.summary = d["summary"].str; t.evidence = d["evidence"].strings
            t.handoff = d["handoff"].str
            if let vd = d["verdict"].string.flatMap(Verdict.init(rawValue:)) { t.verdict = vd }
            // The conclusion carries the whole result: its checks with their methods and its risks.
            t.checks = Check.list(d["checks"]); t.risks = d["risks"].strings
            if d["acceptedBy"].str != "lead" { t.traceGaps = d["traceGaps"].strings }
            endLive(t)
            if let at = t.attempts.last, at.status == .running { at.status = .succeeded }
            let a = ag(t.agent); a.phase = t.agent == "lead" ? .coordinating : .idle; a.note = ""; a.progress = nil
        case .runDelivered:
            let r = RunResult(d: d, seq: e.seq, n: s.deliveries.count + 1, since: s.deliveries.last?.seq ?? 0)
            s.run.status = .delivered; s.result = r; s.deliveries.append(r)
            let a = ag("lead"); a.phase = .idle; a.note = ""
            s.leadTurn = .agent(idle: true)
            dropSess(.agent("lead"))
        case .riskAccepted:
            s.run.status = .accepted; s.result?.accepted = e.ts
            if !s.deliveries.isEmpty { s.deliveries[s.deliveries.count - 1].accepted = e.ts }
            if d["commit"].string != nil { s.run.merged = true }
        // A delivery brought the result to the person's branch.
        case .runMerged: if d["commit"].string != nil { s.run.merged = true }
        default: break
        }
    }
    return s
}

// MARK: - Shared board

/// Which open item a fact is, built from typed parts so two never share an id: a task id can be
/// any string the board allows, so ids made by gluing a prefix, a task and a number together
/// could collide (`T1` + `23` and `T12` + `3`).
enum OpenKey: Hashable {
    /// The person's own items. Their text is what the inbox remembers a snooze or an ignore by,
    /// so it stays as it was.
    case request(seq: Int, kind: String), loop(seq: Int), leadStall(seq: Int)
    case conflict(seq: Int), split(task: String, seq: Int), gap(seq: Int), stall(task: String, seq: Int)
    case blocker(seq: Int), leadQuestion(task: String, seq: Int), failed(task: String), blocked(task: String)

    /// A string id: task ids never hold `:` (letters, digits, `-` and `_`), so it is unique too.
    var text: String {
        switch self {
        case .request(let seq, let kind): return "o\(seq)\(kind)"
        case .loop(let seq): return "oloop\(seq)"
        case .leadStall(let seq): return "olead\(seq)"
        case .conflict(let seq): return "conflict:\(seq)"
        case .split(let task, let seq): return "split:\(task):\(seq)"
        case .gap(let seq): return "gap:\(seq)"
        case .stall(let task, let seq): return "stall:\(task):\(seq)"
        case .blocker(let seq): return "blocker:\(seq)"
        case .leadQuestion(let task, let seq): return "lead_question:\(task):\(seq)"
        case .failed(let task): return "failed:\(task)"
        case .blocked(let task): return "blocked:\(task)"
        }
    }
}

struct OpenFact: Identifiable {
    let key: OpenKey
    let seq: Int?
    let title: String
    let text: String
    let tasks: [String]
    let agents: [String]
    let you: Bool
    var sub: String = ""
    /// For the person's items: the request kind (`question`, `approval`, `input`, `budget`,
    /// `session`) or `lead_stall`, so the inbox knows which can wait.
    var kind: String = ""
    /// Choices that answer the request in one click, where it is a plain choice: an approval's
    /// options, or a question's options. A form or a free-form answer has none.
    var actions: [QuickAction] = []
    var id: String { key.text }
}

/// One click that settles a request.
struct QuickAction: Equatable {
    let label: String
    var help = ""
    var prominent = false
    let action: RunAction
    var body: JSON { action.body }
}

struct ResultFact: Identifiable {
    let key: String, seq: Int, ts: Double, by: String, task: String
    let summary: String
    let evidence: [String], risks: [String]
    let checks: [Check]
    var usedBy: [String] = []
    var old = false
    var amended: [String] = []
    var id: String { key }
}

struct HandoffFact: Identifiable {
    let key: String, seq: Int, ts: Double, by: String
    let from: String, to: String, agent: String
    let summary: String
    let artifacts: [String]
    var id: String { key }
}

struct ArtFact: Identifiable {
    let a: Artifact
    let old: Bool
    let usedBy: [String]
    var id: String { "a\(a.seq)" }
}

struct DecisionFact: Identifiable {
    let key: String, seq: Int, ts: Double, by: String
    let title: String
    let text: String
    var tasks: [String] = []
    var agents: [String] = []
    var msg = false
    var to: String?
    var lost = false
    var id: String { key }
}

struct BoardFacts {
    var open: [OpenFact] = []
    var results: [ResultFact] = []
    var arts: [ArtFact] = []
    var handoffs: [HandoffFact] = []
    var decisions: [DecisionFact] = []
}

// @project-doc docs/architecture/20261031-frontend.md#event_derivation
func boardFacts(_ st: RunState, name: (String) -> String) -> BoardFacts {
    var f = BoardFacts()
    var notes: [String: [(seq: Int, text: String)]] = [:]
    var blockers: [(seq: Int, task: String, text: String, actor: String)] = []
    // The watchdog's latest report per task, until the backend says the agent moved again, the
    // agent itself acts, or a new attempt starts.
    var stalls: [String: (seq: Int, text: String)] = [:]
    // The lead's own stall goes to the person: nobody else watches the lead.
    var leadStall: (seq: Int, text: String)?
    // A Loop's last wake: files it changed in the workspace, or why it could not wake. The next
    // wake replaces it.
    var loopNotice: (seq: Int, kind: String, text: String, files: [String])?
    // Two running tasks that edited the same files, or one (or the lead) that edited files the other owns.
    var conflicts: [(seq: Int, task: String, other: String, paths: [String], owned: Bool, byLead: Bool)] = []
    // Collaborator split proposals waiting on the lead's decision.
    var splits: [String: (seq: Int, text: String)] = [:]
    // Finished results that disagree with what their task left in the workspace.
    var gaps: [String: (seq: Int, gaps: [String])] = [:]
    for e in st.events {
        let d = e.data
        if let task = e.task, stalls[task] != nil, e.actor == st.tasks[task]?.agent || e.type == .attemptStarted { stalls[task] = nil }
        // A loop report has no backend "resolved" event; the lead moving again is the resolution.
        if leadStall != nil, e.actor == "lead" { leadStall = nil }
        switch e.type {
        case .taskStalled: if let task = e.task { stalls[task] = d["resolved"].truthy ? nil : (seq: e.seq, text: e.text) }
        case .leadStalled: leadStall = d["resolved"].truthy ? nil : (seq: e.seq, text: e.text)
        case .loopWoke: loopNotice = nil
        case .loopTouched: loopNotice = (seq: e.seq, kind: "loop_touched", text: e.text, files: d["files"].strings)
        case .loopFailed: loopNotice = (seq: e.seq, kind: "loop_failed", text: e.text, files: [])
        case .taskConflict:
            if let task = e.task {
                conflicts.append((seq: e.seq, task: task, other: d["other"].str, paths: d["paths"].strings, owned: d["owned"].truthy, byLead: d["byLead"].truthy))
            }
        case .taskSplitRequested:
            let reason = d["reason"].string ?? e.text
            let count = d["subtasks"].array.count
            if let task = e.task { splits[task] = (seq: e.seq, text: reason) }
            f.handoffs.append(HandoffFact(key: "h\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, from: e.task ?? "", to: "lead", agent: "lead", summary: L.text("请求拆分（建议 \(count) 个子任务）：\(reason)"), artifacts: []))
        case .resultSubmitted:
            f.results.append(ResultFact(key: "r\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, task: e.task ?? "", summary: d["summary"].string ?? e.text, evidence: d["evidence"].strings, risks: d["risks"].strings, checks: Check.list(d["checks"])))
        case .contextHandoff:
            f.handoffs.append(HandoffFact(key: "h\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, from: d["from"].str, to: e.task ?? "", agent: e.to ?? "", summary: d["summary"].str, artifacts: d["artifacts"].strings))
        case .orchestrationAccepted:
            let ts = d["tasks"].array
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("负责人派出子任务"), text: ts.map { x in "\(x["id"].str) \(x["title"].str) → \(name(x["agent"].str))\(x["deps"].array.isEmpty ? "" : L.text("（依赖 \(x["deps"].strings.joined(separator: L.text("、")))）"))" }.joined(separator: "\n"), tasks: ts.map { $0["id"].str }, agents: ["lead"]))
        case .orchestrationRejected:
            let errs = d["errors"].strings
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("派工方案没通过校验"), text: errs.isEmpty ? e.text : errs.joined(separator: "\n"), agents: ["lead"]))
        case .boardPosted:
            let text = d["text"].string ?? e.text
            if d["kind"].str == "blocker" { blockers.append((seq: e.seq, task: e.task ?? "", text: text, actor: e.actor)) }
            else { f.handoffs.append(HandoffFact(key: "h\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, from: e.task ?? "", to: "lead", agent: "lead", summary: L.text("\(d["kind"].str == "note" ? L.text("备注") : L.text("进展"))：\(text)"), artifacts: [])) }
        case .questionAnswered:
            if let task = e.task { splits[task] = nil }
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("回答 \(e.task ?? "") 的提问"), text: d["answer"].str + (d["note"].truthy ? L.text("；\(d["note"].str)") : ""), tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }))
        case .approvalGranted, .approvalDenied:
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: e.type == .approvalGranted ? L.text("批准一次操作") : L.text("拒绝一次操作"), text: e.text, tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }))
        case .riskAccepted:
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("接受风险并验收"), text: e.text))
        case .runMerged:
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("交付时合并到你的分支"), text: e.text))
        case .resultAmended:
            if let task = e.task { gaps[task] = nil }
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("负责人修订 \(e.task ?? "") 的结论"), text: d["text"].string ?? e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"]))
        case .taskSkippedByDecision:
            if let task = e.task { splits[task] = nil }
            if e.actor == "lead" { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("负责人取消 \(e.task ?? "")"), text: e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"])) }
        case .taskReopened:
            if let task = e.task { gaps[task] = nil; splits[task] = nil }
            if e.actor == "lead" {
                let session: String? = switch d["session"].str {
                case "continue": L.text("会话：接上原会话")
                case "new": L.text("会话：新开会话")
                default: nil
                }
                let parts = [session,
                             d["from"].truthy ? L.text("依据 \(d["from"].str) 的意见") : nil,
                             d["note"].truthy ? L.text("负责人的说明：\(d["note"].str)") : nil,
                             d["instructions"].truthy ? L.text("新说明：\(d["instructions"].str)") : nil,
                             d["accept"].truthy ? L.text("新的验收标准：\(d["accept"].strings.joined(separator: L.text("；")))") : nil].compactMap { $0 }
                let title = d["from"].truthy ? L.text("负责人让 \(e.task ?? "") 返工") : L.text("负责人重做 \(e.task ?? "")")
                f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: title, text: parts.isEmpty ? e.text : parts.joined(separator: "\n"), tasks: [e.task, d["from"].string].compactMap { $0 }, agents: ["lead"]))
            }
        case .taskSucceeded:
            if let task = e.task { splits[task] = nil; gaps[task] = d["traceGaps"].strings.isEmpty ? nil : (seq: e.seq, gaps: d["traceGaps"].strings) }
            if d["acceptedBy"].str == "lead" { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("负责人接受 \(e.task ?? "") 的结果"), text: d["reason"].string ?? e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"])) }
        case .userMessage:
            if e.actor == "lead", let task = e.task { splits[task] = nil }
            let images = Attach.paths(d).count
            let raw = d["text"].string ?? e.text
            let words = images == 0 ? raw : Attach.visibleText(raw)
            let text = images == 0 ? words : [words, L.text("（附 \(images) 张图片）")].filter { !$0.isEmpty }.joined(separator: " ")
            let dup = MessageDelivery(d["delivery"].str) == .prompt && f.decisions.contains { $0.msg && $0.to == e.to && $0.text == text }
            if !dup { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: L.text("给 \(name(e.to ?? "")) 的补充"), text: text, tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }, msg: true, to: e.to)) }
            if !d["id"].exists, MessageDelivery(d["delivery"].str)?.waits == true { notes[e.to ?? "", default: []].append((seq: e.seq, text: text)) }
        case .messageDelivered: if !d["ids"].exists { notes[e.to ?? ""] = nil }
        default: break
        }
    }
    // Messages with ids still waiting for their agent: queued for the turn's end, or left for its next session.
    for m in st.msgs.values where m.deliveredSeq == nil && m.delivery.waits { notes[m.to, default: []].append((seq: m.seq, text: m.text)) }
    // Each result is used by whoever it was handed to; a newer attempt supersedes an older one.
    let results = f.results
    for i in f.results.indices {
        let r = results[i]
        f.results[i].usedBy = f.handoffs.filter { h in h.from == r.task && h.seq > r.seq && !results.contains { $0.task == r.task && $0.seq > r.seq && $0.seq < h.seq } }.map(\.to)
        f.results[i].old = results.contains { $0.task == r.task && $0.seq > r.seq }
        f.results[i].amended = f.results[i].old ? [] : st.tasks[r.task]?.amendments ?? []
    }
    f.arts = st.artifacts.map { a in
        let old = st.artifacts.contains { $0.task == a.task && $0.kind == a.kind && $0.kind != "report" && $0.seq > a.seq } || (a.kind == "report" && st.artifacts.contains { $0.task == a.task && $0.kind == "report" && $0.attempt > a.attempt })
        var used: [String] = []
        for h in f.handoffs where h.artifacts.contains(a.name) && !used.contains(h.to) { used.append(h.to) }
        return ArtFact(a: a, old: old, usedBy: used)
    }
    // Open items: anything that blocks someone until a person or an agent acts.
    for i in st.inbox {
        let t = i.task.flatMap { st.tasks[$0] }
        // Who asks, by name, as the request cards say it; the task goes in the line below.
        let who = name(t?.agent ?? i.agent ?? "")
        let title: String
        var actions: [QuickAction] = []
        switch i.kind {
        case "question" where i.task != nil:
            title = L.text("「\(who)」问你一个问题")
            actions = (t?.question?.options ?? []).map { o in
                QuickAction(label: o, prominent: o == i.d["recommend"].str, action: .answer(key: i.key, task: i.task ?? "", answer: o, note: ""))
            }
        case "question":
            title = L.text("负责人等你拍板：\(L.short(i.d["question"].str, 40))")
            actions = i.d["options"].strings.map { o in
                QuickAction(label: o, help: o == i.d["recommend"].str ? L.text("负责人建议") : "", prominent: o == i.d["recommend"].str, action: .answer(key: i.key, task: "", answer: o, note: ""))
            }
        case "approval":
            title = L.text("「\(who)」等你拍板")
            actions = L.permissionOptions(i.d["options"]).map { o in
                QuickAction(label: o.label, help: o.help, prominent: o.kind == "allow_once", action: .approve(i, o))
            }
        case "input":
            title = i.d["mode"].str == "url" ? L.text("「\(who)」需要你打开一个链接") : L.text("「\(who)」需要你填一张表")
        case "session": title = L.text("「\(who)」这一轮结束，等你回话")
        default: title = L.text("待处理")
        }
        var text: String
        switch i.kind {
        case "question": text = t?.question?.question ?? ""
        case "approval": text = i.d["command"].str
        case "input": text = i.d["message"].str
        default: text = ""
        }
        if text.isEmpty { text = i.d["reason"].str }
        f.open.append(OpenFact(key: .request(seq: i.seq, kind: i.kind), seq: i.seq, title: title, text: text, tasks: [i.task].compactMap { $0 }, agents: [t?.agent ?? i.agent ?? "lead"], you: true, sub: t.map { "\($0.id) · \($0.title)" } ?? "", kind: i.kind, actions: actions))
    }
    if let n = loopNotice {
        let title = n.kind == "loop_touched" ? L.text("值班在自己的副本里改了文件") : L.text("值班没能开工")
        let sub = n.kind == "loop_touched" ? n.files.prefix(5).joined(separator: L.text("、")) : L.text("下次开工会再试；也可以在值班页叫它现在开工")
        f.open.append(OpenFact(key: .loop(seq: n.seq), seq: n.seq, title: title, text: n.text, tasks: [], agents: ["lead"], you: true, sub: sub, kind: n.kind))
    }
    if let ls = leadStall, st.active {
        f.open.append(OpenFact(key: .leadStall(seq: ls.seq), seq: ls.seq, title: L.text("负责人可能卡住了"), text: ls.text, tasks: [], agents: ["lead"], you: true, sub: L.text("可以在负责人的会话里发消息、打断并接管，或者继续等"), kind: "lead_stall"))
    }
    // Everything else that holds work up is the lead's to settle, not the person's.
    let lead = L.text("负责人在处理，你不用管")
    // A shared edit that an overstep already covers is the same fact: keep the overstep.
    let covered = { (c: (seq: Int, task: String, other: String, paths: [String], owned: Bool, byLead: Bool)) in
        !c.owned && conflicts.contains { o in o.owned && o.byLead == c.byLead && Set([o.task, o.other]) == Set([c.task, c.other]) && Set(c.paths).isSubset(of: Set(o.paths)) }
    }
    for c in conflicts where !covered(c) {
        if c.byLead {
            guard !(st.tasks[c.task]?.status.isEnded ?? true) else { continue }
            let title = c.owned ? L.text("负责人改了 \(c.task) 负责的文件") : L.text("负责人和 \(c.task) 改了同一个文件")
            f.open.append(OpenFact(key: .conflict(seq: c.seq), seq: c.seq, title: title, text: c.paths.joined(separator: L.text("、")), tasks: [c.task], agents: [st.tasks[c.task]?.agent ?? "", "lead"], you: false, sub: lead, kind: c.owned ? "overstep" : "conflict"))
        } else if c.owned ? st.tasks[c.task]?.status == .running : (st.tasks[c.task]?.status == .running || st.tasks[c.other]?.status == .running) {
            let title = c.owned ? L.text("\(c.task) 改了 \(c.other) 负责的文件") : L.text("\(c.task) 和 \(c.other) 改了同一个文件")
            f.open.append(OpenFact(key: .conflict(seq: c.seq), seq: c.seq, title: title, text: c.paths.joined(separator: L.text("、")), tasks: [c.task, c.other], agents: [st.tasks[c.task]?.agent ?? "", st.tasks[c.other]?.agent ?? "", "lead"], you: false, sub: lead, kind: c.owned ? "overstep" : "conflict"))
        }
    }
    for (task, sp) in splits.sorted(by: { $0.value.seq < $1.value.seq }) where !(st.tasks[task]?.status.isEnded ?? true) {
        f.open.append(OpenFact(key: .split(task: task, seq: sp.seq), seq: sp.seq, title: L.text("\(task) 请求拆分子任务"), text: sp.text, tasks: [task], agents: [st.tasks[task]?.agent ?? "", "lead"], you: false, sub: lead, kind: "split_request"))
    }
    // Until the lead redoes or corrects the task, or the run is handed over.
    if !(st.run.status.isDone || st.run.status == .cancelled) {
        for (task, g) in gaps.sorted(by: { $0.value.seq < $1.value.seq }) where st.tasks[task]?.status == .succeeded {
            f.open.append(OpenFact(key: .gap(seq: g.seq), seq: g.seq, title: L.text("\(task) 的结果和工作目录对不上"), text: g.gaps.joined(separator: "\n"), tasks: [task], agents: [st.tasks[task]?.agent ?? "", "lead"], you: false, sub: lead, kind: "trace"))
        }
    }
    for (task, s) in stalls.sorted(by: { $0.value.seq < $1.value.seq }) where st.tasks[task]?.status == .running {
        f.open.append(OpenFact(key: .stall(task: task, seq: s.seq), seq: s.seq, title: L.text("\(task) 可能卡住了"), text: s.text, tasks: [task], agents: [st.tasks[task]?.agent ?? "", "lead"], you: false, sub: lead, kind: "stall"))
    }
    for b in blockers where !(st.tasks[b.task]?.status.isEnded ?? false) {
        f.open.append(OpenFact(key: .blocker(seq: b.seq), seq: b.seq, title: L.text("\(b.task) 遇到阻塞"), text: b.text, tasks: [b.task], agents: [b.actor, "lead"], you: false, sub: lead, kind: "blocker"))
    }
    for t in st.orderedTasks where t.status == .waitingInput {
        guard let q = t.question, q.answer == nil, !st.inbox.contains(where: { $0.key == q.key }) else { continue }
        f.open.append(OpenFact(key: .leadQuestion(task: t.id, seq: q.seq), seq: q.seq, title: L.text("\(t.id) 问负责人"), text: q.question, tasks: [t.id], agents: [t.agent, "lead"], you: false, sub: lead, kind: "lead_question"))
    }
    for t in st.orderedTasks where t.status == .failed || t.status == .verificationFailed {
        f.open.append(OpenFact(key: .failed(task: t.id), seq: t.attempts.last?.seq, title: L.text("\(t.id) 失败，等负责人决定"), text: t.error.isEmpty ? t.verificationFailures.joined(separator: L.text("；")) : t.error, tasks: [t.id], agents: [t.agent, "lead"], you: false, sub: L.text("负责人可以重做、接受结果或取消"), kind: "failed"))
    }
    for t in st.orderedTasks where t.status == .blocked {
        f.open.append(OpenFact(key: .blocked(task: t.id), seq: nil, title: L.text("\(t.id) 等待 \(t.blockedBy)"), text: L.text("依赖的 \(t.blockedBy) 没有完成，\(t.id) 还不能开始。"), tasks: [t.blockedBy, t.id], agents: [t.agent, "lead"], you: false, sub: lead, kind: "blocked"))
    }
    // Once the run is over, a message that never arrived is marked on its decision card.
    if st.run.status.isDone || st.run.status == .cancelled || st.run.status == .failed {
        let lost = Set(notes.values.flatMap { $0.map(\.seq) })
        for i in f.decisions.indices where lost.contains(f.decisions[i].seq) { f.decisions[i].lost = true }
    }
    f.open.sort { a, b in a.you != b.you ? a.you : (a.seq ?? 0) > (b.seq ?? 0) }
    return f
}
