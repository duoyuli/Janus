import Foundation

// Setup objects as the server stores them, read once from its JSON. Views read their fields by
// name here instead of by key, so a renamed field is a compile error, not an empty string. The
// editors work on a JSON draft (`json`) and send it back as the server reads it.

/// A digital employee (`backend/src/agents/roles.rs`, `Role`).
struct Role: Hashable, Identifiable {
    let id: String
    let name: String
    /// What it is responsible for.
    let desc: String
    /// The group it is listed under; empty for none.
    let group: String
    /// Its standing instructions.
    let prompt: String
    /// The agent connection it runs on; empty until one is bound.
    let connection: String
    let model: String
    let effort: String
    /// The session controls it chose, by control id.
    let session: JSON
    /// Whether it may lead a workspace.
    let lead: Bool
    let color: String
    let initials: String
    /// The hash of its picture; empty shows the initials.
    let avatar: String
    /// Whether it is one of Janus's default roles.
    let isDefault: Bool
    /// The newer default text of the fields of a default role the person edited, by field.
    let seedUpdates: [String: String]
    /// The record as the server sent it, the editor's starting draft.
    let json: JSON
    /// Why it cannot run now (no Agent, or its Agent not checked), as the server says; nil when it can.
    var unavailable: String? { json["unavailable"].string }

    init(_ j: JSON) {
        json = j
        id = j["id"].str
        name = j["name"].str
        desc = j["desc"].str
        group = j["group"].str
        prompt = j["prompt"].str
        connection = j["connection"].str
        model = j["model"].str
        effort = j["effort"].str
        session = j["session"]
        lead = j["lead"].truthy
        color = j["color"].str
        initials = j["init"].str
        avatar = j["avatar"].str
        isDefault = !j["seed"].isNull
        seedUpdates = j["seedUpdates"].object.mapValues(\.str)
    }

    /// What its tile shows without a picture, as the service writes it (`displayInit`).
    var displayInitials: String { json["displayInit"].string ?? Role.initials(initials, name: name) }
    /// What it does, its permission choices and whether it has instructions, in one line.
    var line: String { json["line"].str }
    /// The model it runs, and its effort; nil until it is bound to an Agent.
    var modelLine: String? { json["modelLine"].string }

    /// A digital employee's initials, or (none given) the first two letters of its name, without
    /// spaces and in capitals. Every avatar of a role, a member or a proposed employee uses it.
    static func initials(_ given: String, name: String) -> String {
        given.isEmpty ? String(name.replacingOccurrences(of: " ", with: "").prefix(2)).uppercased() : given
    }

    /// How many of one digital employee a workspace's team may run at once
    /// (`backend/src/agents/workspaces.rs`, `MAX_SLOTS`).
    static let maxSlots = 8
}

/// A workspace: a folder, its shared material and its team (`backend/src/agents/workspaces.rs`).
struct Workspace: Hashable, Identifiable {
    /// A role in the team, and how many instances of it a run gets (the lead has one).
    struct Member: Hashable {
        let role: String
        let slots: Int
    }

    let id: String
    let name: String
    let path: String
    /// The role that leads its runs.
    let lead: String
    let members: [Member]
    /// The shared material every session of it reads.
    let brief: String
    /// What keeps its team from running, one per cause, as the server says.
    var problems: [(text: String, action: String, tab: AppStore.SettingsTab)] {
        json["problems"].array.map { ($0["text"].str, $0["action"].str, AppStore.SettingsTab(rawValue: $0["tab"].str) ?? .agents) }
    }
    /// The record as the server sent it, the editor's starting draft.
    let json: JSON

    init(_ j: JSON) {
        json = j
        id = j["id"].str
        name = j["name"].str
        path = j["path"].str
        lead = j["lead"].str
        members = j["members"].array.map { Member(role: $0["role"].str, slots: $0["slots"].int ?? 1) }
        brief = j["brief"].str
    }
}
