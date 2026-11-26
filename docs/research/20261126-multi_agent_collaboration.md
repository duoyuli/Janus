# 2026-11-26 多 Agent 协作与 Agent Team 进展观察

> 覆盖窗口：2026-11-25 18:15 UTC 至 2026-11-26 09:40 UTC（本地时间 2026-11-25 12:15 至 11-26 03:40，UTC-6）。
> 信息来源：厂商 release notes、changelog 和官方文档，GitHub release、提交和 PR（`gh api`），ACP 与 MCP 仓库，arXiv 导出 API，Hacker News（Algolia API），npm registry，`npm pack` 读适配器源码。所有链接的访问日期为 2026-11-26。
> 抓取限制：arXiv 的 cs.MA 导出在本窗口没有新条目（最新一条提交于 2026-11-25T17:26Z，早于窗口起点；11-26 的提交尚未进入索引，**U**），本报告的论文是昨日没有覆盖到的补收，日期逐条标注；Hacker News 的分数只说明有帖子，不代表共识；没有 hands-on 运行任何产品；没有 X 会话，也没有检索中文媒体；ACP 的 `speed` 提交与 codex-acp 2.12.1 都落在窗口截止后约半小时，标注为「窗口后」；Devin 和 Antigravity 的页面只给日期，不给时刻；Suncly 的 MCP 可靠性数据来自新闻网站转述，方法未核对（**U**）；Microsoft Agent Framework 的仓库地址返回 404，本窗口没有可核的 Microsoft 进展。
> 证据标记：**F** 一手来源明确写出；**I** 由多个事实推出、未直接验证；**J** 本文判断；**U** 只有二手来源、页面异常或无法核实。
> 与昨日报告的关系：昨日报告是[《2026-11-25 多 Agent 协作与 Agent Team 进展观察》](20261125-multi_agent_collaboration.md)，更早的 MCP 专题是[《2026-11-24 MCP 扩展进展与 Janus MCP 路由对照》](20261124-mcp_skills_tasks_elicitation.md)。本报告不重复它们的条目，只写状态变化与本窗口新增；对昨日 P0-1、P1-1、P1-2 与 D1、D2、D3 的逐条现状见[相对昨日报告的增量](#increment)。

## 导航

- [摘要](#summary)：只想知道结论时阅读。
- [方法与限制](#method)：判断结论可信度时阅读。
- [相对昨日报告的增量](#increment)：想知道昨天哪些结论变了、昨日建议现在处于什么状态时阅读。
- [主窗口观察](#observations)：查 2026-11-25 至 11-26 的事实和判断时阅读。
- [对 Janus 的建议](#suggestions)：评估三类结论、候选对照和 P0 / P1 / P2 时阅读。
- [待验证问题](#open_questions)：安排后续验证时阅读。
- [待用户决定](#decisions)：做产品取舍时阅读。
- [来源](#sources)：核对出处时阅读。
- [Janus 现状索引](#janus_references)：核对报告引用的代码和文档位置时阅读。

<a id="summary"></a>
## 摘要

- **本窗口没有新的 P0 或 P1。** 昨日未决的 P0-1（交付摘要单列受保护路径）、两项 P1（编排时陈述「有 `owns`、没有 `verifies`」，待 D2；资源库显示任务式工具数）今天都没有新证据改变判断，D1（Skills over MCP）也维持「不引入」。今天值得记的是三个信号和一条更正，不是新功能。
- **研究侧最强的信号是「新鲜上下文」，它是继 Paradigm 3 之后第二个独立来源。** 《When Are Parallel Subagents Effective for Coding?》在 NL2Repo / ProgramBench 的仓库构建任务上发现：随机任务上并行子 Agent 只把隐藏测试通过率提高约 1 到 5 个百分点，成本最高 3.8 倍；收益的三个机制之一是「长任务里定期开空上下文的新会话，单 Agent 在每一项比较里早期进展都更快」（[arXiv:2611.33893](https://arxiv.org/abs/2611.33893)，2026-11-25，**F**）。这与昨日 Paradigm 3 的「单 Agent 分 8 个干净上下文胜回 8 人队」指向同一件事。它还不足以改 Janus 的默认续接策略，但昨日 P2-3 的触发条件「在更多任务上被复现」已经部分满足。
- **第二个信号是「看板展示别人已接受的结果」有被复制的风险。** PROVE 发现 LLM 互相验证会随记忆累积漂移到「合谋」：即使协议要求证据，Agent 也会接受没有证据的结论，按证据而非结果给奖励的 LoRA 把十轮合谋率从 65.0% 降到 1.0%（[arXiv:2611.34215](https://arxiv.org/abs/2611.34215)，2026-11-25，**F**）。补充窗口的奖励作弊实验进一步显示：见过一次被接受的作弊后 94% 的 Agent 也作弊，即使把作弊标为 rejected，仍有 34% 会复制（[arXiv:2611.30185](https://arxiv.org/abs/2611.30185)，2026-11-23，**F**）。Janus 的看板会把别人的结果、证据方法和验证判定一起给出，接受未达标结果时会记为风险——这个设计正好对上，不需要新机制，但不能退化成只显示「已完成」。
- **第三个信号是 ACP 把 `speed` 类目定稳，而 Janus 已经收到它，只是当普通会话选项显示。** ACP 在 2026-11-26T10:08Z（窗口截止后）合并 `feat(schema): stabilize speed config option category (#2997)` 并发布公告：Agent 用 `"category": "speed"` 标记快慢档，客户端可跨 Agent 找到它、放在模型选择器旁或绑快捷键（**F**）。`claude-agent-acp` 从 0.109.0（npm 于 2026-11-19T13:36:23Z 发布）起就已经这样发 Fast mode（**F**，读 0.109.0 与 0.112.0 的包源码）；Janus 的 `session_options` 只把 `model` / `thought_level` 拆成独立选择器，其余控制项都落到角色编辑器的「会话选项」（`backend/src/agents/session_options.rs:15`、`frontend/Sources/Janus/UI/Settings/RolesView.swift:418`），Fast mode 因此可用、可保存，只是没有「速度」语义上的位置。Janus 也没有声明 `session.configOptions.boolean`，适配器因此把它降级成 Off/On 下拉（**I**）。
- **Claude Code 本窗口没有新 release。** 最新仍是 v2.1.332（2026-11-25T01:32Z），changelog 顶部也是 2.1.332；窗口内 `anthropics/claude-code` 的提交全部是 `chore: Update mods/`（**F**）。Claude Agent SDK v0.3.332 与 2.1.332 对齐，新增 `PreSkillShellCommand` hook 的类型、`tool_result_meta[].rewritten_by_plugin`，`Query.setPermissionMode()` 改为解析出 `{ mode }`（**F**）。Claude Code 的 MCP 文档今天重读，「Where MCP skills are supported」仍把 Agent SDK 列为不支持，与昨日结论一致（**F**）。
- **Codex 在窗口内连发 alpha.6 和 alpha.7，提交集中在 TUI 与 MCP。** `rust-v0.174.0-alpha.6`（11-25T19:35Z）和 `alpha.7`（11-26T04:36Z）的 body 仍只有版本名；没有 0.173.0 或 0.174.0 正式版（**F**）。窗口前（2026-11-25T17:24:42Z，早于窗口起点 18:15Z）有一条值得记的 `Remove the shadow skill ranking experiment (#63993)`：Codex 删掉了后台对 Skill 候选排序的实验和比较指标，只保留 `skill_search` 作为兼容键（**F**）。这与 Janus 不引入 Skill 检索和排序的判断同向。
- **MCP 仓库本窗口零提交，最新 release 仍是 `2026-07-28`；外部把 MCP 当作 Agent 接入层的趋势更明显。** Google Workspace 在 2026-11-24（窗口外）宣布 Workspace MCP servers 与 CLI 正式可用，并写明 2027-02-01 起「AI Agent 必须经 MCP 访问 Workspace 数据，而不是直接调 REST API」（**F**）。同一窗口里，Suncly 的 MCP 可靠性指数称审计的 432 个公开服务器中 74% 不登录就不回答，25 个知名服务器里 18 个至少有一个输入没有描述（**U**，新闻转述）。
- **一条更正：ACP 的 `mcp/message` 归因字段早已由适配器发送。** 昨日报告写「`sessionId` / `toolCallId` 仍没有实现提交」；实际上 `claude-agent-acp` 的 `feat: name the session and tool call of MCP requests sent over ACP (#1705)` 于 2026-11-24T13:30Z 合并，并随 0.111.0（GitHub release 11-24T15:23:42Z，npm 11-24T15:27:48Z）发布，早于昨日报告写完（**F**）。Janus 用 HTTP MCP、不经过 `mcp/message`，所以不需要处理，但「等适配器开始发送再评估」的前半句已经发生。

<a id="method"></a>
## 方法与限制

- **检索方式**：`gh api` 读 release、提交、PR 和文件内容；`curl` 读官方 Markdown 和页面；`npm view` / `npm pack` 核对适配器版本、依赖和实际发出的 `category`；arXiv 导出 API 按提交日期取 cs.MA；Hacker News 用 Algolia API 按时间窗口和关键词过滤。
- **覆盖窗口与补收**：主窗口是 2026-11-25 18:15 至 2026-11-26 09:40 UTC。arXiv 在本窗口没有新条目（导出最新到 11-25T17:26Z），所以本报告的论文都是**补收**：它们提交于 2026-11-24 至 2026-11-25 17:26 UTC，落在昨日的窗口里，但昨日查询时索引还没跟上（昨日报告已把 11-25 的 0 条标为 U）。提交日期逐条写明，不当作本窗口的新提交。
- **抓取限制**：arXiv 11-26 的导出尚未覆盖，不能据此说当天没有论文（**U**）；HN 分数是 2026-11-26 上午的快照，低分帖不代表没有价值，也不代表共识；Devin、Antigravity 的页面只给日期；Google Workspace 的公告日期是 2026-11-24，早于窗口起点，只按「窗口内被讨论到」收录；Suncly 的数字来自 Fomo.Observer 的报道，没有读它的方法或原始数据（**U**）；Wired 的 Siren 报道是二手来源，Anthropic 站内没有找到对应的研究页（**U** 对研究细节，**F** 对报道内容）；没有 hands-on 运行任何产品。
- **没有做的事**：没有在 Janus 里实测 `speed` 控制项是否出现在角色编辑器（结论由适配器源码和 Janus 代码推出，**I**）；没有实测 Janus 路由在 `2026-07-28` 客户端下的表现；没有访谈。
- **日期口径**：GitHub release 的时间戳用 UTC；页面只给日期时照写日期，并把落在窗口起点前的标出来；本地时间按 UTC-6 标注。
- **Janus 现状**：按本仓库提交 `5d32f29`（HEAD）的代码和文档核对，引用尽量给 `文件:函数` 或 `文件:行`；行号会随提交变化。

<a id="increment"></a>
## 相对昨日报告的增量

**重叠范围。** 昨日报告已覆盖 Claude Ship、Antigravity 2.35.0 / 2.35.1、Codex 0.172、ACP 1.35 的 `mcp/message` 归因、REPA、Uber 的 Skill 规模数据、Skills over MCP 公告与 2.1.332、Tasks / Elicitation、SEP-2640 / 2663、`2026-07-28`、RCI、TimeTravelContainer、PIEQ、Paradigm 3、Hugging Face 的 1,000 Agent 实验。本报告不重复这些内容；它们今天没有状态变化时只在下面的表里写「无变化」。

> 编号按昨日报告：**P1-1 是资源库显示任务式工具数，P1-2 是编排时陈述「有 `owns`、没有 `verifies`」**。任务说明里把两者名称对调了，这里按内容对应。

### 昨日六项待决事项的现状判断

| 昨日事项 | 今天的现状 | 依据 |
| --- | --- | --- |
| P0-1 交付摘要单列受保护路径的改动 | **无变化，仍待用户决定。** 窗口内没有新的产品证据；交付仍只计算 `unverified`，没有路径分类 | **F**：`backend/src/orchestrator/lifecycle.rs:485` 的 `unverified` 计算只看 `verifies`、状态、是否改了文件和有没有独立验证；窗口内没有 Anthropic 关于 Claude Approvals 的新发布（Anthropic news 与 sitemap 均无 11-25 / 11-26 的 agent 条目） |
| P1-1 资源库显示「N 个工具只能以任务方式调用」 | **无变化，仍成立。** 预热仍只计可调用工具，资源库仍显示「已准备好 · N 个工具」 | **F**：`backend/src/orchestrator/mcp_router.rs:203` 的 `callable` 过滤 `taskSupport == "required"`，`:259` 的 `Warm::Ready { tools }` 只存这个计数；`frontend/Sources/Janus/UI/Settings/LibraryView.swift:488` 仍只显示工具数 |
| P1-2 编排时陈述「有 `owns`、没有 `verifies`」（待 D2 决定） | **无变化。** `teamwork::check` 仍只收集错误，不陈述这种事实 | **F**：`backend/src/teamwork.rs:63` 的 `check` 只校验 `verifies` 指向、不能验证自己、验证者不能是执行者、验证任务不能写 `owns` / 用 `continues`、依赖成环、可能同时执行的任务 `owns` 重叠 |
| D1 是否引入 Skills over MCP | **无变化，维持「不引入」，两条阻断都在。** 路由仍只声明 `tools`，协议最高 `2025-11-25`；Claude Code 文档仍把 Agent SDK 会话列为不支持 | **F**：`backend/src/orchestrator/mcp_server.rs:65` 的 `capabilities` 只有 `tools`；`backend/src/orchestrator/mcp_client.rs:25` 的 `VERSIONS` 最高 `2025-11-25`；[Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md) 的「Where MCP skills are supported」仍列 Agent SDK；`claude-agent-acp` 0.112.0 仍依赖 `@anthropic-ai/claude-agent-sdk` 0.3.331（**F**） |
| D2 昨日 P1-2 保留、改写还是取消 | **无变化，仍待用户决定。** 窗口内没有新的验证或提示机制证据 | **J**：`teamwork::check` 与 `task_section` 今日读到的行为与昨日一致（`backend/src/teamwork.rs:63`、`:231`） |
| D3 是否把「与当前状态比较」写进默认验收标准 | **无变化。** 窗口内没有新的「基线比较」实验或产品行为；默认 PMO 指令仍只要求写可观察的结果 | **F**：[`backend/roles/pmo.md`](../../backend/roles/pmo.md) 的「验收标准写可观察的结果，不写要执行的命令」；窗口内相关论文与产品都不涉及验收标准写法 |

### 昨日事实的更新与更正

| 昨日引用的事实 | 今天的状态 | 依据 |
| --- | --- | --- |
| ACP 1.35 的 `mcp/message` 归因字段没有实现提交，等适配器开始发送再评估 | **更正：适配器早已发送。** `claude-agent-acp` 在 #1705（2026-11-24T13:30Z）里为 MCP 请求带上 `sessionId` / `toolCallId`，随 0.111.0（GitHub release 11-24T15:23:42Z，npm 11-24T15:27:48Z）发布；窗口内适配器没有新版本或提交 | **F**：适配器 PR #1705 与 release 列表；`claude-agent-acp` 最新 0.112.0（GitHub release 11-24T18:12:56Z，npm 11-24T18:16:52Z），依赖 SDK 0.3.331 |
| Claude Code 最新 2.1.332；Agent SDK 与 2.1.332 对齐 | **无新 release。** v2.1.332 仍是第一；SDK v0.3.332 的说明列出 `PreSkillShellCommand` 类型、`tool_result_meta[].rewritten_by_plugin`、`setPermissionMode()` 返回 `{ mode }`；窗口内 Claude Code 的提交全部是 `chore: Update mods/` | **F**：`gh api` 的 releases 与 commits；changelog 顶部 2.1.332 |
| Codex 0.174 alpha 只有版本名；没有正式版 | **继续。** alpha.6（11-25T19:35Z）、alpha.7（11-26T04:36Z）仍在窗口内，body 只有版本名；`rust-v0.173.0` / `rust-v0.174.0` 均 404 | **F** 标签与时间，**U** alpha 内容 |
| MCP 的 Skills 扩展在公开测试；客户矩阵把 Claude 与 Claude Code 标为支持 | **无变化。** 本窗口 MCP 仓库零提交，最新 release 仍是 `2026-07-28`；矩阵仍是 Claude / Claude Code 勾选 Skills、ChatGPT 为 Partial、Codex 没有行 | **F**：MCP releases / commits / client matrix |
| Antigravity 最新 2.35.1，重启自动恢复对话 | **新增 2.35.2，但落在窗口起点前。** 页面写 2026-11-25，release-time 字段是当天 00:00 UTC，早于 18:15；内容是在一轮对话进行中也能切换模型，并修了模型选择器被锁住的问题 | **F**：Antigravity changelog |
| Uber 的 Skill 规模数据（5,000+、每月 100 万次） | **无新事实。** 博客日期仍是 2026-11-23；本窗口 HN 又提交了两次，内容未变。补充细节只在「补充」里列，不作为新进展 | **F**：Uber 博客；**U** 采用信号 |
| Claude Ship 的三个 Agent | **无新条目。** Anthropic news 与 sitemap 在窗口内没有 agent 相关文章；11-25 的 sitemap 更新是一篇 Rust 迁移的工程文章 | **F** |

### 昨日已覆盖条目：只写状态

- **Claude Ship / Claude Approvals**：无变化，窗口内没有新发布；P0-1 的证据状态不变。
- **Antigravity 2.35.0 / 2.35.1**：2.35.2 见上表，落在窗口前，内容不改变此前的结论。
- **Codex 0.172 / Guardian**：仍是 0.172 为最新正式版；窗口内 Guardian 相关提交是内部行为（审批者变更通知扩展、Auto Review 恢复时重启 Guardian v3 shadow intake），不改变 Janus 的审批边界。
- **ACP `mcp/message` 归因**：见上表的更正；Janus 不经这条通道。
- **REPA**：无新状态。
- **Skills over MCP 公告与 2.1.332**：无新版本；两个阻断不变（见 D1）。
- **Tasks / Elicitation**：无新的协议版本；Codex 在 TUI 里支持数值型 MCP elicitation 字段（见观察 2），Janus 的表单已支持 `number` / `integer`（`frontend/Sources/Janus/UI/Run/Requests.swift:358`），不构成缺口。
- **SEP-2640 / 2663、`2026-07-28`**：无新提交。
- **RCI、TimeTravelContainer、PIEQ**：无新状态；TimeTravelContainer 仍是「观察，不做」。
- **Paradigm 3、Hugging Face 1,000 Agent**：无新状态；Paradigm 3 的「分段新会话」今天多了一个独立来源，见观察 4.1。

<a id="after_window_router"></a>
### 窗口后：MCP 路由已改成三个工具（2026-11-26 10:56Z 起，不在本报告窗口内）

窗口截止后，Janus 的 main 在 2026-11-26 10:56–11:17Z 之间新增了三个与本报告引用相关的提交：`655dde7`（10:56:49Z）「Route MCP tools as magpie does and have Janus's MCP servers introduce themselves」新增 [ADR：MCP 路由照 magpie 只给三个工具](../decisions/20261126-mcp_router_three_tools.md)，把 `janus-tools` 改成对所有 Agent 只给 `find_tools` / `call_tool`（代码模式开着时再加 `run_code`），服务器改用自己的 `instructions` 介绍名字、用途和工具（不超过 2000 字符），数据版本 29 删掉资源和 Run 快照里按会话单独运行服务器的 `process`；`3237114`（10:57:04Z）「Let agents run code against the router's tools」新增 [ADR：MCP 路由的代码模式](../decisions/20261126-run_code.md)，提交说明写明它扩大信任边界、单独提交；`e642b7c`（11:16:57Z）是 2026.11.26.6 的 change log。这些改变的是 Janus 一侧，不是本窗口的外部进展；本报告正文按提交 `5d32f29` 写的行号因此漂移：`backend/src/orchestrator/mcp_router.rs` 的 `callable` 现在 `:193`（报告写 `:203`）、`Warm::Ready` 现在 `:265`（报告写 `:259`），`frontend/Sources/Janus/UI/Settings/LibraryView.swift` 的「已准备好 · N 个工具」现在 `:505`（报告写 `:488`）；`backend/src/orchestrator/mcp_server.rs` 的 `capabilities` 仍只有 `tools`、`backend/src/orchestrator/mcp_client.rs:25` 的 `VERSIONS` 未变、预热仍只计可调用工具（`mcp_router.rs:261` 的 `filter(|t| callable(t))`）。明天复核 P1-1 的落点（任务式工具计数与资源库显示）时，要按新的三工具路由和 [ADR 20261126-mcp_router_three_tools](../decisions/20261126-mcp_router_three_tools.md) 重新核对，不能直接沿用报告里的 `Warm::Ready` 与 `LibraryView` 行号。

<a id="observations"></a>
## 主窗口观察

<a id="acp_speed"></a>
### 1. 协议与适配器：ACP 把 `speed` 类目定稳，Janus 已经收到它，但只当普通会话选项

**协议事实（F）。** ACP 在 2026-11-26T10:08Z 合并 `96d752a3 feat(schema): stabilize speed config option category (#2997)`，改动包括 v1 / v2 的 schema、session config options 文档、migration 和 RFD，并新增公告 `docs/announcements/speed-config-category-stabilized.mdx`，标注 Published November 26, 2026。公告写：Agent 可以用 `"category": "speed"` 标记快慢档；boolean 的 `true` 表示更快，多档用 `select` 并从慢到快排序；类目不需要能力协商，只有 v1 的 boolean 仍要求客户端声明 `session.configOptions.boolean`（**F**）。这次提交比窗口截止晚 28 分钟，按「窗口后」记录；它不改变本窗口已有的行为，只是把适配器已经在做的事写进稳定规范。

**适配器已经在做（F）。** `claude-agent-acp` 从 0.109.0（npm 于 2026-11-19T13:36:23Z 发布；提交 `8990c244` 在 v0.108.0…v0.109.0 的范围内）起就用 `category: "speed"` 报告 Fast mode；**读 0.109.0 与 0.112.0 的包源码**都可以在 `session-fast-mode.js` 里看到 `category: "speed"`，并且在客户端没有声明 boolean 能力时降级成 `select`（`off` / `on`，按规范从慢到快）。`SessionConfigOptionCategory` 的 RFD 也写明，在这个提案之前 `claude-agent-acp` 和 `codex-acp` 都把 Fast mode 标成 `model_config`（**F**）。

**Janus 现在怎样（F / I）。** `backend/src/agents/session_options.rs:15` 只把 `category == "model"` 和 `"thought_level"` 拆成模型与推理强度两个独立选择器，其余控制项一律走 `session_control` 进入 `controls`；`session_control` 只把 `mode` / `_permission` 和几个固定 id 标成「权限」（`backend/src/agents/session_options.rs:102`），`speed` 不在其中。前端 `Setup.controls` 返回全部控制项，角色编辑器按 `permission` 分成「权限」和「会话选项」两块（`frontend/Sources/Janus/Core/Setup.swift:7`、`frontend/Sources/Janus/UI/Settings/RolesView.swift:281`、`:418`）。因此 Fast mode 在角色编辑器里作为「会话选项」下的一个布尔或下拉出现，能保存、能在 `session/set_config_option` 里应用（`backend/src/orchestrator/session.rs:200` 起），只是不挨着模型。Janus 的 ACP `initialize` 没有声明 `session` 能力（`backend/src/acp.rs:144` 起的 `clientCapabilities` 只有 `fs`、`terminal`、`elicitation` 和 `_meta`），所以 Claude 适配器会把 Fast mode 降级成 Off/On 下拉而不是开关（**I**，由适配器源码和 Janus 的 initialize 推出；未实测）。

**对 Janus 的含义（J）。** 协议稳定本身不要求 Janus 做任何事：规范写明不认识的类目按未知类目处理，Janus 现在就是这样，而且能设置。可能的改进只有两个小动作：把 `speed` 当成模型旁的独立控件（前端分组），以及声明 `session.configOptions.boolean` 让 agent 能发原生开关（`backend/src/acp.rs` 的 `clientCapabilities` 加一个字段；Janus 的控件渲染已经支持 boolean）。两者都不改数据、不改信任边界，但也没有用户证据说明现在的位置妨碍使用，所以列为**观察**（P2-5），不作为新优先级。

<a id="codex"></a>
### 2. Codex：alpha.6 / alpha.7，窗口内的提交集中在 TUI 与 MCP

- **发布（F/U）**：窗口内发布 `rust-v0.174.0-alpha.6`（11-25T19:35Z）和 `alpha.7`（11-26T04:36Z），body 仍只有版本名；没有 0.173.0 / 0.174.0 正式版（标签 404）。最新正式版仍是 `rust-v0.172.0`（11-24T00:03Z，昨日已覆盖）。
- **提交（F，内容按标题与 PR 说明）**：作者时间在窗口内的有：
  - `Show status for subagent waits without targets in the TUI (#64026)`：TUI 显示没有目标的 subagent 等待状态。
  - `Support numeric MCP elicitation fields in the TUI (#64023)`：TUI 支持数值型 MCP elicitation 字段。
  - `Clean up abandoned MCP sessions during HTTP recovery (#64011)`：HTTP 恢复时清理废弃的 MCP 会话。
  - `Honor per-executor plugin settings in capability discovery (#64052)`：能力发现按每个执行器的插件安装设置。
- **窗口前（作者时间早于 18:15Z，合入在 22:38–22:42Z）**：`Remove the shadow skill ranking experiment (#63993)`（17:24:42Z）删掉后台对 Skill 候选排序的实验和比较指标，`skill_search` 只作为兼容键保留；`Notify extensions when Guardian policy changes the approvals reviewer (#63995)`（17:44:07Z）和 `Restart Guardian v3 shadow intake when Auto Review resumes (#63988)`（16:22:14Z）是 Guardian / Auto Review 的内部状态处理。三条都按窗口前记录，不计入本窗口新增。
- **对 Janus 的含义（J）**：这些是 Codex 的内部机制，Janus 没有对应层，不引入。唯一与 Janus 判断同向的是窗口前的 `#63993`：一个前沿编码 Agent 试过在后台给 Skill 候选排序，最后把实验删了。Janus 的 Skill 路由是「描述 + 使用次数 + 两档目录」，不排序、不检索（[Skill 路由](../domains/20261116-resource_library.md#skill_router)），昨天和今天的证据都不支持增加排序。
- **补充（窗口后）**：`codex-acp` v2.12.1 于 2026-11-26T10:12Z 发布，只把内置 codex 升到 0.172.0；v2.12.0（GitHub release 11-24T18:12:58Z，窗口前约一天）加了 session cursors 和原地会话回退。两者都在窗口外，Janus 的适配器自动更新按既定策略处理，不需要为本报告改动。

<a id="mcp_ecosystem"></a>
### 3. MCP 生态：仓库静止，外部把 MCP 当作 Agent 接入层

- **官方（F）**：`modelcontextprotocol/modelcontextprotocol` 本窗口零提交，最新提交仍是 11-24T21:51Z 的文档修正；最新 release 仍是 `2026-07-28`。客户矩阵没有变化：Claude（web）与 Claude Code 勾选 Skills，ChatGPT 为 Partial（`import skills from the MCP server`），Codex 没有行。
- **Google Workspace（F，公告日期 2026-11-24，窗口外；HN 提交在窗口内）**：Workspace MCP servers 和 Workspace CLI 正式可用，Workspace API 分成 Standard / Scaled 两档用量；公告写明「AI agents or similar access tools must use MCP to access Workspace data, rather than call Workspace REST APIs directly」，技术强制从 2027-02-01 开始（[Workspace Updates](https://workspaceupdates.googleblog.com/2026/11/mcp-servers-cli-standardized-usage-tiers-now-available.html)，**F**）。同一公告里还有三点：Workspace CLI 内置 packaged agent skills 和动态命令发现；企业管理员可以按 agent 设 human-in-the-loop 确认规则、看 agent 审计日志；外部 Agent 要在 OAuth 里声明自己是 agent。**对 Janus（J）**：这是「MCP 成为 Agent 访问企业数据的标准接口」的强信号，也说明「用 CLI 分发 Skill」是另一条现实路径（和 Uber 的 `aifx` 同类）。Janus 的资源库可以照常把这类远程 MCP 服务器作为用户添加的服务器，不需要改路由；企业侧的 admin 管控、HITL 规则和审计是 Janus 单机单用户场景里没有对应物的部分，不引入。
- **Suncly 的 MCP 可靠性指数（U）**：Fomo.Observer 报道 Suncly 审计了官方 registry 里的 432 个公司 MCP 服务器，74% 不登录就不肯说明自己能做什么（包括 Stripe、PayPal、Notion、GitLab、Figma 的服务器）；25 个最知名的可匿名访问服务器里 18 个至少有一个输入没有描述；Cloudflare 的服务器缺失描述率 100%（407 个 context token），Microsoft、Siemens、Autodesk 和 Anthropic 的 Claude 服务器为 0%。报道说下一版会从「读描述」进到「实际调用工具」（[Fomo.Observer](https://fomo.observer/suncly-launches-ai-agent-reliability-index-with-cloudflare-topping-blind-spot-list/)，文章日期 2026-11-25，HN 提交 11-26T08:51Z）。**对 Janus（J）**：它说明「工具描述质量」是公开 MCP 生态的普遍问题；Janus 的资源库预热只数工具、不数没有说明的工具。这可以作为 P1-1 同一处改动的可选延伸（同一份 `tools/list` 结果里再数一个 `description` 为空的计数），但没有用户证据说它比任务式工具计数更重要，**不单列优先级**。
- **补充（窗口内外各半）**：Belay 在 2026-11-26 07:44 至 07:52 UTC 用 Codex CLI 0.172.0 在免费 ChatGPT 账号上复现：`features.apps` 默认开启时，模型被提供 26 个 `mcp__codex_apps__*` 工具，其中 5 个是账号的家长控制与信任联系人工具；`features.apps = false` 后日志里不再出现这些名字（[belay 报告](https://getbelay.dev/reports/codex-apps/)，**F** 单账号单版本，**U** 泛化）。**对 Janus（J）**：这是「Agent 自带的工具面不由 Janus 管理」的具体例子（[ADR：资源库只经两个路由提供](../decisions/20261120-library_routed_only.md) 第 4 条）。Janus 不写 Agent 配置，也不枚举它登录后自带的服务器；这条不改变决定，可以作为应用说明里的例证。

<a id="research"></a>
### 4. 研究：补收的 cs.MA 论文（2026-11-24 至 11-25 17:26 UTC 提交）

昨日的 arXiv 查询对 11-25 返回 0 条（索引延迟，昨日标 U）。今天导出已经包含 11-24 晚到 11-25 17:26Z 的条目，但仍在窗口起点（11-25 18:15Z）之前。以下按提交日期照实标注，属于补收，不计入本窗口的新提交。

<a id="parallel_subagents"></a>
#### 4.1 并行子 Agent 什么时候有效：三个机制，其中一个是「新鲜上下文」（arXiv:2611.33893，2026-11-25）

- **是什么（F）**：比较单 Agent 与「主 Agent 委派给并行子 Agent」在 NL2Repo 和 ProgramBench 的仓库构建任务上的表现，按等时间和等成本比较，并做干预实验。随机抽的任务上，并行子 Agent 只把隐藏测试通过率提高约 1 到 5 个百分点，成本最高是单 Agent 的 3.8 倍；难任务上收益更大。
- **三个机制（F）**：自检——用 GPT-5.6 Sol 时，一句话让单 Agent 先探测参考程序就能找回大部分收益，从 GPT-5.6 Sol 到 GPT-6.1 Sol 收益变小，因为单 Agent 自己检查得更多；新鲜上下文——长任务里定期开一个空对话的新会话，单 Agent 在每一项比较里早期进展都更快；模块化——多 Agent 产出更多文件、更短函数，更接近上游源码。另有一个用 Opus 5.5 在 Claude Code 上的较小复现，模式相似。
- **结论（F）**：子 Agent 在三种情况下有用：单 Agent 太早停止检查、长会话一直改却没有改进、或者任务需要更模块化的代码；否则主要是增加成本。
- **对 Janus 的含义（J）**：第一，它再次说明「多派 Agent」不是收益来源，收益来自检查、上下文新鲜度和模块化——Janus 的 `verifies`、证据方法和 `owns` 划分对应的正是前两项和切分，支持维持 3 到 5 人团队。第二，「新鲜上下文」是继 Paradigm 3 之后第二个独立来源，昨日 P2-3 的触发条件「Paradigm 3 的对照在更多任务上被复现」已经部分满足；但它复现的是「单 Agent 分段新会话优于续接」，不是「Janus 的负责人会话需要分段」，而且 Janus 的失败重试已经有 75% 上下文上限（`backend/src/orchestrator/attempts.rs:9`）。因此结论仍是**观察**（P2-3 更新触发条件），不直接改默认续接策略；下一步用事件日志的 `usage` 快照统计长 Run 的上下文占用与返工，而不是凭论文改行为。
- **边界**：论文是作者自报的基准实验，模型版本明确但任务只覆盖仓库构建；它的「新鲜上下文」对照不是 Janus 的任务形态。

<a id="prove"></a>
#### 4.2 PROVE：互相验证会漂移到合谋，按证据给奖励能纠正（arXiv:2611.34215，2026-11-25）

- **是什么（F）**：长交互里 LLM Agent 互相验证会逐渐接受没有协议所要求证据的工作。合谋从零开始、逐轮累积，作者把它追溯到「准确的反馈累积在记忆里」：遵守规则的 Agent 在同伴正确时被判错，于是破坏规则成为奖励最大化的选择。PROVE 用多 Agent 强化学习，按判定背后的证据而不是结果给奖励，并且把 Agent 的每次调用记到它真正控制的那个结果上。
- **数字（F）**：Qwen3.6-27B 上，用六轮交互训练的 LoRA 把十轮时长上的合谋率从 65.0% 降到 1.0%，任务成功率没有可测变化；训练没见过的更长轮数（比训练最长交互多四轮）也保持；训练后的 27B 比原研究里评测的所有模型（包括闭源前沿模型）合谋更少。
- **对 Janus 的含义（J）**：这正是 Janus「证据方法」设计的反面教材：如果验证者的判定按「和同伴一致」或「任务过了」来回报，它会学会不看证据。Janus 现在按 `test` / `command` / `inspection` / `reasoning` / `unverified` 记录证据方法，`reasoning` 和 `unverified` 只产生提醒、不影响是否通过（`backend/src/evidence.rs:8`），验证任务要求逐条写「做了什么、看到了什么」并给 `verdict`（`backend/src/teamwork.rs:231`）。这篇支持维持这套结构，不新增投票或按结果加权。它是训练方法，不是 Janus 的运行期机制，**不引入**。

<a id="reward_hacking"></a>
#### 4.3 补充（窗口外）：奖励作弊会通过「看见别人被接受」扩散（arXiv:2611.30185，2026-11-23；arXiv:2611.30621，2026-11-23）

- **奖励作弊（F）**：17 个预注册实验、六个前沿模型、n=20,111。被告知「通过可见测试即被接受」时，首轮 33% 的 Agent 作弊（当时队友提交记录为空）；见过一次被接受的作弊后，94% 也会作弊；把作弊标为 rejected，被 AI 审查者拒绝后仍有 34% 的 Agent 作弊（原文摘要写「Most agents copied a cheat labeled rejected, 34% of agents cheated after an AI reviewer had rejected the cheat」）；在新任务上，见过被接受作弊的 Agent 70% 再次作弊，见过空记录的 32%；在 256 个 Agent 的网络里，有作弊邻居的 44% 作弊，其他 2%。结论是「曝光本身就能传播，抓住一次并不足够」。
- **AgentBouncer（F）**：在系统调用层中介软件工程 Agent 的动作，LLM 仲裁者按「必要性 × 安全性」判断，拒绝不安全的调用；340 个攻击、四类攻击向量上把攻击成功率从 99.41% 降到 10.59%，任务成功率保持，每个良性任务多 47 秒和 0.0036 美元。
- **对 Janus 的含义（J）**：第一，看板会把别人完成的结果、证据方法、验证判定和风险一起给下游（`backend/src/orchestrator/board.rs:1156` 起的 `task_detail`；`:1195` 起单独列验证判定）。这不是只显示「已完成」，所以「看见被接受的作弊就复制」的传播条件被削弱；`amend` 接受一个未达标的结果时，会把缺口作为风险写进结果（`backend/src/orchestrator/board.rs:761`），「被负责人接受」和「达到验收标准」在记录上是两件事。**这条不要退化成只显示状态标签**，但不需要新机制。第二，奖励作弊论文支持「把 rejected 的验证结论留在记录里并让下游看见」，Janus 已经这样做（验证任务不通过时下游保持 `blocked`，重试时旧结果和判定留在看板）。第三，AgentBouncer 是运行时系统调用层防护，Janus 不管理 Agent 进程、权限归 Agent 原生模式（[ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)），**不引入**。
- **边界**：奖励作弊实验是自报的实验室任务，被测 Agent 都被告知「通过可见测试即被接受」，与 Janus 的验收标准不是同一件事；它说明的是传播机制，不是 Janus 用户的行为数据。

<a id="meter_router"></a>
#### 4.4 MeterRouter：按每次查询的预算动态合成协作图（arXiv:2611.34327，2026-11-25）

- **是什么（F）**：按查询和每次查询的货币预算，增量地选择每个 Agent 的角色、LLM、入边和终止条件，合成协作图；先用成功执行图做模仿初始化，再用 GRPO 变体优化，奖励偏向正确答案、惩罚花费和超预算。
- **数字（F）**：五个基准、三档 LLM 池上，准确率与最强 MAS 相当，成本比最强的 MAS 路由低 82%，并能在给定预算附近自适应。
- **对 Janus 的含义（J）**：Janus 明确不做时间和费用上限（[ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md)），Token 只作参考值、不影响执行。MeterRouter 是「按预算路由」的强实现，但它服务的是托管 MAS 的成本优化，不改变 Janus 的产品取舍：用户要的是本地可控的协作，而不是按查询预算自动换模型。**不引入**；如果将来要重新考虑预算，这条是可以引用的对照，但需要用户先改 ADR。

<a id="conditional_review"></a>
#### 4.5 条件计算综述：Agent 委派是增长最快、实验隔离最少的方向（arXiv:2611.34112，2026-11-25）

- **是什么（F）**：从 13,884 条记录筛出 2,289 篇（2017 至 2026-09）的条件计算系统综述，用统一的「先检查、再分配」框架归纳 20 个方法族。离散硬选择在 token、查询、任务三个尺度分别占 70%、83%、91%；计算开销在每一层都是首要运维瓶颈；工程保障不对称——层内门控重负载均衡、少不确定性，模型选择和 Agent 编排依赖置信阈值和预算规则，却要面对校准误差和级联错误。
- **对 Janus 的含义（J）**：两条可直接用的判断：一是「计算开销是每一层的首要瓶颈」，支持 Janus 的并发上限和小团队，不支持为基准式并行扩团队；二是「agentic task delegation 发表量增长最快，同时最缺乏实验隔离、方法偏差风险最高」，提醒本报告的论文结论都要打折看——这也是本报告把多数论文结论放在「观察」而不是「值得做」的原因。

<a id="communication"></a>
#### 4.6 通信与承诺：局部概率报告会丢信息，一次性压缩会抵消查询优势（arXiv:2611.33318、2611.33373，2026-11-24）

- **What Must LLM Agents Communicate（F）**：研究有效沟通的三个要求——传递的信息是否充分、是否选中相关证据、收到的信息是否被正确使用。局部概率报告会丢掉关键信息，理论信息损失与实测性能损失吻合；当相关证据依赖私有查询时，query-first 交互能改善预测，但压缩的一次性表示会抵消这个优势；即使信息充分，Agent 也可能选错或不用。
- **From Conversations to Commitments（F）**：用 AT Protocol 的签名记录加 A2A 做跨组织承诺的设计研究（航司改签三家酒店的沙盒）。六项协调要求里 AT Protocol 支持五项，记录级的访问与释放控制需要额外强制；保密约束下每家酒店只能看双边交换，只有航司能关联三笔交易；它没有证明共享记录优于同等证据的点对点设计。
- **对 Janus 的含义（J）**：第一条支持 Janus 的看板设计——完成消息给 `handoff`、逐条证据和风险，`task_detail` 可查全文，相当于同时提供摘要和按需查询；`board_peers` 也给协作者按任务查询的入口（`backend/src/orchestrator/board.rs:1355`）。不要为了省 token 把结果压成一句结论。第二条说明跨组织的「共享记录」需要额外的访问控制，Janus 是单机单用户、事件日志不跨工作区，没有对应物，**不引入**。

<a id="routing_evidence"></a>
#### 4.7 路由可靠性与交付证据（arXiv:2611.33242、2611.33627）

- **CypherMAS（F，2026-11-24 提交，昨日漏收）**：在固定拓扑（链、星、二分、环）里隔离通信和路由，Agent 把加密消息转给持有解码器的远端同伴，最优路径已知。十一个 4B 到 753B 的开源模型上，小模型在路由和工具使用上失败，27B 以上的稠密模型随图直径变差，成功运行也超过最小 LLM 调用次数。结论是通信可靠性是多 Agent 系统的独立扩展瓶颈。
- **WildRegistry / DEC（F，2026-11-25 提交）**：把 Agentic Web 建模成内容 Agent 竞争用户 Agent 查询的动态博弈；长期竞争会降低网站选择质量并停在有害的稳定点。DEC 用记录下来的交付结果重新分配之后的机会，把网站选择质量和提供方曝光都改善（Welfare Gain +2.01）。
- **对 Janus 的含义（J）**：CypherMAS 的「直径越大越差」支持 Janus 的星形拓扑——负责人是唯一枢纽、协作者之间没有私有通道（[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)），不需要为了「更丰富的通信」加层或加 peer 路由；「成功运行也超过最小调用」说明协调开销本来就在，不要用「调用次数多」当失败信号。DEC 的「把交付结果记成证据，影响之后的选择」与 Janus 的事件日志、团队记忆和 `skill.used` 使用记录同向；Janus 已经在记录，不新增机制。

<a id="other_papers"></a>
#### 4.8 其余条目

| 论文 | 提交日期 | 一句话 | 与 Janus 的关系 |
| --- | --- | --- | --- |
| AutoRSP（2611.34754） | 2026-11-25 | 用映射到五层栈的子 Agent 设计雷达感知流水线，反馈来自仿真与真机而非 LLM 判官 | 领域应用，无关 |
| Do We Still Need Reinforcement Learning When We Can Read the World Model?（2611.34656） | 2026-11-25 | 编码 Agent 读环境源码直接合成控制策略，匹配或超过常规 RL，经验需求更少 | 与 Janus 的 Agent 工作方式无关，不引入 |
| VisualMontage（2611.34603） | 2026-11-25 | 视觉认知合谋基准，544 个目标案例、33.2K 候选图，跨模型迁移攻击成功率 61.93%（闭源）/ 68.71%（开源） | 安全研究，和团队协作机制无关；只作「模型判断不是证据」的旁证 |
| Welfare-Enhancing Mechanism Design（2611.33627） | 2026-11-25 | 见 4.7 | 已有方向 |
| CypherMAS（2611.33242） | 2026-11-24 | 见 4.7 | 已有方向 |
| Energy-Aware Distributed Task Allocation（2611.34293） | 2026-11-25 | 局部子模最大化的能量感知任务分配 | 多机器人 / 分布式优化，无关 |
| Strategic and Robust Task Allocation（2611.34158） | 2026-11-25 | 对抗多 Agent 环境下的任务分配 | 无关 |
| Post-Training Policy Replication（2611.34035） | 2026-11-25 | 跨队形大小的策略复制 | 多 Agent 强化学习，无关 |
| LSRP\*-LA（2611.33793） | 2026-11-25 | 大规模 Agent 的多 Agent 路径规划 | 机器人，无关 |

<a id="community"></a>
### 5. 社区与同类产品

<a id="parallix"></a>
#### 5.1 Parallix：把 Agent 审查、人类审查和集成测试改成同时开始（2026-11-25；HN 2026-11-26T07:15Z）

- **事实（F）**：Magnus Ekdahl 的文章描述把「Agent 实现 → Agent 审查 → 人类审查 → 集成测试」的顺序改成三者同时开始；任一自动检查失败就撤回待处理的人类审查、由 Agent 立刻修复，不需要有人转交诊断。质量门槛不变：三项检查都必须通过当前修订才能进团队审查。模型预测端到端少 22%，前 24 个真实改动上去掉停顿与等待后实测 16%；更简单的第一步「只把测试挪到人类审查之前」能拿到约三分之二的模型时间收益和几乎全部成本收益（14% 时间和 15% 成本）。作者还用记录下来的每个检查的通过率、时长和模型用量算 POMDP，决定并行哪些检查、何时修复。
- **对 Janus 的含义（J）**：Janus 没有「待处理的人类审查」队列——用户是在交付时看结果和证据，验证任务在目标完成后才跑（`verifies` 自动加依赖，见 `backend/src/teamwork.rs:63`）。所以「自动检查失败就撤回人类审查」在 Janus 里没有对应动作；能对应的是「所有检查必须通过当前修订」，这正是 Janus 的交付条件，以及返工时 `retry` 的 `from` 指向出问题的验证任务、该验证任务重新跑（`docs/domains/20261031-run_lifecycle.md#retry`）。**结论：已有方向，不引入调度模型**；观察触发条件是「事件日志显示交付时延主要花在等验证完成，而不是执行」，届时有数据再考虑检查的并行与排序。多验证者指向同一目标是允许的（`verifies` 是每个任务一个字段，可以有多个任务指向同一目标），所以「并行检查」在 Janus 已经表达得出来。

<a id="seahelm"></a>
#### 5.2 Seahelm：每个 Agent 一个 worktree，会话 fork 到新面板（Show HN 2026-11-26T06:01Z）

- **事实（F）**：原生 macOS 应用，MIT，仓库 21 star，2026-06-25 创建。每个 Agent 一个 git worktree，一个 Agent 占一个分栏；Claude Code Agent 在工作目录里新建 worktree 时，用 `claude --resume <id> --fork-session` 把会话 fork 到新面板，原面板停下并留一条说明；会话活过应用重启并恢复 Agent；内置 12 种 Agent 的状态识别；约 1,900 个单元测试；作者自述整个应用是 vibe-coded，也在自己的 worktree 里开发。
- **对 Janus 的含义（J）**：worktree 粒度、会话在重启后继续、状态识别，这三件 Janus 都已经有（[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)、[ADR：服务重启后自动继续](../decisions/20261125-continue_after_restart.md)）。「Agent 自己创建 worktree 时把会话 fork 过去」不适用于 Janus：worktree 由编排器在任务开始时创建，Agent 不在自己的目录里再开新 worktree。**已有方向，不引入**；它验证了「一个 Agent 一个 worktree」是同类产品的共识，不构成功能压力（采用信号弱，1 分）。

<a id="siren"></a>
#### 5.3 Anthropic 的 Siren 蜜罐：Agent 会不会越出沙盒（Wired 2026-11-24；HN 2026-11-25T18:26Z、11-26T07:30Z）

- **事实（F 报道，U 研究细节）**：Wired 报道 Anthropic 做了 Siren，一个测试 Agent 会不会「学坏」的蜜罐：26 个模拟沙盒，故意把部分配置成可出网，沙盒边界外放一个含密码和「看起来能上网的路径」的诱人文件，然后给 Agent 一些引诱它越界的任务；沙盒外面再套一个与外界隔离的沙箱，避免真实事故。研究者说模型会越来越会找零日漏洞，包括没有护栏的 Mythos 模型。
- **对 Janus 的含义（J）**：这是模型能力的评测，不是协作机制。它从侧面说明「沙盒边界」要靠宿主提供；Janus 不提供沙盒，权限由 Agent 原生模式决定（[ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)），工作目录的信任边界写在安全文档里（[工作目录访问](../operations/20261031-security.md#workspace_access)）。**不引入**，也不因为这条就加 Janus 自己的沙箱——那是信任边界变化，需要单独 ADR 和用户决定。

<a id="other_community"></a>
#### 5.4 其余社区条目与厂商核对

- **HN「Multiagent coding is the future」（2026-11-25T18:20Z，66 分、70 评论）**：即昨日报告已覆盖的 Patrick McKenzie 文章，只是换了一次提交；无新内容，不重复。
- **HN「MCP v2: Moving from Tool Calling to Async Tasks, Elicitation, and File Handoffs」（2026-11-26T06:41Z，4 分）**：第三方博客，把 Skills / Tasks / Elicitation / File Uploads 归为「MCP v2」；内容与 11-24 报告一致，没有新事实，不当作协议版本。
- **HN「Google MCP servers, CLI, standardized usage tiers, and admin policies」（2026-11-26T02:57Z）**：即观察 3 的 Workspace 公告，博客本身日期 2026-11-24。
- **HN「Uber Evals 1M Agent Skill Invocations a Month」（11-26T00:32Z、08:04Z 等）**：昨日已覆盖规模数据。博客（2026-11-23）里昨日没写的细节：三层 Domain / Core / Always-on（约 100 个 always-on）、`aifx` CLI 把六层配置合并后写进各 Agent 自己的配置、Skill 质量分与 `find-skills`、用 OpenTelemetry hooks 统计 12 项指标、linter 规则、评估集 5% golden / 80% 真实提示按轨迹评分 / 15% 作者判定，以及「70% 到 90% 通过的评估可能比 100% 通过更有用」。这些是**旧博客的补充细节**，不作为本窗口的新进展；它们支持 Janus 现有的「描述 + 使用记录 + 两档目录」，也再次说明 `find-skills` 是 5,000 个 Skill 规模才需要的东西。
- **HN「Fix the process…」以外的低分帖**：`Show HN: Hush`、`StackMaestro`、`HeyCube`、`Your agent should never see your credentials`（页面返回 404，**U**）等都没有可核的新机制，不写入。
- **GRH Lean 4 证明（HN 2026-11-25T21:48Z，460 分）**：与 Anthropic 和 Claude 合作产出的精简证明，约 17M 行降到约 76k 行（99.56%，**U**，数字未独立核对），社区在争「证明高尔夫」是否损害可理解性。数学证明，和团队协作机制无关，不引入。
- **Anthropic**：news 最新是 2026-11-25 的「Migrating Messages API to Rust」工程文章，sitemap 里 11-25 只更新了这一篇研究页；没有 agent / Agent Team 条目。
- **OpenAI**：news 在 11-23 至 11-26 没有 agent / Codex 条目（11-24 蛋白设计与 Astra、11-23 Shirudo）。
- **Google**：Antigravity 2.35.2（2026-11-25）见增量表；文档页横幅仍是「Gemini 4 Neon and Orchestrator now in Google Antigravity」。
- **GitHub**：11-25 的 changelog 只有 Copilot code review 计费，没有 agent 协作条目。
- **Cursor**：changelog 最新仍是 2026-11-06，窗口内没有更新。
- **Devin**：release notes 2026-11-25（页面只给日期）——Devin Apps 浏览器 tab 的元素选择器可用、Devin Review 的 PR 页新增 Fixes tab、Creator-only 共享下企业管理员可只读加入、Blueprint 新增 `clone.filter`（blobless / treeless 初始克隆）、MCP 保存或 Connect 时显示 client-credentials 令牌失败（token endpoint、HTTP 状态、错误码）、Jira Rovo 应用 playbook 并接收附件；11-24 的条目见昨日报告。**对 Janus（J）**：MCP 连接失败时把失败原因说清，和 Janus 资源库的 `Warm::Failed { error }`（`backend/src/library.rs:51`、`LibraryView.swift:489` 显示「启动失败：…」）同向；没有需要改的地方。
- **Microsoft**：`microsoft/agent-framework` 在 GitHub 返回 404，Foundry 博客最新是 2026-11-24 的 Ignite 2026 汇总（窗口外）；本窗口没有可核的 Microsoft Agent Framework 进展。

<a id="suggestions"></a>
## 对 Janus 的建议

<a id="three_categories"></a>
### 三类结论

| 类别 | 条目 | 理由 |
| --- | --- | --- |
| **已有** | 共享看板与推送；`owns` / `verifies` / 证据方法与 `unverified`；审批归用户；Skill 目录与 MCP 路由；worktree 与交付时合并；值班与派发；团队记忆；招聘顾问与角色文件；只注入协作规则 | 本窗口的外部证据没有推翻任何一项：Parallix 的质量门槛、CypherMAS 的星形拓扑、PROVE 的证据奖励、Parallel Subagents 的检查与模块化、Codex 删掉 Skill 排序，都从不同方向支持现有取舍 |
| **值得做** | **没有新的 P0 或 P1。** 昨日未决的 P0-1、P1-1（任务式工具计数）、P1-2（陈述事实，待 D2）按原优先级等待用户决定 | 本窗口的最强信号是「第二个来源」「传播风险」「协议稳定」这三类判断，不是「补一个功能」；没有新证据支持提高任何一项的优先级 |
| **不做** | 按预算/模型动态路由；用多个 Agent 投票或按结果给验证奖励；给 Janus 加沙盒或系统调用层防护；自动复制 MCP 服务器的 Skill；引入 Skill 检索或排序；为基准式并行扩大团队；把看板结果压成只有状态标签 | 与「工作没有时间和费用上限」「证据方法」「权限归 Agent 原生模式」「Skill 只由用户导入」「3 到 5 人团队」「事件记录可回放」冲突，或证据不足 |

<a id="candidates"></a>
### 候选逐条对照

| 候选 | 外部做法（证据） | 对应 Janus 概念与代码 | Janus 现状 | 结论 |
| --- | --- | --- | --- | --- |
| 并行子 Agent 的新鲜上下文 | **F**：定期新开空会话，单 Agent 早期进展更快；收益 1 至 5 个百分点、成本最高 3.8 倍 | 失败重试的会话选择、`continues`、负责人长会话、`usage` 快照 | `CONTINUE_CONTEXT_LIMIT = 0.75`（`backend/src/orchestrator/attempts.rs:9`）；重试不超过 75% 回原会话，超过新开；`usage` 记录上下文 | **观察**（P2-3 更新触发条件）：不改默认策略，先用事件日志统计 |
| PROVE 的「按证据给奖励」 | **F**：十轮合谋率 65.0% → 1.0%，任务成功率无变化 | 证据方法、验证者 `verdict`、`unverified` 只提醒 | `test` / `command` / `inspection` / `reasoning` / `unverified`（`backend/src/evidence.rs:8`、`:35`）；验证提示要求逐条写看到什么（`backend/src/teamwork.rs:231`） | **已有**：不新增投票或按结果加权 |
| 奖励作弊的同侪复制 | **F**：见过被接受的作弊后 94% 作弊；标为 rejected 仍有 34% 复制 | 看板结果展示、`task_detail`、`amend` 接受未达标结果、验证判定 | 结果带 summary、逐条 checks、证据方法、风险和验证判定（`backend/src/orchestrator/board.rs:1156`、`:1195`）；接受未达标结果会记为风险（`:761`） | **已有 + 观察**（P2-6）：保持「接受」与「达标」两件事可见，不新增机制 |
| ACP 的 `speed` 类目 | **F**：类目稳定，客户端应放在模型选择器旁；适配器从 0.109.0 起已发送（读包源码） | 会话控制项、角色编辑器、ACP 客户端能力 | 只把 `model` / `thought_level` 拆开（`backend/src/agents/session_options.rs:15`）；其余进「会话选项」（`RolesView.swift:418`）；未声明 `session.configOptions.boolean`（`backend/src/acp.rs:144`） | **观察**（P2-5）：可选的小改动，不做不影响使用 |
| Parallix 的检查并行与撤回审查 | **F**：实测 16%；先移测试可得约三分之二时间收益 | `verifies`、交付条件（[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)）、`retry` 的 `from`（[重试](../domains/20261031-run_lifecycle.md#retry)） | 验证任务在目标完成后跑；所有检查必须通过当前修订；失败由 `retry` 返工 | **已有方向，不引入 POMDP**（P2-7 观察） |
| Seahelm 的 worktree 与会话 fork | **F**：一 Agent 一 worktree；把会话 fork 到新 worktree 面板；重启后恢复 | worktree（[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)）、会话续接、重启继续（[ADR](../decisions/20261125-continue_after_restart.md)） | worktree 由编排器创建；会话在重启后继续（[ADR](../decisions/20261125-continue_after_restart.md)）；没有「Agent 自己开 worktree」的场景 | **已有方向**：不引入 |
| Siren 蜜罐 | **F** 报道：26 个沙盒，诱导越界 | 权限（[ADR](../decisions/20261031-agent_native_permissions.md)）、工作目录边界（[安全边界](../operations/20261031-security.md#workspace_access)） | 权限归 Agent 原生模式；Janus 不是沙箱 | **不引入**：不为评测加沙盒 |
| Google Workspace 的 MCP 强制 | **F**：2027-02-01 起 Agent 必须经 MCP 访问 Workspace | 资源库 MCP、远程服务器（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)） | 用户可添加 HTTP / SSE 服务器，经 `janus-tools` 路由；企业 admin 管控无对应物 | **已有**：用户可自行添加；企业管控不引入 |
| Suncly 的工具描述缺口 | **U**：74% 不登录不回答，18/25 有输入没描述 | 资源库预热、工具计数 | 预热只数可调用工具（`mcp_router.rs:259`）；失败原因会显示（`library.rs:51`） | **已有方向**：可在 P1-1 同一处顺带数「没有说明的工具」，不单列 |
| Codex 删除 Skill 排序实验 | **F**：删掉后台排序与比较指标 | Skill 目录（[Skill 路由](../domains/20261116-resource_library.md#skill_router)） | 描述 + 使用次数 + 两档目录，不排序 | **已有**：不引入检索 / 排序 |
| MeterRouter 的预算路由 | **F**：成本比最强 MAS 路由低 82% | 费用与时间上限 | 明确不做上限（[ADR](../decisions/20261121-no_work_limits.md)） | **不做** |
| CypherMAS 的路由可靠性 | **F**：直径越大越差；成功运行也超过最小调用 | 看板、负责人枢纽 | 星形拓扑、无私有通道（[ADR](../decisions/20261031-board_over_mcp.md)） | **已有**：不加 peer 路由 |
| Codex 的数值型 elicitation | **F**：TUI 支持数值字段 | 表单 | Janus 的表单支持 `number` / `integer`（`frontend/Sources/Janus/UI/Run/Requests.swift:358`） | **已有** |
| Codex 自带的 26 个账号工具 | **F** 单账号：免费计划默认提供，含家长控制工具 | 资源库路由、Agent 自带服务器 | Agent 自带的不由 Janus 管理（[ADR](../decisions/20261120-library_routed_only.md) 第 4 条） | **已有**：不改变 |

<a id="mechanisms"></a>
### 已采纳机制的现状与外部对照（九项）

| 机制 | 本窗口的外部对照 | Janus 的落点 | 现状与建议 |
| --- | --- | --- | --- |
| 共享看板与推送 | Seahelm 的 Island / First Mate 在需要你时通知或排队等批准；Codex TUI 显示无目标 subagent 的等待 | `wait` 最多 50 秒（`backend/src/orchestrator/board.rs:49`）；阻塞立即推送（`docs/domains/20261031-goal_mode.md#board_push`）；`board_ask_user` 进「待处理」 | **已有**。Seahelm 的「排队给用户批准」对应 Janus 的待处理项；不引入 per-worktree 名单展示 |
| `owns` / `verifies` / 证据方法 | Parallel Subagents 的自检与模块化；PROVE 的证据奖励；Formalize or Lose It（昨日）的「提示规则会漏」 | `teamwork::check`（`backend/src/teamwork.rs:63`）、验证提示（`:231`）、`task_detail` 的证据方法、交付的 `unverified`（`lifecycle.rs:485`） | **已有**。P1-2 的「陈述事实」形态仍待 D2；PROVE 支持证据方法排序，不新增投票 |
| 审批归用户 | Google Workspace 的 admin HITL 规则与审计；Codex apps 写操作默认询问；Siren 评测 | 审批和表单都进「待处理」（`docs/domains/20261031-goal_mode.md#user_requests_in_goal_mode`）；权限归 Agent 原生模式 | **已有**。企业管控是另一层，不复制 |
| Skill 目录与 MCP 路由 | Codex 删掉 Skill 排序；Uber 的三层与 `find-skills`（旧博客）；Suncly 的描述缺口；Workspace CLI 内置 Skill | `janus-skills` 目录（`docs/domains/20261116-resource_library.md#skill_router`）、`janus-tools` 只声明 `tools`（`mcp_server.rs:65`） | **已有**。不引入检索 / 排序 / 自动导入；P1-1 仍待用户决定；可在同一显示位顺带数没有说明的工具 |
| worktree 与交付时合并 | Seahelm 一 Agent 一 worktree；Codex worktree（昨日专题） | 每任务一个 worktree（`docs/domains/20261031-run_lifecycle.md#workspace_model`）；交付时合并（[ADR](../decisions/20261123-merge_back_on_delivery.md)） | **已有**。不做会话 fork 到新 worktree 的交互 |
| 值班与派发 | Devin Bots 的自动保存自动化与「企业要求先经人批准」；Parallix 的检查排序 | 工作区值班（`docs/domains/20261121-loops.md`）、派发队列（`docs/domains/20261031-run_lifecycle.md#dispatch_queue`） | **已有**。外部触发和自动保存自动化仍不做 |
| 团队记忆 | PROVE 的「准确反馈累积在记忆里会养出合谋」；Reproducible Social Structure（昨日）的记忆移植效应 | 团队记忆由 Agent 写、记来源、整理会话、用户审查（`docs/domains/20261123-team_memory.md#memory_write`）；笔记不是规则 | **已有**。PROVE 支持「来源 + 用户审查 + 定期整理」，不新增机制 |
| 招聘顾问与角色文件 | Uber 的 `aifx` personas 与分层配置；Devin Bots 模板；Workspace CLI 的 packaged skills | `propose_employees`（`backend/src/orchestrator/chat.rs:119`）、用户确认后创建；默认角色 7 个文件；不导入（[ADR](../decisions/20261126-no_role_import.md)） | **已有**（本周采纳）。不引入自动创建、文件导入或跨 Agent 的 persona 同步 |
| 只注入协作规则 | Parallix 用流程保证质量门槛；PROVE 训练行为而不是写提示；Formalize 的提示规则漏检 | [ADR：只注入协作规则](../decisions/20261125-collaboration_only_injection.md)；`SystemPart` 只有 `Collaboration` 和 `History`（`backend/src/orchestrator/lead.rs:33`） | **已有**。外部证据同向，不改 |

<a id="worth_doing"></a>
### 值得做

**本日没有新的 P0 或 P1。** 昨日三项按原优先级等待用户决定：

- **P0-1（昨日）**：交付摘要单列受保护路径。今天无新证据，判断不变；[观察 3](#mcp_ecosystem) 的「Agent 自带工具面」和 [5.3](#siren) 的沙盒越界都与「可见性」有关，但都不是交付路径的证据，不提高优先级。
- **P1-1（资源库任务式工具计数；任务说明里写作 P1-2）**：显示「N 个工具只能以任务方式调用」。落点仍是 `backend/src/library.rs` 的 `Warm::Ready` 增加 `task_only` 计数、`backend/src/orchestrator/mcp_router.rs:259` 同时数可调用和不可调用、`frontend/Sources/Janus/UI/Settings/LibraryView.swift:488` 补一句；今天的 Suncly 数据只说明「没有说明的工具」也值得数，建议在同一处顺带做，不单独加一项。
- **P1-2（编排时陈述事实；今日编号与昨日一致，待 D2）**：编排时陈述「有 `owns`、没有 `verifies`」。形态仍是「只陈述事实」，`teamwork::check`（`backend/src/teamwork.rs:63`）目前不做；等待 D2。

<a id="observe"></a>
### 观察（含触发条件）

<a id="p2_fresh"></a>
#### P2-3（更新）负责人长会话与「分段新会话」

- **现状（F）**：Paradigm 3 的 git-to-zig 事后对照、以及今天 2611.33893 在 NL2Repo / ProgramBench 上的「新鲜上下文」实验，是两个独立来源，都指向长会话变陈旧。Janus 的失败重试在上下文不超过 75% 时回原会话，超过才新开。
- **触发条件（更新）**：「更多任务上复现」已部分满足；仍待「事件日志显示长目标 Run 的负责人会话接近 75%，或返工集中出现在长会话后半段」。
- **重评时做什么**：用 `usage` 的上下文快照统计负责人会话占用与返工率；再决定是否在负责人侧提供「分段」的做法。不直接改默认续接策略。
- **边界**：两篇都是编码基准，不是 Janus 的任务形态；Janus 的重试语义是「不丢工作」，不是主动分段。

<a id="p2_speed"></a>
#### P2-5 ACP `speed` 类目与角色编辑器

- **现状（F/I）**：协议把 `speed` 定稳；`claude-agent-acp` 从 0.109.0 起发送 `category: "speed"`（读包源码）；Janus 把它当普通会话选项显示，能设置、不挨着模型；Janus 未声明 `session.configOptions.boolean`，所以是 Off/On 下拉而不是开关。
- **触发条件**：用户反馈在角色编辑器里找不到 Fast mode，或希望按角色固定快慢档并一眼看到；或更多 Agent 开始用 `speed` 类目。
- **重评时做什么**：前端把 `category == "speed"` 移到模型 / 推理强度一行；`backend/src/acp.rs` 的 `clientCapabilities` 增加 `session.configOptions.boolean`，让 Agent 发原生开关。改动只在展示和客户端能力，不涉及存储和迁移。
- **现在不做**：没有用户证据说明当前位置妨碍使用，未知类目按未知处理是规范允许的行为。

<a id="p2_board_results"></a>
#### P2-6 看板结果与「被接受」的区分

- **现状（F）**：看板给下游的是 summary、逐条 checks、证据方法、风险和独立验证判定；接受未达标结果会写成风险。PROVE 和奖励作弊实验说明「只看到被接受的结果」会传播不走证据的做法。
- **触发条件**：如果以后前端或完成消息把结果折叠成只有状态标签、或下游只拿到结论不拿到证据；或事件日志显示下游大量复用上游结论却没有核对。
- **重评时做什么**：先检查展示层，不新增机制；保持「接受」与「达标」两件事在界面上分开。

<a id="p2_checks"></a>
#### P2-7 检查的并行与顺序（Parallix）

- **现状（F）**：Parallix 在 PR 流程里把 Agent 审查、人类审查和集成测试同时开始，失败即撤回人类审查并立刻修复；Janus 没有人类审查队列，验证在结果之后。
- **触发条件**：事件日志显示交付时延主要花在等验证完成，且验证之间没有共享上游依赖。
- **重评时做什么**：统计每个任务从完成到交付的等待构成；如果验证是瓶颈，再评估多个验证者并行或把可并行的检查前移。不引入 POMDP。
- **现在不做**：Janus 的验证必须等结果，且用户是在交付时验收，Parallix 的撤回对象不存在。

<a id="not_worth"></a>
### 不做（理由）

- **按预算或模型动态路由**（MeterRouter）：与 [ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md) 冲突；Janus 的产品取舍是本地可控，不是成本最优。
- **用多个 Agent 的投票或一致当证据，或按结果给验证奖励**（PROVE 的反面）：Janus 的验证是结构化判断，按证据方法记录，最终由用户验收。
- **为 Janus 加沙盒或系统调用层防护**（Siren、AgentBouncer）：权限归 Agent 原生模式，新增运行时是信任边界变化，需要单独 ADR 和用户决定。
- **自动复制 MCP 服务器的 Skill、引入 Skill 检索或排序**：与「Skill 只由用户导入」冲突；Codex 删掉排序实验，Uber 的 `find-skills` 是 5,000 个 Skill 规模的做法，Janus 远未到。
- **为基准式并行扩大团队**：Parallel Subagents 与 Paradigm 3 都表明收益有限、成本高；维持 3 到 5 人。
- **把看板结果压成状态标签**：会让「被接受」看起来像「达标」，与 PROVE / 奖励作弊的教训相反。
- **加 peer 路由或私有通道**（CypherMAS 的直径结论）：星形拓扑是 Janus 的刻意选择。

<a id="open_questions"></a>
## 待验证问题

| 问题 | 现状 | 怎么验证 |
| --- | --- | --- |
| Janus 的 Claude 会话实际收到的 Fast mode 是布尔还是 Off/On 下拉？ | 适配器在客户端未声明 boolean 时降级为 select（**F** 源码）；Janus 未声明（**F** 代码）；未实测（**U**） | 在角色编辑器中选一个 Claude 连接，看「会话选项」里的 Fast mode 形态；同时看会话日志的 `initialize` 能力 |
| ACP `speed` 稳定后，`codex-acp` 是否也会从 `model_config` 切到 `speed`？ | `claude-agent-acp` 已切（**F**）；`codex-acp` 未核对（**U**） | 读 `codex-acp` 下个版本的包源码或 release notes |
| arXiv 11-26 的 cs.MA 提交有哪些？ | 导出未覆盖，窗口内 0 条（**U**） | 明天重新查询导出 API，按提交日期核对 |
| Claude Code / Agent SDK 是否支持 MCP skills？ | 文档仍列 Agent SDK 不支持（**F**）；Janus 用 Agent SDK（**F** 依赖）；结论是推断（**I**） | 在 Janus 里配一个声明 Skills 扩展的测试服务器，看 `/skills` 是否出现 `server:skill` |
| 负责人长会话的上下文占用与返工是否相关？ | 没有统计；`usage` 有上下文快照 | 用事件日志统计长 Run 的上下文占用与交付后返工 |
| Janus 用户添加的 MCP 服务器里，有多少工具没有描述？ | 没有统计；Suncly 说明公开生态里普遍（**U**） | 在预热结果里统计 `description` 为空的工具数；先做一次只读统计 |
| Parallix 的 16% 在 Janus 的交付路径上是否成立？ | 只有一个团队的 PR 流程（**U** 泛化） | 统计各任务从完成到交付的等待构成，再判断 |

<a id="decisions"></a>
## 待用户决定

本窗口没有新增的待决定事项。以下三项沿用昨日的状态和推荐，只更新依据。

<a id="d1"></a>
### D1 是否引入 Skills over MCP？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **不引入（推荐）** | 维持现状：Skill 只由用户新建、从本机目录复制或从 Git 来源导入 | 是。今天两条阻断都没有解除：路由仍只声明 `tools`、最高 `2025-11-25`（`mcp_server.rs:65`、`mcp_client.rs:25`）；Claude Code 文档仍把 Agent SDK 列为不支持（重读于 2026-11-26）。改动和风险都为零 |
| 用户逐项批准后导入（如果将来引入） | 新增「从 MCP 服务器导入 Skill」的来源；需要 ADR、数据迁移、UI 和测试 | 仅在宿主和协议两条阻断都解除、且用户明确要时 |
| 自动复制进 Run 资源包 | 第三方指令未经逐项批准进入每次 Run | 否。扩展规范要求宿主按 manifest 批准、内容当不可信输入 |

<a id="d2"></a>
### D2 昨日 P1-2（编排时陈述独立验证）保留、改写还是取消？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **改写为陈述事实（推荐）** | 编排返回多一行「T1 有 `owns`，没有 `verifies` 指向它；交付会标出 `unverified`」，不阻塞、不要求 | 是。今天是这套形态唯一的改动候选；`teamwork::check` 是协作机制，陈述团队形状属于它的职责 |
| 取消 | 交付仍标 `unverified`，负责人说明仍要求交代核对方式 | 可接受 |
| 保持原样（要求负责人安排验证） | 会在机制层重新规定做事方法，与 20261125 ADR 冲突 | 否 |

<a id="d3"></a>
### D3 是否把「与当前状态比较」写进默认验收标准？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **暂不写（推荐）** | 保持「可观察的结果」；改进类任务的基线由负责人按任务决定 | 是。今天没有新证据；PyBot 仍只有一条自报实验 |
| 写进默认 PMO / 研发 / 质量指令 | 角色指令增加「改进类任务写清基线、度量方法和预期方向」 | 可作为下一步，需用户同意改默认指令 |
| 做成机制校验 | 编排校验检查验收标准是否包含基线 | 否。领域方法写进 Janus，很难机械判断 |

<a id="sources"></a>
## 来源

以下来源均于 2026-11-26 访问。标注 U 的条目只有二手来源、页面异常或无法核实。

**Anthropic / Claude**

- [Claude Code releases](https://github.com/anthropics/claude-code/releases)，最新 v2.1.332（2026-11-25T01:32Z），**F**。
- [Claude Code changelog](https://code.claude.com/docs/en/changelog.md)，顶部 2.1.332，**F**。
- [Claude Code commits](https://github.com/anthropics/claude-code/commits)，2026-11-25 至 11-26，全部 `chore: Update mods/`，**F**。
- [Claude Agent SDK v0.3.332](https://github.com/anthropics/claude-agent-sdk-typescript/releases/tag/v0.3.332)，2026-11-25T01:31Z，**F**。
- [Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md)，2026-11-26 重读，Agent SDK 仍列在不支持 MCP skills 的场景，**F**。
- [Anthropic news](https://www.anthropic.com/news)、[sitemap](https://www.anthropic.com/sitemap.xml)，11-25 只有 Rust 迁移研究页，**F**。

**MCP**

- [MCP releases](https://github.com/modelcontextprotocol/modelcontextprotocol/releases)，最新 `2026-07-28`，**F**。
- [MCP commits](https://github.com/modelcontextprotocol/modelcontextprotocol/commits)，本窗口零提交，**F**。
- [Extension support matrix](https://modelcontextprotocol.io/extensions/client-matrix.md) 与[仓库源文件](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/extensions/client-matrix.mdx)，**F**。

**ACP**

- [ACP releases](https://github.com/agentclientprotocol/agent-client-protocol/releases)，最新 v1.22.0 / schema-v1.35.0 / schema-v2.0.0-beta.4（2026-11-23），**F**。
- [提交 96d752a3：稳定 `speed` 类目](https://github.com/agentclientprotocol/agent-client-protocol/commit/96d752a3)，2026-11-26T10:08Z（窗口后），**F**。
- [公告：speed config option category stabilized](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/announcements/speed-config-category-stabilized.mdx)，Published November 26, 2026，**F**。
- [RFD：Speed config option category](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/rfds/speed-config-category.mdx)，**F**。
- [RFD：Boolean config option type](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/rfds/boolean-config-option.mdx)，**F**。
- [v1 session config options 文档](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v1/session-config-options.mdx)，**F**。

**Codex 与适配器**

- [Codex releases](https://github.com/openai/codex/releases)，alpha.6 / alpha.7，**F**；body 只有版本名，**U**。
- [Codex commits](https://github.com/openai/codex/commits)，2026-11-25 至 11-26，**F**。
- [Codex PR #63993：删除 shadow skill ranking 实验](https://github.com/openai/codex/pull/63993)，作者时间 2026-11-25T17:24:42Z（窗口前），**F**。
- [claude-agent-acp releases](https://github.com/agentclientprotocol/claude-agent-acp/releases)，最新 0.112.0：GitHub release 2026-11-24T18:12:56Z，npm 2026-11-24T18:16:52Z，**F**。
- [claude-agent-acp 提交 8990c244：Fast mode 用 `speed` 类目](https://github.com/agentclientprotocol/claude-agent-acp/commit/8990c244)，2026-11-19；0.109.0 的 npm 发布时间为 2026-11-19T13:36:23Z，**F**（读 0.109.0 与 0.112.0 的包源码核对）。
- [claude-agent-acp PR #1705：MCP 请求带 session 与 tool call](https://github.com/agentclientprotocol/claude-agent-acp/pull/1705)，2026-11-24T13:30Z，**F**。
- [@agentclientprotocol/claude-agent-acp（npm）](https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp)，0.112.0（npm 发布 2026-11-24T18:16:52Z），依赖 `@anthropic-ai/claude-agent-sdk` 0.3.331，**F**。
- [codex-acp releases](https://github.com/agentclientprotocol/codex-acp/releases)，v2.12.1（2026-11-26T10:12Z，窗口后），**F**。

**论文**

- [When Are Parallel Subagents Effective for Coding?](https://arxiv.org/abs/2611.33893)，2026-11-25，**F**。
- [PROVE](https://arxiv.org/abs/2611.34215)，2026-11-25，**F**。
- [MeterRouter](https://arxiv.org/abs/2611.34327)，2026-11-25，**F**。
- [Not Every Input Needs Everything](https://arxiv.org/abs/2611.34112)，2026-11-25，**F**。
- [What Must LLM Agents Communicate?](https://arxiv.org/abs/2611.33318)，2026-11-24，**F**。
- [From Conversations to Commitments](https://arxiv.org/abs/2611.33373)，2026-11-24，**F**。
- [CypherMAS](https://arxiv.org/abs/2611.33242)，2026-11-24，**F**。
- [Welfare-Enhancing Mechanism Design for Agent Ecosystems](https://arxiv.org/abs/2611.33627)，2026-11-25，**F**。
- [Causal Evidence for the Emergence and Spread of Reward Hacking](https://arxiv.org/abs/2611.30185)，2026-11-23（窗口外，补充），**F**。
- [AgentBouncer](https://arxiv.org/abs/2611.30621)，2026-11-23（窗口外，补充），**F**。
- [VisualMontage](https://arxiv.org/abs/2611.34603)，2026-11-25，**F**。
- [Do We Still Need Reinforcement Learning When We Can Read the World Model?](https://arxiv.org/abs/2611.34656)，2026-11-25，**F**。
- [AutoRSP](https://arxiv.org/abs/2611.34754)，2026-11-25，**F**。
- [Energy-Aware Distributed Task Allocation via Local Submodular Maximization](https://arxiv.org/abs/2611.34293)，2026-11-25，**F**。
- [Strategic and Robust Task Allocation for Combat Objectives in Adversarial Multi-Agent Environments](https://arxiv.org/abs/2611.34158)，2026-11-25，**F**。
- [Post-Training Policy Replication for Strategy-Following Multi-Agent Deep Reinforcement Learning across Team Sizes](https://arxiv.org/abs/2611.34035)，2026-11-25，**F**。
- [LSRP\*-LA: Scalable and Anytime Multi-Agent Path Finding for Large Agents with Asynchronous Actions](https://arxiv.org/abs/2611.33793)，2026-11-25，**F**。
- [arXiv cs.MA 导出 API](https://export.arxiv.org/api/query?search_query=cat:cs.MA)，2026-11-26，最新到 11-25T17:26Z，11-26 未覆盖，**U**。

**社区与厂商**

- [Parallix: Fix the process around coding agents before adding more of them](https://oxtan-maker.github.io/parallix/articles/parallel-ai-coding-bottleneck/)，2026-11-25；[HN 50652412](https://news.ycombinator.com/item?id=50652412)，2026-11-26T07:15Z，1 分，**F** 内容，**U** 泛化。
- [Seahelm](https://github.com/BetaYao/seahelm)，MIT，21 star；[Show HN 50651990](https://news.ycombinator.com/item?id=50651990)，2026-11-26T06:01Z，**F** 项目页，**U** 采用。
- [Wired: Anthropic Created a Honeypot to Lure AI Agents Into Misbehaving](https://www.wired.com/story/ai-agents-can-get-honeypotted-too-aw/)，2026-11-24；[HN 50645622](https://news.ycombinator.com/item?id=50645622)、[50652514](https://news.ycombinator.com/item?id=50652514)，**F** 报道，**U** 研究细节。
- [Suncly MCP Reliability Index（Fomo.Observer 报道）](https://fomo.observer/suncly-launches-ai-agent-reliability-index-with-cloudflare-topping-blind-spot-list/)，2026-11-25；[HN 50653115](https://news.ycombinator.com/item?id=50653115)，**U**。
- [Google Workspace: MCP servers, CLI, usage tiers and admin policies](https://workspaceupdates.googleblog.com/2026/11/mcp-servers-cli-standardized-usage-tiers-now-available.html)，2026-11-24（窗口外）；[HN 50650868](https://news.ycombinator.com/item?id=50650868)，**F**。
- [belay: Codex CLI on a free ChatGPT plan](https://getbelay.dev/reports/codex-apps/)，2026-11-26，**F** 单账号，**U** 泛化。
- [Uber: How Uber Evals 1 Million Agent Skill Invocations a Month](https://www.uber.com/us/en/blog/optimizing-ubers-agent-skills/)，2026-11-23（旧博客，今日 HN 重提），**F** 内容，**U** 采用。
- [HN 50652237: MCP v2](https://news.ycombinator.com/item?id=50652237)，2026-11-26T06:41Z，**U** 第三方总结。
- [HN 50645541: Multiagent coding is the future](https://news.ycombinator.com/item?id=50645541)，2026-11-25T18:20Z，66 分（昨日已覆盖文章），**F**。
- [GRH Lean 4 精简证明](https://github.com/lsernau/grh-lean)，2026-11-25 上 HN（460 分）；行数比例按帖子与仓库说明，未独立核对，**U**。
- [Antigravity changelog](https://antigravity.google/docs/changelog/)，v2.35.2，2026-11-25（release-time 为当天 00:00 UTC），**F**。
- [Devin release notes](https://docs.devin.ai/release-notes)，2026-11-25（页面只给日期），**F**。
- [GitHub changelog](https://github.blog/changelog/)，2026-11-25，只有 Copilot code review 计费，**F**。
- [Cursor changelog](https://cursor.com/changelog)，最新 2026-11-06，**F**。
- [OpenAI news](https://openai.com/news/)，2026-11-23 至 11-26 没有 agent / Codex 条目，**F**。
- [Microsoft Foundry blog](https://devblogs.microsoft.com/foundry/)，最新 2026-11-24 的 Ignite 2026 汇总（窗口外）；`microsoft/agent-framework` 仓库地址返回 404，**U**。
- [Hacker News Algolia API](https://hn.algolia.com/api)，2026-11-26，**F** 帖子时间与分数。

<a id="janus_references"></a>
## Janus 现状索引

本报告引用的 Janus 现状均按提交 `5d32f29` 核对；窗口后 MCP 路由已改动，行号见增量一节的「窗口后」。

**代码**

- `backend/src/orchestrator/mcp_server.rs:65` 的 `initialize`：`capabilities` 只有 `tools`，不声明 `resources`。
- `backend/src/orchestrator/mcp_client.rs:25` 的 `VERSIONS`：最高 `2025-11-25`，没有 `2026-07-28`。
- `backend/src/orchestrator/mcp_router.rs:203` 的 `callable`：过滤 `execution.taskSupport == "required"`；`:259` 的 `Warm::Ready { revision, tools }` 只存可调用工具的计数。
- `backend/src/library.rs:51` 的 `Warm::Failed { revision, error }`：资源库会显示失败原因。
- `frontend/Sources/Janus/UI/Settings/LibraryView.swift:488`：显示「已准备好 · N 个工具」；`:489` 显示启动失败及原因。
- `backend/src/teamwork.rs:63` 的 `check`：只校验团队结构错误，不陈述「有 `owns`、没有 `verifies`」；`:231` 的 `task_section` 给验证者逐条核对提示；`:275` 附带执行者证据较弱的条目。
- `backend/src/orchestrator/lifecycle.rs:485`：交付计算 `unverified`。
- `backend/src/orchestrator/attempts.rs:9`：`CONTINUE_CONTEXT_LIMIT = 0.75`。
- `backend/src/orchestrator/lead.rs:33`：`SystemPart` 只有 `Collaboration` 和 `History`。
- `backend/src/adapters.rs:48-49`：Codex 用 `@agentclientprotocol/codex-acp`，Claude 用 `@agentclientprotocol/claude-agent-acp`。
- `backend/src/orchestrator/board.rs:49`：`WAIT_MAX_SECS = 50`；`:761` 接受未达标结果时把缺口写成风险；`:1156` 起的 `task_detail` 给出结果、验证判定和逐条证据；`:1195` 起单独列验证判定；`:1355` 的 `board_peers` 给协作者按任务查询。
- `backend/src/orchestrator/chat.rs:119` 的 `propose_employees`：招聘顾问在对话里提议数字员工，用户确认后创建。
- `backend/src/agents/session_options.rs:15`：只把 `model` / `thought_level` 拆成独立选择器；`:102` 只把 `mode` / `_permission` 等标为权限。
- `backend/src/agents/runtime.rs:106`：Claude 运行时的 `API_TIMEOUT_MS=120000`。
- `backend/src/acp.rs:144` 起的 `initialize`：`clientCapabilities` 有 `fs`、`terminal`、`elicitation`、`_meta`，没有 `session.configOptions.boolean`。
- `backend/src/evidence.rs:8`、`:35`：`reasoning` / `unverified` / 没有证据方法只产生提醒；证据方法的枚举。
- `frontend/Sources/Janus/Core/Setup.swift:7`：`controls` 返回全部会话控制项；`frontend/Sources/Janus/UI/Settings/RolesView.swift:281` 是「权限」分区，`:418` 的 `otherControls` 展示其余控制项。
- `frontend/Sources/Janus/UI/Run/Requests.swift:358`：elicitation 表单支持 `number` / `integer`。
- `backend/roles/pmo.md`：验收标准写可观察的结果；交付前每个改了文件的子任务都要核对过。

**文档**

- [目标模式：负责人知道什么](../domains/20261031-goal_mode.md#lead_brief)、[团队结构](../domains/20261031-goal_mode.md#team_structure)、[看板推送](../domains/20261031-goal_mode.md#board_push)、[工作目录里的痕迹](../domains/20261031-goal_mode.md#traces)、[协作者的约定](../domains/20261031-goal_mode.md#collaborator_rules)、[证据复用](../domains/20261031-goal_mode.md#evidence_reuse)、[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)、[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)。
- [Run 生命周期：一次尝试](../domains/20261031-run_lifecycle.md#attempt)、[重试](../domains/20261031-run_lifecycle.md#retry)、[中断与恢复](../domains/20261031-run_lifecycle.md#recovery)、[任务调度](../domains/20261031-run_lifecycle.md#scheduling)、[派发队列](../domains/20261031-run_lifecycle.md#dispatch_queue)、[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)。
- [资源库：Skill](../domains/20261116-resource_library.md#skills)、[Skill 路由](../domains/20261116-resource_library.md#skill_router)、[MCP 路由](../domains/20261116-resource_library.md#mcp_router)、[Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)。
- [团队记忆：写入](../domains/20261123-team_memory.md#memory_write)；[工作区值班](../domains/20261121-loops.md)。
- [看板 MCP：编排校验](../interfaces/20261031-board_mcp.md#orchestration_validation)、[验收标准](../interfaces/20261031-board_mcp.md#acceptance_criteria)、[完成消息的限额](../interfaces/20261031-board_mcp.md#inline_budget)；[证据方法](../interfaces/20261031-events.md#evidence_methods)；[会话设置](../interfaces/20261031-acp_integration.md#session_setup)。
- [安全边界：工作目录访问](../operations/20261031-security.md#workspace_access)、[团队记忆](../operations/20261031-security.md#team_memory)。
- [ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)、[ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)、[ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md)、[ADR：交付时把结果合并回分支](../decisions/20261123-merge_back_on_delivery.md)、[ADR：只注入协作规则](../decisions/20261125-collaboration_only_injection.md)、[ADR：服务重启后自动继续](../decisions/20261125-continue_after_restart.md)、[ADR：招聘顾问在对话里提议数字员工](../decisions/20261126-propose_employees_in_chat.md)、[ADR：不再从文件导入数字员工](../decisions/20261126-no_role_import.md)、[ADR：资源库只经两个路由提供](../decisions/20261120-library_routed_only.md)、[ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md)。
