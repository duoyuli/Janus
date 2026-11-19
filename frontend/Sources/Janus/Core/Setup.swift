import Foundation

/// Rules for the setup screens, the same as the orchestrator's.
enum Setup {
    /// Janus never offers a plan mode (the backend's `is_plan_toggle` / `is_plan_value`); agents
    /// checked before that rule still carry them in their saved options.
    static func isPlanValue(_ v: JSON) -> Bool {
        guard let s = v.string else { return false }
        return s.lowercased().replacingOccurrences(of: #"[-_ ]"#, with: "", options: .regularExpression) == "plan"
    }

    /// Session controls the agent declared, without plan toggles and plan values.
    static func controls(_ c: JSON?) -> [JSON] {
        (c?["options"]["controls"].array ?? []).filter { $0["id"].str.range(of: "plan", options: .caseInsensitive) == nil }.map { x in
            var x = x
            if x["options"].array.contains(where: { isPlanValue($0["value"]) }) { x["options"] = .array(x["options"].array.filter { !isPlanValue($0["value"]) }) }
            return x
        }
    }

    /// Only agents with /goal can lead (same rule as the orchestrator).
    static func leadCapable(_ c: JSON?) -> Bool { ["claude", "codex"].contains(c?["base"].str ?? "") }

    /// The role's value for a control, or the control's `default` (what a session starts with when the role leaves it
    /// open: Codex's Full access and Claude Code's Bypass permissions for their mode, otherwise the agent's own value).
    static func value(_ session: JSON, _ ctrl: JSON) -> JSON {
        let v = session[ctrl["id"].str]
        return v.isNull ? ctrl["default"] : v
    }

    /// "Agent · Plan 关闭": a role's permission choices in the agent's own words.
    static func permLabel(_ session: JSON, _ c: JSON?) -> String {
        controls(c).filter { $0["permission"].truthy }.map { x -> String in
            let v = value(session, x)
            if let b = v.bool { return "\(x["name"].str) \(b ? L.text("开启") : L.text("关闭"))" }
            return x["options"].array.first { $0["value"] == v }?["name"].str ?? v.str
        }.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// Icon for a permission option; whether it widens what the agent may do is the agent's call.
    static func permIcon(_ o: JSON) -> (String, Bool) {
        let v = "\(o["value"].str) \(o["name"].str)"
        func m(_ p: String) -> Bool { v.range(of: p, options: [.regularExpression, .caseInsensitive]) != nil }
        if m(#"read[-_ ]?only"#) { return ("eye", false) }
        if m("full|bypass|always|yolo|danger|dontask") { return ("exclamationmark.shield", true) }
        if m("edit") { return ("pencil", false) }
        return ("checkmark.shield", false)
    }

    // MARK: Agent env text ("KEY=value" lines)

    static func envLine(_ k: String, _ v: String) -> String {
        let needsQuote = v.range(of: #"\s|#"#, options: .regularExpression) != nil
        return "\(k)=\(needsQuote ? String(decoding: JSON(v).data(), as: UTF8.self) : v)"
    }
    static func envText(_ env: JSON) -> String { env.object.sorted { $0.key < $1.key }.map { envLine($0.key, $0.value.str) }.joined(separator: "\n") }
}
