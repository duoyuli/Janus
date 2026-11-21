import AppKit
import SwiftUI

/// Markdown in two sizes: a document read in its own window, and agent prose (messages, the
/// goal, the delivery summary) inside the run page. Both parse with `MDDoc`, so tables, code
/// blocks, quotes and nested lists look the same wherever an agent writes them.
// @project-doc docs/architecture/20261031-frontend.md#markdown_viewer
struct MDStyle {
    let body: Font
    let code: Font
    let headings: [Font]
    /// Space between blocks.
    let spacing: CGFloat
    let lineSpacing: CGFloat
    /// Document headings rule off the top two levels and are `#fragment` targets.
    let document: Bool

    func heading(_ level: Int) -> Font { headings[min(max(level, 1), headings.count) - 1] }

    static let document = MDStyle(
        body: .system(size: 14), code: .system(size: 12.5, design: .monospaced),
        headings: [.system(size: 26, weight: .bold), .system(size: 21, weight: .semibold), .system(size: 17, weight: .semibold),
                   .system(size: 15, weight: .semibold), .system(size: 14, weight: .semibold)],
        spacing: 14, lineSpacing: 3, document: true)

    /// Headings stay close to the body size: a message is a few paragraphs, not a page.
    static let prose = MDStyle(
        body: .body, code: .system(.callout, design: .monospaced),
        headings: [.title3.weight(.semibold), .headline, .headline, .body.weight(.semibold), .body.weight(.semibold)],
        spacing: 8, lineSpacing: 2, document: false)
}

/// Agent prose as Markdown. Absolute paths in links resolve to local files, which open in Janus
/// (`DocLinks`, set by the run page).
struct MarkdownText: View {
    let text: String

    private static let root = URL(fileURLWithPath: "/")

    var body: some View {
        MDBlocks(nodes: MD.blocks(text), base: Self.root, style: .prose)
            .textSelection(.enabled)
    }
}

