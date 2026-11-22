<div align="center">

# Janus

*始于构想，淬于交锋*

<sub>本地优先 · 多智能体 · 可回放</sub>

[English](README.md) · [中文](README.zh-CN.md)

</div>

> 门有两面。一面向前，望向尚未建成之物；一面向后，审视必须成立之事。

Janus 是一个本地优先的多智能体协作工作空间。它通过 ACP（Agent Client Protocol）把 Codex、Claude Code 和其他兼容 ACP 的 Agent 连接成一支团队：负责人 Agent 掌握方向，协作者承担工作，验收在共享看板上公开完成。每一次会话、编排、审批、产物和交付，都会写成可回放的事件。

## 名字

Janus 是罗马神话中的门神，也是开端与转换之神。他有两张面孔：一张朝向未来，那里是创造；一张回望过去，那里是审视。门既是通道，也是关口，未经审视的交付，不算完成。

这个名字把这层意象写成工程目标：让不同模型、不同上下文、不同气质的 Agent 组成同一张网络，围绕同一个问题工作。创造接受质疑，留下的才被交付。

## 为什么是 Janus

- **本地优先。** 仓库只放代码。任务记录、适配器、配置、产物和日志都留在 `~/.janus`，留在工作发生的机器上。
- **一个负责人，多种心智。** 负责人 Agent 对目标负责，决定自己做还是交给协作者；协作者带来不同的模型，也带来不同的盲区。
- **默认可回放。** 每一次会话、编排、审批、产物和交付都写成事件，任务可以被检查、恢复，而不是靠记忆重建。
- **原生权限。** 每个 Agent 保留它原本的权限模型，Janus 不再叠加一套开关或并行的审批层。

## 核心概念

| 概念 | 说明 |
| --- | --- |
| Agent（Provider 连接） | 一个可启动的编码 Agent，例如 Codex 或 Claude Code。每个 Agent 有独立的配置目录和凭据。 |
| 数字员工 | 绑定一个 Agent，并指定名字、职责、模型、推理强度和权限模式。每个数字员工都能用整个资源库。 |
| 资源库 | Skill 和 MCP 服务器只保存一份，分两个标签管理，都给所有数字员工，只经 Skill 路由和 MCP 路由按需取用（页面上有动画说明路由怎样工作），会话的提示词和工具列表不随资源库增长。「推荐」里有社区常用的 Skill 和 MCP 服务器，一键添加。Skill 从 GitHub 仓库导入、从本机文件夹批量复制或在应用中写，按来源分组；MCP 服务器填写表单或粘贴 JSON 配置。每个会话只拿到一个 Skill `janus-skills`（Claude Code 是插件，Codex 是额外的 Skill 目录）和一个 MCP 服务器 `janus-tools`；`janus-skills` 是一份目录，Skill 多了就按关键词搜索，常用的排在前面，资源库显示每个 Skill 被读过几次、描述哪里可以改进；背后的命令服务器由 Janus 启动、HTTP 和 SSE 服务器由 Janus 带着保存的值连接，不写任何 Agent 的配置文件。 |
| 工作区 | 一个本机目录，加上一组数字员工，其中一名是负责人。 |
| 任务 | 在工作区中提交的一个目标，从创建到交付、验收的全过程。所有工作区同时在做的任务默认最多 8 个，更多的排队，空出位置后按先后开始；任务没有时间和费用上限。 |
| 值班（Loop） | 工作区里按时间表醒来的负责人：按你定的时间表（每隔几分钟、每天、工作日、每周，或一条 cron 表达式）醒来，看一眼、做判断，一两句话能答的直接答，要改东西的派成任务（同一件事只派一次），只有你能决定的问你，结果都到收件箱。 |
| 负责人 | 用 `/goal` 推进目标的 Agent，只能是 Claude Code 或 Codex。它决定自己做，还是通过编排交给协作者。 |
| 对话 | 和一名数字员工单独对话，不属于任何工作区：它带着自己的指令、模型、权限和资源库，在 `~/.janus/work/chats/` 下新建的目录里工作，删除对话时目录一起删除。会话视图和负责人的相同：插话、接管、审批、粘贴图片、切换模型和模式。 |
| 编排与共享看板 | 负责人提交的一组子任务（执行者、依赖、验收标准）。Janus 先校验再执行，子任务的进展、阻塞和提问都写在共享看板上。 |

## 快速开始

### 用 macOS 应用（推荐）

