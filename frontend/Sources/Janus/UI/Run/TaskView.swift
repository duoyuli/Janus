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
        let evs = st.events.filter { e in e.task == t.id || e.data["target"].str == t.id || e.data["tasks"].strings.contains(t.id) || e.data["unlocked"].strings.contains(t.id) }
        let lastStart = evs.last { $0.type == "attempt.started" }?.data ?? .null
        let w = run.who(t.agent)
        // The evidence summary is a display grouping of what the events recorded; whether the task
        // passed stays with `task.succeeded` / `verification.*`.
        let summary = EvidenceSummary(accept: t.accept, checks: t.checks)
        let verifiers = st.verifiers(of: t.id)
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
                        let structure = [t.owns.isEmpty ? nil : L.text("负责 \(t.owns.joined(separator: L.text("、")))"), t.verifies.isEmpty ? nil : L.text("独立验证 \(t.verifies)")].compactMap { $0 }
                        if !structure.isEmpty { Text(verbatim: structure.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                        if let git = SessionView.taskGitNote(st, t.id) { Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                    }
                    Spacer()
                    Button(L.text("在审计日志中查看")) { run.auditCat = "all"; run.auditTask = t.id; run.auditActor = ""; run.highlight = nil; run.show(.audit) }
                }
                AdaptiveColumns(side: 330) {
                    VStack(alignment: .leading, spacing: 14) {
                        ChatCard(run: run, st: st, title: L.text("会话"), info: taskSession(st, t), target: .task(t.id))
                            .frame(height: 560)
                        // The full trace is long and repeats the audit log; it stays folded until asked for.
                        if !evs.isEmpty {
                            Card {
                                DisclosureGroup {
                                    // Lazy: a long task has hundreds of events, built only as they scroll into view.
                                    LazyVStack(alignment: .leading, spacing: 8) { ForEach(evs) { e in TimelineRow(run: run, e: e) } }.padding(.top, 6)
                                } label: {
                                    HStack { Text(L.text("执行轨迹")).font(.headline); Text(verbatim: L.text("\(evs.count) 条事件")).font(.caption).foregroundStyle(.secondary) }
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
                                ForEach(CriterionCheck.rows(accept: t.accept, checks: t.checks)) { row in
                                    CheckLine(n: row.n, criterion: row.criterion, check: row.check)
                                }
                                if !t.checks.isEmpty {
                                    Text(verbatim: L.text("证据：\(summary.countsText)")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
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
                                    VerificationList(records: verifiers.map(VerificationRecord.init))
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
                            depRow(L.text("下游"), st.order.filter { st.tasks[$0]?.deps.contains(t.id) == true }, st)
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
                                let cfg = lastStart["config"]
                                let rt = [lastStart["runtime"].truthy ? lastStart["runtime"].str : w.runtime,
                                          cfg["model"].truthy ? cfg["model"].str + (cfg["effort"].truthy ? " (\(cfg["effort"].str))" : "") : nil,
                                          cfg["label"].truthy || cfg["mode"].truthy ? L.text("权限 \(cfg["label"].truthy ? cfg["label"].str : cfg["mode"].str)") : nil,
                                          lastStart["agentInfo"].truthy ? "\(lastStart["agentInfo"]["name"].str) \(lastStart["agentInfo"]["version"].str)" : nil].compactMap { $0 }.joined(separator: " · ")
                                KV(rows: [(L.text("运行时"), rt), (L.text("隔离方式"), lastStart["enforcement"].truthy ? lastStart["enforcement"].str : "—"),
                                          (L.text("会话"), t.attempts.last?.session ?? "—"), (L.text("工作区"), t.attempts.last?.workspace ?? L.text("工作目录"))])
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

struct TimelineRow: View {
    let run: RunModel
    let e: Event

    var body: some View {
        let d = e.data
        if e.type == "attempt.started" {
            HStack(spacing: 10) {
                Text(verbatim: L.text("尝试 \(e.attempt ?? 0)")).fontWeight(.semibold)
                Text(verbatim: "\(d["runtime"].str)\(SessionView.attemptNote(d, task: e.task))").foregroundStyle(.secondary)
                Text(L.short(d["session"].str, 40)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                let place = SessionView.gitNote(d["git"]) ?? d["workspace"].str
                Text(verbatim: place).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).help(place)
                Spacer()
                SeqButton(seq: e.seq, run: run)
            }
            .font(.callout)
            .padding(.vertical, 4)
            .padding(.horizontal, 8)
            .background(Color.secondary.opacity(0.07), in: RoundedRectangle(cornerRadius: 6))
        } else {
            HStack(alignment: .top, spacing: 10) {
                EventIcon(type: e.type)
                VStack(alignment: .leading, spacing: 4) {
                    (Text(run.who(e.actor).name).fontWeight(.semibold) + Text("　" + e.text)).font(.callout).textSelection(.enabled)
                    detail(d)
                }
                Spacer()
                Text(L.clock(e.ts)).font(.caption).foregroundStyle(.secondary)
                SeqButton(seq: e.seq, run: run)
            }
        }
    }

    @ViewBuilder private func detail(_ d: JSON) -> some View {
        switch e.type {
        case "tool.call", "tool.started", "tool.completed":
            Text(verbatim: "\(d["tool"].str) \(L.short(d["input"].str, 200))\n\(e.type == "tool.started" ? L.text("等待返回…") : "→ " + L.short(d["output"].str, 200))").font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
        case "question.asked": Note(text: L.text("\(d["question"].str)\n选项：\(d["options"].strings.joined(separator: " / "))"))
        case "question.answered": if d["note"].truthy { Note(text: L.text("补充说明：\(d["note"].str)")) }
        case "attempt.failed": Note(text: d["error"].str, tone: .red)
        case "task.retry_scheduled": Note(text: L.text("重试策略：\(d["strategy"].str)"))
        case "artifact.created": Note(text: "\(d["desc"].str)\n\(L.tilde(d["path"].str)) · sha256 \(d["hash"].str)…")
        case "result.submitted":
            ForEach(CriterionCheck.rows(accept: [], checks: Check.list(d["checks"]))) { row in
                CheckLine(n: row.n, criterion: row.criterion, check: row.check, dense: true)
            }
            if !d["risks"].array.isEmpty { Note(text: L.text("风险：\(d["risks"].strings.joined(separator: L.text("；")))"), tone: .amber) }
        case "approval.requested": CodeBlock(text: "$ \(d["command"].str)")
        default: EmptyView()
        }
    }
}
