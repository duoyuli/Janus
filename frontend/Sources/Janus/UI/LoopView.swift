import SwiftUI

/// Workspace Loops in the app: the section on a workspace's home, the form to make or change one,
/// and its page (its session, what it dispatched, its notes).
// @project-doc docs/architecture/20261031-frontend.md#loops

/// What the app says about a Loop: its schedule and what it is doing now.
enum LoopText {
    static func schedule(_ l: JSON) -> String {
        guard let cron = l["schedule"]["cron"].string else { return L.text("只在你叫它时") }
        return LoopSchedule(cron: cron).words
    }

    /// When it wakes next, to the minute: today, tomorrow or the day.
    static func when(_ at: Double) -> String { L.dateTime(at) }

    /// The time it asked for itself (`next_wake`) and why, when that is what wakes it next.
    static func asked(_ l: JSON) -> String? {
        let s = l["status"]
        guard s["phase"].str == "sleeping", s["nextWakeReason"].str == "self", let at = s["nextWakeAt"].double else { return nil }
        let why = s["nextWakeWhy"].str
        return why.isEmpty ? L.text("它约了 \(when(at)) 再看一次") : L.text("它约了 \(when(at)) 再看一次：\(why)")
    }

    /// How many runs it dispatched, from the server's counts by status.
    static func dispatchedCount(_ l: JSON) -> Int { l["status"]["dispatched"].object.values.reduce(0) { $0 + ($1.int ?? 0) } }

    /// The order of a workspace's Loops: one that failed first, then those at work, then the
    /// sleeping ones by when they wake next (those without a time after them), the paused last.
    static func byAttention(_ loops: [JSON]) -> [JSON] {
        func rank(_ l: JSON) -> Int {
            switch l["status"]["phase"].str {
            case "failed": return 0
            case "awake": return 1
            case "paused": return 3
            default: return 2
            }
        }
        return loops.enumerated().sorted { a, b in
            let (ra, rb) = (rank(a.element), rank(b.element))
            if ra != rb { return ra < rb }
            let (wa, wb) = (a.element["status"]["nextWakeAt"].double ?? .infinity, b.element["status"]["nextWakeAt"].double ?? .infinity)
            return wa != wb ? wa < wb : a.offset < b.offset
        }.map(\.element)
    }

    /// The phase as a pill: sleeping (with when it wakes, unless `short`), awake, paused, failed.
    static func phase(_ l: JSON, short: Bool = false) -> (String, Tone) {
        let s = l["status"]
        switch s["phase"].str {
        case "awake": return (L.text("干活中"), .blue)
        case "paused": return (L.text("已暂停"), .gray)
        case "failed": return (L.text("出错了"), .red)
        default:
            if !short, let at = s["nextWakeAt"].double { return (L.text("摸鱼中 · \(when(at)) 开工"), .gray) }
            return (L.text("摸鱼中"), .gray)
        }
    }
}

/// The Loops of one workspace, the right-hand column of its page (under what waits on the person
/// when the window is narrow). One compact row each, the ones to look at first on top; a few at a
/// time, the rest a click away.
// @project-doc docs/architecture/20261031-frontend.md#loops
struct LoopSection: View {
    let store: AppStore
    let ws: JSON
    @Local private var all = false
    static let shown = 4

