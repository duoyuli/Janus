import AppKit
import CryptoKit
import Foundation
import Observation
import Security

/// In-app updates of Janus.app from the GitHub releases of `JanusUpdateRepo` (Info.plist).
///
/// A release carries update.json and two parts: the bundle without Node.js, and Node.js. Only the
/// parts that changed are downloaded (Node.js is reused when its content hash is the same), each
/// checked against update.json's SHA-256, put together under `<home>/updates/<version>/`, and
/// checked as signed code: valid, and, when the running app has a stable signing identity,
/// signed by that same identity. Then a small script swaps the bundle in after the app quits and
/// opens the new one.
///
/// The app downloads with URLSession, which sets no quarantine flag, so Gatekeeper does not stop
/// the new version the way it stops a DMG downloaded in a browser.
// @project-doc docs/architecture/20261031-frontend.md#updates
@Observable @MainActor
final class Updater {
    static let shared = Updater()

    /// The update's own progress, with each step a case instead of a string the UI parses.
    enum State: Equatable {
        case idle, checking, upToDate, available
        case downloading(part: String, index: Int, count: Int, size: String)
        case verifying
        case relaunching
        case failed(String)

        var isWorking: Bool {
            switch self {
            case .downloading, .verifying, .relaunching: true
            default: false
            }
        }

        /// What the sidebar shows while installing.
        var workingText: String? {
            switch self {
            case .downloading(let part, let index, let count, let size): L.text("正在下载 \(part)（\(index)/\(count)，\(size)）…")
            case .verifying: L.text("正在校验新版本…")
            case .relaunching: L.text("正在重新打开…")
            default: nil
            }
        }
    }

    struct Part: Equatable {
        let name: String
        let file: String
        let sha256: String
        let size: Int
        let content: String
        var url: URL?
    }

    struct Release: Equatable {
        let version: String
        let parts: [Part]
        let minimumSystemVersion: String
        let notes: String
        let page: URL?
    }

    private(set) var state: State = .idle
    private(set) var latest: Release?
    private(set) var checkedAt: Date?
    /// Set while installing, so quitting does not ask about running tasks a second time.
    private(set) var quitting = false
    /// Stops the server the app started before it exits to install a new version.
    var onQuit: (() -> Void)?

    let current: String
    let repo: String?
    let bundle: URL
    let home: URL
    @ObservationIgnored private var timer: Task<Void, Never>?

    static let checkEvery: UInt64 = 6 * 3600

    init() {
        let info = Bundle.main.infoDictionary ?? [:]
        current = info["CFBundleShortVersionString"] as? String ?? "0"
        repo = (info["JanusUpdateRepo"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        bundle = Bundle.main.bundleURL
        let env = ProcessInfo.processInfo.environment
        home = env["JANUS_HOME"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".janus")
    }

    /// Only a packaged Janus.app updates itself; `swift run` and `just mac` do not.
    var enabled: Bool { repo != nil && bundle.pathExtension == "app" }

    /// How much an update downloads, given what is installed.
    var downloadSize: Int { latest.map { Self.plan($0, installedNode: installedNode).reduce(0) { $0 + $1.size } } ?? 0 }

    var installedNode: String? {
        guard let data = try? Data(contentsOf: bundle.appendingPathComponent("Contents/Resources/update-parts.json")),
              let j = try? JSON.parse(data) else { return nil }
        return j["node"].string
    }

    // MARK: Checking

    /// Check shortly after launch, then every few hours.
    func start() {
        guard enabled, timer == nil else { return }
        timer = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 20_000_000_000)
            while !Task.isCancelled {
                await self?.check()
                try? await Task.sleep(nanoseconds: Self.checkEvery * 1_000_000_000)
            }
        }
    }

