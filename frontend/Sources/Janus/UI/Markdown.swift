import SwiftUI

/// Light Markdown for agent prose: paragraphs, "- " / "1." lists, headings,
/// **bold**, `code` and [text](target). Links point at local files, so they show as code with the
/// target in the help text.
enum MD {
    enum Block: Hashable { case para(String), heading(String), list([String]) }

    static func blocks(_ text: String) -> [Block] {
        let raw = text.replacingOccurrences(of: "\r", with: "")
        // Agents sometimes send a whole list on one line ("…： - a - b"); split it only then.
        let src = raw.contains("\n") ? raw : raw.replacingOccurrences(of: #"([：:。；;）)])\s+-\s+(?=\S)"#, with: "$1\n- ", options: .regularExpression)
        var out: [Block] = []
        var list: [String]?
        var para: [String] = []
        func flushP() { if !para.isEmpty { out.append(.para(para.joined(separator: " "))) }; para = [] }
        func flushL() { if let l = list { out.append(.list(l)) }; list = nil }
        for line in src.split(separator: "\n", omittingEmptySubsequences: false) {
            let l = line.trimmingCharacters(in: .whitespaces)
            let item = match(l, #"^[-*•]\s+(.*)$"#) ?? match(l, #"^\d+[.、)]\s+(.*)$"#)
            if l.isEmpty { flushP(); flushL() }
            else if let item { flushP(); list = (list ?? []) + [item] }
            else if l.range(of: #"^#{1,6}\s"#, options: .regularExpression) != nil { flushP(); flushL(); out.append(.heading(l.replacingOccurrences(of: #"^#+\s*"#, with: "", options: .regularExpression))) }
            else if list != nil { list![list!.count - 1] += " " + l }
            else { para.append(l) }
        }
        flushP(); flushL()
        return out
    }

    private static func match(_ s: String, _ pattern: String) -> String? {
        guard let re = try? NSRegularExpression(pattern: pattern), let m = re.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)), m.numberOfRanges > 1, let r = Range(m.range(at: 1), in: s) else { return nil }
        return String(s[r])
    }

    /// Inline marks as an AttributedString: bold, code, and local links shown as code.
    static func inline(_ s: String) -> AttributedString {
        var out = AttributedString()
        let pattern = #"`([^`\n]+)`|\[([^\]\n]+)\]\(([^)\s]+)\)|\*\*([^*\n]+?)\*\*"#
        guard let re = try? NSRegularExpression(pattern: pattern) else { return AttributedString(s) }
        let ns = s as NSString
        var pos = 0
        for m in re.matches(in: s, range: NSRange(location: 0, length: ns.length)) {
            if m.range.location > pos { out += AttributedString(ns.substring(with: NSRange(location: pos, length: m.range.location - pos))) }
            if m.range(at: 1).location != NSNotFound {
                var c = AttributedString(ns.substring(with: m.range(at: 1)))
                c.font = .system(.body, design: .monospaced).weight(.regular)
                c.backgroundColor = Color.secondary.opacity(0.12)
                out += c
            } else if m.range(at: 2).location != NSNotFound {
                let label = ns.substring(with: m.range(at: 2)), target = ns.substring(with: m.range(at: 3))
                var c = AttributedString(label)
                c.font = .system(.body, design: .monospaced)
                c.backgroundColor = Color.secondary.opacity(0.12)
                if target.hasPrefix("http"), let u = URL(string: target) { c.link = u } else if target.hasPrefix("/"), let u = URL(string: "file://" + (target.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? target)) { c.link = u }
                out += c
            } else if m.range(at: 4).location != NSNotFound {
                var c = AttributedString(ns.substring(with: m.range(at: 4)))
                c.inlinePresentationIntent = .stronglyEmphasized
                out += c
            }
            pos = m.range.location + m.range.length
        }
        if pos < ns.length { out += AttributedString(ns.substring(from: pos)) }
        return out
    }
}

struct MarkdownText: View {
    let text: String
    var font: Font = .body
    var lineLimit: Int?

    var body: some View {
        let blocks = MD.blocks(text)
        VStack(alignment: .leading, spacing: 7) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, b in
                switch b {
                case .para(let p): Text(MD.inline(p)).font(font)
                case .heading(let h): Text(MD.inline(h)).font(font.weight(.semibold))
                case .list(let items):
                    VStack(alignment: .leading, spacing: 3) {
                        ForEach(Array(items.enumerated()), id: \.offset) { _, it in
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text("•").foregroundStyle(.secondary)
                                Text(MD.inline(it)).font(font).frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                    }
                }
            }
        }
        .lineLimit(lineLimit)
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
