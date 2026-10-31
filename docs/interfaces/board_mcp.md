# 看板 MCP 工具

本文是共享看板 MCP 服务的接口参考：端点、方法、主控工具、协作者工具、编排校验规则和验证命令允许列表。看板在协作中的作用和提问的流转见[目标模式与共享看板](../domains/goal_mode.md)。

<a id="transport"></a>
## 端点与传输

```text
POST http://<JANUS_ADDR>/mcp/{run}/{token}/{who}
```

| 路径参数 | 说明 |
| --- | --- |
| `run` | Run id。Run 不在内存中运行时返回 403 |
| `token` | 主控启动时为这个 Run 生成的随机令牌。为空或不匹配时返回 403 |
| `who` | `lead` 表示主控；否则是协作者正在执行的任务 id |

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

所有工具都带 MCP 工具标注：`team`、`wait`、`board_post` 标为只读，`orchestrate`、`reply` 标为非破坏性操作。标注的作用是避免 Codex 把调用看板当作需要用户审批的操作。

工具调用的错误以 `isError: true` 的文本内容返回，不使用 JSON-RPC 错误。

<a id="lead_tools"></a>
## 主控工具

`who` 为 `lead` 时，`tools/list` 返回以下四个工具。

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
      "write": true,
      "accept": ["新增测试通过"],
      "commands": ["cargo test"]
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
| `write` | boolean | 否 | 任务是否修改文件，默认 `false` |
| `accept` | string[] | 否 | 可核对的验收标准 |
| `commands` | string[] | 否 | 完成后由 Janus 执行的验证命令 |

成功时返回已写入看板的任务；失败时返回全部错误，一个任务都不启动。

<a id="orchestration_validation"></a>
#### 校验规则

`validate_orchestration` 一次检查全部问题：

- `tasks` 必须是非空数组。
- `id` 格式合法，且不与看板上已有的任务或同批任务重复。
- `title` 和 `instructions` 不能为空。
- `assignee` 必须是协作者。
- 依赖必须存在，不能依赖自己，同批任务之间不能成环。
- 每条验证命令都必须通过[验证命令](#verification_commands)检查。

通过校验的任务按固定策略执行：`kind` 为 `implement`（`write: true`）或 `explore`；最多尝试 `MAX_ATTEMPTS`（2）次；需要结构化结果。

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
| `[完成]` | `task.succeeded` | 摘要、证据、风险、产物路径 |
| `[尝试失败]` | `attempt.failed` | 失败原因，还会重试 |
| `[失败]` | `task.failed` | 用完尝试次数，需要主控决定 |
| `[无法开始]` | `task.blocked` | 依赖失败 |
| `[进展]` `[阻塞]` `[备注]` | `board.posted` | 协作者写回的内容 |
| `[提问]` | `question.asked` | 问题和选项，需要用 `reply` 回答 |
| `[等用户]` | `approval.requested`、`input.requested` | 协作者在等用户审批或填写 |

只有带任务 id、且任务在看板上的事件才会返回。

### `reply`

```json
{ "task": "T2", "text": "用 alpha 作为名字" }
```

| 情况 | 行为 |
| --- | --- |
| 任务有等待中的提问 | 作为回答发回协作者的会话（`question.answered`，`via: "lead"`） |
| 任务正在执行，没有提问 | 以「主控回复：」开头插入会话 |
| 任务没有在执行 | 消息保留到该协作者下一次开始工作 |
| 任务不存在或 `text` 为空 | 返回错误 |

<a id="member_tools"></a>
## 协作者工具

`who` 为任务 id 时，`tools/list` 只返回 `board_post`。

### `board_post`

```json
{ "kind": "blocker", "text": "缺少测试数据，先用样例继续" }
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `kind` | 否 | `progress`（默认）、`blocker` 或 `note` |
| `text` | 是 | 写到看板的内容 |

写入 `board.posted` 事件后立即返回，不等待回答。需要主控做决定才能继续时，协作者应在结构化结果的 `question` 字段提问并结束本轮。

<a id="verification_commands"></a>
## 验证命令

验证命令由 `verification_argv` 检查，编排校验和执行前的检查共用同一套规则。允许的命令：

- 只读命令 `pwd`、`cat`、`rg`、`sed`（不带 `-i` / `--in-place`）、`nl`、`find`（不带 `-delete`、`-exec`、`-execdir`、`-ok`、`-okdir`、`-fprint`、`-fprintf`、`-fls`）、`sort`（不带 `-o` / `--output`）。只读命令之间可以用 `|` 连接，整条管道通过 `sh -c` 执行。
- `git diff`、`git status`、`git log`、`git show`。
- `cargo test`、`cargo check`、带 `--check` 的 `cargo fmt`。
- `npm test`、`pnpm test`。
- `python3 -m …`、`python -m …`。

语法限制：

- 引号外不允许 `;`、`&`、`>`、`<`、`$`、反引号、`(`、`)`、`\` 和换行。
- 单引号内的字符不受限制；双引号内仍不允许 `$` 和反引号。
- 引号必须闭合。
- 命令前面的 `KEY=VALUE` 环境变量赋值会被忽略。

修改这套规则时，同步更新 `orchestrate` 工具描述中 `commands` 的说明。
