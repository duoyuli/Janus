import AppKit
import SwiftUI

/// `@State` is a macro in recent SDKs whose plugin ships only with Xcode; the property wrapper it
/// expands to builds everywhere (Command Line Tools included), so views use this name.
typealias Local = SwiftUI.State

extension Color {
    init(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        let v = UInt64(s, radix: 16) ?? 0x64748b
        self.init(red: Double((v >> 16) & 0xff) / 255, green: Double((v >> 8) & 0xff) / 255, blue: Double(v & 0xff) / 255)
    }

    /// A color with its own light and dark value, so text keeps its contrast in both appearances.
    init(light: String, dark: String) {
        self.init(nsColor: NSColor(name: nil) { a in
            let v = a.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
            return NSColor(Color(hex: v))
        })
    }

    // The Janus palette: cool slate surfaces in light, and in dark the neutral grays and soft
    // blue accent of Zed's One Dark; near-black (or near-white) for primary actions. Depth comes
    // from tone and hairlines, not from tinted panels and shadows
    // (docs/architecture/20261031-frontend.md#design_tokens).

    /// Signal blue: links, the current stage, "now", work in progress.
    static let accent = Color(light: "#0757c2", dark: "#61afef")
    /// Primary actions: ink on the page, and the text on it.
    static let ink = Color(light: "#15181d", dark: "#e5e9f0")
    static let onInk = Color(light: "#ffffff", dark: "#1e2227")
    /// The content area behind the cards.
    static let page = Color(light: "#f3f5f8", dark: "#1e2227")
    static let card = Color(light: "#ffffff", dark: "#282c34")
    static let soft = Color(light: "#eef1f5", dark: "#1b1f24")
    static let line = Color(light: "#e1e6ec", dark: "#3a3f4a")
    /// The sidebar: one step off the page, in the same family as the cards.
    static let sidebar = Color(light: "#e9edf2", dark: "#21252b")
    /// Team memory's own mark (One Dark's cyan): apart from the shared material's blue and the
    /// Loops' violet, which sit on the same page.
    static let memory = Color(light: "#0b7285", dark: "#56b6c2")
}

extension Font {
    /// Page and section titles: the system serif (New York; Songti for Chinese), the one place
    /// the type has a voice. Everything else stays in the system sans.
    static func display(_ style: Font.TextStyle) -> Font { .system(style, design: .serif).weight(.semibold) }
}

/// The primary action: ink-filled, so it is the darkest (or brightest) thing on the screen
/// without spending the accent color.
struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    @Environment(\.controlSize) private var size
    func makeBody(configuration: Configuration) -> some View {
        let small = size == .small || size == .mini
        configuration.label
            .font(small ? .callout.weight(.medium) : .body.weight(.medium))
            .foregroundStyle(Color.onInk)
            .padding(.horizontal, small ? 9 : 12).padding(.vertical, small ? 3 : 5)
            .background(Color.ink.opacity(configuration.isPressed ? 0.78 : 1), in: RoundedRectangle(cornerRadius: 7))
            .opacity(enabled ? 1 : 0.32)
            .contentShape(RoundedRectangle(cornerRadius: 7))
    }
}

extension ButtonStyle where Self == PrimaryButtonStyle {
    static var primary: PrimaryButtonStyle { PrimaryButtonStyle() }
}

extension Tone {
    var color: Color {
        switch self {
        case .gray: return Color(nsColor: .secondaryLabelColor)
        case .blue: return .accent
        case .green: return Color(light: "#087049", dark: "#98c379")
        case .amber: return Color(light: "#8a5800", dark: "#e5c07b")
        case .red: return Color(light: "#c0283f", dark: "#ef8089")
        case .violet: return Color(light: "#6a42d6", dark: "#c678dd")
        }
    }
}

/// A status or a short label: a colored dot and quiet text on a neutral capsule, so a screen of
/// them reads as information, not as a field of colors.
struct Pill: View {
    let text: String
    var tone: Tone = .gray
    init(_ text: String, _ tone: Tone = .gray) { self.text = text; self.tone = tone }
    var body: some View {
        HStack(spacing: 5) {
            if tone != .gray { Circle().fill(tone.color).frame(width: 6, height: 6).accessibilityHidden(true) }
            Text(text)
        }
        .font(.system(size: 11, weight: .medium).monospacedDigit())
        .lineLimit(1)
        .padding(.horizontal, 7).padding(.vertical, 2.5)
        .foregroundStyle(Color.primary.opacity(0.75))
        .background(Color.secondary.opacity(0.1), in: Capsule())
        .fixedSize()
    }
}

