import SwiftUI

/// The community's picks of a library tab (`Catalog`), last on the tab: tiles of community skills or MCP servers, each added in
/// one click. A skill is imported from its GitHub repository, through the library's source for
/// that repository when there is one. A server that needs nothing is added at once (kept for the
/// workers that pick it, as every new server is); one that needs a key opens the MCP editor with
/// it filled in but for the key. What the library already has says so.
// @project-doc docs/domains/20261116-resource_library.md#catalog
struct CatalogShelf: View {
    let store: AppStore
    let kind: AppStore.LibraryKind
    let query: String

    @AppStorage("janus.catalogOpen", store: .janus) private var open = true
    @Local private var all = false
    @Local private var busy: Set<String> = []

    /// Tiles shown before「显示全部」.
    static let first = 6

    var body: some View {
        // What the library doesn't have yet comes first: that is what the shelf is for.
        let tiles = (kind == .skill ? skillTiles : serverTiles).enumerated()
            .sorted { ($0.element.have ? 1 : 0, $0.offset) < ($1.element.have ? 1 : 0, $1.offset) }.map(\.element)
        let shown = query.isEmpty && !all ? Array(tiles.prefix(Self.first)) : tiles
        if !(tiles.isEmpty && !query.isEmpty) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(L.text("推荐")).font(.headline)
                    Pill("\(tiles.count)")
                    Text(kind == .skill ? L.text("社区常用的 Skill，一键导入") : L.text("社区常用的 MCP 服务器，一键添加"))
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button(open ? L.text("收起") : L.text("展开")) { open.toggle() }.buttonStyle(.link).font(.callout)
                }
                .padding(.top, 6)
                if open || !query.isEmpty {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 230), spacing: 10, alignment: .top)], alignment: .leading, spacing: 10) {
                        ForEach(shown, id: \.id) { tile($0) }
                    }
                    if query.isEmpty && tiles.count > Self.first {
                        Button(all ? L.text("收起") : L.text("显示全部 \(tiles.count) 个")) { all.toggle() }.buttonStyle(.link).font(.callout)
                    }
                }
            }
        }
    }

    /// One pick, as a tile reads it.
    private struct Tile {
        let id: String
        let icon: String
        let title: String
        let byline: String
        let blurb: String
        let tags: [String]
        let have: Bool
        let add: () -> Void
    }

    private func matches(_ text: String) -> Bool {
        query.isEmpty || text.lowercased().contains(query.lowercased())
    }

    private var skillTiles: [Tile] {
        Catalog.skills.filter { matches("\($0.title) \($0.path) \($0.publisher) \(L.catalogText($0.blurb))") }.map { s in
            Tile(id: s.id, icon: "sparkles", title: s.title, byline: "\(s.publisher) · \(s.repo)", blurb: L.catalogText(s.blurb), tags: [],
                 have: Catalog.item(for: s, items: store.libraryItems, sources: store.librarySources) != nil) { addSkill(s) }
        }
    }

    private var serverTiles: [Tile] {
        Catalog.servers.filter { matches("\($0.title) \($0.name) \($0.publisher) \(L.catalogText($0.blurb))") }.map { s in
            let tags = [s.transport == .stdio ? L.text("命令") : "HTTP",
                        s.inputs.contains(where: \.required) ? L.text("需要 API key") : s.inputs.isEmpty ? "" : L.text("API key 可选"),
                        s.process == .perSession ? L.text("每个会话单独运行") : ""].filter { !$0.isEmpty }
            return Tile(id: s.id, icon: s.transport == .stdio ? "terminal" : "globe", title: s.title, byline: s.publisher, blurb: L.catalogText(s.blurb), tags: tags,
                        have: Catalog.item(for: s, items: store.libraryItems) != nil) { addServer(s) }
        }
    }

    private func tile(_ t: Tile) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: t.icon).foregroundStyle(Color.accent)
                    .frame(width: 26, height: 26)
                    .background(Color.accent.opacity(0.1), in: RoundedRectangle(cornerRadius: 6))
                VStack(alignment: .leading, spacing: 1) {
                    Text(t.title).font(.callout.weight(.semibold)).lineLimit(1)
                    Text(t.byline).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 4)
                if busy.contains(t.id) {
                    ProgressView().controlSize(.small)
                } else if t.have {
                    Label(L.text("已添加"), systemImage: "checkmark").labelStyle(.titleAndIcon).font(.caption).foregroundStyle(Tone.green.color)
                } else {
                    Button(L.text("添加")) { t.add() }.controlSize(.small)
                }
            }
            Text(t.blurb).font(.caption).foregroundStyle(.secondary).lineLimit(2, reservesSpace: true)
            if !t.tags.isEmpty {
                HStack(spacing: 4) { ForEach(t.tags, id: \.self) { Pill($0) } }
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line.opacity(0.7)))
    }

    // MARK: Adding

    /// Imports a skill from its repository: through the library's source for the whole repository
    /// when there is one, otherwise by adding that source first (a clone, like a pasted link).
    private func addSkill(_ s: Catalog.Skill) {
        busy.insert(s.id)
        Task {
            defer { busy.remove(s.id) }
            do {
                let have = store.librarySources.first {
                    $0["repo"].str.lowercased() == s.repo.lowercased() && $0["path"].str.isEmpty && $0["gitRef"].str.isEmpty
                }
                let source: JSON
                if let have { source = have } else {
                    source = try await store.api.post("/api/library/sources", ["url": JSON("https://github.com/\(s.repo)")], timeout: 240)
                }
                guard source["offers"].array.contains(where: { $0["path"].str == s.path }) else {
                    await store.loadLibrary()
                    store.show(L.text("\(s.repo) 里没有找到 \(s.path)，可以在来源里「更新」后再试"))
                    return
                }
                let r = try await store.api.post("/api/library/sources/\(source["id"].str)/import", ["paths": JSON([s.path])])
                await store.loadLibrary()
                store.show(L.text("已导入 \(r.array.map { $0["name"].str }.joined(separator: L.text("、")))，全员可用"))
            } catch { store.show(error.localizedDescription) }
        }
    }

    /// Adds a server that needs nothing at once; one that needs a key opens the editor on it.
    private func addServer(_ s: Catalog.Server) {
        guard s.oneClick else {
            store.editingMcp = AppStore.McpEditing(draft: s.draft, item: nil, note: s.needs)
            return
        }
        busy.insert(s.id)
        Task {
            defer { busy.remove(s.id) }
            do {
                let item = try await store.api.post("/api/library/mcp", s.draft.body)
                await store.loadLibrary()
                store.show(L.text("已添加 \(item["name"].str)，全员可用"))
            } catch { store.show(error.localizedDescription) }
        }
    }
}
