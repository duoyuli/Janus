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
                Text("在当前回放位置，这个任务还不存在。").foregroundStyle(.secondary)
                Button("返回工作台") { run.page = .board }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func content(_ st: RunState, _ t: TaskState) -> some View {
        let (label, tone) = L.task(t.status)
        let evs = st.events.filter { e in e.task == t.id || e.data["target"].str == t.id || e.data["tasks"].strings.contains(t.id) || e.data["unlocked"].strings.contains(t.id) }
        let lastStart = evs.last { $0.type == "attempt.started" }?.data ?? .null
        let w = run.who(t.agent)
        return ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Button { run.page = .board } label: { Label("工作台", systemImage: "chevron.left") }.buttonStyle(.link)
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 6) {
                            TaskTag(id: t.id)
                            Pill(label, tone)
                            if t.attempts.count > 1 { Pill("第 \(t.attempts.count) 次尝试") }
                        }
                        Text(t.title).font(.title2.weight(.semibold)).textSelection(.enabled)
                        if !t.desc.isEmpty { Text(t.desc).foregroundStyle(.secondary).textSelection(.enabled) }
                    }
                    Spacer()
                    Button("在审计日志中查看") { run.auditCat = "all"; run.auditTask = t.id; run.auditActor = ""; run.highlight = nil; run.page = .audit }
                }
                HStack(alignment: .top, spacing: 16) {
                    VStack(alignment: .leading, spacing: 14) {
                        ChatCard(run: run, st: st, title: "会话", info: taskSession(st, t), target: .task(t.id))
                            .frame(height: 560)
                        Card {
                            header("验收标准", "执行者自己验证，逐条给出证据")
                            ForEach(t.accept.indices, id: \.self) { i in
                                let c = i < t.checks.count ? t.checks[i] : nil
                                HStack(alignment: .firstTextBaseline, spacing: 8) {
                                    Text(c.map { $0.passed ? "✓" : "✗" } ?? "○").fontWeight(.bold).foregroundStyle(c.map { $0.passed ? Tone.green.color : Tone.red.color } ?? .secondary)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(t.accept[i])
                                        if let ev = c?.evidence, !ev.isEmpty { Text(MD.inline(ev)).font(.caption).foregroundStyle(.secondary) }
                                    }
                                }
                            }
                            if !t.summary.isEmpty {
                                Note(text: "结构化结果：\(t.summary)\n证据：\(t.evidence.joined(separator: "；"))")
                            }
                        }
                        Card {
                            header("执行轨迹", "\(evs.count) 条事件 · 按尝试分组")
                            if evs.isEmpty { Text("任务还没有开始。").foregroundStyle(.secondary) }
                            ForEach(evs) { e in TimelineRow(run: run, e: e) }
                            Text("这里展示 Agent 通过 ACP 回传的工具调用、结构化结果和产物；模型的原始思考（agent_thought_chunk）不会被记录。").font(.caption).foregroundStyle(.secondary)
                        }
                        Card {
                            header("Prompt", "最近一次尝试中由编排器通过 ACP 发给 Agent")
                            CodeBlock(text: lastStart["prompt"].truthy ? lastStart["prompt"].str : "任务还没有开始，Prompt 会在启动尝试时由编排器生成。", maxHeight: 420)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    VStack(alignment: .leading, spacing: 14) {
                        Card {
                            header("为什么分配给 \(w.name)", "主控指定 · 记录于 \(t.delegated ? "orchestration.accepted" : "plan.proposed")")
                            ForEach(t.candidates.indices, id: \.self) { i in
                                let c = t.candidates[i]
                                let cw = run.who(c["id"].str)
                                HStack(alignment: .top) {
                                    Avatar(initials: cw.initials, color: cw.color, size: 18)
                                    Text(cw.name + (c["id"].str == t.agent ? " ✓" : "")).fontWeight(c["id"].str == t.agent ? .semibold : .regular)
                                    Spacer()
                                    Text(c["excluded"].truthy ? "已排除：\(c["excluded"].str)" : c["note"].str).font(.caption).foregroundStyle(.secondary)
                                }
                            }
                            if t.candidates.isEmpty { Text(verbatim: "\(w.name)（\(w.role)）").font(.callout) }
                            if t.overridden { Text("你在确认计划时改派了执行者。").font(.caption).foregroundStyle(.secondary) }
                        }
                        Card {
                            header("依赖", "")
                            depRow("上游", t.deps, st)
                            depRow("下游", st.order.filter { st.tasks[$0]?.deps.contains(t.id) == true }, st)
                        }
                        Card {
                            header("尝试记录", "重试会新建尝试，旧记录保留")
                            if t.attempts.isEmpty { Text("还没有尝试。").foregroundStyle(.secondary) }
                            ForEach(t.attempts.indices, id: \.self) { i in
                                let a = t.attempts[i]
                                let (al, at) = L.attemptStatus[a.status] ?? (a.status, .gray)
                                VStack(alignment: .leading, spacing: 4) {
                                    HStack {
                                        Text(verbatim: "#\(a.n)").fontWeight(.semibold)
                                        Pill(al, at)
                                        Text(L.short(a.session.split(separator: "/").last.map(String.init) ?? "", 24)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).help(a.session)
                                        Spacer()
                                        Text(L.clock(a.start)).font(.caption).foregroundStyle(.secondary)
                                    }
                                    if let err = a.error, !err.isEmpty { Note(text: err, tone: .red) }
                                }
                            }
                        }
                        Card {
                            header("运行环境", "")
                            let cfg = lastStart["config"]
                            let rt = [lastStart["runtime"].truthy ? lastStart["runtime"].str : w.runtime,
                                      cfg["model"].truthy ? cfg["model"].str + (cfg["effort"].truthy ? " (\(cfg["effort"].str))" : "") : nil,
                                      cfg["label"].truthy || cfg["mode"].truthy ? "权限 \(cfg["label"].truthy ? cfg["label"].str : cfg["mode"].str)" : nil,
                                      lastStart["agentInfo"].truthy ? "\(lastStart["agentInfo"]["name"].str) \(lastStart["agentInfo"]["version"].str)" : nil].compactMap { $0 }.joined(separator: " · ")
                            KV(rows: [("Agent", run.agentName(t.agent)), ("运行时", rt), ("协议", "ACP"), ("隔离方式", lastStart["enforcement"].truthy ? lastStart["enforcement"].str : "—"),
                                      ("会话", t.attempts.last?.session ?? "—"), ("工作区", t.attempts.last?.workspace ?? "工作目录"), ("工作区策略", L.wsLabel[w.m["ws"].str] ?? "—")])
                        }
                        Card {
                            header("产物", "\(t.artifacts.count) 个")
                            if t.artifacts.isEmpty { Text("还没有产物。").foregroundStyle(.secondary) }
                            ForEach(t.artifacts, id: \.seq) { a in
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack { Text(a.name).fontWeight(.semibold); Text(verbatim: "尝试 \(a.attempt)").font(.caption).foregroundStyle(.secondary); SeqButton(seq: a.seq, run: run); Spacer(); Button("查看内容") { ArtifactWindow.open(run: run, path: a.path, title: a.name) }.buttonStyle(.link) }
                                    if !a.desc.isEmpty { Text(a.desc).font(.caption).foregroundStyle(.secondary) }
                                    Text(verbatim: "\(L.tilde(a.path)) · sha256 \(a.hash)…").font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary)
                                }
                            }
                        }
                        let reviews = st.events.filter { $0.type.hasPrefix("review.") && ($0.task == t.id || $0.data["target"].str == t.id) }
                        if !reviews.isEmpty {
                            Card {
                                header("评审", "")
                                ForEach(reviews) { e in
                                    HStack {
                                        Pill(e.type == "review.approved" ? "通过" : "驳回", e.type == "review.approved" ? .green : .red)
                                        Text(verbatim: "\(run.who(e.actor).name) · \(e.task ?? "") 尝试 \(e.attempt ?? 0)").font(.callout)
                                        Spacer()
                                        SeqButton(seq: e.seq, run: run)
                                    }
                                }
                            }
                        }
                    }
                    .frame(width: 330)
                }
            }
            .padding(20)
        }
    }

    private func header(_ t: String, _ sub: String) -> some View {
        HStack { Text(t).font(.headline); Spacer(); Text(sub).font(.caption).foregroundStyle(.secondary) }
    }

    private func depRow(_ label: String, _ ids: [String], _ st: RunState) -> some View {
        HStack(alignment: .top) {
            Text(label).foregroundStyle(.secondary).frame(width: 40, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
                if ids.isEmpty { Text("无") }
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
                Text(verbatim: "尝试 \(e.attempt ?? 0)").fontWeight(.semibold)
                Text(d["runtime"].str).foregroundStyle(.secondary)
                Text(L.short(d["session"].str, 40)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                Text(d["workspace"].str).font(.caption).foregroundStyle(.secondary).lineLimit(1)
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
            Text(verbatim: "\(d["tool"].str) \(L.short(d["input"].str, 200))\n\(e.type == "tool.started" ? "等待返回…" : "→ " + L.short(d["output"].str, 200))").font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
        case "decision.summary": Note(text: "\(d["title"].str)\n\(d["why"].str)")
        case "question.asked": Note(text: "\(d["question"].str)\n选项：\(d["options"].strings.joined(separator: " / "))")
        case "question.answered": if d["note"].truthy { Note(text: "补充说明：\(d["note"].str)") }
        case "attempt.failed": Note(text: d["error"].str, tone: .red)
        case "task.retry_scheduled": Note(text: "重试策略：\(d["strategy"].str)")
        case "review.rejected": ForEach(d["findings"].array.indices, id: \.self) { i in Note(text: "[\(d["findings"][i]["severity"].str)] \(d["findings"][i]["text"].str)\n\(d["findings"][i]["ref"].str)", tone: .red) }
        case "review.approved": Note(text: "引用：\(d["refs"].strings.joined(separator: "；"))")
        case "artifact.created": Note(text: "\(d["desc"].str)\n\(L.tilde(d["path"].str)) · sha256 \(d["hash"].str)…")
        case "policy.violation": Note(text: d["files"].strings.joined(separator: "、"), tone: .red)
        case "result.submitted": if !d["risks"].array.isEmpty { Note(text: "风险：\(d["risks"].strings.joined(separator: "；"))", tone: .amber) }
        case "approval.requested": CodeBlock(text: "$ \(d["command"].str)")
        default: EmptyView()
        }
    }
}