struct Dot: View {
    let tone: Tone
    let text: String
    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(tone.color).frame(width: 7, height: 7)
            Text(text).font(.callout)
        }
    }
}

struct Avatar: View {
    let initials: String
    let color: String
    var size: CGFloat = 22
    /// The digital employee shown: its own picture replaces the initials when it has one.
    var role = ""
    /// A picture to show instead, such as one chosen in the editor and not saved yet.
    var picture: NSImage?

    /// The slate a role gets when it has no color of its own.
    static let neutral: Set<String> = ["", "#64748b", "#94a3b8"]
    static let slate = "#5b6675"
    /// The lead's tile when its role has no color: the one colored member of a team.
    static let lead = "#0b6bcb"

    var body: some View {
        if let picture = picture ?? AvatarImages.shared.image(role) {
            Image(nsImage: picture).resizable().interpolation(.high).scaledToFill()
                .frame(width: size, height: size)
                .clipShape(RoundedRectangle(cornerRadius: size * 0.3))
                .accessibilityHidden(true)
        } else {
            tile
        }
    }

    private var tile: some View {
        let base = Color(hex: Self.neutral.contains(color.lowercased()) ? Self.slate : color)
        // Two CJK characters crowd a small tile; one reads at a glance.
        let cjk = initials.unicodeScalars.first.map { $0.value >= 0x2E80 } ?? false
        let text = cjk && size < 34 ? String(initials.prefix(1)) : initials
        return Text(text)
            .font(.system(size: size * (text.count == 1 ? 0.5 : 0.4), weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(base, in: RoundedRectangle(cornerRadius: size * 0.3))
            .accessibilityHidden(true)
    }
}

struct Card<Content: View>: View {
    var padding: CGFloat = 14
    var tint: Color?
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 10) { content }
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            // A card that needs attention says so with its outline only; its surface stays neutral.
            .background(Color.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(tint.map { $0.opacity(0.5) } ?? Color.line))
    }
}

struct SectionHeader: View {
    let title: String
    var sub: String = ""
    var count: Int?
    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            Text(title).font(.display(.title3))
            if let count { Pill("\(count)", .amber) }
            Spacer()
            if !sub.isEmpty { Text(sub).font(.caption).foregroundStyle(.secondary) }
        }
        .padding(.top, 6)
    }
}

struct KV: View {
    let rows: [(String, String)]
    var mono = false
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(rows.indices, id: \.self) { i in
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(rows[i].0).foregroundStyle(.secondary).font(.callout).frame(width: 72, alignment: .leading)
                    Text(rows[i].1).font(mono ? .system(.callout, design: .monospaced) : .callout).textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }
}

struct Note: View {
    let text: String
    var tone: Tone = .gray
    var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(tone == .gray ? Color.secondary : tone.color)
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background((tone == .gray ? Color.secondary : tone.color).opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// A `#seq` chip that opens the event in the audit log.
struct SeqButton: View {
    let seq: Int?
    var run: RunModel
    var body: some View {
        if let seq {
            Button(String("#\(seq)")) {
                run.replay = nil
                run.auditCat = "all"; run.auditTask = ""; run.auditActor = ""
                run.highlight = seq
                run.page = .audit
            }
            .buttonStyle(.plain)
            .font(.system(size: 11, design: .monospaced))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 5).padding(.vertical, 1)
            .background(Color.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 4))
            .help(L.text("在审计日志中查看这条事件"))
            .accessibilityLabel(L.text("事件 \(seq)，在审计日志中查看"))
        }
    }
}

struct Bar: View {
    let value: Double
    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.secondary.opacity(0.15))
                Capsule().fill(Color.accent).frame(width: g.size.width * max(0, min(1, value / 100)))
            }
        }
        .frame(height: 4)
        .accessibilityElement()
        .accessibilityLabel(L.text("进度"))
        .accessibilityValue("\(Int(max(0, min(100, value))))%")
    }
}

