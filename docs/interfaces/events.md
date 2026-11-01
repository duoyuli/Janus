# 事件参考

本文列出事件的结构和全部事件类型。事件是 Run 的唯一事实来源：页面、看板、恢复和导出都从事件读取。为什么这样设计见 [ADR：事件是唯一事实来源](../decisions/events_as_source_of_truth.md)。

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

- 事件只追加，不修改。写入 SQLite 后才广播。
- `text` 用于展示，程序逻辑只读 `type` 和 `data`。
- 等待用户或主控处理的事件，在 `data.key` 中带一个唯一键；回答、撤回和转交都用这个键关联。键的前缀：`q-<task>-<n>-<轮>` 是任务提问，`req-<task 或 agent>-<n>-<时间>` 是审批或表单请求。早期计划模式的 Run 还有 `plan` 和 `plan-invalid`。

新增或修改事件类型时，同步检查：页面的 `derive` 和 `boardFacts`（[Web 工作台](../architecture/frontend.md#event_derivation)）、macOS 应用 `Derive.swift` 中的同名函数（[macOS 应用](../architecture/macos_app.md#event_derivation)）、看板的 `board_entry`（[看板 MCP 工具](board_mcp.md#wait)）、恢复逻辑 `resume_run`。

<a id="message_flow"></a>
## 消息的流转

用户在会话页发的消息、主控通过看板 `reply` 发给协作者的补充，都按同一流程记录：

1. **发出**：立即写 `user.message`（`delivery: "pending"`），带唯一的 `data.id`。`to` 是目标 Agent，从任务会话发出时带 `task`。
2. **路由**：会话处理到这条消息时写 `message.routed`，`data.delivery` 说明它的去向：

   | `delivery` | 含义 | 何时出现 |
   | --- | --- | --- |
   | `steer` | 已插入正在进行的这一轮 | Agent 支持 `_lody/session/steer` 且插入成功 |
   | `queued` | 排队，这一轮结束后作为新一轮发送 | 用户的消息，Agent 不支持插话或插入失败；Agent 正在等用户处理请求；用户接管着会话时主控发来的消息（交还托管时送达） |
   | `interrupt` | 打断正在进行的这一轮，立即作为新一轮发送 | 主控的 `reply`，Agent 不支持插话或插入失败 |
   | `prompt` | 直接作为新的一轮发送 | 人工接管中一轮已结束、正在等消息时；或在等待提问回答时发送 |
   | `next` | 留言，下次会话开始时带给它 | 这个 Agent 当前没有会话 |

3. **送达**：`queued`、`interrupt` 和 `next` 的消息真正进入会话时写 `message.delivered`，`data.ids` 列出送达的消息。用户接管会话期间，排队的消息不会自动发出，交还托管时随交还提示一起送达。

重启后，没有 `steer` / `prompt` 也没有被送达的消息会恢复为留言，在 Agent 下一次会话时带入。早期 Run 的 `user.message` 没有 `id`，直接在 `delivery` 中记录去向。

<a id="event_types"></a>
## 事件类型

### Run

| 类型 | 说明 |
| --- | --- |
| `run.created` | 创建；`data` 含 `goal`、`cwd`、`git`、`baseRef`、`workspace`、`inPlace`、`mode` |
| `run.delivered` | 交付；`data` 是交付结果：`headline`（「已完成：<目标第一行>」或「部分交付：<目标第一行>」）、`summary`、`deliverables`、`evidence`、`risks`、`review`、`confidence`、`diagnosis` |
| `run.accepted_with_risk` | 用户接受部分结果后的交付 |
| `risk.accepted` | 用户验收 |
| `run.failed` | Run 失败；`data.error` |
| `run.cancelled` | 用户取消 |
| `run.interrupted` | 编排器重启时 Run 未结束 |
| `run.recovered` | 从中断处继续 |
| `run.paused` / `run.resumed` | 暂停 / 继续 |
| `run.budget_exhausted` / `run.budget_extended` | 预算用完 / 追加预算 |
| `run.waiting_input` | 等待用户决定（早期计划模式） |

### 主控与目标

| 类型 | 说明 |
| --- | --- |
| `goal.status` | Codex 报告的 `/goal` 状态变化 |
| `lead.failed` | 主控会话异常结束；`data.error` |
| `orchestration.accepted` | 编排通过校验；`data.tasks` 是写入看板的任务，恢复时据此重建看板 |
| `orchestration.rejected` | 编排没有通过校验；`data.errors`、`data.submitted` |
| `board.posted` | 协作者写回看板；`data.kind`（`progress` / `blocker` / `note`）、`data.text` |

### 计划（早期计划模式，只出现在旧 Run 中）

| 类型 | 说明 |
| --- | --- |
| `plan.proposed` | 计划提交给用户 |
| `plan.validation_failed` / `plan.validation_exhausted` | 计划校验失败 / 多次失败后停止 |
| `plan.revision_requested` / `plan.replan_requested` | 用户要求修改 / 重新规划 |
| `plan.approved` / `plan.frozen` | 用户确认 / 计划冻结 |

### 任务与尝试

| 类型 | 说明 |
| --- | --- |
| `task.assigned` | 任务分配给执行者 |
| `task.ready` | 依赖满足，可以启动 |
| `attempt.launch_requested` / `attempt.started` | 请求启动 / 尝试开始 |
| `context.handoff` | 上游结果交给下游；`data.amendments` 是主控对上游结论的修订 |
| `result.amended` | 主控修订已完成任务的结论（`janus.amend`）；`data.text` |
| `result.submitted` | 尝试提交结构化结果；`data.summary`、`data.checks`（逐条验收：`criterion`、`passed`、`evidence`）、`data.evidence`、`data.risks` |
| `artifact.created` | 保存产物；`data.name`、`data.kind`、`data.path`、`data.hash`、`data.bytes` |
| `attempt.failed` | 尝试失败；`data.error` |
| `attempt.late_result` | 过期尝试的结果被忽略 |
| `task.retry_scheduled` | 安排下一次尝试 |
| `task.succeeded` / `task.failed` | 任务成功 / 失败。`task.failed` 的 `data.reason` 是 `verification`（验证没通过，不自动重试）或 `attempts`（用完尝试次数）。主控用 `accept` 接受结果时，`task.succeeded` 的 `actor` 为 `lead`，`data.acceptedBy` 为 `lead`，另有 `data.reason`、`data.risks` |
| `task.blocked` | 硬依赖失败；`data.dependency` |
| `task.cancel_requested` / `task.cancelled_by_dependency` | 因 `failFast` 上游失败而取消（只出现在旧 Run 中） |
| `task.skipped_by_decision` | 跳过：用户接受部分结果时，或主控用 `cancel` 取消（`actor` 为 `lead`，`data.reason`，连带取消的有 `data.dependency`） |
| `task.reopened` | 重试后任务重新开始；`data.reason` 是 `user.retry` 或 `lead.retry`（旧 Run 中还有 `review.rejected`）；主控重试时可能带新的 `data.instructions`、`data.accept`（旧 Run 中还有 `data.commands`） |
| `review.approved` / `review.rejected` | 评审结论（只出现在旧 Run 中） |
| `policy.violation` | 只读任务修改了文件，改动被丢弃（只出现在使用独立 worktree 的旧 Run 中） |

### 验证

| 类型 | 说明 |
| --- | --- |
| `verification.started` | 开始核对结果；`data.accept`（旧 Run 中是 `data.policy`） |
| `verification.passed` / `verification.failed` | 核对结果；`data.checks`，失败时另有 `data.failures`（每条没核对、没证据或没达到的标准） |
| `verification.command` / `verification.command_rejected` / `verification.command_unavailable` | 只出现在旧 Run 中：Janus 曾经执行主控指定的验证命令 |

### 会话

| 类型 | 说明 |
| --- | --- |
| `agent.status` | Agent 阶段变化；会话开始时 `data` 含 `session`、`config`（实际生效的设置） |
| `agent.message` | Agent 的回复文本；`data.text`。一段文字在下一次工具调用、这一轮结束，或 3 秒没有新内容（`MESSAGE_QUIET`）时写入，所以 Agent 说完要做什么、接着长时间写文件时，页面也能先看到它说的话 |
| `session.prompt` | Janus 发给 Agent 的上下文或提示；`data.channel`（`system` / `context` / `goal` / `followup` / `instructions` / `fix`，`fix` 是把验证失败发回协作者的会话修复）、`data.prompt`、`data.session`；`instructions` 另有 `data.via`（`system` / `developer` / `extension`） |
| `tool.call` | 工具调用及其输入输出摘要 |
| `tool.started` | Janus MCP 工具开始执行；`data.callId`、`data.tool`、`data.input`（摘要）、`data.args`（完整参数）、`data.status` |
| `tool.completed` | Janus MCP 工具返回；`data.callId`、`data.tool`、`data.input`、`data.args`、`data.output`、`data.status` |
| `user.message` | 用户或主控发出一条消息，发出时立即记录；`data.id`、`data.text`，`data.delivery` 为 `pending`，见[消息的流转](#message_flow) |
| `message.routed` | 会话决定这条消息怎么进入；`data.id`、`data.delivery`（`steer` / `queued` / `interrupt` / `prompt` / `next`） |
| `message.delivered` | 等待中的消息送达：排队的消息在一轮结束后作为新一轮发送，留言在下次会话开始时带入；`data.ids` |
| `session.interrupted` | 用户打断了一轮 |
| `control.taken` / `control.released` | 接管 / 交还 |

### 需要处理的请求

| 类型 | 说明 |
| --- | --- |
| `question.asked` | 任务提问；`data.key`、`data.question`、`data.options` |
| `question.escalated` | 交给用户的决定：协作者的提问（带任务），或主控用 `ask_user` 提的问题（`actor` 为 `lead`，没有任务，另有 `data.question`、`data.options`、`data.recommend`）；`data.key`、`data.reason` |
| `question.answered` | 提问被回答；`data.answer`、`data.via`（`lead`、`session` 或页面回答时省略） |
| `answer.delivered` | 回答已发回会话 |
| `approval.requested` | Agent 请求审批；`data.key`、`data.options` |
| `approval.granted` / `approval.denied` | 审批结果 |
| `input.requested` | Agent 请求填写表单或打开链接；`data.key`、`data.mode`、`data.schema` |
| `input.answered` | 表单已处理 |
| `decision.withdrawn` | 请求所属的尝试或会话已结束，请求被撤回 |
