import Foundation
import Observation
import os

enum AppLanguage: String, CaseIterable {
    case system
    case chinese = "zh-Hans"
    case english = "en"

    var title: String {
        switch self {
        case .system: L.text("跟随系统")
        case .chinese: "简体中文"
        case .english: "English"
        }
    }

    /// The app's own language as macOS keeps it (`AppleLanguages` in the app's domain, the one
    /// System Settings › Language & Region › Applications sets): what macOS draws for Janus (the
    /// open panel's buttons and sidebar, the standard menu items) follows it. None follows the system.
    var appleLanguages: [String]? { self == .system ? nil : [rawValue] }
}

/// SwiftUI observes the selection through L.text. Background event derivation reads it too.
/// A choice also becomes the app's language for macOS (`appDomain`), so the parts macOS draws
/// match from the next launch; macOS reads it only when an app starts.
// @project-doc docs/architecture/20261031-frontend.md#ui_language
@Observable final class LanguageSettings {
    /// The end-to-end test's app keeps its settings apart and leaves the app's language alone.
    static let shared = LanguageSettings(defaults: .janus, appDomain: UserDefaults.janus === UserDefaults.standard ? .standard : nil)
    private let defaults: UserDefaults
    private let appDomain: UserDefaults?
    private let stored: OSAllocatedUnfairLock<AppLanguage>
    /// The choice macOS started the app with: what it draws stays in that language until a relaunch.
    let atLaunch: AppLanguage
    private static let key = "janus.language"

    init(defaults: UserDefaults, appDomain: UserDefaults? = nil) {
        self.defaults = defaults
        self.appDomain = appDomain
        atLaunch = defaults.string(forKey: Self.key).flatMap(AppLanguage.init(rawValue:)) ?? .system
        stored = OSAllocatedUnfairLock(initialState: atLaunch)
        // A choice made before Janus kept it for macOS too, or changed in System Settings since.
        if let codes = atLaunch.appleLanguages, appDomain?.stringArray(forKey: "AppleLanguages") != codes { appDomain?.set(codes, forKey: "AppleLanguages") }
    }

    var selection: AppLanguage {
        get {
            access(keyPath: \.selection)
            return stored.withLock { $0 }
        }
        set {
            withMutation(keyPath: \.selection) {
                stored.withLock {
                    $0 = newValue
                    defaults.set(newValue.rawValue, forKey: Self.key)
                }
                if let codes = newValue.appleLanguages { appDomain?.set(codes, forKey: "AppleLanguages") } else { appDomain?.removeObject(forKey: "AppleLanguages") }
            }
        }
    }
}

extension L {
    private static let appBundle: Bundle? = Bundle.main.localizations.contains("zh-Hans") ? .main : nil
    private static let resources = appBundle ?? Bundle.module
    /// The language「跟随系统」means. Janus writes its own choice as the app's language for macOS, so
    /// the app's preferred localization is the system's only when the choice was to follow it;
    /// otherwise it is read from the system-wide languages.
    private static let systemLanguage = LanguageSettings.shared.atLaunch == .system
        ? appBundle?.preferredLocalizations.first ?? preferred(Locale.preferredLanguages)
        : preferred(UserDefaults.standard.persistentDomain(forName: UserDefaults.globalDomain)?["AppleLanguages"] as? [String] ?? Locale.preferredLanguages)

    private static func preferred(_ languages: [String]) -> String? {
        Bundle.preferredLocalizations(from: resources.localizations, forPreferences: languages + ["zh-Hans"]).first
    }

    struct LanguageResources {
        let bundle: Bundle
        let locale: Locale
        let clock, time, day, year, calendarDate: DateFormatter

        init(_ identifier: String) {
            let locale = Locale(identifier: identifier)
            let bundle = resources.url(forResource: identifier, withExtension: "lproj").flatMap(Bundle.init(url:)) ?? resources
            self.locale = locale
            self.bundle = bundle
            func formatter(_ pattern: String.LocalizationValue) -> DateFormatter {
                let formatter = DateFormatter()
                formatter.locale = locale
                formatter.dateFormat = String(localized: pattern, bundle: bundle)
                return formatter
            }
            clock = formatter("HH:mm:ss")
            time = formatter("HH:mm")
            day = formatter("M月d日 HH:mm")
            year = formatter("yyyy年M月d日 HH:mm")
            calendarDate = formatter("yyyy年M月d日")
        }
    }

    private static let english = LanguageResources("en")
    private static let chinese = LanguageResources("zh-Hans")

    static func resources(for selection: AppLanguage) -> LanguageResources {
        switch selection {
        case .english: english
        case .chinese: chinese
        case .system: systemLanguage == "en" ? english : chinese
        }
    }

    static var language: LanguageResources { resources(for: LanguageSettings.shared.selection) }
    static var locale: Locale { language.locale }

    static func text(_ value: String.LocalizationValue, bundle: Bundle = language.bundle) -> String {
        String(localized: value, bundle: bundle)
    }

    /// Janus-owned labels supplied by the service's Agent and resource catalogs.
    static func catalogText(_ value: String) -> String {
        language.bundle.localizedString(forKey: value, value: value, table: nil)
    }
}
