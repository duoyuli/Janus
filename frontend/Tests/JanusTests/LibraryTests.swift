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
    #expect(Library.usedBy(docs) == expectedCopy("只给 研发", "Only for 研发"))
    #expect(Library.support(docs, runtime: "codex") == (state: "no", why: expectedCopy("Codex 不支持 SSE 传输的 MCP 服务器", "Codex does not support MCP servers that use SSE")))
    #expect(Library.support(docs, runtime: "claude").state == "yes")
    #expect(Library.support(docs, runtime: "").state == "yes", "no agent chosen yet: nothing to warn about")

    let skill: JSON = ["kind": "skill", "origin": ["type": "created"], "roles": []]
    #expect(Library.origin(skill) == expectedCopy("在 Janus 中新建", "Created in Janus"))
    #expect(Library.vars(skill).complete)
    #expect(Library.usedBy(skill) == expectedCopy("还没有给任何数字员工", "Not given to any digital employee yet"))
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

/// Skills fold into groups by where they came from: every GitHub source in the order added (also
/// one nothing is imported from yet), then each folder they were copied out of, then the ones
/// written in Janus. The group is a field of the origin, not read from the item's id.
@Test func skillsGroupByWhereTheyCameFrom() {
    let sources: [JSON] = [
        ["id": "s1", "repo": "acme/kit", "path": "", "gitRef": ""],
        ["id": "s2", "repo": "acme/more", "path": "skills", "gitRef": "v2"],
    ]
    let skills: [JSON] = [
        ["id": "a", "origin": ["type": "created"]],
        ["id": "b", "origin": ["type": "folder", "path": "/u/skills/pdf"]],
        ["id": "c", "origin": ["type": "git", "source": "s1", "path": "x"]],
        ["id": "d", "origin": ["type": "folder", "path": "/u/skills/docx"]],
        ["id": "e", "origin": ["type": "folder", "path": "/a/one"]],
        ["id": "f", "origin": ["type": "git", "source": "s1", "path": "y"]],
    ]
    let groups = Library.skillGroups(skills, sources: sources)
    #expect(groups.map(\.origin) == [.source("s1"), .source("s2"), .folder("/a"), .folder("/u/skills"), .created])
    #expect(groups.map { $0.items.map { $0["id"].str } } == [["c", "f"], [], ["e"], ["b", "d"], ["a"]])
    #expect(Library.skillGroups([], sources: []).isEmpty, "no sources and no skills: no groups")
    #expect(Library.sourceName(sources[0]) == "acme/kit")
    #expect(Library.sourceName(sources[1]) == "acme/more/skills @v2")
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

/// The worker editor sums up what every digital worker has before the kept items to tick.
@Test func theWorkerEditorSumsUpWhatEveryoneHas() {
    let items: [JSON] = [["kind": "skill"], ["kind": "skill"], ["kind": "mcp"]]
    #expect(RoleEditor.everyoneLine(items) == expectedCopy("给所有数字员工的 2 个 Skill、1 个 MCP，自动就有", "For all digital employees, included automatically: 2 skills, 1 MCP servers"))
    #expect(RoleEditor.everyoneLine([]) == expectedCopy("资源库里还没有给所有数字员工的资源", "The Resource Library has nothing for all digital employees yet"))
}

/// The MCP editor's form and JSON are one server, values included: written out as Claude Code's
/// `.mcp.json` and read back the same.
@Test func mcpServersReadTheSameAsFormOrJson() throws {
    let item: JSON = [
        "id": "i1", "name": "github", "description": "代码托管",
        "server": ["transport": "stdio", "command": "npx", "args": ["-y", "gh mcp"], "process": "perSession"],
        "vars": [["name": "GITHUB_TOKEN", "set": true], ["name": "EMPTY", "set": false]],
    ]
    var d = McpDraft(item: item, values: ["GITHUB_TOKEN": "ghp-1"])
    #expect(d.line == "npx -y 'gh mcp'")
    #expect(d.process == .perSession && d.vars == [McpDraft.Var(name: "GITHUB_TOKEN", value: "ghp-1"), McpDraft.Var(name: "EMPTY")])
    #expect(d.json.contains(#""GITHUB_TOKEN": "ghp-1""#), "the JSON shows the value")

    // Out as JSON and back: the same server.
    let read = try #require(McpDraft.servers(in: d.json))
    #expect(read.count == 1 && read[0].command == "npx" && read[0].args == ["-y", "gh mcp"])
    var back = d
    back.take(read[0])
    #expect(back == d, "nothing changes on a round trip")
    #expect(back.body["vars"] == [["name": "GITHUB_TOKEN", "value": "ghp-1"], ["name": "EMPTY", "value": ""]], "every value goes as it stands")
    #expect(back.body["args"] == ["-y", "gh mcp"], "arguments go as a list, not split again")

    // A typed command line, quotes and all; an unclosed quote doesn't lose what is typed.
    d.setLine(#"uvx  mcp-server-fetch --ua "Janus bot""#)
    #expect(d.command == "uvx" && d.args == ["mcp-server-fetch", "--ua", "Janus bot"])
    d.setLine(#"node "my server"#)
    #expect(d.command == "node" && d.args == ["my server"])
    #expect(McpDraft.words(#"a "b"#) == nil)

    // What READMEs paste: several servers, trailing commas, VS Code's servers, a bare entry.
    let pasted = try #require(McpDraft.servers(in: """
        {"mcpServers": {
          "docs": {"type": "http", "url": "https://d.example/mcp", "headers": {"Authorization": "Bearer t"},},
          "events": {"type": "sse", "url": "https://e.example/sse"},
          "fs": {"command": ["node", "fs.js"], "args": ["--root", "."], "env": {"PORT": 8080}},
        }}
        """))
    #expect(pasted.map(\.name) == ["docs", "events", "fs"])
    #expect(pasted[0].transport == .http && pasted[0].vars == [McpDraft.Var(name: "Authorization", value: "Bearer t")])
    #expect(pasted[1].transport == .sse)
    #expect(pasted[2].command == "node" && pasted[2].args == ["fs.js", "--root", "."] && pasted[2].vars[0].value == "8080")
    #expect(McpDraft.servers(in: #"{"servers": {"a": {"command": "a-mcp"}}}"#)?.first?.name == "a")
    #expect(McpDraft.servers(in: #"{"url": "https://x.example/mcp"}"#)?.first?.transport == .http)
    #expect(McpDraft.servers(in: "[]") == nil && McpDraft.servers(in: #"{"name": "x"}"#) == nil && McpDraft.servers(in: "{") == nil)
}

/// The JSON reads as .mcp.json files and READMEs write it.
@Test func mcpJsonIsWrittenTheUsualWay() {
    var d = McpDraft()
    d.name = "docs"
    d.transport = .http
    d.url = "https://d.example/mcp"
    d.vars = [McpDraft.Var(name: "X-Note", value: #"a " : b"#)]
    #expect(d.json.contains(#""url": "https://d.example/mcp""#))
    #expect(d.json.contains(#""X-Note": "a \" : b""#), "a value's own colon is left alone")
}

/// The picks are each one thing: ids and names unique, names the service accepts, every server
/// either started by a command or reached at an https address.
@Test func catalogPicksAreWellFormed() {
    #expect(Set(Catalog.skills.map(\.id)).count == Catalog.skills.count)
    #expect(Set(Catalog.servers.map(\.name)).count == Catalog.servers.count)
    for s in Catalog.servers {
        #expect(s.name.allSatisfy { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" } && !s.name.lowercased().hasPrefix("janus"), "\(s.name)")
        #expect(s.transport == .stdio ? !s.command.isEmpty && s.url.isEmpty : s.url.hasPrefix("https://") && s.command.isEmpty, "\(s.name)")
    }
    for s in Catalog.skills {
        #expect(s.repo.split(separator: "/").count == 2 && s.path.hasPrefix("skills/"), "\(s.id)")
    }
}

/// A pick says when the library has it already, and a server that needs a key starts the editor
/// with the key's place written up to the user's part.
@Test func catalogPicksKnowWhatTheLibraryHas() throws {
    let sources: [JSON] = [["id": "s1", "repo": "anthropics/skills", "path": "", "gitRef": ""]]
    let pdf = try #require(Catalog.skills.first { $0.path == "skills/pdf" })
    let docx = try #require(Catalog.skills.first { $0.path == "skills/docx" })
    let items: [JSON] = [
        ["kind": "skill", "name": "pdf-tools", "origin": ["type": "git", "source": "s1", "path": "skills/pdf"]],
        ["kind": "mcp", "name": "my-fetch", "server": ["transport": "stdio", "command": "uvx", "args": ["mcp-server-fetch"]]],
    ]
    #expect(Catalog.item(for: pdf, items: items, sources: sources)?["name"] == "pdf-tools", "the same folder of the same repository")
    #expect(Catalog.item(for: docx, items: items, sources: sources) == nil)
    let fetch = try #require(Catalog.servers.first { $0.name == "fetch" })
    #expect(Catalog.item(for: fetch, items: items)?["name"] == "my-fetch", "the same command")
    #expect(fetch.oneClick && fetch.draft.command == "uvx" && fetch.draft.args == ["mcp-server-fetch"])

    let github = try #require(Catalog.servers.first { $0.name == "github" })
    #expect(!github.oneClick)
    #expect(github.draft.vars == [McpDraft.Var(name: "Authorization", value: "Bearer ")])
    let context7 = try #require(Catalog.servers.first { $0.name == "context7" })
    #expect(context7.oneClick && context7.draft.vars.isEmpty, "an optional key is added later, not left empty")
}
