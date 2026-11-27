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
                    ChatCard(run: run, st: st, title: L.text("群聊"), info: st.session(.agent("lead")), target: .agent("lead"),
                             chrome: false, composeTo: composeTo(st), recipients: recipients(st), typing: st.feed.typing, jump: run.jump)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.card)
                if wide {
                    Divider()
                    GroupSidePanel(run: run, st: st).frame(width: Self.side)
                }
            }
            .overlay(alignment: .trailing) {
                if !wide && run.sideShown {
                    GroupSidePanel(run: run, st: st)
                        .frame(width: min(Self.side, geo.size.width - 48))
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.line))
                        .shadow(color: .black.opacity(0.18), radius: 18, y: 6)
                        .padding(12)
                }
            }
        }
    }

    /// The composer's addressee, back to the lead once the collaborator's session is gone.
    private func composeTo(_ st: RunState) -> Binding<SessionTarget> {
        Binding(get: { recipients(st).contains(run.composeTo) ? run.composeTo : .agent("lead") },
                set: { run.composeTo = $0 })
    }

    /// The lead, and each collaborator whose task has a session open now.
    private func recipients(_ st: RunState) -> [SessionTarget] {
        [.agent("lead")] + st.orderedTasks.filter(\.live).map { .task($0.id) }
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
                        .help(run.sideShown ? L.text("收起成员、子任务和文件") : L.text("查看成员、子任务和文件"))
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: "pin").font(.caption).foregroundStyle(.secondary).accessibilityHidden(true)
                if r.goal.trimmingCharacters(in: .whitespacesAndNewlines) != st.title {
                    Button(run.goalOpen ? L.text("收起完整目标") : L.text("完整目标")) { run.goalOpen.toggle() }
                        .buttonStyle(.link).font(.caption)
                }
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
            if run.goalOpen {
                ScrollView { MarkdownText(text: r.goal).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 220)
            }
            StoredImages(run: run, paths: r.images, size: 48)
            notices
        }
        .padding(.horizontal, 18).padding(.vertical, 12)
        .background(Color.page)
    }

    /// Who is in the group, each with a dot for what it is doing; a click opens the team.
    private var faces: some View {
        Button { run.side = .members; run.sideShown = true } label: {
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
                        .help("\(w.name) · \(m.phase.label.0) · \(m.line)")
                }
            }
        }
        .buttonStyle(.plain)
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
            if run.usage.showsLedgerControl(open: run.usageOpen) {
                Button(L.text("用量明细")) { run.usageOpen.toggle() }
                    .buttonStyle(.link).font(.caption)
                    .help(L.text("展开用量：Token、费用、耗时、上下文"))
                    .popover(isPresented: Binding(get: { run.usageOpen }, set: { run.usageOpen = $0 }), arrowEdge: .bottom) {
                        ScrollView { VStack(alignment: .leading, spacing: 10) { UsageSection(run: run, st: st) }.padding(16) }
                            .frame(width: 560, height: 520)
                    }
            }
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

    /// What the run's state asks the person to know: it failed, it stopped short and can go on, the
    /// usage reminders.
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
        if let line = run.usage.words.reminders { Note(text: line, tone: .amber) }
    }
}

/// Who is at work right now, one line each, above the composer like 「对方正在输入」; a click opens
/// that collaborator's session.
struct TypingLine: View {
    let run: RunModel
    let typing: [Feed.Typing]

    var body: some View {
        if !typing.isEmpty && !run.isReplaying {
            HStack(spacing: 14) {
                ForEach(typing, id: \.task) { t in
                    Button { run.side = .topic(t.task, work: true); run.sideShown = true } label: {
                        HStack(spacing: 5) {
                            TypingDots()
                            Text(run.who(t.agent).name).font(.caption.weight(.semibold))
                            Text(verbatim: "\(t.text) · \(t.task)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(L.text("看 \(run.who(t.agent).name) 的工作记录"))
                }
                Spacer(minLength: 0)
            }
        }
    }
}

/// Three dots that pulse in turn, as a chat marks someone typing.
struct TypingDots: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 8, paused: reduceMotion)) { ctx in
            let t = ctx.date.timeIntervalSinceReferenceDate
            HStack(spacing: 2) {
                ForEach(0..<3, id: \.self) { i in
                    let phase = (t * 1.2 - Double(i) * 0.2).truncatingRemainder(dividingBy: 1)
                    Circle().fill(Color.accent).frame(width: 4, height: 4)
                        .opacity(reduceMotion ? 0.7 : (phase < 0.35 ? 1 : 0.3))
                }
            }
        }
        .accessibilityHidden(true)
    }
}
