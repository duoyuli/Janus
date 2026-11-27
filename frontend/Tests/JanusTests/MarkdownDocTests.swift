import Foundation
import Testing
@testable import Janus

@Test func markdownBlocksOfADeliverable() {
    let doc = """
    # 3D 扫雷 PRD
    目标用户是
    休闲玩家，
    在浏览器里直接玩。

    ## 功能
    1. 旋转棋盘
    2. 翻开格子
       - 左键翻开
       - 右键插旗
    - [x] F1 已实现
    - [ ] F2 待定

    > 结论：首版只做 5×5×5。
    > 其余以后再说。

    | 编号 | 验收 | 状态 |
    | :--- | --- | ---: |
    | AC1 | 点击翻开 `a|b` | ✓ |
    | AC2 | 插旗 |

    ```js
    const n = 5
    ```

    ---
    ![截图](shots/a.png)
    """
    let nodes = MDDoc.parse(doc)
    #expect(nodes[0] == .heading(level: 1, text: "3D 扫雷 PRD"))
    // Chinese lines join without a space; the rest with one.
    #expect(nodes[1] == .para("目标用户是休闲玩家，在浏览器里直接玩。"))
    #expect(nodes[2] == .heading(level: 2, text: "功能"))
    guard case .list(true, 1, let items) = nodes[3] else { Issue.record("ordered list: \(nodes[3])"); return }
    #expect(items.count == 2)
    #expect(items[1].blocks == [.para("翻开格子"), .list(ordered: false, start: 0, items: [
        .init(checked: nil, blocks: [.para("左键翻开")]), .init(checked: nil, blocks: [.para("右键插旗")]),
    ])])
    #expect(nodes[4] == .list(ordered: false, start: 0, items: [
        .init(checked: true, blocks: [.para("F1 已实现")]), .init(checked: false, blocks: [.para("F2 待定")]),
    ]))
    #expect(nodes[5] == .quote([.para("结论：首版只做 5×5×5。其余以后再说。")]))
    #expect(nodes[6] == .table(header: ["编号", "验收", "状态"], align: [.leading, .leading, .trailing],
                               rows: [["AC1", "点击翻开 `a|b`", "✓"], ["AC2", "插旗", ""]]))
    #expect(nodes[7] == .code(lang: "js", text: "const n = 5"))
    #expect(nodes[8] == .rule)
    #expect(nodes[9] == .image(alt: "截图", src: "shots/a.png"))
    #expect(nodes.count == 10)
}

@Test func markdownEdgeCases() {
    // Setext headings, a rule made of stars, a fence that is never closed, and hard breaks.
    #expect(MDDoc.parse("标题\n===\n\n小节\n---") == [.heading(level: 1, text: "标题"), .heading(level: 2, text: "小节")])
    #expect(MDDoc.parse("* * *") == [.rule])
    #expect(MDDoc.parse("```\nlet a = 1") == [.code(lang: "", text: "let a = 1")])
    #expect(MDDoc.parse("一行  \n下一行") == [.para("一行\n下一行")])
    // A paragraph with a pipe is not a table unless a matching delimiter row follows.
    #expect(MDDoc.parse("a | b\n---") == [.heading(level: 2, text: "a | b")])
    #expect(MDDoc.slug("2. 验收标准 `AC`") == "2-验收标准-ac")
}
