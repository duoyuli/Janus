import Testing
@testable import Janus

/// An item as `GET /api/library` returns it.
private let docs: JSON = [
    "id": "i1", "kind": "mcp", "name": "docs",
    "origin": ["type": "git", "source": "s1", "path": "mcp.json#docs"],
    "source": ["url": "https://github.com/acme/kit", "inSource": false],
    "server": ["transport": "sse", "url": "https://d.example/sse", "headerKeys": ["Authorization", "X-Team"]],
    "vars": [["name": "Authorization", "set": true], ["name": "X-Team", "set": false]],
]

/// The library's rows say where an item came from, where a server is and what is missing.
@Test func libraryItemsReadTheWayTheServerDescribesThem() {
    #expect(Library.origin(docs) == expectedCopy("来自 acme/kit · 来源中已删除", "From acme/kit · Removed upstream"))
    #expect(Library.endpoint(docs) == "SSE · https://d.example/sse")
    #expect(Library.vars(docs) == (text: expectedCopy("Header 1/2 已设置", "Header: 1 of 2 set"), complete: false))

    let skill: JSON = ["kind": "skill", "origin": ["type": "created"]]
    #expect(Library.origin(skill) == expectedCopy("在 Janus 中新建", "Created in Janus"))
    #expect(Library.vars(skill).complete)
    let stdio: JSON = ["server": ["transport": "stdio", "command": "npx", "args": ["-y", "gh-mcp"]]]
    #expect(Library.endpoint(stdio) == "stdio · npx -y gh-mcp")
}

/// A row says in one line what keeps an item from working.
@Test func libraryRowsWarnInOneLine() {
    #expect(LibraryView.warning(docs) == expectedCopy("Header 1/2 已设置 · 来源中已删除", "Header: 1 of 2 set · Removed upstream"))
    let fine: JSON = ["kind": "skill", "origin": ["type": "created"], "vars": []]
    #expect(LibraryView.warning(fine).isEmpty)
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

/// A new workspace starts with the lead's group: the lead and everyone in its group who has an
/// Agent. A lead in no group takes every role that has one.
@Test func newWorkspacesStartWithTheLeadsGroup() {
    let roles: [JSON] = [
        ["id": "pmo", "connection": "codex-1", "lead": true, "group": "产品研发"],
        ["id": "developer", "connection": "claude-1", "group": "产品研发"],
        ["id": "qa", "connection": "", "group": "产品研发"],
        ["id": "friend", "connection": "claude-1", "group": "对话"],
        ["id": "mine", "connection": "claude-1", "group": ""],
    ]
    #expect(WorkspaceEditor.preset(roles.map(Role.init), lead: "pmo") == ["pmo", "developer"])
    var ungrouped = roles
    ungrouped[0]["group"] = ""
    #expect(WorkspaceEditor.preset(ungrouped.map(Role.init), lead: "pmo") == ["pmo", "developer", "friend", "mine"])
}

/// The MCP editor's form and JSON are one server, values included: written out as Claude Code's
/// `.mcp.json` and read back the same.
@Test func mcpServersReadTheSameAsFormOrJson() throws {
    let item: JSON = [
        "id": "i1", "name": "github", "description": "代码托管",
        "server": ["transport": "stdio", "command": "npx", "args": ["-y", "gh mcp"]],
        "vars": [["name": "GITHUB_TOKEN", "set": true], ["name": "EMPTY", "set": false]],
    ]
    var d = McpDraft(item: item, values: ["GITHUB_TOKEN": "ghp-1"])
    #expect(d.line == "npx -y 'gh mcp'")
    #expect(d.vars == [McpDraft.Var(name: "GITHUB_TOKEN", value: "ghp-1"), McpDraft.Var(name: "EMPTY")])
    #expect(d.json.contains(#""GITHUB_TOKEN": "ghp-1""#), "the JSON shows the value")

    // Out as JSON and back: the same server.
    let read = try #require(McpDraft.servers(in: d.json))
    #expect(read.count == 1 && read[0].command == "npx" && read[0].args == ["-y", "gh mcp"])
    var back = d
    back.take(read[0])
    #expect(back == d, "nothing changes on a round trip")
    #expect(back.body["vars"] == [["name": "GITHUB_TOKEN", "value": "ghp-1"], ["name": "EMPTY", "value": ""]], "every value goes as it stands")
    #expect(back.body["args"] == ["-y", "gh mcp"], "arguments go as a list, not split again")
    #expect(back.body["process"] == .null, "every server runs once per run: no process mode")

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
        #expect(s.repo.split(separator: "/").count == 2 && (s.path.isEmpty || s.path.hasPrefix("skills/")), "\(s.id)")
    }
    #expect(Catalog.skills.first?.title == "ego-browser", "the browser comes first")
}

