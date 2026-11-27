import Testing
@testable import Janus

/// Hiding a path adds it once; showing it again takes it out.
@Test func theHiddenListTakesAPathOnceAndGivesItBack() {
    let list = HostEnv.with([".codex"], ".aws", hide: true)
    #expect(list == [".codex", ".aws"])
    #expect(HostEnv.with(list, ".aws", hide: true) == list)
    #expect(HostEnv.with(list, ".codex", hide: false) == [".aws"])
    #expect(HostEnv.why(["why": "agent"]) == expectedCopy("Agent 的配置", "Agent Settings"))
    #expect(HostEnv.why(["why": "custom"]) == expectedCopy("你加的", "Yours"))
}

/// The list shows what is on this Mac, and what the person added even before it is there.
@Test func theHiddenListShowsWhatIsHereAndWhatThePersonAdded() {
    let hidden: [JSON] = [["path": ".codex", "why": "agent", "here": true], ["path": ".cursor", "why": "agent", "here": false],
                          ["path": ".mine", "why": "custom", "here": false]]
    #expect(HostEnv.shown(hidden).map(\.["path"].str) == [".codex", ".mine"])
}