1. 在 [GitHub Releases](https://github.com/duoyuli/Janus/releases) 下载 `Janus-<版本>.dmg`，把 Janus 拖进「应用程序」。
2. 第一次打开时，macOS 会提示无法验证开发者（安装包没有用 Apple 证书签名）：到「系统设置 › 隐私与安全性」点「仍要打开」。
3. 需要 Apple 芯片（M 系列）的 Mac、macOS 15 以上：Janus 只支持最近三个 macOS 大版本（15、26、27）。安装包自带 Janus 服务和 Node.js，不需要 Rust 和 Node。Codex 和 Claude Code 的适配器自带对应的 CLI。

应用自带 Janus 服务，打开即用；如果你已经用 `just start` 运行了 Janus，应用会直接连接它。详见[部署方案](docs/operations/20261031-deployment.md#macos_app)。

按 ⌘, 打开 Janus 设置，在「通用 → 语言」中选择「跟随系统」「简体中文」或「English」。切换立即生效，重新打开应用时保留选择。

首次安装，或 Janus 的签名身份变化后（例如从 2026.11.18.5 之前的版本升级上来），应用自动显示 macOS 权限引导；授权之后，同一签名的更新会保留这些权限。引导也可以在「设置 › 通用 › macOS 权限」继续。一键按顺序处理完整磁盘访问、应用程序管理和屏幕录制；授权仍需在系统界面开启，三项的状态都由 Janus 自动检测。独立服务启动的 Agent 可能需要单独授权，见 [macOS 权限](docs/architecture/20261031-frontend.md#macos_permissions)。

### 从源码运行

#### 前置条件

- Rust 工具链和 Cargo（edition 2024）、Swift 6（Xcode 或命令行工具），以及 [just](https://github.com/casey/just)。
- Node.js 22.19 或更高，以及随它安装的 npm。
- 内置适配器自带 Codex 和 Claude Code 的 CLI。
- 第一次使用某个 Agent 时需要网络，用来下载它的 ACP 适配器。

#### 安装并启动

在仓库根目录：

```sh
just install   # 编译并安装到 ~/.janus/bin/janus
just start     # 以服务方式启动：登录后自动运行（macOS）；也可以用 just run 在前台运行
```

然后用 `just mac` 从源码运行应用，它会连接这个服务；`just dmg` 在本机打包出 `Janus.app` 和 DMG。开发时用 `just dev` 从源码启动服务。`just` 不带参数会列出全部命令。

### 首次使用

打开应用后按以下顺序配置：

1. 在「设置 › 数字员工」中添加 Agent，并运行配置检查。它需要的 ACP 适配器会在这一步自动下载；以后可以在 Agent 卡片的适配器一行检查更新、升级或回滚。
   通过 API 接入 Claude 时，在「认证」中选择「自定义 API」，点「填入模板」或直接粘贴多行 `export KEY=value`，一起设置网关地址、令牌和模型。
2. 在同一页为数字员工绑定 Agent，选择模型和权限模式，需要它带队时开启「可以担任负责人」。
3. 新建工作区：选择目录，启用数字员工，并指定一名绑定 Claude Code 或 Codex 的数字员工为负责人。
4. 在工作区首页输入目标，交给负责人。

仓库只放代码。Janus 的数据、适配器、配置和日志都在 `~/.janus`，详见[部署方案](docs/operations/20261031-deployment.md)。

`examples/calc/` 是一个最小示例项目。用作工作区前先把它复制到仓库之外（例如 `cp -R examples/calc ~/calc`），并在复制出来的目录里建一个 Git 仓库、提交一次（`git init && git add -A && git commit -m init`）：这样每个任务都在自己的 worktree 里进行，留下 diff，验收时 Janus 把结果提交并快进到你所在的分支，直接出现在你的目录里。不是 Git 仓库也能用，只是子任务直接修改这个目录，也没有 diff。可以用 `python3 -m unittest discover -s examples/calc` 验证。

## 文档

工程文档从 [docs/index.md](docs/index.md) 开始，按用途分为七类：

| 分类 | 内容 |
| --- | --- |
| [架构](docs/architecture/index.md) | 组件与进程模型、存储布局、ACP 适配器管理、macOS 应用 |
| [领域](docs/domains/index.md) | 任务（Run）的生命周期、目标模式与共享看板、Agent / 数字员工 / 工作区与权限模式、资源库 |
| [接口](docs/interfaces/index.md) | HTTP API、事件、看板 MCP 工具、ACP 集成、配置 |
| [运维](docs/operations/index.md) | 部署方案、CI 与发布、运行手册、测试、安全边界 |
| [决策](docs/decisions/index.md) | 架构决策记录（ADR） |
| [研究](docs/research/index.md) | 外部多 Agent 协作与 Agent Team 的观察报告，以及给本项目的借鉴建议 |
| [审查](docs/review/index.md) | 代码审查和审计报告 |

在本仓库工作的编码 Agent 先读 [AGENTS.md](AGENTS.md)（`CLAUDE.md` 指向同一文件）。

## 适用范围

Janus 适合在一台机器上组织多个编码 Agent 完成开发、调研、审查、文档和验证工作。它是单机、单用户的实验性工具：子任务直接修改所选目录，服务没有鉴权，也没有操作系统级沙箱。投入真实项目前，请先阅读[安全边界](docs/operations/20261031-security.md)。
