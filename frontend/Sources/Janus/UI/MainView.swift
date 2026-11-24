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
        .disclosureGroupStyle(.row)
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
        .sheet(isPresented: Binding(get: { store.editingBrief != nil }, set: { if !$0 { store.editingBrief = nil } })) {
            if let ws = store.editingBrief { BriefEditor(store: store, original: ws) }
        }
        .sheet(isPresented: Binding(get: { store.editingLoop != nil }, set: { if !$0 { store.editingLoop = nil } })) {
            if let l = store.editingLoop { LoopEditor(store: store, original: l) }
        }
        .sheet(isPresented: Binding(get: { store.editingMemory != nil }, set: { if !$0 { store.editingMemory = nil } })) {
            if let m = store.editingMemory { MemoryEditor(store: store, original: m) }
        }
    }

    @ViewBuilder private var detail: some View {
        switch store.screen {
        case .inbox:
            InboxView(store: store)
        case .chats:
            ChatsView(store: store)
        case .home(let id):
            if let ws = store.workspace(id) { HomeView(store: store, ws: ws).id(id) }
            else if let past = store.pastWorkspaces.first(where: { $0["id"].str == id }) { PastWorkspaceView(store: store, ws: past) }
            else { NoWorkspaceView(store: store) }
        case .run:
            if let run = store.run { RunView(run: run) }
        case .newWorkspace:
            NoWorkspaceView(store: store)
        case nil:
            ProgressView()
        }
    }
}

