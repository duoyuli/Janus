import Foundation

// The run page keeps no business state of its own: every view is derived from the event log by
// `derive` and `boardFacts`. A new or changed event type must be handled here as well
// (docs/architecture/frontend.md#event_derivation).

struct Event: Identifiable, Hashable {
    let seq: Int
    let ts: Double
    let type: String
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
        type = j["type"].str
        actor = j["actor"].str
        to = j["to"].string
        task = j["task"].string
        attempt = j["attempt"].int
        text = j["text"].str
        data = j["data"]
    }
}

struct Check: Hashable {
    let criterion: String
    let passed: Bool
    let evidence: String
    init(_ j: JSON) { criterion = j["criterion"].str; passed = j["passed"].bool ?? false; evidence = j["evidence"].str }
    static func list(_ j: JSON) -> [Check] { j.array.map(Check.init) }
}

final class Attempt {
    let n: Int
    let session: String
    let workspace: String
    let start: Double
    var status: String
    let seq: Int
    var error: String?
    init(n: Int, session: String, workspace: String, start: Double, status: String, seq: Int) {
        self.n = n; self.session = session; self.workspace = workspace; self.start = start; self.status = status; self.seq = seq
    }
}

struct Artifact: Hashable {
    let name, kind, path, desc, hash: String
    let bytes: Int?
    let task: String
    let attempt: Int
    let seq: Int
    let by: String
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

final class TaskState {
    let id: String
    var title, desc, agent, kind: String
    var deps: [String]
    var accept: [String]
    var candidates: [JSON]
    var delegated: Bool
    var status = "pending"
    var attempts: [Attempt] = []
    var progress: Double = 0
    var artifacts: [Artifact] = []
    var live = false
    var control = "auto"
    var turn = "running"
    var blockedBy = ""
    var checks: [Check] = []
    var verificationFailures: [String] = []
    var error = ""
    var question: Question?
    var amendments: [String] = []
    var summary = ""
    var evidence: [String] = []
    var reason = ""
    var overridden = false

    init(_ x: JSON) {
        id = x["id"].str
        title = x["title"].str
        desc = x["desc"].str
        agent = x["agent"].str
        kind = x["kind"].str
        deps = x["deps"].strings
        accept = x["accept"].strings
        candidates = x["candidates"].array
        delegated = x["delegated"].bool ?? false
    }
}

final class AgentState {
    var phase = "idle"
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
    var delivery: String
    var routedTask: String?
    var routedSeq: Int?
    var deliveredSeq: Int?
    init(id: String, text: String, by: String, to: String, task: String?, seq: Int) {
        self.id = id; self.text = text; self.by = by; self.to = to; self.task = task; self.seq = seq; delivery = "pending"
    }
}

/// Something in front of the person (or, for `run_decision` and `failed`, of an old plan-mode run).
struct InboxItem: Identifiable {
    let kind: String
    let key: String
    let seq: Int
    var task: String?
    var agent: String?
    /// The rest of the request: reason, recommend, question, options, schema, url, command …
    var d: JSON = [:]
    var id: String { key }
}

struct RunInfo {
    var status = "idle"
    var goal = ""
    var start: Double = 0
    var now: Double = 0
    var paused = false
    var kind = ""
    var mode = ""
    var cwd = ""
    var git = false
    var baseRef = ""
    var workspace = ""
    var inPlace: Bool?
    var error = ""
}

struct RunResult {
    let d: JSON
    let seq: Int
    var accepted: Double?
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
    var result: RunResult?
    var events: [Event] = []
    var lastPlanSeq: Int?

    var orderedTasks: [TaskState] { order.compactMap { tasks[$0] } }
    func event(_ seq: Int) -> Event? { seq >= 1 && seq <= events.count && events[seq - 1].seq == seq ? events[seq - 1] : events.first { $0.seq == seq } }

