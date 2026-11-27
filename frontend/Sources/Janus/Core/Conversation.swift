import Foundation

/// One conversation as the server lays it out (`backend/src/run_view/turns.rs` and `aside.rs`):
/// its rows in turns at the display level the app asked for, the agent's plan, the controls and
/// commands it offers, this session's usage, its Git facts, the messages on their way to it and
/// the requests waiting above its composer. The app decodes them and applies only the person's own
/// clicks (`rows(_:events:open:)`); it groups and words nothing itself
/// (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#session_turns
enum Conversation {
    /// The space above a row: turns and sections stand apart, the blocks of one turn sit close.
    enum Gap: String {
        case none, round, block, half, line, loose
    }

    /// A file a turn changed: its lines added and removed over all its edits, and each edit as
    /// the event and the place of the change in its `diffs` (`RunState.diffs`).
    struct EditedFile: Equatable {
        let path: String
        let added: Int
        let removed: Int
        let changes: [Change]
        struct Change: Equatable { let seq: Int; let index: Int }

        init(_ j: JSON) {
            path = j["path"].str
            added = j["added"].int ?? 0
            removed = j["removed"].int ?? 0
            changes = j["changes"].array.map { Change(seq: $0["seq"].int ?? 0, index: $0["index"].int ?? 0) }
        }
    }

    /// The agent's line at the top of its turn.
    struct Head: Equatable {
        let turn: Int
        let agent: String
        let running: Bool
        let start: Double
        let end: Double
        /// What the turn's tools did, after a running turn's clock.
        let stats: String
        /// A finished turn's line: how long it worked and what it did.
        let summary: String
    }

    /// One row as it shows now. Ids are the server's: stable while events arrive.
    enum Row: Identifiable, Equatable {
        /// A section's opening event and what Janus injected under it.
        case section(Event, injected: [Event], gap: Gap)
        /// What opened a turn, or any single event shown as itself.
        case item(Event, gap: Gap)
        /// `open` is nil when the turn did nothing but reply.
        case head(id: Int, Head, open: Bool?, gap: Gap)
        /// Calls and thinking in a row under one line (`summary`); `open` nil: no line of their own.
        case activity(id: Int, events: [Event], open: Bool?, summary: String, gap: Gap)
        /// The agent's text, `reply` when it ended the turn.
        case text(id: Int, events: [Event], reply: Bool, gap: Gap)
        case edits(id: Int, files: [EditedFile], gap: Gap)

        var id: Int {
            switch self {
            case .section(let e, _, _), .item(let e, _): return e.seq
            case .head(let id, _, _, _), .activity(let id, _, _, _, _), .text(let id, _, _, _), .edits(let id, _, _): return id
            }
        }

        var gap: Gap {
            switch self {
            case .section(_, _, let g), .item(_, let g), .head(_, _, _, let g), .activity(_, _, _, _, let g), .text(_, _, _, let g), .edits(_, _, let g): return g
            }
        }
    }

