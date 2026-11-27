import Foundation

/// Where things are in Janus home, as the app needs them. The names are the service's `Layout`
/// (backend/src/layout.rs); nothing else in the app joins names onto the home. The layout and how
/// to place something new: docs/operations/20261031-deployment.md#layout.
struct JanusHome: Equatable, Sendable {
    let url: URL

    /// `JANUS_HOME`, else `~/.janus`.
    static func current(_ env: [String: String] = ProcessInfo.processInfo.environment) -> JanusHome {
        let url = env["JANUS_HOME"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".janus")
        return JanusHome(url: url)
    }

    var path: String { url.path }
    private func at(_ rel: String) -> URL { url.appendingPathComponent(rel) }

    var config: URL { at("config.toml") }
    /// The single-instance lock; it holds the running service's pid.
    var pidFile: URL { at("janus.pid") }
    /// The service `just install` puts here.
    var server: URL { at("bin/janus") }

    var logs: URL { at("logs") }
    /// The service's own log.
    var janusLog: URL { at("logs/janus.log") }
    /// Output of a service the app (or launchd) started.
    var serviceLog: URL { at("logs/janus.out.log") }
    /// What replacing the app during an update did.
    var updateLog: URL { at("logs/update.log") }

    /// In-app updates: `<version>/` being downloaded and `previous/`. A cache: only those two stay.
    var updates: URL { at("cache/updates") }
    /// The app's own record of the macOS permission guidance.
    var permissions: URL { at("data/app/permissions.json") }
}
