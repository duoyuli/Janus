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
| `_meta.lody.steering` | 存在时，用户消息通过 `_lody/session/steer` 插入正在进行的一轮 |

<a id="methods"></a>
## 使用的方法

| 方向 | 方法 | 用途 |
| --- | --- | --- |
| Janus → Agent | `initialize` | 握手 |
| Janus → Agent | `session/new` | 打开会话，带 `cwd`、`mcpServers`、`_meta` |
| Janus → Agent | `session/load` | 恢复中断前的会话；历史回放的更新被丢弃，期间 Agent 的请求一律拒绝 |
| Janus → Agent | `session/set_config_option` | 设置模型、推理强度和其他会话控制项 |
| Janus → Agent | `session/set_mode` | 为通过 session modes 声明的控制项设置模式 |
| Janus → Agent | `session/prompt` | 发送一轮提示：一个文字块；Agent 声明了 `promptCapabilities.image` 时，后面附上文字点名的图片块，见[图片附件](../domains/run_lifecycle.md#attachments) |
| Janus → Agent | `session/cancel`（通知） | 打断当前一轮 |
| Janus → Agent | `_lody/session/steer` | 把用户消息插入正在进行的一轮，内容块与 `session/prompt` 相同；返回 `outcome: "injected"` 表示成功；15 秒超时 |
| Agent → Janus | `session/update`（通知） | 流式输出（`agent_message_chunk`）、工具调用（`tool_call`、`tool_call_update`）、会话信息（`session_info_update`） |
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

<a id="goal_status"></a>
## Codex 的目标状态

Codex 在 `session_info_update` 的 `_meta.codex.goal` 中报告 `/goal` 的状态（例如 `active`、`paused`、`complete`）。Janus 把状态变化记为 `goal.status` 事件；状态为 `active` 时，即使 `session/prompt` 已经返回，也继续跟随会话的输出。

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
