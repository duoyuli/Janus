import SwiftUI

/// The community's picks of a library tab, as the service lists them (`setup_view/picks.rs`), last on the tab: tiles of community skills or MCP servers, each added in
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
        let tiles = tiles
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

    /// One pick, as the service lists it (`GET /api/library/picks`).
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

    /// The tab's tiles, what the library doesn't have yet first, those a search finds.
    private var tiles: [Tile] {
        let kindName = kind == .skill ? "skill" : "mcp"
        let q = query.lowercased()
        return store.picks[kind == .skill ? "skills" : "servers"].array.filter { q.isEmpty || $0["search"].str.contains(q) }.map { t in
            Tile(id: t["id"].str, icon: t["icon"].str, title: t["title"].str, byline: t["byline"].str, blurb: t["blurb"].str,
                 tags: t["tags"].strings, have: t["have"].truthy) { add(kindName, t["id"].str) }
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

    /// Imports a skill from its repository, or adds a server, as the service does it; a server that
    /// needs a key opens the editor on it.
    private func add(_ kind: String, _ id: String) {
        busy.insert(id)
        Task {
            await store.addPick(kind, id)
            busy.remove(id)
        }
    }
}
