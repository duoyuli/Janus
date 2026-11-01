# ADR：权限交给 Agent 原生模式

- 状态：已采纳（accepted），记录于 2026-10-30，2026-10-31 修订第 3 条，并增加第 7 条。取代早期角色上的「可以修改文件」「工具调用需要我批准」开关和能力标签。
- 相关文档：[Agent、角色与工作区](../domains/agents_and_roles.md#permission_modes)、[ACP 集成](../interfaces/acp_integration.md#session_setup)、[macOS 应用](../architecture/frontend.md#request_cards)。

## 背景

早期角色有三项 Janus 自定义的设置：是否可以修改文件、工具调用是否需要用户批准、是否可以担任主控，另有一组能力标签。Janus 再把前两项换算成各 Agent 的固定模式。用户指出这套设计有问题：在 ACP 里每个 Agent 的权限都不一样，例如 Codex 的 Read-only / Agent / Auto review / Full access，Claude Code 的 Manual / Accept edits / Plan / Auto / Bypass，Grok 的每次询问 / 自动 / 始终允许，应该充分利用每个 Agent 自己的特性，参照 lody.ai 的做法。随后用户进一步要求：不要再有「可以修改文件」这种权限，完全走 Agent 本身的权限；权限交给各个 Agent 去执行，Janus 不做任何校验；能力标签从所有地方删除。

## 决策

1. 配置检查时读取 Agent 在 ACP 会话中声明的全部控制项，按类别和 id 识别其中的权限模式。
2. 角色直接选择这些控制项的值，打开会话时原样设置。
3. 是否修改文件不再是角色设置。2026-10-31 修订：Janus 也不再从权限模式推导「会修改文件 / 只读」，界面和主控收到的协作者名单都只给出 Agent 自己的权限模式名，由主控按模式本身理解。
4. Janus 不根据权限模式拦截或校验任何操作；Agent 发出的审批和表单请求统一转给用户，在界面上用同一种卡片渲染。
5. 删除能力标签；主控按职责、Agent、模型和权限模式分配工作。
6. 旧角色的开关在启动时一次性换算成各 Agent 的原生模式。迁移完成后，换算代码已在 2026-10-30 的清理中删除。
7. 例外：各 Agent 的 Plan 模式不提供给角色，打开会话时始终关闭（2026-10-31 增加）。Plan 模式只出方案不执行，Codex 主控在其中还会跳过 `/goal` 的自动推进，曾导致 Run 停在「目标进行中」。见 [不使用 Plan 模式](../domains/agents_and_roles.md#no_plan_mode)。

## 取舍

- **与统一的 Janus 权限模型相比**：用户看到的是每个 Agent 自己的名字和含义，Agent 升级新增模式时不需要改 Janus；代价是同一个工作区里各成员的权限语义不同，主控需要读懂每个成员的权限模式描述。
- **不做校验**：Janus 不再承诺「只读成员不会改文件」，这个保证完全取决于 Agent 自己。
- **不再推导**：早期版本按控制项的 id 和值的命名推导只读，这是启发式规则，新的 Agent 需要不断补规则，已在修订中删除。

## 后果

- 新增一种 Agent 时，只要它通过 ACP 声明控制项，角色编辑器就能直接使用。
- 计划校验中与写权限相关的检查被移除。
- 修订第 3 条后，`session_writes` 及成员的 `writable` 字段被删除。主控只看到模式名，例如 `Agent (full access) · Off`；模式名本身看不出能否改文件时，需要主控自己判断，或在角色职责中写明。
