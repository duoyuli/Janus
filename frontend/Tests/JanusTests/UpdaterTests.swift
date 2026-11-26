import Foundation
import Testing
@testable import Janus

@Test func versionsCompareNumberByNumber() {
    #expect(Updater.isNewer("2026.11.02", than: "2026.11.01.9"))
    #expect(Updater.isNewer("2026.11.01.10", than: "2026.11.01.9"))
    #expect(Updater.isNewer("2026.11.01.2", than: "2026.11.01"))
    #expect(!Updater.isNewer("2026.11.01", than: "2026.11.01.0"))
    #expect(!Updater.isNewer("0.1.0", than: "2026.11.01"))
}

/// A release candidate comes before the version it leads to, after any earlier one, and after
/// the candidates before it; a stable version later than a candidate is newer than it.
@Test func aCandidateComesBeforeItsVersion() {
    #expect(Updater.isNewer("2026.11.27", than: "2026.11.27-rc.2"))
    #expect(Updater.isNewer("2026.11.27-rc.2", than: "2026.11.27-rc.1"))
    #expect(Updater.isNewer("2026.11.27-rc.10", than: "2026.11.27-rc.9"))
    #expect(Updater.isNewer("2026.11.27-rc.1", than: "2026.11.26.9"))
    #expect(!Updater.isNewer("2026.11.26.9", than: "2026.11.27-rc.1"), "leaving RC never goes back to an older stable")
    #expect(!Updater.isNewer("2026.11.27-rc.1", than: "2026.11.27-rc.1"))
    #expect(Updater.isCandidate("2026.11.27-rc.1") && !Updater.isCandidate("2026.11.27.2"))
}

/// The RC channel takes the newest installable release of a GitHub list: candidates and stable
/// versions alike, never a draft nor one without update.json.
@Test func theCandidateChannelTakesTheNewestInstallableRelease() {
    func rel(_ tag: String, draft: Bool = false, manifest: Bool = true, prerelease: Bool = false) -> JSON {
        ["tag_name": JSON(tag), "draft": JSON(draft), "prerelease": JSON(prerelease),
         "assets": manifest ? [["name": "update.json"]] : []]
    }
    let list: JSON = [rel("v2026.11.26.9"), rel("v2026.11.27-rc.1", prerelease: true), rel("v2026.11.27-rc.3", draft: true, prerelease: true),
                      rel("v2026.11.28-rc.1", manifest: false, prerelease: true), rel("v2026.11.26.8")]
    #expect(Updater.newest(list).map(Updater.version(of:)) == "2026.11.27-rc.1")
    let shipped: JSON = [rel("v2026.11.27"), rel("v2026.11.27-rc.2", prerelease: true)]
    #expect(Updater.newest(shipped).map(Updater.version(of:)) == "2026.11.27", "the stable version a candidate led to wins")
    #expect(Updater.newest([]) == nil)
}

private func release(node: String) throws -> Updater.Release {
    let m: JSON = ["version": "2026.11.02", "minimumSystemVersion": "14.0", "parts": [
        ["name": "app", "file": "Janus-2026.11.02-app.tar.gz", "sha256": JSON(String(repeating: "a", count: 64)), "size": 5_000_000, "content": "2026.11.02"],
        ["name": "node", "file": "Janus-2026.11.02-node.tar.gz", "sha256": JSON(String(repeating: "b", count: 64)), "size": 50_000_000, "content": JSON(node)],
    ]]
    let base = "https://github.com/o/r/releases/download/v2026.11.02/"
    let assets = Dictionary(uniqueKeysWithValues: ["Janus-2026.11.02-app.tar.gz", "Janus-2026.11.02-node.tar.gz"].map { ($0, URL(string: base + $0)!) })
    return try Updater.release(from: m, assets: assets, notes: "", page: nil)
}

