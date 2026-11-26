import Foundation
import Testing
@testable import Janus

/// Every English string has a Chinese key still written somewhere: in the app (`L.text`), or in
/// the backend, whose Chinese text the app translates when it shows it (`L.catalogText`). A key
/// whose text left both is dead weight that keeps a removed screen's wording around.
///
/// A key matches when its literal pieces appear in that order on one source line. Format
/// specifiers (`%@`, `%lld`) stand for Swift's `\(…)`; numbers stand for the backend's `{CONST}`.
@Test func everyTranslatedStringIsStillUsed() throws {
    let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../..").standardizedFileURL
    var sources: [String] = []
    for (dir, ext) in [("frontend/Sources", "swift"), ("backend/src", "rs")] {
        let base = repo.appendingPathComponent(dir)
        for case let url as URL in FileManager.default.enumerator(at: base, includingPropertiesForKeys: nil)! where url.pathExtension == ext {
            sources.append(try String(contentsOf: url, encoding: .utf8))
        }
    }
    let corpus = sources.joined(separator: "\n") as NSString
    let strings = try String(contentsOf: repo.appendingPathComponent("frontend/Sources/Janus/en.lproj/Localizable.strings"), encoding: .utf8)
    let key = try Regex(#"^"((?:[^"\\]|\\.)*)" = "#)
    let gap = try Regex(#"%(?:\d+\$)?(?:lld|ld|d|@|\.\d+f|f)|[0-9]+"#)
    var unused: [String] = []
    for line in strings.split(separator: "\n") {
        guard let m = line.firstMatch(of: key), let k = m.output[1].substring else { continue }
        let pieces = k.split(separator: gap).map { $0.replacingOccurrences(of: "%%", with: "%") }.filter { !$0.isEmpty }
        guard let longest = pieces.max(by: { $0.count < $1.count }) else { continue }
        // Each line holding the longest piece, until one holds them all in order.
        var used = false
        var from = 0
        while !used {
            let hit = corpus.range(of: longest, options: .literal, range: NSRange(location: from, length: corpus.length - from))
            if hit.location == NSNotFound { break }
            var rest = Substring(corpus.substring(with: corpus.lineRange(for: hit)))
            used = pieces.allSatisfy { p in
                guard let r = rest.range(of: p) else { return false }
                rest = rest[r.upperBound...]
                return true
            }
            from = hit.location + hit.length
        }
        if !used { unused.append(String(k)) }
    }
    #expect(unused.isEmpty, "no source uses: \(unused)")
}
