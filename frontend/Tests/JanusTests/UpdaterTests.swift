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
