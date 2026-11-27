import Foundation

/// An Agent card's runtime line as the service words it (`line` on each adapter of
/// `GET /api/adapters?lang=`, `backend/src/setup_view/adapters.rs`): the state, the word after the
/// version, the version menu, the notes for a failure. The one thing the app adds is the person's
/// own: an automatic update they already saw on the settings page is not announced again
/// (docs/architecture/20261031-adapters.md#api_shape).
enum AdapterText {
    /// The line's state (`idle`, `updating`, `installing`, `autoUpdated`, `failed`, `notInstalled`,
    /// `installFailed`), with an automatic update the person already saw read as `idle`.
    static func state(_ a: JSON, seen: Set<String>) -> String {
        let line = a["line"]
        if line["state"].str == "autoUpdated", seen.contains(line["autoUpdated"]["key"].str) { return "idle" }
        return line["state"].str
    }

    /// The word after the version, if this state has one.
    static func word(_ a: JSON, seen: Set<String>) -> String? {
        state(a, seen: seen) == "idle" ? nil : a["line"]["word"].string
    }
}
