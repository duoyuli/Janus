import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// A window showing a file the team produced: an artifact, a deliverable, a document an agent
/// wrote. Markdown is rendered; everything else is shown as text. One window per file: opening
/// it again brings that window forward and reads the file again.
// @project-doc docs/architecture/frontend.md#markdown_viewer
enum ArtifactWindow {
    @MainActor private static var windows: [String: NSWindow] = [:]

    @MainActor
    static func open(run: RunModel, path: String, title: String) { open(path: path, title: title, api: run.api) }

    @MainActor
    static func open(path: String, title: String? = nil, api: API? = nil) {
        let view = ArtifactView(api: api, path: path)
        if let w = windows[path] {
            w.contentView = NSHostingView(rootView: view)
            w.makeKeyAndOrderFront(nil)
            return
        }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 880, height: 720), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        w.title = title ?? (path as NSString).lastPathComponent
        w.representedURL = URL(fileURLWithPath: path)
        w.isReleasedWhenClosed = false
        w.tabbingMode = .preferred
        w.contentView = NSHostingView(rootView: view)
        w.setFrameAutosaveName("JanusDocument")
        if windows.isEmpty { w.center() }
        windows[path] = w
        w.makeKeyAndOrderFront(nil)
    }

    /// File > 打开 Markdown 文件…
    @MainActor
    static func choose() {
        let p = NSOpenPanel()
        p.allowedContentTypes = [UTType(filenameExtension: "md"), UTType(filenameExtension: "markdown"), .plainText].compactMap { $0 }
        p.allowsMultipleSelection = true
        p.canChooseDirectories = false
        guard p.runModal() == .OK else { return }
        for u in p.urls { open(path: u.path) }
    }
}

struct ArtifactView: View {
    let api: API?
    let path: String
    @Local private var text: String?
    @Local private var error: String?
    @Local private var source = false
    @Local private var generation = 0

    private var markdown: Bool { RunDocs.isMarkdown(path) }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(L.tilde(path)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).help(path)
                Spacer()
                if markdown {
                    Picker("显示", selection: $source) {
                        Text("预览").tag(false)
                        Text("源文本").tag(true)
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                    .help("在渲染后的文档和 Markdown 源文本之间切换")
                }
                Button { generation += 1 } label: { Label("重新读取", systemImage: "arrow.clockwise") }
                    .labelStyle(.iconOnly).help("重新读取文件（Agent 可能还在修改它）")
                Menu {
                    Button("复制内容") { copyText(text ?? "") }.disabled(text == nil)
                    Button("复制路径") { copyText(path) }
                    Divider()
                    Button("在 Finder 中显示") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
                    Button("用默认应用打开") { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
                } label: { Label("更多", systemImage: "ellipsis.circle") }
                .menuStyle(.borderlessButton).labelStyle(.iconOnly).fixedSize().help("复制、在 Finder 中显示或用其他应用打开")
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            Divider()
            if let text {
                if markdown && !source {
                    MarkdownDocumentView(text: text, file: URL(fileURLWithPath: path))
                } else {
                    ScrollView([.vertical, .horizontal]) {
                        Text(text).font(.system(size: 12, design: .monospaced)).textSelection(.enabled).padding(12).frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            } else if let error {
                Text(error).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 480, minHeight: 360)
        .task(id: generation) { await load() }
    }

    private func load() async {
        // The app runs as this user: read the file directly. Paths the app cannot read go
        // through the server, which serves only Janus's artifacts directory.
        if let t = try? String(contentsOfFile: path, encoding: .utf8) { text = t; error = nil; return }
        guard let api else { error = "读不了这个文件"; return }
        do { text = try await api.text("/api/file?path=\(API.q(path))"); error = nil } catch { self.error = error.localizedDescription }
    }
}

/// Links in rendered Markdown and in agent prose: a local Markdown file opens in Janus, a
/// `#heading` in the same document scrolls to it, anything else goes to the system.
enum DocLinks {
    @MainActor
    static func handle(_ url: URL, from doc: URL? = nil, scrollTo: ((String) -> Void)? = nil) -> OpenURLAction.Result {
        let url = url.absoluteURL
        guard url.isFileURL else { return .systemAction }
        let path = url.standardizedFileURL.path
        if let f = url.fragment(percentEncoded: false), !f.isEmpty, let doc, path == doc.standardizedFileURL.path {
            scrollTo?(MDDoc.slug(f))
            return .handled
        }
        if RunDocs.isMarkdown(path) {
            ArtifactWindow.open(path: path)
            return .handled
        }
        if FileManager.default.fileExists(atPath: path) {
            NSWorkspace.shared.open(URL(fileURLWithPath: path))
            return .handled
        }
        return .discarded
    }
}

/// A Markdown document laid out for reading.
struct MarkdownDocumentView: View {
    let nodes: [MDDoc.Node]
    let file: URL?

    init(text: String, file: URL?) {
        nodes = MDDoc.parse(text)
        self.file = file
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                MDBlocks(nodes: nodes, base: file)
                    .padding(.horizontal, 32).padding(.vertical, 24)
                    .frame(maxWidth: 880, alignment: .leading)
                    .frame(maxWidth: .infinity)
                    .textSelection(.enabled)
            }
            .environment(\.openURL, OpenURLAction { url in
                DocLinks.handle(url, from: file) { id in withAnimation { proxy.scrollTo(id, anchor: .top) } }
            })
        }
        .background(Color.card)
    }
}

private struct MDBlocks: View {
    let nodes: [MDDoc.Node]
    let base: URL?
    var spacing: CGFloat = 14

