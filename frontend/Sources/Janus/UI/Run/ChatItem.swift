import SwiftUI

/// One row of a conversation: a message, a note from Janus, a request or a tool call.
struct ChatItem: View {
    let run: RunModel
    let st: RunState
    let e: Event
    /// Further pieces of the same agent message, streamed after `e`.
    var more: [Event] = []
    /// A waiting request is answered above the composer: here it is one line.
    var docked = false

    /// What the server says about a section's opening event: an attempt's note and where it works.
    var note: Conversation.Note? = nil

    var body: some View {
        let d = e.data
        switch e.type {
        case .attemptStarted:
            VStack(alignment: .leading, spacing: 4) {
                Divider()
                Text(verbatim: L.text("\(e.task.map { "\($0) · " } ?? "")尝试 \(e.attempt ?? 0) · \(d["runtime"].str)\(d["config"]["model"].truthy ? " · \(d["config"]["model"].str)" : "")\(note?.note ?? "")"))
                    .font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                if let git = note?.git {
                    Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                }
                DisclosureGroup(L.text("Janus 发出的子任务说明")) { CodeBlock(text: d["prompt"].str, maxHeight: 360) }.font(.caption)
            }
        case .runCreated where RunKind(d["kind"].str) == .chat:
            divider(L.text("对话 · \(d["role"]["name"].str) · \(L.dateTime(e.ts))"))
            Text(verbatim: L.text("工作目录 \(L.tilde(d["cwd"].str)) · 对话专用，删对话时一起删")).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        case .runCreated where RunKind(d["kind"].str) == .dream:
            divider(L.text("整理团队记忆 · \(L.dateTime(e.ts))"))
        case .runCreated:
            divider(L.text("目标 · \(L.goalTitle(d["goal"].str))"))
            if let git = note?.git {
                Text(verbatim: git).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
            StoredImages(run: run, paths: Attach.paths(d), size: 56)
        case .agentMessage:
            TextRow(events: [e] + more, reply: true)
        case .agentThought:
            ThoughtLine(text: d["text"].str)
        case .sessionControl:
            note("\(d["failed"].truthy ? "⚠" : "↺") \(e.text)", d["failed"].truthy ? .amber : nil)
        case .sessionPrompt:
            let via = d["via"].str == "developer" ? L.text("开发者指令") : L.text("系统提示")
            // The system layer holds up to two sets of instructions, named by where they come from.
            let channel = PromptChannel(rawValue: d["channel"].str)
            let label: String = switch channel {
            case .instructions: L.text("数字员工的指令 · \(via)")
            case .system: run.isLoop ? L.text("值班的工作方式 · \(via)") : run.isDream ? L.text("怎么整理团队记忆 · \(via)") : L.text("团队协作指令 · \(via)")
            case .history: L.text("之前的对话 · \(via)")
            case .goal: L.text("目标提示")
            case .followup: L.text("继续提示")
            case .fix: L.text("提醒交结果 · 这一轮结束了还没交")
            case .board: L.text("看板推送 · 插入正在进行的这一轮")
            case .goalCheck: L.text("目标确认 · 负责人停了，但目标还没完成")
            case .resume: L.text("恢复会话 · 从中断处接着推进目标")
            case nil: L.text("Janus 提示")
            }
            let hint: String? = switch channel {
            case .instructions: L.text("数字员工的指令 + Agent 的自定义提示词。一对一对话只带这一份。")
            case .system: run.isLoop ? L.text("值班守则：不亲自改文件，活儿派成任务，拿不准才问你；附带共享资料。")
                : run.isDream ? L.text("只动笔记：合并重复的、改正过时的、删掉没用的；附带全部笔记的索引。")
                : L.text("团队任务专属：分工、看板、派工和验证规则。")
            case .history: L.text("重开对话时带上最近的聊天，接着聊。")
            default: nil
            }
            // One line in the flow, as the rest of Janus's own notes: what it sent, a click from its text.
            DisclosureGroup {
                CodeBlock(text: d["prompt"].str, maxHeight: 360).padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: "→ \(label)").font(.callout.weight(.medium)).foregroundStyle(.secondary)
                    Text(hint ?? L.text("Janus 发给 Agent")).font(.caption).foregroundStyle(.tertiary).lineLimit(1)
                }
                .help(L.text("Janus 发给 Agent"))
            }
        // Goal mode: a collaborator asks the lead, which answers on the board.
        case .questionAsked: note(L.text("? 问负责人：\(d["question"].string ?? e.text)\(d["options"].array.isEmpty ? "" : L.text("（\(d["options"].strings.joined(separator: " / "))）")) · 负责人会回答，不需要你处理"), .amber)
        // The lead edited files after handing out subtasks: it only manages from then on.
        case .leadEdited: note(L.text("⚠ 派出子任务后改了文件：\(e.data["paths"].strings.joined(separator: L.text("、"))) · 交付时会单独列出"), .amber)
        // The lead called a library tool after handing out subtasks: noted like an edit.
        case .leadRouted: note(L.text("⚠ 派出子任务后调用了资源库工具 \(e.data["tool"].str) · 会写进交付风险"), .amber)
        // A handoff with a verdict is a verification's findings (for a rework), not an upstream result.
        case .contextHandoff: note(d["verdict"].exists ? L.text("⇄ 独立验证 \(d["from"].string ?? e.text) 的意见已带入本次子任务") : L.text("⇄ 上游子任务 \(d["from"].string ?? e.text) 的结果已带入本次子任务"), nil)
        case .boardPosted: note(L.text("\(d["kind"].str == "blocker" ? "⚠" : d["kind"].str == "note" ? "·" : "↗") 协作者写回看板：\(d["text"].string ?? e.text)"), nil)
        case .taskConflict where d["owned"].truthy:
            note(L.text("⚠ 越界：\(e.task ?? "") 改了 \(d["other"].str) 负责的 \(d["paths"].strings.joined(separator: L.text("、")))。已提醒 \(e.task ?? "") 不要再改，并告诉负责人"), .amber)
        case .taskConflict: note(L.text("⚠ \(e.task ?? "") 和 \(d["other"].str) 同时在改 \(d["paths"].strings.joined(separator: L.text("、")))。已提醒双方先读最新内容，并告诉负责人"), .amber)
        case .userMessage:
            let via: String = switch MessageDelivery(d["delivery"].str) {
            case .steer: L.text("插话 · 已插进这一轮")
            case .queued: L.text("排队 · 这一轮结束后发送")
            case .prompt: L.text("作为新一轮发送")
            case .next: L.text("留言 · 下次会话带给它")
            default: ""
            }
            userBubble(d["text"].str, images: Attach.paths(d), via: via, insert: false)
        case .messageRouted:
            if let m = st.msgs[d["id"].str] {
                if MessageDelivery(d["delivery"].str) == .steer { userBubble(m.text, images: m.images, via: "\(m.by == "lead" ? L.text("负责人插话") : L.text("插话")) · \(m.deliveredSeq != nil ? L.text("Agent 已读到") : L.text("插话 · 已插进这一轮，做完手头这步就会读到"))", insert: true) }
                else { userBubble(m.text, images: m.images, via: (m.by == "lead" ? L.text("负责人 · ") : "") + L.text("作为新一轮发送"), insert: false) }
            }
        case .messageDelivered:
            if d["ids"].exists {
                ForEach(d["ids"].strings.compactMap { st.msgs[$0] }.filter { $0.delivery != .steer }, id: \.id) { m in
                    userBubble(m.text, images: m.images, via: (m.by == "lead" ? L.text("负责人 · ") : "") + (m.delivery == .next ? L.text("留言 · 这次会话开始时已带上") : m.delivery == .interrupt ? L.text("插话 · 打断上一轮后立刻送达") : L.text("排队 · 这一轮结束后发送")), insert: false)
                }
            } else { note("\(L.icon[e.type] ?? "·") \(e.text)", L.tone(of: e.type)) }
        // One line in the conversation; each criterion is a click away (and on the task page).
        case .resultSubmitted:
            let checks = Check.list(d["checks"])
            let passed = checks.filter(\.passed).count
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(checks.indices, id: \.self) { i in CheckRow(c: checks[i]) }
                    if !d["evidence"].array.isEmpty { Text(verbatim: L.text("证据：\(d["evidence"].strings.joined(separator: L.text("；")))")).font(.caption).foregroundStyle(.secondary) }
                    if !d["risks"].array.isEmpty { Text(verbatim: L.text("风险：\(d["risks"].strings.joined(separator: L.text("；")))")).font(.caption).foregroundStyle(Tone.amber.color) }
                }
                .padding(.top, 4)
            } label: {
                HStack(spacing: 6) {
                    Text(verbatim: L.text("⇪ 提交结果：\(d["summary"].string ?? e.text)")).font(.callout.weight(.semibold)).lineLimit(2)
                    if !checks.isEmpty { Pill(L.text("\(passed)/\(checks.count) 通过"), passed == checks.count ? .green : .amber) }
                    if !d["risks"].array.isEmpty { Pill(L.text("\(d["risks"].array.count) 个风险"), .amber) }
                }
            }
            .padding(8)
            .background(Tone.green.color.opacity(0.07), in: RoundedRectangle(cornerRadius: 8))
        case .agentStatus: note("◌ \(e.text)", .amber)
        // A Loop's wake: when it started and why, then when it ended, how long it took and when the next one is.
        case .loopWoke: divider(LoopText.woke(e))
        case .loopSlept: note("○ " + LoopText.slept(e, woke: LoopText.wake(before: e, in: st.events)), nil)
        case .loopFailed: note("⚠ \(L.dateTime(e.ts)) · \(e.text)", .amber)
        // A request still waiting is answered right here, with the same card as the inbox.
        case .approvalRequested, .inputRequested:
            if let item = st.inbox.first(where: { $0.key == d["key"].str }) {
                if docked { note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text) · 等你处理，在输入框上方"), .amber) } else { RequestCard(run: run, st: st, item: item) }
            }
            else { note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)"), L.tone(of: e.type)) }
        default: note(L.text("\(L.icon[e.type] ?? "·") \(run.who(e.actor).name)：\(e.text)"), L.tone(of: e.type))
        }
    }

    private func divider(_ t: String) -> some View {
        HStack { VStack { Divider() }; Text(t).font(.caption).foregroundStyle(.secondary).lineLimit(1); VStack { Divider() } }
    }

    private func note(_ t: String, _ tone: Tone?) -> some View {
        Text(t).font(.callout).foregroundStyle(tone?.color ?? .secondary).textSelection(.enabled)
            .frame(maxWidth: ChatRhythm.reading, alignment: .leading)
    }

    private func userBubble(_ text: String, images: [String] = [], via: String, insert: Bool) -> some View {
        let words = images.isEmpty ? text : Attach.visibleText(text)
        return HStack {
            Spacer(minLength: 60)
            VStack(alignment: .trailing, spacing: 4) {
                if insert { Text("↪ " + via).font(.caption.weight(.semibold)).foregroundStyle(Color.accent) }
                StoredImages(run: run, paths: images)
                if !words.isEmpty { Text(words).textSelection(.enabled) }
                if !insert && !via.isEmpty { Text(via).font(.caption).foregroundStyle(.secondary) }
            }
            .padding(10)
            .background(insert ? Color.accent.opacity(0.06) : Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(insert ? Color.accent.opacity(0.4) : .clear, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
        }
    }
}

struct CheckRow: View {
    let c: Check
    var body: some View {
        // The method is shown as text here too; the conversation only shows it, the task page
        // groups it and the delivery repeats it per task.
        CheckLine(n: nil, criterion: c.criterion, check: c, dense: true)
    }
}

/// One row per tool call, readable at a glance, with the full input and output a click away.
/// Janus's board tools are recorded twice (by Janus and by the agent): only Janus's record is
/// shown, and a call that came back replaces its "waiting" row.
struct ToolRow: View {
    let run: RunModel
    let e: Event
    /// The run's directory, so paths inside it read as relative ones.
    let cwd: String

    var body: some View {
        let d = e.data
        let board = d["tool"].str.hasPrefix("janus.")
        let running = e.type == .toolStarted
        // A call still running shows its newest frame: a title the agent filled in later, its output so far.
        let live = running ? run.preview[e.task.map { "task:" + $0 } ?? "agent:" + e.actor]?.tools[d["callId"].str] : nil
        let output = L.inWorkspace(live?.output ?? d["output"].str, cwd)
        let input = live.map { $0.input.isEmpty ? $0.title : $0.input } ?? (d["input"].truthy ? d["input"].str : e.text)
        let failed = d["status"].str == "failed"
        // The first line that says something: not a Markdown fence around the output.
        let firstLine = output.split(separator: "\n").map(String.init).first { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("```") && !$0.trimmingCharacters(in: .whitespaces).isEmpty } ?? ""
        let full = board ? d["args"].or(d["input"]).pretty() : input
        VStack(alignment: .leading, spacing: 4) {
            DisclosureGroup {
                CodeBlock(text: full + (output.isEmpty ? "" : "\n\n→ \(output)"), maxHeight: 300)
                ForEach(Array((run.shown.diffs[e.seq] ?? []).enumerated()), id: \.offset) { _, x in
                    InlineDiff(path: L.tilde(x.path), lines: x.lines)
                }
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        if board {
                            Text(run.isLoop ? L.text("值班") : L.text("看板")).font(.system(size: 11, design: .monospaced)).padding(.horizontal, 4).background(Color.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 3))
                            Text(janusLabel(d["tool"].str, d["args"], output).0).font(.callout).lineLimit(1).truncationMode(.tail)
                        } else {
                            let (verb, what, mono) = Self.label(kind: live?.tool ?? d["tool"].str, input: input, title: live?.title ?? e.text, running: running, cwd: cwd)
                            if running {
                                Shimmer(text: [verb, what].filter { !$0.isEmpty }.joined(separator: " "))
                            } else {
                                if !verb.isEmpty { Text(verb).font(.callout).foregroundStyle(failed ? Tone.red.color : .secondary) }
                                Text(verbatim: what).font(mono ? .system(.callout, design: .monospaced) : .callout)
                                    .foregroundStyle(failed ? Tone.red.color : .primary).lineLimit(1).truncationMode(.tail)
                            }
                        }
                        if running && !d["live"].truthy { Pill(L.text("等待返回"), .blue) }
                        if failed { Pill(L.text("失败"), .red) }
                        if !d["diffs"].array.isEmpty { Text(verbatim: L.text("改了 \(d["diffs"].array.count) 个文件")).font(.caption).foregroundStyle(.secondary) }
                    }
                    .help(input)
                    if !running, !firstLine.isEmpty { Text("→ " + L.short(board ? janusLabel(d["tool"].str, d["args"], output).1 : firstLine, 160)).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
            }
            // A running command's output so far: its last lines, as its own terminal ends.
            if running, !output.isEmpty {
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
        }
    }

    /// How a tool call reads in one line: a verb for its kind, running or done.
    static func verb(_ kind: String, running: Bool) -> String {
        switch kind {
        case "execute": return running ? L.text("正在运行") : L.text("运行")
        case "read": return running ? L.text("正在读") : L.text("读")
        case "edit": return running ? L.text("正在改") : L.text("改")
        case "delete": return running ? L.text("正在删除") : L.text("删除")
        case "move": return running ? L.text("正在移动") : L.text("移动")
        case "search": return running ? L.text("正在搜索") : L.text("搜索")
        case "fetch": return running ? L.text("正在获取") : L.text("获取")
        case "think": return running ? L.text("正在思考") : L.text("思考")
        default: return ""
        }
    }

    /// The last `lines` lines of a tool's output, and whether earlier ones were left out.
    static func tail(_ output: String, lines: Int = 16) -> (String, Bool) {
        let all = output.components(separatedBy: "\n")
        guard all.count > lines else { return (output, false) }
        return (all.suffix(lines).joined(separator: "\n"), true)
    }

    /// A call in one line: a verb for its kind and what it worked on, the command itself for a
    /// command and the file's name for a file. Unknown kinds read as their own title.
    static func label(kind: String, input: String, title: String, running: Bool, cwd: String) -> (verb: String, what: String, mono: Bool) {
        let verb = Self.verb(kind, running: running)
        let path = L.inWorkspace(input, cwd)
        switch kind {
        case "execute": return (verb, input.components(separatedBy: "\n").first ?? input, true)
        case "read", "edit", "delete", "move":
            // A plain path names the file; anything else (a range, a pattern) reads as the agent titled it.
            if !path.contains(" "), !path.isEmpty { return (verb, (path as NSString).lastPathComponent, true) }
            return (verb, title, false)
        case "search", "fetch": return (verb, input.isEmpty ? title : input, false)
        case "think": return (verb, "", false)
        default: return ("", title.isEmpty ? input : title, false)
        }
    }

    private func janusLabel(_ tool: String, _ a: JSON, _ out: String) -> (String, String) {
        let first = out.split(separator: "\n").first.map(String.init) ?? ""
        switch JanusTool(call: tool) {
        case .orchestrate: return (L.text("派出子任务：\(a["tasks"].array.map { "\($0["id"].str) \($0["title"].str) → \(run.who($0["assignee"].str).name)" }.joined(separator: L.text("；")))"), first)
        case .wait: return (L.text("等待看板上的进展"), first)
        case .reply: return (L.text("回复 \(a["task"].str)：\(a["text"].str)"), first)
        case .retry: return (L.text("重新执行 \(a["task"].str)\(a["instructions"].truthy ? L.text("（改了说明）") : "")\(a["accept"].truthy ? L.text("（改了验收标准）") : "")"), first)
        case .amend: return (L.text("修订 \(a["task"].str)：\(a["text"].str)"), first)
        case .cancel: return (L.text("取消 \(a["tasks"].strings.joined(separator: L.text("、")))\(a["reason"].truthy ? L.text("：\(a["reason"].str)") : "")"), first)
        case .askUser: return (L.text("等你拍板：\(a["question"].str)"), first)
        case .board: return (a["task"].truthy ? L.text("查看 \(a["task"].str) 的全部信息") : L.text("查看团队和子任务状态"), "")
        case .boardPost: return (L.text("写回看板：\(a["text"].str)"), "")
        default: return (tool, first)
        }
    }
}
