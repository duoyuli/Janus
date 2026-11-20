# ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入

- 状态：已采纳（accepted），记录于 2026-11-16。取代角色上的 Janus Skill 库、GitHub Skill 来源、MCP 来源和角色自己的手工 MCP。
- 修订：2026-11-19 起，资源默认给所有数字员工、经 MCP 路由和 Skill 路由提供，见 [ADR：资源库默认给所有数字员工](20261119-library_routing.md)；2026-11-20 起，每项资源都给所有数字员工，角色不再选资源，MCP 只经 `janus-tools`、Skill 只经 `janus-skills`，见 [ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](20261120-library_routed_only.md)。会话启动时注入、不写 Agent 配置文件的原则不变。
- 相关文档：[Agent、角色与工作区](../domains/20261031-agents_and_roles.md)、[ACP 集成](../interfaces/20261031-acp_integration.md)、[安全边界](../operations/20261031-security.md)、[研究：magpie 的资源库](../research/20261116-magpie_library.md)。

## 背景

角色能用的 Skill 和 MCP 原来分散在四处：Janus 自建的 Skill、GitHub Skill 来源、MCP 来源和角色自己填写的 MCP。资源 id 写成 `<来源 id>/<key>`，归属靠 id 的形状判断。Skill 以「名字、描述、`SKILL.md` 路径」的形式写进任务说明，由 Agent 自己决定是否读取，并没有用上 Agent 原生的 Skill 机制。

magpie 的资源库把指令、MCP、Skill、子代理等统一入库，再按每个 Agent 的格式写进它真实的配置文件（`~/.claude/settings.json`、`~/.codex/config.toml`、`~/.claude/skills`）。为此它要记录自己写过什么、写之前备份、只删自己写的。Janus 的前提不同：每个会话由 Janus 自己启动，一个会话一个子进程，所以资源可以直接交给这一个会话。用户明确要求：这些配置不写进 Agent 的配置文件，在 ACP 启动时注入。

对照本机安装的适配器（claude-agent-acp 0.107.0、codex-acp 2.6.0）实测：Claude Code 通过 `_meta.claudeCode.options.plugins` 加载的本地插件中的 Skill 出现在会话的命令列表中（`janus:<name>`），配置目录里没有留下安装记录；Codex 通过 `_meta.additionalRoots` 读取 `<目录>/.agents/skills`，Skill 出现在命令列表中（`$<name>`）。

## 决策

1. **统一入库**：Skill 和 MCP 服务器都是资源库中的资源。资源 id 不带含义；类型（`Skill`、`Mcp`）、来源（编辑器新建、从本机目录复制、Git 来源）和版本都是字段。同一类资源的名字唯一，名字就是 Agent 看到的名字；`janus` 保留给看板 MCP。
2. **角色只引用**：角色的 `resources` 是资源 id 的列表，不复制资源；角色不再有自己的 MCP。
3. **内容寻址的 Skill**：Skill 目录按内容的 sha256 存放，编辑或从来源更新时生成新版本，旧内容不被改写。
4. **Run 固定资源**：Run 创建时，成员快照记下每项资源的名字、版本和 MCP 定义（不含值），Skill 复制到这个成员的资源包 `runs/<run>/members/<member>/bundle/`。之后资源库的修改不影响这个 Run。
5. **只在启动时注入，不写配置文件**：资源只通过启动进程的环境变量，以及 `session/new`、`session/load` 的 `mcpServers` 和 `_meta` 交给 Agent。用户的 `~/.claude`、`~/.codex` 和 Janus 隔离目录中的 Agent 配置都不写。
   - Claude Code：资源包是一个本地插件，经 `_meta.claudeCode.options.plugins` 加载；系统层文字经 `_meta.systemPrompt.append`。
   - Codex：资源包经 `_meta.additionalRoots` 传入，Codex 读取其中的 `.agents/skills`；系统层文字经进程环境变量 `CODEX_CONFIG` 的 `developer_instructions`。
   - MCP 都经 `mcpServers`，环境变量和 HTTP header 的值在打开会话时从 `secrets/library/` 读取。
   - 自定义命令的 Agent 没有加载 Skill 的入口：它只拿到 MCP，角色上的 Skill 对它标为不支持。
6. **系统层只放长期有效的内容**：Janus 对这个会话的说明（负责人说明，或协作者的协作规则）在前，用户的长期要求在后。恢复会话（`session/load`）时传入与打开时相同的参数。
7. **例外**：Claude Code 负责人运行 `/goal` 前，工作区信任仍写入隔离目录中的 `.claude.json`。SDK 没有在启动时声明信任的入口，这是 Janus 唯一写入 Agent 配置的地方。

## 取舍

- **与写入 Agent 配置相比**：不需要记录写入归属、备份和合并用户的值；同一个 Agent 被不同角色使用时互不影响。代价是只有在启动时有入口的 Agent 能用资源库。
- **每个 Run 复制一份 Skill**：磁盘占用随 Run 增加；换来 Run 自带资源，恢复和删除 Run 都不需要追踪引用。
- **Codex 的 `additionalRoots` 同时扩大会话的文件访问范围**：资源包是这个成员自己的副本，Agent 改了它也不影响资源库和其他成员。
- **Skill 名在 Agent 中带前缀**：Claude Code 显示为 `janus:<name>`，Codex 显示为 `$<name>`。
- **凭据在打开会话时读取**：值的修改会到达恢复的会话；资源被删除后，旧 Run 恢复时拿不到它的值。

## 后果

- 后端新增 `library` 模块：资源、内容存储、凭据、Git 来源和资源包。`skill_sources`、`mcp_sources` 和角色的 `skills`、`mcp`、`mcpRefs` 字段删除，旧数据不迁移。
- 任务说明不再列出 Skill 和 MCP，也不再列出工作目录中的项目 Skill：Agent 按自己的规则读取工作目录中的 `.claude/skills`、`.agents/skills`。
- Codex 负责人的说明进入开发者指令，不再单独发一轮「背景信息」；协作者的协作规则从任务说明移到系统层。
