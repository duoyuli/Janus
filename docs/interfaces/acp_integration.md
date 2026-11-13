# ACP 集成

本文说明 Janus 作为 ACP 客户端，怎样与适配器子进程通信：传输方式、使用的方法、`_meta` 扩展、会话设置和用户请求的回复格式。Agent 类型和权限模式的业务规则见 [Agent、角色与工作区](../domains/agents_and_roles.md)。

<a id="transport"></a>
## 传输

- JSON-RPC 2.0 over stdio，每行一个 JSON 对象（`backend/src/acp.rs`）。
- 每个会话一个子进程，工作目录是 Run 的工作目录，stderr 写入 `~/.janus/logs/<run>/` 下的日志文件。适配器本身怎样安装和选版本见 [ACP 适配器管理](../architecture/adapters.md)。
- 子进程在自己的进程组中运行，结束会话时整组终止。
- 单个请求默认 30 分钟超时；进程退出时，所有未完成的请求以「Agent 进程已退出」失败。

<a id="initialize"></a>
## initialize

Janus 声明的客户端能力：

```json
{
  "fs": { "readTextFile": false, "writeTextFile": false },
  "terminal": false,
  "elicitation": { "form": {}, "url": {} }
}
```

Janus 不代 Agent 读写文件或运行终端，Agent 用自己的工具直接操作工作目录。`elicitation` 表示 Janus 可以处理表单和链接两种表单请求。

Janus 读取的 Agent 能力：

