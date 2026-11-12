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
4. 隔离目录变量（如 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`）、运行时路径和凭据（如预设 token 对应的 `ANTHROPIC_AUTH_TOKEN`）。Claude Code 还有两项默认值：`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`，以及 Agent 自己没有设置时的 `CLAUDE_ACP_MODEL_DISCOVERY=off`，原因见 [Claude Code 会话的环境默认值](../interfaces/acp_integration.md#background_tasks)。

编辑页把第 2 步的默认值直接填进环境变量框，用户可以逐项修改；切换接入点时只替换该接入点的变量，用户自己加的行保留。保存后这些值成为 Agent 自己的配置，之后预设的默认值变化不会影响已经填写的项。

<a id="connection_check"></a>
## 配置检查

`test_connection` 按顺序执行以下步骤，结果在应用中逐步显示：

1. **ACP 适配器**：内置运行时需要的适配器没有安装时，当场安装最新版本。自定义命令没有这一步。
2. **运行时**：找到 CLI 和适配器入口。
3. **凭据**：检查所选认证方式需要的凭据是否齐全。
4. **启动 Agent**：启动子进程并完成 ACP `initialize`，60 秒超时。
5. **打开会话**：`session/new`，读取 Agent 声明的模型、推理强度和会话控制项，保存到连接的 `options`。
6. **试运行**：发送一句测试消息，确认有回复。

第 5 步读到的控制项是角色编辑器的选项来源。Agent 升级后控制项变化时，重新检查即可刷新。

`GET /api/connections` 为每个连接额外返回 `phase`，应用只按这一个值显示状态，不再自己组合 `status`、`login.state` 和 `loggedIn`。取值依次为 `logging_in`（登录命令正在运行）、`logged_out`（需要账号登录但本机没有 `auth.json`）、`testing`、`ok`、`error`、`untested`。

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

- `session_options` 把 id 含 `plan` 的控制项（`is_plan_toggle`，例如 Codex 的 `plan-mode` 是 on / off 下拉，Grok 的 `plan_mode` 是布尔开关）从 `controls` 中移到 `planToggles`，并从所有下拉控制项中去掉值为 `plan` 的选项（`is_plan_value`，例如 Claude Code 的 Plan 权限模式和旧版 Agent 的 `plan` session mode）。角色编辑器因此看不到这些选项；在这条规则之前检查过的 Agent，应用按同样规则过滤它保存的选项。
- `role_session` 丢弃角色中与 Plan 有关的值，旧角色里开着的 Plan 不再生效，保存角色时也会被清掉。
- 打开会话时，`configure_session` 把仍处于开启状态的 Plan 控制项设为关闭值（`plan_off_value`：布尔为 `false`，下拉为 `off` 一类的选项）；下拉控制项的当前值是 `plan` 且角色没有指定时，改为第一个其他选项。关闭失败时会话以错误结束，不会带着 Plan 模式运行。

Janus 不从权限模式推导成员「会修改文件 / 只读」：应用和主控收到的协作者名单都只显示 Agent 自己的权限模式名，实际能做什么由 Agent 按自己的模式执行，Janus 不做额外校验。设计取舍见 [ADR：权限交给 Agent 原生模式](../decisions/agent_native_permissions.md)。

<a id="roles"></a>
## 角色

| 字段 | 说明 |
| --- | --- |
| `name`、`role`、`desc` | 名字、角色名、职责。职责会写进主控的协作者名单 |
| `connection` | 绑定的 Agent id |
| `model`、`effort` | 模型和推理强度；留空表示使用 Agent 默认值 |
| `session` | 选定的会话控制项值，`{控制项 id: 值}` |
| `prompt` | 角色指令，作为用户的长期要求注入：Claude Code 是系统提示，Codex 是开发者指令，Pi 通过 Janus 的扩展加入系统提示，其他 Agent 附在任务说明末尾，见 [ACP 集成](../interfaces/acp_integration.md#standing_instructions) |
| `skills` | Skill id：Janus 自建的 Skill 是 `<id>`，GitHub 来源中的 Skill 是 `<来源 id>/<key>`，见 [Skill](#skills) |
| `mcp` | 手工 MCP 服务器，固定为 stdio：名称、命令、参数、环境变量名（值另存） |
| `mcpRefs` | 从 MCP 资源库挂载的服务器，每项是 `<来源 id>/<key>`，见 [MCP 来源](#mcp_library) |
| `lead` | 是否可以担任主控 |
| `color`、`init` | 应用中的颜色和缩写 |

规则：

- 保存时，`session` 中只保留与 Agent 声明的控制项匹配的值（`role_session`）。Agent 还没检查过、没有控制项时，先保留字符串和布尔值。
- MCP 名称只能包含字母、数字、`-` 和 `_`，名称 `janus` 保留给看板 MCP。手工 MCP 和资源库 MCP 不能同名；保存角色和创建 Run 时都会检查。手工 MCP 的环境变量值写入 `secrets/roles/<role-id>.json`（`0600`），SQLite 只记录变量名。
- 保存角色时，`mcpRefs` 只保留当前资源库中存在的资源；来源更新后消失的资源在下次保存时去掉（与 Skill 相同）。创建 Run 时同样跳过找不到的资源。
- 被工作区使用的角色不能删除。
- 首次启动时写入 5 个默认角色，组成产品上线团队，见[默认角色](#product_launch_roles)。它们都还没有绑定 Agent，职责和角色指令已经填写。
- 默认角色有版本（`SEED_VERSION`，记在 `meta.seeded`）。用旧版本初始化的 Janus home 升级时只执行一次：
  - 新增旧版本没有的默认角色。用户删掉的默认角色不会恢复。
  - 职责和角色指令仍是某个旧版本的默认值（或都为空）的默认角色，换成新的职责和指令；名字和角色名仍是旧默认值时一并更新。旧默认值记在 `OLD_SEEDS`。
  - Agent 绑定、模型、会话控制项、Skill、MCP、颜色、缩写和密钥保持不变；改过职责或指令的角色不改动。
  - 修改默认角色的文字时，把上一版的值加进 `OLD_SEEDS` 并提高 `SEED_VERSION`，否则已有的 home 不会更新。

<a id="product_launch_roles"></a>
### 默认角色：产品上线团队

默认角色按「一个 PMO 主控推进产品从需求到上线」设计。PMO 只负责计划、进度、依赖、风险和协调，不定义产品范围和成功标准：产品范围、优先级、成功标准和产品验收归产品经理，市场侧成功指标归市场运营，质量结论归质量工程师。其他角色通过看板提交产出、证据和风险。角色指令的全文在 `backend/src/agents.rs` 的 `SEEDS` 中。

| id | 名字 | 角色名 | 职责 |
| --- | --- | --- | --- |
| `coordinator` | PMO | 主控与项目推进 | 计划排期、进度跟踪、依赖协调、风险管理、跨角色沟通和交付汇总；可以担任主控 |
| `researcher` | 市场运营 | 市场研究、用户反馈与增长 | 目标市场、用户、竞品、市场侧成功指标、渠道、发布内容、用户反馈和增长验证 |
| `product-manager` | 产品经理 | 需求定义与 PRD | 产品范围、需求分析、用户流程、优先级、产品成功标准、PRD 和产品验收 |
| `implementer` | 研发工程师 | 产品实现 | 技术方案、前后端实现、数据与接口、自动化测试、技术风险和交付产物 |
| `reviewer` | 质量工程师 | 测试与发布质量 | 测试策略、功能验证、回归验证、缺陷管理、发布风险和最终质量判断 |

指令中的共同约定：

- 区分已确认事实、推断和待验证假设；不把未运行的检查写成已通过，不把未完成的工作写成完成。
- 每个协作者按验收标准逐条提交证据；需求歧义和冲突通过看板交给主控，不自行改变产品目标。
- PMO 按四个阶段推进。澄清：产品经理对照目标、工作区共享资料和已有证据，列出只有用户能回答、而且会明显改变范围、优先级或成功标准的问题（最多 5 个，带选项和建议），已有答案的不问；PMO 合并成一次交给用户，等待时先推进市场研究等不依赖答案的工作，没有要问的就跳过。定义：需要市场证据时先安排市场运营研究，再由产品经理写 PRD，PRD 注明用户的回答和采用的假设；PMO 提供目标、约束和排期，检查 PRD 能否在约束内完成，超出时请产品经理给出删减方案。执行：PMO 按 PRD 的功能编号和优先级把实现派给研发工程师、把验证派给质量工程师，验收标准直接引用 PRD 的编号；市场运营并行准备发布内容。验收：质量结论之后由产品经理做产品验收。
- PRD 写成工作目录中的文件，功能和验收标准都编号（例如 AC-1），任务、提交的证据和质量结论都按编号对应。
- 执行中的提问都先到主控（见[提问的流转](goal_mode.md#question_routing)）。产品问题 PRD 有答案的，PMO 按 PRD 回复；没有答案的，PMO 安排产品经理裁定并更新 PRD，再把结论转给提问的角色。市场、质量、技术问题分别由市场运营、质量工程师、研发工程师决定，PMO 只决定排期、顺序和分工。名单中缺少某个角色时，PMO 在计划里说明由谁承担。
- 改变范围、预算、周期、使用外部资源和不可逆操作交给用户决定，经主控的 `ask_user` 交出；涉及产品取舍时先由产品经理给出建议。是否达到产品目标，以产品经理的产品验收和质量工程师的质量结论为准。
- 用户在澄清中给出的回答、PRD 的位置和其他值得长期保留的决定，由 PMO 列进「建议更新共享资料」，见[工作区共享资料](#workspace_brief)。
- 不可逆和对外的操作需要用户明确授权：所有角色不 commit、push 或部署；市场运营对发帖、投放、发送邮件、提交应用商店等对外操作只准备材料和步骤。
- 研发写明上线所需的运行、配置、数据迁移和回滚方式，质量工程师在发布前确认这些和发布说明是否齐全。

<a id="workspaces"></a>
## 工作区

| 字段 | 说明 |
| --- | --- |
| `path` | 工作目录，必须存在，保存为规范化的绝对路径 |
| `members` | 成员列表，每项是 `{role: <角色 id>}` |
| `lead` | 主控的角色 id，必须在成员中 |
| `name` | 默认取目录名 |
| `brief` | 共享资料，见[工作区共享资料](#workspace_brief)；最多 8000 个字符（`BRIEF_LIMIT`），保存时去掉首尾空白 |

规则：

- 主控的角色必须绑定 Claude Code 或 Codex，见[目标模式](goal_mode.md#lead_role)。
- 除主控外至少还要有一名成员。
- Run 创建时，主控的成员 id 变为 `lead`；如果某个角色的 id 本身是 `lead` 但不是主控，改为 `lead-role`。

<a id="workspace_brief"></a>
### 工作区共享资料

共享资料是用户为一个工作区维护的长期背景：产品定位、目标用户、约定、过去的决定、重要文件的位置。它解决每次 Run 都从零开始的问题：写进共享资料的事实，主控和协作者不再重新调研，也不再问用户。

- **查看与编辑**：工作区首页显示当前的共享资料，从那里或「工作区设置」进入编辑。只有用户能改。Agent 不能直接写入，避免错误的结论一直影响之后的 Run。
- **快照**：Run 创建时随工作区写入 Run 快照（`config.workspace.brief`），之后的修改不影响这个 Run，从中断处继续时也用快照中的版本。
- **注入**：`brief_section` 把它附在主控说明（`goal_brief`）和每个任务提示（`task_prompt`）中，写明与本次目标或任务说明冲突时以后者为准。没有共享资料时什么都不加。
- **积累**：主控说明要求主控在最后一条回复末尾，用「## 建议更新共享资料」标题列出这次确定的、值得长期保留的新事实，只写新增或需要修改的内容。`split_brief_suggestions` 把这一节从交付摘要中拆出来，放进 `run.delivered` 的 `briefSuggestions`（标题前的 `#` 和加粗、末尾的冒号可有可无，条目前的 `- `、`* `、`1. ` 会去掉，到下一个标题为止）。
- **采纳**：应用在交付结果中显示这些条目和「加入共享资料…」按钮，按钮打开工作区设置，把条目以 `- ` 开头追加到共享资料末尾；用户可以修改，保存后才生效。

