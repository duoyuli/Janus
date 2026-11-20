import AppKit
import SwiftUI

/// Digital workers: who uses which model, with which instructions and tools.
struct RolesSection: View {
    let store: AppStore

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(L.text("数字员工")).font(.display(.title3))
                    Text(L.text("决定谁用什么 Agent 和模型、带什么指令做事。资源库里的 Skill 和 MCP 每个员工都有。")).font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button(L.text("新建数字员工")) { store.newRole() }.buttonStyle(.primary)
            }
            if store.okConnections.isEmpty && !store.connections.isEmpty {
                Note(text: L.text("还没有检查通过的 Agent。可以先编辑数字员工，Agent 检查通过后才能运行。"), tone: .amber)
            }
            if store.roles.isEmpty {
                Card(padding: 20) {
                    Text(L.text("还没有数字员工")).font(.display(.title3))
                    Text(L.text("数字员工决定谁做什么。先建一个负责人，再建一个通用数字员工。")).foregroundStyle(.secondary)
                    Button(L.text("新建数字员工")) { store.newRole() }.buttonStyle(.primary)
                }
            } else {
                VStack(spacing: 0) {
                    ForEach(store.roles, id: \.self) { r in row(r); Divider() }
                }
                .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
            }
        }
    }

    private func row(_ r: JSON) -> some View {
        let c = store.connection(r["connection"].str)
        let k = c.flatMap { store.kind($0["kind"].str) }
        let facts = [Setup.permLabel(r["session"], c), r["prompt"].truthy ? L.text("提示词") : ""].filter { !$0.isEmpty }
        return Button { store.editRole(r) } label: {
            HStack(spacing: 12) {
                Avatar(initials: r["init"].truthy ? r["init"].str : String(r["name"].str.prefix(2)).uppercased(), color: r["color"].str, size: 30)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) { Text(r["name"].str).fontWeight(.semibold); if r["lead"].truthy { Pill(L.text("负责人"), .violet) } }
                    Text(([r["desc"].truthy ? r["desc"].str : L.text("还没有填写职责")] + facts).joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if let c {
                    HStack(spacing: 6) {
                        Avatar(initials: k?["init"].str ?? "?", color: k?["color"].str ?? "#64748b", size: 18)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(c["name"].str).font(.callout.weight(.medium))
                            Text(verbatim: "\(r["model"].truthy ? r["model"].str : c["options"]["model"]["current"].truthy ? c["options"]["model"]["current"].str : L.text("默认模型"))\(r["effort"].truthy ? L.text("，\(r["effort"].str)") : "")").font(.caption).foregroundStyle(.secondary)
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
        .help(L.text("编辑 \(r["name"].str)"))
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

    var body: some View {
        let c = store.connection(r["connection"].str)
        let opt = c?["options"] ?? .null
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Button { colors.toggle() } label: {
                    Avatar(initials: r["init"].truthy ? r["init"].str : String(r["name"].str.replacingOccurrences(of: " ", with: "").prefix(2)).uppercased(), color: r["color"].str, size: 34)
                }
                .buttonStyle(.plain).help(L.text("选择颜色")).accessibilityLabel(L.text("数字员工颜色"))
                .popover(isPresented: $colors) {
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
                    .padding(12)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(r["id"].truthy ? L.text("编辑数字员工") : L.text("新建数字员工")).font(.display(.title3))
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
                    labeled(L.text("名称")) { TextField("Reviewer", text: bind("name")).textFieldStyle(.roundedBorder) }
                    labeled(L.text("职责")) {
                        TextField(L.text("独立检查变更，寻找风险与反例"), text: bind("desc")).textFieldStyle(.roundedBorder)
                        Text(L.text("负责人会根据这段描述，把合适的子任务交给它。写下它擅长什么、负责交付什么。")).font(.caption).foregroundStyle(.secondary)
                    }
                    seedUpdate("desc", L.text("默认职责有新版本。你改过这一项，所以升级时没有替换。"))
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
                    Text(L.text("这段指令会作为系统提示词交给 Coding Agent，在整个会话中持续生效。")).font(.caption).foregroundStyle(.secondary)
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
                            else { Button(c?["status"].str == "error" ? L.text("检查未通过，重新检查") : L.text("检查 Agent 以读取模型")) { Task { await store.testConnection(c!["id"].str) } }.buttonStyle(.link) }
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
                        Text(L.text("Agent 自己判断哪些操作要先问你，它发起的审批、表单和链接都会进入「需要你处理」，按 Agent 给出的选项呈现。")).font(.caption).foregroundStyle(.secondary)
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
                if perms.isEmpty { Note(text: L.text("\(c["name"].str) 没有声明权限模式：工具调用直接执行，不会发起审批，可以修改工作目录中的文件。")) }
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
            Note(text: L.text("先选择 Agent。权限选项由 Agent 自己声明，每种 Agent 都不一样。"))
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

    private func save() {
        saving = true
        Task {
            do {
                let saved = try await store.save("roles", r)
                store.show(L.text("已保存数字员工 \(saved["name"].str)"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func remove() {
        guard confirm(L.text("删除数字员工「\(r["name"].str)」？"), ok: L.text("删除")) else { return }
        Task {
            do { try await store.delete("roles", r["id"].str); dismiss() } catch { store.show(error.localizedDescription) }
        }
    }
}

/// ACP adapters: installed on demand into Janus home, one directory per version.
/// A card's ACP adapter line: the version in use, upgrading, checking and switching back. Janus
/// installs it the first time an Agent that needs it is checked; changing it leaves running
/// sessions alone.
struct AdapterLine: View {
    let store: AppStore
    let a: JSON

    var body: some View {
        let job = a["job"]
        let running = job["state"].str == "running"
        let src = a["source"]["kind"].str == "npm" ? "npm · \(a["source"]["package"].str)" : "GitHub · \(a["source"]["repo"].str.replacingOccurrences(of: "https://github.com/", with: ""))"
        let prev = a["versions"].array.first { !$0["current"].truthy }
        let id = a["id"].str
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: "shippingbox").foregroundStyle(.secondary).help(src)
                Text(L.text("ACP 适配器")).font(.caption).foregroundStyle(.secondary)
                Text(a["current"].truthy ? a["current"].str : "—").font(.system(.caption, design: .monospaced))
                if running { Pill(job["action"].str == "install" ? L.text("安装中") : L.text("检查中"), .blue) }
                else if !a["current"].truthy { Pill(L.text("未安装")) }
                else if a["updateAvailable"].truthy { Pill(L.text("有新版本"), .amber) }
                Text(a["latest"].truthy ? L.text("最新 \(a["latest"].str)\(a["checkedAt"].truthy ? L.text(" · \(L.clock(a["checkedAt"].double ?? 0)) 检查") : "")") : L.text("还没有检查更新"))
                    .font(.caption).foregroundStyle(.tertiary).lineLimit(1)
                Spacer()
                if let p = prev {
                    Button(L.text("切回 \(p["version"].str)")) { Task { await store.adapterAction(id, "use", version: p["version"].str) } }
                        .buttonStyle(.link).font(.caption).disabled(running)
                }
                if !a["current"].truthy {
                    Button(L.text("安装")) { Task { await store.adapterAction(id, "install") } }.controlSize(.small).disabled(running)
                } else {
                    if a["updateAvailable"].truthy { Button(String(L.text("升级到 \(a["latest"].str)"))) { Task { await store.adapterAction(id, "install") } }.buttonStyle(.primary).controlSize(.small).disabled(running) }
                    Button(L.text("检查更新")) { Task { await store.adapterAction(id, "check") } }.controlSize(.small).disabled(running)
                }
            }
            if running {
                Text(job["action"].str == "install" ? L.text("正在下载和构建，完成后自动设为当前版本…") : L.text("正在查询最新版本…")).font(.caption).foregroundStyle(.secondary)
                if a["log"].truthy { CodeBlock(text: a["log"].str, maxHeight: 140) }
            } else if job["state"].str == "error" {
                Note(text: job["detail"].str, tone: .red)
                if a["log"].truthy { CodeBlock(text: a["log"].str, maxHeight: 140) }
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .help(L.text("第一次检查用到它的 Agent 时自动安装。升级或切回版本不影响正在运行的会话。"))
    }
}
