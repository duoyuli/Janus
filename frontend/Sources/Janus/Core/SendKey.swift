import AppKit
import Observation

/// Which key sends what is typed in a composer (the goal box, a session, a chat): ⌘↩, with ↩
/// starting a new line, or ↩, with ⇧↩ starting a new line. Saved per user in `janus.sendKey` (`SendKeySetting`)
/// (Settings › 通用); every composer and its hints follow the setting at once.
// @project-doc docs/architecture/20261031-frontend.md#send_key
enum SendKey: String, CaseIterable, Identifiable {
    case commandReturn
    case returnKey = "return"

    static let standard = SendKey.commandReturn

    var id: String { rawValue }

    /// The sending shortcut as hints write it.
    var symbol: String { self == .commandReturn ? "⌘↩" : "↩" }

    /// The setting's choice in Settings › 通用.
    var title: String { self == .commandReturn ? L.text("⌘↩ 发送") : L.text("↩ 发送") }

    /// How to start a new line, and what else sends, for the setting's explanation.
    var note: String { self == .commandReturn ? L.text("现在 ↩ 换行。") : L.text("现在 ⇧↩ 换行，⌘↩ 也能发送。") }

    /// Whether a key press sends. Only Return (36) and keypad Enter (76) can; ⌘ with either
    /// always sends, so the habit carries over after switching. With `.returnKey` a bare ↩ sends
    /// too, and ⇧, ⌥ or ⌃ with it leave it to the text view, which starts a new line.
    func sends(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        guard [36, 76].contains(keyCode) else { return false }
        let held = modifiers.intersection([.command, .shift, .option, .control])
        if held.contains(.command) { return true }
        return self == .returnKey && held.isEmpty
    }
}

/// The send key in use, one for the whole app: the settings pane writes it, and every composer
/// and hint reads it here, so a change reaches the ones already open.
@MainActor @Observable final class SendKeySetting {
    static let shared = SendKeySetting(defaults: .janus)
    private static let key = "janus.sendKey"
    private let defaults: UserDefaults

    var value: SendKey {
        didSet { defaults.set(value.rawValue, forKey: Self.key) }
    }

    init(defaults: UserDefaults) {
        self.defaults = defaults
        value = defaults.string(forKey: Self.key).flatMap(SendKey.init(rawValue:)) ?? .standard
    }
}
