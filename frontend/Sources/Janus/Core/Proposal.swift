import Foundation

/// The digital employees a chat proposed and nobody has acted on yet, as the server has them
/// (`GET /api/runs/{id}/proposal`).
// @project-doc docs/domains/20261120-chats.md#proposed_employees
struct Proposal: Equatable {
    /// Which proposal it is: the `roles.proposed` event that made it. Creating or declining names it,
    /// so a newer proposal is never acted on by mistake.
    let id: JSON
    let roles: [ProposedRole]
    /// As the server answered, for the end-to-end checks that compare it.
    let json: JSON

    /// The server's answer; `nil` when it said none (`null`). Typed, so a chat that proposed
    /// nothing can never show「提议了 0 位数字员工」.
    init?(_ j: JSON) {
        guard !j.isNull else { return nil }
        id = j["proposal"]
        roles = j["roles"].array.map(ProposedRole.init)
        json = j
    }
}

/// One proposed digital employee.
struct ProposedRole: Hashable {
    /// Its number in the proposal: what the person ticks.
    let n: String
    let name: String
    let desc: String
    let group: String
    let prompt: String
    let initials: String
    let color: String
    let lead: Bool
    /// A digital employee already has its name: it starts unticked.
    let existing: Bool

    init(_ j: JSON) {
        n = j["n"].str
        name = j["name"].str
        desc = j["desc"].str
        group = j["group"].str
        prompt = j["prompt"].str
        initials = Role.initials(j["init"].str, name: j["name"].str)
        color = j["color"].truthy ? j["color"].str : "#64748b"
        lead = j["lead"].truthy
        existing = j["existing"].truthy
    }
}
