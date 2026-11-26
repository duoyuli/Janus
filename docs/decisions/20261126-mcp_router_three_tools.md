# ADR：MCP 路由照 magpie 只给三个工具，Janus 的 MCP 服务器都介绍自己

- 状态：已采纳（accepted），记录于 2026-11-26。
- 取代：[ADR：资源库默认给所有数字员工，经 MCP 路由和 Skill 路由提供](20261119-library_routing.md) 第 3 条中的「有状态的服务器选每个会话单独运行」和「路由列出完整的工具列表」，以及取舍中的「不做 Janus 自己的工具搜索」；[ADR：Janus 只注入协作规则](20261125-collaboration_only_injection.md) 第 5 条中「MCP 服务器都不返回 `instructions`」和「负责人说明和协作者约定各有一节协作工具」。
- 相关文档：[资源库：MCP 路由](../domains/20261116-resource_library.md#mcp_router)、[看板 MCP](../interfaces/20261031-board_mcp.md#transport)、[ADR：MCP 路由的代码模式](20261126-run_code.md)、[调研：MCP 路由的通行做法](../research/20261120-mcp_router_practices.md)、[研究：magpie 的资源库](../research/20261116-magpie_library.md)。

## 背景

2026-11-26 用户指出 MCP 路由有问题，要求对照 [yetone/magpie](https://github.com/yetone/magpie)（提交 `88222ca`）的 MCP 路由重新讨论。对照后的差别：

- **Agent 不知道路由背后有什么**：`janus-tools` 回答 `initialize` 时不带 `instructions`（2026-11-25 按「只注入协作规则」删掉）。Codex 把 MCP 工具全部藏在 `tool_search` 后面，开局只给模型看命名空间和这段描述；没有它，模型只看到一个空的 `janus-tools`。magpie 遇到过同样的问题：Agent 不知道路由背后有 DeepWiki，就自己去克隆仓库。看板 `janus` 也不带 `instructions`，工具简介写在负责人说明和协作者约定里。
- **工具怎样给**：magpie 的路由自己有三个工具：`find_tools` 按关键词找工具，`call_tool` 按名字调用，`run_code` 运行一段调用这些工具的脚本。它只对不会自己搜索工具的 Agent 用前两个代替真实工具，对 Claude Code 和 Codex 仍完整列出。
- **进程范围**：magpie 一个服务器全机只起一个进程；Janus 按 Run 共用，或按会话单独运行。

用户逐项定下：

1. `janus-tools` 要带 `instructions`。作为 client，下游服务器的 `instructions` 照 magpie 的方式处理，不整段转发。
2. `janus-tools` 和 magpie 一样，只暴露三个工具，其余工具都隐藏，对所有 Agent 都如此。
3. 代码模式默认开；`save_as` 存下的 Skill 给所有数字员工。
4. 进程只按 Run 运行，去掉「每个会话单独运行」。
5. 给外部应用的地址、不经路由的服务器（magpie 的 `direct`）、关掉单个工具、每个服务器的闲置时间，先不做。
6. 协作、值班、记忆整理这些 Janus 自己的 MCP 服务器也补上 `instructions`。

## 决策

1. **服务器介绍自己**：`initialize` 的 `instructions` 是 MCP 协议里服务器的自我介绍，由 Agent 决定怎样展示：Claude Code 放在「MCP Server Instructions」一节，Codex 当作命名空间的描述。它不是 Janus 注入系统层的提示词，所以 Janus 的每个 MCP 服务器都返回它，只写「有哪些工具、各做什么」，不写怎样做事。
   - **看板 `janus`**：按调用者（负责人、协作者、值班、记忆整理、对话里提议数字员工）列出它的工具，一行一个，只说做什么。负责人说明、协作者约定、值班说明和记忆整理说明里的「协作工具」一节随之删除，一个机制只说一次。
   - **`janus-tools`**：照 magpie 的 `HubLine` 和 `hubDetails` 写，总长不超过 2000 个字符。第一行列出每个服务器和它做什么（不超过 250 个字符）；接着每个服务器一行，写标题、用途和工具（工具带说明的第一个分句，放不下时只写名字，再放不下写「另有 N 个」）；最后一句说怎样用三个工具。服务器的用途优先取资源库的描述，其次取服务器在 `initialize` 里说的：`serverInfo.description`，没有时取它 `instructions` 的第一行（最多 300 个字符）。下游 `instructions` 的其余部分不转交：预算有限，工具的用法在各工具的说明里。
2. **只给三个工具**：`tools/list` 固定给 `find_tools`、`call_tool`，代码模式开着时再加 `run_code`，不论背后有多少服务器、多少工具。
   - `find_tools {query, server?}` 照 magpie 的匹配规则：去掉停用词；中日韩文本在换文字处切开，再在「我、帮、个」这类字处切开；至少命中三分之二的关键词，其中一个在名字里。返回最相符的 5 个工具的完整定义，再列最多 15 个名字。没有命中时列出各服务器和它们的工具，并说明不要调用只是名字相近的工具。它的描述也带上服务器清单（最多 1200 个字符），给不展示 `instructions` 的 Agent 看。
   - `call_tool {name, arguments}` 依次按路由名、服务器内的同名工具、Agent 自己加了前缀的名字查找；`arguments` 收到 JSON 字符串时先解析。找不到时报工具错误，列出名字相近的工具。`tools/call` 直接用路由名调用也照样执行。
   - `run_code` 见 [ADR：MCP 路由的代码模式](20261126-run_code.md)。
3. **按 Run 运行**：去掉资源的 `process`，每个服务器同一个 Run 一个进程，闲置 10 分钟停掉，Run 结束时全部停止。数据版本 29 删除资源和 Run 快照里的 `process`。
4. **调用等 50 秒**：一次调用最多等 50 秒，短于 Claude Code 和 Codex 给 MCP 调用的 60 秒，Agent 收到的是路由的回答，而不是它自己的超时。

## 取舍

- **对所有 Agent 都只给三个工具，比 magpie 更进一步**：magpie 对会自己搜索工具的 Agent 仍完整列出。Janus 只支持 Claude Code 和 Codex，两者都会搜索，但工具搜索的效果取决于 Agent 和网关：经第三方 `ANTHROPIC_BASE_URL` 的 Claude Code，网关拒绝过一次 `defer_loading` 后，整段对话都会退回全量发送；Codex 的 BM25 有时连名字完全对上的工具都搜不到。三个工具让上下文和行为不随 Agent、网关和资源库大小变化。代价是 Agent 原生逐个工具的权限规则和确认只作用于这三个工具，见下一条。
- **逐个工具的权限只到三个工具这一层**：工具都藏在 `call_tool` 和 `run_code` 后面，Agent 的权限模式只决定能不能调用它们，不再按背后的具体工具区分。权限仍由 Agent 的原生模式决定，Janus 不另加开关（[ADR：权限由 Agent 原生模式决定](20261031-agent_native_permissions.md)）；每次真实调用照旧记 `mcp.routed`，负责人派出子任务后的调用照旧进看板、列为交付风险。不想让数字员工用的服务器，从资源库删除。
- **不转交下游的完整 `instructions`**：有的服务器只在 `instructions` 里写跨工具的用法（例如「先调 A 再调 B」），这部分会丢失；换来的是每个服务器都能在预算内被提到，第三方的文字也不会整段进入 Agent 的系统提示。
- **去掉「每个会话单独运行」**：浏览器、终端这类有状态的服务器，同一个 Run 的会话共用一份状态。用户选了简单的模型；这种服务器需要隔离时，交给服务器自己按调用方区分（magpie 的浏览器就是这样做的），Janus 不再提供第二种进程范围。
- **名片存完整的工具定义**：magpie 的名片只留 200 个工具、每个说明 200 个字符，因为它找工具时会问运行中的服务器要完整定义。Janus 的服务器用到才启动，`find_tools` 要在不启动服务器的情况下给出 `inputSchema`，所以名片存服务器列出的完整定义。
- **找不到工具时现场重列**：magpie 按 `<服务器>__` 前缀找到服务器后直接调用；Janus 在名片里找不到时让各服务器现场列一次再查找，能找到包更新后新增的工具，代价是一个写错的名字可能启动还没运行的服务器。
- **看板的工具简介挪进 `instructions`**：负责人和协作者在会话页的系统层里不再看到工具列表；工具列表和工具本身来自同一个服务器，不会在两处各说各的。

## 后果

- 后端：`mcp_router` 拆出 `say`（自我介绍）、`find`（`find_tools` 和按名字查找）和 `code`（`run_code`）；工具缓存改为「名片」（服务器的标题、一句话用途和工具），仍在 `cache/mcp-tools/<资源 id>/<revision>.json`，旧格式的缓存读不出时当作没有，下次用到时重新列出。`McpProcess` 和 `stop_session` 删除；看板的 `Caller::instructions` 列出工具。
- 应用：MCP 编辑器和精选库去掉「每个会话单独运行」；路由说明卡片改为三个工具。
- 文档：资源库的「MCP 路由」、看板 MCP 的「传输」、负责人说明和协作者约定的分节、值班和团队记忆的系统层、ACP 集成的系统层说明随之更新。
