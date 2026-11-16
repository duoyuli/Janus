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
    #expect(Library.origin(docs) == "来自 acme/kit · 来源中已删除")
    #expect(Library.endpoint(docs) == "SSE · https://d.example/sse")
    #expect(Library.vars(docs) == (text: "Header 1/2 已设置", complete: false))
    #expect(Library.usedBy(docs) == "研发 在用")
    #expect(Library.support(docs, runtime: "codex") == (state: "no", why: "Codex 不支持 SSE 传输的 MCP 服务器"))
    #expect(Library.support(docs, runtime: "claude").state == "yes")
    #expect(Library.support(docs, runtime: "").state == "yes", "no agent chosen yet: nothing to warn about")

    let skill: JSON = ["kind": "skill", "origin": ["type": "created"], "roles": []]
    #expect(Library.origin(skill) == "在 Janus 中新建")
    #expect(Library.vars(skill).complete)
    #expect(Library.usedBy(skill) == "还没有数字员工使用")
    let stdio: JSON = ["server": ["transport": "stdio", "command": "npx", "args": ["-y", "gh-mcp"]]]
    #expect(Library.endpoint(stdio) == "stdio · npx -y gh-mcp")
    #expect(Library.runtime(["base": "claude", "kind": "deepseek-claude"]) == "claude")
}