| 字段 | 用途 |
| --- | --- |
| `agentInfo.name`、`agentInfo.version` | 配置检查中显示 |
| `mcpCapabilities.http` | 为 `true` 时，目标模式下的协作者会话接入看板 MCP 服务 |
| `loadSession` | 为 `true` 时，重启后恢复的任务尝试用 `session/load` 重新打开原会话 |
| `_meta.lody.steering` | 存在时，用户的消息、主控的回复、看板推送和冲突提醒通过 `_lody/session/steer` 插入正在进行的一轮，见[插话](#steering) |

<a id="methods"></a>
## 使用的方法

| 方向 | 方法 | 用途 |
| --- | --- | --- |
| Janus → Agent | `initialize` | 握手 |
| Janus → Agent | `session/new` | 打开会话，带 `cwd`、`mcpServers`、`_meta` |
| Janus → Agent | `session/load` | 恢复中断前的会话；历史回放的更新被丢弃，期间 Agent 的请求一律拒绝 |
| Janus → Agent | `session/set_config_option` | 设置模型、推理强度和其他会话控制项；会话进行中用户也可以切换，见[会话中切换控制项](#session_controls) |
| Janus → Agent | `session/set_mode` | 为通过 session modes 声明的控制项设置模式 |
| Janus → Agent | `session/prompt` | 发送一轮提示：一个文字块；Agent 声明了 `promptCapabilities.image` 时，后面附上文字点名的图片块，见[图片附件](../domains/run_lifecycle.md#attachments)；文字里以 `@路径` 提到的工作区文件，每个附一个 `resource_link` 块（`file://` 绝对路径，`name` 是相对路径），ACP 要求所有 Agent 都支持这种块 |
| Janus → Agent | `session/cancel`（通知） | 打断当前一轮 |
| Janus → Agent | `_lody/session/steer` | 把消息插入正在进行的一轮，内容块与 `session/prompt` 相同，`steerId` 是消息 id；返回 `outcome: "injected"` 表示成功；15 秒超时 |
| Agent → Janus | `_lody/session/steer_applied`（通知） | 插入的消息已经进入模型的输入，带 `steerId` |
| Agent → Janus | `session/update`（通知） | 流式输出（`agent_message_chunk`）、思考（`agent_thought_chunk`）、工具调用（`tool_call`、`tool_call_update`）、计划（`plan`）、斜杠命令（`available_commands_update`）、模式和选项的变化（`current_mode_update`、`config_option_update`）、用量（`usage_update`）、会话信息（`session_info_update`），见[会话中能看到什么](#session_view) |
| Agent → Janus | `session/request_permission` | 审批请求，转给用户 |
| Agent → Janus | `elicitation/create` | 表单或链接请求，转给用户 |

<a id="session_meta"></a>
## `_meta` 扩展

`session/new` 和 `session/load` 的 `_meta`（`session_meta`）：

```json
{
  "clientIdentifier": "janus-<成员 id>",
  "lody": {
    "sessionConfig": {
      "version": 1,
      "configOptionValues": { "<控制项 id>": "<值>" }
    }
  },
  "systemPrompt": { "append": "<主控说明和用户的长期要求>" }
}
```

- `lody.sessionConfig` 让只在会话开始时接受权限模式的 Agent（例如 Grok）按角色的选择打开会话。不读取它的 Agent 会忽略。
- `systemPrompt.append` 只在 Claude Code 的会话中设置：主控会话放主控说明和用户的长期要求，协作者会话只放长期要求（见下一节）。`session/load` 恢复会话时同样带上。只追加，不替换 Claude Code 自带的系统提示。

Claude Code 的会话另外带 `_meta.claudeCode.options.allowedTools: ["mcp__janus"]`：Janus 自己的看板工具（只读写这个 Run 的看板）不用审批。没有它时，Claude Code 在默认模式下每次协作者写进展都会请用户批准（真实运行中发现）。Codex 通过工具的只读标注得到同样的效果。角色选择的权限模式对其他所有工具照常生效。

<a id="standing_instructions"></a>
## 用户的长期要求

Provider 的「自定义提示词」和角色的「指令」是用户给这个成员定的长期要求，在整个会话中都有效，优先级高于主控写的任务说明。`standing_instructions` 把两者合成一段（Provider 在前，角色在后），`instruction_channel` 按 Agent 类型选择注入方式：

| Agent | 通道 | 做法 |
| --- | --- | --- |
| Claude Code | 系统提示 | `session/new`、`session/load` 的 `_meta.systemPrompt.append` |
| Codex | 开发者指令 | 适配器启动时读取环境变量 `CODEX_CONFIG`（JSON），合并进 `thread/start` 和 `thread/resume` 的配置。Janus 每个会话启动一个适配器进程，把 `developer_instructions` 写进这个进程的 `CODEX_CONFIG`，与网关配置合并（`with_developer_instructions`） |
| Pi | 系统提示分段 | Pi 的适配器没有系统提示入口，但会把启动参数 `-e <扩展>` 交给 Pi。Janus 在 `agents/<id>/janus/standing-instructions.js` 写入一个扩展，它在 `before_agent_start` 中把环境变量 `JANUS_STANDING_INSTRUCTIONS` 的内容加为系统提示的 `standing_instructions` 分段（`pi_standing_extension`、`with_standing`）。只加分段，不替换 Pi 自己的提示，适配器自己的扩展也能看到这个改动 |
| 其他（Grok、自定义命令） | 任务说明 | 没有系统提示入口，用 `<standing_instructions>` 块附在每次任务说明的末尾 |

走系统提示或开发者指令时，Janus 记一条 `session.prompt` 事件（`channel` 为 `instructions`，`via` 为 `system`、`developer` 或 `extension`），会话页可以展开查看。

Skill、MCP 和权限模式不属于长期要求，它们按各 Agent 自己的机制生效。

<a id="session_setup"></a>
## 会话设置

打开会话后，`configure_session` 按以下顺序应用角色的选择：

1. 模型和推理强度：角色指定的值与当前值不同时，调用 `session/set_config_option`。Agent 不提供角色指定的模型时，会话以错误结束，提示重新选择模型；不提供指定的推理强度时，沿用当前值。改变模型后重新读取选项，因为可选的推理强度可能随模型变化。
2. 会话控制项：对角色 `session` 中的每个值，按控制项的来源调用 `session/set_config_option`（`configId` + `value`）或 `session/set_mode`（`modeId`）。
3. Plan 模式：开启着的 Plan 控制项设为关闭值（布尔为 `false`，Codex 的下拉为 `off`）；当前值为 `plan` 的下拉控制项改为第一个其他选项。规则见 [不使用 Plan 模式](../domains/agents_and_roles.md#no_plan_mode)。

实际生效的设置记录在 `agent.status` 事件的 `data.config` 中。

<a id="session_view"></a>
## 会话中能看到什么

Janus 把会话的更新转成事件（`acp_view.rs`），应用照着显示，让人在 Janus 里看到的和直接用这个 coding agent 时一样：

| ACP 更新 | 事件 | 说明 |
| --- | --- | --- |
| `agent_message_chunk` | `agent.message` | 回复文字。连续的片段在 1 秒没有新内容、下一次工具调用或这一轮结束时写入一条，应用把同一个 Agent 连续的几条合成一个气泡 |
| `agent_thought_chunk` | `agent.thought` | 思考，和回复同时写入，应用中默认折叠 |
| `tool_call`（还没完成） | `tool.started` | 进行中的工具调用，`data.live` 为 `true`；完成后由同一 `callId` 的 `tool.call` 替换 |
| `tool_call_update`（完成或失败） | `tool.call` | 带 `data.callId`；内容里的文件改动（`diff`）记在 `data.diffs`（`path`、`old`、`new`，每一边最多约 12000 字节） |
| `plan` | `agent.plan` | 计划的全部条目（`content`、`status`、`priority`）；和上一份相同时不记录 |
| `available_commands_update` | `session.commands` | 斜杠命令（`name` 不带 `/`、`description`、`hint`）；和上一份相同时不记录 |
| `current_mode_update` | `session.control` | Agent 自己切换了模式 |
| `config_option_update` | `session.controls` | 选项变了，记录整份新的选项 |
| `usage_update`；`session/prompt` 返回的 `usage` | `session.usage` | 上下文用量（`used`、`size`）；一轮的 token 数（`inputTokens`、`outputTokens`、`totalTokens`） |

会话打开时，Janus 还把它提供的选项（模型、推理强度、其他下拉控制项，按 `agents::session_options` 整理，当前值是应用角色设置之后的值）记为一条 `session.controls`。

Janus 仍然声明不提供文件读写和终端（`clientCapabilities.fs`、`terminal` 都是 `false`）：Agent 直接在工作目录里读写、运行命令，输出随工具调用回来。

<a id="session_controls"></a>
## 会话中切换控制项

会话打开期间，用户可以在会话卡片中切换 Agent 提供的模型、推理强度和模式（`control` 操作，见 [Run 操作](http_api.md#run_actions)）。Janus 把它交给这个会话（`Steer::Control`），无论 Agent 正在执行一轮、还是在人工接管中等待消息：来源是 session modes 的调用 `session/set_mode`，其他调用 `session/set_config_option`。成功后记一条 `session.control`（`data.by` 为 `user`）；返回了新的选项时，再记一条 `session.controls`。失败时 `session.control` 的 `data.failed` 为 `true`，会话照常继续。Plan 类的选项和值不在可选范围内，规则见 [不使用 Plan 模式](../domains/agents_and_roles.md#no_plan_mode)。

<a id="goal_status"></a>
## Codex 的目标状态

Codex 在 `session_info_update` 中报告 `/goal` 的状态（例如 `active`、`paused`、`complete`）：npm 版适配器写在 `_meta.codex.goal`，仓库版适配器写在 `_meta.lody.goal`，两处 Janus 都读。Janus 把状态变化记为 `goal.status` 事件；状态为 `active` 时，即使 `session/prompt` 已经返回，也继续跟随会话的输出。

<a id="steering"></a>
## 插话

Agent 声明了 `_meta.lody.steering` 时，会话正在进行的一轮可以插入消息（`steer_turn`）：

1. Janus 发出 `_lody/session/steer`，`steerId` 是消息 id。返回 `outcome: "injected"` 时记 `message.routed`（`delivery: "steer"`）。
2. Agent 在下一个安全点把消息交给模型：Codex 立即读到；Claude Code 在当前工具调用结束后读到。读到时 Agent 发出 `_lody/session/steer_applied`，Janus 记 `message.delivered`（`data.applied: true`），见[消息的流转](events.md#message_flow)。
3. Agent 拒绝（例如这一轮正在结束）或没有声明插话时，用户的消息排队到这一轮结束；主控的回复打断这一轮后立即发送。

主控跟随进行中的目标（`session/prompt` 已经返回、目标仍是 `active`）时没有可以等待的一轮：插不进去的消息立即作为新的一轮发送，不等目标结束。

Codex 和 Claude Code 只有仓库版适配器支持插话，见[适配器清单](../architecture/adapters.md#catalog)。

<a id="background_tasks"></a>
## Claude Code 会话的环境默认值

Janus 为 Claude Code 及基于它的预设设置两个环境变量，保持 Janus 依赖的行为（`spec_for`）：

| 变量 | 值 | 原因 |
| --- | --- | --- |
| `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS` | `1` | Janus 把返回的一轮当作 Agent 的回答。仓库版适配器允许 Claude Code 把命令或子 Agent 放到后台，这一轮会在工作完成之前返回 |
| `CLAUDE_ACP_MODEL_DISCOVERY` | `off`，Agent 自己的环境变量设置了它时不覆盖 | 使用自定义接入点（`ANTHROPIC_BASE_URL`）时，仓库版适配器改为列出接入点自己的模型（`/v1/models`），不再提供 `opus`、`sonnet` 等别名；已保存的角色用的是别名，预设通过 `ANTHROPIC_DEFAULT_OPUS_MODEL` 等变量决定别名对应的模型 |

仓库版 Claude Code 适配器不再提供 `dontAsk` 权限模式。选了这个模式的角色会以 Agent 当前的模式运行（实际生效的模式记在 `agent.status` 的 `data.config` 中），需要在角色设置中重新选择。

<a id="user_request_replies"></a>
## 用户请求的回复

`session/request_permission` 和 `elicitation/create` 的处理（`user_request`）：

1. 记录 `approval.requested` 或 `input.requested` 事件，带上 Agent 给出的选项或 JSON Schema。
2. 等待用户在应用中处理（`approve` 或 `input` 操作）。
3. 按方法回复 Agent：

| 方法 | 用户选择 | 回复 |
| --- | --- | --- |
| `session/request_permission` | 选择某个选项 | `{"outcome": {"outcome": "selected", "optionId": "<id>"}}` |
| `session/request_permission` | 尝试结束、取消 | `{"outcome": {"outcome": "cancelled"}}` |
| `elicitation/create` | 提交、拒绝、取消 | `{"action": "accept", "content": {...}}`、`{"action": "decline"}`、`{"action": "cancel"}` |

尝试结束时，未处理的请求被撤回（`decision.withdrawn`），Agent 收到取消回复。
