# ADR：Web 客户端用 TypeScript、Vite 和 Preact，由 Janus 服务提供

- 状态：已采纳（accepted），记录于 2026-11-26。用户在同一天定下方向（做一个和 macOS 客户端功能对齐、支持手机的 Web 版本）和技术栈（TypeScript + Vite + Preact）。部分取代 [ADR：用原生 macOS 应用做展示层](20261101-native_macos_app.md) 的第 2 条（macOS 应用是唯一的界面，编排器不提供 `/` 页面）。手机怎样连到服务、怎样配对另见 C 路的 ADR（局域网监听和设备令牌）。
- 相关文档：[Web 客户端](../architecture/20261126-web.md)、[ADR：由事件推导的状态只在服务端算](20261126-backend_owns_derived_state.md)、[安全边界](../operations/20261031-security.md#web_build)、[部署方案](../operations/20261031-deployment.md#install)、[CI 与发布](../operations/20261101-ci_release.md)、[端到端验证](../operations/20261031-testing.md#web_e2e)。

## 背景

Janus 只有 macOS 应用一个界面：Intel Mac、其他系统和手机都用不了。用户在 2026-11-26 提出：要做一个功能和 macOS 客户端对齐的 Web 版本，并且要在手机上用。为此，由事件推导的状态和条目文字已经搬到服务端（[ADR：由事件推导的状态只在服务端算](20261126-backend_owns_derived_state.md)），两个客户端读同一份结果。

## 决策

1. **只展示**：Web 客户端和 Mac 应用遵守同一条规则：不在 TypeScript 里做由事件推导的计算，条目文字由服务端按 `?lang=` 写好；还没迁到服务端的计算，先迁到 Rust 再由两个客户端读。Web 只做画面自己的事：路由、折叠、草稿、语言和外观的选择、时间的写法。
2. **技术栈**：TypeScript（strict）、Vite 和 Preact，状态用 `@preact/signals`，样式用自己写的 CSS，不引入 UI 组件库。用户在 Rust/WASM、无构建的原生 JS 和这一种之间选了这一种：有类型检查和组件写法，生态成熟，包体小（Preact 约 4 KB）。
3. **由 Janus 服务提供**：`web/` 构建出的 `web/dist` 在编译时嵌进 `janus` 二进制（`backend/build.rs`），服务对 `/api/`、`/mcp/` 以外的路径返回 Web 客户端的文件或页面。`just install`、包内的 `janus-server` 和发布包都不用另带目录。没有构建时服务照常启动，页面说明怎样构建。
4. **和 Mac 共用一份翻译**：界面文字用中文写在代码里（`t\`…\``），英文在构建时读 Mac 应用的 `en.lproj/Localizable.strings`，键的写法和 Swift 的字符串插值一致。
5. **npm 是新的下载来源，只在构建时用**：依赖锁定在 `web/package-lock.json`（版本写死，带完整性哈希），安装一律 `npm ci --ignore-scripts`；只在开发、CI 和打包时下载，Janus 运行时不联网，也不在用户机器上安装这些包。依赖保持最少：运行时只有 `preact` 和 `@preact/signals`，构建和测试只有 `vite`、`typescript`、`@types/node` 和 `@playwright/test`；单元测试用 Node 自带的 `node:test`。浏览器端到端测试用本机已装的 Chrome，不下载浏览器。
6. **手机布局**：宽度 640 像素以下时，侧栏换成底部页签（收件箱、团队、对话、设置），页面顶部有标题和返回，点按区域至少 44 像素，处理刘海和底部横条的安全区域。

## 取舍

- **与 Rust/WASM（Leptos 等）相比**：可以和服务端共用类型，但工具链重、构建慢、包体大，生态小；计算已经都在服务端，客户端共享类型的收益有限。
- **与无构建的原生 JS 相比**：不需要 npm，但没有类型检查，组件写法要自己约定；按页面对齐 Mac 应用的工作量大，类型检查值得一个构建步骤。
- **与另带一个静态目录相比**：嵌进二进制让服务和页面永远是同一个版本，安装和更新不多一步；代价是改了页面要重新编译服务（开发时用 `just web-dev`，页面热更新，不用重编服务）。
- **与用 vitest 相比**：`node:test` 足够测纯函数，少一组依赖。

## 后果

- 新的构建依赖：开发、CI 的 Web 任务和发布的打包都需要 Node.js 22.18 以上和 npm；CI 的 `CI result` 也等 Web 任务。`just e2e` 多一步「Web 端到端测试」。
- 新增或改动需要在界面显示的状态时，两个客户端都要画，但只有一份计算（服务端）和一份翻译（`Localizable.strings`）。
- Web 版的页面按 [Web 客户端](../architecture/20261126-web.md#add_page) 的约定逐页对齐 Mac 应用；只有 Mac 能做的（启动服务、应用更新、macOS 权限、系统通知、系统的文件夹选择框）不在 Web 版里。
- 服务多了一类响应：非 API 路径返回 Web 客户端。页面本身不含数据，数据仍只经 `/api/`。
