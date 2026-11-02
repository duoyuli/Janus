import Foundation

/// A Markdown document's block structure, for reading deliverables inside the app: headings,
/// paragraphs, nested and task lists, fenced code, quotes, GFM tables, rules and images. Inline
/// marks are left in the text and rendered by `AttributedString(markdown:)`.
// @project-doc docs/architecture/frontend.md#markdown_viewer
enum MDDoc {
    indirect enum Node: Hashable {
        case heading(level: Int, text: String)
        case para(String)
        case list(ordered: Bool, start: Int, items: [Item])
        case code(lang: String, text: String)
        case quote([Node])
        case table(header: [String], align: [Align], rows: [[String]])
        case rule
        case image(alt: String, src: String)
    }

    struct Item: Hashable {
        /// nil for a plain item; true or false for a task item (`- [x]`, `- [ ]`).
        var checked: Bool?
        var blocks: [Node]
    }

    enum Align: Hashable { case leading, center, trailing }

    static func parse(_ text: String) -> [Node] {
        let lines = text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.replacingOccurrences(of: "\t", with: "    ") }
        return blocks(lines)
    }

    /// The id a heading is reached by from a `#fragment` link: lower-cased, spaces to `-`,
    /// punctuation dropped (GitHub's rule, close enough for links inside one document).
    static func slug(_ heading: String) -> String {
        let plain = heading.replacingOccurrences(of: #"[`*_~\[\]]|\]\([^)]*\)"#, with: "", options: .regularExpression)
        return String(plain.lowercased().compactMap { c -> Character? in
            if c == " " { return "-" }
            return c.isLetter || c.isNumber || c == "-" || c == "_" ? c : nil
        })
    }

    // MARK: Blocks

    private static func blocks(_ lines: [String]) -> [Node] {
        var out: [Node] = []
        var i = 0
        while i < lines.count {
            let line = lines[i]
            let t = line.trimmingCharacters(in: .whitespaces)
            if t.isEmpty || t.range(of: #"^<!--.*-->$"#, options: .regularExpression) != nil { i += 1; continue }
            if let fence = fence(line) {
                var code: [String] = []
                i += 1
                while i < lines.count, !closes(lines[i], fence.marker) { code.append(dedent(lines[i], fence.indent)); i += 1 }
                i += 1
                out.append(.code(lang: fence.lang, text: code.joined(separator: "\n")))
            } else if let h = heading(t) {
                out.append(h); i += 1
            } else if isRule(t) {
                out.append(.rule); i += 1
            } else if isQuote(line) {
                var inner: [String] = []
                while i < lines.count {
                    let l = lines[i]
                    if isQuote(l) { inner.append(l.replacingOccurrences(of: #"^\s{0,3}>\s?"#, with: "", options: .regularExpression)) }
                    else if !l.trimmingCharacters(in: .whitespaces).isEmpty, !inner.isEmpty, !startsBlock(l) { inner.append(l) }
                    else { break }
                    i += 1
                }
                out.append(.quote(blocks(inner)))
            } else if i + 1 < lines.count, line.contains("|"), let align = delimiterRow(lines[i + 1]), align.count == cells(line).count {
                let header = cells(line)
                var rows: [[String]] = []
                i += 2
                while i < lines.count, lines[i].contains("|"), !lines[i].trimmingCharacters(in: .whitespaces).isEmpty {
                    rows.append(cells(lines[i])); i += 1
                }
                let n = header.count
                out.append(.table(header: header, align: (0..<n).map { $0 < align.count ? align[$0] : .leading },
                                  rows: rows.map { r in (0..<n).map { $0 < r.count ? r[$0] : "" } }))
            } else if let m = marker(line) {
                let (node, next) = list(lines, from: i, first: m)
                out.append(node); i = next
            } else if let img = image(t) {
                out.append(img); i += 1
            } else {
                var para = [ltrim(line)]
                i += 1
                while i < lines.count {
                    let l = lines[i]
                    let lt = l.trimmingCharacters(in: .whitespaces)
                    // Setext headings: a paragraph underlined with === or ---.
                    if lt.range(of: #"^=+$"#, options: .regularExpression) != nil { out.append(.heading(level: 1, text: joined(para))); para = []; i += 1; break }
                    if lt.range(of: #"^-+$"#, options: .regularExpression) != nil { out.append(.heading(level: 2, text: joined(para))); para = []; i += 1; break }
                    if lt.isEmpty || startsBlock(l) { break }
                    para.append(ltrim(l)); i += 1
                }
                if !para.isEmpty { out.append(.para(joined(para))) }
            }
        }
        return out
    }

    private struct Marker { let indent: Int; let width: Int; let ordered: Bool; let number: Int; let bullet: Character; let rest: String }

    /// A list item's marker: `-`, `*`, `+`, or `1.` / `1)`, followed by a space or the line's end.
    private static func marker(_ line: String) -> Marker? {
        guard let m = groups(line, #"^( *)([-*+]|\d{1,9}[.)])( +|$)(.*)$"#) else { return nil }
        let (indent, mark, gap, rest) = (m[0], m[1], m[2], m[3])
        // `* * *` and `- - -` are rules, not lists.
        if isRule(line.trimmingCharacters(in: .whitespaces)) { return nil }
        let ordered = mark.first?.isNumber ?? false
        // A gap of five or more spaces starts indented content after one space.
        let w = mark.count + (gap.count >= 5 || gap.isEmpty ? 1 : gap.count)
        return Marker(indent: indent.count, width: indent.count + w, ordered: ordered,
                      number: ordered ? Int(mark.dropLast()) ?? 1 : 0, bullet: mark.last ?? "-",
                      rest: gap.count >= 5 ? String(repeating: " ", count: gap.count - 1) + rest : rest)
    }

    private static func list(_ lines: [String], from start: Int, first: Marker) -> (Node, Int) {
        var items: [Item] = []
        var i = start
        var current: Marker? = first
        while let m = current {
            var body = [m.rest]
            i += 1
            current = nil
            var blankRun = 0
            while i < lines.count {
                let l = lines[i]
                if l.trimmingCharacters(in: .whitespaces).isEmpty { blankRun += 1; body.append(""); i += 1; continue }
                let ind = indent(l)
                if ind >= m.width { body.append(dedent(l, m.width)); blankRun = 0; i += 1; continue }
                if let next = marker(l), next.indent < m.width {
                    // A sibling: same kind of list at this item's indentation.
                    if next.ordered == first.ordered, next.ordered || next.bullet == first.bullet, next.indent >= first.indent - 1 { current = next }
                    break
                }
                // Lazy continuation of the item's paragraph.
                if blankRun == 0, !startsBlock(l) { body.append(l.trimmingCharacters(in: .whitespaces)); i += 1; continue }
                break
            }
            while body.last == "" { body.removeLast() }
            var checked: Bool?
            if let first = body.first, let box = groups(first, #"^\[([ xX])\]\s+(.*)$"#) {
                checked = box[0] != " "
                body[0] = box[1]
            }
            items.append(Item(checked: checked, blocks: blocks(body)))
            if current == nil { break }
        }
        // Trailing blank lines belong to no item.
        return (.list(ordered: first.ordered, start: first.number, items: items), i)
    }

    private static func heading(_ t: String) -> Node? {
        guard let m = groups(t, #"^(#{1,6})(?:\s+(.*?))?\s*$"#) else { return nil }
        let text = m[1].replacingOccurrences(of: #"\s+#+$"#, with: "", options: .regularExpression)
        return .heading(level: m[0].count, text: text)
    }

    private static func image(_ t: String) -> Node? {
        guard let m = groups(t, #"^!\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)$"#) else { return nil }
        return .image(alt: m[0], src: m[1])
    }

    private static func isRule(_ t: String) -> Bool {
        t.range(of: #"^(?:(?:\*\s*){3,}|(?:-\s*){3,}|(?:_\s*){3,})$"#, options: .regularExpression) != nil
    }

    private static func isQuote(_ l: String) -> Bool { l.range(of: #"^\s{0,3}>"#, options: .regularExpression) != nil }

    /// Lines that end a paragraph: they start a block of their own.
    private static func startsBlock(_ l: String) -> Bool {
        let t = l.trimmingCharacters(in: .whitespaces)
        if fence(l) != nil || heading(t) != nil || isRule(t) || isQuote(l) { return true }
        // An ordered item only interrupts a paragraph when it starts at 1, as in CommonMark.
        if let m = marker(l), !m.rest.trimmingCharacters(in: .whitespaces).isEmpty { return !m.ordered || m.number == 1 }
        return false
    }

    private struct Fence { let marker: String; let indent: Int; let lang: String }

    private static func fence(_ l: String) -> Fence? {
        guard let m = groups(l, #"^( {0,3})(`{3,}|~{3,})\s*([^`\s]*)[^`]*$"#) else { return nil }
        return Fence(marker: m[1], indent: m[0].count, lang: m[2])
    }

    private static func closes(_ l: String, _ marker: String) -> Bool {
        let t = l.trimmingCharacters(in: .whitespaces)
        guard let c = marker.first, t.count >= marker.count else { return false }
        return t.allSatisfy { $0 == c } && indent(l) <= 3
    }

    private static func delimiterRow(_ l: String) -> [Align]? {
        let t = l.trimmingCharacters(in: .whitespaces)
        guard t.contains("-"), t.range(of: #"^\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?$"#, options: .regularExpression) != nil else { return nil }
        return cells(t).map { c in
            let left = c.hasPrefix(":"), right = c.hasSuffix(":")
            return left && right ? .center : right ? .trailing : .leading
        }
    }

    /// A table row's cells; `\|` stays a literal bar, and pipes inside `code` do not split.
    private static func cells(_ l: String) -> [String] {
        var t = l.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("|") { t.removeFirst() }
        if t.hasSuffix("|"), !t.hasSuffix("\\|") { t.removeLast() }
        var out: [String] = []
        var cur = ""
        var inCode = false
        var prev: Character = " "
        for c in t {
            if c == "`" { inCode.toggle() }
            if c == "|", !inCode, prev != "\\" { out.append(cur); cur = "" } else if c == "|", prev == "\\" { cur.removeLast(); cur.append(c) } else { cur.append(c) }
            prev = c
        }
        out.append(cur)
        return out.map { $0.trimmingCharacters(in: .whitespaces) }
    }

    // MARK: Helpers

    private static func indent(_ l: String) -> Int { l.prefix { $0 == " " }.count }

    private static func dedent(_ l: String, _ n: Int) -> String { String(l.dropFirst(min(n, indent(l)))) }

    private static func ltrim(_ l: String) -> String { String(l.drop { $0 == " " }) }

    private static func rtrim(_ l: String) -> String {
        var s = l
        while s.last == " " { s.removeLast() }
        return s
    }

    /// Soft line breaks join with a space, except between two CJK characters; two trailing
    /// spaces or a backslash make a hard break.
    private static func joined(_ lines: [String]) -> String {
        rtrim(lines.dropFirst().reduce(lines.first ?? "") { acc, l in
            if acc.hasSuffix("  ") { return rtrim(acc) + "\n" + l }
            if acc.hasSuffix("\\") { return String(acc.dropLast()) + "\n" + l }
            let a = rtrim(acc)
            if let x = a.last, let y = l.first, isCJK(x), isCJK(y) { return a + l }
            return a + " " + l
        })
    }

    private static func isCJK(_ c: Character) -> Bool {
        guard let v = c.unicodeScalars.first?.value else { return false }
        return (0x2E80...0x9FFF).contains(v) || (0xF900...0xFAFF).contains(v) || (0xFF00...0xFFEF).contains(v) || (0x3000...0x303F).contains(v)
    }

    private static func groups(_ s: String, _ pattern: String) -> [String]? {
        guard let re = try? NSRegularExpression(pattern: pattern),
              let m = re.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)) else { return nil }
        return (1..<m.numberOfRanges).map { i in Range(m.range(at: i), in: s).map { String(s[$0]) } ?? "" }
    }
}
