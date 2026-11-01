# Janus 仓库说明

本地优先的多 Agent 协作工作空间：Rust 编排器在 `backend/`，界面是 `frontend/` 中的 macOS 应用（SwiftUI，安装包内带编排器和 Node.js）。仓库只放代码：数据库、ACP 适配器、产物、配置和日志都在 Janus home（`~/.janus`）。工程文档从 `docs/index.md` 开始，仓库结构和术语见 `docs/project_overview.md`，部署见 `docs/operations/deployment.md`。

## 常用命令

命令都在仓库根目录用 `just` 执行，`just` 列出全部命令（定义在 `justfile`）。

- 开发：`just dev`（等于 `cd backend && cargo run`）从源码启动服务，`just mac` 从源码运行应用（连接这个服务）。改了 Rust 代码需要重启服务，改了 Swift 代码重新 `just mac`。
- 测试：`just test`。不访问网络、不调用真实 Agent，需要本机有 `git` 和 `python3`。应用的测试：`just test-mac`（需要 Xcode）。
- 打包与发布：`just dmg` 构建 `dist/Janus.app` 和 DMG；推送 `v*` 标签（按日期，例如 `v2026.11.01`）由 CI 构建 DMG 并发布，见 `docs/operations/ci_release.md`。
- 安装与服务：`just install`、`just start`、`just restart`、`just status`、`just logs`，见 `docs/operations/deployment.md`。
- 排障、重启恢复：见 `docs/operations/runbook.md`。

## 约定

- 不修改用户自己的 Agent 配置（`~/.codex`、`~/.claude`、`~/.grok`、`~/.pi`）。Janus 需要为 Agent 写入的设置只写入 `~/.janus/agents/<id>/` 下的隔离配置。
- 凭据不写进代码、日志、文档、SQLite 或设置接口的返回值。
- 不替用户 commit 或 push；需要时先确认。
- 新增或修改事件类型时，同步检查应用的 `derive` / `boardFacts`（`frontend/Sources/Janus/Core/Derive.swift`）、看板的 `board_entry` 和恢复逻辑 `resume_run`，见 `docs/interfaces/events.md`。
- 权限完全由各 Agent 的原生模式决定，Janus 不另加读写开关或校验，见 `docs/decisions/agent_native_permissions.md`。
- `flow_tests` 中每个测试给 `harness` 传唯一的名字，测试并行运行时同名会互相删除临时目录。
- 运行时不向仓库写任何文件；新的数据、依赖、配置或日志放进 Janus home 的对应子目录，并更新 `docs/operations/deployment.md` 的目录规范。
- ACP 适配器按需安装、按版本存放，新增内置运行时要在 `backend/src/adapters.rs` 的 `ADAPTERS` 中登记，见 `docs/architecture/adapters.md`。
- `README.md` 是产品入口，工程事实写在 `docs/` 中。
- 改代码时同步更新相关文档。中文文档遵循 tech-doc-style-chinese：中文与英文、数字之间加空格，引号用「」，一段一行。
- 项目 skill 放在 `.agents/skills/`，`.claude/skills` 是指向它的符号链接；`CLAUDE.md` 是指向本文件的符号链接，内容只写在本文件中。

## Project Doc gate

- 执行任何仓库任务之前，先确认当前 agent 会话能够调用名为 `project-doc` 的 skill。
- 如果该 skill 不可用，立即停止。唯一允许的下一步是回复：「当前环境未安装或未加载 `project-doc` skill，仓库规则不允许继续工作。请安装或启用该 skill 后，在新会话中重试。」
- skill 可用时，先使用 `project-doc`。在本仓库的第一个任务中，完整阅读 `docs/index.md`，再按其中的路由处理任务。
- 同一会话中已读过的 skill 说明、索引和章节，只要其内容仍足够完整地保留在上下文中就复用，编辑文件、继续子任务和完成工作时同样适用。内容缺失、相关文件发生变化或路由冲突时，按 skill 的「Reading and context reuse」规则重读受影响的材料。