    /// The rows the server laid out (`rows` of a conversation), with the person's own clicks
    /// (`open`, by row id) applied: a folded turn leaves out what folds with it, an opened group of
    /// calls shows open, and the running turn's group split off at the step the agent is on shows
    /// whole once clicked. `events` finds an event by its seq.
    static func rows(_ served: JSON, events: @escaping (Int) -> Event?, open: [Int: Bool]) -> [Row] {
        let all = served.array
        let evs = { (j: JSON) in j.array.compactMap { $0.int.flatMap(events) } }
        // Each turn's line as it stands: the person's click, else how it starts.
        var folded = Set<Int>()
        for r in all where r["kind"].str == "head" {
            guard let start = r["open"].bool, let id = r["id"].int, let turn = r["turn"].int else { continue }
            if !(open[id] ?? start) { folded.insert(turn) }
        }
        var out: [Row] = []
        for r in all {
            let gap = Gap(rawValue: r["gap"].str) ?? .none
            let id = r["id"].int ?? 0
            let hidden = r["fold"].bool == true && r["turn"].int.map(folded.contains) == true
            if hidden { continue }
            switch r["kind"].str {
            case "section":
                guard let e = r["seq"].int.flatMap(events) else { continue }
                out.append(.section(e, injected: evs(r["injected"]), gap: gap))
            case "item":
                guard let e = r["seq"].int.flatMap(events) else { continue }
                out.append(.item(e, gap: gap))
            case "head":
                let h = Head(turn: r["turn"].int ?? 0, agent: r["agent"].str, running: r["running"].bool ?? false,
                             start: r["start"].double ?? 0, end: r["end"].double ?? 0, stats: r["stats"].str, summary: r["summary"].str)
                out.append(.head(id: id, h, open: r["open"].bool.map { open[id] ?? $0 }, gap: gap))
            case "activity":
                // The step the agent is on, split off its group: the group shows whole once clicked.
                if let whole = r["partOf"].int, open[whole] != nil { continue }
                var shown = evs(r["events"])
                var state = r["open"].bool
                if let clicked = open[id] {
                    if r["whole"].exists {
                        shown = evs(r["whole"])
                        state = r["sole"].bool == true ? nil : clicked
                    } else if state != nil {
                        state = clicked
                    }
                }
                guard !shown.isEmpty else { continue }
                out.append(.activity(id: id, events: shown, open: state, summary: r["summary"].str, gap: gap))
            case "text":
                let shown = evs(r["events"])
                guard !shown.isEmpty else { continue }
                out.append(.text(id: id, events: shown, reply: r["reply"].bool ?? false, gap: gap))
            case "edits":
                out.append(.edits(id: id, files: r["files"].array.map(EditedFile.init), gap: gap))
            default:
                continue
            }
        }
        return out
    }

    // MARK: Beside the conversation

    struct Step: Equatable { let content: String; let status: String }

    /// A control the agent offers for its session, in its own terms; `label` is its name and the
    /// current choice's.
    struct Control: Equatable, Identifiable {
        let id: String
        let name: String
        /// `mode` for an ACP session mode, `config` for a config option.
        let via: String
        let current: String
        let label: String
        let options: [(value: String, name: String)]
        static func == (a: Control, b: Control) -> Bool { a.id == b.id && a.current == b.current && a.label == b.label }

        init(_ j: JSON) {
            id = j["id"].str; name = j["name"].str; via = j["via"].str; current = j["current"].str; label = j["label"].str
            options = j["options"].array.map { (value: $0["value"].str, name: $0["name"].str) }
        }
    }

    /// This session's context and its last turn, as the meter shows them.
    struct Usage: Equatable {
        let percent: Int?
        /// `26%`, or `15.4K tokens` without a size.
        let meter: String
        let labels: String
        let help: String

        init(_ j: JSON) {
            percent = j["percent"].int; meter = j["meter"].str; labels = j["labels"].str; help = j["help"].str
        }
    }

    /// What a section's opening event says about its session: an attempt's note after its
    /// runtime, and where it works.
    struct Note: Equatable {
        let note: String
        let git: String?
    }

    /// A message on its way to the session, and the tray's badge for it.
    struct Pending: Equatable {
        let id: String
        let text: String
        let delivery: MessageDelivery
        let label: String
    }

    /// What the composer offers for a draft (`GET /api/runs/{id}/suggest`).
    struct Suggestions: Equatable {
        struct Row: Equatable {
            let label: String
            let detail: String
            /// The heading above this row, when it starts a group.
            let group: String?
            /// The draft once this row is picked.
            let completion: String
        }

        /// The draft these are for.
        var draft = ""
        var rows: [Row] = []
        /// The arguments a command picked from the list still takes.
        var ghost: String?

        init() {}

        init(_ j: JSON, draft: String) {
            self.draft = draft
            rows = j["rows"].array.map { Row(label: $0["label"].str, detail: $0["detail"].str, group: $0["group"].string, completion: $0["completion"].str) }
            ghost = j["ghost"].string
        }
    }
}

