import AppKit
import SwiftUI

/// The update pane (Settings › 更新): the version running, whether it is the latest, when it was
/// last checked, and the release notes and install button of a newer one. Everything stays on
/// this page; the update never opens a panel of its own.
// @project-doc docs/architecture/frontend.md#updates
struct UpdatesView: View {
    private let updater = Updater.shared

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
                            if case .available = updater.state, let rel = updater.latest {
                                Button("更新到 \(rel.version)") { Task { await updater.install() } }
                                    .buttonStyle(.primary)
                            }
                            Button(checking ? "正在检查…" : "检查更新") { Task { await updater.check() } }
                                .disabled(updater.state == .checking || updater.state.isWorking)
                        }
                    }
                    if let rel = updater.latest {
                        Divider().padding(.vertical, 4)
                        Text("更新内容").font(.headline)
                        Text(notes(rel)).font(.callout).textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                        if let page = rel.page {
                            Button("在浏览器中打开发布页") { NSWorkspace.shared.open(page) }
                                .buttonStyle(.link).font(.callout)
                        }
                        Text(installNote).font(.caption).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Text("Janus 启动 20 秒后检查一次，之后每 6 小时一次；菜单栏图标菜单里也有「检查更新…」。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .padding(20)
        }
    }

    private var version: String { updater.current == "0" ? "开发版" : updater.current }

    private var checking: Bool { updater.state == .checking }

    private func notes(_ rel: Updater.Release) -> String {
        let text = rel.notes.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? "这个版本没有发布说明。" : text
    }

    private var installNote: String {
        let size = ByteCountFormatter.string(fromByteCount: Int64(updater.downloadSize), countStyle: .file)
        return "更新会下载 \(size)，然后退出并重新打开 Janus；正在执行的任务会中断，重新打开后可以从中断处继续。"
    }

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
}
