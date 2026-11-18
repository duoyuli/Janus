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
}

/// SwiftUI observes the selection through L.text. Background event derivation reads it too.
@Observable final class LanguageSettings {
    static let shared = LanguageSettings(defaults: .standard)
    private let defaults: UserDefaults
    private let stored: OSAllocatedUnfairLock<AppLanguage>
    private static let key = "janus.language"

    init(defaults: UserDefaults) {
        self.defaults = defaults
        stored = OSAllocatedUnfairLock(initialState: defaults.string(forKey: Self.key).flatMap(AppLanguage.init(rawValue:)) ?? .system)
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
            }
        }
    }
}

extension L {
    private static let appBundle: Bundle? = Bundle.main.localizations.contains("zh-Hans") ? .main : nil
    private static let resources = appBundle ?? Bundle.module
    private static let systemLanguage = appBundle?.preferredLocalizations.first
        ?? Bundle.preferredLocalizations(from: resources.localizations, forPreferences: Locale.preferredLanguages + ["zh-Hans"]).first

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
