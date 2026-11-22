import AppKit
import SwiftUI

/// The same permission checklist is used in Settings › 通用 and after installation or an update.
/// The caller supplies the title and the scroll view.
struct MacPermissionsView: View {
    let permissions: MacPermissions

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                Text(L.text("按任务需要开，用不到的可以先跳过。Janus 会自动检测。"))
                    .font(.callout).foregroundStyle(.secondary)
                Spacer(minLength: 12)
                Button(L.text("重新检查")) { permissions.refresh() }
                    .disabled(!permissions.available)
            }
            if !permissions.available {
                Note(text: L.text("从已安装的 Janus.app 打开这页。源码运行时权限可能记在终端名下。"))
            } else {
                HStack(spacing: 12) {
                    Button(L.text("一键开始授权")) { permissions.beginAll() }
                        .buttonStyle(.primary)
                        .disabled(permissions.current != nil)
                    Text(L.text("逐项打开系统授权入口"))
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button(L.text("在 Finder 中显示 Janus")) { permissions.revealApp() }
                }
                if let error = permissions.error { Note(text: error, tone: .amber) }
                if permissions.flow == .complete {
                    Note(text: permissions.outstanding.isEmpty
                         ? L.text("本次引导已完成，所有权限都已授权。")
                         : L.text("本次引导已结束，仍有未授权的项目，可在需要时继续。"))
                }
            }
            ForEach(MacPermission.allCases) { permission in permissionCard(permission) }
            Text(L.text("这里检查的是 Janus 本身。Agent 由别的终端启动时可能要单独授权；系统权限不改变 Agent 的权限模式。"))
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(L.text("授权后系统要是要求重启，等任务结束再重开 Janus，并开新的 Agent 会话。"))
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear { permissions.refresh() }
    }

    /// Switched on in System Settings but not seen here: either Janus has not been reopened since
    /// (macOS applies some grants to a new process only), or the switch belongs to the Janus before
    /// an update, which macOS treats as another app with the same name.
    private func notDetectedHelp(_ permission: MacPermission) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L.text("系统设置里已经打开，这里仍未检测到？")).font(.callout.weight(.semibold))
            Text(L.text("1. 有的授权要等 Janus 重新打开后才生效。点「重新打开 Janus」。"))
            Text(L.text("2. 还是没检测到？在「系统设置 › 隐私与安全性 › \(permission.title)」里删掉旧的 Janus，再用 + 加回当前的 Janus.app，然后重开 Janus。"))
            Button(L.text("重新打开 Janus")) { permissions.relaunch() }
                .padding(.top, 2)
        }
        .font(.callout)
        .fixedSize(horizontal: false, vertical: true)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 8))
    }

    private func permissionCard(_ permission: MacPermission) -> some View {
        let state = permissions.status(permission)
        let active = permissions.current == permission
        return Card(tint: active ? .accent : nil) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: permission.icon)
                    .font(.title3).foregroundStyle(Color.accent).frame(width: 24)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text(permission.title).font(.headline)
                        Spacer()
                        Pill(state.label, state.tone)
                    }
                    Text(permission.purpose).font(.callout).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if active {
                Divider()
                Text(L.text("去「系统设置 › 隐私与安全性 › \(permission.title)」打开 Janus。列表里没有就点 + 添加当前的 Janus.app，回来会自动检测。"))
                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                if state == .notGranted { notDetectedHelp(permission) }
                HStack {
                    Button(L.text("重新检查")) { permissions.refresh() }.buttonStyle(.primary)
                    Button(L.text("打开系统设置")) { permissions.openSettings(permission) }
                    Spacer()
                    Button(L.text("暂时跳过")) { permissions.skipCurrent() }
                }
            } else {
                HStack {
                    Spacer()
                    if state == .granted {
                        Button(L.text("打开系统设置")) { permissions.openSettings(permission) }
                    } else {
                        Button(permission.prompts ? L.text("申请授权") : L.text("前往授权")) { permissions.begin(permission) }
                    }
                }
                .disabled(!permissions.available || permissions.current != nil)
            }
        }
    }
}

struct MacPermissionReview: View {
    let permissions: MacPermissions

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(L.text("检查 Janus 的系统权限")).font(.display(.title2))
                    Text(L.text("首次安装或签名变化后要重新授权；同签名的更新会保留。"))
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button(L.text("稍后")) { permissions.dismissReview() }
                    .keyboardShortcut(.cancelAction)
            }
            .padding(22)
            Divider()
            ScrollView { MacPermissionsView(permissions: permissions).padding(22) }
            Divider()
            HStack {
                Text(L.text("之后可以在「设置 › 通用 › macOS 权限」接着来。")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(L.text("完成")) { permissions.dismissReview() }
            }
            .padding(16)
        }
        .frame(width: 720, height: 620)
        .background(Color.page)
    }
}
