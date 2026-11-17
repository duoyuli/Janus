import AppKit
import SwiftUI
import Testing
@testable import Janus

/// The lead's conversation in a real window: scrolled up, new replies leave the view where the
/// person put it; at the bottom, the view follows them.
@Test @MainActor func aConversationKeepsThePlaceThePersonScrolledTo() throws {
    func reply(_ n: Int) -> Event {
        // Messages to the lead alternate with its replies, so each is its own row.
        n % 2 == 0
            ? Event(["seq": JSON(n), "ts": JSON(n * 1000), "type": "user.message", "actor": "user", "to": "lead", "text": "", "data": ["text": JSON("第 \(n) 条消息"), "delivery": "prompt"]])
            : Event(["seq": JSON(n), "ts": JSON(n * 1000), "type": "agent.message", "actor": "lead", "text": "", "data": ["text": JSON("第 \(n) 条回复\n\n- 一\n- 二")]])
    }
    var events = [Event(["seq": 1, "ts": 1000, "type": "run.created", "actor": "user", "text": "", "data": ["goal": "长对话"]])]
    events += (2...200).map(reply)
    let run = RunModel(id: "scroll", store: testStore)
    func view() -> AnyView {
        run.events = events
        run.replay = events.count
        let st = run.shown
        return AnyView(ChatCard(run: run, st: st, title: "负责人", info: agentSession(st, "lead"), target: .agent("lead")).frame(width: 700, height: 600))
    }
    let host = NSHostingView(rootView: view())
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 600), styleMask: [.titled], backing: .buffered, defer: false)
    window.contentView = host
    func settle() { for _ in 0..<5 { host.layoutSubtreeIfNeeded(); RunLoop.main.run(until: Date().addingTimeInterval(0.03)) } }
    settle()
    // The conversation's scroll view is the one with the tallest content (the composer has one too).
    func scrollViews(_ v: NSView) -> [NSScrollView] { (v as? NSScrollView).map { [$0] } ?? [] + v.subviews.flatMap(scrollViews) }
    let chat = try #require(scrollViews(host).max { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) })
    let clip = chat.contentView
    func gapBelow() -> CGFloat { (chat.documentView?.frame.height ?? 0) - clip.bounds.maxY }
    #expect(gapBelow() < 2, "a conversation opens at its newest item")

    // The person's own scrolling: wheel events, as a mouse or trackpad sends them (positive is up).
    func wheel(_ dy: Int32) throws {
        let cg = try #require(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: dy, wheel2: 0, wheel3: 0))
        chat.scrollWheel(with: try #require(NSEvent(cgEvent: cg)))
        settle()
    }

    // Scroll up and let replies arrive: the view stays.
    for _ in 0..<5 { try wheel(400) }
    let place = clip.bounds.origin.y
    for n in 201...204 { events.append(reply(n)); host.rootView = view(); settle() }
    #expect(abs(clip.bounds.origin.y - place) < 2, "scrolled up, new replies do not move the view")
    #expect(gapBelow() > 100)

    // Back down to the end, and the next replies are followed.
    for _ in 0..<100 where gapBelow() > 0 { try wheel(-400) }
    #expect(gapBelow() == 0)
    for n in 205...208 { events.append(reply(n)); host.rootView = view(); settle() }
    #expect(gapBelow() < 2, "at the bottom, new replies are followed")
}
