# Agent、角色与工作区

本文说明三类设置对象之间的关系和规则：Agent（Provider 连接）、角色、工作区，以及权限模式怎样进入一次会话。角色选择的 Skill 和 MCP 来自[资源库](20261116-resource_library.md)。运行环境的隔离方式见[安全边界](../operations/20261031-security.md#agent_isolation)，设置接口见 [HTTP API](../interfaces/20261031-http_api.md#setup)。

关系：工作区由多个角色组成，每个角色绑定一个 Agent。Run 创建时把这三层合并成团队快照，之后修改设置不影响已有 Run。

<a id="providers"></a>
## Agent 类型

`agents::kinds()` 定义了可创建的 Agent 类型：

| 类型 | 分组 | 基础运行时 | 认证方式 | 配置目录变量 |
| --- | --- | --- | --- | --- |
| `codex` | 内置 | `codex` | 复制本机配置、ChatGPT 登录、自定义 API（OpenAI 或兼容 Responses API 的网关） | `CODEX_HOME` |
| `claude` | 内置 | `claude` | 复制本机配置、Claude 订阅 OAuth Token、API Key | `CLAUDE_CONFIG_DIR` |
| `deepseek-claude`、`minimax-claude`、`glm-claude` | 预设 | `claude` | 粘贴对应厂商的 token，通过预设的环境变量（可修改）把 Claude Code 指向厂商的 Anthropic 兼容接口 | `CLAUDE_CONFIG_DIR` |
| `custom` | 自定义 | `custom` | 无；只使用填写的启动命令和环境变量 | 无 |

规则：

- Agent id 由名字生成（小写字母和 `-`），可以手动指定，不能重复。已有 Agent 不能修改类型。Agent、角色和工作区的 id 都只能用小写字母、数字和 `-`，以字母或数字开头，最长 40 个字符（`check_id`）：它们是 Janus home 中目录和文件的名字，保存和删除时都会校验。
- 内置运行时通过 ACP 适配器启动。适配器在第一次检查用到它的 Agent 时自动安装，见 [ACP 适配器管理](../architecture/20261031-adapters.md#on_demand)。
- 有正在运行的任务使用某个 Agent 时，不能删除它。
- 「复制本机配置」只复制与模型接入有关的配置项和凭据，见[安全边界](../operations/20261031-security.md#local_config_import)。

<a id="agent_env"></a>
### 环境变量

Agent 进程不继承 Janus 自己的环境（见[安全边界](../operations/20261031-security.md)），它看到的变量按以下顺序叠加，同名时后面的覆盖前面的：

1. 沙箱变量：`HOME`、XDG 目录、`TMPDIR` 等，指向 `~/.janus/agents/<id>/` 下的隔离目录。
2. 类型的默认变量（`fixedEnv`，预设还有所选接入点的 `env`），例如 DeepSeek 预设的 `ANTHROPIC_BASE_URL` 和各档模型。
3. 用户在 Agent 上填写的环境变量，以及「复制本机配置」带来的变量。值为空（`KEY=`）表示去掉这个变量。沙箱变量不能在这里改写。
4. 隔离目录变量（如 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`）、运行时路径和凭据（如预设 token 对应的 `ANTHROPIC_AUTH_TOKEN`）。Claude Code 还有一项默认值：`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`，原因见 [Claude Code 会话的环境默认值](../interfaces/20261031-acp_integration.md#background_tasks)。

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
- 用 ACP session modes 报告模式的 Agent，模式转换成 id 为 `mode` 的控制项。

一个控制项满足以下任一条件时，被视为权限模式（`permission: true`），在角色编辑器的「权限」区域显示：

- `category` 是 `mode` 或 `_permission`。
- id（`-` 视为 `_`）是 `mode`、`permission_mode`、`plan_mode`、`approval_policy` 或 `sandbox`。

各 Agent 声明的典型权限模式：Codex 的沙箱预设（Read-only / Agent / Full access 等）、Claude Code 的权限模式（Manual / Accept edits / Auto / Bypass 等）。具体选项以 Agent 实际声明为准。

角色没有选择权限模式时，Codex 默认用 Full access（`agent-full-access`），Claude Code 默认用 Bypass permissions（`bypassPermissions`），由 `Runtime::default_mode` 指定；Agent 不提供这个选项、或是自定义命令的 Agent 时，沿用 Agent 自己的当前模式（`control_default`）。默认值不写进角色：创建 Run 时 `member_session` 把它补进成员的 `session`，设置接口返回的连接给每个控制项带上 `default`，角色编辑器按它选中并标注「默认」，选回默认值时去掉角色中的这一项。设计取舍见 [ADR：Codex 和 Claude Code 默认不受限运行](../decisions/20261118-unrestricted_default_mode.md)。

<a id="no_plan_mode"></a>
### 不使用 Plan 模式

Janus 不让任何 Agent 进入 Plan 模式。Plan 模式只出方案、不执行，而 Janus 的任务要真正完成；Codex 负责人在 Plan 模式下还会跳过 `/goal` 的自动推进（日志为 `skipping goal continuation ... reason=PlanMode`），Run 会一直停在「目标进行中」。

- `session_options` 把 id 含 `plan` 的控制项（`is_plan_toggle`，例如 Codex 的 `plan-mode` 是 on / off 下拉，也可能是布尔开关）从 `controls` 中移到 `planToggles`，并从所有下拉控制项中去掉值为 `plan` 的选项（`is_plan_value`，例如 Claude Code 的 Plan 权限模式和旧版 Agent 的 `plan` session mode）。角色编辑器因此看不到这些选项；在这条规则之前检查过的 Agent，应用按同样规则过滤它保存的选项。
- `role_session` 丢弃角色中与 Plan 有关的值：保存角色时清掉，启动会话时也不使用。
- 打开会话时，`configure_session` 把仍处于开启状态的 Plan 控制项设为关闭值（`plan_off_value`：布尔为 `false`，下拉为 `off` 一类的选项）；下拉控制项的当前值是 `plan` 且角色没有指定时，改为第一个其他选项。关闭失败时会话以错误结束，不会带着 Plan 模式运行。

Janus 不从权限模式推导成员「会修改文件 / 只读」：应用只显示 Agent 自己的权限模式名，负责人收到的协作者名单不含权限模式；实际能做什么由 Agent 按自己的模式执行，Janus 不做额外校验。设计取舍见 [ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md)。

<a id="roles"></a>
## 角色

| 字段 | 说明 |
| --- | --- |
| `name`、`desc` | 名字和职责。负责人只凭这两项认识协作者，按职责派任务，见[负责人知道什么](20261031-goal_mode.md#lead_brief) |
| `connection` | 绑定的 Agent id |
| `model`、`effort` | 模型和推理强度；留空表示使用 Agent 默认值 |
| `session` | 选定的会话控制项值，`{控制项 id: 值}`；没有选择的控制项用它的默认值，见[会话控制项与权限模式](#permission_modes) |
| `prompt` | 角色指令，作为用户的长期要求注入：Claude Code 是系统提示，Codex 是开发者指令，自定义命令的 Agent 附在任务说明末尾，见 [ACP 集成](../interfaces/20261031-acp_integration.md#standing_instructions) |
| `resources` | 选择的资源库资源（Skill 和 MCP 服务器）的 id，见[角色的选择](20261116-resource_library.md#roles) |
| `lead` | 是否可以担任负责人 |
| `seed` | 只有默认角色有：来自哪个默认角色（`id`），以及用户最后拿到的默认职责和指令的哈希（`desc`、`prompt`，SHA-256 前 16 位）。由 Janus 维护，保存角色时沿用已存的值，编辑器传来的不生效 |
| `color`、`init` | 应用中的颜色和缩写 |

规则：

- 保存时，`session` 中只保留与 Agent 声明的控制项匹配的值（`role_session`）。Agent 还没检查过、没有控制项时，先保留字符串和布尔值。
- 保存时，`resources` 只保留资源库中存在的 id，并去掉重复；创建 Run 时同样跳过找不到的资源。资源的名字在同一类中唯一，`janus` 保留给看板，所以一个角色的 MCP 服务器不会重名。
- 被工作区使用的角色不能删除。
- 首次启动时写入 5 个默认角色，组成产品上线团队，见[默认角色](#product_launch_roles)。它们都还没有绑定 Agent，职责和角色指令已经填写。
- 默认角色由 `sync_seeds` 维护：首次启动时写入（写入后记 `meta.seeded`），之后每次启动把用户没改过的职责和指令换成这个版本的默认文本，改过的保留并提示有新版，删除的不会恢复。规则见[默认角色](#product_launch_roles)。

<a id="product_launch_roles"></a>
### 默认角色：产品上线团队

默认角色按「一个 PMO 负责人推进产品从需求到上线」设计。PMO 只负责拆解、顺序、分工、跟进和汇总，不定义产品范围和成功标准：产品范围、优先级、成功标准和产品验收归产品经理，市场侧成功指标归市场运营，技术方案归研发工程师，质量结论归质量工程师。角色指令的全文在 `backend/src/agents/roles.rs` 的 `SEEDS` 中。

默认职责和指令随版本更新（`sync_seeds`，设计见 [ADR：数据随版本自动升级](../decisions/20261118-data_migrations.md)）：

- 每个默认角色的 `seed` 记录它来自哪个默认角色，以及用户最后拿到的职责和指令的哈希。
- 启动时逐项比较：当前文本的哈希与记录相同，说明用户没改过，换成这个版本的默认文本并更新记录；与记录不同，说明用户改过，保留用户的文本。
- 改过的项有更新的默认文本时，设置接口返回的角色带 `seedUpdates`（`{字段: 新文本}`），编辑器在职责或指令下方提示，用户可以「采用新版」或「保留我的」（`POST /api/roles/{id}/seed`）。两种选择都更新记录，直到下一个版本带来新的默认文本前不再提示。
- 数据版本 2 之前的默认角色没有 `seed`：迁移按 `SEED_HISTORY`（历次发布过的文本的哈希）判断，当前文本是发布过的某一版就算没改过，否则按改过处理。

负责人只凭名称和职责认识协作者（见[负责人知道什么](20261031-goal_mode.md#lead_brief)），所以默认职责写成「什么样的子任务适合派给它」。

| id | 名字 | 职责 |
| --- | --- | --- |
| `coordinator` | PMO | 担任负责人：拆解目标、安排顺序、分派子任务、跟进进度与风险、汇总交付；不替产品、技术和质量做决定 |
| `researcher` | 市场运营 | 市场和用户研究、竞品分析、市场侧成功指标、发布与增长方案；适合需要外部证据或对外内容的子任务 |
| `product-manager` | 产品经理 | 澄清需求、定义范围和优先级、编写带编号验收标准的 PRD、裁定产品问题、做产品验收 |
| `implementer` | 研发工程师 | 技术方案、编码实现、缺陷修复和自动化测试；适合需要改代码、配置或构建脚本的子任务 |
| `reviewer` | 质量工程师 | 设计和执行测试、独立验证他人的改动、报告缺陷、给出质量结论；独立验证时只检查不改文件 |

指令作为系统提示词注入，只写各角色的专业判断。结构化结果、看板工具、文件认领、验证判定和不 commit、push、部署这些协作机制由 Janus 的固定约定说明，指令不重复。共同约定：

- 按规模选择流程。小目标（修一个缺陷、加一个小功能、回答一个调研问题）由 PMO 直接派给职责最匹配的协作者，改了代码的再安排一次独立验证；新产品、跨模块的功能或范围不清的目标，按澄清、定义、执行、验收四个阶段推进。PRD 的篇幅也与目标规模相称。
- PMO 按协作者名单中的职责找人，不按名字；名单里没有某类职责的人时，在计划中写明由谁承担，必要时自己做。
- 澄清：产品经理对照目标、工作区共享资料和已有证据，列出只有用户能回答、而且会明显改变范围、优先级或成功标准的问题（最多 5 个，带选项和建议），能推断的写成假设；PMO 合并成一次交给用户，等待时先推进不依赖答案的工作。
- PRD 写成工作目录中的 Markdown 文件，功能（F-1）和验收标准（AC-1）都编号，验收标准写成可观察的行为；子任务、证据、质量结论和产品验收都按编号对应。
- 执行中的提问都先到负责人（见[提问的流转](20261031-goal_mode.md#question_routing)）。产品问题 PRD 有答案的，PMO 按 PRD 回复；没有答案的，由产品经理裁定并更新 PRD。市场、技术和质量问题分别由对应的角色决定。
- 改了代码的子任务完成后，由质量工程师做独立验证；独立验证时只检查不改文件，按 Janus 的要求给出判定。是否达到产品目标，以产品经理的产品验收和质量工程师的质量结论为准。
- 区分事实、推断和假设；不把未运行的检查写成通过，不把未完成的工作写成完成。市场运营不能联网或找不到可靠来源时如实说明，结论标为待验证。
- 改变范围、预算或周期，使用外部资源或付费服务，以及不可逆或影响工作目录之外的操作，交给用户决定；市场运营对发帖、投放、发送邮件、提交应用商店审核等对外操作只准备材料和步骤。
- 研发写明配置、数据迁移和部署的操作步骤和回滚方式；用户在澄清中的回答、PRD 的位置等值得长期保留的事实，由 PMO 列入共享资料建议，见[工作区共享资料](#workspace_brief)。

<a id="workspaces"></a>
## 工作区

| 字段 | 说明 |
| --- | --- |
| `path` | 工作目录，必须存在，保存为规范化的绝对路径 |
| `members` | 成员列表，每项是 `{role: <角色 id>, slots?: <并发实例数>}`；协作者的 `slots` 范围 1 到 8（默认 1），负责人固定为 1 |
| `lead` | 负责人的角色 id，必须在成员中 |
| `name` | 默认取目录名 |
| `brief` | 共享资料，见[工作区共享资料](#workspace_brief)；最多 8000 个字符（`BRIEF_LIMIT`），保存时去掉首尾空白 |

规则：

- 负责人必须是开启了「可以担任负责人」（角色字段 `lead`）并绑定 Claude Code 或 Codex 的角色，见[目标模式](20261031-goal_mode.md#lead_role)。
- 除负责人外至少还要有一名成员。
- Run 创建时，负责人的成员 id 变为 `lead`；如果某个角色的 id 本身是 `lead` 但不是负责人，改为 `lead-role`。
- 协作者 `slots > 1` 时，`team_for` 把它展开为多个独立的 `Member` 实例（例如 `<id>-1`、`<id>-2`，若与工作区中已有角色 id 冲突则顺延后缀），各自拥有独立的看板身份和资源快照目录，但共享同一个 `role_id`，统计仍归到原角色。见 [ADR：角色并发槽位与全局并发上限](../decisions/20261118-role_slots_and_concurrency_cap.md)。

<a id="workspace_brief"></a>
### 工作区共享资料

共享资料是用户为一个工作区维护的长期背景：产品定位、目标用户、约定、过去的决定、重要文件的位置。它解决每次 Run 都从零开始的问题：写进共享资料的事实，负责人和协作者不再重新调研，也不再问用户。

- **查看与编辑**：工作区首页显示当前的共享资料，从那里或「工作区设置」进入编辑。只有用户能改。Agent 不能直接写入，避免错误的结论一直影响之后的 Run。
- **快照**：Run 创建时随工作区写入 Run 快照（`config.workspace.brief`），之后的修改不影响这个 Run，从中断处继续时也用快照中的版本。
- **注入**：`brief_section` 把它附在负责人说明（`goal_brief`）和每个任务提示（`task_prompt`）中，写明与本次目标或任务说明冲突时以后者为准。没有共享资料时什么都不加。
- **积累**：负责人说明要求负责人在最后一条回复末尾，用 `## Suggested Updates to Shared Context` 标题列出这次确定的、值得长期保留的新事实，只写新增或需要修改的内容。`split_brief_suggestions` 把这一节从交付摘要中拆出来，放进 `run.delivered` 的 `briefSuggestions`。解析时也识别旧标题「建议更新共享资料」，接受加粗和末尾冒号，去掉条目的列表标记，读到下一个标题为止。
- **采纳**：应用在交付结果中显示这些条目和「加入共享资料…」按钮，按钮打开工作区设置，把条目以 `- ` 开头追加到共享资料末尾；用户可以修改，保存后才生效。

为什么只让用户写入，而不是让 Agent 像 Grok Bot 的共享记忆那样直接维护：Janus 的 Run 由 Agent 自主推进，用户不一定逐条核对过程中的结论。共享资料会进入之后每一次 Run 的每一个会话，一条错误的结论会被反复当作事实使用。由负责人提出、用户确认，既能积累，又保留了用户的把关。

<a id="role_stats"></a>
## 角色的执行统计

`role_stats`（`GET /api/stats/roles`）按角色汇总最近 50 个 Run（与 Run 列表相同的范围）中协作者任务的结果，用来判断该换 Agent 或模型、改角色指令，还是负责人的任务写法有问题：

| 字段 | 含义 |
| --- | --- |
| `tasks` | 结束了的任务数（成功或失败；被重试的任务按最后一次的结果计） |
| `succeeded` / `failed` | 成功 / 失败的任务数 |
| `firstPass` | 第一次尝试就成功、且验收核对一次通过的任务数 |
| `verificationFailed` | 至少有一次没通过验收核对的任务数（漏了标准、缺证据或自认没达到） |
| `avgMinutes` | 成功任务从第一次开始到完成的平均分钟数 |

任务按 Run 快照中成员的 `role_id` 归到角色，负责人不计入。统计按角色而不是按当时绑定的 Agent 归类，角色换过 Agent 或模型后，新旧数据会合在一起。

角色编辑器在标题下显示这些数字；运行页的 Agent 面板在每个协作者的悬停提示中显示本次 Run 的「首次通过 / 已结束」和验证没通过的任务数。

<a id="member_snapshot"></a>
## 成员快照

`team_for` 把工作区展开成 Run 的成员（`Member`），包括：角色字段、Agent 类型与名称、会话控制项（角色的选择加上没有选择的权限模式的默认值）、权限模式标签（`permission`，例如「Full access」，显示在 Run 的审计页）、角色指令和 Provider 的自定义提示词、固定下来的资源（`resources`，见 [Run 和会话怎样拿到资源](20261116-resource_library.md#delivery)）。

这些字段同时用于：打开会话时设置控制项、负责人说明中的协作者名单、应用的「团队与权限」页。
