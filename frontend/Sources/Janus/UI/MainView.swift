import AppKit
import SwiftUI

/// The window: workspaces, their runs and the settings in the sidebar; the chosen screen on the right.
struct MainView: View {
    let store: AppStore
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            split
            // Settings take over the whole window instead of opening another one; closing them
            // shows the screen that was there, unchanged.
            if store.showingSettings {
                SettingsView(store: store)
                    .background(Color.page)
                    .transition(.opacity)
            }
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: store.showingSettings)
        .tint(Color.accent)
    }

    private var split: some View {
        NavigationSplitView {
            Sidebar(store: store)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            ZStack(alignment: .bottom) {
                detail
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color.page)
                if let t = store.toast {
                    Toast(text: t) { store.toast = nil }
                        .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(.easeOut(duration: 0.2), value: store.toast)
        }
        .sheet(isPresented: Binding(get: { store.editingWorkspace != nil }, set: { if !$0 { store.editingWorkspace = nil } })) {
            if let ws = store.editingWorkspace { WorkspaceEditor(store: store, original: ws) }
        }
    }

    @ViewBuilder private var detail: some View {
        switch store.screen {
        case .inbox:
            InboxView(store: store)
        case .home(let id):
            if let ws = store.workspace(id) { HomeView(store: store, ws: ws).id(id) } else { NoWorkspaceView(store: store) }
        case .run:
            if let run = store.run { RunView(run: run) }
        case .newWorkspace:
            NoWorkspaceView(store: store)
        case nil:
            ProgressView()
        }
    }
}

struct Sidebar: View {
    let store: AppStore

    var body: some View {
        let selection = Binding<Screen?>(get: { store.screen }, set: { if let s = $0 { store.open(s) } })
        let ws = store.currentWorkspace
        List(selection: selection) {
            Section {
                let n = store.inbox.open.count
                Label("收件箱", systemImage: "tray")
                    .badge(n)
                    .accessibilityValue(n == 0 ? "" : "\(n) 项等你处理")
                    .tag(Screen.inbox)
            }
            if let ws {
                let runs = store.runs(of: ws)
                Section("任务记录") {
                    if runs.isEmpty { Text("还没有任务").foregroundStyle(.secondary).font(.callout) }
                    ForEach(runs, id: \.self) { r in
                        // A run waiting on the person says so, with how many, like the inbox.
                        let waiting = store.inbox.open.filter { $0.run == r["id"].str }.count
                        let (label, tone) = waiting > 0 ? ("等你处理", Tone.amber) : L.runListStatus[r["status"].str] ?? (r["status"].str, .gray)
                        HStack(spacing: 8) {
                            Circle().fill(tone.color).frame(width: 7, height: 7).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(L.goalTitle(r["goal"].str)).lineLimit(1)
                                Text(verbatim: "\(label) · \(L.dateTime(r["createdAt"].double ?? 0))").font(.caption).foregroundStyle(waiting > 0 ? tone.color : .secondary)
                            }
                        }
                        .badge(waiting)
                        .accessibilityValue(waiting == 0 ? label : "\(waiting) 项等你处理")
                        .tag(Screen.run(r["id"].str))
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(Color.sidebar.ignoresSafeArea())
        .safeAreaInset(edge: .top, spacing: 0) { WorkspacePicker(store: store, current: ws) }
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Circle().fill(Tone.green.color).frame(width: 6, height: 6).accessibilityHidden(true)
                    Text("服务已连接")
                        .font(.caption).foregroundStyle(.secondary)
                        .help("Janus 服务地址：\(store.api.base.host ?? ""):\(String(store.api.base.port ?? 80))")
                    Spacer()
                    Button { store.openSettings(store.settingsTab) } label: { Label("设置", systemImage: "gearshape") }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .help("Agents、角色和 ACP 适配器（⌘,）")
                }
                UpdateBadge(updater: Updater.shared)
                if !store.skillSourceUpdates.isEmpty {
                    Button { store.openSettings("skills") } label: {
                        Label("\(store.skillSourceUpdates.count) 个 Skill 来源有更新", systemImage: "arrow.down.circle")
                            .font(.caption).foregroundStyle(Tone.blue.color)
                    }
                    .buttonStyle(.plain)
                    .help(store.skillSourceUpdates.map { $0["repo"].str }.joined(separator: "、") + "：GitHub 上有新的提交，打开 Skill 库更新")
                }
                if let n = store.serverNotice { Text(n).font(.caption2).foregroundStyle(Tone.amber.color).fixedSize(horizontal: false, vertical: true) }
            }
            .padding(.horizontal, 14).padding(.vertical, 8)
        }
    }
}

