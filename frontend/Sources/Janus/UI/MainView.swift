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
        case .chats:
            ChatsView(store: store)
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

/// The sidebar in three parts: the inbox at the top (across everything, always in sight), then 工作区
/// (the workspace's team and its task records) and 对话 (chats with one digital employee and their
/// history), which follow one another in one scrolling list. Task records lead with their status;
/// chats with the employee they are with.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct Sidebar: View {
    let store: AppStore
    /// How many chats are shown; the rest wait behind「展示更多」, in steps of ten.
    @Local private var shownChats = 10

    var body: some View {
        let ws = store.currentWorkspace
        // The inbox stays in sight; the two histories follow one another and scroll together, so a
        // short task list leaves no gap above the chats. Laid out by hand rather than in a List, so
        // headings, cards and records line up with the inbox.
        VStack(alignment: .leading, spacing: 0) {
            InboxButton(store: store).padding(.top, 8)
            Rectangle().fill(Color.line).frame(height: 1).padding(.top, 8)
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    // Every workspace, one row each: a click opens it (its task records are on its
                    // page), the plus makes a new one.
                    SidebarHeading(title: L.text("Agent Team"), icon: "person.3", plus: L.text("新建工作区…")) {
                        store.editingWorkspace = AppStore.blankWorkspace
                    }
                    ForEach(store.workspaces, id: \.self) { w in
                        WorkspaceRow(store: store, ws: w, current: w["id"].str == ws?["id"].str)
                    }
                    Rectangle().fill(Color.line).frame(height: 1).padding(.top, 14).padding(.bottom, 8)
                    SidebarHeading(title: L.text("Chat"), icon: "bubble.left.and.bubble.right", plus: L.text("新对话：选一名数字员工单独对话（⌥⌘N）")) { store.open(.chats) }
                    chatRows()
                }
                .padding(.bottom, 8)
            }
        }
        .background(Color.sidebar.ignoresSafeArea())
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Circle().fill(Tone.green.color).frame(width: 6, height: 6).accessibilityHidden(true)
                    Text(L.text("服务已连接"))
                        .font(.caption).foregroundStyle(.secondary)
                        .help(L.text("Janus 服务地址：\(store.api.base.host ?? ""):\(String(store.api.base.port ?? 80))"))
                    Spacer()
                    Button { store.openSettings(store.settingsTab) } label: { Label(L.text("设置"), systemImage: "gearshape") }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .help(L.text("Agent、数字员工和 ACP（⌘,）"))
                }
                UpdateBadge(updater: Updater.shared) { store.openSettings(.updates) }
                if !store.sourceUpdates.isEmpty {
                    Button { store.openLibrary(.skill) } label: {
                        Label(L.text("\(store.sourceUpdates.count) 个资源来源有更新"), systemImage: "arrow.down.circle")
                            .font(.caption).foregroundStyle(Tone.blue.color)
                    }
                    .buttonStyle(.plain)
                    .help(store.sourceUpdates.map { $0["repo"].str }.joined(separator: L.text("、")) + L.text("：GitHub 上有新的提交，打开资源库更新"))
                }
                AdapterUpdatesBadge(store: store)
                if let n = store.serverNotice { Text(n).font(.caption2).foregroundStyle(Tone.amber.color).fixedSize(horizontal: false, vertical: true) }
            }
            .padding(.horizontal, 14).padding(.vertical, 8)
            .background(Color.sidebar)
        }
    }

    private func shown(_ id: String) -> Bool {
        if case .run(let shown) = store.screen { return shown == id }
        return false
    }

    /// The chats: who each one is with, its first message, and whether its session is open.
    @ViewBuilder private func chatRows() -> some View {
        ForEach(store.chats.prefix(shownChats), id: \.self) { c in
            let member = c["config"]["team"][0]
            let waiting = store.inbox.open.filter { $0.run == c["id"].str }.count
            let (label, tone) = waiting > 0 ? (L.text("等你处理"), Tone.amber) : ChatRowStatus.label(RunStatus(c["status"].str))
            HistoryRow(title: ChatRowStatus.title(c), sub: "\(member["name"].str) · \(label) · \(L.dateTime(c["createdAt"].double ?? 0))", waiting: waiting, subTone: waiting > 0 ? tone : nil, accessibility: L.text("和 \(member["name"].str) 的对话，\(label)"), selected: shown(c["id"].str), open: { store.open(.run(c["id"].str)) }) {
                Avatar(initials: member["init"].truthy ? member["init"].str : String(member["name"].str.prefix(2)), color: member["color"].str, size: 18)
            }
            .contextMenu {
                Button(L.text("删除对话…"), role: .destructive) { Task { await store.deleteRun(c["id"].str) } }
            }
        }
        ShowMore(total: store.chats.count, shown: $shownChats, help: L.text("再显示 10 个对话"))
    }
}

