import AppKit
import Observation
import SwiftUI

@main
struct JanusApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("Janus", id: "main") {
            RootView(model: delegate.model)
                .environment(\.locale, L.locale)
                .frame(minWidth: 980, minHeight: 640)
        }
        .defaultSize(width: 1320, height: 860)
        .commands { JanusCommands(model: delegate.model) }

        // Janus stays in the menu bar, next to the clock and battery; the Dock icon and the app's
        // menus show only while one of its windows is open (see `AppDelegate.followWindows`).
        // Settings are a panel over the main window (⌘,), not a window of their own.
        MenuBarExtra {
            StatusMenu(model: delegate.model)
                .environment(\.locale, L.locale)
        } label: {
            MenuBarLabel(model: delegate.model)
        }
    }
}

/// The menu bar icon carries the number of things waiting on the person, so there is no need to
/// open the window to check on runs that are just working. The mark is the app icon's gate, drawn
/// as a template image so the menu bar tints it in either appearance.
struct MenuBarLabel: View {
    let model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let n = model.store?.inbox.open.count ?? 0
        HStack(spacing: 3) {
            Image(nsImage: MenuBarMark.image)
                .frame(width: 16, height: 16)
            if n > 0 { Text("\(n)") }
        }
        .accessibilityLabel(n > 0 ? L.text("Janus，\(n) 项等你处理") : "Janus")
        // The label lives as long as the app, so it is where a notification click finds a way
        // to open the window.
        .onAppear {
            Notifier.shared.onOpen = { run in
                openWindow(id: "main")
                NSApp.activate(ignoringOtherApps: true)
                model.store?.open(.run(run))
            }
        }
    }
}

/// Janus's gate as a menu bar template image: an arch with a seam and two eyes, the same mark as
/// the app icon (frontend/scripts/make-icon.swift).
enum MenuBarMark {
    static let image: NSImage = {
        let side: CGFloat = 18
        let img = NSImage(size: NSSize(width: side, height: side), flipped: true) { rect in
            let s = rect.width
            let cx = s / 2
            let w = s * 0.76, h = s * 0.82, r = w / 2
            let bottom = s * 0.92, top = bottom - h, spring = top + r
            let k: CGFloat = 0.5522847498
            let p = NSBezierPath()
            p.move(to: NSPoint(x: cx - r, y: bottom))
            p.line(to: NSPoint(x: cx - r, y: spring))
            p.curve(to: NSPoint(x: cx, y: top),
                    controlPoint1: NSPoint(x: cx - r, y: spring - r * k),
                    controlPoint2: NSPoint(x: cx - r * k, y: top))
            p.curve(to: NSPoint(x: cx + r, y: spring),
                    controlPoint1: NSPoint(x: cx + r * k, y: top),
                    controlPoint2: NSPoint(x: cx + r, y: spring - r * k))
            p.line(to: NSPoint(x: cx + r, y: bottom))
            p.close()
            let eyeR = s * 0.09 * 0.76
            let eyeY = top + h * 0.40
            for dx in [-1.0, 1.0] {
                p.append(NSBezierPath(ovalIn: NSRect(x: cx + CGFloat(dx) * s * 0.16 - eyeR, y: eyeY - eyeR, width: eyeR * 2, height: eyeR * 2)))
            }
            let seamW = s * 0.030
            p.append(NSBezierPath(rect: NSRect(x: cx - seamW / 2, y: top + h * 0.03, width: seamW, height: h * 0.94)))
            p.windingRule = .evenOdd
            NSColor.black.setFill()
            p.fill()
            return true
        }
        img.isTemplate = true
        return img
    }()
}

/// The menu under Janus's menu bar icon: the way back to the window, and what the Dock and the
/// app menu used to offer.
struct StatusMenu: View {
    let model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let store = model.store
        let waiting = store?.inbox.open.count ?? 0
        let active = store?.runs.filter { RunStatus($0["status"].str).isActive }.count ?? 0
        Button(waiting > 0 ? L.text("收件箱：\(waiting) 项等你处理") : L.text("收件箱：没有等你处理的事")) { show(); store?.open(.inbox) }.disabled(store == nil)
        Button(L.text("打开 Janus")) { show() }
        if active > 0 { Text(L.text("\(active) 个任务正在进行")) }
        Divider()
        Button(L.text("新目标")) { show(); model.newGoal() }.disabled(store == nil)
        Button(L.text("设置…")) { show(); store?.openSettings(store?.settingsTab ?? "general") }.disabled(store == nil)
        Button(L.text("检查更新…")) { show(); store?.openSettings(.updates); Task { await Updater.shared.check() } }
        Button(L.text("打开 Janus 日志")) { model.openLogs() }
        Divider()
        Button(L.text("退出 Janus")) { NSApp.terminate(nil) }.keyboardShortcut("q")
    }

    private func show() {
        openWindow(id: "main")
        NSApp.activate(ignoringOtherApps: true)
    }
}

