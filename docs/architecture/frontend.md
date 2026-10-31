# Web 工作台

本文说明 `frontend/app.html` 的结构：页面如何从事件推导状态、怎样与后端通信，以及主要页面和请求卡片。后端接口见 [HTTP API](../interfaces/http_api.md)，事件含义见[事件参考](../interfaces/events.md)。

## 形态

- 单个 HTML 文件，内联 CSS 和 JavaScript，没有构建步骤和第三方依赖。
- release 构建把文件编进二进制，安装后的 Janus 不需要仓库。开发构建（`just dev`）在每次请求 `/` 时从磁盘读取文件，修改后刷新页面即可生效，不需要重启后端。
- 全局状态在对象 `S` 中。界面通过拼接 HTML 字符串渲染，事件处理通过 `data-act` 属性分派到 `ACT`（运行页）、`SETUP_ACT`（设置页）等表。

<a id="event_derivation"></a>
## 由事件推导状态

运行页不保存自己的业务状态。`derive(events)` 从头遍历事件，推导出任务、Agent 状态、待处理事项（`inbox`）、回答、产物、阶段等，再交给各个视图渲染。

这样做的约束：

- 新增或修改事件类型时，必须同步更新 `derive` 中对应的分支，否则页面不会反映新状态。
- 页面上的「决定」「交接」「待决」等看板条目由 `boardFacts` 从事件生成，同样依赖事件类型和 `data` 字段。
- 同一个待处理事项（按 `key`）在 `inbox` 中只保留一条。事项被回答、撤回（`decision.withdrawn`）或 Run 结束后，从「需要你处理」中移除或标记为过期。

<a id="server_sync"></a>
## 与后端同步

| 场景 | 行为 |
| --- | --- |
| 打开 Run | `GET /api/runs/{id}` 取全部事件，再用 `EventSource` 订阅 `/api/runs/{id}/stream?after=<最后一个 seq>` |
| 收到事件 | `seq` 必须正好是上一个加 1；出现缺口或收到 `lagged` 时，重新打开 Run |
| 发送操作 | `act()` 自动附加 `commandId`（去重）和 `expectedSequence`（当前最后一个 `seq`）。返回 409 时刷新一次再重试 |
| 中断的 Run | 发送 `resumeRun` 或 `message` 后重新连接 |
| 回放 | 审计页可以按 `seq` 回放；回放期间不发送任何操作 |

<a id="screens"></a>
## 页面

| 页面 | 地址参数 | 内容 |
| --- | --- | --- |
| 工作区首页 | `?ws=<id>` | 当前工作区、目标输入框、历史 Run |
| 设置 | `?settings=agents` / `?settings=roles` / `?settings=adapters` | Agents（Provider 连接与配置检查）、角色（模型、权限、Skill、MCP）、ACP 适配器（安装、检查更新、升级、切回，见 [ACP 适配器管理](adapters.md)） |
| 工作区编辑 | 无 | 目录选择、成员、主控 |
| 运行页 | `?run=<id>` | 工作台、审计与回放、团队与权限三个标签，以及任务页和 Agent 会话页 |

运行页的工作台包含阶段条、任务图（`dag`）、团队面板、共享看板和交付卡片。阶段条是「推进目标 / 协作 / 交付 / 验收」（`goalStages`）；早期计划模式的 Run 显示它们自己的「确认计划 / 执行 / 评审 / 交付 / 验收」。

<a id="session_messages"></a>
## 会话中的消息

会话页按[消息的流转](../interfaces/events.md#message_flow)显示用户和主控发给 Agent 的消息：

| 状态 | 显示位置 |
| --- | --- |
| 刚发出（`pending`） | 输入框上方的「待发送」区，标注「发送中」 |
| 排队（`queued`）、留言（`next`），还没送达 | 「待发送」区，标注排队或留言 |
| 插入当前这一轮（`steer`） | 对话中 `message.routed` 的位置，用「插话」框显示 |
| 作为新一轮（`prompt`） | 对话中 `message.routed` 的位置，普通消息气泡 |
| 排队或留言后送达 | 对话中 `message.delivered` 的位置，即它真正进入会话的时刻 |

`derive` 把带 `id` 的消息汇总在 `msgs` 中，`pendingTray` 渲染「待发送」区。看板「待决」中的「留给某 Agent 的消息」也来自 `msgs` 中还没送达的消息。

<a id="request_cards"></a>
## 请求卡片

所有需要用户处理的事项都通过 `askShell` 渲染为统一的卡片：

| 卡片 | 来源 | 函数 |
| --- | --- | --- |
| 提问 | `question.escalated` | `questionAsk` |
| 审批 | `approval.requested`，按 Agent 给出的选项渲染按钮 | `approvalAsk` |
| 表单或链接 | `input.requested`，按 JSON Schema 渲染表单 | `inputAsk`、`schemaForm` |
| 预算用完 | `run.budget_exhausted` | `budgetAsk` |
| 会话等待消息 | 人工接管中一轮结束 | `sessionAsk` |

会话页（`agentSession`）也会内联显示该会话中仍在等待的请求，用户在任一位置处理都生效。

## 角色权限编辑

角色编辑器的「权限」区域只显示 Agent 声明的权限类控制项（`permSection`），其余控制项放在 `otherCtrls`。权限选项按 Agent 原生的名字和说明显示，并标注该模式是否修改文件（`permWrites`）。只有 Claude Code 和 Codex 的角色可以勾选「可以担任主控」（`leadCapable`）。规则见 [Agent、角色与工作区](../domains/agents_and_roles.md#permission_modes)。

## 交互约束

- 重新渲染时，`renderScreen` 保存并恢复滚动位置，避免点击选项后页面跳回顶部。
- 浏览器存储只用于记住上次打开的工作区（`localStorage` 的 `janus.ws`），读写都包在 `try` 中。
