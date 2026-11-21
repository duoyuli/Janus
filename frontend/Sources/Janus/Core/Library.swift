import Foundation

/// How the resource library's items read in the app: their origin, where an MCP server is, and
/// the state of its values.
// @project-doc docs/domains/20261116-resource_library.md#model
enum Library {
    /// Where an item came from, in a few words.
    static func origin(_ item: JSON) -> String {
        let o = item["origin"]
        switch o["type"].str {
        case "folder": return L.text("复制自 \(o["path"].str)")
        case "git":
            let url = item["source"]["url"].str
            let from = url.replacingOccurrences(of: "https://github.com/", with: "")
            return item["source"]["inSource"].truthy ? L.text("来自 \(from)") : L.text("来自 \(from) · 来源中已删除")
        default: return item["kind"].str == "skill" ? L.text("在 Janus 中新建") : L.text("手动添加")
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
        let what = item["server"]["transport"].str == "stdio" ? L.text("变量") : "Header"
        return (L.text("\(what) \(set)/\(vars.count) 已设置"), set == vars.count)
    }

    /// Skills grouped by where they came from: each GitHub source (in the order added, also before
    /// anything is imported from it), then each local folder they were copied out of, then the ones
    /// written in Janus.
    static func skillGroups(_ skills: [JSON], sources: [JSON]) -> [SkillGroup] {
        var groups = sources.map { SkillGroup(origin: .source($0["id"].str), items: []) }
        var folders: [String: [JSON]] = [:]
        var created: [JSON] = []
        for s in skills {
            let o = s["origin"]
            switch o["type"].str {
            case "git":
                let id = o["source"].str
                if let at = groups.firstIndex(where: { $0.origin == .source(id) }) { groups[at].items.append(s) }
                else { groups.append(SkillGroup(origin: .source(id), items: [s])) }
            case "folder": folders[(o["path"].str as NSString).deletingLastPathComponent, default: []].append(s)
            default: created.append(s)
            }
        }
        groups += folders.keys.sorted().map { SkillGroup(origin: .folder($0), items: folders[$0] ?? []) }
        if !created.isEmpty { groups.append(SkillGroup(origin: .created, items: created)) }
        return groups
    }

    /// The skills in the order the router's catalog lists them: the most read first, then by name.
    // @project-doc docs/domains/20261116-resource_library.md#skill_router
    static func catalogOrder(_ skills: [JSON]) -> [JSON] {
        skills.sorted {
            let (a, b) = ($0["uses"]["sessions"].int ?? 0, $1["uses"]["sessions"].int ?? 0)
            return a != b ? a > b : $0["name"].str < $1["name"].str
        }
    }

    /// Whether the router's catalog is too large to list every skill, so the agent searches it
    /// by keyword (`skillRouter.catalog` of `GET /api/library`).
    static func searched(_ library: JSON?) -> Bool { library?["skillRouter"]["catalog"].str == "search" }

    /// How much a skill has been used: "被 3 个会话读过 · 最近一次：2 小时前", or that none has yet.
    // @project-doc docs/domains/20261116-resource_library.md#skill_uses
    static func usage(_ item: JSON, now: Double) -> String {
        let n = item["uses"]["sessions"].int ?? 0
        guard n > 0 else { return L.text("还没有会话读过") }
        let last = item["uses"]["lastAt"].double.map { " · " + L.text("最近一次：\(L.ago($0, now))") } ?? ""
        return L.text("被 \(n) 个会话读过") + last
    }

    /// One hint on how findable a skill is (`hints` of a skill in `GET /api/library`), in words.
    // @project-doc docs/domains/20261116-resource_library.md#skill_hints
    static func hint(_ h: JSON) -> String {
        switch h["kind"].str {
        case "short": return L.text("描述太短：Agent 只凭描述判断什么时候用它，写清它做什么、什么时候用")
        case "noWhen": return L.text("描述没写什么时候用：加一句「在……时使用」，或写在 when_to_use 字段里")
        case "long": return L.text("描述有 \(h["chars"].int ?? 0) 个字，目录里只保留前 \(h["kept"].int ?? 0) 个：把关键用途和触发词写在开头")
        case "alike": return L.text("和 \(h["skill"].str) 的描述很像，Agent 可能分不清用哪个：写出各自适用的场合")
        default: return ""
        }
    }

    /// A source's name: `owner/repo`, with the folder and branch the link names.
    static func sourceName(_ source: JSON) -> String {
        let path = source["path"].str, ref = source["gitRef"].str
        return source["repo"].str + (path.isEmpty ? "" : "/\(path)") + (ref.isEmpty ? "" : " @\(ref)")
    }
}

/// Skills that came from the same place, folded together on the library's Skill tab.
struct SkillGroup: Identifiable {
    enum Origin: Hashable {
        /// A GitHub source, by its id.
        case source(String)
        /// A local folder the skills were copied out of.
        case folder(String)
        /// Written in Janus.
        case created
    }

    let origin: Origin
    var items: [JSON]

    var id: Origin { origin }
}
