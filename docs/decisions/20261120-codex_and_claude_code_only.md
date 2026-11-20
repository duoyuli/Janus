# ADR：去掉「自定义命令」，Agent 只有 Codex 和 Claude Code

- 状态：已采纳（accepted），记录于 2026-11-20。
- 取代：[ADR：内置运行时只保留 Codex 和 Claude Code](20261116-codex_and_claude_only.md) 中「保留自定义命令」的第 3 条。
- 相关文档：[Agent、角色与工作区](../domains/20261031-agents_and_roles.md#providers)、[ACP 集成](../interfaces/20261031-acp_integration.md#standing_instructions)、[测试](../operations/20261031-testing.md#fake_agent)、[数据模型与存储](../architecture/20261031-data_model.md#migrations)。

## 背景

2026-11-16 删除 Grok 和 Pi 时保留了「自定义命令」：用一条启动命令接入任意兼容 ACP 的 Agent。它和两个内置运行时处处不同：没有系统层，协作约定和用户的长期要求只能附在第一条提示里；不能加载 Skill；能不能用 HTTP、SSE 的 MCP 取决于它声明什么；不能做负责人（没有 `/goal`）。为了它，编排器留着一条「提示里的说明」通道、负责人的「背景信息」预热轮、资源的「取决于 Agent」状态，以及测试构建里放开的「自定义 Agent 可以做负责人」。实际在用的只有测试：后端测试和端到端测试的协作者都是以自定义命令启动的脚本化 Agent。用户在 2026-11-20 决定把它去掉。

## 决策

1. Agent 类型只有 Codex 和 Claude Code（`agents::kinds`），基础运行时 `Runtime` 只有 `Codex` 和 `Claude`。接口不再接受 `custom` 类型的 Agent。
2. 随之删除只为它存在的部分：提示里的说明通道（`Channel::Prompt` 和 `prompt_instructions`）、负责人的背景信息轮（`session.prompt` 的 `context` 通道）、资源支持状态里的 `depends`、Agent 的「启动命令」字段，以及测试构建里自定义 Agent 可以做负责人的例外（`Runtime::leads`，现在每个 Agent 都能做负责人，工作区只要求负责人绑定了 Agent）。
3. 数据版本 9 删除已有的自定义命令 Agent，绑定它的数字员工像删除 Agent 时一样改为没有绑定 Agent；旧 Run 快照里用它的成员记为 Claude Code，这样旧 Run 仍能读取和回放，但它的 Agent 已经不在，不能继续运行。
4. 测试改用替身适配器：后端测试把 `testdata/fake_acp.py` 装成 Codex 和 Claude Code 的「适配器」（`stand_in_adapters`），用 `python3` 代替 Node 启动它；端到端测试本来就为 Codex 和 Claude Code 装了替身适配器，协作者改用 Claude Code 类型。测试于是走正式的启动路径和 Claude Code 的系统层。

## 取舍

- **不能再接入其他 ACP Agent**：以后要支持新的 Agent，作为新的内置运行时加入（适配器、系统层、Skill 入口、默认权限模式），而不是一条通用命令。
- **旧 Run 的成员改记为 Claude Code**：这是为了让旧记录还能读出来，不代表它真的用 Claude Code 运行过；Agent 名字（`runtime_name`）保持原样，审计页仍显示原来的名字。
- 删除的 Agent 在 Janus home 中的隔离目录（`agents/<id>/`）不由迁移删除，迁移只改数据库。

## 后果

- 应用的「添加 Agent」只列出 Codex 和 Claude Code，数字员工只要绑定了 Agent 就能开启「可以担任负责人」。
- 文档中关于自定义命令的说明全部删除；`20261116-codex_and_claude_only.md` 标注被本 ADR 部分取代。
