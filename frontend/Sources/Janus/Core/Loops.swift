import Foundation

/// Workspace Loops: reading them, making and changing them, waking, pausing and deleting them
/// (`/api/loops`). The Loop's session is a run of kind `loop`, opened like any run.
// @project-doc docs/interfaces/20261031-http_api.md#loops
extension AppStore {
    static func blankLoop(_ workspace: String) -> JSON {
        ["id": "", "workspace": JSON(workspace), "name": "", "prompt": "", "rhythm": ["kind": "interval", "minutes": 30]]
    }

    func loadLoops() async {
        if let r = try? await api.get("/api/loops"), r["loops"].array != loops { loops = r["loops"].array }
    }

    /// Read one Loop again (the global stream named it); gone from the server, it goes here too.
    func loadLoop(_ id: String) async {
        guard !id.isEmpty else { return }
        do {
            let l = try await api.get("/api/loops/\(API.q(id))")
            if let i = loops.firstIndex(where: { $0["id"].str == id }) {
                if loops[i] != l { loops[i] = l }
            } else {
                loops.append(l)
            }
        } catch let e as APIError where e.status == 404 {
            loops.removeAll { $0["id"].str == id }
        } catch {}
    }

    func loops(of workspace: String) -> [JSON] { loops.filter { $0["workspace"].str == workspace } }

    /// The Loop whose session is run `id`.
    func loop(ofRun id: String) -> JSON? { loops.first { $0["run"].str == id } }

    /// Make a Loop (`original` without an id) or change one; `false` when the server refused.
    func saveLoop(_ original: JSON, _ body: JSON) async -> Bool {
        do {
            if original["id"].str.isEmpty {
                let made = try await api.post("/api/workspaces/\(API.q(original["workspace"].str))/loops", body)
                await loadLoops()
                await loadRuns()
                open(.run(made["run"].str))
            } else {
                _ = try await api.put("/api/loops/\(API.q(original["id"].str))", body)
                await loadLoops()
            }
            return true
        } catch {
            show(L.text("保存值班失败：\(error.localizedDescription)"))
            return false
        }
    }

    /// Wake, pause or resume a Loop.
    func loopAction(_ loop: JSON, _ action: String) async {
        do {
            _ = try await api.post("/api/loops/\(API.q(loop["id"].str))/\(action)", [:])
            await loadLoops()
        } catch { show(error.localizedDescription) }
    }

    /// Delete a Loop with its session; the runs it dispatched stay.
    func deleteLoop(_ loop: JSON) async {
        guard confirm(L.text("删除值班「\(loop["title"].str)」？"), L.text("它的会话和记录一起删除，不可恢复；它派出的任务保留。"), ok: L.text("删除")) else { return }
        do {
            try await api.delete("/api/loops/\(API.q(loop["id"].str))")
            if run?.id == loop["run"].str { open(.home(loop["workspace"].str)) }
            await loadLoops()
            await loadRuns()
        } catch { show(error.localizedDescription) }
    }

    /// A page of the runs a Loop dispatched, newest first, and where the next page starts.
    func dispatched(by loop: String, before: String? = nil) async -> (runs: [JSON], next: String?) {
        guard !loop.isEmpty else { return ([], nil) }
        let cursor = before.map { "&cursor=\(API.q($0))" } ?? ""
        let r = try? await api.get("/api/runs?loop=\(API.q(loop))&limit=100\(cursor)")
        return (r?["runs"].array ?? [], r?["next"].string)
    }
}
