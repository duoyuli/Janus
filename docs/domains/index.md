# 领域文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录 Janus 的业务规则和状态机：Run 与任务怎样推进，负责人与协作者怎样协作，Agent、角色、工作区和权限模式之间的规则。接口参数和事件字段归入[接口](../interfaces/index.md)，组件和存储布局归入[架构](../architecture/index.md)。

## 文档列表

- [协作三方：负责人、协作者和编排器](20261119-collaboration_roles.md)：三方的分工（负责人判断、协作者执行、编排器只做机械的部分），负责人和协作者的全部看板工具，以及编排器在后台自动做的事，每项链接到细节。阅读时机：想知道一个 Run 中谁能做什么、增删看板工具、或想让编排器自动做一件事之前。
- [Run 生命周期](20261031-run_lifecycle.md)：创建、工作目录模型、任务调度、尝试、结构化结果、验证、重试、交付条件、Run 状态、用户操作和恢复。阅读时机：修改 `create_run`、`execute`、`attempt`、`verify_result`、`deliver`、`action`、`resume_run`，或涉及任何 Run / 任务状态。
- [目标模式与共享看板](20261031-goal_mode.md)：负责人的职责与会话、负责人说明的内容、看板的信息流（含看板推送、同时改同一个文件、负责的文件、拆分提议和独立验证、工作目录里的痕迹）、协作者接入条件、提问转交、审批归属、失败处理和交付。阅读时机：修改 `goal`、`lead_goal`、`goal_brief`、`board_*`、`push_board`、`record_edits`、`task_detail`、`escalate_open_questions`、`record_changes`、`backend/src/teamwork.rs`、`backend/src/traces.rs`，或调整负责人与协作者之间的信息流。
- [Agent、角色与工作区](20261031-agents_and_roles.md)：Agent 类型、配置检查、会话控制项与权限模式、角色和工作区规则、分组与数字员工文件（导入导出、招聘顾问）、成员快照。阅读时机：修改 `backend/src/agents.rs` 或 `backend/src/agents/`、角色编辑器或工作区设置，或新增一种 Agent。
- [对话](20261120-chats.md)：和一名数字员工单独对话：对话是 `kind` 为 `chat` 的 Run，在新建的工作目录里，没有看板和交付，会话没了由下一条消息重新打开；应用的对话页。阅读时机：修改 `create_chat`、`lead_conversation`、`chat_brief`、Run 的 `kind`，或应用的对话页和对话的输入框。
- [工作区值班](20261121-loops.md)：按时间表（cron）醒来的负责人会话：一次醒来一轮，看、判断，要改的派成独立的目标 Run（按 key 只派一次，Run 结束前自己闭环、结束时唤醒值班），提问进收件箱、回答唤醒它；下次醒来的时刻记在 `loop.scheduled`；问题不随会话撤回；值班的会话是 `kind` 为 `loop` 的 Run。阅读时机：修改 `loops.rs`、值班的工具或系统层、`RunKind::Loop`、`runs.origin`，或应用的值班页和表单。
- [团队记忆](20261123-team_memory.md)：工作区里 Agent 记下、之后的 Run 读到的笔记：只经 `janus` 工具写、记来源、不跨工作区，会话的系统层有索引、正文按需读；交付 5 个 Run 或手动时由负责人整理（`kind` 为 `dream` 的 Run）；Claude Code 自带的 auto memory 关闭。阅读时机：修改 `memory.rs`、`memories` 表、记忆工具、注入会话的团队记忆一节、整理，或应用的团队记忆一节和整理页。
- [资源库](20261116-resource_library.md)：Skill 和 MCP 服务器的资源模型、内容存储、凭据、Git 来源与更新、Run 的资源包，以及会话怎样只经 Skill 路由和 MCP 路由拿到全部资源。阅读时机：修改 `backend/src/library.rs` 或 `backend/src/library/`、`team_for` 中的资源、会话的 `_meta` 和 `mcpServers`，或应用的资源库页面和角色的资源选择。
