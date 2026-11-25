# 2026-11-25 多 Agent 协作与 Agent Team 进展观察

> 覆盖窗口：2026-11-24 18:50 UTC 至 2026-11-25 18:15 UTC（本地时间 2026-11-24 12:50 至 2026-11-25 12:15，UTC-6）。
> 信息来源：厂商 release notes 和官方文档，GitHub release、提交和 PR（`gh api`），MCP 与 ACP 仓库，arXiv 导出 API，Hacker News（Algolia API），npm registry，各产品 changelog。所有链接的访问日期为 2026-11-25。
> 抓取限制：arXiv 的 cs.MA 导出 API 在本窗口只返回 10 篇 2026-11-24 的论文，2026-11-25 返回 0 篇（提交尚未进入索引，**U**）；Claude Code 和 MCP 的文档页超过单次抓取上限，分两段读取，Skills 一节完整读到；Antigravity 的 changelog 首页重定向到 `/docs/changelog/`；没有 hands-on 运行任何产品；没有可用的 X 会话，也没有检索中文媒体，本报告不含 X 来源。
> 证据标记：**F** 一手来源明确写出；**I** 由多个事实推出、未直接验证；**J** 本文判断；**U** 只有二手来源、页面异常或无法核实。
> 与昨日报告的关系：昨日有两份报告，[《2026-11-24 多 Agent 协作与 Agent Team 进展观察》](20261124-multi_agent_collaboration.md) 和[《2026-11-24 MCP 扩展进展与 Janus MCP 路由对照》](20261124-mcp_skills_tasks_elicitation.md)。本报告不重复它们的条目，只写状态变化和本日新增；对昨日建议的逐条现状判断见[相对昨日报告的增量](#increment)。

## 导航

- [摘要](#summary)：只想知道结论时阅读。
- [方法与限制](#method)：判断结论可信度时阅读。
- [相对昨日报告的增量](#increment)：想知道昨天哪些结论被改变、昨日建议现在处于什么状态时阅读。
- [主窗口观察](#observations)：查 2026-11-24 至 11-25 的事实和判断时阅读。
- [对 Janus 的建议](#suggestions)：评估 P0 / P1 / P2 时阅读。
- [待验证问题](#open_questions)：安排后续验证时阅读。
- [待用户决定](#decisions)：做产品取舍时阅读。
- [来源](#sources)：核对出处时阅读。
- [Janus 现状索引](#janus_references)：核对报告引用的代码和文档位置时阅读。

<a id="summary"></a>
## 摘要

- **本日最重要的一条是 Claude Code 2.1.332 把 MCP 服务器的 Skill 变成宿主能力。** release notes 第一条是「Added skills from MCP servers (beta). They show as `server:skill` in the `/` menu; other skills keep their names」（[v2.1.332](https://github.com/anthropics/claude-code/releases/tag/v2.1.332)，2026-11-25T01:32Z，**F**）。这已经不只是昨天的公告：Claude Code 文档给出了完整的加载规则（服务器必须声明 `resources` 能力和 `io.modelcontextprotocol/skills` 扩展、协议 `2026-07-28` 以上、客户端校验文件大小和 SHA-256、批准绑定到 manifest 版本），MCP 官方客户矩阵在发布前约四小时把 Claude 与 Claude Code 的 Skills 列从空改成支持（[矩阵提交](https://github.com/modelcontextprotocol/modelcontextprotocol/commit/3c8eb0c4)，2026-11-24T21:48Z，**F**）。
- **但 Janus 的 Agent 现在拿不到这个能力，有两条独立的阻断。** 第一，`janus-tools` 路由的 `initialize` 只声明 `tools`，不声明 `resources`，协议最高 `2025-11-25`（`backend/src/orchestrator/mcp_server.rs` 的 `initialize`、`backend/src/orchestrator/mcp_client.rs` 的 `VERSIONS`）；Skills 扩展要求 `resources` 和 `2026-07-28` 以上。第二，Janus 的 Claude 连接跑在 `@agentclientprotocol/claude-agent-acp` 上，它基于 Claude Agent SDK（npm 依赖 `@anthropic-ai/claude-agent-sdk` 0.3.331），而 Claude Code 文档明确把 **Agent SDK 会话列为不支持 MCP skills 的场景**（**F** 文档与依赖，结论 **I**）。所以昨天「Skills over MCP 先写 ADR、不自动复制」的判断被加强，不是被改变。
- **协议层本窗口没有新版本，只有状态变化。** ACP 最新仍是 11-23 的 schema 1.35.0 / 2.0.0-beta.4，窗口内提交都是把客户端和 Agent 加进 registry 的文档改动；MCP 最新仍是 `2026-07-28`，窗口内提交是文档修正、MCP 两周年博客和「把 Claude / Claude Code 的 Skills 支持标进矩阵」。
- **研究侧有三篇值得读的论文，结论都指向「机制和证据，而不是更多 Agent」。** 《Recursive Collective Intelligence》用等预算阶梯说明多 Agent 只有在「转发信息」时才超过同等算力的 pass@n（27B 上约三分之一），把组织写进提示能用不到一半 token 达到最高一档；《TimeTravelContainer》用无 root 的 CoW 实时 fork 整个运行中的容器（84.1 倍克隆加速）；《Provenance-Isolated Epistemic Quorums》把 LLM 排除在可信计算基之外，主张「模型之间的一致不是物理证据」。
- **社区侧最强的一条是 Paradigm 3 的《Swarms do not yet raise the roof》。** 1,100+ 次运行、80+ 队形：8 人队约 6 倍快，但同等预算下质量低约 7%，70 次比较里输 54 次；16/32 人不比 8 人好；32M token 下 8 人队超过单 Agent，但「单 Agent 分 8 个新会话重启」又以 97.0% 对 93.2% 胜回。它对「更多 Agent 更好」是强反证，支持 Janus 的 3 到 5 人团队和小规模并行，不支持扩大团队。
- **昨日四项未决事项今天都没有变成「已批准」。** P0-1（受保护路径）没有新证据，判断不变；P1-1（编排时提醒独立验证）被 20261125 的「只注入协作规则」ADR **收窄**：作为「陈述事实」的机制可以存在，作为「要求负责人安排验证」的做法不该由 Janus 规定；第二份报告的 P1-1（资源库显示只能以任务方式调用的工具数）仍成立；D1（Skills over MCP）被今天的两条阻断加强为「不引入」。

<a id="method"></a>
## 方法与限制

- **检索方式**：WebFetch 读官方页面和文档，`gh api` 读 release、提交和 PR，arXiv 导出 API 按提交日期取 cs.MA，Hacker News Algolia API 按时间过滤，`npm view` 和 `npm pack` 核对适配器依赖。
- **主窗口**：2026-11-24 18:50 UTC 至 2026-11-25 18:15 UTC。发布日期早于 11-24 18:50 的内容只出现在「补充」或「窗口外线索」里，并注明日期。
- **抓取限制**：arXiv 11-25 的查询返回 0 条，不能据此断言当天没有论文（**U**）；HN 的分数只说明有帖子，不代表共识；Paradigm 3 和 Hugging Face 的实验都是机构自报，没有独立复现；Claude Code 的 MCP skills 是 beta，行为可能在写完后变化。
- **没有做的事**：没有购买或 hands-on 运行 Claude Code、Codex、Antigravity、Hub、PyBot；没有实测 Janus 路由在 `2026-07-28` 客户端下的表现；没有访谈。
- **日期口径**：GitHub release 的时间戳是 UTC，正文按页面显示的日期写。本地时间按运行环境的 UTC-6 标注。
- **Janus 现状**：按本仓库提交 `e9d9d71`（2026-11-25 11:49 -0600）的代码和文档核对；行号会随提交变化，引用尽量给 `文件:函数` 或 `文件:行`，并以这个提交为准。

<a id="increment"></a>
## 相对昨日报告的增量

**重叠范围。** 昨日两份报告已覆盖 Claude Ship、Antigravity 2.35.0、Codex 0.172、ACP 1.35、REPA、Uber 的 Skill 规模数据、Agent Hive、TalkTo，以及 Skills over MCP 公告、Tasks、Elicitation、SEP-2640 / 2663、`2026-07-28`、MCP 两周年、Preflight 和 Selective Verification。本报告不重复这些条目；它们今天没有新状态时也不再提，有状态变化的只写在下面的表和观察里。

### 昨日事实的更新

| 昨日报告引用的事实或结论 | 今天的状态 | 依据 |
| --- | --- | --- |
| Skills over MCP 在 Claude 和 Claude Code 公开测试；Claude Code 2.1.331 的 release notes 没有 MCP skills 条目 | 2.1.332 正式在 release notes 里写出 MCP 服务器 Skill，`/` 菜单显示 `server:skill`；Claude Code 文档写明需要实现 Skills 扩展、v2.1.331 以上 | **F**：v2.1.332 release notes、`mcp.md#use-skills-from-mcp-servers` |
| Codex 0.174 alpha 只有状态更新 | 继续：alpha.1 至 alpha.5 在 11-24 01:46Z 至 11-25 01:11Z 之间发布，body 只有版本名；没有 0.173.0 或 0.174.0 的正式版（`rust-v0.173.0`、`rust-v0.174.0` 均 404） | **F**：GitHub releases；**U**：内容 |
| ACP 1.35.0 / 2.0.0-beta.4 的 `mcp/message` 归因字段未实现 | 窗口内没有新 release；提交只有 registry 和客户端列表的文档改动；归因字段仍没有实现提交 | **F**：ACP releases 与 commits |
| MCP 的 Skills 扩展在公开测试 | 同一窗口内 MCP 仓库把 Claude（web）和 Claude Code 的 Skills 支持标进客户矩阵；扩展规范仍是 beta，SEP-2640 为 Final | **F**：提交 `3c8eb0c4`、扩展概览页 |
| Janus 的验证者提示没有多视角要求（`backend/src/teamwork.rs`） | 验证者提示已移到 `teamwork::task_section`，只写「逐条核对、写明看到什么、给出 verdict」，不再提测试和代码；`teamwork::lead_guide` 已删除 | **F**：`backend/src/teamwork.rs:262` 附近、`docs/decisions/20261125-collaboration_only_injection.md` |
| 会话续接由 `continues`、`retry` 的 `session: continue`、`loadSession` 回退承担 | 新增失败重试回原会话（上下文不超过 75%）、负责人断开后重开、服务重启后进行中的目标 Run 自动继续 | **F**：`docs/decisions/20261125-resume_after_failure.md`、`docs/decisions/20261125-continue_after_restart.md` |
| 注入的系统层说明是英文措辞 | 注入文字跟随应用语言，`PUT /api/language` 推送；术语和标题有中英两版 | **F**：`docs/decisions/20261125-prompt_language.md` |

### 四条 20261125 ADR 对昨日结论的影响

- **只注入协作规则**（[ADR](../decisions/20261125-collaboration_only_injection.md)）：昨日报告引用的验证者提示和负责人说明措辞已过时：`teamwork::lead_guide` 删除，验证者提示改由 `teamwork::task_section` 生成，不再提测试和代码；系统层不再写 `skills_note`（这一项在更早的报告里）。昨日 MCP 报告建议「验证者应运行受影响的入口路径并给出输出」——这条现在属于质量工程师 / 研发的数字员工指令，不该再加进验证者提示。它同时**削弱**了昨日早间 P1-1 的原始形态（Janus 要求负责人安排独立验证），只留下「陈述事实」的空间。
- **断掉的会话回到原处继续**（[ADR](../decisions/20261125-resume_after_failure.md)）：昨日报告「已有：会话续接和事件回放」里只写了 `continues` 和 `retry` 的 `session: continue`；现在协作者失败后回原会话（≤75%），负责人断开后也重开，`resume_session` 从会话 id 改为 `Reload`。结论方向不变，细节更新。
- **服务重启后自动继续**（[ADR](../decisions/20261125-continue_after_restart.md)）：昨日报告没有覆盖这一点；此前服务重启后进行中的 Run 记为 `interrupted`，要等用户点「从中断处继续」。现在 `planning` / `running` 的目标 Run 自动继续，暂停过和上次自动继续后没有进展的除外。这是对现状的**取代**，不是补充。
- **注入文字跟随应用语言**（[ADR](../decisions/20261125-prompt_language.md)）：昨日报告引用的系统层英文措辞现在只是英文版；引用注入文字时要说明语言，中文界面下注入的是中文。

### 昨日四条待决事项的现状判断

- **早间 P0-1（交付摘要单列受保护路径）——判断不变，仍待用户决定。** 今天的窗口里没有新的产品证据，20261125 的 ADR 也不涉及交付摘要和合并行为。Claude Code 对 MCP Skill 按 manifest 版本批准的做法（批准绑定到文件摘要，服务器一改文件就要重新批准）是同一类「让信任边界的变化可见」的设计，但它服务的是 Skill 加载，不是交付；只能作为方向上的旁证，不能当作新证据。
- **早间 P1-1（编排时提醒「改了文件但没有独立验证」）——被 20261125 的 ADR 收窄，不是推翻。** 先回答「它属于协作机制还是做事方法」：`teamwork::check` 是编排校验，检查的是团队形状（`verifies` 指向、执行者不能验证自己、`owns` 重叠、依赖成环），这属于 ADR 列出的协作机制；但「改了文件就应该安排独立验证」是一条做事与验证流程的要求，ADR 第 2 条明确把它交给数字员工的指令，默认 PMO 的指令也已经写了「交付前每个改了文件的子任务都要核对过」。因此可行的形态只剩一种：在编排返回里**陈述事实**（例如「T1 有 `owns`，没有任何 `verifies` 指向它；交付会标出 `unverified`」），不写成要求、不阻塞编排。是否要加这一行仍由用户决定；直接把「要求验证」写回 `teamwork::check` 会和 ADR 冲突。
- **第二份报告的 P1-1（资源库显示「N 个工具只能以任务方式调用」）——仍成立，且有轻微加强。** 今天的 Claude Code 2.1.332 修了「服务器任务 ID 超过 128 字符时报 malformed task handle」的问题，把上限提到 1,024，说明任务式 MCP 工具在真实使用中；Janus 仍然只把可调用的工具计入预热结果（`backend/src/orchestrator/mcp_router.rs:255`），资源库仍显示「已准备好 · N 个工具」（`frontend/Sources/Janus/UI/Settings/LibraryView.swift:488`），用户分不清「没有工具」和「有任务式工具」。优先级不变。
- **第二份报告的 D1（是否引入 Skills over MCP）——加强为「不引入」，并新增两条硬前提。** 见[观察 1](#claude_code_skills)和[待用户决定 D1](#d1)。

<a id="observations"></a>
## 主窗口观察（2026-11-24 18:50 至 2026-11-25 18:15 UTC）

<a id="claude_code_skills"></a>
### 1. Claude Code 2.1.332：宿主开始加载 MCP 服务器的 Skill，但 Agent SDK 会话拿不到

**发布事实（F）。** v2.1.332 于 2026-11-25T01:32Z 发布，第一条是「Added skills from MCP servers (beta). They show as `server:skill` in the `/` menu; other skills keep their names」。同一条 release 还有 `PreSkillShellCommand` hook（在 Skill 或命令加载内联 shell 命令时可以拒绝）、跨会话消息的「Always deliver」，以及一批 MCP / SSE / compaction 修复：SSE 服务器事件流断开后不再挂死、本地 MCP 服务器刷 stdout 不再冻住终端、任务 ID 上限从 128 提到 1,024、恢复长会话和启动时 MCP 工具很多的内存占用。[changelog](https://code.claude.com/docs/en/changelog.md) 最新一版也是 2.1.332，没有 2.1.333（**F**）。

**它是不是昨天说的 Skills over MCP：是（F）。** Claude Code 文档写「With Skills over MCP, a server can provide skills alongside its tools」，并让服务器作者实现 [Skills extension](https://modelcontextprotocol.io/extensions/skills/overview)。扩展概览页写明这是官方扩展 `io.modelcontextprotocol/skills`，SEP-2640 为 Final，规范在 `modelcontextprotocol/ext-skills`。

**宿主怎样展示、加载和限制（F，除非另注）。**

- **命名与来源**：Skill 的名字是 `<server>:<skill>`，例如 `docs:pdf`；connector 加 `claude_ai_` 前缀，plugin 加 `plugin_<plugin>_` 前缀；`<server>` 里的空格和 `-` 以外的标点变成 `_`。宿主保留服务器身份和 URI，不同服务器的同名 Skill 不会互相覆盖。
- **服务器要求**：必须声明 `resources` 能力和 `io.modelcontextprotocol/skills` 扩展（在 `server/discover` 里），实现 `skills/list` 和 `skills/get`，Skill 文件经 `resources/read` 提供；规范示例使用协议 `2026-07-28` 或以上（**F** 扩展规范）。Claude Code 的排错表也写「没有 Protocol 行时，确认服务器支持 MCP revision `2026-07-28` 或以上」（**F**）。
- **加载与校验**：宿主只读清单里列出的文件，逐个校验字节数和 SHA-256，解析 `SKILL.md` 的 frontmatter 并与清单比对；清单变化就要求重新批准；支持文件不预先抓取。
- **批准**：auto 模式下由分类器决定，不读内容、默认不弹窗；Manual / `acceptEdits` 模式弹窗，「Allow this version」只在服务器改变该 Skill 的文件前有效；`ask` / `deny` 规则按服务器（`Skill(mcp__docs__*)` 或 `Skill(docs:pdf)`），`allow` 规则只对用户级、local 或管理设置里的服务器生效，连接器、`.mcp.json` 和 plugin 服务器不适用。
- **安全**：MCP Skill 拿不到 `allowed-tools` 授权；frontmatter 里的 `hooks`、`model`、`paths` 等被忽略，`!` 命令不运行，`@` 引用不附加；只有部分连接器会被 Anthropic 扫描；文档警告「Skills can direct Claude to run code. Servers can change skills at any time. Add only servers you trust.」
- **可用范围**：终端、JetBrains、桌面端 Code 标签页的本地会话；需要 Fable 模型或 Opus 4.8 / Sonnet 5 / Haiku 5.5 以上，直连 Anthropic 且能取 feature flag。**明确不支持：云会话、`claude -p`、`claude remote-control` server 模式、GitHub Actions 和 Agent SDK**；自定义 `ANTHROPIC_BASE_URL`、`apiKeyHelper`、`ANTHROPIC_AUTH_TOKEN` 也不支持。每个服务器最多显示 100 个 Skill。

**对 Janus 的含义（J，前提是上面的 F/I）。**

1. **路由侧接不上。** `janus-tools` 的 `initialize` 只声明 `tools`，`mcp_server.rs` 的注释也写明「tools only」；路由只处理 `initialize` / `ping` / `tools/list` / `tools/call`，其余一律 `-32601`；`mcp_client.rs` 的 `VERSIONS` 是 `2025-11-25`、`2025-06-18`、`2025-03-26`、`2024-11-05`，没有 `2026-07-28`。即使资源库里的服务器实现了 Skills 扩展，Janus 也不会转发 `resources`，Agent 看不到任何服务器 Skill。
2. **宿主侧也接不上。** Janus 的 Claude 连接是 `@agentclientprotocol/claude-agent-acp`（`backend/src/adapters.rs` 的 `AdapterDef { id: "claude", package: "@agentclientprotocol/claude-agent-acp" }`），这个包基于 Claude Agent SDK（npm 依赖 `@anthropic-ai/claude-agent-sdk` 0.3.331，README 写明「implement an ACP agent by using the official Claude Agent SDK」）。Claude Code 文档把 Agent SDK 会话列为 MCP skills 不支持的场景。也就是说，就算 Janus 明天把路由升到 `2026-07-28` 并转发 `resources`，它启动的 Claude 会话也不会加载服务器 Skill；这条结论是推出来的（**I**），验证方法见[待验证问题](#open_questions)。
3. **Codex 侧未知（U）。** 本窗口没有找到 Codex 支持 Skills 扩展的证据；MCP 客户矩阵里也没有 Codex 行。
4. **结论：这不是一个「现在补一个显示」的缺口，而是「宿主能力和协议版本都不在」的缺口。** 昨天 D1 的推荐（不引入；如果引入，只做用户逐项批准后导入，且先写 ADR）被加强。扩展规范还要求宿主把 Skill 内容当不可信输入、按服务器身份隔离、按 manifest 批准，这些与 Janus「Skill 只由用户导入」的模型同向，也意味着「自动复制进资源包」的方案在信任边界上更不可接受。
5. **`PreSkillShellCommand` 对 Janus 无直接影响。** 它是宿主侧 hook，Janus 的角色资源只在会话启动时注入、不写 Agent 配置，也没有注入 hook 的入口；不引入。

<a id="codex"></a>
### 2. Codex：0.174 alpha 连续发布，仍然只有版本名

- 窗口内发布的是 `rust-v0.174.0-alpha.3`（11-24T20:14Z）、`alpha.4`（23:01Z）、`alpha.5`（11-25T01:11Z）；`alpha.1`（01:46Z）和 `alpha.2`（16:14Z）在窗口起点之前。body 只有「Release 0.174.0-alpha.N」；没有 `rust-v0.173.0` 或 `rust-v0.174.0` 正式版（**F** 标签与时间，**U** 内容）。
- 最新正式版仍是 `rust-v0.172.0`（11-24T00:03Z，昨天已覆盖它的 Guardian 审批层）。本报告不推断 alpha 里新增了什么。

<a id="protocols"></a>
### 3. 协议：ACP 与 MCP 都没有新版本，只有矩阵和文档在动

- **ACP（F）**：最新 release 仍是 11-23 的 `v1.22.0`、`schema-v1.35.0`、`schema-v2.0.0-beta.4`。11-24 的两条提交（registry、消息客户端）在窗口起点前；窗口内只有 11-25 11:43 至 11:46 的三条文档提交（Gerdr、AppuBot、WrongStack 加进客户端列表）。`mcp/message` 的 `sessionId` / `toolCallId` 仍没有实现提交，昨天「等适配器开始发送再评估」的结论不变。
- **MCP（F）**：最新 release 仍是 `2026-07-28`（发布 2026-07-28T16:47Z）。窗口前的 11-24 17:39 与 17:43 两条（IG 章程对齐、MCP 两周年博客，昨天已覆盖）不计入；窗口内是 21:47 与 21:51 修正 `2026-07-28` 和 draft 文档的小错，**21:48 把 Claude（web）和 Claude Code 的 Skills 支持标进客户矩阵**（`3c8eb0c4`）。没有扩展状态从 beta 转正，也没有新的 SEP。
- **对 Janus**：协议支持范围没有变化，不需要动 `VERSIONS`；真正变化的是宿主生态（Claude Code 开始消费 Skills 扩展），见观察 1。

<a id="research"></a>
### 4. 研究：等预算下的团队收益、可 fork 的状态沙箱，以及「一致不是证据」

本节的 4.1 至 4.3 提交于 2026-11-24 06:10 至 13:37 UTC，早于本窗口起点（18:50），属于昨日报告窗口内但昨日两份都没有收录的 cs.MA 条目；按本次任务线索补收，提交日期照实标注。4.4 是 11-23 的补充，4.5 只列目录。

#### 4.1 Recursive Collective Intelligence：团队只在转发信息时超过算力匹配的基线（arXiv:2611.32606，2026-11-24）

- **是什么（F）**：作者提出「等预算阶梯」，把团队和三种基线比较：单 Agent、算力匹配的多数投票、算力匹配的 pass@n；并定义 RCI 为「团队用自己这一代的产出，在同等成本下改进下一代，不需要老师或外部标签」。实验覆盖 9B 到 27B 的模型，任务包括 PlanBench、HiddenBench 和 Hanabi。
- **关键数字（F）**：把拆分的事实**转发**给彼此，能让团队在三个模型规模上超过算力匹配的 pass@n，27B 上约高出三分之一；同质的 27B Agent 会自发形成 hub，准确率提高 18 个百分点；把这个组织写成提示，在描述性比较里用不到一半的 token 预算达到第三档；连续两代无标签改进逐步爬梯，第二代 9B 超过 pass@n 和一个用基座模型回合训练的对照，在三个任务上连续四到六代都胜过上一代。
- **对 Janus 的含义（J）**：看板的上游结果和 `handoff` 正是「转发信息」的机制，`task_detail` 里的证据也让下游不必重做；这篇论文说明这类信息传递是团队收益的主要来源，值得继续把「下游需要知道什么」写进完成消息，而不是增加 Agent 数量或让 Agent 互相讨论。把组织写成提示成本更低这一条，和 Janus 由负责人把 `owns`、`deps`、`verifies` 写进编排的做法同向；但它是实验室任务上的结论，不能直接外推到 Janus 的工作目录和长任务。
- **边界**：论文的基线是同一模型多次采样和投票，不是「单 Agent 分多个新会话」；团队规模也比 Janus 的 3 到 5 人大。它的「组织涌现」是自发 hub，Janus 是固定负责人，两者不能互相替代论证。

#### 4.2 TimeTravelContainer：无 root 的 CoW 实时 fork 整个运行中的容器（arXiv:2611.32255，2026-11-24）

- **是什么（F）**：一个无 root 的容器运行时，用写时复制在运行中 fork 整个容器，包括进程、内存、文件系统和网络；不需要改内核、硬件虚拟化或 root，声称支持 Linux、macOS（Apple Silicon）和 Windows WSL 2。在两个 Agent 化的 EDA 工作流（MAGE / Innovus、EvoDRC / KLayout）上报告 84.1 倍环境克隆加速、31% 到 72% 的内存下降、1.7 到 9.4 倍的端到端加速。
- **与 Janus 的关系（J）**：Janus 的 fork 粒度是文件系统（worktree）和会话（`session/load`），不是进程状态；worktree 让并行任务互不覆盖文件，但两个任务不能从「同一个正在运行的工具状态」分叉。TTC 解决的是「有状态工具探索」：试一条路径，失败后回到分叉点再试另一条，而不是重开整个环境。Janus 现在没有这类需求，Agent 的工具由它自己按权限模式运行，Janus 也不管理它们的进程和内存；把容器运行时引入会改变信任边界（新增下载来源、执行第三方运行时），按仓库规则要单独 ADR 和用户决定。
- **结论**：**观察，不做**。触发条件：出现「同一任务需要从中间状态并行试多条路径，且状态在进程内存里」的真实需求；或者 worktree 的冷启动成为长任务的瓶颈。

#### 4.3 Provenance-Isolated Epistemic Quorums：模型之间的一致不是物理证据（arXiv:2611.32032，2026-11-24）

- **是什么（F）**：把每个 LLM 严格排除在可信计算基之外，用「执行绑定的证据认证」和「可信世界」安全门来批准对生产控制面的改动：诊断前提必须来自互相独立的物理故障域，候选改动要对所有未被推翻的故障假设成立，否则退回只读诊断；证书绑定到目标执行的快照、修订和授权新鲜度。报告「零次在假设内不安全准入」「零次错误前提提交」；Agent 省略证明时转成可恢复的假阻断而不是不安全动作。
- **对 Janus 的含义（J）**：`verifies` 是另一个模型给出的判断，不是执行证据；Janus 的证据方法已经把 `test` / `command` 排在前、`reasoning` 只作提醒、`unverified` 单列，交付也保留 `traceGaps`（从工作目录读出的实际改动），这和「执行绑定证据」同向。这篇论文支持继续把独立验证当作**结构化判断**而不是事实本身：验证者说 approved 不等于文件真的正确，用户验收仍然是最后一道门。它也反对把「多个 Agent 投票一致」当证据，Janus 没有投票机制，不需要新增。
- **边界**：论文的基准是作者自标注的，模型快照没有固定，Tier E2 的物理故障转移还是未完成工作（**F** 论文自述）。它的控制面场景（etcd / PostgreSQL 集群）比 Janus 的本地工作目录重得多，不能照搬。

#### 4.4 补充（窗口外）：Reproducible Social Structure（arXiv:2611.31543，2026-11-23）

- **是什么（F）**：50 个带 persona 和持久记忆的 Agent、三个社交平台、同一初始条件跑 10 次、共 700 小时；每个世界里广泛的通信都收敛成稀疏的持续 dyad；静态人格相似度基本不能预测配对，第一次接触时表达的情绪和特质的对齐程度可以预测；移植 persona 或记忆会让接收者的行为向 donor 移动约 60%，从下一轮就开始变。
- **为什么只作旁证（J）**：它说明 LLM 社群会自发形成稳定但稀疏的关系结构，且记忆会显著改变行为。对 Janus 的团队记忆有一点间接含义（写进记忆的内容会实质影响之后的 Agent 行为），但它是无目标、自由互动的模拟，没有任务、看板和验证，不能作为改团队记忆机制的依据。**不引入。**

<a id="other_papers"></a>
#### 4.5 其余 11-24 的 cs.MA 论文

| 论文 | 一句话 | 与 Janus 的关系 |
| --- | --- | --- |
| CANON（2611.33100） | Agent 辅助构造 Feynman 积分的规范微分方程 | 领域应用，无关 |
| Multi-Agent Coordination Through Stigmergy（2611.32966） | 用共识主动性在多 Agent 间形成运河网络 | 环境式协调，和看板式显式协调不同，不引入 |
| MRBench（2611.32803） | 异构多机器人操作的物理必要性推理基准 | 机器人，无关 |
| TeamFlowRL（2611.32645） | 分散式流策略的在线策略优化 | 多 Agent 强化学习，不是 LLM 团队，无关 |
| Unifying Partner and Environment Diversity（2611.32031） | 人机协作中的伙伴与环境多样性 | 人机协作，弱相关，不引入 |
| AgentDRC（2611.31876） | 空间感知的自动 DRC 纠正多 Agent 算法 | EDA 领域应用，无关 |
| Multi-Player Social Games as Verifiable Training Grounds（2611.31821） | 用多玩家社交游戏当可验证的训练场 | 训练方法，和 Janus 的运行期协作无关 |

2026-11-25 的 cs.MA 导出查询返回 0 条，不能据此说当天没有新论文（**U**）。

<a id="community"></a>
### 5. 社区与工业：更多 Agent 更快但更差，赢的是「证明比基线好」的门

#### 5.1 Paradigm 3《Swarms do not yet raise the roof》（2026-11-24；页面只给日期，HN 提交为 11-25T15:57Z，是否早于窗口起点 **U**）

- **规模与方法（F）**：作者 Niccolò Zanichelli 和 Gavin Leech；1,100+ 次运行、80+ 队形、4 个长任务（git-to-zig、AHC014、AHC038、Anthropic performance take-home）；预算主要是 800 万加权 token，部分 3,200 万；队形分协议团队、自组织团队（Codex 消息板、Claude 直接消息、SwarmChat）和编排团队；规模 1 / 2 / 4 / 8，Terra 在 32M 下另跑 16 和 32；团队成绩取任一成员的最好提交。
- **质量（F）**：同等预算下团队比单 Agent 低约 7%；汇总表里 70 次比较输了 54 次，只在单 Agent 接近饱和时打平；800 万 token 下在未饱和任务上没有团队质量收益；惩罚随规模变大：Terra 的 16 人队 33.3%，与 8 人队相当，32 人队掉到 23.2%，低于单 Agent 的 26.6%。
- **速度与协调惩罚（F）**：8 人队通常比单 Agent 早约 6 倍用完同一预算，加速范围约 3 到 16 倍；协调惩罚参数 λ 在 0.6 到 0.9（对一个可良好确定的未饱和任务），32M 的 git-to-zig 上 Sol 为 0.94、Terra 为 0.75，8M 上为 0.89 和 0.60。
- **32M 的表面优势与重启对照（F）**：git-to-zig 32M，Sol 的 8 人队 93.2% 对单会话 83.7%；Terra 的 8 人队 32.0% 对 26.6%。但把同一个单 Agent 分成 8 个新会话、每次清空上下文但保留文件，Sol 得到 97.0%（对 8 人队 93.2%、长会话 83.7%），Terra 得到 41.9%（对 32.0%、26.6%）；8M 下 Sol 62.1% 对 55.0%，Terra 28.0% 对 17.8%。作者据此认为「swarm 的表面优势来自长会话变陈旧，不是协作提高了上限」。
- **沟通与多样性（F）**：沟通的收益从 −17 到 +53 个百分点；git-to-zig 上 4 到 8 人队靠沟通提高 16 到 53 分，自组织的 8 人 Sol 队提高 53 分，协议团队提高 20 分；AHC 任务上 GPT 团队沟通反而损失 2 到 12 分；GPT/Claude 混合队没有明显优势。
- **对 Janus 的含义（J）**：第一，支持 Janus 保持 3 到 5 个协作者、清晰的 `owns` 和有限并发，不支持扩大团队；第二，它把「长会话变陈旧」单独拎出来，这一点和 Janus 今天的两个续接 ADR 相关但**不冲突**：Janus 的失败重试回到原会话是为了不丢掉已经做过的工作，并且有 75% 上下文上限，超过就新开会话；论文对照的是「同一个长会话一直做到底」和「计划性地切成多个干净上下文」，说明**陈旧上下文**才是问题，Janus 的 75% 上限方向一致，但「负责人的长目标会话」是否也需要计划性换新，值得观测，不建议现在改。第三，论文的赢家是「单 Agent + 保留文件的多段会话」，这提示 Janus 的 `retry` 选 `session: new`、以及新任务不滥用 `continues`，可能比一直续接更稳；但论文只在 git-to-zig 上做了这个事后对照，重复次数少，预算也远低于前沿实验室，不能据此改默认策略。
- **口径（U）**：机构自报，数据与代码公开（[data](https://paradigm3.org/research/swarms/data)、[code](https://github.com/paradigm-3-institute/raising-the-roof)），复现成本约 7 万美元；本报告没有复现。

#### 5.2 Hugging Face《Scaling Swarms to 1000s of Agents》（2026-11-25；HN 提交时间为 11-25T16:27Z，页面本身没有日期，**U**）

- **是什么（U，平台自报）**：1,000 个 DeepSeek Agent 跑 9 天，产生 175,000 条消息、8,500+ 个合并 PR，做出一个纯 Python 的聊天机器人，声称在日常对话上匹配 SmolLM-360M 而无学习权重；组织成 10 条 lane（理解、回答、对话、路由、质量、安全、意图、不确定、多轮、steward），工具是 Msgboard（频道、线程、私信、协作文档）和 Legion（跨本地与分布式跑上千 Agent 的编排框架）。
- **最有效的设计（U）**：每个 PR 必须用一组由 swarm 自己写的真实对话证明比当前 main 有改进，Agent 按风格指南评估回复，评审者再重复比较后才批准；作者说这一条比层级、投票机制和固定团队都更有效，早期运行主要靠「其他 Agent 喜欢」就合并。
- **评测与事故（U）**：用 SmolTalkBench（1,000 条日常对话，DeepSeek-V4.1-Flash 对照参考答案打分）跟踪；约 2,000 次提交时分数下跌，因为一个合并引入 import 错误让机器人崩溃，swarm 自己修好并继续提升；评测对 swarm 保密（提示、控制器和 Agent 文件都不提，评分在 Hugging Face Jobs 上单独跑，事后扫描 trace 没有发现引用），但这条保证依赖作者自己的运行隔离和审计，不是独立核实。
- **对 Janus 的含义（J）**：它和 Paradigm 3 不矛盾：一个是「同等预算下团队不比单 Agent 质量高」，一个是「有一个可执行的准入规则时，大规模团队能持续改进」。共同点是**赢家都是可验证的门**（「证明比基线好」），不是组织形状。Janus 的 `verifies`、验收标准和证据方法承担同样的角色；它的 Msgboard 也和 Janus 的看板同向，都是共享的协调通道。差别在并行 PR：swarm 用大量小 PR 逐个过门，Janus 是每个 Run 一次交付合并，没有 PR 流程，不引入分批合并。这条经验支持把「与当前状态比较」写进验收标准，而不是增加层级或投票。注意它是自报实验、没有独立复现，**U**。

#### 5.3 其余社区条目

- **Formalize or Lose It（Zenodo 预印本，2026-11-17，窗口外；11-25 上 HN）**：一次 FBS 运行里 125 个测试全过，但系统有 84 条监督规则，39 条只靠提示传递；从 Root 计划传到 Worker 的 23 条规则里，19 条没有可执行检查支撑；19 个故障注入用例中，FBS 拦住了所有计划和交付类故障，但放过了 6 个它的检查没有有效覆盖的违规，另一组宿主侧检查找回了其中 5 个；交接研究里，拿到完整笔记的评审者修好了全部违规，拿到压缩笔记的评审者接受了 71% 含缺陷的尝试；用 ledger 直接传要求给执行者时，覆盖到的谓词只花了约六分之一的监督 token。**对 Janus（J）**：它说明「写在提示里的规则会漏」，能由宿主持有的可执行检查才可靠；这和 Janus 把 `owns` 重叠、`verifies` 指向、痕迹核对做成机制一致，也提示 20261125 ADR 的取舍边界：协作规则留在机制里，做事方法交给指令，但**必须成立的结构性规则仍应是可执行的**。只作原则旁证，不引入（**F** 内容，**U** 独立复现）。
- **Hub（icons8，Show HN 2026-11-25，Apache-2.0，26 star）**：本地调度器，用一条会话协调多个 Claude Code / Codex 会话；核心是「journal 的唯一写者」，状态是事件的纯函数；每个执行者在自己的 worktree，`claude -p` 每个会话一个进程或走 `codex app-server`；有 watchdog 检测循环、沉默和重复错误，问题卡片直接给操作者；明确「不是沙箱」。**对 Janus（J）**：方向和 Janus 的事件日志、worktree、会话隔离一致，但采用信号很弱（**U**），不构成功能压力；它的 watchdog 值得对照 Janus 的停滞报告，但本窗口没有证据表明 Janus 缺少它。
- **Voynich Swarm（2026-11-25，HN 3 分）**：公开的、带隐藏测试页的 Agent swarm 尝试读伏尼契手稿，0 / 34 页读对，12 条主张自 1921 年以来没有被接受；有测试门和排行榜。**不引入**：它是研究众包，不是团队协作机制。
- **Agentic Leaderboard 的「56% unranked」说法**：HN 帖子标题这么说，但页面本身只展示「最近 30 天启动的项目按 momentum 排名，每月全部换新」，1423 个启动、100 个上榜、30 天滚动窗口，最新更新 11-23；**页面不支持 56% 这个数字（U）**，不写入结论。
- **Infisical「hazmat suits」博客**：同一篇在 2026-11-02 已上过 HN，11-25 重新上（**U**），不是新文章，不写入。
- **Patrick McKenzie《Multiagent Coding Is the Future》（2026-11-24）**：自述在 devbox 里跑 2 到 100+ 个 Claude，管理进程秒级加 Agent；举例 42 个 PR / 一天、部署延迟降 80%、8,600 行 / 161 个文件、823 美元推理成本；Stripe 每周数千个 Agent 写的 PR；某工程师一个月 170 万美元推理。**对 Janus（J）**：它描述的是「管理者 + 多 Agent」的采用现状，和 Janus 的负责人分工同向；数字全部自报（**U**），没有 worktree、验证或看板细节，不能作为机制依据。

<a id="vendors"></a>
#### 5.4 厂商公告核对

- **Anthropic**：11-24 的「MCP servers can now ship skills, run tasks, and more with Claude」昨天已覆盖；11-24 另有「Turn artifacts into Mac and Windows apps」，与 Agent Team 无关；11-25 没有新的 agent 相关文章。
- **OpenAI**：11-23 至 11-25 的 news 没有 agent / Codex / 多 Agent 条目；Codex 只有 alpha release。
- **Google**：Antigravity **v2.35.1（11-24；页面只给日期，未给时刻）** 修了「应用重启或更新后，进行中的对话在启动时自动恢复」、插件面板链接自动打开、诊断上传压缩。它的「自动恢复」是**对话**，不是任务；和 Janus 今天采纳的「服务重启后进行中的目标 Run 自动继续」（[ADR](../decisions/20261125-continue_after_restart.md)）是同一类用户可感知行为，说明「重启后自动继续」正在成为同类产品的默认，不构成改动理由。
- **GitHub**：11-24 只有通知体验的改动，没有 agent 相关 changelog。
- **Cursor**：changelog 最新仍是 11-06，窗口内没有更新。

<a id="suggestions"></a>
## 对 Janus 的建议

<a id="three_categories"></a>
### 三类结论

| 类别 | 条目 | 理由 |
| --- | --- | --- |
| **已有** | 看板作为唯一协作通道与上游结果 / `handoff`；`owns` 与并行冲突校验；`verifies`、证据方法、`traceGaps`、交付 `unverified`；审批归用户；Skill 只由用户导入、经 `janus-skills` 目录；MCP 路由只声明 `tools`、按需启动；worktree 与交付时合并；3 到 5 人团队与并发上限；会话续接与重启自动继续 | 本日的外部进展没有推翻它们；RCI、PIEQ、PyBot 和 Paradigm 3 分别从「转发信息」「执行证据」「可执行准入」「团队规模」四个方向支持其中的一部分 |
| **值得做** | 本日没有新的 P0 或 P1；昨日未决的 P0-1、P1-1（需改写为陈述事实）、资源库 P1-1 按原优先级等待用户决定 | 本日最强的信号是「宿主能力与协议都不在」，不是「补一个小功能」；没有新证据支持新增优先级 |
| **不做** | 自动复制或自动读取服务器 Skill；把 MCP Skills 当 Skill 来源；用多个 Agent 投票当证据；为基准式并行扩大团队；现在引入容器级 fork；把「证明比基线好」做成 Janus 的强制门 | 与「Skill 只由用户导入」「审批归用户」「3 到 5 人团队」「本地优先、运行时不写仓库」冲突，或证据不足、改变信任边界 |

<a id="candidates"></a>
### 候选逐条对照

| 候选 | 证据 | 对应 Janus 概念 | Janus 当前状态 | 结论 |
| --- | --- | --- | --- | --- |
| Claude Code 的 MCP 服务器 Skill | **F**：v2.1.332；扩展要求 `resources` + `io.modelcontextprotocol/skills` + `2026-07-28`；Agent SDK 会话不支持 | Skill 路由、MCP 路由、会话宿主 | 路由只声明 `tools`、最高 `2025-11-25`；资源库 Skill 走 `janus-skills` 目录（[Skill 路由](../domains/20261116-resource_library.md#skill_router)）；Claude 适配器基于 Agent SDK | **不引入**；观察，触发条件见 P2-1 |
| `PreSkillShellCommand` hook | **F**：可在 Skill / 命令加载时拒绝其内联 shell 命令 | Agent 权限模式、Skill 加载 | Janus 不写 Agent 配置、不注入 hook | **不引入**：没有配置层可以落 |
| Claude Code 2.1.332 的 MCP / SSE / 任务 ID 修复 | **F**：SSE 断流、任务 ID 上限 1,024、启动内存 | MCP 路由、任务式工具 | 路由自己处理 SSE 重连（下次调用重连）；任务式工具仍过滤 | **已有方向**：不因宿主修复而改路由 |
| Antigravity 2.35.1 重启自动恢复对话 | **F**：应用重启后自动恢复进行中的对话 | 重启自动继续（目标 Run） | 今天已采纳（[ADR](../decisions/20261125-continue_after_restart.md)），且覆盖的是目标 Run | **已有**：不因为同类产品做了就扩大范围到对话 |
| RCI 的「转发信息才提升集体智能」 | **F**：27B 上约三分之一；组织写成提示少于一半 token | 看板、`handoff`、`task_detail`、负责人编排 | 上游结果带 `handoff`、风险和改动；`task_detail` 可查全文（[团队结构](../domains/20261031-goal_mode.md#team_structure)） | **已有**：不新增机制；可把「下游需要什么」写进完成消息的语气 |
| RCI 的「组织写成提示」 | **F**：描述性比较里不到一半 token 达到第三档 | 负责人说明、编排 | 负责人把 `owns` / `deps` / `verifies` 写进编排；协作规则注入（[负责人知道什么](../domains/20261031-goal_mode.md#lead_brief)） | **已有**：不改成让 Agent 自组织 |
| TimeTravelContainer | **F**：84.1 倍克隆、31% 到 72% 内存下降；无 root；支持 macOS Apple Silicon | worktree、会话续接、重试 | fork 粒度是文件和会话，不是进程；不管理 Agent 进程 | **观察，不做**（P2-2） |
| PIEQ 的「一致不是证据」 | **F**：LLM 在 TCB 外；执行绑定证据；作者自标注基准 | `verifies`、证据方法、`traceGaps`、用户验收 | 已有证据方法排序、`unverified`、`traceGaps`；审批归用户 | **已有**：不引入投票或 quorum |
| Paradigm 3 的团队规模与质量 | **F**：8 人 6 倍快但质量低 7%；54/70 输；16/32 不更好 | 团队规模、并发、`owns` | 3 到 5 个协作者；`max_concurrent_attempts` 3、`max_agent_sessions` 12（[任务调度](../domains/20261031_run_lifecycle.md#scheduling)） | **已有**：维持 |
| Paradigm 3 的「重启单 Agent 胜出」 | **F**：97.0% 对 93.2% / 41.9% 对 32.0%（git-to-zig，事后对照） | 重试的新会话、`continues`、负责人长会话 | 失败重试回原会话（≤75%），超过就新开；`continues` 复用前置会话 | **观察**（P2-3）：不改默认策略 |
| PyBot 的「PR 必须证明比 main 好」 | **U**：平台自报；8,500+ 合并 PR；比层级 / 投票 / 固定团队有效 | 验收标准、`verifies`、交付 | 验收标准要求可观察结果；验证者给 `verdict`；交付汇总证据 | **已有方向**：可把「与当前状态比较」写进验收标准（角色指令层面，P2-4） |
| Formalize or Lose It 的「提示规则会漏」 | **F**：39/84 只靠提示；19/23 无可执行检查；压缩笔记接受 71% 缺陷 | 协作规则注入、`teamwork::check`、痕迹核对 | 结构性规则已可执行；方法要求写在数字员工指令里（[ADR](../decisions/20261125-collaboration_only_injection.md)） | **已有**：作为「规则放哪里」的判据，不改代码 |
| Hub（icons8） | **U**：26 star；journal 唯一写者；worktree；watchdog | 事件日志、worktree、会话 | 事件日志可回放；worktree；停滞报告 | **已有方向**：不追功能 |
| 自动复制服务器 Skill / 读 `skill://` | **F**：扩展要求宿主批准绑定 manifest、内容不可信；**F**：Janus 路由不声明 `resources` | Skill 来源、资源库 | 只由用户新建、复制或从 Git 导入 | **不做** |

<a id="worth_doing"></a>
### 值得做

本日没有 P0。下面两项都是 P1，且都还要用户同意：第一项是昨日第二份报告 P1-1 的延续，第二项是昨日早间 P1-1 在 20261125 ADR 之后唯一可行的形态。

<a id="p1_task_only"></a>
#### P1-1（沿用昨日）资源库显示「N 个工具只能以任务方式调用」

- **落点**：`backend/src/library.rs` 的 `Warm::Ready` 增加一个 `task_only` 计数；`backend/src/orchestrator/mcp_router.rs:255` 预热时同时数可调用和不可调用的工具（`remember_tools` 已保存全部工具，不需要额外请求）；`frontend/Sources/Janus/UI/Settings/LibraryView.swift:488` 在「已准备好 · N 个工具」后补一句；测试补在 `backend/src/orchestrator/flow_tests/library_router.rs`，现有 fixture `backend/testdata/fake_mcp.py` 已够用。
- **成功指标**：资源库对一个只提供任务式工具的服务器显示「有 M 个只能以任务方式调用」，不再是「0 个工具」；对没有工具的服务器仍显示 0；`tools/list` 和 `tools/call` 的行为不变（任务式工具仍不列出、调用仍返回 `-32602`）。
- **边界**：只显示，不实现 tasks 协议、不列这些工具、不让 Agent 试调用；不新增持久化字段，不需要数据迁移；文件都远低于 2000 行。
- **今天的证据**：Claude Code 2.1.332 把任务 ID 上限从 128 提到 1,024（**F**），说明任务式工具的调用路径在真实使用；Janus 的预热仍只计可调用工具（**F** 代码），显示缺口没有变化。

<a id="p1_fact_line"></a>
#### P1-2（待 D2 决定）编排时陈述「有 `owns`、没有 `verifies`」

- **落点**：`backend/src/teamwork.rs` 的 `check` 或编排返回的提示文本；只在任务有非空 `owns`、没有下游 `verifies` 指向它、且它不是验证任务时，输出一行事实，例如「T1 有 `owns`，没有 `verifies` 指向它；交付会标出 `unverified`」。措辞领域无关，两种语言各一份。
- **成功指标**：编排返回能看到这行事实；交付里 `unverified` 的子任务比例下降，或负责人说明里出现对应的核对说明；没有因为它产生新的编排拒绝或额外的验证任务。
- **边界**：只陈述事实，不阻塞编排、不要求负责人安排验证、不改变 `verifies` 的语义；不写「应该」「必须」；如果用户选 D2 的「取消」，这一项不做。

<a id="observe"></a>
### 观察（含触发条件）

<a id="p2_mcp_skills"></a>
#### P2-1 MCP 服务器 Skill 的宿主支持

- **现状（F/I）**：Claude Code 已支持，但要求服务器实现 Skills 扩展、协议 `2026-07-28` 以上、声明 `resources`；Janus 路由只声明 `tools`、最高 `2025-11-25`；Janus 的 Claude 会话跑在 Agent SDK 上，而文档把 Agent SDK 列为不支持。
- **触发条件**：ACP 适配器所用的宿主开始支持 MCP skills（适配器 release notes 或文档写明），**且** Janus 把路由升到 `2026-07-28` 并愿意转发 `resources`。两条都满足时再按 D1 的第二种方案评估（用户逐项批准后导入），先写 ADR。
- **重评时做什么**：按扩展规范实现 `skills/list` / `skills/get` / `resources/read`，校验文件大小与 SHA-256，把批准绑定到 manifest；把服务器身份写进 Skill 来源；不自动复制进 Run 资源包。
- **约束**：会扩大协议支持和信任边界，需要 ADR、数据迁移和 `data_shape`；不改变「Skill 只由用户导入」的产品决定，除非用户另行同意。

<a id="p2_ttc"></a>
#### P2-2 容器级状态 fork（TimeTravelContainer）

- **触发条件**：出现「同一任务从中间状态并行探索多条路径、且状态在进程内存里」的真实需求；或 worktree 冷启动成为长任务的瓶颈。
- **重评时做什么**：先评估它是否真的比「新会话 + 文件保留 + 快照」更省，再评估在 macOS 上的部署方式和信任边界（新增第三方运行时、下载来源、执行权限）。
- **现在不做**：Janus 不管理 Agent 进程，Agent 的工具状态也不在 Janus 手里；引入容器运行时是信任边界变化，按仓库规则要单独 ADR 和用户决定。

<a id="p2_restart"></a>
#### P2-3 负责人长会话与「分段新会话」

- **现状（F）**：Paradigm 3 在 git-to-zig 上做了事后对照，单 Agent 分 8 个干净上下文的会话优于一个长会话，也优于 8 人队；Janus 的失败重试在上下文不超过 75% 时回原会话，超过才新开。
- **触发条件**：事件日志显示长目标 Run 的负责人会话接近 75%、或交付后返工集中出现在长会话的后半段；或 Paradigm 3 的对照在更多任务上被复现。
- **重评时做什么**：用事件日志统计「负责人会话上下文占用」与交付质量、返工率的关系（`usage` 记录了上下文快照），再决定是否在负责人侧提供「分段」的做法；不直接改默认续接策略。
- **边界**：论文的对照只在一个任务上、重复次数少、预算 8M 到 32M，不能直接推广；Janus 的重试语义是「不丢工作」，和论文的「主动分段」不是同一件事。

<a id="p2_admission"></a>
#### P2-4 验收标准写成「与当前状态比较」

- **现状（F）**：PyBot 的经验是「每个 PR 必须证明比 main 好」比层级、投票和固定团队更有效；Janus 的验收标准目前只要求「可观察的结果」，没有要求「与改动前比较」。
- **触发条件**：用户希望在性能、质量或行为改进类任务上减少「改完了但没证明更好」的情况。
- **落点**：默认 PMO / 研发 / 质量数字员工的指令（不是 Janus 代码），例如「改进类任务的验收标准写清基线、度量方法和预期方向」。属于角色指令层面的改动，需要用户同意，不改变机制。

<a id="not_worth"></a>
### 不做（理由）

- **自动复制服务器 Skill、或让 Agent 直接读 `skill://`**：违反「Skill 只由用户导入」；扩展规范要求宿主把内容当不可信输入、按 manifest 批准、按服务器身份隔离，自动复制会把用户看不见的第三方指令带进每次会话。
- **用多个 Agent 投票或 quorum 当证据**：PIEQ 明确说模型之间的一致不是物理证据；Janus 的验证是结构化判断，最终由用户验收。
- **为基准式并行扩大团队**：Paradigm 3 的强反证；Janus 的 3 到 5 人和并发上限是对的。
- **现在引入容器级 fork**：信任边界和收益都不成立，列为观察。
- **把「证明比基线好」做成 Janus 的强制门**：会改变编排校验的语义、增加配置面；先用角色指令表达。
- **`PreSkillShellCommand` 或任何 Agent 配置写入**：Janus 的角色资源只在会话启动时注入，不写 Agent 配置（[ADR：资源在会话启动时注入](../decisions/20261116-launch_time_resources.md)）；hook 不在启动注入的入口里，也没有落点。

<a id="open_questions"></a>
## 待验证问题

| 问题 | 现状 | 怎么验证 |
| --- | --- | --- |
| Claude Code 2.1.332 的 MCP skills 在 ACP 适配器 / Agent SDK 会话里到底可不可用？ | 文档明确列 Agent SDK 为不支持（**F**），Janus 用该 SDK（**F** 依赖），结论是推断（**I**） | 在 Janus 里配一个声明 Skills 扩展的测试 MCP 服务器，开一个 Claude 会话，看 `/` 菜单或 `/skills` 是否出现 `server:skill`；同时看会话日志里的协议版本 |
| Codex 是否支持 Skills 扩展？ | MCP 客户矩阵没有 Codex 行；本窗口没有证据（**U**） | 查 Codex release notes / 文档；用测试服务器试 `server/discover` |
| Janus 路由在 Claude Code 请求 `2026-07-28` 时实际怎样降级？ | 代码按 `VERSIONS[0]` 回答 `2025-11-25`（**F** 代码），宿主行为未实测（**U**） | 用 v2 客户端的 `claude mcp` 连 `janus-tools`，看 `/mcp` 的 Protocol 行和工具是否可用；对应昨天 P2-1 |
| Paradigm 3 的重启对照能否复现？ | 论文自报，有 data 和 code，复现成本约 7 万美元（**U**） | 在小预算任务上复现 8M 的 Sol 对照；先看作者公开的数据 |
| Janus 的负责人长会话是否出现「陈旧上下文」？ | 没有统计；`usage` 记录上下文快照 | 用事件日志统计长 Run 的上下文占用与返工、交付质量 |
| 资源库「N 个工具只能以任务方式调用」的基线是多少？ | 没有统计；预热结果只计可调用工具 | 统计资源库服务器返回 `taskSupport: required` 的工具数和比例 |
| PyBot 的「证明比基线好」在 Janus 的验收标准里怎么表达最自然？ | 只有一条自报实验（**U**） | 在几个改进类任务上试用「基线 + 度量 + 方向」的写法，看验证者能否据此判定 |

<a id="decisions"></a>
## 待用户决定

以下事项会改变行为、信任边界或产品范围，本报告不假定已批准。每项给出推荐，最终由用户决定。

<a id="d1"></a>
### D1 是否引入 Skills over MCP？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **不引入（推荐）** | 维持现状：Skill 只由用户新建、从本机目录复制或从 Git 来源导入；服务器附带的 Skill 不进入上下文 | 是。今天新增两条阻断：Janus 路由不声明 `resources`、最高 `2025-11-25`；Claude 的 ACP 会话基于 Agent SDK，而文档把 Agent SDK 列为不支持 MCP skills。改动为零，风险为零 |
| 用户逐项批准后导入（如果将来引入，选这条） | 新增「从 MCP 服务器导入 Skill」的来源：应用列出服务器提供的 Skill，用户勾选后导入为资源项，按 revision 固定；需要 ADR、数据迁移、UI 和测试 | 仅在宿主和协议两条阻断都解除、且用户明确要时；导入时按内容 hash 固定 revision，刷新由用户发起 |
| Run 开始时调用 `skills/list` 自动复制进资源包 | 第三方指令未经逐项批准进入每次 Run；版本随服务器变化；服务器不可达时 Run 开始会变慢或失败 | 否。扩展规范要求宿主批准绑定 manifest，自动复制违背它的安全模型 |
| 让 Agent 直接读 `skill://` 资源 | 路由要声明并转发 `resources`；内容按需进入上下文；用户看不到清单，版本不固定 | 否。先完成前两条阻断的评估，单独 ADR |

- **推荐理由**：Janus 的 Skill 模型已经把「可信与否」交给用户导入的那一刻；今天的证据（宿主按 manifest 批准、内容不可信、服务器可随时改文件）说明这条路只会更严格，不会更宽松。详见[观察 1](#claude_code_skills)。

<a id="d2"></a>
### D2 昨日 P1-1（编排时提醒独立验证）保留、改写还是取消？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **改写为陈述事实（推荐）** | `teamwork::check` 或编排返回里多一行「T1 有 `owns`，没有 `verifies` 指向它；交付会标出 `unverified`」，不阻塞、不要求 | 是。`teamwork::check` 是协作机制，陈述团队形状属于它的职责；但「应该安排独立验证」是做事方法，按 20261125 ADR 属于数字员工指令 |
| 取消 | 交付仍会标出 `unverified`，负责人说明仍要求交代核对方式 | 可接受。可见性已经存在，只是晚一步 |
| 保持原样（要求负责人安排验证） | 会在机制层重新规定做事方法，与 20261125 ADR 的取舍冲突 | 否。除非用户明确要 Janus 对所有领域统一要求独立验证 |

<a id="d3"></a>
### D3 是否把「与当前状态比较」写进默认验收标准？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **暂不写（推荐）** | 保持「可观察的结果」；改进类任务的基线由负责人按任务决定 | 是。PyBot 只有一条自报实验；验收标准的写法属于数字员工指令（PMO 指令已要求写可观察的结果），先观察 |
| 写进默认 PMO / 研发 / 质量指令 | 角色指令增加「改进类任务写清基线、度量方法和预期方向」；不改机制 | 可作为下一步，需用户同意改默认指令 |
| 做成机制校验 | 编排校验检查验收标准是否包含基线 | 否。会把领域方法写进 Janus，且很难机械判断 |

<a id="sources"></a>
## 来源

以下来源均于 2026-11-25 访问。标注 U 的条目只有二手来源、页面异常或无法核实。

**Anthropic**

- [Claude Code v2.1.332 release](https://github.com/anthropics/claude-code/releases/tag/v2.1.332)，2026-11-25T01:32Z，**F**。
- [Claude Code changelog](https://code.claude.com/docs/en/changelog.md)，最新 2.1.332，2026-11-25，**F**。
- [Claude Code MCP 文档：Use skills from MCP servers](https://code.claude.com/docs/en/mcp.md#use-skills-from-mcp-servers)，2026-11-25，**F**。
- [Claude Code Skills 文档](https://code.claude.com/docs/en/skills.md)，2026-11-25，**F**。
- [Claude Code v2.1.331 release](https://github.com/anthropics/claude-code/releases/tag/v2.1.331)，2026-11-23，**F**（核对 2.1.331 的 release notes 没有 MCP skills 条目，而文档写「v2.1.331 或以上」）。
- [MCP servers can now ship skills, run tasks, and more with Claude](https://claude.com/resources/articles)，2026-11-24（昨天已覆盖），**F**。

**MCP**

- [Skills extension overview](https://modelcontextprotocol.io/extensions/skills/overview)，2026-11-25，**F**。
- [Extension support matrix](https://modelcontextprotocol.io/extensions/client-matrix.md)，2026-11-25，**F**。
- [提交 3c8eb0c4：把 Claude 与 Claude Code 的 Skills 支持标进矩阵](https://github.com/modelcontextprotocol/modelcontextprotocol/commit/3c8eb0c4)，2026-11-24T21:48Z，**F**。
- [MCP releases](https://github.com/modelcontextprotocol/modelcontextprotocol/releases)，最新 `2026-07-28`，**F**。
- [MCP commits](https://github.com/modelcontextprotocol/modelcontextprotocol/commits)，2026-11-24 至 11-25，**F**。

**ACP**

- [ACP releases](https://github.com/agentclientprotocol/agent-client-protocol/releases)，最新 schema 1.35.0 / 2.0.0-beta.4，2026-11-23，**F**。
- [ACP commits](https://github.com/agentclientprotocol/agent-client-protocol/commits)，2026-11-24 至 11-25，**F**。

**Codex 与适配器**

- [Codex releases](https://github.com/openai/codex/releases)，`rust-v0.174.0-alpha.1` 至 `alpha.5`，**F**；body 只有版本名，**U**。
- [@agentclientprotocol/claude-agent-acp](https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp)，0.112.0，依赖 `@anthropic-ai/claude-agent-sdk` 0.3.331，**F**。

**论文**

- [Recursive Collective Intelligence](https://arxiv.org/abs/2611.32606)，2026-11-24，**F**。
- [TimeTravelContainer](https://arxiv.org/abs/2611.32255)，2026-11-24，**F**。
- [Provenance-Isolated Epistemic Quorums](https://arxiv.org/abs/2611.32032)，2026-11-24，**F**。
- [Reproducible Social Structure Emerges From Free-Form Interaction Among LLM Agents](https://arxiv.org/abs/2611.31543)，2026-11-23（窗口外），**F**。
- [arXiv cs.MA 导出 API](https://export.arxiv.org/api/query?search_query=cat:cs.MA)，2026-11-25，**F**（只到 11-24；11-25 为 0 条，**U**）。
- [Formalize or Lose It](https://zenodo.org/records/24292369)，预印本 v8，2026-11-17，**F**（内容），**U**（独立复现）。

**社区与工业**

- [Paradigm 3: Swarms do not yet raise the roof](https://www.paradigm3.org/research/swarms/)，2026-11-24，作者 Niccolò Zanichelli、Gavin Leech，**F**；采用 **U**。
- [Paradigm 3 数据](https://paradigm3.org/research/swarms/data)、[代码](https://github.com/paradigm-3-institute/raising-the-roof)，**F**（链接存在）。
- [Hugging Face: Scaling Swarms to 1000s of Agents](https://huggingfaceswarms-scaling-swarms.hf.space/)，2026-11-25，**U**（平台自报）。
- [Hub（icons8）](https://github.com/icons8/hub)，2026-11-25，Apache-2.0，26 star，**F**（项目页），**U**（采用）。
- [Voynich Swarm](https://voynichswarm.com/)，2026-11-25，**F**。
- [The Agentic Leaderboard](https://www.theagenticleaderboard.com/)，2026-11-23 更新，**F**（页面），**U**（HN 标题的 56% 说法）。
- [Multiagent Coding Is the Future](https://www.kalzumeus.com/2026/11/25/multiagent-coding-is-the-future/)，2026-11-24，**F**（文章），**U**（数字自报）。
- [Infisical: Secure AI agents with hazmat suits](https://infisical.com/blog/ai-agent-security-hazmat-suits)，2026-11-02 已上过 HN，**U**（旧文重上）。
- [Antigravity changelog](https://antigravity.google/docs/changelog/)，v2.35.1，2026-11-24，**F**。
- [GitHub changelog](https://github.blog/changelog/)，2026-11-24，**F**（无 agent 条目）。
- [OpenAI news](https://openai.com/news/)，2026-11-23 至 11-25，**F**（无 agent 条目）。
- [Cursor changelog](https://cursor.com/changelog)，最新 2026-11-06，**F**。
- [Hacker News Algolia API](https://hn.algolia.com/api)，2026-11-25，**F**（帖子时间与分数）。

<a id="janus_references"></a>
## Janus 现状索引

本报告引用的 Janus 现状均按提交 `e9d9d71` 核对。

**代码**

- `backend/src/orchestrator/mcp_server.rs` 的 `initialize`：只声明 `tools`，不声明 `resources`，不返回 `instructions`。
- `backend/src/orchestrator/mcp_client.rs` 的 `VERSIONS`：最高 `2025-11-25`，没有 `2026-07-28`；`initialize` 请求只声明 `roots`。
- `backend/src/orchestrator/mcp_router.rs`：`callable` 过滤 `execution.taskSupport == "required"`；预热结果只计可调用的工具（`:255`）；请求分发只有 `initialize` / `ping` / `tools/list` / `tools/call`。
- `backend/src/library.rs` 的 `Warm::Ready { revision, tools }`：只有一个工具计数。
- `frontend/Sources/Janus/UI/Settings/LibraryView.swift:488`：显示「已准备好 · N 个工具」。
- `backend/src/teamwork.rs` 的 `check`：校验 `verifies` 指向、不能验证自己、验证者不能是执行者、验证任务不能写 `owns`、`continues` 规则、依赖成环、可能同时执行的任务 `owns` 重叠；**不**提醒「有 `owns` 但没有验证者」。
- `backend/src/teamwork.rs` 的 `task_section`：验证者提示只写「不改文件、逐条核对、写明看到什么、给出 verdict」，不提测试和代码。
- `backend/src/orchestrator/lifecycle.rs:485` 附近：交付计算 `unverified`（成功、改了文件、没有 `verifies`、没有独立验证）。
- `backend/src/orchestrator/lead.rs` 的 `SystemPart`：只有 `Collaboration` 和 `History`，没有 Skills 部分。
- `backend/src/adapters.rs` 的 `AdapterDef`：`claude` 用 `@agentclientprotocol/claude-agent-acp`。
- `backend/src/orchestrator/attempts.rs` 的 `CONTINUE_CONTEXT_LIMIT = 0.75`：上下文超过 75% 不回原会话。
- `backend/roles/pmo.md`、`qa.md`、`generalist.md`：验证方法写在数字员工指令里；PMO 要求交付前每个改了文件的子任务都核对过。

**文档**

- [目标模式：负责人知道什么](../domains/20261031-goal_mode.md#lead_brief)、[团队结构](../domains/20261031-goal_mode.md#team_structure)、[工作目录里的痕迹](../domains/20261031-goal_mode.md#traces)、[协作者的约定](../domains/20261031-goal_mode.md#collaborator_rules)、[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)。
- [Run 生命周期：一次尝试](../domains/20261031-run_lifecycle.md#attempt)、[重试、返工与失败](../domains/20261031-run_lifecycle.md#retry)、[中断与恢复](../domains/20261031-run_lifecycle.md#recovery)、[任务调度](../domains/20261031-run_lifecycle.md#scheduling)、[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)。
- [资源库：Skill](../domains/20261116-resource_library.md#skills)、[Skill 路由](../domains/20261116-resource_library.md#skill_router)、[MCP 路由](../domains/20261116-resource_library.md#mcp_router)、[Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)。
- [看板 MCP：编排校验](../interfaces/20261031-board_mcp.md#orchestration_validation)、[验收标准](../interfaces/20261031-board_mcp.md#acceptance_criteria)；[证据方法](../interfaces/20261031-events.md#evidence_methods)。
- [安全边界：工作目录访问](../operations/20261031-security.md#workspace_access)。
- [ADR：只注入协作规则](../decisions/20261125-collaboration_only_injection.md)、[ADR：断掉的会话回到原处继续](../decisions/20261125-resume_after_failure.md)、[ADR：服务重启后自动继续](../decisions/20261125-continue_after_restart.md)、[ADR：注入文字跟随应用语言](../decisions/20261125-prompt_language.md)。
- [ADR：交付时把结果合并回分支](../decisions/20261123-merge_back_on_delivery.md)；[ADR：用户要求时提交、推送和部署](../decisions/20261123-commit_push_on_request.md)。
