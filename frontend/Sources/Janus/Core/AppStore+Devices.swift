import Foundation

/// 手机访问: the switch that lets phones on the local network reach Janus, pairing codes for the
/// QR code, and the browsers and phones already paired. Only this app may turn the switch or make
/// a code (docs/operations/20261031-security.md#paired_devices).
extension AppStore {
    /// Read the switch and the paired devices; the error to show, if any.
    func loadDevices() async -> String? {
        do {
            lanAccess = try await api.get("/api/lan-access")
            devices = try await api.get("/api/devices").array
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// Turn phone access on or off; the listener starts or stops at once.
    func setLanAccess(_ on: Bool) async -> String? {
        do {
            lanAccess = try await api.put("/api/lan-access", ["enabled": JSON(on)])
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// A new one-time pairing code with the links to open it at (`{code, expiresAt, urls}`).
    func newPairing() async throws -> JSON {
        try await api.post("/api/devices/pairing")
    }

    /// Unpair a device: its cookie stops working at once.
    func revokeDevice(_ id: String) async {
        do {
            try await api.delete("/api/devices/\(API.q(id))")
            _ = await loadDevices()
        } catch { show(error.localizedDescription) }
    }
}
