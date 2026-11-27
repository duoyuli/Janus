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
    func sync(_ roles: [Role], api: API) async {
        let want = Dictionary(roles.map { ($0.id, $0.avatar) }, uniquingKeysWith: { a, _ in a })
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
}