/// A section heading of the sidebar, with an icon for the top-level sections and, where something
/// can be started there, a quiet plus.
struct SidebarHeading: View {
    let title: String
    var icon: String?
    var small = false
    var plus: String?
    var action: () -> Void = {}

    var body: some View {
        HStack(spacing: 6) {
            if let icon { Image(systemName: icon).font(.system(size: 10, weight: .semibold)).accessibilityHidden(true) }
            Text(title).fontWeight(small ? .regular : .semibold)
            Spacer(minLength: 6)
            if let plus {
                Button(action: action) {
                    Image(systemName: "plus")
                        .font(.system(size: 11, weight: .semibold))
                        .frame(width: 18, height: 18)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .hoverable(5)
                .help(plus)
                .accessibilityLabel(plus)
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 20)
        .padding(.top, small ? 8 : 10)
        .padding(.bottom, 3)
        .accessibilityAddTraits(.isHeader)
    }
}

/// One record in a sidebar history: what leads it (a status dot, an avatar), a title, and a line
/// saying how it stands; a count when something waits on the person. Selected like the inbox.
struct HistoryRow<Lead: View>: View {
    let title: String
    let sub: String
    let waiting: Int
    var subTone: Tone?
    let accessibility: String
    var selected = false
    let open: () -> Void
    @ViewBuilder var lead: Lead

    var body: some View {
        Button(action: open) {
                HStack(spacing: 8) {
                    lead
                    VStack(alignment: .leading, spacing: 1) {
                        Text(title).lineLimit(1).fontWeight(selected ? .semibold : .regular)
                        Text(verbatim: sub).font(.caption).foregroundStyle(subTone?.color ?? .secondary).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    if waiting > 0 { Pill("\(waiting)", .amber) }
                }
                .foregroundStyle(Color.primary)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(selected ? Color.accentColor.opacity(0.15) : .clear, in: RoundedRectangle(cornerRadius: 8))
                .contentShape(RoundedRectangle(cornerRadius: 8))
            }
        .buttonStyle(.plain)
        .hoverable(8)
        .accessibilityValue(accessibility)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .padding(.horizontal, 10)
    }
}

/// 「展示更多」at the end of a sidebar history: ten more each time.
struct ShowMore: View {
    let total: Int
    @Binding var shown: Int
    let help: String

    var body: some View {
        if total > shown {
            Button { shown = min(shown + 10, total) } label: {
                Text(L.text("展示更多"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.leading, 15)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .hoverable(6)
            .help(help)
            .padding(.horizontal, 10)
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
                    Text(L.text("开始使用 Janus")).font(.headline)
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
                            switch next.2 {
                            case .newWorkspace: store.editingWorkspace = AppStore.blankWorkspace
                            case .settings(let tab): store.openSettings(tab)
                            }
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
                    Text(L.text("还没有工作区")).font(.display(.title3))
                    Text(L.text("工作区指定任务在哪个目录执行、由哪些数字员工参与。")).foregroundStyle(.secondary)
                    Button(L.text("新建工作区")) { store.editingWorkspace = AppStore.blankWorkspace }
                        .buttonStyle(.primary)
                }
            }
            .padding(24)
            .frame(maxWidth: 860)
        }
    }
}

/// A new Janus in the sidebar: one click opens the updates pane, the progress while it installs,
/// or why it failed.
struct UpdateBadge: View {
    let updater: Updater
    var open: () -> Void = {}

    var body: some View {
        switch updater.state {
        case .available:
            Button { open() } label: {
                Label(L.text("Janus \(updater.latest?.version ?? "") 可以更新"), systemImage: "arrow.down.app.fill")
                    .font(.caption.weight(.semibold)).foregroundStyle(Color.accentColor)
            }
            .buttonStyle(.plain)
            .help(L.text("在设置 → 通用 → 更新中查看更新内容并安装"))
        case .downloading, .verifying, .relaunching:
            HStack(spacing: 6) { ProgressView().controlSize(.mini); Text(updater.state.workingText ?? "").font(.caption).foregroundStyle(.secondary) }
        case .failed(let text) where updater.latest != nil:
            VStack(alignment: .leading, spacing: 2) {
                Text(text).font(.caption2).foregroundStyle(Tone.red.color).fixedSize(horizontal: false, vertical: true)
                Button(L.text("重试更新")) { Task { await updater.install() } }.buttonStyle(.link).font(.caption)
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
                Text(L.text("收件箱")).fontWeight(on ? .semibold : .regular)
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
        .help(L.text("所有工作区中需要你处理的事"))
        .accessibilityLabel(n > 0 ? L.text("收件箱，所有工作区，\(n) 项等你处理") : L.text("收件箱，所有工作区"))
        .accessibilityAddTraits(on ? .isSelected : [])
        .padding(.horizontal, 10)
    }
}

/// Installed ACP adapters with a newer version on npm: one click opens the digital employees
/// pane, where each agent's adapter line upgrades it. Sessions already running keep their version.
// @project-doc docs/architecture/20261031-adapters.md#update_check
struct AdapterUpdatesBadge: View {
    let store: AppStore

    var body: some View {
        let updates = store.adapterUpdates
        if !updates.isEmpty {
            Button { store.openSettings(.agents) } label: {
                Label(L.text("\(updates.count) 个 ACP 适配器有新版本"), systemImage: "arrow.up.circle")
                    .font(.caption).foregroundStyle(Tone.blue.color)
            }
            .buttonStyle(.plain)
            .help(updates.map { "\($0["name"].str) \($0["current"].str) → \($0["latest"].str)" }.joined(separator: L.text("、")) + L.text("：在设置的「数字员工」页升级，正在运行的会话不受影响"))
        }
    }
}

/// One workspace in the sidebar, as a row like the inbox: a click opens its page (its task records
/// are there), and a right click sets it up. The current one is highlighted on its page and while
/// one of its runs is open; how many things its runs wait on the person for is counted.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct WorkspaceRow: View {
    let store: AppStore
    let ws: JSON
    let current: Bool

    private var shown: Bool {
        guard current else { return false }
        switch store.screen {
        case .home(let id): return id == ws["id"].str
        case .run(let id): return store.runs(of: ws).contains { $0["id"].str == id }
        default: return false
        }
    }

    var body: some View {
        let ids = Set(store.runs(of: ws).map { $0["id"].str })
        let waiting = store.inbox.open.filter { ids.contains($0.run) }.count
        HistoryRow(title: ws["name"].str, sub: L.tilde(ws["path"].str), waiting: waiting,
                   accessibility: waiting == 0 ? "" : L.text("\(waiting) 项等你处理"), selected: shown,
                   open: { store.open(.home(ws["id"].str)) }) {
            Image(systemName: shown ? "folder.fill" : "folder")
                .frame(width: 18)
                .foregroundStyle(shown ? Color.accent : Color.secondary)
                .accessibilityHidden(true)
        }
        .help(ws["path"].str)
        .contextMenu {
            Button(L.text("工作区设置…")) { store.editingWorkspace = ws }
            Button(L.text("在 Finder 中显示")) { NSWorkspace.shared.open(URL(fileURLWithPath: ws["path"].str)) }
        }
    }
}
