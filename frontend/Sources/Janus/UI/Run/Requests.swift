import SwiftUI

/// Every request an agent puts in front of the person shares one card: who asks and where, what it
/// is about, then the agent's own options or a form, as the server words them (`ServedCard`).
// @project-doc docs/architecture/20261031-frontend.md#request_cards
struct RequestCard: View {
    let run: RunModel
    let st: RunState
    let card: ServedCard

    var body: some View {
        switch card.kind {
        case .question, .decision: QuestionAsk(run: run, st: st, card: card)
        case .approval: ApprovalAsk(run: run, st: st, card: card)
        case .link, .form: InputAsk(run: run, st: st, card: card)
        case .session: SessionAsk(run: run, st: st, card: card)
        }
    }
}

struct AskShell<Body: View, Controls: View>: View {
    let run: RunModel
    let card: ServedCard
    @ViewBuilder var content: Body
    @ViewBuilder var controls: Controls

    var body: some View {
        let w = run.who(card.agent)
        let tone = card.pill.1
        Card(tint: tone.color) {
            HStack(spacing: 8) {
                Pill(card.pill.0, tone)
                Avatar(initials: w.initials, color: w.color, size: 20, role: w.role)
                Text(card.title).font(.headline).lineLimit(3)
                Spacer()
                Text(L.clock(card.ts)).font(.caption).foregroundStyle(.secondary)
                SeqButton(seq: card.seq, run: run)
            }
            content
            KV(rows: card.facts)
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
                    ChoiceLabel(title: o, sub: o == recommend ? L.text("负责人建议") : "", subTint: true, on: picked == o)
                }
                .buttonStyle(.plain)
                .disabled(disabled)
                .accessibilityAddTraits(picked == o ? .isSelected : [])
            }
        }
    }
}

/// One option of a single or multiple choice: the selection shows as a checkmark and an outline
/// in the system accent color, not by tint alone.
struct ChoiceLabel: View {
    let title: String
    var sub = ""
    var subTint = false
    let on: Bool
    var multi = false
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 7) {
            Image(systemName: multi ? (on ? "checkmark.square.fill" : "square") : (on ? "checkmark.circle.fill" : "circle"))
                .foregroundStyle(on ? Color.accentColor : .secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).multilineTextAlignment(.leading)
                if !sub.isEmpty { Text(sub).font(subTint ? .caption2 : .caption).foregroundStyle(subTint ? Color.accent : .secondary) }
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(on ? Color.accentColor.opacity(0.12) : Color.secondary.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Color.accentColor : Color.line))
        .contentShape(Rectangle())
        .hoverable(8)
    }
}

/// A collaborator's question to the person, or a decision the lead handed to them
/// (`janus.ask_user`): pick an option, add a note, or for a decision with no options write the
/// answer.
struct QuestionAsk: View {
    let run: RunModel
    let st: RunState
    let card: ServedCard

    var body: some View {
        let decision = card.kind == .decision
        let options = card.options
        let empty = run.picks[card.key] == nil && (decision ? note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty : true)
        AskShell(run: run, card: card) {
            if !card.text.isEmpty { Text(card.text).font(.body).textSelection(.enabled) }
            if !card.reason.isEmpty { Text(verbatim: card.reason).font(.callout).foregroundStyle(.secondary) }
        } controls: {
            if !options.isEmpty {
                OptionButtons(options: options, recommend: card.recommend, picked: Binding(get: { run.picks[card.key] }, set: { run.picks[card.key] = $0 }), disabled: !run.canAct)
            }
            let placeholder = decision ? (options.isEmpty ? L.text("你的回答") : L.text("补充说明（可选）")) : L.text("补充说明（可选），会和回答一起发给 Agent")
            TextField(placeholder, text: Binding(get: { run.notes[card.key] ?? "" }, set: { run.notes[card.key] = $0 }), axis: .vertical)
                .textFieldStyle(.roundedBorder).lineLimit(2...6).disabled(!run.canAct)
            Button(decision ? L.text("提交") : L.text("提交回答")) { submit() }.buttonStyle(.primary).disabled(!run.canAct || empty)
                .help(empty ? (!decision ? L.text("先选择一个选项") : options.isEmpty ? L.text("先写下你的回答") : L.text("先选择一个选项，或写下你的回答")) : "")
            // Notes go under the buttons, so a narrow card does not squeeze them into a strip.
            if !card.footnote.isEmpty { Text(verbatim: card.footnote).font(.caption).foregroundStyle(.secondary) }
        }
    }

