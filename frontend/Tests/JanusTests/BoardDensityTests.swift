import AppKit
import Foundation
import SwiftUI
import Testing
@testable import Janus

// The workbench's density rules (W1 F2, F6, F9): the summary line, the delivery overview and
// acceptance rows, and the width they wrap at, all read here without a screenshot.

// MARK: Run card summary

/// The run card's line puts the subtasks' progress before the server's items, and while the run
/// ticks counts its wall-clock item on from the start; the reading for VoiceOver does the same.
@Test func runCardLinePutsProgressFirstAndCountsTheTime() {
    var l = UsageLedger()
    l.run.durationMs = 59_000
    l.words = UsageWords(hasUsage: true, card: ["用量 15.4K tokens（部分上报）", "耗时 59 秒（总时长）"],
                         voice: ["用量 15,400 tokens（部分上报）", "耗时 59 秒（总时长）"], elapsed: 1)
    let still = l.cardLine(progress: "子任务 2 / 3", at: 2_000_000, since: 1_000_000, ticking: false)
    #expect(still.line == "子任务 2 / 3 · 用量 15.4K tokens（部分上报） · 耗时 59 秒（总时长）")
    #expect(still.voice == "子任务 2 / 3" + L.text("，") + "用量 15,400 tokens（部分上报）" + L.text("，") + "耗时 59 秒（总时长）")
    let ticking = l.cardLine(progress: nil, at: 1_061_000, since: 1_000_000, ticking: true)
    #expect(ticking.line.hasSuffix(L.text("耗时 \(L.text("\(L.dur(61_000))（总时长）"))")))
    #expect(ticking.voice.contains(L.dur(61_000)))
}

@Test func ledgerControlStaysWhileOpenWhereNothingWasReported() {
    // A replayed position before any usage: no control while folded, but an already open ledger
    // keeps the control so it can be folded again.
    let empty = UsageLedger()
    #expect(!empty.showsLedgerControl(open: false))
    #expect(empty.showsLedgerControl(open: true))
    var reported = UsageLedger()
    reported.words.hasUsage = true
    #expect(reported.showsLedgerControl(open: false) && reported.showsLedgerControl(open: true))
}

// MARK: Delivery rows

/// A delivery row reads what the server worked out, and VoiceOver reads it whole.
@Test func deliveryRowReadsTheServedNumbersAndMarkers() {
    let row = DeliveryAcceptance(["task": "T3", "title": "清理旧构建", "status": "succeeded", "criteria": 5, "passed": 4,
                                  "counts": "测试 1 条、命令 2 条", "metrics": "通过 4 / 5 条 · 测试 1 条、命令 2 条",
                                  "markers": ["提醒 1 条"], "needsAttention": true,
                                  "strength": ["ran": 3, "read": 0, "said": 1, "text": "运行 3 · 只是说法 1"],
                                  "verifications": [["task": "V1", "status": "succeeded", "verdict": "rejected", "passed": false]]])
    #expect(row.needsAttention && row.strength.total == 4 && row.verifications.first?.passed == false)
    #expect(row.voice(open: true) == expectedCopy("T3，清理旧构建，通过 4 / 5 条，测试 1 条、命令 2 条，已展开，提醒 1 条", "T3, 清理旧构建, Passed 4 of 5, 测试 1 条、命令 2 条, Expanded, 提醒 1 条"))
}

// MARK: Layout rules

@Test func deliveryRowsStackUnderTheirWidth() {
    #expect(BoardLayout.deliveryRowStacks(837) && !BoardLayout.deliveryRowStacks(838))
}

// MARK: Rendered cards

