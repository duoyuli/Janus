# 架构概览

本文介绍 Janus 由哪些组件构成、它们如何通信、启动时做什么，以及一次 Run 的数据流。Run 和任务的状态机见 [Run 生命周期](../domains/run_lifecycle.md)，存储细节见[数据模型与存储](data_model.md)，界面见 [macOS 应用](frontend.md)。

<a id="principles"></a>
## 设计原则

`backend/src/orchestrator.rs` 的模块注释定下了分工（它的子模块各管 `App` 的一部分，见[模块划分](#modules)）：

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
| ACP 子进程 | 每个会话启动一个独立的适配器子进程：主控会话一个，每个任务的每次尝试一个。环境变量由 Janus 完整构造，见[安全边界](../operations/security.md#agent_isolation) |
| 看板 MCP 服务 | 编排器自己提供的 HTTP MCP 端点，供主控和协作者会话调用，见[看板 MCP 工具](../interfaces/board_mcp.md) |
| macOS 应用 | `frontend/` 中的 SwiftUI 应用，Janus 唯一的界面。安装包内带编排器和 Node.js；打开时连接已有的编排器，没有就启动包内的，见 [macOS 应用](frontend.md) |
| 适配器管理 | 按需下载、升级、回滚 ACP 适配器，见 [ACP 适配器管理](adapters.md) |

<a id="modules"></a>
## 模块划分

| 文件 | 职责 |
| --- | --- |
| `backend/src/main.rs` | 命令行参数、启动顺序、HTTP 路由与处理函数 |
| `backend/src/config.rs` | Janus home、配置项（环境变量与 `config.toml`）、Janus 日志。配置只在启动时读一次，写进 `App.cfg`（home、监听地址、Node 路径）；其余代码只读 `App.cfg`，所以指向另一个 home 的 `App`（例如测试的）不会碰到用户的 `~/.janus` |
| `backend/src/git_probe.rs` | 只读 Git 查询的共用入口：设置 `GIT_OPTIONAL_LOCKS=0`，不与同一工作目录中 Agent 的 git 操作抢 `index.lock`。和 HEAD 比较用 `git diff-index`，因为 `git diff` 不受这个变量控制，仍会刷新索引 |
| `backend/src/adapters.rs` | `App` 的适配器部分：清单、按需安装、检查更新、升级与回滚。进行中的安装、查到的最新版本和会话占用的版本（`AdapterState`）由 `App` 持有，不是进程级的全局变量 |
| `backend/src/acp.rs` | 基于官方 `agent-client-protocol` SDK 的 ACP 客户端：启动子进程、`initialize`、请求与通知收发、超时 |
| `backend/src/agents.rs` | `App` 的设置部分的公共部分：设置对象的类型名、命令拆分、id 与 slug 等共用函数；子模块如下 |
| `backend/src/agents/catalog.rs` | Agent 类型、登录方式和环境变量预设 |
| `backend/src/agents/connections.rs` | Agent 连接的保存与删除、隔离目录和密钥、会话的启动命令和沙箱环境 |
| `backend/src/agents/checks.rs` | 配置检查（适配器、运行时、凭据、会话、试运行）和账号登录 |
| `backend/src/agents/roles.rs` | 角色、角色选择的资源、首次启动写入的默认角色 |
| `backend/src/agents/workspaces.rs` | 工作区和由工作区组成的 Run 团队 |
| `backend/src/agents/session_options.rs` | 会话控制项的解析、角色保存的值、Plan 模式不运行的规则 |
| `backend/src/agents/local_config.rs` | 「复制本机配置」：把本机 Agent 的设置复制到隔离目录 |
| `backend/src/library.rs` | `App` 的资源库部分：资源（Skill、MCP 服务器）的类型、新建、编辑、导入、删除和应用读到的列表；子模块 `store`（按内容寻址的 Skill）、`skill`、`mcp`（服务器定义和值）、`sources`（Git 来源）、`deliver`（资源包和会话参数），见[资源库](../domains/resource_library.md) |
| `backend/src/orchestrator.rs` | `App` 的运行部分的核心：Run、任务和尝试的内存状态，事件的写入与广播，等待者和状态转移；子模块如下 |
| `backend/src/orchestrator/lifecycle.rs` | 创建和驱动 Run：调度循环、停滞检测、交付 |
| `backend/src/orchestrator/lead.rs` | 主控：目标会话、交付后的对话、主控说明和推送进会话的看板消息 |
| `backend/src/orchestrator/board.rs` | 看板 MCP：鉴权、主控和协作者的工具调用、角色统计 |
| `backend/src/orchestrator/board_tools.rs` | 看板提供和输出的内容：工具定义、编排校验、主控读到的看板消息 |
| `backend/src/orchestrator/messages.rs` | 用户和主控发给会话的消息：记录、插入正在进行的一轮或排队 |
| `backend/src/orchestrator/attempts.rs` | 任务的一次尝试：会话、结果核对、改动和产物 |
| `backend/src/orchestrator/turn.rs` | ACP 会话中的一轮提示：把更新转成事件、插话、控制项和 Agent 的请求 |
| `backend/src/orchestrator/session.rs` | 为成员打开 ACP 会话：启动 Agent 并登记会话（`open_session`），会话结束时撤销登记、撤回它等待的决定并停止 Agent（`close_session`），以及会话打开时的参数（`opening`：成员的资源、看板和系统层）和角色要求的控制项。主控和每个任务的会话用 `SessionKey` 区分，任务 id 可以取任何看板允许的值 |
| `backend/src/orchestrator/prompts.rs` | 发给会话的说明和任务提示，以及结构化结果的解析 |
| `backend/src/orchestrator/actions.rs` | 用户操作、验收归档的交付、重启后的恢复与继续 |
| `backend/src/orchestrator/tests.rs`、`flow_tests.rs`、`flow_tests/` | 编排器的单元测试和端到端流程测试 |
| `backend/src/teamwork.rs` | 编排中的团队结构：任务负责的文件（`owns`）和独立验证（`verifies`）的校验、协作者的认领、谁负责某个文件、协作者提示中的对应段落、主控说明中「团队怎么组织」一节 |
| `backend/src/goal_settle.rs` | 主控的 `/goal` 一直不自己结束时：会话安静一段时间后问主控，确认完成后通过 `_session/goal` 清除目标 |
| `backend/src/acp_view.rs` | ACP 会话中人能看到的部分：把思考、计划、斜杠命令、模式和选项变化、用量、工具调用的 diff 转成事件；会话打开时的选项；消息里 `@路径` 的文件引用；`@` 菜单的文件列表 |
| `backend/src/evidence.rs` | 验收证据：check 的证据方法、结果核对（`gaps`）、证据汇总（`Summary`）和交付中每个任务的验收记录 |
| `backend/src/usage.rs` | Run 级用量账本：从事件日志实时派生的只读汇总，覆盖 Run、任务、成员、尝试和单个会话；每轮的 token 相加并按模型归集，上下文快照不计入，缺失如实标注，不改调度、预算和交付 |
| `backend/src/bill.rs` | 按时间段的用量账单：跨 Run 按 Agent 和模型汇总区间里的 token 和上报的费用 |
| `backend/src/prices.rs` | 模型价格：每天从 models.dev 同步，每个模型取厂商的 API 价格，给账单估算花费。正在用的价格表（`PriceBook`）由 `App` 持有 |
| `backend/src/traces.rs` | 任务在工作目录里留下的痕迹：每个任务改过的文件和每个文件最后的修改者、diff 产物的范围、结果与痕迹的对账、完成后又被改过的文件 |
| `backend/src/status.rs` | Run、任务、尝试和 Agent 阶段的状态枚举；Run 和任务的合法转移集中在 `can_transition_to` |
| `backend/src/db.rs` | SQLite 表结构与读写 |
| `backend/testdata/fake_acp.py` | 测试用的脚本化 ACP Agent |
| `frontend/` | macOS 应用（Swift Package），结构见 [macOS 应用](frontend.md#source_layout) |

`agents.rs`、`adapters.rs`、`library.rs` 和 `orchestrator` 的各个子模块都在 `impl App` 上添加方法；`App` 结构体定义在 `orchestrator.rs`。

<a id="startup"></a>
## 启动顺序

`main` 按以下顺序执行：

1. 处理命令行参数（`--version`、`--help`）。读取配置项：Janus home（`JANUS_HOME`，默认 `~/.janus`，权限设为 `0700`）、监听地址和 Node 路径，见[配置](../interfaces/configuration.md#settings)。
2. 打开 SQLite 并建表。
3. `recover`：把上次未结束的尝试标记为中断，未结束的 Run 标记为 `interrupted`。
4. `seed_roles`：第一次启动时写入 5 个组成产品上线团队的默认角色，之后不再写入（删掉的默认角色不会回来），见[角色](../domains/agents_and_roles.md#roles)；`reset_stale_checks`：重启前没跑完的配置检查改回「未检查」。
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
