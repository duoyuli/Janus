import SwiftUI

// The delivery on the run page, reviewed one round at a time. While it waits for the person the
// card is the whole review: the lead's conclusion, what needs looking at, what changed, the action,
// then the subtasks and the rest folded. At any other stage it folds to one line that says who it
// waits on now (docs/architecture/20261031-frontend.md#review).

struct ResultCard: View {
    let run: RunModel
    let st: RunState
    /// The content width; under 838 the acceptance rows wrap their numbers (W1 F9).
    var width: CGFloat

    var body: some View {
        if let r = st.result, let stage = st.review?.stage {
            let d = Delivery(r)
            SectionHeader(title: L.text("交付"))
            if stage.reviews {
                Card(padding: 18, tint: stage == .awaiting ? Tone.green.color : Tone.amber.color) {
                    ReviewBody(run: run, st: st, d: d, stage: stage, width: width)
                }
            } else {
                Card(padding: 14) {
                    ReviewLine(run: run, st: st, d: d, stage: stage)
                    if run.reviewOpen { Divider(); ReviewBody(run: run, st: st, d: d, stage: stage, width: width) }
                }
            }
        }
    }
}

/// The delivery folded to one line, at every stage but the two that wait for the person's review.
struct ReviewLine: View {
    let run: RunModel
    let st: RunState
    let d: Delivery
    let stage: ReviewStage

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                RoundTag(n: d.n)
                Text(text).font(.callout).foregroundStyle(stage == .held ? Tone.amber.color : Color.primary)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 8)
                Button(run.reviewOpen ? L.text("收起") : L.text("看这次交付")) { run.reviewOpen.toggle() }.buttonStyle(.link)
            }
            if stage == .held && run.canAct {
                HStack(spacing: 8) {
                    Button(L.text("让它接着做")) { Task { await run.act(.release(.agent("lead"))) } }
                        .buttonStyle(.primary)
                        .help(L.text("交还负责人，它接着做你交代的事"))
                    Button(L.text("按现在的样子交付")) { Task { await run.act(.finish(.agent("lead"))) } }
                        .help(L.text("这一轮做到哪里就交付到哪里，交付时合并"))
                }
            }
        }
    }

    private var text: String {
        switch stage {
        case .resolving: return L.text("合并到你的分支时冲突了，你的目录和分支都没动。负责人在任务的副本里解决，解决后再次交付。")
        case .leadWorking: return L.text("负责人在处理你的消息，处理完再验收。")
        case .teamWorking: return L.text("负责人派了新的子任务，做完后再次交付。")
        case .asking: return L.text("有事等你回答，见上面的「待处理」。回答后负责人接着做。")
        case .held: return L.text("你打断了负责人这一轮，它在等你。这一轮改的东西还没有交付。")
        case .accepted:
            let when = d.result.accepted.map { L.clock($0) } ?? ""
            return L.text("你已于 \(when) 验收了这次交付。") + (d.mergeNote.isEmpty ? "" : " " + d.mergeNote)
        case .stopped: return L.text("交付之后任务停下了，见上面的运行卡片。")
        case .awaiting, .blocked: return ""
        }
    }
}

/// 「第 2 次交付」, shown once there has been more than one.
struct RoundTag: View {
    let n: Int
    var body: some View {
        Text(verbatim: L.text("第 \(n) 次交付")).font(.caption.weight(.medium)).foregroundStyle(.secondary)
            .padding(.horizontal, 6).padding(.vertical, 1)
            .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
            .fixedSize()
    }
}

/// The review of one delivery, from the conclusion down; the actions only while it waits for the
/// person.
struct ReviewBody: View {
    let run: RunModel
    let st: RunState
    let d: Delivery
    let stage: ReviewStage
    var width: CGFloat

    var body: some View {
        let acceptance = d.acceptance
        let signals = st.review?.signals ?? []
        VStack(alignment: .leading, spacing: 14) {
            header
            requests
            conclusion
            facts(acceptance)
            attention(signals)
            changes
            if run.offersReview(stage) { actions(signals) }
            if !acceptance.isEmpty { tasks(acceptance) }
            folds
            if !d.d["briefSuggestions"].strings.isEmpty { BriefSuggestions(run: run, lines: d.d["briefSuggestions"].strings) }
            Button(L.text("在审计日志中逐条核对")) { run.show(.audit) }.buttonStyle(.link)
        }
    }

    /// The round and the event; the stage itself is the run card's status, right above.
    private var header: some View {
        HStack(spacing: 8) {
            if st.deliveries.count > 1 { RoundTag(n: d.n) }
            SeqButton(seq: d.result.seq, run: run)
            Spacer()
        }
    }

