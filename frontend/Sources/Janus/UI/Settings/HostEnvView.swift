import AppKit
import SwiftUI

/// 本机环境, in Settings › 通用 › 权限: every agent's and MCP server's HOME mirrors the person's home
/// folder, so their sign-ins, toolchains and settings work there as in their terminal, except the
/// paths in one list: agents' own settings and sensitive data to begin with, then whatever the
/// person adds or takes out. Janus home is always hidden. Below, a command can be tried as the
/// person and as an agent, to see whether a tool works for agents. Changes apply at once,
/// sessions already running included.
// @project-doc docs/architecture/20261031-frontend.md#host_env
struct HostEnvView: View {
    let store: AppStore
    @Local private var command = ""
    @Local private var trying = false
    /// Why it could not be read, when it could not.
    @Local private var failure: String?

    var body: some View {
        let env = store.hostEnv ?? [:]
        VStack(alignment: .leading, spacing: 18) {
            Text(L.text("Agent 和 MCP 服务器的用户目录是你用户目录的镜像：你本机的登录、钥匙串、工具链和配置，它们都能直接用，刷新的令牌也写回你这里。只有下面列出的路径它们看不到。改动立即生效，正在运行的会话也一样。"))
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if let failure, store.hostEnv == nil {
                HStack {
                    Note(text: L.text("读取本机环境失败：\(failure)"), tone: .red)
                    Button(L.text("重试")) { Task { await load() } }
                }
            } else if store.hostEnv == nil {
                ProgressView().frame(maxWidth: .infinity)
            } else {
                hiddenList(env)
                tryCommand
            }
        }
        .task { await load() }
    }

    private func hiddenList(_ env: JSON) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(L.text("Agent 看不到的路径")).font(.headline)
                Spacer()
                Button(L.text("恢复默认")) { Task { await store.hideFromAgents(nil) } }
                    .buttonStyle(.link)
                    .disabled(env["defaults"].bool == true)
                    .help(L.text("回到 Janus 默认挡住的路径：常见 Agent 的配置和敏感数据"))
            }
            Text(L.text("默认挡住常见 Agent 自己的配置（数字员工的指令、Skill 和 MCP 只由 Janus 给）和一些敏感数据，可以删掉，也可以再加。"))
                .font(.caption).foregroundStyle(.secondary)
            Card(padding: 4) {
                VStack(alignment: .leading, spacing: 0) {
                    if let janus = env["janusHome"].string {
                        row("~/" + janus, label: L.text("Janus 的数据"), here: true, removable: false)
                    }
                    // Only what is on this Mac (most defaults are for Agents not installed here),
                    // and what the person added, so a path they just added does not vanish.
                    ForEach(HostEnv.shown(env["hidden"].array), id: \.self) { h in
                        row("~/" + h["path"].str, label: HostEnv.why(h), here: h["here"].bool == true, removable: true) {
                            Task { await store.hideFromAgents(HostEnv.with(store.hostHidden, h["path"].str, hide: false)) }
                        }
                    }
                    Button(L.text("选择要挡住的文件夹或文件…")) { pick() }
                        .padding(10)
                        .help(L.text("在用户目录里选，可以一次选多个；隐藏的文件和文件夹也显示"))
                }
            }
        }
    }

    private func row(_ path: String, label: String, here: Bool, removable: Bool, remove: (() -> Void)? = nil) -> some View {
        HStack(spacing: 8) {
            Text(verbatim: path).font(.callout.monospaced())
            Pill(label, .gray)
            if !here { Text(L.text("本机没有")).font(.caption).foregroundStyle(.secondary) }
            Spacer()
            if removable, let remove {
                Button(L.text("移除"), action: remove).buttonStyle(.link)
                    .help(L.text("让 Agent 也能用这个路径"))
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
    }

    private var tryCommand: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L.text("试一条命令")).font(.headline)
            Text(L.text("同一条命令在本机和 Agent 的环境里各跑一次（/bin/sh，最多 20 秒），看看某个工具在 Agent 里能不能用。"))
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                TextField(L.text("例如 gh auth status"), text: $command)
                    .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                    .onSubmit(runCommand)
                Button(trying ? L.text("正在试…") : L.text("试一下")) { runCommand() }
                    .disabled(trying || command.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let result = store.hostTry {
                HStack(alignment: .top, spacing: 12) {
                    outcome(L.text("本机"), result["host"])
                    outcome(L.text("Agent"), result["agent"])
                }
            }
        }
    }

    private func outcome(_ title: String, _ r: JSON) -> some View {
        let code = r["code"].int
        return VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title).font(.callout.weight(.semibold))
                Spacer()
                Pill(code.map { $0 == 0 ? L.text("成功") : L.text("退出码 \($0)") } ?? L.text("没有跑完"), code == 0 ? .green : .red)
            }
            ScrollView {
                Text(verbatim: r["output"].str.isEmpty ? L.text("（没有输出）") : r["output"].str)
                    .font(.caption.monospaced()).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 160)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.card, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.line))
    }

    private func load() async {
        let why = await store.loadHostEnv()
        failure = why
    }

    /// The system's open panel, in the home folder with hidden items shown: what is picked is
    /// hidden (the server makes full paths relative to the home folder, and refuses others).
    private func pick() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = true
        panel.showsHiddenFiles = true
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.prompt = L.text("挡住")
        panel.message = L.text("选择 Agent 不该看到的文件夹或文件")
        guard panel.runModal() == .OK else { return }
        let next = panel.urls.reduce(store.hostHidden) { HostEnv.with($0, $1.path, hide: true) }
        Task { await store.hideFromAgents(next) }
    }

    private func runCommand() {
        let c = command.trimmingCharacters(in: .whitespaces)
        guard !c.isEmpty else { return }
        trying = true
        Task {
            _ = await store.tryOnHost(c)
            trying = false
        }
    }
}
