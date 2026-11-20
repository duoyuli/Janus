import AppKit
import SwiftUI

/// The resource library (Settings › 资源库), a tab each for skills and MCP servers. Everything in it
/// is for every digital worker by default, reached through the tab's router, which the top of the
/// tab explains with a short animation: the Skill router (skills load directly, or through one
/// `janus-skills` past ten) and the MCP router (`janus-tools`, which runs the command servers on
/// demand). A server with side effects or credentials can be kept for the workers that pick it.
/// Skills come from GitHub links (dragged or pasted), folders or the editor, and are listed in
/// folding groups by where they came from; MCP servers are written or pasted, and grouped by who
/// gets them. MCP values live only in Janus home and are never shown again.
// @project-doc docs/domains/20261116-resource_library.md#reach
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
                Text(L.text("资源默认给所有数字员工，经路由按需取用；有副作用或带凭据的可以收口，只给选中它的数字员工。任务开始时固定它用的资源，这里的改动从下一个任务起生效。"))
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
            McpEditor(store: store, start: e.draft, item: e.item, json: e.json, remove: e.item.map { i in { remove(i) } })
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

    /// How the Skill router works, then the skills added, by where they came from, with the ways
    /// to add one.
    // @project-doc docs/domains/20261116-resource_library.md#skill_router
    @ViewBuilder private var skillTab: some View {
        let skills = items(.skill)
        let everyone = skills.filter { $0["reach"].str == "everyone" }.map { $0["name"].str }
        RouterGuide(kind: .skill, names: everyone, now: everyone.count > SkillRoute.over
            ? L.text("现在 \(everyone.count) 个 Skill 给所有数字员工，超过 \(SkillRoute.over) 个，合并成一个 janus-skills。只给选中的 Skill 不进路由，照常逐个加载。")
            : L.text("现在 \(everyone.count) 个 Skill 给所有数字员工，不超过 \(SkillRoute.over) 个，每个照常直接加载；超过后自动改由 janus-skills 路由。"))
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
                empty(query.isEmpty ? L.text("还没有 Skill。拖入 GitHub 链接、从文件夹复制，或新建一个。") : L.text("没有匹配的 Skill"))
            } else {
                HStack {
                    Spacer()
                    Text(L.text("给所有数字员工")).font(.caption).foregroundStyle(.secondary)
                }
                .padding(.trailing, 10).padding(.bottom, -4)
            }
            ForEach(shown) { skillGroup($0) }
        }
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
        .help(L.text("Janus 拉取仓库，列出其中的 Skill（SKILL.md），由你选择导入哪些。只读取，不执行仓库中的任何文件。导入的 Skill 默认给所有数字员工，第三方内容请先看过再导入。"))
    }

    /// One place skills came from: a GitHub source (with its updates and the skills still to
    /// import), a folder, or the editor.
    @ViewBuilder private func skillGroup(_ g: SkillGroup) -> some View {
        let list = g.items.filter(matches)
        let open = isOpen(Self.key(g.origin), count: g.items.count)
        let everyone = g.items.filter { $0["reach"].str == "everyone" }.count
        let summary = everyone == g.items.count ? L.text("\(g.items.count) 个") : L.text("\(g.items.count) 个 · \(everyone) 个给所有数字员工")
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
                if s["checkError"].truthy { Note(text: L.text("检查更新失败：\(s["checkError"].str)"), tone: .amber).padding(10) }
                ForEach(list, id: \.self) { i in Divider(); row(i) }
                if !rest.isEmpty {
                    Divider()
                    VStack(alignment: .leading, spacing: 6) {
                        Button(choosing.contains(id) ? L.text("收起未导入的 Skill") : L.text("从这个仓库再导入（\(rest.count) 个未导入）")) {
                            if choosing.contains(id) { choosing.remove(id) } else { choosing.insert(id) }
                        }
                        .buttonStyle(.link)
                        if choosing.contains(id) {
                            offerSelection(Self.offering(s, rest), imported: imported, selection: Binding(get: { picks[id] ?? [] }, set: { picks[id] = $0 })) {
                                importOffers(id, $0)
                            }
                        }
                    }
                    .padding(10)
                }
                Divider()
                HStack(spacing: 12) {
                    Text(s["url"].str).font(.caption).foregroundStyle(.secondary).textSelection(.enabled).lineLimit(1)
                    Spacer()
                    Button(L.text("删除来源…")) { removeSource(s) }.buttonStyle(.link).font(.caption)
                        .disabled(!g.items.isEmpty).help(g.items.isEmpty ? "" : L.text("还有从它导入的 Skill 时不能删除"))
                }
                .padding(.horizontal, 10).padding(.vertical, 8)
            }
        case .folder(let path):
            group(Self.key(g.origin), open: open, icon: "folder", title: (path as NSString).abbreviatingWithTildeInPath,
                  caption: L.text("本机文件夹 · 复制时的内容，展开某个 Skill 可以刷新"), summary: summary) {
                EmptyView()
            } content: {
                ForEach(list, id: \.self) { i in Divider(); row(i) }
            }
        case .created:
            group(Self.key(g.origin), open: open, icon: "square.and.pencil", title: L.text("在 Janus 中新建"),
                  caption: L.text("可以在 Janus 里编辑"), summary: summary) {
                EmptyView()
            } content: {
                ForEach(list, id: \.self) { i in Divider(); row(i) }
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

    /// How the MCP router works, then the servers added, by who gets them, with the way to add one.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    @ViewBuilder private var mcpTab: some View {
        let servers = items(.mcp)
        let everyone = servers.filter { $0["reach"].str == "everyone" }
        let routed = everyone.filter { $0["server"]["transport"].str == "stdio" }.map { $0["name"].str }
        let direct = everyone.count - routed.count
        RouterGuide(kind: .mcp, names: routed, now: [
            routed.isEmpty ? L.text("现在没有经 janus-tools 的服务器：命令启动的服务器给所有数字员工后才经过它。")
                : L.text("现在 \(routed.count) 个命令启动的服务器经 janus-tools。"),
            direct > 0 ? L.text("\(direct) 个 HTTP、SSE 服务器直接交给会话。") : "",
            L.text("只给选中的服务器不经过路由，照常逐个交给会话。"),
        ].joined())
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader(L.text("已添加"), count: servers.count, caption: L.text("按给谁用分组")) {
                Button(L.text("添加服务器")) { Task { await store.editMcp(nil) } }
                    .help(L.text("填写表单，或切到 JSON 粘贴 README 里的配置（Claude 的 mcpServers、VS Code 的 servers）"))
            }
            Text(L.text("环境变量和 HTTP header 的值只保存在本机的 Janus home，在编辑器里查看和修改，不写进数据库、任务记录和事件。")).font(.caption).foregroundStyle(.secondary)
            mcpGroup("everyone", icon: "person.3", title: L.text("给所有数字员工"),
                     caption: L.text("命令启动的经 janus-tools 用到才启动，HTTP、SSE 直接交给会话"),
                     list: everyone, empty: L.text("还没有给所有数字员工的服务器。新添加的服务器先收口，打开它的开关就给所有数字员工。"))
            mcpGroup("picked", icon: "lock", title: L.text("只给选中的数字员工"),
                     caption: L.text("在「数字员工」或展开的服务器里勾选给谁，会话启动时逐个交给 Agent"),
                     list: servers.filter { $0["reach"].str == "picked" }, empty: L.text("没有收口的服务器"))
        }
    }

    @ViewBuilder private func mcpGroup(_ id: String, icon: String, title: String, caption: String, list: [JSON], empty text: String) -> some View {
        let shown = list.filter(matches)
        if query.isEmpty || !shown.isEmpty {
            group("mcp:\(id)", open: isOpen("mcp:\(id)", count: list.count), icon: icon, title: title, caption: caption, summary: L.text("\(list.count) 个")) {
                EmptyView()
            } content: {
                if list.isEmpty { Divider(); empty(text).padding(.horizontal, 10) }
                ForEach(shown, id: \.self) { i in Divider(); row(i) }
            }
        }
    }

    // MARK: Pieces

    /// Groups up to this size start open; bigger ones start folded.
    static let openByDefault = 6

    /// A group is open when opened by hand, when a search finds something in it, or when it is small.
    private func isOpen(_ key: String, count: Int) -> Bool {
        if !query.isEmpty { return true }
        return groupsOpen[key] ?? (count <= Self.openByDefault)
    }

    /// A folding group: a header with what it is and how many, its rows under it when open.
    private func group(_ key: String, open: Bool, icon: String, title: String, caption: String, summary: String,
                       @ViewBuilder trailing: () -> some View, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    groupsOpen[key] = !open
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                            .rotationEffect(.degrees(open ? 90 : 0))
                        Image(systemName: icon).foregroundStyle(Color.accent).frame(width: 18)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(title).font(.callout.weight(.semibold)).lineLimit(1).truncationMode(.middle)
                            Text(caption).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        Spacer(minLength: 8)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(open ? L.text("收起") : L.text("展开"))
                trailing()
                Pill(summary)
            }
            .padding(.horizontal, 12).padding(.vertical, 10)
            if open { content() }
        }
        .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line.opacity(0.7)))
        .animation(.easeOut(duration: 0.18), value: open)
    }

    private func empty(_ text: String) -> some View {
        Text(text).font(.callout).foregroundStyle(.secondary).padding(.vertical, 8)
    }

    /// One line: name, a line of description, what keeps it from working (amber) and the switch;
    /// the rest unfolds.
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
                        Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary).rotationEffect(.degrees(open ? 90 : 0))
                        Text(i["name"].str).font(.system(.body, design: .monospaced).weight(.medium)).lineLimit(1)
                        Text(i["description"].truthy ? i["description"].str : mcp ? Library.endpoint(i) : "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Spacer(minLength: 8)
                        if i["reach"].str == "picked" { Text(Library.usedBy(i)).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                        if !warning.isEmpty { Text(warning).font(.caption).foregroundStyle(Tone.amber.color).lineLimit(1) }
                        switch i["warm"]["state"].str {
                        case "preparing": Pill(L.text("准备中…"), .blue)
                        case "failed": Text(L.text("启动失败")).font(.caption).foregroundStyle(Tone.red.color)
                        default: EmptyView()
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(open ? L.text("收起") : L.text("展开详情"))
                Toggle(L.text("\(i["name"].str) 给所有数字员工"), isOn: Binding(get: { i["reach"].str == "everyone" }, set: { setReach(i, $0) }))
                    .toggleStyle(.switch).controlSize(.mini).labelsHidden()
                    .help(i["reach"].str == "everyone" ? L.text("关掉：只给选中它的数字员工") : L.text("打开：给所有数字员工"))
            }
            if open { details(i) }
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
    }

    private func details(_ i: JSON) -> some View {
        let mcp = i["kind"].str == "mcp"
        let vars = Library.vars(i)
        let process = i["server"]["process"].str == "perSession" ? L.text("每个会话单独运行") : L.text("同一个任务的会话共用一个进程，用到才启动")
        let lines = [mcp ? Library.endpoint(i) : "", i["description"].str, Library.origin(i), vars.text,
                     i["server"]["transport"].str == "stdio" ? process : ""].filter { !$0.isEmpty }
        return VStack(alignment: .leading, spacing: 3) {
            ForEach(lines, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
            ForEach(Self.unsupported(i), id: \.self) { Text($0).font(.caption).foregroundStyle(Tone.amber.color) }
            if i["server"]["transport"].str == "stdio" { warmState(i) }
            if i["reach"].str == "picked" { givenTo(i) }
            HStack(spacing: 12) {
                if mcp || i["origin"]["type"].str == "created" { Button(L.text("编辑")) { edit(i) }.buttonStyle(.link) }
                if i["origin"]["type"].str == "folder" { Button(L.text("刷新")) { refresh(i) }.buttonStyle(.link).help(L.text("从原文件夹重新复制")) }
                Button(L.text("删除")) { remove(i) }.buttonStyle(.link)
            }
            .padding(.top, 2)
        }
        .padding(.leading, 22)
    }

    /// How preparing a command server went: started once when added, so its packages are downloaded
    /// before any run needs it and a server that can't start shows here.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    @ViewBuilder private func warmState(_ i: JSON) -> some View {
        let w = i["warm"]
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            switch w["state"].str {
            case "preparing": Text(L.text("正在启动一次，下载它需要的包，大的包可能要几分钟…")).font(.caption).foregroundStyle(.secondary)
            case "ready": Text(L.text("已准备好 · \(w["tools"].int ?? 0) 个工具")).font(.caption).foregroundStyle(Tone.green.color)
            case "failed": Text(L.text("启动失败：\(w["error"].str)")).font(.caption).foregroundStyle(Tone.red.color).textSelection(.enabled)
            default: Text(L.text("还没有准备过：任务里第一次启动可能较慢")).font(.caption).foregroundStyle(.secondary)
            }
            if w["state"].str != "preparing" {
                Button(L.text("重新准备")) { Task { await store.warmMcp(i["id"].str) } }.buttonStyle(.link).font(.caption)
            }
        }
    }

    /// The digital workers a kept item is given to, ticked here or in the worker's editor: both
    /// change the same choice.
    @ViewBuilder private func givenTo(_ i: JSON) -> some View {
        let given = Set(i["roles"].array.map { $0["id"].str })
        VStack(alignment: .leading, spacing: 2) {
            Text(L.text("给这些数字员工")).font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.top, 4)
            ForEach(store.roles, id: \.self) { r in
                let id = r["id"].str
                Toggle(r["name"].str, isOn: Binding(get: { given.contains(id) }, set: { on in
                    var next = given
                    if on { next.insert(id) } else { next.remove(id) }
                    Task { await store.setItemRoles(i["id"].str, Array(next).sorted()) }
                }))
                .toggleStyle(.checkbox).font(.callout)
            }
        }
    }

    /// What keeps an item from working, in one line: values not filled in, or gone from its source.
    static func warning(_ i: JSON) -> String {
        let vars = Library.vars(i)
        let gone = i["origin"]["type"].str == "git" && !i["source"]["inSource"].truthy
        return [vars.complete ? "" : vars.text, gone ? L.text("来源中已删除") : ""].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// The runtimes that can't use the item, with why.
    static func unsupported(_ i: JSON) -> [String] {
        i["support"].array.filter { $0["support"]["state"].str == "no" }.map { L.catalogText($0["support"]["why"].str) }
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

    /// Opening a server to everyone lets every digital worker call it, side effects included: ask first.
    private func setReach(_ i: JSON, _ everyone: Bool) {
        if everyone && i["kind"].str == "mcp"
            && !confirm(L.text("把 \(i["name"].str) 给所有数字员工？"), L.text("每个任务的负责人和所有协作者都能调用它的工具，包括写数据、发消息这类有副作用的操作。"), ok: L.text("给所有数字员工")) {
            return
        }
        Task { await store.setReach(i["id"].str, everyone ? "everyone" : "picked") }
    }

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
                store.show(r["revision"].str == i["revision"].str ? L.text("\(i["name"].str) 没有变化") : L.text("已从文件夹刷新 \(i["name"].str)，新的任务使用新内容"))
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func remove(_ i: JSON) {
        let picked = i["roles"].array.map { $0["name"].str }
        let note = [picked.isEmpty ? "" : L.text("这些数字员工选了它，删除后一并去掉：\(picked.joined(separator: L.text("、")))。"),
                    L.text("已经开始的任务有自己的副本，不受影响；正在运行的这个 MCP 服务器会停止。MCP 服务器的值会一起删除。")].joined()
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
            Text(editing ? L.text("SKILL.md（front matter 中的 name 以上面的名称为准）") : L.text("正文：Agent 使用这个 Skill 时读到的说明")).font(.caption).foregroundStyle(.secondary)
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
