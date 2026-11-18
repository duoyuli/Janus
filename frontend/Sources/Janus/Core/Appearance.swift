import AppKit

/// The app's own appearance: follow macOS, or always light or dark for every Janus window.
/// Saved per user, so it survives quitting; applied at launch from `AppDelegate`.
enum Appearance: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var title: String {
        switch self {
        case .system: return L.text("系统")
        case .light: return L.text("浅色")
        case .dark: return L.text("深色")
        }
    }

    var nsAppearance: NSAppearance? {
        switch self {
        case .system: return nil
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        }
    }

    static var saved: Appearance {
        UserDefaults.standard.string(forKey: "janus.appearance").flatMap(Appearance.init(rawValue:)) ?? .system
    }

    func apply() {
        UserDefaults.standard.set(rawValue, forKey: "janus.appearance")
        NSApp.appearance = nsAppearance
    }
}
