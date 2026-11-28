import SwiftUI

// Beside the group, as a chat's info panel: the tasks and the files; or one task's
// topic, with what was said about it
// (docs/architecture/20261031-frontend.md#run_group).

struct GroupSidePanel: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        VStack(spacing: 0) {
            switch run.side {
            case .topic(let id) where st.tasks[id] != nil: TopicView(run: run, st: st, taskId: id)
            default: lists
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color.page)
        .clipped()
    }

    private var tab: Binding<GroupSide> {
        Binding(get: { if case .topic = run.side { return .tasks }; return run.side }, set: { run.side = $0 })
    }

    private var lists: some View {
        VStack(spacing: 0) {
            Picker(L.text("查看"), selection: tab) {
                Text(verbatim: L.text("子任务 \(st.order.count)")).tag(GroupSide.tasks)
                Text(verbatim: L.text("文件 \(st.page.diffs.count + st.page.files.count + st.page.delivered.count)")).tag(GroupSide.files)
            }
            .pickerStyle(.segmented).labelsHidden()
            .padding(12)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch run.side {
                    case .files: FileList(run: run, st: st)
                    default: TaskList(run: run, st: st)
                    }
                }
                .padding(.horizontal, 12).padding(.vertical, 6)
            }
        }
    }
}

/// The tasks in the order the lead put them on the board; a click opens one's topic.
struct TaskList: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        ForEach(Array(st.orderedTasks.enumerated()), id: \.element.id) { i, t in
            if i > 0 { Divider() }
            let (label, tone) = t.status.label
            let w = run.who(t.agent)
            Button { run.side = .topic(t.id) } label: {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        TaskTag(id: t.id)
                        Text(t.title).font(.callout.weight(.medium)).lineLimit(2).multilineTextAlignment(.leading)
                        Spacer(minLength: 0)
                    }
                    HStack(spacing: 6) {
                        HStack(spacing: 6) {
                            Avatar(initials: w.initials, color: w.color, size: 14, role: w.role)
                            Text(w.name).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        .padding(.vertical, 3).padding(.trailing, 6)
                        .memberHover(w)
                        Pill(label, tone)
                        Spacer(minLength: 0)
                        if let n = st.feed.topics[t.id]?.count, n > 0 {
                            Text(verbatim: L.text("\(n) 条")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(.vertical, 8)
                .contentShape(Rectangle())
                .hoverable(6)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(t.id) \(t.title) · \(w.name) · \(label)")
        }
    }
}

/// The run's changes and files: the whole run's diff first, each task's newest; what the
/// deliveries hand over; each task's own files.
struct FileList: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let delivered = st.page.delivered
        if !delivered.isEmpty {
            heading(L.text("交付物"))
            ForEach(Array(delivered.enumerated()), id: \.element.path) { i, d in
                if i > 0 { Divider() }
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(d.name).font(.callout.weight(.medium)).lineLimit(1)
                        Text(d.dir).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                    }
                    .help(L.tilde(d.path))
                    Spacer()
                    Button(L.text("查看")) { ArtifactWindow.open(run: run, path: d.path, title: d.name) }.buttonStyle(.link)
                }
                .padding(.vertical, 8)
            }
        }
        heading(L.text("改动"))
        artifacts(st.page.diffs, empty: L.text("还没有改动。"))
        if !st.page.files.isEmpty {
            heading(L.text("产物"))
            artifacts(st.page.files, empty: "")
        }
    }

    private func heading(_ s: String) -> some View {
        Text(s).font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.top, 10).padding(.bottom, 2)
    }

    @ViewBuilder private func artifacts(_ list: [Artifact], empty: String) -> some View {
        if list.isEmpty && !empty.isEmpty { Text(empty).font(.callout).foregroundStyle(.secondary).padding(.vertical, 6) }
        ForEach(Array(list.enumerated()), id: \.element.seq) { i, a in
            if i > 0 { Divider() }
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        if a.ofRun { Text(L.text("整个任务")).font(.callout.weight(.medium)) } else { TaskTag(id: a.task) }
                        Text(a.name).font(.callout.weight(a.ofRun ? .regular : .medium)).foregroundStyle(a.ofRun ? .secondary : .primary).lineLimit(1)
                    }
                    if !a.desc.isEmpty { Text(a.desc).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                }
                Spacer()
                Button(a.isDiff ? L.text("审阅") : L.text("查看")) {
                    if a.isDiff { DiffWindow.open(run: run, artifact: a) } else { ArtifactWindow.open(run: run, path: a.path, title: a.name) }
                }
                .buttonStyle(.link)
            }
            .padding(.vertical, 8)
        }
    }
}

/// One task as a topic: what it is, then everything said about it. Its collaborator's session
/// opens on the left, where the group is.
struct TopicView: View {
    let run: RunModel
    let st: RunState
    let taskId: String

    var body: some View {
        let t = st.tasks[taskId]
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Button { run.side = .tasks } label: { Label(L.text("全部子任务"), systemImage: "chevron.backward").font(.caption) }
                        .buttonStyle(.link)
                    Spacer()
                    // The collaborator's session opens where the group is, wide enough to follow it
                    // and step in; the topic stays here beside it.
                    if focused {
                        Button(L.text("回到群聊")) { run.focus = nil }.controlSize(.small)
                    } else {
                        Button(L.text("看工作记录")) { run.focus = taskId }
                            .controlSize(.small)
                            .help(L.text("在左边打开 \(taskId) 的工作记录：看它每一步，插话或接管"))
                    }
                }
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    TaskTag(id: taskId)
                    Text(t?.title ?? taskId).font(.display(.headline)).lineLimit(3).fixedSize(horizontal: false, vertical: true)
                }
                if let t {
                    let w = run.who(t.agent)
                    let (label, tone) = t.status.label
                    HStack(spacing: 6) {
                        HStack(spacing: 6) {
                            Avatar(initials: w.initials, color: w.color, size: 16, role: w.role)
                            Text(w.name).font(.caption).foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 3).padding(.trailing, 6)
                        .memberHover(w)
                        Pill(label, tone)
                        if t.attempts.count > 1 { Pill(L.text("第 \(t.attempts.count) 次")) }
                        Spacer(minLength: 0)
                        Text(verbatim: L.text("话题 \(st.feed.topics[taskId]?.count ?? 0)")).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(12)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let t { brief(t) }
                    ForEach(st.feed.topics[taskId] ?? []) { p in PostRow(run: run, st: st, post: p, inTopic: true) }
                }
                .padding(12)
            }
            .defaultScrollAnchor(.bottom, for: .initialOffset)
        }
    }

    /// Its work log is open where the group was.
    private var focused: Bool { run.focus == taskId }

    /// What the lead asked for: the brief, the criteria, what the task covers.
    private func brief(_ t: TaskState) -> some View {
        let lead = run.who("lead")
        let structure = st.page.tasks[t.id]?.structure ?? ""
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Avatar(initials: lead.initials, color: lead.color, size: 16, role: lead.role)
                Text(verbatim: L.text("\(lead.name) 派的活")).font(.caption.weight(.semibold))
            }
            if !t.desc.isEmpty { MarkdownText(text: t.desc) }
            if !t.accept.isEmpty {
                Text(L.text("验收标准")).font(.caption).foregroundStyle(.secondary)
                ForEach(Array(t.accept.enumerated()), id: \.offset) { _, a in
                    Text(MD.inline("· " + a)).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
            }
            if !structure.isEmpty { Text(verbatim: structure).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.line))
    }
}
