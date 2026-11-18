import AppKit
import CoreGraphics
import Observation
import Security

enum MacPermission: String, CaseIterable, Codable, Identifiable {
    case fullDisk, appManagement, screenRecording

    var id: Self { self }
    var manual: Bool { self != .screenRecording }
    var title: String {
        switch self {
        case .fullDisk: "完整磁盘访问"
        case .appManagement: "应用程序管理"
        case .screenRecording: "屏幕录制"
        }
    }
    var icon: String {
        switch self {
        case .fullDisk: "internaldrive"
        case .appManagement: "app.badge.checkmark"
        case .screenRecording: "rectangle.dashed.badge.record"
        }
    }
    var purpose: String {
        switch self {
        case .fullDisk: "让 Agent 按任务需要访问受保护的文件。文件自身的读写权限和系统保护仍然生效。"
        case .appManagement: "让 Agent 按任务需要更新、替换或移除应用程序。"
        case .screenRecording: "让 Agent 的截屏工具读取屏幕内容。检查权限时不会截图。"
        }
    }
    var settingsURL: URL {
        let pane: String
        switch self {
        case .fullDisk: pane = "Privacy_AllFiles"
        case .appManagement: pane = "Privacy_AppBundles"
        case .screenRecording: pane = "Privacy_ScreenCapture"
        }
        return URL(string: "x-apple.systempreferences:com.apple.preference.security?\(pane)")!
    }
}

enum MacPermissionStatus: Equatable {
    case unavailable, needsConfirmation, manuallyConfirmed, granted, notGranted

    var label: String {
        switch self {
        case .unavailable: "请使用安装包"
        case .needsConfirmation: "需手动确认"
        case .manuallyConfirmed: "已手动确认"
        case .granted: "已授权"
        case .notGranted: "未检测到授权"
        }
    }
    var tone: Tone {
        switch self {
        case .granted: .green
        case .manuallyConfirmed: .blue
        case .needsConfirmation, .notGranted: .amber
        case .unavailable: .gray
        }
    }
    var needsAttention: Bool { self == .needsConfirmation || self == .notGranted }
}

/// System permission guidance belongs to this app instance. A manual acknowledgement is never
/// treated as a system grant, nor carried over to a different installed build.
// @project-doc docs/architecture/20261031-frontend.md#macos_permissions
// @project-doc docs/operations/20261031-security.md#macos_permissions
@Observable @MainActor
final class MacPermissions {
    struct Installation: Codable, Equatable {
        var bundleID: String
        var path: String
        var version: String
        var build: String
        var codeHash: String?

        static func current(_ bundle: Bundle = .main) -> Self? {
            guard bundle.bundleURL.pathExtension == "app", let id = bundle.bundleIdentifier else { return nil }
            var code: SecStaticCode?
            var info: CFDictionary?
            var hash: String?
            if SecStaticCodeCreateWithPath(bundle.bundleURL as CFURL, [], &code) == errSecSuccess, let code,
               SecCodeCopySigningInformation(code, [], &info) == errSecSuccess, let info,
               let data = (info as NSDictionary)[kSecCodeInfoUnique] as? Data {
                hash = data.map { String(format: "%02x", $0) }.joined()
            }
            return Self(bundleID: id, path: bundle.bundleURL.resolvingSymlinksInPath().path,
                        version: bundle.infoDictionary?["CFBundleShortVersionString"] as? String ?? "",
                        build: bundle.infoDictionary?["CFBundleVersion"] as? String ?? "", codeHash: hash)
        }
    }

    struct Record: Codable, Equatable {
        var installation: Installation
        var reminded = false
        var confirmed: Set<MacPermission> = []
    }

    struct System {
        var screenAllowed: () -> Bool
        var requestScreen: () -> Bool
        var openSettings: (MacPermission) -> Bool
        var revealApp: () -> Void
    }

    enum Flow: Equatable {
        case idle
        case waiting(MacPermission, remaining: [MacPermission])
        case complete
    }

    let installation: Installation?
    private(set) var screenAllowed = false
    private(set) var record: Record?
    private(set) var flow: Flow = .idle
    private(set) var reviewPresented = false
    private var actionError: String?
    private var persistenceError: String?
    @ObservationIgnored private let system: System
    @ObservationIgnored private let save: (Record) throws -> Void