@Test func onlyChangedPartsAreDownloaded() throws {
    let r = try release(node: "n1")
    #expect(Updater.plan(r, installedNode: "n1").map(\.name) == ["app"], "Node.js unchanged: reuse it")
    #expect(Updater.plan(r, installedNode: "n0").map(\.name) == ["app", "node"])
    #expect(Updater.plan(r, installedNode: nil).map(\.name) == ["app", "node"], "an app without update-parts.json")
    // A part the release does not carry makes the manifest unusable.
    #expect(throws: UpdateError.self) { try Updater.release(from: ["version": "1", "parts": [["name": "app", "file": "x", "sha256": JSON(String(repeating: "a", count: 64))]]], assets: [:], notes: "", page: nil) }
}

/// A minimal signed bundle, the way package.sh signs Janus.app (ad hoc here).
private func signedApp(_ dir: URL, version: String) throws -> URL {
    let app = dir.appendingPathComponent("Janus.app")
    try? FileManager.default.removeItem(at: app)
    let macos = app.appendingPathComponent("Contents/MacOS")
    try FileManager.default.createDirectory(at: macos, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: app.appendingPathComponent("Contents/Resources/node"), withIntermediateDirectories: true)
    try "node".write(to: app.appendingPathComponent("Contents/Resources/node/LICENSE"), atomically: true, encoding: .utf8)
    try FileManager.default.copyItem(at: URL(fileURLWithPath: "/usr/bin/true"), to: macos.appendingPathComponent("Janus"))
    let plist: NSDictionary = ["CFBundleIdentifier": "local.janus.test", "CFBundleExecutable": "Janus", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version]
    plist.write(to: app.appendingPathComponent("Contents/Info.plist"), atomically: true)
    try Updater.run("/usr/bin/codesign", ["--force", "--sign", "-", app.path])
    return app
}

@Test func anUpdateMustBeValidSignedCodeOfTheReleasedVersion() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("janus-updater-\(getpid())")
    try? FileManager.default.removeItem(at: dir)
    defer { try? FileManager.default.removeItem(at: dir) }
    let current = try signedApp(dir.appendingPathComponent("current"), version: "1")
    #expect(Updater.stableRequirement(current) == nil, "an ad-hoc signature names no signer to require")
    let new = try signedApp(dir.appendingPathComponent("new"), version: "2")
    try await Updater.verify(new, like: current, version: "2")
    await #expect(throws: UpdateError.self) { try await Updater.verify(new, like: current, version: "3") }
    // A file changed after signing breaks the seal.
    try "tampered".write(to: new.appendingPathComponent("Contents/Resources/node/LICENSE"), atomically: true, encoding: .utf8)
    await #expect(throws: UpdateError.self) { try await Updater.verify(new, like: current, version: "2") }
    #expect(try Updater.sha256(current.appendingPathComponent("Contents/Resources/node/LICENSE")) == "545ea538461003efdc8c81c244531b003f6f26cfccf6c0073b3239fdedf49446", "sha256 of \"node\"")
}

/// Runs the install script with `mv` and `open` stood in: `mv` of the new bundle leaves part of it
/// behind and fails, as a copy across volumes can; `open` records what it was asked to open.
private func install(in dir: URL, newArrives: Bool) throws -> (opened: String, log: String) {
    let fm = FileManager.default
    let bin = dir.appendingPathComponent("bin"), cur = dir.appendingPathComponent("Applications/Janus.app")
    let new = dir.appendingPathComponent("new/Janus.app"), prev = dir.appendingPathComponent("previous/Janus.app")
    for d in [bin, cur, new] { try fm.createDirectory(at: d, withIntermediateDirectories: true) }
    try "old".write(to: cur.appendingPathComponent("version"), atomically: true, encoding: .utf8)
    try "new".write(to: new.appendingPathComponent("version"), atomically: true, encoding: .utf8)
    let opened = dir.appendingPathComponent("opened")
    let fakes = [
        ("mv", newArrives ? "exec /bin/mv \"$@\"" : """
         if [ "$1" = "\(new.path)" ]; then mkdir -p "$2"; echo partial > "$2/partial"; exit 1; fi
         exec /bin/mv "$@"
         """),
        ("open", "echo \"$1\" > \"\(opened.path)\""),
    ]
    for (name, body) in fakes {
        let f = bin.appendingPathComponent(name)
        try "#!/bin/sh\n\(body)\n".write(to: f, atomically: true, encoding: .utf8)
        try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: f.path)
    }
    let script = dir.appendingPathComponent("install.sh"), log = dir.appendingPathComponent("update.log")
    try Updater.installScript.write(to: script, atomically: true, encoding: .utf8)
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/bin/sh")
    // A pid no process has: the script does not wait.
    p.arguments = [script.path, "99999999", cur.path, new.path, prev.path, log.path]
    p.environment = ["PATH": "\(bin.path):/bin:/usr/bin"]
    try p.run()
    p.waitUntilExit()
    return (try String(contentsOf: opened, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines),
            try String(contentsOf: log, encoding: .utf8))
}

