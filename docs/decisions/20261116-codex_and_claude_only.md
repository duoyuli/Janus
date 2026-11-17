# ADR：内置运行时只保留 Codex 和 Claude Code

- 状态：已采纳（accepted），记录于 2026-11-16。取代 [使用官方 ACP 适配器](20261115-official_acp_adapters.md) 中关于 Grok 和 Pi 的条目。
- 相关文档：[Agent、角色与工作区](../domains/20261031-agents_and_roles.md#providers)、[ACP 适配器管理](../architecture/20261031-adapters.md#catalog)、[配置](../interfaces/20261031-configuration.md#agent_env)。

## 背景

资源库要在会话启动时把角色选择的 Skill、MCP 和指令交给 Agent，不写任何配置文件。对照本机安装的适配器源码：Claude Code 的适配器把 `session/new` 的 `_meta.claudeCode.options` 原样交给 SDK，可以按会话加载本地插件；Codex 的适配器读取进程的 `CODEX_CONFIG` 和 `session/new` 的 `_meta.additionalRoots`。Pi 的适配器 `pi-acp` 启动 `pi` 时参数固定，无法传入 Skill；Grok 没有找到会话级的入口。用户决定先不支持 Pi 和 Grok，把 Codex 和 Claude Code 的完整流程做通。

## 决策

1. 删除 Grok 和 Pi 的 Agent 类型、适配器、账号登录、「复制本机配置」规则和文档。
2. Agent 的基础运行时用枚举 `Runtime`（`Codex`、`Claude`、`Custom`）表达，成员快照和启动参数都使用它，不再比较字符串。
3. 保留自定义命令：它可以接入任意 ACP Agent，能拿到 MCP，角色指令附在任务说明里。

## 取舍

- **少了两种 Agent**：以后加回时，要先确认它在会话启动时有注入 Skill 和指令的入口，否则角色选择的资源对它无效。
- **不做迁移**：已有的 Grok 和 Pi 连接仍会列出，但不能再检查或启动，只能删除；`~/.janus/adapters/` 中它们的适配器目录不再使用，可以手动删除。绑定它们的角色要换 Agent，成员中有它们的旧 Run 不能再继续。

## 后果

- `ADAPTERS` 只有 Codex 和 Claude Code 两项，适配器清单不再需要启动参数字段。
- 只有 Codex 的 ChatGPT 认证由 Janus 驱动登录（`account_login`）。
- 隔离目录变量只剩 `CODEX_HOME` 和 `CLAUDE_CONFIG_DIR`。