    init(installation: Installation?, record: Record? = nil, system: System, save: @escaping (Record) throws -> Void = { _ in }) {
        self.installation = installation
        self.system = system
        self.save = save
        if let installation {
            self.record = record?.installation == installation ? record : Record(installation: installation)
        }
    }

    var available: Bool { installation != nil }
    var error: String? { actionError ?? persistenceError }
    var current: MacPermission? {
        if case .waiting(let permission, _) = flow { return permission }
        return nil
    }
    var outstanding: [MacPermission] { MacPermission.allCases.filter { status($0).needsAttention } }

    func status(_ permission: MacPermission) -> MacPermissionStatus {
        guard available else { return .unavailable }
        if permission == .screenRecording { return screenAllowed ? .granted : .notGranted }
        return record?.confirmed.contains(permission) == true ? .manuallyConfirmed : .needsConfirmation
    }

    /// Called once per launch, even when the backend cannot start. The identity is that of the
    /// running bundle; the updater does not have to set a separate, potentially stale flag.
    func prepareForLaunch() {
        guard available else { return }
        refresh()
        guard record?.reminded == false else { return }
        record?.reminded = true
        persist()
        reviewPresented = true
    }

    func dismissReview() {
        reviewPresented = false
        flow = .idle
    }

    func refresh() {
        guard available else { return }
        screenAllowed = system.screenAllowed()
        if current == .screenRecording && screenAllowed { advance() }
    }

    func beginAll() {
        guard available else { return }
        refresh()
        begin(outstanding)
    }

    func begin(_ permission: MacPermission) {
        guard available else { return }
        begin([permission])
    }

    private func begin(_ permissions: [MacPermission]) {
        actionError = nil
        guard let first = permissions.first else { flow = .complete; return }
        flow = .waiting(first, remaining: Array(permissions.dropFirst()))
        if first.manual { openSettings(first) }
        else {
            // The return value alone is not cached as a grant: the preflight check is the source.
            if !system.requestScreen() {
                actionError = "尚未获得屏幕录制权限。请在系统提示中允许；若没有提示，请打开系统设置。"
            }
            refresh()
            if screenAllowed { actionError = nil }
        }
    }

    func openSettings(_ permission: MacPermission) {
        guard available else { return }
        actionError = nil
        if !system.openSettings(permission) {
            actionError = "无法打开系统设置。请手动打开「系统设置 → 隐私与安全性 → \(permission.title)」。"
        }
    }

    func revealApp() {
        guard available else { return }
        system.revealApp()
    }

    func confirmCurrent() {
        guard let permission = current, permission.manual, available else { return }
        record?.confirmed.insert(permission)
        persist()
        advance()
    }

    func skipCurrent() {
        guard current != nil else { return }
        advance()
    }

    private func advance() {
        guard case .waiting(_, let remaining) = flow else { return }
        begin(remaining.filter { status($0).needsAttention })
    }

    private func persist() {
        guard let record else { return }
        do { try save(record); persistenceError = nil }
        catch { persistenceError = "无法保存权限确认记录，下次启动可能再次提醒：\(error.localizedDescription)" }
    }

    static func unavailable() -> MacPermissions {
        MacPermissions(installation: nil, system: System(screenAllowed: { false }, requestScreen: { false }, openSettings: { _ in false }, revealApp: {}))
    }

    static func live(home: URL) -> MacPermissions {
        guard let installation = Installation.current() else { return unavailable() }
        let file = home.appendingPathComponent("permissions/review.json")
        let record = (try? Data(contentsOf: file)).flatMap { try? JSONDecoder().decode(Record.self, from: $0) }
        let system = System(screenAllowed: { CGPreflightScreenCaptureAccess() }, requestScreen: { CGRequestScreenCaptureAccess() }, openSettings: { permission in
            if NSWorkspace.shared.open(permission.settingsURL) { return true }
            // Settings anchors may change between macOS releases. Keep a path to the privacy pane.
            return NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy")!)
        }, revealApp: {
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: installation.path)])
        })
        return MacPermissions(installation: installation, record: record, system: system) { record in
            let fm = FileManager.default
            try fm.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            try JSONEncoder().encode(record).write(to: file, options: .atomic)
            try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        }
    }
}
