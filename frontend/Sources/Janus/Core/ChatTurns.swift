import Foundation

/// A session's conversation in turns, the way Lody shows one: each turn opens with what was sent
/// to the agent, its work (thinking, tool calls, notes along the way) folds to one line once it
/// is done, and the reply that ended it stays in view with the files it changed. What must not be
/// missed (requests, results, errors, messages cut into the turn, Janus's warnings) never folds.
/// Pure over the events the conversation shows, so the rows are tested without a view.
// @project-doc docs/architecture/20261031-frontend.md#session_turns
enum ChatTurns {
    /// What an event is to a turn.
    enum Role: Equatable {
        /// Starts a section of the conversation: the run, an attempt, a Loop's wake.
        case section
        /// What Janus put into the session's system layer, listed under its section.
        case injected
        /// Sent to the agent as a new turn: a person's message, a prompt from Janus.
        case opener
        /// Stays in view when the turn folds.
        case pinned
        case tool
        case message
        /// The rest of the agent's work: thinking, status, switches, board notes.
        case work
    }

    static func role(_ e: Event) -> Role {
        switch e.type {
        case .runCreated, .attemptStarted, .loopWoke: return .section
        case .contextHandoff: return .injected
        case .sessionPrompt:
            switch PromptChannel(rawValue: e.data["channel"].str) {
            case let c? where c.injected: return .injected
            // Pushed into the turn already running, like a message cut into it.
            case .board: return .pinned
            default: return .opener
            }
        case .userMessage: return .opener
        case .messageRouted: return MessageDelivery(e.data["delivery"].str) == .steer ? .pinned : .opener
        case .messageDelivered: return e.data["ids"].exists ? .opener : .pinned
        case .toolCall, .toolStarted, .toolCompleted: return .tool
        case .agentMessage: return .message
        // The session's own status lines (`agent.status` once a turn is idle) say where it stands: they stay.
        case .agentThought, .sessionControl, .boardPosted, .questionAnswered, .approvalGranted, .approvalDenied, .inputAnswered:
            return .work
        default: return .pinned
        }
    }

    /// One row of the conversation. Ids are stable while events arrive: an event's own `seq`, or
    /// a negative id made from the seq that starts what the row stands for.
    enum Row: Identifiable, Equatable {
        /// A section's opening event and what Janus injected under it.
        case section(Event, injected: [Event])
        /// What opened a turn, or any single event shown as itself.
        case item(Event)
        /// The agent's name and what its turn did, folded or open; `open` is nil when the turn
        /// did nothing but reply.
        case head(Turn, open: Bool?)
        /// Tool calls and thinking in a row: one line that opens to every call. `open` is nil
        /// when the calls are all the turn did: its own line already sums them up.
        case activity(id: Int, events: [Event], open: Bool?)
        /// The agent's text, `reply` when it is what ended the turn.
        case text([Event], reply: Bool)
        /// The files the turn changed, with their diffs.
        case edits(id: Int, files: [EditedFile])

        var id: Int {
            switch self {
            case .section(let e, _), .item(let e): return e.seq
            case .head(let t, _): return -(t.id * 8 + 1)
            case .activity(let id, _, _): return -(id * 8 + 2)
            case .text(let es, _): return es[0].seq
            case .edits(let id, _): return -(id * 8 + 3)
            }
        }
    }

    /// A file a turn changed: its path and the change of each call that edited it, in order.
    struct EditedFile: Equatable {
        let path: String
        var changes: [(old: String, new: String)]
        var added: Int
        var removed: Int
        static func == (a: EditedFile, b: EditedFile) -> Bool { a.path == b.path && a.added == b.added && a.removed == b.removed && a.changes.count == b.changes.count }
    }

    /// One turn: what opened it and everything the agent did until the next one.
    struct Turn: Equatable {
        /// The seq of its first event.
        let id: Int
        var opener: Event?
        var items: [Event] = []
        /// Running: the session's newest turn while the agent is at it.
        var running = false
        /// When the turn started and its last event.
        var start: Double
        var end: Double
        /// The session's working directory: a file named by its path there and by its full path
        /// is one file.
        var cwd = ""

        /// Whose work the turn is: the first agent that did something in it.
        var agent: String {
            items.first { $0.actor != "janus" && $0.actor != "user" && ChatTurns.role($0) != .pinned }?.actor ?? opener?.to ?? ""
        }

        /// What the agent's tools did, counted for the folded line.
        var stats: Stats {
            var s = Stats()
            var calls = Set<String>()
            var paths = Set<String>()
            for e in items where ChatTurns.role(e) == .tool {
                let id = e.data["callId"].truthy ? "\(e.actor):\(e.data["callId"].str)" : "seq:\(e.seq)"
                if !calls.insert(id).inserted { continue }
                switch e.data["tool"].str {
                case "execute": s.commands += 1
                case "read": s.reads += 1
                case "search", "fetch": s.searches += 1
                case "edit", "delete", "move": break
                case let t where t.hasPrefix("janus."): s.board += 1
                default: s.other += 1
                }
                let rel = { (p: String) in p.hasPrefix(cwd + "/") && !cwd.isEmpty ? String(p.dropFirst(cwd.count + 1)) : p }
                for p in e.data["paths"].strings { paths.insert(rel(p)) }
                for d in e.data["diffs"].array { paths.insert(rel(d["path"].str)) }
            }
            s.edited = paths.count
            return s
        }

