import SwiftUI

/// The global inbox: what every run is waiting on the person for. For each item there are three
/// choices: deal with it now (open the run), later (put it off, only when nothing waits on it),
/// or ignore it (stop showing it; the run stays as it is). Runs that are just working are folded
/// away by default, so the window is not a place to watch them work.
// @project-doc docs/architecture/20261031-frontend.md#inbox
struct InboxView: View {
    let store: AppStore

    var body: some View {
        let inbox = store.inbox
        let open = inbox.open
        let later = inbox.later
        let working = store.attention.filter { RunStatus($0["status"].str).isActive }
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                SectionHeader(title: L.text("待处理"), sub: L.text("所有工作区"), count: open.isEmpty ? nil : open.count)
                if open.isEmpty {
                    Card {
                        Text(L.text("清空了，没有待处理的事。")).font(.headline)
                        Text(working.isEmpty ? L.text("也没有在干活的任务。") : L.text("\(working.count) 个任务在干活，需要你拍板时会出现在这里。")).foregroundStyle(.secondary)
                    }
                } else {
                    Card(padding: 4) {
                        ForEach(open) { e in
                            row(e, now: inbox.now)
                            quick(e)
                            if e != open.last { Divider() }
                        }
                    }
                }

                if !later.isEmpty {
                    DisclosureGroup(isExpanded: Binding(get: { inbox.showLater }, set: { inbox.showLater = $0 })) {
                        Card(padding: 4) {
                            ForEach(later) { e in
                                HStack(alignment: .firstTextBaseline) {
                                    summary(e, now: inbox.now)
                                    Spacer()
                                    Text(L.text("到 \(L.dateTime(inbox.snoozed[e.key] ?? 0))")).font(.caption).foregroundStyle(.secondary)
                                    Button(L.text("现在处理")) { inbox.wake(e.key) }
                                }
                                .padding(.horizontal, 10).padding(.vertical, 8)
                                if e != later.last { Divider() }
                            }
                        }
                        .padding(.top, 6)
                    } label: {
                        Text(L.text("稍后处理（\(later.count)）")).font(.headline)
                    }
                }

                DisclosureGroup(isExpanded: Binding(get: { inbox.showWorking }, set: { inbox.showWorking = $0 })) {
                    Card(padding: 4) {
                        if working.isEmpty { Text(L.text("没有在干活的任务")).foregroundStyle(.secondary).padding(10) }
                        ForEach(working, id: \.self) { r in
                            let (label, tone) = RunStatus(r["status"].str).listLabel
                            Button { store.open(.run(r["id"].str)) } label: {
                                HStack {
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text(L.goalTitle(r["goal"].str)).lineLimit(1)
                                        Text(workspaceName(r)).font(.caption).foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    Pill(label, tone)
                                }
                                .padding(.horizontal, 10).padding(.vertical, 8)
                                .contentShape(Rectangle())
                                .hoverable(6)
                            }
                            .buttonStyle(.plain)
                            if r != working.last { Divider() }
                        }
                    }
                    .padding(.top, 6)
                } label: {
                    Text(L.text("干活中（\(working.count)）")).font(.headline)
                }
                .help(L.text("干活中的任务默认收起，需要你时会冒出来"))
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .navigationTitle(L.text("收件箱"))
    }

    /// What kind of thing it is, at a glance: red holds an agent up, the rest can wait.
    static func badge(_ e: InboxEntry) -> (String, Tone) {
        switch e.kind {
        case "delivered": return ("checkmark.seal.fill", .green)
        case "merge_blocked": return ("arrow.triangle.merge", .amber)
        case "held": return ("hand.raised.fill", .amber)
        case "interrupted": return ("pause.circle.fill", .gray)
        case "failed": return ("xmark.octagon.fill", .red)
        default: return (TimelineMark.openKinds[e.kind]?.0 ?? "exclamationmark.circle.fill", e.canWait ? .amber : .red)
        }
    }

