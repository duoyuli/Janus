import AppKit
import Foundation
import SwiftUI
import Testing
@testable import Janus

// The workbench's density rules (W1 F2, F6, F9, U2, U6): the summary line, the delivery overview
// and acceptance rows, and the layout widths and heights, all read here without a screenshot.

// MARK: Run card summary

private func ledger(tokens: UsageTokens? = nil, completeness: UsageCompleteness = .missing,
                    durationMs: Int? = nil, context: UsageContext? = nil,
                    cost: UsageCost? = nil, sessions: [UsageSession] = []) -> UsageLedger {
    var l = UsageLedger()
    l.run.tokens = tokens ?? UsageTokens()
    l.run.completeness = completeness
    l.run.durationMs = durationMs
    l.run.context = context ?? UsageContext()
    l.run.cost = cost ?? UsageCost()
    l.sessions = sessions
    return l
}

@Test func runCardLineKeepsTheReportedNumbersInOrder() {
    let all = ledger(
        tokens: UsageTokens(total: 76_800_000, input: 50_000_000, output: 1_000_000), completeness: .complete,
        durationMs: 2_878_000, context: UsageContext(used: 78_000, peak: 90_000, size: 200_000, reports: 4),
        cost: UsageCost(lines: [UsageCostLine(currency: "USD", amount: 44.2864)], reportedSessions: 1),
        sessions: [UsageSession(key: "a", actor: "lead", task: nil, attempt: nil, turns: 2,
                                context: UsageContext(used: 78_000, size: 200_000, reports: 4))])
    #expect(all.runCardLine(tasks: 10, done: 3)
        == expectedCopy("子任务 3 / 10 · 用量 76.8M tokens（完整） · 费用 44.2864 USD · 耗时 47 分 58 秒（墙钟） · 上下文 39%", "Subtasks: 3 / 10 · Usage: 76.8M tokens (Complete) · Cost: 44.2864 USD · Elapsed: 47 min 58 sec (wall time) · Context: 39%"))
    // VoiceOver gets every digit, never the compact form.
    #expect(all.runCardVoice(tasks: 10, done: 3)
        == expectedCopy("子任务 3 / 10，用量 76,800,000 tokens（完整），费用 44.2864 USD，耗时 47 分 58 秒（墙钟），上下文 78,000 / 200,000（39%） · 峰值 90,000", "Subtasks: 3 / 10, Usage: 76,800,000 tokens (Complete), Cost: 44.2864 USD, Elapsed: 47 min 58 sec (wall time), Context: 78,000 / 200,000 (39%) · Peak: 90,000"))

    // The compact line keeps 「N 个会话」 out (it is in the expanded ledger), while a cost row
    // that names several sessions still says so.
    var manySessions = all
    manySessions.run.cost = UsageCost(lines: [UsageCostLine(currency: "USD", amount: 44.2864, sessions: 9)], reportedSessions: 9)
    #expect(manySessions.runCardLine(tasks: 10, done: 3).contains(expectedCopy("费用 44.2864 USD ·", "Cost: 44.2864 USD ·")))
    #expect(!manySessions.runCardLine(tasks: 10, done: 3).contains(expectedCopy("个会话", "sessions")))
    #expect(manySessions.run.costDetail == expectedCopy("44.2864 USD（9 个会话）", "44.2864 USD (9 sessions)"))
}

@Test func runCardLineLeavesOutWhatWasNotReported() {
    let partial = ledger(tokens: UsageTokens(total: 15_400), completeness: .partial, durationMs: 65_000,
                         sessions: [UsageSession(key: "a", actor: "m", task: "T1", attempt: 1, turns: 1)])
    #expect(partial.runCardLine(tasks: 3, done: 2) == expectedCopy("子任务 2 / 3 · 用量 15.4K tokens（部分上报） · 耗时 1 分 05 秒（墙钟）", "Subtasks: 2 / 3 · Usage: 15.4K tokens (Partially reported) · Elapsed: 1 min 05 sec (wall time)"))

    // Only a context snapshot: no tokens, no cost, but the ledger opens and the ratio is shown.
    let contextOnly = ledger(durationMs: 2_000, context: UsageContext(used: 52_000, size: 200_000, reports: 2),
                             sessions: [UsageSession(key: "a", actor: "m", task: "T1", attempt: 1,
                                                     context: UsageContext(used: 52_000, size: 200_000, reports: 2))])
    #expect(contextOnly.runCardLine(tasks: 2, done: 1) == expectedCopy("子任务 1 / 2 · 耗时 2 秒（墙钟） · 上下文 26%", "Subtasks: 1 / 2 · Elapsed: 2 sec (wall time) · Context: 26%"))
    #expect(contextOnly.hasUsage)

    // Nothing reported at all: only the wall-clock time, and no ledger control.
    let fresh = ledger(durationMs: 400)
    #expect(fresh.runCardLine(tasks: 0, done: 0) == expectedCopy("耗时 不到 1 秒（墙钟）", "Elapsed: Under 1 sec (wall time)"))
    #expect(!fresh.hasUsage)
    #expect(!UsageLedger.empty.hasUsage)
}

