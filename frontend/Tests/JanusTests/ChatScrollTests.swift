import AppKit
import SwiftUI
import Testing
@testable import Janus

/// Whether the login session's screen is locked: the window server then gives windows no display
/// cycles, and scrolling a real window cannot be checked (an unattended run often has it locked).
private var screenLocked: Bool {
    (CGSessionCopyCurrentDictionary() as? [String: Any])?["CGSSessionScreenIsLocked"] as? Bool ?? false
}

/// The lead's conversation in a real window: scrolled up, new replies leave the view where the
/// person put it; at the bottom, the view follows them.
@Test(.disabled(if: screenLocked, "屏幕已锁定：窗口没有显示周期，无法检查真实窗口的滚动；解锁后重跑 just test-mac"))
@MainActor func aConversationKeepsThePlaceThePersonScrolledTo() async throws {
    func reply(_ n: Int) -> Event {
        // Messages to the lead alternate with its replies, so each is its own row. Some replies
        // are much longer, so the lazy list's estimates of rows it has not built are far off.
        n % 2 == 0
            ? Event(["seq": JSON(n), "ts": JSON(n * 1000), "type": "user.message", "actor": "user", "to": "lead", "text": "", "data": ["text": JSON("第 \(n) 条消息"), "delivery": "prompt"]])
            : Event(["seq": JSON(n), "ts": JSON(n * 1000), "type": "agent.message", "actor": "lead", "text": "", "data": ["text": JSON("第 \(n) 条回复\n\n" + Array(repeating: "- 一行", count: n % 7 == 1 ? 40 : 2).joined(separator: "\n"))]])
    }
    var events = [Event(["seq": 1, "ts": 1000, "type": "run.created", "actor": "user", "text": "", "data": ["goal": "长对话"]])]
    events += (2...200).map(reply)
    let run = RunModel(id: "scroll", store: testStore)
    func view() -> AnyView {
        run.events = events
        run.replay = events.count
        // The view the server serves at the replayed position: everything is the lead's conversation.
        let st = RunState(["run": ["status": "running", "goal": "长对话"],
                           "sessions": ["agent:lead": ["events": .array(events.map { JSON($0.seq) }), "open": true, "composer": "replay"]]], events: events)
        return AnyView(ChatCard(run: run, st: st, title: "负责人", info: st.session(.agent("lead")), target: .agent("lead")).frame(width: 700, height: 600))
    }
    let host = NSHostingView(rootView: view())
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 600), styleMask: [.titled], backing: .buffered, defer: false)
    window.contentView = host
    // Deliver display cycles as well as layout passes while other main-actor tests run.
    // Synthetic swipes below go straight to the scroll view; real pointer input must not enter.
    window.isReleasedWhenClosed = false
    window.ignoresMouseEvents = true
    window.alphaValue = 0
    window.orderFrontRegardless()
    defer { window.close() }
    // Waits by giving the main actor up: a nested run loop inside a main-actor test does not run
    // the main queue, where SwiftUI's updates and the view's own follow-up scrolls are queued.
    func settle() async {
        for _ in 0..<10 {
            host.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 30_000_000)
        }
    }
    var mainQueueAdvanced = false
    DispatchQueue.main.async { mainQueueAdvanced = true }
    await settle()
    try #require(mainQueueAdvanced, "the fixture must run deferred scrolls before checking their result")
    // The conversation's scroll view is the one with the tallest content (the composer has one too).
    func scrollViews(_ v: NSView) -> [NSScrollView] { (v as? NSScrollView).map { [$0] } ?? [] + v.subviews.flatMap(scrollViews) }
    let chat = try #require(scrollViews(host).max { ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0) })
    let clip = chat.contentView
    func gapBelow() -> CGFloat { (chat.documentView?.frame.height ?? 0) - clip.bounds.maxY }
    /// Settles until the end is in view, for as long as the view keeps going after it
    /// (`ChatFollow.landingTime`): a slow machine measures the rows over more passes.
    func settleAtEnd() async {
        let deadline = Date().addingTimeInterval(ChatFollow.landingTime + 1)
        repeat { await settle() } while gapBelow() >= 2 && Date() < deadline
    }
    await settleAtEnd()
    #expect(gapBelow() < 2, "a conversation opens at its newest item")

    // The person's own scrolling: one trackpad swipe of `dy` points (positive is up), sent as the
    // gesture's phases so SwiftUI takes it as the person scrolling.
    func swipe(_ dy: Int32) async throws {
        for (phase, delta) in [(CGScrollPhase.began, Int32(0)), (.changed, dy / 2), (.changed, dy - dy / 2), (.ended, 0)] {
            let cg = try #require(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: delta, wheel2: 0, wheel3: 0))
            cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
            cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: Int64(phase.rawValue))
            chat.scrollWheel(with: try #require(NSEvent(cgEvent: cg)))
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        await settle()
    }

    // Scroll up and let replies arrive: the view stays.
    for _ in 0..<5 { try await swipe(400) }
    let place = clip.bounds.origin.y
    for n in 201...204 { events.append(reply(n)); host.rootView = view(); await settle() }
    #expect(abs(clip.bounds.origin.y - place) < 2, "scrolled up, new replies do not move the view")
    #expect(gapBelow() > 100)

    // Back down to the end, and the next replies are followed.
    for _ in 0..<60 where gapBelow() > 1 { try await swipe(-400) }
    #expect(gapBelow() < 2)
    for n in 205...208 { events.append(reply(n)); host.rootView = view(); await settle() }
    await settleAtEnd()
    #expect(gapBelow() < 2, "at the bottom, new replies are followed")
}
