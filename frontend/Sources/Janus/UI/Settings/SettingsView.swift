import SwiftUI

/// The settings page (⌘,): it covers the main window, a pane each for Agents, roles, the Skill and
/// MCP libraries and ACP adapters. Closing it (「完成」 or Esc) returns to the screen
/// underneath, and the last pane opened comes back next time.
// @project-doc docs/architecture/frontend.md#screens
struct SettingsView: View {
    let store: AppStore

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("设置").font(.display(.title2))
                Spacer()
                Button("完成") { store.showingSettings = false }
                    .keyboardShortcut(.cancelAction)
                    .help("关闭设置，回到之前的页面（Esc）")
            }
            .padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 6)
            TabView(selection: Binding(get: { store.settingsTab }, set: { store.settingsTab = $0 })) {
                AgentsView(store: store)
                    .tabItem { Label("Agents", systemImage: "cpu") }
                    .tag("agents")
                RolesView(store: store)
                    .tabItem { Label("角色", systemImage: "person.2") }
                    .tag("roles")
                SkillsView(store: store)
                    .tabItem { Label(store.skillSourceUpdates.isEmpty ? "Skill 库" : "Skill 库（\(store.skillSourceUpdates.count) 个更新）", systemImage: "sparkles") }
                    .tag("skills")
                McpView(store: store)
                    .tabItem { Label(store.mcpSourceUpdates.isEmpty ? "MCP 库" : "MCP 库（\(store.mcpSourceUpdates.count) 个更新）", systemImage: "wrench.and.screwdriver") }
                    .tag("mcp")
                AdaptersView(store: store)
                    .tabItem { Label("ACP 适配器", systemImage: "shippingbox") }
                    .tag("adapters")
            }
            .padding([.horizontal, .bottom], 12)
        }
        .sheet(isPresented: Binding(get: { store.editingRole != nil }, set: { if !$0 { store.editingRole = nil } })) {
            if let r = store.editingRole { RoleEditor(store: store, original: r) }
        }
    }
}
