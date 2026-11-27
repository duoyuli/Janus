import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Where a document window reads its file. A run's documents and what they link to, and an
/// adapter's log, come from the service (`GET /api/runs/{id}/doc`, `/doc/asset`,
/// `/api/adapters/{id}/log`), as any client reads them; only a file the person picked on this Mac
/// is read from the disk.
enum DocSource {
    /// One of the run's Markdown documents or artifacts.
    case run(API, run: String)
    /// A file a run's document links to (`doc`).
    case linked(API, run: String, doc: String)
    /// An adapter's install log (`path` is only its name for the window).
    case adapterLog(API, id: String)
    /// A file the person chose on this Mac.
    case local

    /// What reads `path` from this source.
    func read(_ path: String) async throws -> String {
        switch self {
        case .run(let api, let run): return try await api.text("/api/runs/\(run)/doc?path=\(API.q(path))")
        case .linked(let api, let run, let doc): return try await api.text("/api/runs/\(run)/doc/asset?doc=\(API.q(doc))&path=\(API.q(path))")
        case .adapterLog(let api, let id): return try await api.text("/api/adapters/\(API.q(id))/log")
        case .local: return try String(contentsOfFile: path, encoding: .utf8)
        }
    }

    /// Where a link in document `doc` read from here leads: the same run, through the document.
    func linked(from doc: String) -> DocSource {
        switch self {
        case .run(let api, let run), .linked(let api, let run, _): return .linked(api, run: run, doc: doc)
        case .adapterLog, .local: return .local
        }
    }

    /// The pictures a document read from here shows, by their path, read the same way.
    func image(doc: String) -> (@Sendable (String) async -> Data?)? {
        switch self {
        case .run(let api, let run), .linked(let api, let run, _):
            return { path in try? await api.data("/api/runs/\(run)/doc/asset?doc=\(API.q(doc))&path=\(API.q(path))") }
        case .adapterLog, .local: return nil
        }
    }
}

/// A window showing a file the team produced: an artifact, a deliverable, a document an agent
/// wrote. Markdown is rendered; everything else is shown as text. One window per file: opening
/// it again brings that window forward and reads the file again.
// @project-doc docs/architecture/20261031-frontend.md#markdown_viewer
enum ArtifactWindow {
    @MainActor private static var windows: [String: NSWindow] = [:]

    @MainActor
    static func open(run: RunModel, path: String, title: String) { open(path: path, title: title, source: .run(run.api, run: run.id)) }

    @MainActor
    static func open(path: String, title: String? = nil, source: DocSource = .local) {
        let view = ArtifactView(from: source, path: path)
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
    /// Where the file is read from.
    let from: DocSource
    let path: String
    @Local private var text: String?
    @Local private var error: String?
    @Local private var source = false
    @Local private var generation = 0

    private var markdown: Bool { DocLinks.isMarkdown(path) }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(L.tilde(path)).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).help(path)
                Spacer()
                if markdown {
                    Picker(L.text("显示"), selection: $source) {
                        Text(L.text("预览")).tag(false)
                        Text(L.text("源文本")).tag(true)
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                    .help(L.text("在渲染后的文档和 Markdown 源文本之间切换"))
                }
                Button { generation += 1 } label: { Label(L.text("重新读取"), systemImage: "arrow.clockwise") }
                    .labelStyle(.iconOnly).help(L.text("重新读取文件（Agent 可能还在修改它）"))
                Menu {
                    Button(L.text("复制内容")) { copyText(text ?? "") }.disabled(text == nil)
                    Button(L.text("复制路径")) { copyText(path) }
                    Divider()
                    Button(L.text("在 Finder 中显示")) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
                    Button(L.text("用默认应用打开")) { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
                } label: { Label(L.text("更多"), systemImage: "ellipsis.circle") }
                .menuStyle(.borderlessButton).labelStyle(.iconOnly).fixedSize().help(L.text("复制、在 Finder 中显示或用其他应用打开"))
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            Divider()
            if let text {
                if markdown && !source {
                    MarkdownDocumentView(text: text, file: URL(fileURLWithPath: path), source: from)
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
        do { text = try await from.read(path); error = nil } catch { self.error = error.localizedDescription }
    }
}

/// Links in rendered Markdown and in agent prose: a local Markdown file opens in Janus, a
/// `#heading` in the same document scrolls to it, anything else goes to the system.
enum DocLinks {
    /// A Markdown file, by its name: rendered rather than shown as text.
    static func isMarkdown(_ path: String) -> Bool {
        ["md", "markdown"].contains((path as NSString).pathExtension.lowercased())
    }

    /// A link clicked in a document (`doc`, read from `source`) or in a run's prose (`source` the
    /// run): another Markdown file opens in Janus, read the same way.
    @MainActor
    static func handle(_ url: URL, from doc: URL? = nil, source: DocSource = .local, scrollTo: ((String) -> Void)? = nil) -> OpenURLAction.Result {
        let url = url.absoluteURL
        guard url.isFileURL else { return .systemAction }
        let path = url.standardizedFileURL.path
        if let f = url.fragment(percentEncoded: false), !f.isEmpty, let doc, path == doc.standardizedFileURL.path {
            scrollTo?(MDDoc.slug(f))
            return .handled
        }
        if isMarkdown(path) {
            ArtifactWindow.open(path: path, source: doc.map { source.linked(from: $0.path) } ?? source)
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
    let source: DocSource

    init(text: String, file: URL?, source: DocSource = .local) {
        nodes = MDDoc.parse(text)
        self.file = file
        self.source = source
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                MDBlocks(nodes: nodes, base: file, style: .document)
                    .padding(.horizontal, 32).padding(.vertical, 24)
                    .frame(maxWidth: 880, alignment: .leading)
                    .frame(maxWidth: .infinity)
                    .textSelection(.enabled)
            }
            .environment(\.openURL, OpenURLAction { url in
                DocLinks.handle(url, from: file, source: source) { id in withAnimation { proxy.scrollTo(id, anchor: .top) } }
            })
            .environment(\.docImages, file.flatMap { source.image(doc: $0.path) })
        }
        .background(Color.card)
    }
}
