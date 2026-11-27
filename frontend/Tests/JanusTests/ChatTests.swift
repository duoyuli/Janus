import Foundation
import Testing
@testable import Janus

/// A chat's events as the orchestrator records them (docs/domains/20261120-chats.md).
private struct ChatLog {
    var events: [Event] = []
    mutating func add(_ type: String, actor: String = "janus", to: String? = nil, task: String? = nil, text: String = "", data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(1_000_000 + events.count * 1000), "type": JSON(type), "actor": JSON(actor), "text": JSON(text), "data": data]
        if let to { j["to"] = JSON(to) }
        if let task { j["task"] = JSON(task) }
        events.append(Event(j))
    }
}

/// A chat that proposed nothing (the server answers `null`) shows no proposal; `nil` written as
/// a JSON literal would have become `JSON.null` and shown「提议了 0 位数字员工」.
@Test func noProposalShowsNoBanner() {
    #expect(Proposal(.null) == nil)
    let p = try! #require(Proposal(["proposal": 12, "roles": [["n": "1", "name": "测试", "existing": true]],
                                    "groups": [["group": "", "title": "未分组", "roles": ["1"]]]]))
    #expect(p.id == 12 && p.roles.map(\.name) == ["测试"] && p.roles[0].existing && p.roles[0].n == "1")
    // The groups are the server's, titles and order included.
    #expect(p.groups.map(\.title) == ["未分组"] && p.groups[0].roles == ["1"])
}
