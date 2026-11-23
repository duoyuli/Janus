# Agent、角色与工作区

本文说明三类设置对象之间的关系和规则：Agent（Provider 连接）、角色、工作区，以及权限模式怎样进入一次会话。角色选择的 Skill 和 MCP 来自[资源库](20261116-resource_library.md)。运行环境的隔离方式见[安全边界](../operations/20261031-security.md#agent_isolation)，设置接口见 [HTTP API](../interfaces/20261031-http_api.md#setup)。

关系：工作区由多个角色组成，每个角色绑定一个 Agent。Run 创建时把这三层合并成团队快照，之后修改设置不影响已有 Run。

## 导航

- [Agent 类型](#providers)：加一种 Agent 或改它的登录方式时阅读。
- [配置检查](#connection_check)：排查 Agent 测试不通过时阅读。
- [会话控制项与权限模式](#permission_modes)：修改模型、权限模式等会话选项时阅读。
- [角色](#roles)：修改数字员工的字段和默认数字员工时阅读。
- [头像](#avatars)：修改数字员工的头像、默认头像或头像的显示时阅读。
- [工作区](#workspaces)：修改工作区的成员和负责人时阅读。
- [角色的执行统计](#role_stats)：修改数字员工的执行统计时阅读。
- [成员快照](#member_snapshot)：修改 Run 创建时固定的成员信息时阅读。

<a id="providers"></a>
## Agent 类型

`agents::kinds()` 定义了可创建的 Agent 类型：

| 类型 | 基础运行时 | 认证方式 | 配置目录变量 |
| --- | --- | --- | --- |
| `codex` | `codex` | 复制本机配置、ChatGPT 登录、自定义 API（OpenAI 或兼容 Responses API 的网关） | `CODEX_HOME` |
| `claude` | `claude` | 复制本机配置、Claude 订阅 OAuth Token、自定义 API（Anthropic API 或兼容的网关，用环境变量设置） | `CLAUDE_CONFIG_DIR` |

只有这两种：不能用启动命令接入其他 ACP Agent，见 [ADR：去掉「自定义命令」](../decisions/20261120-codex_and_claude_code_only.md)。

规则：

- Agent ID 由名字生成（小写字母和 `-`），可以手动指定，不能重复。已有 Agent 不能修改类型。Agent、角色和工作区的 id 都只能用小写字母、数字和 `-`，以字母或数字开头，最长 40 个字符（`check_id`）：它们是 Janus home 中目录和文件的名字，保存和删除时都会校验。
- 内置运行时通过 ACP 适配器启动。适配器在第一次检查用到它的 Agent 时自动安装，见 [ACP 适配器管理](../architecture/20261031-adapters.md#on_demand)。
- 有正在运行的任务使用某个 Agent 时，不能删除它。
- 「复制本机配置」只复制与模型接入有关的配置项和凭据，见[安全边界](../operations/20261031-security.md#local_config_import)。
- 通过 Anthropic API 或兼容网关接入 Claude Code 时，选择「自定义 API」（`apikey`），在环境变量中整段粘贴服务地址、凭据和模型设置；「填入模板」把 catalog 中这种认证方式的模板（`template`）里还没有填写的变量加到末尾，已填写的不变。网关可使用 `ANTHROPIC_AUTH_TOKEN`，Anthropic API 可使用 `ANTHROPIC_API_KEY`。旧连接中单独填写的 API Key 在数据版本 13 升级为该 Agent 的 `ANTHROPIC_API_KEY`。

<a id="agent_env"></a>
### 环境变量

每行可写 `KEY=value` 或 `export KEY=value`。空行和以 `#` 开头的行会跳过，值两端配对的引号会去掉。Janus 按文本读取这些赋值。保存时，普通服务地址、模型和常用运行参数存进设置，令牌等私有值存进 Agent 的凭据文件，设置里只记名字。编辑时应用从本机服务读回全部值（`GET /api/connections/{id}/env`），输入框显示完整的 `KEY=value`，令牌也在，改哪行就改哪行，删除整行可移除；读不到值时只显示名字，名字单独一行表示沿用已保存的值。取舍见 [ADR：Claude Code 的自定义 API 用环境变量设置](../decisions/20261120-claude_env_auth.md) 和 [ADR：Agent 的环境变量在编辑器里全部显示](../decisions/20261121-agent_env_visible.md)。

Agent 进程不继承 Janus 自己的环境（见[安全边界](../operations/20261031-security.md)），它看到的变量按以下顺序叠加，同名时后面的覆盖前面的：

1. 沙箱变量：`HOME` 和 XDG 目录指向 `~/.janus/data/agents/<id>/home`，`TMPDIR` 指向 `~/.janus/tmp/agents/<id>`。
2. 用户在 Agent 上填写的环境变量，以及「复制本机配置」带来的变量。值为空（`KEY=`）表示去掉这个变量。沙箱变量不能在这里改写。
3. 隔离目录变量（如 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`）、运行时路径和所选登录方式的凭据（如 Claude 订阅的 `CLAUDE_CODE_OAUTH_TOKEN`）。Claude 的「自定义 API」使用第 2 步填写的变量。Claude Code 还有两项默认值 `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` 和 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`，原因见 [Claude Code 会话的环境默认值](../interfaces/20261031-acp_integration.md#background_tasks)。

<a id="connection_check"></a>
## 配置检查

`test_connection` 按顺序执行以下步骤，结果在应用中逐步显示：

1. **ACP 适配器**：Agent 需要的适配器没有安装时，当场安装最新版本。
2. **运行时**：找到 CLI 和适配器入口。
3. **凭据**：检查所选认证方式需要的凭据是否齐全。Claude 的「自定义 API」由 Claude Code 在后续的试运行中验证，网关可以使用自己的变量名。
4. **启动 Agent**：启动子进程并完成 ACP `initialize`，60 秒超时。
5. **打开会话**：`session/new`，读取 Agent 声明的模型、推理强度和会话控制项，保存到连接的 `options`。
6. **试运行**：发送一句测试消息，确认有回复。

第 5 步读到的控制项是角色编辑器的选项来源。Agent 升级后控制项变化时，重新检查即可刷新。

一个连接同一时间只记录一次检查的结果。检查开始时把 `status` 写成 `testing`，`testStartedAt` 写成开始时间；服务在内存中记着每个连接正在跑的检查（`App.checks`）。检查进行中再次发起，返回正在跑的这一次；记录还是 `testing` 但这次检查已经不在跑（检查中途 panic），就开始新的一次。每一步和最终结果只在记录的 `testStartedAt` 仍是自己的开始时间时写入：保存连接会写入不带开始时间的新记录，新检查会写入自己的开始时间，被取代的旧检查跑完后什么也不写，不会用旧配置的结果覆盖新的。应用轮询到检查从 `testing` 变为 `untested`（被保存取代或服务重启）时，提示检查没有完成，而不是「检查未通过」。

`GET /api/connections` 为每个连接额外返回 `phase`，应用只按这一个值显示状态，不再自己组合 `status`、`login.state` 和 `loggedIn`。取值依次为 `logging_in`（登录命令正在运行；服务重启时登录命令随旧进程结束，启动时把留下的 `login.state=running` 改为 `failed`，和中途重启的配置检查改回 `untested` 一样，见 `reset_stale_checks`）、`logged_out`（需要账号登录但本机没有 `auth.json`）、`testing`、`ok`、`error`、`untested`。

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

角色没有选择权限模式时，Codex 默认用 Full access（`agent-full-access`），Claude Code 默认用 Bypass permissions（`bypassPermissions`），由 `Runtime::default_mode` 指定；Agent 不提供这个选项时，沿用 Agent 自己的当前模式（`control_default`）。默认值不写进角色：创建 Run 时 `member_session` 把它补进成员的 `session`，设置接口返回的连接给每个控制项带上 `default`，角色编辑器按它选中并标注「默认」，选回默认值时去掉角色中的这一项。设计取舍见 [ADR：Codex 和 Claude Code 默认不受限运行](../decisions/20261118-unrestricted_default_mode.md)。

模式还要在 Agent 启动时给一次（`Runtime::mode_settings`，`member_spec` 合并进适配器进程的 `CODEX_CONFIG`）：Codex 的适配器只把会话模式带进它自己发出的每一轮提示，`/goal` 和 Codex 接着推进目标的各轮用的是线程自己的审批和沙箱设置，这些设置在线程创建时取自配置，没有时是「需要时询问」加「只能写工作目录」的沙箱。所以负责人显示 Full access，`/goal` 里却会弹审批、访问钥匙串和工作目录外的文件失败。Janus 按成员的模式把线程的设置一起给上：Read-only 是 `on-request` 加 `read-only`，Workspace access 是 `on-request` 加 `workspace-write`，Auto review 再加 `approvals_reviewer = auto_review`，Full access 是 `never` 加 `danger-full-access`；Janus 不认识的模式（例如 Codex 的权限配置文件）不给，由 Agent 自己的配置决定。会话中途切换模式时，之后的普通提示照新模式，已经在跑的目标仍按启动时的设置，要整个目标换模式就在下一个会话生效。Claude Code 的适配器把模式设在整个会话上（`setPermissionMode`），`/goal` 也照它执行，不需要这一步。

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
| `connection` | 绑定的 Agent ID |
| `model`、`effort` | 模型和推理强度；留空表示使用 Agent 默认值 |
| `session` | 选定的会话控制项值，`{控制项 id: 值}`；没有选择的控制项用它的默认值，见[会话控制项与权限模式](#permission_modes) |
| `prompt` | 角色指令，作为用户的长期要求注入：Claude Code 是系统提示，Codex 是开发者指令，见 [ACP 集成](../interfaces/20261031-acp_integration.md#standing_instructions) |
| `lead` | 是否可以担任负责人 |
| `seed` | 只有默认角色有：来自哪个默认角色（`id`），以及用户最后拿到的默认职责和指令的哈希（`desc`、`prompt`，SHA-256 前 16 位）。由 Janus 维护，保存角色时沿用已存的值，编辑器传来的不生效 |
| `color`、`init` | 应用中的颜色和缩写 |

规则：

- 保存时，`session` 中只保留与 Agent 声明的控制项匹配的值（`role_session`）。Agent 还没检查过、没有控制项时，先保留字符串和布尔值。
- 删除 Agent 时，绑定它的角色变成未绑定：`connection`、`model`、`effort` 清空，`session` 变为 `{}`，和不选 Agent 保存的角色相同。
- 角色不选资源：资源库里的每项资源都给所有数字员工，见 [Run 和会话怎样拿到资源](20261116-resource_library.md#delivery)。数据版本 11 删除了角色的 `resources`。
- 被工作区使用的角色不能删除。
- 首次启动时写入 7 个默认角色：PMO 负责人、通用数字员工、4 个组成产品上线团队的专职角色和对话用的「老友」，见[默认角色](#product_launch_roles)。它们都还没有绑定 Agent，职责和角色指令已经填写。
- 默认角色由 `sync_seeds` 维护：首次启动时写入（写入后记 `meta.seeded`），之后每次启动把用户没改过的职责和指令换成这个版本的默认文本，改过的保留并提示有新版，删除的不会恢复。规则见[默认角色](#product_launch_roles)。

<a id="product_launch_roles"></a>
### 默认角色：通用员工和产品上线团队

默认团队是 PMO 负责人加一个**通用数字员工**（`generalist`）：资源库里的工具和 Skill 每个员工都能用（见 [Run 和会话怎样拿到资源](20261116-resource_library.md#delivery)），一个通用员工开几个实例就能接各种子任务，独立验证由另一个实例做。新建工作区的预设是 PMO 加通用员工（3 个实例）。数据版本 6 给以前就有默认角色的数据加上通用数字员工；删掉之后不再恢复。

「老友」（`friend`）用于[对话](20261120-chats.md)：像老朋友一样陪用户把一件事聊透，好奇、会追问、敢说真话，不急着给建议。它不是 Agent Team 的成员设计，没有负责人能力。数据版本 15 给以前就有默认角色的数据加上它；用户自己已经有一个同 id 的角色（当时的 id 是 `inner-grill`）时保留用户的，只有文本和默认完全一样（手动复制进来的）时才把它认作默认角色。

专职角色仍然保留，需要时加进工作区：它们按「一个 PMO 负责人推进产品从需求到上线」设计。PMO 只负责拆解、顺序、分工、跟进和汇总，不定义产品范围和成功标准：产品范围、优先级、成功标准和产品验收归产品经理，市场侧成功指标归市场运营，技术方案归研发工程师，质量结论归质量工程师。每个默认角色一个文件 `backend/roles/<id>.md`，编译时打进 Janus（`ROLE_FILES`）：开头两行 `---` 之间是元数据 `id`（和文件名相同，发布后不改）、`name`、`desc`（职责）、`color`、`init`（头像上的字）、`lead`（`true` 或 `false`），之后是指令全文。改职责或指令不需要迁移，按下面的规则随版本更新；新增一名要把文件加进 `ROLE_FILES`，再加一条迁移（照数据版本 15，用 `add_seed`），老用户升级时才会有它。

默认职责和指令随版本更新（`sync_seeds`，设计见 [ADR：数据随版本自动升级](../decisions/20261118-data_migrations.md)）：

- 每个默认角色的 `seed` 记录它来自哪个默认角色，以及用户最后拿到的职责和指令的哈希。
- 启动时逐项比较：当前文本的哈希与记录相同，说明用户没改过，换成这个版本的默认文本并更新记录；与记录不同，说明用户改过，保留用户的文本。
- 改过的项有更新的默认文本时，设置接口返回的角色带 `seedUpdates`（`{字段: 新文本}`），编辑器在职责或指令下方提示，用户可以「采用新版」或「保留我的」（`POST /api/roles/{id}/seed`）。两种选择都更新记录，直到下一个版本带来新的默认文本前不再提示。
- 数据版本 2 之前的默认角色没有 `seed`：迁移按 `SEED_HISTORY`（历次发布过的文本的哈希）判断，当前文本是发布过的某一版就算没改过，否则按改过处理。这些数据说不出用户是从哪一版默认文本改起的，所以改过的项会提示一次当前的默认文本，选「保留我的」后不再提示。

负责人只凭名称和职责认识协作者（见[负责人知道什么](20261031-goal_mode.md#lead_brief)），所以默认职责写成「什么样的子任务适合派给它」。

| id | 名字 | 职责 |
| --- | --- | --- |
| `pmo` | PMO | 负责人：拆目标、排顺序、派活、盯进度和风险、汇总交付；不替专业角色拍板 |
| `generalist` | 通用数字员工 | 什么都能接：调研、需求、编码、测试、文档、独立验证 |
| `marketing` | 市场运营 | 用户研究、竞品分析、发布与增长；需要外部证据或对外内容时找它 |
| `product-manager` | 产品经理 | 理清需求和优先级，写带验收标准的 PRD，产品问题由它拍板 |
| `developer` | 研发工程师 | 写方案、写代码、修 bug、补测试；改代码和配置的活儿交给它 |
| `qa` | 质量工程师 | 设计和跑测试、独立验证别人的改动、给出质量结论；验证时只看不改 |
| `friend` | 老友 | 陪你把一件事聊透：会追问、敢说真话，帮你看清自己真正想要什么 |

指令作为系统提示词注入，只写各角色的专业判断。结构化结果、看板工具、文件认领、验证判定和只在用户要求时 commit、push、部署这些协作机制由 Janus 的固定约定说明，指令不重复。共同约定：

- 按规模选择流程。小目标（修一个缺陷、加一个小功能、回答一个调研问题）由 PMO 直接派给职责最匹配的协作者，改了代码的再安排一次独立验证；新产品、跨模块的功能或范围不清的目标，按澄清、定义、执行、验收四个阶段推进。PRD 的篇幅也与目标规模相称。
- PMO 按协作者名单中的职责找人，不按名字；名单里没有某类职责的人时，交给职责最接近的协作者并写清要求，谁都做不了时告诉用户缺什么能力；一个人一轮能做完的小目标 PMO 可以自己做，一旦派出子任务就只做管理。
- 澄清：产品经理对照目标、工作区共享资料和已有证据，列出只有用户能回答、而且会明显改变范围、优先级或成功标准的问题（最多 5 个，带选项和建议），能推断的写成假设；PMO 合并成一次交给用户，等待时先推进不依赖答案的工作。
- PRD 写成工作目录中的 Markdown 文件，功能（F-1）和验收标准（AC-1）都编号，验收标准写成可观察的行为；子任务、证据、质量结论和产品验收都按编号对应。
- 执行中的提问都先到负责人（见[提问的流转](20261031-goal_mode.md#question_routing)）。产品问题 PRD 有答案的，PMO 按 PRD 回复；没有答案的，由产品经理裁定并更新 PRD。市场、技术和质量问题分别由对应的角色决定。
- 改了代码的子任务完成后，由质量工程师做独立验证；独立验证时只检查不改文件，按 Janus 的要求给出判定。是否达到产品目标，以产品经理的产品验收和质量工程师的质量结论为准。
- 区分事实、推断和假设；不把未运行的检查写成通过，不把未完成的工作写成完成。市场运营不能联网或找不到可靠来源时如实说明，结论标为待验证。
- 改变范围、预算或周期，使用外部资源或付费服务，以及不可逆或影响工作目录之外的操作，交给用户决定；市场运营对发帖、投放、发送邮件、提交应用商店审核等对外操作只准备材料和步骤。
- 研发写明配置、数据迁移和部署的操作步骤和回滚方式；用户在澄清中的回答、PRD 的位置等值得长期保留的事实，由 PMO 列入共享资料建议，见[工作区共享资料](#workspace_brief)。

<a id="avatars"></a>
### 头像

数字员工的头像默认是底色（`color`）加文字（`init`）；有自己的图片时，所有显示头像的地方都换成图片：设置页、工作区成员、任务的团队和看板、收件箱、对话。

- **存放**：图片是 256×256 的 PNG，存在 `data/roles/<id>/avatar.png`，不进数据库。角色记录的 `avatar` 是图片内容的哈希，空字符串表示没有图片（数据版本 16 给已有的角色加上这个字段，取空）。删除数字员工时图片一起删除。
- **更换**：在数字员工编辑器标题下点「更换头像…」选一张图片，有图时旁边有「移除头像」，也可以把图片文件拖到头像上；点头像本身选颜色（没有图片时显示）；应用从中间裁成正方形、缩到 256 像素再上传（`AvatarImages.square`），和角色一起保存，新建的数字员工也可以先选图。接口是 `PUT`、`DELETE`、`GET /api/roles/{id}/avatar`，服务只收 PNG，最大 1 MB。编辑角色不会改动图片：`avatar` 只由这组接口改变。
- **默认头像**：每个默认数字员工在 `backend/roles/<id>.png` 带一张拟人的头像（由 OpenAI ImageGen 生成），首次启动时写入。之后的版本换了默认头像时，按职责和指令同样的规则更新（`seed.avatar` 记录用户拿到的那一张）：用户没换过的换成新的，换成自己的图或移除了的保持不变；数据版本 16 之前就有的默认数字员工，升级后得到默认头像。
- **任务里的显示**：成员快照记着它来自哪个数字员工（`role_id`），头像按这个数字员工现在的图片显示；数字员工被删除后回到文字头像。应用按哈希缓存图片，图片没变不重新读取。

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

- **查看与编辑**：工作区首页头部右边显示当前的共享资料，从标题旁的按钮（「+」或铅笔）打开只编辑共享资料的表单（`BriefEditor`），和管理团队的工作区设置分开。只有用户能改。Agent 不能直接写入，避免错误的结论一直影响之后的 Run。
- **快照**：Run 创建时随工作区写入 Run 快照（`config.workspace.brief`），之后的修改不影响这个 Run，从中断处继续时也用快照中的版本。
- **注入**：`brief_section` 把它附在负责人说明（`goal_brief`）和每个任务提示（`task_prompt`）中，写明与本次目标或任务说明冲突时以后者为准；值班的系统层（`loop_brief`）在会话打开时附上工作区当时的共享资料，冲突时以值班的提示为准。没有共享资料时什么都不加。
- **积累**：负责人说明要求负责人在最后一条回复末尾，用 `## Suggested Updates to Shared Context` 标题列出这次确定的、值得长期保留的新事实，只写新增或需要修改的内容。`split_brief_suggestions` 把这一节从交付摘要中拆出来，放进 `run.delivered` 的 `briefSuggestions`。解析时也识别旧标题「建议更新共享资料」，接受加粗和末尾冒号，去掉条目的列表标记，读到下一个标题为止。
- **采纳**：应用在交付结果中显示这些条目和「加入共享资料…」按钮，按钮打开共享资料的表单，把条目以 `- ` 开头追加到共享资料末尾；用户可以修改，保存后才生效。

为什么只让用户写入，而不是让 Agent 像 Grok Bot 的共享记忆那样直接维护：Janus 的 Run 由 Agent 自主推进，用户不一定逐条核对过程中的结论。共享资料会进入之后每一次 Run 的每一个会话，一条错误的结论会被反复当作事实使用。由负责人提出、用户确认，既能积累，又保留了用户的把关。Agent 自己在 Run 里摸清、还没到要团队遵守的事，记在下面一层的[团队记忆](20261123-team_memory.md)里：那是 Agent 写、记来源、可以推翻的笔记，用户认可的一条可以从那里加进共享资料。

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

`team_for` 把工作区展开成 Run 的成员（`Member`），每个角色先经 `role_member` 转成成员，[对话](20261120-chats.md)的唯一成员也由它生成。成员包括：角色字段、Agent 类型与名称、会话控制项（角色的选择加上没有选择的权限模式的默认值）、权限模式标签（`permission`，例如「Full access」，显示在 Run 的审计页）、角色指令和 Provider 的自定义提示词、固定下来的资源（`resources`，见 [Run 和会话怎样拿到资源](20261116-resource_library.md#delivery)）。

这些字段同时用于：打开会话时设置控制项、负责人说明中的协作者名单、应用的「团队与权限」页。
