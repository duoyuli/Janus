import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// Settings › 手机访问: let phones on the local network reach Janus, pair a phone by its QR code
/// (a fresh one-time code each time it is shown, renewed before it expires), open the web client
/// in this Mac's browser already paired, and unpair what was paired.
// @project-doc docs/architecture/20261031-frontend.md#mobile_access
struct MobileAccessView: View {
    let store: AppStore
    @Local private var failure: String?
    @Local private var pairing: JSON?
    @Local private var busy = false

    private var access: JSON { store.lanAccess ?? .null }
    private var on: Bool { access["enabled"].bool ?? false }
    private var listening: Bool { access["listening"].bool ?? false }
    /// The link the QR code opens: the first network address while the switch is on.
    private var link: String? {
        listening ? pairing?["urls"].array.first?.string : nil
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text(L.text("手机访问"))
                    .font(.display(.title))
                    .accessibilityAddTraits(.isHeader)
                HStack(spacing: 22) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(L.text("允许局域网里的手机访问")).font(.headline)
                        Text(L.text("打开后，同一网络里的手机扫码配对后能用 Janus 的 Web 版。局域网里是明文传输，只在可信的网络上打开；在外面用 Tailscale 或 VPN 连回来。"))
                            .font(.callout).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 0)
                    Toggle(L.text("允许局域网里的手机访问"), isOn: Binding(get: { on }, set: { v in Task { await toggle(v) } }))
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .disabled(busy || store.lanAccess == nil)
                }
                if let error = access["error"].string, !error.isEmpty {
                    Note(text: error, tone: .red)
                }
                if let failure {
                    Note(text: failure, tone: .red)
                }
                if listening {
                    qrSection
                }
                Divider()
                HStack(spacing: 22) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(L.text("在这台 Mac 的浏览器里用")).font(.headline)
                        Text(L.text("用默认浏览器打开 Web 版，并配对这个浏览器。"))
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 0)
                    Button(L.text("在浏览器中打开")) { Task { await openInBrowser() } }
                }
                Divider()
                Text(L.text("已配对的设备")).font(.headline)
                if store.devices.isEmpty {
                    Text(L.text("还没有配对的设备")).font(.callout).foregroundStyle(.secondary)
                } else {
                    VStack(spacing: 0) {
                        ForEach(Array(store.devices.enumerated()), id: \.offset) { i, d in
                            if i > 0 { Divider() }
                            deviceRow(d)
                        }
                    }
                }
            }
            .padding(20)
        }
        .task { failure = await store.loadDevices() }
        .task(id: listening) { await keepCodeFresh() }
    }

    private var qrSection: some View {
        HStack(alignment: .top, spacing: 22) {
            Group {
                if let link, let image = Self.qr(link) {
                    Image(nsImage: image)
                        .interpolation(.none)
                        .resizable()
                        .frame(width: 180, height: 180)
                        .accessibilityLabel(L.text("配对二维码"))
                } else {
                    ProgressView().frame(width: 180, height: 180)
                }
            }
            .padding(10)
            .background(Color.white, in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 8) {
                Text(L.text("用手机的相机扫码")).font(.headline)
                Text(L.text("手机和这台 Mac 要在同一个网络里。二维码只能用一次，几分钟后自动换一个。"))
                    .font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let link {
                    Text(verbatim: link)
                        .font(.callout.monospaced())
                        .textSelection(.enabled)
                }
                // The other network addresses (a Tailscale one, a second interface); the last one is this Mac's own.
                let more = Array((pairing?["urls"].array ?? []).compactMap(\.string).dropFirst().dropLast())
                if !more.isEmpty {
                    Text(L.text("扫不上时，在手机浏览器里打开其他地址之一："))
                        .font(.callout).foregroundStyle(.secondary)
                    ForEach(more, id: \.self) { Text(verbatim: $0).font(.caption.monospaced()).textSelection(.enabled) }
                }
            }
        }
    }

    private func deviceRow(_ d: JSON) -> some View {
        let now = L.nowMs()
        return HStack(spacing: 12) {
            Image(systemName: "iphone").foregroundStyle(.secondary).frame(width: 18)
            VStack(alignment: .leading, spacing: 3) {
                Text(verbatim: d["name"].str)
                Text(L.text("配对于 \(L.ago(d["createdAt"].double ?? 0, now)) · 最近使用 \(L.ago(d["lastSeenAt"].double ?? 0, now))"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            Button(L.text("取消配对")) { Task { await store.revokeDevice(d["id"].str) } }
        }
        .padding(.vertical, 8)
    }

    private func toggle(_ value: Bool) async {
        busy = true
        defer { busy = false }
        failure = await store.setLanAccess(value)
    }

    /// While the network listener runs, show a fresh code, and a new one shortly before it expires.
    private func keepCodeFresh() async {
        guard listening else { pairing = nil; return }
        while !Task.isCancelled {
            do {
                let p = try await store.newPairing()
                pairing = p
                let left = (p["expiresAt"].double ?? 0) - L.nowMs() - 20_000
                try await Task.sleep(nanoseconds: UInt64(max(5_000, left)) * 1_000_000)
            } catch is CancellationError {
                return
            } catch {
                failure = error.localizedDescription
                return
            }
        }
    }

    /// Open the web client in the default browser on this Mac's own address, with a code to pair it.
    private func openInBrowser() async {
        do {
            let p = try await store.newPairing()
            if let url = p["urls"].array.last?.string.flatMap(URL.init(string:)) {
                NSWorkspace.shared.open(url)
            }
        } catch { failure = error.localizedDescription }
    }

    /// `text` as a QR code, drawn crisp at any size.
    static func qr(_ text: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let out = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
              let cg = CIContext().createCGImage(out, from: out.extent) else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: out.extent.width, height: out.extent.height))
    }
}
