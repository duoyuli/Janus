# MCP 路由的通行做法

- 日期：2026-11-20。
- 对象：Janus 的 MCP 路由 `janus-tools`（当天 `main` 的 `orchestrator::mcp_router` 和 `mcp_client`），对照 MCP 规范、MCP 网关的公开实现，以及 Claude Code 和 Codex 对 MCP 工具的处理。
- 信息来源：MCP 规范 2025-11-25 的 Tools 一节和 2026-07-28 发布候选公告，Tyk 的 MCP 聚合实现，Claude Code 和 Codex 的文档与 GitHub issue，社区文章，见文末「来源」。
- 范围限制：Claude Code 和 Codex 的行为来自文档、issue 和第三方测试，没有在 Janus 中用真实 Agent 逐项复现；标为「未确认」的需要实测。

## 结论

`janus-tools` 的整体模式和公开的 MCP 网关一致：一个端点聚合多个服务器，工具名带服务器前缀，按名字转发调用。不另做工具搜索的决定仍然成立，因为两个 Agent 都已默认延迟加载 MCP 工具。差距在协议细节：分页、取消、版本协商、未知工具的错误码，以及为 Agent 的工具搜索准备名字和说明。这些已经采纳，当前行为见[资源库：MCP 路由](../domains/20261116-resource_library.md#mcp_router)。

## 外部现状

### MCP 规范 2025-11-25

- 工具名应为 1 到 128 个字符，只用字母、数字、`_`、`-` 和 `.`，在一个服务器内唯一（SEP-986，规范中是 SHOULD）。
- 未知工具是协议错误（JSON-RPC 错误，示例用 `-32602`）；API 失败、参数校验失败这类是工具执行错误（`isError: true`），应交给模型以便它改正。
- `tools/list` 支持分页：请求带 `cursor`，回应带 `nextCursor`。
- 工具可以声明 `execution.taskSupport`（`forbidden`、`optional`、`required`），`required` 的只能以 task 方式调用。
- `annotations` 只是提示，来自不受信任的服务器时客户端必须当作不可信。
- 客户端应给工具调用设超时；放弃一个请求时应发送 `notifications/cancelled`。

### MCP 规范 2026-07-28（发布候选）

协议层变成无状态：去掉 `initialize` 握手和 `Mcp-Session-Id`，每个请求在 `_meta` 中带协议版本和能力，新增 `server/discover`；Streamable HTTP 的请求带 `Mcp-Method`，工具调用带 `Mcp-Name`，方便网关不解析正文就能路由。SSE 传输、roots、sampling 和 logging 被弃用。

### 网关的做法

- 给工具名加服务器前缀消除冲突，`__` 是常见的分隔符（例如 `github__issue_read`）。不能简单拼接各服务器的列表再指望名字不重复；调用时按配置好的服务器身份路由，而不是按「最近一次声明这个名字的服务器」。
- 在工具说明里也写上命名空间：模型读说明比读命名约定可靠。
- 列表按确定的顺序返回。
- 未知的服务器前缀和上游拒绝的未知工具都回答 `-32602`。
- 转发取消，调用有超时。
- 有的网关把每次的工具列表和批准时的指纹比较，变化时等待审核，防止服务器在被批准后悄悄改变工具（rug pull）。

### Claude Code

- 工具搜索默认开启：会话开始只加载 MCP 工具的名字和各服务器的 instructions，需要时由 ToolSearch 按需取出完整定义。
- 每个服务器的 instructions 和每个工具的说明各截到 2048 个字符，超出部分丢弃；工具搜索开启时，instructions 是模型判断何时去搜这个服务器的主要依据，官方建议把关键信息写在最前面。
- 以下情况会退回一次性加载：`ANTHROPIC_BASE_URL` 指向第三方代理（较新版本已改为默认开启，以文档为准）、Vertex、`CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`、HIPAA 配置。
- issue #40314 报告 HTTP 传输的 MCP 服务器的工具没有被延迟加载。`janus-tools` 正是 HTTP 服务器，这一点在 Janus 中**未确认**。
- 单个工具结果超过 10,000 个 token 时警告，默认在 25,000 个 token 处截断。
- 进度通知不延长 `MCP_TOOL_TIMEOUT`。

### Codex

- 从 0.142.2 起，模型和服务商支持搜索工具时，所有 MCP 工具都放在 `tool_search` 后面（`defer_loading`）；之前是超过 100 个工具才延迟。0.167.0 起开局只列出延迟工具的命名空间。
- `tool_search` 用 BM25 检索名字和说明，有时连名字完全对上的工具都搜不到（#21503）。
- `notifications/tools/list_changed` 不会刷新延迟工具的缓存（#33266）。
- `tool_timeout_sec` 默认 60 秒；超时时是否发送 `notifications/cancelled`，源码阅读和实测的结论相反，**未确认**。进度通知不延长超时。
- 0.147.0 起可以选择使用 2026-07-28 协议。

## 对照 Janus

| 项 | 当天的 Janus | 处理 |
| --- | --- | --- |
| 上游 `tools/list` 分页 | 只读第一页，分页的服务器后面的工具丢失 | 采纳：读完所有页，最多 100 页 |
| 取消 | Agent 的 `notifications/cancelled` 被忽略，超时后上游继续执行 | 采纳：按会话和请求 id 停止等待，并通知上游 |
| 协议版本 | 路由和看板原样回显 Agent 请求的版本；连上游时不核对服务器回答的版本 | 采纳：只同意 Janus 支持的版本 |
| 未知工具 | 返回 `isError` | 采纳：返回 `-32602`（只改路由，看板中角色不对的调用仍返回 `isError`，方便负责人改正） |
| 工具名冲突 | 服务器名含 `__` 或以 `_` 结尾时，两个不同的工具可能同名 | 采纳：这类服务器名一律带哈希 |
| 说明中的命名空间 | 只有工具名带服务器前缀 | 采纳：说明前加 `[<服务器>] ` |
| instructions 长度 | 没有预算，服务器多时被 Claude Code 截断 | 采纳：控制在 2048 个字符内，先写用法 |
| 只能以 task 方式调用的工具 | 照样列出，调用必然失败 | 采纳：不列出 |
| 工具搜索 | 列出全部，靠 Agent 自己搜索 | 维持，见下文 |

## 不采纳的部分

- **路由自己的 `tools/search`**：只有把真实工具藏在 `search_tools` 和 `call_tool` 后面，搜索才有用；但那样所有调用在 Agent 看来都是同一个 `call_tool`，Agent 原生的逐个工具的权限规则和确认失效，与 [ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md) 冲突；Agent 也拿不到真实的参数结构和 `annotations`，事件里只剩 `call_tool`。两个 Agent 自带的搜索已经做了这件事。
- **转发进度通知**：两个 Agent 收到进度都不延长超时，转发还要把路由的回应改成事件流。
- **向 Agent 推送工具列表变化**：路由的回应是 JSON，没有推送通道；Codex 收到变化也不刷新延迟工具的缓存。
- **工具定义变化的审核（rug pull 防护）**：需要「工具变了，等用户确认」的界面和流程，属于扩大安全策略，应单独写 ADR 决定。
- **迁移到 2026-07-28 无状态协议**：两个 Agent 默认还不用它，上游服务器大多仍支持旧版本；迁移是路由和客户端的重写，应单独立项。
- **转发上游服务器的 resources、prompts 和 instructions**：路由只做工具；上游 `initialize` 中的 instructions 目前不交给 Agent，放进路由的 instructions 会和 2048 个字符的预算竞争，待有实际需要再评估。

## 待核实

- Claude Code 是否延迟加载 `janus-tools` 的工具（#40314）：用真实 Claude Code 连一个工具很多的路由，在 `/context` 中看工具是否显示为按需加载。如果不延迟，资源库一大，全部工具定义会在开局进入上下文。
- 用户的 Claude Code 连接是否常配置第三方 `ANTHROPIC_BASE_URL`，使工具搜索关闭；如果是，再评估是否在这类连接上提供搜索加调用的方式，并写进 ADR。

## 来源

- [MCP 规范 2025-11-25：Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
- [SEP-986：工具名格式](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/986)
- [MCP 2026-07-28 发布候选](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
- [Tyk MCP Aggregator PR #8904](https://github.com/TykTechnologies/tyk/pull/8904)
- [One MCP Gateway for All Your Internal APIs（DEV）](https://dev.to/jeff_pdc/one-mcp-gateway-for-all-your-internal-apis-the-aggregation-pattern-4po)
- [Claude Code：Scale to many tools with tool search](https://code.claude.com/docs/en/agent-sdk/tool-search)
- [Claude Code issue #40314](https://github.com/anthropics/claude-code/issues/40314)
- [Claude Code 把 server instructions 截到 2048 个字符（DEV）](https://dev.to/rulestack/20-of-20-runs-sent-serverdiscover-to-our-stdio-mcp-server-off-by-default-per-the-docs-and-its-12m2)
- [Codex MCP 文档](https://developers.openai.com/codex/mcp)
- [mcpmini PR #552：Codex 自 0.142.2 起延迟所有 MCP 工具](https://github.com/mcpmini/mini/pull/552)
- [Codex issue #21503](https://github.com/openai/codex/issues/21503)
- [Codex issue #33266](https://github.com/openai/codex/issues/33266)
- [gori PR #1795：Codex 的超时与取消](https://github.com/hahwul/gori/pull/1795)
- [queuekeeper issue #269：Codex 超时实测](https://github.com/vocaro/queuekeeper/issues/269)
