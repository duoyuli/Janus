import AppKit
import SwiftUI

/// The resource library (Settings › 资源库), a tab each for skills and MCP servers. Everything in it
/// is for every digital worker, reached only through the tab's router, which the top of the tab
/// explains with a short animation: the Skill router (one `janus-skills` that lists them all) and
/// the MCP router (`janus-tools`, which reaches every server on demand). Skills come from GitHub
/// links (dragged or pasted), folders or the editor, and are listed in folding groups by where they
/// came from; MCP servers are written or pasted. MCP values live only in Janus home.
// @project-doc docs/domains/20261116-resource_library.md#delivery
struct LibraryView: View {
    let store: AppStore
    @Local private var query = ""
    @Local private var sheet: Sheet?
    @Local private var link = ""
    @Local private var adding = false
    @Local private var checking = false
    @Local private var picks: [String: Set<String>] = [:]
    @Local private var folder: FolderImport = .idle
    @Local private var folderPicks: Set<String> = []
    @Local private var expanded: Set<String> = []
    /// Groups opened or closed by hand; the rest follow `Self.openByDefault`.
    @Local private var groupsOpen: [String: Bool] = [:]
    /// Sources whose skills not yet imported are listed to pick from.
    @Local private var choosing: Set<String> = []
    @Local private var dropping = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 10) {
                    Picker(L.text("资源类型"), selection: Binding(get: { store.libraryKind }, set: { store.libraryKind = $0 })) {
                        Text(L.text("Skill \(count(.skill))")).tag(AppStore.LibraryKind.skill)
                        Text(L.text("MCP \(count(.mcp))")).tag(AppStore.LibraryKind.mcp)
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                    Spacer(minLength: 8)
                    TextField(store.libraryKind == .skill ? L.text("搜索 Skill") : L.text("搜索 MCP 服务器"), text: $query)
                        .textFieldStyle(.roundedBorder).frame(width: 200)
                }
                Text(L.text("资源库全员可用，按需取用。改动下个任务起生效。"))
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 20).padding(.top, 18).padding(.bottom, 12)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if store.library == nil { ProgressView() }
                    else {
                        switch store.libraryKind {
                        case .skill: skillTab
                        case .mcp: mcpTab
                        }
                    }
                }
                .padding(.horizontal, 20).padding(.bottom, 20)
            }
        }
        .task { await store.loadLibrary() }
        // While a server is being prepared, keep its state current.
        .task(id: store.libraryItems.filter { $0["warm"]["state"].str == "preparing" }.map { $0["id"].str }) {
            while store.libraryItems.contains(where: { $0["warm"]["state"].str == "preparing" }) {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                await store.loadLibrary()
            }
        }
        .sheet(item: $sheet) { s in
            switch s {
            case .skill(let d): SkillEditor(store: store, draft: d)
            }
        }
        .sheet(item: Binding(get: { store.editingMcp }, set: { store.editingMcp = $0 })) { e in
            McpEditor(store: store, start: e.draft, item: e.item, json: e.json, note: e.note, remove: e.item.map { i in { remove(i) } })
        }
    }

    /// The one sheet open over the library.
    enum Sheet: Identifiable {
        case skill(JSON)
        var id: String {
            switch self {
            case .skill(let d): return "skill-\(d["id"].str)"
            }
        }
    }

    /// Copying skills from a folder: picking it, reading what it offers, choosing, copying. The
    /// preview stays while copying, and comes back with any warnings after.
    enum FolderImport {
        case idle
        case scanning
        case choosing(JSON)
        case importing(JSON)
        var busy: Bool { switch self { case .scanning, .importing: return true; default: return false } }
        var preview: JSON? { switch self { case .choosing(let p), .importing(let p): return p; default: return nil } }
    }

    private func items(_ kind: AppStore.LibraryKind) -> [JSON] {
        store.libraryItems.filter { $0["kind"].str == kind.rawValue }
    }

    private func count(_ kind: AppStore.LibraryKind) -> Int { items(kind).count }

    private func matches(_ i: JSON) -> Bool {
        let q = query.lowercased()
        return q.isEmpty || "\(i["name"].str) \(i["description"].str)".lowercased().contains(q)
    }

    /// GitHub sources with new commits, as a notice over the Skill tab.
    @ViewBuilder private var updatesNotice: some View {
        let updates = store.sourceUpdates
        if !updates.isEmpty {
            HStack(spacing: 10) {
                Image(systemName: "arrow.triangle.2.circlepath").foregroundStyle(Tone.blue.color)
                Text(L.text("\(updates.count) 个 GitHub 来源有更新")).font(.callout)
                Spacer()
                Button(L.text("查看")) { for s in updates { groupsOpen[Self.key(.source(s["id"].str))] = true } }.buttonStyle(.link)
                Button(L.text("全部更新")) { Task { await store.updateSources() } }.buttonStyle(.primary)
            }
            .padding(10)
            .background(Tone.blue.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
        }
    }

    // MARK: Skill tab

    /// Three parts, top to bottom: how the Skill router works; the skills added, by where they came
    /// from, with the ways to add one; the community's picks.
    // @project-doc docs/domains/20261116-resource_library.md#skill_router
    @ViewBuilder private var skillTab: some View {
        let skills = items(.skill)
        let searched = Library.searched(store.library)
        RouterGuide(kind: .skill, names: Library.catalogOrder(skills).map { $0["name"].str }, now: skills.isEmpty
            ? L.text("还没有 Skill。加进来就全员可用。")
            : searched
            ? L.text("\(skills.count) 个 Skill 全员可用，经 janus-skills 按关键词搜索。")
            : L.text("\(skills.count) 个 Skill 全员可用，经 janus-skills 直接列出。"),
            searched: searched)
        let groups = Library.skillGroups(skills, sources: store.librarySources)
        let shown = groups.filter { query.isEmpty || $0.items.contains(where: matches) }
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader(L.text("已添加"), count: skills.count, caption: L.text("按来源分组")) {
                if !store.librarySources.isEmpty {
                    Button(checking ? L.text("正在检查…") : L.text("检查 GitHub 更新")) {
                        checking = true
                        Task { await store.checkSources(); checking = false }
                    }
                    .buttonStyle(.link).font(.callout).disabled(checking)
                    .help(L.text("服务每 6 小时检查一次"))
                }
            }
            updatesNotice
            HStack(spacing: 8) {
                githubLink
                Button(L.text("从文件夹…")) { scanFolder() }.disabled(folder.busy)
                Button(L.text("新建")) { sheet = .skill(["name": "", "description": "", "content": ""]) }
            }
            if folder.busy { ProgressView().controlSize(.small) }
            if let preview = folder.preview {
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text(preview["path"].str).fontWeight(.semibold).textSelection(.enabled)
                        Spacer()
                        Button(L.text("取消")) { folder = .idle }.buttonStyle(.link)
                    }
                    offerSelection(preview, imported: importedNames(preview), selection: $folderPicks) { importFolder(preview, $0) }
                }
                .padding(10)
                .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
                .disabled(folder.busy)
            }
            if shown.isEmpty {
                empty(query.isEmpty ? L.text("还没有 Skill。拖个 GitHub 链接进来，或者从文件夹导入、新建、挑个推荐。") : L.text("没有匹配的 Skill"))
            }
            ForEach(shown) { skillGroup($0) }
        }
        CatalogShelf(store: store, kind: .skill, query: query)
    }

    /// A part's title: what it holds, how many, a word on how it is laid out, and its actions.
    private func sectionHeader(_ title: String, count: Int, caption: String, @ViewBuilder actions: () -> some View) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title).font(.headline)
            Pill("\(count)")
            Text(caption).font(.caption).foregroundStyle(.secondary)
            Spacer()
            actions()
        }
        .padding(.top, 6)
    }

    /// The main way a skill arrives: a GitHub link, dropped on the field or pasted into it.
    private var githubLink: some View {
        HStack(spacing: 8) {
            Image(systemName: "link").foregroundStyle(.secondary)
            TextField(L.text("拖入或粘贴 GitHub 链接：仓库，或其中的一个目录"), text: $link)
                .textFieldStyle(.plain)
                .onSubmit(addSource)
                .disabled(adding)
            if adding { ProgressView().controlSize(.small); Text(L.text("正在拉取…")).font(.caption).foregroundStyle(.secondary) }
            else {
                Button(L.text("查找 Skill")) { addSource() }
                    .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(dropping ? Color.accent.opacity(0.08) : Color.card, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(dropping ? Color.accent : Color.line, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        .dropDestination(for: URL.self) { urls, _ in
            guard let u = urls.first, !adding else { return false }
            link = u.absoluteString
            addSource()
            return true
        } isTargeted: { dropping = $0 }
        .help(L.text("Janus 只读取仓库、列出 Skill，不执行任何文件。第三方内容先看再导入。"))
    }

    /// One place skills came from: a GitHub source (with its updates and the skills still to
    /// import), a folder, or the editor.
    @ViewBuilder private func skillGroup(_ g: SkillGroup) -> some View {
        let list = g.items.filter(matches)
        let open = isOpen(Self.key(g.origin), count: g.items.count)
        let summary = L.text("\(g.items.count) 个")
        switch g.origin {
        case .source(let id):
            let s = store.librarySources.first { $0["id"].str == id } ?? [:]
            let imported = importedNames(s)
            let rest = s["offers"].array.filter { imported[$0["path"].str] == nil }
            group(Self.key(g.origin), open: open, icon: "shippingbox", title: Library.sourceName(s),
                  caption: L.text("GitHub · 提交 \(String(s["commit"].str.prefix(7)))") + (rest.isEmpty ? "" : L.text(" · \(rest.count) 个未导入")),
                  summary: summary) {
                if store.updatingSources.contains(id) { ProgressView().controlSize(.small) }
                else if s["updateAvailable"].truthy { Pill(L.text("有更新"), .blue); Button(L.text("更新")) { Task { await store.updateSources([id]) } }.controlSize(.small) }
            } content: {
                ForEach(list, id: \.self) { i in row(i).treeLeaf(last: rest.isEmpty && i == list.last) }
                if !rest.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Button {
                            if choosing.contains(id) { choosing.remove(id) } else { choosing.insert(id) }
                        } label: {
                            Label(choosing.contains(id) ? L.text("收起未导入的 Skill") : L.text("从这个仓库再导入（\(rest.count) 个未导入）"),
                                  systemImage: choosing.contains(id) ? "minus.circle" : "plus.circle")
                        }
                        .buttonStyle(.link).font(.callout)
                        if choosing.contains(id) {
                            VStack(alignment: .leading, spacing: 6) {
                                offerSelection(Self.offering(s, rest), imported: imported, selection: Binding(get: { picks[id] ?? [] }, set: { picks[id] = $0 })) {
                                    importOffers(id, $0)
                                }
                            }
                            .padding(10)
                            .background(Color.soft, in: RoundedRectangle(cornerRadius: 8))
                        }
                    }
                    .padding(.horizontal, 8).padding(.vertical, 8)
                    .treeLeaf(last: true)
                }
            } footer: {
                if s["checkError"].truthy { Note(text: L.text("检查更新失败：\(s["checkError"].str)"), tone: .amber) }
                HStack(spacing: 8) {
                    Image(systemName: "link").font(.caption).foregroundStyle(.tertiary)
                    Text(s["url"].str).font(.caption).foregroundStyle(.secondary).textSelection(.enabled).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button(L.text("删除来源…")) { removeSource(s) }.buttonStyle(.link).font(.caption)
                        .disabled(!g.items.isEmpty).help(g.items.isEmpty ? "" : L.text("还有从它导入的 Skill 时不能删除"))
                }
            }
        case .folder(let path):
            group(Self.key(g.origin), open: open, icon: "folder", title: (path as NSString).abbreviatingWithTildeInPath,
                  caption: L.text("本机文件夹 · 导入时的快照，展开可刷新"), summary: summary) {
                EmptyView()
            } content: {
                ForEach(list, id: \.self) { i in row(i).treeLeaf(last: i == list.last) }
            } footer: {
                EmptyView()
            }
        case .created:
            group(Self.key(g.origin), open: open, icon: "square.and.pencil", title: L.text("在 Janus 中新建"),
                  caption: L.text("可以在 Janus 里编辑"), summary: summary) {
                EmptyView()
            } content: {
                ForEach(list, id: \.self) { i in row(i).treeLeaf(last: i == list.last) }
            } footer: {
                EmptyView()
            }
        }
    }

    /// A source with only the offers given, for choosing among the skills not imported yet.
    static func offering(_ source: JSON, _ offers: [JSON]) -> JSON {
        var s = source
        s["offers"] = .array(offers)
        return s
    }

    static func key(_ origin: SkillGroup.Origin) -> String {
        switch origin {
        case .source(let id): return "source:\(id)"
        case .folder(let path): return "folder:\(path)"
        case .created: return "created"
        }
    }

    // MARK: MCP tab

    /// Three parts, top to bottom: how the MCP router works; the servers added, with the way to add
    /// one; the community's picks.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    @ViewBuilder private var mcpTab: some View {
        let servers = items(.mcp)
        RouterGuide(kind: .mcp, names: servers.map { $0["name"].str }, now: servers.isEmpty
            ? L.text("还没有 MCP 服务器。加进来就全员可用。")
            : L.text("\(servers.count) 个服务器全员可用，经 janus-tools 一个入口取用。"))
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader(L.text("已添加"), count: servers.count, caption: L.text("全员可用，经 janus-tools 取用")) {
                Button(L.text("添加服务器")) { Task { await store.editMcp(nil) } }
                    .help(L.text("填表单，或切到 JSON 贴 README 里的配置"))
            }
            Text(L.text("值只存在本机，不进数据库、任务记录和事件。")).font(.caption).foregroundStyle(.secondary)
            let shown = servers.filter(matches)
            if shown.isEmpty {
                empty(query.isEmpty ? L.text("还没有 MCP 服务器") : L.text("没有匹配的 MCP 服务器"))
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(shown, id: \.self) { i in
                        if i != shown.first { Divider().padding(.leading, Self.inset + Self.tile + 10) }
                        HStack(alignment: .top, spacing: 10) {
                            Self.iconTile(i["server"]["transport"].str == "stdio" ? "terminal" : "globe")
                                .padding(.top, 3)
                                .help(i["server"]["transport"].str == "stdio" ? L.text("命令启动") : L.text("由 Janus 连接"))
                            row(i)
                        }
                        .padding(.horizontal, Self.inset).padding(.vertical, 4)
                    }
                }
                .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line.opacity(0.7)))
            }
        }
        CatalogShelf(store: store, kind: .mcp, query: query)
    }

    // MARK: Pieces

    /// Skill groups up to this size start open; bigger ones start folded.
    static let openByDefault = 6

    /// A group is open when opened by hand, when a search finds something in it, or when it is small.
    private func isOpen(_ key: String, count: Int) -> Bool {
        if !query.isEmpty { return true }
        return groupsOpen[key] ?? (count <= Self.openByDefault)
    }

    /// A folding group: a card whose header says what it is and how many, with its rows hanging
    /// from the header's icon on a tree line, and a footer bar for what belongs to the group itself.
    private func group(_ key: String, open: Bool, icon: String, title: String, caption: String, summary: String,
                       @ViewBuilder trailing: () -> some View, @ViewBuilder content: () -> some View,
                       @ViewBuilder footer: () -> some View) -> some View {
        let footer = footer()
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    groupsOpen[key] = !open
                } label: {
                    HStack(spacing: Self.headerGap) {
                        DisclosureChevron(open: open)
                        Self.iconTile(icon)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(title).font(.body.weight(.semibold)).lineLimit(1).truncationMode(.middle)
                            Text(caption).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        .padding(.leading, 2)
                        Spacer(minLength: 8)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(open ? L.text("收起") : L.text("展开"))
                .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
                trailing()
                Pill(summary)
            }
            .padding(.horizontal, Self.inset).padding(.top, 10).padding(.bottom, open ? 4 : 10)
            if open {
                VStack(alignment: .leading, spacing: 0) { content() }
                    .padding(.leading, Self.inset + 18 + Self.headerGap + Self.tile / 2)
                    .padding(.trailing, Self.inset).padding(.bottom, 8)
                if !(footer is EmptyView) {
                    Divider()
                    VStack(alignment: .leading, spacing: 8) { footer }
                        .padding(.horizontal, Self.inset).padding(.vertical, 8)
                        .background(Color.soft.opacity(0.6))
                }
            }
        }
        .background(Color.card)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line.opacity(0.7)))
        .animation(.easeOut(duration: 0.18), value: open)
    }

    /// What a group or a server is, as a glyph on a tinted tile.
    private static func iconTile(_ symbol: String) -> some View {
        Image(systemName: symbol)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(Color.accent)
            .frame(width: tile, height: tile)
            .background(Color.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 7))
            .accessibilityHidden(true)
    }

    /// A group header's measures; the tree line hangs from the middle of its icon tile.
    private static let inset: CGFloat = 12
    private static let headerGap: CGFloat = 8
    private static let tile: CGFloat = 28

    private func empty(_ text: String) -> some View {
        Text(text).font(.callout).foregroundStyle(.secondary).padding(.vertical, 8)
    }

    /// One resource: name, a line of description, what keeps it from working (amber), and
    /// at the end of the line the fold for the rest, so it never looks like the group's own fold.
    private func row(_ i: JSON) -> some View {
        let id = i["id"].str
        let open = expanded.contains(id)
        let mcp = i["kind"].str == "mcp"
        let warning = Self.warning(i)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Button {
                    if open { expanded.remove(id) } else { expanded.insert(id) }
                } label: {
                    HStack(spacing: 8) {
                        Text(i["name"].str).font(.system(.callout, design: .monospaced).weight(.medium)).lineLimit(1)
                        Text(i["description"].truthy ? i["description"].str : mcp ? Library.endpoint(i) : "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Spacer(minLength: 8)
                        if !warning.isEmpty { Text(warning).font(.caption).foregroundStyle(Tone.amber.color).lineLimit(1) }
                        switch i["warm"]["state"].str {
                        case "preparing": Pill(L.text("准备中…"), .blue)
                        case "failed": Text(L.text("启动失败")).font(.caption).foregroundStyle(Tone.red.color)
                        default: EmptyView()
                        }
                        DisclosureChevron(open: open)
                    }
                    .padding(.horizontal, 8).padding(.vertical, 7)
                    .contentShape(Rectangle())
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(open ? L.text("收起") : L.text("展开详情"))
                .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
            }
            if open { details(i).padding(.horizontal, 8).padding(.bottom, 8) }
        }
        .padding(.vertical, 1)
    }

    private func details(_ i: JSON) -> some View {
        let mcp = i["kind"].str == "mcp"
        let vars = Library.vars(i)
        let process = i["server"]["process"].str == "perSession" ? L.text("每个会话单独运行") : L.text("同一任务共用一个进程，用到才启动")
        let lines = [mcp ? Library.endpoint(i) : "", i["description"].str, Library.origin(i), vars.text,
                     i["server"]["transport"].str == "stdio" ? process : "", mcp ? "" : Library.usage(i, now: L.nowMs())].filter { !$0.isEmpty }
        return VStack(alignment: .leading, spacing: 3) {
            ForEach(lines, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
            // How findable the skill is: the agent picks it by its description alone.
            ForEach(i["hints"].array.map(Library.hint), id: \.self) { Text($0).font(.caption).foregroundStyle(Tone.amber.color).textSelection(.enabled) }
            if mcp { warmState(i) }
            HStack(spacing: 12) {
                if mcp || i["origin"]["type"].str == "created" { Button(L.text("编辑")) { edit(i) }.buttonStyle(.link) }
                if i["origin"]["type"].str == "folder" { Button(L.text("刷新")) { refresh(i) }.buttonStyle(.link).help(L.text("从原文件夹重新复制")) }
                Button(L.text("删除")) { remove(i) }.buttonStyle(.link)
            }
            .padding(.top, 2)
        }
    }

    /// How preparing a server went: a command started once when added, so its packages are
    /// downloaded before any run needs it, or a URL reached once; a server that can't start or
    /// refuses its values shows here.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    @ViewBuilder private func warmState(_ i: JSON) -> some View {
        let w = i["warm"]
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            switch w["state"].str {
            case "preparing":
                Text(i["server"]["transport"].str == "stdio" ? L.text("首次启动，正在下载依赖，大的包要等几分钟…") : L.text("正在连接，列出它的工具…"))
                    .font(.caption).foregroundStyle(.secondary)
            case "ready": Text(L.text("已准备好 · \(w["tools"].int ?? 0) 个工具")).font(.caption).foregroundStyle(Tone.green.color)
            case "failed": Text(L.text("启动失败：\(w["error"].str)")).font(.caption).foregroundStyle(Tone.red.color).textSelection(.enabled)
            default: Text(L.text("还没有准备过：任务里第一次用到时再启动或连接")).font(.caption).foregroundStyle(.secondary)
            }
            if w["state"].str != "preparing" {
                Button(L.text("重新准备")) { Task { await store.warmMcp(i["id"].str) } }.buttonStyle(.link).font(.caption)
            }
        }
    }

    /// What keeps an item from working, in one line: values not filled in, or gone from its source.
    static func warning(_ i: JSON) -> String {
        let vars = Library.vars(i)
        let gone = i["origin"]["type"].str == "git" && !i["source"]["inSource"].truthy
        let hints = i["hints"].array.isEmpty ? "" : L.text("描述可以改进")
        return [vars.complete ? "" : vars.text, gone ? L.text("来源中已删除") : "", hints].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private func importedNames(_ preview: JSON) -> [String: String] {
        Dictionary(preview["imported"].array.map { ($0["path"].str, $0["name"].str) }, uniquingKeysWith: { a, _ in a })
    }

    @ViewBuilder private func offerSelection(
        _ preview: JSON,
        imported: [String: String],
        selection: Binding<Set<String>>,
        onImport: @escaping ([String]) -> Void
    ) -> some View {
        let offers = preview["offers"].array
        let available = offers.map { $0["path"].str }.filter { imported[$0] == nil }
        let chosen = available.filter { selection.wrappedValue.contains($0) }
        ForEach(offers, id: \.self) { o in
            let path = o["path"].str
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if let name = imported[path] {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Tone.green.color).frame(width: 16)
                    offerText(o, extra: name == o["name"].str ? L.text("已导入") : L.text("已导入为 \(name)"))
                } else {
                    Toggle(isOn: Binding(get: { selection.wrappedValue.contains(path) }, set: { on in
                        var set = selection.wrappedValue
                        if on { set.insert(path) } else { set.remove(path) }
                        selection.wrappedValue = set
                    })) { offerText(o, extra: "") }
                    .toggleStyle(.checkbox)
                }
            }
        }
        ForEach(preview["warnings"].strings, id: \.self) { w in
            Text("⚠ \(L.catalogText(w))").font(.caption).foregroundStyle(Tone.amber.color)
        }
        if !available.isEmpty {
            HStack {
                Button(chosen.count == available.count ? L.text("全不选") : L.text("全选")) {
                    selection.wrappedValue = chosen.count == available.count ? [] : Set(available)
                }
                .buttonStyle(.link)
                Spacer()
                Button(L.text("导入所选（\(chosen.count)）")) { onImport(chosen) }
                    .buttonStyle(.primary).disabled(chosen.isEmpty)
            }
        }
    }

    private func offerText(_ o: JSON, extra: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: 6) {
                Text(o["name"].str).fontWeight(.medium)
                if !extra.isEmpty { Text(extra).font(.caption).foregroundStyle(.secondary) }
            }
            Text([o["description"].str, o["path"].str].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(2)
        }
    }

    // MARK: Actions

    private func edit(_ i: JSON) {
        guard i["kind"].str == "skill" else { Task { await store.editMcp(i) }; return }
        Task {
            do {
                let full = try await store.api.get("/api/library/items/\(i["id"].str)")
                sheet = .skill(["id": full["id"], "name": full["name"], "content": full["content"]])
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func refresh(_ i: JSON) {
        Task {
            do {
                let r = try await store.api.post("/api/library/items/\(i["id"].str)/refresh")
                await store.loadLibrary()
                store.show(r["revision"].str == i["revision"].str ? L.text("\(i["name"].str) 没有变化") : L.text("已刷新 \(i["name"].str)，下个任务起生效"))
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func remove(_ i: JSON) {
        let note = L.text("进行中的任务不受影响；这个服务器会停下，它的值一起删除。")
        guard confirm(L.text("删除 \(i["name"].str)？"), note, ok: L.text("删除")) else { return }
        Task {
            do { try await store.api.delete("/api/library/items/\(i["id"].str)"); await store.loadLibrary() }
            catch { store.show(error.localizedDescription) }
        }
    }

    private func scanFolder() {
        guard !folder.busy else { return }
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.prompt = L.text("查找 Skill")
        p.message = L.text("选择一个 Skill 文件夹，或包含多个 Skill 的父文件夹")
        guard p.runModal() == .OK, let u = p.url else { return }
        let before = folder
        folder = .scanning
        Task {
            do {
                let preview = try await store.api.post("/api/library/skills/scan", ["path": JSON(u.path)])
                let imported = importedNames(preview)
                folderPicks = Set(preview["offers"].array.map { $0["path"].str }.filter { imported[$0] == nil })
                folder = .choosing(preview)
            } catch {
                folder = before
                store.show(error.localizedDescription)
            }
        }
    }

    private func importFolder(_ preview: JSON, _ paths: [String]) {
        guard case .choosing = folder else { return }
        folder = .importing(preview)
        Task {
            do {
                let result = try await store.api.post("/api/library/skills/import", ["path": preview["path"], "paths": JSON(paths)])
                let imported = result["imported"].array
                if result["warnings"].strings.isEmpty {
                    folder = .idle
                } else {
                    var updated = preview
                    updated["imported"] = .array(preview["imported"].array + imported)
                    updated["warnings"] = result["warnings"]
                    folder = .choosing(updated)
                }
                await store.loadLibrary()
                if !imported.isEmpty {
                    store.show(L.text("已导入 \(imported.map { $0["name"].str }.joined(separator: "、"))"))
                }
            } catch {
                folder = .choosing(preview)
                store.show(error.localizedDescription)
            }
        }
    }

    private func addSource() {
        let url = link.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty, !adding else { return }
        adding = true
        Task {
            defer { adding = false }
            do {
                let s = try await store.api.post("/api/library/sources", ["url": JSON(url)], timeout: 240)
                link = ""
                picks[s["id"].str] = Set(s["offers"].array.map { $0["path"].str })
                groupsOpen[Self.key(.source(s["id"].str))] = true
                choosing.insert(s["id"].str)
                await store.loadLibrary()
                store.show(L.text("找到 \(s["offers"].array.count) 个 Skill，选择要导入的"))
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importOffers(_ id: String, _ paths: [String]) {
        Task {
            do {
                let r = try await store.api.post("/api/library/sources/\(id)/import", ["paths": JSON(paths)])
                picks[id] = []
                await store.loadLibrary()
                store.show(L.text("已导入 \(r.array.map { $0["name"].str }.joined(separator: L.text("、")))"))
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func removeSource(_ s: JSON) {
        guard confirm(L.text("删除来源 \(s["url"].str)？"), L.text("本机的仓库副本会删除。还有从它导入的资源时不能删除。"), ok: L.text("删除")) else { return }
        Task {
            do { try await store.api.delete("/api/library/sources/\(s["id"].str)"); await store.loadLibrary() }
            catch { store.show(error.localizedDescription) }
        }
    }
}

/// The line a group's rows hang on: down from the header's icon, with a short branch to each row
/// and a rounded end at the last one.
private struct TreeGuide: Shape {
    let last: Bool
    /// Where the branch meets a row: the middle of its first line.
    static let branch: CGFloat = 16
    static let width: CGFloat = 22

    func path(in r: CGRect) -> Path {
        let x = r.minX + 0.5, y = r.minY + Self.branch, end = r.minX + 12
        var p = Path()
        p.move(to: CGPoint(x: x, y: r.minY))
        if last {
            p.addLine(to: CGPoint(x: x, y: y - 6))
            p.addQuadCurve(to: CGPoint(x: x + 6, y: y), control: CGPoint(x: x, y: y))
            p.addLine(to: CGPoint(x: end, y: y))
        } else {
            p.addLine(to: CGPoint(x: x, y: r.maxY))
            p.move(to: CGPoint(x: x, y: y))
            p.addLine(to: CGPoint(x: end, y: y))
        }
        return p
    }
}

private extension View {
    /// One row of a group, hung on the group's tree line.
    func treeLeaf(last: Bool) -> some View {
        padding(.leading, TreeGuide.width)
            .background(alignment: .leading) {
                TreeGuide(last: last).stroke(Color.line, lineWidth: 1).frame(width: TreeGuide.width)
            }
    }
}

/// A skill written in Janus: name, description and body when new; the whole SKILL.md when edited.
struct SkillEditor: View {
    let store: AppStore
    let draft: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var d: JSON = [:]
    @Local private var saving = false

    var body: some View {
        let editing = d["id"].truthy
        VStack(alignment: .leading, spacing: 10) {
            Text(editing ? L.text("编辑 Skill") : L.text("新建 Skill")).font(.display(.title3))
            TextField(L.text("名称：小写字母、数字和 -，例如 release-notes"), text: field("name")).textFieldStyle(.roundedBorder)
            if !editing {
                TextField(L.text("什么时候用，例如：写发布说明时使用"), text: field("description")).textFieldStyle(.roundedBorder)
            }
            Text(editing ? L.text("SKILL.md（名称以上面填写的为准）") : L.text("正文：Agent 使用这个 Skill 时读到的说明")).font(.caption).foregroundStyle(.secondary)
            TextEditor(text: field("content"))
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 260)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
            HStack {
                Spacer()
                Button(L.text("取消")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(saving ? L.text("正在保存…") : L.text("保存")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(saving)
            }
        }
        .padding(18)
        .frame(width: 620)
        .onAppear { d = draft }
    }

    private func field(_ k: String) -> Binding<String> { Binding(get: { d[k].str }, set: { d[k] = JSON($0) }) }

    private func save() {
        saving = true
        Task {
            defer { saving = false }
            do {
                let body: JSON = ["name": d["name"], "description": d["description"], "content": d["content"]]
                let item = d["id"].truthy
                    ? try await store.api.put("/api/library/skills/\(d["id"].str)", body)
                    : try await store.api.post("/api/library/skills", body)
                await store.loadLibrary()
                store.show(L.text("已保存 Skill \(item["name"].str)"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}