@Test func aFailedSwapPutsTheOldAppBackWhole() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("janus-install-\(getpid())")
    try? FileManager.default.removeItem(at: dir)
    defer { try? FileManager.default.removeItem(at: dir) }
    let cur = dir.appendingPathComponent("Applications/Janus.app")

    let failed = try install(in: dir.appendingPathComponent("failed"), newArrives: false)
    let back = dir.appendingPathComponent("failed/Applications/Janus.app")
    #expect(try String(contentsOf: back.appendingPathComponent("version"), encoding: .utf8) == "old")
    #expect(!FileManager.default.fileExists(atPath: back.appendingPathComponent("partial").path), "nothing of the new one is left")
    #expect(!FileManager.default.fileExists(atPath: back.appendingPathComponent("Janus.app").path), "the old one is not nested inside")
    #expect(failed.opened == back.path)
    #expect(failed.log.contains("restored"))

    let done = try install(in: dir, newArrives: true)
    #expect(try String(contentsOf: cur.appendingPathComponent("version"), encoding: .utf8) == "new")
    #expect(done.opened == cur.path)
}

/// A new download starts from a clean folder: versions an earlier update left behind go, the
/// app kept for going back by hand stays (docs/operations/20261031-deployment.md#layout).
@Test func aDownloadClearsWhatEarlierUpdatesLeft() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("janus-updates-\(UUID().uuidString)")
    let fm = FileManager.default
    for name in ["2026.11.16.2/Janus.app", "2026.11.20.8/Janus.app", "previous/Janus.app"] {
        try fm.createDirectory(at: dir.appendingPathComponent(name), withIntermediateDirectories: true)
    }
    try Data().write(to: dir.appendingPathComponent("install.sh"))
    Updater.clearStaged(dir)
    #expect(try fm.contentsOfDirectory(atPath: dir.path) == ["previous"])
    try? fm.removeItem(at: dir)
}

/// Automatic updates are on until the person turns them off, and the choice is kept.
@MainActor @Test func automaticUpdatesAreOnUntilTurnedOff() throws {
    let suite = "janus.tests.autoUpdate.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    #expect(Updater(defaults: defaults).automatic, "on by default")
    Updater(defaults: defaults).automatic = false
    #expect(!Updater(defaults: defaults).automatic, "kept across launches")
}

/// A Mac follows the stable channel until the person picks RC, and the choice is kept.
@MainActor @Test func theStableChannelUntilRCIsPicked() throws {
    let suite = "janus.tests.channel.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    #expect(Updater(defaults: defaults).channel == .stable)
    Updater(defaults: defaults).channel = .rc
    #expect(Updater(defaults: defaults).channel == .rc, "kept across launches")
    #expect(Updater.label("2026.11.27-rc.1") == expectedCopy("2026.11.27-rc.1（测试版）", "2026.11.27-rc.1 (RC)"))
    #expect(Updater.label("2026.11.27") == "2026.11.27")
}

/// A staged version is swapped in only after enough checks in a row with nothing at work: one
/// busy check starts the count again.
@Test func aStagedVersionWaitsForQuietChecksInARow() {
    var rounds = 0
    for busy in [false, true, false] { rounds = Updater.quiet(after: rounds, busy: busy) }
    #expect(rounds == 1 && rounds < Updater.quietRounds, "a task that started in between starts the count again")
    rounds = Updater.quiet(after: rounds, busy: false)
    #expect(rounds >= Updater.quietRounds)
}
