import AppKit
import SwiftUI

/// Digital workers: who uses which model, with which instructions and tools, listed by group.
/// A group's menu renames it, puts it on one Agent or exports it as role files; 「导入…」 reads
/// role files back, and 招聘顾问 designs a group in a chat.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_groups
struct RolesSection: View {
    let store: AppStore
    /// The group being renamed, and its new name.
    @Local private var renaming: String?
    @Local private var newName = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(L.text("数字员工")).font(.display(.title3))
                    Text(L.text("给每位数字员工配 Agent、模型和指令；资源库全员可用。")).font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                if store.role(AppStore.recruiter) != nil {
                    Button(L.text("找招聘顾问…")) { Task { await store.chatWithRecruiter() } }
                        .help(L.text("和招聘顾问聊聊你要做的事，它为你设计一组数字员工，聊完在对话里导入"))
                }
                Button(L.text("导入…")) { Task { await store.openRoleImport("") } }
                    .help(L.text("从文件夹或 .md 文件导入数字员工"))
                Button(L.text("新建数字员工")) { store.newRole() }.buttonStyle(.primary)
            }
            if store.okConnections.isEmpty && !store.connections.isEmpty {
                Note(text: L.text("还没有通过检查的 Agent。可以先编辑数字员工，Agent 通过检查后才能运行。"), tone: .amber)
            }
            if store.roles.isEmpty {
                Card(padding: 20) {
                    Text(L.text("还没有数字员工")).font(.display(.title3))
                    Text(L.text("先建一个负责人，再来一个通用数字员工，团队就能开工。")).foregroundStyle(.secondary)
                    HStack {
                        Button(L.text("新建数字员工")) { store.newRole() }.buttonStyle(.primary)
                        Button(L.text("导入…")) { Task { await store.openRoleImport("") } }
                    }
                }
            } else {
                ForEach(store.roleSections, id: \.group) { section in group(section.group, section.roles) }
            }
        }
        .alert(L.text("重命名分组"), isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField(L.text("分组名"), text: $newName)
            Button(L.text("取消"), role: .cancel) { renaming = nil }
            Button(L.text("重命名")) {
                if let from = renaming { Task { await store.renameRoleGroup(from, to: newName) } }
                renaming = nil
            }
        }
    }

    private func group(_ g: String, _ members: [Role]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(AppStore.groupTitle(g)).font(.headline)
                Text(verbatim: "\(members.count)").font(.caption).foregroundStyle(.secondary).monospacedDigit()
                Spacer()
                Menu {
                    Menu(L.text("整组改用 Agent")) {
                        ForEach(store.connections, id: \.self) { c in
                            Button(c["name"].str) { Task { await store.bindRoleGroup(g, connection: c["id"].str) } }
                        }
                    }
                    .disabled(store.connections.isEmpty)
                    if !g.isEmpty {
                        Button(L.text("重命名…")) { newName = g; renaming = g }
                        Button(L.text("导出为文件…")) { Task { await store.exportRoleGroup(g) } }
                    }
                } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help(L.text("\(AppStore.groupTitle(g)) 的整组操作"))
                .accessibilityLabel(L.text("\(AppStore.groupTitle(g)) 的整组操作"))
            }
            VStack(spacing: 0) {
                ForEach(members, id: \.self) { r in row(r); Divider() }
            }
            .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
        }
        .padding(.top, 4)
    }

    private func row(_ r: Role) -> some View {
        let c = store.connection(r.connection)
        let k = c.flatMap { store.kind($0["kind"].str) }
        let facts = [Setup.permLabel(r.session, c), r.prompt.isEmpty ? "" : L.text("提示词")].filter { !$0.isEmpty }
        return Button { store.editRole(r.json) } label: {
            HStack(spacing: 12) {
                Avatar(initials: r.initials.isEmpty ? String(r.name.prefix(2)).uppercased() : r.initials, color: r.color, size: 30, role: r.id)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) { Text(r.name).fontWeight(.semibold); if r.lead { Pill(L.text("负责人"), .violet) } }
                    Text(([r.desc.isEmpty ? L.text("还没有填写职责") : r.desc] + facts).joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if let c {
                    HStack(spacing: 6) {
                        Avatar(initials: k?["init"].str ?? "?", color: k?["color"].str ?? "#64748b", size: 18)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(c["name"].str).font(.callout.weight(.medium))
                            Text(verbatim: "\(!r.model.isEmpty ? r.model : c["options"]["model"]["current"].truthy ? c["options"]["model"]["current"].str : L.text("默认模型"))\(r.effort.isEmpty ? "" : L.text("，\(r.effort)"))").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .frame(width: 200, alignment: .leading)
                } else {
                    Text(L.text("未选择 Agent")).foregroundStyle(Tone.amber.color).frame(width: 200, alignment: .leading)
                }
                Image(systemName: "chevron.right").foregroundStyle(.tertiary)
            }
            .padding(12)
            .contentShape(Rectangle())
            .hoverable(0)
        }
        .buttonStyle(.plain)
        .help(L.text("编辑 \(r.name)"))
    }

    /// "最近 27 个任务：10 个子任务 · 首次通过 70% · 验证失败 2 · 成功平均 5.2 分钟"
    static func statLine(_ x: JSON, runs: Int) -> String? {
        guard let tasks = x["tasks"].int, tasks > 0 else { return nil }
        let pct = { (n: Int) in "\(Int((Double(n) / Double(tasks) * 100).rounded()))%" }
        let parts = [L.text("\(tasks) 个子任务"), L.text("首次通过 \(pct(x["firstPass"].int ?? 0))"), (x["failed"].int ?? 0) > 0 ? L.text("失败 \(x["failed"].str)") : "", (x["verificationFailed"].int ?? 0) > 0 ? L.text("验证没通过 \(x["verificationFailed"].str)") : "", x["avgMinutes"].truthy ? L.text("成功平均 \(x["avgMinutes"].str) 分钟") : ""].filter { !$0.isEmpty }
        return L.text("最近 \(runs) 个任务：\(parts.joined(separator: " · "))")
    }
}

