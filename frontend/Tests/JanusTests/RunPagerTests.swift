import Foundation
import Testing
@testable import Janus

/// A stand-in for `GET /api/runs`: `total` runs newest first, paged by the cursor the server gives.
@MainActor private final class FakeRuns {
    var ids: [String]
    var asked: [String] = []
    init(_ total: Int) { ids = (0..<total).map { String(format: "r%03d", total - 1 - $0) } }

    func get(_ path: String) async throws -> JSON {
        asked.append(path)
        let q = URLComponents(string: path)!.queryItems ?? []
        let limit = Int(q.first { $0.name == "limit" }?.value ?? "50")!
        let start = q.first { $0.name == "cursor" }?.value.flatMap { c in ids.firstIndex(of: String(c.split(separator: ":")[1])).map { $0 + 1 } } ?? 0
        let page = Array(ids[start..<min(start + limit, ids.count)])
        let next: JSON = start + limit < ids.count ? JSON("0:\(page.last!)") : .null
        return ["runs": .array(page.map { ["id": JSON($0)] }), "next": next]
    }
}

/// Scrolling to the end reads page after page until every run is in, each once, newest first; a
/// refresh reads the shown ones again in one request and puts a new run on top.
@MainActor @Test func everyRunIsReachedByScrolling() async {
    let fake = FakeRuns(120)
    let pager = RunPager(query: "kind=chat", pageSize: 50) { try await fake.get($0) }
    await pager.refresh()
    #expect(pager.runs.count == 50 && pager.more)
    await pager.loadMore()
    await pager.loadMore()
    #expect(pager.runs.count == 120 && !pager.more)
    #expect(pager.runs.map { $0["id"].str } == fake.ids)
    await pager.loadMore()
    #expect(fake.asked.count == 3, "nothing is read past the last page")

    fake.ids.insert("r999", at: 0)
    await pager.refresh()
    #expect(pager.runs.first?["id"].str == "r999")
    #expect(fake.asked.last?.contains("limit=120") == true, "what is shown is read again in one request")
}

/// Past 500 rows a refresh reads the first 500 again and keeps the rest it had, so the list does not
/// shrink, and scrolling on still reaches the end (FL-20).
@MainActor @Test func aRefreshKeepsWhatWasReadPast500() async {
    let fake = FakeRuns(600)
    let pager = RunPager(query: "kind=chat", pageSize: 50) { try await fake.get($0) }
    await pager.refresh()
    for _ in 0..<10 { await pager.loadMore() }
    #expect(pager.runs.count == 550)
    await pager.refresh()
    #expect(pager.runs.count == 550, "nothing read is dropped")
    #expect(fake.asked.last?.contains("limit=500") == true)
    await pager.loadMore()
    #expect(pager.runs.count == 600 && !pager.more)
    #expect(pager.runs.map { $0["id"].str } == fake.ids)
}
