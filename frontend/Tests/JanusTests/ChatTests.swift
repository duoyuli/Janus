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

/// A chat is listed by its first message, by who it is with before.
@Test func aNewChatIsListedByWhoItIsWith() {
    #expect(ChatRowStatus.title(["goal": "", "config": ["team": [["name": "研究员"]]]]) == L.text("和 \("研究员") 的新对话"))
    #expect(ChatRowStatus.title(["goal": "看看这个报错\n细节", "config": ["team": [["name": "研究员"]]]]) == "看看这个报错")
}

/// The recent chats say whether a chat's session is open; a closed one is not a run to resume.
@Test func chatRowsSayWhetherTheSessionIsOpen() {
    #expect(ChatRowStatus.label(.interrupted) == (L.text("已结束"), .gray))
    #expect(ChatRowStatus.label(.failed) == (L.text("已结束"), .gray))
    #expect(ChatRowStatus.label(.running) == (L.text("会话中"), .blue))
}

/// A chat that proposed nothing (the server answers `null`) shows no proposal; `nil` written as
/// a JSON literal would have become `JSON.null` and shown「提议了 0 位数字员工」.
@Test func noProposalShowsNoBanner() {
    #expect(Proposal(.null) == nil)
    let p = try! #require(Proposal(["proposal": 12, "roles": [["n": "1", "name": "测试", "existing": true]]]))
    #expect(p.id == 12 && p.roles.map(\.name) == ["测试"] && p.roles[0].existing && p.roles[0].n == "1")
}