@Test func ledgerOpensOnlyWhenASessionReportedSomething() {
    // An opened session that reported nothing is not a ledger.
    let opened = ledger(sessions: [UsageSession(key: "a", actor: "m", task: "T1", attempt: 1)])
    #expect(!opened.hasUsage)
    let costOnly = ledger(cost: UsageCost(lines: [UsageCostLine(currency: "USD", amount: 1)], reportedSessions: 1),
                          sessions: [UsageSession(key: "a", actor: "m", task: "T1", attempt: 1,
                                                  cost: [UsageCostLine(currency: "USD", amount: 1)])])
    #expect(costOnly.hasUsage)
}

@Test func ledgerControlStaysWhileOpenWhereNothingWasReported() {
    // A replayed position before any usage: no control while folded, but an already open ledger
    // keeps the control so it can be folded again.
    let empty = ledger(durationMs: 400)
    #expect(!empty.showsLedgerControl(open: false))
    #expect(empty.showsLedgerControl(open: true))
    let reported = ledger(durationMs: 400, sessions: [UsageSession(key: "a", actor: "m", task: nil, attempt: nil, turns: 1)])
    #expect(reported.showsLedgerControl(open: false) && reported.showsLedgerControl(open: true))
}

// MARK: Delivery overview and rows

private func check(_ criterion: String, _ method: String?, passed: Bool = true, evidence: String = "看到结果") -> Check {
    var j: JSON = ["criterion": JSON(criterion), "passed": JSON(passed), "evidence": JSON(evidence)]
    if let method { j["method"] = JSON(method) }
    return Check(j)
}

private func verification(_ task: String, passed: Bool) -> VerificationRecord {
    VerificationRecord(["task": JSON(task), "label": JSON(passed ? "已完成" : "没通过验收"), "passed": JSON(passed),
                        "summary": JSON(""), "checks": [], "evidence": [], "risks": []])!
}

private func record(_ task: String, title: String = "标题", status: TaskStatus = .succeeded,
                    accept: [String] = [], checks: [Check] = [], verifications: [VerificationRecord] = [],
                    traceGaps: [String] = [], risks: [String] = []) -> DeliveryAcceptance {
    DeliveryAcceptance(task: task, title: title, status: status,
                       summary: EvidenceSummary(accept: accept, checks: checks),
                       checks: checks, verifications: verifications, traceGaps: traceGaps, risks: risks)
}

@Test func deliveryRowCarriesTheNumbersAndOnlyMarksProblems() {
    let accept = ["一", "二", "三", "四", "五"]
    let checks = [check("一", "test"), check("二", "command"), check("三", "command", passed: false),
                  check("四", "reasoning"), check("五", "inspection")]
    let r = record("T3", title: "清理旧构建", accept: accept, checks: checks).row
    #expect(r.metrics == expectedCopy("通过 4 / 5 条 · 测试 1 条、命令 2 条、检查 1 条、推理判断 1 条", "Passed 4 of 5 · Test: 1, Command: 2, Inspection: 1, Reasoning: 1"))
    #expect(r.markers == [expectedCopy("提醒 1 条", "Notes: 1")])
    #expect(r.needsAttention, "a failed criterion opens the row by default")
    #expect(r.voice(open: true)
        == expectedCopy("T3，清理旧构建，通过 4 / 5 条，测试 1 条、命令 2 条、检查 1 条、推理判断 1 条，已展开，提醒 1 条", "T3, 清理旧构建, Passed 4 of 5, Test: 1, Command: 2, Inspection: 1, Reasoning: 1, Expanded, Notes: 1"))

    // Everything passed and nothing else reported: collapsed, no markers.
    let clean = record("T1", title: "调研", accept: ["一", "二"], checks: [check("一", "test"), check("二", "command")]).row
    #expect(clean.markers.isEmpty && !clean.needsAttention)
    #expect(clean.voice(open: false) == expectedCopy("T1，调研，通过 2 / 2 条，测试 1 条、命令 1 条，已收起", "T1, 调研, Passed 2 of 2, Test: 1, Command: 1, Collapsed"))
}

