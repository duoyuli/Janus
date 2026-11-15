import AppKit
import SwiftUI

/// The window: workspaces, their runs and the settings in the sidebar; the chosen screen on the right.
struct MainView: View {
    let store: AppStore
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            split
            // Settings float over the window in a centered panel instead of opening another one;
            // closing them shows the screen that was there, unchanged.
            if store.showingSettings {
                SettingsView(store: store)
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
            if let ws {
                let runs = store.runs(of: ws)
                Section {
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
                } header: {
                    // A finished run no longer needs a session to start the next one: the
                    // workspace home is where a new goal is written. A quiet plus in the
                    // header keeps the entry out of the list of runs.
                    HStack(spacing: 6) {
                        Text("任务记录")
                        Spacer(minLength: 6)
                        Button { store.open(.home(ws["id"].str)) } label: {
                            Image(systemName: "plus")
                                .font(.system(size: 11, weight: .semibold))
                                .frame(width: 16, height: 16)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)
                        .hoverable(5)
                        .help("新建任务：打开工作区，写下目标并交给主控")
                        .accessibilityLabel("新建任务")
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(Color.sidebar.ignoresSafeArea())
        .safeAreaInset(edge: .top, spacing: 0) {
            VStack(alignment: .leading, spacing: 0) {
                sidebarLabel("全局")
                InboxButton(store: store)
                Rectangle().fill(Color.line).frame(height: 1).padding(.top, 6)
                sidebarLabel("工作区").padding(.top, 2)
                WorkspacePicker(store: store, current: ws)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
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

    /// A section label above the global inbox and the workspace card, like the sidebar's own
    /// headers: the inbox is personal and spans every workspace, the card is the one open.
    private func sidebarLabel(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 20)
            .padding(.top, 10)
            .padding(.bottom, 3)
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

/// The global inbox: everything across workspaces that waits on the person. It sits above the
/// workspace card because it is personal and spans every workspace, not part of the one open.
struct InboxButton: View {
    let store: AppStore

    private var on: Bool {
        if case .inbox = store.screen { return true }
        return false
    }

    var body: some View {
        let n = store.inbox.open.count
        Button { store.open(.inbox) } label: {
            HStack(spacing: 8) {
                Image(systemName: "tray")
                    .frame(width: 18)
                    .foregroundStyle(on ? Color.accent : Color.secondary)
                Text("收件箱").fontWeight(on ? .semibold : .regular)
                Spacer(minLength: 4)
                if n > 0 { Pill("\(n)", .amber) }
            }
            .foregroundStyle(Color.primary)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(on ? Color.accentColor.opacity(0.15) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .hoverable(8)
        .help("所有工作区中需要你处理的事")
        .accessibilityLabel(n > 0 ? "收件箱，所有工作区，\(n) 项等你处理" : "收件箱，所有工作区")
        .accessibilityAddTraits(on ? .isSelected : [])
        .padding(.horizontal, 10)
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
