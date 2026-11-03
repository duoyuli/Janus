# 领域文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录 Janus 的业务规则和状态机：Run 与任务怎样推进，主控与协作者怎样协作，Agent、角色、工作区和权限模式之间的规则。接口参数和事件字段归入[接口](../interfaces/index.md)，组件和存储布局归入[架构](../architecture/index.md)。

## 文档列表

- [Run 生命周期](run_lifecycle.md)：创建、工作目录模型、任务调度、尝试、结构化结果、验证、重试、交付条件、Run 状态、用户操作、恢复，以及早期版本 Run 的处理。阅读时机：修改 `create_run`、`execute`、`attempt`、`verify_result`、`deliver`、`action`、`resume_run`，或涉及任何 Run / 任务状态。
- [目标模式与共享看板](goal_mode.md)：主控的职责与会话、主控说明的内容、看板的信息流（含看板推送和同时改同一个文件）、协作者接入条件、提问转交、审批归属、失败处理和交付。阅读时机：修改 `goal`、`lead_goal`、`goal_brief`、`board_*`、`push_board`、`record_edits`、`task_detail`、`escalate_open_questions`，或调整主控与协作者之间的信息流。
- [Agent、角色与工作区](agents_and_roles.md)：Agent 类型、配置检查、会话控制项与权限模式、角色和工作区规则、成员快照、Skill（含 GitHub Skill 来源）和 MCP（含 GitHub MCP 来源与资源挂载）。阅读时机：修改 `backend/src/agents.rs`、`backend/src/skill_sources.rs`、`backend/src/mcp_sources.rs`、角色编辑器、设置的 Skill 库 / MCP 库或工作区设置，或新增一种 Agent。
