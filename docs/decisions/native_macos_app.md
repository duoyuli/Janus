# ADR：用原生 macOS 应用做展示层

- 状态：已采纳（accepted），记录于 2026-11-01。
- 相关文档：[macOS 应用](../architecture/macos_app.md)、[部署方案](../operations/deployment.md#macos_app)、[CI 与发布](../operations/ci_release.md)。

## 背景

Janus 原来只有 Web 工作台：先用 `just install` 编译 Rust 服务，再 `just start` 以 launchd 服务运行，最后在浏览器里打开页面。使用 Janus 的人因此要装 Rust 工具链、`just`，并理解服务和端口。用户希望把展示层做成原生 macOS 应用，发布时打成 DMG，下载后直接使用，不需要编译 Rust。

## 决策

1. 用 SwiftUI 重写展示层，作为独立的 Swift Package 放在 `macos/`。编排器、事件格式和 HTTP API 都不变，应用和 Web 工作台用同一套接口。
2. 应用包内带上 release 构建的 Janus 服务（`janus-server`）。应用启动时连接已经在运行的 Janus，没有时启动包内的服务，退出时停止它自己启动的服务。
3. 事件推导（`derive` / `boardFacts`）在 Swift 中逐条移植，不在应用里嵌网页。
4. 保留 Web 工作台：服务照常提供页面，用于浏览器访问和没有 macOS 应用的环境。
5. 安装包只做 ad-hoc 签名，不用 Apple 开发者证书，也不做公证。第一次打开下载的应用时，由用户在「系统设置 → 隐私与安全性」中允许。
6. CI 在每次推送时运行编排器和应用的测试；推送 `v*` 标签时构建通用（Apple silicon + Intel）DMG，并发布到 GitHub Release。

## 取舍

- **与把网页嵌进 WKWebView 相比**：原生界面能用系统的侧栏、工具栏、表单、文件选择框、菜单和快捷键，选择目录时不再需要服务端的目录浏览接口。代价是事件推导有两份实现，新增事件类型时两边都要改；用一次性比对（同一批真实 Run 在两边推导，结果逐项一致）和 Swift 测试控制偏差。
- **与把编排器改写进应用相比**：服务保持独立进程，launchd 服务、`just dev`、Web 工作台和应用可以共用同一个 Janus home 和数据库，编排器的测试和恢复逻辑不受影响。
- **与 Developer ID 签名和公证相比**：没有证书成本，构建流程简单；代价是第一次打开需要用户手动允许，发布说明和部署文档写明了步骤。将来有证书时，只需在打包脚本里换签名身份并增加公证步骤。

## 后果

- 新增或修改事件类型时，除了页面的 `derive` / `boardFacts`、看板的 `board_entry` 和 `resume_run`，还要同步 `macos/Sources/Janus/Core/Derive.swift`。
- Node.js 和 Agent CLI 仍需用户自己安装：ACP 适配器运行在 Node 上，安装包不带 Node。
- 只装命令行工具的机器可以构建应用，但 Swift 测试需要 Xcode（`@Test` 宏插件只随 Xcode 提供）；CI 的 macOS 运行环境自带 Xcode。
