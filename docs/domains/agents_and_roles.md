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
| `deepseek-claude`、`minimax-claude`、`glm-claude` | 预设 | `claude` | 粘贴对应厂商的 token，通过固定环境变量把 Claude Code 指向厂商的 Anthropic 兼容接口 | `CLAUDE_CONFIG_DIR` |
| `custom` | 自定义 | `custom` | 无；只使用填写的启动命令和环境变量 | 无 |

规则：

- Agent id 由名字生成（小写字母和 `-`），可以手动指定，不能重复。已有 Agent 不能修改类型。
- 内置运行时通过 ACP 适配器启动。适配器在第一次检查用到它的 Agent 时自动安装，见 [ACP 适配器管理](../architecture/adapters.md#on_demand)。
- 有正在运行的任务使用某个 Agent 时，不能删除它。
- 「复制本机配置」只复制与模型接入有关的配置项和凭据，见[安全边界](../operations/security.md#local_config_import)。

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

各 Agent 声明的典型权限模式：Codex 的沙箱预设（Read-only / Agent / Full access 等）、Claude Code 的权限模式（Manual / Accept edits / Plan / Auto / Bypass 等）、Grok 的权限模式和 Plan 开关。具体选项以 Agent 实际声明为准。

<a id="writability"></a>
### 是否修改文件

成员是否会修改文件由权限模式推导（`session_writes`），不再有单独的「可以修改文件」开关：

- 任一权限控制项的值是只读预设（规范化后为 `readonly`）或 `plan`，或者 id 含 `plan` 的开关为开启时，只读。
- 其余情况，包括 Agent 没有声明任何权限模式（Pi 和多数自定义命令），都视为会修改文件。

这个结论只用于告诉主控「按这个模式会修改文件 / 只读」，以及在页面上标注。实际能做什么由 Agent 自己按权限模式执行，Janus 不做额外校验。设计取舍见 [ADR：权限交给 Agent 原生模式](../decisions/agent_native_permissions.md)。

<a id="roles"></a>
## 角色

| 字段 | 说明 |
| --- | --- |
| `name`、`role`、`desc` | 名字、角色名、职责。职责会写进主控的协作者名单 |
| `connection` | 绑定的 Agent id |
| `model`、`effort` | 模型和推理强度；留空表示使用 Agent 默认值 |
| `session` | 选定的会话控制项值，`{控制项 id: 值}` |
| `prompt` | 角色指令，附加到该角色每个会话的提示中 |
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

<a id="member_snapshot"></a>
## 成员快照

`team_for` 把工作区展开成 Run 的成员（`Member`），包括：角色字段、Agent 类型与名称、会话控制项、权限模式标签（例如「Agent · Plan 开启」）、模型标签（例如「GPT-6.1 Sol，推理强度 high（Agent 默认）」）、是否修改文件、Skill 和 MCP。

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
