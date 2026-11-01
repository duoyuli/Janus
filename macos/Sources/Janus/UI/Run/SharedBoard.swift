import AppKit
import SwiftUI

/// The facts every agent can rely on, rebuilt from the event log: open items, conclusions,
/// artifacts, handoffs and decisions. Selecting a card highlights the tasks and agents it involves.
struct SharedBoard: View {
    let run: RunModel
    let st: RunState
    private let limit = 5

    var body: some View {
        if !st.run.goal.isEmpty {
            HStack {
                Text("共享看板").font(.title3.weight(.semibold))
                Spacer()
                Picker("", selection: Binding(get: { run.boardView }, set: { run.boardView = $0; run.boardSel = nil })) {
                    Text("看板").tag("board")
                    Text("动态").tag("feed")
                }
                .pickerStyle(.segmented)
                .frame(width: 140)
            }
            .padding(.top, 6)
            if run.boardView == "feed" { Feed(run: run, st: st) } else { board }
        }
    }

    private var board: some View {
        let f = boardFacts(st) { run.who($0).name }
        let open = f.open.filter { pick(by: nil, agents: $0.agents, tasks: $0.tasks) }
        return Card(padding: 12) {
            HStack(spacing: 10) {
                Picker("Agent", selection: Binding(get: { run.boardAgent }, set: { run.boardAgent = $0; run.boardSel = nil })) {
                    Text("全部 Agent").tag("")
                    ForEach(run.team, id: \.self) { m in Text(m["name"].str).tag(m["id"].str) }
                }
                .frame(width: 180)
                Picker("任务", selection: Binding(get: { run.boardTask }, set: { run.boardTask = $0; run.boardSel = nil })) {
                    Text("全部任务").tag("")
                    ForEach(st.order, id: \.self) { id in Text(verbatim: "\(id) \(L.short(st.tasks[id]?.title ?? "", 16))").tag(id) }
                }
                .frame(width: 220)
                if run.boardSel != nil { Button("取消高亮") { run.boardSel = nil }.buttonStyle(.link) }
                Spacer()
                if open.isEmpty { Text("没有卡住的事").font(.callout).foregroundStyle(Tone.green.color) }
            }
            .labelsHidden()
            if !open.isEmpty {
                lane("待处理", open.count, "open", yours: open.contains { $0.you }) {
                    ForEach(shown(open, "open")) { x in openCard(x) }
                }
            }
            let results = f.results.filter { pick(by: $0.by, agents: [], tasks: [$0.task] + $0.usedBy) }.reversed() as [ResultFact]
            let arts = f.arts.filter { pick(by: $0.a.by, agents: [], tasks: [$0.a.task] + $0.usedBy) }.reversed() as [ArtFact]
            let handoffs = f.handoffs.filter { pick(by: $0.by, agents: [$0.agent], tasks: [$0.from, $0.to]) }.reversed() as [HandoffFact]
            let decisions = f.decisions.filter { pick(by: $0.by, agents: $0.agents, tasks: $0.tasks) }.reversed() as [DecisionFact]
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 230), spacing: 10, alignment: .top)], alignment: .leading, spacing: 10) {
                lane("结论", results.count, "results") {
                    if results.isEmpty { empty("还没有任务交付结论") }
                    ForEach(shown(results, "results")) { x in resultCard(x) }
                }
                lane("产物", arts.count, "arts") {
                    if arts.isEmpty { empty("还没有产物") }
                    ForEach(shown(arts, "arts")) { x in artCard(x) }
                }
                lane("交接", handoffs.count, "handoffs") {
                    if handoffs.isEmpty { empty("还没有交接") }
                    ForEach(shown(handoffs, "handoffs")) { x in handoffCard(x) }
                }
                lane("决定", decisions.count, "decisions") {
                    if decisions.isEmpty { empty("还没有决定") }
                    ForEach(shown(decisions, "decisions")) { x in decisionCard(x) }
                }
            }
        }
    }

    private func pick(by: String?, agents: [String], tasks: [String]) -> Bool {
        (run.boardAgent.isEmpty || ([by].compactMap { $0 } + agents).contains(run.boardAgent)) && (run.boardTask.isEmpty || tasks.contains(run.boardTask))
    }

    private func shown<T>(_ items: [T], _ lane: String) -> [T] { run.boardMore.contains(lane) ? items : Array(items.prefix(limit)) }

    private func lane<C: View>(_ title: String, _ n: Int, _ k: String, yours: Bool = false, @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack { Text(title).font(.callout.weight(.semibold)); Text(verbatim: "\(n)").font(.caption).foregroundStyle(.secondary); Spacer() }
            content()
            if n > limit {
                Button(run.boardMore.contains(k) ? "收起" : "还有 \(n - limit) 条") {
                    if run.boardMore.contains(k) { run.boardMore.remove(k) } else { run.boardMore.insert(k) }
                }
                .buttonStyle(.link).font(.caption)
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .background((k == "open" ? (yours ? Tone.amber.color : Color.secondary) : Color.secondary).opacity(k == "open" ? 0.08 : 0.04), in: RoundedRectangle(cornerRadius: 10))
    }

    private func empty(_ t: String) -> some View { Text(t).font(.caption).foregroundStyle(.secondary) }

    private func by(_ id: String) -> some View {
        let w = run.who(id)
        return HStack(spacing: 4) { Avatar(initials: w.initials, color: w.color, size: 15); Text(w.name).font(.caption).foregroundStyle(.secondary) }
    }

    /// A card that expands when selected and highlights what it involves.
    private func fact<Head: View, More: View>(_ key: String, tasks: [String], agents: [String], old: Bool = false, urgent: Bool = false, @ViewBuilder head: () -> Head, @ViewBuilder more: () -> More) -> some View {
        let on = run.boardSel?.key == key
        return VStack(alignment: .leading, spacing: 6) {
            head()
            if on { Divider(); more() }
        }
        .padding(9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Color.accent : urgent ? Tone.amber.color.opacity(0.5) : Color.line.opacity(0.7)))
        .opacity(old && !on ? 0.6 : 1)
        .contentShape(Rectangle())
        .onTapGesture { run.boardSel = on ? nil : (key: key, tasks: tasks.filter { !$0.isEmpty }, agents: agents.filter { !$0.isEmpty }) }
    }

    private func preview(_ text: String, _ n: Int = 90) -> some View {
        Text(L.short(L.plain(text), n)).font(.callout).foregroundStyle(.primary.opacity(0.85))
    }

    private func full(_ text: String, _ n: Int = 90) -> some View {
        Group { if text.count > n { MarkdownText(text: text, font: .callout) } }
    }

    private func openCard(_ x: OpenFact) -> some View {
        fact(x.key, tasks: x.tasks, agents: x.agents, urgent: true) {
            HStack(spacing: 6) {
                Pill(x.you ? "等你决定" : "主控处理", x.you ? .amber : .gray)
                ForEach(x.tasks, id: \.self) { TaskTag(id: $0) }
                Text(x.title).font(.callout.weight(.semibold)).lineLimit(2)
            }
            if !x.text.isEmpty { preview(x.text) }
            if !x.sub.isEmpty { Text(x.sub).font(.caption).foregroundStyle(.secondary) }
        } more: {
            full(x.text)
            HStack {
                if let t = x.tasks.first { Button("去处理") { run.page = .task(t) } }
                if let a = x.agents.first { Button(String("和 \(run.who(a).name) 对话")) { run.page = .agent(a) } }
                SeqButton(seq: x.seq, run: run)
            }
            .controlSize(.small)
        }
    }

    private func resultCard(_ x: ResultFact) -> some View {
        fact(x.key, tasks: [x.task] + x.usedBy, agents: [x.by] + x.usedBy.compactMap { st.tasks[$0]?.agent }, old: x.old) {
            HStack(spacing: 6) {
                TaskTag(id: x.task); by(x.by)
                if x.old { Text("旧版本").font(.caption).foregroundStyle(.secondary) }
                if !x.amended.isEmpty { Text("已被主控修订").font(.caption).foregroundStyle(Tone.amber.color) }
            }
            preview(x.summary)
            Text([x.checks.isEmpty ? nil : "验收 \(x.checks.filter(\.passed).count)/\(x.checks.count)", "\(x.evidence.count) 条证据", x.risks.isEmpty ? nil : "\(x.risks.count) 条风险", x.usedBy.isEmpty ? nil : "交给 \(x.usedBy.joined(separator: "、"))"].compactMap { $0 }.joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary)
        } more: {
            full(x.summary)
            if !x.checks.isEmpty { list("验收", x.checks.map { "\($0.passed ? "✓" : "✗") \($0.criterion)：\($0.evidence)" }) }
            if !x.amended.isEmpty { list("主控的修订（以此为准）", x.amended) }
            if !x.evidence.isEmpty { list("证据", x.evidence) }
            if !x.risks.isEmpty { list("风险", x.risks.map { "⚠ " + $0 }) }
            HStack {
                Button("任务详情") { run.page = .task(x.task) }
                Button(String("和 \(run.who(x.by).name) 对话")) { run.page = .agent(x.by) }
                SeqButton(seq: x.seq, run: run)
            }
            .controlSize(.small)
        }
    }

    private func list(_ title: String, _ items: [String]) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            ForEach(items.indices, id: \.self) { Text(MD.inline(items[$0])).font(.caption).textSelection(.enabled) }
        }
    }

    private func artCard(_ x: ArtFact) -> some View {
        let a = x.a
        return fact("a\(a.seq)", tasks: [a.task] + x.usedBy, agents: [a.by], old: x.old) {
            HStack(spacing: 6) { TaskTag(id: a.task); Text(a.name).font(.system(.callout, design: .monospaced).weight(.semibold)).lineLimit(1) }
            Text([L.artKind[a.kind] ?? a.kind, a.bytes.map { L.kb($0) }, x.old ? "旧版本" : nil, x.usedBy.isEmpty ? nil : "交给 \(x.usedBy.joined(separator: "、"))"].compactMap { $0 }.joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary)
        } more: {
            if !a.desc.isEmpty { Text(a.desc).font(.callout) }
            Text(verbatim: "由 \(run.who(a.by).name) 在第 \(a.attempt) 次尝试中提交\(a.hash.isEmpty ? "" : " · \(a.hash)")").font(.caption).foregroundStyle(.secondary)
            HStack { Button("打开") { ArtifactWindow.open(run: run, path: a.path, title: a.name) }; SeqButton(seq: a.seq, run: run) }.controlSize(.small)
        }
    }

    private func handoffCard(_ x: HandoffFact) -> some View {
        fact(x.key, tasks: [x.from, x.to], agents: [x.by, x.agent]) {
            HStack(spacing: 6) {
                TaskTag(id: x.from); Image(systemName: "arrow.right").font(.caption); TaskTag(id: x.to); by(x.agent)
            }
            preview(x.summary.isEmpty ? "上游没有提交结论" : x.summary, 80)
            if !x.artifacts.isEmpty { Text(verbatim: "带上 \(x.artifacts.count) 个产物").font(.caption).foregroundStyle(.secondary) }
        } more: {
            ForEach(x.artifacts, id: \.self) { Text($0).font(.system(.caption, design: .monospaced)) }
            Text(verbatim: "\(L.clock(x.ts)) 注入 \(run.who(x.agent).name) 的新会话").font(.caption).foregroundStyle(.secondary)
            HStack {
                if x.to != "lead" { Button(String("看 \(x.to) 收到了什么")) { run.page = .task(x.to) } }
                SeqButton(seq: x.seq, run: run)
            }
            .controlSize(.small)
        }
    }

    private func decisionCard(_ x: DecisionFact) -> some View {
        fact(x.key, tasks: x.tasks, agents: x.agents) {
            HStack(spacing: 6) {
                ForEach(x.tasks.prefix(2), id: \.self) { TaskTag(id: $0) }
                Text(x.title).font(.callout.weight(.semibold)).lineLimit(2)
            }
            preview(x.text)
            HStack(spacing: 4) {
                Text(verbatim: "\(run.who(x.by).name) · \(L.clock(x.ts))").font(.caption).foregroundStyle(.secondary)
                if x.lost { Text("· 未送达").font(.caption).foregroundStyle(Tone.amber.color) }
            }
        } more: {
            full(x.text)
            HStack {
                if let a = x.agents.first, a != "user" { Button(String("和 \(run.who(a).name) 对话")) { run.page = .agent(a) } }
                SeqButton(seq: x.seq, run: run)
            }
            .controlSize(.small)
        }
    }
}