    private var note: String { run.notes[card.key] ?? "" }

    func submit() {
        var note = self.note.trimmingCharacters(in: .whitespacesAndNewlines)
        var answer = run.picks[card.key] ?? ""
        // A free-form decision (no options) is answered with the text itself.
        if card.kind == .decision && answer.isEmpty && !note.isEmpty { answer = note; note = "" }
        guard !answer.isEmpty else { return }
        let task = st.inbox.first { $0.seq == card.seq }?.task ?? ""
        Task { await run.act(.answer(key: card.key, task: task, answer: answer, note: note)) }
    }
}

/// ACP permission request: the agent's own options, each answered by its optionId.
struct ApprovalAsk: View {
    let run: RunModel
    let st: RunState
    let card: ServedCard

    var body: some View {
        let diffs = st.diffs[card.seq] ?? []
        AskShell(run: run, card: card) {
            if !card.command.isEmpty { CodeBlock(text: card.command) }
            ForEach(diffs.indices, id: \.self) { i in
                InlineDiff(path: L.inWorkspace(diffs[i].path, st.run.cwd), lines: diffs[i].lines)
            }
        } controls: {
            HStack {
                ForEach(card.actions.indices, id: \.self) { i in
                    let o = card.actions[i]
                    let button = Button(o.label) {
                        Task { await run.act(.served(o.body)) }
                    }
                    .disabled(!run.canAct)
                    .help(o.help)
                    if o.prominent { button.buttonStyle(.primary) } else { button.buttonStyle(.bordered) }
                }
            }
            if !card.impact.isEmpty { Text(card.impact).font(.caption).foregroundStyle(.secondary) }
            if !card.always.isEmpty { Text(card.always).font(.caption).foregroundStyle(.secondary) }
        }
    }
}

/// ACP elicitation: a form, or a link to finish elsewhere.
struct InputAsk: View {
    let run: RunModel
    let st: RunState
    let card: ServedCard

    var body: some View {
        if card.kind == .link {
            AskShell(run: run, card: card) {
                Text(card.text)
                if !card.url.isEmpty { Link(card.url, destination: URL(string: card.url) ?? URL(string: "about:blank")!).font(.system(.callout, design: .monospaced)) }
            } controls: {
                HStack {
                    Button(L.text("拒绝")) { send("decline") }.disabled(!run.canAct)
                    Button(L.text("我已完成")) { send("accept") }.buttonStyle(.primary).disabled(!run.canAct)
                }
            }
        } else {
            AskShell(run: run, card: card) {
                Text(card.text)
                SchemaForm(run: run, key: card.key, fields: card.fields)
            } controls: {
                let missing = missingRequired
                HStack {
                    Button(L.text("取消请求"), role: .destructive) { send("cancel") }.disabled(!run.canAct).help(L.text("告诉 Agent 取消这次请求"))
                    Button(L.text("跳过")) { send("decline") }.disabled(!run.canAct).help(L.text("跳过，让 Agent 接着干"))
                    Button(L.text("提交")) { send("accept") }.buttonStyle(.primary).disabled(!run.canAct || !missing.isEmpty)
                }
                Text(missing.isEmpty ? card.footnote : L.text("还有必填项：\(missing.joined(separator: L.text("、")))"))
                    .font(.caption).foregroundStyle(missing.isEmpty ? Color.secondary : Tone.amber.color)
            }
        }
    }

    /// What the person filled in, without the fields left empty.
    private var content: [String: JSON] {
        (run.forms[card.key]?.object ?? [:]).filter { _, v in
            if v.isNull { return false }
            if v.string == "" { return false }
            if case .array(let a) = v, a.isEmpty { return false }
            return true
        }
    }

