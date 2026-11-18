import AppKit
import SwiftUI

/// The same permission checklist is used in settings and after installation or an update.
struct MacPermissionsView: View {
    let permissions: MacPermissions
    var showsTitle = true

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 6) {
                        if showsTitle { Text("macOS 权限").font(.display(.title2)) }
                        Text("按 Agent 的任务需要开启。系统授权需要你确认，可以跳过暂时不用的权限。")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 12)
                    Button("重新检查") { permissions.refresh() }
                        .disabled(!permissions.available)
                }
                if !permissions.available {
                    Note(text: "请从已安装的 Janus.app 打开此页。从源码运行时，系统权限可能属于启动它的终端。")
                } else {
                    HStack(spacing: 12) {
                        Button("一键开始授权") { permissions.beginAll() }
                            .buttonStyle(.primary)
                            .disabled(permissions.current != nil)
                        Text("逐项打开系统授权入口")
                            .font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("在 Finder 中显示 Janus") { permissions.revealApp() }
                    }
                    if let error = permissions.error { Note(text: error, tone: .amber) }
                    if permissions.flow == .complete {
                        Note(text: permissions.outstanding.isEmpty
                             ? "本次引导已完成。手动确认的项目仍以系统设置为准。"
                             : "本次引导已结束，仍有未授权或未确认的项目，可在需要时继续。")
                    }
                }
                ForEach(MacPermission.allCases) { permission in permissionCard(permission) }
                Text("本页检查当前 Janus。Agent 由独立服务或其他终端启动时，可能需要给实际执行程序单独授权。系统权限不会改变 Agent 自己的权限模式。")
                    .font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Text("授权后如系统要求重启，请在任务结束后重新打开 Janus，并启动新的 Agent 会话。")
                    .font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(22)
        }
        .onAppear { permissions.refresh() }
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
                        Text("需在系统设置中查看；手动确认不代表应用检测到了授权。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if active {
                Divider()
                Text("在「系统设置 → 隐私与安全性 → \(permission.title)」中允许 Janus。列表里没有时，用添加按钮选择当前 Janus.app。")
                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                HStack {
                    if permission.manual {
                        Button("我已在系统设置中开启") { permissions.confirmCurrent() }
                            .buttonStyle(.primary)
                    } else {
                        Button("重新检查") { permissions.refresh() }.buttonStyle(.primary)
                    }
                    Button("打开系统设置") { permissions.openSettings(permission) }
                    Spacer()
                    Button("暂时跳过") { permissions.skipCurrent() }
                }
            } else {
                HStack {
                    Spacer()
                    if state == .granted {
                        Button("打开系统设置") { permissions.openSettings(permission) }
                    } else {
                        Button(permission.manual ? "前往确认" : "申请授权") { permissions.begin(permission) }
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
                    Text("检查 Janus 的系统权限").font(.display(.title2))
                    Text("首次安装或更新后，已有授权可能需要重新确认。")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button("稍后") { permissions.dismissReview() }
                    .keyboardShortcut(.cancelAction)
            }
            .padding(22)
            Divider()
            MacPermissionsView(permissions: permissions, showsTitle: false)
            Divider()
            HStack {
                Text("之后可从「设置 → macOS 权限」继续。").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("完成") { permissions.dismissReview() }
            }
            .padding(16)
        }
        .frame(width: 720, height: 620)
        .background(Color.page)
    }
}
