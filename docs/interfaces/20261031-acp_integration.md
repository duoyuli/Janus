# ACP 集成

本文说明 Janus 作为 ACP 客户端，怎样与适配器子进程通信：传输方式、使用的方法、`_meta` 扩展、会话设置和用户请求的回复格式。Agent 类型和权限模式的业务规则见 [Agent、角色与工作区](../domains/20261031-agents_and_roles.md)。

## 导航

- [传输](#transport)：修改适配器进程的启动和结束时阅读。
- [initialize](#initialize)：修改能力协商时阅读。
- [使用的方法](#methods)：查 Janus 调用了哪些 ACP 方法时阅读。
- [`_meta` 扩展](#session_meta)：修改传给适配器的扩展参数时阅读。
- [系统层：Janus 的说明和用户的长期要求](#standing_instructions)：修改系统层注入的内容时阅读。
- [会话设置](#session_setup)：修改会话打开后的设置时阅读。
- [会话中能看到什么](#session_view)：修改会话更新怎样变成事件时阅读。
- [会话中切换控制项](#session_controls)：修改中途切换模型或模式时阅读。
- [目标状态](#goal_status)：修改 `/goal` 的状态跟随时阅读。
- [插话](#steering)：修改插话时阅读。
- [Claude Code 会话的环境默认值](#background_tasks)：修改 Claude Code 的环境变量时阅读。
- [用户请求的回复](#user_request_replies)：修改审批、表单的回复时阅读。

<a id="transport"></a>
## 传输

- JSON-RPC 2.0 over stdio，每行一个 JSON 对象。帧、请求 id、响应路由和取消由官方 Rust SDK [`agent-client-protocol`](https://github.com/agentclientprotocol/rust-sdk) 处理（稳定 v1）；`backend/src/acp.rs` 负责启动子进程、`initialize`，并把 SDK 收到的通知和请求交给编排器。
- 每个会话一个子进程：环境变量只来自 Janus 构造的 `RuntimeSpec.env`，不继承 Janus 自己的环境；工作目录是 Run 的工作目录，stderr 写入 `~/.janus/logs/runs/<run>/`（对话是 `logs/chats/<run>/`）下的日志文件。适配器本身怎样安装和选版本见 [ACP 适配器管理](../architecture/20261031-adapters.md)。
- 子进程在自己的进程组中运行，结束会话时整组终止：先向进程组发 `SIGTERM`，5 秒内没退出再向整组发 `SIGKILL`，适配器启动的 Agent 进程不会留下来；之后最多再等 3 秒，卡在内核里退不出的进程不会让关闭会话一直挂着（`AcpConn::kill`）。
- 单个请求默认 30 分钟超时；进程退出时，所有未完成的请求以「Agent 进程已退出」失败，会话随后收到结束信号。

<a id="initialize"></a>
## initialize

Janus 声明的客户端能力：

```json
{
  "fs": { "readTextFile": false, "writeTextFile": false },
  "terminal": false,
  "elicitation": { "form": {}, "url": {} },
  "_meta": { "jetbrains": { "air": {} } }
}
```

Janus 不代 Agent 读写文件或运行终端，Agent 用自己的工具直接操作工作目录。`elicitation` 表示 Janus 可以处理表单和链接两种表单请求。声明 AIR 客户端后，官方适配器会提供目标状态（`_meta.jetbrains.air.goal`）。

Janus 读取的 Agent 能力：

| 字段 | 用途 |
| --- | --- |
| `agentInfo.name`、`agentInfo.version` | 配置检查中显示 |
| `mcpCapabilities.http` | 为 `true` 时，目标模式下的协作者会话接入看板 MCP 服务 |
| `loadSession` | 为 `true` 时，重启后恢复的任务尝试用 `session/load` 重新打开原会话 |
| `_meta.steering.supported` | 为 `true` 时，用户的消息、负责人的回复、看板推送和冲突提醒通过 `_session/steering` 插入正在进行的一轮，见[插话](#steering) |
| `_meta.jetbrains.air.goal.controlMethod` | 清除目标时调用的方法，官方适配器为 `_session/goal` |

<a id="methods"></a>
## 使用的方法

| 方向 | 方法 | 用途 |
| --- | --- | --- |
| Janus → Agent | `initialize` | 握手 |
| Janus → Agent | `session/new` | 打开会话，带 `cwd`、`mcpServers`、`_meta` |
| Janus → Agent | `session/load` | 恢复中断前的会话；历史回放的更新被丢弃，期间 Agent 的请求一律拒绝 |
| Janus → Agent | `session/set_config_option` | 设置模型、推理强度和其他会话控制项；会话进行中用户也可以切换，见[会话中切换控制项](#session_controls) |
| Janus → Agent | `session/set_mode` | 为通过 session modes 声明的控制项设置模式 |
| Janus → Agent | `session/prompt` | 发送一轮提示：一个文字块；Agent 声明了 `promptCapabilities.image` 时，后面附上文字点名的图片块，见[图片附件](../domains/20261031-run_lifecycle.md#attachments)；文字里以 `@路径` 提到的工作区文件，每个附一个 `resource_link` 块（`file://` 绝对路径，`name` 是相对路径），ACP 要求所有 Agent 都支持这种块 |
| Janus → Agent | `session/cancel`（通知） | 打断当前一轮 |
| Janus → Agent | `_session/steering` | 把消息插入正在进行的一轮，内容块与 `session/prompt` 相同，带 `_meta.steering.idleBehavior: "promptRequired"`；返回 `outcome: "injected"` 表示成功；15 秒超时 |
| Janus → Agent | `_session/goal` | 清除负责人的目标（`action: "clear"`），见 [目标状态](#goal_status) |
| Agent → Janus | `session/update`（通知） | 流式输出（`agent_message_chunk`）、思考（`agent_thought_chunk`）、工具调用（`tool_call`、`tool_call_update`）、计划（`plan`）、斜杠命令（`available_commands_update`）、模式和选项的变化（`current_mode_update`、`config_option_update`）、用量（`usage_update`）、会话信息（`session_info_update`），见[会话中能看到什么](#session_view) |
| Agent → Janus | `session/request_permission` | 审批请求，转给用户 |
| Agent → Janus | `elicitation/create` | 表单或链接请求，转给用户 |

<a id="session_meta"></a>
## `_meta` 扩展

`session/new` 和 `session/load` 用同一组 `mcpServers` 和 `_meta`（`App::opening`），恢复的会话因此保留它的 Skill、MCP 和系统层。`mcpServers` 只有 Janus 自己的两个 HTTP 端点：资源库里有 MCP 服务器时是这个 Run 的 MCP 路由 `janus-tools`，然后是看板 `janus`；资源库的服务器不逐个放进去，它们的值也不交给 Agent，见[资源库](../domains/20261116-resource_library.md#delivery)。`_meta` 按运行时不同。

Claude Code：

```json
{
  "claudeCode": {
    "options": {
      "settings": { "permissions": { "allow": ["mcp__janus"] } },
      "plugins": [{ "type": "local", "path": "<成员的资源包>" }]
    }
  },
  "systemPrompt": { "append": "<Janus 的说明和用户的长期要求>" }
}
```

- `claudeCode.options` 原样交给官方 Claude Code 适配器，适配器把它展开进 SDK 的选项。`settings.permissions.allow` 让 Janus 自己的看板工具（只读写这个 Run 的看板）不用审批；没有它时，Claude Code 在默认模式下每次协作者写进展都会请用户批准（真实运行中发现）。Codex 通过工具的只读标注得到同样的效果。角色选择的权限模式对其他所有工具照常生效。
- `plugins`：资源库里有 Skill 时才有。资源包作为本地插件加载，只在这个会话中有效，不写进配置目录；插件里只有一个 Skill `janus-skills`，在会话中显示为 `janus:janus-skills`，见[资源库](../domains/20261116-resource_library.md#delivery)。
- `systemPrompt.append`：负责人会话放负责人说明和用户的长期要求，协作者会话放协作约定和用户的长期要求（见下一节）。只追加，不替换 Claude Code 自带的系统提示。

Codex：

```json
{ "additionalRoots": ["<成员的资源包>"] }
```

- 资源库里有 Skill 时才有。适配器调用 Codex 的 `skills/extraRoots/set`，读取资源包中的 `.agents/skills`，其中只有 `janus-skills`，显示为 `$janus-skills`；这个目录同时加入会话的文件访问范围。Codex 的系统层在进程环境变量 `CODEX_CONFIG` 中，不在 `_meta` 里，见下一节。

<a id="standing_instructions"></a>
## 系统层：Janus 的说明和用户的长期要求

一个会话的系统层（`system_layer`）依次由下面几部分组成，在整个会话中都有效，优先级高于任务说明：

1. **Janus 的说明**：负责人会话是负责人说明（`goal_brief`；交付后的对话新开会话时还是它，后面接 `conversation_brief`：目标、上次交付和最近的对话），协作者会话是协作约定（`collaborator_rules`），见[目标模式](../domains/20261031-goal_mode.md#collaborator_rules)。对话没有这一段，新开会话时换成最近的对话（`chat_brief`）。
2. **用户的长期要求**：Provider 的「自定义提示词」和角色的「指令」，`standing_instructions` 把两者合成一段（Provider 在前，角色在后）。

系统层不写 MCP 工具和 Skill 的用法：看板、值班、团队记忆和资源库的工具由 Agent 从 `mcpServers` 自己加载，用法和限制只在各工具的说明里；负责人说明和协作者约定各有一节「协作工具」，一个工具一行，只说它是做什么的，让 Agent 知道有这个工具、用到时去看它的说明；资源库的 Skill 经 `janus-skills` 由 Agent 按自己的 Skill 机制加载，见 [ADR：Janus 只注入协作规则](../decisions/20261125-collaboration_only_injection.md)。

Janus 编写的说明、段落标签、工具说明和看板消息按 Run 的语言（中文或英文）给出，见[注入的语言](../domains/20261031-goal_mode.md#prompt_language)。数字员工指令、Provider 自定义提示词、共享资料、用户留言和历史对话保留原文。

`Runtime::instruction_channel` 按运行时选择注入方式（`InstructionChannel`，`backend/src/agents/runtime.rs`）：

| Agent | 通道 | 做法 |
| --- | --- | --- |
| Claude Code | 系统提示 | `session/new`、`session/load` 的 `_meta.systemPrompt.append` |
| Codex | 开发者指令 | 适配器启动时读取环境变量 `CODEX_CONFIG`（JSON），合并进 `thread/start` 和 `thread/resume` 的配置。Janus 每个会话启动一个适配器进程，把 `developer_instructions` 写进这个进程的 `CODEX_CONFIG`，与网关配置合并；成员权限模式对应的线程审批和沙箱设置也一起写进去（`member_spec`，见[会话控制项与权限模式](../domains/20261031-agents_and_roles.md#permission_modes)） |

Janus 把注入的内容记成 `session.prompt` 事件，都带 `via`（`system` 或 `developer`），会话页可以展开查看。Agent Team 的会话记两条：协作指令（`channel` 为 `system`，负责人说明或协作约定，会话页标为「团队协作指令」）和用户的长期要求（`channel` 为 `instructions`，会话页标为「数字员工的指令」）。对话只记数字员工的指令；新开会话接着之前的对话时，最近的对话另记一条（`channel` 为 `history`，标为「之前的对话」）。

<a id="reload_system_layer"></a>
### 恢复会话时的系统层

`session/load` 恢复的会话已经有自己的系统层，Janus 原样交回它打开时的那一份，不重新拼：`recorded_system` 从事件日志中取这个会话最近一次记下的各段（带 `via` 的 `session.prompt`），按原顺序拼回。系统层一变，适配器就会把它当作新的指令再交给模型（codex-acp 在 `thread/resume` 时把不同的 `developer_instructions` 当作修改系统提示词），所以恢复时只要有一个字不同，Agent 就会收到第二份指令。恢复成功时 Janus 不记新的 `session.prompt`，会话页不会出现第二份注入。

负责人的目标会话在编排器重启后从中断处继续时同样这样加载（`lead_goal` 和对话共用 `open_conversation`）。只有新开会话才用新拼的系统层：对话和交付后负责人的对话在这里带上之前的对话（`chat_brief`、`conversation_brief`），重启后新开的负责人会话带上看板状况和最近的对话（`resume_brief`）。Codex 的系统层在适配器启动时就定了，所以 `lead_conversation` 先用原会话的系统层启动适配器去加载；加载失败要新开、而新系统层和原来不同时，关掉这个适配器，用新的系统层重新启动（`open_conversation`）。Claude Code 的系统层随 `session/new` 传入，用同一个适配器新开即可。任务的尝试恢复会话（从中断处继续、`continues`、回到原会话返工）时，成员和协作约定都没变，系统层和原来相同，同样不再记录。

Skill、MCP 和权限模式本身不属于系统层，它们按各 Agent 自己的机制生效，系统层只指出 Skill 目录在哪里，见[资源库](../domains/20261116-resource_library.md#delivery)。

<a id="session_setup"></a>
## 会话设置

打开会话后，`configure_session` 按以下顺序应用角色的选择：

1. 模型和推理强度：角色指定的值与当前值不同时，调用 `session/set_config_option`。Agent 不提供角色指定的模型时，会话以错误结束，提示重新选择模型；不提供指定的推理强度时，沿用当前值。改变模型后重新读取选项，因为可选的推理强度可能随模型变化。
2. 会话控制项：对成员 `session` 中的每个值（角色的选择，加上角色没有选择时 Codex 的 Full access 或 Claude Code 的 Bypass permissions，见[会话控制项与权限模式](../domains/20261031-agents_and_roles.md#permission_modes)），按控制项的来源调用 `session/set_config_option`（`configId` + `value`）或 `session/set_mode`（`modeId`）。
3. Plan 模式：开启着的 Plan 控制项设为关闭值（布尔为 `false`，Codex 的下拉为 `off`）；当前值为 `plan` 的下拉控制项改为第一个其他选项。规则见 [不使用 Plan 模式](../domains/20261031-agents_and_roles.md#no_plan_mode)。

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
| `usage_update`；`session/prompt` 返回的 `usage` 和 `_meta.quota.model_usage` | `session.usage` | 上下文快照（`used`、`size`、可选 `cost`）；一轮的 token（`inputTokens`、`outputTokens`、`totalTokens`、`cachedReadTokens`、`cachedWriteTokens`、`thoughtTokens`）和按模型的 `models` |

会话打开时，Janus 还把它提供的选项（模型、推理强度、其他下拉控制项，按 `agents::session_options` 整理，当前值是应用角色设置之后的值）记为一条 `session.controls`。

### 用量字段的口径

`session.usage` 的两种形态含义不同，Janus 原样记录，口径由用量账本统一处理：

- `usage_update`：`used` 是此刻在上下文里的 token，`size` 是上下文窗口大小，`cost` 带 `amount` 和 `currency`，是会话的累计费用，但不是所有适配器都发。快照按更新原样记录，不去重。
- `session/prompt` 返回的 `usage`：这一轮提示自己的 token。ACP v1 的 `Usage` 注释写「across session」，与 `PromptResponse` 的「Token usage for this turn」矛盾；v2 改为「the completed work」，Claude Code 和 Codex 适配器也都按每轮实现（Claude Code 在每轮开始清零，Codex 取这一轮前后的会话总量之差）。
- 返回值的 `_meta.quota.model_usage`：Claude Code 和 Codex 适配器按模型给出这一轮的计数（`token_count` 里缓存读取叫 `cachedInputTokens`，缓存写入叫 `cachedWriteTokens` 或 `cacheWriteInputTokens`，推理叫 `reasoningOutputTokens`）。Claude Code 的 `usage` 只算主循环，`model_usage` 还算子 Agent 和压缩，所以 Janus 把它按 ACP 的字段名记为 `models`，账本优先用它（`acp_view::prompt_usage`）。
- 缺的字段写 `null`，不写 0。

每轮相加、按模型归集、费用和提醒的规则见[用量口径](20261031-events.md#usage_accounting)。会话卡片的「本会话 上下文」「上一轮 N tokens」读的是这个会话最后一条 `session.usage`，和 Run 累计值分开，见 [macOS 应用](../architecture/20261031-frontend.md#usage_ledger)。

Janus 仍然声明不提供文件读写和终端（`clientCapabilities.fs`、`terminal` 都是 `false`）：Agent 直接在工作目录里读写、运行命令，输出随工具调用回来。

<a id="session_controls"></a>
## 会话中切换控制项

会话打开期间，用户可以在会话卡片中切换 Agent 提供的模型、推理强度和模式（`control` 操作，见 [Run 操作](20261031-http_api.md#run_actions)）。Janus 把它交给这个会话（`Steer::Control`），无论 Agent 正在执行一轮、还是在人工接管中等待消息：来源是 session modes 的调用 `session/set_mode`，其他调用 `session/set_config_option`。成功后记一条 `session.control`（`data.by` 为 `user`）；返回了新的选项时，再记一条 `session.controls`。失败时 `session.control` 的 `data.failed` 为 `true`，会话照常继续。Plan 类的选项和值不在可选范围内，规则见 [不使用 Plan 模式](../domains/20261031-agents_and_roles.md#no_plan_mode)。恢复的历史会话沿用事件中最后一次 `session.controls`；重新打开的会话返回了选项时以新的为准。

<a id="goal_status"></a>
## 目标状态

官方 Codex 和 Claude Code 适配器在 `session_info_update` 中报告 `/goal` 的状态（例如 `active`、`paused`、`complete`），写在 `_meta.jetbrains.air.goal`，只在客户端声明了 AIR 时发送。Janus 把状态变化记为 `goal.status` 事件；状态为 `active` 时，即使 `session/prompt` 已经返回，也继续跟随会话的输出。负责人确认完成后，Janus 用 `_session/goal`（`action: "clear"`）清除目标。

<a id="steering"></a>
## 插话

Agent 声明了 `_meta.steering.supported` 时，会话正在进行的一轮可以插入消息（`steer_turn`）：

1. Janus 发出 `_session/steering`，内容是消息的文字和 `@路径` 引用块，并带 `_meta.steering.idleBehavior: "promptRequired"`。返回 `outcome: "injected"` 时记 `message.routed`（`delivery: "steer"`）和 `message.delivered`（`data.applied: true`）。
2. Agent 在下一个安全点把消息交给模型：Codex 立即读到；Claude Code 在当前工具调用结束后读到。这次插入已经成功时，不再另发通知。
3. Agent 拒绝（例如这一轮正在结束）或没有声明插话时，用户的消息排队到这一轮结束；负责人的回复打断这一轮后立即发送。

负责人跟随进行中的目标（`session/prompt` 已经返回、目标仍是 `active`）时没有可以等待的一轮：插不进去的消息立即作为新的一轮发送，不等目标结束。

`idleBehavior: "promptRequired"` 表示没有正在进行的一轮时不替客户端开始新一轮：Agent 返回 `promptRequired`，Janus 把消息留在队列里，按正常的 `session/prompt` 流程发送，完成和用量仍归那一轮。

<a id="background_tasks"></a>
## Claude Code 会话的环境默认值

Janus 为 Claude Code 设置 `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`：Janus 把返回的一轮当作 Agent 的回答，而 Claude Code 允许把命令或子 Agent 放到后台，这一轮会在工作完成之前返回。

Janus 还设置 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`：Claude Code 的 auto memory 按工作目录保存，而 Janus 里每个 Run 和对话都有自己的目录，下一个 Run 读不到；工作区的知识放在[团队记忆](../domains/20261123-team_memory.md#native_memory)里。

官方 Claude Code 适配器不提供 `dontAsk` 权限模式。选了这个模式的角色会以 Agent 当前的模式运行（实际生效的模式记在 `agent.status` 的 `data.config` 中），需要在角色设置中重新选择。

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
