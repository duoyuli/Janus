import AppKit

/// Digital employees' own pictures, by role id, as every `Avatar` shows them. A role whose record
/// has no `avatar` shows its initials; one that has gets its picture loaded once per version (the
/// record holds the picture's hash), whenever the roles are read
/// (docs/domains/20261031-agents_and_roles.md#avatars).
@Observable @MainActor
final class AvatarImages {
    static let shared = AvatarImages()

    private(set) var images: [String: NSImage] = [:]
    /// The version of each picture loaded, so an unchanged one is not read again.
    @ObservationIgnored private var loaded: [String: String] = [:]

    func image(_ role: String) -> NSImage? { role.isEmpty ? nil : images[role] }

    /// Follow the roles as read: load new and changed pictures, forget removed ones.
    func sync(_ roles: [JSON], api: API) async {
        let want = Dictionary(roles.map { ($0["id"].str, $0["avatar"].str) }, uniquingKeysWith: { a, _ in a })
        for (id, _) in loaded where (want[id] ?? "").isEmpty {
            loaded[id] = nil
            images[id] = nil
        }
        for (id, hash) in want where !hash.isEmpty && loaded[id] != hash {
            guard let data = try? await api.data("/api/roles/\(API.q(id))/avatar"), let image = NSImage(data: data) else { continue }
            loaded[id] = hash
            images[id] = image
        }
    }

    /// The picture a digital employee gets from an image file: the largest centered square,
    /// scaled to `side` pixels, as PNG. Nil when the data is not an image.
    nonisolated static func square(_ data: Data, side: Int = 256) -> Data? {
        guard let image = NSImage(data: data), let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        let edge = min(cg.width, cg.height)
        guard edge > 0, let crop = cg.cropping(to: CGRect(x: (cg.width - edge) / 2, y: (cg.height - edge) / 2, width: edge, height: edge)),
              let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8, samplesPerPixel: 4,
                                         hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let ctx = NSGraphicsContext(bitmapImageRep: rep) else { return nil }
        ctx.cgContext.interpolationQuality = .high
        ctx.cgContext.draw(crop, in: CGRect(x: 0, y: 0, width: side, height: side))
        return rep.representation(using: .png, properties: [:])
    }
}