/// One file change an event carries, as the server worked it out (`RunView.diffs`): the changed
/// lines with three lines of context.
struct ServedDiff: Equatable {
    let path: String
    let added: Int
    let removed: Int
    /// `+` added, `-` removed, ` ` kept, and the line.
    let lines: [(mark: String, text: String)]
    static func == (a: ServedDiff, b: ServedDiff) -> Bool { a.path == b.path && a.lines.map(\.text) == b.lines.map(\.text) }

    init(_ j: JSON) {
        path = j["path"].str
        added = j["added"].int ?? 0
        removed = j["removed"].int ?? 0
        lines = j["lines"].array.map { (mark: $0["mark"].str, text: $0["text"].str) }
    }
}

/// A Markdown document of the run, as the 文档 menu lists it (`RunView.docs`).
struct RunDoc: Equatable {
    enum Source: String { case deliverable, artifact, mentioned }
    let path: String
    let source: Source
    let seq: Int
    /// The path from the run's directory, or with `~`.
    let label: String
    let name: String
    /// The folder it is in, with `~`.
    let dir: String

    init(_ j: JSON) {
        path = j["path"].str
        source = Source(rawValue: j["source"].str) ?? .mentioned
        seq = j["seq"].int ?? 0
        label = j["label"].str
        name = j["name"].str
        dir = j["dir"].str
    }
}

/// What a conversation's session is writing and running right now, before it is on record, as
/// the run's stream sends it (`live` frames: `backend/src/run_view/preview.rs`).
// @project-doc docs/architecture/20261031-frontend.md#live_preview
struct LivePreview: Equatable {
    struct Tool: Equatable {
        let title: String
        let tool: String
        let input: String
        let output: String
        let status: String
        /// The call in one line as it stands now, as the server words it.
        let verb: String
        let what: String
        let mono: Bool
    }

    var text = ""
    var thought = ""
    var tools: [String: Tool] = [:]

    init(_ j: JSON) {
        text = j["text"].str
        thought = j["thought"].str
        tools = j["tools"].object.mapValues {
            Tool(title: $0["title"].str, tool: $0["tool"].str, input: $0["input"].str, output: $0["output"].str, status: $0["status"].str,
                 verb: $0["verb"].str, what: $0["what"].str, mono: $0["mono"].bool ?? false)
        }
    }
}

/// One event of a conversation in one line, as the server words it (`backend/src/run_view/lines.rs`).
/// A reply or a thought has none: it is the agent's own text.
// @project-doc docs/architecture/20261031-frontend.md#session_turns
enum ServedLine {
    struct Bubble { let text: String; let images: [String]; let via: String; let insert: Bool }
    struct Tool {
        let board: Bool
        let tag: String
        let verb: String
        let what: String
        let mono: Bool
        let running: Bool
        let failed: Bool
        let waiting: Bool
        let summary: String
        let files: String
    }

    case divider(text: String, sub: String?, git: String?, images: [String])
    case attempt(title: String, git: String?, brief: String)
    case note(String, Tone?)
    case prompt(label: String, hint: String)
    case bubbles([Bubble])
    case result(title: String, pills: [(String, Tone)], evidence: String?, risks: String?)
    case request(key: String, open: Bool, text: String, tone: Tone?, docked: String)
    case tool(Tool)

    init?(_ j: JSON) {
        let tone = Tone(rawValue: j["tone"].str)
        switch j["kind"].str {
        case "divider": self = .divider(text: j["text"].str, sub: j["sub"].string, git: j["git"].string, images: j["images"].strings)
        case "attempt": self = .attempt(title: j["title"].str, git: j["git"].string, brief: j["brief"].str)
        case "note": self = .note(j["text"].str, tone)
        case "prompt": self = .prompt(label: j["label"].str, hint: j["hint"].str)
        case "bubbles": self = .bubbles(j["items"].array.map { Bubble(text: $0["text"].str, images: $0["images"].strings, via: $0["via"].str, insert: $0["insert"].bool ?? false) })
        case "result":
            self = .result(title: j["title"].str, pills: j["pills"].array.map { ($0["text"].str, Tone(rawValue: $0["tone"].str) ?? .gray) },
                           evidence: j["evidence"].string, risks: j["risks"].string)
        case "request": self = .request(key: j["key"].str, open: j["open"].bool ?? false, text: j["text"].str, tone: tone, docked: j["docked"].str)
        case "tool":
            self = .tool(Tool(board: j["board"].bool ?? false, tag: j["tag"].str, verb: j["verb"].str, what: j["what"].str, mono: j["mono"].bool ?? false,
                              running: j["running"].bool ?? false, failed: j["failed"].bool ?? false, waiting: j["waiting"].bool ?? false,
                              summary: j["summary"].str, files: j["files"].str))
        default: return nil
        }
    }
}