    var body: some View {
        let loops = LoopText.byAttention(store.loops(of: ws["id"].str))
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(L.text("值班")).font(.headline)
                Spacer()
                IconButton(symbol: "plus", label: L.text("新建值班")) { store.editingLoop = AppStore.blankLoop(ws["id"].str) }
            }
            // What a Loop is, until there is one to show it.
            if loops.isEmpty {
                Text(L.text("按时间表开工：看一眼、做判断，要改的事派成任务")).font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Card(padding: 4) {
                    let rows = all ? loops : Array(loops.prefix(Self.shown))
                    ForEach(rows, id: \.self) { l in
                        row(l)
                        if l != rows.last { Divider() }
                    }
                    if loops.count > Self.shown {
                        Divider()
                        Button(all ? L.text("收起") : L.text("展开全部（\(loops.count)）")) { all.toggle() }
                            .buttonStyle(.link).font(.caption)
                            .padding(.horizontal, 10).padding(.vertical, 6)
                    }
                }
            }
        }
    }

    /// Name and state, then the schedule (with when it wakes next) and the first line of what it
    /// said last; one line each. The column is narrow, so the pill keeps to the state alone.
    private func row(_ l: JSON) -> some View {
        let (label, tone) = LoopText.phase(l, short: true)
        let wakes = l["status"]["phase"].str == "sleeping" ? l["status"]["nextWakeAt"].double.map { L.text("\(LoopText.when($0)) 开工") } : nil
        let schedule = [LoopText.schedule(l), wakes].compactMap { $0 }.joined(separator: " · ")
        let last = l["status"]["lastSummary"].str
        return Button { store.open(.run(l["run"].str)) } label: {
            HStack(alignment: .top, spacing: 8) {
                LoopBadge(size: 20)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(l["title"].str).lineLimit(1)
                        Spacer(minLength: 4)
                        Pill(label, tone)
                    }
                    Text(verbatim: schedule).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    if !last.isEmpty {
                        Text(L.text("上次：\(last)")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
            }
            .padding(.horizontal, 10).padding(.vertical, 8)
            .contentShape(Rectangle())
            .hoverable(6)
        }
        .buttonStyle(.plain)
        .help([l["title"].str, schedule, label, last.isEmpty ? "" : L.text("上次：\(last)")].filter { !$0.isEmpty }.joined(separator: "\n"))
    }
}

