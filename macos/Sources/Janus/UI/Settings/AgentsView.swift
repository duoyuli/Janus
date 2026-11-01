import SwiftUI

/// Agents: each runs in its own process and config directory, without touching the user's own agents.
struct AgentsView: View {
    let store: AppStore
    @Local private var editing: JSON?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                OnboardingBar(store: store)
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Agents").font(.largeTitle.weight(.semibold))
                        Text("每个 Agent 在独立的进程和配置目录里运行，不影响你本机的 Agent。").foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("添加 Agent") { editing = newDraft(store.connections.isEmpty ? "codex" : "claude") }.buttonStyle(.borderedProminent)
                }
                if store.connections.isEmpty {
                    Card(padding: 20) {
                        Text("还没有接入 Agent").font(.title3.weight(.semibold))
                        Text("把你本机已经在用的 Agent 迁移过来，配置和凭据会一并复制。").foregroundStyle(.secondary)
                        HStack {
                            ForEach(["codex", "claude", "pi", "grok"].compactMap { store.kind($0) }, id: \.self) { k in
                                Button { editing = newDraft(k["id"].str) } label: {
                                    HStack {
                                        Avatar(initials: k["init"].str, color: k["color"].str, size: 28)
                                        VStack(alignment: .leading) { Text(k["name"].str).fontWeight(.semibold); Text("复制本机配置").font(.caption).foregroundStyle(.secondary) }
                                    }
                                    .padding(8)
                                }
                            }
                        }
                        Button("查看全部类型，包括 API Key、预设和自定义命令") { editing = newDraft("codex") }.buttonStyle(.link)
                    }
                } else {
                    VStack(spacing: 0) {
                        ForEach(store.connections, id: \.self) { c in
                            ConnectionRow(store: store, c: c) { editing = editDraft(c) } newRole: { store.newRole(connection: c["id"].str) }
                            Divider()
                        }
                    }
                    .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
                }
            }
            .padding(24)
            .frame(maxWidth: 1000, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .sheet(isPresented: Binding(get: { editing != nil }, set: { if !$0 { editing = nil } })) {
            if let d = editing { ConnectionEditor(store: store, draft: d) }
        }
    }

    private func newDraft(_ kind: String) -> JSON {
        let k = store.kind(kind)
        var d: JSON = ["kind": JSON(kind), "auth": k?["auths"][0]["id"] ?? "", "name": k?["name"] ?? "", "api": "openai-completions", "prompt": "", "endpoint": k?["endpoints"][0]["id"] ?? .null]
        d["envText"] = JSON(Setup.setEnvLines("", Setup.presetEnv(k, nil).object.mapValues { Optional($0.str) }))
        return d
    }

    private func editDraft(_ c: JSON) -> JSON {
        var d = c
        var env = Setup.presetEnv(store.kind(c["kind"].str), c["endpoint"].string)
        for (k, v) in c["env"].object { env[k] = v }
        d["envText"] = JSON(Setup.envText(env))
        return d
    }
}

struct ConnectionRow: View {
    let store: AppStore
    let c: JSON
    let edit: () -> Void
    let newRole: () -> Void

    var body: some View {
        let k = store.kind(c["kind"].str)
        let (label, tone) = status
        let roles = store.roles.filter { $0["connection"].str == c["id"].str }
        let login = loginAuth
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                Button(action: edit) {
                    HStack(spacing: 10) {
                        Avatar(initials: k?["init"].str ?? "?", color: k?["color"].str ?? "#64748b", size: 30)
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text(c["name"].str).fontWeight(.semibold)
                                Text(c["id"].str).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                            }
                            Text(sub(k)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Spacer()
                if roles.isEmpty { Button("新建角色", action: newRole).buttonStyle(.link) }
                else {
                    HStack(spacing: -4) { ForEach(roles.prefix(4), id: \.self) { r in Avatar(initials: r["init"].str, color: r["color"].str, size: 20) } }
                    Text(verbatim: "\(roles.count) 个角色").font(.caption).foregroundStyle(.secondary)
                }
                Dot(tone: tone, text: label).frame(width: 90, alignment: .leading)
                Menu {
                    if let login { Button(String("\(c["loggedIn"].truthy ? "重新登录" : "登录") \(login["label"].str)")) { Task { await store.login(c["id"].str) } } }
                    Button("检查配置") { Task { await store.testConnection(c["id"].str) } }
                    Button("编辑", action: edit)
                    Button("在这个 Agent 上新建角色", action: newRole)
                    Divider()
                    Button("删除", role: .destructive) { remove() }
                } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }
            extra(login)
        }
        .padding(12)
    }