    /// Ask GitHub for the latest release. The result is shown where the update page reads it;
    /// nothing pops up.
    func check() async {
        guard enabled, let repo else { return }
        if state.isWorking { return }
        state = .checking
        do {
            let rel = try await Self.fetchLatest(repo: repo)
            checkedAt = Date()
            if Self.isNewer(rel.version, than: current) && Self.systemAllows(rel.minimumSystemVersion) {
                latest = rel
                state = .available
            } else {
                latest = nil
                state = .upToDate
            }
        } catch {
            state = .failed(L.text("检查更新失败：\(error.localizedDescription)"))
        }
    }

    nonisolated static func fetchLatest(repo: String) async throws -> Release {
        var req = URLRequest(url: URL(string: "https://api.github.com/repos/\(repo)/releases/latest")!)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.timeoutInterval = 20
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard (resp as? HTTPURLResponse)?.statusCode == 200 else { throw UpdateError(L.text("GitHub 返回 \((resp as? HTTPURLResponse)?.statusCode ?? 0)")) }
        let rel = try JSON.parse(data)
        var assets: [String: URL] = [:]
        for a in rel["assets"].array { if let u = URL(string: a["browser_download_url"].str) { assets[a["name"].str] = u } }
        guard let manifestURL = assets["update.json"] else { throw UpdateError(L.text("最新的发布 \(rel["tag_name"].str) 没有 update.json，只能下载 DMG 手动安装")) }
        var mreq = URLRequest(url: manifestURL)
        mreq.timeoutInterval = 30
        let (mdata, mresp) = try await URLSession.shared.data(for: mreq)
        guard (mresp as? HTTPURLResponse)?.statusCode == 200 else { throw UpdateError(L.text("下载 update.json 失败")) }
        return try release(from: JSON.parse(mdata), assets: assets, notes: rel["body"].str, page: URL(string: rel["html_url"].str))
    }

    nonisolated static func release(from m: JSON, assets: [String: URL], notes: String, page: URL?) throws -> Release {
        let parts = m["parts"].array.map { p in
            Part(name: p["name"].str, file: p["file"].str, sha256: p["sha256"].str.lowercased(), size: p["size"].int ?? 0, content: p["content"].str, url: assets[p["file"].str])
        }
        guard m["version"].truthy, parts.contains(where: { $0.name == "app" }), parts.allSatisfy({ $0.url != nil && $0.sha256.count == 64 }) else {
            throw UpdateError(L.text("update.json 不完整"))
        }
        return Release(version: m["version"].str, parts: parts, minimumSystemVersion: m["minimumSystemVersion"].string ?? "15.0", notes: notes, page: page)
    }

    /// Dotted versions compared number by number: 2026.11.01.2 is newer than 2026.11.01.
    nonisolated static func isNewer(_ a: String, than b: String) -> Bool {
        let pa = a.split(separator: ".").map { Int($0) ?? 0 }, pb = b.split(separator: ".").map { Int($0) ?? 0 }
        for i in 0..<max(pa.count, pb.count) {
            let x = i < pa.count ? pa[i] : 0, y = i < pb.count ? pb[i] : 0
            if x != y { return x > y }
        }
        return false
    }

    nonisolated static func systemAllows(_ minimum: String) -> Bool {
        let v = ProcessInfo.processInfo.operatingSystemVersion
        return !isNewer(minimum, than: "\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)")
    }

    /// The parts to download: always the app; Node.js only when it is not the one installed.
    nonisolated static func plan(_ r: Release, installedNode: String?) -> [Part] {
        r.parts.filter { $0.name != "node" || installedNode == nil || $0.content != installedNode }
    }

    // MARK: Installing