/// A text field that grows with its content, sent with the key chosen in Settings › 通用
/// (`SendKey`, ⌘↩ unless changed). Given `images`, it also takes
/// images: ⌘V pastes a screenshot or image files copied in Finder, images can be dropped on it,
/// and the paperclip picks files. They show as thumbnails above the text until sent. Given a
/// `footer` (the page's send button), the box ends in a bar: the paperclip at its start, the
/// footer after it.
struct Composer<Footer: View>: View {
    @Binding var text: String
    var images: Binding<[PastedImage]>?
    var placeholder: String
    var disabled: Bool
    var minHeight: CGFloat
    /// Each change puts the cursor here (and scrolls to it).
    var focus: Int
    /// Takes the keyboard as it first appears, so the person can type at once.
    var focusOnAppear: Bool
    var onSubmit: () -> Void
    /// Arrow keys, Tab, ↩ and Esc while a list of suggestions is open; true when it took the key.
    var onNavigate: ((ComposerNav) -> Bool)?
    /// Grey text after what is typed, such as a command's arguments still to write.
    var ghost: String?
    var footer: Footer
    @Local private var height: CGFloat = 0
    private var sendKey: SendKey { SendKeySetting.shared.value }

    init(text: Binding<String>, images: Binding<[PastedImage]>? = nil, placeholder: String, disabled: Bool = false, minHeight: CGFloat = 60,
         focus: Int = 0, focusOnAppear: Bool = false, onNavigate: ((ComposerNav) -> Bool)? = nil, ghost: String? = nil,
         onSubmit: @escaping () -> Void, @ViewBuilder footer: () -> Footer) {
        _text = text
        self.onNavigate = onNavigate
        self.ghost = ghost
        self.images = images
        self.placeholder = placeholder
        self.disabled = disabled
        self.minHeight = minHeight
        self.focus = focus
        self.focusOnAppear = focusOnAppear
        self.onSubmit = onSubmit
        self.footer = footer()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let images, !images.wrappedValue.isEmpty { AttachmentStrip(images: images) }
            if Footer.self == EmptyView.self {
                HStack(alignment: .bottom, spacing: 6) {
                    field
                    clip
                }
            } else {
                field
                HStack(spacing: 8) {
                    clip
                    footer
                }
            }
        }
        .padding(10)
        // An inset well: darker than a card, so typing reads as going into the system.
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(Color.line))
        .opacity(disabled ? 0.6 : 1)
    }

    private var field: some View {
        ZStack(alignment: .topLeading) {
            if text.isEmpty {
                Text(placeholder).foregroundStyle(.tertiary).padding(.leading, 5).allowsHitTesting(false).accessibilityHidden(true)
            } else if let ghost, !text.contains("\n") {
                // Laid out like the field's own line: what is typed, invisible, then the grey rest.
                (Text(verbatim: text).foregroundStyle(.clear) + Text(verbatim: ghost).foregroundStyle(.tertiary))
                    .font(.system(size: NSFont.systemFontSize)).lineLimit(1)
                    .padding(.leading, 5).allowsHitTesting(false).accessibilityHidden(true)
            }
            ComposerField(text: $text, height: $height, disabled: disabled, sendKey: sendKey, focus: focus, focusOnAppear: focusOnAppear, onImages: images.map { b in { add($0, to: b) } }, onNavigate: onNavigate, onSubmit: onSubmit)
                .frame(height: max(minHeight, min(max(height, 18), 240)))
                .accessibilityLabel(placeholder)
        }
    }

    @ViewBuilder private var clip: some View {
        if let images {
            Button { pick(into: images) } label: { Image(systemName: "paperclip") }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .disabled(disabled || images.wrappedValue.count >= Attach.maxImages)
                .help(L.text("添加图片（也可以直接 ⌘V 粘贴截图，或把图片拖进来）"))
                .accessibilityLabel(L.text("添加图片"))
        }
    }

    private func add(_ new: [PastedImage], to images: Binding<[PastedImage]>) {
        let room = Attach.maxImages - images.wrappedValue.count
        images.wrappedValue.append(contentsOf: new.prefix(max(0, room)))
        if new.count > room { NSSound.beep() }
    }

    private func pick(into images: Binding<[PastedImage]>) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = true
        panel.message = L.text("选择要附上的图片")
        guard panel.runModal() == .OK else { return }
        add(panel.urls.compactMap { Attach.prepare(try? Data(contentsOf: $0), name: $0.lastPathComponent) }, to: images)
    }
}

