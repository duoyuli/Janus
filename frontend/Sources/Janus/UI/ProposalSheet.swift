import SwiftUI

/// The digital employees a chat proposed, for the person to create: each one with its duty and
/// instructions to read, ticked or not (those whose name is taken start unticked), and one agent
/// for the whole batch. Nothing is saved until they choose Create; creating never replaces anyone.
// @project-doc docs/domains/20261120-chats.md#proposed_employees
struct ProposalSheetView: View {
    let store: AppStore
    @Environment(\.dismiss) private var dismiss
    @Local private var open: Set<String> = []
    @Local private var creating = false

    private var sheet: AppStore.ProposalSheet? { store.proposalSheet }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(L.text("创建数字员工")).font(.display(.title2))
                Text(L.text("对话里提议的数字员工。看过指令，勾选要的人，选好由哪个 Agent 来做。"))
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(20)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) { list }.padding(20)
            }
            Divider()
            footer.padding(16)
        }
        .frame(minWidth: 640, idealWidth: 700, minHeight: 480, idealHeight: 640)
    }

    @ViewBuilder private var list: some View {
        if let error = sheet?.error { Note(text: error, tone: .red) }
        let roles = sheet?.roles ?? []
        let groups = Array(Set(roles.map { $0["group"].str })).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        ForEach(groups, id: \.self) { g in
            VStack(alignment: .leading, spacing: 0) {
                Text(AppStore.groupTitle(g)).font(.headline).padding(.bottom, 6)
                ForEach(roles.filter { $0["group"].str == g }, id: \.self) { r in row(r); Divider() }
            }
        }
    }

    private func row(_ r: JSON) -> some View {
        let n = r["n"].str
        let name = r["name"].str
        return VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 10) {
                Toggle(L.text("创建 \(name)"), isOn: Binding(get: { sheet?.picked.contains(n) == true }, set: { if $0 { store.proposalSheet?.picked.insert(n) } else { store.proposalSheet?.picked.remove(n) } }))
                    .labelsHidden()
                Avatar(initials: Role.initials(r["init"].str, name: name), color: r["color"].truthy ? r["color"].str : "#64748b", size: 26)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(name).fontWeight(.semibold)
                        if r["lead"].truthy { Pill(L.text("负责人"), .violet) }
                        if r["existing"].truthy { Pill(L.text("已有同名"), .amber) }
                    }
                    Text(r["desc"].truthy ? r["desc"].str : L.text("没有写职责")).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer()
                Button { if open.contains(n) { open.remove(n) } else { open.insert(n) } } label: {
                    HStack(spacing: 4) { Text(L.text("指令")); DisclosureChevron(open: open.contains(n)) }
                }
                .buttonStyle(.borderless)
                .help(L.text("创建前看看它的指令"))
            }
            if open.contains(n) {
                ScrollView {
                    Text(r["prompt"].str).font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220)
                .padding(10)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
            }
        }
        .padding(.vertical, 8)
    }

    private var footer: some View {
        let picked = sheet?.picked.count ?? 0
        return HStack(spacing: 10) {
            Text(L.text("由谁来做")).foregroundStyle(.secondary)
            Picker("Agent", selection: Binding(get: { sheet?.connection ?? "" }, set: { store.proposalSheet?.connection = $0 })) {
                Text(L.text("稍后再选")).tag("")
                ForEach(store.connections, id: \.self) { c in Text(c["name"].str).tag(c["id"].str) }
            }
            .labelsHidden()
            .frame(maxWidth: 200)
            .help(L.text("这一批都用这个 Agent 的默认模型和权限，之后可以逐个改"))
            Spacer()
            Button(L.text("取消")) { dismiss() }.keyboardShortcut(.cancelAction)
            Button(creating ? L.text("正在保存…") : L.text("创建 \(picked) 位")) {
                creating = true
                Task {
                    await store.createProposed()
                    creating = false
                }
            }
            .buttonStyle(.primary).keyboardShortcut(.defaultAction)
            .disabled(picked == 0 || creating)
        }
    }
}
