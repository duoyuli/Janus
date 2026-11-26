import Foundation

/// What a session's events say about it now, beside the conversation itself: the agent's latest
/// plan, the controls it offers (model, effort, mode …) with their current values, its slash
/// commands, and its context and token usage. Read from the same events the chat shows.
// @project-doc docs/architecture/20261031-frontend.md#session_view
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

    struct Command: Equatable {
        let name: String
        let description: String
        let hint: String
        /// A skill the agent offers as a command, listed in a group of its own.
        var skill = false
    }

    struct Usage: Equatable {
        /// Context used and its size, when the agent reports them.
        var used: Int?
        var size: Int?
        /// Tokens of the last turn.
        var tokens: Int?
        var percent: Int? { if let used, let size, size > 0 { return min(100, used * 100 / size) } else { return nil } }

        /// This session's context, labelled: `本会话 上下文 26%` / `本会话 上下文 52,000 / 200,000`.
        /// It is a snapshot of one session, never part of the run's token total.
        var contextLabel: String? {
            guard let used else { return nil }
            guard let percent else { return L.text("本会话 上下文 \(usageNum(used))") }
            return L.text("本会话 上下文 \(percent)%")
        }

        /// The last turn's tokens, labelled: `上一轮 1,500 tokens`.
        var turnLabel: String? {
            guard let tokens else { return nil }
            return L.text("上一轮 \(usageNum(tokens)) tokens")
        }

        /// Both labels in the words the run page uses, for the help text and the ledger.
        var labels: String? {
            let parts = [contextLabel, turnLabel].compactMap { $0 }
            return parts.isEmpty ? nil : parts.joined(separator: " · ")
        }
    }

    /// The latest plan, if the agent published one.
    static func plan(_ evs: [Event]) -> [Step] {
        guard let e = evs.last(where: { $0.type == .agentPlan }) else { return [] }
        return e.data["entries"].array.map { Step(content: $0["content"].str, status: $0["status"].str) }
    }

    /// The session's controls as they stand: the last full list, then each switch since.
    static func controls(_ evs: [Event]) -> [Control] {
        guard let i = evs.lastIndex(where: { $0.type == .sessionControls }) else { return [] }
        let d = evs[i].data
        var out: [Control] = []
        func add(_ id: String, _ name: String, _ via: String, _ c: JSON) {
            let opts = c["options"].array.map { (value: $0["value"].str, name: $0["name"].truthy ? $0["name"].str : $0["value"].str) }
            guard opts.count > 1 else { return }
            out.append(Control(id: id, name: name, via: via, current: c["current"].str, options: opts))
        }
        if d["model"].truthy { add(d["model"]["id"].str, L.text("模型"), "config", d["model"]) }
        if d["effort"].truthy { add(d["effort"]["id"].str, L.text("推理强度"), "config", d["effort"]) }
        for c in d["controls"].array where c["type"].str == "select" || c["options"].array.count > 1 {
            add(c["id"].str, c["category"].str == "mode" ? L.text("模式") : c["name"].str, c["via"].str == "mode" ? "mode" : "config", c)
        }
        for e in evs[(i + 1)...] where e.type == .sessionControl && !e.data["failed"].truthy {
            if let k = out.firstIndex(where: { $0.id == e.data["id"].str || ($0.via == "mode" && e.data["via"].str == "mode") }) {
                out[k].current = e.data["value"].str
            }
        }
        return out
    }

    /// The slash commands the agent offers now.
    static func commands(_ evs: [Event]) -> [Command] {
        guard let e = evs.last(where: { $0.type == .sessionCommands }) else { return [] }
        return e.data["commands"].array.map { Command(name: $0["name"].str, description: $0["description"].str, hint: $0["hint"].str, skill: $0["skill"].bool ?? false) }
    }

    /// The commands matching what follows `/`, best first, in Lody's tiers: the name itself, a
    /// prefix of it, a prefix of one of its words (after `-` `_` `/` `:` `.`), anywhere in it,
    /// its letters in order, and last only the description. Within a tier the earlier and the
    /// shorter match wins. Nothing typed yet keeps the agent's order, its own commands first.
    static func rank(_ commands: [Command], _ query: String) -> [Command] {
        let q = query.lowercased()
        if q.isEmpty { return commands.filter { !$0.skill } + commands.filter(\.skill) }
        func score(_ c: Command) -> (Int, Int, Int)? {
            let n = c.name.lowercased()
            if n == q { return (0, 0, n.count) }
            if n.hasPrefix(q) { return (1, 0, n.count) }
            let words = n.split(whereSeparator: { "-_/:. ".contains($0) })
            if let w = words.firstIndex(where: { $0.hasPrefix(q) }) { return (2, w, n.count) }
            if let r = n.range(of: q) { return (3, n.distance(from: n.startIndex, to: r.lowerBound), n.count) }
            var rest = Substring(q)
            for ch in n where rest.first == ch { rest = rest.dropFirst() }
            if rest.isEmpty { return (4, 0, n.count) }
            if c.description.lowercased().contains(q) { return (5, 0, n.count) }
            return nil
        }
        return commands.compactMap { c in score(c).map { (c, $0) } }
            .enumerated()
            .sorted { a, b in a.element.1 != b.element.1 ? a.element.1 < b.element.1 : a.offset < b.offset }
            .map(\.element.0)
    }

    /// The arguments still to write after a command picked from the list: its hint, shown grey
    /// after `/name ` until the person types on.
    static func argumentHint(_ draft: String, _ commands: [Command]) -> String? {
        guard draft.hasPrefix("/"), draft.hasSuffix(" "), draft.filter({ $0 == " " }).count == 1 else { return nil }
        let name = String(draft.dropFirst().dropLast())
        guard let c = commands.first(where: { $0.name == name }), !c.hint.isEmpty else { return nil }
        return c.hint
    }

    static func usage(_ evs: [Event]) -> Usage? {
        var u = Usage()
        for e in evs where e.type == .sessionUsage {
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

    /// The Git facts a session works with, in one line: the repository, the branch accepting
    /// merges back into and the commit; where Janus keeps its copy is not the person's concern.
    /// `nil` for a run outside a worktree.
    static func gitNote(repo: String, branch: String, head: String) -> String? {
        guard !repo.isEmpty else { return nil }
        var parts = [L.text("Git 仓库 \(repo)"), branch.isEmpty ? "detached HEAD" : L.text("分支 \(branch)")]
        if !head.isEmpty { parts.append(L.text("提交 \(String(head.prefix(8)))")) }
        return parts.joined(separator: " · ")
    }

    /// `gitNote` of an attempt's start (`attempt.started`'s `data.git`).
    static func gitNote(_ g: JSON) -> String? {
        gitNote(repo: g["repo"].str, branch: g["branch"].str, head: g["head"].str)
    }

    /// `gitNote` of the run's start (`run.created`): where the lead works.
    static func runGitNote(_ d: JSON) -> String? {
        gitNote(repo: d["worktree"]["repo"].str, branch: d["worktree"]["branch"].str, head: d["baseRef"].str)
    }

    /// `gitNote` of a task's latest attempt.
    static func taskGitNote(_ st: RunState, _ task: String) -> String? {
        gitNote(st.events.last { $0.type == .attemptStarted && $0.task == task }?.data["git"] ?? .null)
    }

    /// What an attempt's header adds about how it started (`attempt.started`): which session it
    /// is in when not a plain new one, and why it runs again. A rework or a re-check continues
    /// the task's own conversation, so the header says so instead of looking like a new session.
    static func attemptNote(_ d: JSON, task: String?) -> String {
        var parts: [String] = []
        if d["resumed"].truthy { parts.append(L.text("接上了原会话")) }
        let c = d["continuation"]
        if c["loaded"].truthy {
            parts.append(c["from"].str == task ? L.text("接上原会话") : L.text("接上 \(c["from"].str) 的会话"))
        } else if c.exists {
            switch c["fallback"].str {
            case "context_over_75pct": parts.append(L.text("新开会话（原会话上下文超过 75%）"))
            case "unsupported": parts.append(L.text("新开会话（Agent 不支持加载会话）"))
            case "no_session": parts.append(L.text("新开会话（这个子任务之前没有会话）"))
            default: parts.append(L.text("新开会话（原会话加载失败）"))
            }
        }
        let r = d["reopen"]
        switch r["reason"].str {
        case "lead.retry" where r["from"].truthy: parts.append(L.text("按 \(r["from"].str) 的意见返工"))
        case "target.reworked": parts.append(L.text("\(r["target"].str) 返工后复审"))
        default: break
        }
        return parts.map { " · \($0)" }.joined()
    }
}
