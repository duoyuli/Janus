import SwiftUI

/// Settings › 数字员工: the Agents the digital workers run on, each card with its login, check and
/// ACP adapter version, then the digital workers themselves.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct WorkersSettingsView: View {
    let store: AppStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                AgentsSection(store: store)
                RolesSection(store: store)
            }
            .padding(20)
        }
        // Read the runtime lines when the page opens; the global stream keeps them current.
        .task { await store.loadAdapters() }
    }
}

/// Agents: each runs in its own process and config directory, without touching the user's own agents.
struct AgentsSection: View {
    let store: AppStore
    @Local private var editing: JSON?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Agent").font(.display(.title3))
                    Text(L.text("每个 Agent 在独立的进程和配置目录里运行，不影响你本机的 Agent。")).font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button(L.text("添加 Agent")) { editing = newDraft(store.connections.isEmpty ? "codex" : "claude") }
            }
            if store.connections.isEmpty {
                Card(padding: 20) {
                    Text(L.text("还没有接入 Agent")).font(.display(.title3))
                    Text(L.text("把本机在用的 Agent 搬过来，配置和凭据一起带上。")).foregroundStyle(.secondary)
                    HStack {
                        ForEach(["codex", "claude"].compactMap { store.kind($0) }, id: \.self) { k in
                            Button { editing = newDraft(k["id"].str) } label: {
                                HStack {
                                    Avatar(initials: k["init"].str, color: k["color"].str, size: 28)
                                    VStack(alignment: .leading) { Text(k["name"].str).fontWeight(.semibold); Text(L.text("复制本机配置")).font(.caption).foregroundStyle(.secondary) }
                                }
                                .padding(8)
                            }
                            .help(L.text("复制本机 \(k["name"].str) 的配置和凭据"))
                        }
                    }
                    Button(L.text("用 API Key 或其他方式添加")) { editing = newDraft("codex") }.buttonStyle(.link)
                }
            } else {
                ForEach(store.connections, id: \.self) { c in
                    VStack(alignment: .leading, spacing: 0) {
                        ConnectionRow(store: store, c: c) { editing = editDraft(c) } newRole: { store.newRole(connection: c["id"].str) }
                        if let a = store.adapters["adapters"].array.first(where: { $0["id"].str == c["base"].str }) {
                            Divider().padding(.horizontal, 12)
                            AdapterLine(store: store, a: a)
                        }
                    }
                    .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line.opacity(0.6)))
                }
            }
        }
        .sheet(isPresented: Binding(get: { editing != nil }, set: { if !$0 { editing = nil } })) {
            if let d = editing { ConnectionEditor(store: store, draft: d) }
        }
        .onAppear(perform: takeAgentRequest)
        .onChange(of: store.agentEditorRequest) { takeAgentRequest() }
    }

    /// Opens the agent editor another screen asked for: a new agent, or one by id.
    private func takeAgentRequest() {
        guard let request = store.agentEditorRequest else { return }
        store.agentEditorRequest = nil
        switch request {
        case let .new(kind, auth, template): editing = newDraft(kind, auth: auth, template: template)
        case let .existing(id): if let c = store.connection(id) { editing = editDraft(c) }
        }
    }

    private func newDraft(_ kind: String, auth: String? = nil, template: Bool = false) -> JSON {
        let k = store.kind(kind)
        let auth = auth ?? k?["auths"][0]["id"].str ?? ""
        let env = template ? Setup.withTemplate("", store.auth(kind, auth)?["template"].str ?? "") : ""
        return ["kind": JSON(kind), "auth": JSON(auth), "name": k?["name"] ?? "", "api": "openai-completions", "prompt": "", "envText": JSON(env)]
    }

    private func editDraft(_ c: JSON) -> JSON {
        var d = c
        d["envText"] = JSON(Setup.envText(c["env"]))
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
        let roles = store.roles.filter { $0.connection == c["id"].str }
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
                    .hoverable(6)
                }
                .buttonStyle(.plain)
                .help(L.text("编辑 \(c["name"].str)"))
                Spacer()
                if roles.isEmpty { Button(L.text("新建数字员工"), action: newRole).buttonStyle(.link) }
                else {
                    HStack(spacing: -4) { ForEach(roles.prefix(4), id: \.self) { r in Avatar(initials: r.initials, color: r.color, size: 20, role: r.id) } }
                    Text(verbatim: L.text("\(roles.count) 个数字员工")).font(.caption).foregroundStyle(.secondary)
                }
                Dot(tone: tone, text: label).frame(width: 90, alignment: .leading).accessibilityElement(children: .combine)
                Menu {
                    if let login { Button(String("\(c["loggedIn"].truthy ? L.text("重新登录") : L.text("登录")) \(login["label"].str)")) { Task { await store.login(c["id"].str) } } }
                    Button(L.text("检查配置")) { Task { await store.testConnection(c["id"].str) } }
                    Button(L.text("编辑…"), action: edit)
                    Button(L.text("在这个 Agent 上新建数字员工…"), action: newRole)
                    Divider()
                    Button(L.text("删除…"), role: .destructive) { remove() }
                } label: { Label(L.text("更多操作"), systemImage: "ellipsis.circle") }
                .labelStyle(.iconOnly)
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help(L.text("更多操作"))
            }
            extra(login)
        }
        .padding(12)
    }

    @ViewBuilder private func extra(_ login: JSON?) -> some View {
        if login != nil && c["login"]["state"].str == "running" {
            VStack(alignment: .leading, spacing: 6) {
                Text(verbatim: L.text("按提示完成 \(login!["label"].str) 登录，完成后自动检查。")).font(.callout)
                if c["loginLog"].truthy {
                    let log = c["loginLog"].str.replacingOccurrences(of: "\u{1b}\\[[0-9;]*m", with: "", options: .regularExpression)
                    CodeBlock(text: log, maxHeight: 160)
                    ForEach(urls(log), id: \.self) { u in Link(u, destination: URL(string: u)!).font(.callout) }
                }
            }
        } else if let login, !c["loggedIn"].truthy {
            HStack { Note(text: L.text("还没有登录 \(login["label"].str)。"), tone: .amber); Button(L.text("登录")) { Task { await store.login(c["id"].str) } } }
        } else if c["status"].str == "testing" {
            CheckList(c: c)
        } else if c["status"].str == "error" {
            HStack {
                Note(text: c["error"].str, tone: .red)
                Button(L.text("重新检查")) { Task { await store.testConnection(c["id"].str) } }
                Button(L.text("修改配置"), action: edit)
            }
        } else if c["status"].str == "untested" {
            HStack { Text(L.text("还没有检查配置，读到模型后才能给数字员工使用。")).font(.callout).foregroundStyle(.secondary); Button(L.text("检查")) { Task { await store.testConnection(c["id"].str) } }.buttonStyle(.link) }
        }
    }

    private func urls(_ s: String) -> [String] {
        guard let re = try? NSRegularExpression(pattern: #"https?://[^\s<]+"#) else { return [] }
        return re.matches(in: s, range: NSRange(s.startIndex..., in: s)).compactMap { Range($0.range, in: s).map { String(s[$0]) } }
    }

    /// The auth method's own `login` flag says whether Janus drives an account login (Codex ChatGPT).
    private var loginAuth: JSON? { store.kind(c["kind"].str)?["auths"].array.first { $0["id"].str == c["auth"].str && $0["login"].truthy } }

    /// The backend's single `phase` combines the check status with the account login, so the
    /// label cannot disagree with the buttons below it.
    private var status: (String, Tone) {
        switch c["phase"].str {
        case "logging_in": return (L.text("等待登录"), .amber)
        case "logged_out": return (L.text("未登录"), .amber)
        case "testing": return (L.text("检查中…"), .blue)
        case "ok": return (L.text("可用"), .green)
        case "error": return (L.text("失败"), .red)
        default: return (L.text("未检查"), .gray)
        }
    }

    private func sub(_ k: JSON?) -> String {
        let auth = k?["auths"].array.first { $0["id"].str == c["auth"].str }?["label"].string.map(L.catalogText)
        let model = c["options"]["model"]
        let perms = Setup.controls(c).filter { $0["permission"].truthy }
        let permText: String? = c["options"].isNull ? nil : !c["options"]["controls"].exists ? L.text("未读取权限模式") : perms.isEmpty ? L.text("无权限模式") : perms.map { $0["type"].str == "boolean" ? $0["name"].str : $0["options"].array.map { $0["name"].str }.joined(separator: " / ") }.joined(separator: L.text("；"))
        return [k?["name"].string, auth, model.exists ? L.text("\(model["options"].array.count) 个模型") : nil, permText].compactMap { $0 }.joined(separator: L.text("，"))
    }

    private func remove() {
        let used = store.roles.filter { $0.connection == c["id"].str }.map(\.name)
        let msg = used.isEmpty ? "" : L.text("这些数字员工正在使用它，删除后会变成未绑定，需要重新选择 Agent：\(used.joined(separator: L.text("、")))")
        guard confirm(L.text("删除「\(c["name"].str)」和它的独立目录（包括登录凭据）？"), msg, ok: L.text("删除")) else { return }
        Task {
            do { try await store.delete(.connections, c["id"].str); store.show(L.text("已删除 \(c["name"].str)")) } catch { await store.loadSetup(); store.show(error.localizedDescription) }
        }
    }
}

