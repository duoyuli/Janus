import CoreGraphics

/// How the delivery's acceptance rows pack themselves (W1 F9): a test can assert the rule without
/// a screenshot.
enum BoardLayout {
    /// Under this content width the acceptance rows put the numbers on a second line instead of
    /// truncating them (F9).
    static let rowInlineWidth: CGFloat = 838

    static func deliveryRowStacks(_ width: CGFloat) -> Bool { width < rowInlineWidth }
}
