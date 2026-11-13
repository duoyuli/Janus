import AppKit
import UserNotifications

/// System notifications for what newly waits on the person, while Janus is not in front: a
/// request, a decision, a delivered run. Clicking one opens that run. What was already waiting
/// when the app started is not announced again, and nothing is posted while the window is in
/// front: the inbox and the menu bar count show it there.
// @project-doc docs/architecture/frontend.md#notifications
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifier()

    /// Opens a run in the main window; set by the menu bar item, which can open windows.
    var onOpen: ((String) -> Void)?
    /// Keys seen so far; nil until the first look, which only takes note.
    private var seen: Set<String>?

    /// Only a bundled app may post (a binary run from the build directory has no bundle id).
    private var center: UNUserNotificationCenter? {
        Bundle.main.bundleIdentifier == nil ? nil : UNUserNotificationCenter.current()
    }

    /// The inbox as it stands now; anything not seen before is new.
    func update(_ open: [InboxEntry]) {
        let fresh = Self.fresh(open, seen: seen)
        seen = Set(open.map(\.key))
        guard !fresh.isEmpty, !NSApp.isActive, let center else { return }
        center.delegate = self
        let requests = fresh.map { e in
            let c = UNMutableNotificationContent()
            c.title = e.title
            c.body = [L.goalTitle(e.goal), e.text].filter { !$0.isEmpty }.joined(separator: " · ")
            c.sound = .default
            c.userInfo = ["run": e.run]
            return UNNotificationRequest(identifier: e.key, content: c, trigger: nil)
        }
        Task {
            guard (try? await center.requestAuthorization(options: [.alert, .sound])) == true else { return }
            for r in requests { try? await center.add(r) }
        }
    }

    /// Entries not among `seen`; none on the first look.
    nonisolated static func fresh(_ open: [InboxEntry], seen: Set<String>?) -> [InboxEntry] {
        guard let seen else { return [] }
        return open.filter { !seen.contains($0.key) }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        guard let run = response.notification.request.content.userInfo["run"] as? String else { return }
        await MainActor.run { onOpen?(run) }
    }
}
