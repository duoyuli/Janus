import SwiftUI

/// The global inbox: what every run is waiting on the person for. For each item there are three
/// choices: deal with it now (open the run), later (put it off, only when nothing waits on it),
/// or leave it (a run that stopped). Runs that are just working are folded away by default, so
/// the window is not a place to watch them work.
// @project-doc docs/architecture/frontend.md#inbox
struct InboxView: View {
    let store: AppStore

    var body: some View {
        let inbox = store.inbox
        let open = inbox.open
        let later = inbox.later
        let working = store.runs.filter { Inbox.active.contains($0["status"].str) }
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                SectionHeader(title: "需要你处理", sub: "所有工作区", count: open.isEmpty ? nil : open.count)
                if open.isEmpty {
                    Card {
                        Text("没有需要你处理的事。").font(.headline)
                        Text(working.isEmpty ? "也没有正在进行的 Run。" : "\(working.count) 个 Run 在进行，有需要你决定的事会出现在这里。").foregroundStyle(.secondary)
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
                                    Text("到 \(L.dateTime(inbox.snoozed[e.key] ?? 0))").font(.caption).foregroundStyle(.secondary)
                                    Button("现在处理") { inbox.wake(e.key) }
                                }
                                .padding(.horizontal, 10).padding(.vertical, 8)
                                if e != later.last { Divider() }
                            }
                        }
                        .padding(.top, 6)
                    } label: {
                        Text("稍后处理（\(later.count)）").font(.headline)
                    }
                }

                DisclosureGroup(isExpanded: Binding(get: { inbox.showWorking }, set: { inbox.showWorking = $0 })) {
                    Card(padding: 4) {
                        if working.isEmpty { Text("没有正在进行的 Run").foregroundStyle(.secondary).padding(10) }
                        ForEach(working, id: \.self) { r in
                            let (label, tone) = L.runListStatus[r["status"].str] ?? (r["status"].str, .gray)
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
                    Text("进行中（\(working.count)）").font(.headline)
                }
                .help("正在工作的 Run 默认收起：有需要你的地方，它会出现在上面")
            }
            .padding(24)
            .frame(maxWidth: 820, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .navigationTitle("收件箱")
    }

    /// What kind of thing it is, at a glance: red holds an agent up, the rest can wait.
    static func badge(_ e: InboxEntry) -> (String, Tone) {
        switch e.kind {
        case "delivered": return ("checkmark.seal.fill", .green)
        case "interrupted": return ("pause.circle.fill", .gray)
        case "failed": return ("xmark.octagon.fill", .red)
        default: return (TimelineMark.openKinds[e.kind]?.0 ?? "exclamationmark.circle.fill", e.canWait ? .amber : .red)
        }
    }

    private func row(_ e: InboxEntry, now: Double) -> some View {
        let (symbol, tone) = Self.badge(e)
        return HStack(alignment: .top, spacing: 12) {
            Image(systemName: symbol)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(tone.color)
                .frame(width: 30, height: 30)
                .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                .accessibilityHidden(true)
            summary(e, now: now)
            Spacer()
            if e.canWait {
                Menu("稍后") {
                    Button("1 小时后") { store.inbox.snooze(e.key, until: Date().addingTimeInterval(3600)) }
                    Button("4 小时后") { store.inbox.snooze(e.key, until: Date().addingTimeInterval(4 * 3600)) }
                    Button("明天上午 9 点") { store.inbox.snooze(e.key, until: Self.tomorrowMorning()) }
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("放到「稍后处理」，到时间再回到这里。没有人在等它，推迟不会卡住任务")
            }
            if e.canSettle {
                Button("标记已处理") { store.inbox.settle(e.key) }
                    .help("不再提醒这个 Run；它保持现在的状态，仍可以在任务记录中打开")
            }
            if e.actions.isEmpty {
                Button("去处理") { store.open(.run(e.run)) }.buttonStyle(.primary)
            } else {
                Button("详情") { store.open(.run(e.run)) }.help("打开这个 Run，看完整的请求")
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 12)
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
