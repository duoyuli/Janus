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

/// A Return key press with the given modifiers.
private func returnKey(_ flags: NSEvent.ModifierFlags = []) throws -> NSEvent {
    try #require(NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0,
        windowNumber: 0, context: nil, characters: "\r", charactersIgnoringModifiers: "\r",
        isARepeat: false, keyCode: 36))
}

/// Which presses send under each choice in Settings › 通用: ⌘↩ always does; a bare ↩ only
/// when ↩ is the send key, and then ⇧, ⌥ or ⌃ with it starts a new line instead.
@Test func theSendKeyDecidesWhichPressesSend() {
    let enter: UInt16 = 76, letter: UInt16 = 0
    #expect(SendKey.standard == .commandReturn)
    #expect(SendKey.commandReturn.sends(keyCode: 36, modifiers: .command))
    #expect(SendKey.commandReturn.sends(keyCode: enter, modifiers: .command))
    #expect(!SendKey.commandReturn.sends(keyCode: 36, modifiers: []))
    #expect(!SendKey.commandReturn.sends(keyCode: 36, modifiers: .shift))

    #expect(SendKey.returnKey.sends(keyCode: 36, modifiers: []))
    #expect(SendKey.returnKey.sends(keyCode: enter, modifiers: .numericPad))
    #expect(SendKey.returnKey.sends(keyCode: 36, modifiers: .command))
    for flags: NSEvent.ModifierFlags in [.shift, .option, .control] {
        #expect(!SendKey.returnKey.sends(keyCode: 36, modifiers: flags))
    }
    #expect(!SendKey.returnKey.sends(keyCode: letter, modifiers: []))
}

/// With ↩ as the send key, ↩ sends and ⇧↩ starts a new line in the field.
@Test @MainActor func returnSendsWhenItIsTheSendKey() throws {
    let tv = ComposerField.TextView(frame: NSRect(x: 0, y: 0, width: 240, height: 60))
    tv.string = "hello"
    tv.setSelectedRange(NSRange(location: 5, length: 0))
    var sends = 0
    tv.onSubmit = { sends += 1 }

    tv.keyDown(with: try returnKey())
    #expect(sends == 0 && tv.string == "hello\n", "⌘↩ is the send key until changed")

    tv.sendKey = .returnKey
    tv.keyDown(with: try returnKey(.shift))
    #expect(sends == 0 && tv.string == "hello\n\n")
    tv.keyDown(with: try returnKey())
    #expect(sends == 1 && tv.string == "hello\n\n")
    tv.keyDown(with: try returnKey(.command))
    #expect(sends == 2)
}

/// A bare ↩ during an input method's composition commits the composition; it does not send.
@Test @MainActor func returnCommitsACompositionInsteadOfSending() throws {
    let tv = composing("ni")
    tv.sendKey = .returnKey
    var sends = 0
    tv.onSubmit = { sends += 1 }
    tv.keyDown(with: try returnKey())
    #expect(sends == 0)
}

/// The choice is kept on this Mac: a new setting over the same defaults reads it back.
@Test @MainActor func theSendKeyChoiceIsRemembered() throws {
    let suite = "janus-test-send-key-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    #expect(SendKeySetting(defaults: defaults).value == .commandReturn)
    SendKeySetting(defaults: defaults).value = .returnKey
    #expect(SendKeySetting(defaults: defaults).value == .returnKey)
}
