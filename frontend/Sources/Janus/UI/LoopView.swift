import SwiftUI

/// Workspace Loops in the app: the section on a workspace's home, the rows under it in the sidebar,
/// the form to make or change one, and its page (its session, what it dispatched, its notes).
// @project-doc docs/architecture/20261031-frontend.md#loops

/// What the app says about a Loop: its rhythm and what it is doing now.
enum LoopText {
    static func rhythm(_ l: JSON) -> String {
        switch l["rhythm"]["kind"].str {
        case "interval": return every(l["rhythm"]["minutes"].int ?? 0)
        case "self_paced": return L.text("自己决定")
        default: return L.text("只在叫它时")
        }
    }

    /// An interval in words: every 30 minutes, every hour, every day.
    static func every(_ m: Int) -> String {
        switch m {
        case 60: return L.text("每小时")
        case 1440: return L.text("每天")
        case _ where m % 60 == 0: return L.text("每 \(m / 60) 小时")
        default: return L.text("每 \(m) 分钟")
        }
    }

    /// How many runs it dispatched, from the server's counts by status.
    static func dispatchedCount(_ l: JSON) -> Int { l["status"]["dispatched"].object.values.reduce(0) { $0 + ($1.int ?? 0) } }

    /// The phase as a pill: sleeping (with when it wakes), awake, paused, failed.
    static func phase(_ l: JSON) -> (String, Tone) {
        let s = l["status"]
        switch s["phase"].str {
        case "awake": return (L.text("醒着"), .blue)
        case "paused": return (L.text("已暂停"), .gray)
        case "failed": return (L.text("出错"), .red)
        default:
            if let at = s["nextWakeAt"].double { return (L.text("睡着 · \(L.clock(at)) 醒"), .gray) }
            return (L.text("睡着"), .gray)
        }
    }
}

/// The Loops of one workspace, between its shared material and its task records.
struct LoopSection: View {
    let store: AppStore
    let ws: JSON

    var body: some View {
        let loops = store.loops(of: ws["id"].str)
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(L.text("值班")).font(.headline)
                Text(L.text("按节奏醒来，看一眼、做判断，把要改的事派成任务")).font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button(L.text("新建值班…")) { store.editingLoop = AppStore.blankLoop(ws["id"].str) }.buttonStyle(.link)
            }
            if !loops.isEmpty {
                Card(padding: 4) {
                    ForEach(loops, id: \.self) { l in
                        let (label, tone) = LoopText.phase(l)
                        Button { store.open(.run(l["run"].str)) } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                HStack(spacing: 10) {
                                    Image(systemName: "arrow.triangle.2.circlepath").foregroundStyle(.secondary).accessibilityHidden(true)
                                    Text(l["title"].str).lineLimit(1)
                                    Text(LoopText.rhythm(l)).font(.caption).foregroundStyle(.secondary)
                                    Spacer()
                                    Pill(label, tone)
                                }
                                if let last = l["status"]["lastSummary"].string, !last.isEmpty {
                                    Text(L.text("上次：\(last)")).font(.caption).foregroundStyle(.secondary).lineLimit(1).padding(.leading, 26)
                                }
                            }
                            .padding(.horizontal, 10).padding(.vertical, 8)
                            .contentShape(Rectangle())
                            .hoverable(6)
                        }
                        .buttonStyle(.plain)
                        if l != loops.last { Divider() }
                    }
                }
            }
        }
    }
}

/// A Loop under its workspace in the sidebar: a click opens its page.
struct LoopRow: View {
    let store: AppStore
    let loop: JSON

    var body: some View {
        let open: Bool = { if case .run(let id) = store.screen { return id == loop["run"].str } else { return false } }()
        let waiting = store.inbox.open.filter { $0.run == loop["run"].str }.count
        SidebarItem(title: loop["title"].str, selected: open, waiting: waiting, live: loop["status"]["phase"].str == "awake",
                    help: "\(LoopText.rhythm(loop)) · \(LoopText.phase(loop).0)",
                    accessibility: waiting == 0 ? "" : L.text("\(waiting) 项等你处理"),
                    action: { store.open(.run(loop["run"].str)) }) {
            Image(systemName: "arrow.triangle.2.circlepath").font(.caption).foregroundStyle(.secondary).frame(width: 18)
        }
        .padding(.leading, 18)
        .opacity(loop["state"].str == "paused" ? 0.6 : 1)
    }
}

/// Make a Loop, or change one: what it does each time it wakes, and how often it wakes. It has no
/// name to type: its agent titles its session the first time it wakes, and that names it.
// @project-doc docs/architecture/20261031-frontend.md#loops
struct LoopEditor: View {
    let store: AppStore
    let original: JSON
    @Environment(\.dismiss) private var dismiss
    @Local private var prompt = ""
    @Local private var kind = "interval"
    @Local private var minutes = 30
    @Local private var saving = false