@Test func deliveryRowsOpenOnAttentionAndOnlyMarkReminders() {
    func needs(_ r: DeliveryAcceptance) -> Bool { r.row.needsAttention }
    let passed = [check("一", "test")]

    #expect(!needs(record("T1", accept: ["一"], checks: passed)))
    #expect(needs(record("T1", accept: ["一"], checks: [check("一", "test", passed: false)])))
    #expect(needs(record("T1", accept: ["一"], checks: passed, traceGaps: ["T1 改了没有的文件"])))
    #expect(needs(record("T1", accept: ["一"], checks: passed, risks: ["没有测试覆盖"])))
    #expect(needs(record("T1", accept: ["一"], checks: passed, verifications: [verification("V1", passed: false)])))
    #expect(!needs(record("T1", accept: ["一"], checks: passed, verifications: [verification("V1", passed: true)])))

    // A reminder alone marks the row and leaves it folded.
    let reminder = record("T1", accept: ["一"], checks: [check("一", "reasoning")])
    #expect(!reminder.row.needsAttention)
    #expect(reminder.row.markers == [expectedCopy("提醒 1 条", "Notes: 1")])

    // A task that did not succeed says so in words.
    let failed = record("T2", status: .verificationFailed, accept: ["一"], checks: passed)
    #expect(failed.row.markers == [expectedCopy("没通过验收", "Acceptance check failed")])
    #expect(!failed.row.needsAttention, "the default expansion rules do not include the status alone")
}

@Test func deliveryOverviewAddsTheTasksAndTheRunRisks() {
    let checks = [check("一", "test"), check("二", "command"), check("三", "command", passed: false),
                  check("四", "reasoning"), check("五", "inspection")]
    let first = record("T1", accept: ["一", "二", "三", "四", "五"], checks: checks,
                       verifications: [verification("V2", passed: false)], traceGaps: ["改了别的任务的文件"], risks: ["旧数据不迁移"])
    let second = record("T2", accept: ["一", "二", "三"],
                        checks: [check("一", "test"), check("二", "test"), check("三", "test")])
    let o = deliveryOverview([first, second], runRisks: ["上线前要跑一次全量"])
    #expect(o.criteria == 8 && o.passed == 7 && o.failed == 1)
    #expect(o.runRisks == 1 && o.taskRisks == 1 && o.risks == 2)
    #expect(o.text == expectedCopy("通过 7 / 8 条 · 测试 4 条、命令 2 条、检查 1 条、推理判断 1 条 · 运行级风险 1 条 · 子任务风险 1 条 · 痕迹 1 处", "Passed 7 of 8 · Test: 4, Command: 2, Inspection: 1, Reasoning: 1 · Run risks: 1 · Subtask risks: 1 · Trace discrepancies: 1"))
    #expect(o.issuesText == expectedCopy("1 条标准未通过、运行级风险 1 条、子任务风险 1 条、1 处痕迹对不上、1 项独立验证未确认", "Failed criteria: 1, Run risks: 1, Subtask risks: 1, Trace discrepancies: 1, Unconfirmed independent reviews: 1"))
    #expect(deliveryOverview([], runRisks: []).issuesText == nil)
}

@Test func runRiskListFoldsLongListsAndNamesWhatIsHidden() {
    let few = runRiskList(["a", "b"], open: false)
    #expect(few.shown == ["a", "b"] && !few.isFoldable && few.toggleLabel(open: false) == nil)
    #expect(runRiskList([], open: false).shown.isEmpty)
    #expect(runRiskList([], open: true).toggleLabel(open: true) == nil)

    let many = (1...50).map { "r\($0)" }
    let folded = runRiskList(many, open: false)
    #expect(folded.shown.count == RunRiskList.previewLimit && folded.hiddenNow == 45)
    #expect(folded.toggleLabel(open: false) == expectedCopy("展开全部 50 条（余下 45 条）", "Show all 50 (45 more)"))
    let open = runRiskList(many, open: true)
    #expect(open.shown == many && open.hiddenNow == 0)
    #expect(open.toggleLabel(open: true) == expectedCopy("收起，只看前 5 条", "Collapse to the first 5"))
    // At or under the preview there is no control, and opening changes nothing.
    let exact = runRiskList(Array(many.prefix(RunRiskList.previewLimit)), open: false)
    #expect(!exact.isFoldable && exact.toggleLabel(open: false) == nil && exact.shown.count == 5)
}

