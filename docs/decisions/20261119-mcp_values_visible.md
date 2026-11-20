# ADR：MCP 服务器的值在编辑器里能看能改

- 状态：已采纳（accepted），记录于 2026-11-19。
- 相关文档：[安全边界](../operations/20261031-security.md#secrets)、[资源库](../domains/20261116-resource_library.md#mcp)、[HTTP API](../interfaces/20261031-http_api.md#library)、[ADR：每个 Agent 使用独立的配置目录](20261031-isolated_agent_homes.md)。

## 背景

资源库 MCP 服务器的环境变量和 HTTP header 值保存在 `~/.janus/secrets/library/<资源 id>.json`（`0600`），此前「只能写入」：接口只返回变量名和「是否已设置」，编辑器里显示「已设置，留空保留」。用户参考 magpie 改版 MCP 编辑器后，要求像 magpie 一样在表单和 JSON 里直接看到、修改这些值；否则 JSON 视图里的令牌总是空字符串，看起来像被清掉了。`AGENTS.md` 的约定「凭据不写进……设置接口的返回值」因此要开一个口子。按合并前检查第 5 条，这扩大了信任边界，单独记录。

## 决策

1. 新增一个只读接口 `GET /api/library/mcp/{id}/values`，返回这个服务器声明的变量当前的值：`{"values": {"NAME": "value"}}`。应用只在打开 MCP 编辑器时调用它。
2. 这个接口只回答本机的应用（`from_local_app`）：`Host` 必须是 `127.0.0.1`、`localhost` 或 `[::1]`，请求不能带 `Origin` 或 `Sec-Fetch-Site`。浏览器里的网页发出的请求会带这两个头；把自己的域名改绑到 `127.0.0.1` 的网页（DNS rebinding）发出的 `Host` 是它自己的域名。三种情况都返回 403。
3. 其余地方不变：`GET /api/library`、`GET /api/library/items/{id}`、Run 快照、事件、导出和日志仍然只有变量名和「是否已设置」。值仍然只存在 `secrets/library/` 的 `0600` 文件里，不进 SQLite。Agent 的凭据（`agents/<id>/secrets.json`）仍然不经任何接口返回。
4. 保存时值按原样提交：给出字符串就写入，空字符串清除这个值；没有 `value` 字段时保留已保存的值，供不读取值的客户端使用。

## 取舍

- **只为 MCP 开口**：MCP 的值是用户在资源库里自己填写或粘贴的配置，编辑时需要对照着改；Agent 的凭据来自登录或复制本机配置，用户不需要在 Janus 里查看。
- **单独的接口而不是在资源详情里带上值**：列表、详情、快照这些读取路径多、会被缓存和记录，值只走一条用途单一、可以单独加限制的路径。
- **同一用户的其他进程**：它们本来就能读 `0600` 的值文件和 SQLite，这个接口没有给它们新的能力。HTTP 服务仍然没有鉴权，见[网络暴露](../operations/20261031-security.md#network_exposure)。
- **非回环地址**：把 `JANUS_ADDR` 改成非回环地址时，`Host` 检查使接口拒绝请求，应用打不开已有 MCP 服务器的编辑器；这是有意的，值不经网络发出。

## 后果

- `AGENTS.md` 的约定改为：凭据不写进代码、日志、文档、SQLite 或设置接口的返回值，资源库 MCP 服务器的值经本 ADR 的接口返回给本机应用除外。
- 安全文档的「凭据保存」一节、资源库和 HTTP API 文档同步更新。
- 应用的 MCP 编辑器在表单和 JSON 里都显示值，清空一个值保存后就删除它。
