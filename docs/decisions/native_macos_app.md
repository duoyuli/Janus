# ADR：用原生 macOS 应用做展示层

- 状态：已采纳（accepted），记录于 2026-11-01；同日修订：删除 Web 工作台，应用移到 `frontend/`。
- 相关文档：[macOS 应用](../architecture/frontend.md)、[部署方案](../operations/deployment.md#macos_app)、[CI 与发布](../operations/ci_release.md)。

## 背景

Janus 原来只有 Web 工作台（`frontend/app.html`）：先用 `just install` 编译 Rust 服务，再 `just start` 以 launchd 服务运行，最后在浏览器里打开页面。使用 Janus 的人因此要装 Rust 工具链、`just`，并理解服务和端口。用户希望把展示层做成原生 macOS 应用，发布时打成 DMG，下载后直接使用，不需要编译 Rust。应用可用之后，用户要求删除 Web 工作台，只保留 macOS 应用。

## 决策

1. 用 SwiftUI 重写展示层，作为 Swift Package 放在 `frontend/`。编排器、事件格式和 HTTP API 不变。
2. macOS 应用是唯一的界面：删除 `frontend/app.html`，编排器不再提供 `/` 页面，也删除只为网页目录选择器服务的 `/api/fs`。选择目录用系统的文件夹选择框。
3. 应用包内带上 release 构建的 Janus 服务（`janus-server`）。应用启动时连接已经在运行的 Janus，没有时启动包内的服务，退出时停止它自己启动的服务。
4. 事件推导（`derive` / `boardFacts`）在 Swift 中实现，不在应用里嵌网页。删除网页前，同一批真实 Run 在网页和 Swift 两边推导，结果逐项一致。
5. 安装包只做 ad-hoc 签名，不用 Apple 开发者证书，也不做公证。第一次打开下载的应用时，由用户在「系统设置 → 隐私与安全性」中允许。
6. 只支持 Apple 芯片（M 系列）：不构建 Intel 版本，打包脚本和 CI 都只产出 arm64。
7. 安装包内带 Node.js（官方构建，按校验和核对），ACP 适配器不再依赖用户安装的 Node。
8. CI 在每次推送时运行编排器和应用的测试；推送 `v*` 标签时构建 DMG，并发布到 GitHub Release。

## 取舍

- **与把网页嵌进 WKWebView 相比**：原生界面能用系统的侧栏、工具栏、表单、文件选择框、菜单和快捷键，选择目录时不再需要服务端的目录浏览接口。
- **与同时保留 Web 工作台相比**：只有一份界面和一份事件推导，新增事件类型时只改一处；代价是没有 macOS 应用的环境（Intel Mac、其他系统）只能直接调用 HTTP API，没有现成的界面。
- **与把编排器改写进应用相比**：服务保持独立进程，launchd 服务、`just dev` 和应用可以共用同一个 Janus home 和数据库，编排器的测试和恢复逻辑不受影响。
- **与 Developer ID 签名和公证相比**：没有证书成本，构建流程简单；代价是第一次打开需要用户手动允许，发布说明和部署文档写明了步骤。将来有证书时，只需在打包脚本里换签名身份并增加公证步骤。

## 后果

- 新增或修改事件类型时，要同步 `frontend/Sources/Janus/Core/Derive.swift`、看板的 `board_entry` 和 `resume_run`。
- 安装包约增加一个 Node.js 的体积。Agent CLI（`codex`、`claude` 等）仍需用户自己安装，它们就是被调度的 Agent 本身。
- Intel Mac 和其他系统不能使用安装包，也没有网页可用，只能从源码运行服务并调用 HTTP API。
- 只装命令行工具的机器可以构建应用，但 Swift 测试需要 Xcode（`@Test` 宏插件只随 Xcode 提供）；CI 的 macOS 运行环境自带 Xcode。
