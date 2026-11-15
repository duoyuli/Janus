# 事件参考

本文列出事件的结构和全部事件类型。事件是 Run 的唯一事实来源：应用、看板、恢复和导出都从事件读取。为什么这样设计见 [ADR：事件是唯一事实来源](../decisions/events_as_source_of_truth.md)。

<a id="event_shape"></a>
## 结构

```json
{
  "run_id": "rmvvdi046",
  "seq": 42,
  "ts": 1793350000000,
  "type": "question.asked",
  "actor": "implementer",
  "to": "lead",
  "task": "T2",
  "attempt": 1,
  "text": "给笔记定个名字",
  "data": { "key": "q-T2-1-0", "question": "...", "options": [], "blocking": true }
}
```

| 字段 | 说明 |
| --- | --- |
| `run_id` | Run id |
| `seq` | Run 内从 1 开始递增的序号 |
| `ts` | 毫秒时间戳 |
| `type` | 事件类型，SQLite 中的列名也是 `type` |
| `actor` | 发出者：`user`、`janus`、`lead` 或成员 id |
| `to` | 可选，接收者 |
| `task`、`attempt` | 可选，关联的任务和尝试序号 |
| `text` | 给人看的一句话，中文 |
| `data` | 可选，结构化数据，字段随类型变化 |

约束：

- 事件只追加，不修改。写入 SQLite 后才广播。同一个 Run 的分配序号、写入和广播在一把锁内完成（`Run.emitting`），事件流按 `seq` 顺序送出，应用把序号缺口视为漏收。
- `text` 用于展示，程序逻辑只读 `type` 和 `data`。
- 等待用户或主控处理的事件，在 `data.key` 中带一个唯一键；回答、撤回和转交都用这个键关联。键的前缀：`q-<task>-<n>-<轮>` 是任务提问，`q-lead-` 是主控用 `ask_user` 提的问题，`req-<task 或 agent>-<n>-<时间>` 是审批或表单请求。

