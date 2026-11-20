import Foundation
import Observation

/// A run list read from the server a page at a time, newest first (`GET /api/runs`, see
/// docs/interfaces/20261031-http_api.md#run_list): the sidebar's chats, a workspace's task records.
/// Scrolling to the end reads the next page (`loadMore`), until there is none, so every run can be
/// reached however many there are. `refresh` reads again what is already shown, in one request, so
/// statuses stay current and new runs appear at the top.
@Observable @MainActor
final class RunPager {
    /// The query of the list, e.g. `kind=chat` or `workspace=w1`.
    let query: String
    let pageSize: Int
    private(set) var runs: [JSON] = []
    /// The cursor of the next page; nil once the last page is in.
    private(set) var next: String?
    private(set) var loaded = false
    @ObservationIgnored private let fetch: (String) async throws -> JSON
    @ObservationIgnored private var loading = false

    init(query: String, pageSize: Int = 50, fetch: @escaping (String) async throws -> JSON) {
        self.query = query
        self.pageSize = pageSize
        self.fetch = fetch
    }

    convenience init(query: String, api: API) {
        self.init(query: query) { try await api.get($0) }
    }

    var more: Bool { next != nil }

    /// Read the newest runs again, as many as are shown (at least a page). A failed read keeps what
    /// was shown; an unchanged list leaves the views alone.
    func refresh() async {
        guard let page = try? await fetch(path(limit: max(pageSize, runs.count), cursor: nil)) else { return }
        let fresh = page["runs"].array
        if fresh != runs { runs = fresh }
        next = page["next"].string
        loaded = true
    }

    /// Read the page after the last run shown, once at a time; nothing when all are in.
    func loadMore() async {
        guard let cursor = next, !loading else { return }
        loading = true
        defer { loading = false }
        guard let page = try? await fetch(path(limit: pageSize, cursor: cursor)) else { return }
        // A refresh in the meantime moved on; this page belongs to the old list.
        guard cursor == next else { return }
        let known = Set(runs.map { $0["id"].str })
        runs += page["runs"].array.filter { !known.contains($0["id"].str) }
        next = page["next"].string
    }

    private func path(limit: Int, cursor: String?) -> String {
        var p = "/api/runs?\(query)&limit=\(min(limit, 500))"
        if let cursor { p += "&cursor=\(cursor.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? cursor)" }
        return p
    }
}
