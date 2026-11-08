# 看板 MCP 工具

本文是共享看板 MCP 服务的接口参考：端点、方法、主控工具、协作者工具、编排校验规则和验收标准。看板在协作中的作用和提问的流转见[目标模式与共享看板](../domains/goal_mode.md)。

<a id="transport"></a>
## 端点与传输

```text
POST http://<JANUS_ADDR>/mcp/{run}/{token}/{who}
```

| 路径参数 | 说明 |
| --- | --- |
| `run` | Run id。Run 不在内存中运行时返回 403 |
| `token` | 这个会话的令牌：由主控启动时为 Run 生成的随机密钥和 `who` 一起做 SHA-256 得到（`session_token`）。为空或与 `who` 不匹配时返回 403 |
| `who` | `lead` 表示主控；否则是协作者正在执行的任务 id |

- 令牌和 `who` 绑定：协作者把 URL 中的 `who` 改成 `lead` 或其他任务 id 都会得到 403，所以协作者拿不到主控工具，也不能以其他任务的身份写看板。Run 的密钥只在内存中，服务重启后恢复的会话拿到新的 URL。
- MCP streamable HTTP 传输，响应直接在对应的 POST 中以 JSON 返回，不使用 SSE。
- `GET` 返回 405，服务不主动推送消息。
- 没有 `id` 的消息（通知）返回 202，无响应体。
- 支持的方法：`initialize`、`ping`、`tools/list`、`tools/call`；其他方法返回 JSON-RPC 错误 `-32601`。
- `initialize` 回显客户端请求的 `protocolVersion`（缺省 `2025-06-18`），服务名 `janus`。

会话中的 MCP 服务器条目：

```json
{ "type": "http", "name": "janus", "url": "http://127.0.0.1:7878/mcp/<run>/<token>/<who>", "headers": [] }
```

URL 的主机和端口取 `JANUS_ADDR`。

<a id="tool_annotations"></a>
## 工具标注

所有工具都带 MCP 工具标注：`team`、`wait`、`task_detail`、`board_post`、`peers` 标为只读，`orchestrate`、`reply`、`retry`、`accept`、`amend`、`cancel`、`ask_user` 标为非破坏性操作。标注的作用是避免 Codex 把调用看板当作需要用户审批的操作。

工具调用的错误以 `isError: true` 的文本内容返回，不使用 JSON-RPC 错误。

<a id="lead_tools"></a>
## 主控工具

`who` 为 `lead` 时，`tools/list` 返回以下十个工具。

### `team`

查看协作者（职责、Agent、模型、权限模式）和看板上所有任务的当前状态。无参数。

<a id="orchestrate"></a>
### `orchestrate`

提交一份编排，即一组交给协作者的任务。