    func install() async {
        guard let r = latest else { return }
        if bundle.path.contains("/AppTranslocation/") {
            state = .failed(L.text("Janus 正从隔离位置运行，不能自动更新。先把 Janus 拖到「应用程序」文件夹，再从那里打开。"))
            return
        }
        let parent = bundle.deletingLastPathComponent()
        guard FileManager.default.isWritableFile(atPath: parent.path) else {
            state = .failed(L.text("没有写入 \(parent.path) 的权限，不能替换 Janus.app。请下载 DMG 手动安装。"))
            return
        }
        let stage = home.appendingPathComponent("updates/\(r.version)")
        let installed = installedNode
        do {
            try? FileManager.default.removeItem(at: stage)
            try FileManager.default.createDirectory(at: stage, withIntermediateDirectories: true)
            var parts = Self.plan(r, installedNode: installed)
            try await fetch(parts, into: stage)
            state = .verifying
            var app = try await Self.assemble(r, parts: parts, stage: stage, current: bundle)
            if (try? await Self.verify(app, like: bundle, version: r.version)) == nil, !parts.contains(where: { $0.name == "node" }),
               let node = r.parts.first(where: { $0.name == "node" }) {
                // The reused Node.js does not match the new signature: take the release's.
                parts.append(node)
                try await fetch([node], into: stage)
                app = try await Self.assemble(r, parts: parts, stage: stage, current: bundle)
            }
            try await Self.verify(app, like: bundle, version: r.version)
            state = .relaunching
            try relaunch(into: app)
        } catch {
            state = .failed(L.text("更新失败：\(error.localizedDescription)"))
        }
    }

    private func fetch(_ parts: [Part], into stage: URL) async throws {
        for (i, p) in parts.enumerated() {
            let size = ByteCountFormatter.string(fromByteCount: Int64(p.size), countStyle: .file)
            state = .downloading(part: p.name == "node" ? "Node.js" : "Janus", index: i + 1, count: parts.count, size: size)
            guard let url = p.url else { throw UpdateError(L.text("缺少 \(p.file) 的下载地址")) }
            var req = URLRequest(url: url)
            req.timeoutInterval = 120
            let (tmp, resp) = try await URLSession.shared.download(for: req)
            guard (resp as? HTTPURLResponse)?.statusCode == 200 else { throw UpdateError(L.text("下载 \(p.file) 失败")) }
            let dest = stage.appendingPathComponent(p.file)
            try? FileManager.default.removeItem(at: dest)
            try FileManager.default.moveItem(at: tmp, to: dest)
            let got = try await Task.detached { try Self.sha256(dest) }.value
            guard got == p.sha256 else { throw UpdateError(L.text("\(p.file) 的校验和不符，可能下载不完整")) }
        }
    }

    /// Unpack the app part, then Node.js: the downloaded part, or a copy of the installed one.
    nonisolated static func assemble(_ r: Release, parts: [Part], stage: URL, current: URL) async throws -> URL {
        try await Task.detached {
            let app = stage.appendingPathComponent("Janus.app")
            try? FileManager.default.removeItem(at: app)
            guard let appPart = parts.first(where: { $0.name == "app" }) else { throw UpdateError(L.text("缺少应用部分")) }
            try run("/usr/bin/tar", ["-xzf", stage.appendingPathComponent(appPart.file).path, "-C", stage.path])
            let res = app.appendingPathComponent("Contents/Resources")
            if let node = parts.first(where: { $0.name == "node" }) {
                try run("/usr/bin/tar", ["-xzf", stage.appendingPathComponent(node.file).path, "-C", res.path])
            } else {
                // A clone on APFS: instant, and no extra disk space.
                try run("/bin/cp", ["-cpR", current.appendingPathComponent("Contents/Resources/node").path, res.path + "/"])
            }
            _ = try? run("/usr/bin/xattr", ["-dr", "com.apple.quarantine", app.path])
            return app
        }.value
    }