/// The latest events, without progress chatter.
struct Feed: View {
    let run: RunModel
    let st: RunState

    var body: some View {
        let all = st.events.filter { L.category($0.type) != "progress" }
        let list = Array(all.filter { run.feedCategory == "all" || L.category($0.type) == run.feedCategory }.suffix(14).reversed())
        Card(padding: 12) {
            Picker("", selection: Binding(get: { run.feedCategory }, set: { run.feedCategory = $0 })) {
                ForEach(L.categoryLabel.filter { $0.0 != "progress" }, id: \.0) { Text($0.1).tag($0.0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if list.isEmpty { Text("还没有事件。").foregroundStyle(.secondary) }
            ForEach(list) { e in EventRow(run: run, e: e) }
            Button(String("查看全部 \(st.events.count) 条事件 →")) { run.page = .audit }.buttonStyle(.link)
        }
    }
}

struct EventRow: View {
    let run: RunModel
    let e: Event
    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            EventIcon(type: e.type)
            VStack(alignment: .leading, spacing: 3) {
                (Text(run.who(e.actor).name + (e.to.map { " → \(run.who($0).name)" } ?? "")).fontWeight(.semibold) + Text("　" + e.text)).font(.callout)
                HStack(spacing: 8) {
                    Text(e.type).font(.system(.caption, design: .monospaced))
                    if let t = e.task { Text(t + (e.attempt.map { " · 尝试 \($0)" } ?? "")).font(.caption) }
                    Text(L.clock(e.ts)).font(.caption)
                    SeqButton(seq: e.seq, run: run)
                }
                .foregroundStyle(.secondary)
            }
        }
    }
}

struct EventIcon: View {
    let type: String
    var body: some View {
        let tone = L.tone(of: type)
        Text(L.icon[type] ?? "·")
            .font(.system(size: 12, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(tone?.color ?? .secondary)
            .background((tone?.color ?? .secondary).opacity(0.12), in: Circle())
    }
}

/// A window showing an artifact (a reply, a diff, a test report).
enum ArtifactWindow {
    @MainActor
    static func open(run: RunModel, path: String, title: String) {
        let view = ArtifactView(api: run.api, path: path)
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 820, height: 640), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        w.title = title
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: view)
        w.center()
        w.makeKeyAndOrderFront(nil)
    }
}

struct ArtifactView: View {
    let api: API
    let path: String
    @Local private var text: String?
    @Local private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(L.tilde(path)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("在 Finder 中显示") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
                Button("复制") { copyText(text ?? "") }.disabled(text == nil)
            }
            .padding(10)
            Divider()
            if let text {
                ScrollView([.vertical, .horizontal]) {
                    Text(text).font(.system(size: 12, design: .monospaced)).textSelection(.enabled).padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
            } else if let error {
                Text(error).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            do { text = try await api.text("/api/file?path=\(API.q(path))") } catch {
                // Deliverables can live outside the artifacts directory; this user can read them directly.
                if let t = try? String(contentsOfFile: path, encoding: .utf8) { text = t } else { self.error = error.localizedDescription }
            }
        }
    }
}
