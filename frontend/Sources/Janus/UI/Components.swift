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

    /// Clay accent: the Janus brand color. Darker in light mode and lighter in dark mode so text in
    /// it stays above 4.5:1 on cards and on its own tinted pill.
    static let accent = Color(light: "#a5482a", dark: "#e8875f")
    static let card = Color(nsColor: .controlBackgroundColor)
    static let soft = Color(nsColor: .underPageBackgroundColor)
    static let line = Color(nsColor: .separatorColor)
}

extension Tone {
    var color: Color {
        switch self {
        case .gray: return Color(nsColor: .secondaryLabelColor)
        case .blue: return .accent
        case .green: return Color(light: "#0b6e47", dark: "#3fc28a")
        case .amber: return Color(light: "#865606", dark: "#e0a33a")
        case .red: return Color(light: "#ad2f43", dark: "#f0697d")
        case .violet: return Color(light: "#6d45c4", dark: "#a989f0")
        }
    }
}

struct Pill: View {
    let text: String
    var tone: Tone = .gray
    init(_ text: String, _ tone: Tone = .gray) { self.text = text; self.tone = tone }
    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .semibold))
            .lineLimit(1)
            .padding(.horizontal, 7).padding(.vertical, 2.5)
            .foregroundStyle(tone.color)
            .background(tone.color.opacity(0.13), in: Capsule())
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
    var body: some View {
        Text(initials)
            .font(.system(size: size * 0.4, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(Color(hex: color), in: RoundedRectangle(cornerRadius: size * 0.28))
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
            .background(tint.map { $0.opacity(0.07) } ?? Color.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(tint.map { $0.opacity(0.35) } ?? Color.line.opacity(0.6)))
    }
}

struct SectionHeader: View {
    let title: String
    var sub: String = ""
    var count: Int?
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title).font(.title3.weight(.semibold))
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
            .help("在审计日志中查看这条事件")
            .accessibilityLabel("事件 \(seq)，在审计日志中查看")
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
        .accessibilityLabel("进度")
        .accessibilityValue("\(Int(max(0, min(100, value))))%")
    }
}

/// A text field that grows with its content, sent with ⌘↩.
struct Composer: View {
    @Binding var text: String
    var placeholder: String
    var disabled = false
    var minHeight: CGFloat = 60
    var onSubmit: () -> Void
    var body: some View {
        TextField(placeholder, text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .lineLimit(3...12)
            .frame(minHeight: minHeight, alignment: .topLeading)
            .padding(10)
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 9))
            .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(Color.line))
            .disabled(disabled)
            .accessibilityLabel(placeholder)
            .onKeyPress(.return, phases: .down) { press in
                if press.modifiers.contains(.command) { onSubmit(); return .handled }
                return .ignored
            }
    }
}

func copyText(_ s: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(s, forType: .string)
}

/// Asks before something that cannot be undone.
@MainActor
func confirm(_ title: String, _ message: String = "", ok: String = "继续", destructive: Bool = true) -> Bool {
    let a = NSAlert()
    a.messageText = title
    a.informativeText = message
    a.alertStyle = destructive ? .warning : .informational
    a.addButton(withTitle: ok)
    // A Chinese "取消" is not recognized as Cancel, so give it Esc (and ⌘.) explicitly.
    a.addButton(withTitle: "取消").keyEquivalent = "\u{1b}"
    if destructive { a.buttons.first?.hasDestructiveAction = true }
    return a.runModal() == .alertFirstButtonReturn
}

/// Asks before a sheet closes with edits that were not saved.
@MainActor
func confirmDiscard() -> Bool {
    confirm("放弃未保存的更改？", "关闭后，这次的修改不会保存。", ok: "放弃更改")
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
