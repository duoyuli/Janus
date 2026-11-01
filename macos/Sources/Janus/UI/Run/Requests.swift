import SwiftUI

/// Every request an agent puts in front of the person shares one card: who asks and where, what it
/// is about, then the agent's own options or a form built from its JSON Schema.
// @project-doc docs/architecture/macos_app.md#request_cards
struct RequestCard: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        switch item.kind {
        case "question": if item.task == nil { LeadAsk(run: run, st: st, item: item) } else { QuestionAsk(run: run, st: st, item: item) }
        case "approval": ApprovalAsk(run: run, st: st, item: item)
        case "input": InputAsk(run: run, st: st, item: item)
        case "budget": BudgetAsk(run: run, st: st, item: item)
        case "session": SessionAsk(run: run, st: st, item: item)
        default: EmptyView()
        }
    }

    /// Kinds that still have a card; plan confirmations and old failure decisions belong to the
    /// removed plan-first flow and have nothing left to answer them with.
    static let kinds: Set<String> = ["question", "approval", "input", "budget", "session"]
}

struct AskShell<Body: View, Controls: View>: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem
    let pill: String
    var tone: Tone = .amber
    let agent: String
    let title: String
    var foot: [(String, String)] = []
    @ViewBuilder var content: Body
    @ViewBuilder var controls: Controls

    var body: some View {
        let w = run.who(agent)
        let whereText = item.task.map { "\($0) · \(st.tasks[$0]?.title ?? "")" } ?? "主控会话"
        Card(tint: tone.color) {
            HStack(spacing: 8) {
                Pill(pill, tone)
                Avatar(initials: w.initials, color: w.color, size: 20)
                Text(title).font(.headline).lineLimit(3)
                Spacer()
                if let e = st.event(item.seq) { Text(L.clock(e.ts)).font(.caption).foregroundStyle(.secondary) }
                SeqButton(seq: item.seq, run: run)
            }
            content
            KV(rows: [("来自", "\(w.name) · \(whereText)")] + foot)
            controls
        }
    }
}

struct OptionButtons: View {
    let options: [String]
    var recommend: String = ""
    @Binding var picked: String?
    var disabled: Bool
    var body: some View {
        FlowLayout(spacing: 8) {
            ForEach(options, id: \.self) { o in
                Button {
                    picked = picked == o ? nil : o
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(o).multilineTextAlignment(.leading)
                        if o == recommend { Text("主控建议").font(.caption2).foregroundStyle(Color.accent) }
                    }
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(picked == o ? Color.accent.opacity(0.14) : Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(picked == o ? Color.accent : Color.line))
                }
                .buttonStyle(.plain)
                .disabled(disabled)
            }
        }
    }
}