extension Composer where Footer == EmptyView {
    init(text: Binding<String>, images: Binding<[PastedImage]>? = nil, placeholder: String, disabled: Bool = false, minHeight: CGFloat = 60,
         focus: Int = 0, onNavigate: ((ComposerNav) -> Bool)? = nil, ghost: String? = nil, onSubmit: @escaping () -> Void) {
        self.init(text: text, images: images, placeholder: placeholder, disabled: disabled, minHeight: minHeight, focus: focus, onNavigate: onNavigate, ghost: ghost, onSubmit: onSubmit) { EmptyView() }
    }
}

/// The keys a composer hands to the suggestions open above it.
enum ComposerNav { case up, down, accept, dismiss }

/// Thumbnails of the images about to be sent, each with a way to take it out.
struct AttachmentStrip: View {
    @Binding var images: [PastedImage]
    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(images) { img in
                    ZStack(alignment: .topTrailing) {
                        Thumb(image: NSImage(data: img.data), size: 56).help(img.name)
                        Button { images.removeAll { $0.id == img.id } } label: {
                            Image(systemName: "xmark.circle.fill").symbolRenderingMode(.palette).foregroundStyle(.white, .black.opacity(0.6))
                        }
                        .buttonStyle(.plain).offset(x: 5, y: -5).help(L.text("移除这张图片")).accessibilityLabel(L.text("移除 \(img.name)"))
                    }
                }
            }
            .padding(.top, 5).padding(.trailing, 5)
        }
    }
}

/// A square image preview; a click opens the file when there is one.
struct Thumb: View {
    let image: NSImage?
    var size: CGFloat = 56
    var body: some View {
        Group {
            if let image { Image(nsImage: image).resizable().aspectRatio(contentMode: .fit).padding(2) }
            else { Image(systemName: "photo").foregroundStyle(.secondary) }
        }
        .frame(width: size, height: size)
        .background(Color.secondary.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.line))
    }
}

/// Images stored with a run (a goal's, a message's), shown small, read from the service
/// (`GET /api/runs/{id}/image`); a click opens one in Preview.
struct StoredImages: View {
    let run: RunModel
    let paths: [String]
    var size: CGFloat = 72
    var body: some View {
        if !paths.isEmpty {
            HStack(spacing: 6) {
                ForEach(paths, id: \.self) { p in StoredImage(run: run, path: p, size: size) }
            }
        }
    }
}

private struct StoredImage: View {
    let run: RunModel
    let path: String
    let size: CGFloat
    @Local private var image: NSImage?

    var body: some View {
        Button { NSWorkspace.shared.open(URL(fileURLWithPath: path)) } label: { Thumb(image: image, size: size) }
            .buttonStyle(.plain).help(L.text("在预览中打开")).accessibilityLabel(L.text("图片 \((path as NSString).lastPathComponent)"))
            .task(id: path) {
                image = (try? await run.api.data("/api/runs/\(run.id)/image?path=\(API.q(path))")).flatMap(NSImage.init(data:))
            }
    }
}

