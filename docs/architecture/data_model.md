# 数据模型与存储

本文说明 Janus 把数据存在哪里：Janus home 的布局、SQLite 表、Run 快照和产物。事件本身的字段和类型见[事件参考](../interfaces/events.md)，环境变量见[配置](../interfaces/configuration.md)。

<a id="locations"></a>
## 存储位置

Janus 的全部数据都在 Janus home（`JANUS_HOME`），仓库不保存运行数据。目录规范和每一类的用途见[部署方案](../operations/deployment.md#layout)，下面是与数据有关的部分：

```text
$JANUS_HOME（默认 ~/.janus，权限 0700）
├── janus.db                          # SQLite（WAL 模式）
├── agents/<agent-id>/                # 每个 Agent 一个目录
│   ├── home/                         # 交给 Agent 的配置目录（CODEX_HOME、CLAUDE_CONFIG_DIR 等）
│   ├── root/                         # Agent 进程的 HOME 和 XDG 目录
│   ├── tmp/                          # TMPDIR
│   └── secrets.json                  # Agent 的凭据字段，权限 0600
├── secrets/roles/<role-id>.json      # 角色手工 MCP 的环境变量值，权限 0600
├── secrets/mcp/<source-id>/<key>.json # MCP 资源库服务器的环境变量和 HTTP header 值，权限 0600
├── skills/<skill-id>/SKILL.md        # Janus Skill 库（自建的 Skill）
├── skill-sources/<source-id>/        # GitHub Skill 来源
│   ├── source.json                   # 仓库、ref、目录、提交、Skill 索引（key 与目录）、更新检查结果
│   └── repo/                         # 仓库的浅克隆
├── mcp-sources/<source-id>/          # GitHub MCP 来源
│   ├── source.json                   # 仓库、ref、目录、提交、服务器索引、解析警告、更新检查结果
│   └── repo/                         # 仓库的浅克隆
├── adapters/<id>/                    # ACP 适配器，见 ACP 适配器管理
├── artifacts/<run>/<task>/           # 产物：回复和 diff
├── logs/<run>/                       # 每个 ACP 子进程的 stderr
│   ├── lead-goal.log                 # 目标模式主控
│   └── <task>-a<n>.log               # 任务的第 n 次尝试
└── runs/<run>/skills/                # Run 创建时的 Skill 快照
```

适配器的目录结构见 [ACP 适配器管理](adapters.md#layout)。

<a id="sqlite_tables"></a>
## SQLite 表

表结构定义在 `Db::open`（`backend/src/db.rs`），启动时用 `CREATE TABLE IF NOT EXISTS` 创建。

| 表 | 主键 | 内容 |
| --- | --- | --- |
| `runs` | `id` | 目标、工作目录、状态、创建时间；`config` 是 Run 创建时的快照（团队、权限摘要、是否 Git、基线、工作区） |
| `events` | `(run_id, seq)` | 事件日志，Run 的唯一事实来源 |
| `attempts` | `(run_id, task, n)` | 每次尝试的执行者、运行时、会话、工作目录、状态、错误、起止时间 |
| `artifacts` | 无 | 产物的路径、类型、SHA-256、大小 |
| `configs` | `(kind, id)` | 设置对象：`connection`、`role`、`workspace`，以及 `meta` 中的标记（例如默认角色的版本 `seeded`） |
| `verifications` | `(run_id, task, attempt, attempt_id)` | 每次验证的结果和细节 |
| `commands` | `(run_id, command_id)` | 已占用的 Run 操作 id：执行前写入，失败时删除，用于按 `commandId` 只执行一次 |
| `leases` | `(run_id, name)` | Run 租约，防止两个 Janus 实例驱动同一个 Run |

约束：

- `configs` 不保存凭据。Agent 的凭据在 `secrets.json`，角色手工 MCP 的环境变量值在 `secrets/roles/`，MCP 资源库服务器的环境变量和 HTTP header 值在 `secrets/mcp/`，`configs` 中只记录变量名（`envKeys`、`headerKeys`）。
- 事件按 `seq` 递增，`seq` 在单个 Run 内唯一。应用用 `after` 参数从某个 `seq` 之后续读。
- `events` 另有索引 `events_type_ts (type, ts)`，按时间段的用量账单用它找出区间里有 `session.usage` 的 Run，不必读全部事件（[用量账单](../interfaces/http_api.md#usage_bill)）。
- 租约有效期 10 分钟。持有者是 `janus-<pid>`；该进程已经不存在时，其他实例可以接管。

<a id="snapshots"></a>
## Run 快照

Run 创建后不再读取最新的设置对象：

- 团队成员、角色、模型、会话控制项、权限摘要、Skill 和 MCP 写入 `runs.config`，工作区（含[共享资料](../domains/agents_and_roles.md#workspace_brief)）写入 `runs.config.workspace`。
- 成员的 Skill 复制到 `~/.janus/runs/<run>/skills/`，Agent 读的是快照路径，之后修改 Skill 库不影响已有 Run。
- 随目标粘贴的图片保存在 `~/.janus/runs/<run>/attachments/`，路径列表写入 `runs.config.goalImages`，从中断处继续时主控说明里仍有这些图片，见[图片附件](../domains/run_lifecycle.md#attachments)。
- 怎样连上 Agent（启动命令、凭据、环境变量预设、适配器版本）不进快照，属于 Agent 的设置：每次打开会话时按当前设置读取（`spec_for`），凭据也因此不会写进 `runs.config`。快照冻结的是角色要求的东西。
- 成员的 MCP 定义包含手工 MCP 和 `mcpRefs` 展开的资源库服务器。快照只保存名称、命令、地址和 `envKeys` / `headerKeys`；值在会话启动时从 `secrets/roles/` 和 `secrets/mcp/` 读取，`Member.mcp_env` / `mcp_headers` 不序列化，所以 `runs.config` 和事件中没有值。

恢复 Run 时，团队从 `runs.config` 重建，任务状态从事件日志重建，见 [Run 生命周期](../domains/run_lifecycle.md#recovery)。

<a id="artifacts"></a>
## 产物

每次尝试结束时保存：

| 文件名 | 类型（`kind`） | 条件 |
| --- | --- | --- |
| `<task>-a<n>-report.md` | `report` | 总是保存，内容是 Agent 的最终回复 |
| `<task>-a<n>.diff` | `git_diff` | 工作目录是 Git 仓库，任务负责和改过的文件相对 `HEAD` 有未提交改动；两样都没有记录时是整个工作目录的改动，且工作目录与上一个任务结束时不同，见[工作目录里的痕迹](../domains/goal_mode.md#traces) |

目标模式下，diff 的比较基准从 Run 开始时的工作目录状态算起，Run 之前就存在的改动不会算到任务头上。

产物只能通过 `GET /api/file` 读取，且路径必须位于 `~/.janus/artifacts/` 内，见 [HTTP API](../interfaces/http_api.md#files)。
