import Foundation

/// What an Agent card's runtime line shows, worked out from `GET /api/adapters` alone: the other
/// installed versions, whether the version menu is there, the state word, and what the menu holds.
/// The view only lays these out (docs/architecture/20261031-adapters.md#api_shape).
enum AdapterText {
    /// How one adapter's line reads after the version.
    enum State: Equatable {
        /// Installed, up to date, nothing to say.
        case idle
        /// An update installs in the background.
        case updating
        /// The first install, or the person's install, runs in the background.
        case installing
        /// An automatic update just finished: shown for a few minutes, then gone.
        case autoUpdated
        /// An update or an install over an installed version failed.
        case failed
        /// Nothing is installed yet.
        case notInstalled
        /// The first install (or a retry) failed; nothing is installed.
        case installFailed
    }

    /// One entry of the version menu.
    enum MenuItem: Equatable, Hashable {
        /// Resume automatic updates, naming the newest version when one is known.
        case resume(String)
        /// Switch to an installed version.
        case switchTo(String)
    }

    /// How long "已自动更新" stays after the automatic job ended.
    static let autoUpdatedFor: Double = 10 * 60 * 1000

    static func running(_ a: JSON) -> Bool { a["job"]["state"].str == "running" }

    /// The installed versions other than the current one, the version installed last first.
    static func others(_ a: JSON) -> [String] {
        a["versions"].array
            .filter { !$0["current"].truthy && $0["version"].truthy }
            .sorted { ($0["installedAt"].double ?? 0) > ($1["installedAt"].double ?? 0) }
            .map { $0["version"].str }
    }

    /// Whether the line has a version menu: another version to switch to, or a pin to undo.
    static func showsMenu(_ a: JSON) -> Bool { a["pinned"].truthy || !others(a).isEmpty }

    /// The state after the version. `seen` holds `autoUpdatedKey`s the person has already left the
    /// settings page on, so coming back does not show the word again.
    static func state(_ a: JSON, now: Double, seen: Set<String> = []) -> State {
        if running(a) { return a["current"].truthy ? .updating : .installing }
        if !a["current"].truthy { return a["job"]["state"].str == "error" ? .installFailed : .notInstalled }
        if a["job"]["state"].str == "error" { return .failed }
        if a["job"]["action"].str == "auto", a["job"]["state"].str == "ok",
           now - (a["job"]["ended"].double ?? 0) < autoUpdatedFor, !seen.contains(autoUpdatedKey(a)) {
            return .autoUpdated
        }
        return .idle
    }

    /// How a finished automatic update is recognized, so it is shown once and only while it is
    /// fresh.
    static func autoUpdatedKey(_ a: JSON) -> String { "\(a["id"].str)@\(a["job"]["ended"].int ?? 0)" }

    /// When "已自动更新" stops showing, if an automatic update just finished: the view waits until
    /// then so the word goes even while the page is left alone.
    static func autoUpdatedUntil(_ a: JSON) -> Double? {
        guard a["job"]["action"].str == "auto", a["job"]["state"].str == "ok" else { return nil }
        return (a["job"]["ended"].double ?? 0) + autoUpdatedFor
    }

    /// What the version menu holds: resuming automatic updates first while the version is pinned,
    /// then every other installed version.
    static func menuItems(_ a: JSON) -> [MenuItem] {
        var items: [MenuItem] = []
        if a["pinned"].truthy { items.append(.resume(a["latest"].str)) }
        items += others(a).map(MenuItem.switchTo)
        return items
    }

    /// Janus's own notes about the last install, for the failure popover's 「最近日志」: the log's
    /// `== <time> <note>` lines without the time, newest last. The tool's command line and output
    /// stay in the log for 「查看日志」 only — the popover never shows them, and a line naming the
    /// package or `node_modules` (an old log written before the notes dropped them) is skipped
    /// too, so the interface keeps ACP, adapter and npm words out (P1 AC-15).
    static func recentNotes(_ a: JSON, limit: Int = 4) -> [String] {
        let hidden = ["npm", "@agentclientprotocol/", "node_modules"]
        return a["log"].str.split(separator: "\n").compactMap { line -> String? in
            let s = line.trimmingCharacters(in: .whitespaces)
            guard s.hasPrefix("== "), let space = s.dropFirst(3).firstIndex(of: " ") else { return nil }
            let note = String(s[s.index(space, offsetBy: 1)...]).trimmingCharacters(in: .whitespaces)
            guard !note.isEmpty, !hidden.contains(where: { note.lowercased().contains($0) }) else { return nil }
            return note
        }.suffix(limit).map { $0 }
    }
}
