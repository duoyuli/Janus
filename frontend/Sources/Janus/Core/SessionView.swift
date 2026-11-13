import Foundation

/// What a session's events say about it now, beside the conversation itself: the agent's latest
/// plan, the controls it offers (model, effort, mode …) with their current values, its slash
/// commands, and its context and token usage. Read from the same events the chat shows.
// @project-doc docs/architecture/frontend.md#session_view
enum SessionView {
    struct Step: Equatable { let content: String; let status: String }

    struct Control: Equatable, Identifiable {
        let id: String
        let name: String
        /// `mode` for an ACP session mode, `config` for a config option.
        let via: String
        var current: String
        let options: [(value: String, name: String)]
        static func == (a: Control, b: Control) -> Bool { a.id == b.id && a.current == b.current && a.options.map(\.value) == b.options.map(\.value) }
    }

    struct Command: Equatable { let name: String; let description: String; let hint: String }

    struct Usage: Equatable {
        /// Context used and its size, when the agent reports them.
        var used: Int?
        var size: Int?
        /// Tokens of the last turn.
        var tokens: Int?
        var percent: Int? { if let used, let size, size > 0 { return min(100, used * 100 / size) } else { return nil } }
    }

    /// The latest plan, if the agent published one.
    static func plan(_ evs: [Event]) -> [Step] {
        guard let e = evs.last(where: { $0.type == "agent.plan" }) else { return [] }
        return e.data["entries"].array.map { Step(content: $0["content"].str, status: $0["status"].str) }
    }

    /// The session's controls as they stand: the last full list, then each switch since.
    static func controls(_ evs: [Event]) -> [Control] {
        guard let i = evs.lastIndex(where: { $0.type == "session.controls" }) else { return [] }
        let d = evs[i].data
        var out: [Control] = []
        func add(_ id: String, _ name: String, _ via: String, _ c: JSON) {
            let opts = c["options"].array.map { (value: $0["value"].str, name: $0["name"].truthy ? $0["name"].str : $0["value"].str) }
            guard opts.count > 1 else { return }
            out.append(Control(id: id, name: name, via: via, current: c["current"].str, options: opts))
        }
        if d["model"].truthy { add(d["model"]["id"].str, "模型", "config", d["model"]) }
        if d["effort"].truthy { add(d["effort"]["id"].str, "推理强度", "config", d["effort"]) }
        for c in d["controls"].array where c["type"].str == "select" || c["options"].array.count > 1 {
            add(c["id"].str, c["category"].str == "mode" ? "模式" : c["name"].str, c["via"].str == "mode" ? "mode" : "config", c)
        }
        for e in evs[(i + 1)...] where e.type == "session.control" && !e.data["failed"].truthy {
            if let k = out.firstIndex(where: { $0.id == e.data["id"].str || ($0.via == "mode" && e.data["via"].str == "mode") }) {
                out[k].current = e.data["value"].str
            }
        }
        return out
    }

    /// The slash commands the agent offers now.
    static func commands(_ evs: [Event]) -> [Command] {
        guard let e = evs.last(where: { $0.type == "session.commands" }) else { return [] }
        return e.data["commands"].array.map { Command(name: $0["name"].str, description: $0["description"].str, hint: $0["hint"].str) }
    }

    static func usage(_ evs: [Event]) -> Usage? {
        var u = Usage()
        for e in evs where e.type == "session.usage" {
            // An agent that reports 0 before it has read anything says nothing yet.
            if let used = e.data["used"].int, used > 0 { u.used = used }
            if let size = e.data["size"].int { u.size = size }
            if let t = e.data["totalTokens"].int { u.tokens = t }
        }
        return u.used == nil && u.tokens == nil ? nil : u
    }

    /// The token being typed at the end of a draft, when it asks for a suggestion: a command
    /// (`/re` at the start, no space yet) or a file (`@sr` as the last word).
    enum Asking: Equatable { case command(String), file(String) }
    static func asking(_ draft: String) -> Asking? {
        if draft.hasPrefix("/"), !draft.contains(where: \.isWhitespace) { return .command(String(draft.dropFirst())) }
        let last = draft.split(omittingEmptySubsequences: false, whereSeparator: \.isWhitespace).last.map(String.init) ?? ""
        if last.hasPrefix("@") { return .file(String(last.dropFirst())) }
        return nil
    }

    /// The draft with the token being typed replaced by the pick, and a space to go on typing.
    static func complete(_ draft: String, with pick: Asking) -> String {
        switch pick {
        case .command(let name): return "/\(name) "
        case .file(let path):
            let cut = draft.lastIndex(where: \.isWhitespace).map { draft.index(after: $0) } ?? draft.startIndex
            return String(draft[..<cut]) + "@\(path) "
        }
    }

    static func tokens(_ n: Int) -> String { n >= 1000 ? String(format: "%.1fk", Double(n) / 1000) : "\(n)" }
}
