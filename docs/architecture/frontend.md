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

- 新增或修改事件类型时，必须同步更新 `derive` 中对应的分支，否则页面不会反映新状态。macOS 应用的 `Derive.swift` 是同一套规则的移植，要一起改，见 [macOS 应用](macos_app.md#event_derivation)。
- 页面上的「决定」「交接」「待处理」等看板条目由 `boardFacts` 从事件生成，同样依赖事件类型和 `data` 字段。「待处理」分两类：用户要处理的事项（来自 `inbox`，标「等你决定」，排在前面）和主控在处理的事项（协作者的阻塞和提问、失败的任务、依赖失败的任务，标「主控处理」）。只有存在前一类时，这一栏才用警示底色。还在路上的消息只显示在各会话的「待发送」区，不进入这一栏。
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
| 设置 | `?settings=agents` / `?settings=roles` / `?settings=adapters` | Agents（Provider 连接与配置检查）、角色（模型、权限、Skill、MCP，以及[执行统计](../domains/agents_and_roles.md#role_stats)）、ACP 适配器（安装、检查更新、升级、切回，见 [ACP 适配器管理](adapters.md)） |
| 工作区编辑 | 无 | 目录选择、成员、主控 |
| 运行页 | `?run=<id>` | 工作台、审计与回放、团队与权限三个标签，以及任务页和 Agent 会话页 |

运行页的工作台包含阶段条、任务图（`dag`）、团队面板、共享看板和交付卡片。目标可能是一大段粘贴的说明，顶栏、Run 列表和运行卡片只显示它的第一行（`goalTitle`），完整目标在运行卡片中展开查看。交付卡片（`resultCard`）依次显示主控的总结、交付物、证据、风险与限制，以及验收操作；偏差分类只在编排器判定了某一类（`diagnosis.cat`）时显示。阶段条是「推进目标 / 协作 / 交付 / 验收」（`goalStages`）；早期计划模式的 Run 显示它们自己的「确认计划 / 执行 / 评审 / 交付 / 验收」。

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

`derive` 把带 `id` 的消息汇总在 `msgs` 中，`pendingTray` 渲染「待发送」区，主控打断这一轮后送达的消息显示为「正在打断这一轮，马上送达」。

会话中的其他内容：

- Agent 的消息按轻量 Markdown 渲染（`md`）。
- 每次工具调用一行（`toolRow`），点开看完整输入和输出。Janus 看板工具按用途显示，例如「提交编排：T1 … → Researcher」「回复 T2：…」，完整参数来自 `tool.started` / `tool.completed` 的 `data.args`。同一次调用返回后替换「等待返回」那一行；Agent 自己上报的同一次看板调用（`mcp.janus.*`）不重复显示。
- 连续超过 4 次工具调用折叠为「N 个操作」，只露出最后两次（`chatBody`）。
- 协作者问主控的问题显示为「问主控 · 主控会回答，不需要你处理」。没通过验收核对、发回会话补充的提示显示为「验证没有通过 · 发回给它修复」。结构化结果逐条显示验收标准的 ✓ / ✗ 和证据；任务页的验收标准也按最新结果标出每一条。
- 看板「结论」中，被主控修订过的结论标「已被主控修订」，展开后先列修订；主控的修订、重做、取消、接受结果都记在「决定」中。
- Agent 自主执行时，会话标题旁显示距离最后一次活动的时间（超过 20 秒才显示，`tickQuiet` 每 5 秒更新，不重新渲染页面）。Agent 长时间思考或写大段内容时，ACP 不会有任何更新，这行字用来区分「在工作」和「卡住」。
- 对话区高度随窗口变化。重新渲染时，`patch` 保留带 `data-k` 的折叠块的展开状态。

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
| 主控请你决定 | 主控的 `ask_user`（`question.escalated`，没有任务） | `leadAsk`；没有选项时直接写回答 |

会话页（`agentSession`）也会内联显示该会话中仍在等待的请求，用户在任一位置处理都生效。

## 角色权限编辑

角色编辑器的「权限」区域只显示 Agent 声明的权限类控制项（`permSection`），其余控制项放在 `otherCtrls`。权限选项按 Agent 原生的名字和说明显示。页面不标注「可写」「只读」，角色列表和工作区只显示 Agent 自己的权限模式名（`permLabel`）。只有 Claude Code 和 Codex 的角色可以勾选「可以担任主控」（`leadCapable`）。规则见 [Agent、角色与工作区](../domains/agents_and_roles.md#permission_modes)。

## 交互约束

- 重新渲染时，`renderScreen` 保存并恢复滚动位置，避免点击选项后页面跳回顶部。
- Agent 写的文字（目标、交付总结）用 `md` 按轻量 Markdown 渲染：段落、列表、粗体、行内代码和链接。先转义再加标签；本地文件链接显示为代码，完整路径放在 `title` 中。看板卡片的摘要按长度截断，用 `plain` 去掉 Markdown 标记再显示，展开后用 `md` 显示全文。
- 浏览器存储只用于记住上次打开的工作区（`localStorage` 的 `janus.ws`），读写都包在 `try` 中。