/// The editable text of a `Composer`: an NSTextView, so that paste and drop can take images
/// (a SwiftUI text field only takes text), sent with `sendKey`.
struct ComposerField: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    var disabled: Bool
    var sendKey: SendKey
    var focus = 0
    var focusOnAppear = false
    var onImages: (([PastedImage]) -> Void)?
    var onNavigate: ((ComposerNav) -> Bool)?
    var onSubmit: () -> Void

    final class TextView: NSTextView {
        var onImages: (([PastedImage]) -> Void)?
        var onNavigate: ((ComposerNav) -> Bool)?
        var onSubmit: () -> Void = {}
        var sendKey = SendKey.standard
        /// Take the keyboard once, when first shown in a window.
        var focusWhenShown = false

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard focusWhenShown, let window else { return }
            focusWhenShown = false
            DispatchQueue.main.async { [weak self] in
                // Not from under a sheet, nor from a field the person is already typing in.
                guard let self, window.attachedSheet == nil, !(window.firstResponder is NSTextView) else { return }
                window.makeFirstResponder(self)
            }
        }
        /// Where ⌘V and Edit > Paste read from; a test gives it a pasteboard of its own.
        var pasteboard: NSPasteboard = .general

        override func keyDown(with event: NSEvent) {
            // Suggestions above the field take the arrows, Tab, a bare ↩ and Esc while open.
            if !hasMarkedText(), let onNavigate, let nav = Self.nav(event), onNavigate(nav) { return }
            // The send key sends; anything else edits as usual. An input method's composition is
            // not in the draft yet: ⌘↩ waits for it to be committed, and a bare ↩ goes to the
            // input method, which commits the composition with it.
            guard sendKey.sends(keyCode: event.keyCode, modifiers: event.modifierFlags) else { return super.keyDown(with: event) }
            if !hasMarkedText() { onSubmit() }
            else if !event.modifierFlags.contains(.command) { super.keyDown(with: event) }
        }

        /// What a key does to open suggestions: ↑ ↓ move, Tab and a bare ↩ take one, Esc closes them.
        static func nav(_ event: NSEvent) -> ComposerNav? {
            let mods = event.modifierFlags.intersection([.command, .shift, .option, .control])
            switch event.keyCode {
            case 126 where mods.isEmpty: return .up
            case 125 where mods.isEmpty: return .down
            case 48 where mods.isEmpty, 36 where mods.isEmpty, 76 where mods.isEmpty: return .accept
            case 53: return .dismiss
            default: return nil
            }
        }

        /// Applies text from SwiftUI, except while an input method is composing. The marked
        /// text is already in `string` but has not reached the bindings yet; writing the old
        /// text back would end the composition and drop what the person is typing.
        func syncText(_ text: String) {
            guard string != text, !hasMarkedText() else { return }
            let length = (text as NSString).length
            let caret = selectedRange().location
            string = text
            setSelectedRange(NSRange(location: caret == NSNotFound ? length : min(caret, length), length: 0))
        }

        override func paste(_ sender: Any?) {
            if takeImages(from: pasteboard) { return }
            pasteAsPlainText(sender)
        }

        /// A text view alone refuses an image-only pasteboard, which leaves the Edit > Paste
        /// item disabled, so ⌘V never reaches `paste(_:)`. This field does take images.
        override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
            if takesImages(item) { return true }
            return super.validateUserInterfaceItem(item)
        }

        override func validateMenuItem(_ item: NSMenuItem) -> Bool {
            if takesImages(item) { return true }
            return super.validateMenuItem(item)
        }

        private func takesImages(_ item: any NSValidatedUserInterfaceItem) -> Bool {
            item.action == #selector(paste(_:)) && onImages != nil && isEditable && Attach.hasImages(pasteboard)
        }

        /// What ⌘V does with a pasteboard holding images; false leaves it to the text paste.
        @discardableResult
        func takeImages(from pb: NSPasteboard) -> Bool {
            guard let onImages, isEditable else { return false }
            let images = Attach.images(from: pb)
            if images.isEmpty { return false }
            onImages(images)
            return true
        }

        override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
            if onImages != nil, isEditable, Attach.hasImages(sender.draggingPasteboard) { return .copy }
            return super.draggingEntered(sender)
        }

        override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
            if onImages != nil, isEditable, Attach.hasImages(sender.draggingPasteboard) { return .copy }
            return super.draggingUpdated(sender)
        }

        override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
            if let onImages, isEditable, Attach.hasImages(sender.draggingPasteboard) {
                let images = Attach.images(from: sender.draggingPasteboard)
                if !images.isEmpty { onImages(images); return true }
            }
            return super.performDragOperation(sender)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerField
        /// The `focus` already acted on.
        var focused: Int
        init(_ parent: ComposerField) { self.parent = parent; focused = parent.focus }
        func textDidChange(_ n: Notification) {
            guard let tv = n.object as? NSTextView else { return }
            // Only committed text goes into the draft; a composition belongs to the input method.
            if !tv.hasMarkedText() { parent.text = tv.string }
            measure(tv)
        }
        func measure(_ tv: NSTextView) {
            guard let lm = tv.layoutManager, let tc = tv.textContainer else { return }
            lm.ensureLayout(for: tc)
            let h = ceil(lm.usedRect(for: tc).height)
            if abs(h - parent.height) > 0.5 { DispatchQueue.main.async { self.parent.height = h } }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        let tv = TextView()
        tv.delegate = context.coordinator
        tv.isRichText = false
        tv.importsGraphics = false
        tv.allowsUndo = true
        tv.drawsBackground = false
        tv.font = .systemFont(ofSize: NSFont.systemFontSize)
        tv.textColor = .labelColor
        tv.textContainerInset = .zero
        tv.isAutomaticQuoteSubstitutionEnabled = false
        tv.isAutomaticDashSubstitutionEnabled = false
        tv.isVerticallyResizable = true
        tv.isHorizontallyResizable = false
        tv.autoresizingMask = [.width]
        tv.textContainer?.widthTracksTextView = true
        tv.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        tv.registerForDraggedTypes([.fileURL, .png, .tiff])
        tv.focusWhenShown = focusOnAppear
        scroll.documentView = tv
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let tv = scroll.documentView as? TextView else { return }
        tv.syncText(text)
        tv.isEditable = !disabled
        tv.isSelectable = !disabled
        tv.onImages = onImages
        tv.onSubmit = onSubmit
        tv.onNavigate = onNavigate
        tv.sendKey = sendKey
        context.coordinator.measure(tv)
        if focus != context.coordinator.focused {
            context.coordinator.focused = focus
            DispatchQueue.main.async {
                tv.window?.makeFirstResponder(tv)
                scroll.scrollToVisible(scroll.bounds)
            }
        }
    }
}

