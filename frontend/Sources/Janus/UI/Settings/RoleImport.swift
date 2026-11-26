import AppKit
import SwiftUI

/// Import digital employees from role files: a folder (every `.md` with role front matter under
/// it, grouped by the folder it is in) or one file, from anywhere — written by hand, exported from
/// another Mac, a Claude Code subagent. The same sheet shows the digital employees a chat proposed
/// (`RoleImport.chat`). Each one is shown before anything is saved; those whose name is taken
/// start unticked, and importing never replaces anyone. The whole batch goes on one agent, chosen
/// here.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_files
struct RoleImportSheet: View {
    let store: AppStore
    @Environment(\.dismiss) private var dismiss
    @Local private var open: Set<String> = []
    @Local private var importing = false

    private var im: AppStore.RoleImport { store.roleImport ?? .init(path: "", connection: "") }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(proposed ? L.text("创建数字员工") : L.text("导入数字员工")).font(.display(.title2))
                Text(proposed ? L.text("对话里提议的数字员工。看过指令，勾选要的人，选好由哪个 Agent 来做。")
                     : L.text("每位数字员工一个 Markdown 文件：开头是名字、职责和分组，正文是指令。Claude Code 的 subagent 文件也能直接导入。"))
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(20)
            Divider()
            if !proposed {
                HStack {
                    Text(im.path.isEmpty ? L.text("还没有选择文件夹") : L.tilde(im.path))
                        .font(.system(.body, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                        .foregroundStyle(im.path.isEmpty ? .secondary : .primary)
                    Spacer()
                    Button(L.text("选择文件夹或文件…")) { pick() }
                }
                .padding(.horizontal, 20).padding(.vertical, 12)
                Divider()
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) { list }.padding(20)
            }
            Divider()
            footer.padding(16)
        }
        .frame(minWidth: 640, idealWidth: 700, minHeight: 480, idealHeight: 640)
        .onAppear { if im.path.isEmpty && !proposed { pick() } }
    }

    private var found: [JSON] { im.found }
    /// Showing a chat's proposal rather than files.
    private var proposed: Bool { im.chat != nil }

    @ViewBuilder private var list: some View {
        if let error = im.error { Note(text: error, tone: .red) }
        if im.scan != nil && found.isEmpty && im.error == nil {
            Note(text: L.text("这里没有数字员工文件：每个文件开头要有 --- 包起来的 name 和职责，后面是指令。"), tone: .amber)
        }
        let groups = Array(Set(found.map { $0["group"].str })).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        ForEach(groups, id: \.self) { g in
            VStack(alignment: .leading, spacing: 0) {
                Text(AppStore.groupTitle(g)).font(.headline).padding(.bottom, 6)
                ForEach(found.filter { $0["group"].str == g }, id: \.self) { f in row(f); Divider() }
            }
        }
        if im.scan?["limited"].truthy == true {
            Note(text: L.text("文件太多或目录太深，只列出了一部分；可以选更具体的文件夹。"), tone: .amber)
        }
        let skipped = im.scan?["skipped"].array ?? []
        if !skipped.isEmpty {
            Note(text: L.text("读不了的文件：") + skipped.map { "\($0[0].str)（\($0[1].str)）" }.joined(separator: "；"), tone: .amber)
        }
    }

    private func row(_ f: JSON) -> some View {
        let file = f["file"].str
        let name = f["name"].str
        return VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 10) {
                Toggle(L.text("导入 \(name)"), isOn: Binding(get: { im.picked.contains(file) }, set: { if $0 { store.roleImport?.picked.insert(file) } else { store.roleImport?.picked.remove(file) } }))
                    .labelsHidden()
                Avatar(initials: f["init"].truthy ? f["init"].str : String(name.prefix(2)).uppercased(), color: f["color"].truthy ? f["color"].str : "#64748b", size: 26)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(name).fontWeight(.semibold)
                        if f["lead"].truthy { Pill(L.text("负责人"), .violet) }
                        if f["existing"].truthy { Pill(L.text("已有同名"), .amber) }
                    }
                    Text(f["desc"].truthy ? f["desc"].str : L.text("没有写职责")).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    if !proposed { Text(file).font(.caption2.monospaced()).foregroundStyle(.tertiary) }
                }
                Spacer()
                Button { if open.contains(file) { open.remove(file) } else { open.insert(file) } } label: {
                    HStack(spacing: 4) { Text(L.text("指令")); DisclosureChevron(open: open.contains(file)) }
                }
                .buttonStyle(.borderless)
                .help(L.text("导入前看看它的指令"))
            }
            if open.contains(file) {
                ScrollView {
                    Text(f["prompt"].str).font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220)
                .padding(10)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
            }
        }
        .padding(.vertical, 8)
    }

    private var footer: some View {
        HStack(spacing: 10) {
            Text(L.text("由谁来做")).foregroundStyle(.secondary)
            Picker("Agent", selection: Binding(get: { im.connection }, set: { store.roleImport?.connection = $0 })) {
                Text(L.text("稍后再选")).tag("")
                ForEach(store.connections, id: \.self) { c in Text(c["name"].str).tag(c["id"].str) }
            }
            .labelsHidden()
            .frame(maxWidth: 200)
            .help(L.text("这一批都用这个 Agent 的默认模型和权限，之后可以逐个改"))
            Spacer()
            Button(L.text("取消")) { dismiss() }.keyboardShortcut(.cancelAction)
            Button(importing ? L.text("正在保存…") : proposed ? L.text("创建 \(im.picked.count) 位") : L.text("导入 \(im.picked.count) 位")) { save() }
                .buttonStyle(.primary).keyboardShortcut(.defaultAction)
                .disabled(im.picked.isEmpty || importing)
        }
    }

    private func pick() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = true
        p.allowsMultipleSelection = false
        p.prompt = L.text("读取")
        p.message = L.text("选一个放着数字员工文件的文件夹，或其中一个 .md 文件")
        if p.runModal() == .OK, let u = p.url {
            store.roleImport?.path = u.path
            open = []
            Task { await store.scanRoleImport() }
        } else if im.path.isEmpty {
            dismiss()
        }
    }

    private func save() {
        importing = true
        Task {
            await store.finishRoleImport()
            importing = false
        }
    }
}
