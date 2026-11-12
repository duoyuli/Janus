# 架构概览

本文介绍 Janus 由哪些组件构成、它们如何通信、启动时做什么，以及一次 Run 的数据流。Run 和任务的状态机见 [Run 生命周期](../domains/run_lifecycle.md)，存储细节见[数据模型与存储](data_model.md)，界面见 [macOS 应用](frontend.md)。

<a id="principles"></a>
## 设计原则

`backend/src/orchestrator.rs` 的模块注释定下了分工：

- **主控 Agent 决定做什么**：目标模式下，主控决定是否协作、怎么拆分任务。
- **编排器决定何时做、由谁做**：只有编排器改变任务状态，并按依赖调度。
- **编码 Agent 决定怎么做**：每个 Agent 按自己的权限模式工作，Janus 不改写它们的行为。
- **事实先落库**：每次状态变化都先作为事件写入 SQLite，再广播给界面。界面状态完全由事件推导，重启后的内存状态也从事件重建。

这些原则的取舍见 [ADR：事件是唯一事实来源](../decisions/events_as_source_of_truth.md)。

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
  ├── ACP 客户端：JSON-RPC 2.0 over stdio
  └── SQLite：配置、Run、事件、尝试、产物
  │  stdio（每个会话一个子进程）
  ▼
ACP 适配器（~/.janus/adapters/，按需安装）
  ├── acp-extension-codex   → Codex CLI
  ├── acp-extension-claude  → Claude Code
  ├── acp-extension-grok    → Grok CLI
  ├── acp-extension-pi      → Pi
  └── 自定义 ACP 命令
  │
  ▼
用户选择的工作目录（任务直接在这里读写）
```

| 组件 | 说明 |
| --- | --- |
| 编排器进程 | 一个 Rust 二进制 `janus`，默认监听 `127.0.0.1:7878`。进行中的 Run 保存在内存的 `App.runs` 中，事实写入 SQLite |
| ACP 子进程 | 每个会话启动一个独立的适配器子进程：主控会话一个，每个任务的每次尝试一个。环境变量由 Janus 完整构造，见[安全边界](../operations/security.md#agent_isolation) |
| 看板 MCP 服务 | 编排器自己提供的 HTTP MCP 端点，供主控和协作者会话调用，见[看板 MCP 工具](../interfaces/board_mcp.md) |
| macOS 应用 | `frontend/` 中的 SwiftUI 应用，Janus 唯一的界面。安装包内带编排器和 Node.js；打开时连接已有的编排器，没有就启动包内的，见 [macOS 应用](frontend.md) |
| 适配器管理 | 按需下载、升级、回滚 ACP 适配器，见 [ACP 适配器管理](adapters.md) |

<a id="modules"></a>
## 模块划分

| 文件 | 职责 |
| --- | --- |
| `backend/src/main.rs` | 命令行参数、启动顺序、HTTP 路由与处理函数 |
| `backend/src/config.rs` | Janus home、配置项（环境变量与 `config.toml`）、Janus 日志 |
| `backend/src/git_probe.rs` | 只读 Git 查询的共用入口：设置 `GIT_OPTIONAL_LOCKS=0`，不与同一工作目录中 Agent 的 git 操作抢 `index.lock`。和 HEAD 比较用 `git diff-index`，因为 `git diff` 不受这个变量控制，仍会刷新索引 |
| `backend/src/adapters.rs` | `App` 的适配器部分：清单、按需安装、检查更新、升级与回滚 |
| `backend/src/acp.rs` | 最小 ACP 客户端：启动子进程、`initialize`、请求与通知收发、超时 |
| `backend/src/agents.rs` | `App` 的设置部分：Agent 连接、运行环境、配置检查与登录、角色、工作区、Skill、会话控制项解析 |
| `backend/src/orchestrator.rs` | `App` 的运行部分：Run 驱动、主控目标会话、看板 MCP、调度、尝试、验证、交付、用户操作、恢复，以及集成测试 |
| `backend/src/teamwork.rs` | 编排中的团队结构：任务负责的文件（`owns`）和独立验证（`verifies`）的校验、协作者提示中的对应段落、主控说明中「团队怎么组织」一节 |
| `backend/src/status.rs` | Run、任务、尝试和 Agent 阶段的状态枚举；Run 和任务的合法转移集中在 `can_transition_to` |
| `backend/src/skill_sources.rs` | `App` 的 Skill 来源部分：解析 GitHub 链接、克隆、查找 Skill、更新与删除，以及按 id 找到 Skill（自建或来源） |
| `backend/src/db.rs` | SQLite 表结构与读写 |
| `backend/testdata/fake_acp.py` | 测试用的脚本化 ACP Agent |
| `frontend/` | macOS 应用（Swift Package），结构见 [macOS 应用](frontend.md#source_layout) |

`agents.rs`、`adapters.rs`、`skill_sources.rs` 和 `orchestrator.rs` 都在 `impl App` 上添加方法；`App` 结构体定义在 `orchestrator.rs`。

<a id="startup"></a>
## 启动顺序

`main` 按以下顺序执行：

1. 处理命令行参数（`--version`、`--help`）。读取配置项：Janus home（`JANUS_HOME`，默认 `~/.janus`，权限设为 `0700`）、监听地址和 Node 路径，见[配置](../interfaces/configuration.md#settings)。
2. 打开 SQLite 并建表。
3. `recover`：把上次未结束的尝试标记为中断，未结束的 Run 标记为 `interrupted`。
4. `seed_roles`：首次启动时写入 5 个组成产品上线团队的默认角色；旧版本升级时只执行一次：新增旧版本没有的默认角色，并把仍是旧默认职责和指令的角色换成新版本，见[角色](../domains/agents_and_roles.md#roles)；`reset_stale_checks`：重启前没跑完的配置检查改回「未检查」。
5. 注册路由并开始监听（`JANUS_ADDR`）。

<a id="run_data_flow"></a>
## 一次 Run 的数据流

以默认的目标模式为例：

1. 用户在工作区提交目标（`POST /api/runs`）。Janus 把成员、角色、模型、权限模式、Skill 和 MCP 做成快照，写入 `runs.config`。
2. Janus 启动主控会话，接入看板 MCP 服务，先给主控一段说明（团队名单、看板工具、协作要点），再发送 `/goal <目标>`。
3. 主控自行推进目标。需要协作时调用 `orchestrate` 提交编排；Janus 校验通过后把任务写入看板。
4. 调度器按依赖启动任务。每次尝试是一个新的 ACP 会话，结束时 Agent 返回结构化结果，Janus 执行验证并保存产物。
5. 协作者的进展、阻塞、提问和结果都写成事件。主控用 `wait` 读取，用 `reply` 回答。
6. 主控的目标结束、且没有任务还在执行或可以启动时，Janus 生成交付结果，用户核对后验收。

整个过程中，Agent 发出的审批请求（`session/request_permission`）和表单请求（`elicitation/create`）都转给用户，在应用中统一渲染。

详细规则见 [Run 生命周期](../domains/run_lifecycle.md) 和[目标模式与共享看板](../domains/goal_mode.md)。