    @ViewBuilder private func extra(_ login: JSON?) -> some View {
        if login != nil && c["login"]["state"].str == "running" {
            VStack(alignment: .leading, spacing: 6) {
                Text(verbatim: "按提示完成 \(login!["label"].str) 登录，完成后会自动测试。").font(.callout)
                if c["loginLog"].truthy {
                    let log = c["loginLog"].str.replacingOccurrences(of: "\u{1b}\\[[0-9;]*m", with: "", options: .regularExpression)
                    CodeBlock(text: log, maxHeight: 160)
                    ForEach(urls(log), id: \.self) { u in Link(u, destination: URL(string: u)!).font(.callout) }
                }
            }
        } else if let login, !c["loggedIn"].truthy {
            HStack { Note(text: "还没有登录 \(login["label"].str)。", tone: .amber); Button("登录") { Task { await store.login(c["id"].str) } } }
        } else if c["status"].str == "testing" {
            CheckList(c: c)
        } else if c["status"].str == "error" {
            HStack {
                Note(text: c["error"].str, tone: .red)
                Button("重新检查") { Task { await store.testConnection(c["id"].str) } }
                Button("修改配置", action: edit)
            }
        } else if c["status"].str == "untested" {
            HStack { Text("还没有检查配置，读到模型后才能给角色使用。").font(.callout).foregroundStyle(.secondary); Button("检查") { Task { await store.testConnection(c["id"].str) } }.buttonStyle(.link) }
        }
    }

    private func urls(_ s: String) -> [String] {
        guard let re = try? NSRegularExpression(pattern: #"https?://[^\s<]+"#) else { return [] }
        return re.matches(in: s, range: NSRange(s.startIndex..., in: s)).compactMap { Range($0.range, in: s).map { String(s[$0]) } }
    }

    /// The auth method's own `login` flag says whether Janus drives an account login (Codex ChatGPT, Grok xAI).
    private var loginAuth: JSON? { store.kind(c["kind"].str)?["auths"].array.first { $0["id"].str == c["auth"].str && $0["login"].truthy } }

    private var status: (String, Tone) {
        if c["status"].str == "testing" { return ("检查中…", .blue) }
        if loginAuth != nil && !c["loggedIn"].truthy { return c["login"]["state"].str == "running" ? ("等待登录", .amber) : ("未登录", .amber) }
        return ["ok": ("可用", Tone.green), "error": ("失败", .red), "untested": ("未测试", .gray)][c["status"].str] ?? ("未测试", .gray)
    }

    private func sub(_ k: JSON?) -> String {
        let auth = k?["auths"].array.first { $0["id"].str == c["auth"].str }?["label"].string
        let model = c["options"]["model"]
        let perms = Setup.controls(c).filter { $0["permission"].truthy }
        let permText: String? = c["options"].isNull ? nil : !c["options"]["controls"].exists ? "未读取权限模式" : perms.isEmpty ? "无权限模式" : perms.map { $0["type"].str == "boolean" ? $0["name"].str : $0["options"].array.map { $0["name"].str }.joined(separator: " / ") }.joined(separator: "；")
        return [k?["name"].string, auth, model.exists ? "\(model["options"].array.count) 个模型" : nil, permText].compactMap { $0 }.joined(separator: "，")
    }

    private func remove() {
        let used = store.roles.filter { $0["connection"].str == c["id"].str }.map { $0["name"].str }
        let msg = used.isEmpty ? "" : "这些角色正在使用它，删除后会变成未绑定，需要重新选择 Agent：\(used.joined(separator: "、"))"
        guard confirm("删除「\(c["name"].str)」和它的独立目录（包括登录凭据）？", msg, ok: "删除") else { return }
        Task {
            do { try await store.delete("connections", c["id"].str); store.show("已删除 \(c["name"].str)") } catch { await store.loadSetup(); store.show(error.localizedDescription) }
        }
    }
}

struct CheckList: View {
    let c: JSON
    static let steps = ["ACP 适配器", "运行时", "凭据", "启动 Agent", "打开会话", "试运行"]

    var body: some View {
        let done = c["check"].array
        let testing = c["status"].str == "testing"
        // Custom commands bring their own agent: they have no adapter step.
        let steps = c["base"].str == "custom" ? Array(Self.steps.dropFirst()) : Self.steps
        if !done.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(steps, id: \.self) { name in
                    let st = done.first { $0["step"].str == name }
                    if st != nil || testing {
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            if let st, st["ok"].bool == true { Image(systemName: "checkmark.circle.fill").foregroundStyle(Tone.green.color) }
                            else if let st, st["ok"].bool == false { Image(systemName: "xmark.circle.fill").foregroundStyle(Tone.red.color) }
                            else if st != nil { ProgressView().controlSize(.mini) }
                            else { Image(systemName: "circle").foregroundStyle(.tertiary) }
                            Text(name).font(.callout.weight(.medium)).frame(width: 80, alignment: .leading)
                            Text(st?["detail"].str ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(3)
                        }
                    }
                }
            }
        }
    }
}

