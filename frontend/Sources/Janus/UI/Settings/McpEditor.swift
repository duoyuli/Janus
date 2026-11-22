import SwiftUI

/// Adding or editing an MCP server. The server is a form or, switched at the top, the same server
/// as JSON in Claude Code's `.mcp.json` shape: a valid edit there is the form's at once, and one
/// that isn't keeps the editor from going back to the form or saving. What the server is for stays
/// below either. Every server is for every digital worker, through the MCP router. A new server's JSON may hold several servers (a README's
/// `mcpServers`); saving adds them all. Values show and change like the rest of the server: they
/// are read when the editor opens and sent as they stand.
// @project-doc docs/domains/20261116-resource_library.md#mcp
struct McpEditor: View {
    let store: AppStore
    let start: McpDraft
    /// The server as the library lists it; nil for a new one.
    let item: JSON?
    /// Opens on the JSON rather than the form.
    var json = false
    /// What to fill in, said over the form: a pick from the catalog that needs a key.
    var note = ""
    /// Deletes the server, after asking; nil for a new one.
    var remove: (() -> Void)?

    enum Mode: Hashable { case form, json }

    @Environment(\.dismiss) private var dismiss
    @Local private var d = McpDraft()
    @Local private var mode: Mode = .form
    /// The command line as typed, so typing a space doesn't get tidied away.
    @Local private var line = ""
    @Local private var text = ""
    /// Why the JSON isn't one server's, while it isn't.
    @Local private var bad = ""
    /// The servers a new server's JSON holds, when it holds more than one.
    @Local private var several: [McpDraft] = []
    @Local private var saving = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            VStack(alignment: .leading, spacing: 14) {
                if !note.isEmpty { Note(text: note, tone: .blue) }
                if mode == .json { jsonFields } else { formFields }
                if several.isEmpty {
                    field(L.text("说明"), hint: L.text("可选：它做什么、什么时候用。经 MCP 路由时，Agent 在 janus-tools 的说明里读到它。")) {
                        TextField(L.text("例如：店里的数据库，只读"), text: $d.description).textFieldStyle(.roundedBorder)
                    }
                    if d.transport == .stdio { processField }
                }
            }
            .padding(.horizontal, 22).padding(.vertical, 18)
            Divider()
            footer
        }
        .frame(width: 660)
        .onAppear {
            d = start
            line = start.line
            text = start.json
            if json { mode = .json }
        }
    }

    // MARK: Parts

    private var header: some View {
        HStack(spacing: 10) {
            Image(systemName: d.transport == .stdio ? "terminal" : "globe")
                .foregroundStyle(.secondary)
                .frame(width: 28, height: 28)
                .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 7))
            VStack(alignment: .leading, spacing: 1) {
                Text(d.name.isEmpty ? (item == nil ? L.text("添加 MCP 服务器") : L.text("MCP 服务器")) : d.name)
                    .font(.headline).lineLimit(1)
                if item == nil {
                    Text(L.text("填表单，或切到 JSON 贴 README 里的配置")).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            Picker(L.text("编辑方式"), selection: Binding(get: { mode }, set: switchTo)) {
                Text(L.text("表单")).tag(Mode.form)
                Text("JSON").tag(Mode.json)
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize()
        }
        .padding(.horizontal, 22).padding(.vertical, 14)
    }

    @ViewBuilder private var formFields: some View {
        field(L.text("名称"), hint: L.text("字母、数字、- 和 _，Agent 用这个名字称呼它")) {
            TextField(L.text("例如 github"), text: $d.name).textFieldStyle(.roundedBorder)
        }
        field(L.text("运行方式"), hint: d.transport == .stdio ? "" : L.text("Janus 连接这个地址，带上下面的 header；Agent 只看到 janus-tools。")) {
            Picker(L.text("运行方式"), selection: $d.transport) {
                Text(L.text("命令")).tag(McpDraft.Transport.stdio)
                Text("HTTP").tag(McpDraft.Transport.http)
                Text("SSE").tag(McpDraft.Transport.sse)
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize()
        }
        if d.transport == .stdio {
            field(L.text("命令"), hint: L.text("命令和它的参数，像在终端里那样写；含空格的参数加引号")) {
                TextField("npx -y @modelcontextprotocol/server-github", text: $line)
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                    .onChange(of: line) { d.setLine(line) }
            }
            field(L.text("环境变量"), hint: valuesHint) { pairs(key: "GITHUB_TOKEN") }
        } else {
            field("URL") {
                TextField("https://example.com/mcp", text: $d.url)
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
            }
            field(L.text("请求头"), hint: valuesHint) { pairs(key: "Authorization") }
        }
    }

    private var valuesHint: String {
        L.text("值只存在本机（权限 0600），会话启动时交给 Agent，不进数据库和日志。")
    }

    @ViewBuilder private var jsonFields: some View {
        field("JSON", hint: bad.isEmpty
            ? (item == nil
                ? L.text("兼容 Claude Code 的 .mcp.json 和 VS Code 的 servers，可以一次贴多个。")
                : L.text("与 Claude Code 的 .mcp.json 格式相同，值也在里面。"))
            : bad, warn: !bad.isEmpty) {
            TextEditor(text: $text)
                .font(.system(.callout, design: .monospaced))
                .scrollContentBackground(.hidden)
                .padding(6)
                .frame(height: 230)
                .background(Color.card, in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(bad.isEmpty ? Color.line : Tone.red.color))
                .onChange(of: text) { read() }
        }
        if !several.isEmpty {
            field("") {
                Note(text: L.text("JSON 里有 \(several.count) 个服务器：\(several.map(\.name).joined(separator: L.text("、")))。保存时全部添加，重名自动加后缀。"), tone: .blue)
            }
        }
    }

    private var processField: some View {
        field(L.text("进程"), hint: L.text("浏览器、终端这类有状态的服务器打开这项，每个会话各开一份。")) {
            Toggle(L.text("每个会话单独运行"), isOn: Binding(get: { d.process == .perSession }, set: { d.process = $0 ? .perSession : .perRun }))
                .toggleStyle(.switch).controlSize(.small)
        }
    }

    private var footer: some View {
        HStack(spacing: 10) {
            if let remove {
                Button(L.text("从资源库删除…")) { dismiss(); remove() }
                    .buttonStyle(.link).foregroundStyle(Tone.red.color)
            }
            Spacer()
            Button(L.text("取消")) { dismiss() }.keyboardShortcut(.cancelAction)
            Button(saving ? L.text("正在保存…") : several.isEmpty ? L.text("保存") : L.text("添加 \(several.count) 个服务器")) { save() }
                .buttonStyle(.primary).keyboardShortcut(.defaultAction)
                .disabled(saving || !bad.isEmpty)
        }
        .padding(.horizontal, 22).padding(.vertical, 14)
    }

    /// A row of the form: the label on the left, the control and a hint under it on the right.
    private func field(_ label: String, hint: String = "", warn: Bool = false, @ViewBuilder _ content: () -> some View) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 14) {
            Text(label).font(.callout).foregroundStyle(.secondary)
                .frame(width: 72, alignment: .trailing)
            VStack(alignment: .leading, spacing: 5) {
                content()
                if !hint.isEmpty {
                    Text(hint).font(.caption).foregroundStyle(warn ? Tone.red.color : .secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// Name and value rows for the environment (stdio) or the headers (http, sse).
    private func pairs(key: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(d.vars.indices, id: \.self) { i in
                HStack(spacing: 6) {
                    TextField(key, text: varName(i))
                        .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced)).frame(width: 200)
                    TextField(L.text("值"), text: varValue(i))
                        .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                    Button { d.vars.remove(at: i) } label: { Image(systemName: "xmark").font(.caption) }
                        .buttonStyle(.borderless).help(L.text("移除这个变量和它的值"))
                }
            }
            Button(L.text("+ 添加")) { d.vars.append(McpDraft.Var(name: "")) }.buttonStyle(.link)
        }
    }

    private func varName(_ i: Int) -> Binding<String> {
        Binding(get: { i < d.vars.count ? d.vars[i].name : "" }, set: { if i < d.vars.count { d.vars[i].name = $0 } })
    }

    private func varValue(_ i: Int) -> Binding<String> {
        Binding(get: { i < d.vars.count ? d.vars[i].value : "" }, set: { if i < d.vars.count { d.vars[i].value = $0 } })
    }

    // MARK: Actions

    /// Going to JSON writes the form out; back to the form needs the JSON to be one server.
    private func switchTo(_ next: Mode) {
        guard next != mode else { return }
        if next == .json {
            text = d.json
            bad = ""
        } else {
            guard bad.isEmpty, several.isEmpty else {
                if bad.isEmpty { bad = L.text("这里有多个服务器，表单一次只能编辑一个") }
                return
            }
            line = d.line
        }
        mode = next
    }

    /// Reads the JSON as it is typed: one server goes into the form at once.
    private func read() {
        guard let found = McpDraft.servers(in: text) else {
            bad = L.text("这不是 MCP 服务器的 JSON")
            several = []
            return
        }
        if found.count > 1 && item != nil {
            bad = L.text("这里只能是这一个服务器")
            return
        }
        bad = ""
        several = found.count > 1 ? found : []
        if found.count == 1 { d.take(found[0]) }
    }

    private func save() {
        saving = true
        Task {
            defer { saving = false }
            do {
                var saved: [JSON] = []
                var warnings: [String] = []
                if several.isEmpty {
                    saved = [d.id.isEmpty
                        ? try await store.api.post("/api/library/mcp", d.body)
                        : try await store.api.put("/api/library/mcp/\(d.id)", d.body)]
                } else {
                    let r = try await store.api.post("/api/library/mcp/import", ["text": JSON(text)])
                    saved = r["items"].array
                    warnings = r["warnings"].strings
                }
                await store.loadLibrary()
                let names = saved.map { $0["name"].str }.joined(separator: L.text("、"))
                store.show(warnings.isEmpty ? L.text("已保存 \(names)") : L.text("已添加 \(names)；跳过：\(warnings.joined(separator: L.text("；")))"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}
