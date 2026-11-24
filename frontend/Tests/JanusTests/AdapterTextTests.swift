import Testing
@testable import Janus

/// What an Agent card's runtime line derives from `GET /api/adapters`:
/// docs/architecture/20261031-adapters.md#api_shape.
@Test func theMenuListsTheOtherInstalledVersionsNewestFirst() {
    let a: JSON = [
        "pinned": false,
        "versions": [
            ["version": "1.0.0", "installedAt": 10, "current": true],
            ["version": "0.9.0", "installedAt": 30],
            ["version": "0.8.0", "installedAt": 20],
        ],
    ]
    #expect(AdapterText.others(a) == ["0.9.0", "0.8.0"])
    #expect(AdapterText.showsMenu(a))
    #expect(AdapterText.menuItems(a) == [.switchTo("0.9.0"), .switchTo("0.8.0")])
}

/// One installed version has nothing to switch to, so the entry is not there at all.
@Test func oneInstalledVersionHasNoSwitchingEntry() {
    let a: JSON = [
        "pinned": false,
        "versions": [["version": "1.0.0", "installedAt": 10, "current": true]],
    ]
    #expect(!AdapterText.showsMenu(a))
    #expect(AdapterText.menuItems(a).isEmpty)
}

/// A pinned version offers resuming automatic updates first, naming the newest version when one is
/// known, then the other installed versions.
@Test func aPinnedVersionOffersResumingFirst() {
    let a: JSON = [
        "pinned": true,
        "latest": "2.0.0",
        "versions": [
            ["version": "1.0.0", "installedAt": 10, "current": true],
            ["version": "0.9.0", "installedAt": 30],
        ],
    ]
    #expect(AdapterText.menuItems(a) == [.resume("2.0.0"), .switchTo("0.9.0")])
    #expect(AdapterText.showsMenu(a))

    let unpinned: JSON = ["pinned": false, "latest": "2.0.0", "versions": [["version": "1.0.0", "current": true]]]
    #expect(AdapterText.menuItems(unpinned).isEmpty)
}

/// A finished automatic update reads as "已自动更新" for ten minutes; after that, or once the person
/// has left the page on it, the line says nothing.
@Test func anAutomaticUpdateShowsForTenMinutesAndOnlyOnce() {
    let a: JSON = ["id": "claude", "current": "2.0.0", "job": ["action": "auto", "state": "ok", "ended": 1000]]
    #expect(AdapterText.state(a, now: 1000 + 60_000) == .autoUpdated)
    #expect(AdapterText.state(a, now: 1000 + AdapterText.autoUpdatedFor + 1) == .idle)
    #expect(AdapterText.state(a, now: 1000 + 60_000, seen: [AdapterText.autoUpdatedKey(a)]) == .idle)
    #expect(AdapterText.autoUpdatedKey(a) == "claude@1000")
    #expect(AdapterText.autoUpdatedUntil(a) == 1000 + AdapterText.autoUpdatedFor)
    #expect(AdapterText.autoUpdatedUntil(["job": ["action": "install", "state": "ok", "ended": 1000]]) == nil)
}

/// Only an automatic success gets that word: an install the person asked for ends without it.
@Test func aManualInstallThatFinishedIsNotAnAutomaticUpdate() {
    let a: JSON = ["id": "claude", "current": "2.0.0", "job": ["action": "install", "state": "ok", "ended": 1000]]
    #expect(AdapterText.state(a, now: 2000) == .idle)
}

/// A running job is "更新中…" when a version is in use and "安装中…" when none is.
@Test func aRunningJobReadsAsUpdatingOrInstalling() {
    let updating: JSON = ["current": "1.0.0", "job": ["state": "running", "action": "auto"]]
    #expect(AdapterText.state(updating, now: 0) == .updating)
    let installing: JSON = ["job": ["state": "running", "action": "install"]]
    #expect(AdapterText.state(installing, now: 0) == .installing)
    #expect(AdapterText.running(installing))
}

/// A failed update over a working version and a first install that failed read differently.
@Test func failuresReadDifferentlyWithAndWithoutAVersion() {
    let failed: JSON = ["id": "claude", "current": "2.0.0", "job": ["action": "auto", "state": "error", "ended": 1]]
    #expect(AdapterText.state(failed, now: 0) == .failed)
    let installFailed: JSON = ["id": "claude", "job": ["action": "install", "state": "error", "ended": 1]]
    #expect(AdapterText.state(installFailed, now: 0) == .installFailed)
    #expect(AdapterText.state(["id": "claude"], now: 0) == .notInstalled)
}

/// The failure popover's 最近日志 shows only the steps Janus itself wrote, without their times: no
/// command line, no tool output and no package name, not even from a log an older version wrote
/// (P1 AC-15). The whole raw log is what 查看日志 opens.
@Test func theFailurePopoverShowsOnlyJanussOwnRecentNotes() {
    let a: JSON = ["log": """
    == 2026-11-24T10:00:00Z 开始安装 9.10.0
    $ "npm" install --omit=dev --ignore-scripts --no-audit --no-fund @agentclientprotocol/claude-agent-acp@9.10.0
    npm error code ECONNREFUSED
    npm error A complete log of this run can be found in: /Users/x/.npm/_logs/debug-0.log
    == 2026-11-24T10:00:01Z 安装失败：安装命令失败（exit 1），详见 /Users/x/.janus/logs/adapters/claude.log
    == 2026-11-24T09:00:00Z 开始安装 @agentclientprotocol/claude-agent-acp@9.9.9
    """]
    let notes = AdapterText.recentNotes(a)
    #expect(notes == ["开始安装 9.10.0", "安装失败：安装命令失败（exit 1），详见 /Users/x/.janus/logs/adapters/claude.log"])
    let hidden = ["npm", "@agentclientprotocol", "node_modules", "$ "]
    #expect(!notes.contains { note in hidden.contains { note.lowercased().contains($0) } })
    #expect(AdapterText.recentNotes(a, limit: 1) == ["安装失败：安装命令失败（exit 1），详见 /Users/x/.janus/logs/adapters/claude.log"])
    #expect(AdapterText.recentNotes(["log": ""]).isEmpty)
    #expect(AdapterText.recentNotes(["log": "npm error ECONNREFUSED"]).isEmpty)
}
