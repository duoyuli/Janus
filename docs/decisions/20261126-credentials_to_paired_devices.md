# ADR：MCP 的值和 Agent 的环境变量也给配对的设备

- 状态：已采纳（accepted），记录于 2026-11-26，用户在 2026-11-26 定下。扩大了信任边界（合并前检查第 5 条）。它取代下面两份 ADR 中「只返回给本机应用」的部分：
  - [ADR：MCP 服务器的值在编辑器里能看能改](20261119-mcp_values_visible.md)的第 2 条；
  - [ADR：Agent 的环境变量在编辑器里全部显示](20261121-agent_env_visible.md)的第 2 条。
- 相关文档：
  - [安全边界：凭据保存](../operations/20261031-security.md#secrets)、[安全边界：配对的设备](../operations/20261031-security.md#paired_devices)；
  - [ADR：手机在局域网里扫码配对后访问 Janus](20261126-lan_access_pairing.md)；
  - [HTTP API](../interfaces/20261031-http_api.md#devices)。

## 背景

Web 版要和 Mac 应用对齐，MCP 编辑器和 Agent 编辑器也要能在 Web 和手机上用。可是这两个编辑器读值的接口只回答本机应用。用户决定，凭据仍由服务端（Rust）保管，Mac 应用和配对过的设备都能读。

## 决策

1. `GET /api/library/mcp/{id}/values` 和 `GET /api/connections/{id}/env` 对本机应用和已配对、有效的设备都返回值。
2. 这两个接口不另写判断。所有 `/api/*` 请求都先经过设备鉴权（`devices::guard`），只放行两种请求：本机应用的请求，和带有效 `janus_device` cookie 的请求。所以下面这些请求读不到值，都返回 401：
   - 没有配对的浏览器发来的；
   - 带伪造 cookie 的，或带已吊销 cookie 的；
   - 局域网上没有配对的设备发来的；
   - 把自己的域名指向本机的页面发来的（DNS 重绑定）：它拿不到 Janus 地址下的 cookie。
3. 存放不变：值仍只在 `data/secrets/` 的 `0600` 文件里，不进 SQLite、日志和事件，也不进其他接口的返回值。Codex 的 API Key、Claude 订阅的 OAuth Token 和复制来的凭据变量，仍然不经任何接口返回。
4. 下面这些仍只给本机应用，配对的设备会收到 403：本机环境的读写和「试命令」、取配对码、拨手机访问的开关。它们不是凭据，前者列出并决定 Agent 能用到用户目录的哪些内容，后两者决定谁能进来。

## 取舍

明确接受下面两项新增的风险：

- **凭据值随配对的设备在网络上传输**：手机或浏览器经局域网打开这两个编辑器时，值走 HTTP。前提见[手机访问的 ADR](20261126-lan_access_pairing.md)：只在用户自己的局域网和 tailnet 里使用，不直接暴露在外网，也不做端口转发。在局域网里，值是明文；经 Tailscale 时，由 WireGuard 加密。
- **被盗的设备 cookie 能读到全部凭据**：拿到某台设备 cookie 的人，能读到每个 MCP 服务器的值和每个 Agent 的环境变量，和用户在 Mac 上能看到的一样多。

对策：

- 手机访问默认关闭。
- 在外面用 Tailscale 或 VPN 连回来。
- 设备丢了或怀疑泄露时，在 Mac 上取消那台设备的配对，令牌立即失效。
- 换掉泄露的凭据：这一步本来就该做。

## 后果

- `AGENTS.md` 的凭据约定改为两个例外，都只返回给本机应用和已配对的设备。
- Web 版的 MCP 编辑器和 Agent 编辑器可以和 Mac 上一样显示、修改值。