/// One column, top to bottom, the way the role is read.
struct RoleEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var r: JSON = [:]
    @Local private var saving = false
    @Local private var colors = false
    /// The picture chosen here, saved with the role.
    @Local private var picture = PictureChange.unchanged

    enum PictureChange: Equatable {
        case unchanged
        case set(Data)
        case remove
    }

    var body: some View {
        let c = store.connection(r["connection"].str)
        let opt = c?["options"] ?? .null
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Button { colors.toggle() } label: {
                    Avatar(initials: r["init"].truthy ? r["init"].str : String(r["name"].str.replacingOccurrences(of: " ", with: "").prefix(2)).uppercased(), color: r["color"].str, size: 34,
                           role: picture == .remove ? "" : r["id"].str, picture: chosen)
                }
                .buttonStyle(.plain).help(L.text("选颜色，或拖张图片当头像")).accessibilityLabel(L.text("数字员工头像"))
                .dropDestination(for: URL.self) { urls, _ in choose(urls.first) }
                .popover(isPresented: $colors) {
                    VStack(alignment: .leading, spacing: 10) {
                    Text(L.text("没有头像图片时，显示这个颜色和文字。")).font(.caption).foregroundStyle(.secondary)
                    HStack(spacing: 8) {
                        ForEach(AppStore.roleColors, id: \.self) { col in
                            Button { r["color"] = JSON(col); colors = false } label: {
                                Circle().fill(Color(hex: col)).frame(width: 22, height: 22)
                                    .overlay(Circle().strokeBorder(Color.primary, lineWidth: r["color"].str == col ? 2 : 0).padding(-3))
                                    .padding(3)
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(col)
                            .accessibilityAddTraits(r["color"].str == col ? .isSelected : [])
                        }
                    }
                    }
                    .padding(12)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(r["id"].truthy ? L.text("编辑数字员工") : L.text("新建数字员工")).font(.display(.title3))
                    HStack(spacing: 12) {
                        Button(L.text("更换头像…")) { pickPicture() }
                            .help(L.text("选张图片当头像，会裁成正方形；也可以直接拖进来"))
                        if hasPicture { Button(L.text("移除头像")) { picture = .remove } }
                    }
                    .buttonStyle(.link).font(.caption)
                    if let line = RolesSection.statLine(store.roleStats["roles"][r["id"].str], runs: store.roleStats["runs"].int ?? 0) {
                        Text(line).font(.caption).foregroundStyle(.secondary).help(L.text("首次通过：第一次尝试就完成，且验证一次通过"))
                    }
                }
                Spacer()
            }
            .padding(16)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if store.offersRecruiter(r) {
                        // The shortcut from adding someone by hand to having them designed in a chat.
                        HStack(spacing: 10) {
                            Avatar(initials: "招聘", color: store.role(AppStore.recruiter)?.color ?? "#c2410c", size: 26, role: AppStore.recruiter)
                            Text(L.text("不知道怎么写职责和指令？和招聘顾问聊聊你要做的事，它帮你设计好再导入。"))
                                .font(.callout).fixedSize(horizontal: false, vertical: true)
                            Spacer()
                            Button(L.text("找招聘顾问")) { recruit() }
                        }
                        .padding(10)
                        .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
                    }
                    labeled(L.text("名称")) { TextField("Reviewer", text: bind("name")).textFieldStyle(.roundedBorder) }
                    labeled(L.text("职责")) {
                        TextField(L.text("独立检查变更，寻找风险与反例"), text: bind("desc")).textFieldStyle(.roundedBorder)
                        Text(L.text("写它擅长什么、交付什么，负责人据此派活。")).font(.caption).foregroundStyle(.secondary)
                    }
                    seedUpdate("desc", L.text("默认职责有新版本。你改过这一项，所以升级时没有替换。"))
                    labeled(L.text("分组")) {
                        HStack(spacing: 6) {
                            TextField(L.text("不分组"), text: bind("group")).textFieldStyle(.roundedBorder)
                            Menu {
                                ForEach(store.roleGroups.filter { !$0.isEmpty }, id: \.self) { g in Button(g) { r["group"] = JSON(g) } }
                            } label: { Image(systemName: "chevron.down") }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                            .disabled(store.roleGroups.allSatisfy(\.isEmpty))
                            .help(L.text("选一个已有的分组"))
                            .accessibilityLabel(L.text("选一个已有的分组"))
                        }
                        Text(L.text("按团队或行业归类，只用来管理，负责人看不到。")).font(.caption).foregroundStyle(.secondary)
                    }
                    Toggle(isOn: Binding(get: { r["lead"].truthy }, set: { r["lead"] = JSON($0) })) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(L.text("可以担任负责人"))
                            Text(Setup.leadCapable(c) ? L.text("用 /goal 推进目标，需要时编排协作者") : L.text("绑定 Agent 后才能担任负责人")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .toggleStyle(.switch)
                    .disabled(!Setup.leadCapable(c))

                    section(L.text("指令"))
                    TextEditor(text: bind("prompt"))
                        .font(.body)
                        .accessibilityLabel(L.text("指令"))
                        .frame(minHeight: 120, maxHeight: 260)
                        .overlay(alignment: .topLeading) {
                            if !r["prompt"].truthy { Text(L.text("告诉这个数字员工怎么做事，例如：先读相关代码和已有测试；重点检查边界条件、错误处理和测试覆盖。")).foregroundStyle(.tertiary).padding(6).allowsHitTesting(false) }
                        }
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    Text(L.text("作为系统提示词交给 Agent，整个会话都有效。")).font(.caption).foregroundStyle(.secondary)
                    seedUpdate("prompt", L.text("默认指令有新版本。你改过这一项，所以升级时没有替换。"))

                    section(L.text("由谁来做"))
                    HStack(alignment: .top, spacing: 12) {
                        labeled("Agent") {
                            Picker("Agent", selection: Binding(get: { r["connection"].str }, set: { r["connection"] = JSON($0); r["model"] = ""; r["effort"] = ""; r["session"] = [:] })) {
                                Text(L.text("选择 Agent")).tag("")
                                ForEach(store.connections, id: \.self) { x in Text(x["name"].str).tag(x["id"].str) }
                            }
                            .labelsHidden()
                        }
                        labeled(L.text("模型")) {
                            if c == nil { Picker(L.text("模型"), selection: .constant("")) { Text(L.text("先选择 Agent")).tag("") }.labelsHidden().disabled(true) }
                            else if !opt["model"]["options"].array.isEmpty {
                                Picker(L.text("模型"), selection: bind("model")) {
                                    Text(verbatim: L.text("默认（\(opt["model"]["current"].str)）")).tag("")
                                    ForEach(opt["model"]["options"].array, id: \.self) { o in Text(o["name"].truthy ? o["name"].str : o["value"].str).tag(o["value"].str) }
                                }
                                .labelsHidden()
                            } else if c?["status"].str == "testing" { Text(L.text("读取模型中…")).foregroundStyle(.secondary) }
                            else { Button(c?["status"].str == "error" ? L.text("检查没通过，再查一次") : L.text("检查 Agent 以读取模型")) { Task { await store.testConnection(c!["id"].str) } }.buttonStyle(.link) }
                        }
                        labeled(L.text("推理强度")) {
                            if !opt["effort"]["options"].array.isEmpty {
                                Picker(L.text("推理强度"), selection: bind("effort")) {
                                    Text(verbatim: L.text("默认（\(opt["effort"]["current"].str)）")).tag("")
                                    ForEach(opt["effort"]["options"].array, id: \.self) { o in Text(o["name"].truthy ? o["name"].str : o["value"].str).tag(o["value"].str) }
                                }
                                .labelsHidden()
                            } else { Picker(L.text("推理强度"), selection: .constant("")) { Text(L.text("默认")).tag("") }.labelsHidden().disabled(true) }
                        }
                    }

                    section(L.text("权限"), trailing: Setup.controls(c).contains { $0["permission"].truthy } ? L.text("\(c?["name"].str ?? "") 的权限模式") : "")
                    permissions(c)
                    if c?["options"]["controls"].exists == true {
                        Text(L.text("要不要先问你，由 Agent 自己判断；它发来的审批、表单和链接都进「待处理」。")).font(.caption).foregroundStyle(.secondary)
                    }
                    otherControls(c)
                }
                .padding(18)
            }
            Divider()
            HStack {
                if r["id"].truthy { Button(L.text("删除…"), role: .destructive) { remove() } }
                Spacer()
                Button(L.text("取消")) { if r == original || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                Button(saving ? L.text("正在保存…") : r["id"].truthy ? L.text("保存") : L.text("创建")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction)
                    .disabled(saving || r["name"].str.trimmingCharacters(in: .whitespaces).isEmpty)
                    .help(r["name"].str.trimmingCharacters(in: .whitespaces).isEmpty ? L.text("先填写名称") : "")
            }
            .padding(14)
        }
        .frame(minWidth: 620, idealWidth: 680, minHeight: 480, idealHeight: 760)
        .onAppear { r = original }
        .task { await store.loadLibrary() }
    }

    private func bind(_ k: String) -> Binding<String> {
        Binding(get: { r[k].str }, set: { v in r[k] = JSON(v); if k == "name" && !r["id"].truthy { r["init"] = "" } })
    }

    /// A default digital employee whose `field` the person edited, when Janus ships newer default
    /// text for it: show the new text and let them take it or keep theirs.
    @ViewBuilder private func seedUpdate(_ field: String, _ note: String) -> some View {
        if r["id"].truthy, let text = r["seedUpdates"][field].string {
            VStack(alignment: .leading, spacing: 8) {
                Text(note).font(.callout)
                DisclosureGroup(L.text("查看新版")) {
                    Text(text).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                }
                // A sheet: the style set on the main window does not reach it.
                .disclosureGroupStyle(.row)
                HStack {
                    Button(L.text("采用新版")) { chooseSeed(field, adopt: true) }.buttonStyle(.primary)
                    Button(L.text("保留我的")) { chooseSeed(field, adopt: false) }
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Tone.blue.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func chooseSeed(_ field: String, adopt: Bool) {
        Task {
            do {
                let saved = try await store.seedChoice(r["id"].str, field, adopt: adopt)
                r[field] = saved[field]
                r["seed"] = saved["seed"]
                var updates = r["seedUpdates"].object
                updates[field] = nil
                r["seedUpdates"] = .object(updates)
                store.show(adopt ? L.text("已采用新版") : L.text("已保留你的版本"))
            } catch {
                store.show(error.localizedDescription)
            }
        }
    }

    private func labeled<C: View>(_ label: String, @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 5) { Text(label).font(.callout.weight(.semibold)); content() }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private func section(_ t: String, trailing: String = "", count: Int = 0, action: (String, () -> Void)? = nil) -> some View {
        HStack {
            Text(t).font(.headline)
            if count > 0 { Pill("\(count)") }
            Spacer()
            if !trailing.isEmpty { Text(trailing).font(.caption).foregroundStyle(.secondary) }
            if let action { Button(action.0, action: action.1).buttonStyle(.link) }
        }
        .padding(.top, 6)
    }

    // MARK: Session controls: whatever the agent declares

    @ViewBuilder private func permissions(_ c: JSON?) -> some View {
        if let c {
            if c["status"].str == "testing" { Note(text: L.text("正在检查 \(c["name"].str)，读取它的权限模式…")) }
            else if !c["options"]["controls"].exists {
                HStack { Note(text: L.text("还没有读取 \(c["name"].str) 的权限模式。"), tone: .amber); Button(L.text("检查 Agent")) { Task { await store.testConnection(c["id"].str) } } }
            } else {
                let perms = Setup.controls(c).filter { $0["permission"].truthy }
                if perms.isEmpty { Note(text: L.text("\(c["name"].str) 没有权限模式：工具直接执行，不经审批，能改工作目录里的文件。")) }
                ForEach(perms, id: \.self) { x in
                    let v = Setup.value(r["session"], x)
                    if x["type"].str == "boolean" {
                        Toggle(isOn: Binding(get: { v.bool ?? false }, set: { setCtrl(x, JSON($0)) })) {
                            VStack(alignment: .leading, spacing: 1) { Text(x["name"].str); if x["desc"].truthy { Text(x["desc"].str).font(.caption).foregroundStyle(.secondary) } }
                        }
                        .toggleStyle(.switch)
                    } else {
                        VStack(alignment: .leading, spacing: 6) {
                            HStack { Text(x["name"].str).fontWeight(.semibold); if x["desc"].truthy { Text(x["desc"].str).font(.caption).foregroundStyle(.secondary) } }
                            ForEach(x["options"].array, id: \.self) { o in
                                let on = o["value"] == v
                                let (icon, risky) = Setup.permIcon(o)
                                Button { setCtrl(x, o["value"]) } label: {
                                    HStack(alignment: .top, spacing: 10) {
                                        Image(systemName: icon).foregroundStyle(risky ? Tone.red.color : .secondary).frame(width: 18)
                                        VStack(alignment: .leading, spacing: 2) {
                                            HStack(spacing: 6) {
                                                Text(o["name"].truthy ? o["name"].str : o["value"].str).fontWeight(.medium)
                                                if o["value"] == x["default"] { Text(L.text("默认")).font(.caption2).foregroundStyle(.secondary) }
                                            }
                                            if o["desc"].truthy { Text(o["desc"].str).font(.caption).foregroundStyle(.secondary) }
                                        }
                                        Spacer()
                                        if on { Image(systemName: "checkmark").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                                    }
                                    .padding(9)
                                    .background(on ? Color.accentColor.opacity(0.1) : Color.secondary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
                                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Color.accentColor : Color.line.opacity(0.6)))
                                    .contentShape(Rectangle())
                                    .hoverable(8)
                                }
                                .buttonStyle(.plain)
                                .accessibilityAddTraits(on ? .isSelected : [])
                            }
                        }
                    }
                }
            }
        } else {
            Note(text: L.text("先选 Agent，权限选项由它自己声明。"))
        }
    }

    @ViewBuilder private func otherControls(_ c: JSON?) -> some View {
        let rest = Setup.controls(c).filter { !$0["permission"].truthy }
        if !rest.isEmpty {
            section(L.text("会话选项"), trailing: L.text("\(c?["name"].str ?? "") 提供"))
            ForEach(rest, id: \.self) { x in
                let v = Setup.value(r["session"], x)
                HStack {
                    VStack(alignment: .leading, spacing: 1) { Text(x["name"].str); if x["desc"].truthy { Text(x["desc"].str).font(.caption).foregroundStyle(.secondary) } }
                    Spacer()
                    if x["type"].str == "boolean" {
                        Toggle(x["name"].str, isOn: Binding(get: { v.bool ?? false }, set: { setCtrl(x, JSON($0)) })).labelsHidden().toggleStyle(.switch)
                    } else {
                        Picker(x["name"].str, selection: Binding(get: { v.str }, set: { setCtrl(x, JSON($0)) })) {
                            ForEach(x["options"].array, id: \.self) { o in Text((o["name"].truthy ? o["name"].str : o["value"].str) + (o["value"] == x["default"] ? L.text("（默认）") : "")).tag(o["value"].str) }
                        }
                        .labelsHidden().frame(width: 220)
                    }
                }
            }
        }
    }

    /// Choosing the default drops the key, so the role follows the default if it changes.
    private func setCtrl(_ x: JSON, _ v: JSON) {
        var s = r["session"].isNull ? [:] : r["session"]
        if v == x["default"] { var o = s.object; o[x["id"].str] = nil; s = .object(o) } else { s[x["id"].str] = v }
        r["session"] = s
    }

    /// The picture chosen here and not saved yet.
    private var chosen: NSImage? {
        if case .set(let png) = picture { return NSImage(data: png) }
        return nil
    }

    private var hasPicture: Bool {
        switch picture {
        case .set: true
        case .remove: false
        case .unchanged: store.role(r["id"].str).map { !$0.avatar.isEmpty } == true
        }
    }

    private func pickPicture() {
        colors = false
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.message = L.text("选张图片当头像，会裁成正方形")
        if panel.runModal() == .OK { _ = choose(panel.url) }
    }

    /// Take an image file as the picture: cropped square and scaled down here, saved with the role.
    @discardableResult
    private func choose(_ url: URL?) -> Bool {
        guard let url, let data = try? Data(contentsOf: url), let png = AvatarImages.square(data) else {
            if url != nil { store.show(L.text("这个文件不是图片")) }
            return false
        }
        picture = .set(png)
        return true
    }

    private func save() {
        saving = true
        Task {
            do {
                let saved = try await store.save(.roles, r)
                switch picture {
                case .set(let png): try await store.setAvatar(saved["id"].str, png: png)
                case .remove: try await store.setAvatar(saved["id"].str, png: nil)
                case .unchanged: break
                }
                store.show(L.text("已保存数字员工 \(saved["name"].str)"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    /// Leave the new employee for a chat with 招聘顾问, asking first if something was typed.
    private func recruit() {
        guard r == original || confirmDiscard() else { return }
        Task { await store.chatWithRecruiter() }
    }

    private func remove() {
        guard confirm(L.text("删除数字员工「\(r["name"].str)」？"), ok: L.text("删除")) else { return }
        Task {
            do { try await store.delete(.roles, r["id"].str); dismiss() } catch { store.show(error.localizedDescription) }
        }
    }
}

/// A card's runtime line: the version Janus starts this kind of Agent with, and a quiet menu back
/// to an installed version. Janus keeps it up to date in the background, so the line stays out of
/// the way; only an update that did not finish asks for a look.
// @project-doc docs/architecture/20261031-adapters.md#api_shape
struct AdapterLine: View {
    let store: AppStore
    let a: JSON
    @Local private var why = false
    /// Ticks when "已自动更新" runs out, so the word goes without another change on the page.
    @Local private var tick = false

    private var id: String { a["id"].str }
    private var running: Bool { AdapterText.running(a) }
    private var state: AdapterText.State {
        _ = tick
        return AdapterText.state(a, now: L.nowMs(), seen: store.autoUpdateSeen)
    }

    var body: some View {
        HStack(spacing: 8) {
            info
            if state == .failed || state == .installFailed {
                let failed = state == .installFailed
                Text(failed ? L.text("安装没成功") : L.text("更新没成功"))
                    .font(.caption).foregroundStyle(Tone.red.color)
                    .accessibilityHidden(true)
                Button(L.text("查看原因")) { why.toggle() }
                    .buttonStyle(.link).font(.caption)
                    // One element to reach and open the popover from the keyboard or VoiceOver.
                    .accessibilityLabel(failed ? L.text("安装没成功，查看原因") : L.text("更新没成功，查看原因"))
                    .popover(isPresented: $why) { reason }
            }
            Spacer(minLength: 8)
            if AdapterText.showsMenu(a) {
                menu
            } else if state == .notInstalled || state == .installFailed {
                Button(L.text("安装")) { Task { await store.adapterAction(id, .install) } }
                    .buttonStyle(.link).font(.caption).disabled(running)
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 6)
        .help(hover)
        .task(id: AdapterText.autoUpdatedKey(a)) {
            guard let until = AdapterText.autoUpdatedUntil(a) else { return }
            let wait = until - L.nowMs()
            guard wait > 0 else { return }
            try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000))
            if !Task.isCancelled { tick.toggle() }
        }
        .onDisappear { store.acknowledgeAutoUpdate(a) }
    }

    /// The label, the version and the one short word that belongs with them. A failure is a button
    /// of its own, so it stays outside this group.
    private var info: some View {
        HStack(spacing: 8) {
            Image(systemName: "shippingbox").font(.caption).foregroundStyle(.tertiary)
            if a["current"].truthy {
                Text(L.text("运行组件")).font(.caption).foregroundStyle(.secondary)
                Text(a["current"].str).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                if let word {
                    Text(word).font(.caption).foregroundStyle(.secondary)
                }
                if a["pinned"].truthy { Text(L.text("不会自动更新")).font(.caption).foregroundStyle(.secondary) }
            } else if let word {
                // "运行组件还没装" or "安装中…" says what the label would say, once.
                Text(word).font(.caption).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLine)
    }

    /// The state word that belongs on the line itself, if this state has one.
    private var word: String? {
        switch state {
        case .updating: return L.text("更新中…")
        case .installing: return L.text("安装中…")
        case .autoUpdated: return L.text("已自动更新")
        case .notInstalled: return L.text("运行组件还没装")
        default: return nil
        }
    }

    /// What VoiceOver reads for the row as one element. The menu is an element of its own (its
    /// action must stay reachable), so the row names it too: 「运行组件 <版本>，切回历史版本」,
    /// with 「不会自动更新」 while the version is pinned (P1 AC-16).
    private var accessibilityLine: String {
        var parts: [String]
        if a["current"].truthy {
            parts = [L.text("运行组件"), a["current"].str]
            if let word { parts.append(word) }
            if a["pinned"].truthy { parts.append(L.text("不会自动更新")) }
        } else {
            parts = [word ?? L.text("运行组件")]
        }
        if AdapterText.showsMenu(a) { parts.append(L.text("切回历史版本")) }
        return parts.joined(separator: L.text("，"))
    }

    /// The version menu: a native menu, so Tab reaches it, Space or Return opens it and Esc closes
    /// it. A plain button style keeps it grey like the rest of the line instead of brand blue.
    private var menu: some View {
        Menu {
            ForEach(AdapterText.menuItems(a), id: \.self) { item in
                switch item {
                case .resume(let latest):
                    Button(latest.isEmpty ? L.text("恢复自动更新") : L.text("恢复自动更新（\(latest)）")) {
                        Task { await store.adapterAction(id, .install) }
                    }
                    if !AdapterText.others(a).isEmpty { Divider() }
                case .switchTo(let version):
                    Button(L.text("切回 \(version)")) {
                        Task { await store.adapterAction(id, .use, version: version) }
                    }
                }
            }
        } label: {
            HStack(spacing: 3) {
                Text(L.text("切回历史版本"))
                Image(systemName: "chevron.right").font(.system(size: 8, weight: .semibold)).accessibilityHidden(true)
            }
            .font(.caption).foregroundStyle(.secondary)
            .padding(.horizontal, 4).padding(.vertical, 2)
            .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .hoverable(4)
        .disabled(running)
        .help(L.text("切到装过的版本。同一类 Agent 共用这个组件，切换后从下个会话开始用。"))
        .accessibilityLabel(L.text("切回历史版本"))
    }

    /// What a failed update or install shows when the person asks why: the server-worded reason,
    /// Janus's own recent steps, a retry and the whole log. The popover never shows the install
    /// tool's command line or output; 「查看日志」 opens the complete log instead (P1 AC-15).
    private var reason: some View {
        let recent = AdapterText.recentNotes(a)
        return VStack(alignment: .leading, spacing: 10) {
            Text(a["job"]["detail"].truthy ? a["job"]["detail"].str : L.text("没有更多信息"))
                .font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            if !recent.isEmpty {
                DisclosureGroup(L.text("最近日志")) {
                    CodeBlock(text: recent.joined(separator: "\n"), maxHeight: 160)
                }
                .disclosureGroupStyle(.row)
            }
            HStack(spacing: 14) {
                Button(state == .installFailed ? L.text("安装") : L.text("重试更新")) {
                    why = false
                    Task { await store.adapterAction(id, .install) }
                }
                .disabled(running)
                Button(L.text("查看日志")) {
                    ArtifactWindow.open(path: JanusHome.current().adapterLog(id).path, title: L.text("运行组件日志"))
                }
                .buttonStyle(.link)
            }
        }
        .padding(12)
        .frame(width: 420)
    }

    private var hover: String {
        if !a["current"].truthy { return L.text("第一次检查 Agent 时会自动装。装不上时可以在这里再来一次。") }
        if a["pinned"].truthy { return L.text("你选了这个版本，它不会被自动更新，直到你恢复自动更新。") }
        return L.text("Janus 用它启动 Agent。它会自动保持最新，正在跑的会话不受影响。")
    }
}
