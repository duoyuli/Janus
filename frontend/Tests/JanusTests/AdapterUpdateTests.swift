import Testing
@testable import Janus

/// The sidebar reminds about an adapter with a later version on npm, and not while it is being
/// installed (docs/architecture/20261031-adapters.md#update_check).
@Test func adapterUpdatesAreTheOnesStillToInstall() {
    let adapters: JSON = ["adapters": [
        ["id": "codex", "current": "2.6.0", "latest": "2.7.0", "updateAvailable": true],
        ["id": "claude", "current": "0.107.0", "latest": "0.108.0", "updateAvailable": true, "job": ["state": "running"]],
        ["id": "other", "current": "1.0.0", "latest": "1.0.0", "updateAvailable": false],
    ]]
    #expect(AppStore.adapterUpdates(adapters).map { $0["id"].str } == ["codex"])
    #expect(AppStore.adapterUpdates(.null).isEmpty)
}
