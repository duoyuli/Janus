import AppKit
import SwiftUI

/// Talk with one digital employee: choose who, write the first message, and find the recent chats.
/// A chat is a run of its own kind (docs/domains/20261120-chats.md): its page is `ChatView`.
// @project-doc docs/domains/20261120-chats.md#chat_app
struct ChatsView: View {
    let store: AppStore

    var body: some View {
        let roles = store.roles
        let chosen = Self.chosen(store.chatRole, in: roles, connection: store.connection)
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(L.text("对话")).font(.display(.largeTitle))
                    Text(L.text("和一名数字员工单独对话：它带着自己的指令、模型、权限和资源库，在为这次对话新建的工作目录里工作，没有团队和看板。"))
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }

                if roles.isEmpty {
                    HStack {
                        Note(text: L.text("还没有数字员工。"), tone: .amber)
                        Button(L.text("打开数字员工设置")) { store.openSettings(.agents) }
                    }
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(L.text("和谁对话")).font(.headline)
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 10)], spacing: 10) {
                            ForEach(roles, id: \.self) { r in
                                RoleChoice(store: store, role: r, selected: chosen?["id"].str == r["id"].str) { store.chatRole = r["id"].str }
                            }
                        }
                    }
                    composer(chosen)
                }

                recent
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    /// The employee a new chat is with: the one chosen last if it can still run, otherwise the
    /// first that can.
    static func chosen(_ id: String, in roles: [JSON], connection: (String) -> JSON?) -> JSON? {
        let ready = roles.filter { unavailable($0, connection: connection) == nil }
        return ready.first { $0["id"].str == id } ?? ready.first
    }

    /// Why a digital employee cannot chat now: no agent bound, or its agent not checked yet.
    static func unavailable(_ role: JSON, connection: (String) -> JSON?) -> String? {
        guard let c = connection(role["connection"].str) else { return L.text("还没有绑定 Agent") }
        return c["phase"].str == "ok" ? nil : L.text("Agent「\(c["name"].str)」还没有测试通过")
    }

    private func composer(_ chosen: JSON?) -> some View {
        let name = chosen?["name"].str ?? ""
        let empty = store.chatDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && store.chatImages.isEmpty
        return VStack(alignment: .leading, spacing: 10) {
            Composer(
                text: Binding(get: { store.chatDraft }, set: { store.chatDraft = $0 }),
                images: Binding(get: { store.chatImages }, set: { store.chatImages = $0 }),
                placeholder: chosen == nil ? L.text("先选一名可以运行的数字员工") : L.text("给 \(name) 发消息，可以 ⌘V 粘贴截图，⌘↩ 开始对话"),
                disabled: chosen == nil,
                minHeight: 80
            ) { start(chosen) }
            HStack {
                if let chosen { Text(verbatim: Self.setup(chosen, store: store)).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                Spacer()
                Button(L.text("开始对话")) { start(chosen) }
                    .buttonStyle(.primary)
                    .disabled(chosen == nil || empty)
                    .help(chosen == nil ? L.text("没有可以运行的数字员工") : empty ? L.text("先写下第一条消息") : "⌘↩")
            }
        }
    }

    /// "Claude Code · claude-sonnet · Bypass permissions": who does the work, in one line.
    static func setup(_ role: JSON, store: AppStore) -> String {
        let conn = store.connection(role["connection"].str)
        let parts = [conn?["name"].str ?? "", role["model"].str, Setup.permLabel(role["session"], conn)]
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private func start(_ chosen: JSON?) {
        let text = store.chatDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        let images = store.chatImages
        guard let chosen, !text.isEmpty || !images.isEmpty else { return }
        Task { await store.createChat(role: chosen["id"].str, text: text, images: images) }
    }

    @ViewBuilder private var recent: some View {
        if !store.chats.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text(L.text("最近的对话")).font(.headline)
                Card(padding: 4) {
                    ForEach(store.chats, id: \.self) { c in
                        ChatRow(store: store, chat: c)
                        if c != store.chats.last { Divider() }
                    }
                }
            }
        }
    }
}

/// One digital employee to chat with: who it is, what it does, and why it cannot run if it can't.
private struct RoleChoice: View {
    let store: AppStore
    let role: JSON
    let selected: Bool
    let pick: () -> Void

