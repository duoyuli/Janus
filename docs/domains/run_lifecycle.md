# Run 生命周期

本文说明 Run 和任务的状态机：创建、调度、尝试、验证、重试、交付、用户操作和恢复。主控与协作者之间的协作规则见[目标模式与共享看板](goal_mode.md)，事件字段见[事件参考](../interfaces/events.md)。

## 导航

- [创建 Run](#create_run)：修改 `create_run` 或快照内容时阅读。
- [图片附件](#attachments)：修改 `backend/src/attachments.rs`、目标或消息的图片、提示中的图片块时阅读。
- [工作目录](#workspace_model)：涉及任务在哪里读写、diff 和 Git 时阅读。
- [任务调度](#scheduling)、[一次尝试](#attempt)、[验证](#verification)、[重试与失败](#retry)：修改调度循环或尝试流程时阅读。
- [停滞检测](#watchdog)：修改 `watchdog`、`watch_call`，或调整什么算「没有进展」时阅读。
- [交付条件](#delivery_conditions)：修改 `execute` 的结果判断时阅读。
- [Run 状态](#run_statuses)、[用户操作](#user_actions)、[中断与恢复](#recovery)、[早期版本的 Run](#early_plan_runs)。

每个 Run 都以主控用 `/goal` 推进目标开始，任务全部来自主控的编排，见[目标模式与共享看板](goal_mode.md)。调度循环（`execute`）只负责执行看板上的任务并判断何时交付。

<a id="create_run"></a>
## 创建 Run

`App::create_run` 依次执行：

1. 目标不能为空（只附了图片时目标记为「按附图完成」），工作区和工作目录必须存在。
2. 为每个成员构建运行环境（`spec_for`），缺少 CLI 或适配器没有安装时直接报错。
3. 主控必须是 Claude Code 或 Codex（`lead_capable`）。
4. 判断工作目录是不是 Git 仓库；是则记录当前 `HEAD` 作为基线。
5. 写入 Run 快照，复制 Skill，保存随目标粘贴的图片（快照的 `goalImages`），见[数据模型与存储](../architecture/data_model.md#snapshots)和[图片附件](#attachments)。
6. 申请 Run 租约（10 分钟）。
7. 启动驱动任务，进入主控的目标会话（`goal`）。

Run id 是 `r` 加当前毫秒时间戳的 base36 编码。`run.created` 事件的 `data.mode` 固定为 `goal`，`resume_run` 据此拒绝继续早期版本的 Run。

Run 快照中的工作区带着创建时的[共享资料](agents_and_roles.md#workspace_brief)，之后修改工作区的共享资料不影响这个 Run，从中断处继续时也使用快照中的版本。

<a id="attachments"></a>
## 图片附件

用户可以在目标和发给会话的消息里粘贴图片。实现在 `backend/src/attachments.rs`，接口见 [HTTP API](../interfaces/http_api.md#images)。

- **保存**：图片随请求以 base64 上传，按文件头识别类型（PNG、JPEG、GIF、WebP，声明的类型不作数），每张最多 10 MB，一次最多 10 张；任何一张不合格时整条请求拒绝，不保存任何文件。文件写入 `~/.janus/runs/<run>/attachments/`，和这个 Run 的 Skill 快照放在一起。
- **点名**：Agent 收到的文字中，每张图片单独占一行 `[附图 N] <路径>`。消息只有图片时，文字是「用户发来了 N 张图片，请先查看：」加这些行。目标的图片写进主控说明，并提示主控把路径写进任务说明，方便协作者参考。
- **发送**：每次 `session/prompt` 和 `_lody/session/steer` 发出前，`prompt_blocks` 找出文字点名的附件，在 Agent 声明了 `promptCapabilities.image` 时作为 ACP 图片块附在文字后面。只读取 `runs/*/attachments/` 下的文件，所以文字里看起来像附图行的内容不能让 Janus 发出任意文件。Agent 没有声明这项能力时只收到路径，由 Agent 自己打开文件。目前 Claude 适配器声明了这项能力，Codex 适配器没有声明，Codex 用自己的看图工具读取路径。
- **跟随消息流转**：图片行就是消息文字的一部分，所以插话、排队、留言、交还托管和中断后恢复的消息都带着它们，不需要另外处理。
- **事件**：`run.created` 和 `user.message` 的 `data.images` 列出保存的图片（`path`、`mimeType`、`name`、`size`），事件只记录路径，不包含图片数据。

<a id="workspace_model"></a>
## 工作目录

- 主控和所有任务都直接在工作区目录中运行，改动立即彼此可见。
- 一个任务能否修改文件，完全由它的 Agent 权限模式决定，见 [Agent、角色与工作区](agents_and_roles.md#permission_modes)。
- 非 Git 目录中也可以修改文件，但没有 diff 产物。
- Janus 不替任务提交，也不推送。

早期版本为每个写任务创建独立 worktree，并有集成任务和只读快照，这部分代码已经移除。取舍见 [ADR：任务直接在工作目录中运行](../decisions/in_place_workspaces.md)。

<a id="scheduling"></a>
## 任务调度

### 任务状态

| 状态 | 含义 | 终态 |
| --- | --- | --- |
| `pending` | 等待依赖完成或等待执行者空闲 | 否 |
| `running` | 有一次尝试在进行 | 否 |
| `blocked` | 依赖失败，无法开始；依赖重试成功后自动回到 `pending` | 否 |
| `succeeded` | 成功 | 是 |
| `failed` | 用完尝试次数 | 是 |
| `cancelled` | Run 被取消时正在执行 | 是 |
| `skipped_by_decision` | 被主控取消（`cancel`） | 是 |

### 启动条件

一个任务可以启动，需要同时满足：

- 状态为 `pending`。
- 所有依赖（`deps`）都已 `succeeded`。
- 执行者当前没有别的任务在执行。每个成员同一时间只做一个任务，派给同一人的任务排队。
- Run 没有暂停、没有取消，也没有超出预算。

依赖失败（失败、取消、阻塞或被跳过）时，下游任务变为 `blocked`（`task.blocked`）。

<a id="attempt"></a>
## 一次尝试

每次尝试是一个新的 ACP 子进程和会话（`App::attempt`）：

1. 打开会话，接入角色配置的 MCP 服务器；Agent 声明支持 HTTP MCP 时，还接入共享看板。
2. 按角色设置模型、推理强度和权限模式等会话控制项，见 [ACP 集成](../interfaces/acp_integration.md#session_setup)。
3. 发送任务提示（`task_prompt`）：总目标、工作区共享资料（有时，与任务说明冲突时以任务说明为准）、任务说明、验收标准（要求协作者结束前用自己判断的方式逐条验证，在 `checks` 中给出证据；标准不合理时在 `question` 中请主控调整）、上游结果（含主控的修订）、上次失败原因、Skill 列表和结构化结果格式。角色指令和 Provider 的自定义提示词按 [用户的长期要求](../interfaces/acp_integration.md#standing_instructions) 注入，只有没有系统层入口的 Agent 才附在这里。
4. 收集流式输出、工具调用、审批请求和表单请求。
5. 解析回复中最后一个 ```` ```json ```` 代码块作为结构化结果。

<a id="structured_result"></a>
### 结构化结果

```json
{
  "summary": "一句话结果",
  "checks": [{ "criterion": "验收标准原文", "passed": true, "evidence": "怎么验证的、看到了什么" }],
  "evidence": ["其他可核对的证据"],
  "risks": ["已知风险，没有就留空数组"],
  "question": null
}
```

- `checks` 按验收标准的顺序逐条给出，验证方式由 Agent 自己决定。
- 没有 JSON 块时，摘要取回复第一行，并记一条风险「Agent 没有按约定返回结构化结果」。
- `question` 不为空表示 Agent 在提问：`{"text": "问题", "options": ["A", "B"], "recommend": "A"}`。回答作为下一轮提示发回同一个会话。同一次尝试最多连续提问 4 轮。提问由谁回答见[提问的流转](goal_mode.md#question_routing)。

<a id="verification"></a>
## 验证

结果提交后，Janus 核对结果（`verify_result` / `result_gaps`，`verification.*` 事件）。Janus 不运行任何验证命令，验证由协作者按验收标准自己完成，见[验收标准](../interfaces/board_mcp.md#acceptance_criteria)。核对的内容：

- `summary` 不能为空。
- 每条验收标准都有对应的 check（按顺序），check 有证据，且没有 `passed: false`。

验证没通过时，同一次尝试中按以下顺序处理：

1. **在原会话里补一次**：把没核对、没证据或没达到的标准逐条发回同一个会话（`session.prompt`，`channel: "fix"`），请协作者修复后重新验证、补上证据；确实无法达到的如实写明原因。协作者保留着上下文，比开新会话从头做快，也更容易修对。补完后重新提交结果、重新核对。
2. **仍然没通过**：任务失败（`task.failed`，`reason: "verification"`），不自动开新会话重试，由主控决定接受结果、修改后重试还是取消，见[看板 MCP 工具](../interfaces/board_mcp.md#accept_tool)。

用户在会话中直接以当前结果结束时，不走修复这一步。

<a id="retry"></a>
## 重试与失败

- 每个任务最多尝试 2 次（`MAX_ATTEMPTS`）。尝试失败（Agent 进程退出、超时、自行取消等，不含验证没通过）后还有剩余次数时，任务回到 `pending`，下一次尝试带上上次失败的原因（`task.retry_scheduled`）。
- 用完次数或验证没通过后任务变为 `failed`（`task.failed`），Run 不停下，由主控在看板上读到后用 `retry`、`accept`、`cancel` 处理，或自己做。被它阻塞的任务不需要重新编排。
- 只有主控能重试任务（看板的 `retry`），重试会重新给满尝试次数（`task.reopened`，`reason: "lead.retry"`）。依赖它的 `blocked` 任务在它成功后自动恢复。用户想重做某个任务时，给主控发消息。
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
| Run 预算 | 6 小时，可用 `extendBudget` 追加 |

<a id="watchdog"></a>
### 停滞检测

时间限制只在超时后才生效。为了更早发现卡住的协作者，调度循环每次唤醒（最多间隔 2 秒）都检查正在进行的尝试，发现以下两种情况时写入 `task.stalled` 事件：

| 情况 | 判断 | `data.reason` |
| --- | --- | --- |
| 没有动静 | 尝试的会话 10 分钟（`STALL_AFTER`，即 `Config.stall_after`）没有发出任何更新：文本、思考、工具调用和工具进度都算 | `idle` |
| 原地打转 | 连续 5 次（`LOOP_REPEAT`）完成同一个工具调用：类型、标题和输入摘要都相同 | `loop` |

规则：

- 等人的时间不算：尝试在等提问的回答、审批或表单（`Run::held` 返回的守卫存在期间），或会话被用户接管或打断（`Run::set_human`）时，计时从头开始。这些状态由尝试自己在内存中标记，停滞检测不读事件日志。
- 同一段没有动静的时间只报告一次。会话再发出任何更新后重新计时，并写入一条 `data.resolved` 为 `true` 的 `task.stalled`，应用据此撤下「可能卡住了」，主控不会读到它。原地打转每满 5 次报告一次。
- 主控会话按同样的规则检查（`LEAD_WATCH`），结果写 `lead.stalled`，`to` 为 `user`：主控之上没有别人会发现它卡住。应用在「需要你处理」中显示「主控可能卡住了」，再有动静时撤下。主控调用 Janus 看板工具不计入原地打转，反复 `wait` 是正常的。
- 停滞检测只报告，不打断、不重试也不取消。主控在 `wait` 中读到 `[停滞]`，自己判断继续等、`reply`、`retry` 还是 `cancel`，见[目标模式与共享看板](goal_mode.md#failures_in_goal_mode)。
- 计时状态只在内存中（`Run.watch`），重启后从头开始；`task.stalled` 不影响恢复。

<a id="delivery_conditions"></a>
## 交付条件

调度循环每次唤醒都重新判断结果。「没有可执行任务」指没有任务在执行，也没有任务满足启动条件。

| 条件 | 结果 |
| --- | --- |
| 没有可执行任务，且主控会话异常结束 | Run 失败（`run.failed`），可以从中断处继续 |
| 没有可执行任务，且主控目标已结束 | 交付（`run.delivered`），摘要取主控最后一条回复 |
| 超出预算，且还有未结束的任务 | 等待用户追加预算（`run.budget_exhausted`） |
| 用户暂停，且没有任务在执行 | 暂停 |
| 其他 | 继续运行 |

任务失败不会让 Run 停下来等用户。主控结束时还有协作者在等主控回答的，提问转给用户，见[目标模式与共享看板](goal_mode.md#question_routing)。

<a id="run_statuses"></a>
## Run 状态

| 状态 | 含义 |
| --- | --- |
| `planning` | 刚创建，主控会话正在启动 |
| `running` | 运行中；暂停时状态也保持 `running` |
| `waiting_input` | 超出预算，等待用户追加预算或取消 |
| `delivered` | 已交付，等待验收 |
| `accepted` | 用户核对后验收 |
| `cancelled` | 用户取消 |
| `failed` | Run 失败，例如主控会话异常结束 |
| `interrupted` | 编排器重启时 Run 还未结束 |


<a id="user_actions"></a>
## 用户操作

用户通过 `POST /api/runs/{id}/actions` 操作 Run，参数见 [HTTP API](../interfaces/http_api.md#run_actions)：

- **回答与审批**：`answer`、`approve`、`input`。
- **介入会话**：`message`、`interrupt`、`takeover`、`release`、`finish`。会话正在运行时，消息通过 ACP 实时插入（支持 `_lody/session/steer` 时，见[插话](../interfaces/acp_integration.md#steering)）或排队到这一轮结束；会话已结束时，消息保留到这个 Agent 下一次开始工作。每条消息发出时立即记录，之后再记录它的去向和送达时刻，见[消息的流转](../interfaces/events.md#message_flow)。
- **控制 Run**：`pause`、`resume`、`extendBudget`、`cancel`（界面上叫「停止」）、`accept`、`resumeRun`。

任务的重试、接受和取消由主控通过看板处理，用户不直接操作任务；用户可以给主控发消息提出要求。

<a id="recovery"></a>
## 中断与恢复

编排器启动时 `recover`：

- 仍在进行的尝试标记为 `interrupted`。
- 未结束的 Run 写入 `run.interrupted` 事件，状态改为 `interrupted`。

用户「从中断处继续」（`resumeRun`），或直接给某个 Agent 发消息时，`resume_run` 重新申请租约，从 `runs.config` 重建团队，从事件日志重建任务：

- 从所有 `orchestration.accepted` 事件重建看板任务。已成功的任务保留结果，失败和阻塞的任务保持原状态，未完成的任务重新开始；重启前仍在进行的尝试，Agent 支持 `session/load` 时重新打开原会话。
- 快照中的 MCP 定义按当前规则重新校验（`check_mcp_names`）：名称规范化后是看板保留名 `janus`，或同一成员有两个同名服务器时，恢复被拒绝并说明原因，Run 保持原来的 `interrupted` 或 `failed` 状态，不会启动任何会话。
- 排队但还没送达的消息保留下来。
- 主控以「从中断处继续」的说明重新启动，说明中列出看板上之前的任务。

`run.failed` 之后同样可以从中断处继续。已交付的 Run 在重启后仍可验收（`accept`），不需要恢复。

<a id="early_plan_runs"></a>
## 早期版本的 Run

早期版本的 Run 采用「主控先出计划、用户确认、再按 DAG 执行」的流程，带评审返工、集成任务和独立 worktree（见 [ADR：先出计划、用户确认、再按 DAG 执行](../decisions/plan_first_flow.md)）。这套流程和它在应用中的显示都已删除：

- 它们的事件仍保存在数据库中，应用可以打开这些 Run，查看目标、主控和协作者的会话、交付结果和审计日志，但不再显示计划、评审和任务卡片。
- 它们不能「从中断处继续」，`resumeRun` 会提示用同样的目标新建 Run。判断依据是 `run.created` 事件的 `data.mode` 不是 `goal`。
