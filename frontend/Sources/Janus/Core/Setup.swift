import Foundation

/// Rules for the setup screens, the same as the orchestrator's.
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
        let quoted = v.first == v.last && (v.first == "\"" || v.first == "'")
        return "\(k)=\(needsQuote || quoted ? "\"\(v)\"" : v)"
    }
    static func envText(_ env: JSON) -> String {
        env.object.sorted { $0.key < $1.key }.map { name, value in
            value == .null ? name : envLine(name, value.str)
        }.joined(separator: "\n")
    }

    /// The variable a line sets or keeps: `export KEY=value`, `KEY=value` or a bare `KEY`.
    static func envName(_ line: String) -> String? {
        var l = line.trimmingCharacters(in: .whitespaces)
        if l.isEmpty || l.hasPrefix("#") { return nil }
        if l.hasPrefix("export ") { l = String(l.dropFirst(7)).trimmingCharacters(in: .whitespaces) }
        let name = l.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).first.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        return name.isEmpty ? nil : name
    }

    /// Bring an auth's template into the environment text: its lines for variables the text does
    /// not set yet go after what is there, so a value already typed or pasted is never replaced.
    static func withTemplate(_ text: String, _ template: String) -> String {
        let have = Set(text.components(separatedBy: .newlines).compactMap(envName))
        let add = template.components(separatedBy: .newlines).filter { envName($0).map { !have.contains($0) } ?? false }
        let kept = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return ([kept].filter { !$0.isEmpty } + add).joined(separator: "\n")
    }
}
