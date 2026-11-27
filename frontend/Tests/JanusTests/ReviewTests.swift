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

