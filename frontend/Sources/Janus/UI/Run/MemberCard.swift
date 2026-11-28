import SwiftUI

// Who a member of the run is, shown when the pointer rests on its picture or name: its
// responsibility and how it runs (docs/architecture/20261031-frontend.md#run_group).

/// The card: picture, name, what it is doing now when known, its responsibility and how it runs.
struct MemberCard: View {
    let who: RunModel.Who
    /// What it is doing now (the group header's faces know it).
    var now: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Avatar(initials: who.initials, color: who.color, size: 28, role: who.role)
                Text(who.name).font(.headline)
                if who.lead { Tag(text: L.text("负责人"), tone: .blue) }
            }
            if let now, !now.isEmpty { Text(verbatim: now).font(.callout).foregroundStyle(.secondary) }
            if !who.duty.isEmpty {
                Text(verbatim: who.duty).font(.callout).fixedSize(horizontal: false, vertical: true)
            }
            if !who.setup.isEmpty { Text(verbatim: who.setup).font(.caption).foregroundStyle(.secondary) }
        }
        .padding(14)
        .frame(width: 320, alignment: .leading)
    }
}

/// Rest the pointer on it a moment and the member's card shows; leave and it goes.
struct MemberHover: ViewModifier {
    let who: RunModel.Who
    var now: String?
    @Local private var shown = false
    @Local private var wait: Task<Void, Never>?

    /// How long the pointer rests before the card shows (seconds).
    static let delay: UInt64 = 350_000_000

    func body(content: Content) -> some View {
        content
            .contentShape(Rectangle())
            .onHover { inside in
                wait?.cancel()
                if inside {
                    wait = Task { @MainActor in
                        try? await Task.sleep(nanoseconds: Self.delay)
                        if !Task.isCancelled { shown = true }
                    }
                } else {
                    shown = false
                }
            }
            .popover(isPresented: Binding(get: { shown }, set: { shown = $0 }), arrowEdge: .bottom) {
                MemberCard(who: who, now: now)
            }
            .accessibilityHint(Text(verbatim: [who.duty, who.setup].filter { !$0.isEmpty }.joined(separator: "\n")))
    }
}

extension View {
    /// The member's card on a short hover (`MemberHover`); nothing for the person and Janus.
    @ViewBuilder func memberHover(_ who: RunModel.Who, now: String? = nil) -> some View {
        if who.m == .null { self } else { modifier(MemberHover(who: who, now: now)) }
    }
}
