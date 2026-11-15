import SwiftUI

/// The settings panel (⌘,): a floating card in the middle of the window, a pane each for Agents,
/// roles, the Skill and MCP libraries and ACP adapters. Clicking the dimmed area or pressing Esc
/// closes it and shows the screen underneath unchanged; the last pane opened comes back next time.
/// The bottom of the sidebar picks the app's appearance.
// @project-doc docs/architecture/frontend.md#screens
struct SettingsView: View {
    let store: AppStore
    @Local private var appearance = Appearance.saved

    var body: some View {
        ZStack {
            // The dimmed area is the way back: a click anywhere outside the panel closes it.
            Color.black.opacity(0.3)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture { store.showingSettings = false }
            panel
                .frame(maxWidth: 1080, maxHeight: 720)
                .padding(28)
        }
        .onExitCommand { store.showingSettings = false }
        .sheet(isPresented: Binding(get: { store.editingRole != nil }, set: { if !$0 { store.editingRole = nil } })) {
            if let r = store.editingRole { RoleEditor(store: store, original: r) }
        }
    }

    private var panel: some View {
        HStack(spacing: 0) {
            sidebar
            Rectangle().fill(Color.line).frame(width: 1)
            pane
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(Color.page)
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color.line))
        .shadow(color: .black.opacity(0.22), radius: 30, y: 12)
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("设置")
                .font(.display(.title2))
                .padding(.horizontal, 16).padding(.top, 18).padding(.bottom, 10)
            VStack(spacing: 2) {
                nav("agents", "Agents", "cpu")
                nav("roles", "角色", "person.2")
                nav("skills", "Skill 库", "sparkles", updates: store.skillSourceUpdates.count)
                nav("mcp", "MCP 库", "wrench.and.screwdriver", updates: store.mcpSourceUpdates.count)
                nav("adapters", "ACP 适配器", "shippingbox")
            }
            .padding(.horizontal, 8)
            Spacer(minLength: 16)
            VStack(alignment: .leading, spacing: 6) {
                Text("外观").font(.caption).foregroundStyle(.secondary)
                Picker("外观", selection: $appearance) {
                    ForEach(Appearance.allCases) { a in Text(a.title).tag(a) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .onChange(of: appearance) { _, a in a.apply() }
                .help("系统、浅色或深色")
            }
            .padding(12)
        }
        .frame(width: 196)
        .background(Color.sidebar)
    }

    private func nav(_ id: String, _ title: String, _ icon: String, updates: Int = 0) -> some View {
        let selected = store.settingsTab == id
        return Button { store.settingsTab = id } label: {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .frame(width: 18)
                    .foregroundStyle(selected ? Color.accent : Color.secondary)
                Text(title)
                Spacer(minLength: 4)
                if updates > 0 { Pill("\(updates)", .amber) }
            }
            .font(.callout)
            .foregroundStyle(Color.primary)
            .padding(.horizontal, 8).padding(.vertical, 6)
            .background(selected ? Color.accent.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 7))
            .contentShape(RoundedRectangle(cornerRadius: 7))
        }
        .buttonStyle(.plain)
        .hoverable(7)
        .help(updates > 0 ? "\(updates) 个来源有更新" : "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    @ViewBuilder private var pane: some View {
        switch store.settingsTab {
        case "roles": RolesView(store: store)
        case "skills": SkillsView(store: store)
        case "mcp": McpView(store: store)
        case "adapters": AdaptersView(store: store)
        default: AgentsView(store: store)
        }
    }
}