/// The sidebar in three parts: the inbox at the top (across everything, always in sight), then the
/// workspaces (one row each: its team, Loops and task records are on its page) and the chats with
/// one digital employee, which follow one another in one scrolling list. Chats lead with the
/// employee they are with.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct Sidebar: View {
    let store: AppStore

    var body: some View {
        let ws = store.currentWorkspace
        // The inbox stays in sight; the two histories follow one another and scroll together, so a
        // short task list leaves no gap above the chats. Laid out by hand rather than in a List, so
        // headings, cards and records line up with the inbox.
        // Calm, like a source list: grey section labels instead of icons and rules, one line per
        // item, the open one on a white card.
        VStack(alignment: .leading, spacing: 0) {
            InboxButton(store: store).padding(.top, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 1) {
                    // Every workspace, one row each: a click opens it (its task records are on its
                    // page), the plus makes a new one.
                    SidebarHeading(title: L.text("团队"), plus: L.text("新建工作区…")) {
                        store.editingWorkspace = AppStore.blankWorkspace
                    }
                    ForEach(store.workspaces, id: \.self) { w in
                        WorkspaceRow(store: store, ws: w, current: w["id"].str == ws?["id"].str)
                    }
                    // Deleted workspaces whose runs stay, so those runs are still found.
                    ForEach(store.pastWorkspaces, id: \.self) { w in
                        let open: Bool = { if case .home(let id) = store.screen { return id == w["id"].str } else { return false } }()
                        SidebarItem(title: L.text("\(w["name"].str)（已删除）"), selected: open, help: L.text("已删除的工作区，留下 \(w["runs"].int ?? 0) 个任务记录"),
                                    action: { store.open(.home(w["id"].str)) }) {
                            InitialMark(text: w["name"].str, selected: open).opacity(0.6)
                        }
                        .foregroundStyle(.secondary)
                    }
                    SidebarHeading(title: L.text("对话"), plus: L.text("新对话：选一名数字员工单独对话（⌥⌘N）")) { store.open(.chats) }
                    chatRows()
                }
                .padding(.bottom, 10)
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
                        .help(L.text("设置（⌘,）"))
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

    /// The chats: who each one is with, its first message, and whether its session is open. Every
    /// one of them: scrolling to the last reads the next page.
    @ViewBuilder private func chatRows() -> some View {
        ForEach(store.chats, id: \.self) { c in
            let member = c["config"]["team"][0]
            let waiting = store.inbox.open.filter { $0.run == c["id"].str }.count
            let open = RunStatus(c["status"].str).isActive
            let (label, _) = ChatRowStatus.label(RunStatus(c["status"].str))
            SidebarItem(title: ChatRowStatus.title(c), selected: shown(c["id"].str), waiting: waiting, live: open,
                        help: "\(member["name"].str) · \(label) · \(L.dateTime(c["createdAt"].double ?? 0))",
                        accessibility: L.text("和 \(member["name"].str) 的对话，\(label)"),
                        action: { store.open(.run(c["id"].str)) }) {
                InitialMark(text: member["name"].str, selected: shown(c["id"].str))
            }
            .contextMenu {
                Button(L.text("删除对话…"), role: .destructive) { Task { await store.deleteRun(c["id"].str) } }
            }
            .onAppear { if c == store.chats.last { Task { await store.chatList.loadMore() } } }
        }
        if store.chatList.more { ProgressView().controlSize(.small).frame(maxWidth: .infinity).padding(.vertical, 6) }
    }
}

/// A section label of the sidebar: small grey text, and a quiet plus where something can be
/// started there. Sections are told apart by these labels and the space above them, not by rules.
struct SidebarHeading: View {
    let title: String
    var plus: String?
    var action: () -> Void = {}

    var body: some View {
        HStack(spacing: 6) {
            Text(title)
            Spacer(minLength: 6)
            if let plus {
                IconButton(symbol: "plus", label: plus, size: 11, color: .secondary, action: action)
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 20)
        .padding(.top, 18)
        .padding(.bottom, 4)
        .accessibilityAddTraits(.isHeader)
    }
}

/// One item of the sidebar, on one line: what leads it, its name, a count when something waits on
/// the person, a small green dot while it is at work. The open one sits on a white card with a
/// faint shadow. Details go in the tooltip.
struct SidebarItem<Lead: View>: View {
    let title: String
    let selected: Bool
    var waiting = 0
    var live = false
    var help = ""
    var accessibility = ""
    let action: () -> Void
    @ViewBuilder var lead: Lead

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                lead.frame(width: 18)
                Text(title).lineLimit(1)
                Spacer(minLength: 4)
                if waiting > 0 {
                    Text(verbatim: "\(waiting)")
                        .font(.caption.monospacedDigit().weight(.medium))
                        .foregroundStyle(Tone.amber.color)
                } else if live {
                    Circle().fill(Tone.green.color).frame(width: 6, height: 6).accessibilityHidden(true)
                }
            }
            .foregroundStyle(Color.primary)
            .padding(.horizontal, 10).padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background {
                if selected {
                    RoundedRectangle(cornerRadius: 8).fill(Color.card)
                        .shadow(color: .black.opacity(0.08), radius: 1.5, y: 1)
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .hoverable(8)
        .help(help)
        .accessibilityValue(accessibility)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .padding(.horizontal, 10)
    }
}

/// A name's first character in a thin rounded outline: the line-icon look for items that have no
/// icon of their own (workspaces, the employee of a chat).
struct InitialMark: View {
    let text: String
    let selected: Bool

    var body: some View {
        Text(String(text.prefix(1)).uppercased())
            .font(.system(size: 10, weight: .semibold))
            .frame(width: 17, height: 17)
            .foregroundStyle(selected ? Color.primary : Color.secondary)
            .overlay(RoundedRectangle(cornerRadius: 4.5).strokeBorder(selected ? Color.primary.opacity(0.7) : Color.secondary.opacity(0.6), lineWidth: 1.1))
            .accessibilityHidden(true)
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
                    Text(L.text("工作区 = 一个目录 + 一支数字员工团队")).foregroundStyle(.secondary)
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
            .help(L.text("去「设置 › 通用 › 更新」看更新内容并安装"))
        case .ready:
            Button { open() } label: {
                Label(L.text("Janus \(updater.latest?.version ?? "") 已下载，空闲时更新"), systemImage: "arrow.down.app")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .help(L.text("没有任务和对话在跑时自动重开换上；去「设置 › 通用 › 更新」可以现在更新"))
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
        SidebarItem(title: L.text("收件箱"), selected: on, waiting: n, help: L.text("所有工作区的待处理"),
                    accessibility: n > 0 ? L.text("所有工作区，\(n) 件待处理") : L.text("所有工作区"),
                    action: { store.open(.inbox) }) {
            Image(systemName: "tray").foregroundStyle(on ? Color.primary : Color.secondary)
        }
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
            .help(updates.map { "\($0["name"].str) \($0["current"].str) → \($0["latest"].str)" }.joined(separator: L.text("、")) + L.text("：去「设置 › 数字员工」升级，正在跑的会话不受影响"))
        }
    }
}

/// One workspace in the sidebar, on one line led by its initial: a click opens its page (its task
/// records are there), and a right click sets it up. The current one is highlighted on its page and while
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
        case .run(let id):
            return store.runs(of: ws).contains { $0["id"].str == id } || store.loops(of: ws["id"].str).contains { $0["run"].str == id }
        default: return false
        }
    }

    // The sidebar lists workspaces only: a workspace's Loops are on its page, and count here.
    var body: some View {
        let waiting = store.waiting(in: ws).reduce(0) { $0 + $1.entries.count }
        let working = store.runs(of: ws).contains { RunStatus($0["status"].str).isActive }
            || store.loops(of: ws["id"].str).contains { $0["status"]["phase"].str == "awake" }
        SidebarItem(title: ws["name"].str, selected: shown, waiting: waiting, live: working, help: L.tilde(ws["path"].str),
                    accessibility: waiting == 0 ? "" : L.text("\(waiting) 件待处理"),
                    action: { store.open(.home(ws["id"].str)) }) {
            InitialMark(text: ws["name"].str, selected: shown)
        }
        .contextMenu {
            Button(L.text("工作区设置…")) { store.editingWorkspace = ws }
            Button(L.text("在 Finder 中显示")) { NSWorkspace.shared.open(URL(fileURLWithPath: ws["path"].str)) }
        }
    }
}