```json
{
  "tasks": [
    {
      "id": "T1",
      "title": "实现解析器",
      "instructions": "背景……需要做……注意……",
      "assignee": "implementer",
      "deps": [],
      "accept": ["空输入返回明确的错误", "新增测试覆盖空输入并通过"]
    }
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `id` | string | 是 | 字母、数字、`-` 和 `_`，最长 32 个字符，在整个看板内唯一 |
| `title` | string | 是 | 任务标题 |
| `instructions` | string | 是 | 给执行者的完整说明。执行者看不到主控的对话 |
| `assignee` | string | 是 | 协作者 id，不能是主控 |
| `deps` | string[] | 否 | 必须先成功完成的任务 id，可以引用之前编排中的任务 |
| `accept` | string[] | 是 | 可核对的验收标准，至少一条，见[验收标准](#acceptance_criteria) |

成功时返回已写入看板的任务；失败时返回全部错误，一个任务都不启动。

<a id="orchestration_validation"></a>
#### 校验规则

`validate_orchestration` 一次检查全部问题：

- `tasks` 必须是非空数组。
- `id` 格式合法，且不与看板上已有的任务或同批任务重复。
- `title` 和 `instructions` 不能为空。
- `assignee` 必须是协作者。
- 依赖必须存在，不能依赖自己，同批任务之间不能成环。
- 每个任务至少有一条验收标准。

通过校验的任务按固定策略执行：最多尝试 `MAX_ATTEMPTS`（2）次，需要结构化结果。任务能否修改文件由执行者的 Agent 权限模式决定，编排中没有读写字段，见[权限交给 Agent 原生模式](../decisions/agent_native_permissions.md)。

<a id="wait"></a>
### `wait`

```json
{ "timeout_sec": 40 }
```

取回看板上的新消息。有新消息时立即返回；否则每 0.5 秒检查一次，最多等待 `timeout_sec` 秒（1 到 45，默认 40）。看板上没有未结束的任务时直接返回。

返回的消息：

| 标记 | 来源事件 | 内容 |
| --- | --- | --- |
| `[开始]` | `attempt.started` | 任务、执行者、第几次尝试 |
| `[完成]` | `task.succeeded` | 摘要、逐条验收（✓ / ✗、标准、证据）、证据、风险、产物路径。超过限额的部分删去，并提示用 `task_detail` 查看，见[完成消息的限额](#inline_budget) |
| `[尝试失败]` | `attempt.failed` | 失败原因，还会重试 |
| `[没通过验收]` | `task.failed`（`reason: "verification"`） | 协作者在原会话补过一次，仍有标准没核对、没证据或自认没达到，逐条列出；提示用 `accept`、`retry` 或 `cancel` 处理 |
| `[失败]` | `task.failed`（`reason: "attempts"`） | 用完尝试次数，需要主控决定 |
| `[无法开始]` | `task.blocked` | 依赖失败；处理好依赖后会自动继续 |
| `[停滞]` | `task.stalled` | 协作者很久没有动静，或连续重复同一个调用；提示继续等、`reply`、`retry` 或 `cancel` |
| `[用户已回答]` | `question.answered`（不是主控回答的） | 用户在会话或应用中直接回答了协作者的提问，主控不要再回答 |
| `[用户决定]` | `question.answered`（`ask_user` 的问题） | 用户对主控所提问题的回答 |
| `[进展]` `[阻塞]` `[备注]` | `board.posted` | 协作者写回的内容 |
| `[冲突]` | `task.conflict` | 两个同时执行的任务改了同一批文件，提示主控确认分工 |
| `[提问]` | `question.asked` | 问题和选项，需要用 `reply` 回答 |
| `[等用户]` | `approval.requested`、`input.requested` | 协作者在等用户审批或填写 |

除 `[用户决定]` 外，只有带任务 id、且任务在看板上的事件才会返回。

主控的 Agent 支持插话时，需要主控处理的消息还会直接推送进它的会话，推送过的消息算作已读，不再由 `wait` 返回，见[看板推送](../domains/goal_mode.md#board_push)。

<a id="inline_budget"></a>
#### 完成消息的限额

`[完成]` 消息让主控能决定下一步，完整结果留在 `task_detail` 中，长 Run 不会把主控的上下文占满：摘要最多 300 个字符，每条验收的标准 60 个、证据 160 个字符，证据和风险各列前 3 条（每条 160 个字符）并注明总数。有删节时末尾提示「完整结果用 janus.task_detail 查看」。交给下游任务的上游摘要同样最多 300 个字符。

### `reply`

```json
{ "task": "T2", "text": "用 alpha 作为名字" }
```

| 情况 | 行为 |
| --- | --- |
| 任务有等待中的提问 | 作为回答发回协作者的会话（`question.answered`，`via: "lead"`） |
| 任务正在执行，没有提问 | 以「主控回复：」开头立即送达：Agent 支持插话时插入当前这一轮，否则打断当前这一轮后作为新一轮发送；用户接管着这个会话时排队，交还托管时送达。见[消息的流转](events.md#message_flow) |
| 任务还没开始 | 消息保留到该协作者下一次开始工作 |
| 任务已经结束（完成、失败或已取消） | 返回错误，提示用 `retry` 或新建任务。这样消息不会被带进这个协作者之后的其他任务 |
| 任务不存在或 `text` 为空 | 返回错误 |

<a id="retry_tool"></a>
### `retry`

```json
{ "task": "T1", "instructions": "可选，新的完整说明", "accept": ["可选，新的验收标准"] }
```

重新执行失败、已取消或被主控取消的任务，重新给满尝试次数（`task.reopened`，`reason: "lead.retry"`，改过的说明和验收标准记在 `data.instructions`、`data.accept`，恢复时按它们重建任务）。`accept` 不能是空数组。被它阻塞的任务在它成功后自动继续。

<a id="accept_tool"></a>
### `accept`

```json
{ "task": "T1", "reason": "我打开页面核对过，移动端触控那一条不影响这次的目标" }
```

接受一个没通过验收核对的任务的结果：用它最后提交的结构化结果把任务标记为完成（`task.succeeded`，`actor` 为 `lead`，`data.acceptedBy: "lead"`），没达到的部分和 `reason` 记为风险。只对 `task.failed` 的 `reason` 为 `verification` 的任务有效。

<a id="amend_tool"></a>
### `amend`

```json
{ "task": "T4", "text": "改用经典 8 邻域规则，3D 只用于呈现" }
```

修订一个已完成任务的结论（`result.amended`）。主控推翻协作者的结论时用它，让下游以同一个结论为准：

- 正在执行的下游任务立即收到一条主控消息（与 `reply` 相同的送达方式）。
- 还没开始的下游任务在交接时拿到修订：任务说明的上游结果中附「主控修订了这个结论，以修订为准：…」，`context.handoff` 的 `data.amendments` 列出修订。
- 已经基于旧结论完成的下游任务列在返回结果中，由主控决定是否 `retry`。

只能修订已完成的任务；任务被重试后，之前的修订作废。

<a id="cancel_tool"></a>
### `cancel`

```json
{ "tasks": ["T3"], "reason": "改由主控自己完成" }
```

取消任务：还没开始的不再开始，执行中的会话被停止，依赖它们的任务一并取消（都记为 `task.skipped_by_decision`，`actor` 为 `lead`，连带取消的在 `data.dependency` 记录原因）。已完成的任务不受影响。取消后可以用 `retry` 恢复。

<a id="task_detail"></a>
### `task_detail`

```json
{ "task": "T2" }
```

按 id 返回一个任务的全部信息：执行者和状态、说明、验收标准、上游和下游任务（带状态）、完整结果（逐条验收、证据、风险）、主控的修订、最近一次没通过验收或尝试失败的原因、改过的文件、产物，以及看板上的进展、阻塞、提问、回答和冲突。主控可以顺着上下游的 id 继续查看相关任务。任务不存在时返回错误。

<a id="ask_user_tool"></a>
### `ask_user`

```json
{ "question": "部署到哪个环境？", "why": "需要用户的账号", "options": ["测试", "生产"], "recommend": "测试" }
```

把只有用户能做的决定交给用户：写 `question.escalated`（`actor` 为 `lead`，没有任务，`data.key` 以 `q-lead-` 开头），应用在「需要你处理」中显示。工具立即返回，不等待；用户回答后，`wait` 返回 `[用户决定]`。主控会话结束时，未回答的问题被撤回。`question` 和 `why` 必填。技术选型和实现方式不应使用这个工具，见[谁来做决定](../domains/goal_mode.md#decision_ownership)。

<a id="member_tools"></a>
## 协作者工具

`who` 为任务 id 时，`tools/list` 返回 `board_post` 和 `peers`。

### `board_post`

```json
{ "kind": "blocker", "text": "缺少测试数据，先用样例继续" }
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `kind` | 否 | `progress`（默认）、`blocker` 或 `note` |
| `text` | 是 | 写到看板的内容 |

