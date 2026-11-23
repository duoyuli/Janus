# ADR：Codex 和 Claude Code 默认不受限运行

- 状态：已采纳（accepted），记录于 2026-11-18。
- 相关文档：[会话控制项与权限模式](../domains/20261031-agents_and_roles.md#permission_modes)、[会话设置](../interfaces/20261031-acp_integration.md#session_setup)、[工作目录访问](../operations/20261031-security.md#workspace_access)、[ADR：权限交给 Agent 原生模式](20261031-agent_native_permissions.md)。

## 背景

角色没有选择权限模式时，会话沿用 Agent 自己的当前模式：Codex 是 Auto review（`agent`），Claude Code 是用户上次的模式（例如 Auto）。这些模式会在部分操作前停下来等审批，负责人用 `/goal` 自主推进目标时，审批请求转到收件箱，Run 在用户回应前停住。用户要求数字员工的权限模式默认选择 Codex 的 Full access 和 Claude Code 的 Bypass permissions。

## 决策

1. 每个内置运行时可以指定一个默认权限模式（`Runtime::default_mode`）：Codex 是 `agent-full-access`（Full access），Claude Code 是 `bypassPermissions`（Bypass permissions）。自定义命令的 Agent 没有默认值，沿用它自己的当前模式。
2. 默认值只在角色没有选择权限模式、且 Agent 声明了这个选项时生效；角色选择的模式始终优先。Agent 不提供这个选项时，沿用 Agent 的当前模式。
3. 默认值不写进角色。角色保存时仍只保留用户选择的值，修改默认值后，没有选择的角色随之变化。
4. Janus 仍然只使用 Agent 自己声明的模式，不新增 Janus 的权限开关或校验，[ADR：权限交给 Agent 原生模式](20261031-agent_native_permissions.md)的决策不变。

## 取舍

- **与沿用 Agent 当前模式相比**：目标可以一路执行到交付，不因审批停住；代价是没有改过权限的数字员工可以不经确认修改任意文件、执行命令和访问网络，误操作的影响范围变大。
- **与把默认值写进角色相比**：不需要迁移已有角色，默认值集中在一处；代价是角色上看不到显式的值，界面需要另外标出默认项。

## 后果

- 已有的、没有选择权限模式的角色，下一次创建 Run 时按新的默认值运行。
- 应用的连接数据给每个控制项带上 `default`，角色编辑器按它选中和标注「默认」。
- 安全边界文档记录了这一默认值；需要审批的场景，在数字员工的「权限」中改选其他模式。
- 2026-11-22 补充：Codex 负责人显示 Full access，`/goal` 里却仍弹审批，钥匙串和工作目录外的文件也用不了。原因是 Codex 的适配器只把会话模式带进它自己发出的提示，`/goal` 的各轮用线程自己的设置，默认是「需要时询问」加工作目录沙箱。现在 Janus 启动 Codex 时按成员的模式把线程的审批和沙箱设置写进 `CODEX_CONFIG`，默认的 Full access 在 `/goal` 里也真正生效，见[会话控制项与权限模式](../domains/20261031-agents_and_roles.md#permission_modes)。