struct QuestionAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        let t = st.tasks[item.task ?? ""]
        let q = t?.question
        let from = q?.from ?? t?.agent ?? ""
        let downstream = st.order.filter { st.tasks[$0]?.deps.contains(item.task ?? "") == true }
        AskShell(run: run, st: st, item: item, pill: "问题", agent: from, title: "\(run.who(from).name) 在 \(item.task ?? "") 中提问\(q?.blocking == true ? " · 阻塞中" : "")") {
            Text(q?.question ?? "").font(.body).textSelection(.enabled)
            if item.d["reason"].truthy { Text(verbatim: "↳ 主控 Agent 转交给你：\(item.d["reason"].str)").font(.callout).foregroundStyle(.secondary) }
        } controls: {
            OptionButtons(options: q?.options ?? [], recommend: item.d["recommend"].str, picked: pick, disabled: !run.canAct)
            TextField("补充说明（可选），会和回答一起发给 Agent", text: note, axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(2...6).disabled(!run.canAct)
            HStack {
                Button("提交回答") { answer(task: item.task ?? "") }.buttonStyle(.borderedProminent).disabled(!run.canAct)
                Text(verbatim: "回答会写入事件日志，并通过 ACP 发回会话。\(downstream.isEmpty ? "" : "受影响的下游任务：\(downstream.joined(separator: "、"))。")").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    var pick: Binding<String?> { Binding(get: { run.picks[item.key] }, set: { run.picks[item.key] = $0 }) }
    var note: Binding<String> { Binding(get: { run.notes[item.key] ?? "" }, set: { run.notes[item.key] = $0 }) }

    func answer(task: String) {
        guard let a = run.picks[item.key] else { run.store.show("先选择一个选项。"); return }
        Task { await run.act(["type": "answer", "key": JSON(item.key), "task": JSON(task), "answer": JSON(a), "note": JSON((run.notes[item.key] ?? "").trimmingCharacters(in: .whitespacesAndNewlines))]) }
    }
}

/// A decision the lead handed to the person (janus.ask_user): it shapes the whole run.
struct LeadAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        let options = item.d["options"].strings
        AskShell(run: run, st: st, item: item, pill: "需要你决定", agent: "lead", title: "主控请你决定：\(item.d["question"].str)") {
            if item.d["reason"].truthy { Text(verbatim: "为什么要你决定：\(item.d["reason"].str)").font(.callout).foregroundStyle(.secondary) }
        } controls: {
            if !options.isEmpty {
                OptionButtons(options: options, recommend: item.d["recommend"].str, picked: Binding(get: { run.picks[item.key] }, set: { run.picks[item.key] = $0 }), disabled: !run.canAct)
            }
            TextField(options.isEmpty ? "你的回答" : "补充说明（可选）", text: Binding(get: { run.notes[item.key] ?? "" }, set: { run.notes[item.key] = $0 }), axis: .vertical)
                .textFieldStyle(.roundedBorder).lineLimit(2...6).disabled(!run.canAct)
            HStack {
                Button("提交") { submit() }.buttonStyle(.borderedProminent).disabled(!run.canAct)
                Text("主控在等这个决定期间，会继续做不依赖它的工作。").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    func submit() {
        var note = (run.notes[item.key] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        var answer = run.picks[item.key] ?? ""
        // A free-form decision (no options) is answered with the text itself.
        if answer.isEmpty && !note.isEmpty { answer = note; note = "" }
        guard !answer.isEmpty else { run.store.show("先选择一个选项，或写下你的回答。"); return }
        Task { await run.act(["type": "answer", "key": JSON(item.key), "task": "", "answer": JSON(answer), "note": JSON(note)]) }
    }
}

/// ACP permission request: the agent's own options, each answered by its optionId.
struct ApprovalAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        let agent = item.agent ?? st.tasks[item.task ?? ""]?.agent ?? ""
        let d = item.d
        var opts = d["options"].array
        if opts.isEmpty { opts = [["optionId": "", "name": "批准这一次", "kind": "allow_once"], ["optionId": "", "name": "拒绝", "kind": "reject_once"]] }
        let diffs = d["content"].array.filter { $0["type"].str == "diff" }
        return AskShell(run: run, st: st, item: item, pill: "审批", tone: .red, agent: agent, title: "\(run.who(agent).name) 请求你决定：\(d["title"].truthy ? d["title"].str : "执行操作")", foot: d["policy"].truthy ? [("原因", d["policy"].str)] : []) {
            if d["command"].truthy && d["command"] != d["title"] { CodeBlock(text: "$ \(d["command"].str)") }
            ForEach(diffs.indices, id: \.self) { i in CodeBlock(text: "\(diffs[i]["path"].str)\n\(String(diffs[i]["newText"].str.prefix(2000)))") }
        } controls: {
            HStack {
                ForEach(opts.indices, id: \.self) { i in
                    let o = opts[i]
                    let allow = o["kind"].str.hasPrefix("allow")
                    let button = Button(o["name"].truthy ? o["name"].str : o["optionId"].str) {
                        Task { await run.act(["type": "approve", "key": JSON(item.key), "task": JSON(item.task ?? ""), "agent": JSON(item.agent ?? ""), "allow": JSON(allow), "option": o["optionId"].truthy ? o["optionId"] : .null, "label": o["name"]]) }
                    }
                    .disabled(!run.canAct)
                    if allow && o["kind"].str == "allow_once" { button.buttonStyle(.borderedProminent) } else if allow { button.buttonStyle(.bordered) } else { button.buttonStyle(.bordered).tint(Tone.red.color) }
                }
                Text(d["impact"].str).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// ACP elicitation: a form (JSON Schema) or a link to finish elsewhere.
struct InputAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        let agent = item.agent ?? ""
        let d = item.d
        if d["mode"].str == "url" {
            AskShell(run: run, st: st, item: item, pill: "打开链接", agent: agent, title: "\(run.who(agent).name) 需要你在浏览器里完成一步") {
                Text(d["message"].str)
                if d["url"].truthy { Link(d["url"].str, destination: URL(string: d["url"].str) ?? URL(string: "about:blank")!).font(.system(.callout, design: .monospaced)) }
            } controls: {
                HStack {
                    Button("已完成") { send("accept") }.buttonStyle(.borderedProminent).disabled(!run.canAct)
                    Button("拒绝") { send("decline") }.disabled(!run.canAct)
                }
            }
        } else {
            AskShell(run: run, st: st, item: item, pill: "需要填写", agent: agent, title: "\(run.who(agent).name) 需要你提供信息") {
                Text(d["message"].str)
                SchemaForm(run: run, key: item.key, schema: d["schema"])
            } controls: {
                HStack {
                    Button("提交") { send("accept") }.buttonStyle(.borderedProminent).disabled(!run.canAct)
                    Button("跳过") { send("decline") }.disabled(!run.canAct)
                    Button("取消", role: .destructive) { send("cancel") }.disabled(!run.canAct)
                    Text("内容通过 ACP 原样交回 Agent。").font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    func send(_ action: String) {
        let draft = run.forms[item.key]?.object ?? [:]
        let content = draft.filter { _, v in
            if v.isNull { return false }
            if v.string == "" { return false }
            if case .array(let a) = v, a.isEmpty { return false }
            return true
        }
        if action == "accept" {
            let missing = item.d["schema"]["required"].strings.filter { content[$0] == nil }
            if !missing.isEmpty {
                run.store.show("还有必填项：\(missing.map { item.d["schema"]["properties"][$0]["title"].truthy ? item.d["schema"]["properties"][$0]["title"].str : $0 }.joined(separator: "、"))")
                return
            }
        }
        Task { await run.act(["type": "input", "key": JSON(item.key), "task": JSON(item.task ?? ""), "agent": JSON(item.agent ?? ""), "action": JSON(action), "content": .object(content)]) }
    }
}

/// The subset of JSON Schema ACP forms use: single and multi select (oneOf/anyOf/enum), text,
/// number, boolean, and per-question "custom answer" fields.
struct SchemaForm: View {
    let run: RunModel
    let key: String
    let schema: JSON

    struct Choice: Hashable { let value: JSON; let label: String; let desc: String }

    var body: some View {
        let props = schema["properties"].object.sorted { $0.key < $1.key }
        let names = props.map(\.key)
        let required = Set(schema["required"].strings)
        let customs: [String: String] = Dictionary(uniqueKeysWithValues: names.compactMap { k in customFor(k, schema["properties"][k], names).map { ($0, k) } })
        VStack(alignment: .leading, spacing: 12) {
            ForEach(names, id: \.self) { k in
                let p = schema["properties"][k]
                if customFor(k, p, names) == nil {
                    VStack(alignment: .leading, spacing: 5) {
                        Text(p["title"].truthy ? p["title"].str : k).font(.callout.weight(.semibold)) + Text(required.contains(k) ? " *" : "").foregroundStyle(.secondary)
                        if p["description"].truthy { Text(p["description"].str).font(.caption).foregroundStyle(.secondary) }
                        field(k, p)
                        if let c = customs[k] {
                            TextField(schema["properties"][c]["description"].truthy ? schema["properties"][c]["description"].str : "或者写下你自己的回答", text: textBinding(c)).textFieldStyle(.roundedBorder)
                        }
                    }
                }
            }
        }
        .disabled(!run.canAct)
    }

    private func customFor(_ k: String, _ p: JSON, _ names: [String]) -> String? {
        if p["_meta"]["lody"]["elicitation"]["customAnswerFor"].truthy { return p["_meta"]["lody"]["elicitation"]["customAnswerFor"].str }
        if k.hasSuffix("_custom") {
            let base = String(k.dropLast(7))
            if names.contains(base) { return base }
        }
        return nil
    }

    private func choices(_ p: JSON) -> [Choice] {
        var list = p["oneOf"].array
        if list.isEmpty { list = p["anyOf"].array }
        if list.isEmpty { list = p["items"]["anyOf"].array }
        if list.isEmpty { list = p["items"]["oneOf"].array }
        if list.isEmpty {
            let en = p["enum"].array.isEmpty ? p["items"]["enum"].array : p["enum"].array
            list = en.map { ["const": $0, "title": JSON($0.str)] }
        }
        return list.map { o in
            let title = o["title"].truthy ? o["title"].str : o["const"].str
            let parts = title.components(separatedBy: " — ")
            let label = o["_meta"]["label"].truthy ? o["_meta"]["label"].str : parts[0]
            let desc = o["description"].truthy ? o["description"].str : o["_meta"]["description"].truthy ? o["_meta"]["description"].str : parts.dropFirst().joined(separator: " — ")
            return Choice(value: o["const"], label: label, desc: desc)
        }
    }

    @ViewBuilder private func field(_ k: String, _ p: JSON) -> some View {
        let opts = choices(p)
        let multi = p["type"].str == "array"
        let cur = run.forms[key]?[k] ?? .null
        if !opts.isEmpty {
            FlowLayout(spacing: 8) {
                ForEach(opts, id: \.self) { o in
                    let on = multi ? cur.array.contains(o.value) : cur == o.value
                    Button {
                        var d = run.forms[key] ?? [:]
                        if multi {
                            var set = d[k].array
                            if let i = set.firstIndex(of: o.value) { set.remove(at: i) } else { set.append(o.value) }
                            d[k] = .array(set)
                        } else { d[k] = d[k] == o.value ? .null : o.value }
                        run.forms[key] = d
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(o.label)
                            if !o.desc.isEmpty { Text(o.desc).font(.caption).foregroundStyle(.secondary) }
                        }
                        .padding(.horizontal, 10).padding(.vertical, 6)
                        .background(on ? Color.accent.opacity(0.14) : Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
                        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Color.accent : Color.line))
                    }
                    .buttonStyle(.plain)
                }
            }
        } else if p["type"].str == "boolean" {
            Toggle("", isOn: Binding(get: { run.forms[key]?[k].bool ?? false }, set: { var d = run.forms[key] ?? [:]; d[k] = JSON($0); run.forms[key] = d })).labelsHidden().toggleStyle(.switch)
        } else if ["number", "integer"].contains(p["type"].str) {
            TextField("", text: Binding(get: { run.forms[key]?[k].str ?? "" }, set: { v in var d = run.forms[key] ?? [:]; d[k] = Double(v).map { .number($0) } ?? .null; run.forms[key] = d }))
                .textFieldStyle(.roundedBorder).frame(maxWidth: 200)
        } else if let max = p["maxLength"].int, max <= 120 {
            TextField("", text: textBinding(k)).textFieldStyle(.roundedBorder)
        } else {
            TextField("", text: textBinding(k), axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(3...8)
        }
    }

    private func textBinding(_ k: String) -> Binding<String> {
        Binding(get: { run.forms[key]?[k].str ?? "" }, set: { var d = run.forms[key] ?? [:]; d[k] = JSON($0); run.forms[key] = d })
    }
}

struct SessionAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        let agent = item.agent ?? st.tasks[item.task ?? ""]?.agent ?? ""
        let w = run.who(agent)
        Card(tint: Tone.amber.color) {
            HStack(spacing: 8) {
                Pill("会话", .amber)
                Avatar(initials: w.initials, color: w.color, size: 20)
                Text(item.task.map { "\(w.name) 在 \($0) 中等你的消息" } ?? "\(w.name) 在等你的消息").font(.headline)
                Spacer()
                if let e = st.event(item.seq) { Text(L.clock(e.ts)).font(.caption).foregroundStyle(.secondary) }
            }
            if item.task != nil { Text("你接管了这个会话。Agent 这一轮已经结束，在你发消息、交还托管或结束任务之前，它会一直等待。").font(.callout).foregroundStyle(.secondary) }
            Button(item.task != nil ? "进入会话" : "进入对话") { run.page = item.task.map { .task($0) } ?? .agent(agent) }.buttonStyle(.borderedProminent)
        }
    }
}

struct BudgetAsk: View {
    let run: RunModel
    let st: RunState
    let item: InboxItem

    var body: some View {
        Card(tint: Tone.amber.color) {
            HStack {
                Pill("预算", .amber)
                Text("Run 已达到墙钟预算").font(.headline)
                Spacer()
                SeqButton(seq: item.seq, run: run)
            }
            Note(text: item.d["text"].str, tone: .red)
            HStack {
                Button("追加 60 分钟") { Task { await run.act(["type": "extendBudget", "minutes": 60]) } }.buttonStyle(.borderedProminent).disabled(!run.canAct)
                Button("取消这个 Run", role: .destructive) { cancelRun(run) }.disabled(!run.canAct)
            }
        }
    }
}

@MainActor
func cancelRun(_ run: RunModel) {
    if confirm("取消这个 Run？", "取消后会结束所有进行中的 Agent 会话。", ok: "取消 Run") { Task { await run.act(["type": "cancel"]) } }
}

struct CodeBlock: View {
    let text: String
    var maxHeight: CGFloat = 240
    var body: some View {
        ScrollView {
            Text(text)
                .font(.system(size: 11.5, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
        }
        .frame(maxHeight: maxHeight)
        .fixedSize(horizontal: false, vertical: true)
        .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// Wraps its children onto as many rows as needed.
struct FlowLayout: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, rowH: CGFloat = 0, maxX: CGFloat = 0
        for s in subviews {
            let size = s.sizeThatFits(ProposedViewSize(width: width, height: nil))
            if x > 0 && x + size.width > width { x = 0; y += rowH + spacing; rowH = 0 }
            x += size.width + spacing
            rowH = max(rowH, size.height)
            maxX = max(maxX, x - spacing)
        }
        return CGSize(width: width.isFinite ? width : maxX, height: y + rowH)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, rowH: CGFloat = 0
        for s in subviews {
            let size = s.sizeThatFits(ProposedViewSize(width: bounds.width, height: nil))
            if x > bounds.minX && x + size.width > bounds.maxX { x = bounds.minX; y += rowH + spacing; rowH = 0 }
            s.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowH = max(rowH, size.height)
        }
    }
}
