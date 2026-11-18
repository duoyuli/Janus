import AppKit
import SwiftUI

/// The same permission checklist is used in Settings › 通用 and after installation or an update.
/// The caller supplies the title and the scroll view.
struct MacPermissionsView: View {
    let permissions: MacPermissions

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                Text(L.text("按 Agent 的任务需要开启。系统授权需要你确认，可以跳过暂时不用的权限。"))
                    .font(.callout).foregroundStyle(.secondary)
                Spacer(minLength: 12)
                Button(L.text("重新检查")) { permissions.refresh() }
                    .disabled(!permissions.available)
            }
            if !permissions.available {
                Note(text: L.text("请从已安装的 Janus.app 打开此页。从源码运行时，系统权限可能属于启动它的终端。"))
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
                         ? L.text("本次引导已完成。手动确认的项目仍以系统设置为准。")
                         : L.text("本次引导已结束，仍有未授权或未确认的项目，可在需要时继续。"))
                }
            }
            ForEach(MacPermission.allCases) { permission in permissionCard(permission) }
            Text(L.text("本页检查当前 Janus。Agent 由独立服务或其他终端启动时，可能需要给实际执行程序单独授权。系统权限不会改变 Agent 自己的权限模式。"))
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(L.text("授权后如系统要求重启，请在任务结束后重新打开 Janus，并启动新的 Agent 会话。"))
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear { permissions.refresh() }
    }

    /// Screen Recording switched on in System Settings but not seen here: either Janus has not been
    /// reopened since (macOS applies the grant to a new process only), or the switch belongs to the
    /// Janus before an update, which macOS treats as another app.
    private func screenRecordingHelp(settings: Bool) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L.text("系统设置里已经打开，这里仍未检测到？")).font(.callout.weight(.semibold))
            Text(L.text("1. macOS 要等 Janus 重新打开后，新的屏幕录制授权才生效。点「重新打开 Janus」。"))
            Text(L.text("2. 重新打开后仍未检测到，说明列表里打开的是更新前的旧 Janus。在「系统设置 → 隐私与安全性 → 屏幕录制」中选中 Janus，点「−」删除，回到这里点「申请授权」重新开启，再重新打开 Janus。"))
            HStack {
                Button(L.text("重新打开 Janus")) { permissions.relaunch() }
                if settings { Button(L.text("打开系统设置")) { permissions.openSettings(.screenRecording) } }
            }
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
                    if permission.manual {
                        Text(L.text("需在系统设置中查看；手动确认不代表应用检测到了授权。"))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if permission == .screenRecording && state == .notGranted {
                        screenRecordingHelp(settings: !active)
                    }
                }
            }
            if active {
                Divider()
                Text(L.text("在「系统设置 → 隐私与安全性 → \(permission.title)」中允许 Janus。列表里没有时，用添加按钮选择当前 Janus.app。"))
                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                HStack {
                    if permission.manual {
                        Button(L.text("我已在系统设置中开启")) { permissions.confirmCurrent() }
                            .buttonStyle(.primary)
                    } else {
                        Button(L.text("重新检查")) { permissions.refresh() }.buttonStyle(.primary)
                    }
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
                        Button(permission.manual ? L.text("前往确认") : L.text("申请授权")) { permissions.begin(permission) }
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
                    Text(L.text("首次安装，或 Janus 的签名身份变化后，需要重新授权。授权之后，同一签名的更新会保留这些权限。"))
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
                Text(L.text("之后可从「设置 → 通用 → macOS 权限」继续。")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(L.text("完成")) { permissions.dismissReview() }
            }
            .padding(16)
        }
        .frame(width: 720, height: 620)
        .background(Color.page)
    }
}