        /// The agent's text that ended the turn: the messages after its last piece of work.
        /// Pinned items in between (a result handed in, a request) do not end the reply.
        var replyStart: Int? {
            guard let last = items.lastIndex(where: { $0.type == .agentMessage }) else { return nil }
            var i = last
            while i > 0 {
                let r = ChatTurns.role(items[i - 1])
                if r == .message || r == .pinned { i -= 1 } else { break }
            }
            // The first message of the reply: pinned items before it are not part of it.
            while i < last, items[i].type != .agentMessage { i += 1 }
            return i
        }
    }

    struct Stats: Equatable {
        var commands = 0, reads = 0, searches = 0, edited = 0, other = 0
        /// Janus's own tools: the board, or a Loop's duty tools.
        var board = 0
        var isEmpty: Bool { commands + reads + searches + edited + other + board == 0 }

        /// `运行 3 条命令 · 读 2 个文件 · 改 1 个文件`, only what happened.
        var text: String {
            var parts: [String] = []
            if commands > 0 { parts.append(L.text("运行 \(commands) 条命令")) }
            if reads > 0 { parts.append(L.text("读 \(reads) 个文件")) }
            if searches > 0 { parts.append(L.text("搜索 \(searches) 次")) }
            if edited > 0 { parts.append(L.text("改 \(edited) 个文件")) }
            if board > 0 { parts.append(L.text("用看板 \(board) 次")) }
            if other > 0 { parts.append(L.text("其他操作 \(other) 次")) }
            return parts.joined(separator: " · ")
        }
    }

    /// How the conversation folds. `detail` opens every turn and every group of calls; otherwise a
    /// finished turn shows one line, and only the running turn's newest group is open. `open`
    /// holds what the person opened or folded themselves, by row id.
    struct Options {
        var detail = false
        /// The session's newest turn is still running.
        var busy = false
        /// The session's working directory (`Turn.cwd`).
        var cwd = ""
        var open: [Int: Bool] = [:]
    }

    /// The turns of a session's shown events, in order, with the sections between them.
    static func turns(_ evs: [Event]) -> [Either] {
        var out: [Either] = []
        var turn: Turn?
        func close() { if let t = turn { out.append(.turn(t)) }; turn = nil }
        for e in evs {
            switch role(e) {
            case .section:
                close()
                out.append(.section(e, []))
            case .injected:
                // Under the section it belongs to, when nothing was said since; otherwise in the turn.
                if turn == nil, case .section(let s, let injected)? = out.last {
                    out[out.count - 1] = .section(s, injected + [e])
                } else if turn != nil {
                    turn!.items.append(e)
                    turn!.end = e.ts
                } else {
                    out.append(.section(e, []))
                }
            case .opener:
                // Janus's prompt carrying the message that just opened the turn belongs to that turn.
                if e.type == .sessionPrompt, let t = turn, t.opener != nil, t.items.isEmpty {
                    turn!.items.append(e)
                    turn!.end = e.ts
                    continue
                }
                close()
                turn = Turn(id: e.seq, opener: e, start: e.ts, end: e.ts)
            default:
                if turn == nil { turn = Turn(id: e.seq, start: e.ts, end: e.ts) }
                turn!.items.append(e)
                turn!.end = e.ts
            }
        }
        close()
        return out
    }

    enum Either: Equatable {
        case section(Event, [Event])
        case turn(Turn)
    }

    /// The rows to show for `evs`.
    static func rows(_ evs: [Event], _ o: Options) -> [Row] {
        let parts = turns(evs)
        let lastTurn = parts.lastIndex { if case .turn = $0 { return true } else { return false } }
        var rows: [Row] = []
        for (k, p) in parts.enumerated() {
            switch p {
            case .section(let e, let injected): rows.append(.section(e, injected: injected))
            case .turn(var t):
                t.running = o.busy && k == lastTurn
                t.cwd = o.cwd
                rows += turnRows(t, o)
            }
        }
        return rows
    }

    static func turnRows(_ t: Turn, _ o: Options) -> [Row] {
        var rows: [Row] = []
        if let opener = t.opener { rows.append(.item(opener)) }
        // Janus's prompt carrying the opening message sits with it, above the agent's line.
        var t = t
        while let first = t.items.first, role(first) == .opener {
            rows.append(.item(first))
            t.items.removeFirst()
        }
        guard !t.items.isEmpty else { return rows }
        let reply = t.running ? nil : t.replyStart
        let work = Array(t.items[..<(reply ?? t.items.count)])
        let hasWork = work.contains { ![.pinned, .opener].contains(role($0)) }
        // A turn cut off or with nothing but calls has no reply to show instead: it stays open.
        let headId = -(t.id * 8 + 1)
        let open = o.open[headId] ?? (o.detail || t.running || reply == nil)
        if hasWork || reply != nil { rows.append(.head(t, open: hasWork ? open : nil)) }
        rows += workRows(work, open: open || !hasWork, running: t.running, o)
        if let reply {
            var texts: [Event] = []
            for e in t.items[reply...] {
                if e.type == .agentMessage, texts.isEmpty || texts[0].actor == e.actor { texts.append(e); continue }
                if !texts.isEmpty { rows.append(.text(texts, reply: true)); texts = [] }
                if e.type == .agentMessage { texts = [e] } else { rows.append(.item(e)) }
            }
            if !texts.isEmpty { rows.append(.text(texts, reply: true)) }
        }
        let files = edited(t.items)
        if !files.isEmpty { rows.append(.edits(id: t.id, files: files)) }
        return rows
    }