// MARK: Layout rules

@Test func boardLayoutFollowsTheDensityWidths() {
    #expect(BoardLayout.isTwoColumn(838) && !BoardLayout.isTwoColumn(837))
    #expect(BoardLayout.runCardStacks(639) && !BoardLayout.runCardStacks(640))
    #expect(BoardLayout.deliveryRowStacks(837) && !BoardLayout.deliveryRowStacks(838))

    let roomy = BoardLayout.cardHeights(contentHeight: 1_000, twoColumn: true, empty: false)
    #expect(roomy.chat == 700 && roomy.inspector == 700)
    let short = BoardLayout.cardHeights(contentHeight: 300, twoColumn: true, empty: false)
    #expect(short.chat == 420 && short.inspector == 420)
    let tall = BoardLayout.cardHeights(contentHeight: 2_000, twoColumn: true, empty: false)
    #expect(tall.chat == 800 && tall.inspector == 800)
    let fresh = BoardLayout.cardHeights(contentHeight: 1_000, twoColumn: true, empty: true)
    #expect(fresh.chat == 320 && fresh.inspector == 320)
    let stacked = BoardLayout.cardHeights(contentHeight: 1_000, twoColumn: false, empty: false)
    #expect(stacked.chat == 700 && stacked.inspector == 480)
}

@Test @MainActor func inspectorStartsOnTheSavedTabOrByState() {
    #expect(RunModel.initialInspectorTab(saved: nil, hasTasks: false) == "team")
    #expect(RunModel.initialInspectorTab(saved: nil, hasTasks: true) == "tasks")
    #expect(RunModel.initialInspectorTab(saved: "", hasTasks: true) == "tasks")
    #expect(RunModel.initialInspectorTab(saved: "changes", hasTasks: false) == "changes")
    #expect(RunModel.initialInspectorTab(saved: "team", hasTasks: true) == "team")
}

// MARK: Rendered cards

