import Foundation

/// An MCP server being written in the library's editor: the same server as a form or as JSON in
/// Claude Code's `.mcp.json` shape. The form's fields are bound here; taking a typed command line
/// or JSON apart, writing the server out and the body to save it with are the service's
/// (`POST /api/library/mcp/draft`, `backend/src/setup_view/mcp_draft.rs`).
// @project-doc docs/domains/20261116-resource_library.md#mcp
struct McpDraft: Equatable {
    enum Transport: String, CaseIterable { case stdio, http, sse }

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

    init() {}

    /// The draft as the service sends it.
    init(_ j: JSON) {
        id = j["id"].str
        name = j["name"].str
        description = j["description"].str
        transport = Transport(rawValue: j["transport"].str) ?? .stdio
        command = j["command"].str
        args = j["args"].strings
        url = j["url"].str
        vars = j["vars"].array.map { Var(name: $0["name"].str, value: $0["value"].str) }
    }

    /// The draft as the service takes it.
    var wire: JSON {
        ["id": JSON(id), "name": JSON(name), "description": JSON(description), "transport": JSON(transport.rawValue),
         "command": JSON(command), "args": JSON(args), "url": JSON(url),
         "vars": .array(vars.map { ["name": JSON($0.name), "value": JSON($0.value)] })]
    }
}

/// What the editor draws after an edit, as the service answers it: the draft with the typed
/// command line or JSON taken in, written both ways, the body to save it with, the servers a new
/// server's JSON holds when it holds more than one, and why the JSON can't be taken.
struct McpEdit: Equatable {
    var draft: McpDraft
    var line: String
    var text: String
    var body: JSON
    var several: [String]
    var error: String?

    init(_ j: JSON) {
        draft = McpDraft(j["draft"])
        line = j["line"].str
        text = j["json"].str
        body = j["body"]
        several = j["several"].strings
        error = j["error"].string
    }
}

extension AppStore {
    /// One edit in the MCP editor: the draft with what was just typed, as the service reads it.
    func mcpEdit(_ d: McpDraft, line: String? = nil, text: String? = nil) async throws -> McpEdit {
        var body: JSON = ["draft": d.wire]
        if let line { body["line"] = JSON(line) }
        if let text { body["text"] = JSON(text) }
        return McpEdit(try await api.post("/api/library/mcp/draft?lang=\(L.serverLanguage)", body))
    }
}
