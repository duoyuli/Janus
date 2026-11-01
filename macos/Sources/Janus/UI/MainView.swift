import SwiftUI

/// The window: workspaces, their runs and the settings in the sidebar; the chosen screen on the right.
struct MainView: View {
    let store: AppStore

    var body: some View {
        NavigationSplitView {
            Sidebar(store: store)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            ZStack(alignment: .bottom) {
                detail
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if let t = store.toast {
                    Text(t)
                        .font(.callout)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 16).padding(.vertical, 10)
                        .background(Color.black.opacity(0.82), in: Capsule())
                        .padding(.bottom, 22)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                        .onTapGesture { store.toast = nil }
                }
            }
            .animation(.easeOut(duration: 0.2), value: store.toast)
        }
        .sheet(isPresented: Binding(get: { store.editingWorkspace != nil }, set: { if !$0 { store.editingWorkspace = nil } })) {
            if let ws = store.editingWorkspace { WorkspaceEditor(store: store, original: ws) }
        }
        .sheet(isPresented: Binding(get: { store.editingRole != nil }, set: { if !$0 { store.editingRole = nil } })) {
            if let r = store.editingRole { RoleEditor(store: store, original: r) }
        }
    }

    @ViewBuilder private var detail: some View {
        switch store.screen {
        case .home(let id):
            if let ws = store.workspace(id) { HomeView(store: store, ws: ws) } else { NoWorkspaceView(store: store) }
        case .run:
            if let run = store.run { RunView(run: run) }
        case .settings(let tab):
            switch tab {
            case "roles": RolesView(store: store)
            case "adapters": AdaptersView(store: store)
            default: AgentsView(store: store)
            }
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
            Section("工作区") {
                ForEach(store.workspaces, id: \.self) { w in
                    Label {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(w["name"].str)
                            Text(L.tilde(w["path"].str)).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                        }
                    } icon: { Image(systemName: "folder") }
                    .tag(Screen.home(w["id"].str))
                    .contextMenu {
                        Button("工作区设置…") { store.editingWorkspace = w }
                    }
                }
                Button {
                    store.editingWorkspace = ["id": "", "name": "", "path": "", "lead": "", "members": []]
                } label: { Label("新建工作区", systemImage: "plus") }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
            if let ws {
                let runs = store.runs(of: ws)
                Section("任务记录 · \(ws["name"].str)") {
                    if runs.isEmpty { Text("还没有任务").foregroundStyle(.secondary).font(.callout) }
                    ForEach(runs, id: \.self) { r in
                        let (label, tone) = L.runListStatus[r["status"].str] ?? (r["status"].str, .gray)
                        HStack(spacing: 8) {
                            Circle().fill(tone.color).frame(width: 7, height: 7)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(L.goalTitle(r["goal"].str)).lineLimit(1)
                                Text(verbatim: "\(label) · \(L.dateTime(r["createdAt"].double ?? 0))").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        .tag(Screen.run(r["id"].str))
                    }
                }
            }
            Section("设置") {
                Label("Agents", systemImage: "cpu").tag(Screen.settings("agents"))
                    .badge(store.connections.count)
                Label("角色", systemImage: "person.2").tag(Screen.settings("roles"))
                    .badge(store.roles.count)
                Label("ACP 适配器", systemImage: "shippingbox").tag(Screen.settings("adapters"))
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Circle().fill(Tone.green.color).frame(width: 6, height: 6)
                    Text(verbatim: "\(store.api.base.host ?? ""):\(String(store.api.base.port ?? 80))")
                        .font(.caption).foregroundStyle(.secondary)
                        .help("Janus 服务地址")
                    Spacer()
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
                        Button(String("\(next.0) →")) {
                            if next.2 == .newWorkspace { store.editingWorkspace = ["id": "", "name": "", "path": "", "lead": "", "members": []] } else { store.open(next.2) }
                        }
                        .buttonStyle(.borderedProminent)
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
                    Text("还没有工作区").font(.title3.weight(.semibold))
                    Text("工作区指定任务在哪个目录执行、由哪些角色参与。").foregroundStyle(.secondary)
                    Button("新建工作区") { store.editingWorkspace = ["id": "", "name": "", "path": "", "lead": "", "members": []] }
                        .buttonStyle(.borderedProminent)
                }
            }
            .padding(24)
            .frame(maxWidth: 860)
        }
    }
}