/// Steps still missing before a first task can run.
struct OnboardingBar: View {
    let store: AppStore
    var body: some View {
        let steps = store.onboarding
        if !steps.allSatisfy(\.1) {
            Card(tint: .accent) {
                HStack(spacing: 14) {
                    Text("开始使用 Janus").font(.headline)
                    ForEach(Array(steps.enumerated()), id: \.offset) { i, s in
                        HStack(spacing: 5) {
                            Text(s.1 ? "✓" : "\(i + 1)")
                                .font(.caption.weight(.bold))
                                .frame(width: 18, height: 18)
                                .background((s.1 ? Tone.green.color : Color.secondary).opacity(0.18), in: Circle())
                            Text(s.0).foregroundStyle(s.1 ? .secondary : .primary)
                        }
                    }
                    Spacer()
                    if let next = steps.first(where: { !$0.1 }) {
                        Button(next.0) {
                            if next.2 == "workspace" { store.editingWorkspace = AppStore.blankWorkspace } else { store.openSettings(next.2) }
                        }
                        .buttonStyle(.primary)
                    }
                }
            }
        }
    }
}

struct NoWorkspaceView: View {
    let store: AppStore
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                OnboardingBar(store: store)
                Card {
                    Text("还没有工作区").font(.display(.title3))
                    Text("工作区指定任务在哪个目录执行、由哪些角色参与。").foregroundStyle(.secondary)
                    Button("新建工作区") { store.editingWorkspace = AppStore.blankWorkspace }
                        .buttonStyle(.primary)
                }
            }
            .padding(24)
            .frame(maxWidth: 860)
        }
    }
}

/// A new Janus in the sidebar: one click to update, the progress while it installs, or why it failed.
struct UpdateBadge: View {
    let updater: Updater

    var body: some View {
        switch updater.state {
        case .available:
            Button { updater.offer() } label: {
                Label("Janus \(updater.latest?.version ?? "") 可以更新", systemImage: "arrow.down.app.fill")
                    .font(.caption.weight(.semibold)).foregroundStyle(Color.accentColor)
            }
            .buttonStyle(.plain)
            .help("下载更新（\(ByteCountFormatter.string(fromByteCount: Int64(updater.downloadSize), countStyle: .file))），完成后自动重新打开")
        case .downloading, .verifying, .relaunching:
            HStack(spacing: 6) { ProgressView().controlSize(.mini); Text(updater.state.workingText ?? "").font(.caption).foregroundStyle(.secondary) }
        case .failed(let text) where updater.latest != nil:
            VStack(alignment: .leading, spacing: 2) {
                Text(text).font(.caption2).foregroundStyle(Tone.red.color).fixedSize(horizontal: false, vertical: true)
                Button("重试更新") { Task { await updater.install() } }.buttonStyle(.link).font(.caption)
            }
        default:
            EmptyView()
        }
    }
}

/// The workspace in use, as a dropdown: the whole card opens the menu to pick a workspace (which
/// opens its home page), create one, or set it up. Highlighted while its
/// home page is shown. The sidebar below lists only this workspace's run history.
// @project-doc docs/architecture/frontend.md#screens
struct WorkspacePicker: View {
    let store: AppStore
    let current: JSON?

    private var onHome: Bool {
        if case .home(let id) = store.screen { return id == current?["id"].str }
        return false
    }

    var body: some View {
        Menu {
            ForEach(store.workspaces, id: \.self) { w in
                Button { store.open(.home(w["id"].str)) } label: {
                    if w["id"].str == current?["id"].str { Label(w["name"].str, systemImage: "checkmark") } else { Text(w["name"].str) }
                }
            }
            if !store.workspaces.isEmpty { Divider() }
            Button("新建工作区…") { store.editingWorkspace = AppStore.blankWorkspace }
            if let current {
                Button("工作区设置…") { store.editingWorkspace = current }
                Button("在 Finder 中显示") { NSWorkspace.shared.open(URL(fileURLWithPath: current["path"].str)) }
            }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: "folder.fill").foregroundStyle(onHome ? Color.white : Color.accentColor)
                VStack(alignment: .leading, spacing: 1) {
                    Text(current?["name"].str ?? "选择工作区").fontWeight(.semibold).lineLimit(1)
                    if let current {
                        Text(L.tilde(current["path"].str)).font(.caption).lineLimit(1).truncationMode(.middle)
                            .foregroundStyle(onHome ? Color.white.opacity(0.8) : Color.secondary)
                    }
                }
                Spacer(minLength: 4)
                Image(systemName: "chevron.up.chevron.down").font(.caption.weight(.semibold))
                    .foregroundStyle(onHome ? Color.white.opacity(0.9) : Color.secondary)
            }
            .foregroundStyle(onHome ? Color.white : Color.primary)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(onHome ? Color.accentColor : Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .help("切换或新建工作区")
        .accessibilityLabel("工作区：\(current?["name"].str ?? "未选择")")
        .padding(.horizontal, 10).padding(.top, 6).padding(.bottom, 4)
    }
}
