import Foundation

/// How the resource library's items read in the app: their origin, the state of an MCP server's
/// values, and whether a role's agent can use them. The rules are the server's (`support` on each
/// item); this only words them.
// @project-doc docs/domains/resource_library.md#roles
enum Library {
    /// The runtime a connection runs on: `claude` (presets too), `codex` or `custom`.
    static func runtime(_ connection: JSON?) -> String { connection?["base"].str ?? "" }

    /// Whether `runtime` can use the item: `yes`, `no` or `depends` (on what the agent declares),
    /// with the reason when it isn't a plain yes. Unknown runtimes (no agent chosen) say nothing.
    static func support(_ item: JSON, runtime: String) -> (state: String, why: String) {
        guard let s = item["support"].array.first(where: { $0["runtime"].str == runtime }) else { return ("yes", "") }
        return (s["support"]["state"].str, s["support"]["why"].str)
    }

    /// Where an item came from, in a few words.
    static func origin(_ item: JSON) -> String {
        let o = item["origin"]
        switch o["type"].str {
        case "folder": return "复制自 \(o["path"].str)"
        case "git":
            let url = item["source"]["url"].str
            let from = url.replacingOccurrences(of: "https://github.com/", with: "")
            return item["source"]["inSource"].truthy ? "来自 \(from)" : "来自 \(from) · 来源中已删除"
        default: return item["kind"].str == "skill" ? "在 Janus 中新建" : "手动添加"
        }
    }

    /// "stdio · npx -y gh-mcp", "HTTP · https://…", "SSE · https://…".
    static func endpoint(_ item: JSON) -> String {
        let s = item["server"]
        switch s["transport"].str {
        case "http": return "HTTP · \(s["url"].str)"
        case "sse": return "SSE · \(s["url"].str)"
        default: return "stdio · \(([s["command"].str] + s["args"].strings).joined(separator: " "))"
        }
    }

    /// How many of a server's variables have values, e.g. "变量 1/2 已设置"; empty when it has none.
    static func vars(_ item: JSON) -> (text: String, complete: Bool) {
        let vars = item["vars"].array
        guard !vars.isEmpty else { return ("", true) }
        let set = vars.filter { $0["set"].truthy }.count
        let what = item["server"]["transport"].str == "stdio" ? "变量" : "Header"
        return ("\(what) \(set)/\(vars.count) 已设置", set == vars.count)
    }

    /// The roles using an item, for its row.
    static func usedBy(_ item: JSON) -> String {
        let names = item["roles"].array.map { $0["name"].str }
        return names.isEmpty ? "还没有数字员工使用" : "\(names.joined(separator: "、")) 在用"
    }
}
