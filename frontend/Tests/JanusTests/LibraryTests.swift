import Testing
@testable import Janus

/// An item as `GET /api/library` returns it.
private let docs: JSON = [
    "id": "i1", "kind": "mcp", "name": "docs",
    "origin": ["type": "git", "source": "s1", "path": "mcp.json#docs"],
    "source": ["url": "https://github.com/acme/kit", "inSource": false],
    "server": ["transport": "sse", "url": "https://d.example/sse", "headerKeys": ["Authorization", "X-Team"]],
    "vars": [["name": "Authorization", "set": true], ["name": "X-Team", "set": false]],
    "roles": [["id": "r1", "name": "研发"]],
    "support": [
        ["runtime": "claude", "support": ["state": "yes"]],
        ["runtime": "codex", "support": ["state": "no", "why": "Codex 不支持 SSE 传输的 MCP 服务器"]],
    ],
]

/// The library's rows say where an item came from, what is missing and who can't use it.
@Test func libraryItemsReadTheWayTheServerDescribesThem() {
    #expect(Library.origin(docs) == expectedCopy("来自 acme/kit · 来源中已删除", "From acme/kit · Removed upstream"))
    #expect(Library.endpoint(docs) == "SSE · https://d.example/sse")
    #expect(Library.vars(docs) == (text: expectedCopy("Header 1/2 已设置", "Header: 1 of 2 set"), complete: false))
    #expect(Library.usedBy(docs) == expectedCopy("研发 在用", "Used by 研发"))
    #expect(Library.support(docs, runtime: "codex") == (state: "no", why: expectedCopy("Codex 不支持 SSE 传输的 MCP 服务器", "Codex does not support MCP servers that use SSE")))
    #expect(Library.support(docs, runtime: "claude").state == "yes")
    #expect(Library.support(docs, runtime: "").state == "yes", "no agent chosen yet: nothing to warn about")

    let skill: JSON = ["kind": "skill", "origin": ["type": "created"], "roles": []]
    #expect(Library.origin(skill) == expectedCopy("在 Janus 中新建", "Created in Janus"))
    #expect(Library.vars(skill).complete)
    #expect(Library.usedBy(skill) == expectedCopy("还没有数字员工使用", "No digital employees use this resource"))
    let stdio: JSON = ["server": ["transport": "stdio", "command": "npx", "args": ["-y", "gh-mcp"]]]
    #expect(Library.endpoint(stdio) == "stdio · npx -y gh-mcp")
    #expect(Library.runtime(["base": "claude", "kind": "claude"]) == "claude")
}

/// A row says in one line what keeps an item from working; runtimes that can't use it show when unfolded.
@Test func libraryRowsWarnInOneLine() {
    #expect(LibraryView.warning(docs) == expectedCopy("Header 1/2 已设置 · 来源中已删除", "Header: 1 of 2 set · Removed upstream"))
    #expect(LibraryView.unsupported(docs) == [expectedCopy("Codex 不支持 SSE 传输的 MCP 服务器", "Codex does not support MCP servers that use SSE")])
    let fine: JSON = ["kind": "skill", "origin": ["type": "created"], "vars": []]
    #expect(LibraryView.warning(fine).isEmpty && LibraryView.unsupported(fine).isEmpty)
}

/// A new workspace starts with the lead and the general digital worker once that has an Agent;
/// before then, with every role that has one.
@Test func newWorkspacesStartWithTheLeadAndTheGeneralWorker() {
    let roles: [JSON] = [
        ["id": "coordinator", "connection": "codex-1", "lead": true],
        ["id": "generalist", "connection": "claude-1"],
        ["id": "reviewer", "connection": "claude-1"],
        ["id": "researcher", "connection": ""],
    ]
    #expect(WorkspaceEditor.preset(roles, lead: "coordinator") == ["coordinator", "generalist"])
    var unbound = roles
    unbound[1]["connection"] = ""
    #expect(WorkspaceEditor.preset(unbound, lead: "coordinator") == ["coordinator", "reviewer"])
}