    /// What the person asked for this round, quoted: the delivery answers it.
    @ViewBuilder private var requests: some View {
        let asked = st.review?.requests ?? []
        if !asked.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text(L.text("你在交付后的要求")).font(.caption).foregroundStyle(.secondary)
                ForEach(asked, id: \.seq) { m in
                    Text(m.text).font(.callout).lineLimit(4).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.leading, 10)
            .overlay(alignment: .leading) { Rectangle().fill(Color.accent).frame(width: 3) }
        }
    }

    /// The lead's conclusion; a lead that wrote none has its report here instead, previewed.
    @ViewBuilder private var conclusion: some View {
        if !d.headline.isEmpty {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(L.text("负责人")).font(.caption).foregroundStyle(.secondary)
                    .padding(.horizontal, 5).overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color.line))
                Text(MD.inline(d.headline)).font(.title3).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            }
        } else if !d.summary.isEmpty {
            let long = d.summary.count > 360
            if long && !run.resultOpen {
                Text(MD.inline(d.summary)).lineLimit(8).textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button(L.text("展开全文")) { run.resultOpen.toggle() }.buttonStyle(.link)
            } else {
                MarkdownText(text: d.summary)
                if long { Button(L.text("收起")) { run.resultOpen.toggle() }.buttonStyle(.link) }
            }
        }
    }

    /// One line of numbers for the round: subtasks done, criteria met, how the evidence was got.
    @ViewBuilder private func facts(_ acceptance: [DeliveryAcceptance]) -> some View {
        let line = ReviewBody.factsLine(acceptance, changed: d.changed.count)
        Text(verbatim: line).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// 「子任务 3/3 完成 · 验收标准 12/12 · 运行 5 · 检查 7 · 改了 2 个文件」
    static func factsLine(_ acceptance: [DeliveryAcceptance], changed: Int) -> String {
        var parts: [String] = []
        if acceptance.isEmpty {
            parts.append(L.text("没有子任务，负责人自己完成"))
        } else {
            let done = acceptance.filter { $0.status == .succeeded }.count
            parts.append(L.text("子任务 \(done)/\(acceptance.count) 完成"))
            let criteria = acceptance.map(\.summary.criteria).reduce(0, +), passed = acceptance.map(\.summary.passed).reduce(0, +)
            parts.append(L.text("验收标准 \(passed)/\(criteria) 达到"))
            var all = EvidenceStrength(EvidenceSummary(accept: [], checks: []))
            for a in acceptance {
                let s = EvidenceStrength(a.summary)
                all.ran += s.ran; all.read += s.read; all.said += s.said
            }
            if all.total > 0 { parts.append(all.text) }
        }
        parts.append(changed == 0 ? L.text("没有改动文件") : L.text("改了 \(changed) 个文件"))
        return parts.joined(separator: " · ")
    }

    /// What the person should look at before accepting; says so when there is nothing.
    @ViewBuilder private func attention(_ signals: [ReviewSignal]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text(L.text("需要你看的")).font(.callout.weight(.semibold))
                if !signals.isEmpty { Text(verbatim: "\(signals.count)").font(.callout.weight(.semibold)).foregroundStyle(Tone.amber.color) }
            }
            if signals.isEmpty {
                Text(L.text("没有。负责人没留下要你决定的事，Janus 也没查出问题。")).font(.callout).foregroundStyle(.secondary)
            }
            ForEach(signals) { s in
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(s.source == .lead ? "?" : "⚠").foregroundStyle(s.source == .lead ? Color.accent : Tone.amber.color).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(MD.inline(s.text)).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                        if !s.detail.isEmpty {
                            Text(s.detail).font(.caption).foregroundStyle(.secondary).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    Spacer(minLength: 4)
                    Text(s.source == .lead ? L.text("负责人") : "Janus").font(.caption2)
                        .foregroundStyle(s.source == .lead ? Color.accent : Tone.violet.color)
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background((s.source == .lead ? Color.accent : Tone.violet.color).opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
                        .help(s.source == .lead ? L.text("负责人留给你的问题") : L.text("Janus 从记录里查出来的"))
                    if let t = s.task, st.tasks[t] != nil {
                        Button(L.text("查看")) { run.show(.task(t)) }.buttonStyle(.link).font(.caption)
                    }
                }
                .accessibilityElement(children: .combine)
            }
        }
    }

    /// Where the result is, and what changed: the files, the diff to review, the other deliverables.
    @ViewBuilder private var changes: some View {
        let diff = d.deliverables.compactMap { a in st.artifacts.first { $0.path == a["path"].str && $0.isDiff } }.first
        let others = d.deliverables.filter { a in !st.artifacts.contains { $0.path == a["path"].str && $0.isDiff } }
        VStack(alignment: .leading, spacing: 6) {
            Text(st.deliveries.count > 1 ? L.text("这一轮改了什么") : L.text("改了什么")).font(.callout.weight(.semibold))
            if !d.mergeNote.isEmpty {
                Text(d.mergeNote).font(.callout)
                    .foregroundStyle(d.merge?.waitsOnPerson == true ? Tone.amber.color : Color.secondary)
                    .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            }
            if !d.changed.isEmpty {
                HStack(spacing: 6) {
                    ForEach(d.changed.prefix(5), id: \.self) { f in
                        Text(f).font(.system(.caption, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
                    }
                    if d.changed.count > 5 { Text(verbatim: L.text("还有 \(d.changed.count - 5) 个")).font(.caption).foregroundStyle(.secondary) }
                    if let diff { Button(L.text("审阅改动")) { DiffWindow.open(run: run, artifact: diff) }.buttonStyle(.link).font(.caption) }
                }
                .help(d.changed.joined(separator: "\n"))
            }
            ForEach(others.indices, id: \.self) { i in
                let a = others[i]
                HStack(spacing: 6) {
                    Text(a["name"].str).font(.callout.weight(.medium))
                    if a["desc"].truthy { Text(a["desc"].str).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                    Button(L.text("查看")) { ArtifactWindow.open(run: run, path: a["path"].str, title: a["name"].str) }.buttonStyle(.link)
                    SeqButton(seq: a["seq"].int, run: run)
                }
            }
        }
    }

    /// Accept, or tell the lead what to change. Waiting on the person's side of a merge, the
    /// action says what it really does.
    @ViewBuilder private func actions(_ signals: [ReviewSignal]) -> some View {
        HStack(spacing: 10) {
            switch stage {
            case .blocked(conflict: true):
                Button(L.text("让负责人再解决")) { Task { await run.act(.accept) } }.buttonStyle(.primary)
                    .help(L.text("把冲突交给负责人，它在任务的副本里解决后再次交付"))
            case .blocked(conflict: false):
                Button(L.text("处理好了，合并并验收")) { Task { await run.act(.accept) } }.buttonStyle(.primary)
                    .help(L.text("你那边挡路的东西处理掉之后，验收时把结果合并到你的分支"))
            default:
                Button(signals.isEmpty ? L.text("验收") : L.text("看过了，验收")) { Task { await run.act(.accept) } }.buttonStyle(.primary)
            }
            Button(L.text("让负责人改…")) { run.leadFocus += 1 }
                .help(L.text("在下面负责人的对话里写下要改什么；它改完会再交付一次"))
            Spacer()
            Text(L.text("下面是细节，不用看完也能验收")).font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 6)
    }

    private func tasks(_ acceptance: [DeliveryAcceptance]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(st.deliveries.count > 1 ? L.text("这一轮的子任务") : L.text("子任务")).font(.callout.weight(.semibold))
            ForEach(acceptance) { record in DeliveryTaskView(run: run, record: record, width: width) }
        }
    }

    /// The lead's whole report, the other evidence and the deliveries before this one, folded.
    @ViewBuilder private var folds: some View {
        if !d.headline.isEmpty && !d.summary.isEmpty {
            DisclosureGroup(L.text("负责人的完整报告")) { MarkdownText(text: d.summary).padding(.top, 4) }
        }
        if !d.evidence.isEmpty {
            DisclosureGroup(L.text("其他证据 \(d.evidence.count) 条")) {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(d.evidence.indices, id: \.self) { i in
                        HStack(alignment: .firstTextBaseline) { tagged(d.evidence[i]["text"].str); SeqButton(seq: d.evidence[i]["seq"].int, run: run) }
                    }
                }
                .padding(.top, 4)
            }
        }
        let earlier = st.deliveries.filter { $0.seq < d.result.seq }
        if !earlier.isEmpty {
            DisclosureGroup(L.text("之前的交付 \(earlier.count) 次")) {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(earlier.reversed(), id: \.seq) { r in
                        let e = Delivery(r)
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            RoundTag(n: e.n)
                            Text(MD.inline(ReviewBody.firstLine(e))).font(.callout).lineLimit(2)
                            Spacer(minLength: 4)
                            if r.accepted != nil { Text(L.text("已验收")).font(.caption).foregroundStyle(Tone.green.color) }
                            SeqButton(seq: r.seq, run: run)
                        }
                    }
                }
                .padding(.top, 4)
            }
        }
    }

    /// A delivery in one line: its conclusion, or its report's first line.
    static func firstLine(_ d: Delivery) -> String {
        let text = d.headline.isEmpty ? d.summary : d.headline
        return text.split(separator: "\n").first.map(String.init) ?? ""
    }

    private static let taskId = regex(#"^[A-Za-z][\w-]*$"#)

    /// "T1：text" from the orchestrator: the task id as a tag.
    private func tagged(_ x: String) -> some View {
        let parts = x.split(separator: L.text("："), maxSplits: 1).map(String.init)
        let isTag = parts.count == 2 && Self.taskId.found(in: parts[0])
        return HStack(alignment: .firstTextBaseline, spacing: 6) {
            if isTag { TaskTag(id: parts[0]) }
            Text(MD.inline(isTag ? parts[1] : x)).font(.callout).textSelection(.enabled)
        }
    }
}