    var body: some View {
        VStack(alignment: .leading, spacing: spacing) {
            ForEach(nodes.indices, id: \.self) { i in MDBlock(node: nodes[i], base: base) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct MDBlock: View {
    let node: MDDoc.Node
    let base: URL?

    static let body = Font.system(size: 14)

    var body: some View {
        switch node {
        case .heading(let level, let text):
            let font = Self.heading(level)
            VStack(alignment: .leading, spacing: 6) {
                Text(MDInline.text(text, base: base, font: font)).font(font).foregroundStyle(level >= 5 ? .secondary : .primary)
                if level <= 2 { Divider() }
            }
            .padding(.top, level <= 2 ? 10 : 4)
            .id(MDDoc.slug(text))
        case .para(let text):
            Text(MDInline.text(text, base: base, font: Self.body)).font(Self.body).lineSpacing(3).fixedSize(horizontal: false, vertical: true)
        case .list(let ordered, let start, let items):
            VStack(alignment: .leading, spacing: 5) {
                ForEach(items.indices, id: \.self) { i in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Group {
                            if let checked = items[i].checked {
                                Image(systemName: checked ? "checkmark.square.fill" : "square").foregroundStyle(checked ? Color.accentColor : .secondary)
                                    .accessibilityLabel(checked ? "已完成" : "未完成")
                            } else {
                                Text(verbatim: ordered ? "\(start + i)." : "•").foregroundStyle(.secondary).monospacedDigit()
                            }
                        }
                        .font(Self.body)
                        .frame(minWidth: ordered ? 22 : 12, alignment: .trailing)
                        AnyView(MDBlocks(nodes: items[i].blocks, base: base, spacing: 6))
                    }
                }
            }
        case .code(let lang, let text):
            ScrollView(.horizontal) {
                Text(text).font(.system(size: 12.5, design: .monospaced)).padding(12).fixedSize()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            .overlay(alignment: .topTrailing) {
                HStack(spacing: 6) {
                    if !lang.isEmpty { Text(lang).font(.caption2).foregroundStyle(.tertiary) }
                    Button { copyText(text) } label: { Image(systemName: "doc.on.doc") }
                        .buttonStyle(.borderless).font(.caption).help("复制代码")
                }
                .padding(6)
            }
        case .quote(let nodes):
            AnyView(MDBlocks(nodes: nodes, base: base, spacing: 8))
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
        let font = bold ? Self.body.weight(.semibold) : Self.body
        let a = Self.alignment(align)
        return Text(MDInline.text(s, base: base, font: font)).font(font)
            .multilineTextAlignment(a == .center ? .center : a == .trailing ? .trailing : .leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: Alignment(horizontal: a, vertical: .top))
    }

    static func heading(_ level: Int) -> Font {
        switch level {
        case 1: return .system(size: 26, weight: .bold)
        case 2: return .system(size: 21, weight: .semibold)
        case 3: return .system(size: 17, weight: .semibold)
        case 4: return .system(size: 15, weight: .semibold)
        default: return .system(size: 14, weight: .semibold)
        }
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
