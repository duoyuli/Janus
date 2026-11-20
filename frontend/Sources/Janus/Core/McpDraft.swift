import Foundation

/// An MCP server being written in the library's editor: the same server as a form or as JSON in
/// Claude Code's `.mcp.json` shape, switched freely while the JSON reads as one server. Its values
/// are read from the service when the editor opens, shown and changed like the rest; saving sends
/// each one as it stands, so an emptied value is cleared.
// @project-doc docs/domains/20261116-resource_library.md#mcp
struct McpDraft: Equatable {
    enum Transport: String, CaseIterable { case stdio, http, sse }
    enum Process: String { case perRun, perSession }

    /// An environment variable (stdio) or HTTP header (http, sse) and its value.
    struct Var: Equatable {
        var name: String
        var value = ""
    }

    /// The library item being edited; empty for a new server.
    var id = ""
    var name = ""
    var description = ""
    var transport: Transport = .stdio
    var command = ""
    var args: [String] = []
    var url = ""
    var vars: [Var] = []
    var process: Process = .perRun

    init() {}

    /// A saved server, as `GET /api/library/items/{id}` returns it, with the values
    /// `GET /api/library/mcp/{id}/values` gives.
    init(item: JSON, values: JSON) {
        let s = item["server"]
        id = item["id"].str
        name = item["name"].str
        description = item["description"].str
        transport = Transport(rawValue: s["transport"].str) ?? .stdio
        command = s["command"].str
        args = s["args"].strings
        url = s["url"].str
        process = Process(rawValue: s["process"].str) ?? .perRun
        vars = item["vars"].array.map { Var(name: $0["name"].str, value: values[$0["name"].str].str) }
    }

    // MARK: The command line

    /// The command and its arguments as one would type them, quoted where needed.
    var line: String {
        command.isEmpty && args.isEmpty ? "" : ([command] + args).map(Self.quote).joined(separator: " ")
    }

    /// Takes a typed command line apart; an unclosed quote keeps the words read so far.
    mutating func setLine(_ text: String) {
        let w = Self.words(text) ?? Self.words(text + (text.contains("\"") ? "\"" : "'")) ?? []
        command = w.first ?? ""
        args = Array(w.dropFirst())
    }

    /// A command line split the way `split_command` in `backend/src/agents.rs` splits it: on
    /// spaces, with '…' and "…" keeping theirs. Nil for an unclosed quote.
    static func words(_ s: String) -> [String]? {
        var out: [String] = [], cur = "", quote: Character?, any = false
        for c in s {
            if let q = quote {
                if c == q { quote = nil } else { cur.append(c) }
            } else if c == "\"" || c == "'" {
                quote = c
                any = true
            } else if c.isWhitespace {
                if any || !cur.isEmpty { out.append(cur); cur = ""; any = false }
            } else {
                cur.append(c)
            }
        }
        if quote != nil { return nil }
        if any || !cur.isEmpty { out.append(cur) }
        return out
    }

    private static func quote(_ word: String) -> String {
        guard word.isEmpty || word.contains(where: { $0.isWhitespace || $0 == "\"" || $0 == "'" }) else { return word }
        return word.contains("'") ? "\"\(word)\"" : "'\(word)'"
    }

    // MARK: JSON

