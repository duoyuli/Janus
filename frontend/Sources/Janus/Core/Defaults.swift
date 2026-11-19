import Foundation

extension UserDefaults {
    /// Where the app keeps its own view settings (inbox snoozes, chosen workspace, appearance …).
    /// `JANUS_DEFAULTS_SUITE` names a separate store, so the end-to-end test's app never changes
    /// the person's own settings (docs/operations/20261031-testing.md#product_e2e).
    nonisolated(unsafe) static let janus: UserDefaults = ProcessInfo.processInfo.environment["JANUS_DEFAULTS_SUITE"]
        .flatMap { $0.isEmpty ? nil : UserDefaults(suiteName: $0) } ?? .standard
}
