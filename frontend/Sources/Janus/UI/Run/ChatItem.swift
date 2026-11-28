import SwiftUI

/// One row of a conversation: a message, a note from Janus, a request or a tool call, as the
/// server words it (`RunState.lines`).
struct ChatItem: View {
    let run: RunModel
    let st: RunState
    let e: Event
    /// Further pieces of the same agent message, streamed after `e`.
    var more: [Event] = []
    /// A waiting request is answered above the composer: here it is one line.
    var docked = false

    var body: some View {
        let d = e.data
        switch st.lines[e.seq] {
        case nil:
            if e.type == .agentMessage { TextRow(events: [e] + more, reply: true) }
            else if e.type == .agentThought { ThoughtLine(text: d["text"].str) }
        case .divider(let text, let sub, let git, let images)?:
            divider(text)
            if let sub { Text(verbatim: sub).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
            if let git { Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
            StoredImages(run: run, paths: images, size: 56)
        case .attempt(let title, let git, let brief)?:
            VStack(alignment: .leading, spacing: 4) {
                Divider()
                Text(verbatim: title).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                if let git { Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                DisclosureGroup(brief) { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
        case .note(let text, let tone)?:
            note(text, tone)
        case .prompt(let label, let hint)?:
            // One line in the flow, as the rest of Janus's own notes: what it sent, a click from its text.
            DisclosureGroup {
                CodeBlock(text: d["prompt"].str, maxHeight: 360).padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: "→ \(label)").font(.callout.weight(.medium)).foregroundStyle(.secondary)
                    Text(verbatim: hint).font(.caption).foregroundStyle(.tertiary).lineLimit(1)
                }
                .help(hint)
            }
        case .bubbles(let items)?:
            ForEach(Array(items.enumerated()), id: \.offset) { _, b in userBubble(b) }
        // One line in the conversation; each criterion is a click away (and on the task page).
        case .result(let title, let pills, let evidence, let risks)?:
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 6) {
                    // The method is shown as text here too; the conversation only shows it, the task
                    // page groups it and the delivery repeats it per task.
                    CheckLines(lines: st.checks[e.seq] ?? [], dense: true)
                    if let evidence { Text(verbatim: evidence).font(.caption).foregroundStyle(.secondary) }
                    if let risks { Text(verbatim: risks).font(.caption).foregroundStyle(Tone.amber.color) }
                }
                .padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: title).font(.callout.weight(.semibold)).lineLimit(2)
                    ForEach(Array(pills.enumerated()), id: \.offset) { _, p in Pill(p.0, p.1) }
                }
            }
            .padding(8)
            .background(Tone.green.color.opacity(0.07), in: RoundedRectangle(cornerRadius: 8))
        // A request still waiting is answered right here, with the same card as the inbox.
        case .request(_, let open, let text, let tone, let docked)?:
            if open, let card = st.cards.first(where: { $0.seq == e.seq }) {
                if self.docked { note(docked, .amber) } else { RequestCard(run: run, st: st, card: card) }
            } else {
                note(text, tone)
            }
        case .tool?:
            ToolRow(run: run, st: st, e: e)
        }
    }

    private func divider(_ t: String) -> some View {
        HStack { VStack { Divider() }; Text(t).font(.caption).foregroundStyle(.secondary).lineLimit(1); VStack { Divider() } }
    }

    private func note(_ t: String, _ tone: Tone?) -> some View {
        Text(t).font(.callout).foregroundStyle(tone?.color ?? .secondary).textSelection(.enabled)
            .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
    }

    private func userBubble(_ b: ServedLine.Bubble) -> some View {
        HStack {
            Spacer(minLength: 60)
            VStack(alignment: .trailing, spacing: 4) {
                if b.insert { Text("↪ " + b.via).font(.caption.weight(.semibold)).foregroundStyle(Color.accent) }
                StoredImages(run: run, paths: b.images)
                if !b.text.isEmpty { Text(b.text).textSelection(.enabled) }
                if !b.insert && !b.via.isEmpty { Text(b.via).font(.caption).foregroundStyle(.secondary) }
            }
            .padding(10)
            .background(b.insert ? Color.accent.opacity(0.06) : Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(b.insert ? Color.accent.opacity(0.4) : .clear, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        }
    }
}

/// One row per tool call, readable at a glance as the server words it, with the full input and
/// output a click away. A running call shows its newest frame (`RunModel.preview`).
struct ToolRow: View {
    let run: RunModel
    let st: RunState
    let e: Event

    var body: some View {
        let d = e.data
        guard case .tool(let t)? = st.lines[e.seq] else { return AnyView(EmptyView()) }
        // A call still running shows its newest frame: a title the agent filled in later, its output so far.
        let live = t.running ? run.preview[e.task.map { "task:" + $0 } ?? "agent:" + e.actor]?.tools[d["callId"].str] : nil
        let input = live.map { $0.input.isEmpty ? $0.title : $0.input } ?? (d["input"].truthy ? d["input"].str : e.text)
        let (verb, what, mono) = live.map { ($0.verb, $0.what, $0.mono) } ?? (t.verb, t.what, t.mono)
        return AnyView(VStack(alignment: .leading, spacing: 4) {
            // The whole input and output only once opened: a call's output runs to many kilobytes,
            // and working it out for every call on every update of a busy run is the page's cost.
            DisclosureGroup {
                ToolDetail(st: st, e: e, board: t.board, input: input, output: live?.output)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        if t.board {
                            Text(t.tag).font(.system(size: 11, design: .monospaced)).padding(.horizontal, 4).background(Color.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 3))
                            Text(t.what).font(.callout).lineLimit(1).truncationMode(.tail)
                        } else if t.running {
                            Shimmer(text: [verb, what].filter { !$0.isEmpty }.joined(separator: " "))
                        } else {
                            if !verb.isEmpty { Text(verb).font(.callout).foregroundStyle(t.failed ? Tone.red.color : .secondary) }
                            Text(verbatim: what).font(mono ? .system(.callout, design: .monospaced) : .callout)
                                .foregroundStyle(t.failed ? Tone.red.color : .primary).lineLimit(1).truncationMode(.tail)
                        }
                        if t.waiting { Pill(L.text("等待返回"), .blue) }
                        if t.failed { Pill(L.text("失败"), .red) }
                        if !t.files.isEmpty { Text(verbatim: t.files).font(.caption).foregroundStyle(.secondary) }
                    }
                    .help(input)
                    if !t.summary.isEmpty { Text(t.summary).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
            }
            // A running command's output so far: its last lines, as its own terminal ends.
            if t.running, case let output = L.inWorkspace(live?.output ?? d["output"].str, st.run.cwd), !output.isEmpty {
                let (tail, cut) = Self.tail(output)
                VStack(alignment: .leading, spacing: 2) {
                    if cut { Text(L.text("更早的输出没有显示")).font(.caption2).foregroundStyle(.tertiary) }
                    Text(verbatim: tail).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).lineLimit(16)
                }
                .padding(8)
                .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
                .background(Color.soft, in: RoundedRectangle(cornerRadius: 6))
                .textSelection(.enabled)
            }
        })
    }

    /// The last `lines` lines of a tool's output, and whether earlier ones were left out.
    static func tail(_ output: String, lines: Int = 16) -> (String, Bool) {
        let all = output.components(separatedBy: "\n")
        guard all.count > lines else { return (output, false) }
        return (all.suffix(lines).joined(separator: "\n"), true)
    }
}

/// An opened tool call: its whole input (a board call's arguments as JSON), its output, and the
/// changes it made.
struct ToolDetail: View {
    let st: RunState
    let e: Event
    let board: Bool
    let input: String
    /// The running call's output so far; the event's own otherwise.
    let output: String?

    var body: some View {
        let d = e.data
        let output = L.inWorkspace(output ?? d["output"].str, st.run.cwd)
        let full = board ? d["args"].or(d["input"]).pretty() : input
        CodeBlock(text: full + (output.isEmpty ? "" : "\n\n→ \(output)"), maxHeight: 300)
        ForEach(Array((st.diffs[e.seq] ?? []).enumerated()), id: \.offset) { _, x in
            InlineDiff(path: L.tilde(x.path), lines: x.lines)
        }
    }
}
