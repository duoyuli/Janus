import AppKit
import SwiftUI

/// A workspace's team memory (its tile on the workspace page, the popover the tile opens), the
/// sheet to read and change one note, and the page of a consolidation.
// @project-doc docs/architecture/20261031-frontend.md#team_memory

/// The notes of one workspace, in the popover of its tile on the workspace page: what its runs
/// kept for the runs after them. A note is one line (kind and title); hovering shows the line the
/// index gives and who wrote it, clicking opens it. `leave` closes the popover before a sheet or
/// a page opens.
struct MemoryPanel: View {
    let store: AppStore
    let ws: Workspace
    var leave: () -> Void = {}
    @Local private var all = false
    private static let shown = 5

    var body: some View {
        let id = ws.id
        let notes = store.notes(of: id)
        let c = store.memories[id]?["consolidation"] ?? .null
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(L.text("团队记忆")).font(.headline)
                if !notes.isEmpty {
                    Text(verbatim: "\(notes.count)").font(.headline).foregroundStyle(.secondary).accessibilityLabel(L.text("\(notes.count) 条"))
                }
                HowItWorks(title: L.text("团队记忆"), lines: [
                    L.text("Agent 写的：负责人、协作者和值班在任务里记下之后还用得上的事（怎么跑测试、踩过的坑、为什么这样做），每条记着谁在哪个任务里写的。"),
                    L.text("不会塞进每个任务的说明里：Agent 只知道这里有多少条，需要时按关键词检索，拿到最相关的几条。它是笔记不是规则：用之前先核对，和共享资料冲突时以共享资料为准。"),
                    L.text("攒够 5 个交付的任务、且距上次超过一天，负责人自动整理一次：合并重复的、改正过时的、删掉没用的；也可以点右上角现在整理。"),
                    L.text("你可以改、删，或把一条加进共享资料，让它变成团队必须遵守的规则。"),
                ])
                Spacer()
                if let running = c["running"].string {
                    Button { leave(); store.open(.run(running)) } label: { Pill(L.text("整理中"), .blue) }.buttonStyle(.plain).help(L.text("打开这次整理"))
                } else if !notes.isEmpty {
                    IconButton(symbol: "arrow.triangle.2.circlepath", label: L.text("现在整理"),
                               help: L.text("现在整理：由负责人合并重复的、改正过时的、删掉没用的笔记")) { Task { await store.consolidate(id) } }
                }
            }
            if notes.isEmpty {
                Text(L.text("还没有笔记。Agent 在任务里记下的会出现在这里。")).font(.callout).foregroundStyle(.secondary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        let rows = all ? notes : Array(notes.prefix(Self.shown))
                        ForEach(rows, id: \.self) { m in
                            Button { leave(); store.editingMemory = m } label: {
                                HStack(spacing: 6) {
                                    Pill(MemoryText.kind(m["kind"].str), m["kind"].str == "pitfall" ? .amber : .gray)
                                    Text(m["title"].str).font(.callout).lineLimit(1).truncationMode(.tail)
                                    Spacer(minLength: 0)
                                }
                                .padding(.vertical, 5)
                                .contentShape(Rectangle())
                                .hoverable(5)
                            }
                            .buttonStyle(.plain)
                            .help("\(m["hook"].str)\n\(MemoryText.source(m))")
                            if m != rows.last { Divider() }
                        }
                    }
                }
                .frame(maxHeight: 360)
                HStack {
                    if notes.count > Self.shown {
                        Button(all ? L.text("收起") : L.text("展开全部（\(notes.count)）")) { all.toggle() }.buttonStyle(.link)
                    }
                    Spacer()
                    if c["running"].string == nil, let last = c["last"]["run"].string {
                        Button(L.text("上次整理：\(L.dateTime(c["last"]["at"].double ?? 0))")) { leave(); store.open(.run(last)) }.buttonStyle(.link)
                    }
                }
                .font(.caption)
            }
        }
    }
}

