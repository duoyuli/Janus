import SwiftUI

/// Adding or editing an MCP server. The server is a form or, switched at the top, the same server
/// as JSON in Claude Code's `.mcp.json` shape: a valid edit there is the form's at once, and one
/// that isn't keeps the editor from going back to the form or saving. What the server is for and
/// who gets it stay below either. A new server's JSON may hold several servers (a README's
/// `mcpServers`); saving adds them all. Values show and change like the rest of the server: they
/// are read when the editor opens and sent as they stand.
// @project-doc docs/domains/20261116-resource_library.md#mcp
struct McpEditor: View {
    let store: AppStore
    let start: McpDraft
    /// The server as the library lists it, for who it is given to; nil for a new one.
    let item: JSON?
    /// Opens on the JSON rather than the form.
    var json = false
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
    @Local private var everyone = false
    @Local private var picked: Set<String> = []
    @Local private var saving = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            VStack(alignment: .leading, spacing: 14) {
                if mode == .json { jsonFields } else { formFields }
                if several.isEmpty {
                    field(L.text("说明"), hint: L.text("可选：它做什么、什么时候用。经 MCP 路由时，Agent 在 janus-tools 的说明里读到它。")) {
                        TextField(L.text("例如：店里的数据库，只读"), text: $d.description).textFieldStyle(.roundedBorder)
                    }
                    if d.transport == .stdio { processField }
                }
                reachField
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
            everyone = item?["reach"].str == "everyone"
            picked = Set(item?["roles"].array.map { $0["id"].str } ?? [])
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
                    Text(L.text("填写表单，或切到 JSON 粘贴 README 里的配置")).font(.caption).foregroundStyle(.secondary)
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
        field(L.text("运行方式"), hint: d.transport == .sse ? L.text("Codex 不支持 SSE，绑定 Codex 的数字员工拿不到它。") : "") {
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
        L.text("值只保存在本机的 Janus home（0600），会话启动时交给 Agent，不写进数据库、任务记录和事件。")
    }

    @ViewBuilder private var jsonFields: some View {
        field("JSON", hint: bad.isEmpty
            ? (item == nil
                ? L.text("与 Claude Code 的 .mcp.json 格式相同，也接受 VS Code 的 servers。可以一次粘贴多个服务器，保存时全部添加。")
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
                Note(text: L.text("JSON 里有 \(several.count) 个服务器：\(several.map(\.name).joined(separator: L.text("、")))。保存时全部添加，说明取 JSON 里写的，名字被占用时自动加后缀。"), tone: .blue)
            }
        }
    }

    private var processField: some View {
        field(L.text("进程"), hint: L.text("给所有数字员工时，默认同一个任务的会话共用一个进程，用到才启动。浏览器、终端这类会记住状态的服务器打开这一项，每个会话各开一份。")) {
            Toggle(L.text("每个会话单独运行"), isOn: Binding(get: { d.process == .perSession }, set: { d.process = $0 ? .perSession : .perRun }))
                .toggleStyle(.switch).controlSize(.small)
        }
    }

    /// Who gets the server: every digital worker through the MCP router, or the ones ticked here
    /// (the same choice as in each worker's editor).
    private var reachField: some View {
        field(L.text("给谁用"), hint: everyone
            ? L.text("每个任务的负责人和所有协作者都能调用它的工具，包括写数据、发消息这类有副作用的操作。")
            : L.text("只有勾选的数字员工拿到它；在数字员工编辑器里勾选是同一个选择。")) {
            VStack(alignment: .leading, spacing: 8) {
                Picker(L.text("给谁用"), selection: $everyone) {
                    Text(L.text("只给选中的数字员工")).tag(false)
                    Text(L.text("所有数字员工")).tag(true)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                if !everyone {
                    FlowLayout(spacing: 6) {
                        ForEach(store.roles, id: \.self) { r in chip(r) }
                    }
                }
            }
        }
    }

    private func chip(_ role: JSON) -> some View {
        let id = role["id"].str
        let on = picked.contains(id)
        return Button {
            if on { picked.remove(id) } else { picked.insert(id) }
        } label: {
            HStack(spacing: 5) {
                Image(systemName: on ? "checkmark.circle.fill" : "circle").foregroundStyle(on ? Color.accent : Color.secondary)
                Text(role["name"].str)
            }
            .font(.callout)
            .padding(.horizontal, 9).padding(.vertical, 4)
            .background(on ? Color.accent.opacity(0.1) : Color.secondary.opacity(0.06), in: Capsule())
            .overlay(Capsule().strokeBorder(on ? Color.accent.opacity(0.5) : Color.line))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(on ? .isSelected : [])
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
            Button(L.text("＋ 添加")) { d.vars.append(McpDraft.Var(name: "")) }.buttonStyle(.link)
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
        let wasEveryone = item?["reach"].str == "everyone"
        let wasPicked = Set(item?["roles"].array.map { $0["id"].str } ?? [])
        if everyone && !wasEveryone && !confirm(
            L.text("把 \(several.isEmpty ? d.name : several.map(\.name).joined(separator: L.text("、"))) 给所有数字员工？"),
            L.text("每个任务的负责人和所有协作者都能调用它的工具，包括写数据、发消息这类有副作用的操作。"), ok: L.text("给所有数字员工")) {
            return
        }
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
                for id in saved.map({ $0["id"].str }) {
                    if everyone != wasEveryone {
                        _ = try await store.api.post("/api/library/items/\(id)/reach", ["reach": JSON(everyone ? "everyone" : "picked")])
                    }
                    if !everyone && picked != wasPicked {
                        _ = try await store.api.post("/api/library/items/\(id)/roles", ["roles": JSON(picked.sorted())])
                    }
                }
                await store.loadLibrary()
                await store.loadSetup()
                let names = saved.map { $0["name"].str }.joined(separator: L.text("、"))
                store.show(warnings.isEmpty ? L.text("已保存 \(names)") : L.text("已添加 \(names)；跳过：\(warnings.joined(separator: L.text("；")))"))
                dismiss()
            } catch { store.show(error.localizedDescription) }
        }
    }
}