func copyText(_ s: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(s, forType: .string)
}

/// Asks before something that cannot be undone. With `dontAskAgainFor`, the alert offers a
/// "不再提醒" checkbox whose state is remembered in the given `UserDefaults` key; once set, the
/// answer is yes without asking.
@MainActor
func confirm(
    _ title: String, _ message: String = "", ok: String = L.text("继续"), destructive: Bool = true,
    dontAskAgainFor key: String? = nil
) -> Bool {
    if let key, UserDefaults.janus.bool(forKey: key) { return true }
    let a = NSAlert()
    a.messageText = title
    a.informativeText = message
    a.alertStyle = destructive ? .warning : .informational
    a.addButton(withTitle: ok)
    // A Chinese "取消" is not recognized as Cancel, so give it Esc (and ⌘.) explicitly.
    a.addButton(withTitle: L.text("取消")).keyEquivalent = "\u{1b}"
    if destructive { a.buttons.first?.hasDestructiveAction = true }
    if key != nil {
        a.showsSuppressionButton = true
        a.suppressionButton?.title = L.text("不再提醒")
    }
    let confirmed = a.runModal() == .alertFirstButtonReturn
    if confirmed, let key, a.suppressionButton?.state == .on {
        UserDefaults.janus.set(true, forKey: key)
    }
    return confirmed
}

/// The one warning before a run and everything Janus kept for it go away. The workspace's files
/// are not touched, so they are not part of the warning's promise.
@MainActor
func confirmDeleteRun(_ title: String) -> Bool {
    let name = title.isEmpty ? L.text("这个任务") : L.text("「\(title)」")
    return confirm(
        L.text("删除任务？"),
        L.text("\(name)的记录、子任务、产物和日志都会删除，找不回来。没提交的改动先存成补丁；你的目录和已合并的结果不受影响。"),
        ok: L.text("删除"),
        dontAskAgainFor: "janus.deleteRunNoConfirm"
    )
}

/// Asks before a chat goes: its record and its scratch directory, files the agent left there included.
@MainActor
func confirmDeleteChat(_ title: String, role: String) -> Bool {
    let name = title.isEmpty ? L.text("这个对话") : L.text("「\(title)」")
    return confirm(
        L.text("删除对话？"),
        L.text("\(name)的聊天记录、日志和专属工作目录（含\(role.isEmpty ? L.text("这名数字员工") : role)留下的文件）都会删除，找不回来；进行中的会话会停止。"),
        ok: L.text("删除")
    )
}

/// Asks before a sheet closes with edits that were not saved.
@MainActor
func confirmDiscard() -> Bool {
    confirm(L.text("放弃未保存的更改？"), L.text("关闭后，这次的修改不会保存。"), ok: L.text("放弃更改"))
}

