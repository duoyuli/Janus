import SwiftUI

// The run page as a team's group chat: the goal and who is in it at the top, the lead's
// conversation with the team's posts in their place in the middle, and beside it the team, the
// tasks and the files, or one task's topic. No phases: what it says comes from what happened
// (docs/architecture/20261031-frontend.md#run_group).

/// The run's 工作台: the group.
// @project-doc docs/architecture/20261031-frontend.md#run_group
struct GroupView: View {
    let run: RunModel
    /// Narrower than this, the side panel shows over the group on request.
    static let wide: CGFloat = 940
    /// The side panel's width: a collaborator's session with its controls fits in it.
    static let side: CGFloat = 400

    var body: some View {
        let st = run.shown
        GeometryReader { geo in
            let wide = geo.size.width >= Self.wide
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    GroupHeader(run: run, st: st, wide: wide)
                    Divider()
                    if let id = run.focus, st.tasks[id] != nil {
                        FocusBar(run: run, st: st, taskId: id)
                        Divider()
                        ChatCard(run: run, st: st, title: run.who(st.tasks[id]?.agent ?? "").name, info: st.session(.task(id)), target: .task(id), chrome: false)
                    } else {
                        ChatCard(run: run, st: st, title: L.text("群聊"), info: st.session(.agent("lead")), target: .agent("lead"),
                                 chrome: false, jump: run.jump)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.card)
                if wide {
                    Divider()
                    // Clipped at its own width: nothing in it may push the window wider.
                    GroupSidePanel(run: run, st: st).frame(width: Self.side).clipped()
                }
            }
            .overlay(alignment: .trailing) {
                if !wide && run.sideShown {
                    GroupSidePanel(run: run, st: st)
                        .frame(width: min(Self.side, geo.size.width - 48))
                        .clipped()
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line))
                        .shadow(color: .black.opacity(0.18), radius: 18, y: 6)
                        .padding(12)
                }
            }
            // The goal in the middle of the page, over everything, to read and change at ease.
            .overlay {
                if run.goalOpen {
                    ZStack {
                        Color.black.opacity(0.22).ignoresSafeArea()
                            .onTapGesture { run.goalOpen = false }
                            .accessibilityHidden(true)
                        GoalPanel(run: run, st: st)
                            .frame(width: min(680, geo.size.width - 48))
                            .fixedSize(horizontal: false, vertical: true)
                            .background(Color.card, in: RoundedRectangle(cornerRadius: 14))
                            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color.line))
                            .shadow(color: .black.opacity(0.25), radius: 24, y: 8)
                    }
                }
            }
        }
    }
}

/// The group's top: what the run is about, who is in it and how it stands, then one line with the
/// numbers and what waits on the person.
struct GroupHeader: View {
    let run: RunModel
    let st: RunState
    /// The side panel is beside the group, not behind a button.
    var wide = true
    @AppStorage("janus.chatDensity", store: .janus) private var density = ChatDensity.summary

