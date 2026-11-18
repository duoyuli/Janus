import Foundation
import Security

/// How macOS identifies an app bundle: its designated requirement. With the release certificate it
/// is `identifier "<bundle id>" and certificate leaf = H"…"`, the same for every release, so
/// permissions granted once stay with the app; an ad-hoc signature's is its own `cdhash`, different
/// for every build.
enum CodeSigning {
    nonisolated static func designatedRequirement(_ app: URL) -> (requirement: SecRequirement, text: String)? {
        var code: SecStaticCode?
        var req: SecRequirement?
        var text: CFString?
        guard SecStaticCodeCreateWithPath(app as CFURL, [], &code) == errSecSuccess, let code,
              SecCodeCopyDesignatedRequirement(code, [], &req) == errSecSuccess, let req,
              SecRequirementCopyString(req, [], &text) == errSecSuccess, let text else { return nil }
        return (req, text as String)
    }
}
