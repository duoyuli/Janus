import AppKit
import SwiftUI

/// The resource library (Settings › 资源库). Everything in it is for every digital worker by
/// default, reached through two routers: the Skill router (skills load directly, or through one
/// `janus-skills` past ten) and the MCP router (`janus-tools`, which runs the command servers on
/// demand). A server with side effects or credentials can be kept for the workers that pick it.
/// Skills come from GitHub links (dragged or pasted), folders or the editor; MCP servers are
/// written or pasted. MCP values live only in Janus home and are never shown again.
// @project-doc docs/domains/20261116-resource_library.md#reach
struct LibraryView: View {
    let store: AppStore
    @Local private var query = ""
    @Local private var skillDraft: JSON?
    @Local private var mcpDraft: JSON?
    @Local private var paste: String?
    @Local private var link = ""
    @Local private var adding = false
    @Local private var checking = false
    @Local private var picks: [String: Set<String>] = [:]
    @Local private var folderPreview: JSON?
    @Local private var folderPicks: Set<String> = []
    @Local private var folderBusy = false
    @Local private var expanded: Set<String> = []
    @Local private var showKept = false
    @Local private var showSources = false
    @Local private var dropping = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(L.text("资源库里的 Skill 和 MCP 默认给所有数字员工，经两个路由按需取用；有副作用或带凭据的工具可以收口，只给选中它的数字员工。"))
                        .font(.callout).foregroundStyle(.secondary)
                    Text(L.text("任务开始时固定它用的资源：这里的添加、修改和开关从下一个任务起生效。"))
                        .font(.caption).foregroundStyle(.tertiary)
                }
                Spacer(minLength: 8)
                TextField(L.text("搜索"), text: $query).textFieldStyle(.roundedBorder).frame(width: 180)
            }
            .padding(.horizontal, 20).padding(.top, 18).padding(.bottom, 10)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if store.library == nil { ProgressView() }
                    else {
                        updatesNotice
                        skillRouter
                        mcpRouter
                        kept
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
        .sheet(isPresented: Binding(get: { skillDraft != nil }, set: { if !$0 { skillDraft = nil } })) {
            if let d = skillDraft { SkillEditor(store: store, draft: d) }
        }
        .sheet(isPresented: Binding(get: { mcpDraft != nil }, set: { if !$0 { mcpDraft = nil } })) {
            if let d = mcpDraft { McpEditor(store: store, draft: d) }
        }
        .sheet(isPresented: Binding(get: { paste != nil }, set: { if !$0 { paste = nil } })) { pasteSheet }
    }

    private func shown(_ kind: String?, reach: String) -> [JSON] {
        let q = query.lowercased()
        return store.libraryItems.filter { i in
            (kind == nil || i["kind"].str == kind) && i["reach"].str == reach
                && (q.isEmpty || "\(i["name"].str) \(i["description"].str)".lowercased().contains(q))
        }
    }

    /// GitHub sources with new commits, as a notice over the page rather than a tab of its own.
    @ViewBuilder private var updatesNotice: some View {
        let n = store.sourceUpdates.count
        if n > 0 {
            HStack(spacing: 10) {
                Image(systemName: "arrow.triangle.2.circlepath").foregroundStyle(Tone.blue.color)
                Text(L.text("\(n) 个 GitHub 来源有更新")).font(.callout)
                Spacer()
                Button(L.text("查看")) { showSources = true }.buttonStyle(.link)
                Button(L.text("全部更新")) { Task { await store.updateSources() } }.buttonStyle(.primary)
            }
            .padding(10)
            .background(Tone.blue.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
        }
    }

    // MARK: Skill router

    /// Skills for everyone: each loads directly up to `SKILL_ROUTE_OVER`, past it one
    /// `janus-skills` lists them and the agent reads the one it needs.
    // @project-doc docs/domains/20261116-resource_library.md#skill_router
    @ViewBuilder private var skillRouter: some View {
        let all = store.libraryItems.filter { $0["kind"].str == "skill" && $0["reach"].str == "everyone" }.count
        router(
            icon: "sparkles", title: L.text("Skill 路由"), count: all,
            line: all > Self.skillRouteOver
                ? L.text("\(all) 个 Skill 合并成一个 janus-skills：提示词里只有一条描述，Agent 用得上时再读对应的 SKILL.md。")
                : L.text("每个 Skill 直接交给会话（不超过 \(Self.skillRouteOver) 个时）；更多时合并成一个 janus-skills，按需读取。")
        ) {
            Button(L.text("从文件夹…")) { scanFolder() }.disabled(folderBusy)
            Button(L.text("新建")) { skillDraft = ["name": "", "description": "", "content": ""] }
        } content: {
            githubLink
            if folderBusy { ProgressView().controlSize(.small) }
            if let preview = folderPreview {
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text(preview["path"].str).fontWeight(.semibold).textSelection(.enabled)
                        Spacer()
                        Button(L.text("取消")) { folderPreview = nil }.buttonStyle(.link)
                    }
                    offerSelection(preview, imported: importedNames(preview), selection: $folderPicks) { importFolder(preview, $0) }
                }
                .padding(10)
                .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
                .disabled(folderBusy)
            }
            sourcesList
            let list = shown("skill", reach: "everyone")
            if list.isEmpty {
                empty(query.isEmpty ? L.text("还没有给所有数字员工的 Skill。拖入 GitHub 链接、从文件夹复制，或新建一个。") : L.text("没有匹配的 Skill"))
            } else { rows(list) }
        }
    }

    /// `SKILL_ROUTE_OVER` in `backend/src/library/deliver.rs`.
    static let skillRouteOver = 10

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
        .padding(.horizontal, 10).padding(.vertical, 8)
        .background(dropping ? Color.accent.opacity(0.08) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(dropping ? Color.accent : Color.line, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        .dropDestination(for: URL.self) { urls, _ in
            guard let u = urls.first, !adding else { return false }
            link = u.absoluteString
            addSource()
            return true
        } isTargeted: { dropping = $0 }
        .help(L.text("Janus 拉取仓库，列出其中的 Skill（SKILL.md），由你选择导入哪些。只读取，不执行仓库中的任何文件。导入的 Skill 默认给所有数字员工，第三方内容请先看过再导入。"))
    }

    /// The GitHub sources skills came from: what each offers, and updating or removing it.
    @ViewBuilder private var sourcesList: some View {
        let sources = store.librarySources
        if !sources.isEmpty {
            DisclosureGroup(isExpanded: $showSources) {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Button(checking ? L.text("正在检查…") : L.text("检查更新")) {
                            checking = true
                            Task { await store.checkSources(); checking = false }
                        }
                        .disabled(checking)
                        Spacer()
                        Text(L.text("服务每 6 小时检查一次")).font(.caption).foregroundStyle(.secondary)
                    }
                    ForEach(sources, id: \.self) { source($0) }
                }
                .padding(.top, 6)
            } label: {
                HStack(spacing: 6) {
                    Text(L.text("GitHub 来源 \(sources.count)")).font(.callout.weight(.medium))
                    if !store.sourceUpdates.isEmpty { Pill(L.text("\(store.sourceUpdates.count) 个有更新"), .blue) }
                }
            }
        }
    }

    private func source(_ s: JSON) -> some View {
        let id = s["id"].str
        let imported = importedNames(s)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(s["url"].str).fontWeight(.semibold).textSelection(.enabled)
                    Text(L.text("提交 \(String(s["commit"].str.prefix(7))) · \(s["offers"].array.count) 项可导入 · 已导入 \(imported.count) 项")).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if store.updatingSources.contains(id) { ProgressView().controlSize(.small) }
                else if s["updateAvailable"].truthy { Pill(L.text("有更新"), .blue); Button(L.text("更新")) { Task { await store.updateSources([id]) } } }
                Button(L.text("删除…")) { removeSource(s) }.buttonStyle(.link)
            }
            if s["checkError"].truthy { Note(text: L.text("检查更新失败：\(s["checkError"].str)"), tone: .amber) }
            offerSelection(s, imported: imported, selection: Binding(get: { picks[id] ?? [] }, set: { picks[id] = $0 })) {
                importOffers(id, $0)
            }
        }
        .padding(10)
        .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
    }

    // MARK: MCP router

    /// Servers for everyone: the command ones run behind the run's `janus-tools`, started on their
    /// first call in the run's working directory; HTTP and SSE ones go to each session directly.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    @ViewBuilder private var mcpRouter: some View {
        let all = store.libraryItems.filter { $0["kind"].str == "mcp" && $0["reach"].str == "everyone" }.count
        router(
            icon: "wrench.and.screwdriver", title: L.text("MCP 路由"), count: all,
            line: L.text("会话只多一个 janus-tools：命令启动的服务器在第一次调用时才在任务的工作目录里启动，同一个任务的数字员工共用。HTTP、SSE 服务器直接交给会话。")
        ) {
            Button(L.text("粘贴配置…")) { paste = "" }.help(L.text("Claude 的 mcpServers、VS Code 的 servers，或一个带 name 的服务器"))
            Button(L.text("添加服务器")) { mcpDraft = ["name": "", "description": "", "transport": "stdio", "command": "", "url": "", "vars": []] }
        } content: {
            let list = shown("mcp", reach: "everyone")
            if list.isEmpty {
                empty(query.isEmpty ? L.text("还没有给所有数字员工的 MCP 服务器。新添加的服务器先收口，在下面「只给选中的数字员工」里打开开关放开。") : L.text("没有匹配的服务器"))
            } else { rows(list) }
            Text(L.text("环境变量和 HTTP header 的值只保存在 Janus home，设置后不再显示，也不会进入数字员工、数据库或任务记录。")).font(.caption).foregroundStyle(.secondary)
        }
    }

    // MARK: Kept

    /// Items kept for the digital workers whose role picks them, folded away until wanted.
    @ViewBuilder private var kept: some View {
        let list = shown(nil, reach: "picked")
        let total = store.libraryItems.filter { $0["reach"].str == "picked" }.count
        DisclosureGroup(isExpanded: Binding(get: { showKept || (!query.isEmpty && !list.isEmpty) }, set: { showKept = $0 })) {
            VStack(alignment: .leading, spacing: 8) {
                Text(L.text("这些资源只给在「数字员工」里选中它的员工，照常在会话启动时逐个交给 Agent。打开开关就给所有数字员工。")).font(.caption).foregroundStyle(.secondary)
                if list.isEmpty { empty(total == 0 ? L.text("没有收口的资源") : L.text("没有匹配的资源")) } else { rows(list) }
            }
            .padding(.top, 6)
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "lock").foregroundStyle(.secondary)
                Text(L.text("只给选中的数字员工")).font(.headline)
                Pill("\(total)")
            }
        }
    }

    // MARK: Pieces

    private func router(icon: String, title: String, count: Int, line: String, @ViewBuilder actions: () -> some View, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: icon).foregroundStyle(Color.accent)
                Text(title).font(.display(.title3))
                Pill(L.text("\(count) 个给所有数字员工"), count > 0 ? .green : .gray)
                Spacer()
                actions()
            }
            Text(line).font(.callout).foregroundStyle(.secondary)
            content()
        }
        .padding(16)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
    }

    private func empty(_ text: String) -> some View {
        Text(text).font(.callout).foregroundStyle(.secondary).padding(.vertical, 4)
    }

    private func rows(_ list: [JSON]) -> some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                Text(L.text("给所有数字员工")).font(.caption).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 10).padding(.bottom, 2)
            ForEach(list, id: \.self) { i in Divider(); row(i) }
        }
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
                        Image(systemName: mcp ? "wrench.and.screwdriver" : "sparkles").foregroundStyle(.secondary).frame(width: 16)
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
        let lines = [mcp ? Library.endpoint(i) : "", i["description"].str, Library.origin(i), vars.text].filter { !$0.isEmpty }
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
        .padding(.leading, 44)
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

    private var pasteSheet: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(L.text("粘贴 MCP 配置")).font(.display(.title3))
            Text(L.text("支持 Claude 的 {\"mcpServers\": {…}}、VS Code 的 {\"servers\": {…}}，或一个带 name 的服务器对象。配置中写的值存进 Janus home，名字被占用时自动加后缀。")).font(.caption).foregroundStyle(.secondary)
            TextEditor(text: Binding(get: { paste ?? "" }, set: { paste = $0 }))
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 220)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
            HStack {
                Spacer()
                Button(L.text("取消")) { paste = nil }.keyboardShortcut(.cancelAction)
                Button(L.text("导入")) { importPasted() }.buttonStyle(.primary).disabled((paste ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(18)
        .frame(width: 560)
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
        Task {
            do {
                let full = try await store.api.get("/api/library/items/\(i["id"].str)")
                if full["kind"].str == "skill" {
                    skillDraft = ["id": full["id"], "name": full["name"], "content": full["content"]]
                } else {
                    let s = full["server"]
                    let command = ([s["command"].str] + s["args"].strings).joined(separator: " ")
                    let vars: [JSON] = full["vars"].array.map { ["name": $0["name"], "value": "", "set": $0["set"]] }
                    mcpDraft = ["id": full["id"], "name": full["name"], "description": full["description"], "transport": s["transport"], "command": JSON(command), "url": s["url"], "vars": .array(vars)]
                }
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
        guard !folderBusy else { return }
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.prompt = L.text("查找 Skill")
        p.message = L.text("选择一个 Skill 文件夹，或包含多个 Skill 的父文件夹")
        guard p.runModal() == .OK, let u = p.url else { return }
        folderBusy = true
        Task {
            defer { folderBusy = false }
            do {
                let preview = try await store.api.post("/api/library/skills/scan", ["path": JSON(u.path)])
                let imported = importedNames(preview)
                folderPicks = Set(preview["offers"].array.map { $0["path"].str }.filter { imported[$0] == nil })
                folderPreview = preview
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importFolder(_ preview: JSON, _ paths: [String]) {
        guard !folderBusy else { return }
        folderBusy = true
        Task {
            defer { folderBusy = false }
            do {
                let result = try await store.api.post("/api/library/skills/import", ["path": preview["path"], "paths": JSON(paths)])
                let imported = result["imported"].array
                if result["warnings"].strings.isEmpty {
                    folderPreview = nil
                } else {
                    var updated = preview
                    updated["imported"] = .array(preview["imported"].array + imported)
                    updated["warnings"] = result["warnings"]
                    folderPreview = updated
                }
                await store.loadLibrary()
                if !imported.isEmpty {
                    store.show(L.text("已导入 \(imported.map { $0["name"].str }.joined(separator: "、"))"))
                }
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importPasted() {
        let text = paste ?? ""
        Task {
            do {
                let r = try await store.api.post("/api/library/mcp/import", ["text": JSON(text)])
                paste = nil
                await store.loadLibrary()
                let names = r["items"].array.map { $0["name"].str }.joined(separator: L.text("、"))
                store.show(r["warnings"].strings.isEmpty ? L.text("已添加 \(names)") : L.text("已添加 \(names)；跳过：\(r["warnings"].strings.joined(separator: L.text("；")))"))
            } catch { store.show(error.localizedDescription) }
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
                showSources = true
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

/// An MCP server: how to start or reach it, and the names of its variables (stdio) or headers
/// (http, sse). A value left empty keeps the one already saved; saved values are never shown.
struct McpEditor: View {
    let store: AppStore
    let draft: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var d: JSON = [:]
    @Local private var saving = false

    var body: some View {
        let stdio = d["transport"].str == "stdio"
        VStack(alignment: .leading, spacing: 10) {
            Text(d["id"].truthy ? L.text("编辑 MCP 服务器") : L.text("添加 MCP 服务器")).font(.display(.title3))
            HStack {
                TextField(L.text("名称，例如 github"), text: field("name")).textFieldStyle(.roundedBorder).frame(width: 200)
                Picker(L.text("传输方式"), selection: field("transport")) {
                    Text(L.text("stdio（命令）")).tag("stdio")
                    Text("HTTP").tag("http")
                    Text("SSE").tag("sse")
                }
                .labelsHidden()
                .frame(width: 150)
            }
            TextField(L.text("说明（可选）"), text: field("description")).textFieldStyle(.roundedBorder)
            if stdio {
                TextField(L.text("启动命令，例如 npx -y @modelcontextprotocol/server-github"), text: field("command"))
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
            } else {
                TextField(L.text("地址，例如 https://example.com/mcp"), text: field("url"))
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                if d["transport"].str == "sse" { Text(L.text("Codex 不支持 SSE 传输，绑定 Codex 的数字员工选择它时会被跳过。")).font(.caption).foregroundStyle(Tone.amber.color) }
            }
            HStack {
                Text(stdio ? L.text("环境变量") : "HTTP header").font(.callout.weight(.semibold))
                Spacer()
                Button(L.text("添加")) { d["vars"] = .array(d["vars"].array + [["name": "", "value": "", "set": false]]) }.buttonStyle(.link)
            }
            ForEach(d["vars"].array.indices, id: \.self) { i in
                HStack {
                    TextField(stdio ? L.text("名称，例如 GITHUB_TOKEN") : L.text("名称，例如 Authorization"), text: varField(i, "name"))
                        .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)).frame(width: 220)
                    SecureField(d["vars"][i]["set"].truthy ? L.text("已设置，留空保留") : L.text("值"), text: varField(i, "value")).textFieldStyle(.roundedBorder)
                    Button { var v = d["vars"].array; v.remove(at: i); d["vars"] = .array(v) } label: { Image(systemName: "minus.circle") }
                        .buttonStyle(.borderless).help(L.text("移除这个变量和它的值"))
                }
            }
            Text(L.text("值只保存在 Janus home（0600），会话启动时交给 Agent，不会再显示。")).font(.caption).foregroundStyle(.secondary)
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

    private func varField(_ i: Int, _ k: String) -> Binding<String> {
        Binding(get: { d["vars"][i][k].str }, set: { v in
            var list = d["vars"].array
            guard i < list.count else { return }
            list[i][k] = JSON(v)
            d["vars"] = .array(list)
        })
    }

    private func save() {
        saving = true
        Task {
            defer { saving = false }
            do {
                let vars: [JSON] = d["vars"].array.filter { !$0["name"].str.isEmpty }.map { ["name": $0["name"], "value": $0["value"]] }
                let body: JSON = ["name": d["name"], "description": d["description"], "transport": d["transport"], "command": d["command"], "url": d["url"], "vars": .array(vars)]
                let item = d["id"].truthy
                    ? try await store.api.put("/api/library/mcp/\(d["id"].str)", body)
                    : try await store.api.post("/api/library/mcp", body)
                await store.loadLibrary()
                store.show(L.text("已保存 \(item["name"].str)"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}