    /// The server as Claude Code's `.mcp.json` has it, values included.
    var json: String {
        let pairs = JSON.object(Dictionary(vars.filter { !$0.name.isEmpty }.map { ($0.name, JSON($0.value)) }, uniquingKeysWith: { _, b in b }))
        let entry: JSON = transport == .stdio
            ? ["type": "stdio", "command": JSON(command), "args": JSON(args), "env": pairs]
            : ["type": JSON(transport.rawValue), "url": JSON(url), "headers": pairs]
        // JSONSerialization writes `"key" : value`; READMEs and .mcp.json write `"key": value`. A
        // quote inside a string is escaped, so `" : ` only ever follows a key.
        return JSON.object(["mcpServers": .object([name: entry])]).pretty()
            .replacingOccurrences(of: #"(?<!\\)" : "#, with: "\": ", options: .regularExpression)
    }

    /// The servers in pasted or typed JSON: a README's `{"mcpServers": {…}}`, VS Code's
    /// `{"servers": {…}}`, one server with a `name`, or one server's entry alone. Nil when the
    /// text isn't JSON holding a server. Trailing commas are forgiven, as READMEs have them.
    static func servers(in text: String) -> [McpDraft]? {
        let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: #",\s*([}\]])"#, with: "$1", options: .regularExpression)
        guard let doc = try? JSON.parse(Data(cleaned.utf8)), case .object(let o) = doc else { return nil }
        let map = o["mcpServers"]?.object ?? o["servers"]?.object
        let found: [McpDraft]
        if let map {
            found = map.keys.sorted().compactMap { entry($0, map[$0] ?? .null) }
        } else if doc["command"].exists || doc["url"].exists {
            found = [entry(doc["name"].str, doc)].compactMap { $0 }
        } else {
            return nil
        }
        return found.isEmpty ? nil : found
    }

    private static func entry(_ name: String, _ e: JSON) -> McpDraft? {
        var d = McpDraft()
        d.name = name.trimmingCharacters(in: .whitespaces)
        d.description = e["description"].str
        let values = { (field: String) in
            e[field].object.keys.sorted().compactMap { k -> Var? in
                switch e[field][k] {
                case .string(let s): return Var(name: k, value: s)
                case .number(let n): return Var(name: k, value: n == n.rounded() ? String(Int(n)) : String(n))
                case .bool(let b): return Var(name: k, value: String(b))
                default: return nil
                }
            }
        }
        let type = e["type"].str.lowercased()
        if e["url"].str.isEmpty == false && !(type == "stdio" && e["command"].exists) {
            d.transport = type == "sse" ? .sse : .http
            d.url = e["url"].str
            d.vars = values("headers")
        } else if !e["command"].str.isEmpty || !e["command"].array.isEmpty {
            let parts = e["command"].array.isEmpty ? [e["command"].str] : e["command"].strings
            d.transport = .stdio
            d.command = parts.first ?? ""
            d.args = Array(parts.dropFirst()) + e["args"].strings
            d.vars = values("env")
        } else {
            return nil
        }
        return d
    }

    /// Takes the server read from the JSON view. The item, its description (when the JSON has
    /// none), how it runs and the order of its variables stay.
    mutating func take(_ read: McpDraft) {
        if !read.name.isEmpty { name = read.name }
        if !read.description.isEmpty { description = read.description }
        transport = read.transport
        command = read.command
        args = read.args
        url = read.url
        // JSON objects have no order: the variables keep the form's, new ones after.
        let order = Dictionary(vars.enumerated().map { ($1.name, $0) }, uniquingKeysWith: { a, _ in a })
        vars = read.vars.enumerated().sorted { (order[$0.element.name] ?? vars.count + $0.offset) < (order[$1.element.name] ?? vars.count + $1.offset) }.map(\.element)
    }

    // MARK: Saving

    /// The body of `POST /api/library/mcp` and `PUT /api/library/mcp/{id}`. The arguments go as a
    /// list, so the service doesn't split the command again; every value goes as it stands.
    var body: JSON {
        let named = vars.filter { !$0.name.trimmingCharacters(in: .whitespaces).isEmpty }
        return [
            "name": JSON(name.trimmingCharacters(in: .whitespaces)),
            "description": JSON(description),
            "transport": JSON(transport.rawValue),
            "command": JSON(command),
            "args": JSON(args),
            "url": JSON(url.trimmingCharacters(in: .whitespaces)),
            "process": JSON(process.rawValue),
            "vars": .array(named.map { ["name": JSON($0.name.trimmingCharacters(in: .whitespaces)), "value": JSON($0.value)] }),
        ]
    }
}
