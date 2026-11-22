import Foundation

/// What of the person's home folder agents and MCP servers do not see (`GET/PUT /api/host-env`):
/// their HOME mirrors the person's but for a list of hidden paths, and a command can be tried both
/// ways (`POST /api/host-env/try`).
// @project-doc docs/architecture/20261031-frontend.md#host_env
enum HostEnv {
    /// The hidden list with `path` added (`hide`) or taken out.
    static func with(_ hidden: [String], _ path: String, hide: Bool) -> [String] {
        hide ? (hidden.contains(path) ? hidden : hidden + [path]) : hidden.filter { $0 != path }
    }

    /// How a hidden path is labelled: an agent's own settings, something sensitive, or the person's.
    static func why(_ entry: JSON) -> String {
        switch entry["why"].str {
        case "agent": return L.text("Agent 的配置")
        case "sensitive": return L.text("敏感")
        default: return L.text("你加的")
        }
    }
}

extension AppStore {
    /// Read the hidden list; the reason when it could not be read.
    func loadHostEnv() async -> String? {
        do {
            hostEnv = try await api.get("/api/host-env")
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// Hide exactly `paths` from now on (`nil`: back to the defaults); the server refuses a path it
    /// cannot take.
    @discardableResult
    func hideFromAgents(_ paths: [String]?) async -> Bool {
        do {
            hostEnv = try await api.put("/api/host-env", ["paths": paths.map { JSON.array($0.map(JSON.init)) } ?? .null])
            return true
        } catch {
            show(error.localizedDescription)
            return false
        }
    }

    /// The paths hidden now, as the list stands.
    var hostHidden: [String] { hostEnv?["hidden"].array.map(\.["path"].str) ?? [] }

    /// Run `command` as the person and as an agent would; the two results, or nil when it could not run.
    func tryOnHost(_ command: String) async -> JSON? {
        do {
            hostTry = try await api.post("/api/host-env/try", ["command": JSON(command)], timeout: 60)
            return hostTry
        } catch {
            show(error.localizedDescription)
            return nil
        }
    }
}