/// Every command in the window is also in the menu bar, with the standard shortcuts where one
/// exists. Commands that do not apply right now stay visible and disabled.
struct JanusCommands: Commands {
    let model: AppModel

    var body: some Commands {
        let store = model.store
        let run = store?.run
        let onRun: Bool = { if case .run = store?.screen { return run != nil } else { return false } }()
        CommandGroup(after: .appInfo) {
            Button(L.text("检查更新…")) {
                store?.openSettings(.updates)
                Task { await Updater.shared.check() }
            }.disabled(store == nil)
        }
        CommandGroup(replacing: .appSettings) {
            Button(L.text("设置…")) { store?.openSettings(store?.settingsTab ?? "general") }.keyboardShortcut(",").disabled(store == nil)
        }
        SidebarCommands()
        CommandGroup(replacing: .newItem) {
            Button(L.text("新目标")) { model.newGoal() }.keyboardShortcut("n").disabled(store == nil)
            Button(L.text("新建工作区…")) { store?.editingWorkspace = AppStore.blankWorkspace }.keyboardShortcut("n", modifiers: [.command, .shift]).disabled(store == nil)
            Divider()
            Button(L.text("打开 Markdown 文件…")) { ArtifactWindow.choose() }.keyboardShortcut("o")
        }
        CommandGroup(replacing: .importExport) {
            Button(L.text("导出事件日志…")) { run?.exportLog() }.keyboardShortcut("e", modifiers: [.command, .shift]).disabled(!onRun)
        }
        CommandGroup(before: .sidebar) {
            Button(L.text("工作台")) { run?.show(.board) }.keyboardShortcut("1").disabled(!onRun)
            Button(L.text("审计与回放")) { run?.show(.audit) }.keyboardShortcut("2").disabled(!onRun)
            Button(L.text("团队与权限")) { run?.show(.team) }.keyboardShortcut("3").disabled(!onRun)
            Button(L.text("返回工作台")) { run?.show(.board) }.keyboardShortcut("[").disabled(!onRun || !(run?.page.isDetail ?? false))
            Divider()
            Button(L.text("刷新")) { Task { await model.refresh() } }.keyboardShortcut("r").disabled(store == nil)
            Divider()
        }
        CommandMenu(L.text("任务")) {
            let r = run?.state.run
            let active = onRun && run?.canAct == true && r.map { !$0.status.isFinished } == true
            Button(r?.paused == true ? L.text("继续") : L.text("暂停")) { Task { await run?.togglePause() } }
                .keyboardShortcut("p", modifiers: [.command, .shift])
                .disabled(!active || !(r?.status == .running || r?.paused == true))
            Button(L.text("从中断处继续")) { Task { await run?.act(["type": "resumeRun"]) } }
                .disabled(!(onRun && run?.canAct == true && r?.status.canResume == true))
            Button(L.text("停止任务…")) { if let run { cancelRun(run) } }.disabled(!active)
            Button(L.text("删除任务…"), role: .destructive) {
                if let run { Task { await run.store.deleteRun(run.id) } }
            }
            .disabled(!(onRun && run?.loaded == true && !(run?.state.run.status.isActive ?? true)))
            Divider()
            Button(L.text("复制交付摘要")) { run?.copySummary() }.keyboardShortcut("c", modifiers: [.command, .shift]).disabled(!onRun || run?.state.result == nil)
            Button(L.text("回到实时")) { run?.replay = nil }.disabled(!(run?.isReplaying ?? false))
        }
        CommandGroup(replacing: .help) {
            Button(L.text("打开 Janus 日志")) { model.openLogs() }
        }
    }
}

/// The backend and, once it answers, the store every view reads.
@Observable @MainActor
final class AppModel {
    let backend: Backend
    let permissions: MacPermissions
    var store: AppStore?

    init() {
        let backend = Backend()
        self.backend = backend
        permissions = .live(home: backend.home)
    }

    func start() async {
        permissions.prepareForLaunch()
        await backend.start()
        if backend.phase == .ready {
            let s = AppStore(api: API(base: backend.base), permissions: permissions)
            s.serverNotice = backend.notice
            store = s
            await s.boot()
        }
    }

    func retry() async {
        store = nil
        await backend.retry()
        await start()
    }

    func refresh() async {
        guard let s = store else { return }
        await s.loadSetup()
        await s.loadRuns()
        await s.run?.load()
    }