写入 `board.posted` 事件后立即返回，不等待回答。需要主控做决定才能继续时，协作者应在结构化结果的 `question` 字段提问并结束本轮。

<a id="peers"></a>
### `peers`

```json
{ "task": "T1" }
```

不带 `task` 时返回看板上的其他任务：状态、和调用者的关系（你的上游、依赖你、和你改了同一批文件），正在执行的任务改过的文件，以及最近 5 条其他任务写回看板的内容。带 `task` 时返回那个任务的全部信息，与主控的 [`task_detail`](#task_detail) 相同，例如上游任务的完整证据和主控的修订。

<a id="acceptance_criteria"></a>
## 验收标准

每个任务都必须有验收标准（`accept`），它是执行者验证自己工作的依据。Janus 不替执行者运行任何验证命令，设计取舍见 [ADR：按验收标准自行验证](../decisions/criteria_over_commands.md)。

- **主控写标准**：写结果和行为，例如「点击格子能翻开并显示周围雷数」「新增测试覆盖空输入并通过」，而不是某条具体命令。需要特定的验证方式时，写进标准或说明，作为要求而不是唯一的检查。
- **执行者自己验证**：任务说明要求执行者结束前逐条验证，方式由它判断（运行测试、启动程序看实际表现、读代码推理、对比输出），在结构化结果的 `checks` 中按顺序逐条写明 `passed` 和 `evidence`。标准本身不合理或当前环境无法验证时，在 `question` 中请主控调整。
- **Janus 核对结构**：每条标准都有对应的 check、有证据、没有自认未达到的，才算通过，见[验证](../domains/run_lifecycle.md#verification)。
- **独立核对由主控负责**：执行者的证据是它自己的说法。主控说明要求主控自己核对关键结果，改了代码的目标在结束前安排独立评审。

修改这些规则时，同步更新 `orchestrate` 工具中 `accept` 的描述、任务说明（`task_prompt`）和主控说明（`goal_brief`）中的验收标准一节。
