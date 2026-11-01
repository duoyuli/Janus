import SwiftUI

/// The settings window (⌘,): a pane each for Agents, roles and ACP adapters. The window title
/// follows the pane, and the last pane opened comes back.
// @project-doc docs/architecture/frontend.md#screens
struct SettingsView: View {
    let model: AppModel

    var body: some View {
        Group {
            if let store = model.store {
                TabView(selection: Binding(get: { store.settingsTab }, set: { store.settingsTab = $0 })) {
                    AgentsView(store: store)
                        .tabItem { Label("Agents", systemImage: "cpu") }
                        .tag("agents")
                    RolesView(store: store)
                        .tabItem { Label("角色", systemImage: "person.2") }
                        .tag("roles")
                    AdaptersView(store: store)
                        .tabItem { Label("ACP 适配器", systemImage: "shippingbox") }
                        .tag("adapters")
                }
                .sheet(isPresented: Binding(get: { store.editingRole != nil }, set: { if !$0 { store.editingRole = nil } })) {
                    if let r = store.editingRole { RoleEditor(store: store, original: r) }
                }
            } else {
                Text("Janus 服务还没有就绪，启动后再打开设置。")
                    .foregroundStyle(.secondary)
                    .padding(40)
            }
        }
        .frame(minWidth: 780, idealWidth: 880, minHeight: 520, idealHeight: 640)
    }
}
