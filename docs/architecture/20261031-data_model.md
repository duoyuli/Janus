# 数据模型与存储

本文说明 Janus 把数据存在哪里：Janus home 的布局、SQLite 表、Run 快照和产物。事件本身的字段和类型见[事件参考](../interfaces/20261031-events.md)，环境变量见[配置](../interfaces/20261031-configuration.md)。

## 导航

- [存储位置](#locations)：找数据库、日志、产物放在哪里时阅读。
- [SQLite 表](#sqlite_tables)：修改表结构或查询时阅读。
- [数据升级](#migrations)：改了存储的数据、要加迁移时阅读。
- [Run 快照](#snapshots)：修改 Run 创建时固定的配置时阅读。
- [产物](#artifacts)：修改产物的保存和读取时阅读。

<a id="locations"></a>
## 存储位置

Janus 的全部数据都在 Janus home（`JANUS_HOME`），仓库不保存运行数据。目录规范和每一类的用途见[部署方案](../operations/20261031-deployment.md#layout)，下面是与数据有关的部分：

```text
$JANUS_HOME（默认 ~/.janus，权限 0700）
├── data/
│   ├── janus.db                          # SQLite（WAL 模式）
│   ├── secrets/                          # 0700
│   │   ├── agents/<agent-id>.json        # Agent 的凭据字段和环境变量中的私有值，权限 0600
│   │   └── library/<item-id>.json        # 资源库 MCP 服务器的环境变量和 HTTP header 值，权限 0600
│   ├── roles/<role-id>/avatar.png        # 数字员工的头像，256×256 PNG
│   ├── agents/<agent-id>/
│   │   ├── config/                       # 交给 Agent 的配置目录（CODEX_HOME、CLAUDE_CONFIG_DIR）
│   │   └── home/                         # Agent 进程的 HOME 和 XDG 目录
│   ├── library/
│   │   ├── skills/<sha256>/              # Skill 的内容，按内容寻址，存下后不再修改
│   │   ├── sources/<source-id>/repo/     # Git 来源的浅克隆
│   │   └── servers/<item-id>/            # MCP 路由启动的命令服务器的 HOME
│   ├── runs/<run>/                       # 任务
│   │   ├── bundle/<member>/              # 成员的资源包：Run 创建时复制的 Skill
│   │   ├── attachments/                  # 随目标和消息粘贴的图片
│   │   └── artifacts/<task>/             # 产物：回复和 diff
│   └── chats/<run>/                      # 对话：bundle/<member>/、attachments/
├── work/runs/<run>/                      # Git 仓库上任务的 worktree
├── work/chats/<run>/                     # 对话的工作目录：每个对话新建一个，删除对话时一起删除
├── cache/adapters/<id>/                  # ACP 适配器，见 ACP 适配器管理
├── tmp/agents/<agent-id>/                # Agent 的 TMPDIR，启动时清空
└── logs/runs/<run>/、logs/chats/<run>/   # 每个 ACP 子进程的 stderr
    ├── lead-goal.log                     # 目标模式负责人
    ├── lead-chat.log                     # 交付或验收后继续对话时的负责人会话
    ├── chat.log                          # 对话中数字员工的会话
    └── <task>-a<n>.log                   # 任务的第 n 次尝试
```

路径都来自 `backend/src/layout.rs` 的 `Layout`，任务和对话按 `runs.kind` 落在 `runs/` 或 `chats/` 下（`Layout::run`）。

适配器的目录结构见 [ACP 适配器管理](20261031-adapters.md#layout)。

<a id="sqlite_tables"></a>
## SQLite 表

数据版本 0 的表结构是 `migrations::BASELINE`（`backend/src/migrations.rs`），之后的表结构变化都是迁移，见[数据升级](#migrations)。

| 表 | 主键 | 内容 |
| --- | --- | --- |
| `runs` | `id` | 类型（`kind`：`goal` 目标或 `chat` [对话](../domains/20261120-chats.md)）、目标（对话是第一条消息）、工作目录（Git 仓库上的 Run 是它自己的 worktree `work/runs/<run>`，对话是 `work/chats/<run>`）、状态、创建时间；`config` 是 Run 开始时的快照（团队、权限摘要、是否 Git、基线、工作区，worktree 中的 Run 另有 `worktree.repo` 和 `worktree.branch`） |
| `events` | `(run_id, seq)` | 事件日志，Run 的唯一事实来源 |
| `attempts` | `(run_id, task, n)` | 每次尝试的执行者、运行时、会话、工作目录、状态、错误、起止时间 |
| `artifacts` | 无 | 产物的路径、类型、SHA-256、大小 |
| `configs` | `(kind, id)` | 设置对象：`connection`、`role`、`workspace`，资源库的 `library_item`、`library_source`，以及 `meta` 中的标记（例如默认角色已经写入过的标记 `seeded`）；各类的字段登记在 `data_shape.rs` |
| `verifications` | `(run_id, task, attempt, attempt_id)` | 每次验证的结果和细节 |
| `commands` | `(run_id, command_id)` | 已占用的 Run 操作 id：执行前写入（`state` 为 `running`），生效后改为 `done`，失败时删除；服务启动时把还是 `running` 的改为 `interrupted`（`CommandState`）。用于按 `commandId` 只执行一次 |

约束：

- `configs` 不保存凭据。Agent 的凭据在 `data/secrets/agents/<id>.json`，资源库 MCP 服务器的环境变量和 HTTP header 值在 `data/secrets/library/`，`configs` 中只记录变量名（`envKeys`、`headerKeys`）。
- 事件按 `seq` 递增，`seq` 在单个 Run 内唯一。应用用 `after` 参数从某个 `seq` 之后续读。
- `events` 另有索引 `events_type_ts (type, ts)`，按时间段的用量账单用它找出区间里有 `session.usage` 的 Run，不必读全部事件（[用量账单](../interfaces/20261031-http_api.md#usage_bill)）。
- 一个 Janus home 同一时间只有一个服务（[`janus.pid` 的独占锁](../operations/20261031-deployment.md#single_instance)），所以数据库里不记录 Run 归哪个进程驱动。同一个服务里，恢复、验收历史交付和删除一个不在运行的 Run 依次进行（`App.revival`），见 [Run 生命周期](../domains/20261031-run_lifecycle.md#recovery)。
- 删除一个 Run 会删掉 `runs` 和其余各表中属于它的行、它在 `data/`、`work/`、`logs/` 下的目录（`RunDirs`），以及它的会话在 Agent 配置目录里留下的会话记录；工作区的目录不受影响。正在进行的 Run 不能删除，规则见 [HTTP API](../interfaces/20261031-http_api.md#run_delete)。

<a id="migrations"></a>
## 数据升级

`janus.db` 的数据版本记在 `PRAGMA user_version`，当前版本是 `migrations::latest()`。设计取舍见 [ADR：数据随版本自动升级](../decisions/20261118-data_migrations.md)。

- **执行时机**：服务启动时 `Db::open` 对数据版本 0 的库（新库或有迁移之前的库）先建出版本 0 的表（`BASELINE`），再按顺序执行版本号大于当前版本的迁移（`migrations::upgrade`）。版本 0 以后的库不再执行 `BASELINE`，否则迁移删掉的表会被重新建出来。每条迁移在一个事务中执行并写入自己的版本号，失败时回滚，服务不启动。新数据库同样从 0 执行到最新，不需要备份。
- **备份**：库里已有数据时，升级前用 `VACUUM INTO` 复制到 `<Janus home>/backups/janus-v<旧版本>-<毫秒时间>.db`，只保留最近 5 份。日志写明从哪个版本升到哪个版本、每条迁移改了几处和备份位置。
- **更新的数据**：数据版本高于当前版本时拒绝打开，提示升级 Janus 或从 `backups/` 恢复。
- **迁移的写法**：`MIGRATIONS` 中每条是一个版本号、一句说明和若干步骤。`Sql` 改表结构；`Add` 加字段，旧对象取给定的默认值（已有这个字段的不变）；`Drop` 删字段；其他情况（改名、由其他字段算出的默认值、改类型）用 `Custom` 函数；也要改 Janus home 中文件的（凭据、移动目录）用 `Home`，它拿到那个数据版本时的 Janus home，`--dry-run` 时拿不到、不改文件。字段步骤的目标是某类设置对象（`Target::Config(kind)`）或 Run 快照中的每个成员（`Target::RunMember`），重复执行不会再改动。事件不迁移，唯一的例外是移动目录时把事件里的旧路径改成新路径（`backend/src/layout/relocate.rs` 的 `Relocation`）：改的是文件现在在哪，不是发生了什么。
- **登记的结构**：`data_shape.rs` 登记每类设置对象的必填和可选字段（`connection` 另外允许 Agent 类型目录中登录方式的普通字段），资源库对象和 Run 快照成员按读回再写出必须不变检查。测试中每次 `put_config` 都按它检查；`janus migrate --dry-run` 在库的副本上执行升级，列出每条迁移改了几处和仍不符合当前结构的对象，不修改原库。
- **快照**：表结构和登记的字段记在 `backend/testdata/data_shape.json`（`janus migrate --shape` 生成，`just data-shape` 写入）。结构变了但数据版本没变时测试失败，提示先加迁移。

已有的迁移：

| 版本 | 内容 |
| --- | --- |
| 1 | 删除数字员工的「定位」（`role`），删除 Run 快照成员的 `role` 和 `model_label` |
| 2 | 默认数字员工记录来源（`seed`），见[默认角色：产品上线团队](../domains/20261031-agents_and_roles.md#product_launch_roles) |
| 3 | `commands` 表加 `state` 列（`running`、`done`、`interrupted`），已有的行取 `done` |
| 4 | 删除 `leases` 表：Run 租约和目录锁重复，持有者按进程号判断存活，旧进程号被其他进程复用时会误拒恢复 |
| 5 | 资源和 Run 快照中的资源加 `reach`：Skill 取 `everyone`，MCP 取 `picked`（版本 11 又删除） |
| 6 | 已经创建过默认数字员工的数据加上通用数字员工（`generalist`），见[默认角色](../domains/20261031-agents_and_roles.md#product_launch_roles) |
| 7 | 去掉预设的 Agent 类型：`deepseek-claude`、`minimax-claude`、`glm-claude` 的 Agent 改为用 API Key 登录的 `claude`，预设的变量写进它的 `env`，去掉 `endpoint`，状态改为未检查，见 [Agent 类型](../domains/20261031-agents_and_roles.md#providers) |
| 8 | 命令启动的 MCP 服务器（资源和 Run 快照中的资源）加 `server.process`，已有的取 `perRun`（同一个 Run 共用一个进程），见 [MCP 路由](../domains/20261116-resource_library.md#mcp_router) |
| 9 | 去掉「自定义命令」的 Agent：删除 `base` 为 `custom` 的 Agent，绑定它的数字员工改为没有绑定 Agent，Run 快照中用它的成员记为 `claude`（只能查看，不能继续），见 [ADR](../decisions/20261120-codex_and_claude_code_only.md) |
| 10 | worktree 中的 Run 的快照加 `worktree.branch`：取升级时工作目录检出的分支，这个分支的历史不含 Run 的起点（`baseRef`）时取 `null`，见 [ADR：验收时把结果合并回任务开始时的分支](../decisions/20261120-merge_back_on_accept.md) |
| 11 | 每项资源都给所有数字员工：资源和 Run 快照中的资源删除 `reach`，数字员工删除 `resources`，见 [ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](../decisions/20261120-library_routed_only.md) |
| 12 | `runs` 表加 `kind` 列（`goal`、`chat`），已有的 Run 都取 `goal`，见[对话](../domains/20261120-chats.md) |
| 13 | Claude 连接的自定义 API（`apikey`）改用环境变量：原来单独保存的密钥作为 `ANTHROPIC_API_KEY` 继续使用。手动填写的私有变量移入 Agent 的凭据文件（当时是 `agents/<id>/secrets.json`，版本 14 移到 `data/secrets/agents/<id>.json`），连接的 `env` 将对应值记为 `null`，见[凭据保存](../operations/20261031-security.md#secrets) |
| 14 | Janus home 按类别整理（这一步除了数据库还移动文件，`Step::Home`）：服务打开数据库前先把顶层的 `janus.db` 移到 `data/`；再把旧布局的每个目录移到新位置（Agent 的 `home/` 改名 `config/`、`root/` 改名 `home/`，凭据移到 `data/secrets/`，适配器、价格表、工具列表和应用更新移到 `cache/`，Agent 和服务器的 `tmp/` 删除），数据库（`runs`、`attempts`、`artifacts`、事件）和 `data/agents/`、`data/library/servers/` 下进程自己写的文本文件和符号链接中记下的旧路径按完整前缀改成新路径，Claude Code 的项目目录随工作目录改名，worktree 用 `git worktree repair` 告诉原仓库新位置，见 [ADR：Janus home 按「丢了能不能再有」分类](../decisions/20261121-home_layout.md) |
| 15 | 已经写入过默认角色的数据加上默认的「老友」（当时的 id 是 `inner-grill`，版本 17 改为 `friend`），没有绑定 Agent；用户已有同 id 的角色时保留用户的，文本和默认完全一样时只补上 `seed`。数据版本 2 的迁移只认当时就有的默认角色（`SEED_HISTORY` 中的），不再把用户自己用了之后默认角色 id 的角色当作默认角色，见[默认角色](../domains/20261031-agents_and_roles.md#product_launch_roles) |
| 16 | 数字员工可以有自己的头像：角色加 `avatar`（图片的哈希），已有的取空字符串（显示文字），见[头像](../domains/20261031-agents_and_roles.md#avatars) |
| 17 | 默认数字员工的 id 改为它的职能：`coordinator`→`pmo`、`researcher`→`marketing`、`implementer`→`developer`、`reviewer`→`qa`、`inner-grill`→`friend`（`RENAMED`）。默认数字员工（`seed.id` 是旧 id 的）的记录、工作区的负责人和成员、历史任务和对话的成员（`role_id`）、头像目录一起改；用户自己用旧 id 建的角色、新 id 已被占用的不改。更早的迁移按当时的 id 写入，所以从任何旧版本升级都先得到旧 id，再由这一版改名 |

<a id="snapshots"></a>
## Run 快照

目标 Run 开始后（离开[派发队列](../domains/20261031-run_lifecycle.md#dispatch_queue)时）不再读取最新的设置对象。排队中的 Run 的 `runs.config` 只有工作区、`goalImages` 和 `tokenLimit`，`runs.cwd` 是工作区目录；开始时才写入下面的完整快照：

- 团队成员、角色、模型、会话控制项、权限摘要和资源写入 `runs.config`，工作区（含[共享资料](../domains/20261031-agents_and_roles.md#workspace_brief)）写入 `runs.config.workspace`。对话的快照只有一名成员（`team`），没有工作区。
- 成员的资源（`resources`）是整个资源库当时的完整记录，Skill 复制到成员的资源包 `data/runs/<run>/bundle/<member>/`（对话在 `data/chats/<run>/` 下），之后修改资源库不影响已有 Run，见 [Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)。
- Git 仓库上的 Run 在自己的 worktree 中运行：`runs.cwd` 是 worktree 的路径，`runs.config.worktree.repo` 是原仓库，`runs.config.worktree.branch` 是开始时检出的分支（验收时合并回这里，detached HEAD 时为 `null`），没有 `worktree` 的 Run 直接在工作区目录中运行（非 Git 目录，或者这个功能之前创建的 Run）。见[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)。
- 随目标粘贴的图片保存在 `data/runs/<run>/attachments/`，路径列表写入 `runs.config.goalImages`，从中断处继续时负责人说明里仍有这些图片，见[图片附件](../domains/20261031-run_lifecycle.md#attachments)。
- 怎样连上 Agent（启动命令、凭据、环境变量、适配器版本）不进快照，属于 Agent 的设置：每次打开会话时按当前设置读取（`spec_for`），凭据也因此不会写进 `runs.config`。快照冻结的是角色要求的东西。
- MCP 服务器的快照只有名称、命令、地址和 `envKeys` / `headerKeys`；值在会话启动时从 `data/secrets/library/` 读取，直接放进会话参数，所以 `runs.config` 和事件中没有值。

恢复 Run 时，团队从 `runs.config` 重建，任务状态从事件日志重建，见 [Run 生命周期](../domains/20261031-run_lifecycle.md#recovery)。

<a id="artifacts"></a>
## 产物

每次尝试结束时保存：

| 文件名 | 类型（`kind`） | 条件 |
| --- | --- | --- |
| `<task>-a<n>-report.md` | `report` | 总是保存，内容是 Agent 的最终回复 |
| `<task>-a<n>.diff` | `git_diff` | 工作目录是 Git 仓库，任务负责和改过的文件相对 `HEAD` 有未提交改动；两样都没有记录时是整个工作目录的改动，且工作目录与上一个任务结束时不同，见[工作目录里的痕迹](../domains/20261031-goal_mode.md#traces) |

目标模式下，diff 的比较基准从 Run 开始时的工作目录状态算起，Run 之前就存在的改动不会算到任务头上。

产物保存在 `data/runs/<run>/artifacts/<task>/`，只能通过 `GET /api/file` 读取，且路径必须是这样的产物文件（`Layout::is_artifact`），见 [HTTP API](../interfaces/20261031-http_api.md#files)。