    var body: some View {
        let why = ChatsView.unavailable(role, connection: store.connection)
        Button(action: pick) {
            HStack(alignment: .top, spacing: 10) {
                Avatar(initials: role["init"].truthy ? role["init"].str : String(role["name"].str.prefix(2)), color: role["color"].str, size: 30)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 4) {
                        Text(role["name"].str).fontWeight(.semibold).lineLimit(1)
                        if selected { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                    }
                    Text(role["desc"].str.isEmpty ? L.text("没有写职责") : role["desc"].str)
                        .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    if let why { Text(why).font(.caption).foregroundStyle(Tone.amber.color).lineLimit(1) }
                }
                Spacer(minLength: 0)
            }
            .padding(10)
            .frame(maxWidth: .infinity, minHeight: 64, alignment: .topLeading)
            .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(selected ? Color.accentColor : Color.line, lineWidth: selected ? 1.5 : 1))
            .contentShape(RoundedRectangle(cornerRadius: 10))
            .opacity(why == nil ? 1 : 0.55)
        }
        .buttonStyle(.plain)
        .hoverable(10)
        .disabled(why != nil)
        .help(why.map { L.text("\($0)：在设置的「数字员工」页处理") } ?? ChatsView.setup(role, store: store))
        .accessibilityLabel(role["name"].str)
        .accessibilityValue(why ?? (selected ? L.text("已选择") : ""))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A chat in the recent list: its first message, who it is with and when, and whether its session is open.
private struct ChatRow: View {
    let store: AppStore
    let chat: JSON

    var body: some View {
        let member = chat["config"]["team"][0]
        let (label, tone) = ChatRowStatus.label(RunStatus(chat["status"].str))
        Button { store.open(.run(chat["id"].str)) } label: {
            HStack(spacing: 10) {
                Avatar(initials: member["init"].truthy ? member["init"].str : String(member["name"].str.prefix(2)), color: member["color"].str, size: 24)
                VStack(alignment: .leading, spacing: 1) {
                    Text(L.goalTitle(chat["goal"].str)).lineLimit(1)
                    Text(verbatim: "\(member["name"].str) · \(L.dateTime(chat["createdAt"].double ?? 0))").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Pill(label, tone)
            }
            .padding(.horizontal, 10).padding(.vertical, 8)
            .contentShape(Rectangle())
            .hoverable(6)
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button(L.text("删除对话…"), role: .destructive) { Task { await store.deleteRun(chat["id"].str) } }
        }
    }
}

/// A chat is talking while its session is open; otherwise the next message reopens it.
enum ChatRowStatus {
    static func label(_ s: RunStatus) -> (String, Tone) {
        s.isActive ? (L.text("会话中"), .blue) : (L.text("已关闭"), .gray)
    }
}

/// An open chat: the employee, where it works, and the conversation, which is the same session
/// view as the lead's on the workbench.
// @project-doc docs/domains/20261120-chats.md#chat_app
struct ChatView: View {
    let run: RunModel

    var body: some View {
        let st = run.shown
        let me = run.who("lead")
        let info = chatSession(st, live: run.live)
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 12) {
                Avatar(initials: me.initials, color: me.color, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(me.name).font(.display(.title3))
                    Text(verbatim: [me.runtime, me.model, me.m["permission"].str].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                Button { NSWorkspace.shared.open(URL(fileURLWithPath: st.run.cwd)) } label: {
                    Label(L.tilde(st.run.cwd), systemImage: "folder")
                        .lineLimit(1).truncationMode(.middle)
                }
                .buttonStyle(.borderless)
                .disabled(st.run.cwd.isEmpty || !FileManager.default.fileExists(atPath: st.run.cwd))
                .help(L.text("在 Finder 中显示这次对话的工作目录：为它新建，删除对话时一起删除"))
            }
            if st.run.status == .failed && !st.run.error.isEmpty {
                Note(text: L.text("会话已结束：\(st.run.error)"), tone: .amber)
            }
            ChatCard(run: run, st: st, title: me.name, info: info, target: .agent("lead"))
                .frame(maxHeight: .infinity)
        }
        .padding(20)
        .frame(maxWidth: 980, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
    }
}