    var body: some View {
        let r = st.run
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 12) {
                Text(st.title.isEmpty ? L.text("正在连接 Janus 服务…") : st.title)
                    .font(.display(.title3)).lineLimit(1).truncationMode(.tail)
                    .help(st.title)
                    .textSelection(.enabled)
                Spacer(minLength: 8)
                faces
                Pill(st.capsule.text, st.capsule.tone)
                actions
                if !wide {
                    Button { run.sideShown.toggle() } label: { Label(L.text("群信息"), systemImage: "sidebar.right") }
                        .labelStyle(.iconOnly).buttonStyle(.borderless)
                        .help(run.sideShown ? L.text("收起子任务和文件") : L.text("查看子任务和文件"))
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Button { run.goalOpen.toggle() } label: {
                    Label(L.text("目标"), systemImage: "pin").font(.caption)
                }
                .buttonStyle(.link)
                .help(L.text("查看完整目标，或者给负责人一个新目标"))
                summary
                Spacer(minLength: 8)
                Picker(L.text("显示"), selection: $density) {
                    Text(L.text("详细")).tag(ChatDensity.detail)
                    Text(L.text("摘要")).tag(ChatDensity.summary)
                }
                .pickerStyle(.segmented).labelsHidden().controlSize(.small).fixedSize()
                .help(L.text("详细：每次工具调用都显示；摘要：只看对话、请求和结果"))
                mentions
            }
            StoredImages(run: run, paths: r.images, size: 48)
            notices
        }
        .padding(.horizontal, 18).padding(.vertical, 12)
        .background(Color.page)
    }

    /// Who is in the group, each with a dot for what it is doing; a hover says who it is, what it
    /// is responsible for and what it is at.
    private var faces: some View {
        HStack(spacing: -6) {
            ForEach(st.members, id: \.id) { m in
                let w = run.who(m.id)
                Avatar(initials: w.initials, color: w.color, size: 26, role: w.role)
                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.page, lineWidth: 2))
                    .overlay(alignment: .bottomTrailing) {
                        Circle().fill(m.phase.label.1 == .gray ? Color.secondary : m.phase.label.1.color)
                            .frame(width: 8, height: 8)
                            .overlay(Circle().strokeBorder(Color.page, lineWidth: 1.5))
                            .offset(x: 2, y: 2)
                    }
                    .memberHover(w, now: "\(m.phase.label.0) · \(m.line)")
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(L.text("成员：\(st.members.map { run.who($0.id).name }.joined(separator: "、"))"))
    }

    /// Pause and stop, as the server allows them.
    @ViewBuilder private var actions: some View {
        let r = st.run
        let pause = st.allowed.ok(r.paused ? "resume" : "pause"), stop = st.allowed.ok("cancel")
        if pause {
            Button { Task { await run.togglePause() } } label: {
                Label(r.paused ? L.text("继续") : L.text("暂停"), systemImage: r.paused ? "play.fill" : "pause.fill")
            }
            .controlSize(.small)
            .help(r.paused ? L.text("继续调度子任务（⇧⌘P）") : L.text("暂停：不再派新活，已开工的照常跑（⇧⌘P）"))
        }
        if stop {
            Button(L.text("停止…"), role: .destructive) { cancelRun(run) }.controlSize(.small).help(L.text("结束这个任务的所有会话"))
        }
    }

    /// The run's numbers in one line, as the usage ledger words them, and its detail on request.
    private var summary: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Group {
                if run.elapsedTicks {
                    TimelineView(.periodic(from: .now, by: 1)) { ctx in numbers(now: ctx.date.timeIntervalSince1970 * 1000) }
                } else {
                    numbers(now: L.nowMs())
                }
            }
            // A reminder is a mark beside the numbers it is about, its words a click away.
            if let line = run.usage.words.reminders {
                Button { run.usageOpen.toggle() } label: {
                    Label(L.text("用量提醒"), systemImage: "exclamationmark.triangle.fill").font(.caption)
                        .foregroundStyle(Tone.amber.color)
                }
                .buttonStyle(.plain)
                .help(line)
            }
            if run.usage.showsLedgerControl(open: run.usageOpen) {
                Button(L.text("用量明细")) { run.usageOpen.toggle() }
                    .buttonStyle(.link).font(.caption)
                    .help(L.text("展开用量：Token、费用、耗时、上下文"))
            }
        }
        .popover(isPresented: Binding(get: { run.usageOpen }, set: { run.usageOpen = $0 }), arrowEdge: .bottom) {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if let line = run.usage.words.reminders { Note(text: line, tone: .amber) }
                    UsageSection(run: run, st: st)
                }
                .padding(16)
            }
            .frame(width: 560, height: 520)
        }
    }

    private func numbers(now: Double) -> some View {
        let (line, voice) = run.usageCard(at: now)
        return Text(verbatim: line).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail)
            .help(voice)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(voice)
    }

    /// 「@你 2 件」: each click brings the start of the next post waiting on the person to the top.
    @ViewBuilder private var mentions: some View {
        let ids = st.feed.mentions
        if !ids.isEmpty {
            Button {
                let n = (run.jump?.n ?? -1) + 1
                run.jump = RunModel.Jump(row: ids[n % ids.count], n: n)
            } label: {
                Text(verbatim: L.text("@你 \(ids.count) 件 ↓"))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 9).padding(.vertical, 3)
                    .background(Tone.red.color, in: Capsule())
            }
            .buttonStyle(.plain)
            .help(L.text("跳到等你处理的消息"))
        }
    }

    /// What the run's state asks the person to know: it failed, it stopped short and can go on.
    @ViewBuilder private var notices: some View {
        let r = st.run
        if r.status == .failed { Note(text: r.error, tone: .red) }
        if let note = st.resumeNote {
            HStack {
                Text(verbatim: note).font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button(L.text("从中断处继续")) { Task { await run.act(.resumeRun) } }.buttonStyle(.primary)
                    .disabled(!st.allowed.ok("resumeRun"))
                    .help(st.allowed.why("resumeRun"))
            }
            .padding(10)
            .background(Color.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
        if let reference = run.runJSON["config"]["tokenLimit"].double, reference > 0 {
            Text(verbatim: L.text("Token 用量参考值：\(reference.formatted(.number.locale(L.locale)))"))
                .font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// Above a collaborator's session in the group's place: whose it is and how the task stands, and
/// the way back to the group.
struct FocusBar: View {
    let run: RunModel
    let st: RunState
    let taskId: String

    var body: some View {
        let t = st.tasks[taskId]
        let w = run.who(t?.agent ?? "")
        HStack(spacing: 10) {
            Button { run.focus = nil } label: { Label(L.text("群聊"), systemImage: "chevron.backward").font(.callout) }
                .buttonStyle(.link)
                .help(L.text("回到群聊（⌘[）"))
            Divider().frame(height: 16)
            TaskTag(id: taskId)
            Text(t?.title ?? taskId).font(.callout.weight(.semibold)).lineLimit(1).truncationMode(.tail)
            HStack(spacing: 10) {
                Avatar(initials: w.initials, color: w.color, size: 18, role: w.role)
                Text(w.name).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            .padding(.vertical, 3).padding(.trailing, 6)
            .memberHover(w)
            if let t {
                let (label, tone) = t.status.label
                Pill(label, tone)
                if t.attempts.count > 1 { Pill(L.text("第 \(t.attempts.count) 次")) }
            }
            Spacer(minLength: 8)
            Button(L.text("子任务详情")) { run.show(.task(taskId)) }
                .buttonStyle(.link).font(.caption)
                .help(L.text("验收证据、执行轨迹、产物和运行环境"))
        }
        .padding(.horizontal, 18).padding(.vertical, 8)
        .background(Color.page)
    }
}

/// The goal whole, ready to change: a new one goes to the lead as its own goal command and is
/// taken up from its next turn; the header keeps the first goal's title. Read-only while the lead
/// cannot be written to.
struct GoalPanel: View {
    let run: RunModel
    let st: RunState
    /// What the person has written; `nil` until they change something.
    @Local private var draft: String?
    @Local private var sending = false

    var body: some View {
        let r = st.run
        let now = r.goalNow ?? r.goal
        let lead = st.session(.agent("lead"))
        let editable = lead.mode.enabled && r.kind == .goal
        let text = draft ?? now
        let changed = text.trimmingCharacters(in: .whitespacesAndNewlines) != now.trimmingCharacters(in: .whitespacesAndNewlines)
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(r.goalNow == nil ? L.text("目标") : L.text("现在的目标")).font(.display(.title3))
                Spacer()
                Button { close() } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .keyboardShortcut(.cancelAction)
                    .help(L.text("关闭（esc）"))
            }
            if editable {
                TextEditor(text: Binding(get: { text }, set: { draft = $0 }))
                    .font(.body)
                    .scrollContentBackground(.hidden)
                    .padding(8)
                    .frame(height: 300)
                    .background(Color.soft, in: RoundedRectangle(cornerRadius: 8))
                Text(L.text("改完发给负责人：它把这当作新的目标，正在忙的话，这一轮结束后收到。"))
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                ScrollView { MarkdownText(text: now).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 300)
                if !lead.mode.placeholder.isEmpty && r.kind == .goal {
                    Text(verbatim: lead.mode.placeholder).font(.caption).foregroundStyle(.secondary)
                }
            }
            if r.goalNow != nil {
                DisclosureGroup(L.text("最初的目标")) {
                    MarkdownText(text: r.goal).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                }
                .font(.caption)
            }
            if editable {
                HStack {
                    Spacer()
                    Button(L.text("取消")) { close() }
                    Button(L.text("发给负责人")) { send(text) }
                        .buttonStyle(.primary)
                        .keyboardShortcut(.return, modifiers: .command)
                        .disabled(sending || !changed || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .help("⌘↩")
                }
            }
        }
        .padding(20)
    }

    private func close() {
        draft = nil
        run.goalOpen = false
    }

    private func send(_ text: String) {
        sending = true
        Task {
            if await run.act(.goal(text.trimmingCharacters(in: .whitespacesAndNewlines))) { close() }
            sending = false
        }
    }
}
