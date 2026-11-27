import Foundation

/// The resource library as the service lists it: its skills by group and in the catalog's order.
/// What the screens say about each item and source is the service's (`setup_view::library`).
// @project-doc docs/domains/20261116-resource_library.md#model
enum Library {
    /// Skills grouped by where they came from, as the server folds them (`skillGroups` of
    /// `GET /api/library`): each GitHub source, then each local folder, then the ones written in
    /// Janus. A group names its skills by id; `skills` gives them.
    static func skillGroups(_ library: JSON?, skills: [JSON]) -> [SkillGroup] {
        (library?["skillGroups"].array ?? []).map { g in
            let origin: SkillGroup.Origin = switch g["type"].str {
            case "source": .source(g["source"].str)
            case "folder": .folder(g["path"].str)
            default: .created
            }
            let items = g["items"].strings.compactMap { id in skills.first { $0["id"].str == id } }
            return SkillGroup(origin: origin, items: items)
        }
    }

    /// The skills in the order the router's catalog lists them, as the server says
    /// (`skillRouter.order`: the most read first, then by name).
    // @project-doc docs/domains/20261116-resource_library.md#skill_router
    static func catalogOrder(_ library: JSON?, skills: [JSON]) -> [JSON] {
        (library?["skillRouter"]["order"].strings ?? []).compactMap { id in skills.first { $0["id"].str == id } }
    }

    /// Whether the router's catalog is too large to list every skill, so the agent searches it
    /// by keyword (`skillRouter.catalog` of `GET /api/library`).
    static func searched(_ library: JSON?) -> Bool { library?["skillRouter"]["catalog"].str == "search" }

    /// How much a skill has been used: the service's words (`usageText`), and when it was last
    /// read, e.g. "被 3 个会话读过 · 最近一次：2 小时前".
    // @project-doc docs/domains/20261116-resource_library.md#skill_uses
    static func usage(_ item: JSON, now: Double) -> String {
        let last = item["uses"]["lastAt"].double.map { " · " + L.text("最近一次：\(L.ago($0, now))") } ?? ""
        return item["usageText"].str + last
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
