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
