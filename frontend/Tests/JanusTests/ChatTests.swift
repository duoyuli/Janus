import Foundation
import Testing
@testable import Janus

/// A chat's events as the orchestrator records them (docs/domains/20261120-chats.md).
private struct ChatLog {
    var events: [Event] = []
    mutating func add(_ type: String, actor: String = "janus", to: String? = nil, text: String = "", data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(1_000_000 + events.count * 1000), "type": JSON(type), "actor": JSON(actor), "text": JSON(text), "data": data]
        if let to { j["to"] = JSON(to) }
        events.append(Event(j))
    }

    static func opened() -> ChatLog {
        var log = ChatLog()
        log.add("run.created", actor: "user", data: ["kind": "chat", "cwd": "/Users/me/.janus/chats/r1", "git": false, "role": ["id": "m1", "name": "研究员"]])
        log.add("user.message", actor: "user", to: "lead", text: "你好", data: ["id": "m1", "text": "你好", "delivery": "pending"])
        log.add("agent.status", actor: "lead", text: "开始对话", data: ["phase": "thinking", "session": "s1", "mode": "chat"])
        log.add("agent.message", actor: "lead", text: "回复：你好", data: ["text": "回复：你好"])
        log.add("agent.status", actor: "lead", text: "这一轮已结束，等待你的消息", data: ["phase": "waiting_for_input", "turn": "idle"])
        return log
    }
}

/// A chat is known from its creation; its session's idle turn is its normal state, so it waits
/// on nobody: nothing goes to「需要你处理」or the inbox.
@Test func aChatWaitingForTheNextMessageAsksNothing() {
    let st = derive(ChatLog.opened().events)
    #expect(st.run.chat)
    #expect(st.run.status == .running)
    #expect(st.inbox.isEmpty, "\(st.inbox.map(\.kind))")
    #expect(boardFacts(st, name: { $0 }).open.isEmpty)
}

/// The composer of a chat: a plain message while the session waits, cutting in while it works,
/// and a message that reopens it once the session is gone, whatever ended it.
@Test func aChatAlwaysTakesTheNextMessage() {
    var log = ChatLog.opened()
    let idle = chatSession(derive(log.events), live: true)
    #expect(idle.open && idle.idle)
    #expect(SessionComposer(idle, live: true, replaying: false) == .converse)
    #expect(SessionComposer(idle, live: true, replaying: true) == .replay)

    log.add("user.message", actor: "user", to: "lead", text: "再来", data: ["id": "m2", "text": "再来", "delivery": "pending"])
    log.add("message.routed", to: "lead", data: ["id": "m2", "delivery": "prompt"])
    let working = chatSession(derive(log.events), live: true)
    #expect(SessionComposer(working, live: true, replaying: false) == .managed)

    log.add("run.interrupted", text: "编排器重启")
    let after = derive(log.events)
    #expect(after.run.status == .interrupted)
    let closed = chatSession(after, live: false)
    #expect(!closed.open)
    #expect(SessionComposer(closed, live: false, replaying: false) == .converse, "not「从中断处继续」: sending reopens it")

    // The reopened session makes it a running chat again.
    log.add("agent.status", actor: "lead", text: "恢复了之前的会话", data: ["phase": "thinking", "session": "s1", "mode": "chat"])
    #expect(derive(log.events).run.status == .running)
    log.add("run.failed", text: "对话的会话结束了", data: ["error": "Agent 进程已退出"])
    let failed = derive(log.events)
    #expect(failed.run.status == .failed && failed.run.error == "Agent 进程已退出")
    #expect(SessionComposer(chatSession(failed, live: false), live: false, replaying: false) == .converse)
}

/// A goal's lead conversation keeps its own composer: an open lead session is still managed.
@Test func aGoalLeadIsNotAChat() {
    var st = RunState()
    st.run.status = .running
    let lead = agentSession(st, "lead")
    #expect(!lead.chat)
    #expect(SessionComposer(lead, live: true, replaying: false) == .managed)
}

/// The recent chats say whether a chat's session is open; a closed one is not a run to resume.
@Test func chatRowsSayWhetherTheSessionIsOpen() {
    #expect(ChatRowStatus.label(.interrupted) == (L.text("已关闭"), .gray))
    #expect(ChatRowStatus.label(.failed) == (L.text("已关闭"), .gray))
    #expect(ChatRowStatus.label(.running) == (L.text("会话中"), .blue))
}