/// A pick says when the library has it already, and a server that needs a key starts the editor
/// with the key's place written up to the user's part.
@Test func catalogPicksKnowWhatTheLibraryHas() throws {
    let sources: [JSON] = [
        ["id": "s1", "repo": "anthropics/skills", "path": "", "gitRef": ""],
        ["id": "s2", "repo": "unclebob/ub-mode", "path": "", "gitRef": ""],
    ]
    let pdf = try #require(Catalog.skills.first { $0.path == "skills/pdf" })
    let docx = try #require(Catalog.skills.first { $0.path == "skills/docx" })
    let items: [JSON] = [
        ["kind": "skill", "name": "pdf-tools", "origin": ["type": "git", "source": "s1", "path": "skills/pdf"]],
        ["kind": "skill", "name": "uncle-bob", "origin": ["type": "git", "source": "s2", "path": ""]],
        ["kind": "mcp", "name": "my-devtools", "server": ["transport": "stdio", "command": "npx", "args": ["-y", "chrome-devtools-mcp@latest"]]],
    ]
    #expect(Catalog.item(for: pdf, items: items, sources: sources)?["name"] == "pdf-tools", "the same folder of the same repository")
    #expect(Catalog.item(for: docx, items: items, sources: sources) == nil)
    // A skill whose SKILL.md is at its repository's top: the repository names it.
    let ub = try #require(Catalog.skills.first { $0.repo == "unclebob/ub-mode" })
    #expect(ub.id == "unclebob/ub-mode" && ub.folder == "ub-mode" && ub.location == "SKILL.md")
    #expect(Catalog.item(for: ub, items: items, sources: sources)?["name"] == "uncle-bob", "the top of the same repository")
    let chinese = try #require(Catalog.skills.first { $0.repo == "Fenng/tech-doc-style-chinese" })
    #expect(Catalog.item(for: chinese, items: [["kind": "skill", "name": "tech-doc-style-chinese"]], sources: []) != nil, "by its name")
    let devtools = try #require(Catalog.servers.first { $0.name == "chrome-devtools" })
    #expect(Catalog.item(for: devtools, items: items)?["name"] == "my-devtools", "the same command")
    #expect(devtools.oneClick && devtools.draft.command == "npx" && devtools.draft.args == ["-y", "chrome-devtools-mcp@latest"])

    let github = try #require(Catalog.servers.first { $0.name == "github" })
    #expect(!github.oneClick)
    #expect(github.draft.vars == [McpDraft.Var(name: "Authorization", value: "Bearer ")])
    let context7 = try #require(Catalog.servers.first { $0.name == "context7" })
    #expect(context7.oneClick && context7.draft.vars.isEmpty, "an optional key is added later, not left empty")
}

/// The skill tab follows the router: the guide shows the skills in the catalog's order (most read
/// first), says whether the catalog is listed or searched, and each row says how often its skill
/// was read and what would make it easier to find.
@Test func skillRowsShowUseAndHints() {
    let skills: [JSON] = [
        ["name": "beta", "uses": ["sessions": 2, "lastAt": 1_000]],
        ["name": "alpha", "uses": ["sessions": 0, "lastAt": nil]],
        ["name": "gamma", "uses": ["sessions": 5, "lastAt": 1_000]],
    ]
    #expect(Library.catalogOrder(skills).map { $0["name"].str } == ["gamma", "beta", "alpha"])
    #expect(Library.searched(["skillRouter": ["catalog": "search"]]))
    #expect(!Library.searched(["skillRouter": ["catalog": "list"]]) && !Library.searched(nil))
    #expect(Library.usage(skills[1], now: 1_000) == expectedCopy("还没有会话读过", "Not read by any session yet"))
    #expect(Library.usage(skills[0], now: 3_000) == expectedCopy("被 2 个会话读过 · 最近一次：刚刚", "Sessions that read it: 2 · last: Just now"))
    let hinted: JSON = ["kind": "skill", "origin": ["type": "created"], "hints": [["kind": "short"]]]
    #expect(LibraryView.warning(hinted) == expectedCopy("描述可以改进", "Description could be better"))
    #expect(Library.hint(["kind": "long", "chars": 1700, "kept": 1536])
        == expectedCopy("描述有 1700 个字，目录里只保留前 1536 个：把关键用途和触发词写在开头",
                        "The description has 1,700 characters; the catalog keeps the first 1,536, so put the key use and trigger words first."))
    #expect(Library.hint(["kind": "alike", "skill": "review"])
        == expectedCopy("和 review 的描述很像，Agent 可能分不清用哪个：写出各自适用的场合",
                        "Reads much like review's description, so the agent may not tell them apart: say when each one applies."))
    #expect(!Library.hint(["kind": "short"]).isEmpty && !Library.hint(["kind": "noWhen"]).isEmpty)
}
