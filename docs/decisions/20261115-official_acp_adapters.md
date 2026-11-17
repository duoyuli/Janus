# ADR：使用官方 ACP 适配器

- 状态：已采纳（accepted），记录于 2026-11-15。其中 Grok 和 Pi 的条目已被 [内置运行时只保留 Codex 和 Claude Code](20261116-codex_and_claude_only.md) 取代。
- 相关文档：[ACP 适配器管理](../architecture/20261031-adapters.md#catalog)、[ACP 集成](../interfaces/20261031-acp_integration.md)、[安全边界](../operations/20261031-security.md#adapter_install)、[需要负责人处理的看板消息推送进负责人的会话](20261102-board_push.md)。

## 背景

Codex、Claude Code 和 Grok 的适配器此前从 GitHub 仓库的默认分支构建：npm 上的版本没有插话，仓库版本支持插话，但需要安装依赖、构建和按提交存放。2026-11 起，ACP 项目在 `@agentclientprotocol` 下发布了 Codex 和 Claude Code 的官方适配器，xAI 也在 `@xai-official/grok` 中提供 `agent stdio`；它们自带对应的 CLI，支持官方约定的 `_session/steering`、`_session/goal` 和 AIR 目标状态。Pi 的适配器改用 ACP registry 中的 `pi-acp`。

## 决策

1. Codex 使用 npm `@agentclientprotocol/codex-acp`，Claude Code 使用 `@agentclientprotocol/claude-agent-acp`，Grok 使用 `@xai-official/grok`（启动参数 `agent stdio`）。
2. Pi 使用 npm `pi-acp`。它启动本机的 `pi` 命令；不会把 ACP 的 MCP 服务器转发给 pi，所以 Pi 会话看不到看板工具，只适合不依赖看板的用途。
3. npm 安装统一带 `--ignore-scripts`，安装和升级不执行包自带的生命周期脚本。
4. 不再要求用户安装 `codex`、`claude` 或 `grok`：官方包自带 CLI；Agent 设置中填写的二进制路径作为覆盖传给适配器。
5. 插话改用 `_session/steering`（能力 `_meta.steering.supported`，空闲时 `idleBehavior: "promptRequired"`），清除目标改用 `_session/goal`，目标状态读 `_meta.jetbrains.air.goal`。Janus 在 `initialize` 中声明 AIR 客户端。
6. 适配器版本目录记录包名。（2026-11-15：替换旧包安装版本的兼容逻辑已删除，安装过的版本就是当前的包。）

## 取舍

- **自带 CLI 的包更大**：每个官方适配器都带 Codex、Claude Code 或 Grok 的二进制，适配器目录比原来的纯 JS 包大；换来的是用户不需要另装 CLI，版本也和适配器匹配。
- **信任转移到 npm**：Codex、Claude Code 和 Grok 的包由官方 scope 发布，Pi 使用 ACP registry 中的 `pi-acp`。Janus 仍不校验签名；`--ignore-scripts` 去掉安装期执行包脚本这一项。
- **Pi 没有看板工具**：`pi-acp` 接受 `mcpServers` 但不把它接到 pi，Pi 会话无法调用 `janus.*`。需要看板协作时使用其他内置运行时或自定义 ACP 命令。
- **AIR 元数据改变工具调用报告**：声明 AIR 后，官方适配器按 AIR 的约定报告工具调用，标准字段仍在，但会多出 `_meta` 字段；目标状态依赖这一声明。

## 后果

- `ADAPTERS` 全部是 npm 来源；npm 安装禁用安装脚本。
- 适配器安装不再需要用户安装 Codex、Claude Code 或 Grok 的 CLI；Pi 仍需本机的 `pi` 命令。
- Codex 和 Claude Code 的插话、目标状态和看板 MCP 通过官方扩展工作。
- 配置检查和登录使用适配器自带的 CLI：Codex 通过 `codex-acp cli login`，Grok 通过 `grok login --device-auth`。
