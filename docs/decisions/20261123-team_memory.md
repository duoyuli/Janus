# ADR：工作区的团队记忆由 Agent 写、记来源、定期整理

- 状态：已采纳（accepted），记录于 2026-11-23。
- 相关文档：[团队记忆](../domains/20261123-team_memory.md)、[工作区共享资料](../domains/20261031-agents_and_roles.md#workspace_brief)、[看板 MCP 工具：团队记忆的工具](../interfaces/20261031-board_mcp.md#memory_tools)、[HTTP API：团队记忆](../interfaces/20261031-http_api.md#memories)、[安全边界：团队记忆](../operations/20261031-security.md#team_memory)、[ADR：工作区值班派发独立的 Run](20261121-workspace_loops.md)。

## 背景

GitHub issue #4 提出：一个工作区里的 Agent 团队本该复用记忆，现在没有这样的机制。共享资料只有用户能写，Agent 只能在交付时「建议」；Agent 在 Run 里摸清的东西（怎么跑测试、一个坑、为什么这么做），用户不采纳，下一个 Run 就忘了。Agent 自带的记忆在 Janus 里也用不上：Claude Code 的 auto memory 按工作目录分，而每个 Run、每个对话都有自己的目录；Codex 的 memories 默认关闭，两种 Agent 之间也不共享。

调研了业内做法：Claude Code 的 auto memory（`MEMORY.md` 索引加主题文件，Auto Dream 在会话间隙合并、删除被推翻的、相对日期改绝对日期）；Codex 把必须遵守的放 `AGENTS.md`、memories 只当回忆层；Anthropic 的 memory tool（目录加 view、create、str_replace 等命令）；Letta、CrewAI、LangGraph 的共享记忆块和命名空间，Letta 的 sleep-time 整理；Mem0 的 ADD / UPDATE / DELETE / NOOP；Windsurf、Devin 的 memories 和 rules；以及 MINJA 等记忆投毒研究，OWASP 列为 ASI06，建议按来源而不是内容判断可信度。方案贴在 issue 上，用户 2026-11-23 回复按推荐的做。

## 决策

1. **两层**：共享资料不变，仍只有用户写，必须遵守；下面加一层团队记忆，由 Agent 写，是可以被推翻的笔记，用之前先核对。
2. **Agent 直接写，写入即生效**：负责人和协作者、值班都能用 `janus.remember` 写或更新笔记，不等用户批准；用户事后在工作区页和收件箱审查，可以改、删，或把一条加进共享资料。
3. **每条记来源**：最后写它的 Run、成员和时间；被取消的 Run 写的笔记不给之后的 Run。外部内容只留链接，像凭据的内容拒绝写入。
4. **索引常驻、正文按需**：每个会话的系统层只放一行一条的索引（最多 200 行），正文用 `janus.recall` 读。
5. **只有整理能删**：整理是工作区负责人的一个单独会话（`kind` 为 `dream` 的 Run），只有记忆工具，一轮结束；自上次以来交付了 5 个 Run 且距上次超过 24 小时时自动开始，也可以手动开始。结果作为一条收件箱消息给用户。
6. **范围是工作区**：笔记不跨工作区，对话没有团队记忆；跨工作区的个人记忆以后再说。
7. **关掉 Claude Code 自带的 auto memory**：启动时设置 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`，团队的知识只放在一处。Codex 的 memories 保持关闭。

## 取舍

- **存在 Janus 的 SQLite 里，经工具读写，而不是一个让 Agent 直接改的目录**：工具调用能校验工作区、记来源、拒绝凭据、按角色限制删除，并记成事件；直接写文件做不到这些，还要把一个 Janus home 里的目录开放给 Agent 的沙箱。
- **不放进共享资料**：共享资料会进入每个会话、被当作事实，[只让用户写入](20261031-agents_and_roles.md#workspace_brief)的理由没有变；团队记忆明确是可以推翻的笔记，用户把认可的升级进共享资料。
- **不让 Agent 逐条等批准**：每条都等用户会让记忆积累不起来；靠来源、只能经工具写、整理和用户事后审查来控制错误的笔记。
- **整理在空目录里，不看代码**：Claude 的 dream 会对照代码找过时的记忆，但那要把整理会话放进工作区（或一个副本）里，按负责人的权限它就能改文件；先只凭笔记和交付摘要整理，写笔记的会话本身被要求用之前先核对代码。
- **不做**：向量检索、Mem0 或 Zep 这类外部记忆服务、同步 Agent 自带的记忆目录、按角色的私有记忆、重要度和衰减打分、跨工作区的记忆。

## 后果

- 每个工作区会话的系统层多了团队记忆一节和两个工具（整理会话三个）；看板工具的数量从 10 个变成 12 个，协作者从 4 个变成 6 个。
- 新增 `memories` 表（数据版本 22）和 Run 类型 `dream`；全局事件流多了 `memory` 通知；收件箱多了整理的结果。
- Agent 写的内容会进入之后的每个会话，这扩大了信任边界，见[安全边界：团队记忆](../operations/20261031-security.md#team_memory)。
- Claude Code 在 Janus 里不再有自己的 auto memory；用户自己在 Janus 之外用 Claude Code 不受影响（Janus 只改它启动的进程的环境变量）。
