import AppKit
import SwiftUI
import Testing
@testable import Janus

/// Columns that fit keep their content's width, stretched to the width offered.
@Test func tableColumnsThatFitStretchToTheWidth() {
    #expect(MDTableLayout.share([50, 100], 300) == [100, 200])
    #expect(MDTableLayout.share([50, 100], nil) == [50, 100])
    #expect(MDTableLayout.share([50, 100], .infinity) == [50, 100])
}

/// Columns that do not fit share the width: a narrow column keeps its width, the wider ones split
/// the rest by how much more they want, and together they take exactly the width offered.
@Test func tableColumnsThatDoNotFitShareTheWidth() {
    let w = MDTableLayout.share([40, 400, 600], 600)
    #expect(w[0] == 40)
    #expect(abs(w.reduce(0, +) - 600) < 0.001)
    #expect(w[1] >= 200 && w[2] >= 200 && w[2] > w[1])
}

/// An agent's five-column table with long cells, drawn in a narrow message, stays within the width
/// it is offered: `Grid` gave the first column every point and pushed the others out of the view,
/// and the message overflowed the height its row was measured at.
@Test @MainActor func aWideTableStaysWithinItsMessage() {
    let text = """
    | Provider | 名称 | 上游 base_url | 今天成功请求 | 上游返回模型 |
    |---|---|---|---|---|
    | 983 | 企业级 Kiro（laogou 0.35） | api.laogou.org | 257 | 全部 `claude-opus-5-5`，mismatch=false |
    | 984 | 企业级 Kiro（神鸡 0.3) | aishenji.top | 12 | 10 条 `quickwork/claude-opus-5-5`（mismatch=true），2 条 `claude-opus-5-5`（false） |
    """
    let host = NSHostingController(rootView: MarkdownText(text: text))
    for width in [360.0, 520.0] {
        let size = host.sizeThatFits(in: CGSize(width: width, height: 100_000))
        #expect(size.width <= width + 0.5, "the table fits \(width) points (it took \(size.width))")
        // Wrapped cells make it taller than a header and two one-line rows, but never page-tall.
        #expect(size.height > 90 && size.height < 600, "a row is as high as its highest cell (\(size.height))")
    }
}
