import SwiftUI

/// One task: its session, acceptance criteria, trace, prompt, assignment, dependencies, attempts,
/// environment and artifacts.
struct TaskView: View {
    let run: RunModel
    let taskId: String

    var body: some View {
        let st = run.shown
        if let t = st.tasks[taskId] {
            content(st, t)
        } else {
            VStack(spacing: 10) {
                Text(run.isReplaying ? L.text("在当前回放位置，这个子任务还不存在。") : L.text("这个任务里没有 \(taskId)。")).foregroundStyle(.secondary)
                Button(L.text("返回工作台")) { run.show(.board) }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func content(_ st: RunState, _ t: TaskState) -> some View {
        let (label, tone) = t.status.label
        let page = st.page.tasks[t.id] ?? ServedPage.TaskPage()
        let trace = page.trace
        let w = run.who(t.agent)
        // The evidence summary is a display grouping of what the events recorded; whether the task
        // passed stays with `task.succeeded` / `verification.*`.
        let summary = st.evidence[t.id] ?? TaskEvidence()
        let verifiers = summary.verifications
        let session = st.session(.task(t.id))
        return ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 6) {
                            TaskTag(id: t.id)
                            Pill(label, tone)
                            if t.attempts.count > 1 { Pill(L.text("第 \(t.attempts.count) 次尝试")) }
                        }
                        Text(t.title).font(.display(.title2)).textSelection(.enabled)
                        if !t.desc.isEmpty { Text(t.desc).foregroundStyle(.secondary).textSelection(.enabled) }
                        if !page.structure.isEmpty { Text(verbatim: page.structure).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                        if let git = session.git { Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                    }
                    Spacer()
                    Button(L.text("在审计日志中查看")) { run.auditCat = "all"; run.auditTask = t.id; run.auditActor = ""; run.highlight = nil; run.show(.audit) }
                }
                AdaptiveColumns(side: 330) {
                    VStack(alignment: .leading, spacing: 14) {
                        ChatCard(run: run, st: st, title: L.text("会话"), info: session, target: .task(t.id))
                            .frame(height: 560)
                        // The full trace is long and repeats the audit log; it stays folded until asked for.
                        if !trace.isEmpty {
                            Card {
                                DisclosureGroup {
                                    // Lazy: a long task has hundreds of events, built only as they scroll into view.
                                    LazyVStack(alignment: .leading, spacing: 8) { ForEach(trace) { r in TimelineRow(run: run, row: r) } }.padding(.top, 6)
                                } label: {
                                    HStack { Text(L.text("执行轨迹")).font(.headline); Text(verbatim: L.text("\(trace.count) 条事件")).font(.caption).foregroundStyle(.secondary) }
                                }
                            }
                        }
                    }
                    .frame(maxWidth: .infinity)
                    VStack(alignment: .leading, spacing: 14) {
                        // What the task came back with, first: where it disagrees with the workspace,
                        // then each criterion with its evidence method and the whole evidence. What the
                        // executor added, the independent verification, the risks and the reminders stay
                        // apart from each other.
                        if !t.accept.isEmpty || !t.checks.isEmpty || !t.summary.isEmpty {
                            Card {
                                HStack(spacing: 8) {
                                    Text(L.text("验收标准")).font(.headline)
                                    Spacer()
                                    if summary.criteria > 0 {
                                        Text(verbatim: L.text("\(summary.criteria) 条 · 通过 \(summary.passed) 条")).font(.caption).foregroundStyle(.secondary)
                                    }
                                }
                                if !t.traceGaps.isEmpty {
                                    Note(text: L.text("和工作目录对不上：\n\(t.traceGaps.map { "· \($0)" }.joined(separator: "\n"))"), tone: .amber)
                                }
                                // One row per criterion, and every check is kept even when it names no criterion.
                                ForEach(summary.rows) { row in
                                    CheckLine(line: row.line)
                                }
                                if !t.checks.isEmpty {
                                    Text(verbatim: L.text("证据：\(summary.counts)")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                                }
                                EvidenceReminders(reminders: summary.reminders)
                                if !t.verificationFailures.isEmpty {
                                    Note(text: L.text("最近一次核对没有通过：\n\(t.verificationFailures.map { "· \($0)" }.joined(separator: "\n"))"), tone: .red)
                                }
                                if !t.evidence.isEmpty {
                                    Divider()
                                    Text(L.text("协作者提交的其他证据")).font(.callout).foregroundStyle(.secondary)
                                    VStack(alignment: .leading, spacing: 3) {
                                        ForEach(t.evidence.indices, id: \.self) { i in
                                            Text(MD.inline(t.evidence[i])).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                                        }
                                    }
                                }
                                if !verifiers.isEmpty {
                                    Divider()
                                    Text(L.text("独立验证")).font(.callout).foregroundStyle(.secondary)
                                    VerificationList(records: verifiers)
                                }
                                if !t.risks.isEmpty {
                                    Divider()
                                    Text(L.text("风险（协作者自报）")).font(.callout).foregroundStyle(.secondary)
                                    VStack(alignment: .leading, spacing: 3) { RiskList(risks: t.risks) }
                                }
                                if !t.summary.isEmpty {
                                    Divider()
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(L.text("结果")).font(.callout).foregroundStyle(.secondary)
                                        Text(MD.inline(t.summary)).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                                    }
                                }
                            }
                        }
                        Card {
                            KV(rows: [(L.text("协作者"), w.name)])
                            depRow(L.text("上游"), t.deps, st)
                            depRow(L.text("下游"), page.downstream, st)
                            if !t.attempts.isEmpty {
                                Divider()
                                Text(L.text("尝试")).font(.callout).foregroundStyle(.secondary)
                                ForEach(t.attempts.indices, id: \.self) { i in
                                    let a = t.attempts[i]
                                    let (al, at) = a.status.label
                                    VStack(alignment: .leading, spacing: 4) {
                                        HStack {
                                            Text(verbatim: "#\(a.n)").fontWeight(.semibold)
                                            Pill(al, at)
                                            Spacer()
                                            Text(L.clock(a.start)).font(.caption).foregroundStyle(.secondary)
                                        }
                                        .help(a.session)
                                        if let err = a.error, !err.isEmpty { Note(text: err, tone: .red) }
                                    }
                                }
                            }
                            Divider()
                            DisclosureGroup(L.text("运行环境")) {
                                KV(rows: page.env)
                                    .padding(.top, 6)
                            }
                            .font(.callout)
                        }
                        if !t.artifacts.isEmpty {
                            Card {
                                HStack { Text(L.text("产物")).font(.headline); Text(verbatim: "\(t.artifacts.count)").font(.caption).foregroundStyle(.secondary) }
                                ForEach(t.artifacts, id: \.seq) { a in
                                    VStack(alignment: .leading, spacing: 2) {
                                        HStack { Text(a.name).fontWeight(.semibold); Spacer(); Button(a.isDiff ? L.text("审阅") : L.text("查看")) { if a.isDiff { DiffWindow.open(run: run, artifact: a) } else { ArtifactWindow.open(run: run, path: a.path, title: a.name) } }.buttonStyle(.link) }
                                        if !a.desc.isEmpty { Text(a.desc).font(.caption).foregroundStyle(.secondary) }
                                    }
                                    .help(L.text("\(L.tilde(a.path)) · 尝试 \(a.attempt) · sha256 \(a.hash)…"))
                                }
                            }
                        }
                    }
                }
            }
            .padding(20)
        }
    }

