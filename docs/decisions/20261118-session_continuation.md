# ADR：基于 `session/load` 复用前置任务会话

- 状态：已采纳（accepted），记录于 2026-11-18。第 2 条（`retry` 的 `continue_session`）和「验证没通过后派修复子任务（`continues`）」的用法已被 [ADR：返工由负责人用 `retry` 发回原任务](20261119-rework_in_original_session.md) 取代（superseded），其余各条仍然有效。
- 相关文档：[Run 生命周期](../domains/20261031-run_lifecycle.md#attempt)、[目标模式与共享看板](../domains/20261031-goal_mode.md#team_structure)、[看板 MCP 工具](../interfaces/20261031-board_mcp.md#orchestrate)、[ACP 集成](../interfaces/20261031-acp_integration.md#session_setup)、[ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](20261116-launch_time_resources.md)。

## 背景

在多 Agent 协作中，「实现 → 独立验证 → 按验证意见修复」是最常见的闭环。此前的实现里，每次任务尝试结束时都会立即关闭 ACP 子进程（`close_session`），已结束的任务也不接受 `janus.reply`。当独立验证发现缺陷、负责人派发修复子任务或调用 `janus.retry` 重做时，执行者必须启动新的 ACP 会话，从零开始重新读取同一批源码，既带来子进程与 MCP 启动延迟，又让模型损失前序对话积累的上下文和云端 KV Prompt Cache。

与此同时，Janus 在编排器中断恢复（`resume_run`）和交付后负责人对话（`lead_conversation`）中已经实现了基于 ACP `session/load`（配合 `drain_replay`）恢复既有会话，并在打开会话时传入与原会话一致的 `Opening`（Skill 资源包、`mcpServers` 与系统层）。

## 决策

1. **编排支持显式续接会话（`continues`）**：`orchestrate` 的任务定义新增可选字段 `continues`，填写同一执行者（`assignee`）此前已提交的子任务 id。`teamwork::check` 校验目标任务存在、不是自身、不是独立验证任务（不能同时声明 `verifies`），且执行者完全一致；校验通过后自动将目标任务加入 `deps` 并检查无环。
2. **重试支持可选续接会话（`continue_session`）**：`janus.retry` 新增可选布尔参数 `continue_session`（默认 `false`）。设为 `true` 时，下一次尝试优先恢复该子任务上一次尝试的会话，而不是从空白会话重来；记入 `task.reopened` 事件的 `data.continueSession`，重启恢复时保持一致。
3. **上下文窗口 75% 安全阈值**：续接会话前，编排器从事件日志读取目标会话最近一次 `session.usage` 快照（`used` 与 `size`）。当 `size > 0` 且 `used / size > 0.75` 时，不再执行 `session/load`，自动回退为新建干净会话（`session/new`），防止长链条修复直接撞上上下文窗口上限。
4. **能力检查与优雅回退**：仅当 Agent 在 `initialize` 中声明 `loadSession == true` 时调用 `session/load`；若未声明或 `session/load` 调用返回错误，自动回退为 `session/new`，不因会话无法恢复而令尝试失败。实际走的路径与回退原因记录在 `attempt.started` 的 `data.continuation` 中。
5. **续接提示词完整携带新契约**：与编排器崩溃后继续同一未完成子任务（`Interrupted`）不同，通过 `continues` 或 `retry(continue_session: true)` 恢复原会话（`Followup`）时，提示词明确说明「在原会话基础上继续」，并完整附上新子任务（或重试后更新）的说明、验收标准、上游交接信息（如独立验证的 `rejected` 结论）和负责文件范围。

## 取舍

- **用 `session/load` 按需恢复，而不是保持常驻空闲子进程池**：常驻进程池会持续占用本机内存与 stdio MCP 子进程，且需要额外的空闲淘汰和崩溃清理状态机；按需启动适配器并调用 `session/load` 复用服务端/本地会话记录与前缀缓存，同时保持「尝试结束即释放进程」的简单生命周期。
- **要求同一 `assignee` 才能 `continues`**：不同成员绑定不同的隔离目录（`~/.janus/agents/<id>/`）、模型、权限模式和资源包，跨成员加载会话既在本地存储上不可达，也会混淆角色指令与权限边界。
- **独立验证任务不允许 `continues`**：独立验证的价值在于脱离执行者心智上下文做客观核查，始终使用全新的干净会话。

## 后果

- `TaskDef` 新增可选字段 `continues`；`task.reopened` 新增可选布尔字段 `data.continueSession`；`attempt.started` 在续接场景下新增 `data.continuation`（记录来源任务、是否成功加载及回退原因）。
- 不改变任务状态机，不写入任何 Agent 配置文件，也不扩大信任边界。