    /// One entry: what it is, then its actions. The one way forward is the only worded button; the
    /// quieter choices, later and ignore, are glyphs in fixed places, so every row lines up.
    private func row(_ e: InboxEntry, now: Double) -> some View {
        let (symbol, tone) = Self.badge(e)
        return HStack(alignment: .center, spacing: 12) {
            Image(systemName: symbol)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(tone.color)
                .frame(width: 30, height: 30)
                .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                .accessibilityHidden(true)
            summary(e, now: now)
            Spacer(minLength: 12)
            HStack(spacing: 2) {
                if e.canWait {
                    later(e)
                } else {
                    // Nothing to put off: the place stays, so the glyphs below and above line up.
                    Color.clear.frame(width: Self.glyph, height: Self.glyph).accessibilityHidden(true)
                }
                IconButton(symbol: "eye.slash", label: L.text("忽略"), size: 13, color: .secondary,
                           help: e.canWait ? L.text("忽略：不再提醒；任务保持原样，在任务记录里还能找到") : L.text("忽略：不再提醒；对方会一直等着，在任务页还能处理")) {
                    store.inbox.ignore(e.key)
                }
            }
            // Both buttons take the place of 「去处理」, so the glyphs line up on every row.
            ZStack(alignment: .trailing) {
                Button(L.text("去处理")) {}.buttonStyle(.primary).hidden().accessibilityHidden(true)
                if e.actions.isEmpty {
                    Button(L.text("去处理")) { store.open(.run(e.run)) }.buttonStyle(.primary)
                } else {
                    // The quick answers below are the way forward; this only opens the run.
                    Button(L.text("详情")) { store.open(.run(e.run)) }.help(L.text("打开这个任务，看完整的请求"))
                }
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 12)
    }

    /// The side of an `IconButton` of size 13, which the later menu and the empty place match.
    private static let glyph: CGFloat = 24

    /// Put it off: a clock that opens the times to come back at.
    private func later(_ e: InboxEntry) -> some View {
        Menu {
            Button(L.text("1 小时后")) { store.inbox.snooze(e.key, until: Date().addingTimeInterval(3600)) }
            Button(L.text("4 小时后")) { store.inbox.snooze(e.key, until: Date().addingTimeInterval(4 * 3600)) }
            Button(L.text("明天上午 9 点")) { store.inbox.snooze(e.key, until: Self.tomorrowMorning()) }
        } label: {
            Image(systemName: "clock")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: Self.glyph, height: Self.glyph)
                .contentShape(Rectangle())
        }
        // A plain menu: the glyph keeps its gray, as the ignore glyph beside it.
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .hoverable(5)
        .help(L.text("稍后：先放一放，到点再提醒。没人在等，推迟不会卡住任务"))
        .accessibilityLabel(L.text("稍后"))
    }

    /// A plain choice is answered right here: the agent waiting on it goes on at once.
    @ViewBuilder private func quick(_ e: InboxEntry) -> some View {
        if !e.actions.isEmpty {
            let busy = store.inbox.answering.contains(e.key)
            HStack(spacing: 8) {
                ForEach(e.actions.indices, id: \.self) { i in
                    let a = e.actions[i]
                    let b = Button(a.label) { Task { await store.inbox.act(e, a, store: store) } }.disabled(busy).help(a.help)
                    if a.prominent { b.buttonStyle(.primary) } else { b.buttonStyle(.bordered) }
                }
                if busy { ProgressView().controlSize(.small) }
            }
            .padding(.leading, 52).padding(.trailing, 10).padding(.bottom, 12)
        }
    }

    private func summary(_ e: InboxEntry, now: Double) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(e.title).font(.body.weight(.medium)).lineLimit(2)
            if !e.text.isEmpty { Text(e.text).font(.callout).foregroundStyle(.secondary).lineLimit(2) }
            Text(verbatim: [e.workspace, L.goalTitle(e.goal), e.sub, e.ts > 0 ? L.ago(e.ts, now) : ""].filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
    }

    private func workspaceName(_ r: JSON) -> String {
        store.workspace(r["config"]["workspace"]["id"].str)?["name"].string ?? r["cwd"].str
    }

    private static func tomorrowMorning() -> Date {
        let cal = Calendar.current
        let tomorrow = cal.date(byAdding: .day, value: 1, to: Date()) ?? Date()
        return cal.date(bySettingHour: 9, minute: 0, second: 0, of: tomorrow) ?? tomorrow
    }
}
