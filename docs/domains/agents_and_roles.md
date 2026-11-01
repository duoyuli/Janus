# Agent、角色与工作区

本文说明三类设置对象之间的关系和规则：Agent（Provider 连接）、角色、工作区，以及权限模式、Skill 和 MCP 怎样进入一次会话。运行环境的隔离方式见[安全边界](../operations/security.md#agent_isolation)，设置接口见 [HTTP API](../interfaces/http_api.md#setup)。

关系：工作区由多个角色组成，每个角色绑定一个 Agent。Run 创建时把这三层合并成团队快照，之后修改设置不影响已有 Run。

<a id="providers"></a>
## Agent 类型

`agents::kinds()` 定义了可创建的 Agent 类型：

| 类型 | 分组 | 基础运行时 | 认证方式 | 配置目录变量 |
| --- | --- | --- | --- | --- |
| `codex` | 内置 | `codex` | 复制本机配置、ChatGPT 登录、自定义 API（OpenAI 或兼容 Responses API 的网关） | `CODEX_HOME` |
| `claude` | 内置 | `claude` | 复制本机配置、Claude 订阅 OAuth Token、API Key | `CLAUDE_CONFIG_DIR` |
| `grok` | 内置 | `grok` | 复制本机配置、xAI 账号设备码登录、API Key | `GROK_HOME` |
| `pi` | 内置 | `pi` | 复制本机配置、OpenAI 兼容服务 | `PI_CODING_AGENT_DIR` |
| `deepseek-claude`、`minimax-claude`、`glm-claude` | 预设 | `claude` | 粘贴对应厂商的 token，通过预设的环境变量（可修改）把 Claude Code 指向厂商的 Anthropic 兼容接口 | `CLAUDE_CONFIG_DIR` |
| `custom` | 自定义 | `custom` | 无；只使用填写的启动命令和环境变量 | 无 |

规则：

- Agent id 由名字生成（小写字母和 `-`），可以手动指定，不能重复。已有 Agent 不能修改类型。
- 内置运行时通过 ACP 适配器启动。适配器在第一次检查用到它的 Agent 时自动安装，见 [ACP 适配器管理](../architecture/adapters.md#on_demand)。
- 有正在运行的任务使用某个 Agent 时，不能删除它。
- 「复制本机配置」只复制与模型接入有关的配置项和凭据，见[安全边界](../operations/security.md#local_config_import)。

<a id="agent_env"></a>
### 环境变量

Agent 进程不继承 Janus 自己的环境（见[安全边界](../operations/security.md)），它看到的变量按以下顺序叠加，同名时后面的覆盖前面的：

1. 沙箱变量：`HOME`、XDG 目录、`TMPDIR` 等，指向 `~/.janus/agents/<id>/` 下的隔离目录。
2. 类型的默认变量（`fixedEnv`，预设还有所选接入点的 `env`），例如 DeepSeek 预设的 `ANTHROPIC_BASE_URL` 和各档模型，Grok 的 `GROK_DISABLE_AUTOUPDATER=1`。
3. 用户在 Agent 上填写的环境变量，以及「复制本机配置」带来的变量。值为空（`KEY=`）表示去掉这个变量。沙箱变量不能在这里改写。
4. 隔离目录变量（如 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`）、运行时路径和凭据（如预设 token 对应的 `ANTHROPIC_AUTH_TOKEN`）。

编辑页把第 2 步的默认值直接填进环境变量框，用户可以逐项修改；切换接入点时只替换该接入点的变量，用户自己加的行保留。保存后这些值成为 Agent 自己的配置，之后预设的默认值变化不会影响已经填写的项。

<a id="connection_check"></a>
## 配置检查

`test_connection` 按顺序执行以下步骤，结果在页面上逐步显示：

1. **ACP 适配器**：内置运行时需要的适配器没有安装时，当场安装最新版本。自定义命令没有这一步。
2. **运行时**：找到 CLI 和适配器入口。
3. **凭据**：检查所选认证方式需要的凭据是否齐全。
4. **启动 Agent**：启动子进程并完成 ACP `initialize`，60 秒超时。
5. **打开会话**：`session/new`，读取 Agent 声明的模型、推理强度和会话控制项，保存到连接的 `options`。
6. **试运行**：发送一句测试消息，确认有回复。

第 5 步读到的控制项是角色编辑器的选项来源。Agent 升级后控制项变化时，重新检查即可刷新。

<a id="permission_modes"></a>
## 会话控制项与权限模式

Janus 不定义自己的权限模型，而是使用每个 Agent 在 ACP 会话中声明的控制项（`session_options`）：

- `category` 为 `model` 的控制项是模型，`thought_level` 是推理强度。
- 其余 `select` 和 `boolean` 类型的控制项都是会话控制项（`controls`）。
- 旧版 Agent 用 ACP session modes 报告模式时，转换成 id 为 `mode` 的控制项。

一个控制项满足以下任一条件时，被视为权限模式（`permission: true`），在角色编辑器的「权限」区域显示：

- `category` 是 `mode` 或 `_permission`。
- id（`-` 视为 `_`）是 `mode`、`permission_mode`、`plan_mode`、`approval_policy` 或 `sandbox`。

各 Agent 声明的典型权限模式：Codex 的沙箱预设（Read-only / Agent / Full access 等）、Claude Code 的权限模式（Manual / Accept edits / Auto / Bypass 等）、Grok 的权限模式。具体选项以 Agent 实际声明为准。

<a id="no_plan_mode"></a>
### 不使用 Plan 模式

Janus 不让任何 Agent 进入 Plan 模式。Plan 模式只出方案、不执行，而 Janus 的任务要真正完成；Codex 主控在 Plan 模式下还会跳过 `/goal` 的自动推进（日志为 `skipping goal continuation ... reason=PlanMode`），Run 会一直停在「目标进行中」。

- `session_options` 把 id 含 `plan` 的控制项（`is_plan_toggle`，例如 Codex 的 `plan-mode` 是 on / off 下拉，Grok 的 `plan_mode` 是布尔开关）从 `controls` 中移到 `planToggles`，并从所有下拉控制项中去掉值为 `plan` 的选项（`is_plan_value`，例如 Claude Code 的 Plan 权限模式和旧版 Agent 的 `plan` session mode）。角色编辑器因此看不到这些选项；在这条规则之前检查过的 Agent，页面按同样规则过滤它保存的选项。
- `role_session` 丢弃角色中与 Plan 有关的值，旧角色里开着的 Plan 不再生效，保存角色时也会被清掉。
- 打开会话时，`configure_session` 把仍处于开启状态的 Plan 控制项设为关闭值（`plan_off_value`：布尔为 `false`，下拉为 `off` 一类的选项）；下拉控制项的当前值是 `plan` 且角色没有指定时，改为第一个其他选项。关闭失败时会话以错误结束，不会带着 Plan 模式运行。

Janus 不从权限模式推导成员「会修改文件 / 只读」：页面和主控收到的协作者名单都只显示 Agent 自己的权限模式名，实际能做什么由 Agent 按自己的模式执行，Janus 不做额外校验。设计取舍见 [ADR：权限交给 Agent 原生模式](../decisions/agent_native_permissions.md)。

<a id="roles"></a>
## 角色

| 字段 | 说明 |
| --- | --- |
| `name`、`role`、`desc` | 名字、角色名、职责。职责会写进主控的协作者名单 |
| `connection` | 绑定的 Agent id |
| `model`、`effort` | 模型和推理强度；留空表示使用 Agent 默认值 |
| `session` | 选定的会话控制项值，`{控制项 id: 值}` |
| `prompt` | 角色指令，作为用户的长期要求注入：Claude Code 是系统提示，Codex 是开发者指令，Pi 通过 Janus 的扩展加入系统提示，其他 Agent 附在任务说明末尾，见 [ACP 集成](../interfaces/acp_integration.md#standing_instructions) |
| `skills` | Skill 库中的 Skill id |
| `mcp` | stdio MCP 服务器：名称、命令、参数、环境变量名（值另存） |
| `lead` | 是否可以担任主控 |
| `color`、`init` | 页面上的颜色和缩写 |

规则：

- 保存时，`session` 中只保留与 Agent 声明的控制项匹配的值（`role_session`）。Agent 还没检查过、没有控制项时，先保留字符串和布尔值。
- MCP 名称只能包含字母、数字、`-` 和 `_`。环境变量值写入 `secrets/roles/<role-id>.json`（`0600`），SQLite 只记录变量名。
- 被工作区使用的角色不能删除。
- 首次启动时写入四个默认角色：Lead（主控）、Researcher（调研）、Implementer（实现）、Reviewer（评审），它们都还没有绑定 Agent。

<a id="workspaces"></a>
## 工作区

| 字段 | 说明 |
| --- | --- |
| `path` | 工作目录，必须存在，保存为规范化的绝对路径 |
| `members` | 成员列表，每项是 `{role: <角色 id>}` |
| `lead` | 主控的角色 id，必须在成员中 |
| `name` | 默认取目录名 |

规则：

- 主控的角色必须绑定 Claude Code 或 Codex，见[目标模式](goal_mode.md#lead_role)。
- 除主控外至少还要有一名成员。
- Run 创建时，主控的成员 id 变为 `lead`；如果某个角色的 id 本身是 `lead` 但不是主控，改为 `lead-role`。

<a id="role_stats"></a>
## 角色的执行统计

`role_stats`（`GET /api/stats/roles`）按角色汇总最近 50 个 Run（与 Run 列表相同的范围）中协作者任务的结果，用来判断该换 Agent 或模型、改角色指令，还是主控的任务写法有问题：

| 字段 | 含义 |
| --- | --- |
| `tasks` | 结束了的任务数（成功或失败；被重试的任务按最后一次的结果计） |
| `succeeded` / `failed` | 成功 / 失败的任务数 |
| `firstPass` | 第一次尝试就成功、且验收核对一次通过的任务数 |
| `verificationFailed` | 至少有一次没通过验收核对的任务数（漏了标准、缺证据或自认没达到） |
| `avgMinutes` | 成功任务从第一次开始到完成的平均分钟数 |

任务按 Run 快照中成员的 `role_id` 归到角色，主控不计入。统计按角色而不是按当时绑定的 Agent 归类，角色换过 Agent 或模型后，新旧数据会合在一起。

角色列表在每个角色下显示这些数字；运行页的 Agent 状态面板显示每个协作者本次 Run 的「首次通过 / 已结束」和验证没通过的任务数。

<a id="member_snapshot"></a>
## 成员快照

`team_for` 把工作区展开成 Run 的成员（`Member`），包括：角色字段、Agent 类型与名称、会话控制项、权限模式标签（例如「Agent · Plan 开启」）、模型标签（例如「GPT-6.1 Sol，推理强度 high（Agent 默认）」）、角色指令和 Provider 的自定义提示词、Skill 和 MCP。

这些字段同时用于：打开会话时设置控制项、主控说明中的协作者名单、页面「团队与权限」标签页。

<a id="skills"></a>
## Skill

- **Janus Skill 库**：`$JANUS_HOME/skills/<id>/SKILL.md`，在页面中新建，或从用户选择的目录复制一次。复制时只复制普通文件，符号链接取其内容，不跟随到所选目录之外。Agent 不读取用户自己的 Agent Skill 目录。
- **Run 快照**：Run 创建时，成员的 Skill 复制到 `~/.janus/runs/<run>/skills/`，提示中给出快照路径。
- **项目 Skill**：工作目录中 `.agents/skills`、`.claude/skills`、`.codex/skills`、`.pi/skills`、`.janus/skills` 下的 Skill，只要真实路径仍在工作目录内，就会列在每个会话的提示中。

提示只列出 Skill 的名字、描述和 `SKILL.md` 路径，由 Agent 自己决定是否读取。

<a id="mcp"></a>
## MCP

- 角色配置的 MCP 是 stdio 服务器，在 `session/new` 的 `mcpServers` 中传给 Agent，环境变量值在此时从 `secrets/roles/` 读取。
- 目标模式下，Janus 还会附加自己的 HTTP 看板服务（名称 `janus`），见[看板 MCP 工具](../interfaces/board_mcp.md)。
