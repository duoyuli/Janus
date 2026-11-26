import AppKit
import CryptoKit
import Foundation
import Observation
import Security

/// In-app updates of Janus.app from the GitHub releases of `JanusUpdateRepo` (Info.plist).
///
/// A release carries update.json and two parts: the bundle without Node.js, and Node.js. Only the
/// parts that changed are downloaded (Node.js is reused when its content hash is the same), each
/// checked against update.json's SHA-256, put together under `cache/updates/<version>/` in Janus
/// home (only it and `previous/` are kept there), and
/// checked as signed code: valid, and, when the running app has a stable signing identity,
/// signed by that same identity. Then a small script swaps the bundle in after the app quits and
/// opens the new one.
///
/// The app downloads with URLSession, which sets no quarantine flag, so Gatekeeper does not stop
/// the new version the way it stops a DMG downloaded in a browser.
///
/// With automatic updates on (the default, Settings › 通用), a new version found by a check is
/// downloaded and checked at once, then swapped in once the server has no run at work.
// @project-doc docs/architecture/20261031-frontend.md#updates
@Observable @MainActor
final class Updater {
    static let shared = Updater()

    /// The update's own progress, with each step a case instead of a string the UI parses.
    enum State: Equatable {
        case idle, checking, upToDate, available
        case downloading(part: String, index: Int, count: Int, size: String)
        case verifying
        /// Downloaded and checked (`staged`): swapped in once nothing is at work, or by hand.
        case ready
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
        /// A release candidate (`2026.11.27-rc.1`): only the RC channel gets it.
        var candidate: Bool { Updater.isCandidate(version) }
    }

    /// Which releases this Mac follows. Stable: the date releases from main, GitHub's latest
    /// release. RC: the newest of those and the release candidates (prereleases) tagged on test
    /// branches. Leaving RC never goes back to an older stable version: the build stays until a
    /// stable one is newer, as an older server refuses data a newer one has upgraded.
    // @project-doc docs/architecture/20261031-frontend.md#update_channels
    enum Channel: String, CaseIterable, Identifiable {
        case stable, rc
        var id: String { rawValue }
        var title: String {
            switch self {
            case .stable: L.text("稳定版")
            case .rc: L.text("测试版（RC）")
            }
        }
    }

    private(set) var state: State = .idle
    private(set) var latest: Release?
    private(set) var checkedAt: Date?
    /// Set while installing, so quitting does not ask about running tasks a second time.
    private(set) var quitting = false
    /// Stops the server the app started before it exits to install a new version.
    var onQuit: (() -> Void)?
    /// Whether a run is at work that reopening the app would cut off, as the server says now
    /// (`atWork` of `GET /api/meta`); until the app can ask (not connected, no answer), it counts as busy.
    var isBusy: () async -> Bool = { true }
    /// The new version downloaded and checked, waiting to be swapped in (`State.ready`).
    private(set) var staged: URL?
    /// Install new versions without asking: download one as soon as it is found and swap it in
    /// once nothing is at work. Saved in the app's preferences; on unless turned off.
    var automatic: Bool {
        didSet {
            defaults.set(automatic, forKey: Self.automaticKey)
            if automatic, case .ready = state { swapInWhenQuiet() } else if !automatic { waiting?.cancel() }
        }
    }

    /// The channel this Mac follows, saved in the app's preferences; stable unless chosen.
    var channel: Channel {
        didSet {
            guard channel != oldValue else { return }
            defaults.set(channel.rawValue, forKey: Self.channelKey)
            // What was found on the other channel no longer applies; a staged version stays.
            if state != .ready && !state.isWorking {
                latest = nil
                Task { await check() }
            }
        }
    }

    let current: String
    let repo: String?
    let bundle: URL
    let home: JanusHome
    @ObservationIgnored private var timer: Task<Void, Never>?
    @ObservationIgnored private var waiting: Task<Void, Never>?
    @ObservationIgnored private let defaults: UserDefaults

    /// Every five minutes: with automatic updates on, a release reaches people within minutes,
    /// and twelve checks an hour stay well inside GitHub's 60 unauthenticated requests per hour.
    static let checkEvery: UInt64 = 5 * 60
    static let automaticKey = "janus.autoUpdate"
    static let channelKey = "janus.updateChannel"
    /// How often a staged version looks whether nothing is at work.
    static let quietCheck: UInt64 = 30
    /// Checks in a row with nothing at work before swapping in: a task that is only starting
    /// (created, not yet listed as working) is not cut off.
    static let quietRounds = 2

