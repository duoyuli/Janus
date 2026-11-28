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
        // The server words each message in a bubble (`lines`); a reply is the agent's own text.
        let lines = JSON.object(Dictionary(uniqueKeysWithValues: events.filter { $0.type == .userMessage }.map { e in
            (String(e.seq), ["kind": "bubbles", "items": [["text": e.data["text"], "images": [], "via": "作为新一轮发送", "insert": false]]] as JSON)
        }))
        // The rows the server lays the conversation out in (`run_view::turns`): the run's start as
        // a section, then each message opening its turn, the turn's line, and the reply.
        var rows: [JSON] = [["kind": "section", "id": 1, "seq": 1, "injected": [], "gap": "none"]]
        for e in events where e.type == .userMessage {
            let n = e.seq
            rows.append(["kind": "item", "id": JSON(n), "seq": JSON(n), "turn": JSON(n), "fold": false, "gap": "round"])
            guard events.contains(where: { $0.seq == n + 1 }) else { continue }
            rows.append(["kind": "head", "id": JSON(-(n * 8 + 1)), "turn": JSON(n), "agent": "lead", "running": false, "open": .null,
                         "start": JSON(n * 1000), "end": JSON((n + 1) * 1000), "stats": "", "summary": "工作了 1 秒", "gap": "block"])
            rows.append(["kind": "text", "id": JSON(n + 1), "events": [JSON(n + 1)], "reply": true, "turn": JSON(n), "fold": false, "gap": "block"])
        }
        let st = RunState(["run": ["status": "running", "goal": "长对话"], "lines": lines,
                           "sessions": ["agent:lead": ["events": .array(events.map { JSON($0.seq) }), "open": true, "composer": "replay",
                                                       "rows": .array(rows)]]], events: events)
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
    // A swipe on a view with nothing to scroll is taken as a navigation swipe, whose tracking waits
    // for gesture events that never come: fail here instead of hanging below.
    try #require((chat.documentView?.frame.height ?? 0) > clip.bounds.height * 3,
                 "the conversation must be much taller than its window (the fixture's rows were not drawn)")
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
    #expect(!run.scrolling, "a scroll that ended lets the live run's updates in again")
    let place = clip.bounds.origin.y
    for n in 201...204 { events.append(reply(n)); host.rootView = view(); await settle() }
    #expect(abs(clip.bounds.origin.y - place) < 2, "scrolled up, new replies do not move the view")
    #expect(gapBelow() > 100)

    // Back down to the end, and the next replies are followed.
    // Unmeasured rows below are estimates: each swipe down measures some and the content grows
    // while the person scrolls, more on a slow machine (GitHub's macOS 15 runner once needed more
    // than 60 swipes), so swipe until the end, with a cap far above what the content needs.
    for _ in 0..<300 where gapBelow() > 1 { try await swipe(-400) }
    #expect(gapBelow() < 2)
    for n in 205...208 { events.append(reply(n)); host.rootView = view(); await settle() }
    await settleAtEnd()
    #expect(gapBelow() < 2, "at the bottom, new replies are followed")
}