/// Agent prose: its blocks for `MarkdownText`, and inline marks for one-line notes.
enum MD {
    private static let oneLineList = regex(#"([：:。；;）)])\s+-\s+(?=\S)"#)
    private static let marks = regex(#"`([^`\n]+)`|\[([^\]\n]+)\]\(([^)\s]+)\)|\*\*([^*\n]+?)\*\*"#)

    /// Agents sometimes send a whole list on one line ("…： - a - b"); split it only then.
    static func prose(_ text: String) -> String {
        text.contains("\n") ? text : oneLineList.replacing(in: text, with: "$1\n- ")
    }

    /// The blocks of agent prose. A conversation draws its visible messages again on every
    /// event, so a message is parsed once and kept by its text; the cache is bounded.
    static func blocks(_ text: String) -> [MDDoc.Node] {
        let key = text as NSString
        if let hit = parsed.object(forKey: key) { return hit.nodes }
        let nodes = MDDoc.parse(prose(text))
        parsed.setObject(Parsed(nodes), forKey: key, cost: text.utf16.count)
        return nodes
    }

    private final class Parsed {
        let nodes: [MDDoc.Node]
        init(_ nodes: [MDDoc.Node]) { self.nodes = nodes }
    }

    private static let parsed: NSCache<NSString, Parsed> = {
        let c = NSCache<NSString, Parsed>()
        c.countLimit = 500
        c.totalCostLimit = 8 << 20
        return c
    }()

    /// Inline marks of a one-line note (evidence, risks) as an AttributedString: bold, code, and
    /// local links shown as code.
    static func inline(_ s: String) -> AttributedString {
        var out = AttributedString()
        let ns = s as NSString
        var pos = 0
        for m in marks.matches(in: s, range: NSRange(location: 0, length: ns.length)) {
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

struct MDBlocks: View {
    let nodes: [MDDoc.Node]
    let base: URL?
    let style: MDStyle
    var spacing: CGFloat?

    var body: some View {
        VStack(alignment: .leading, spacing: spacing ?? style.spacing) {
            ForEach(nodes.indices, id: \.self) { i in MDBlock(node: nodes[i], base: base, style: style) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct MDBlock: View {
    let node: MDDoc.Node
    let base: URL?
    let style: MDStyle

    private var nested: CGFloat { min(6, style.spacing) }

    var body: some View {
        switch node {
        case .heading(let level, let text):
            let font = style.heading(level)
            let title = VStack(alignment: .leading, spacing: 6) {
                Text(MDInline.text(text, base: base, font: font)).font(font).foregroundStyle(level >= 5 ? .secondary : .primary)
                if style.document && level <= 2 { Divider() }
            }
            if style.document {
                title.padding(.top, level <= 2 ? 10 : 4).id(MDDoc.slug(text))
            } else {
                title.padding(.top, 2)
            }
        case .para(let text):
            Text(MDInline.text(text, base: base, font: style.body)).font(style.body).lineSpacing(style.lineSpacing).fixedSize(horizontal: false, vertical: true)
        case .list(let ordered, let start, let items):
            VStack(alignment: .leading, spacing: nested - 1) {
                ForEach(items.indices, id: \.self) { i in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Group {
                            if let checked = items[i].checked {
                                Image(systemName: checked ? "checkmark.square.fill" : "square").foregroundStyle(checked ? Color.accentColor : .secondary)
                                    .accessibilityLabel(checked ? L.text("已完成") : L.text("未完成"))
                            } else {
                                Text(verbatim: ordered ? "\(start + i)." : "•").foregroundStyle(.secondary).monospacedDigit()
                            }
                        }
                        .font(style.body)
                        .frame(minWidth: ordered ? 22 : 12, alignment: .trailing)
                        AnyView(MDBlocks(nodes: items[i].blocks, base: base, style: style, spacing: nested))
                    }
                }
            }
        case .code(let lang, let text):
            ScrollView(.horizontal) {
                Text(text).font(style.code).padding(12).fixedSize()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            .overlay(alignment: .topTrailing) {
                HStack(spacing: 6) {
                    if !lang.isEmpty { Text(lang).font(.caption2).foregroundStyle(.tertiary) }
                    Button { copyText(text) } label: { Image(systemName: "doc.on.doc") }
                        .buttonStyle(.borderless).font(.caption).help(L.text("复制代码"))
                }
                .padding(6)
            }
        case .quote(let nodes):
            AnyView(MDBlocks(nodes: nodes, base: base, style: style, spacing: nested + 2))
                .foregroundStyle(.secondary)
                .padding(.leading, 14)
                .overlay(alignment: .leading) { RoundedRectangle(cornerRadius: 1.5).fill(Color.secondary.opacity(0.35)).frame(width: 3) }
        case .table(let header, let align, let rows):
            Grid(alignment: .topLeading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(header.indices, id: \.self) { c in
                        cell(header[c], bold: true, align: align[c]).background(Color.secondary.opacity(0.1))
                    }
                }
                ForEach(rows.indices, id: \.self) { r in
                    Divider()
                    GridRow { ForEach(rows[r].indices, id: \.self) { c in cell(rows[r][c], bold: false, align: align[c]) } }
                }
            }
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
            .clipShape(RoundedRectangle(cornerRadius: 6))
        case .rule:
            Divider().padding(.vertical, 4)
        case .image(let alt, let src):
            MDImage(alt: alt, src: src, base: base)
        }
    }

    /// A cell fills its column, so the header's shading and the alignment span the column.
    private func cell(_ s: String, bold: Bool, align: MDDoc.Align) -> some View {
        let font = bold ? style.body.weight(.semibold) : style.body
        let a = Self.alignment(align)
        return Text(MDInline.text(s, base: base, font: font)).font(font)
            .multilineTextAlignment(a == .center ? .center : a == .trailing ? .trailing : .leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: Alignment(horizontal: a, vertical: .top))
    }

    static func alignment(_ a: MDDoc.Align) -> HorizontalAlignment {
        switch a {
        case .leading: return .leading
        case .center: return .center
        case .trailing: return .trailing
        }
    }
}

/// A standalone image: local files relative to the document; web images are left as a link.
private struct MDImage: View {
    let alt: String
    let src: String
    let base: URL?

    var body: some View {
        let url = URL(string: src, relativeTo: base) ?? URL(fileURLWithPath: src)
        if url.isFileURL, let img = NSImage(contentsOf: url) {
            Image(nsImage: img).resizable().scaledToFit()
                .frame(maxWidth: min(img.size.width, 820), alignment: .leading)
                .clipShape(RoundedRectangle(cornerRadius: 6))
                .accessibilityLabel(alt)
                .help(alt)
        } else {
            Link(destination: url.absoluteURL) { Label(alt.isEmpty ? src : alt, systemImage: "photo") }
        }
    }
}

enum MDInline {
    /// Inline Markdown (emphasis, code, links, strikethrough) with links resolved against the
    /// document; code spans take the monospaced form of `font`.
    static func text(_ s: String, base: URL?, font: Font) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace, failurePolicy: .returnPartiallyParsedIfPossible)
        var a = (try? AttributedString(markdown: s, options: options, baseURL: base)) ?? AttributedString(s)
        // Text draws bold and links itself; code, italics and strikethrough are set here.
        let runs = a.runs.compactMap { run in run.inlinePresentationIntent.map { (run.range, $0) } }
        for (r, intent) in runs {
            if intent.contains(.code) {
                a[r].font = font.monospaced()
                a[r].backgroundColor = Color.secondary.opacity(0.14)
            } else if intent.contains(.emphasized) {
                a[r].font = intent.contains(.stronglyEmphasized) ? font.bold().italic() : font.italic()
            }
            if intent.contains(.strikethrough) { a[r].swiftUI.strikethroughStyle = Text.LineStyle(pattern: .solid) }
        }
        return a
    }
}
