# 目标模式与共享看板

本文说明目标模式下的协作规则：主控怎样启动和了解团队，主控与协作者怎样通过共享看板传递任务、进展、阻塞和提问，以及主控结束后未完成的事项怎样处理。看板工具的参数和校验规则见[看板 MCP 工具](../interfaces/board_mcp.md)，Run 的整体状态机见 [Run 生命周期](run_lifecycle.md)。

<a id="lead_role"></a>
## 主控的职责

主控是工作区中被指定为主控的角色，在 Run 中的成员 id 固定为 `lead`。它：

- 用 `/goal` 推进用户的目标，可以自己从头做到尾。
- 需要时把一部分工作编排给协作者。是否协作、交给谁、怎么拆，都由主控判断，Janus 不强制规划。
- 在协作者完成后自行核对结果，确认目标达成后结束。

只有 Claude Code 和 Codex 能做主控，因为只有它们提供 `/goal`。这条规则在三处执行：保存工作区（`save_workspace`）、创建 Run（`create_run`）、启动主控会话（`lead_goal`）。应用的角色编辑和工作区设置也只允许这两种 Agent 的角色担任主控。

<a id="lead_session"></a>
## 主控会话

`App::goal` 启动主控会话并进入调度循环，`App::lead_goal` 负责会话本身：

