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
| Janus → Agent | `session/prompt` | 发送一轮提示 |
| Janus → Agent | `session/cancel`（通知） | 打断当前一轮 |
| Janus → Agent | `_lody/session/steer` | 把用户消息插入正在进行的一轮，返回 `outcome: "injected"` 表示成功；15 秒超时 |
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
  "systemPrompt": { "append": "<主控说明>" }
}
```

- `lody.sessionConfig` 让只在会话开始时接受权限模式的 Agent（例如 Grok）按角色的选择打开会话。不读取它的 Agent 会忽略。
- `systemPrompt.append` 只在 Claude Code 主控的会话中设置，用于注入主控说明。

<a id="session_setup"></a>
## 会话设置

打开会话后，`configure_session` 按以下顺序应用角色的选择：

1. 模型和推理强度：角色指定的值与当前值不同时，调用 `session/set_config_option`。Agent 不提供角色指定的模型时，会话以错误结束，提示重新选择模型；不提供指定的推理强度时，沿用当前值。改变模型后重新读取选项，因为可选的推理强度可能随模型变化。
2. 会话控制项：对角色 `session` 中的每个值，按控制项的来源调用 `session/set_config_option`（`configId` + `value`）或 `session/set_mode`（`modeId`）。

实际生效的设置记录在 `agent.status` 事件的 `data.config` 中。

<a id="goal_status"></a>
## Codex 的目标状态

Codex 在 `session_info_update` 的 `_meta.codex.goal` 中报告 `/goal` 的状态（例如 `active`、`paused`、`complete`）。Janus 把状态变化记为 `goal.status` 事件；状态为 `active` 时，即使 `session/prompt` 已经返回，也继续跟随会话的输出。

<a id="user_request_replies"></a>
## 用户请求的回复

`session/request_permission` 和 `elicitation/create` 的处理（`user_request`）：

1. 记录 `approval.requested` 或 `input.requested` 事件，带上 Agent 给出的选项或 JSON Schema。
2. 等待用户在页面上处理（`approve` 或 `input` 操作）。
3. 按方法回复 Agent：

| 方法 | 用户选择 | 回复 |
| --- | --- | --- |
| `session/request_permission` | 选择某个选项 | `{"outcome": {"outcome": "selected", "optionId": "<id>"}}` |
| `session/request_permission` | 尝试结束、取消 | `{"outcome": {"outcome": "cancelled"}}` |
| `elicitation/create` | 提交、拒绝、取消 | `{"action": "accept", "content": {...}}`、`{"action": "decline"}`、`{"action": "cancel"}` |

尝试结束时，未处理的请求被撤回（`decision.withdrawn`），Agent 收到取消回复。
