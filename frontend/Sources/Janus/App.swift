import AppKit
import Observation
import SwiftUI

@main
struct JanusApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("Janus", id: "main") {
            RootView(model: delegate.model)
                .frame(minWidth: 980, minHeight: 640)
        }
        .defaultSize(width: 1320, height: 860)
        .commands { JanusCommands(model: delegate.model) }

        // Agents, roles and adapters: the standard settings window (⌘,), one pane each.
        Settings {
            SettingsView(model: delegate.model)
        }
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
        SidebarCommands()
        CommandGroup(replacing: .newItem) {
            Button("新目标") { model.newGoal() }.keyboardShortcut("n").disabled(store == nil)
            Button("新建工作区…") { store?.editingWorkspace = AppStore.blankWorkspace }.keyboardShortcut("n", modifiers: [.command, .shift]).disabled(store == nil)
        }
        CommandGroup(replacing: .importExport) {
            Button("导出事件日志…") { run?.exportLog() }.keyboardShortcut("e", modifiers: [.command, .shift]).disabled(!onRun)
        }
        CommandGroup(before: .sidebar) {
            Button("工作台") { run?.show(.board) }.keyboardShortcut("1").disabled(!onRun)
            Button("审计与回放") { run?.show(.audit) }.keyboardShortcut("2").disabled(!onRun)
            Button("团队与权限") { run?.show(.team) }.keyboardShortcut("3").disabled(!onRun)
            Button("返回工作台") { run?.show(.board) }.keyboardShortcut("[").disabled(!onRun || !(run?.page.isDetail ?? false))
            Divider()
            Button("刷新") { Task { await model.refresh() } }.keyboardShortcut("r").disabled(store == nil)
            Divider()
        }
        CommandMenu("Run") {
            let r = run?.state.run
            let active = onRun && run?.canAct == true && r.map { !RunState.finished.contains($0.status) } == true
            Button(r?.paused == true ? "继续" : "暂停") { Task { await run?.togglePause() } }
                .keyboardShortcut("p", modifiers: [.command, .shift])
                .disabled(!active || !(r?.status == "running" || r?.paused == true))
            Button("从中断处继续") { Task { await run?.act(["type": "resumeRun"]) } }
                .disabled(!(onRun && run?.canAct == true && ["interrupted", "failed"].contains(r?.status ?? "")))
            Button("停止 Run…") { if let run { cancelRun(run) } }.disabled(!active)
            Divider()
            Button("复制交付摘要") { run?.copySummary() }.keyboardShortcut("c", modifiers: [.command, .shift]).disabled(!onRun || run?.state.result == nil)
            Button("回到实时") { run?.replay = nil }.disabled(!(run?.isReplaying ?? false))
        }
        CommandGroup(replacing: .help) {
            Button("打开 Janus 日志") { model.openLogs() }
        }
    }
}

/// The backend and, once it answers, the store every view reads.
@Observable @MainActor
final class AppModel {
    let backend = Backend()
    var store: AppStore?

    func start() async {
        await backend.start()
        if backend.phase == .ready {
            let s = AppStore(api: API(base: backend.base))
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
        // Run as a regular app also when started outside a bundle (`swift run`).
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        Task { await model.start() }
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

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Quitting stops the server the app started; its running runs become interrupted and can be
        // resumed later. A server the app found running (the launchd service) keeps going.
        if model.backend.owned, let s = model.store,
           s.runs.contains(where: { ["planning", "awaiting_plan", "running", "waiting_input"].contains($0["status"].str) }) {
            if !confirm("退出 Janus？", "还有正在执行的任务。退出会停止 Janus 服务，这些任务会中断，下次打开后可以「从中断处继续」。", ok: "退出") { return .terminateCancel }
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
                Text("Janus 服务没有启动").font(.title2.weight(.semibold))
                ScrollView {
                    Text(err).font(.system(.callout, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxWidth: 640, maxHeight: 260)
                .padding(10)
                .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
                HStack {
                    Button("打开日志目录") { model.openLogs() }
                    Button("重试") { Task { await model.retry() } }.buttonStyle(.borderedProminent)
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
        return "正在读取数据…"
    }
}
