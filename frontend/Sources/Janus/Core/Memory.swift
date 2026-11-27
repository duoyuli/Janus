import Foundation

/// A workspace's team memory: the notes its runs' agents keep for the runs after them, and their
/// consolidation (`/api/workspaces/{id}/memories`). Read when the workspace's page shows, and
/// again when the global stream says that workspace's memory changed.
// @project-doc docs/interfaces/20261031-http_api.md#memories
extension AppStore {
    func loadMemories(_ workspace: String) async {
        guard !workspace.isEmpty else { return }
        if let r = try? await api.get("/api/workspaces/\(API.q(workspace))/memories?lang=\(L.serverLanguage)"), memories[workspace] != r { memories[workspace] = r }
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

/// What the app says about team memory, as the server words it (`GET /api/workspaces/{id}/memories`).
enum MemoryText {
    /// Who wrote a note last and where (`source`), and when.
    static func source(_ m: JSON) -> String { "\(m["source"].str) · \(L.dateTime(m["updatedAt"].double ?? 0))" }
}
