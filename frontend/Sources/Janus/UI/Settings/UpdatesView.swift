import AppKit
import SwiftUI

/// The update pane (Settings › 更新): the version running, whether it is the latest, when it was
/// last checked, the release notes of a newer one, and a check on demand. The versions, download
/// and signature checks are in `Updater`.
// @project-doc docs/architecture/frontend.md#updates
struct UpdatesView: View {
    private let updater = Updater.shared
    @Local private var notes = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("安装包里的 Janus 在应用内更新：下载新版本、校验签名后替换当前应用并重新打开。")
                    .font(.callout).foregroundStyle(.secondary)
                Card(padding: 22) {
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(verbatim: "Janus \(version)").font(.display(.title3))
                            Text(status.text).font(.callout).foregroundStyle(status.tone)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer(minLength: 12)
                        HStack(spacing: 8) {
                            if updater.latest != nil {
                                Button("更新内容") { notes = true }.help("这一版做了什么")
                            }
                            if case .available = updater.state {
                                Button("更新到 \(updater.latest?.version ?? "")") { updater.offer() }.buttonStyle(.primary)
                            }
                            Button(checking ? "正在检查…" : "检查更新") { Task { await updater.check(manual: true) } }
                                .disabled(updater.state == .checking || updater.state.isWorking)
                        }
                    }
                }
                Text("Janus 启动 20 秒后检查一次，之后每 6 小时一次；菜单栏图标菜单里也有「检查更新…」。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .padding(20)
        }
        .sheet(isPresented: $notes) { notesSheet }
    }

    private var version: String { updater.current == "0" ? "开发版" : updater.current }

    private var checking: Bool { updater.state == .checking }

    /// What the pane says above the buttons, and how it should read.
    private var status: (text: String, tone: Color) {
        guard updater.enabled else {
            return ("从源码运行，不检查更新。安装包里的 Janus.app 才会在应用内更新。", .secondary)
        }
        switch updater.state {
        case .idle:
            return ("还没有检查。启动后会自己检查一次。", .secondary)
        case .checking:
            return ("正在检查…", .secondary)
        case .upToDate:
            return ("已是最新版本。" + checked, .secondary)
        case .available:
            return ("有新版本 \(updater.latest?.version ?? "")。" + checked, Color.accent)
        case .downloading, .verifying, .relaunching:
            return (updater.state.workingText ?? "正在更新…", .secondary)
        case .failed(let text):
            return (text, Tone.red.color)
        }
    }

    private var checked: String {
        guard let at = updater.checkedAt else { return "" }
        return "检查于 \(L.clock(at.timeIntervalSince1970 * 1000))。"
    }

    private var notesSheet: some View {
        let text = updater.latest?.notes.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return VStack(alignment: .leading, spacing: 12) {
            Text(verbatim: "Janus \(updater.latest?.version ?? "") 的更新内容").font(.display(.title3))
            ScrollView {
                Text(text.isEmpty ? "这个版本没有发布说明。" : text)
                    .font(.callout)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.trailing, 6)
            }
            HStack {
                if let page = updater.latest?.page {
                    Button("在浏览器中打开") { NSWorkspace.shared.open(page) }
                }
                Spacer()
                Button("关闭") { notes = false }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(18)
        .frame(minWidth: 480, idealWidth: 560, minHeight: 280, idealHeight: 420)
    }
}
