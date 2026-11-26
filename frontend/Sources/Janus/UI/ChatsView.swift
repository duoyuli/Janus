import AppKit
import SwiftUI

/// Start a chat, laid out like a workspace page: the title and what a chat is at the top, one card
/// per digital employee below, two to a row. A click picks one and 「开始对话」 under the cards opens
/// the chat with it (a double click does both). Its session starts through ACP with the system
/// layer in place, and the person writes first. The chats so far are in the sidebar. A chat is a
/// run of its own kind (docs/domains/20261120-chats.md): its page is `ChatView`.
// @project-doc docs/domains/20261120-chats.md#chat_app
struct ChatsView: View {
    let store: AppStore
    /// The employee a chat is being started with, so nothing else starts meanwhile.
    @Local private var starting: String?

    var body: some View {
        let roles = store.roles
        let chosen = Self.chosen(store.chatRole, in: roles, connection: store.connection)
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(L.text("新对话")).font(.display(.largeTitle))
                    Text(L.text("选一名数字员工一对一聊。它带着自己的指令和资源库，在专属目录里干活，不带团队和看板。"))
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                if roles.isEmpty {
                    HStack {
                        Note(text: L.text("还没有数字员工。"), tone: .amber)
                        Button(L.text("打开数字员工设置")) { store.openSettings(.agents) }
                    }
                } else {
                    // By group, as in Settings › 数字员工; a single group needs no heading.
                    let sections = store.roleSections
                    VStack(alignment: .leading, spacing: 16) {
                        ForEach(sections, id: \.group) { section in
                            VStack(alignment: .leading, spacing: 8) {
                                if sections.count > 1 { Text(AppStore.groupTitle(section.group)).font(.headline) }
                                LazyVGrid(columns: [GridItem(.adaptive(minimum: 300), spacing: 12, alignment: .top)], spacing: 12) {
                                    ForEach(section.roles, id: \.self) { r in
                                        RoleChoice(store: store, role: r, selected: chosen?.id == r.id, starting: starting == r.id,
                                                   pick: { store.chatRole = r.id }, start: { start(r) })
                                    }
                                }
                            }
                        }
                    }
                    .disabled(starting != nil)
                    // Under the cards' right edge, where the eye ends after picking one.
                    HStack(alignment: .center) {
                        Text(L.text("双击卡片直接开始")).font(.caption).foregroundStyle(.tertiary)
                        Spacer()
                        Button { if let chosen { start(chosen) } } label: {
                            HStack(spacing: 6) {
                                if starting != nil { ProgressView().controlSize(.small) }
                                Text(starting != nil ? L.text("正在打开…") : chosen.map { L.text("和 \($0.name) 开始对话") } ?? L.text("开始对话"))
                            }
                            .frame(minWidth: 160)
                        }
                        .buttonStyle(.primary)
                        .controlSize(.large)
                        .keyboardShortcut(.defaultAction)
                        .disabled(chosen == nil || starting != nil)
                        .help(chosen == nil ? L.text("没有可以运行的数字员工") : L.text("开聊，你先说（↩）"))
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }

    /// The employee a new chat is with: the one picked last if it can still run, otherwise the
    /// first that can.
    static func chosen(_ id: String, in roles: [Role], connection: (String) -> JSON?) -> Role? {
        let ready = roles.filter { unavailable($0, connection: connection) == nil }
        return ready.first { $0.id == id } ?? ready.first
    }

    private func start(_ role: Role) {
        store.chatRole = role.id
        starting = role.id
        Task {
            await store.createChat(role: role.id)
            starting = nil
        }
    }

    /// Why a digital employee cannot chat now: no agent bound, or its agent not checked yet.
    static func unavailable(_ role: Role, connection: (String) -> JSON?) -> String? {
        guard let c = connection(role.connection) else { return L.text("还没有绑定 Agent") }
        return c["phase"].str == "ok" ? nil : L.text("Agent「\(c["name"].str)」还没通过检查")
    }

    /// "Claude Code · claude-sonnet · Bypass permissions": who does the work, in one line.
    static func setup(_ role: Role, store: AppStore) -> String {
        let conn = store.connection(role.connection)
        let parts = [conn?["name"].str ?? "", role.model, Setup.permLabel(role.session, conn)]
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// One digital employee to chat with: who it is, what it does, and why it cannot run if it can't.
private struct RoleChoice: View {
    let store: AppStore
    let role: Role
    let selected: Bool
    /// Its chat is being opened.
    let starting: Bool
    let pick: () -> Void
    let start: () -> Void

    var body: some View {
        let why = ChatsView.unavailable(role, connection: store.connection)
        let shape = RoundedRectangle(cornerRadius: 12)
        Button(action: pick) {
            HStack(alignment: .top, spacing: 12) {
                Avatar(initials: role.initials.isEmpty ? String(role.name.prefix(2)) : role.initials, color: role.color, size: 36, role: role.id)
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(role.name).font(.headline).lineLimit(1)
                        Spacer(minLength: 0)
                        if starting { ProgressView().controlSize(.small) }
                        else if selected { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
                    }
                    Text(role.desc.isEmpty ? L.text("没有写职责") : role.desc)
                        .font(.callout).foregroundStyle(.secondary).lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                    if let why { Text(why).font(.caption).foregroundStyle(Tone.amber.color).lineLimit(1) }
                }
            }
            .padding(14)
            // As tall as the tallest card in its row, so a row reads as one band.
            .frame(maxWidth: .infinity, minHeight: 96, maxHeight: .infinity, alignment: .topLeading)
            .background(selected ? Color.accentColor.opacity(0.06) : Color.card, in: shape)
            .overlay(shape.strokeBorder(selected ? Color.accentColor : Color.line, lineWidth: selected ? 1.5 : 1))
            .contentShape(shape)
            .opacity(why == nil ? 1 : 0.55)
        }
        .buttonStyle(.plain)
        // A double click picks and starts at once.
        .simultaneousGesture(TapGesture(count: 2).onEnded { if why == nil { start() } })
        .hoverable(12)
        .disabled(why != nil)
        .help(why.map { L.text("\($0)：去「设置 › 数字员工」处理") } ?? ChatsView.setup(role, store: store))
        .accessibilityLabel(role.name)
        .accessibilityValue(why ?? (starting ? L.text("正在打开") : selected ? L.text("已选择") : ""))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A chat is talking while its session is open; otherwise the next message reopens it.
enum ChatRowStatus {
    static func label(_ s: RunStatus) -> (String, Tone) {
        s.isActive ? (L.text("会话中"), .blue) : (L.text("已结束"), .gray)
    }

    /// A chat is listed by its first message; before there is one, by who it is with.
    static func title(_ chat: JSON) -> String {
        let goal = chat["goal"].str
        return goal.isEmpty ? L.text("和 \(chat["config"]["team"][0]["name"].str) 的新对话") : L.goalTitle(goal)
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
                Avatar(initials: me.initials, color: me.color, size: 34, role: me.role)
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
                .help(L.text("在 Finder 中显示工作目录（对话专用，随对话删除）"))
            }
            if st.run.status == .failed && !st.run.error.isEmpty {
                Note(text: L.text("会话已结束：\(st.run.error)"), tone: .amber)
            }
            if let p = run.proposal {
                let names = p["roles"].array.map { $0["name"].str }
                HStack(spacing: 10) {
                    Image(systemName: "person.crop.rectangle.stack").foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(L.text("\(me.name) 提议了 \(names.count) 位数字员工")).font(.callout).fontWeight(.semibold)
                        Text(verbatim: names.joined(separator: L.text("、"))).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                    Spacer()
                    Button(L.text("不用了")) { Task { await run.store.declineProposal(run) } }
                    Button(L.text("查看并创建…")) { Task { await run.store.openProposal(run) } }.buttonStyle(.primary)
                }
                .padding(10)
                .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line.opacity(0.6)))
            }
            ChatCard(run: run, st: st, title: me.name, info: info, target: .agent("lead"))
                .frame(maxHeight: .infinity)
        }
        // Read again from the server when the chat proposes or the person acts on a proposal.
        .task(id: "\(run.id)|\(run.state.events.filter { [.rolesProposed, .rolesCreated, .rolesDeclined].contains($0.type) }.count)") {
            await run.loadProposal()
        }
        .padding(20)
        .frame(maxWidth: 980, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
    }
}

extension RunModel {
    /// The chat's open proposal, as the server has it.
    // @project-doc docs/domains/20261120-chats.md#chat_app
    func loadProposal() async {
        guard let p = try? await api.get("/api/runs/\(API.q(id))/proposal") else { return }
        let open: JSON? = p.isNull ? nil : p
        // Set only on a change: every set redraws the chat, which would pull it off where it follows.
        if proposal != open { proposal = open }
    }

    /// A chat's title: its first message as soon as it is sent (the list's copy follows on the next
    /// refresh), otherwise who it is with.
    var chatTitle: String {
        if let first = state.events.first(where: { $0.type == .userMessage }) { return L.goalTitle(first.data["text"].str) }
        return ChatRowStatus.title(runJSON)
    }
}
