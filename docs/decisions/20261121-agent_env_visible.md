# ADR：Agent 的环境变量在编辑器里全部显示，令牌也在

- 状态：已采纳（accepted），记录于 2026-11-21。取代 [ADR：Claude Code 的自定义 API 用环境变量设置](20261120-claude_env_auth.md) 中「编辑时私有变量只显示名字」的部分。第 2 条「只回答本机应用」已被 [ADR：MCP 的值和 Agent 的环境变量也给配对的设备](20261126-credentials_to_paired_devices.md) 取代：2026-11-26 起本机应用和配对的设备都能读。
- 相关文档：[安全边界：凭据保存](../operations/20261031-security.md#secrets)、[Agent 环境变量](../domains/20261031-agents_and_roles.md#agent_env)、[HTTP API](../interfaces/20261031-http_api.md#setup)、[ADR：MCP 服务器的值在编辑器里能看能改](20261119-mcp_values_visible.md)。

## 背景

Claude 的「自定义 API」是一个环境变量输入框，令牌这类私有值保存在 Agent 的 `0600` 凭据文件里，编辑时只显示变量名，要改就得整行重写。用户指出 Janus 是本地优先的工具，自己的令牌放在自己的机器上，编辑器应当把所有变量连同值都显示出来，密钥就放在这个框里。`AGENTS.md` 的约定「凭据不写进……设置接口的返回值」此前只为资源库 MCP 的值开了口子，这是第二个。按合并前检查第 5 条，这扩大了信任边界，单独记录。

## 决策

1. 新增只读接口 `GET /api/connections/{id}/env`，返回 Agent 环境变量的全部值：`{"env": {"NAME": "value"}}`，私有值从凭据文件读出（`connection_env`）。应用打开 Agent 编辑器时调用它，输入框显示完整的 `KEY=value`。
2. 它和 MCP 的值用同一个限制（`from_local_app`）：`Host` 必须是回环地址，带 `Origin` 或 `Sec-Fetch-Site` 的请求返回 403，网页和改绑到 `127.0.0.1` 的域名都读不到。
3. 存放不变：私有值仍然只在 `data/secrets/agents/<id>.json`（`0600`），SQLite、`GET /api/setup`、快照、事件和日志里仍然只有名字（`env` 中为 `null`）。保存时按输入框的文本处理，私有值照旧移进凭据文件。
4. 编辑器读不到值时（例如服务监听在非回环地址），照旧显示名字，名字单独一行表示沿用已保存的值。

## 取舍

- **为什么不干脆把值存进设置对象**：设置对象会被列表、快照和导出读到，值只走一条用途单一、能单独限制的路径，和 MCP 的值一致。
- **同一用户的其他进程**：它们本来就能读 `0600` 的凭据文件，这个接口没有给它们新的能力；HTTP 服务仍然没有鉴权，见[网络暴露](../operations/20261031-security.md#network_exposure)。（注：2026-11-26 起有设备鉴权，本机应用以外的客户端要先扫码配对，见 [ADR：手机在局域网里扫码配对后访问 Janus](20261126-lan_access_pairing.md)。）
- **屏幕上可见**：令牌会显示在编辑器里，被旁人看到或截屏的风险由用户自己掌握；这是本地优先工具的常见做法。

## 后果

- `AGENTS.md` 的约定改为两个例外：资源库 MCP 服务器的值和 Agent 的环境变量，都只经各自的接口返回给本机应用。
- Codex 的「自定义 API」的 API Key 仍是单独的密码框，不经这个接口返回；它不是环境变量。