为什么只让用户写入，而不是让 Agent 像 Grok Bot 的共享记忆那样直接维护：Janus 的 Run 由 Agent 自主推进，用户不一定逐条核对过程中的结论。共享资料会进入之后每一次 Run 的每一个会话，一条错误的结论会被反复当作事实使用。由主控提出、用户确认，既能积累，又保留了用户的把关。

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

角色编辑器在标题下显示这些数字；运行页的 Agent 面板在每个协作者的悬停提示中显示本次 Run 的「首次通过 / 已结束」和验证没通过的任务数。

<a id="member_snapshot"></a>
## 成员快照

`team_for` 把工作区展开成 Run 的成员（`Member`），包括：角色字段、Agent 类型与名称、会话控制项、权限模式标签（`permission`，例如「Agent · Plan 开启」）、模型标签（例如「GPT-6.1 Sol，推理强度 high（Agent 默认）」）、角色指令和 Provider 的自定义提示词、Skill 和 MCP。

这些字段同时用于：打开会话时设置控制项、主控说明中的协作者名单、应用的「团队与权限」页。

<a id="skills"></a>
## Skill

角色可以挂载两类 Skill，它们在应用的「设置 › Skill 库」中管理，在角色编辑器中按来源分组勾选：

- **Janus 自建**：`$JANUS_HOME/skills/<id>/SKILL.md`，在应用中新建，或从用户选择的目录复制一次。复制时只复制普通文件，符号链接取其内容，不跟随到所选目录之外。Agent 不读取用户自己的 Agent Skill 目录。
- **GitHub 来源**：用户粘贴的仓库链接，Janus 拉取仓库并找出其中所有的 Skill，见 [Skill 来源](#skill_sources)。
- **Run 快照**：Run 创建时，成员的 Skill 复制到 `~/.janus/runs/<run>/skills/<Skill id>/`（复制时跳过符号链接和 `.git`），提示中给出快照路径。之后修改、更新或删除来源，不影响已有 Run。
- **保存角色**：`save_role` 只保留能找到 `SKILL.md` 的 Skill id（`App::skill_dir`），无效的 id 直接去掉。来源更新后不再存在的 Skill 仍留在角色上，Run 创建时跳过，角色编辑器把它标成「找不到这个 Skill」。
- **项目 Skill**：工作目录中 `.agents/skills`、`.claude/skills`、`.codex/skills`、`.pi/skills`、`.janus/skills` 下的 Skill，只要真实路径仍在工作目录内，就会列在每个会话的提示中。

提示只列出 Skill 的名字、描述和 `SKILL.md` 路径，由 Agent 自己决定是否读取。

<a id="skill_sources"></a>
### Skill 来源

Skill 来源是一个 GitHub 仓库（或仓库中的一个目录），参考 magpie 的资源库：添加一次链接，仓库中的所有 Skill 都可以给角色使用。实现在 `backend/src/skill_sources.rs`。

- **链接格式**（`parse_github`）：`https://github.com/o/r`、`github.com/o/r`、`git@github.com:o/r.git`、`o/r`，以及 `…/tree/<ref>/<目录>` 和 `…/blob/<ref>/<目录>/SKILL.md`。`tree/` 后的第一段视为分支或标签，所以名字里带 `/` 的分支无法指定。其他网站的链接直接拒绝。
- **拉取**：`git clone --depth 1`（指定了 ref 时加 `--branch`）到 `~/.janus/skill-sources/<来源 id>/` 下的临时目录，带 `GIT_TERMINAL_PROMPT=0`（私有仓库只能用本机已有的 git 凭据）和 `GIT_LFS_SKIP_SMUDGE=1`，180 秒超时。成功后替换 `repo/`，并把仓库、ref、目录、提交和 Skill 索引写进 `source.json`。拉取失败或找不到任何 Skill 时，新来源不保留，已有来源保持原样。
- **解析**（`scan`）：从链接指定的目录开始，找所有含普通文件 `SKILL.md` 的目录，最深 8 层、最多 500 个。跳过 `.git`、`node_modules` 和符号链接；`.claude/skills` 这类隐藏目录照常查找；找到一个 Skill 后不再进入它的子目录。链接指定的目录解析符号链接后必须仍在克隆目录内，否则视为没有 Skill。名称和描述来自 `SKILL.md` 的 front matter。
- **Skill id**：`<来源 id>/<key>`。key 取 Skill 目录名（仓库根目录本身是 Skill 时取仓库名），重名时加 `-2`、`-3`。更新时同一目录保留原来的 key（`assign_keys`），所以角色上的选择不会因为仓库新增或删除了别的 Skill 而错位。
- **更新**：重新拉取，Skill 跟随仓库的最新内容。只在用户点击时更新，有更新时会提醒，见[检测更新](#skill_source_updates)。每个来源一把锁（`folder_lock`，按来源目录区分）：同一来源的拉取、检查结果的写入和删除依次进行，不同来源互不等待；添加来源另用来源根目录的锁，同一链接同时添加两次时只有一次成功。
- **删除**：有角色挂载这个来源中的 Skill 时不能删除；这个来源正在拉取时也不能删除，提示稍后再删。
- **同一来源**：仓库、ref 和目录都相同的链接不能重复添加；同一仓库的不同目录可以分别添加。

<a id="skill_source_updates"></a>
#### 检测更新

- **检查**（`check_skill_sources`）：对每个来源执行 `git ls-remote`，只读取远端 ref 指向的提交，不下载内容。没有指定 ref 时看 `HEAD`；指定了时依次找同名分支、同名标签（附注标签取它指向的提交）。远端提交与本地 `commit` 不同时，`source.json` 记 `updateAvailable: true` 和 `latest`；检查失败时记 `checkError` 并清掉上一次的 `updateAvailable` 和 `latest`，界面显示错误而不是旧的「有更新」，不影响已有的 Skill。
- **时机**：服务启动 30 秒后检查一次，之后每 6 小时一次（`CHECK_EVERY`）；用户也可以在 Skill 库中点「检查更新」（`POST /api/skill-sources/check`）。
- **提醒**：应用启动时读取来源，之后每 10 分钟重读一次。有来源可以更新时，主窗口侧栏底部显示「N 个 Skill 来源有更新」（点击打开 Skill 库），设置的「Skill 库」标签带更新数量，Skill 库顶部列出这些来源并提供「全部更新」，每个来源标「有更新」和「更新」按钮，角色编辑器中该来源的分组旁也有「更新」。
- **一键更新**：与手动更新相同，重新拉取后 `updateAvailable` 清零。更新后角色挂载的 Skill 不在仓库中时，应用提示数量。

<a id="mcp_library"></a>
### MCP 来源

MCP 来源是一个 GitHub 仓库（或仓库中的一个目录），参考 magpie 的资源库：添加一次链接，仓库中的 MCP 服务器定义就可以给角色使用。实现在 `backend/src/mcp_sources.rs`。

- **链接格式**：与 Skill 来源共用 `parse_github`，支持 `https://github.com/o/r`、`github.com/o/r`、`git@github.com:o/r.git`、`o/r`，以及 `…/tree/<ref>/<目录>`。其他网站的链接直接拒绝。
- **拉取**：`git clone --depth 1`（指定了 ref 时加 `--branch`）到 `~/.janus/mcp-sources/<来源 id>/` 下的临时目录，带 `GIT_TERMINAL_PROMPT=0` 和 `GIT_LFS_SKIP_SMUDGE=1`，180 秒超时。成功后替换 `repo/`，并把仓库、ref、目录、提交、服务器索引和解析警告写进 `source.json`。拉取失败或一个可用服务器都没有找到时，新来源不保留，已有来源保持原样。
- **解析**（`scan`）：从链接指定的目录开始，找所有 `mcp.json` 和 `.mcp.json`，最深 8 层、最多 500 个服务器。跳过 `.git`、`node_modules` 和符号链接；链接指定的目录解析符号链接后必须仍在克隆目录内。每个文件接受 Claude 的 `mcpServers`、VS Code 的 `servers`，或一个带 `name` 的单独服务器对象。
- **传输**：有 `command` 的是 stdio；`type` 为 `http` / `streamable-http`，或只有 `url` 的是 streamable HTTP，地址必须以 `http://` 或 `https://` 开头。SSE 传输不支持，解析时给出警告并跳过。服务器名规范化（`slug`）后是 `janus` 的名称（如 `janus`、`Janus`、`janus!`、`janus_`）保留给看板 MCP，解析时跳过并写入警告。
- **资源 id**：`<来源 id>/<key>`。key 取服务器名称的 slug，重名时加 `-2`、`-3`。更新时同一 manifest 中同名条目保留原来的 key（`assign_keys`），已有条目的 key 先保留，新条目再从剩下的名字中取，所以角色上的选择不会因为仓库新增或删除了别的服务器而错位，也不会出现两个服务器同一个 id。
- **凭据**：manifest 中 `env` / `headers` 的值在添加和更新时写入 `secrets/mcp/<来源 id>/<key>.json`（目录 `0700`，文件 `0600`）。已经保存的值优先，更新仓库不会覆盖；更新后消失的服务器保留旧值，供已经开始的 Run 使用；删除来源时一并删除。`source.json`、SQLite、设置接口和 Run 快照都只保存变量名（`envKeys`、`headerKeys`）和是否设置（`envSet`、`headerSet`），不保存值。
- **更新**：重新拉取，服务器定义跟随仓库。只在用户点击时更新；检测更新（`check_mcp_sources`）用 `git ls-remote`，服务启动 45 秒后检查一次、之后每 6 小时一次，应用也可以点「检查更新」。有更新的来源在设置的「MCP 库」标签和列表中标出，并可一键更新。
- **删除**：有角色挂载这个来源中的服务器时不能删除；这个来源正在拉取时不能删除（锁与 Skill 来源相同，每个来源一把），否则拉取结束后会把 `source.json` 和凭据写回；有 `planning`、`running`、`waiting_input`、`interrupted` 或 `failed` 的 Run 快照还引用这个来源时也不能删除，等 Run 结束或删除后再删。检查覆盖全部 Run（`runs_may_use_mcp`），不只应用显示的最新 50 个，所以较早的中断或失败 Run 同样会阻止删除。`delivered`、`accepted` 和 `cancelled` 的 Run 已结束，不再阻止删除。
- **同一来源**：仓库、ref 和目录都相同的链接不能重复添加；同一仓库的不同目录可以分别添加。

<a id="mcp"></a>
## MCP

角色可以挂载两类 MCP 服务器，打开会话时一起放进 `session/new`（恢复时 `session/load`）的 `mcpServers`：

- **手工 MCP**：角色自己的 `mcp` 字段，固定为 stdio 服务器：名称、命令、参数和环境变量名。环境变量的值写入 `secrets/roles/<role-id>.json`（`0600`）。
- **资源库 MCP**：角色 `mcpRefs` 中的 `<来源 id>/<key>`，来自 [MCP 来源](#mcp_library)。stdio 和 streamable HTTP 都支持，环境变量和 HTTP header 的值写入 `secrets/mcp/<来源 id>/<key>.json`（`0600`）。

运行时规则：

- `team_for` 先放手工 MCP，再放资源库 MCP。定义随 Run 快照保存，值不进入快照；会话启动时由 `attach_mcp_secrets` 从两个 secrets 目录读取，所以中断后继续的会话也会读到最新保存的值。
- 只注入服务器声明的变量名（`envKeys`、`headerKeys`），secrets 中多余的键和空值不会传给 Agent。
- 手工 MCP 与资源库 MCP 不能同名；保存角色和创建 Run 时都会报错。名称规范化后是 `janus` 的服务器与看板 MCP 冲突：资源库在解析时跳过，手工 MCP 在保存角色时拒绝，创建 Run 时再检查一次已保存的角色。
- Agent 没有声明 HTTP MCP 能力时（例如 Pi），会话只带 stdio 服务器，资源库中的 http 服务器被跳过，会话仍能打开。
- 目标模式下，Janus 还会附加自己的 HTTP 看板服务（名称 `janus`），见[看板 MCP 工具](../interfaces/board_mcp.md)。