/// One note, to read and change: what it is, its title, the line the index shows and the note
/// itself; who wrote it last and where. Deleting it, or adding it to the shared material, which
/// every session then follows as the person's own.
// @project-doc docs/domains/20261123-team_memory.md#memory_review
struct MemoryEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var kind = "fact"
    @Local private var title = ""
    @Local private var hook = ""
    @Local private var text = ""
    @Local private var saving = false

    private var dirty: Bool {
        kind != original["kind"].str || title != original["title"].str || hook != original["hook"].str || text != original["body"].str
    }
    private var valid: Bool { ![title, hook, text].contains { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(L.text("团队记忆")).font(.display(.title2))
                Text(MemoryText.source(original)).font(.callout).foregroundStyle(.secondary)
            }
            .padding(20)
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Picker(L.text("类型"), selection: $kind) {
                        ForEach(MemoryText.kinds, id: \.self) { Text(MemoryText.kind($0)).tag($0) }
                    }
                    .labelsHidden().fixedSize()
                    TextField(L.text("标题"), text: $title).textFieldStyle(.roundedBorder)
                }
                TextField(L.text("一句话摘要：之后的任务检索到这条时先看到这一行"), text: $hook).textFieldStyle(.roundedBorder)
                TextEditor(text: $text)
                    .font(.body)
                    .frame(minHeight: 200)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
                    .accessibilityLabel(L.text("正文"))
                Text(L.text("这是 Agent 记的笔记，之后的任务读到后会先核对再用。要团队必须遵守，加入共享资料。"))
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(20)
            Divider()
            HStack {
                Button(L.text("删除…"), role: .destructive) {
                    Task { if await store.deleteMemory(original) { dismiss() } }
                }
                Button(L.text("加入共享资料…")) { addToBrief() }
                    .help(L.text("把这条笔记作为一行加进共享资料，保存后每个任务都按它来"))
                Spacer()
                Button(L.text("取消")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(saving ? L.text("正在保存…") : L.text("保存")) { save() }.buttonStyle(.primary).keyboardShortcut(.defaultAction)
                    .disabled(saving || !dirty || !valid)
            }
            .padding(16)
        }
        .frame(minWidth: 560, idealWidth: 600, minHeight: 440, idealHeight: 500)
        .onAppear {
            kind = original["kind"].str
            title = original["title"].str
            hook = original["hook"].str
            text = original["body"].str
        }
    }

    private func save() {
        saving = true
        let body: JSON = ["kind": JSON(kind), "title": JSON(title), "hook": JSON(hook), "body": JSON(text)]
        Task {
            if await store.saveMemory(original, body) { dismiss() }
            saving = false
        }
    }

    /// Close this sheet, then open the shared material with the note added as a line; it counts as
    /// unsaved there.
    private func addToBrief() {
        guard let ws = store.workspace(original["workspace"].str) else { return }
        var note = original
        note["title"] = JSON(title)
        note["body"] = JSON(text)
        let line = MemoryText.briefLine(note)
        dismiss()
        Task {
            try? await Task.sleep(nanoseconds: 400_000_000)
            store.editingBrief = BriefSuggestions.adding([line], to: ws.json)
        }
    }
}

/// A consolidation's page: the workspace lead's one turn over the notes, with what it changed.
struct DreamView: View {
    let run: RunModel

    var body: some View {
        let st = run.shown
        let me = run.who("lead")
        let info = st.session(.agent("lead"))
        let done = st.events.last { $0.type == .memoryConsolidated }
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 12) {
                Avatar(initials: me.initials, color: me.color, size: 34, role: me.role)
                VStack(alignment: .leading, spacing: 2) {
                    Text(L.text("整理团队记忆")).font(.display(.title3))
                    Text(L.text("\(me.name) · \(run.runJSON["config"]["workspace"]["name"].str)")).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(L.text("打开工作区")) { run.store.open(.home(run.workspaceId)) }
                    .disabled(run.store.workspace(run.workspaceId) == nil)
            }
            if let d = done {
                Note(text: L.text("整理完了：改了 \(d.data["saved"].int ?? 0) 条，删了 \(d.data["forgotten"].int ?? 0) 条。\(d.data["summary"].str)"), tone: .green)
            } else if st.run.status == .failed || st.run.status == .interrupted {
                Note(text: L.text("这次整理没有完成：\(st.run.error.isEmpty ? L.text("会话中断了") : st.run.error)。已经改的笔记保留，可以在工作区页重新整理。"), tone: .amber)
            }
            ChatCard(run: run, st: st, title: me.name, info: info, target: .agent("lead"))
                .frame(maxHeight: .infinity)
        }
        .padding(20)
        .frame(maxWidth: 980, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
    }
}
