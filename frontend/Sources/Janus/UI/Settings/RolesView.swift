import AppKit
import SwiftUI

/// Roles: who uses which model, with which instructions and tools.
struct RolesView: View {
    let store: AppStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .firstTextBaseline) {
                    Text("决定谁用什么模型、带什么指令和工具做事。").font(.callout).foregroundStyle(.secondary)
                    Spacer()
                    Button("新建角色") { store.newRole() }.buttonStyle(.borderedProminent)
                }
                if store.okConnections.isEmpty { Note(text: "还没有可用的 Agent。可以先编辑角色，但要在 Agents 里测试通过后才能运行。", tone: .amber) }
                if store.roles.isEmpty {
                    Card(padding: 20) {
                        Text("还没有角色").font(.title3.weight(.semibold))
                        Text("角色决定谁做什么。先建一个主控，再建几个负责具体工作的成员。").foregroundStyle(.secondary)
                        Button("新建角色") { store.newRole() }.buttonStyle(.borderedProminent)
                    }
                } else {
                    VStack(spacing: 0) {
                        ForEach(store.roles, id: \.self) { r in row(r); Divider() }
                    }
                    .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
                }
            }
            .padding(20)
        }
    }

    private func row(_ r: JSON) -> some View {
        let c = store.connection(r["connection"].str)
        let k = c.flatMap { store.kind($0["kind"].str) }
        let facts = [Setup.permLabel(r["session"], c), r["prompt"].truthy ? "提示词" : "", r["skills"].array.isEmpty ? "" : "\(r["skills"].array.count) Skill", r["mcp"].array.isEmpty ? "" : "\(r["mcp"].array.count) MCP"].filter { !$0.isEmpty }
        return Button { store.editingRole = r } label: {
            HStack(spacing: 12) {
                Avatar(initials: r["init"].truthy ? r["init"].str : String(r["name"].str.prefix(2)).uppercased(), color: r["color"].str, size: 30)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) { Text(r["name"].str).fontWeight(.semibold); if r["lead"].truthy { Pill("主控", .violet) } }
                    Text(([r["desc"].truthy ? r["desc"].str : r["role"].truthy ? r["role"].str : "还没有填写职责"] + facts).joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if let c {
                    HStack(spacing: 6) {
                        Avatar(initials: k?["init"].str ?? "?", color: k?["color"].str ?? "#64748b", size: 18)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(c["name"].str).font(.callout.weight(.medium))
                            Text(verbatim: "\(r["model"].truthy ? r["model"].str : c["options"]["model"]["current"].truthy ? c["options"]["model"]["current"].str : "默认模型")\(r["effort"].truthy ? "，\(r["effort"].str)" : "")").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .frame(width: 200, alignment: .leading)
                } else {
                    Text("未选择 Agent").foregroundStyle(Tone.amber.color).frame(width: 200, alignment: .leading)
                }
                Image(systemName: "chevron.right").foregroundStyle(.tertiary)
            }
            .padding(12)
            .contentShape(Rectangle())
            .hoverable(0)
        }
        .buttonStyle(.plain)
        .help("编辑 \(r["name"].str)")
    }

    /// "最近 27 个 Run：10 个任务 · 首次通过 70% · 验证失败 2 · 成功平均 5.2 分钟"
    static func statLine(_ x: JSON, runs: Int) -> String? {
        guard let tasks = x["tasks"].int, tasks > 0 else { return nil }
        let pct = { (n: Int) in "\(Int((Double(n) / Double(tasks) * 100).rounded()))%" }
        let parts = ["\(tasks) 个任务", "首次通过 \(pct(x["firstPass"].int ?? 0))", (x["failed"].int ?? 0) > 0 ? "失败 \(x["failed"].str)" : "", (x["verificationFailed"].int ?? 0) > 0 ? "验证没通过 \(x["verificationFailed"].str)" : "", x["avgMinutes"].truthy ? "成功平均 \(x["avgMinutes"].str) 分钟" : ""].filter { !$0.isEmpty }
        return "最近 \(runs) 个 Run：\(parts.joined(separator: " · "))"
    }
}