/// Make a Loop, or change one: what it does each time it wakes, and when it wakes: on a schedule
/// (a common shape or a cron expression, with the next times the server reads from it) or only when
/// called. It has no name to type: its agent titles its session the first time it wakes.
// @project-doc docs/architecture/20261031-frontend.md#loops
struct LoopEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var prompt = ""
    @Local private var scheduled = true
    /// Which shape of schedule: minutes, hours, daily, weekdays, weekly, monthly or cron.
    @Local private var shape = "daily"
    @Local private var minutes = 30
    @Local private var hours = 1
    @Local private var hour = 9
    @Local private var minute = 0
    @Local private var weekday = 1
    @Local private var monthDay = 1
    @Local private var cronText = ""
    @Local private var upcoming: [Double] = []
    @Local private var problem: String?
    @Local private var saving = false

    /// Ready-made prompts: a name for the button and the whole prompt it fills in.
    static let examples: [(String, String)] = [
        (L.text("分拣 GitHub issue"), L.text("用 gh 看 <owner/repo> 新开和有更新的 issue。能复现的 bug 和范围清楚的需求派成任务，key 用 github:<owner/repo>#<编号>；只是提问的，直接在 issue 下回答；需求不清楚的问我。")),
        (L.text("每天看 CI"), L.text("看 <owner/repo> 主分支最近的 CI。有失败就找出原因：能修的派成任务，key 用 ci:<运行编号>；偶发的先记下来，连续出现再派。")),
        (L.text("盯依赖更新"), L.text("看项目的依赖有没有新版本。补丁版本直接派成升级任务；大版本先读更新说明，有破坏性改动的问我。key 用 dep:<包名>@<版本>。")),
    ]

    var body: some View {
        let isNew = original["id"].str.isEmpty
        let ws = store.workspace(original["workspace"].str)
        let lead = ws.flatMap { store.role($0["lead"].str) }
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 4) {
                Text(isNew ? L.text("新建值班") : L.text("编辑值班")).font(.display(.title2))
                if let ws {
                    let place = ws["name"].str
                    let who = lead?["name"].str ?? ""
                    Text(who.isEmpty ? L.text("在「\(place)」值班") : L.text("在「\(place)」值班，由负责人「\(who)」来跑"))
                        .font(.callout).foregroundStyle(.secondary)
                }
                // How a Loop works, under the title: read before writing what it should do.
                Text(L.text("它自己不改文件，要改的事派成任务，结果到收件箱；名字等它第一次开工时自己起。"))
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(L.text("要它做什么")).font(.headline)
                ZStack(alignment: .topLeading) {
                    TextEditor(text: $prompt)
                        .font(.body)
                        .scrollContentBackground(.hidden)
                        .padding(8)
                    if prompt.isEmpty {
                        Text(L.text("每次开工都把这段话交给它。写清楚看哪里、什么情况派成任务、什么情况问你。"))
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 13).padding(.vertical, 8)
                            .allowsHitTesting(false)
                    }
                }
                .frame(minHeight: 150)
                .background(Color.primary.opacity(0.03), in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.primary.opacity(0.12)))
                HStack(spacing: 6) {
                    Text(L.text("从示例开始：")).font(.caption).foregroundStyle(.secondary)
                    ForEach(Self.examples, id: \.0) { name, text in
                        Button(name) { prompt = text }
                            .buttonStyle(.bordered).controlSize(.small)
                            .help(text)
                    }
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(L.text("什么时候开工")).font(.headline)
                // Two choices as radio buttons; what the schedule says sits under its own choice.
                radio(L.text("按时间表"), selected: scheduled) { scheduled = true }
                if scheduled {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            Picker(L.text("频率"), selection: $shape) {
                                Text(L.text("每隔几分钟")).tag("minutes")
                                Text(L.text("每隔几小时")).tag("hours")
                                Text(L.text("每天")).tag("daily")
                                Text(L.text("工作日")).tag("weekdays")
                                Text(L.text("每周")).tag("weekly")
                                Text(L.text("每月")).tag("monthly")
                                Divider()
                                Text(L.text("写 cron 表达式")).tag("cron")
                            }
                            .labelsHidden().fixedSize()
                            // Writing the expression starts from the one the form said.
                            .onChange(of: shape) { old, new in
                                if new == "cron", cronText.isEmpty { cronText = schedule(old).cron }
                            }
                            shapeControls
                        }
                        Group {
                            if let problem {
                                Text(problem).foregroundStyle(.red)
                            } else if !upcoming.isEmpty {
                                Text(L.text("接下来：\(upcoming.map(LoopText.when).joined(separator: " · "))"))
                            }
                        }
                        .font(.caption).fixedSize(horizontal: false, vertical: true)
                        Text(scheduleNote).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.leading, 24)
                }
                radio(L.text("只在你叫它时"), selected: !scheduled) { scheduled = false }
                if !scheduled {
                    Text(scheduleNote).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        .padding(.leading, 24)
                }
            }
            .task(id: scheduled ? schedule.cron : "") { await check() }

            HStack {
                Spacer()
                Button(L.text("取消"), role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                if isNew {
                    Button(L.text("只创建")) { save(wakeNow: false) }.disabled(!valid || saving)
                    Button(L.text("创建并马上开工")) { save(wakeNow: true) }
                        .buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(!valid || saving)
                } else {
                    Button(L.text("保存")) { save(wakeNow: false) }
                        .buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(!valid || saving)
                }
            }
        }
        .padding(24)
        .frame(width: 600)
        .onAppear { load() }
    }

    /// One of the two choices of when it wakes, as a radio button.
    private func radio(_ title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: selected ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(selected ? Color.accentColor : .secondary)
                    .accessibilityHidden(true)
                Text(title)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    /// The controls of the chosen shape: how many minutes or hours, which day, what time, or the
    /// expression itself.
    @ViewBuilder private var shapeControls: some View {
        // The time as two menus like the rest of the row: AppKit's typed time field took a pause
        // between two digits as the end of the hour, so a slowly typed 18 became 08 (issue #3).
        let timePicker = HStack(spacing: 4) {
            Picker(L.text("时"), selection: $hour) {
                ForEach(0..<24, id: \.self) { h in Text(String(format: "%02d", h)).tag(h) }
            }
            .labelsHidden().fixedSize()
            Text(":")
            Picker(L.text("分"), selection: $minute) {
                ForEach(LoopSchedule.minuteMarks(keeping: minute), id: \.self) { m in Text(String(format: "%02d", m)).tag(m) }
            }
            .labelsHidden().fixedSize()
        }
        switch shape {
        case "minutes":
            Picker(L.text("分钟"), selection: $minutes) {
                ForEach(LoopSchedule.minuteChoices, id: \.self) { m in Text(LoopSchedule.minutes(m).words).tag(m) }
            }
            .labelsHidden().fixedSize()
        case "hours":
            Picker(L.text("小时"), selection: $hours) {
                ForEach(LoopSchedule.hourChoices, id: \.self) { h in Text(LoopSchedule.hours(h).words).tag(h) }
            }
            .labelsHidden().fixedSize()
        case "weekly":
            Picker(L.text("星期"), selection: $weekday) {
                ForEach([1, 2, 3, 4, 5, 6, 0], id: \.self) { d in Text(LoopSchedule.weekday(d)).tag(d) }
            }
            .labelsHidden().fixedSize()
            timePicker
        case "monthly":
            Picker(L.text("日"), selection: $monthDay) {
                ForEach(1...31, id: \.self) { d in Text(L.text("\(d) 日")).tag(d) }
            }
            .labelsHidden().fixedSize()
            timePicker
        case "cron":
            TextField(L.text("分 时 日 月 星期"), text: $cronText)
                .font(.system(.body, design: .monospaced)).frame(width: 220)
        default:
            timePicker
        }
    }

    /// The schedule the form says.
    private var schedule: LoopSchedule { schedule(shape) }

    /// The schedule the form says in one shape.
    private func schedule(_ shape: String) -> LoopSchedule {
        let (h, m) = (hour, minute)
        switch shape {
        case "minutes": return .minutes(minutes)
        case "hours": return .hours(hours)
        case "weekdays": return .weekdays(hour: h, minute: m)
        case "weekly": return .weekly(day: weekday, hour: h, minute: m)
        case "monthly": return .monthly(day: monthDay, hour: h, minute: m)
        case "cron": return .cron(cronText.trimmingCharacters(in: .whitespaces))
        default: return .daily(hour: h, minute: m)
        }
    }

    /// The form set from the Loop it edits.
    private func load() {
        prompt = original["prompt"].str
        guard let cron = original["schedule"]["cron"].string else { scheduled = false; return }
        let parsed = LoopSchedule(cron: cron)
        cronText = parsed.cron
        let at = { (h: Int, m: Int) in (hour, minute) = (h, m) }
        switch parsed {
        case .minutes(let m): (shape, minutes) = ("minutes", m)
        case .hours(let h): (shape, hours) = ("hours", h)
        case .daily(let h, let m): shape = "daily"; at(h, m)
        case .weekdays(let h, let m): shape = "weekdays"; at(h, m)
        case .weekly(let d, let h, let m): (shape, weekday) = ("weekly", d); at(h, m)
        case .monthly(let d, let h, let m): (shape, monthDay) = ("monthly", d); at(h, m)
        case .cron: shape = "cron"
        }
    }

    /// Ask the server when the schedule comes round, a moment after the person stops changing it.
    private func check() async {
        guard scheduled else { (upcoming, problem) = ([], nil); return }
        try? await Task.sleep(for: .milliseconds(300))
        if Task.isCancelled { return }
        let cron = schedule.cron
        if cron.isEmpty { (upcoming, problem) = ([], L.text("写一条 5 段的 cron 表达式：分 时 日 月 星期")); return }
        let (times, error) = await store.preview(cron: cron)
        if Task.isCancelled { return }
        (upcoming, problem) = (times, error)
    }

    private var scheduleNote: String {
        scheduled
            ? L.text("按本机时间（\(TimeZone.current.identifier)）。上次没干完就顺延；Mac 睡着错过的，醒来补一次。它也能自己约个时间再来，值班页会写明。")
            : L.text("只在你点「现在开工」或回答它时开工；它也能自己约一次，值班页会写明。")
    }

    private var valid: Bool {
        !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (!scheduled || (problem == nil && !upcoming.isEmpty))
    }

    private func save(wakeNow: Bool) {
        saving = true
        let when: JSON = scheduled ? ["cron": JSON(schedule.cron), "tz": JSON(TimeZone.current.identifier)] : .null
        let body: JSON = ["prompt": JSON(prompt), "schedule": when, "wakeNow": JSON(wakeNow)]
        Task {
            if await store.saveLoop(original, body) { dismiss() }
            saving = false
        }
    }
}

