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
            // One line when every invariant holds; the list opens on its own when one does not.
            let invs = full.audit
            let broken = invs.filter { !$0.1 }.count
            DisclosureGroup(isExpanded: Binding(get: { broken > 0 || run.invariantsOpen }, set: { run.invariantsOpen = $0 })) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), alignment: .leading)], alignment: .leading, spacing: 6) {
                    ForEach(invs, id: \.0) { inv in
                        HStack(spacing: 6) {
                            Text(inv.1 ? "✓" : "✕").fontWeight(.bold).foregroundStyle(inv.1 ? Tone.green.color : Tone.red.color)
                                .accessibilityLabel(inv.1 ? L.text("满足") : L.text("不满足"))
                            Text(inv.0).font(.callout)
                            Text(inv.2).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(.top, 6)
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: broken == 0 ? "checkmark.seal" : "exclamationmark.triangle.fill").foregroundStyle(broken == 0 ? Tone.green.color : Tone.red.color).accessibilityHidden(true)
                    Text(broken == 0 ? L.text("系统规则全部满足") : L.text("\(broken) 条系统规则没满足")).font(.callout.weight(.medium))
                    Text(verbatim: L.text("\(invs.count) 条，基于 \(events.count) 条事件")).font(.caption).foregroundStyle(.secondary)
                }
            }
            .help(L.text("工作台的状态都由这些事件推出。选中一条看原文，点「回放到这里」看当时的样子。"))
            HStack(spacing: 10) {
                Picker(L.text("事件类别"), selection: Binding(get: { run.auditCat }, set: { run.auditCat = $0 })) {
                    ForEach(L.categoryLabel, id: \.0) { Text($0.1).tag($0.0) }
                }
                .pickerStyle(.segmented).frame(maxWidth: 460)
                Picker(L.text("子任务"), selection: Binding(get: { run.auditTask }, set: { run.auditTask = $0 })) {
                    Text(L.text("全部子任务")).tag("")
                    ForEach(full.order, id: \.self) { id in Text(verbatim: "\(id) \(L.short(full.tasks[id]?.title ?? "", 14))").tag(id) }
                }
                .frame(width: 200)
                Picker(L.text("参与者"), selection: Binding(get: { run.auditActor }, set: { run.auditActor = $0 })) {
                    Text(L.text("全部参与者")).tag("")
                    ForEach(actors, id: \.self) { Text(run.who($0).name).tag($0) }
                }
                .frame(width: 160)
                Text(verbatim: L.text("\(rows.count) 条")).font(.caption).foregroundStyle(.secondary)
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
                                Text(e.raw["type"].str).font(.system(.caption, design: .monospaced)).frame(width: 170, alignment: .leading).lineLimit(1)
                                Text(run.who(e.actor).name + (e.to.map { " → \(run.who($0).name)" } ?? "")).font(.callout).frame(width: 150, alignment: .leading).lineLimit(1)
                                Text((e.task ?? "—") + (e.attempt.map { " #\($0)" } ?? "")).font(.system(.caption, design: .monospaced)).frame(width: 90, alignment: .leading).lineLimit(1)
                                Text(e.text).font(.callout).lineLimit(1)
                                Spacer()
                                Button(L.text("回放到这里")) { run.replay = e.seq; run.page = .board }.controlSize(.small).help(L.text("把工作台回放到第 \(e.seq) 条事件时的状态"))
                            }
                            // A result event opened here shows each criterion with its evidence
                            // method and evidence, so the log does not have to be read as raw JSON.
                            if run.highlight == e.seq, !e.data["checks"].array.isEmpty,
                               [.resultSubmitted, .verificationPassed, .verificationFailed, .taskSucceeded].contains(e.type) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(L.text("逐条验收")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                                    CheckLines(lines: full.checks[e.seq] ?? [], dense: true)
                                }
                                .padding(.leading, 50)
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
}

/// The team snapshot stored with the run, and where it works.
struct TeamView: View {
    let run: RunModel

    var body: some View {
        let full = run.state
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(L.text("团队与权限")).font(.display(.title2))
                        Text(L.text("这个任务创建时的团队快照。")).font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if let ws = run.store.workspace(run.workspaceId) { Button(L.text("工作区设置…")) { run.store.editingWorkspace = ws.json } }
                }
                Card {
                    KV(rows: [(L.text("负责人"), run.agentName("lead")), (L.text("工作区"), full.page.workspaceNote)]
                       + (full.page.branchNote.map { [(L.text("工作目录"), full.run.cwd), (L.text("分支"), $0)] } ?? [])
                       + [(L.text("开始"), L.dateTime(full.run.start))])
                }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 12, alignment: .top)], spacing: 12) {
                    ForEach(run.team, id: \.self) { m in
                        let id = m["id"].str
                        let w = run.who(id)
                        let (pl, pt) = (full.agents[id]?.phase ?? .idle).label
                        Card {
                            HStack(spacing: 10) {
                                Avatar(initials: w.initials, color: w.color, size: 34, role: w.role)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(w.name + (w.lead ? L.text(" · 负责人") : "")).font(.headline)
                                    Text(verbatim: "\(w.runtime) · ACP").font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Pill(pl, pt)
                            }
                            if m["desc"].truthy { Text(m["desc"].str).font(.callout) }
                            KV(rows: [(L.text("权限"), m["permission"].truthy ? m["permission"].str : L.text("Agent 没有权限模式")),
                                      ("Agent", "\(w.runtime) · \(w.model.isEmpty ? L.text("默认模型") : w.model)\(m["effort"].truthy ? " · \(m["effort"].str)" : "")")])
                        }
                    }
                }
            }
            .padding(20)
        }
    }
}
