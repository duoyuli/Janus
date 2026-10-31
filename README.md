# Janus

Janus 是一个本地优先的多 Agent 协作工作空间。它通过 ACP（Agent Client Protocol）连接 Codex、Claude Code、Grok、Pi 和其他兼容 ACP 的 Agent，让一个主控 Agent 带着一组协作者完成用户给出的目标，并把每一次会话、编排、审批、产物和交付都记录成可回放的事件。

本仓库包含 Rust 编排器（`backend/`）和单页 Web 工作台（`frontend/app.html`）。本文介绍 Janus 是什么、怎样在本机启动，以及其余文档的阅读顺序。

## 核心概念

| 概念 | 说明 |
| --- | --- |
| Agent（Provider 连接） | 一个可启动的编码 Agent，例如 Codex 或 Claude Code。每个 Agent 有独立的配置目录和凭据。 |
| 角色 | 绑定一个 Agent，并指定名字、职责、模型、推理强度、权限模式、Skill 和 MCP。 |
| 工作区 | 一个本机目录，加上一组角色成员，其中一名是主控。 |
| Run | 在工作区中提交的一个目标，从创建到交付、验收的全过程。 |
| 主控 | 用 `/goal` 推进目标的 Agent，只能是 Claude Code 或 Codex。它决定自己做，还是通过编排交给协作者。 |
| 编排与共享看板 | 主控提交的一组任务（执行者、依赖、验收标准）。Janus 先校验再执行，任务进展、阻塞和提问都写在共享看板上。 |

## 快速开始

### 前置条件

- Rust 工具链和 Cargo（edition 2024），以及 [just](https://github.com/casey/just)。
- Node.js 22.19 或更高，以及随它安装的 npm；还需要 `git`。
- 要使用的 Agent CLI，例如 `codex`、`claude`、`grok`。也可以配置自定义 ACP 启动命令。
- 第一次使用某个 Agent 时需要网络，用来下载它的 ACP 适配器。

### 安装并启动

在仓库根目录：

```sh
just install   # 编译并安装到 ~/.janus/bin/janus
just start     # 以服务方式启动：登录后自动运行（macOS）；也可以用 just run 在前台运行
```

开发时直接 `just dev`（从源码启动）。`just` 不带参数会列出全部命令。

启动后打开 [http://127.0.0.1:7878](http://127.0.0.1:7878)。首次使用按以下顺序配置：

1. 在「设置 → Agents」中添加 Agent，并运行配置检查。它需要的 ACP 适配器会在这一步自动下载；以后可以在「设置 → ACP 适配器」检查更新、升级或回滚。
2. 在「设置 → 角色」中为角色绑定 Agent，选择模型和权限模式。
3. 新建工作区：选择目录，添加成员，并指定一名 Claude Code 或 Codex 角色为主控。
4. 在工作区首页输入目标，交给主控。

仓库只放代码。Janus 的数据、适配器、配置和日志都在 `~/.janus`，详见[部署方案](docs/operations/deployment.md)。

`examples/calc/` 是一个最小示例工作区，可以用 `python3 -m unittest discover -s examples/calc` 验证。

## 文档

工程文档从 [docs/index.md](docs/index.md) 开始，按用途分为五类：

| 分类 | 内容 |
| --- | --- |
| [架构](docs/architecture/index.md) | 组件与进程模型、存储布局、ACP 适配器管理、Web 工作台 |
| [领域](docs/domains/index.md) | Run 生命周期、目标模式与共享看板、Agent / 角色 / 工作区与权限模式 |
| [接口](docs/interfaces/index.md) | HTTP API、事件、看板 MCP 工具、ACP 集成、配置 |
| [运维](docs/operations/index.md) | 部署方案、运行手册、测试、安全边界 |
| [决策](docs/decisions/index.md) | 架构决策记录（ADR） |

在本仓库工作的编码 Agent 先读 [AGENTS.md](AGENTS.md)（`CLAUDE.md` 指向同一文件）。

## 适用范围

Janus 适合在本机组织多个编码 Agent 协作完成开发、调研、审查、文档和验证工作。它是单机、单用户的实验性工具：任务直接修改所选目录，服务没有鉴权，也没有操作系统级沙箱。投入真实项目前，请先阅读[安全边界](docs/operations/security.md)。