/// A Loop's page: what it is and does now, its session (the same conversation card as a chat:
/// talk to it any time, which does not change when it wakes), what it dispatched and its notes.
// @project-doc docs/architecture/20261031-frontend.md#loops
struct LoopView: View {
    let run: RunModel

    var body: some View {
        let st = run.shown
        let loop = run.store.loop(ofRun: run.id) ?? .null
        let info = chatSession(st, live: run.live)
        let (phase, tone) = LoopText.phase(loop)
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 10) {
                LoopBadge(size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(loop["title"].string ?? st.run.goal).font(.display(.title3))
                        Pill(phase, tone)
                    }
                    Text(verbatim: [LoopText.schedule(loop), run.who("lead").name,
                                    L.text("开工 \(loop["status"]["wakes"].int ?? 0) 次"),
                                    L.text("派出 \(LoopText.dispatchedCount(loop)) 个任务")].joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    if let asked = LoopText.asked(loop) {
                        Text(asked).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
                Spacer()
                Button(L.text("现在开工")) { Task { await run.store.loopAction(loop, "wake") } }
                    .disabled(loop["status"]["phase"].str == "awake")
                    .help(L.text("不等时间表，叫它现在就干一次活"))
                if loop["state"].str == "paused" {
                    Button(L.text("继续")) { Task { await run.store.loopAction(loop, "resume") } }
                } else {
                    Button(L.text("暂停")) { Task { await run.store.loopAction(loop, "pause") } }
                        .help(L.text("停下它的时间表；暂停时仍可以叫它开工"))
                }
                Menu {
                    Button(L.text("编辑…")) { run.store.editingLoop = loop }
                    Button(L.text("删除值班…"), role: .destructive) { Task { await run.store.deleteLoop(loop) } }
                } label: { Image(systemName: "ellipsis.circle") }
                    .menuStyle(.borderlessButton).fixedSize()
                    .accessibilityLabel(L.text("更多"))
            }
            if loop["status"]["watchesFiles"] == .bool(false) {
                Note(text: L.text("不是 Git 仓库：值班直接改你的目录，Janus 看不出它改了什么，也就没法提醒你。"), tone: .amber)
            }
            if st.run.status == .failed && !st.run.error.isEmpty {
                Note(text: L.text("会话已结束：\(st.run.error) · 下次开工时再接上"), tone: .amber)
            }
            HStack(alignment: .top, spacing: 14) {
                ChatCard(run: run, st: st, title: loop["title"].str, info: info, target: .agent("lead"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                VStack(alignment: .leading, spacing: 10) {
                    // Its questions are answered here, with the same cards as the inbox.
                    InboxSection(run: run, st: st)
                    Text(L.text("派出的任务")).font(.headline)
                    if run.loopDispatched.isEmpty {
                        Text(L.text("还没有派出过任务")).font(.callout).foregroundStyle(.secondary)
                    } else {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 2) {
                                ForEach(run.loopDispatched, id: \.self) { r in
                                    let (l, t) = runLabel(r)
                                    Button { run.store.open(.run(r["id"].str)) } label: {
                                        VStack(alignment: .leading, spacing: 3) {
                                            Text(L.goalTitle(r["goal"].str)).lineLimit(2).multilineTextAlignment(.leading)
                                            HStack(spacing: 6) {
                                                Text(r["dispatchKey"].str).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                                Spacer()
                                                Pill(l, t)
                                            }
                                        }
                                        .padding(8).contentShape(Rectangle()).hoverable(6)
                                    }
                                    .buttonStyle(.plain)
                                    .help(r["sourceUrl"].string ?? "")
                                }
                                if let next = run.loopMore {
                                    Button(L.text("更早的任务…")) {
                                        Task {
                                            let page = await run.store.dispatched(by: loop["id"].str, before: next)
                                            run.loopDispatched += page.runs
                                            run.loopMore = page.next
                                        }
                                    }
                                    .buttonStyle(.link).padding(8)
                                }
                            }
                        }
                    }
                    if let saved = st.events.last(where: { $0.type == .loopStateSaved })?.data["text"].str, !saved.isEmpty {
                        Text(L.text("它记下的状态")).font(.headline).padding(.top, 6)
                        Text(saved).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
                .frame(width: 280, alignment: .topLeading)
            }
            .frame(maxHeight: .infinity)
        }
        .padding(20)
        .frame(maxWidth: 1200, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        // Its counts change with every dispatch and every change of a dispatched run's status.
        .task(id: [loop["id"], loop["status"]["dispatched"]]) {
            let page = await run.store.dispatched(by: loop["id"].str)
            (run.loopDispatched, run.loopMore) = (page.runs, page.next)
        }
    }
}
