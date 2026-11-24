import AppKit
import SwiftUI

/// The update section of Settings › 通用: whether updates install themselves, the version running,
/// whether it is the latest, when it was last checked, and the release notes and install button of
/// a newer one. Everything stays in this section; the update never opens a panel of its own.
// @project-doc docs/architecture/20261031-frontend.md#updates
struct UpdatesView: View {
    @Bindable private var updater = Updater.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 22) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(L.text("自动更新")).font(.headline)
                    Text(L.text("有新版本就自动下载，没有任务和对话在跑时重开换上。"))
                        .font(.callout).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                Toggle(L.text("自动更新"), isOn: $updater.automatic)
                    .labelsHidden().toggleStyle(.switch)
                    .disabled(!updater.enabled)
            }
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
                            Button(L.text("更新到 \(rel.version)")) { Task { await updater.install() } }
                                .buttonStyle(.primary)
                        }
                        if updater.state == .ready {
                            Button(L.text("现在更新")) { updater.installStaged() }
                                .buttonStyle(.primary)
                        }
                        Button(checking ? L.text("正在检查…") : L.text("检查更新")) { Task { await updater.check() } }
                            .disabled(updater.state == .checking || updater.state.isWorking || updater.state == .ready)
                    }
                }
                if let rel = updater.latest {
                    Divider().padding(.vertical, 4)
                    Text(L.text("更新内容")).font(.headline)
                    Text(notes(rel)).font(.callout).textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    if let page = rel.page {
                        Button(L.text("在浏览器中打开发布页")) { NSWorkspace.shared.open(page) }
                            .buttonStyle(.link).font(.callout)
                    }
                    Text(installNote).font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Text(L.text("启动 20 秒后查一次，之后每 5 分钟一次。"))
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var version: String { updater.current == "0" ? L.text("开发版") : updater.current }

    private var checking: Bool { updater.state == .checking }

    private func notes(_ rel: Updater.Release) -> String {
        let text = rel.notes.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? L.text("这个版本没有发布说明。") : text
    }

    private var installNote: String {
        let size = ByteCountFormatter.string(fromByteCount: Int64(updater.downloadSize), countStyle: .file)
        return L.text("将下载 \(size) 并重开 Janus；进行中的任务会中断，重开后可以接着做。")
    }

    /// What the pane says above the buttons, and how it should read.
    private var status: (text: String, tone: Color) {
        guard updater.enabled else {
            return (L.text("这个 Janus 不在应用内更新：从源码运行、本地打包和测试中的都不检查。装上发布的 Janus.app 才会更新。"), .secondary)
        }
        switch updater.state {
        case .idle:
            return (L.text("还没有检查。启动后会自己检查一次。"), .secondary)
        case .checking:
            return (L.text("正在检查…"), .secondary)
        case .upToDate:
            return (L.text("已是最新版本。") + checked, .secondary)
        case .available:
            return (L.text("有新版本 \(updater.latest?.version ?? "")。") + checked, Color.accent)
        case .ready:
            let v = updater.latest?.version ?? ""
            return (updater.automatic
                ? L.text("\(v) 已下载，没有任务和对话在跑时自动重开换上。")
                : L.text("\(v) 已下载，点「现在更新」重开换上。"), Color.accent)
        case .downloading, .verifying, .relaunching:
            return (updater.state.workingText ?? L.text("正在更新…"), .secondary)
        case .failed(let text):
            return (text, Tone.red.color)
        }
    }

    private var checked: String {
        guard let at = updater.checkedAt else { return "" }
        return L.text("检查于 \(L.clock(at.timeIntervalSince1970 * 1000))。")
    }
}
