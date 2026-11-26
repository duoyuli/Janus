import AppKit
import Foundation

/// Digital employees by group: how the lists order them, and renaming, binding and exporting a
/// whole group at once.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_groups
extension AppStore {
    /// The groups digital employees are listed under: named ones in order, then "" (no group).
    var roleGroups: [String] {
        let named = Set(roles.map(\.group)).subtracting([""]).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        return roles.contains { $0.group.isEmpty } ? named + [""] : named
    }

    /// The digital employees group by group, in `roleGroups` order: how Settings › 数字员工, the
    /// new-chat page and the workspace editor list them.
    var roleSections: [(group: String, roles: [Role])] {
        roleGroups.map { g in (g, roles.filter { $0.group == g }) }
    }

    /// A group's name as the lists show it.
    static func groupTitle(_ group: String) -> String { group.isEmpty ? L.text("未分组") : group }

    func renameRoleGroup(_ from: String, to: String) async {
        do {
            _ = try await api.post("/api/role-groups/rename", ["from": JSON(from), "to": JSON(to)])
            await loadSetup()
        } catch { show(error.localizedDescription) }
    }

    /// Put a whole group on one agent; each takes the agent's default model unless it was on it already.
    func bindRoleGroup(_ group: String, connection: String) async {
        do {
            let bound = try await api.post("/api/role-groups/bind", ["group": JSON(group), "connection": JSON(connection)])
            await loadSetup()
            show(L.text("\(Self.groupTitle(group)) 的 \(bound.array.count) 位数字员工改用 \(self.connection(connection)?["name"].str ?? connection)"))
        } catch { show(error.localizedDescription) }
    }

    /// Write a group's digital employees as role files (`<id>.md`, and `<id>.png` for a picture)
    /// into a folder named after the group, inside a folder the person picks.
    func exportRoleGroup(_ group: String) async {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.canCreateDirectories = true
        p.prompt = L.text("导出到这里")
        p.message = L.text("会在这里新建文件夹「\(group)」，每位数字员工一个 .md 文件")
        guard p.runModal() == .OK, let base = p.url else { return }
        let dir = base.appendingPathComponent(group, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let members = roles.filter { $0.group == group }
            for r in members {
                let text = try await api.text("/api/roles/\(API.q(r.id))/file")
                try text.write(to: dir.appendingPathComponent("\(r.id).md"), atomically: true, encoding: .utf8)
                if !r.avatar.isEmpty, let png = try? await api.data("/api/roles/\(API.q(r.id))/avatar") {
                    try png.write(to: dir.appendingPathComponent("\(r.id).png"))
                }
            }
            show(L.text("已导出 \(members.count) 位数字员工到 \(L.tilde(dir.path))"))
            NSWorkspace.shared.activateFileViewerSelecting([dir])
        } catch { show(L.text("导出失败：\(error.localizedDescription)")) }
    }
}