/// The card a request waiting on the person is answered with, as the server words it
/// (`backend/src/run_view/cards.rs`), its form already read into fields.
// @project-doc docs/architecture/20261031-frontend.md#request_cards
struct ServedCard {
    enum Kind: String { case question, decision, approval, link, form, session }
    struct Field {
        enum Kind: String { case choice, multi, bool, number, line, text }
        struct Choice: Hashable { let value: JSON; let label: String; let desc: String }
        let key: String
        let title: String
        let description: String
        let required: Bool
        let kind: Kind
        let choices: [Choice]
        /// The field for the person's own answer beside the choices, and its placeholder.
        let custom: (key: String, placeholder: String)?

        init(_ j: JSON) {
            key = j["key"].str; title = j["title"].str; description = j["description"].str
            required = j["required"].bool ?? false
            kind = Kind(rawValue: j["kind"].str) ?? .text
            choices = j["choices"].array.map { Choice(value: $0["value"], label: $0["label"].str, desc: $0["desc"].str) }
            custom = j["custom"].isNull ? nil : (key: j["custom"]["key"].str, placeholder: j["custom"]["placeholder"].str)
        }
    }

    let kind: Kind
    let key: String
    let seq: Int
    let ts: Double
    let pill: (String, Tone)
    let agent: String
    let title: String
    let text: String
    let reason: String
    let command: String
    let url: String
    let facts: [(String, String)]
    let options: [String]
    let recommend: String
    let actions: [QuickAction]
    let impact: String
    let always: String
    let fields: [Field]
    let footnote: String
    /// The conversation a session's card opens.
    let session: SessionTarget?

    init(_ j: JSON) {
        kind = Kind(rawValue: j["kind"].str) ?? .session
        key = j["key"].str
        seq = j["seq"].int ?? 0
        ts = j["ts"].double ?? 0
        pill = (j["pill"]["text"].str, Tone(rawValue: j["pill"]["tone"].str) ?? .amber)
        agent = j["agent"].str
        title = j["title"].str
        text = j["text"].str
        reason = j["reason"].str
        command = j["command"].str
        url = j["url"].str
        facts = j["facts"].array.map { ($0[0].str, $0[1].str) }
        options = j["options"].strings
        recommend = j["recommend"].str
        actions = j["actions"].array.map(QuickAction.init)
        impact = j["impact"].str
        always = j["always"].str
        fields = j["fields"].array.map(Field.init)
        footnote = j["footnote"].str
        session = SessionTarget(label: j["session"].str)
    }
}

extension Conversation {
    /// What a composer shows and offers, as the server works it out from its session: the state
    /// in a pill, the buttons, the placeholder (`keys`: add how to paste and send) and whether it
    /// takes a message.
    struct Mode {
        enum Act: String { case message, interrupt, takeover, release, finish, resumeRun }
        struct Button { let act: Act; let label: String; let primary: Bool; let danger: Bool }
        let state: String
        let tone: Tone
        let buttons: [Button]
        let placeholder: String
        let keys: Bool
        let enabled: Bool

        init(_ j: JSON) {
            state = j["state"].str
            tone = Tone(rawValue: j["tone"].str) ?? .gray
            buttons = j["buttons"].array.compactMap { b in
                Act(rawValue: b["act"].str).map { Button(act: $0, label: b["label"].str, primary: b["primary"].bool ?? false, danger: b["danger"].bool ?? false) }
            }
            placeholder = j["placeholder"].str
            keys = j["keys"].bool ?? false
            enabled = j["enabled"].bool ?? false
        }
    }
}
