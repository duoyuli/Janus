import Foundation

/// The run page as a group chat, as the server writes it (`backend/src/run_view/feed.rs`): the
/// posts everyone should see sit in the lead's conversation at their place (`Conversation.Row.post`);
/// each task's topic holds everything said about it; who is at work is one line each. The app
/// decodes and draws them; it groups and words nothing itself
/// (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#run_group
struct Post: Equatable, Identifiable {
    enum Kind: String {
        case dispatch, start, result, failed, rework, lead, conflict, stuck, note, question, message, ask, delivery
    }

    let seq: Int
    let ts: Double
    let kind: Kind
    /// A member's id, `user` or `janus`.
    let by: String
    let task: String?
    let other: String?
    /// In the group too, not only in its task's topic.
    let group: Bool
    let title: String
    let text: String
    let lines: [String]
    let meta: String
    /// A dispatch's tasks, each with what it covers.
    let tasks: [(id: String, scope: String)]
    /// The request an ask is: its card while it is open (`RequestCard.key`).
    let key: String
    /// How an ask was settled once it is not open.
    let settled: String
    /// A delivery's number.
    let n: Int
    /// Waits on the person now: drawn as a mention.
    let you: Bool
    let tone: Tone?

    var id: Int { seq }

    init(_ j: JSON) {
        seq = j["seq"].int ?? 0
        ts = j["ts"].double ?? 0
        kind = Kind(rawValue: j["kind"].str) ?? .note
        by = j["by"].str
        task = j["task"].string
        other = j["other"].string
        group = j["group"].bool ?? false
        title = j["title"].str
        text = j["text"].str
        lines = j["lines"].strings
        meta = j["meta"].str
        tasks = j["tasks"].array.map { (id: $0["id"].str, scope: $0["scope"].str) }
        key = j["key"].str
        settled = j["settled"].str
        n = j["n"].int ?? 0
        you = j["you"].bool ?? false
        tone = j["tone"].string.flatMap(Tone.init(rawValue:))
    }

    static func == (a: Post, b: Post) -> Bool {
        a.seq == b.seq && a.kind == b.kind && a.title == b.title && a.text == b.text && a.lines == b.lines && a.meta == b.meta
            && a.settled == b.settled && a.you == b.you && a.tasks.map(\.id) == b.tasks.map(\.id) && a.tasks.map(\.scope) == b.tasks.map(\.scope)
    }
}

/// What sits beside the group: each task's topic, and the posts waiting on the person (their row
/// ids, where 「@你」 jumps).
struct Feed {
    var topics: [String: [Post]] = [:]
    var mentions: [Int] = []

    init() {}

    init(_ j: JSON) {
        topics = j["topics"].object.mapValues { $0.array.map(Post.init) }
        mentions = j["mentions"].array.compactMap(\.int)
    }
}

/// What the group's side panel shows: the tasks, the files, or one task's topic (its collaborator's
/// session opens where the group is, `RunModel.focus`).
enum GroupSide: Hashable {
    case tasks, files
    case topic(String)
}
