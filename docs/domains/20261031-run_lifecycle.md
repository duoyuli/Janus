# Run 生命周期

本文说明 Run 和任务的状态机：创建、调度、尝试、验证、重试、交付、用户操作和恢复。负责人与协作者之间的协作规则见[目标模式与共享看板](20261031-goal_mode.md)，事件字段见[事件参考](../interfaces/20261031-events.md)。

## 导航

- [创建 Run](#create_run)：修改 `create_run` 或快照内容时阅读。
- [图片附件](#attachments)：修改 `backend/src/attachments.rs`、目标或消息的图片、提示中的图片块时阅读。
- [工作目录](#workspace_model)：涉及任务在哪里读写、diff 和 Git 时阅读。
- [任务调度](#scheduling)、[一次尝试](#attempt)、[验证](#verification)、[重试与失败](#retry)：修改调度循环或尝试流程时阅读。
- [停滞检测](#watchdog)：修改 `watchdog`、`watch_call`，或调整什么算「没有进展」时阅读。
- [交付条件](#delivery_conditions)：修改 `execute` 的结果判断时阅读。
- [Run 状态](#run_statuses)、[用户操作](#user_actions)、[中断与恢复](#recovery)。

每个 Run 都以负责人用 `/goal` 推进目标开始，任务全部来自负责人的编排，见[目标模式与共享看板](20261031-goal_mode.md)。调度循环（`execute`）只负责执行看板上的任务并判断何时交付。

<a id="create_run"></a>
## 创建 Run

`App::create_run` 依次执行：

1. 目标不能为空（只附了图片时目标记为「按附图完成」），工作区和工作目录必须存在。
2. 为每个成员构建运行环境（`spec_for`），缺少 CLI 或适配器没有安装时直接报错。
3. 负责人必须是 Claude Code 或 Codex（`Runtime::leads`）。
4. 判断工作目录是不是 Git 仓库；是则记录当前 `HEAD` 作为基线。
5. 写入 Run 快照（含可选的 Token 展示参考值 `tokenLimit` 和费用上限 `costLimitUsd`），复制 Skill，保存随目标粘贴的图片（快照的 `goalImages`），见[数据模型与存储](../architecture/20261031-data_model.md#snapshots)和[图片附件](#attachments)。
6. 启动驱动任务，进入负责人的目标会话（`goal`）。

Run id 是 `r` 加当前毫秒时间戳的 base36 编码（`agents::new_id`）；同一毫秒内创建的 Run 依次加一，id 不会重复。

Run 快照中的工作区带着创建时的[共享资料](20261031-agents_and_roles.md#workspace_brief)，之后修改工作区的共享资料不影响这个 Run，从中断处继续时也使用快照中的版本。

<a id="attachments"></a>
## 图片附件

用户可以在目标和发给会话的消息里粘贴图片。实现在 `backend/src/attachments.rs`，接口见 [HTTP API](../interfaces/20261031-http_api.md#images)。

- **保存**：图片随请求以 base64 上传，按文件头识别类型（PNG、JPEG、GIF、WebP，声明的类型不作数），每张最多 10 MB，一次最多 10 张；任何一张不合格时整条请求拒绝，不保存任何文件。文件写入 `~/.janus/runs/<run>/attachments/`，和这个 Run 的 Skill 快照放在一起。
- **点名**：Agent 收到的文字中，每张图片单独占一行 `[Image N] <路径>`。只有图片的消息会带英文查看提示。历史消息的 `[附图 N]` 标记仍可解析。目标的图片写进负责人说明，并提示负责人把路径写进任务说明，方便协作者参考。
- **发送**：每次 `session/prompt` 和 `_session/steering` 发出前，`prompt_blocks` 找出文字点名的附件，在 Agent 声明了 `promptCapabilities.image` 时作为 ACP 图片块附在文字后面。只读取 `runs/*/attachments/` 下的文件，所以文字里看起来像附图行的内容不能让 Janus 发出任意文件。官方 Claude、Codex 和 Grok 适配器都声明了这项能力；其他 Agent 没有声明时只收到路径，由 Agent 自己打开文件。
- **跟随消息流转**：图片行就是消息文字的一部分，所以插话、排队、留言、交还托管和中断后恢复的消息都带着它们，不需要另外处理。
- **事件**：`run.created` 和 `user.message` 的 `data.images` 列出保存的图片（`path`、`mimeType`、`name`、`size`），事件只记录路径，不包含图片数据。

<a id="workspace_model"></a>
## 工作目录

- 负责人和所有任务都直接在工作区目录中运行，改动立即彼此可见。
- 一个任务能否修改文件，完全由它的 Agent 权限模式决定，见 [Agent、角色与工作区](20261031-agents_and_roles.md#permission_modes)。
- 非 Git 目录中也可以修改文件，但没有 diff 产物。
- Janus 不替任务提交，也不推送。

不使用独立的 worktree，取舍见 [ADR：任务直接在工作目录中运行](../decisions/20261031-in_place_workspaces.md)。

<a id="scheduling"></a>
## 任务调度

### 任务状态

| 状态 | 含义 | 终态 |
| --- | --- | --- |
| `pending` | 等待依赖完成或等待执行者空闲 | 否 |
| `running` | 有一次尝试在进行 | 否 |
| `blocked` | 依赖失败，无法开始；所有依赖都不再是失败状态（被重试或接受）后自动回到 `pending` | 否 |
| `succeeded` | 成功 | 是 |
| `failed` | 用完尝试次数 | 是 |
| `cancelled` | Run 被取消时正在执行 | 是 |
| `skipped_by_decision` | 被负责人取消（`cancel`）；连带取消的任务随上游的 `retry` 回到 `pending` | 是 |

这 7 个是调度器写出的状态（`TaskStatus`，`backend/src/status.rs`），合法转移集中在 `TaskStatus::can_transition_to`。应用另外从事件推导几个只用于显示的状态，它们不参与调度：`ready`（`task.ready`）、`assigned`（`task.assigned`）、`verifying`（`verification.started`）、`verification_failed`（`verification.failed`）、`waiting_input`（`question.asked` / `input.requested`）、`waiting_approval`（`approval.requested`）。重启恢复时以看板重建出的 7 个状态为准。应用中这 13 个状态是 `TaskStatus` 的成员（`frontend/Sources/Janus/Core/Status.swift`），Run 的状态是 `RunStatus`；进行中、已结束、可以从中断处继续这些分组也在那里定义，界面不再各自列字符串。

### 启动条件

一个任务可以启动，需要同时满足：

- 状态为 `pending`。
- 所有依赖（`deps`）都已 `succeeded`，并且没有被未通过的独立验证挡住（见下文）。
- 执行者当前没有别的任务在执行。每个成员实例同一时间只做一个任务，派给同一实例的任务排队；工作区中配置了 `slots > 1` 的角色会展开为多个成员实例，可并行执行不同任务，见 [Agent、角色与工作区](20261031-agents_and_roles.md#workspaces)。
- 当前正在运行的协作者尝试总数低于 `max_concurrent_attempts`（默认 3，见 [ADR：角色并发槽位与全局并发上限](../decisions/20261118-role_slots_and_concurrency_cap.md)）。超出上限时，已就绪的任务保持 `pending`，等有槽位空出后自动启动。
- Run 没有暂停、没有取消，也没有超出墙钟或已上报美元费用预算。

依赖失败（失败、取消、阻塞或被跳过）时，下游任务变为 `blocked`（`task.blocked`）。有多个依赖失败时，处理好一个后任务仍保持 `blocked`，并再写一条 `task.blocked` 指向还没处理的依赖。

独立验证任务判定 `rejected` 或 `inconclusive` 时自身是 `succeeded`，但被验证的工作没有得到确认。在判定写入之前就编排好、依赖这个验证任务的任务，是按「验证会通过」安排的，同样变为 `blocked`（`task.blocked` 带 `data.verdict`），不会在没确认的工作上继续；负责人读到判定之后再编排、依赖它的任务（例如修复任务）照常开始。判断依据是任务写入看板时的事件序号（`Task::planned_seq`）和验证任务 `task.succeeded` 的序号（`DepGate::holds`），重启后从 `orchestration.accepted` 的序号重建。

尝试结束、负责人 `cancel` / `retry` / `accept` 都会改任务状态。尝试结束时，确认这次尝试仍是任务当前的尝试（`active_attempt`）和写入结果在同一把锁内完成；负责人已经取消或重试了这个任务时，这次尝试的结果按过期处理（`attempt.late_result`），尝试记录标为 `cancelled`，不会覆盖负责人的决定。

<a id="attempt"></a>
## 一次尝试

每次尝试是一个新的 ACP 子进程和会话（`App::attempt`）：

1. 打开会话（或续接原会话），默认调用 `session/new`，有两种续接：
   - 从中断处恢复未完成的尝试：Agent 支持 `loadSession` 时直接 `session/load` 加载被中断的会话，不看上下文占用；加载失败或不支持时新建会话。
   - 后续任务续接（任务声明了 `continues`，只在首次尝试）或负责人 `retry` 指定了 `continue_session: true`：目标会话最新上下文占用严格大于 75%、Agent 不支持 `loadSession` 或加载失败时新建会话，否则 `session/load` 复用原会话；新建还是复用、新建的原因都记在 `attempt.started` 的 `data.continuation`（见 [ADR：热会话续接](../decisions/20261118-session_continuation.md)）。

   接入角色配置的 MCP 服务器；Agent 声明支持 HTTP MCP 时，还接入共享看板。
2. 按角色设置模型、推理强度和权限模式等会话控制项，见 [ACP 集成](../interfaces/20261031-acp_integration.md#session_setup)。
3. 发送任务提示（`task_prompt`）：总目标、工作区共享资料（有时，与任务说明冲突时以任务说明为准）、任务说明、验收标准、工作目录、上游结果（含一句话摘要、专门给下游的 `handoff` 交接说明、前 5 条上游风险、负责人的修订、改过的文件及后续被他人或负责人修改的提醒）、同时进行的任务和上次失败原因；独立验证任务还会收到被验证任务的专属 diff 路径、实际改动文件、痕迹核对异常、弱证据条目提醒和风险（`teamwork::VerifiedContext`）。怎样验证、提问和返回结构化结果的约定，以及角色指令和 Provider 的自定义提示词，都在会话的[系统层](../interfaces/20261031-acp_integration.md#standing_instructions)中，见[协作者的约定](20261031-goal_mode.md#collaborator_rules)；只有没有系统层入口的 Agent 才附在任务提示末尾。
4. 收集流式输出、工具调用、审批请求和表单请求。单轮超时发送 `session/cancel` 后，仍等待最多 2 秒读取返回值中的 `prompt_usage`；每轮发送前记录事件序号和会话日志末尾位置。若尝试异常结束且本轮尚未上报 Token，则从 Janus 隔离目录（Claude Code 的 `CLAUDE_CONFIG_DIR`，即 `~/.janus/agents/<id>/home`）下的 `projects/**/<sid>.jsonl` 读取本轮新增记录，按 `message.id` 取最后一次读数补齐用量。同一会话前面几轮的读数照常保留。
5. 解析回复中最后一个 ```` ```json ```` 代码块作为结构化结果。

<a id="structured_result"></a>
### 结构化结果

```json
{
  "summary": "一句话结果",
  "handoff": "给下游子任务的接口约定、关键决策与注意事项（没有下游依赖时可填 null）",
  "verdict": "approved",
  "checks": [{ "criterion": "验收标准原文", "passed": true, "method": "test", "evidence": "怎么验证的、看到了什么" }],
  "evidence": ["其他可核对的证据"],
  "risks": ["已知风险，没有就留空数组"],
  "question": null
}
```

- `handoff` 可选（最长 1200 个字符），供上游执行者写给下游的接口签名、边界约定和注意事项，与给负责人扫读的一句话 `summary` 分离。
- `verdict` 供独立验证任务（带 `verifies`）填写对被验证产物的判定：`approved`（达标）、`rejected`（存在阻断问题，需原执行者修改）或 `inconclusive`（当前环境证据不足）；未填时按 `checks` 是否全部通过推导（任一条 `passed: false` 推导为 `rejected`），填写了允许值以外的内容时核对报错。
- `checks` 按验收标准的顺序逐条给出，验证方式由 Agent 自己决定。
- `method` 可选，说明这条依据的类型：`test`、`command`、`inspection`、`reasoning`、`unverified`，含义见[证据方法](../interfaces/20261031-events.md#evidence_methods)。省略它的结果没有方法，解析、恢复、核对和展示都按「未说明」处理，不当作 `reasoning`，也不因此判失败；存在但不是允许值时，原样保留在结果里并由核对报错，不会静默换成合法方法。证据方法由 `backend/src/evidence.rs` 解析和汇总，结构化结果的读取（`parse_result`）、事件里结果的重建和恢复（`resume_run`）都用同一段逻辑。
- 没有 JSON 块时，摘要取回复第一行，并记一条风险「Agent 没有按约定返回结构化结果」。
- `question` 不为空表示 Agent 在提问：`{"text": "问题", "options": ["A", "B"], "recommend": "A"}`。回答作为下一轮提示发回同一个会话。同一次尝试最多连续普通提问 4 轮（伴有 `request_split` 拆分提议的暂停不计入这 4 轮，总暂停硬上限为 8 轮）。提问由谁回答见[提问的流转](20261031-goal_mode.md#question_routing)。

<a id="verification"></a>
## 验证

结果提交后，Janus 核对结果（`verify_result` / `evidence::gaps_for`，`verification.*` 事件）。Janus 不运行任何验证命令，验证由协作者按验收标准自己完成，见[验收标准](../interfaces/20261031-board_mcp.md#acceptance_criteria)。核对将「任务自身是否完成了核查工作」与「独立验证者对目标产物的审批结论」分开：

- `summary` 不能为空。
- 普通执行任务：每条验收标准都有对应的 check（按顺序），check 有证据，且没有 `passed: false`。
- 独立验证任务（带 `verifies`）：每条标准都必须核对并给出具体证据，`verdict` 若填写必须是 `approved`、`rejected` 或 `inconclusive`，且 `verdict: "approved"` 不能与 `passed: false` 矛盾。只读验证者发现真实缺陷并给出可复现证据、填 `passed: false` 或 `verdict: "rejected"` / `"inconclusive"` 时，它的核查工作已经完成，第一轮即通过结果结构核对并标记验证任务完成（`task.succeeded`），不会触发无意义的自我修复轮；同时在看板和交付中把目标产物标为未通过确认（`[验证未通过]`、`Verification.passed == false`），唤醒负责人派修复任务和复验任务。
- check 给出 `method` 时必须是允许的值；其他值作为「不是允许值」的失败项报出，不会被改成合法方法。
- `reasoning`、`unverified` 和没有 `method` 只记提醒，不阻断：提醒随核对结果记录在 `verification.*` 和 `task.succeeded` 的 `data.acceptance` 中，看板的完成消息、`task_detail` 和交付都显示。

验证没通过时，同一次尝试中按以下顺序处理：

1. **在原会话里补一次**：把没核对、没证据或没达到的标准逐条发回同一个会话（`session.prompt`，`channel: "fix"`），请协作者修复后重新验证、补上证据；确实无法达到的如实写明原因。协作者保留着上下文，比开新会话从头做快，也更容易修对。补完后重新提交结果、重新核对。
2. **仍然没通过**：任务失败（`task.failed`，`reason: "verification"`），不自动开新会话重试，由负责人决定接受结果、修改后重试还是取消，见[看板 MCP 工具](../interfaces/20261031-board_mcp.md#accept_tool)。

用户在会话中直接以当前结果结束时，不走修复这一步。

<a id="retry"></a>
## 重试与失败

- 每个任务最多尝试 2 次（`MAX_ATTEMPTS`）。尝试失败（Agent 进程退出、超时、自行取消等，不含验证没通过）后还有剩余次数时，任务回到 `pending`，下一次尝试带上上次失败的原因（`task.retry_scheduled`）。
- 用完次数或验证没通过后任务变为 `failed`（`task.failed`），Run 不停下，由负责人在看板上读到后用 `retry`、`accept`、`cancel` 处理，或自己做。被它阻塞的任务不需要重新编排。
- 只有负责人能重试任务（看板的 `retry`），重试会重新给满尝试次数（`task.reopened`，`reason: "lead.retry"`）。依赖它的 `blocked` 任务在它成功后自动恢复。用户想重做某个任务时，给负责人发消息。
- 被打断而没有任何输出的一轮不保存回复产物。
- 尝试结束后，它名下未处理的提问、审批和表单被撤回（`decision.withdrawn`）。过期尝试返回的结果被忽略（`attempt.late_result`）。

<a id="timeouts"></a>
### 时间限制

| 限制 | 值 |
| --- | --- |
| Agent 启动和打开会话 | 10 分钟 |
| 单次尝试 | 30 分钟 |
| 单个 ACP 请求 | 30 分钟 |
| 发送打断等控制指令 | 15 秒 |
| Run 预算 | 墙钟 6 小时，以及可选的已上报 `USD` 费用上限（`maxCostUsd` / `JANUS_RUN_COST_BUDGET_USD`），均可用 `extendBudget` 追加；未上报用量（`None`）永不触发暂停；调度循环每次唤醒时读取内存中的美元合计。费用读数持久化后按原生会话更新增量，从中断处继续时按事件顺序重建一次；从中断处继续时墙钟重新计 6 小时 |

<a id="watchdog"></a>
### 停滞检测

时间限制只在超时后才生效。为了更早发现卡住的协作者，调度循环每次唤醒（最多间隔 2 秒）都检查正在进行的尝试，发现以下两种情况时写入 `task.stalled` 事件：

| 情况 | 判断 | `data.reason` |
| --- | --- | --- |
| 没有动静 | 尝试的会话 10 分钟（`STALL_AFTER`，即 `Config.stall_after`）没有发出任何更新：文本、思考、工具调用和工具进度都算 | `idle` |
| 原地打转 | 连续 5 次（`LOOP_REPEAT`）完成同一个调用（`period: 1`，连续次数不受滑窗限制），或在最近 12 次工具调用的滑窗内连续 4 轮重复 2 步交替序列（`ABABABAB`，`period: 2`）、连续 3 轮重复 3 步循环序列（`ABCABCABC`，`period: 3`） | `loop` |

规则：

- 等人的时间不算：尝试在等提问的回答、审批或表单（`Run::held` 返回的守卫存在期间），或会话被用户接管或打断（`Run::set_human`）时，计时从头开始。这些状态由尝试自己在内存中标记，停滞检测不读事件日志。
- 同一段没有动静的时间只报告一次。会话再发出任何更新后重新计时，并写入一条 `data.resolved` 为 `true` 的 `task.stalled`，应用据此撤下「可能卡住了」，负责人不会读到它。同一个调用每连续满 5 次报告一次（5、10、15……），换了调用后重新计数。
- 负责人会话按同样的规则检查（看门狗按会话记录，负责人的会话是 `SessionKey::Lead`），结果写 `lead.stalled`，`to` 为 `user`：负责人之上没有别人会发现它卡住。应用在「需要你处理」中显示「负责人可能卡住了」，再有动静时撤下。负责人调用 Janus 看板工具不计入原地打转，反复 `wait` 是正常的。
- 停滞检测只报告，不打断、不重试也不取消。负责人在 `wait` 中读到 `[停滞]`，自己判断继续等、`reply`、`retry` 还是 `cancel`，见[目标模式与共享看板](20261031-goal_mode.md#failures_in_goal_mode)。
- 计时状态只在内存中（`Run.watch`），重启后从头开始；`task.stalled` 不影响恢复。

<a id="delivery_conditions"></a>
## 交付条件

调度循环每次唤醒都重新判断结果。「没有可执行任务」指没有任务在执行，也没有 `pending` 任务的依赖已全部成功。因暂停或预算而没有启动的任务仍算可执行：负责人在暂停期间结束时，Run 等用户继续，不会丢下这些任务直接交付。

| 条件 | 结果 |
| --- | --- |
| 没有可执行任务，且负责人会话异常结束 | Run 失败（`run.failed`），可以从中断处继续 |
| 没有可执行任务，且负责人目标已结束 | 交付（`run.delivered`），摘要取负责人最后一条回复 |
| 超出预算，且还有未结束的任务 | 等待用户追加预算（`run.budget_exhausted`）。每次进入等待都写一条：回到运行后负责人再加任务、又超出预算时会再次提醒 |
| 用户暂停，且没有任务在执行 | 暂停 |
| 其他 | 继续运行 |

任务失败不会让 Run 停下来等用户。负责人结束时还有协作者在等负责人回答的，提问转给用户，见[目标模式与共享看板](20261031-goal_mode.md#question_routing)。

交付时 `run.delivered` 的 `data.usage` 是一行用量摘要（token 及完整性、费用状态、墙钟时长），和 Run 页、`task_detail`、`GET /api/runs/{id}/usage` 读同一份账本，见[用量口径](../interfaces/20261031-events.md#usage_accounting)。用量不是交付条件：没有会话上报 token、费用缺失或口径未知时，摘要写「未上报」「部分上报」「口径未知」，Run 照常交付。

交付后负责人的会话不结束，而是保留为常规对话：用户可以直接写下一轮任务，或用 `/goal` 设置新的目标。负责人空闲等消息时不接收看板推送，协作者的提问转给用户；处理一条消息的这一轮里，看板推送和提问照第一个目标处理（见[提问的流转](20261031-goal_mode.md#question_routing)）。它在对话中通过看板提交的新任务会重新启动调度循环，完成后再次交付。服务重启或会话退出后，第一次给负责人发消息会重新打开会话：能 `session/load` 就恢复原会话，否则新建会话，并带上目标、交付摘要和最近的对话。已验收的 Run 也按同样方式恢复，继续工作或再次交付时重新打开。

<a id="run_statuses"></a>
## Run 状态

| 状态 | 含义 |
| --- | --- |
| `planning` | 刚创建，负责人会话正在启动 |
| `running` | 运行中；暂停时状态也保持 `running` |
| `waiting_input` | 超出预算，等待用户追加预算或取消 |
| `delivered` | 已交付，等待验收 |
| `accepted` | 用户核对后验收。只能从 `delivered` 进入；继续下指令或设置新目标时重新打开 |
| `cancelled` | 用户取消 |
| `failed` | Run 失败，例如负责人会话异常结束 |
| `interrupted` | 编排器重启时 Run 还未结束 |

状态是 `RunStatus`（`backend/src/status.rs`），合法转移集中在 `RunStatus::can_transition_to`。`paused` 不是独立状态：暂停时状态保持 `running`，只有 `planning`、`running` 和 `waiting_input` 可以暂停、继续或追加预算；只有 `delivered` 可以验收；已验收、已取消的 Run 不能再停止；已验收的 Run 在会话里继续工作或再次交付时重新打开；取消直接由 `status == RunStatus::Cancelled` 表示，不另存布尔标记。


<a id="user_actions"></a>
## 用户操作

用户通过 `POST /api/runs/{id}/actions` 操作 Run，参数见 [HTTP API](../interfaces/20261031-http_api.md#run_actions)：

- **回答与审批**：`answer`、`approve`、`input`。
- **介入会话**：`message`、`interrupt`、`takeover`、`release`、`finish`。会话正在运行时，消息通过 ACP 实时插入（支持 `_session/steering` 时，见[插话](../interfaces/20261031-acp_integration.md#steering)）或排队到这一轮结束；会话已结束时，消息保留到这个 Agent 下一次开始工作。每条消息发出时立即记录，之后再记录它的去向和送达时刻，见[消息的流转](../interfaces/20261031-events.md#message_flow)。
- **控制 Run**：`pause`、`resume`、`extendBudget`、`cancel`（界面上叫「停止」）、`accept`、`resumeRun`。
- **删除 Run**：应用里删除一个已结束的 Run（`DELETE /api/runs/{id}`，见 [HTTP API](../interfaces/20261031-http_api.md#run_delete)）。删除会移除它的事件、派生索引，以及 `~/.janus` 下的产物、日志、附件和成员资源包，不可恢复；工作目录中的文件不动。`planning`、`running` 或 `waiting_input` 的 Run 不能删除，要先停止。删除不是取消，也不影响其他 Run。

取消 Run（`run.cancelled`）和取消任务（`task.skipped_by_decision`）在用量账本里的范围不同：前者把取消时刻仍在跑的所有尝试记「已取消」，备注「任务被取消」；后者只影响那个任务的尝试，备注「负责人取消了这个子任务」，同一个 Run 里其他任务的尝试保持「未完成」。两者都不改动取消前已经结束的尝试（失败、验证没通过、过期保持原状态），已经上报的用量照常计入。Janus 不为这两类取消写尝试级事件，杀连接后可能还有一条失败事件，也不算真的失败。见[用量口径](../interfaces/20261031-events.md#usage_accounting)。

任务的重试、接受和取消由负责人通过看板处理，用户不直接操作任务；用户可以给负责人发消息提出要求。

<a id="recovery"></a>
## 中断与恢复

编排器启动时 `recover`：

- 仍在进行的尝试标记为 `interrupted`。
- 未结束的 Run 写入 `run.interrupted` 事件，状态改为 `interrupted`。

用户「从中断处继续」（`resumeRun`），或直接给某个 Agent 发消息时，`resume_run` 从 `runs.config` 重建团队，从事件日志重建任务。恢复、验收历史交付（`accept_archived`）和删除一个不在运行的 Run 先拿 `App.revival`，一次只做一件：两条消息同时发给已中断或已交付的 Run 时，第二条等第一条把 Run 恢复好，再发进同一个 Run，不会各自恢复出一个负责人会话；恢复期间到达的验收按运行中的 Run 处理：

- 从所有 `orchestration.accepted` 事件重建看板任务（含 `owns`、`verifies`、`continues`）。编排事件的 `plannedSeq` 保存任务加入看板时的事件序号，恢复后沿用同一序号判断依赖的验证结论。旧事件缺少该字段时采用接纳前的序号，若已有明确的验证阻塞记录则保留该阻塞。从快照和追加预算事件恢复美元费用上限。旧版 Token 预算只作为用量参考值显示。已成功的任务从它的 `task.succeeded` 读回完整结果（摘要、`handoff`、验证判定 `verdict`、证据、风险、`traceGaps`、`checks` 及每条的证据方法，负责人 `accept` 的还有接受理由），证据方法计数和提醒随之重建，所以重启后看板、`task_detail` 和交付给出的证据汇总不变；没有完成的任务读回最后一次 `result.submitted`。失败和阻塞的任务保持原状态，被连带取消的任务记住是因哪个上游取消的，未完成的任务重新开始（带 `continueSession` 的重试保留该标记）；重启前仍在进行的尝试，Agent 支持 `session/load` 时重新打开原会话。协作者和负责人的文件改动痕迹（`Actor::Task` 与 `Actor::Lead`）及已报告的冲突一并从事件日志恢复。
- 排队但还没送达的消息保留下来。
- 负责人以「从中断处继续」的说明重新启动，说明中列出看板上之前的任务。

`run.failed` 之后同样可以从中断处继续。已交付的 Run 在重启后仍可验收（`accept`），不需要恢复。

用量账本不存数据库表，重启后从事件重算，所以恢复前后的数字相同；取消、失败和过期尝试的用量也照常从事件里读回，见[用量口径](../interfaces/20261031-events.md#usage_accounting)。