1. 为主控生成看板令牌，记录当前 diff 作为基线（之后的 diff 产物只包含 Run 期间的改动）。
2. Claude Code 主控：在它的隔离配置 `~/.janus/agents/<id>/home/.claude.json` 中把工作目录标记为已信任，因为 Claude Code 只在受信任的工作区运行 `/goal`。不修改用户自己的 Claude 配置。
3. 启动 ACP 子进程，打开会话，接入看板 MCP 服务（`janus`），按角色设置会话控制项。
4. 注入说明（见[主控知道什么](#lead_brief)）：
   - Claude Code：通过 `session/new` 的 `_meta.systemPrompt.append`，主控说明之后接着用户的长期要求。
   - Codex：主控说明没有系统层入口，先作为第一条消息发送，要求只回复「已了解」；用户的长期要求作为开发者指令注入，见 [ACP 集成](../interfaces/acp_integration.md#standing_instructions)。
5. 发送 `/goal <目标>`。目标超过 3800 个字符时截断。
6. 如果回复表明 Agent 不支持或拒绝 `/goal`，主控以失败结束。
7. Codex 在提示返回后可能继续工作。Janus 读取 `session_info_update` 中的目标状态（记为 `goal.status` 事件），状态为 `active` 时继续跟随会话。
8. 一轮结束后，如果有用户排队的消息，把它们发给主控继续；如果用户接管了会话，等待用户消息或交还。

发给主控的上下文、`/goal` 和后续提示会作为 `session.prompt` 事件记录，主控会话页可以展开查看。主控调用 `orchestrate`、`wait`、`reply` 或 `team` 时，Janus MCP 入口会记录 `tool.started` 和 `tool.completed`，长时间的 `wait` 会先显示等待状态。

主控会话结束时，结果记为「完成」（附最后一条回复）或「异常」（附错误）。用户取消 Run 时不记结果。

<a id="lead_brief"></a>
## 主控知道什么

主控对团队的全部了解来自说明（`goal_brief`），`team` 工具会重复其中的团队信息和任务状态。说明包含：

- 主控的职责和自主权，以及「不要为了协作而协作」。
- 工作目录，以及主控和协作者都直接在这里工作、改动立即彼此可见。
- 协作者名单，每人一行（`team_lines`）：

  ```text
  - <id>：<名字>（<角色>）· 职责：<职责> · Agent <Agent 名称> · 模型 <模型和推理强度> · 权限模式 <Agent 自己的模式名>
  ```

- 工作目录是不是 Git 仓库。
- 十个看板工具的用法，以及完成消息只有摘要、细节用 `task_detail` 查看。
- 看板推送：需要主控处理的消息会以「[Janus 看板推送]」开头插入它的会话，推送过的不再出现在 `wait` 里；收到后先处理会卡住别人的事。见[看板推送](#board_push)。
- 谁来做决定，见[谁来做决定](#decision_ownership)。
- 怎么写验收标准、执行者怎么验证、没通过验收时怎么处理，与[验收标准](../interfaces/board_mcp.md#acceptance_criteria)一致；执行者的证据是它的说法，关键结果由主控自己核对。
- 协作要点：任务说明要写全（协作者看不到主控的对话）；同一协作者的任务会排队；会改同一批文件的任务要用依赖串行，两个任务同时改了同一个文件时看板上会出现「[冲突]」；任务派出去后主控不要自己改它负责的文件，要调整就 `reply`，要自己做就先 `cancel`；提问用 `reply` 回答；协作者完成后要自行核对；不要 `git commit`、push 或部署。
- 工作区共享资料的用法：已有的事实不再调研、不再问用户，与本次目标冲突时以本次目标为准；结束时把值得长期保留的新事实写在「建议更新共享资料」一节。工作区有共享资料时，正文附在说明末尾，见[工作区共享资料](agents_and_roles.md#workspace_brief)。
- 主控角色的 Skill、MCP，以及工作目录中的项目 Skill。角色指令和 Provider 的自定义提示词不在说明里，按 [用户的长期要求](../interfaces/acp_integration.md#standing_instructions) 单独注入。
- 从中断处继续时，看板上之前的任务及其状态。

<a id="board_flow"></a>
## 共享看板

主控和协作者之间没有直接对话，所有协作信息都经过看板：

| 方向 | 方式 |
| --- | --- |
| 主控 → 看板 | `orchestrate` 提交编排。Janus 先校验，全部通过后任务写入看板（`orchestration.accepted`）并按依赖执行；有任何错误则整份编排不执行（`orchestration.rejected`） |
| 协作者 → 看板 | 执行中用 `board_post` 写回进展、阻塞和备注；结束时返回结构化结果；需要主控决定时在结果中提问并停下 |
| 看板 → 主控 | `wait` 取回新消息，`team` 查看全部状态，`task_detail` 按 id 查看一个任务的全部信息；需要主控处理的消息还会推送进它的会话，见[看板推送](#board_push) |
| 看板 → 协作者 | `peers` 查看其他任务的状态、关系、改过的文件和完整结果；和别的任务改了同一个文件时收到提醒，见[同时改同一个文件](#conflicts) |
| 主控 → 协作者 | `reply` 回答提问，或给正在执行的协作者补充指示（立即送达） |
| 主控 → 任务 | `retry` 重做、`accept` 接受只是验证没通过的结果、`amend` 修订已完成任务的结论、`cancel` 取消 |
| 主控 → 用户 | `ask_user` 交出只有用户能做的决定 |

看板没有单独的存储，它就是 Run 的事件日志。`wait` 从主控上次读到的 `seq` 继续，把与任务相关的事件格式化成消息返回（`board_entry`）。主控能看到的事件类型见[看板 MCP 工具](../interfaces/board_mcp.md#wait)。

可以多次提交编排，后提交的任务可以依赖之前的任务。看板任务的 id 在整个 Run 内唯一。

<a id="board_push"></a>
### 看板推送

主控只有调用 `wait` 才读看板，而它可以自己动手做事。它做别的事情时，协作者的提问只能停着等，失败和停滞也没人处理。所以需要主控处理的消息会直接推送进主控正在进行的这一轮（`push_board`），设计取舍见 [ADR：需要主控处理的看板消息推送进主控的会话](../decisions/board_push.md)：

- **触发**：写入以下事件时（`needs_lead`）：`question.asked`、`task.failed`、`task.blocked`、没有 `resolved` 的 `task.stalled`、`kind` 为 `blocker` 的 `board.posted`、`task.conflict`，以及用户的回答（`ask_user` 问题的回答，或用户在会话中直接回答协作者）。
- **合并**：触发后等 2 秒（`PUSH_SETTLE`），把主控上次读到之后看板上的全部消息（格式与 `wait` 相同）合成一条，以「[Janus 看板推送]」开头，通过[插话](../interfaces/acp_integration.md#steering)插入主控的这一轮，并把游标移到最后一条。推送过的消息不再由 `wait` 返回。
- **条件**：主控的 Agent 声明了插话、会话正在进行、主控没有在 `wait` 中。不支持插话的主控不推送：推送只能排队到这一轮结束，在此期间 `wait` 反而看不到这些消息。推送只插话，插不进去时排队到这一轮结束，不打断主控。
- **记录**：推送记为 `session.prompt`（`channel: "board"`），主控会话中显示为「看板推送」；送达和读到的时刻记在 `message.routed`、`message.delivered` 中。

<a id="conflicts"></a>
### 同时改同一个文件

主控和协作者都直接在工作目录中工作，并行的任务可能改到同一个文件。Janus 从工具调用中识别改动（`record_edits`）：

- 协作者完成一次 `edit`、`delete` 或 `move` 类型的工具调用时，从 ACP 的 `locations`（没有时取输入中的 `file_path`、`path` 等）得到文件，转成相对工作目录的路径，记在这个任务名下（`tool.call` 的 `data.paths`）。
- 另一个正在执行的任务也改过其中的文件时，写 `task.conflict` 给主控（`wait` 中的「[冲突]」，并触发看板推送）；双方协作者的 Agent 支持插话时，各收到一条「[Janus 提醒]」，说明对方在改哪些文件、改之前先读最新内容、分工不清时写 blocker。同一对任务的同一个文件只报告一次。
- 只看协作者的改动，不看主控的；记录只在内存中，编排器重启后从头开始。
- 每个任务改过的文件可以用 `task_detail` 和 `peers` 查看。

<a id="collaborator_board"></a>
### 协作者接入看板的条件

协作者的会话只在以下条件同时满足时接入看板工具：

- Run 是目标模式。
- Agent 在 `initialize` 中声明 `mcpCapabilities.http` 为 `true`。

Pi 只支持 stdio MCP，不接入看板。它照常执行任务，通过结构化结果和提问回报。

<a id="decision_ownership"></a>
## 谁来做决定

一次 Run 里有两类决定，处理方式不同：

| 类型 | 例子 | 谁决定 | 应用中 |
| --- | --- | --- | --- |
| 执行中的选择 | 技术选型、实现方式、方案取舍、协作者在任务中的提问、失败任务怎么处理 | 主控 | 看板「待处理」中标「主控处理」，不需要用户操作 |
| 影响整个目标的决定 | 需求有多种理解且结果明显不同；需要用户的账号、凭据、付费或外部资源；不可逆或影响工作目录之外的操作 | 用户 | 「需要你处理」卡片，看板「待处理」中标「等你决定」 |

主控说明写明了这条分界，主控用 `ask_user` 交出第二类决定，交出后继续做不依赖它的工作。审批和表单请求始终属于用户，见[审批和表单](#user_requests_in_goal_mode)。用户直接回答了协作者的提问时，主控在 `wait` 中看到「用户已回答」，不再给出不同的答案。主控要推翻协作者已经给出的结论时，用 `amend` 修订，下游任务以修订为准，避免前后两个结论同时在看板上生效。

<a id="question_routing"></a>
## 提问的流转

1. 协作者在结构化结果中提问，任务停下等待（`question.asked`）。
2. 主控在 `wait` 中读到 `[提问]`，用 `reply` 回答（`question.answered`，`via: "lead"`）。
3. 回答作为下一轮提示发回协作者原来的会话，协作者继续完成任务。

主控在场时，提问不打扰用户。以下情况提问转给用户（`question.escalated`，由 `escalate_open_questions` 处理）：

- 主控的会话已经结束（完成目标或异常中断），而协作者还在等回答。
- 主控结束后才出现的新提问。

只转交仍然有效的提问：任务正在执行、提问之后没有开始新的尝试、还没有被回答或转交过。用户也可以直接在协作者的会话页回答，先到的回答生效，主控会在看板上收到「用户已回答」。

<a id="user_requests_in_goal_mode"></a>
## 审批和表单

协作者和主控发出的审批请求（`session/request_permission`）和表单请求（`elicitation/create`）始终交给用户。主控只能在看板上看到「等用户」，不能代为批准。这是有意的设计：权限决定属于用户。

<a id="failures_in_goal_mode"></a>
## 失败处理

- 委派任务失败（`task.failed`）不会让 Run 停下等用户，应用也不弹出「等你决定」卡片。主控在 `wait` 中读到 `[失败]` 或 `[没通过验收]`（逐条列出没达到的标准），核对后用 `retry`（可改说明和验收标准）、`accept`（结果可以接受）或 `cancel` 处理，也可以自己做。被失败任务阻塞的任务在它处理好后自动继续，不需要重新编排。
- 协作者很久没有动静，或连续重复同一个调用时，主控在 `wait` 中读到 `[停滞]`（见[停滞检测](run_lifecycle.md#watchdog)），由主控判断继续等、用 `reply` 指出问题、`retry` 还是 `cancel`。Janus 不会自动打断它。
- 主控自己很久没有动静或原地打转时，报告给用户（`lead.stalled`），应用在「需要你处理」中显示「主控可能卡住了」，用户可以给主控发消息、打断并接管或者继续等。
- 主控会话异常结束，且没有任务还在执行时，Run 失败。「从中断处继续」会恢复看板上的任务并重启主控，见 [Run 生命周期](run_lifecycle.md#recovery)。

<a id="delivery_in_goal_mode"></a>
## 交付

主控的目标结束、没有任务还在执行或可以启动时，Run 交付：

- 摘要取主控最后一条 `agent.message`，去掉末尾的 JSON 块和「建议更新共享资料」一节，最长 2000 个字符，后面附上「改动直接在工作目录中，Janus 没有提交」。
- 「建议更新共享资料」一节的条目单独放在 `briefSuggestions` 中，由用户决定是否写入工作区，见[工作区共享资料](agents_and_roles.md#workspace_brief)。
- 证据和风险从看板上各任务的结构化结果汇总；失败、已取消、被主控取消、因依赖失败没有执行的任务也列为风险。
- 主控会话异常结束时不交付，Run 失败，见[失败处理](#failures_in_goal_mode)。

设计取舍见 [ADR：主控以 /goal 推进目标](../decisions/goal_mode_lead.md) 和 [ADR：通过 MCP 看板协作](../decisions/board_over_mcp.md)。
