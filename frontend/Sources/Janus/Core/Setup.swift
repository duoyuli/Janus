import Foundation

/// What the setup editors bind to; the lines they show are the service's (`setup_view.rs`).
enum Setup {
    /// Session controls the agent declared. The server leaves plan toggles and plan values out
    /// when it reads them (`session_options`): Janus never runs an agent in a plan mode.
    static func controls(_ c: JSON?) -> [JSON] { c?["options"]["controls"].array ?? [] }

    /// A role can lead once it is bound to an agent: Codex and Claude Code both pursue goals with /goal.
    static func leadCapable(_ c: JSON?) -> Bool { c != nil }

    /// The role's value for a control, or the control's `default` (what a session starts with when the role leaves it
    /// open: Codex's Full access and Claude Code's Bypass permissions for their mode, otherwise the agent's own value).
    static func value(_ session: JSON, _ ctrl: JSON) -> JSON {
        let v = session[ctrl["id"].str]
        return v.isNull ? ctrl["default"] : v
    }
}