/// The cards the workbench shows, drawn off screen: the fresh group header and the delivery's default
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
    let freshState = RunState(["run": ["status": "running", "goal": "刚创建的目标", "start": 1_000_000, "now": 1_000_000]], events: fresh)
    try render("group-header-fresh-light", dark: false, width: 1100, GroupHeader(run: freshRun, st: freshState))
    try render("group-header-fresh-dark", dark: true, width: 1100, GroupHeader(run: freshRun, st: freshState))
    try render("group-header-fresh-narrow-light", dark: false, width: 600, GroupHeader(run: freshRun, st: freshState, wide: false).frame(width: 600))

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
                         "verifications": [["task": "V1", "label": "验收没通过", "passed": false, "summary": "没有独立复现", "checks": [], "evidence": [], "risks": []]],
                         "traceGaps": ["T2 声称改了 shared.swift，但 diff 里没有"], "risks": ["没有跑集成测试"]]
    func served(_ record: JSON, _ extra: [String: JSON]) -> JSON {
        var j = record
        for (k, v) in extra { j[k] = v }
        return j
    }
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
    let deliveredData: JSON = [
        "headline": "工作台按新密度拆好了，两个子任务都核对过。",
        "summary": "运行卡片一行摘要，账本和逐条验收页内展开。",
        "decisions": ["旧截图要不要按新折叠位置重拍？"],
        "risks": ["T2：独立验证 V2 判定为「需要修改」，结论没有得到确认"],
        "changed": ["frontend/Sources/Janus/UI/Run/RunView.swift", "docs/architecture/20261031-frontend.md"],
        "leadEdits": ["docs/architecture/20261031-frontend.md"],
        "merge": ["state": "merged", "note": "结果已合并到 /tmp/repo 的分支 main（提交 3f2a91c）。"],
        "deliverables": [["name": "工作台报告", "path": "/tmp/report.md", "seq": 6, "desc": "改动和证据"]],
        "evidence": [["text": "T1：三处调用方", "seq": 5]],
        "acceptance": [accept1, accept2],
    ]
    add("run.delivered", actor: "lead", ts: 1_200_000, data: deliveredData)
    let run = RunModel(id: "delivery", store: testStore)
    run.events = events
    run.replay = events.count
    // The view the server serves for these events: the delivery awaits review, with what to look at.
    let st = RunState([
        "run": ["status": "delivered", "goal": "把工作台的信息密度调好", "start": 1_000_000, "now": 1_200_000],
        "tasks": [["id": "T1", "title": "调研调用方", "agent": "researcher", "status": "succeeded", "accept": ["列出调用方"]],
                  ["id": "T2", "title": "实现一个很长的子任务标题，用来检查单行截断和数字不被截断", "agent": "impl", "status": "pending", "deps": ["T1"], "accept": ["测试通过"]]],
        "deliveries": [["d": deliveredData, "seq": JSON(events.count), "n": 1, "since": 0]],
        // Each task's row as the server writes it from the record.
        "delivery": ["facts": "子任务 1/2 完成 · 验收标准 6/8 达到 · 运行 5 · 检查 1 · 只是说法 1 · 改了 2 个文件", "tasks": [
            served(accept1, ["counts": "测试 2 条、命令 3 条", "reminders": [], "metrics": "通过 5 / 5 条 · 测试 2 条、命令 3 条",
                             "markers": [], "needsAttention": false, "strength": ["ran": 5, "read": 0, "said": 0, "text": "运行 5"]]),
            served(accept2, ["counts": "检查 1 条、推理判断 1 条", "reminders": ["第 3 条「性能不变」是推理判断，不是运行得到的结果"],
                             "metrics": "通过 1 / 3 条 · 检查 1 条、推理判断 1 条", "markers": ["验收没通过", "自述风险 1 条", "对不上 1 处", "提醒 1 条"],
                             "needsAttention": true, "strength": ["ran": 0, "read": 1, "said": 1, "text": "检查 1 · 只是说法 1"]]),
        ]],
        "review": ["stage": "awaiting", "conflict": false, "requests": [], "signals": [
            ["source": "lead", "kind": "decision", "text": "旧截图要不要按新折叠位置重拍？"],
            ["source": "janus", "kind": "risk", "text": "T2：独立验证 V2 判定为「需要修改」，结论没有得到确认", "task": "T2"],
            ["source": "janus", "kind": "failed_checks", "text": "T2 有 2 条验收标准没通过", "task": "T2"],
            ["source": "janus", "kind": "trace_gaps", "text": "T2 的结果和工作目录对不上 1 处", "detail": "T2 声称改了 shared.swift，但 diff 里没有", "task": "T2"],
            ["source": "janus", "kind": "lead_edits", "text": "负责人自己改了 1 个文件，没有子任务验收和独立验证", "detail": "docs/architecture/20261031-frontend.md"],
        ]],
    ], events: events)
    #expect(st.delivery?.tasks.count == 2)
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
