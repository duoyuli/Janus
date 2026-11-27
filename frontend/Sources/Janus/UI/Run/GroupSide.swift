import SwiftUI

// Beside the group, as a chat's info panel: who is in it, the tasks, the files; or one task's
// topic, with what was said about it and its collaborator's session
// (docs/architecture/20261031-frontend.md#run_group).

struct GroupSidePanel: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        VStack(spacing: 0) {
            switch run.side {
            case .topic(let id, let work) where st.tasks[id] != nil: TopicView(run: run, st: st, taskId: id, work: work)
            default: lists
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color.page)
        .clipped()
    }

    private var tab: Binding<GroupSide> {
        Binding(get: { if case .topic = run.side { return .members }; return run.side }, set: { run.side = $0 })
    }

    private var lists: some View {
        VStack(spacing: 0) {
            Picker(L.text("查看"), selection: tab) {
                Text(verbatim: L.text("成员 \(st.members.count)")).tag(GroupSide.members)
                Text(verbatim: L.text("子任务 \(st.order.count)")).tag(GroupSide.tasks)
                Text(verbatim: L.text("文件 \(st.page.diffs.count + st.page.files.count + st.page.delivered.count)")).tag(GroupSide.files)
            }
            .pickerStyle(.segmented).labelsHidden()
            .padding(12)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch run.side {
                    case .tasks: TaskList(run: run, st: st)
                    case .files: FileList(run: run, st: st)
                    default: MemberList(run: run, st: st)
                    }
                }
                .padding(.horizontal, 12).padding(.vertical, 6)
            }
        }
    }
}

/// The team, each with what it is doing; a click opens the topic of the task it is on, or its last.
struct MemberList: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        ForEach(Array(st.members.enumerated()), id: \.offset) { i, m in
            if i > 0 { Divider().padding(.leading, 42) }
            row(m)
        }
    }

    private func row(_ m: MemberLine) -> some View {
        let w = run.who(m.id)
        let (pl, pt) = m.phase.label
        let mine = st.orderedTasks.filter { $0.agent == m.id }
        let task = mine.first { $0.status.isBusy } ?? mine.last
        var meta: [String] = []
        if let beat = m.beat { meta.append(L.text("最近活动 \(L.ago(beat, st.run.now))")) }
        if let stats = m.stats { meta.append(stats) }
        return Button { if let task { run.side = .topic(task.id, work: true) } } label: {
            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .top, spacing: 10) {
                    Avatar(initials: w.initials, color: w.color, size: 30, role: w.role)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Text(w.name).font(.callout.weight(.semibold))
                            if w.lead { Tag(text: L.text("负责人"), tone: .blue) }
                            Spacer()
                            Pill(pl, pt)
                        }
                        Text(m.line).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                        if let stats = m.stats { Text(stats).font(.caption2).foregroundStyle(.tertiary) }
                    }
                }
                if let p = m.progress { Bar(value: p).padding(.leading, 40) }
            }
            .padding(.vertical, 9).padding(.horizontal, 4)
            .contentShape(Rectangle())
            .hoverable(8)
        }
        .buttonStyle(.plain)
        .disabled(task == nil)
        .help(([w.runtime, w.model].filter { !$0.isEmpty } + meta).joined(separator: " · ") + (task.map { L.text("\n点按查看 \($0.id) 的话题") } ?? ""))
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
            Button { run.side = .topic(t.id, work: false) } label: {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        TaskTag(id: t.id)
                        Text(t.title).font(.callout.weight(.medium)).lineLimit(2).multilineTextAlignment(.leading)
                        Spacer(minLength: 0)
                    }
                    HStack(spacing: 6) {
                        Avatar(initials: w.initials, color: w.color, size: 14, role: w.role)
                        Text(w.name).font(.caption).foregroundStyle(.secondary).lineLimit(1)
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

/// One task as a topic: what it is, then everything said about it; or its collaborator's
/// session, where the person cuts in or takes over.
struct TopicView: View {
    let run: RunModel
    let st: RunState
    let taskId: String
    let work: Bool

    var body: some View {
        let t = st.tasks[taskId]
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Button { run.side = .tasks } label: { Label(L.text("群信息"), systemImage: "chevron.backward").font(.caption) }
                        .buttonStyle(.link)
                    Spacer()
                    Button(L.text("子任务详情")) { run.show(.task(taskId)) }
                        .buttonStyle(.link).font(.caption)
                        .help(L.text("验收证据、执行轨迹、产物和运行环境"))
                }
                HStack(spacing: 6) {
                    TaskTag(id: taskId)
                    Text(t?.title ?? taskId).font(.display(.headline)).lineLimit(2)
                }
                if let t {
                    let w = run.who(t.agent)
                    let (label, tone) = t.status.label
                    HStack(spacing: 6) {
                        Avatar(initials: w.initials, color: w.color, size: 16, role: w.role)
                        Text(w.name).font(.caption).foregroundStyle(.secondary)
                        Pill(label, tone)
                        if t.attempts.count > 1 { Pill(L.text("第 \(t.attempts.count) 次")) }
                    }
                }
                Picker(L.text("查看"), selection: Binding(get: { work }, set: { run.side = .topic(taskId, work: $0) })) {
                    Text(verbatim: L.text("话题 \(st.feed.topics[taskId]?.count ?? 0)")).tag(false)
                    Text(L.text("工作记录")).tag(true)
                }
                .pickerStyle(.segmented).labelsHidden()
                .padding(.top, 4)
            }
            .padding(12)
            Divider()
            if work {
                ChatCard(run: run, st: st, title: run.who(t?.agent ?? "").name, info: st.session(.task(taskId)), target: .task(taskId), compact: true)
            } else {
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
    }

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