/// The cards the workbench shows, drawn off screen: the fresh run card and the delivery's default
/// and opened rows, in both appearances. The pictures are read beside the real-window shots; the
/// assertions keep them from silently turning into blank images.
@Test @MainActor func workbenchCardsRenderFoldedAndOpen() throws {
    let dir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("janus-workbench", isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    func render(_ name: String, dark: Bool, width: CGFloat, _ view: some View) throws {
        // The background sits inside the scheme so the page (not just the card) is dark.
        let lit = view.padding(16).frame(width: width, alignment: .leading)
            .background(Color.page)
            .environment(\.colorScheme, dark ? .dark : .light)
        let renderer = ImageRenderer(content: lit)
        renderer.scale = 2
        let image = try #require(renderer.nsImage, "SwiftUI drew \(name)")
        let tiff = try #require(image.tiffRepresentation)
        let rep = try #require(NSBitmapImageRep(data: tiff))
        let data = try #require(rep.representation(using: .png, properties: [:]))
        try data.write(to: dir.appendingPathComponent("\(name).png"))
        #expect(data.count > 10_000, "\(name) is not blank")
    }

    // A run that just started: one created event, so its duration is under a second.
    var fresh: [Event] = []
    fresh.append(Event(["seq": 1, "ts": 1_000_000, "type": "run.created", "actor": "user", "text": "", "data": ["goal": "刚创建的目标"]]))
    let freshRun = RunModel(id: "fresh", store: testStore)
    freshRun.events = fresh
    freshRun.replay = fresh.count
    let freshState = freshRun.shown
    try render("run-card-fresh-light", dark: false, width: 1100, RunCard(run: freshRun, st: freshState, width: 1100))
    try render("run-card-fresh-dark", dark: true, width: 1100, RunCard(run: freshRun, st: freshState, width: 1100))
    try render("run-card-fresh-narrow-light", dark: false, width: 600, RunCard(run: freshRun, st: freshState, width: 600).frame(width: 600))

    // A delivery with two tasks: one clean and folded, one with a failed criterion, a trace gap
    // and a risk, which starts open.
    func check(_ n: Int, _ criterion: String, _ method: String, passed: Bool) -> JSON {
        ["criterion": JSON(criterion), "passed": JSON(passed), "evidence": JSON("运行结果 \(n)"), "method": JSON(method)]
    }
    let accept1: JSON = ["task": "T1", "title": "调研调用方", "status": "succeeded",
                         "criteria": 5, "passed": 5, "methods": [["method": "test", "label": "测试", "count": 2], ["method": "command", "label": "命令", "count": 3]],
                         "checks": [check(1, "调用方列全", "test", passed: true), check(2, "测试通过", "test", passed: true),
                                    check(3, "构建通过", "command", passed: true), check(4, "类型检查通过", "command", passed: true),
                                    check(5, "没有警告", "command", passed: true)],
                         "verifications": [], "traceGaps": [], "risks": []]
    let accept2: JSON = ["task": "T2", "title": "实现一个很长的子任务标题，用来检查单行截断和数字不被截断", "status": "verification_failed",
                         "criteria": 3, "passed": 1, "methods": [["method": "inspection", "label": "检查", "count": 1], ["method": "reasoning", "label": "推理判断", "count": 1]],
                         "checks": [check(1, "新接口有测试", "test", passed: false), check(2, "旧调用方迁移", "inspection", passed: true),
                                    check(3, "性能不变", "reasoning", passed: false)],
                         "verifications": [["task": "V1", "label": "没通过验收", "passed": false, "summary": "没有独立复现", "checks": [], "evidence": [], "risks": []]],
                         "traceGaps": ["T2 声称改了 shared.swift，但 diff 里没有"], "risks": ["没有跑集成测试"]]
    var events: [Event] = []
    func add(_ type: String, actor: String = "janus", task: String? = nil, attempt: Int? = nil, ts: Int, data: JSON = [:]) {
        var j: JSON = ["seq": JSON(events.count + 1), "ts": JSON(ts), "type": JSON(type), "actor": JSON(actor), "text": "", "data": data]
        if let task { j["task"] = JSON(task) }
        if let attempt { j["attempt"] = JSON(attempt) }
        events.append(Event(j))
    }
    add("run.created", actor: "user", ts: 1_000_000, data: ["goal": "把工作台的信息密度调好"])
    add("orchestration.accepted", actor: "lead", ts: 1_001_000, data: ["tasks": [
        ["id": "T1", "title": "调研调用方", "agent": "researcher", "deps": [], "accept": ["列出调用方"]],
        ["id": "T2", "title": "实现一个很长的子任务标题，用来检查单行截断和数字不被截断", "agent": "impl", "deps": ["T1"], "accept": ["测试通过"]],
    ]])
    add("attempt.started", actor: "researcher", task: "T1", attempt: 1, ts: 1_010_000, data: ["session": "s1"])
    add("session.usage", actor: "researcher", task: "T1", attempt: 1, ts: 1_020_000,
        data: ["inputTokens": 12000, "outputTokens": 3400, "totalTokens": 15400, "used": 52000, "size": 200000, "cost": ["amount": 0.25, "currency": "USD"]])
    add("task.succeeded", actor: "researcher", task: "T1", attempt: 1, ts: 1_100_000, data: ["summary": "三处调用方"])
    add("run.delivered", actor: "lead", ts: 1_200_000, data: [
        "summary": "工作台按新密度拆好了：运行卡片一行摘要，账本和逐条验收页内展开。",
        "risks": ["旧截图里的数字要按新折叠位置复核"],
        "deliverables": [["name": "工作台报告", "path": "/tmp/report.md", "seq": 6, "desc": "改动和证据"]],
        "evidence": [["text": "T1：三处调用方", "seq": 5]],
        "acceptance": [accept1, accept2],
    ])
    let run = RunModel(id: "delivery", store: testStore)
    run.events = events
    run.replay = events.count
    let st = run.shown
    #expect(deliveryAcceptance(st).count == 2)
    try render("result-card-light", dark: false, width: 1100, ResultCard(run: run, st: st, width: 1100))
    try render("result-card-dark", dark: true, width: 1100, ResultCard(run: run, st: st, width: 1100))
    // Every row closed: what the delivery looks like before anything is opened.
    for t in ["T1", "T2"] { run.taskOpen[t] = false }
    try render("result-card-all-closed-light", dark: false, width: 1100, ResultCard(run: run, st: st, width: 1100))
    // Every row open: every check, verification, trace gap and risk.
    for t in ["T1", "T2"] { run.taskOpen[t] = true }
    try render("result-card-all-open-light", dark: false, width: 1100, ResultCard(run: run, st: st, width: 1100))
    try render("result-card-all-open-dark", dark: true, width: 1100, ResultCard(run: run, st: st, width: 1100))
    // The narrow layout: numbers wrap under the title instead of being cut off.
    try render("result-card-all-open-narrow-light", dark: false, width: 800, ResultCard(run: run, st: st, width: 800).frame(width: 800))
    print("workbench screenshots: \(dir.path)")
}
