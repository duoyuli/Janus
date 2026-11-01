# 接口文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录 Janus 对外和对 Agent 的接口：应用使用的 HTTP / SSE 接口、事件格式、看板 MCP 工具、ACP 客户端行为和配置项。这些接口背后的业务规则归入[领域](../domains/index.md)。

## 文档列表

- [HTTP API](http_api.md)：路由、设置对象、Run 创建与详情、SSE 事件流、Run 操作和产物读取。阅读时机：修改 `backend/src/main.rs` 的路由或处理函数、`App::action`，或应用调用的接口。
- [事件参考](events.md)：事件结构、`data.key` 约定和全部事件类型。阅读时机：新增、修改或依赖某个事件类型或 `data` 字段。
- [看板 MCP 工具](board_mcp.md)：MCP 端点与传输、主控和协作者的工具、编排校验规则、验收标准。阅读时机：修改 `App::mcp`、`lead_tools`、`member_tools`、`validate_orchestration`、`result_gaps`，或主控调用看板出现问题。
- [ACP 集成](acp_integration.md)：stdio 传输、`initialize` 能力、使用的 ACP 方法、`_meta` 扩展、会话设置、Codex 目标状态和用户请求的回复格式。阅读时机：修改 `backend/src/acp.rs`、`new_session`、`configure_session`、`stream_prompt`、`user_request`，或接入新的 ACP 适配器。
- [配置](configuration.md)：编排器配置项（环境变量与 `config.toml`）、尝试次数与时间限制、传给 Agent 的变量、适配器路径。阅读时机：修改 `backend/src/config.rs`，调整超时、预算或尝试次数，修改 `spec_for` / `sandbox_env`，或改变监听地址、Node 路径。
