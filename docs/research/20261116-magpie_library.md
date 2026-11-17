# magpie 的资源库（2026-11-16）

> 研究对象：[yetone/magpie](https://github.com/yetone/magpie) 的「资源库」（Library），提交 `c867a12`。
> 信息来源：仓库中的 `docs/subsystems/library.md`、`mcp-router.md`、`skill-router.md` 和 `internal/library` 的类型定义；另外对照了本机安装的 claude-agent-acp 0.107.0、codex-acp 2.6.0、pi-acp 0.0.34 的源码。只读阅读，没有运行 magpie。

## 摘要

magpie 用一个 `library.json` 统一管理指令、MCP 服务器、Skill、Hooks、环境变量、子代理、斜杠命令、Claude 插件和 Pi 包，每条资源勾选给哪些 Agent，再按每个 Agent 自己的格式写进它真实的配置文件。它的大部分复杂度来自「写进用户共享的配置」：记录写过什么、写之前备份、只删自己写的、不覆盖用户的值。

Janus 的前提不同：每个会话由 Janus 自己启动，可以在启动时把资源交给这一个会话。借鉴它的资源模型、来源与更新、凭据引用和界面结构，不借鉴写配置文件的同步方式。采纳的设计见 [ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](../decisions/20261116-launch_time_resources.md)。

## magpie 的做法

| 机制 | 做法 |
| --- | --- |
| 数据 | `Library` 保存各类资源，每条带勾选的 Agent 列表；`Applied` 按 Agent 记录 magpie 写入的内容和哈希，只删自己写的 |
| 一次修改 | 加锁、读取、改一处、保存、同步所有 Agent、再保存；页面和命令行的修改依次执行 |
| 同步 | 指令写在 Agent 指令文件的两行标记之间；MCP 按 Agent 的格式编码并与用户自己的字段合并；Skill 链接或复制进 Agent 的 Skill 目录；写之前备份，保留最近 30 次 |
| Skill 来源 | GitHub、任意 git 地址、本机目录、skills.sh 市场；记下提交和内容哈希，判断上游更新和本地修改；上游删除时只做标记，由用户清理；可选自动更新 |
| Git 拉取 | 用户自己的 `git`；地址放在 `--` 之后，拒绝以 `-` 开头和 `ext::` 等传输方式，`protocol.ext.allow=never`、`core.symlinks=false`、`GIT_TERMINAL_PROMPT=0`；带密码的地址直接拒绝 |
| MCP | stdio、http、sse；像 Agent 一样连接检测并列出工具；可以关闭单个工具；OAuth 登录和多个账号；`${NAME}` 按各 Agent 的写法翻译，读不了引用的 Agent 不给这个服务器 |
| 子代理 | 名字、描述、提示和它用到的资源库 Skill、MCP，按 Agent 的格式写成文件 |
| 项目 | 全局资源加减项目自己的资源，写进项目目录并用 `.git/info/exclude` 排除 |
| MCP 路由 | 每个 Agent 只拿到一个 `magpie` 服务器，由 magpie 运行背后的服务器；工具超过 10 个时改为 `find_tools` / `call_tool` 按需查找 |
| Skill 路由 | 每个 Agent 只拿到一个 `magpie-skills`，它的 `SKILL.md` 列出背后的 Skill 和路径，节省提示中的描述 |
| 界面 | 侧栏「资源库」按类型分组并显示数量：所有智能体（指令、记忆、MCP、技能、Hooks、环境变量、子代理、斜杠命令）、特定智能体（Claude 插件、Pi 包）和 magpie 自己的工具 |

## 各 Agent 在启动时能接收什么

| Agent | Skill | 指令 | MCP |
| --- | --- | --- | --- |
| Claude Code | `session/new` 的 `_meta.claudeCode.options.plugins` 加载本地插件，插件中的 Skill 显示为 `janus:<name>`（实测） | `_meta.systemPrompt.append` | `mcpServers`：stdio、http、sse |
| Codex | `_meta.additionalRoots` 中目录下的 `.agents/skills`（适配器调用 `skills/extraRoots/set`），显示为 `$<name>`（实测） | 进程环境变量 `CODEX_CONFIG` 的 `developer_instructions` | `mcpServers`：stdio、http，不支持 sse |
| Pi | `pi` 支持 `--skill`，但 pi-acp 启动 `pi` 的参数固定，只能用 `PI_ACP_PI_COMMAND` 换命令 | 同样需要命令行参数 | 只有 stdio |
| Grok | 没有找到会话级入口（未确认） | 未确认 | `mcpServers` |

Claude Code 的适配器还会原样转交 `options.agents`（子代理定义）、`options.settings`（内联设置）和 `options.disallowedTools`，以后加子代理、Hook 或按角色关闭工具时可以沿用同一个入口。

## 给 Janus 的建议

- **采纳**：统一入库；角色按引用选择；Skill 按内容寻址存放；Git 来源的安全做法；凭据只保存一份、值不进快照；界面按类型分组显示数量。
- **改变做法**：不写 Agent 的配置文件，资源在会话启动时注入，见上表。Pi 和 Grok 没有入口，暂不支持，见 [ADR：内置运行时只保留 Codex 和 Claude Code](../decisions/20261116-codex_and_claude_only.md)。
- **以后考虑**：MCP 检测和工具列表、按角色关闭工具（Claude Code 可用 `disallowedTools`）、资源套装、版本固定、子代理、MCP 路由（多个协作者共用一个 MCP 进程，并按需查找工具）。