    /// Intervals offered, in minutes.
    static let intervals = [5, 10, 15, 30, 60, 120, 240, 480, 1440]

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
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(L.text("要它做什么")).font(.headline)
                ZStack(alignment: .topLeading) {
                    TextEditor(text: $prompt)
                        .font(.body)
                        .scrollContentBackground(.hidden)
                        .padding(8)
                    if prompt.isEmpty {
                        Text(L.text("每次醒来都把这段话交给它。写清楚看哪里、什么情况派成任务、什么情况问你。"))
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
                Text(L.text("多久醒一次")).font(.headline)
                HStack(spacing: 12) {
                    Picker(L.text("节奏"), selection: $kind) {
                        Text(L.text("固定间隔")).tag("interval")
                        Text(L.text("自己决定")).tag("self_paced")
                        Text(L.text("只在叫它时")).tag("manual")
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                    if kind == "interval" {
                        Picker(L.text("间隔"), selection: $minutes) {
                            ForEach(intervalChoices, id: \.self) { m in Text(LoopText.every(m)).tag(m) }
                        }
                        .labelsHidden().fixedSize()
                    }
                }
                Text(rhythmNote).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }

            Label(L.text("它自己不改文件，要改的事派成任务，结果到收件箱；名字由它第一次醒来时自己起。"), systemImage: "info.circle")
                .font(.caption).foregroundStyle(.secondary)

            HStack {
                Spacer()
                Button(L.text("取消"), role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                if isNew {
                    Button(L.text("只创建")) { save(wakeNow: false) }.disabled(!valid || saving)
                    Button(L.text("创建并醒来一次")) { save(wakeNow: true) }
                        .buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(!valid || saving)
                } else {
                    Button(L.text("保存")) { save(wakeNow: false) }
                        .buttonStyle(.primary).keyboardShortcut(.defaultAction).disabled(!valid || saving)
                }
            }
        }
        .padding(24)
        .frame(width: 600)
        .onAppear {
            prompt = original["prompt"].str
            kind = original["rhythm"]["kind"].string ?? "interval"
            minutes = original["rhythm"]["minutes"].int ?? 30
        }
    }

    /// The offered intervals, with the Loop's own when it is none of them.
    private var intervalChoices: [Int] { Self.intervals.contains(minutes) ? Self.intervals : (Self.intervals + [minutes]).sorted() }

    private var rhythmNote: String {
        switch kind {
        case "interval": return L.text("上一次醒来结束后，隔这么久再醒。Mac 睡眠时错过的，醒来后只补一次。")
        case "self_paced": return L.text("每次醒来时由它决定下次什么时候醒，1 分钟到 24 小时；没决定就 30 分钟后。适合有事时勤看、没事时少看。")
        default: return L.text("只在你点「现在醒来」或回答它的问题时醒。")
        }
    }

    private var valid: Bool { !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func save(wakeNow: Bool) {
        saving = true
        var rhythm: JSON = ["kind": JSON(kind)]
        if kind == "interval" { rhythm["minutes"] = JSON(minutes) }
        let body: JSON = ["prompt": JSON(prompt), "rhythm": rhythm, "wakeNow": JSON(wakeNow)]
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
                Image(systemName: "arrow.triangle.2.circlepath").font(.title3).foregroundStyle(.secondary).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(loop["title"].string ?? st.run.goal).font(.display(.title3))
                        Pill(phase, tone)
                    }
                    Text(verbatim: [LoopText.rhythm(loop), run.who("lead").name,
                                    L.text("醒来 \(loop["status"]["wakes"].int ?? 0) 次"),
                                    L.text("派出 \(LoopText.dispatchedCount(loop)) 个任务")].joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                Button(L.text("现在醒来")) { Task { await run.store.loopAction(loop, "wake") } }
                    .disabled(loop["status"]["phase"].str == "awake")
                    .help(L.text("不等节奏，现在就醒来一次"))
                if loop["state"].str == "paused" {
                    Button(L.text("继续")) { Task { await run.store.loopAction(loop, "resume") } }
                } else {
                    Button(L.text("暂停")) { Task { await run.store.loopAction(loop, "pause") } }
                        .help(L.text("停下它的节奏；暂停时仍可以手动让它醒来"))
                }
                Menu {
                    Button(L.text("编辑…")) { run.store.editingLoop = loop }
                    Button(L.text("删除值班…"), role: .destructive) { Task { await run.store.deleteLoop(loop) } }
                } label: { Image(systemName: "ellipsis.circle") }
                    .menuStyle(.borderlessButton).fixedSize()
                    .accessibilityLabel(L.text("更多"))
            }
            if loop["status"]["watchesFiles"] == .bool(false) {
                Note(text: L.text("工作区不在 Git 仓库里：值班醒来期间改了哪些文件，Janus 看不出来，也就不会提醒你。"), tone: .amber)
            }
            if st.run.status == .failed && !st.run.error.isEmpty {
                Note(text: L.text("会话已结束：\(st.run.error)。下次醒来会重新打开。"), tone: .amber)
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
                    if let saved = st.events.last(where: { $0.type == "loop.state_saved" })?.data["text"].str, !saved.isEmpty {
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