/// Pointer feedback for custom clickable surfaces (cards, rows, chips): a light tint while hovered.
struct Hoverable: ViewModifier {
    var cornerRadius: CGFloat = 8
    @Local private var on = false
    func body(content: Content) -> some View {
        content
            .overlay(RoundedRectangle(cornerRadius: cornerRadius).fill(Color.primary.opacity(on ? 0.05 : 0)).allowsHitTesting(false))
            .onHover { on = $0 }
    }
}

extension View {
    func hoverable(_ cornerRadius: CGFloat = 8) -> some View { modifier(Hoverable(cornerRadius: cornerRadius)) }
}

/// A small action beside a heading or an item (add, edit, remove): a glyph, not a word. What it
/// does is its hover help and its VoiceOver label, so the page shows a button and not a link.
struct IconButton: View {
    let symbol: String
    let label: String
    var size: CGFloat = 13
    var color: Color = .accent
    var help: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: size, weight: .medium))
                .frame(width: size + 11, height: size + 11)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(color)
        .hoverable(5)
        .help(help ?? label)
        .accessibilityLabel(label)
    }
}

/// A 「?」 beside a card's title: how that part works, shown only when asked for, so the card
/// itself holds only its content.
struct HowItWorks: View {
    let title: String
    let lines: [String]
    @Local private var shown = false

    var body: some View {
        IconButton(symbol: "questionmark.circle", label: L.text("\(title)怎么工作"), size: 12, color: .secondary) { shown.toggle() }
            .popover(isPresented: $shown, arrowEdge: .bottom) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(L.text("\(title)怎么工作")).font(.headline)
                    ForEach(lines, id: \.self) { line in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(verbatim: "·").foregroundStyle(.secondary)
                            Text(line).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                .font(.callout)
                .padding(14)
                .frame(width: 360, alignment: .leading)
            }
    }
}

/// A transient confirmation at the bottom of the window. It uses the system material so it reads
/// in both appearances, and VoiceOver announces it.
struct Toast: View {
    let text: String
    let dismiss: () -> Void
    var body: some View {
        Text(text)
            .font(.callout)
            .padding(.horizontal, 16).padding(.vertical, 10)
            .background(.regularMaterial, in: Capsule())
            .overlay(Capsule().strokeBorder(Color.line))
            .shadow(color: .black.opacity(0.12), radius: 8, y: 2)
            .padding(.bottom, 22)
            .onTapGesture(perform: dismiss)
            .accessibilityAddTraits(.isStaticText)
            .onAppear { AccessibilityNotification.Announcement(text).post() }
            .onChange(of: text) { AccessibilityNotification.Announcement(text).post() }
    }
}

/// A main column with a side column beside it when the window is wide enough, and below it when
/// not, so the main column never gets squeezed into a strip.
struct AdaptiveColumns: Layout {
    var side: CGFloat = 300
    var minMain: CGFloat = 540
    var spacing: CGFloat = 16

    private func wide(_ width: CGFloat) -> Bool { width >= minMain + spacing + side }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard subviews.count == 2 else { return .zero }
        let w = proposal.width ?? (minMain + spacing + side)
        if wide(w) {
            let m = subviews[0].sizeThatFits(.init(width: w - side - spacing, height: nil))
            let s = subviews[1].sizeThatFits(.init(width: side, height: nil))
            return CGSize(width: w, height: max(m.height, s.height))
        }
        let m = subviews[0].sizeThatFits(.init(width: w, height: nil))
        let s = subviews[1].sizeThatFits(.init(width: w, height: nil))
        return CGSize(width: w, height: m.height + spacing + s.height)
    }

    func placeSubviews(in b: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        if wide(b.width) {
            subviews[0].place(at: b.origin, proposal: .init(width: b.width - side - spacing, height: nil))
            subviews[1].place(at: CGPoint(x: b.maxX - side, y: b.minY), proposal: .init(width: side, height: nil))
        } else {
            let m = subviews[0].sizeThatFits(.init(width: b.width, height: nil))
            subviews[0].place(at: b.origin, proposal: .init(width: b.width, height: nil))
            subviews[1].place(at: CGPoint(x: b.minX, y: b.minY + m.height + spacing), proposal: .init(width: b.width, height: nil))
        }
    }
}
