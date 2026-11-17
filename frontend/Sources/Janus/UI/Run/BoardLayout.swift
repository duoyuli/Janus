import CoreGraphics

/// How the workbench folds and packs itself (W1 F3, F8, F9, U3, U6). The views read these
/// numbers and widths, so a test can assert the layout rules without a screenshot.
enum BoardLayout {
    /// Beside the conversation from this content width up: inspector 340 + spacing 18 + main 480,
    /// the same rule `AdaptiveColumns` uses to stack the inspector under the conversation.
    static let twoColumnWidth: CGFloat = 838
    /// Below this card width the run card's pause/stop move under the title.
    static let runCardInlineWidth: CGFloat = 640

    static func isTwoColumn(_ width: CGFloat) -> Bool { width >= twoColumnWidth }
    static func runCardStacks(_ width: CGFloat) -> Bool { width < runCardInlineWidth }
    /// Under this content width the acceptance rows put the numbers on a second line instead of
    /// truncating them (F9); the same width where the inspector moves below the conversation.
    static func deliveryRowStacks(_ width: CGFloat) -> Bool { width < twoColumnWidth }

    /// The conversation and inspector heights. A run with no tasks and nothing said in the lead's
    /// conversation gets 320; otherwise 0.7 × the window's content height clamped to 420–800. The
    /// inspector uses the same height beside the conversation and 480 when it is stacked below.
    static func cardHeights(contentHeight: CGFloat, twoColumn: Bool, empty: Bool) -> (chat: CGFloat, inspector: CGFloat) {
        let chat: CGFloat = empty ? 320 : min(max(contentHeight * 0.7, 420), 800)
        return (chat, twoColumn ? chat : 480)
    }
}
