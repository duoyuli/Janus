# ADR：工作区角色并发槽位与 Run 级并发上限

- 状态：已采纳（accepted），记录于 2026-11-18。
- 相关文档：[Agent、角色与工作区](../domains/20261031-agents_and_roles.md#workspaces)、[Run 生命周期](../domains/20261031-run_lifecycle.md#scheduling)、[配置](../interfaces/20261031-configuration.md#settings)、[ADR：任务直接在工作目录中运行](20261031-in_place_workspaces.md)。

## 背景

此前工作区配置将每个启用的角色 1:1 展开为单个 Run 成员（`Member`），而调度器限制每个成员同一时间只能执行一个任务（`!busy.contains(&t.agent)`）。在典型工作区中，「研发工程师」角色只有一个；即便负责人将目标拆分为多个负责范围（`owns`）互不重叠的独立模块，这些子任务也只能完全串行排队，用户若想并行只能手工复制多个相同的角色。

反过来，当工作区配置了较多成员且多个无依赖子任务同时就绪时，调度器会在同一轮循环中将所有就绪任务一次性全部启动，瞬时并发拉起大量 ACP 适配器子进程与 MCP 服务器，容易造成本机负载尖峰和上游接口限流。

## 决策

1. **工作区协作者角色支持并发槽位（`slots`）**：工作区的 `members` 条目除 `role` 外可指定可选整数 `slots`（范围 `1..=8`，缺省为 `1`；负责人角色固定为 `1`）。
2. **创建 Run 时展开为共享 `role_id` 的独立成员快照**：
   - 当 `slots == 1` 时，保持原有行为，生成单个 `Member`（id 为角色 id，或与 `"lead"` 冲突时为 `"lead-role"`）。
   - 当 `slots > 1` 时，`team_for` 自动展开为 `slots` 个独立的 `Member` 快照（id 为 `<role>-1` 至 `<role>-<slots>`，与已有角色 id 冲突时自动避让；显示名为 `<角色名> #1` 至 `<角色名> #<slots>`），所有分片快照的 `role_id` 字段均指向原角色 id，不从成员 id 的字符串形状反推角色归属。
   - 角色执行统计（`role_stats`）按 `Member.role_id` 聚合，多个分片的执行结果自然归入同一角色。
3. **Run 级全局并发上限（`max_concurrent_attempts`）**：
   - 新增编排器配置项 `JANUS_MAX_CONCURRENT_ATTEMPTS`（`config.toml` 键 `max_concurrent_attempts`，默认 `3`，范围 `1..=32`），保存在 `App.cfg.max_concurrent_attempts` 中。
   - 调度循环（`App::execute`）统计当前 `TaskStatus::Running` 的任务数，达到 `max_concurrent_attempts` 时不再启动新的尝试，其余满足依赖的就绪任务继续留在 `TaskStatus::Pending`。
   - 交付条件中的 `runnable` 判断仍将仅因并发上限或成员忙碌而未启动的 `Pending` 任务计为未完成工作，确保 Run 不会在仍有排队任务时提前交付；一旦在跑任务结束唤醒调度循环，立即补位启动下一个就绪任务。

## 取舍

- **在 `team_for` 展开为具名成员，而不是让单个 `Member` id 同时绑定多个并发会话**：Janus 中用户消息路由（`live_agent`）、留言队列（`notes`）和负责人的 `orchestrate` 指派都以成员 `id` 为寻址对象；展开为 `implementer-1`、`implementer-2` 后，每个分片拥有独立的留言与会话映射，负责人也能显式把一组关联子任务固定派给同一个分片（配合 `continues` 复用该分片的会话），同时保留按 `owns` 校验并发重叠的不变量。
- **不新增排队状态或事件类型**：受并发上限暂缓的任务保持 `Pending`，不污染状态机和前端状态推导。
- **默认并发上限取 `3`**：与主流多 Agent 运行时的默认并发（3 至 4）一致，兼顾并行吞吐、共享工作目录的构建稳定性与本机资源占用；重度机器可通过配置调高。

## 后果

- 工作区设置界面允许为已启用的协作者数字员工设置并发槽位数（`1` 至 `8`）。
- `App.cfg` 与 `config.rs` 新增 `max_concurrent_attempts` 配置读取；[Run 生命周期](../domains/20261031-run_lifecycle.md#scheduling)、[Agent、角色与工作区](../domains/20261031-agents_and_roles.md#workspaces) 与 [配置](../interfaces/20261031-configuration.md#settings) 同步更新。
