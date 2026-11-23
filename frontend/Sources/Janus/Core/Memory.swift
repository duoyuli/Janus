import Foundation

/// A workspace's team memory: the notes its runs' agents keep for the runs after them, and their
/// consolidation (`/api/workspaces/{id}/memories`). Read when the workspace's page shows, and
/// again when the global stream says that workspace's memory changed.
// @project-doc docs/interfaces/20261031-http_api.md#memories
extension AppStore {
    func loadMemories(_ workspace: String) async {
        guard !workspace.isEmpty else { return }
        if let r = try? await api.get("/api/workspaces/\(API.q(workspace))/memories"), memories[workspace] != r { memories[workspace] = r }
    }

    func notes(of workspace: String) -> [JSON] { memories[workspace]?["memories"].array ?? [] }

    /// The person rewrites a note; `false` when the server refused.
    func saveMemory(_ note: JSON, _ body: JSON) async -> Bool {
        do {
            _ = try await api.put("/api/memories/\(API.q(note["id"].str))", body)
            await loadMemories(note["workspace"].str)
            return true
        } catch {
            show(L.text("保存笔记失败：\(error.localizedDescription)"))
            return false
        }
    }

    func deleteMemory(_ note: JSON) async -> Bool {
        guard confirm(L.text("删除笔记「\(note["title"].str)」？"), L.text("之后的任务不会再读到它。"), ok: L.text("删除")) else { return false }
        do {
            try await api.delete("/api/memories/\(API.q(note["id"].str))")
            await loadMemories(note["workspace"].str)
            return true
        } catch {
            show(error.localizedDescription)
            return false
        }
    }

    /// Consolidate a workspace's notes now, and open the consolidation.
    func consolidate(_ workspace: String) async {
        do {
            let r = try await api.post("/api/workspaces/\(API.q(workspace))/memories/consolidate", [:])
            await loadMemories(workspace)
            await loadRuns()
            open(.run(r["run"].str))
        } catch { show(error.localizedDescription) }
    }
}

/// What the app says about team memory.
enum MemoryText {
    static let kinds = ["fact", "decision", "pitfall", "howto"]

    static func kind(_ k: String) -> String {
        switch k {
        case "decision": return L.text("决定")
        case "pitfall": return L.text("坑")
        case "howto": return L.text("做法")
        default: return L.text("事实")
        }
    }

    /// Who wrote a note last and where: a member in a run, or the person.
    static func source(_ m: JSON) -> String {
        let day = L.dateTime(m["updatedAt"].double ?? 0)
        if m["who"].str == "user" { return L.text("你 · \(day)") }
        if m["runStatus"].str == "cancelled" { return L.text("\(m["author"].str) · 来自已取消的任务 · \(day)") }
        let goal = m["runGoal"].string.map { L.goalTitle($0) } ?? L.text("已删除的任务")
        return L.text("\(m["author"].str) · \(goal) · \(day)")
    }

    /// The line a note adds to the shared material.
    static func briefLine(_ m: JSON) -> String {
        let body = m["body"].str.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }.joined(separator: " ")
        return "\(m["title"].str)：\(body)"
    }
}