    /// Titles of required fields still empty.
    private var missingRequired: [String] {
        card.fields.filter { $0.required && content[$0.key] == nil }.map(\.title)
    }

    func send(_ action: String) {
        let content = content
        if action == "accept" && !missingRequired.isEmpty { return }
        let item = st.inbox.first { $0.seq == card.seq }
        Task { await run.act(.input(key: card.key, task: item?.task ?? "", agent: item?.agent ?? "", action: action, content: content)) }
    }
}

/// A form's fields as the server read them from its JSON Schema (`ServedCard.Field`).
struct SchemaForm: View {
    let run: RunModel
    let key: String
    let fields: [ServedCard.Field]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(fields, id: \.key) { f in
                VStack(alignment: .leading, spacing: 5) {
                    Text(f.title).font(.callout.weight(.semibold)) + Text(f.required ? " *" : "").foregroundStyle(.secondary)
                    if !f.description.isEmpty { Text(f.description).font(.caption).foregroundStyle(.secondary) }
                    field(f)
                    if let c = f.custom {
                        TextField(c.placeholder, text: textBinding(c.key)).textFieldStyle(.roundedBorder)
                    }
                }
            }
        }
        .disabled(!run.canAct)
    }

    @ViewBuilder private func field(_ f: ServedCard.Field) -> some View {
        let k = f.key
        let cur = run.forms[key]?[k] ?? .null
        switch f.kind {
        case .choice, .multi:
            let multi = f.kind == .multi
            FlowLayout(spacing: 8) {
                ForEach(f.choices, id: \.self) { o in
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
                        ChoiceLabel(title: o.label, sub: o.desc, on: on, multi: multi)
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(on ? .isSelected : [])
                }
            }
        case .bool:
            Toggle(f.title, isOn: Binding(get: { run.forms[key]?[k].bool ?? false }, set: { var d = run.forms[key] ?? [:]; d[k] = JSON($0); run.forms[key] = d })).labelsHidden().toggleStyle(.switch)
        case .number:
            TextField(f.title, text: Binding(get: { run.forms[key]?[k].str ?? "" }, set: { v in var d = run.forms[key] ?? [:]; d[k] = Double(v).map { .number($0) } ?? .null; run.forms[key] = d }))
                .textFieldStyle(.roundedBorder).labelsHidden().frame(maxWidth: 200)
        case .line:
            TextField(f.title, text: textBinding(k)).textFieldStyle(.roundedBorder).labelsHidden()
        case .text:
            TextField(f.title, text: textBinding(k), axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(3...8).labelsHidden()
        }
    }

    private func textBinding(_ k: String) -> Binding<String> {
        Binding(get: { run.forms[key]?[k].str ?? "" }, set: { var d = run.forms[key] ?? [:]; d[k] = JSON($0); run.forms[key] = d })
    }
}

struct SessionAsk: View {
    let run: RunModel
    let st: RunState
    let card: ServedCard

    var body: some View {
        let w = run.who(card.agent)
        Card(tint: Tone.amber.color) {
            HStack(spacing: 8) {
                Pill(card.pill.0, .amber)
                Avatar(initials: w.initials, color: w.color, size: 20, role: w.role)
                Text(card.title).font(.headline)
                Spacer()
                Text(L.clock(card.ts)).font(.caption).foregroundStyle(.secondary)
            }
            if !card.text.isEmpty { Text(card.text).font(.callout).foregroundStyle(.secondary) }
            switch card.session {
            case .task(let t)?: Button(L.text("打开会话")) { run.show(.task(t)) }.buttonStyle(.primary)
            case .agent(let a)?: Button(L.text("打开对话")) { run.show(.agent(a)) }.buttonStyle(.primary)
            case nil: EmptyView()
            }
        }
    }
}

@MainActor
func cancelRun(_ run: RunModel) {
    if confirm(L.text("停止这个任务？"), L.text("所有会话都会结束，已有改动保留。停了就不能再继续。"), ok: L.text("停止")) { Task { await run.act(.cancel) } }
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
