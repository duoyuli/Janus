import SwiftUI

/// Every state on the board is derived from these events. Selecting one shows its JSON; "回放"
/// replays the board to that moment.
struct AuditView: View {
    let run: RunModel

    var body: some View {
        let full = run.state
        let events = full.events
        let rows = events.filter { e in
            (run.auditCat == "all" || L.category(e.type) == run.auditCat)
                && (run.auditTask.isEmpty || e.task == run.auditTask || e.data["target"].str == run.auditTask)
                && (run.auditActor.isEmpty || e.actor == run.auditActor || e.to == run.auditActor)
        }
        var actors: [String] = []
        for e in events { for a in [e.actor, e.to].compactMap({ $0 }) where !a.isEmpty && !actors.contains(a) { actors.append(a) } }
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("审计与回放").font(.title2.weight(.semibold))
                    Text("工作台上的每个状态都由下面的事件推导出来。Agent 只能提交进度、问题和结果；任务状态只由编排器改变。选中任意一条事件，可以回放到当时的状态。").font(.callout).foregroundStyle(.secondary)
                }
            }
            Card(padding: 12) {
                HStack { Text("系统不变量检查").font(.headline); Spacer(); Text(verbatim: "基于当前 \(events.count) 条事件实时计算").font(.caption).foregroundStyle(.secondary) }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), alignment: .leading)], alignment: .leading, spacing: 6) {
                    ForEach(invariants(events), id: \.0) { inv in
                        HStack(spacing: 6) {
                            Text(inv.1 ? "✓" : "✕").fontWeight(.bold).foregroundStyle(inv.1 ? Tone.green.color : Tone.red.color)
                            Text(inv.0).font(.callout)
                            Text(inv.2).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            HStack(spacing: 10) {
                Picker("", selection: Binding(get: { run.auditCat }, set: { run.auditCat = $0 })) {
                    ForEach(L.categoryLabel, id: \.0) { Text($0.1).tag($0.0) }
                }
                .pickerStyle(.segmented).frame(maxWidth: 460)
                Picker("", selection: Binding(get: { run.auditTask }, set: { run.auditTask = $0 })) {
                    Text("全部任务").tag("")
                    ForEach(full.order, id: \.self) { id in Text(verbatim: "\(id) \(L.short(full.tasks[id]?.title ?? "", 14))").tag(id) }
                }
                .frame(width: 200)
                Picker("", selection: Binding(get: { run.auditActor }, set: { run.auditActor = $0 })) {
                    Text("全部参与者").tag("")
                    ForEach(actors, id: \.self) { Text(run.who($0).name).tag($0) }
                }
                .frame(width: 160)
                Text(verbatim: "\(rows.count) 条").font(.caption).foregroundStyle(.secondary)
                Spacer()
            }
            .labelsHidden()
            ScrollViewReader { proxy in
                List(selection: Binding(get: { run.highlight }, set: { run.highlight = $0 })) {
                    ForEach(rows) { e in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack(spacing: 10) {
                                Text(verbatim: "\(e.seq)").font(.system(.caption, design: .monospaced)).frame(width: 40, alignment: .trailing)
                                Text(L.clock(e.ts)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                                Text(e.type).font(.system(.caption, design: .monospaced)).frame(width: 170, alignment: .leading).lineLimit(1)
                                Text(run.who(e.actor).name + (e.to.map { " → \(run.who($0).name)" } ?? "")).font(.callout).frame(width: 150, alignment: .leading).lineLimit(1)
                                Text((e.task ?? "—") + (e.attempt.map { " #\($0)" } ?? "")).font(.system(.caption, design: .monospaced)).frame(width: 90, alignment: .leading).lineLimit(1)
                                Text(e.text).font(.callout).lineLimit(1)
                                Spacer()
                                Button("回放") { run.replay = e.seq; run.page = .board }.controlSize(.small)
                            }
                            if run.highlight == e.seq {
                                Text(e.raw.pretty()).font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
                                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                                    .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
                            }
                        }
                        .tag(e.seq)
                        .id(e.seq)
                    }
                }
                .listStyle(.inset(alternatesRowBackgrounds: true))
                .onAppear { if let h = run.highlight { DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { proxy.scrollTo(h, anchor: .center) } } }
            }
        }
        .padding(20)
    }

    private func invariants(_ events: [Event]) -> [(String, Bool, String)] {
        var active: [String: Int] = [:], numbers: [String: [Int]] = [:]
        var overlap = 0, gaps = 0, noEvidence = 0, unsubmitted = 0, badQuestion = 0, badReview = 0, badActor = 0
        var submitted = Set<String>()
        for e in events {
            let task = e.task ?? ""
            if e.type == "attempt.started" { if active[task] != nil { overlap += 1 }; active[task] = e.attempt ?? 0; numbers[task, default: []].append(e.attempt ?? 0) }
            if ["attempt.failed", "task.succeeded", "review.rejected"].contains(e.type) { active[task] = nil }
            if e.type == "result.submitted" { submitted.insert("\(task)#\(e.attempt ?? 0)") }
            if e.type == "task.succeeded" {
                if !e.data["summary"].truthy || e.data["evidence"].array.isEmpty { noEvidence += 1 }
                if !submitted.contains("\(task)#\(e.attempt ?? 0)") { unsubmitted += 1 }
            }
            if e.type == "question.asked" && !(e.data["question"].truthy && !e.data["options"].array.isEmpty) { badQuestion += 1 }
            if e.type.hasPrefix("review.") && !(!e.data["refs"].array.isEmpty || e.data["findings"].array.allSatisfy { $0["ref"].truthy }) { badReview += 1 }
            if e.type.range(of: #"^(task\.(ready|assigned|succeeded|reopened|retry_scheduled)|attempt\.)"#, options: .regularExpression) != nil && e.actor != "janus" { badActor += 1 }
        }
        for ns in numbers.values { for (i, n) in ns.enumerated() where n != i + 1 { gaps += 1 } }
        let attempts = numbers.values.reduce(0) { $0 + $1.count }
        return [
            ("每个任务同一时间最多一个进行中的尝试", overlap == 0, "\(attempts) 次尝试"),
            ("重试会新建尝试，旧记录保留", gaps == 0, "\(numbers.count) 个任务"),
            ("任务状态只由编排器（Janus）改变", badActor == 0, "Agent 只提交进度与结果"),
            ("标记完成前已提交结构化结果和证据", noEvidence == 0 && unsubmitted == 0, "\(events.filter { $0.type == "task.succeeded" }.count) 次完成"),
            ("等待输入时附带可回答的问题", badQuestion == 0, "\(events.filter { $0.type == "question.asked" }.count) 个问题"),
            ("评审结论引用了具体产物或证据", badReview == 0, "\(events.filter { $0.type.hasPrefix("review.") }.count) 次评审"),
        ]
    }
}

/// The team snapshot stored with the run.
struct TeamView: View {
    let run: RunModel

    var body: some View {
        let full = run.state
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("团队与权限").font(.title2.weight(.semibold))
                        Text("Agent 定义描述它能做什么；团队成员身份决定它在这个目录里以什么角色、什么权限工作。下面是这个 Run 创建时的团队快照。").font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if let ws = run.store.workspace(run.workspaceId) { Button("工作区设置") { run.store.editingWorkspace = ws } }
                }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 12, alignment: .top)], spacing: 12) {
                    ForEach(run.team, id: \.self) { m in
                        let id = m["id"].str
                        let w = run.who(id)
                        let (pl, pt) = L.phase(full.agents[id]?.phase ?? "idle")
                        Card {
                            HStack(spacing: 10) {
                                Avatar(initials: w.initials, color: w.color, size: 34)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(w.name + (w.lead ? " · 主控" : "")).font(.headline)
                                    Text(verbatim: "\(w.role) · \(w.runtime) · ACP").font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Pill(pl, pt)
                            }
                            if m["desc"].truthy { Text(m["desc"].str).font(.callout) }
                            KV(rows: [("工作区策略", L.wsLabel[m["ws"].str] ?? "—"), ("权限", m["limits"].array.last?.str ?? "—"), ("最大并发", "1"),
                                      ("Agent", "\(w.runtime) · \(w.model.isEmpty ? "默认模型" : w.model)\(m["effort"].truthy ? " · \(m["effort"].str)" : "")")])
                        }
                    }
                }
            }
            .padding(20)
        }
    }
}