    private func depRow(_ label: String, _ ids: [String], _ st: RunState) -> some View {
        HStack(alignment: .top) {
            Text(label).foregroundStyle(.secondary).frame(width: 72, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
                if ids.isEmpty { Text(L.text("无")).foregroundStyle(.secondary) }
                ForEach(ids, id: \.self) { d in Button(String("\(d) \(st.tasks[d]?.title ?? "")")) { run.page = .task(d) }.buttonStyle(.link) }
            }
        }
        .font(.callout)
    }
}

/// One row of a task's trace, as the server writes it.
struct TimelineRow: View {
    let run: RunModel
    let row: TraceRow

    var body: some View {
        if let a = row.attempt {
            HStack(spacing: 10) {
                Text(verbatim: a.title).fontWeight(.semibold)
                Text(verbatim: a.runtime).foregroundStyle(.secondary)
                Text(a.session).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                Text(verbatim: a.place).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).help(a.place)
                Spacer()
                SeqButton(seq: row.seq, run: run)
            }
            .font(.callout)
            .padding(.vertical, 4)
            .padding(.horizontal, 8)
            .background(Color.secondary.opacity(0.07), in: RoundedRectangle(cornerRadius: 6))
        } else {
            HStack(alignment: .top, spacing: 10) {
                EventIcon(mark: row.mark)
                VStack(alignment: .leading, spacing: 4) {
                    (Text(row.who).fontWeight(.semibold) + Text("　" + row.text)).font(.callout).textSelection(.enabled)
                    if let d = row.detail { detail(d) }
                }
                Spacer()
                Text(L.clock(row.ts)).font(.caption).foregroundStyle(.secondary)
                SeqButton(seq: row.seq, run: run)
            }
        }
    }

    @ViewBuilder private func detail(_ d: TraceRow.Detail) -> some View {
        switch d.kind {
        case "call": Text(verbatim: d.text).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
        case "code": CodeBlock(text: d.text)
        case "checks":
            CheckLines(lines: d.checks, dense: true)
            if !d.text.isEmpty { Note(text: d.text, tone: .amber) }
        default: Note(text: d.text, tone: d.tone)
        }
    }
}

/// An event kind's symbol, as the server names it (`EventMark`), drawn as the app's glyph.
struct EventIcon: View {
    let mark: EventMark

    static let glyph: [String: String] = [
        "created": "✦", "stop": "■", "pause": "Ⅱ", "start": "▶", "next": "▸", "assign": "↗", "fail": "✕", "retry": "↻",
        "reopen": "↺", "submit": "⇪", "done": "✓", "artifact": "▤", "handoff": "⇄", "question": "?", "escalate": "⇡",
        "write": "✎", "reply": "↩", "flag": "⚑", "delivered": "★", "blocked": "⊘", "forward": "→", "waiting": "◌",
        "tool": "⌘", "route": "↪", "quote": "❝", "hand": "✋", "interrupt": "‖", "warning": "⚠", "stall": "⏳", "split": "✂",
    ]

    var body: some View {
        let tone = mark.tone
        Text(Self.glyph[mark.icon] ?? "·")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(tone?.color ?? .secondary)
            .background((tone?.color ?? .secondary).opacity(0.12), in: Circle())
    }
}
