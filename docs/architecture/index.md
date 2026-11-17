# 架构文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录 Janus 的组件、进程模型、模块边界、存储布局，以及 macOS 应用的结构。Run 与任务的状态规则归入[领域](../domains/index.md)，对外接口和事件格式归入[接口](../interfaces/index.md)。文档列表之后是本分类的总览：[设计原则](#principles)、[组件](#components)和[一次 Run 的数据流](#run_data_flow)，第一次修改编排器时读。

## 文档列表

- [模块与启动顺序](20261116-modules.md)：编排器每个源码模块的职责、`App` 的方法分布和进程的启动顺序。阅读时机：第一次修改编排器，需要确定某项职责在哪个文件，调整模块职责或启动流程。
- [数据模型与存储](20261031-data_model.md)：Janus home 中的数据、SQLite 表、Run 快照和产物。阅读时机：修改 `backend/src/db.rs`、数据目录结构、产物保存或快照内容。
- [ACP 适配器管理](20261031-adapters.md)：适配器清单、按版本存放、按需安装、安装步骤、检查更新、升级与回滚、对运行中会话的影响。阅读时机：修改 `backend/src/adapters.rs`、新增内置运行时，或排查适配器安装和升级问题。
- [macOS 应用](20261031-frontend.md)：应用与 Janus 服务的关系（连接或启动包内服务、端口被占用）、源码结构、由事件推导状态、同步方式、页面、跨 Run 的收件箱、请求卡片和会话中的消息。阅读时机：修改 `frontend/`，或新增、修改需要在应用中展示的事件。

<a id="principles"></a>
## 设计原则

`backend/src/orchestrator.rs` 的模块注释定下了分工（它的子模块各管 `App` 的一部分，见[模块划分](20261116-modules.md#modules)）：

- **负责人 Agent 决定做什么**：目标模式下，负责人决定是否协作、怎么拆分任务。
- **编排器决定何时做、由谁做**：只有编排器改变任务状态，并按依赖调度。
- **编码 Agent 决定怎么做**：每个 Agent 按自己的权限模式工作，Janus 不改写它们的行为。
- **事实先落库**：每次状态变化都先作为事件写入 SQLite，再广播给界面。界面状态完全由事件推导，重启后的内存状态也从事件重建。

这些原则的取舍见 [ADR：事件是唯一事实来源](../decisions/20261031-events_as_source_of_truth.md)。

<a id="components"></a>
## 组件

```text
macOS 应用（frontend/，SwiftUI）
  │  REST + SSE（应用找到或启动编排器，见 macOS 应用）
  ▼
Rust 编排器（backend/，axum + tokio）
  ├── HTTP 服务：设置、Run、操作、事件流、产物读取
  ├── 编排器 App：Run 驱动、调度、尝试、验证、交付、恢复
  ├── 共享看板 MCP 服务：/mcp/{run}/{token}/{who}
  ├── ACP 客户端（官方 agent-client-protocol SDK）：JSON-RPC 2.0 over stdio
  └── SQLite：配置、Run、事件、尝试、产物
  │  stdio（每个会话一个子进程）
  ▼
ACP 适配器（~/.janus/adapters/，按需安装）
  ├── @agentclientprotocol/codex-acp         → Codex CLI
  ├── @agentclientprotocol/claude-agent-acp  → Claude Code
  └── 自定义 ACP 命令
  │
  ▼
用户选择的工作目录（任务直接在这里读写）
```

| 组件 | 说明 |
| --- | --- |
| 编排器进程 | 一个 Rust 二进制 `janus`，默认监听 `127.0.0.1:7878`。进行中的 Run 保存在内存的 `App.runs` 中，事实写入 SQLite |
| ACP 子进程 | 每个会话启动一个独立的适配器子进程：负责人会话一个，每个任务的每次尝试一个。环境变量由 Janus 完整构造，见[安全边界](../operations/20261031-security.md#agent_isolation) |
| 看板 MCP 服务 | 编排器自己提供的 HTTP MCP 端点，供负责人和协作者会话调用，见[看板 MCP 工具](../interfaces/20261031-board_mcp.md) |
| macOS 应用 | `frontend/` 中的 SwiftUI 应用，Janus 唯一的界面。安装包内带编排器和 Node.js；打开时连接已有的编排器，没有就启动包内的，见 [macOS 应用](20261031-frontend.md) |
| 适配器管理 | 按需下载、升级、回滚 ACP 适配器，见 [ACP 适配器管理](20261031-adapters.md) |

<a id="run_data_flow"></a>
## 一次 Run 的数据流

以默认的目标模式为例：

1. 用户在工作区提交目标（`POST /api/runs`）。Janus 把成员、角色、模型、权限模式、Skill 和 MCP 做成快照，写入 `runs.config`。
2. Janus 启动负责人会话，接入看板 MCP 服务，先给负责人一段说明（团队名单、看板工具、协作要点），再发送 `/goal <目标>`。
3. 负责人自行推进目标。需要协作时调用 `orchestrate` 提交编排；Janus 校验通过后把任务写入看板。
4. 调度器按依赖启动任务。每次尝试是一个新的 ACP 会话，结束时 Agent 返回结构化结果，Janus 执行验证并保存产物。
5. 协作者的进展、阻塞、提问和结果都写成事件。负责人用 `wait` 读取，用 `reply` 回答。
6. 负责人的目标结束、且没有任务还在执行或可以启动时，Janus 生成交付结果，用户核对后验收。

整个过程中，Agent 发出的审批请求（`session/request_permission`）和表单请求（`elicitation/create`）都转给用户，在应用中统一渲染。

详细规则见 [Run 生命周期](../domains/20261031-run_lifecycle.md) 和[目标模式与共享看板](../domains/20261031-goal_mode.md)。