/// Add or edit an Agent: type, auth, credentials, preset env, runtime and the provider prompt.
struct ConnectionEditor: View {
    let store: AppStore
    @Local var draft: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var query = ""
    @Local private var secrets: [String: String] = [:]
    @Local private var preview: JSON?
    @Local private var saving = false
    @Local private var idTouched = false
    @Local private var showSource = false
    @Local private var advanced = false

    init(store: AppStore, draft: JSON) {
        self.store = store
        _draft = Local(initialValue: draft)
    }

    var body: some View {
        let editing = draft["id"].truthy
        let k = store.kind(draft["kind"].str) ?? [:]
        let prev = editing ? store.connection(draft["id"].str) : nil
        HStack(spacing: 0) {
            rail(editing)
                .frame(width: 210)
                .background(Color.secondary.opacity(0.05))
            Divider()
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Avatar(initials: k["init"].str, color: k["color"].str, size: 34)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(editing ? "编辑 \(prev?["name"].str ?? k["name"].str)" : "添加 Agent").font(.title3.weight(.semibold))
                        Text(k["desc"].str).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                }
                .padding(16)
                Divider()
                ScrollView { form(k, prev, editing).padding(18) }
                Divider()
                HStack {
                    Spacer()
                    Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                    Button("保存并检查") { save(test: true) }.disabled(saving).help("保存并检查运行时、凭据、启动、会话和一次真实回复")
                    Button(saving ? "保存中…" : editing ? "保存" : "创建") { save(test: false) }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(saving)
                }
                .padding(14)
            }
        }
        .frame(width: 860, height: 640)
        .task(id: "\(draft["kind"].str)|\(draft["source"].str)|\(draft["auth"].str)") {
            guard draft["auth"].str == "import" else { return }
            preview = nil
            try? await Task.sleep(nanoseconds: 250_000_000)
            do { preview = try await store.api.get("/api/local-config?kind=\(API.q(draft["kind"].str))&source=\(API.q(draft["source"].str))") }
            catch { preview = ["error": JSON(error.localizedDescription)] }
        }
        .task(id: "\(draft["kind"].str)|\(draft["name"].str)") {
            // Readable id = folder name, suggested from type and name until the user types their own.
            guard !editing, !idTouched else { return }
            try? await Task.sleep(nanoseconds: 200_000_000)
            if let r = try? await store.api.get("/api/agent-id?kind=\(API.q(draft["kind"].str))&name=\(API.q(draft["name"].str))"), !idTouched { draft["newId"] = r["id"] }
        }
    }

    private func rail(_ editing: Bool) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            TextField("搜索", text: $query).textFieldStyle(.roundedBorder).padding(10)
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach([("builtin", "内置"), ("preset", "预设"), ("custom", "自定义")], id: \.0) { g in
                        let q = query.lowercased()
                        let items = store.meta["kinds"].array.filter { $0["group"].str == g.0 && (q.isEmpty || "\($0["name"].str) \($0["id"].str) \($0["desc"].str)".lowercased().contains(q)) }
                        if !items.isEmpty {
                            Text(g.1).font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.horizontal, 12).padding(.top, 8)
                            ForEach(items, id: \.self) { t in
                                Button { pickType(t["id"].str) } label: {
                                    HStack(spacing: 8) {
                                        Avatar(initials: t["init"].str, color: t["color"].str, size: 20)
                                        Text(t["name"].str).lineLimit(1)
                                        Spacer()
                                        if t["id"].str == draft["kind"].str { Image(systemName: "checkmark").foregroundStyle(Color.accent) }
                                    }
                                    .padding(.horizontal, 10).padding(.vertical, 6)
                                    .background(t["id"].str == draft["kind"].str ? Color.accent.opacity(0.1) : .clear, in: RoundedRectangle(cornerRadius: 6))
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .disabled(editing)
                            }
                        }
                    }
                }
                .padding(.horizontal, 6)
            }
        }
    }

    private func str(_ k: String) -> Binding<String> { Binding(get: { draft[k].str }, set: { draft[k] = JSON($0) }) }

    @ViewBuilder private func form(_ k: JSON, _ prev: JSON?, _ editing: Bool) -> some View {
        let auths = k["auths"].array
        let auth = auths.first { $0["id"].str == draft["auth"].str } ?? auths.first
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                field("名称") { TextField(k["name"].str, text: str("name")).textFieldStyle(.roundedBorder) }
                field("ID", hint: "~/.janus/agents/\(editing ? draft["id"].str : draft["newId"].str)") {
                    if editing { TextField("", text: .constant(draft["id"].str)).textFieldStyle(.roundedBorder).disabled(true).font(.system(.body, design: .monospaced)) }
                    else { TextField("codex", text: Binding(get: { draft["newId"].str }, set: { draft["newId"] = JSON($0); idTouched = !$0.isEmpty })).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)) }
                }
            }
            if k["command"].truthy {
                field("启动命令") { TextField("npx -y @zed-industries/claude-code-acp", text: str("command")).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)) }
            }
            if let auth {
                field("认证", hint: auth["desc"].str) {
                    Picker("", selection: Binding(get: { auth["id"].str }, set: { draft["auth"] = JSON($0) })) {
                        ForEach(auths, id: \.self) { a in Text(a["label"].str).tag(a["id"].str) }
                    }
                    .pickerStyle(.segmented).labelsHidden()
                }
                if auth["id"].str == "import" {
                    if showSource { ForEach(auth["fields"].array, id: \.self) { f in inputField(f, prev) } }
                    importPreview(k)
                } else {
                    ForEach(auth["fields"].array, id: \.self) { f in inputField(f, prev) }
                }
                if auth["login"].truthy, let prev {
                    HStack {
                        Text(prev["loggedIn"].truthy ? "已登录 \(auth["label"].str)。" : "还没有登录。").font(.callout).foregroundStyle(.secondary)
                        Button(prev["loggedIn"].truthy ? "重新登录" : "现在登录") { Task { await store.login(prev["id"].str) } }.buttonStyle(.link)
                    }
                }
            }
            if k["token"].truthy {
                let eps = k["endpoints"].array
                if !eps.isEmpty {
                    field("接入点") {
                        Picker("", selection: Binding(get: { draft["endpoint"].string ?? eps[0]["id"].str }, set: { pickEndpoint($0, k) })) {
                            ForEach(eps, id: \.self) { e in Text(e["label"].str).tag(e["id"].str) }
                        }
                        .pickerStyle(.segmented).labelsHidden()
                    }
                }
                inputField(["key": "token", "label": k["token"]["label"], "type": "secret", "required": true, "help": k["token"]["help"]], prev)
                envField(k)
            }
            if let prev, !prev["check"].array.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    HStack { Text("配置检查").font(.headline); Spacer(); if prev["status"].str != "testing" { Button("重新检查") { Task { await store.testConnection(prev["id"].str) } }.buttonStyle(.link) } else { Text("检查中…").font(.caption).foregroundStyle(.secondary) } }
                    CheckList(c: prev)
                }
                .padding(12)
                .background(Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            }
            DisclosureGroup("高级设置", isExpanded: Binding(get: { advanced || draft["prompt"].truthy || draft["bin"].truthy }, set: { advanced = $0 })) {
                VStack(alignment: .leading, spacing: 12) {
                    if k["runtime"].truthy {
                        field("运行时路径") { TextField(k["runtime"]["detected"].truthy ? k["runtime"]["detected"].str : k["runtime"]["placeholder"].str, text: str("bin")).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)) }
                    }
                    field("自定义提示词", hint: "排在角色指令之前，和角色指令走同一种注入方式（Claude Code、Pi 是系统提示，Codex 是开发者指令，其他 Agent 附在任务说明末尾）。") {
                        TextEditor(text: str("prompt")).font(.body).frame(height: 70).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    }
                    if !k["token"].truthy { envField(k) }
                }
                .padding(.top, 8)
            }
        }
    }

    private func field<C: View>(_ label: String, hint: String = "", @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(label).font(.callout.weight(.semibold))
            content()
            if !hint.isEmpty { Text(hint).font(.caption).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func inputField(_ f: JSON, _ prev: JSON?) -> some View {
        let key = f["key"].str
        let label = f["label"].str + (f["required"].truthy ? "" : "（可选）")
        if f["type"].str == "select" {
            field(label) {
                Picker("", selection: str(key)) { ForEach(f["options"].array, id: \.self) { o in Text(o[1].str).tag(o[0].str) } }.labelsHidden()
            }
        } else if f["type"].str == "secret" {
            let saved = prev?["secretsSet"].strings.contains(key) == true
            field(label, hint: f["help"].str) {
                SecureField(saved ? "已保存，留空表示不修改" : f["placeholder"].str, text: Binding(get: { secrets[key] ?? "" }, set: { secrets[key] = $0 })).textFieldStyle(.roundedBorder)
            }
        } else {
            field(label, hint: f["help"].str) { TextField(f["placeholder"].str, text: str(key)).textFieldStyle(.roundedBorder) }
        }
    }

    private func envField(_ k: JSON) -> some View {
        field("环境变量", hint: k["token"].truthy ? "预设的默认值已经填好，可以直接修改；写成 KEY= 表示不设置这个变量。\(k["token"]["envKey"].str) 由上面的 token 填入。" : "写成 KEY= 表示不设置这个变量。") {
            TextEditor(text: str("envText")).font(.system(.callout, design: .monospaced)).frame(minHeight: 70, maxHeight: 200).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
        }
    }

    @ViewBuilder private func importPreview(_ k: JSON) -> some View {
        if let p = preview {
            if p["error"].truthy { Note(text: p["error"].str, tone: .red) }
            else if p["files"].array.isEmpty && p["env"].array.isEmpty {
                HStack { Note(text: "在 \(p["source"].str) 没有找到 \(k["name"].str) 的配置。", tone: .amber); if !showSource { Button("更改来源") { showSource = true } } }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    HStack { Text(verbatim: "从 \(L.tilde(p["source"].str)) 复制").fontWeight(.semibold); if !showSource { Button("更改") { showSource = true }.buttonStyle(.link) } }
                    ForEach(p["files"].array, id: \.self) { f in HStack { Text(f["file"].str).font(.system(.caption, design: .monospaced)); Text(f["note"].str).font(.caption).foregroundStyle(.secondary) } }
                    ForEach(p["env"].array, id: \.self) { e in HStack { Text("$" + e["name"].str).font(.system(.caption, design: .monospaced)); Text("shell 环境变量").font(.caption).foregroundStyle(.secondary) } }
                }
                .padding(10)
                .background(Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            }
        } else {
            Text("正在检查本机配置…").font(.callout).foregroundStyle(.secondary)
        }
    }

    /// Switching type swaps one preset's defaults for the other's; the person's own lines stay.
    private func pickType(_ v: String) {
        let k = store.kind(v)
        let oldKind = store.kind(draft["kind"].str)
        let keepName = draft["name"].truthy && draft["name"].str != oldKind?["name"].str
        var vars: [String: String?] = [:]
        for key in Setup.presetEnv(oldKind, draft["endpoint"].string).object.keys { vars[key] = .some(nil) }
        for (key, val) in Setup.presetEnv(k, nil).object { vars[key] = val.str }
        var d: JSON = ["kind": JSON(v), "auth": k?["auths"][0]["id"] ?? "", "name": keepName ? draft["name"] : (k?["name"] ?? ""), "api": "openai-completions", "prompt": draft["prompt"], "endpoint": k?["endpoints"][0]["id"] ?? .null]
        d["envText"] = JSON(Setup.setEnvLines(draft["envText"].str, vars))
        if idTouched { d["newId"] = draft["newId"] }
        draft = d
        preview = nil
        showSource = false
    }

    private func pickEndpoint(_ v: String, _ k: JSON) {
        var vars: [String: String?] = [:]
        for key in Setup.endpointEnv(k, draft["endpoint"].string).object.keys { vars[key] = .some(nil) }
        for (key, val) in Setup.endpointEnv(k, v).object { vars[key] = val.str }
        draft["envText"] = JSON(Setup.setEnvLines(draft["envText"].str, vars))
        draft["endpoint"] = JSON(v)
    }

    private func save(test: Bool) {
        saving = true
        var body = draft
        for (k, v) in secrets where !v.isEmpty { body[k] = JSON(v) }
        Task {
            do {
                let c = try await store.save("connections", body)
                let id = c["id"].str
                let auth = store.kind(c["kind"].str)?["auths"].array.first { $0["id"].str == c["auth"].str && $0["login"].truthy }
                if test {
                    var d = store.connection(id) ?? c
                    var env = Setup.presetEnv(store.kind(c["kind"].str), c["endpoint"].string)
                    for (k, v) in c["env"].object { env[k] = v }
                    d["envText"] = JSON(Setup.envText(env))
                    draft = d
                    secrets = [:]
                } else {
                    store.show("已保存 \(c["name"].str)，正在检查配置")
                    dismiss()
                }
                if auth != nil && store.connection(id)?["loggedIn"].truthy != true { await store.login(id) } else { await store.testConnection(id) }
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }
}
