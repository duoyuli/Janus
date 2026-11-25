# ADR：协作者用工具交结果和提问

- 状态：已采纳（accepted），记录于 2026-11-25。取代协作者在回复末尾写 ```` ```json ```` 结构化结果、由 Janus 从回复正文里解析的做法。
- 相关文档：[交结果和提问](../domains/20261031-run_lifecycle.md#structured_result)、[验证](../domains/20261031-run_lifecycle.md#verification)、[看板 MCP 工具](../interfaces/20261031-board_mcp.md#member_tools)、[ADR：Janus 只注入协作规则](20261125-collaboration_only_injection.md)。

## 背景

协作者交结果靠提示词约定：系统层里写一段 JSON 样例，要求回复以这个代码块结束，Janus 在这一轮结束后用 `extract_json` 从回复正文里找最后一个代码块。格式错了、字段漏了，要等整轮结束、解析之后才发现，再另发一轮「修复」提示让它重写；找不到 JSON 时只能取回复第一行当摘要。提问也塞在同一段 JSON 的 `question` 字段里。用户指出：这种交接不能靠提示词驱动，应该给一个工具。

## 决策

1. 协作者的看板多两个工具：`janus.submit` 交结果，`janus.ask` 向负责人提问。只有子任务的会话看得到它们。
2. `janus.submit` 的参数结构由 `inputSchema` 约束，调用时当场核对格式（漏了标准、没有证据、方法或判定不是允许值、判定和逐条结论矛盾），不合格就退回全部问题，Agent 在同一轮里改好再交。
3. 收下的结果里有标准写了没达到时，第一次交会提醒它可以修好后再交一次；同一次尝试里再交就照收。这一轮结束时那份结果才算交上，仍有没达到的标准就交给负责人，不再另开修复的一轮。
4. 这一轮结束时什么都没交，提醒一次；仍然没交，这次尝试失败。
5. 不再从回复正文里读结果：`extract_json`、`parse_result` 删除，系统层不再有 JSON 样例，应用也不再从回复里隐藏结尾的 JSON 代码块。不保留兼容。
6. 看板服务器按 URL 里的席位（`/lead`、`/task/<id>`）分辨负责人和子任务，不再比较 `who` 是不是 `"lead"`：交结果要靠工具以后，id 恰好是 `lead` 的子任务原来会拿到负责人的工具和令牌，这个缺陷才暴露出来。

## 取舍

- **当场退回比事后修复快**：格式问题在调用时就返回，Agent 还在这一轮里、上下文完整，改一次参数即可；不再多花一整轮。
- **提问单独一个工具**：交结果和「卡住了要问」是两件事，拆开后不会出现既交结果又提问的情况，也和负责人的 `ask_user` 对称。
- **依赖 Agent 调用工具**：没调用就没有结果。两种 Agent 都支持 HTTP MCP，协作者的会话总有看板；没交的提醒一次，真实 Agent 的端到端测试检查它们会主动调用。

## 后果

- 新模块 `orchestrator/hand_in.rs`（`Handed`、`board_submit`、`board_ask`、`take_handed`），`RunState.handed` 保存每个子任务这一轮交上来的东西；`TaskResult` 去掉 `question`。
- `Seat`（`mcp_server.rs`）进入看板和资源库路由的 URL、令牌和调用方判断，路由改为 `/mcp/{run}/{token}/lead` 和 `/mcp/{run}/{token}/task/{task}`（加 `/tools` 是资源库路由）。
- `session.prompt` 的 `fix` 通道改为「提醒交结果」；看板上验证者「[验证没通过]」那一种消息不会再出现，删除。
- 后端流程测试的 `App` 在本地端口上提供服务（`served_app`），脚本化 Agent 经 HTTP 调用工具交结果，和真实 Agent 一样。
