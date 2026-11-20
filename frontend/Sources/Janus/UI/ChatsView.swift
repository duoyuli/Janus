import AppKit
import SwiftUI

/// Start a chat, in the middle of the page: one card per digital employee, a click picks one and
/// 「开始对话」 opens the chat with it (a double click does both). Its session starts through ACP with
/// the system layer in place, and the person writes first. The chats so far are in the sidebar. A
/// chat is a run of its own kind (docs/domains/20261120-chats.md): its page is `ChatView`.
// @project-doc docs/domains/20261120-chats.md#chat_app
struct ChatsView: View {
    let store: AppStore
    /// The employee a chat is being started with, so nothing else starts meanwhile.
    @Local private var starting: String?

    var body: some View {
        let roles = store.roles
        let chosen = Self.chosen(store.chatRole, in: roles, connection: store.connection)
        GeometryReader { geo in
            ScrollView {
                VStack(spacing: 18) {
                    VStack(spacing: 4) {
                        Text(L.text("和谁对话")).font(.display(.title2))
                        Text(L.text("选一名数字员工，开始后直接和它对话")).font(.callout).foregroundStyle(.secondary)
                    }
                    if roles.isEmpty {
                        HStack {
                            Note(text: L.text("还没有数字员工。"), tone: .amber)
                            Button(L.text("打开数字员工设置")) { store.openSettings(.agents) }
                        }
                    } else {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 10)], spacing: 10) {
                            ForEach(roles, id: \.self) { r in
                                RoleChoice(store: store, role: r, selected: chosen?["id"].str == r["id"].str, starting: starting == r["id"].str,
                                           pick: { store.chatRole = r["id"].str }, start: { start(r) })
                            }
                        }
                        .disabled(starting != nil)
                        Button { if let chosen { start(chosen) } } label: {
                            HStack(spacing: 6) {
                                if starting != nil { ProgressView().controlSize(.small) }
                                Text(starting != nil ? L.text("正在打开…") : chosen.map { L.text("和 \($0["name"].str) 开始对话") } ?? L.text("开始对话"))
                            }
                            .frame(minWidth: 160)
                        }
                        .buttonStyle(.primary)
                        .controlSize(.large)
                        .keyboardShortcut(.defaultAction)
                        .disabled(chosen == nil || starting != nil)
                        .help(chosen == nil ? L.text("没有可以运行的数字员工") : L.text("打开会话，注入数字员工的指令，然后由你先说（↩）"))
                    }
                }
                .padding(24)
                .frame(maxWidth: 820)
                .frame(maxWidth: .infinity, minHeight: geo.size.height)
            }
        }
    }

    /// The employee a new chat is with: the one picked last if it can still run, otherwise the
    /// first that can.
    static func chosen(_ id: String, in roles: [JSON], connection: (String) -> JSON?) -> JSON? {
        let ready = roles.filter { unavailable($0, connection: connection) == nil }
        return ready.first { $0["id"].str == id } ?? ready.first
    }

    private func start(_ role: JSON) {
        store.chatRole = role["id"].str
        starting = role["id"].str
        Task {
            await store.createChat(role: role["id"].str)
            starting = nil
        }
    }

    /// Why a digital employee cannot chat now: no agent bound, or its agent not checked yet.
    static func unavailable(_ role: JSON, connection: (String) -> JSON?) -> String? {
        guard let c = connection(role["connection"].str) else { return L.text("还没有绑定 Agent") }
        return c["phase"].str == "ok" ? nil : L.text("Agent「\(c["name"].str)」还没有测试通过")
    }

    /// "Claude Code · claude-sonnet · Bypass permissions": who does the work, in one line.
    static func setup(_ role: JSON, store: AppStore) -> String {
        let conn = store.connection(role["connection"].str)
        let parts = [conn?["name"].str ?? "", role["model"].str, Setup.permLabel(role["session"], conn)]
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// One digital employee to chat with: who it is, what it does, and why it cannot run if it can't.
private struct RoleChoice: View {
    let store: AppStore
    let role: JSON
    let selected: Bool
    /// Its chat is being opened.
    let starting: Bool
    let pick: () -> Void
    let start: () -> Void

    var body: some View {
        let why = ChatsView.unavailable(role, connection: store.connection)
        Button(action: pick) {
            HStack(alignment: .top, spacing: 10) {
                Avatar(initials: role["init"].truthy ? role["init"].str : String(role["name"].str.prefix(2)), color: role["color"].str, size: 30)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(role["name"].str).fontWeight(.semibold).lineLimit(1)
                        if starting { ProgressView().controlSize(.mini) }
                        else if selected { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor).accessibilityHidden(true) }
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
        // A double click picks and starts at once.
        .simultaneousGesture(TapGesture(count: 2).onEnded { if why == nil { start() } })
        .hoverable(10)
        .disabled(why != nil)
        .help(why.map { L.text("\($0)：在设置的「数字员工」页处理") } ?? ChatsView.setup(role, store: store))
        .accessibilityLabel(role["name"].str)
        .accessibilityValue(why ?? (starting ? L.text("正在打开") : selected ? L.text("已选择") : ""))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A chat is talking while its session is open; otherwise the next message reopens it.
enum ChatRowStatus {
    static func label(_ s: RunStatus) -> (String, Tone) {
        s.isActive ? (L.text("会话中"), .blue) : (L.text("已关闭"), .gray)
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

extension RunModel {
    /// A chat's title: its first message as soon as it is sent (the list's copy follows on the next
    /// refresh), otherwise who it is with.
    var chatTitle: String {
        if let first = state.events.first(where: { $0.type == "user.message" }) { return L.goalTitle(first.data["text"].str) }
        return ChatRowStatus.title(runJSON)
    }
}
