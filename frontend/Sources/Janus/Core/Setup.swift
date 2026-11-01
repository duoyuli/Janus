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

    static func value(_ session: JSON, _ ctrl: JSON) -> JSON {
        let v = session[ctrl["id"].str]
        return v.isNull ? ctrl["current"] : v
    }

    /// "Agent · Plan 关闭": a role's permission choices in the agent's own words.
    static func permLabel(_ session: JSON, _ c: JSON?) -> String {
        controls(c).filter { $0["permission"].truthy }.map { x -> String in
            let v = value(session, x)
            if let b = v.bool { return "\(x["name"].str) \(b ? "开启" : "关闭")" }
            return x["options"].array.first { $0["value"] == v }?["name"].str ?? v.str
        }.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// Where the role's instructions reach the agent; same rule as the orchestrator's `instruction_channel`.
    static func instructionsVia(_ c: JSON?) -> String {
        guard let c else { return "绑定 Agent 后，按它支持的方式注入。" }
        switch c["base"].str {
        case "claude": return "作为系统提示追加在 Claude Code 自带的提示之后，整个会话都有效，优先于任务说明。"
        case "codex": return "作为 Codex 的开发者指令注入，整个会话都有效，优先于任务说明。"
        case "pi": return "通过 Janus 的 Pi 扩展加入系统提示，整个会话都有效，优先于任务说明。"
        default: return "\(c["name"].str) 不支持系统提示，指令附在每次任务说明的末尾。"
        }
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

    /// A preset's default variables (fixed, then the chosen endpoint's): where its editable env starts.
    static func endpointEnv(_ k: JSON?, _ ep: String?) -> JSON {
        let eps = k?["endpoints"].array ?? []
        let id = (ep?.isEmpty == false ? ep : nil) ?? eps.first?["id"].str
        return eps.first { $0["id"].str == id }?["env"] ?? [:]
    }
    static func presetEnv(_ k: JSON?, _ ep: String?) -> JSON {
        var out = k?["fixedEnv"] ?? [:]
        if out.isNull { out = [:] }
        for (key, v) in endpointEnv(k, ep).object { out[key] = v }
        return out
    }

    /// Sets KEY=… lines in env text, keeping the person's other lines; a nil value drops the key.
    static func setEnvLines(_ text: String, _ vars: [String: String?]) -> String {
        var left = vars
        var lines: [String] = []
        for l in text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init) {
            let k = l.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: #"^export\s+"#, with: "", options: .regularExpression).split(separator: "=", maxSplits: 1).first.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
            guard let entry = left[k] else { lines.append(l); continue }
            left[k] = nil
            if let v = entry { lines.append(envLine(k, v)) }
        }
        let add = left.sorted { $0.key < $1.key }.compactMap { k, v in v.map { envLine(k, $0) } }
        while lines.last?.trimmingCharacters(in: .whitespaces).isEmpty == true { lines.removeLast() }
        return (lines + add).joined(separator: "\n").replacingOccurrences(of: #"^\n+"#, with: "", options: .regularExpression)
    }
}
