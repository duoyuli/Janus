import Foundation
import Testing
@testable import Janus

// Reviewing a delivery (docs/architecture/20261031-frontend.md#review): where the run stands after
// one and what to look at are the server's (`backend/src/run_view/review.rs`), tested there. These
// tests read the served review and the app's own numbers for a delivery.

/// The served review reads back: each stage, and what to look at with who says so and where.
@Test func theServedReviewReadsBack() {
    for (served, conflict, stage) in [("awaiting", false, ReviewStage.awaiting), ("resolving", false, .resolving), ("blocked", true, .blocked(conflict: true)),
                                       ("blocked", false, .blocked(conflict: false)), ("lead_working", false, .leadWorking), ("team_working", false, .teamWorking),
                                       ("accepted", false, .accepted), ("asking", false, .asking), ("held", false, .held), ("stopped", false, .stopped)] {
        #expect(ReviewStage(served: served, conflict: conflict) == stage, "\(served)")
    }
    let r = ServedReview(["stage": "awaiting", "conflict": false, "requests": [["seq": 3, "text": "顺便改一下文案"]], "signals": [
        ["source": "lead", "kind": "decision", "text": "被忽略的文件要带回吗？", "task": .null, "count": 0],
        ["source": "janus", "kind": "trace_gaps", "text": "T2 的结果和工作目录对不上 1 处", "detail": "改了 c.rs", "task": "T2", "count": 1],
    ]])
    #expect(r.stage.reviews && r.requests.map(\.text) == ["顺便改一下文案"])
    #expect(r.signals.first?.source == .lead && r.signals.first?.task == nil)
    #expect(r.signals[1].source == .janus && r.signals[1].detail == "改了 c.rs" && r.signals[1].task == "T2")
}

@Test func evidenceStrengthBandsTheMethods() {
    let checks = ["test", "command", "inspection", "reasoning", "unverified"].map { Check(["criterion": "x", "passed": true, "method": JSON($0)]) }
        + [Check(["criterion": "y", "passed": true])]
    let e = EvidenceStrength(EvidenceSummary(accept: [], checks: checks))
    #expect((e.ran, e.read, e.said) == (2, 1, 3))
    #expect(e.text == expectedCopy("运行 2 · 检查 1 · 只是说法 3", "Ran 2 · Read 1 · Only said 3"))
    let line = ReviewBody.factsLine([], changed: 0)
    #expect(line == expectedCopy("没有子任务，负责人自己完成 · 没有改动文件", "No subtasks, the lead did it itself · No files changed"))
}

/// A subtask's own risks mark its row but do not open it; a failed criterion, a trace gap or an
/// unconfirmed verification does.
@Test func onlyWhatJanusFindsOpensASubtaskRow() {
    let risky = DeliveryAcceptance(["task": "T1", "title": "改", "status": "succeeded", "criteria": 1, "passed": 1, "checks": [["criterion": "一", "passed": true]], "risks": ["没跑全量"]])!
    #expect(!risky.row.needsAttention)
    #expect(risky.row.markers == [expectedCopy("自述风险 1 条", "Self-reported risks: 1")])
    let gap = DeliveryAcceptance(["task": "T2", "title": "写", "status": "succeeded", "criteria": 1, "passed": 1, "traceGaps": ["改了 c.rs"]])!
    #expect(gap.row.needsAttention)
}
