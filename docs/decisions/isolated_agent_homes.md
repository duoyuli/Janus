# ADR：每个 Agent 使用独立的配置目录

- 状态：已采纳（accepted），记录于 2026-10-30，决策早于本记录。
- 相关文档：[安全边界](../operations/security.md#agent_isolation)、[配置](../interfaces/configuration.md#agent_env)、[Agent、角色与工作区](../domains/agents_and_roles.md#providers)。

## 背景

Janus 同时运行多个编码 Agent，它们各自有配置目录、凭据、指令文件、hooks、插件和 MCP 设置。如果直接使用用户自己的 `~/.codex`、`~/.claude` 等目录，Janus 的运行会受用户个人配置影响，也可能反过来改动用户的配置。用户明确要求：复制本机配置时不能改动用户自己的 Agent 配置；Claude Code 的工作区信任只能写在 Janus 为它创建的配置里。

## 决策

1. 每个 Agent 在 `~/.janus/agents/<id>/` 下有自己的配置目录（`home/`）、HOME（`root/`）和临时目录（`tmp/`），通过各运行时的配置目录变量交给 Agent。
2. 子进程环境由 Janus 完整构造，只从宿主继承少量变量；用户填写的变量不能覆盖 Janus 控制的变量。
3. 「复制本机配置」只复制模型接入相关的配置项和凭据，其余一律不复制。
4. 凭据单独保存为 `0600` 文件，不进入 SQLite，接口不返回值。
5. Janus 需要为 Agent 写入的设置（例如 Claude Code 主控的工作区信任）只写入该 Agent 的隔离配置。

## 取舍

- **与直接使用用户配置相比**：行为可预测，不会污染用户环境；代价是用户需要为 Janus 单独登录或复制一次配置，个人的指令文件、插件和 MCP 不会自动生效，需要在角色中重新配置。
- **不是沙箱**：这只隔离配置和凭据，不限制 Agent 访问文件系统和网络。

## 后果

- 同一种 Agent 可以配置多个实例（例如不同账号、不同厂商的 Claude Code 预设），互不影响。
- 新增运行时类型时，需要确定它的配置目录变量和需要保留的配置项。