    init(defaults: UserDefaults = .janus) {
        self.defaults = defaults
        automatic = defaults.object(forKey: Self.automaticKey) as? Bool ?? true
        channel = defaults.string(forKey: Self.channelKey).flatMap(Channel.init(rawValue:)) ?? .stable
        let info = Bundle.main.infoDictionary ?? [:]
        current = info["CFBundleShortVersionString"] as? String ?? "0"
        repo = (info["JanusUpdateRepo"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        bundle = Bundle.main.bundleURL
        let env = ProcessInfo.processInfo.environment
        home = JanusHome.current(env)
    }

    /// Before a download: everything an earlier update left in `dir` goes, versions downloaded and
    /// never installed included, except `previous/`, the app to go back to by hand.
    nonisolated static func clearStaged(_ dir: URL) {
        let fm = FileManager.default
        for name in (try? fm.contentsOfDirectory(atPath: dir.path)) ?? [] where name != "previous" {
            try? fm.removeItem(at: dir.appendingPathComponent(name))
        }
    }

    /// Only a released Janus.app updates itself: `swift run` and `just mac` do not, a local build
    /// has no update source (`package.sh` without `--version`), and an app a test drives
    /// (`JANUS_SNAPSHOT_DIR`) never asks GitHub, nor swaps itself out from under the test.
    var enabled: Bool { repo != nil && bundle.pathExtension == "app" && !Snapshots.active }

    /// How much an update downloads, given what is installed.
    var downloadSize: Int { latest.map { Self.plan($0, installedNode: installedNode).reduce(0) { $0 + $1.size } } ?? 0 }

    var installedNode: String? {
        guard let data = try? Data(contentsOf: bundle.appendingPathComponent("Contents/Resources/update-parts.json")),
              let j = try? JSON.parse(data) else { return nil }
        return j["node"].string
    }

    // MARK: Checking

    /// Check shortly after launch, then every few minutes.
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
        // Installing, or a version waiting to be swapped in: nothing to ask GitHub.
        if state.isWorking || state == .ready { return }
        state = .checking
        do {
            let rel = try await Self.fetchNewest(repo: repo, channel: channel)
            checkedAt = Date()
            if Self.isNewer(rel.version, than: current) && Self.systemAllows(rel.minimumSystemVersion) {
                latest = rel
                state = .available
                if automatic { await install(now: false) }
            } else {
                latest = nil
                state = .upToDate
            }
        } catch {
            state = .failed(L.text("检查更新失败：\(error.localizedDescription)"))
        }
    }

    /// The newest release of `channel`: GitHub's latest release (never a prerelease) for stable;
    /// for RC, the newest version among the recent releases, candidates included. One request
    /// either way, then the release's update.json.
    nonisolated static func fetchNewest(repo: String, channel: Channel) async throws -> Release {
        switch channel {
        case .stable:
            return try await manifest(of: get("https://api.github.com/repos/\(repo)/releases/latest"))
        case .rc:
            let list = try await get("https://api.github.com/repos/\(repo)/releases?per_page=20")
            guard let rel = newest(list) else { throw UpdateError(L.text("GitHub 上还没有可以安装的发布")) }
            return try await manifest(of: rel)
        }
    }

    /// The newest release in a GitHub list that can be installed: published (not a draft), with
    /// update.json, the version read from its tag.
    nonisolated static func newest(_ list: JSON) -> JSON? {
        let installable = list.array.filter { r in
            !r["draft"].truthy && r["assets"].array.contains { $0["name"].str == "update.json" }
        }
        return installable.max { a, b in isNewer(version(of: b), than: version(of: a)) }
    }

    /// A release's version: its tag without the leading `v`.
    nonisolated static func version(of release: JSON) -> String {
        let tag = release["tag_name"].str
        return tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
    }

    private nonisolated static func get(_ url: String) async throws -> JSON {
        var req = URLRequest(url: URL(string: url)!)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.timeoutInterval = 20
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard (resp as? HTTPURLResponse)?.statusCode == 200 else { throw UpdateError(L.text("GitHub 返回 \((resp as? HTTPURLResponse)?.statusCode ?? 0)")) }
        return try JSON.parse(data)
    }

    /// The release's update.json, read into a `Release`.
    private nonisolated static func manifest(of rel: JSON) async throws -> Release {
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

    /// Dotted versions compared number by number: 2026.11.01.2 is newer than 2026.11.01. A
    /// release candidate comes before its version: 2026.11.27-rc.1 < 2026.11.27-rc.2 < 2026.11.27.
    nonisolated static func isNewer(_ a: String, than b: String) -> Bool {
        let (ba, ra) = split(a), (bb, rb) = split(b)
        if let order = compare(ba, bb) { return order }
        switch (ra, rb) {
        case (nil, nil): return false
        case (nil, _): return true
        case (_, nil): return false
        case let (x?, y?): return compare(x, y) ?? false
        }
    }

    /// Whether `version` is a release candidate.
    nonisolated static func isCandidate(_ version: String) -> Bool { split(version).1 != nil }

    /// A version as the app writes it: a release candidate says it is one.
    nonisolated static func label(_ version: String) -> String {
        isCandidate(version) ? L.text("\(version)（测试版）") : version
    }

    /// A version's numbers, and its candidate's numbers (`rc.2` → [2]) if it is one.
    private nonisolated static func split(_ v: String) -> ([Int], [Int]?) {
        let numbers = { (s: Substring) in s.split(separator: ".").map { Int($0) ?? 0 } }
        guard let dash = v.firstIndex(of: "-") else { return (numbers(v[...]), nil) }
        return (numbers(v[..<dash]), numbers(v[v.index(after: dash)...].drop { !$0.isNumber }))
    }

    /// Number by number, missing ones as 0; nil when equal.
    private nonisolated static func compare(_ a: [Int], _ b: [Int]) -> Bool? {
        for i in 0..<max(a.count, b.count) {
            let x = i < a.count ? a[i] : 0, y = i < b.count ? b[i] : 0
            if x != y { return x > y }
        }
        return nil
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

    /// Download, check and put together the latest release, then swap it in: now (`now`, the
    /// person asked), or once nothing is at work (an automatic update).
    func install(now: Bool = true) async {
        guard let app = await prepare() else { return }
        if now {
            swapIn(app)
        } else {
            staged = app
            state = .ready
            swapInWhenQuiet()
        }
    }

    /// The person asked to update now. Reopening the app cuts off the runs at work, so with any
    /// at work it asks first; `false` when the person keeps them going.
    func mayInterrupt() async -> Bool {
        guard await isBusy() else { return true }
        return confirm(L.text("现在更新 Janus？"), L.text("还有任务或对话在跑。更新会重开 Janus 并中断它们，重开后任务自动接着做。"), ok: L.text("现在更新"))
    }

    /// The staged version, swapped in now: the person asked for it.
    func installStaged() {
        guard let app = staged else { return }
        swapIn(app)
    }

    // @project-doc docs/architecture/20261031-frontend.md#auto_update
    /// Wait until nothing has been at work for `quietRounds` checks in a row, then swap in the
    /// staged version. Turning automatic updates off stops waiting; the version stays staged.
    private func swapInWhenQuiet() {
        waiting?.cancel()
        waiting = Task { [weak self] in
            var quiet = 0
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: Self.quietCheck * 1_000_000_000)
                guard let self, self.automatic, self.state == .ready, let app = self.staged, !Task.isCancelled else { return }
                let busy = await self.isBusy()
                guard self.automatic, self.state == .ready, !Task.isCancelled else { return }
                quiet = Self.quiet(after: quiet, busy: busy)
                if quiet >= Self.quietRounds {
                    self.swapIn(app)
                    return
                }
            }
        }
    }

    /// Checks in a row with nothing at work, after one more check.
    nonisolated static func quiet(after rounds: Int, busy: Bool) -> Int { busy ? 0 : rounds + 1 }

    private func swapIn(_ app: URL) {
        state = .relaunching
        do {
            try relaunch(into: app)
        } catch {
            state = .failed(L.text("更新失败：\(error.localizedDescription)"))
        }
    }

    /// The latest release downloaded (only the parts that changed), put together and checked as
    /// signed code; `nil` (with the reason in `state`) when it cannot be installed.
    private func prepare() async -> URL? {
        guard let r = latest else { return nil }
        if bundle.path.contains("/AppTranslocation/") {
            state = .failed(L.text("Janus 正从隔离位置运行，不能自动更新。先把 Janus 拖到「应用程序」文件夹，再从那里打开。"))
            return nil
        }
        let parent = bundle.deletingLastPathComponent()
        guard FileManager.default.isWritableFile(atPath: parent.path) else {
            state = .failed(L.text("没有写入 \(parent.path) 的权限，替换不了 Janus.app。下载 DMG 手动装吧。"))
            return nil
        }
        let stage = home.updates.appendingPathComponent(r.version)
        let installed = installedNode
        do {
            Self.clearStaged(home.updates)
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
            return app
        } catch {
            state = .failed(L.text("更新失败：\(error.localizedDescription)"))
            return nil
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
    /// `cache/updates/previous/` (kept for going back by hand), moves the new one in and opens it.
    /// When the new one cannot be moved in, the old one goes back in its place; when even that
    /// fails, the old one opens from `previous/`.
    private func relaunch(into app: URL) throws {
        let dir = home.updates
        let script = dir.appendingPathComponent("install.sh")
        try Self.installScript.write(to: script, atomically: true, encoding: .utf8)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = [script.path, String(getpid()), bundle.path, app.path,
                       dir.appendingPathComponent("previous/Janus.app").path,
                       home.updateLog.path]
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

    nonisolated static let installScript = """
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