struct CheckList: View {
    let c: JSON
    static var steps: [String] { [L.text("运行组件"), L.text("运行时"), L.text("凭据"), L.text("启动 Agent"), L.text("打开会话"), L.text("试运行")] }

    var body: some View {
        let done = c["check"].array
        let testing = c["status"].str == "testing"
        let steps = Self.steps
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
    @Local private var envDrafts: [String: JSON] = [:]
    @Local private var preview: JSON?
    @Local private var saving = false
    @Local private var idTouched = false
    @Local private var showSource = false
    @Local private var advanced = false

    /// The draft as opened, to tell whether Cancel would throw edits away.
    @Local private var saved: JSON

    init(store: AppStore, draft: JSON) {
        self.store = store
        _draft = Local(initialValue: draft)
        _saved = Local(initialValue: draft)
    }

    /// The suggested ID fills in by itself, so it does not count as an edit.
    private var dirty: Bool {
        func edits(_ j: JSON) -> JSON { var o = j.object; o["newId"] = nil; return .object(o) }
        return edits(draft) != edits(saved) || (idTouched && !draft["id"].truthy) || secrets.values.contains { !$0.isEmpty }
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
                        Text(editing ? L.text("编辑 \(prev?["name"].str ?? k["name"].str)") : L.text("添加 Agent")).font(.display(.title3))
                        Text(L.catalogText(k["desc"].str)).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                }
                .padding(16)
                Divider()
                ScrollView { form(k, prev, editing).padding(18) }
                Divider()
                HStack {
                    Spacer()
                    Button(L.text("取消")) { if !dirty || confirmDiscard() { dismiss() } }.keyboardShortcut(.cancelAction)
                    Button(saving ? L.text("正在保存…") : editing ? L.text("保存") : L.text("创建")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(saving)
                        .help(L.text("保存后自动检查：运行时、凭据、启动、会话、一次真实回复，进度看卡片。"))
                }
                .padding(14)
            }
        }
        .frame(minWidth: 760, idealWidth: 860, minHeight: 480, idealHeight: 640)
        .onAppear { store.agentEditorEnv = draft["envText"].str }
        .task { await showSavedValues() }
        .onChange(of: draft["envText"].str) { store.agentEditorEnv = draft["envText"].str }
        .onDisappear { store.agentEditorEnv = nil }
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
            TextField(L.text("搜索 Agent 类型"), text: $query).textFieldStyle(.roundedBorder).padding(10)
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    let q = query.lowercased()
                    let items = store.meta["kinds"].array.filter {
                        q.isEmpty || "\(L.catalogText($0["name"].str)) \($0["id"].str) \(L.catalogText($0["desc"].str))".lowercased().contains(q)
                    }
                    ForEach(items, id: \.self) { t in
                        Button { pickType(t["id"].str) } label: {
                            HStack(spacing: 8) {
                                Avatar(initials: t["init"].str, color: t["color"].str, size: 20)
                                Text(L.catalogText(t["name"].str)).lineLimit(1)
                                Spacer()
                                if t["id"].str == draft["kind"].str { Image(systemName: "checkmark").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                            }
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(t["id"].str == draft["kind"].str ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 6))
                            .contentShape(Rectangle())
                            .hoverable(6)
                        }
                        .buttonStyle(.plain)
                        .disabled(editing)
                        .accessibilityAddTraits(t["id"].str == draft["kind"].str ? .isSelected : [])
                    }
                }
                .padding(.top, 4)
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
                field(L.text("名称")) { TextField(L.catalogText(k["name"].str), text: str("name")).textFieldStyle(.roundedBorder) }
                field("ID", hint: L.tilde(JanusHome.current().agent(editing ? draft["id"].str : draft["newId"].str).path)) {
                    if editing { TextField("ID", text: .constant(draft["id"].str)).labelsHidden().textFieldStyle(.roundedBorder).disabled(true).font(.system(.body, design: .monospaced)) }
                    else { TextField("ID", text: Binding(get: { draft["newId"].str }, set: { draft["newId"] = JSON($0); idTouched = !$0.isEmpty }), prompt: Text(verbatim: "codex")).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)) }
                }
            }
            if let auth {
                field(L.text("认证"), hint: L.catalogText(auth["desc"].str)) {
                    Picker(L.text("认证"), selection: Binding(get: { auth["id"].str }, set: { next in
                        draft["envText"] = environment(for: draft["kind"].str, auth: next)
                        draft["auth"] = JSON(next)
                    })) {
                        ForEach(auths, id: \.self) { a in Text(L.catalogText(a["label"].str)).tag(a["id"].str) }
                    }
                    .pickerStyle(.segmented).labelsHidden()
                }
                if auth["id"].str == "import" {
                    if showSource { ForEach(auth["fields"].array, id: \.self) { f in inputField(f, prev) } }
                    importPreview(k)
                } else if auth["env"].truthy {
                    envField(template: auth["template"].str)
                } else {
                    ForEach(auth["fields"].array, id: \.self) { f in inputField(f, prev) }
                }
                if auth["login"].truthy, let prev {
                    HStack {
                        Text(prev["loggedIn"].truthy ? L.text("已登录 \(auth["label"].str)。") : L.text("还没有登录。")).font(.callout).foregroundStyle(.secondary)
                        Button(prev["loggedIn"].truthy ? L.text("重新登录") : L.text("现在登录")) { Task { await store.login(prev["id"].str) } }.buttonStyle(.link)
                    }
                }
            }
            if let prev, !prev["check"].array.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    HStack { Text(L.text("配置检查")).font(.headline); Spacer(); if prev["status"].str != "testing" { Button(L.text("重新检查")) { Task { await store.testConnection(prev["id"].str) } }.buttonStyle(.link) } else { Text(L.text("检查中…")).font(.caption).foregroundStyle(.secondary) } }
                    CheckList(c: prev)
                }
                .padding(12)
                .background(Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            }
            DisclosureGroup(L.text("高级设置"), isExpanded: Binding(get: { advanced || draft["prompt"].truthy || draft["bin"].truthy }, set: { advanced = $0 })) {
                VStack(alignment: .leading, spacing: 12) {
                    if k["runtime"].truthy {
                        field(L.text("运行时路径")) { TextField(k["runtime"]["detected"].truthy ? k["runtime"]["detected"].str : k["runtime"]["placeholder"].str, text: str("bin")).textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)) }
                    }
                    field(L.text("自定义提示词"), hint: L.text("放在数字员工指令前面一起注入（Claude Code 走系统提示，Codex 走开发者指令）。")) {
                        TextEditor(text: str("prompt")).font(.body).frame(height: 70).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    }
                    if !store.envAuth(draft["kind"].str, draft["auth"].str) { envField(template: "") }
                }
                .padding(.top, 8)
            }
            // A sheet: the style set on the main window does not reach it.
            .disclosureGroupStyle(.row)
        }
    }

    private func field<C: View>(_ label: String, hint: String = "", @ViewBuilder content: () -> C) -> some View {
        field(label, hint: hint, accessory: { EmptyView() }, content: content)
    }

    /// A labelled field with something beside the label on the right, such as a button.
    private func field<A: View, C: View>(_ label: String, hint: String = "", @ViewBuilder accessory: () -> A, @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack { Text(label).font(.callout.weight(.semibold)); Spacer(); accessory() }
            content()
            if !hint.isEmpty { Text(hint).font(.caption).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func inputField(_ f: JSON, _ prev: JSON?) -> some View {
        let key = f["key"].str
        let name = L.catalogText(f["label"].str)
        let label = name + (f["required"].truthy ? "" : L.text("（可选）"))
        if f["type"].str == "select" {
            field(label) {
                Picker(name, selection: str(key)) { ForEach(f["options"].array, id: \.self) { o in Text(o[1].str).tag(o[0].str) } }.labelsHidden()
            }
        } else if f["type"].str == "secret" {
            let saved = prev?["secretsSet"].strings.contains(key) == true
            field(label, hint: L.catalogText(f["help"].str)) {
                SecureField(saved ? L.text("已保存，留空表示不修改") : f["placeholder"].str, text: Binding(get: { secrets[key] ?? "" }, set: { secrets[key] = $0 })).textFieldStyle(.roundedBorder)
            }
        } else {
            field(label, hint: L.catalogText(f["help"].str)) { TextField(f["placeholder"].str, text: str(key)).textFieldStyle(.roundedBorder) }
        }
    }

    /// The environment lines: the whole form of an auth set up by its environment (Claude's custom
    /// API, with its template one click away), else an advanced setting.
    private func envField(template: String) -> some View {
        let primary = store.envAuth(draft["kind"].str, draft["auth"].str)
        // A line with a name and no value keeps the saved value: shown when the values could not be read.
        let keepsSaved = draft["envText"].str.components(separatedBy: .newlines).contains { line in
            Setup.envName(line) != nil && !line.contains("=")
        }
        let hint = keepsSaved
            ? L.text("只写变量名的行保留原值；删掉整行就移除。")
            : L.text("每行一个 KEY=value，可以直接贴 export 行；写 KEY= 表示清空。")
        return field(L.text("环境变量"), hint: hint, accessory: {
            if !template.isEmpty {
                Button(L.text("填入模板")) { draft["envText"] = JSON(Setup.withTemplate(draft["envText"].str, template)) }
                    .buttonStyle(.link)
                    .help(L.text("把模板中还没有填写的变量加到末尾，已填写的不变"))
            }
        }) {
            TextEditor(text: str("envText"))
                .font(.system(.callout, design: .monospaced))
                .frame(minHeight: primary ? 155 : 70, maxHeight: 200)
                .overlay(alignment: .topLeading) {
                    if primary && draft["envText"].str.isEmpty {
                        Text(verbatim: "export ANTHROPIC_BASE_URL=https://api.example.com\nexport ANTHROPIC_AUTH_TOKEN=…\nexport ANTHROPIC_MODEL=…")
                            .font(.system(.callout, design: .monospaced)).foregroundStyle(.tertiary)
                            .padding(.horizontal, 6).allowsHitTesting(false).accessibilityHidden(true)
                    }
                }
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                .accessibilityLabel(L.text("环境变量"))
        }
    }

    /// Janus is local first: an agent's environment shows every value, its token included, read
    /// from this Mac's service when the editor opens (`GET /api/connections/{id}/env`). The text
    /// opened with names in place of private values; it changes only if nobody typed meanwhile.
    private func showSavedValues() async {
        let id = draft["id"].str
        guard !id.isEmpty, draft["env"].object.values.contains(.null),
              let r = try? await store.api.get("/api/connections/\(API.q(id))/env") else { return }
        let text = JSON(Setup.envText(r["env"]))
        if draft["envText"] == saved["envText"] { draft["envText"] = text }
        saved["envText"] = text
    }

    /// Keep pasted credentials with the provider and auth form where they were entered.
    private func environment(for kind: String, auth: String) -> JSON {
        let current = store.envAuth(draft["kind"].str, draft["auth"].str) ? "env" : draft["kind"].str
        let next = store.envAuth(kind, auth) ? "env" : kind
        if current == next { return draft["envText"] }
        envDrafts[current] = draft["envText"]
        return envDrafts[next] ?? ""
    }

    @ViewBuilder private func importPreview(_ k: JSON) -> some View {
        if let p = preview {
            if p["error"].truthy { Note(text: p["error"].str, tone: .red) }
            else if p["files"].array.isEmpty && p["env"].array.isEmpty {
                HStack { Note(text: L.text("在 \(p["source"].str) 没有找到 \(k["name"].str) 的配置。"), tone: .amber); if !showSource { Button(L.text("更改来源")) { showSource = true } } }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    HStack { Text(verbatim: L.text("从 \(L.tilde(p["source"].str)) 复制")).fontWeight(.semibold); if !showSource { Button(L.text("更改")) { showSource = true }.buttonStyle(.link) } }
                    ForEach(p["files"].array, id: \.self) { f in HStack { Text(f["file"].str).font(.system(.caption, design: .monospaced)); Text(f["note"].str).font(.caption).foregroundStyle(.secondary) } }
                    ForEach(p["env"].array, id: \.self) { e in HStack { Text("$" + e["name"].str).font(.system(.caption, design: .monospaced)); Text(L.text("shell 环境变量")).font(.caption).foregroundStyle(.secondary) } }
                }
                .padding(10)
                .background(Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
            }
        } else {
            Text(L.text("正在检查本机配置…")).font(.callout).foregroundStyle(.secondary)
        }
    }

    /// Switching type keeps the name the person typed and the prompt.
    private func pickType(_ v: String) {
        guard v != draft["kind"].str else { return }
        let k = store.kind(v)
        let oldKind = store.kind(draft["kind"].str)
        let keepName = draft["name"].truthy && draft["name"].str != oldKind?["name"].str
        let auth = k?["auths"][0]["id"] ?? ""
        var d: JSON = ["kind": JSON(v), "auth": auth, "name": keepName ? draft["name"] : (k?["name"] ?? ""), "api": "openai-completions", "prompt": draft["prompt"], "envText": environment(for: v, auth: auth.str)]
        if idTouched { d["newId"] = draft["newId"] }
        draft = d
        preview = nil
        showSource = false
    }

    /// Saving always checks: the editor closes and the card shows the check (or the login) as it runs.
    private func save() {
        saving = true
        var body = draft
        for (k, v) in secrets where !v.isEmpty { body[k] = JSON(v) }
        Task {
            do {
                let c = try await store.save(.connections, body)
                let id = c["id"].str
                let auth = store.kind(c["kind"].str)?["auths"].array.first { $0["id"].str == c["auth"].str && $0["login"].truthy }
                store.show(L.text("已保存 \(c["name"].str)，正在检查配置"))
                dismiss()
                if auth != nil && store.connection(id)?["loggedIn"].truthy != true { await store.login(id) } else { await store.testConnection(id) }
            } catch { store.show(error.localizedDescription) }
            saving = false
        }
    }
}
