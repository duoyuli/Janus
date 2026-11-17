import AppKit
import Testing
@testable import Janus

/// A composer field holding an input method's composition of `text`.
@MainActor
private func composing(_ text: String) -> ComposerField.TextView {
    let tv = ComposerField.TextView(frame: NSRect(x: 0, y: 0, width: 240, height: 60))
    tv.string = ""
    tv.setMarkedText(text, selectedRange: NSRange(location: (text as NSString).length, length: 0),
                     replacementRange: NSRange(location: NSNotFound, length: 0))
    return tv
}

/// While a pinyin composition is in progress, the bindings still hold the last committed
/// text. A SwiftUI redraw must not write that back over the composition.
@Test @MainActor func aStateUpdateDoesNotSwallowAnInputMethodComposition() {
    let tv = composing("ni")
    #expect(tv.hasMarkedText())

    tv.syncText("")
    #expect(tv.string == "ni")
    #expect(tv.hasMarkedText())

    // Committing replaces the marked text; from then on the bindings are in charge again.
    tv.insertText("你", replacementRange: NSRange(location: NSNotFound, length: 0))
    #expect(!tv.hasMarkedText() && tv.string == "你")
    tv.syncText("你好")
    #expect(tv.string == "你好")
}

/// Text that really changes from outside replaces the field's text but keeps the caret in it.
@Test @MainActor func externalTextKeepsTheCaretInsideTheText() {
    let tv = ComposerField.TextView(frame: NSRect(x: 0, y: 0, width: 240, height: 60))
    tv.string = "hello world"
    tv.setSelectedRange(NSRange(location: 5, length: 0))

    tv.syncText("hello brave world")
    #expect(tv.selectedRange() == NSRange(location: 5, length: 0))

    tv.setSelectedRange(NSRange(location: 17, length: 0))
    tv.syncText("hi")
    #expect(tv.selectedRange() == NSRange(location: 2, length: 0))
}

/// ⌘↩ must wait until the input method commits; otherwise it sends a draft without the
/// characters still being composed.
@Test @MainActor func commandReturnWaitsForTheInputMethod() throws {
    let tv = composing("ni")
    var sends = 0
    tv.onSubmit = { sends += 1 }
    let event = try #require(NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: .command, timestamp: 0,
        windowNumber: 0, context: nil, characters: "\r", charactersIgnoringModifiers: "\r",
        isARepeat: false, keyCode: 36))

    tv.keyDown(with: event)
    #expect(sends == 0)

    tv.unmarkText()
    tv.keyDown(with: event)
    #expect(sends == 1)
}
