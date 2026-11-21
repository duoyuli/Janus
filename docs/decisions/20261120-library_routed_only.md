# ADR：资源库的每项资源都给所有数字员工，只经两个路由提供

- 状态：已采纳（accepted），记录于 2026-11-20。第 3 条中 `janus-skills` 的内容由 [ADR：Skill 路由是一份按需查找的目录](20261120-skill_catalog.md) 修订（同日）：按使用次数排序、太大时按关键词搜索，系统层写明它的位置。
- 取代：[ADR：资源库默认给所有数字员工，经 MCP 路由和 Skill 路由提供](20261119-library_routing.md) 的第 1、2 条（「给谁用」和收口的资源直接注入）、第 4 条（Skill 按数量切换），以及第 3 条中「http、sse 服务器照旧直接注入」的实现修订。那份 ADR 中路由本身的设计（用到才启动、同一个 Run 共用、有状态的服务器按会话运行、列工具不启动进程、工具名、调用记录、负责人派出后的提醒）不变。
- 相关文档：[资源库](../domains/20261116-resource_library.md)、[安全边界](../operations/20261031-security.md#library)、[ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](20261116-launch_time_resources.md)。

## 背景

2026-11-20，用户在一个 Codex 负责人会话里看到 3 个 MCP 服务器：Codex 登录账号后自带的 `codex_apps`（283 个工具）、看板 `janus`，以及资源库里给所有人的 HTTP 服务器 `ops-global-infra`。按当时的规则，`janus-tools` 只接给所有人的命令启动的服务器；HTTP、SSE 服务器和只给选中数字员工的服务器逐个放进 `mcpServers`；给所有人的 Skill 不超过 10 个时逐个加载。会话里的服务器和 Skill 描述因此随资源库增长。

用户的要求：

- 数字员工的 Agent 默认只拿到路由的 Skill 和 MCP。资源库里的 MCP 以后越来越多，逐个注入会撑爆上下文。
- 范围是 Janus 资源库里添加的资源；Agent 自带的（例如 `codex_apps`）不管。
- 去掉「给谁用」：资源库目前就是给所有数字员工的。

## 决策

1. **去掉「给谁用」**：资源没有 `reach`，角色没有 `resources`。创建 Run 时，每个成员固定整个资源库。数据版本 11 删除资源（含 Run 快照中的资源）的 `reach` 和角色的 `resources`；`POST /api/library/items/{id}/reach` 和 `/roles` 删除。
2. **MCP 只经 `janus-tools`**：命令、HTTP、SSE 服务器都由这个 Run 的 MCP 路由提供，会话的 `mcpServers` 只有 `janus-tools` 和看板 `janus`。HTTP、SSE 服务器由 Janus 进程连接，带上资源保存的 header；同一个 Run 共用一个连接（Streamable HTTP 的会话），闲置 10 分钟或 Run 停止时结束。SSE 服务器给出的消息地址必须和它自己同源。
3. **Skill 只经 `janus-skills`**：不再按数量切换，成员的资源包里只有 `janus-skills`，它列出每个 Skill 的描述和 `SKILL.md` 路径。
4. **Agent 自带的 Skill 和 MCP 不管**：Codex 的 `codex_apps`、系统 Skill，Claude Code 的内置 Skill 等由 Agent 自己决定，Janus 不在启动时关掉它们。

## 取舍

- **上下文只随路由增长**：会话只多 `janus-tools` 和 `janus-skills` 两项；资源库再大，Skill 也只占一条描述。`janus-tools` 仍列出背后的全部工具，Claude Code 和 Codex 自己按需搜索 MCP 工具，不会全部放进提示词。
- **不能再给个别数字员工收口**：每个数字员工都能调用资源库里所有服务器的工具，包括写数据、发消息这类有副作用的操作。权限由 Agent 的原生模式决定，Janus 不另加开关（见 [ADR：权限交给 Agent 原生模式](20261031-agent_native_permissions.md)）；负责人派出子任务后调用路由工具，照旧记在看板上、交付时列为风险。不想让数字员工用的服务器，从资源库删除。
- **Janus 进程联网并使用凭据**：以前由 Agent 进程连接 HTTP、SSE 服务器、使用 header 的值；现在由 Janus 进程连接。这扩大了 Janus 的信任边界：Janus 向用户填写的地址发出请求，`Authorization` 这类值在 Janus 进程中使用。值仍只在连接时从 `secrets/library/` 读取，不进快照、事件和日志。HTTP 客户端用 `reqwest`，TLS 用 macOS 系统的证书库，遵循系统代理。
- **少量 Skill 也走路由**：Agent 多读一次 `SKILL.md`，在原生 `/` 菜单里也不能点名单个资源库 Skill；换来规则只有一条，提示词不随 Skill 数量增长。
- **Codex 不支持 SSE 的限制消失**：路由用 HTTP 交给 Agent，SSE 服务器对两种 Agent 都能用。

## 后果

- 后端删除 `library::Reach`、`set_reach`、`set_item_roles`、`support`、`SKILL_ROUTE_OVER` 和 `McpServer::acp`；新增 `orchestrator::mcp_client`，统一 stdio、Streamable HTTP 和 SSE 三种连接。HTTP、SSE 服务器在资源库里新建或修改后也预先连接一次，记下它的工具。
- 应用的资源库去掉「给所有数字员工」开关、两个分组和勾选；数字员工编辑器去掉「能用的资源」；MCP 编辑器去掉「给谁用」。
- [安全边界](../operations/20261031-security.md#library)的「资源库」一节相应更新。
