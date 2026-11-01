import Foundation
import Observation

/// The orchestrator process behind the window.
///
/// On launch the app looks for a Janus serving this Janus home: at the configured address
/// (`JANUS_ADDR`, else `addr` in `<home>/config.toml`, else 127.0.0.1:7878), then at the address a
/// still-running owner of `<home>/janus.pid` logged. If there is one (the launchd service, or
/// `just dev`), the app uses it. Otherwise it starts the `janus-server` binary bundled in
/// Janus.app, on the configured port or, when another program holds it, the next free one, and
/// stops it again when the app quits. Either way the data stays in Janus home.
// @project-doc docs/architecture/frontend.md#server_process
@Observable @MainActor
final class Backend {
    enum Phase: Equatable { case starting(String), ready, failed(String) }

    private(set) var phase: Phase = .starting("正在启动 Janus 服务…")
    /// Whether the app started the server itself (and so owns its lifetime).
    private(set) var owned = false
    let home: URL
    /// The address Janus is configured for.
    let configured: String
    /// The address in use: the configured one, or the one the app moved to.
    private(set) var addr: String
    /// Why the address differs from the configured one, shown in the sidebar.
    private(set) var notice: String?
    var base: URL { URL(string: "http://\(addr)")! }
    var logFile: URL { home.appendingPathComponent("logs/janus.out.log") }

    private var process: Process?
    private var stopping = false

    init() {
        let env = ProcessInfo.processInfo.environment
        let home = env["JANUS_HOME"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".janus")
        self.home = home
        // Same rule as the server's config.rs and the justfile's `_addr`.
        configured = env["JANUS_ADDR"].flatMap { $0.isEmpty ? nil : $0 } ?? Self.config(home, "addr") ?? "127.0.0.1:7878"
        addr = configured
    }