    /// The work of a turn, open: groups of calls, the agent's text between them, notes; folded:
    /// only what is pinned.
    private static func workRows(_ work: [Event], open: Bool, running: Bool, _ o: Options) -> [Row] {
        var rows: [Row] = []
        var group: [Event] = []
        var text: [Event] = []
        func flushGroup() {
            guard let first = group.first else { return }
            rows.append(.activity(id: first.seq, events: group, open: o.open[-(first.seq * 8 + 2)] ?? o.detail))
            group = []
        }
        func flushText() { if !text.isEmpty { rows.append(.text(text, reply: false)); text = [] } }
        for e in work {
            let r = role(e)
            if !open && r != .pinned && r != .opener { continue }
            if r == .tool || e.type == .agentThought {
                flushText()
                group.append(e)
            } else if r == .message {
                flushGroup()
                if let f = text.first, f.actor != e.actor { flushText() }
                text.append(e)
            } else {
                flushGroup()
                flushText()
                rows.append(.item(e))
            }
        }
        flushGroup()
        flushText()
        let last = rows.lastIndex { if case .activity = $0 { return true } else { return false } }
        // The summary of a running turn: its newest calls fold too, all but the step the agent is
        // on, so switching levels changes what is in view at the bottom, not only rows above it.
        if running, !o.detail, let k = last, case .activity(let id, let es, _) = rows[k], es.count > 2, o.open[-(id * 8 + 2)] == nil {
            let now = es[es.count - 1]
            rows.replaceSubrange(k...k, with: [.activity(id: id, events: Array(es.dropLast()), open: false), .activity(id: now.seq, events: [now], open: nil)])
            return rows
        }
        // Calls are all the turn did: the turn's own line sums them up, so they need no line of their own.
        if open, rows.count == 1, case .activity(let id, let es, _) = rows[0] {
            return [.activity(id: id, events: es, open: nil)]
        }
        // While the turn runs, its newest calls are open: that is where the agent is now.
        if running, let k = last, case .activity(let id, let es, _) = rows[k], o.open[-(id * 8 + 2)] == nil {
            rows[k] = .activity(id: id, events: es, open: true)
        }
        return rows
    }

    /// The files the calls of a turn changed, with lines added and removed over all their edits.
    static func edited(_ items: [Event]) -> [EditedFile] {
        var files: [EditedFile] = []
        for e in items where e.type == .toolCall {
            for d in e.data["diffs"].array {
                let path = d["path"].str
                let (add, del) = lineCounts(old: d["old"].str, new: d["new"].str)
                if let i = files.firstIndex(where: { $0.path == path }) {
                    files[i].changes.append((d["old"].str, d["new"].str))
                    files[i].added += add
                    files[i].removed += del
                } else {
                    files.append(EditedFile(path: path, changes: [(d["old"].str, d["new"].str)], added: add, removed: del))
                }
            }
        }
        return files
    }

    /// Lines added and removed from `old` to `new`.
    static func lineCounts(old: String, new: String) -> (Int, Int) {
        let a = old.isEmpty ? [] : old.components(separatedBy: "\n"), b = new.isEmpty ? [] : new.components(separatedBy: "\n")
        var add = 0, del = 0
        for c in b.difference(from: a) {
            switch c { case .insert: add += 1; case .remove: del += 1 }
        }
        return (add, del)
    }

    /// How a tool call reads in one line: a verb for its kind, running or done, and what it
    /// worked on. `execute` shows its command, file tools the file's name.
    static func verb(_ kind: String, running: Bool) -> String {
        switch kind {
        case "execute": return running ? L.text("正在运行") : L.text("运行")
        case "read": return running ? L.text("正在读") : L.text("读")
        case "edit": return running ? L.text("正在改") : L.text("改")
        case "delete": return running ? L.text("正在删除") : L.text("删除")
        case "move": return running ? L.text("正在移动") : L.text("移动")
        case "search": return running ? L.text("正在搜索") : L.text("搜索")
        case "fetch": return running ? L.text("正在获取") : L.text("获取")
        case "think": return running ? L.text("正在思考") : L.text("思考")
        default: return ""
        }
    }

    /// The last `lines` lines of a tool's output, and whether earlier ones were left out.
    static func tail(_ output: String, lines: Int = 16) -> (String, Bool) {
        let all = output.components(separatedBy: "\n")
        guard all.count > lines else { return (output, false) }
        return (all.suffix(lines).joined(separator: "\n"), true)
    }
}
