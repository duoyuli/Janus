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
    // The groups as the server folds them, by skill id.
    let library: JSON = ["skillGroups": [
        ["type": "source", "source": "s1", "items": ["c", "f"]], ["type": "source", "source": "s2", "items": []],
        ["type": "folder", "path": "/a", "items": ["e"]], ["type": "folder", "path": "/u/skills", "items": ["b", "d"]],
        ["type": "created", "items": ["a"]],
    ]]
    let groups = Library.skillGroups(library, skills: skills)
    #expect(groups.map(\.origin) == [.source("s1"), .source("s2"), .folder("/a"), .folder("/u/skills"), .created])
    #expect(groups.map { $0.items.map { $0["id"].str } } == [["c", "f"], [], ["e"], ["b", "d"], ["a"]])
    #expect(Library.skillGroups(nil, skills: []).isEmpty, "no sources and no skills: no groups")
}

/// The skill tab follows the router: the guide shows the skills in the catalog's order (most read
/// first), says whether the catalog is listed or searched, and each row says how often its skill
/// was read and what would make it easier to find.
@Test func skillRowsShowUseAndHints() {
    let skills: [JSON] = [
        ["id": "b", "name": "beta", "uses": ["sessions": 2, "lastAt": 1_000], "usageText": "被 2 个会话读过"],
        ["id": "a", "name": "alpha", "uses": ["sessions": 0, "lastAt": nil], "usageText": "还没有会话读过"],
        ["id": "g", "name": "gamma", "uses": ["sessions": 5, "lastAt": 1_000]],
    ]
    #expect(Library.catalogOrder(["skillRouter": ["order": ["g", "b", "a"]]], skills: skills).map { $0["name"].str } == ["gamma", "beta", "alpha"])
    #expect(Library.searched(["skillRouter": ["catalog": "search"]]))
    #expect(!Library.searched(["skillRouter": ["catalog": "list"]]) && !Library.searched(nil))
    #expect(Library.usage(skills[1], now: 1_000) == "还没有会话读过", "the service's words, and no time when never read")
    #expect(Library.usage(skills[0], now: 3_000) == "被 2 个会话读过 · " + L.text("最近一次：\(L.ago(1_000, 3_000))"))
}