    /// A string setting from `<home>/config.toml` (`key = "value"` lines).
    nonisolated static func config(_ home: URL, _ key: String) -> String? {
        guard let text = try? String(contentsOf: home.appendingPathComponent("config.toml"), encoding: .utf8) else { return nil }
        for line in text.split(separator: "\n") {
            let l = line.trimmingCharacters(in: .whitespaces)
            guard let eq = l.firstIndex(of: "="), l[..<eq].trimmingCharacters(in: .whitespaces) == key else { continue }
            let v = l[l.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            if v.count >= 2 && v.hasPrefix("\"") && v.hasSuffix("\"") { return String(v.dropFirst().dropLast()) }
        }
        return nil
    }

    /// Node.js shipped in Janus.app (Contents/Resources/node), for the ACP adapters.
    nonisolated static func bundledNode() -> URL? {
        guard let res = Bundle.main.resourceURL else { return nil }
        let node = res.appendingPathComponent("node/bin/node")
        return FileManager.default.isExecutableFile(atPath: node.path) ? node : nil
    }

    func start() async {
        phase = .starting("正在连接 Janus 服务…")
        addr = configured
        notice = nil
        if await servesThisHome(configured) { owned = false; phase = .ready; return }
        // A Janus already owns this home (single instance), maybe on another port: use it.
        if let other = runningOwnerAddr() {
            if await servesThisHome(other) {
                addr = other
                notice = other == configured ? nil : "已在运行的 Janus 使用 \(other)"
                owned = false
                phase = .ready
                return
            }
            phase = .failed("另一个 Janus 正在使用 \(home.path)，但连不上它（\(other)）。先退出那个 Janus（以服务方式运行时执行 just stop），再点重试。")
            return
        }
        guard let bin = Self.serverBinary() else {
            phase = .failed("找不到 Janus 服务程序。安装包里的 Janus.app 自带服务；从源码运行时先执行 just install，或设置 JANUS_SERVER_BIN。")
            return
        }
        // The port can be taken between the check and the server binding it: try a few.
        var tried: Set<UInt16> = []
        for _ in 0..<3 {
            guard let (host, port) = Self.split(configured), let free = Self.freePort(host: host, from: port, skipping: tried) else {
                phase = .failed("\(configured) 不是有效的监听地址，检查 JANUS_ADDR 或 config.toml 的 addr。")
                return
            }
            tried.insert(free)
            addr = "\(host):\(free)"
            notice = free == port ? nil : "端口 \(port) 被其他程序占用，改用 \(free)"
            phase = .starting(notice.map { "\($0)，正在启动 Janus 服务…" } ?? "正在启动 Janus 服务…")
            do { try launch(bin) } catch {
                phase = .failed("启动 Janus 服务失败：\(error.localizedDescription)")
                return
            }
            for _ in 0..<100 {
                if case .failed = phase { break }
                if await servesThisHome(addr) { phase = .ready; return }
                try? await Task.sleep(nanoseconds: 300_000_000)
            }
            guard case .failed = phase else {
                phase = .failed("Janus 服务 30 秒内没有响应。\n\n\(logTail())")
                return
            }
            // Lost the race for the port: try the next one; any other failure stands.
            if !logTail(lines: 6).contains("Address already in use") { return }
        }
    }

    func retry() async {
        stop()
        await start()
    }

    /// Stops the server the app started (SIGTERM). A server it found running is left alone.
    func stop() {
        guard let p = process, p.isRunning else { return }
        stopping = true
        p.terminate()
        let deadline = Date().addingTimeInterval(3)
        while p.isRunning && Date() < deadline { usleep(50_000) }
        if p.isRunning { kill(p.processIdentifier, SIGKILL) }
        process = nil
    }

    /// Whether a Janus answers at `addr` with this Janus home's data. Something else on the port
    /// (another program, or a Janus of another home) does not count.
    private func servesThisHome(_ addr: String, timeout: TimeInterval = 1.5) async -> Bool {
        guard let url = URL(string: "http://\(addr)/api/meta") else { return false }
        var req = URLRequest(url: url)
        req.timeoutInterval = timeout
        let cfg = URLSessionConfiguration.ephemeral
        cfg.timeoutIntervalForRequest = timeout
        guard let (data, resp) = try? await URLSession(configuration: cfg).data(for: req),
              (resp as? HTTPURLResponse)?.statusCode == 200,
              let meta = try? JSON.parse(data), meta["kinds"].exists, let dir = meta["dataDir"].string else { return false }
        return Self.samePath(dir, home.path)
    }

    nonisolated static func samePath(_ a: String, _ b: String) -> Bool {
        URL(fileURLWithPath: a).resolvingSymlinksInPath().standardizedFileURL.path == URL(fileURLWithPath: b).resolvingSymlinksInPath().standardizedFileURL.path
    }

    /// The address of the Janus holding `<home>/janus.pid`, if that process is alive: the last
    /// "http://<addr>  ·  home <home>" line it wrote to janus.log.
    private func runningOwnerAddr() -> String? {
        guard let pid = (try? String(contentsOf: home.appendingPathComponent("janus.pid"), encoding: .utf8)).flatMap({ Int32($0.trimmingCharacters(in: .whitespacesAndNewlines)) }),
              pid != process?.processIdentifier, kill(pid, 0) == 0 else { return nil }
        guard let log = try? String(contentsOf: home.appendingPathComponent("logs/janus.log"), encoding: .utf8) else { return configured }
        for line in log.split(separator: "\n").reversed() {
            guard let r = line.range(of: #"http://(\S+)\s+·\s+home (.+)$"#, options: .regularExpression) else { continue }
            let m = line[r]
            let parts = m.dropFirst("http://".count).split(separator: " ", maxSplits: 1)
            guard let a = parts.first, let h = m.range(of: "home ") else { continue }
            if Self.samePath(String(m[h.upperBound...]), home.path) { return String(a) }
        }
        return configured
    }

    nonisolated static func split(_ addr: String) -> (String, UInt16)? {
        guard let i = addr.lastIndex(of: ":"), let port = UInt16(addr[addr.index(after: i)...]) else { return nil }
        let host = String(addr[..<i])
        return (host.isEmpty ? "127.0.0.1" : host, port)
    }

    /// `port` if nothing listens on it, else the next free one within 20, else one the system picks.
    nonisolated static func freePort(host: String, from port: UInt16, skipping: Set<UInt16> = []) -> UInt16? {
        let candidates = (0...20).compactMap { UInt16(exactly: Int(port) + $0) }.filter { !skipping.contains($0) }
        if let p = candidates.first(where: { bindable(host: host, port: $0) != nil }) { return p }
        return bindable(host: host, port: 0)
    }

    /// Binds a socket the way the server will (SO_REUSEADDR, like Rust's TcpListener) and returns
    /// the port it got, or nil when the address is taken.
    nonisolated static func bindable(host: String, port: UInt16) -> UInt16? {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        defer { close(fd) }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &on, socklen_t(MemoryLayout<Int32>.size))
        var sa = sockaddr_in()
        sa.sin_family = sa_family_t(AF_INET)
        sa.sin_port = port.bigEndian
        guard inet_pton(AF_INET, host == "localhost" ? "127.0.0.1" : host, &sa.sin_addr) == 1 else { return nil }
        let ok = withUnsafePointer(to: &sa) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) } }
        guard ok == 0 else { return nil }
        var got = sockaddr_in()
        var len = socklen_t(MemoryLayout<sockaddr_in>.size)
        _ = withUnsafeMutablePointer(to: &got) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &len) } }
        return UInt16(bigEndian: got.sin_port)
    }

    /// The bundled server (Janus.app/Contents/MacOS/janus-server), else one named by
    /// `JANUS_SERVER_BIN`, else the one `just install` puts in Janus home.
    static func serverBinary() -> URL? {
        let fm = FileManager.default
        var candidates: [URL] = []
        if let u = Bundle.main.url(forAuxiliaryExecutable: "janus-server") { candidates.append(u) }
        if let p = ProcessInfo.processInfo.environment["JANUS_SERVER_BIN"], !p.isEmpty { candidates.append(URL(fileURLWithPath: p)) }
        let home = ProcessInfo.processInfo.environment["JANUS_HOME"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0) }
            ?? fm.homeDirectoryForCurrentUser.appendingPathComponent(".janus")
        candidates.append(home.appendingPathComponent("bin/janus"))
        return candidates.first { fm.isExecutableFile(atPath: $0.path) }
    }

    private func launch(_ bin: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: logFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !fm.fileExists(atPath: logFile.path) { fm.createFile(atPath: logFile.path, contents: nil) }
        let log = try FileHandle(forWritingTo: logFile)
        log.seekToEndOfFile()

        let p = Process()
        p.executableURL = bin
        var env = ProcessInfo.processInfo.environment
        // Apps opened from Finder get a minimal PATH; agents need node, git and their own CLIs.
        env["PATH"] = Self.userPath()
        // The bundled Node.js runs the ACP adapters and their npm installs, so the Mac needs no
        // Node of its own. A node the person configured (JANUS_NODE, `node` in config.toml) wins.
        if let node = Self.bundledNode() {
            env["PATH"] = node.deletingLastPathComponent().path + ":" + (env["PATH"] ?? "")
            if (env["JANUS_NODE"] ?? "").isEmpty && Self.config(home, "node") == nil { env["JANUS_NODE"] = node.path }
        }
        env["JANUS_ADDR"] = addr
        p.environment = env
        p.currentDirectoryURL = fm.homeDirectoryForCurrentUser
        p.standardOutput = log
        p.standardError = log
        p.standardInput = FileHandle.nullDevice
        p.terminationHandler = { [weak self] proc in
            Task { @MainActor in
                guard let self else { return }
                let expected = self.stopping
                self.stopping = false
                if !expected { self.phase = .failed("Janus 服务退出了（退出码 \(proc.terminationStatus)）。\n\n\(self.logTail())") }
            }
        }
        try p.run()
        process = p
        owned = true
    }

    func logTail(lines: Int = 12) -> String {
        guard let text = try? String(contentsOf: logFile, encoding: .utf8) else { return "" }
        return text.split(separator: "\n", omittingEmptySubsequences: false).suffix(lines).joined(separator: "\n")
    }

    /// The PATH the user's login shell sets up (Homebrew, nvm, ~/.local/bin …), with common
    /// install locations added in case the shell could not be asked.
    nonisolated static func userPath() -> String {
        let env = ProcessInfo.processInfo.environment
        let shell = env["SHELL"].flatMap { $0.isEmpty ? nil : $0 } ?? "/bin/zsh"
        var parts: [String] = []
        for args in [["-ilc"], ["-lc"]] {
            if let out = run(shell, args + ["printf '__JANUS_PATH__%s' \"$PATH\""], timeout: 5),
               let r = out.range(of: "__JANUS_PATH__") {
                parts = out[r.upperBound...].trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ":").map(String.init)
                if !parts.isEmpty { break }
            }
        }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let extra = ["/opt/homebrew/bin", "/usr/local/bin", "\(home)/.local/bin", "\(home)/.cargo/bin", "\(home)/.bun/bin", "\(home)/.volta/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        parts += (env["PATH"] ?? "").split(separator: ":").map(String.init) + extra
        var seen = Set<String>()
        return parts.filter { !$0.isEmpty && seen.insert($0).inserted }.joined(separator: ":")
    }

    nonisolated private static func run(_ cmd: String, _ args: [String], timeout: TimeInterval) -> String? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: cmd)
        p.arguments = args
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return nil }
        let deadline = Date().addingTimeInterval(timeout)
        while p.isRunning && Date() < deadline { usleep(20_000) }
        if p.isRunning { p.terminate(); return nil }
        return String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    }
}