新增或修改事件类型时，同步检查：应用的 `derive` 和 `boardFacts`（`frontend/Sources/Janus/Core/Derive.swift`，见 [macOS 应用](../architecture/frontend.md#event_derivation)）、看板的 `board_entry`（[看板 MCP 工具](board_mcp.md#wait)）、恢复逻辑 `resume_run`。改 `session.usage` 的字段时还要同步用量账本的两份实现（[用量口径](#usage_accounting)）：`backend/src/usage.rs` 和 `Core/UsageLedger.swift`，并在 `backend/testdata/usage_cases.json` 加用例。

<a id="message_flow"></a>
## 消息的流转

用户在会话页发的消息、主控通过看板 `reply` 发给协作者的补充，都按同一流程记录：

1. **发出**：立即写 `user.message`（`delivery: "pending"`），带唯一的 `data.id`。`to` 是目标 Agent，从任务会话发出时带 `task`。
2. **路由**：会话处理到这条消息时写 `message.routed`，`data.delivery` 说明它的去向：

   | `delivery` | 含义 | 何时出现 |
   | --- | --- | --- |
   | `steer` | 已插入正在进行的这一轮 | Agent 支持 `_session/steering` 且插入成功，见[插话](acp_integration.md#steering) |
   | `queued` | 排队，这一轮结束后作为新一轮发送 | 用户的消息，Agent 不支持插话或插入失败；Agent 正在等用户处理请求；用户接管着会话时主控发来的消息（交还托管时送达） |
   | `interrupt` | 打断正在进行的这一轮，立即作为新一轮发送 | 主控的 `reply`，Agent 不支持插话或插入失败 |
   | `prompt` | 直接作为新的一轮发送 | 人工接管中一轮已结束、正在等消息时；或在等待提问回答时发送 |
   | `next` | 留言，下次会话开始时带给它 | 这个 Agent 当前没有会话 |

3. **送达**：`queued`、`interrupt` 和 `next` 的消息真正进入会话时写 `message.delivered`，`data.ids` 列出送达的消息。`steer` 的消息在 `_session/steering` 返回 `injected` 时写 `message.delivered`，`data.applied` 为 `true`，表示消息已经进入模型的输入。用户接管会话期间，排队的消息不会自动发出，交还托管时随交还提示一起送达。主控跟随进行中的目标时，插不进去的消息立即作为新的一轮发送，写 `message.delivered`，不等目标结束。

Janus 自己发给会话的两种消息也走插话，但不记 `user.message`：看板推送记为 `session.prompt`（`channel: "board"`），见[看板推送](../domains/goal_mode.md#board_push)；冲突提醒随 `task.conflict` 发出，见[同时改同一个文件](../domains/goal_mode.md#conflicts)。

重启后，没有 `steer` / `prompt` 也没有被送达的消息会恢复为留言，在 Agent 下一次会话时带入。

<a id="event_types"></a>
## 事件类型

### Run

| 类型 | 说明 |
| --- | --- |
| `run.created` | 创建；`data` 含 `goal`、`cwd`、`git`、`baseRef`、`workspace`（工作目录的一句话描述）、`mode`（固定为 `goal`）、`images`（随目标粘贴的图片，见[图片附件](../domains/run_lifecycle.md#attachments)） |
| `run.delivered` | 交付；`data` 是交付结果：`summary`、`briefSuggestions`（主控建议写入工作区共享资料的条目，见[工作区共享资料](../domains/agents_and_roles.md#workspace_brief)）、`deliverables`、`evidence`（每个任务的全部证据，`text` 以任务 id 开头）、`acceptance`（每个任务的验收记录：验收总数、通过数、证据方法计数、`checks`、独立验证、`traceGaps` 和 `risks`，见[证据方法](#evidence_methods)）、`usage`（一行用量摘要，和 Run 页、看板读同一份账本，见[用量口径](#usage_accounting)）、`risks` |
| `risk.accepted` | 用户验收 |
| `run.failed` | Run 失败；`data.error` |
| `run.cancelled` | 用户取消 |
| `run.interrupted` | 编排器重启时 Run 未结束 |
| `run.recovered` | 从中断处继续 |
| `run.paused` / `run.resumed` | 暂停 / 继续 |
| `run.budget_exhausted` / `run.budget_extended` | 预算用完 / 追加预算 |

### 主控与目标

| 类型 | 说明 |
| --- | --- |
| `goal.status` | Agent 报告的 `/goal` 状态变化，见 [Codex 的目标状态](acp_integration.md#goal_status)；主控向 Janus 确认完成后由 Janus 结束目标时，`actor` 为 `janus`，`data.by` 为 `janus`，见[目标不自己结束时](../domains/goal_mode.md#goal_settle) |
| `lead.failed` | 主控会话异常结束；`data.error` |
| `lead.stalled` | 停滞检测发现主控会话没有进展，`to` 为 `user`。`data.reason` 是 `idle`（`data.idleMs`）或 `loop`（`data.tool`、`data.repeats`）；主控再有动静时写一条 `data.resolved: true` 的 `lead.stalled`，见[停滞检测](../domains/run_lifecycle.md#watchdog) |
| `orchestration.accepted` | 编排通过校验；`data.tasks` 是写入看板的任务（含 `owns`、`verifies`，验证任务的 `deps` 已包含被验证的任务），恢复时据此重建看板 |
| `orchestration.rejected` | 编排没有通过校验；`data.errors`、`data.submitted` |
| `board.posted` | 协作者写回看板；`data.kind`（`progress` / `blocker` / `note`）、`data.text` |

### 任务与尝试

| 类型 | 说明 |
| --- | --- |
| `task.assigned` | 任务分配给执行者 |
| `task.ready` | 依赖满足，可以启动 |
| `attempt.launch_requested` / `attempt.started` | 请求启动 / 尝试开始 |
| `context.handoff` | 上游结果交给下游；`data.amendments` 是主控对上游结论的修订，`data.files` 是上游改过的文件，`data.stale` 是上游完成后又被别的任务改过的文件（`[{path, by}]`），见[工作目录里的痕迹](../domains/goal_mode.md#traces) |
| `result.amended` | 主控修订已完成任务的结论（`janus.amend`）；`data.text` |
| `result.submitted` | 尝试提交结构化结果；`data.summary`、`data.checks`（逐条验收：`criterion`、`passed`、`evidence`，可选的 `method` 见[证据方法](#evidence_methods)）、`data.evidence`、`data.risks` |
| `artifact.created` | 保存产物；`data.name`、`data.kind`、`data.path`、`data.hash`、`data.bytes` |
| `attempt.failed` | 尝试失败；`data.error` |
| `attempt.late_result` | 过期尝试的结果被忽略 |
| `task.retry_scheduled` | 安排下一次尝试 |
| `task.succeeded` / `task.failed` | 任务成功 / 失败。`task.failed` 的 `data.reason` 是 `verification`（验证没通过，不自动重试）或 `attempts`（用完尝试次数）。`task.succeeded` 另带 `data.checks`（逐条验收及其 `method`）和 `data.acceptance`（验收总数、通过数、证据方法计数、提醒），恢复时据此重建结果；主控用 `accept` 接受结果时，`actor` 为 `lead`，`data.acceptedBy` 为 `lead`，另有 `data.reason`、`data.risks`，`data.checks` 来自被接受的那次 `result.submitted`。协作者完成的 `task.succeeded` 带 `data.traceGaps`：结果和工作目录对不上的地方，没有时为空数组，见[工作目录里的痕迹](../domains/goal_mode.md#traces) |
| `task.blocked` | 硬依赖失败；`data.dependency`。一个依赖处理好后仍有别的依赖失败时再写一条，指向那个依赖 |
| `task.stalled` | 停滞检测发现尝试没有进展，`to` 为 `lead`。`data.reason` 是 `idle`（`data.idleMs`：多久没有动静）或 `loop`（`data.tool`：重复的调用，`data.repeats`：连续次数）。没有动静的尝试再次有更新时，会再写一条 `data.resolved: true` 的 `task.stalled`，见 [Run 生命周期](../domains/run_lifecycle.md#watchdog) |
| `task.conflict` | 两个同时执行的任务改了同一批文件；`task` 是发现冲突的一方，`to` 为 `lead`，`data.other` 是另一个任务，`data.paths` 是相对工作目录的文件。`data.owned` 为 `true` 时，是 `task` 改了 `data.other` 负责的文件（越界）。见[同时改同一个文件](../domains/goal_mode.md#conflicts) |
| `task.claimed` | 协作者用 `claim` 认领文件；`actor` 是执行者，`to` 为 `lead`，`data.paths` 是新认领的路径，恢复时并入任务的 `owns`，见[团队结构](../domains/goal_mode.md#team_structure) |
| `task.skipped_by_decision` | 主控用 `cancel` 取消（`actor` 为 `lead`，`data.reason`，连带取消的有 `data.dependency`） |
| `task.reopened` | 任务重新开始。`data.reason` 是 `lead.retry`（主控重试，`actor` 为 `lead`，可能带新的 `data.instructions`、`data.accept`）或 `upstream.retry`（因上游被取消而连带取消的任务，随上游的重试恢复，`actor` 为 `janus`，`data.upstream` 是上游任务） |

### 验证

| 类型 | 说明 |
| --- | --- |
| `verification.started` | 开始核对结果；`data.accept` |
| `verification.passed` / `verification.failed` | 核对结果；`data.checks`、`data.acceptance`（验收总数、通过数、证据方法计数和提醒，见[证据方法](#evidence_methods)），失败时另有 `data.failures`（每条没核对、没证据、没达到或方法不合法的标准） |

<a id="evidence_methods"></a>
### 证据方法

每条 `check` 可以带可选的 `method`，说明这条验收结论的依据是什么：

| `method` | 含义 |
| --- | --- |
| `test` | 运行自动化测试得到的结果 |
| `command` | 运行命令、脚本或检查工具得到的结果 |
| `inspection` | 检查代码、文件、界面或实际状态得到的结果 |
| `reasoning` | 根据推理或自我判断得到的结果 |
| `unverified` | 证据缺失、无法复现或暂时无法核实 |

- 方法只说明依据的类型，不代表结论一定真实：Agent 仍可能把推理写成 `test`，关键结果由主控自己核对。
- 没有 `method` 是旧结果和省略该字段的结果，显示为「未说明」，不当作 `reasoning`，也不因此判失败。
- `method` 存在时必须是上表中的值。其他值原样记录在 `result.submitted` 和 `verification.failed` 的 `data.checks` 中，`evidence::gaps` 报出「不是允许值」并让这次核对失败，不会静默换成合法方法。
- `reasoning`、`unverified` 和没有方法只产生提醒（`data.acceptance.reminders`），不阻断任务。
- `data.acceptance` 的结构（`verification.*`、`task.succeeded` 和 `run.delivered` 的 `acceptance` 条目）：`criteria`（验收标准总数）、`passed`（通过数）、`methods`（`method`、`label`、`count` 的数组，按上表顺序）、`unspecified`（没有方法的条数）、`invalid`（不合法的值）、`reminders`（要主控注意的提醒）。交付中的条目另有 `task`、`title`、`status`、`label`、`checks`、`verifications`、`traceGaps`、`risks`。

### 会话

| 类型 | 说明 |
| --- | --- |
| `agent.status` | Agent 阶段变化；会话开始时 `data` 含 `session`、`config`（实际生效的设置） |
| `agent.message` | Agent 的回复文本；`data.text`。一段文字在下一次工具调用、这一轮结束，或 1 秒没有新内容（`MESSAGE_QUIET`）时写入，所以 Agent 说完要做什么、接着长时间写文件时，应用也能先看到它说的话。每段保留原样的空白和换行（可能在代码块中间切开），按顺序直接拼接就是原文；主控的交付摘要取它最后连续的几段拼起来 |
| `session.prompt` | Janus 发给 Agent 的上下文或提示；`data.channel`（`system` / `context` / `goal` / `followup` / `instructions` / `fix` / `board` / `goal_check`，`goal_check` 是问主控目标是否已经完成，见[目标不自己结束时](../domains/goal_mode.md#goal_settle)，`fix` 是把验证失败发回协作者的会话修复，`board` 是插入主控会话的看板推送）、`data.prompt`、`data.session`；`instructions` 另有 `data.via`（`system` / `developer`） |
| `tool.call` | 工具调用及其输入输出摘要，`data.callId`，文件改动在 `data.diffs`（`data.output`，没有输出时为空；早期记录在这里写的是 ACP 状态，应用不显示），状态在 `data.status`；改了文件的调用（`edit`、`delete`、`move`）另有 `data.paths`，是相对工作目录的文件 |
| `tool.started` | Janus MCP 工具开始执行；`data.callId`、`data.tool`、`data.input`（摘要）、`data.args`（完整参数）、`data.status`。Agent 自己的工具调用开始时也记一条（`data.live` 为 `true`，没有 `args`），完成后由同一 `callId` 的 `tool.call` 替换 |
| `agent.thought` | Agent 的思考；`data.text`，见[会话中能看到什么](acp_integration.md#session_view) |
| `agent.plan` | Agent 的计划；`data.entries`（`content`、`status`、`priority`） |
| `session.controls` | 会话提供的选项和当前值：`data.model`、`data.effort`（`id`、`current`、`options`）、`data.controls`（其他控制项）；会话打开时和选项变化时各记一条 |
| `session.control` | 一个控制项切换了：`data.id`、`data.value`、`data.via`（`mode` / `config`）；用户切换的 `data.by` 为 `user`，失败的 `data.failed` 为 `true`。见[会话中切换控制项](acp_integration.md#session_controls) |
| `session.commands` | Agent 提供的斜杠命令；`data.commands`（`name`、`description`、`hint`） |
| `session.usage` | 会话用量，有两种形态：上下文快照（`data.used`、`data.size`、可选的 `data.cost`，来自 ACP 的 `usage_update`）和一轮的 token（`data.inputTokens`、`data.outputTokens`、`data.totalTokens`、`data.cachedReadTokens`、`data.cachedWriteTokens`、`data.thoughtTokens`，以及 Agent 按模型上报时的 `data.models`，每项带 `model` 和同样的计数，来自 `session/prompt` 的返回，见 `acp_view::prompt_usage`）。缺的字段写 `null`，不写 0。语义和汇总口径见[用量口径](#usage_accounting) |
| `tool.completed` | Janus MCP 工具返回；`data.callId`、`data.tool`、`data.input`、`data.args`、`data.output`、`data.status` |
| `user.message` | 用户或主控发出一条消息，发出时立即记录；`data.id`、`data.text`，`data.delivery` 为 `pending`，见[消息的流转](#message_flow)。带图片时 `data.images` 列出图片，`data.text` 末尾有点名图片的 `[附图 N] <路径>` 行，应用显示缩略图并隐藏这些行 |
| `message.routed` | 会话决定这条消息怎么进入；`data.id`、`data.delivery`（`steer` / `queued` / `interrupt` / `prompt` / `next`） |
| `message.delivered` | 等待中的消息送达：排队的消息在一轮结束后作为新一轮发送，留言在下次会话开始时带入；`data.ids`。插入的消息被 Agent 读到时另有 `data.applied: true` |
| `session.interrupted` | 用户打断了一轮 |
| `control.taken` / `control.released` | 接管 / 交还 |

### 需要处理的请求

| 类型 | 说明 |
| --- | --- |
| `question.asked` | 任务提问；`data.key`、`data.question`、`data.options` |
| `question.escalated` | 交给用户的决定：协作者的提问（带任务），或主控用 `ask_user` 提的问题（`actor` 为 `lead`，没有任务，另有 `data.question`、`data.options`、`data.recommend`）；`data.key`、`data.reason` |
| `question.answered` | 提问被回答；`data.answer`、`data.via`（`lead`、`session`；用户在请求卡片上回答时省略） |
| `answer.delivered` | 回答已发回会话 |
| `approval.requested` | Agent 请求审批；`data.key`、`data.options` |
| `approval.granted` / `approval.denied` | 审批结果 |
| `input.requested` | Agent 请求填写表单或打开链接；`data.key`、`data.mode`、`data.schema` |
| `input.answered` | 表单已处理 |
| `decision.withdrawn` | 请求所属的尝试或会话已结束，请求被撤回 |

<a id="usage_accounting"></a>
## 用量口径

Run 的用量账本是从事件实时算出的只读汇总：后端是 `ledger`（`backend/src/usage.rs`），应用是 `usageLedger`（`frontend/Sources/Janus/Core/UsageLedger.swift`），两边同一口径，并用同一份用例 `backend/testdata/usage_cases.json` 测试，结果不一致时两边的测试都会失败。账本不读专门的表，也不改变调度、预算、任务状态机、事件写入或交付条件。全部输入是 `session.usage`，再加上 `session.controls` / `session.control` 给出的模型。

### 两种用量事件

| 形态 | 来源 | 字段 | 性质 |
| --- | --- | --- | --- |
| 上下文快照 | ACP `usage_update` | `used`、`size`、可选 `cost` | 某个时刻的上下文窗口，不是消耗；`cost` 是会话的累计费用 |
| 一轮的 token | `session/prompt` 的返回 | `inputTokens`、`outputTokens`、`totalTokens`、`cachedReadTokens`、`cachedWriteTokens`、`thoughtTokens`，以及按模型的 `models` | 这一轮提示自己的用量 |

- 一轮的 token 是这一轮的，不是到这一轮为止的累计。ACP v2 的 `Usage` 写明是「the completed work」；Claude Code 适配器在每轮开始时清零（`accumulatedUsage`），Codex 适配器取这一轮结束时的会话总量减去开始时的总量（`PromptTokenUsage`）。因此同一会话的每一轮都相加，不按数字是否上涨去猜口径：对话越长，每轮带的历史越多，单轮的数字本来就会上涨。
- `models` 来自返回值的 `_meta.quota.model_usage`，Claude Code 和 Codex 适配器用同一种形状发送。Claude Code 的 `usage` 只算主循环，`model_usage` 还算上子 Agent 和压缩等内部调用（Claude Agent SDK 注明计费以它为准），所以有 `models` 时这一轮按它计，每行带模型名；没有时按 `usage` 计，模型取这个成员当时的模型（`session.controls` 的 `model.current`，或之后切换成功的 `session.control`），也没有时写「未知模型」。
- `inputTokens` 不含缓存，`totalTokens` 是输入、缓存读取、缓存写入和输出之和；`thoughtTokens` 是输出的一部分。某一行没有 `totalTokens` 但有输入或输出时，按各部分之和计。缺的字段写 `null`，不写 0。
- 上下文快照每次更新都原样记录，不去重。`used` / `size` 只取最近值、峰值和比例，`used` 小于等于 0 的快照忽略（与会话卡片一致），永远不进 token 总量。

### 费用

- 账本的费用只取 Agent 上报的 `cost`，原值加币种，不做任何换算，也不按 token 估算。
- `cost` 是会话的累计费用（Claude Agent SDK 的 `total_cost_usd`：每次带「到目前为止」的总数，应读最新值）。每个会话按币种取最新一次读数；读数比上一次小说明累计重新开始（例如会话中途 `/clear`），之前达到的数额保留并加上之后的读数；一次正数之后读到 0 是出错的结果，忽略。
- 同一币种的会话合计成一个数；不同币种分别列出并标「币种或口径不同，未合并」；没有币种的每条单列并标「未标注币种」。
- 账单另有按 models.dev 价格的估算（[ADR：按 models.dev 的价格估算用量](../decisions/model_prices.md)），它和上报的费用分开显示，不写进账本或事件。

### 归集与范围

事件已带 `actor`、`task`、`attempt`，每个数字都能追溯到对应的 `session.usage`。一个 ACP 会话（或主控的一次会话）算一条记录；`attempt.started` 是 Janus 在启动尝试时写的，那个会话属于 `to` 指向的 Agent（`actor` 是 janus），其余事件的 `actor` 就是会话主人。打开的会话即使一条用量都不上报，也算一个「未上报」的会话。

| 范围 | 内容 |
| --- | --- |
| Run | 主控和协作者的全部会话，以及墙钟时长 |
| 主控 / 协作者小计 | Run 总量包含主控，两者另外单列 |
| 任务 | 这个任务所有尝试的会话；没有会话时是「未开始」，不是「未上报」。任务账单再按成员和模型分开（`task_bill`） |
| 成员 | 一个 Agent 在这个 Run 里的全部会话，含它做过的多个任务 |
| 尝试 | 一次尝试的会话、结束状态和时长 |
| 会话 | 这个会话自己的最近用量，会话卡片显示为「本会话」「上一轮」 |

跨 Run 按时间段的账单见 [HTTP API](http_api.md#usage_bill)。

### 完整性

`完整`、`部分上报`、`未上报` 三种，按该范围里的会话计数：没有任何会话上报 token 是「未上报」，部分会话上报、部分没有是「部分上报」，全部上报是「完整」。缺失的数字位置写文字，不写 0；上报了 0 就写 0。范围里还没有任何会话（任务没开始）写「未开始」，和「未上报」分开。

### 时间

时长是墙钟时长：Run 从 `run.created` 到最后一条事件，任务从它的第一条事件到最后一条，尝试从 `attempt.started` 到结束事件。`question.asked`、`approval.requested`、`input.requested` 到对应的回答、处理或撤回之间的时间单独记为等待时间（`waitMs`），界面写成「其中等待你」，不冒充 Agent 执行时间。没有会话的范围没有时长，写「没有时长记录」，不写 0。时间统计不替代 Run 预算，也不改变预算状态。

### 取消、过期和失败

- `run.cancelled` 取消整个 Run：取消时刻仍在跑的尝试全部记「已取消」，备注「Run 被取消」，结束时间取取消时刻或更晚的结束时间。
- `task.skipped_by_decision` 只取消一个任务：只有这个任务仍在跑的尝试记「已取消」，备注「主控取消了这个任务」；同一个 Run 里其他任务的尝试不受影响。
- 两种取消都不覆盖取消前已经结束的尝试：失败、验证没通过、过期保持原状态。已经上报的用量照常计入。Janus 不为取消写尝试级事件，杀连接后可能还有一条失败事件，也不算真的失败。
- `attempt.late_result` 的尝试记「已过期但仍计入」；`verification.failed`、`task.failed` 的尝试带结束原因。
- 失败的 Run 保留已经发生的用量；隐藏它会低估成本。

### 回放与恢复

账本是这段事件的纯函数：全量读取、重启后重新读事件、历史 Run 和回放位置（前 N 条事件）都得到同一组数字。Run 页、`team`、`task_detail`、交付摘要和 `GET /api/runs/{id}/usage` 读的是同一份结果，见 [HTTP API](http_api.md#run_usage)。

### 提醒

默认阈值：Run 累计 500,000 token；单个任务占 Run 的 60% 且 Run 不少于 50,000；5 个以上协作者 30 分钟没有任何任务成功；Run 有 100,000 token 却没有任何费用上报。前后端各有一份默认值，修改时同步 `backend/src/usage.rs` 和 `Core/UsageLedger.swift`。提醒只写文字，完整性不是「完整」时前缀「提醒（按已上报数据）」；它不进收件箱，也不暂停、取消、重试或追加预算。

账本不需要新增事件类型：模型、子 Agent 的用量和费用都来自 Agent 已经发来的字段。