    func newGoal() {
        guard let s = store else { return }
        if let ws = s.currentWorkspace { s.open(.home(ws["id"].str)) } else { s.open(.newWorkspace) }
    }

    func openLogs() {
        let logs = backend.home.appendingPathComponent("logs")
        NSWorkspace.shared.open(logs)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        // The appearance chosen in settings outlives the session; the window opens at launch, so
        // start with the Dock icon and the menus.
        Appearance.saved.apply()
        NSApp.setActivationPolicy(.regular)
        followWindows()
        NSApp.activate(ignoringOtherApps: true)
        Task { await model.start() }
        Updater.shared.onQuit = { [weak model] in model?.backend.stop() }
        Updater.shared.start()
        Snapshots.start(model)
        // Killed rather than quit (kill, Ctrl-C in a terminal): still stop the server it started.
        for sig in [SIGTERM, SIGINT] {
            signal(sig, SIG_IGN)
            let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            src.setEventHandler { [model] in
                MainActor.assumeIsolated { model.backend.stop() }
                exit(0)
            }
            src.resume()
            signals.append(src)
        }
    }

    private var signals: [DispatchSourceSignal] = []
    private var windowObservers: [NSObjectProtocol] = []

    /// While a Janus window is open, Janus is a regular app (Dock icon, its menus in the menu bar);
    /// once the last one closes it only keeps its menu bar icon. The menu bar icon's own window
    /// cannot become main, so it does not count.
    private func followWindows() {
        let center = NotificationCenter.default
        let update: @Sendable (Notification) -> Void = { note in
            MainActor.assumeIsolated {
                let closing = note.name == NSWindow.willCloseNotification ? note.object as? NSWindow : nil
                let open = NSApp.windows.contains { $0 !== closing && $0.isVisible && $0.canBecomeMain }
                let policy: NSApplication.ActivationPolicy = open ? .regular : .accessory
                guard NSApp.activationPolicy() != policy else { return }
                NSApp.setActivationPolicy(policy)
                if open { NSApp.activate(ignoringOtherApps: true) }
            }
        }
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.willCloseNotification] {
            windowObservers.append(center.addObserver(forName: name, object: nil, queue: .main, using: update))
        }
    }

    /// Closing the window keeps Janus in the menu bar; it quits from there (or ⌘Q).
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Quitting stops the server the app started; its running runs become interrupted and can be
        // resumed later. A server the app found running (the launchd service) keeps going.
        // Installing an update: the person already agreed to quit.
        if !Updater.shared.quitting, model.backend.owned, let s = model.store,
           s.runs.contains(where: { RunStatus($0["status"].str).isActive }) {
            if !confirm(L.text("退出 Janus？"), L.text("还有正在执行的任务。退出会停止 Janus 服务，这些任务会中断，下次打开后可以「从中断处继续」。"), ok: L.text("退出")) { return .terminateCancel }
        }
        model.backend.stop()
        return .terminateNow
    }
}

struct RootView: View {
    let model: AppModel

    var body: some View {
        Group {
            if let store = model.store {
                MainView(store: store)
            } else {
                StartupView(model: model)
            }
        }
        .sheet(isPresented: Binding(get: { model.permissions.reviewPresented }, set: { if !$0 { model.permissions.dismissReview() } })) {
            MacPermissionReview(permissions: model.permissions)
        }
        .onChange(of: L.locale.identifier) {
            guard let store = model.store else { return }
            store.run?.refreshLocalizedState()
            Task { await store.inbox.refresh(store) }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            model.permissions.refresh()
        }
    }
}

struct StartupView: View {
    let model: AppModel

    var body: some View {
        VStack(spacing: 16) {
            Text("J")
                .font(.system(size: 34, weight: .bold, design: .serif))
                .foregroundStyle(.white)
                .frame(width: 64, height: 64)
                .background(Color.accent, in: RoundedRectangle(cornerRadius: 16))
            if case .failed(let err) = model.backend.phase {
                Text(L.text("Janus 服务没有启动")).font(.display(.title2))
                ScrollView {
                    Text(err).font(.system(.callout, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxWidth: 640, maxHeight: 260)
                .padding(10)
                .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
                HStack {
                    Button(L.text("打开日志目录")) { model.openLogs() }
                    Button(L.text("重试")) { Task { await model.retry() } }.buttonStyle(.primary)
                }
            } else {
                ProgressView().controlSize(.small)
                Text(textOf(model.backend.phase)).foregroundStyle(.secondary)
            }
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func textOf(_ p: Backend.Phase) -> String {
        if case .starting(let t) = p { return t }
        return L.text("正在读取数据…")
    }
}