    static let finished: Set<String> = ["delivered", "accepted", "accepted_with_risk", "cancelled", "failed", "interrupted"]
    var active: Bool { !run.goal.isEmpty && !Self.finished.contains(run.status) }
}

// @project-doc docs/architecture/frontend.md#event_derivation
func derive(_ events: [Event]) -> RunState {
    var s = RunState()
    s.events = events
    func ag(_ id: String) -> AgentState {
        if let a = s.agents[id] { return a }
        let a = AgentState()
        s.agents[id] = a
        return a
    }
    func dropSess(_ id: String) { s.inbox.removeAll { $0.key == "sess-" + id } }
    func endLive(_ t: TaskState?) { if let t { t.live = false; dropSess(t.id) } }
    func dropKey(_ k: String) { s.inbox.removeAll { $0.key == k } }

    for e in events {
        s.run.now = e.ts
        if !e.actor.isEmpty && e.actor != "user" && e.actor != "janus" { ag(e.actor).beat = e.ts }
        let t = e.task.flatMap { s.tasks[$0] }
        let d = e.data
        switch e.type {
        case "run.created":
            s.run.status = d["mode"].str == "goal" ? "running" : "planning"
            s.run.mode = d["mode"].string ?? "plan"
            s.run.goal = d["goal"].str
            s.run.start = e.ts
            s.run.kind = d["kind"].str
            s.run.cwd = d["cwd"].str
            s.run.git = d["git"].truthy
            s.run.baseRef = d["baseRef"].str
            s.run.workspace = d["workspace"].str
            s.run.inPlace = d["inPlace"].bool
            ag("lead").phase = "thinking"
        case "plan.proposed":
            s.tasks = [:]; s.order = []; s.lastPlanSeq = e.seq; dropKey("plan")
            for x in d["tasks"].array { let nt = TaskState(x); nt.status = "created"; s.tasks[nt.id] = nt; s.order.append(nt.id) }
            s.run.status = "awaiting_plan"
            s.inbox.append(InboxItem(kind: "plan", key: "plan", seq: e.seq))
            ag("lead").phase = "waiting_for_input"
        case "plan.approved":
            s.answers["plan"] = true; dropKey("plan"); s.run.status = "running"
            for id in s.order { s.tasks[id]?.status = "pending" }
            for (id, a) in d["overrides"].object { if let x = s.tasks[id] { x.agent = a.str; x.reason = "你在确认计划时改派"; x.overridden = true } }
            ag("lead").phase = "coordinating"
        case "orchestration.accepted":
            for x in d["tasks"].array {
                let nt = TaskState(x)
                if s.tasks[nt.id] == nil { s.order.append(nt.id) }
                s.tasks[nt.id] = nt
            }
            ag("lead").phase = "coordinating"
        case "goal.status": ag("lead").note = e.text
        case "run.waiting_input":
            s.run.status = "waiting_input"
            let stuck = d["tasks"].exists ? d["tasks"].array.map { $0["id"].str }
                : s.orderedTasks.filter { ["failed", "blocked", "verification_failed", "cancelled", "cancelled_by_dependency"].contains($0.status) }.map(\.id)
            s.inbox.removeAll { $0.kind == "run_decision" }
            if !stuck.isEmpty { s.inbox.append(InboxItem(kind: "run_decision", key: "run-decision", seq: e.seq, d: ["tasks": JSON(stuck)])) }
        case "run.budget_exhausted":
            s.run.status = "waiting_input"; s.inbox.removeAll { $0.kind == "budget" }
            s.inbox.append(InboxItem(kind: "budget", key: "budget", seq: e.seq, d: ["text": JSON(e.text)]))
        case "run.budget_extended": s.run.status = "running"; s.inbox.removeAll { $0.kind == "budget" }
        case "plan.validation_exhausted":
            s.run.status = "waiting_input"; s.inbox.removeAll { $0.kind == "plan_invalid" }
            s.inbox.append(InboxItem(kind: "plan_invalid", key: "plan-invalid", seq: e.seq, d: ["text": JSON(e.text), "error": d["error"], "reason": d["reason"]]))
            ag("lead").phase = "error"
        case "plan.replan_requested": s.inbox.removeAll { $0.kind == "plan_invalid" }; s.run.status = "planning"; ag("lead").phase = "thinking"
        case "decision.withdrawn":
            dropKey(d["key"].str)
            if let t, ["waiting_input", "waiting_approval"].contains(t.status) { t.status = "running" }
        case "run.cancelled":
            s.run.status = "cancelled"; s.inbox = []
            for x in s.orderedTasks { x.live = false; if x.status != "succeeded" { x.status = "cancelled" } }
        case "run.paused": s.run.paused = true
        case "run.resumed": s.run.paused = false
        case "task.ready":
            let ids = d["tasks"].exists ? d["tasks"].strings : [e.task].compactMap { $0 }
            for id in ids { s.tasks[id]?.status = "ready" }
        case "task.assigned": t?.status = "assigned"
        case "task.blocked": t?.status = d["kind"].str == "rework" ? "waiting_review" : "blocked"; t?.blockedBy = d["dependency"].str
        case "task.cancel_requested": t?.status = "cancelling"
        case "task.cancelled_by_dependency": t?.status = "cancelled_by_dependency"; endLive(t)
        case "task.skipped_by_decision": t?.status = "skipped_by_decision"; endLive(t)
        case "result.submitted": t?.checks = Check.list(d["checks"])
        case "verification.started": t?.status = "verifying"
        case "verification.failed":
            guard let t else { break }
            t.status = "verification_failed"; t.verificationFailures = d["failures"].strings
            if !t.delegated { s.inbox.append(InboxItem(kind: "failed", key: "verify-" + t.id, seq: e.seq, task: t.id, d: ["error": JSON(t.verificationFailures.joined(separator: "；"))])) }
        case "attempt.started":
            guard let t else { break }
            t.attempts.append(Attempt(n: e.attempt ?? t.attempts.count + 1, session: d["session"].str, workspace: d["workspace"].str, start: e.ts, status: "running", seq: e.seq))
            // Runs recorded before "inPlace" existed: an attempt in "工作目录 …" changed the directory itself.
            if s.run.inPlace == nil && d["workspace"].str.hasPrefix("工作目录 ") { s.run.inPlace = true }
            t.live = true; t.control = "auto"; t.turn = "running"; t.status = "running"; t.progress = 5
            let a = ag(t.agent); a.session = d["session"].string; a.phase = "thinking"; a.note = ""; a.progress = 5
        case "agent.status":
            let a = ag(e.actor)
            a.phase = d["phase"].str; a.note = e.text; a.progress = d["progress"].double
            if d["session"].exists { a.session = d["session"].string }
            if let t, let p = d["progress"].double { t.progress = p }
            if d["turn"].str == "idle" {
                if let t {
                    t.turn = "idle"
                    if !s.inbox.contains(where: { $0.key == "sess-" + t.id }) { s.inbox.append(InboxItem(kind: "session", key: "sess-" + t.id, seq: e.seq, task: t.id)) }
                } else if !s.inbox.contains(where: { $0.key == "sess-" + e.actor }) {
                    s.inbox.append(InboxItem(kind: "session", key: "sess-" + e.actor, seq: e.seq, agent: e.actor))
                }
            }
        case "tool.started":
            let a = ag(e.actor); a.phase = d["tool"].str == "janus.wait" ? "waiting_for_agent" : "running_tool"; a.note = e.text
        case "tool.completed":
            if ["janus.wait", "janus.reply", "janus.orchestrate"].contains(d["tool"].str) { let a = ag(e.actor); a.phase = e.actor == "lead" ? "coordinating" : "thinking"; a.note = e.text }
        case "control.taken": t?.control = "human"
        case "session.interrupted": if let t, !d["finish"].truthy { t.control = "human" }
        case "control.released":
            if let t { t.control = "auto"; t.turn = "running"; dropSess(t.id) } else if let to = e.to { dropSess(to) }
        case "user.message":
            if let id = d["id"].string {
                s.msgs[id] = Msg(id: id, text: d["text"].str, by: e.actor, to: e.to ?? "", task: e.task, seq: e.seq)
                break
            }
            if let t, d["delivery"].str == "prompt" { t.turn = "running"; dropSess(t.id); ag(t.agent).phase = "thinking" }
            else if t == nil, d["delivery"].str == "prompt", let to = e.to { dropSess(to) }
        case "message.routed":
            if let m = s.msgs[d["id"].str] { m.delivery = d["delivery"].str; m.routedTask = e.task; m.routedSeq = e.seq }
            if d["delivery"].str == "prompt" {
                if let t { t.turn = "running"; dropSess(t.id); ag(t.agent).phase = "thinking" } else if let to = e.to { dropSess(to) }
            }
        case "message.delivered":
            t?.turn = "running"
            for id in d["ids"].strings { s.msgs[id]?.deliveredSeq = e.seq }
        case "question.asked":
            guard let t else { break }
            t.status = "waiting_input"
            t.question = Question(key: d["key"].str, question: d["question"].str, options: d["options"].strings, blocking: d["blocking"].truthy, seq: e.seq, from: e.actor)
            let a = ag(e.actor); a.phase = "waiting_for_input"; a.note = e.text
        // A collaborator's question passed on to the person, or a decision the lead hands over itself (no task).
        case "question.escalated":
            dropKey(d["key"].str)
            var item = InboxItem(kind: "question", key: d["key"].str, seq: e.seq, task: e.task, d: ["reason": d["reason"], "recommend": d["recommend"]])
            if e.task == nil { item.d["question"] = d["question"]; item.d["options"] = d["options"].or([]); item.d["from"] = JSON(e.actor) }
            s.inbox.append(item)
        case "question.answered":
            s.answers[d["key"].str] = d["answer"]; dropKey(d["key"].str)
            guard let t else { break }
            if t.status == "waiting_input" { t.status = "running" }
            t.question?.answer = d["answer"].str
            ag(t.agent).phase = "thinking"
        case "approval.requested":
            t?.status = "waiting_approval"
            s.inbox.append(InboxItem(kind: "approval", key: d["key"].str, seq: e.seq, task: e.task, agent: e.actor, d: d))
            let a = ag(e.actor); a.phase = "waiting_approval"; a.note = e.text
        case "input.requested":
            t?.status = "waiting_input"
            s.inbox.append(InboxItem(kind: "input", key: d["key"].str, seq: e.seq, task: e.task, agent: e.actor, d: d))
            let a = ag(e.actor); a.phase = "waiting_for_input"; a.note = e.text
        case "input.answered":
            s.answers[d["key"].str] = d["action"]; dropKey(d["key"].str)
            if let t, t.status == "waiting_input" { t.status = "running" }
            if let to = e.to { ag(to).phase = "thinking" }
        case "approval.granted", "approval.denied":
            s.answers[d["key"].str] = JSON(e.type == "approval.granted" ? "granted" : "denied"); dropKey(d["key"].str)
            if let t, t.status == "waiting_approval" { t.status = "running"; ag(t.agent).phase = "running_tool" }
        case "task.failed":
            guard let t else { break }
            t.status = "failed"; t.error = d["error"].str
            // A task the lead orchestrated is the lead's to deal with: it reads the failure on the board.
            if !t.delegated { s.inbox.append(InboxItem(kind: "failed", key: "fail-" + t.id, seq: e.seq, task: t.id, d: ["error": d["error"]])) }
        case "run.failed": s.run.status = "failed"; s.run.error = d["error"].string ?? e.text
        case "run.interrupted":
            s.run.status = "interrupted"; s.inbox = []
            for x in s.orderedTasks { x.live = false }
            for a in s.agents.values { a.phase = "idle" }
        case "run.recovered":
            s.run.status = d["phase"].str
            if d["phase"].str == "awaiting_plan", let p = s.lastPlanSeq { s.inbox.append(InboxItem(kind: "plan", key: "plan", seq: p)) }
            if d["tasks"].exists {
                for x in d["tasks"].array {
                    guard let rt = s.tasks[x["id"].str] else { continue }
                    rt.status = x["status"].str; rt.blockedBy = x["blockedBy"].str; rt.live = false
                    if rt.status == "failed" && !rt.delegated {
                        s.inbox.append(InboxItem(kind: "failed", key: "fail-" + rt.id, seq: e.seq, task: rt.id, d: ["error": JSON(rt.attempts.last?.error ?? "重启前已用完重试次数")]))
                    }
                }
            } else { for x in s.orderedTasks where x.status != "succeeded" { x.status = "pending" } }
            if d["phase"].str == "planning" { ag("lead").phase = "thinking" }
        case "plan.revision_requested":
            dropKey("plan"); s.run.status = "planning"; s.tasks = [:]; s.order = []; ag("lead").phase = "thinking"
        case "artifact.created":
            let a = Artifact(name: d["name"].str, kind: d["kind"].str, path: d["path"].str, desc: d["desc"].str, hash: d["hash"].str, bytes: d["bytes"].int, task: e.task ?? "", attempt: e.attempt ?? 0, seq: e.seq, by: e.actor)
            s.artifacts.append(a); t?.artifacts.append(a)
        case "attempt.failed":
            guard let t else { break }
            endLive(t)
            if let at = t.attempts.last { at.status = "failed"; at.error = d["error"].str }
            t.status = "ready"
            let a = ag(t.agent); a.phase = "error"; a.note = d["error"].str; a.progress = nil
        case "review.rejected":
            if let target = s.tasks[d["target"].str] { target.attempts.last?.status = "rejected" }
            guard let t else { break }
            t.attempts.last?.status = "verdict"; endLive(t)
            t.status = "blocked"; t.blockedBy = d["target"].str
            let a = ag(e.actor); a.phase = "idle"; a.note = ""; a.progress = nil
        case "result.amended": t?.amendments.append(d["text"].str)
        case "session.prompt": if let t, d["channel"].str == "fix" { t.status = "running" }
        case "task.reopened":
            guard let t else { break }
            t.amendments = []
            if let ins = d["instructions"].string, !ins.isEmpty { t.desc = ins }
            // Tasks held by this one's failure wait for it again instead of showing as stuck.
            for x in s.orderedTasks where x.status == "blocked" && x.blockedBy == t.id { x.status = "pending" }
            t.status = "ready"
            s.inbox.removeAll { ["fail-" + t.id, "verify-" + t.id, "run-decision"].contains($0.key) }
            if ["delivered", "accepted", "accepted_with_risk"].contains(s.run.status) { s.run.status = "running" }
        case "task.succeeded":
            guard let t else { break }
            t.status = "succeeded"; t.progress = 100; t.summary = d["summary"].str; t.evidence = d["evidence"].strings
            endLive(t)
            if let at = t.attempts.last, at.status == "running" { at.status = "succeeded" }
            let a = ag(t.agent); a.phase = t.agent == "lead" ? "coordinating" : "idle"; a.note = ""; a.progress = nil
        case "fix.requested": s.answers[d["key"].str] = true
        case "run.delivered":
            s.run.status = "delivered"; s.result = RunResult(d: d, seq: e.seq)
            let a = ag("lead"); a.phase = "idle"; a.note = ""
        case "run.accepted_with_risk":
            s.run.status = "accepted_with_risk"; s.inbox.removeAll { ["run_decision", "failed", "budget"].contains($0.kind) }
        case "risk.accepted": s.run.status = "accepted"; s.result?.accepted = e.ts
        default: break
        }
    }
    return s
}

// MARK: - Shared board

struct OpenFact: Identifiable {
    let key: String
    let seq: Int?
    let title: String
    let text: String
    let tasks: [String]
    let agents: [String]
    let you: Bool
    var sub: String = ""
    var id: String { key }
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

// @project-doc docs/architecture/frontend.md#event_derivation
func boardFacts(_ st: RunState, name: (String) -> String) -> BoardFacts {
    var f = BoardFacts()
    var notes: [String: [(seq: Int, text: String)]] = [:]
    var blockers: [(key: String, seq: Int, task: String, text: String, actor: String)] = []
    let recorded = st.events.contains { $0.type == "context.handoff" }
    var plans = 0
    for e in st.events {
        let d = e.data
        switch e.type {
        case "plan.proposed": plans += 1
        case "result.submitted":
            f.results.append(ResultFact(key: "r\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, task: e.task ?? "", summary: d["summary"].string ?? e.text, evidence: d["evidence"].strings, risks: d["risks"].strings, checks: Check.list(d["checks"])))
        case "context.handoff":
            f.handoffs.append(HandoffFact(key: "h\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, from: d["from"].str, to: e.task ?? "", agent: e.to ?? "", summary: d["summary"].str, artifacts: d["artifacts"].strings))
        case "attempt.started":
            // Runs from before handoffs were recorded: the orchestrator injected every dependency's latest result here.
            guard !recorded, !d["resumed"].truthy, let task = e.task else { break }
            for dep in st.tasks[task]?.deps ?? [] {
                let r = f.results.last { $0.task == dep && $0.seq < e.seq }
                let arts = st.artifacts.filter { $0.task == dep && $0.seq < e.seq }.map(\.name)
                f.handoffs.append(HandoffFact(key: "h\(e.seq)\(dep)", seq: e.seq, ts: e.ts, by: r?.by ?? st.tasks[dep]?.agent ?? "", from: dep, to: task, agent: e.to ?? "", summary: r?.summary ?? "", artifacts: arts))
            }
        case "plan.approved":
            let ov = d["overrides"].object
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "确认计划\(plans > 1 ? "（第 \(plans) 版）" : "")", text: "\(st.order.count) 个任务\(ov.isEmpty ? "" : "，改派 " + ov.map { "\($0.key) → \(name($0.value.str))" }.joined(separator: "、"))", agents: ["lead"]))
        case "orchestration.accepted":
            let ts = d["tasks"].array
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "主控提交编排", text: ts.map { x in "\(x["id"].str) \(x["title"].str) → \(name(x["agent"].str))\(x["deps"].array.isEmpty ? "" : "（依赖 \(x["deps"].strings.joined(separator: "、"))）")" }.joined(separator: "\n"), tasks: ts.map { $0["id"].str }, agents: ["lead"]))
        case "orchestration.rejected":
            let errs = d["errors"].strings
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "编排没有通过校验", text: errs.isEmpty ? e.text : errs.joined(separator: "\n"), agents: ["lead"]))
        case "board.posted":
            let text = d["text"].string ?? e.text
            if d["kind"].str == "blocker" { blockers.append((key: "ob\(e.seq)", seq: e.seq, task: e.task ?? "", text: text, actor: e.actor)) }
            else { f.handoffs.append(HandoffFact(key: "h\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, from: e.task ?? "", to: "lead", agent: "lead", summary: "\(d["kind"].str == "note" ? "备注" : "进展")：\(text)", artifacts: [])) }
        case "plan.revision_requested":
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "要求重新规划", text: d["feedback"].string ?? e.text, agents: ["lead"]))
        case "question.answered":
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "回答 \(e.task ?? "") 的提问", text: d["answer"].str + (d["note"].truthy ? "；\(d["note"].str)" : ""), tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }))
        case "approval.granted", "approval.denied":
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: e.type == "approval.granted" ? "批准一次操作" : "拒绝一次操作", text: e.text, tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }))
        case "review.approved", "review.rejected":
            let fs = d["findings"].array.map { "[\($0["severity"].str)] \($0["text"].str)" }
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: e.type == "review.approved" ? "评审通过\(d["target"].truthy ? " \(d["target"].str)" : "")" : "评审驳回 \(d["target"].str)", text: fs.isEmpty ? e.text : fs.joined(separator: "\n"), tasks: [e.task, d["target"].string].compactMap { $0 }, agents: [e.actor]))
        case "risk.accepted":
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "接受风险并验收", text: e.text))
        case "result.amended":
            f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "主控修订 \(e.task ?? "") 的结论", text: d["text"].string ?? e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"]))
        case "task.skipped_by_decision":
            if e.actor == "lead" { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "主控取消 \(e.task ?? "")", text: e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"])) }
        case "task.reopened":
            if e.actor == "lead" {
                let parts = [d["instructions"].truthy ? "新说明：\(d["instructions"].str)" : nil, d["accept"].truthy ? "新的验收标准：\(d["accept"].strings.joined(separator: "；"))" : nil].compactMap { $0 }
                f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "主控重做 \(e.task ?? "")", text: parts.isEmpty ? e.text : parts.joined(separator: "\n"), tasks: [e.task].compactMap { $0 }, agents: ["lead"]))
            }
        case "task.succeeded":
            if d["acceptedBy"].str == "lead" { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "主控接受 \(e.task ?? "") 的结果", text: d["reason"].string ?? e.text, tasks: [e.task].compactMap { $0 }, agents: ["lead"])) }
        case "user.message":
            let text = d["text"].string ?? e.text
            let dup = d["delivery"].str == "prompt" && f.decisions.contains { $0.msg && $0.to == e.to && $0.text == text }
            if !dup { f.decisions.append(DecisionFact(key: "d\(e.seq)", seq: e.seq, ts: e.ts, by: e.actor, title: "给 \(name(e.to ?? "")) 的补充", text: text, tasks: [e.task].compactMap { $0 }, agents: [e.to].compactMap { $0 }, msg: true, to: e.to)) }
            if !d["id"].exists, ["next", "queued"].contains(d["delivery"].str) { notes[e.to ?? "", default: []].append((seq: e.seq, text: text)) }
        case "message.delivered": if !d["ids"].exists { notes[e.to ?? ""] = nil }
        default: break
        }
    }
    // Messages with ids still waiting for their agent: queued for the turn's end, or left for its next session.
    for m in st.msgs.values where m.deliveredSeq == nil && ["queued", "next"].contains(m.delivery) { notes[m.to, default: []].append((seq: m.seq, text: m.text)) }
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
        let who = i.task ?? name(i.agent ?? "")
        let title: String
        switch i.kind {
        case "plan": title = "计划等你确认"
        case "question": title = i.task != nil ? "\(i.task!) 的提问等你回答" : "主控请你决定：\(L.short(i.d["question"].str, 40))"
        case "approval": title = "\(who) 请求你决定"
        case "input": title = "\(who) 需要你\(i.d["mode"].str == "url" ? "打开链接" : "填写")"
        case "failed": title = "\(i.task ?? "") 用完重试次数"
        case "session": title = "\(who) 这一轮结束，等你的消息"
        default: title = "等你处理"
        }
        var text: String
        switch i.kind {
        case "question": text = t?.question?.question ?? ""
        case "approval": text = i.d["command"].str
        case "input": text = i.d["message"].str
        case "failed": text = i.d["error"].str
        case "plan": text = "\(st.order.count) 个任务"
        default: text = ""
        }
        if text.isEmpty { text = i.d["reason"].str }
        f.open.append(OpenFact(key: "o\(i.seq)\(i.kind)", seq: i.seq, title: title, text: text, tasks: [i.task].compactMap { $0 }, agents: [t?.agent ?? i.agent ?? "lead"], you: true))
    }
    // Everything else that holds work up is the lead's to settle, not the person's.
    let lead = "主控在处理，不需要你操作"
    let closed: Set<String> = ["succeeded", "failed", "cancelled", "skipped_by_decision"]
    for b in blockers where !closed.contains(st.tasks[b.task]?.status ?? "") {
        f.open.append(OpenFact(key: b.key, seq: b.seq, title: "\(b.task) 遇到阻塞", text: b.text, tasks: [b.task], agents: [b.actor, "lead"], you: false, sub: lead))
    }
    if st.run.mode == "goal" {
        for t in st.orderedTasks where t.status == "waiting_input" {
            guard let q = t.question, q.answer == nil, !st.inbox.contains(where: { $0.key == q.key }) else { continue }
            f.open.append(OpenFact(key: "oq\(t.id)\(q.seq)", seq: q.seq, title: "\(t.id) 问主控", text: q.question, tasks: [t.id], agents: [t.agent, "lead"], you: false, sub: lead))
        }
        for t in st.orderedTasks where ["failed", "verification_failed"].contains(t.status) && t.delegated {
            f.open.append(OpenFact(key: "of\(t.id)", seq: t.attempts.last?.seq, title: "\(t.id) 失败，等主控决定", text: t.error.isEmpty ? t.verificationFailures.joined(separator: "；") : t.error, tasks: [t.id], agents: [t.agent, "lead"], you: false, sub: "主控可以重做、接受结果或取消"))
        }
    }
    for t in st.orderedTasks where t.status == "blocked" {
        f.open.append(OpenFact(key: "ob\(t.id)", seq: nil, title: "\(t.id) 等待 \(t.blockedBy)", text: "依赖的 \(t.blockedBy) 没有完成，\(t.id) 还不能开始。", tasks: [t.blockedBy, t.id], agents: [t.agent, "lead"], you: false, sub: lead))
    }
    // Once the run is over, a message that never arrived is marked on its decision card.
    if ["delivered", "accepted", "accepted_with_risk", "cancelled", "failed"].contains(st.run.status) {
        let lost = Set(notes.values.flatMap { $0.map(\.seq) })
        for i in f.decisions.indices where lost.contains(f.decisions[i].seq) { f.decisions[i].lost = true }
    }
    f.open.sort { a, b in a.you != b.you ? a.you : (a.seq ?? 0) > (b.seq ?? 0) }
    return f
}