/// One column, top to bottom, the way the role is read.
struct RoleEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var r: JSON = [:]
    @Local private var skillPick = false
    @Local private var skillQuery = ""
    @Local private var skillDraft: JSON?
    @Local private var mcpDraft: JSON?
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
                .buttonStyle(.plain).help("选择颜色").accessibilityLabel("角色颜色")
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
                    Text(r["id"].truthy ? "编辑角色" : "新建角色").font(.title3.weight(.semibold))
                    if let line = RolesView.statLine(store.roleStats["roles"][r["id"].str], runs: store.roleStats["runs"].int ?? 0) {
                        Text(line).font(.caption).foregroundStyle(.secondary).help("首次通过：第一次尝试就完成，且验证一次通过")
                    }
                }
                Spacer()
            }
            .padding(16)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(spacing: 12) {
                        labeled("名称") { TextField("Reviewer", text: bind("name")).textFieldStyle(.roundedBorder) }
                        labeled("定位") { TextField("评审", text: bind("role")).textFieldStyle(.roundedBorder) }
                    }
                    labeled("职责") { TextField("独立检查变更，寻找风险与反例", text: bind("desc")).textFieldStyle(.roundedBorder) }

                    section("由谁来做")
                    HStack(alignment: .top, spacing: 12) {
                        labeled("Agent") {
                            Picker("Agent", selection: Binding(get: { r["connection"].str }, set: { r["connection"] = JSON($0); r["model"] = ""; r["effort"] = ""; r["session"] = [:] })) {
                                Text("选择 Agent").tag("")
                                ForEach(store.connections, id: \.self) { x in Text(x["name"].str).tag(x["id"].str) }
                            }
                            .labelsHidden()
                        }
                        labeled("模型") {
                            if c == nil { Picker("模型", selection: .constant("")) { Text("先选择 Agent").tag("") }.labelsHidden().disabled(true) }
                            else if !opt["model"]["options"].array.isEmpty {
                                Picker("模型", selection: bind("model")) {
                                    Text(verbatim: "默认（\(opt["model"]["current"].str)）").tag("")
                                    ForEach(opt["model"]["options"].array, id: \.self) { o in Text(o["name"].truthy ? o["name"].str : o["value"].str).tag(o["value"].str) }
                                }
                                .labelsHidden()
                            } else if c?["status"].str == "testing" { Text("读取模型中…").foregroundStyle(.secondary) }
                            else { Button(c?["status"].str == "error" ? "检查未通过，重新检查" : "检查 Agent 以读取模型") { Task { await store.testConnection(c!["id"].str) } }.buttonStyle(.link) }
                        }
                        labeled("推理强度") {
                            if !opt["effort"]["options"].array.isEmpty {
                                Picker("推理强度", selection: bind("effort")) {
                                    Text(verbatim: "默认（\(opt["effort"]["current"].str)）").tag("")
                                    ForEach(opt["effort"]["options"].array, id: \.self) { o in Text(o["name"].truthy ? o["name"].str : o["value"].str).tag(o["value"].str) }
                                }
                                .labelsHidden()
                            } else { Picker("推理强度", selection: .constant("")) { Text("默认").tag("") }.labelsHidden().disabled(true) }
                        }
                    }

                    section("指令")
                    TextEditor(text: bind("prompt"))
                        .font(.body)
                        .accessibilityLabel("指令")
                        .frame(minHeight: 120, maxHeight: 260)
                        .overlay(alignment: .topLeading) {
                            if !r["prompt"].truthy { Text("告诉这个角色怎么做事，例如：先读相关代码和已有测试；重点检查边界条件、错误处理和测试覆盖。").foregroundStyle(.tertiary).padding(6).allowsHitTesting(false) }
                        }
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    Text(Setup.instructionsVia(c)).font(.caption).foregroundStyle(.secondary)

                    skills
                    mcp

                    section("权限", trailing: Setup.controls(c).contains { $0["permission"].truthy } ? "\(c?["name"].str ?? "") 的权限模式" : "")
                    permissions(c)
                    if c?["options"]["controls"].exists == true {
                        Text("主控规划时会看到这个模式，按它分配任务。Agent 自己判断哪些操作要先问你，它发起的审批、表单和链接都会进入「需要你处理」，按 Agent 给出的选项呈现。").font(.caption).foregroundStyle(.secondary)
                    }
                    otherControls(c)

                    section("协作")
                    Toggle(isOn: Binding(get: { r["lead"].truthy }, set: { r["lead"] = JSON($0) })) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("可以担任主控")
                            Text(Setup.leadCapable(c) ? "用 /goal 推进目标，需要时编排协作者" : "只有 Claude Code 或 Codex 能做主控（需要 /goal）").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .toggleStyle(.switch)
                    .disabled(!Setup.leadCapable(c))
                }
                .padding(18)
            }
            Divider()
            HStack {
                if r["id"].truthy { Button("删除…", role: .destructive) { remove() } }
                Spacer()
                Button("取消") { if r == original || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                Button(saving ? "正在保存…" : r["id"].truthy ? "保存" : "创建") { save() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(saving || r["name"].str.trimmingCharacters(in: .whitespaces).isEmpty)
                    .help(r["name"].str.trimmingCharacters(in: .whitespaces).isEmpty ? "先填写名称" : "")
            }
            .padding(14)
        }
        .frame(minWidth: 620, idealWidth: 680, minHeight: 480, idealHeight: 760)
        .onAppear { r = original }
        .task { if store.skills == nil { await store.loadSkills() } }
    }

    private func bind(_ k: String) -> Binding<String> {
        Binding(get: { r[k].str }, set: { v in r[k] = JSON(v); if k == "name" && !r["id"].truthy { r["init"] = "" } })
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

    // MARK: Skills

    @ViewBuilder private var skills: some View {
        let lib = store.skills ?? []
        let picked = r["skills"].strings.map { id in lib.first { $0["id"].str == id } ?? ["id": JSON(id), "name": JSON(id), "description": ""] }
        section("Skills", count: picked.count, action: skillPick ? nil : ("添加", { skillPick = true }))
        ForEach(picked, id: \.self) { s in
            mounted("sparkles", s["name"].str, s["description"].str) { r["skills"] = JSON(r["skills"].strings.filter { $0 != s["id"].str }) }
        }
        if skillPick {
            VStack(alignment: .leading, spacing: 8) {
                if let d = skillDraft {
                    TextField("名称，例如 code-review", text: Binding(get: { d["name"].str }, set: { skillDraft?["name"] = JSON($0) })).textFieldStyle(.roundedBorder)
                    TextField("什么时候用，例如：评审代码变更时使用", text: Binding(get: { d["description"].str }, set: { skillDraft?["description"] = JSON($0) })).textFieldStyle(.roundedBorder)
                    TextEditor(text: Binding(get: { d["content"].str }, set: { skillDraft?["content"] = JSON($0) })).frame(height: 110).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    HStack { Spacer(); Button("取消") { skillDraft = nil }; Button("添加到 Skill 库") { createSkill() }.buttonStyle(.borderedProminent) }
                } else {
                    if !lib.isEmpty { TextField("搜索 Skill 库", text: $skillQuery).textFieldStyle(.roundedBorder) }
                    let q = skillQuery.lowercased()
                    let list = lib.filter { q.isEmpty || "\($0["name"].str) \($0["description"].str)".lowercased().contains(q) }
                    if store.skills == nil { Text("读取中…").foregroundStyle(.secondary) }
                    else if list.isEmpty { Text(lib.isEmpty ? "Skill 库还是空的" : "没有匹配的 Skill").font(.callout).foregroundStyle(.secondary) }
                    ForEach(list, id: \.self) { s in
                        let on = r["skills"].strings.contains(s["id"].str)
                        Button {
                            var set = r["skills"].strings
                            if on { set.removeAll { $0 == s["id"].str } } else { set.append(s["id"].str) }
                            r["skills"] = JSON(set)
                        } label: {
                            HStack(alignment: .top) {
                                Image(systemName: on ? "checkmark.square.fill" : "square").foregroundStyle(on ? Color.accentColor : .secondary).accessibilityHidden(true)
                                VStack(alignment: .leading, spacing: 1) { Text(s["name"].str).fontWeight(.medium); Text(s["description"].str).font(.caption).foregroundStyle(.secondary) }
                                Spacer()
                            }
                            .contentShape(Rectangle())
                            .hoverable(6)
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
                    HStack {
                        Button("新建…") { skillDraft = ["name": "", "description": "", "content": ""] }.buttonStyle(.link)
                        Button("从文件夹导入…") { importSkill() }.buttonStyle(.link)
                        Spacer()
                        Button("完成") { skillPick = false }
                    }
                }
            }
            .padding(10)
            .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func mounted(_ icon: String, _ title: String, _ sub: String, remove: @escaping () -> Void) -> some View {
        HStack(alignment: .top) {
            Image(systemName: icon).foregroundStyle(Color.accent)
            VStack(alignment: .leading, spacing: 1) { Text(title).fontWeight(.medium); if !sub.isEmpty { Text(sub).font(.caption).foregroundStyle(.secondary) } }
            Spacer()
            Button(action: remove) { Label("移除 \(title)", systemImage: "xmark") }.labelStyle(.iconOnly).buttonStyle(.borderless).help("移除")
        }
        .padding(8)
        .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
    }

    private func createSkill() {
        guard let d = skillDraft else { return }
        Task {
            do {
                let sk = try await store.api.post("/api/skills", d)
                skillDraft = nil
                await store.loadSkills()
                r["skills"] = JSON(r["skills"].strings + [sk["id"].str])
                store.show("已添加 Skill \(sk["name"].str)")
            } catch { store.show(error.localizedDescription) }
        }
    }

    private func importSkill() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.showsHiddenFiles = true
        p.prompt = "复制到 Skill 库"
        p.message = "选择包含 SKILL.md 的文件夹"
        guard p.runModal() == .OK, let u = p.url else { return }
        Task {
            do {
                let sk = try await store.api.post("/api/skills/import", ["path": JSON(u.path)])
                await store.loadSkills()
                r["skills"] = JSON(r["skills"].strings + [sk["id"].str])
                store.show("已导入 Skill \(sk["name"].str)")
            } catch { store.show(error.localizedDescription) }
        }
    }

    // MARK: MCP

    @ViewBuilder private var mcp: some View {
        let list = r["mcp"].array
        section("MCP", count: list.count, action: mcpDraft == nil ? ("添加", { mcpDraft = ["name": "", "command": "", "envText": ""] }) : nil)
        ForEach(list.indices, id: \.self) { i in
            let m = list[i]
            let cmd = ([m["command"].str] + m["args"].strings).joined(separator: " ")
            mounted("square.on.square", m["name"].str, cmd + (m["envKeys"].array.isEmpty ? "" : " · \(m["envKeys"].array.count) 个环境变量")) {
                var l = r["mcp"].array; l.remove(at: i); r["mcp"] = .array(l)
            }
        }
        if let d = mcpDraft {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    TextField("名称，例如 filesystem", text: Binding(get: { d["name"].str }, set: { mcpDraft?["name"] = JSON($0) })).textFieldStyle(.roundedBorder).frame(width: 180)
                    TextField("启动命令，例如 npx -y @modelcontextprotocol/server-filesystem .", text: Binding(get: { d["command"].str }, set: { mcpDraft?["command"] = JSON($0) })).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                }
                TextField("环境变量 KEY=value，每行一个", text: Binding(get: { d["envText"].str }, set: { mcpDraft?["envText"] = JSON($0) }), axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(2...5).font(.system(.body, design: .monospaced))
                HStack {
                    Spacer()
                    Button("取消") { mcpDraft = nil }
                    Button("添加") { addMcp(d) }.buttonStyle(.borderedProminent)
                }
            }
            .padding(10)
            .background(Color.secondary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func addMcp(_ d: JSON) {
        guard d["name"].str.range(of: "^[A-Za-z0-9_-]+$", options: .regularExpression) != nil else { store.show("名称只能包含字母、数字、- 和 _。"); return }
        let cmd = d["command"].str.trimmingCharacters(in: .whitespaces)
        guard !cmd.isEmpty else { store.show("请填写启动命令。"); return }
        r["mcp"] = .array(r["mcp"].array + [["name": d["name"], "command": JSON(cmd), "envText": d["envText"]]])
        mcpDraft = nil
    }

    // MARK: Session controls: whatever the agent declares

    @ViewBuilder private func permissions(_ c: JSON?) -> some View {
        if let c {
            if c["status"].str == "testing" { Note(text: "正在检查 \(c["name"].str)，读取它的权限模式…") }
            else if !c["options"]["controls"].exists {
                HStack { Note(text: "还没有读取 \(c["name"].str) 的权限模式。", tone: .amber); Button("检查 Agent") { Task { await store.testConnection(c["id"].str) } } }
            } else {
                let perms = Setup.controls(c).filter { $0["permission"].truthy }
                if perms.isEmpty { Note(text: "\(c["name"].str) 没有声明权限模式：工具调用直接执行，不会发起审批，可以修改工作目录中的文件。") }
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
                                                if o["value"] == x["current"] { Text("Agent 默认").font(.caption2).foregroundStyle(.secondary) }
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
            Note(text: "先选择 Agent。权限选项由 Agent 自己声明，每种 Agent 都不一样。")
        }
    }

    @ViewBuilder private func otherControls(_ c: JSON?) -> some View {
        let rest = Setup.controls(c).filter { !$0["permission"].truthy }
        if !rest.isEmpty {
            section("会话选项", trailing: "\(c?["name"].str ?? "") 提供")
            ForEach(rest, id: \.self) { x in
                let v = Setup.value(r["session"], x)
                HStack {
                    VStack(alignment: .leading, spacing: 1) { Text(x["name"].str); if x["desc"].truthy { Text(x["desc"].str).font(.caption).foregroundStyle(.secondary) } }
                    Spacer()
                    if x["type"].str == "boolean" {
                        Toggle(x["name"].str, isOn: Binding(get: { v.bool ?? false }, set: { setCtrl(x, JSON($0)) })).labelsHidden().toggleStyle(.switch)
                    } else {
                        Picker(x["name"].str, selection: Binding(get: { v.str }, set: { setCtrl(x, JSON($0)) })) {
                            ForEach(x["options"].array, id: \.self) { o in Text((o["name"].truthy ? o["name"].str : o["value"].str) + (o["value"] == x["current"] ? "（默认）" : "")).tag(o["value"].str) }
                        }
                        .labelsHidden().frame(width: 220)
                    }
                }
            }
        }
    }

    /// Choosing the agent's default drops the key, so the role follows the agent if its default changes.
    private func setCtrl(_ x: JSON, _ v: JSON) {
        var s = r["session"].isNull ? [:] : r["session"]
        if v == x["current"] { var o = s.object; o[x["id"].str] = nil; s = .object(o) } else { s[x["id"].str] = v }
        r["session"] = s
    }

    private func save() {
        saving = true
        Task {
            do {
                let saved = try await store.save("roles", r)
                store.show("已保存角色 \(saved["name"].str)")
                dismiss()
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }

    private func remove() {
        guard confirm("删除角色「\(r["name"].str)」？", ok: "删除") else { return }
        Task {
            do { try await store.delete("roles", r["id"].str); dismiss() } catch { store.show(error.localizedDescription) }
        }
    }
}

/// ACP adapters: installed on demand into Janus home, one directory per version.
struct AdaptersView: View {
    let store: AppStore

    var body: some View {
        let data = store.adapters
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .firstTextBaseline) {
                    Text("第一次检查用到它的 Agent 时自动安装。升级或切回版本不影响正在运行的会话。").font(.callout).foregroundStyle(.secondary)
                    Spacer()
                    if data["dir"].truthy {
                        Button(L.tilde(data["dir"].str)) { NSWorkspace.shared.open(URL(fileURLWithPath: data["dir"].str)) }.font(.system(.caption, design: .monospaced)).help("适配器安装目录")
                    }
                }
                if data.isNull { ProgressView() }
                VStack(spacing: 0) {
                    ForEach(data["adapters"].array, id: \.self) { a in row(a); Divider() }
                }
                .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
            }
            .padding(20)
        }
        .task { await store.loadAdapters() }
    }

    private func row(_ a: JSON) -> some View {
        let job = a["job"]
        let running = job["state"].str == "running"
        let k = store.kind(a["id"].str)
        let src = a["source"]["kind"].str == "npm" ? "npm · \(a["source"]["package"].str)" : "GitHub · \(a["source"]["repo"].str.replacingOccurrences(of: "https://github.com/", with: ""))"
        let prev = a["versions"].array.filter { !$0["current"].truthy }
        let id = a["id"].str
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 12) {
                Avatar(initials: k?["init"].str ?? "?", color: k?["color"].str ?? "#64748b", size: 30)
                VStack(alignment: .leading, spacing: 2) { Text(a["name"].str).fontWeight(.semibold); Text(src).font(.caption).foregroundStyle(.secondary) }
                    .frame(width: 260, alignment: .leading)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(a["current"].truthy ? a["current"].str : "—").font(.system(.body, design: .monospaced))
                        if running { Pill(job["action"].str == "install" ? "安装中" : "检查中", .blue) }
                        else if !a["current"].truthy { Pill("未安装") }
                        else if a["updateAvailable"].truthy { Pill("有新版本", .amber) }
                        else { Pill("已安装", .green) }
                    }
                    Text(a["latest"].truthy ? "最新 \(a["latest"].str)\(a["checkedAt"].truthy ? " · \(L.clock(a["checkedAt"].double ?? 0)) 检查" : "")" : "还没有检查更新").font(.caption).foregroundStyle(.secondary)
                    if let p = prev.first {
                        HStack(spacing: 4) {
                            Text(verbatim: "上一个版本 \(p["version"].str)").font(.caption).foregroundStyle(.secondary)
                            Button("切回") { Task { await store.adapterAction(id, "use", version: p["version"].str) } }.buttonStyle(.link).font(.caption).disabled(running)
                        }
                    }
                }
                Spacer()
                if !a["current"].truthy {
                    Button("安装") { Task { await store.adapterAction(id, "install") } }.buttonStyle(.borderedProminent).disabled(running)
                } else {
                    if a["updateAvailable"].truthy { Button(String("升级到 \(a["latest"].str)")) { Task { await store.adapterAction(id, "install") } }.buttonStyle(.borderedProminent).disabled(running) }
                    Button("检查更新") { Task { await store.adapterAction(id, "check") } }.disabled(running)
                }
            }
            if running {
                Text(job["action"].str == "install" ? "正在下载和构建，完成后自动设为当前版本…" : "正在查询最新版本…").font(.callout).foregroundStyle(.secondary)
                if a["log"].truthy { CodeBlock(text: a["log"].str, maxHeight: 140) }
            } else if job["state"].str == "error" {
                Note(text: job["detail"].str, tone: .red)
                if a["log"].truthy { CodeBlock(text: a["log"].str, maxHeight: 140) }
            }
        }
        .padding(12)
    }
}