    /// Valid signed code of the expected version; signed by the running app's identity when it has
    /// a stable one (an ad-hoc signature's requirement is its own hash, which no update can meet).
    nonisolated static func verify(_ app: URL, like current: URL, version: String) async throws {
        try await Task.detached {
            var code: SecStaticCode?
            guard SecStaticCodeCreateWithPath(app as CFURL, [], &code) == errSecSuccess, let code else { throw UpdateError(L.text("读不到新版本的签名")) }
            let flags = SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSStrictValidate | kSecCSCheckNestedCode)
            let status = SecStaticCodeCheckValidity(code, flags, stableRequirement(current))
            guard status == errSecSuccess else {
                throw UpdateError(status == errSecCSReqFailed ? L.text("新版本的签名者与当前的 Janus 不同，已拒绝安装") : L.text("新版本的签名无效（\(status)）"))
            }
            let info = NSDictionary(contentsOf: app.appendingPathComponent("Contents/Info.plist"))
            guard info?["CFBundleShortVersionString"] as? String == version else { throw UpdateError(L.text("新版本的版本号与发布不符")) }
        }.value
    }

    nonisolated static func stableRequirement(_ app: URL) -> SecRequirement? {
        guard let (req, text) = CodeSigning.designatedRequirement(app) else { return nil }
        return text.contains("cdhash") ? nil : req
    }

    /// Hand over to a script that waits for this process to end, moves the old bundle to
    /// `<home>/updates/previous/` (kept for going back by hand), moves the new one in and opens it.
    /// When the new one cannot be moved in, the old one goes back in its place; when even that
    /// fails, the old one opens from `previous/`.
    private func relaunch(into app: URL) throws {
        let dir = home.appendingPathComponent("updates")
        let script = dir.appendingPathComponent("install.sh")
        try Self.installScript.write(to: script, atomically: true, encoding: .utf8)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = [script.path, String(getpid()), bundle.path, app.path,
                       dir.appendingPathComponent("previous/Janus.app").path,
                       home.appendingPathComponent("logs/update.log").path]
        p.standardInput = FileHandle.nullDevice
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        try p.run()
        quitting = true
        onQuit?()
        NSApp.terminate(nil)
        // A modal panel can hold termination; the installer only needs this process to end.
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { exit(0) }
    }

    static let installScript = """
    #!/bin/sh
    # Janus in-app update: wait for the app to quit, swap the bundle, open the new one.
    pid="$1"; cur="$2"; new="$3"; prev="$4"; log="$5"
    mkdir -p "$(dirname "$log")"
    exec >>"$log" 2>&1
    echo "$(date '+%F %T') update: $new -> $cur"
    i=0
    while kill -0 "$pid" 2>/dev/null; do
      sleep 0.2; i=$((i + 1))
      [ "$i" -gt 900 ] && { echo "Janus did not quit; update not installed"; exit 1; }
    done
    rm -rf "$prev"; mkdir -p "$(dirname "$prev")"
    if mv "$cur" "$prev"; then
      if mv "$new" "$cur"; then
        echo "installed"
      else
        # A move across volumes copies: a failed one can leave part of the new bundle behind,
        # and moving the old one back onto it would nest it inside.
        echo "could not move the new version in; restoring"
        rm -rf "$cur"
        if mv "$prev" "$cur"; then echo "restored"; else echo "could not restore; the previous version is at $prev"; fi
      fi
    else
      echo "could not move $cur aside"
    fi
    if [ -d "$cur" ]; then open "$cur"; else open "$prev"; fi
    """

    // MARK: Helpers

    nonisolated static func sha256(_ url: URL) throws -> String {
        let h = try FileHandle(forReadingFrom: url)
        defer { try? h.close() }
        var hasher = SHA256()
        while let chunk = try h.read(upToCount: 1 << 20), !chunk.isEmpty { hasher.update(data: chunk) }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    @discardableResult
    nonisolated static func run(_ cmd: String, _ args: [String]) throws -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: cmd)
        p.arguments = args
        let err = Pipe()
        p.standardError = err
        p.standardOutput = FileHandle.nullDevice
        try p.run()
        let msg = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            throw UpdateError(L.text("\((cmd as NSString).lastPathComponent) 失败：\(String(decoding: msg, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))"))
        }
        return ""
    }
}

struct UpdateError: LocalizedError {
    let message: String
    init(_ m: String) { message = m }
    var errorDescription: String? { message }
}
