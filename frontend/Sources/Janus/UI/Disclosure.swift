import SwiftUI

/// The fold mark every collapsible row uses: a chevron on a small neutral plate, so it reads as a
/// control and not as a stray glyph, turning down when open.
// @project-doc docs/architecture/20261031-frontend.md#interaction
struct DisclosureChevron: View {
    let open: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Image(systemName: "chevron.right")
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(.secondary)
            .rotationEffect(.degrees(open ? 90 : 0))
            .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: open)
            .frame(width: 18, height: 18)
            .background(Color.primary.opacity(0.09), in: RoundedRectangle(cornerRadius: 5))
            .accessibilityHidden(true)
    }
}

/// Every `DisclosureGroup` in the app: the whole label row folds and unfolds, not only the
/// system's small triangle, and it lights up under the pointer like other clickable rows.
struct RowDisclosureStyle: DisclosureGroupStyle {
    func makeBody(configuration: Configuration) -> some View {
        let open = configuration.isExpanded
        VStack(alignment: .leading, spacing: 0) {
            Button { configuration.isExpanded.toggle() } label: {
                HStack(spacing: 8) {
                    DisclosureChevron(open: open)
                    configuration.label
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 3).padding(.trailing, 4)
                .contentShape(Rectangle())
                .hoverable(6)
            }
            .buttonStyle(.plain)
            .help(open ? L.text("收起") : L.text("展开"))
            .accessibilityValue(open ? L.text("已展开") : L.text("已收起"))
            if open {
                configuration.content.padding(.leading, 26)
            }
        }
    }
}

extension DisclosureGroupStyle where Self == RowDisclosureStyle {
    static var row: RowDisclosureStyle { RowDisclosureStyle() }
}
