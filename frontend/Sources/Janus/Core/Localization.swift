import Foundation

extension L {
    // Installed apps expose their localizations to macOS. The SwiftPM executable has no app
    // bundle, so it resolves the language against the package's resources instead.
    private static let appBundle: Bundle? = Bundle.main.localizations.contains("zh-Hans") ? .main : nil
    private static let resources = appBundle ?? Bundle.module

    static let locale: Locale = {
        let fallback = resources.developmentLocalization ?? "zh-Hans"
        let preferences = Locale.preferredLanguages + [fallback]
        let language = appBundle?.preferredLocalizations.first
            ?? Bundle.preferredLocalizations(from: resources.localizations, forPreferences: preferences).first
            ?? fallback
        return Locale(identifier: language)
    }()

    private static let bundle: Bundle = {
        if let appBundle { return appBundle }
        return resources.url(forResource: locale.identifier, withExtension: "lproj")
            .flatMap(Bundle.init(url:)) ?? resources
    }()

    static func text(_ value: String.LocalizationValue, bundle: Bundle = bundle) -> String {
        String(localized: value, bundle: bundle)
    }

    /// Janus-owned labels supplied by the service's Agent and resource catalogs.
    static func catalogText(_ value: String) -> String {
        bundle.localizedString(forKey: value, value: value, table: nil)
    }
}
