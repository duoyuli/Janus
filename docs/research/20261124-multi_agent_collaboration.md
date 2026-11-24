# 2026-11-24 多 Agent 协作与 Agent Team 进展观察

> 覆盖窗口：主窗口为 2026-11-23 00:00 至 2026-11-24 报告时点（北京时间）；另有两节补充，分别覆盖 2026-11-21 至 2026-11-23 的遗漏和窗口外线索，均单独标注，不计入主窗口。
> 信息来源：厂商官方博客、文档和 changelog，GitHub release、提交与 PR，arXiv，Hacker News（Algolia API），X（opencli 已登录会话），以及 `gh api`。所有链接的访问日期为 2026-11-24，来源表逐条列出发布日期。
> 抓取限制：Perplexity 官方博客返回 403，只能用搜索摘要；X 以搜索为主，不能代表完整时间线；部分新闻稿和评测站是二手来源；没有 hands-on 运行任何产品。无法核实的条目标 **U**。
> 证据标记：**F** 一手来源明确写出；**I** 由多个事实推出、未直接验证；**J** 本文判断；**U** 只有二手来源、页面异常或无法核实。
> 与上一份报告的关系：上一份是专题报告[《2026-11-21 Codex 怎样用 git worktree》](20261121-codex_worktrees.md)，不是市场观察。本报告是新的时点快照，相对它的增量见[相对上一份报告的增量](#increment)；不重复 worktree 的内部实现和已有结论。

## 导航

- [摘要](#summary)：只想知道结论时阅读。
- [方法与限制](#method)：判断结论可信度时阅读。
- [相对上一份报告的增量](#increment)：想知道这一天新增了什么时阅读。
- [主窗口观察](#observations)：查 2026-11-23 至 11-24 的事实和判断时阅读。
- [补充：2026-11-21 至 2026-11-23 的遗漏](#supplement)：补齐上一份报告未覆盖的论文和工具时阅读。
- [窗口外线索核对](#outside_window)：核对任务线索是否属于本窗口时阅读。
- [对 Janus 的建议](#suggestions)：评估 P0 / P1 / P2 时阅读。
- [待验证问题](#open_questions)：安排后续验证时阅读。
- [待用户决定](#decisions)：做产品取舍时阅读。
- [来源](#sources)：核对出处时阅读。

<a id="summary"></a>
## 摘要

- **本日不是「无重大进展」。** Anthropic 发布 Claude Ship（[官方文章](https://claude.com/resources/articles/introducing-claude-ship)，2026-11-23，访问 2026-11-24），把 PR 的审查、修复和策略审批做成三个 Agent：Code Review（正式可用）、PR Steward（公开测试）、Claude Approvals（公开测试）。这是本窗口最重要的一条。
- **最值得 Janus 借的不是自动批准，而是「策略文件 + 受保护文件 + observe 模式」。** Claude Approvals 用 `.claude/approvals.yml` 按路径规定需要几个人批准、Claude 能否批准，并把策略文件、`REVIEW.md`、`CODEOWNERS`、`.gitattributes`、符号链接和子模块指针列为受保护文件；它自己警告，策略不要求真人时自动合并可以在没有人操作的情况下合入（[策略参考](https://code.claude.com/docs/en/approvals-policy)，访问 2026-11-24）。这与 Janus「审批始终归用户」冲突，不能照搬自动批准，但受保护路径的分类可以立即用上。
- **Janus 刚在 2026-11-23 把合并时机从验收改到交付**（[ADR](../decisions/20261123-merge_back_on_delivery.md)），包括值班派出、没有人看着的 Run；安全文档已把它写成明确接受的风险（[工作目录访问](../operations/20261031-security.md#workspace_access)）。同一天 Claude Ship 的受保护文件设计正好指出这条路径上缺的可见性：**交付摘要应单列受保护路径的改动**，但不改变合并行为。
- **验证正在变成产品默认，而不是建议。** 仓库开通 Code Review 后，Claude 会按仓库设置在 PR 上审查并做对抗性核实；Google DeepMind 在同日让多支 Gemini 4 Agent 团队的改进先经 Lean 证明，才进入共享笔记本（[DeepMind 博客](https://deepmind.google/blog/finding-faster-quantum-algorithms-with-agents-and-formal-verification/)，2026-11-23，访问 2026-11-24）。Janus 已有 `verifies`、证据方法和交付汇总，但编排校验不会提醒「改了文件却没有独立验证」，这一点仍是 2026-11-16 报告 P0-1 的未完成部分。
- **多 Agent 的收益仍然有限。** 补充窗口的 DiscernBench 显示角色化多 Agent  deliberation 把过度升级从 3.79% 提高到 28.27%，并出现「少数派信号覆盖」；REPA 显示错误会沿中间消息传播（[DiscernBench](https://arxiv.org/abs/2611.28787)，[REPA](https://arxiv.org/abs/2611.31063)，访问 2026-11-24）。这些证据支持 Janus 的小团队、清晰 `owns`、独立验证和负责人裁决，不支持扩大团队规模。
- **协议层只有小幅前进。** ACP 在 2026-11-23 发布 schema 1.35.0 和 2.0.0-beta.4，为不稳定的 `mcp/message` 请求增加可选的 `sessionId` 和 `toolCallId`；PR 明确说还没有实现发送或校验这两个字段（[PR #2988](https://github.com/agentclientprotocol/agent-client-protocol/pull/2988)，访问 2026-11-24）。Janus 用 HTTP MCP 而不是 `mcp/message`，本日无需改动。
- **同类产品出现一个值得对照的小项目。** Agent Hive 是 macOS 上的本地多 Agent 应用，复用用户已有 CLI 登录、每步并行用 worktree、最后合并，支持时间和事件触发，无遥测（[README](https://github.com/ericwang915/agent-hive)，访问 2026-11-24）；但它的 Show HN 只有 1 分、仓库约 29 个 star（[HN 50605688](https://news.ycombinator.com/item?id=50605688)，访问 2026-11-24），采用信号只能标 U。它验证了 Janus 的本地优先和 worktree 方向，不构成功能压力。

<a id="method"></a>
## 方法与限制

- **检索方式**：WebSearch 找官方页面和一手公告，WebFetch 读原文；`gh api` 读 GitHub release、提交和 PR；Hacker News 用 Algolia API 按时间过滤；X 用 `opencli twitter search` 读取公开时间线；arXiv 用导出 API 按提交日期取摘要。
- **主窗口**：2026-11-23 00:00 至 2026-11-24 报告时点。凡发布日期早于 11-23 的内容，只在「补充」或「窗口外线索核对」中出现，并注明日期。
- **抓取限制**：Perplexity 官方博客返回 403，只能用搜索摘要，标 U；X 的搜索排序噪声大，点赞和转发不能当作采用数据；HN 的分数很低，只能说明有帖子，不能说明有共识；部分新闻稿和评测站是二手来源，标 U 或只作线索。
- **没有做的事**：没有购买或 hands-on 运行 Claude Ship、Antigravity、Codex、Agent Hive、TalkTo、Perplexity Computer、Devin Bots；没有访谈；没有读取任何厂商后台数据。报告中的产品行为都来自公开文档和 release notes，Beta 行为可能在写完后变化。
- **日期口径**：GitHub release 的时间戳是 UTC，正文按页面显示的发布日期写；同一天内跨时区的条目按来源标注，不推断成「过去几小时」。
- **Janus 当前状态**：按本仓库 2026-11-24 的文档和代码核对，尽量给出章节锚点或文件路径；没有核对到的不写「已有」。

<a id="increment"></a>
## 相对上一份报告的增量

上一份 [2026-11-21 Codex 怎样用 git worktree](20261121-codex_worktrees.md) 是专题研究，覆盖 Codex 和 Antigravity 的 worktree 行为，并给出加固建议。本节只写状态变化和本报告新增的内容。

**上一份报告建议的状态**

- **已落地**：Janus 在 worktree 池根写 `.metadata_never_index`（`backend/src/worktrees.rs:86`）；所有 Git 调用关掉 `core.fsmonitor`，并清理继承的 `GIT_DIR`、`GIT_CONFIG_PARAMETERS` 等变量（`backend/src/git.rs:85-106`）。这两条对应上一份报告的 Spotlight 索引和 Git 环境加固建议。
- **未见变化**：worktree 路径仍是 `<Janus home>/work/runs/<run>`，没有把仓库目录名放进路径；变基失败时把提交绑定到命名 ref 的做法也没有在 `backend/src/worktrees.rs` 中看到。上一份报告自己把它们标为建议，不是缺陷。
- **没有新增结论**：worktree 的创建参数、`.worktreeinclude`、清理和冲突处理不再重复；本报告只引用[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)的现状。

**相对 2026-11-16 市场研究的状态变化**

- **P1-1 全局并发上限已实现**：`max_concurrent_attempts` 默认 3，`max_running_runs` 默认 8，`max_agent_sessions` 默认 12（[任务调度](../domains/20261031-run_lifecycle.md#scheduling)、[座位](../domains/20261031-run_lifecycle.md#agent_seats)、[配置](../interfaces/20261031-configuration.md#settings)）。2026-11-16 报告里「没有全局并发上限」的结论已过时。
- **2026-11-16 报告的 P1-2（Run 级费用预算）被明确否决**：2026-11-21 的 [ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md) 删掉了墙钟和美元上限、`waiting_input` 和「追加预算」，只保留 Token 参考值，且参考值不影响执行（[HTTP API](../interfaces/20261031-http_api.md)）。本报告不再建议费用预算。
- **2026-11-16 报告的 P2-1（定时 Run）以工作区值班的形式落地**：按时间表醒来的值班、派发独立 Run 和收件箱见[工作区值班](../domains/20261121-loops.md)；仍然不做外部 webhook、PR 交付和代验收。2026-11-16 的 D2「仅用户发起」因此部分变为「本地定时、用户配置」，外部触发仍未做。
- **「不要自动写入 Memory」已被部分取代**：2026-11-23 的团队记忆让负责人、协作者和值班直接用 `janus.remember` 写笔记，写入即生效，靠来源、工具校验、整理会话和用户事后审查控制风险（[团队记忆](../domains/20261123-team_memory.md#memory_write)、[ADR](../decisions/20261123-team_memory.md)）；Skill 仍然只由用户导入（[资源库](../domains/20261116-resource_library.md#skills)）。本报告按新状态分析，不重复旧结论。

**本报告新增的内容**

- Claude Ship、Claude Approvals 的受保护文件和 observe 模式；Anthropic 同日发布的企业安全指南。
- Google Antigravity 2.35.0 的会话管理 API 和 swarm board 变更；DeepMind 的 Lean 验证门禁。
- Codex 0.172.0 的 Guardian 审批审查器、内存消息板和 MCP 懒启动；OpenAI Shirudo。
- ACP 1.35.0 / 2.0.0-beta.4 的 `mcp/message` 归因字段；MCP Core Primitives 工作组章程。
- Uber 的 5,000 个 Skill、每月 100 万次调用数据；REPA 的错误传播模型；Agent Hive、TalkTo 和 T3 Code 的社区实践。
- 11-21 至 11-23 的 MASSys、DiscernBench、Insider Attacks、PAV-Twin 和 Beyond Evidence Coverage。

<a id="observations"></a>
## 主窗口观察（2026-11-23 至 2026-11-24）

<a id="claude_ship"></a>
### 1. Anthropic：Claude Ship 把 PR 的审查、修复和审批做成三个 Agent

**事实（F）**

- Anthropic 于 2026-11-23 发布 Claude Ship，定位是「让一组 Agent 帮团队把 GitHub PR 送到可合并」：Code Review 正式可用，PR Steward 和 Claude Approvals 处于公开测试；面向 Team 和 Enterprise 计划，只支持 GitHub，不提供给启用 Zero Data Retention、HIPAA 配置或客户管理密钥的组织（[官方文章](https://claude.com/resources/articles/introducing-claude-ship)、[Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，访问 2026-11-24）。
- **Code Review** 从多个角度读 PR，收集发现后做对抗性核实，再分类贴到 PR 上；仓库可选 Quick、Standard、Deep 三档（[官方文章](https://claude.com/resources/articles/introducing-claude-ship)，访问 2026-11-24）；发现分 Important、Nit、Pre-existing 三级；检查运行即使有 Important 发现也不会以失败结束（[Code Review 文档](https://code.claude.com/docs/en/code-review-pull-requests)，访问 2026-11-24）。
- **PR Steward** 处理失败的 CI、合并冲突和评审意见，推送修复，并在每次推送后评论改了什么、怎么撤销；文档明确说它「不批准也不合并 PR」（[Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，访问 2026-11-24）。
- **Claude Approvals** 读取仓库里的 `.claude/approvals.yml`，按路径规定需要几个人批准（`seats`）、Claude 是 `approves`、`comments` 还是 `blocks`，以及什么条件下换一套判定（`override`）；`mode: observe` 只报告策略会怎么判，不阻断合并（[策略参考](https://code.claude.com/docs/en/approvals-policy)，访问 2026-11-24）。
- **受保护文件**是 Approvals 内置的概念：所有策略文件和 `.claude/classifiers/`、根目录 `REVIEW.md`、`.gitmodules`、`.gitattributes`、符号链接、子模块指针和 `CODEOWNERS`。改动受保护文件时，通常至少要一名有写权限的真人；Claude 不会批准改动 `.gitattributes`、符号链接或子模块指针的 PR，无论策略怎么写（[策略参考](https://code.claude.com/docs/en/approvals-policy)，访问 2026-11-24）。
- **自动批准是可能的**：如果某条规则的 `seats` 集合允许 `bots`、`everyone` 或 `bot:claude`，且所有适用规则都写 `approves`，Claude 可以在没有真人批准的情况下让检查通过；文档在「Merges without human review」里警告，配合 GitHub auto-merge 或 merge queue 时，改动可以在没有人操作的情况下合入（[Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，访问 2026-11-24）。
- **计费和限制**：三个 Agent 都按 token 计费，管理员可为 Code Review 和 PR Steward 设花费上限；`/ship` 在 Claude Code 和桌面应用中按普通用量计费；Anthropic 建议先在 observe 模式跑，再把 Approvals 设为分支保护的必需检查（[Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，访问 2026-11-24）。
- **自报指标**：Anthropic 说 Claude Code 团队 6 月到 9 月合并的 PR 翻了一倍多，回滚比例下降超过 40%（[官方文章](https://claude.com/resources/articles/introducing-claude-ship)，访问 2026-11-24）。这是厂商自报，没有独立复现（U）。
- 同一天发布的 Claude Code 2.1.331 说明里没有 Ship 条目（[release](https://github.com/anthropics/claude-code/releases/tag/v2.1.331)，2026-11-23，访问 2026-11-24）；Ship 是组织侧开通的服务，不能从 CLI release notes 推断它的版本门槛。网传的「需要 2.1.330+」只有二手来源（[MarkTechPost](https://www.marktechpost.com/2026/11/23/anthropic-claude-ship-pull-request-agents/)，访问 2026-11-24，U）。

**对 Janus 的含义（J）**

- 对应 Janus 的概念是**独立验证（`verifies`）+ 证据方法 + 交付验收**。Janus 已有：验证者必须是另一个协作者、验证任务不能改文件、逐条给出 `method`、验证未通过会挡住下游任务、交付里汇总 `acceptance.verifications` 和 `unverified`（[团队结构](../domains/20261031-goal_mode.md#team_structure)、[证据方法](../interfaces/20261031-events.md#evidence_methods)）。
- 差距有两处：一是 Janus 不会自动审查每一个改动，验证仍是负责人按判断安排；二是 Janus 没有「按路径决定审查强度」和「受保护文件」的概念。前者是产品取舍，后者可以在不改协作模型的情况下补。
- **不能照搬的是自动批准**。Janus 的审批和表单始终交给用户，跨 Agent 消息不算授权（[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)、[ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)）。Claude Approvals 的 `seats` 在单用户 Janus 里没有对应物；能借的是路径分级、受保护文件和 observe-first 的验证方式。

<a id="anthropic_security"></a>
### 2. Anthropic：企业安全指南强调分层防御，而不是单一权限开关

**事实（F）**

- Anthropic 于 2026-11-23 发布面向安全和工程负责人的 Claude Code 企业指南，核心是「防御必须分层」：概率性的判断控制加确定性的边界（[指南](https://claude.com/resources/articles/enterprise-guide-to-securing-coding-agents)，访问 2026-11-24）。
- 权限按 deny → ask → allow 的顺序检查；托管设置里的 deny 规则不能被用户或项目设置覆盖；建议把 Bash 命令拆成部分再匹配，避免复合命令绕过规则；不建议 `Bash(python:*)` 这类宽泛放行（同上）。
- 沙箱优先用操作系统沙箱，最好开启严格模式和 `failIfUnavailable`，让沙箱不可用时不要静默地在无隔离状态下执行；限制网络出口，按 Agent 允许尽可能窄的目标；对高 stakes 操作（生产数据库迁移、IAM 或凭据变更、基础设施修改）要求人工批准；启用带工具内容的 OpenTelemetry 导出，并在试点期检查分类器拒绝、沙箱阻断和出口拒绝（同上）。
- 指南建议试点先跑两周不改策略，再用它给出的六个评估问题打分（同上）。

**对 Janus 的含义（J）**

- Janus 的 [ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md) 明确不另加读写开关；这条指南正好说明，权限质量取决于 Agent 原生模式、沙箱和托管设置，而不是编排器再包一层。Janus 不应复制一套 deny/ask/allow。
- 可以作为应用内的权限说明和角色编辑器的解释文字引用，但不值得为它改代码；本报告不把它列为建议。

<a id="google"></a>
### 3. Google：Antigravity 2.35 管会话，DeepMind 用 Lean 做验证门禁

**事实（F）**

- Antigravity 在 2026-11-23 发布 v2.35.0，主题是「无限会话历史、Conversation Management API 和 HTML artifact 搜索」：本地会话历史不再在 500 条后裁剪；新增归档运行中的会话；提供 Conversation Management API 和 conversations Skill，让 Agent、脚本、插件和自动化创建、列出、检查、重命名、固定、加星、归档、打标签、分组和删除本地会话（[changelog](https://antigravity.google/changelog.md)，访问 2026-11-24）。
- 同一版本改进 Swarm board：大 swarm 用按需消息分页，打开更快；增量更新，隐藏时减少轮询；连续并行的子 Agent 启动折叠成一行；排队消息卡片会写出占住投递的后台任务或子 Agent；Settings 里的默认 Orchestrator 和 Executor 模型改为 High thinking effort（同上）。
- Google DeepMind 在同一天宣布用多支 Gemini 4 Agent 团队改进量子算法：各团队提出改进，**先用 Lean 定理证明器验证，再进入共享笔记本**；每 4 小时开一次「虚拟会议」，交换、辩论和比较想法；开放了 Lean 库、问题规格和已验证的解（[DeepMind 博客](https://deepmind.google/blog/finding-faster-quantum-algorithms-with-agents-and-formal-verification/)，2026-11-23，访问 2026-11-24）。
- 报告的数字包括：惯性约束聚变计算成本降低 11.8 倍、分子模拟便宜 8 倍、Cytochrome P450 改进 2.5 倍、Grover 实现加速 2.57 倍、arcsin 加速 290 倍（同上）。

**对 Janus 的含义（J）**

- Antigravity 的会话管理 API 是「宿主让 Agent 管理宿主会话」，Janus 的方向相反：会话属于任务和成员，由编排器打开、续接和关闭（[一次尝试](../domains/20261031-run_lifecycle.md#attempt)、[热会话续接 ADR](../decisions/20261118-session_continuation.md)）。不需要引入会话管理 API。
- swarm board 的分页和折叠是 UI 规模问题；Janus 的看板消息有 `inline_budget` 和 `task_detail` 的限额（[完成消息的限额](../interfaces/20261031-board_mcp.md#inline_budget)），当前规模不构成问题。
- DeepMind 的「先证明再共享」在 Janus 里有等价结构：验证任务不通过时，依赖它的下游任务保持 `blocked`，不会在未确认的工作上继续（[任务调度](../domains/20261031-run_lifecycle.md#scheduling)）。差别是 Janus 的强证据方法是 `test` / `command`，没有形式化证明；考虑到用户群和数据结构成本，本报告不建议为 Lean 增加新的 `method`，只作为「机器可验证证据是方向」的信号。

<a id="openai"></a>
### 4. OpenAI：Codex 0.172 的 Guardian 审批层，以及 Shirudo 运行时

**事实（F）**

- Codex `rust-v0.172.0` 于 2026-11-24 00:03 UTC 发布（页面日期 2026-11-24），新增：为连接的 app 工具按工具设置审批审查器；Guardian V3 用看板证据做同步审批；Guardian V3 可选的内存消息板；打断父轮时保留子 Agent 已排队的回复；有缓存工具列表的可选 stdio MCP 服务器默认在第一次工具调用时懒启动；在 TUI 中打开线程链接和嵌套 Agent；Agent 命令中心用 Shift+Up/Down 跳转任务组（[release](https://github.com/openai/codex/releases/tag/rust-v0.172.0)，访问 2026-11-24）。
- OpenAI 与 Cisco 在 2026-11-23 发布 Shirudo：一个开源的 Linux 运行时，用 eBPF 让 Agent 在不重启系统的前提下调查漏洞、生成针对性的防护盾并验证效果；安全团队决定部署什么（[OpenAI 页面](https://openai.com/index/introducing-shirudo/)，访问 2026-11-24）。
- Shirudo 的边界写得很清楚：它是临时防护，不是补丁；漏洞和合法请求可能依赖同一个 Linux 操作，阻断它可能让服务下线；只适用于 Linux / eBPF，需要操作者批准，官方称它是「第一步」（同上）。

**对 Janus 的含义（J）**

- Guardian 的审批审查器和内存消息板是 Codex 内部的子 Agent 协作机制，不是跨厂商的共享通道。Janus 的看板是唯一协作通道，且没有 Agent 间私聊（[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)）；不引入 Codex 的消息板，也不把 Guardian 的策略当成 Janus 的审批策略。
- Codex 的 MCP 懒启动与 Janus 的做法一致：Janus 的 MCP 路由在第一次调用某个服务器的工具时才启动它（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)）。这条不需要改。
- Shirudo 属于 Linux 主机安全运行时，和 Janus 的本地 macOS 工作空间不在一个层面；不引入。

<a id="protocols"></a>
### 5. 协议：ACP schema 1.35，MCP 工作组章程

**事实（F）**

- ACP 在 2026-11-23 发布 schema v1.35.0（Rust crate v1.22.0）和 v2.0.0-beta.4。两者都为不稳定的 `mcp/message` 请求增加两个可选字段：`sessionId` 和 `toolCallId`；schema 约束 `toolCallId` 必须同时有 `sessionId`；文档要求 Agent 发送它知道的字段，不能猜（[1.35.0](https://github.com/agentclientprotocol/agent-client-protocol/releases/tag/schema-v1.35.0)、[PR #2988](https://github.com/agentclientprotocol/agent-client-protocol/pull/2988)，访问 2026-11-24）。
- PR #2988 在 2026-11-23 合并，说明理由是 `serverId` 只能选 MCP 服务器，不能标识 ACP 会话或工具调用；provider 可能需要会话来在正确的工作目录或沙箱里运行工具，`toolCallId` 则用来区分并发调用。PR 明确说「还没有实现发送或校验这些字段」（同上）。
- MCP 在 2026-11-23 合入 Core Primitives 工作组的章程（[提交 b0be8f0](https://github.com/modelcontextprotocol/modelcontextprotocol/commit/b0be8f0f2481be8d5ea5294cf3fa57cd03cedc48)，访问 2026-11-24）。这是治理文档，不是规范版本；MCP 最新的发布标签仍是 2026-07-28（[releases](https://github.com/modelcontextprotocol/modelcontextprotocol/releases)，访问 2026-11-24）。
- A2A 的最新发布仍是 v1.0.1（2026-05-28），AGNTCY 的 SLIM 最新是 v3.4.6（2026-10-20），两者在窗口内都没有新版本（[A2A releases](https://github.com/a2aproject/A2A/releases)、[SLIM releases](https://github.com/agntcy/slim/releases)，访问 2026-11-24）。

**对 Janus 的含义（J）**

- Janus 通过 HTTP 把 `janus-tools` 和 `janus` 两个 MCP 端点交给 Agent，使用 ACP 的 `session/new`、`session/load` 和 `_meta`，不经过 `mcp/message`（[传输](../interfaces/20261031-acp_integration.md#transport)、[ACP 的 `_meta` 扩展](../interfaces/20261031-acp_integration.md#session_meta)）。因此 ACP 1.35 的新字段本日不影响 Janus。
- 如果以后 Janus 要把 MCP 代理进 ACP 的 `mcp/message`（例如为了在客户端侧统一沙箱），这两个字段正好用于把路由调用归到会话和工具调用；到那时再升级 SDK 并读字段。当前列为 P2 观察。
- Janus 的 MCP 路由支持 `2025-11-25`、`2025-06-18`、`2025-03-26` 和 `2024-11-05`，不支持无状态版本 `2026-07-28`（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)）。这是已知限制，不是本日新增；如果以后要接无状态服务器，需要单独评估。

<a id="research_repa"></a>
### 6. 研究：REPA 把局部错误和系统失败连起来

**事实（F）**

- 2026-11-23 的 arXiv 论文《From Local Errors to System Failures》提出 REPA，一个把 Agent 的局部响应和系统级错误传播连起来的结构模型；在三个 Qwen 基座、六种多 Agent 工作流和十个知识 / 推理设置上，REPA 的条件级 Spearman 相关平均 0.84，直接回归是 0.47；用 REPA 引导的检索增强生成比不检索平均提高 19 个百分点，额外消耗 16% token（[arXiv:2611.31063](https://arxiv.org/abs/2611.31063)，访问 2026-11-24）。

**对 Janus 的含义（J）**

- Janus 的事件日志、任务 diff、`traceGaps` 和验证者的逐条证据已经把「哪个任务、哪个文件、哪条验收标准」定位得很细；REPA 的增量是「错误在哪一步被放大」的定量模型，不是 Janus 现在缺的能力。
- 可以在验证者的提示里加一句「指出问题出在哪个上游结果」，但 Janus 的 `retry` 和 `from` 已经要求负责人判断问题出在哪个子任务（[团队结构](../domains/20261031-goal_mode.md#team_structure)）。本报告不把它列为独立建议。

<a id="community"></a>
### 7. 社区与同类产品：Agent Hive、TalkTo、T3 Code 和一份调查

**事实（F / U）**

- **Agent Hive**：一个 MIT 许可的 macOS 桌面应用，用「Queen 和 bees」组织 Claude Code、Codex CLI、Gemini CLI、Antigravity、DeepSeek Harness 和 Cursor Agent；复用用户已有 CLI 登录，不带 API key，不做遥测，只在启动时检查新版本；本地服务绑定 `127.0.0.1` 并带每次启动的令牌；一个 relay 团队最多 6 个 bee 共享文件夹，评审者最多把工作退回 3 轮；同一步并行的 bee 各用自己的 git worktree，最后由一个 bee 合并；支持时间表和新提交、PR、评论、CI 失败等事件触发，带每日自动运行预算和冷却（[README](https://github.com/ericwang915/agent-hive)，访问 2026-11-24）。
- Agent Hive 在 2026-11-23 00:22 UTC 的 Show HN 只有 1 分，11-19 的帖子 2 分，仓库约 29 个 star（[HN 50605688](https://news.ycombinator.com/item?id=50605688)、[HN 50554228](https://news.ycombinator.com/item?id=50554228)、[README](https://github.com/ericwang915/agent-hive)，访问 2026-11-24）。采用信号标 U。
- **TalkTo**：面向 Agent 的消息板，提供 MCP 服务器 `https://talkto.sh/mcp`，Agent 加入服务器后在频道或直接聊天里通信；频道从不私有，Agent 之间的聊天对人类只读，任何 Agent 可以给同服务器的任何 Agent 发消息，但消息对服务器上有权限的人可见；可以自托管在 Cloudflare 账户里，Apache-2.0；README 明确说还没有独立安全审计或渗透测试（[README](https://github.com/plannotator/talkto)、[HN 50616872](https://news.ycombinator.com/item?id=50616872)，访问 2026-11-24）。
- **T3 Code 工作坊**：2026-11-23 的 X 帖子介绍一个 1 小时工作坊，演示用 T3 Code 管理 100 多个 Agent、子 Agent 构建和审查、跨机器远程运行、合并 28 个 PR（[X 帖子](https://x.com/i/status/2124880498220491122)，2026-11-23，访问 2026-11-24）。这是个人演示，没有独立评测（U）。
- **The State of Agentic Engineering 2026**：在 SF Ruby Conference（2026-11-10 至 11-12）做的自报调查，页面称 69% 的受访者每天或有时用两个以上 Agent，78% 在人工审查前让 Agent 审查代码，46% 要求提示者以外的人审查，61% 没有共享过 Agent 会话但想要共享，59% 每月为编码 Agent 支付 100 美元以上（[报告页](https://www.superconductor.com/state-of-agentic-engineering-2026)，访问 2026-11-24）。样本量和抽样方法未公开，受众偏向 Ruby / 初创 / 早期采用者，标 U。

**对 Janus 的含义（J）**

- Agent Hive 是方向上最接近 Janus 的公开产品：本地、复用登录、worktree、最后合并、无遥测。它证明这条产品路线有人做，但也说明市场很小（29 star）；Janus 的差异仍在事件溯源看板、角色、验收、交付合并和跨厂商 ACP，不在「有没有 worktree」。
- Agent Hive 的时间和事件触发、每日自动运行预算，Janus 只有本地值班和派发队列，没有事件触发和每日预算。按 2026-11-21 的 [ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md)，预算不是当前方向；事件触发属于 P2 观察，不能因为一个 29 star 的项目就改信任边界。
- TalkTo 的「直接聊天也对所有人可见」从侧面支持 Janus 的「看板是唯一通道」：即使产品提供 Agent 间消息，也选择不做私密通道。Janus 不引入 TalkTo 的私聊或消息板。
- T3 Code 的 100+ Agent 和 28 个 PR 是演示，不是可复现的团队实践；结合 Bespoke Labs Multi Agent Coordination Bench（下称 MAC Bench）和 DiscernBench，本报告不建议扩大团队规模。
- 调查的「61% 想要共享 Agent 会话」是 Janus 的潜在需求信号，但样本偏向早期采用者，不能当作市场结论；列为待验证问题。

<a id="skills_scale"></a>
### 8. Skill 路由的规模证据：Uber 的 5,000 个 Skill 和每月 100 万次调用

**事实（F）**

- Uber 在 2026-11-23 的工程博客说，它的 Agent Skill 市场从 2025 年 10 月的 1 个增长到一年内 5,000 多个，2026 年 11 月每周超过 5,300 个；Agent 在一个月内调用 Skill 超过 100 万次（[Uber 博客](https://www.uber.com/us/en/blog/optimizing-ubers-agent-skills/)，访问 2026-11-24）。
- Uber 的测量只收集 Skill 名称、版本和是 Agent 选择还是用户直接调用，不收集提示文本或文件内容；每个 Skill 有 12 个指标，包括用户、留存、成本、延迟和错误（同上）。
- 路由主要靠描述：只有名称和描述（约 100 token）常驻，Agent 用描述决定 Skill 是否适用；在数千个 Skill 时，光靠描述不够，Uber 增加了 `find-skills` Skill、基于评测和运行次数的排序、质量分和同伴使用推荐（同上）。
- 消融实验显示，去掉一个仓库查询 Skill 后，会话多 13% 的工具调用、工具错误多 6 倍、耗时多 31%；带 Skill 的回答在 87% 的盲评中胜出（同上）。
- Uber 还发现，通过率 70%-90% 的评测比 100% 的评测更有用；描述越短不一定越好，模型变强后更多文字可能降低表现，因此用盲评测试删词和删 Skill（同上）。

**对 Janus 的含义（J）**

- Janus 的 Skill 路由已经是「描述驱动 + 使用次数排序 + 两档目录」：描述最多 1536 个字符，目录在 16 KB 以内逐行列出，超过时只列名字并提供 `catalog.txt` 和 `grep` 用法；`skill.used` 记录哪些 Skill 被读过，用于排序（[Skill 路由](../domains/20261116-resource_library.md#skill_router)、[使用记录](../domains/20261116-resource_library.md#skill_uses)）。
- 本日证据支持现有方向，也给出一个规模阈值：Uber 在数千个 Skill 时才需要 `find-skills` 类工具和排序 / 质量分；Janus 的资源库是单用户、规模远小于此，暂不需要。列为 P2，不现在实现。
- Uber 的「描述是第一个路由信号」与 2026-11-20 的 [Skill 路由研究](20261120-skill_routing_practices.md) 一致；新增的是 5,000 个 Skill 的实际运行数据和 `find-skills` 的出现，不改变「不做向量检索、不做 Skill 搜索工具」的既有 ADR（[ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md)）。

<a id="supplement"></a>
## 补充：2026-11-21 至 2026-11-23 的遗漏（不属于主窗口）

这一节补齐上一份报告（2026-11-21）之后、主窗口之前出现的论文和工具。它们不在「过去一天」内，但对判断多 Agent 的可靠性和验证有价值。

**论文（F）**

- **MASSys（2026-11-21）**：同一个多 Agent 工作流在不同 serving substrate 和负载下排名会变。在受控诊断里，Star 工作流在 KV-cache 压力下从平均延迟第一掉到第四，Chain 从第四升到第一；负载升高带来长尾和超时压力，某些任务族的「有效质量」（提交任务中可用结果的比例）比存活输出的质量下降更多（[arXiv:2611.28671](https://arxiv.org/abs/2611.28671)，访问 2026-11-24）。对 Janus 的含义：本机并发、座位和派发队列不只是成本问题，也影响结果可用性；现有上限和停滞检测方向正确。
- **DiscernBench（2026-11-21）**：用航空、执法、应急等真实事件构造场景，评估 Agent 在制度约束下是服从、拒绝还是升级。角色化多 Agent  deliberation 把平均过度升级从单 Agent 的 3.79% 提高到 28.27%，并出现「少数派信号覆盖」：一个专家角色的建议可以不成比例地影响集体决定（[arXiv:2611.28787](https://arxiv.org/abs/2611.28787)，访问 2026-11-24）。对 Janus 的含义：不要用「更多角色、更多讨论」代替验证；验证者应产出可核对证据，而不是再开一场辩论。
- **Insider Attacks（2026-11-21）**：在多 Agent 辩论和共识中，一个恶意成员可以先从离线轨迹学习良性 Agent 的世界模型，再训练说服策略和规划消息；它比提示式或训练过的说服者更有效、更隐蔽，能迁移到其他模型、更大群体和未见数据集，并且在现有 Byzantine 防御下仍然有效（[arXiv:2611.28660](https://arxiv.org/abs/2611.28660)，访问 2026-11-24）。对 Janus 的含义：多 Agent 共识不是安全边界；Janus 的看板来源、任务归属、审批归用户和独立验证比「让 Agent 投票」更可靠。
- **PAV-Twin（2026-11-21）**：为 Agent 网络的数字孪生提出传播感知的监测和干预选择；在较大振荡器网络上，监测的 AUPRC 比不带传播特征的图基线高 0.13，规划器能恢复约 99% 的随机干预可改进量（[arXiv:2611.28517](https://arxiv.org/abs/2611.28517)，访问 2026-11-24）。对 Janus 的含义：错误传播建模有前景，但当前是仿真研究，不构成产品需求。
- **Beyond Evidence Coverage（2026-11-22）**：在缓存共享的多 Agent 系统里，答案时能看到全部证据 token 的表示，不等于编码时能互相访问；在两个冻结模型上，阻断编码期访问会让 MuSiQue 和 HotpotQA 的 F1 分别下降 5.6 和 6.8 个点（[arXiv:2611.29107](https://arxiv.org/abs/2611.29107)，访问 2026-11-24）。对 Janus 的含义：这是模型内部的编码问题，和 Janus 的事件、看板、验证不在一个层面；不引入。

**工具（F / U）**

- 2026-11-21 还有 ACP Rust crate v1.21.0、v1.21.1、v1.21.2 和 schema v1.34.1 的发布（[releases](https://github.com/agentclientprotocol/agent-client-protocol/releases)，访问 2026-11-24）；发布说明没有本报告需要展开的行为变化。
- Codex 在 11-21 至 11-23 还有若干 alpha 版本（例如 `rust-v0.173.0-alpha.4`、`rust-v0.172.0-alpha.2.1`），11-24 的 `0.172.0` 已包含本报告引用的行为（[releases](https://github.com/openai/codex/releases)，访问 2026-11-24）。

**判断（J）**：补充窗口的证据整体上削弱「更多 Agent、更多讨论会更好」的假设，支持 Janus 当前的 3 到 5 人团队、`owns`、`verifies`、证据和人工审批。没有出现需要改动协作模型的发现。

<a id="outside_window"></a>
## 窗口外线索核对

以下是任务线索中提到的条目。多数早于 2026-11-23，不属于本日窗口，也没有被上一份 2026-11-21 报告覆盖；其中 Atos 的发布在 2026-11-24、属于主窗口，但它是企业服务和咨询，不构成 Janus 可借鉴的产品能力；xAI 在主窗口内没有找到官方更新。列在这里是为了说明「已核对但不计入」，不是用旧闻填充。

| 线索 | 实际日期 | 核实结果 | 处理 |
| --- | --- | --- | --- |
| Bespoke Labs Multi Agent Coordination Bench | 2026-11-18 | 12 个原生多 Agent 长任务；单 Agent 0.35，8 个协调 Agent 0.42，8 个不协调 Agent 0.44；8 Agent 团队成本约 9 倍，达到单 Agent 分数只快约 2.2 倍；只有 Opus 5.5 团队从消息中获益（[benchmark](https://benchmarks.bespokelabs.ai/multi-agent-coordination-bench)、[GitHub](https://github.com/bespokelabsai/multi-agent-coordination-bench)，访问 2026-11-24） | 窗口外；作为「协调收益有限」的旁证引用，不重复计入本日进展 |
| Perplexity Computer Multiagent | 2026-11-17 | 官方博客返回 403，只有搜索摘要称最多 16 个 Agent、混合前沿模型、Max 订阅、Web 先行（[官方博客](https://www.perplexity.ai/hub/blog/introducing-computer-multiagent)，访问 2026-11-24，U） | 只作线索，不写入结论 |
| Microsoft Ignite 2026：Agent 365 / Foundry | 2026-11-17 至 11-20 | Agent 365 扩展为跨 Microsoft 和第三方 Agent 的观测与治理面，50M+ 注册 Agent（F）；Foundry Agent Service 增加托管 Agent、A2A 工具、计算机使用等，以及「没有新的 Microsoft Agent Framework 1.0 发布」只有搜索摘要（U）（[Agent 365](https://techcommunity.microsoft.com/blog/agent-365-blog/whats-new-in-microsoft-agent-365-ignite-2026/4571753)、[Foundry](https://devblogs.microsoft.com/foundry/whats-new-in-foundry-agent-service-at-ignite-2026/)，访问 2026-11-24） | 窗口外；企业控制面，与 Janus 单机单用户不在同一层 |
| AWS Quick 多 Agent 编排 | 2026-11-20 | Flows 可以编排 Agent、Skill 和工具，支持确定性步骤和规则校验；页面日期为 2026-11-20（[What's New](https://aws.amazon.com/about-aws/whats-new/2026/11/aws-quick-flows-automations-chat/)，访问 2026-11-24） | 窗口外；托管工作流，不引入 |
| Cognition Devin Bots | 2026-11-17 | 持久 Agent 作为「工程队友」，带自己的记忆和工具，领导多个并行 Devin 会话，保留跨会话上下文，只把需要判断的决定交给用户，提供 Needs You 收件箱；官方强调需要评估循环和明确的自主边界（[官方博客](https://devin.ai/blog/introducing-devin-bots)，访问 2026-11-24） | 窗口外；云端 VM 团队模型，和 Janus 本地 worktree 不同 |
| Cursor Projects multiplayer | 2026-11-10 | 团队成员共享 Project、任务板和上下文层，各自保留 coordinator / worker Agent 和计费（[官方博客](https://cursor.com/blog/projects-multiplayer)，访问 2026-11-24）；[changelog](https://cursor.com/changelog) 在 11-23 没有多 Agent 条目 | 窗口外 |
| Red Hat 企业 Agent 编排平台 | 2026-11-09 至 11-12 | 实际是 OpenShift 5 的 Agent 管理工具，包括 MCP server / gateway、Agent 沙箱（技术预览）和 Lightspeed；「企业 Agent 编排平台」的说法只出现在二手报道（[Network World](https://www.networkworld.com/article/4247825/red-hat-joins-race-to-be-air-traffic-controller-for-ai-agents.html)，访问 2026-11-24，U） | 名称按二手来源处理，不当作独立产品发布 |
| GitHub Agent HQ | 2026-10-28（Universe 2026） | Agent HQ 的多 Agent 更新在 2026-10-28，不在 11 月；11-23 的 GitHub changelog 只有 issue 状态、仓库看板、secret scanning 等条目，没有 Agent HQ（[changelog](https://github.blog/changelog/)，访问 2026-11-24） | 窗口外；11-23 无 Agent HQ 更新 |
| Atos Sovereign Agentic Studio | 2026-11-24（中国区） | 在中国推出 Sovereign Agentic Studio，包含 Agentic Business Orchestrator 和 AI Control Plane（50+ LLM provider、320+ 模型、成本 / 延迟 / 预算治理）；全球首发是 2026-03-12（[新闻稿](https://www.globenewswire.com/news-release/2026/11/24/3409007/0/en/atos-launches-its-sovereign-agentic-studio-in-china-to-accelerate-enterprise-ai-from-pilot-to-scale.html)，访问 2026-11-24，U（只有搜索摘要）） | 在主窗口内，但是企业服务和咨询，不是 Janus 可借鉴的产品能力 |
| xAI Grok Bot | — | 2026-11-23 至 11-24 没有找到官方更新；11-23 的 X 讨论只有用户帖（U） | 本日无进展；D3 结论不变 |

<a id="suggestions"></a>
## 对 Janus 的建议

<a id="three_categories"></a>
### 三类结论

| 类别 | 条目 | 理由 |
| --- | --- | --- |
| **已有** | 独立验证 `verifies`、证据方法、`traceGaps`、交付 `acceptance`；审批归用户、权限归 Agent 原生模式；看板是唯一通道；worktree 和交付时合并；并发上限、座位、派发队列；团队记忆；Skill / MCP 路由；工作区值班；会话续接和事件回放 | 这些能力在 2026-11-16 之后陆续落地，本日的外部进展没有推翻它们；Claude Ship、TalkTo、Antigravity 的设计从侧面确认了其中几条 |
| **值得做** | P0-1 交付摘要单列受保护文件；P1-1 编排时提醒「改了文件但没有独立验证」；P2-1 验收策略文件（observe-only，需用户决定）；P2-2 ACP 1.35 归因字段观察；P2-3 Skill 目录规模阈值复核 | 前两项改动小、直接补本日暴露的可见性缺口；后三项需要先观察或先由用户决定 |
| **不值得做** | AI 自动批准；PR Steward 式自动修复并合并；100+ Agent 团队；Agent 间私聊 / 私有消息板；云端企业控制面；Shirudo 式 eBPF 运行时；自动写入 Skill；现在增加 `proof` 证据方法 | 与 Janus 的审批归用户、单用户、本地优先、看板唯一通道、事件可回放等约束冲突，或证据不足 |

<a id="candidates"></a>
### 候选逐条对照

| 候选 | 证据 | 对应 Janus 概念 | Janus 当前状态 | 结论 |
| --- | --- | --- | --- | --- |
| Claude Ship 的受保护文件 | F：策略文件和 `.claude/`、`REVIEW.md`、`.gitmodules`、`.gitattributes`、符号链接、子模块指针、`CODEOWNERS` 改动需要真人 | 交付摘要、验收、worktree 合并 | 交付有 `changed` 文件列表，但没有路径分类；交付时合并是明确接受的风险（[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)、[安全边界](../operations/20261031-security.md#workspace_access)） | **P0-1**：只显示，不改变合并 |
| Claude Code Review 的多视角和对抗性核实 | F：多角度读 PR，收集发现后对抗性核实，分 Important / Nit / Pre-existing | 独立验证、证据方法 | 已有 `verifies`、`method`、`traceGaps`；验证者提示没有多视角要求（`backend/src/teamwork.rs`） | **P1-1** 的一部分：在验证者提示和编排校验里使用，不新增事件 |
| Claude Approvals 的策略文件 | F：按路径设 `seats`、`approves` / `comments` / `blocks`、`override`、observe 模式 | 验收规则、交付条件 | 没有路径级验收策略，验收由用户点按钮（[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)）；单用户没有 `seats` 对应物 | **P2-1**：observe-only 的策略文件，需用户决定；不引入自动批准 |
| ACP 1.35 的 `sessionId` / `toolCallId` | F：可选字段，PR 说没有实现发送或校验 | ACP 集成、MCP 路由 | 用 HTTP MCP，不经过 `mcp/message`（[传输](../interfaces/20261031-acp_integration.md#transport)） | **P2-2**：观察，等适配器实现 |
| Codex 0.172 的 Guardian 审批和消息板 | F：按工具审批审查器、Guardian V3 看板证据、可选内存消息板 | 审批、看板 | 审批始终交用户（[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)）；看板唯一通道（[ADR](../decisions/20261031-board_over_mcp.md)） | **不引入**：内部机制，与单用户和唯一通道冲突 |
| Antigravity 2.35 的会话管理 API 和 swarm board | F：Agent 可管理宿主会话；看板按需分页、折叠并行子 Agent | 会话、看板 | 会话属于任务 / 成员（[一次尝试](../domains/20261031-run_lifecycle.md#attempt)）；看板有消息限额和 `task_detail`（[限额](../interfaces/20261031-board_mcp.md#inline_budget)） | **不引入**：Janus 的宿主 / Agent 分工相反；当前规模不需要分页 |
| DeepMind 的 Lean 验证门禁 | F：团队改进先经 Lean 证明，再进共享笔记本 | 独立验证、交付条件 | 验证不通过时下游 `blocked`（[任务调度](../domains/20261031-run_lifecycle.md#scheduling)）；证据方法没有 `proof`（[证据方法](../interfaces/20261031-events.md#evidence_methods)） | **不引入（现在）**：机制已有等价物，新增证据方法需要数据升级 |
| Uber 的 Skill 规模数据 | F：5,000+ Skill、100 万次调用 / 月、描述路由、`find-skills`、质量分 | Skill 路由、使用记录 | 描述 + 使用次数 + 两档目录（[Skill 路由](../domains/20261116-resource_library.md#skill_router)） | **P2-3**：设规模阈值，超过再评估；不现在做 |
| REPA 错误传播模型 | F：局部响应到系统错误，0.84 vs 0.47 相关 | 验证、重试、痕迹 | `traceGaps`（[工作目录里的痕迹](../domains/20261031-goal_mode.md#traces)）、验证者逐条证据、`retry` 的 `from`（[重试](../domains/20261031-run_lifecycle.md#retry)） | **不引入**：定位能力已够；可作为验证者提示的一句话 |
| Agent Hive | F 产品行为，U 采用信号：本地、worktree、最后合并、触发、每日预算、无遥测 | 工作区、worktree、值班 | 本地、worktree（[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)）、交付合并、工作区值班（[值班](../domains/20261121-loops.md)）；没有事件触发和每日预算 | **已有方向，不追功能**：29 star，信任边界变化需要单独决定 |
| TalkTo | F：Agent 消息板，频道和直接聊天都对有权限的人可见，Agent 间聊天人类只读 | 看板 | 看板是唯一通道，没有私聊（[ADR](../decisions/20261031-board_over_mcp.md)） | **不引入**：不增加私有通道；「不私密」这一点与 Janus 一致 |
| Shirudo | F：Linux eBPF 运行时，Agent 生成临时防护，需操作者批准 | 权限、沙箱 | 权限归 Agent 原生模式（[权限模式](../domains/20261031-agents_and_roles.md#permission_modes)）；macOS 本地 | **不引入**：不在同一层 |
| Anthropic 企业安全指南 | F：deny / ask / allow、托管 deny、沙箱严格模式、出口白名单、高 stakes 人工批准 | 权限模式、应用说明 | 权限由 Agent 原生模式决定（[ADR](../decisions/20261031-agent_native_permissions.md)） | **不引入**：不复制权限层；可作为说明文字 |

<a id="worth_doing"></a>
### 值得做

<a id="p0_protected_paths"></a>
#### P0-1 交付摘要单列受保护路径的改动（只显示，不改变合并）

- **为什么**：Janus 从 2026-11-23 起在每次交付时把结果合并回用户的分支，包括值班派出、没有人看着的 Run；安全文档把「Agent 写的代码在用户核对之前就进分支」写成明确接受的风险（[ADR](../decisions/20261123-merge_back_on_delivery.md)、[安全边界](../operations/20261031-security.md#workspace_access)）。Claude Ship 在同一天把策略文件、评审说明、`.gitattributes`、符号链接和子模块指针列为受保护文件，并在文档里警告「策略不要求真人时，自动合并可以在没有人操作的情况下合入」（[策略参考](https://code.claude.com/docs/en/approvals-policy)，访问 2026-11-24）。Janus 不需要自动批准，但需要让用户在交付卡片上看到「这一轮改了会改变自动化或信任边界的文件」。
- **落点**：`frontend/Sources/Janus/UI/Run/RunView.swift` 的交付卡片（现有 `ReviewBody.factsLine` 和 `acceptance` 汇总旁边）；[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode) 增加一段说明；[安全边界](../operations/20261031-security.md#workspace_access) 写明这是显示项、不是阻断。数据从 `run.delivered` 的 `changed` 和当前工作目录推导，不新增事件类型，不改数据库。
- **受保护路径的第一版建议**：`.github/workflows/**`、`AGENTS.md`、`CLAUDE.md`、`.claude/**`、`.agents/**`、`.mcp.json`、`.gitmodules`、`.gitattributes`、`.worktreeinclude`、符号链接和子模块指针。列表要用户确认；不要把它说成「安全 / 不安全」的判定，只说「会改变自动化或信任边界」。
- **成功指标**：交付卡片中，改动命中受保护路径的交付 100% 出现单独一行；用户从交付到验收的时间不显著增加；没有因为这一行产生自动阻断；端到端场景覆盖一次 `.github/workflows/` 或 `.gitattributes` 改动。
- **风险 / 边界**：路径分类是启发式，符号链接和子模块指针需要读工作目录；不要把它做成新的审批门，也不要改交付时合并的行为——那会推翻 2026-11-23 的 ADR，属于待用户决定。

<a id="p1_verification"></a>
#### P1-1 编排时提醒「改了文件但没有独立验证」

- **为什么**：Janus 的 `teamwork::check` 只校验 `verifies` 指向、执行者和依赖，不会提醒一个带 `owns` 的任务没有验证者（`backend/src/teamwork.rs:55`）；交付里的 `unverified` 只陈述事实（[交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)）。仓库开通 Code Review 后，Claude 会按仓库设置在 PR 上审查；DeepMind 让改进先过 Lean；DiscernBench 又说明「更多角色讨论」可能让判断更差——这三条合起来支持「默认提醒独立验证，但不要把验证做成第二个执行者」。这也是 2026-11-16 报告 P0-1 的剩余部分，本日新增了产品侧证据。
- **落点**：`backend/src/teamwork.rs` 的 `check`；[校验规则](../interfaces/20261031-board_mcp.md#orchestration_validation) 增加一条提醒；[团队结构](../domains/20261031-goal_mode.md#team_structure) 说明它只是提醒。提醒条件：任务有非空 `owns`、会改文件、没有下游 `verifies` 指向它；验证任务自己除外。
- **成功指标**：带代码改动的任务中，有验证者的比例（先测基线，4 周目标 80%）；独立验证发现问题的比例；交付后 7 天内返工率；每个验证任务的 token / 时间成本。
- **风险 / 边界**：只提醒、不拒绝编排；验证会增加 token 和时延；验证者可能复述执行者结论，继续要求 `method: test` / `command` 和实际输出；不要用「多个 Agent 投票」替代独立验证。

<a id="p2_policy"></a>
#### P2-1 验收策略文件（observe-only，先由用户决定）

- **为什么**：Claude Approvals 把「哪些路径需要多少人批准、Claude 能否批准」写成仓库里的策略文件，并建议先在 observe 模式跑（[策略参考](https://code.claude.com/docs/en/approvals-policy)、[Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，访问 2026-11-24）。Janus 是单用户，没有 `seats`，但可以借鉴「按路径规定验证强度」：例如受保护路径必须独立验证、普通路径只需证据、所有路径都不能自动批准。
- **落点**：新 ADR（先写决策再实现）；`backend/src/teamwork.rs` 的校验和 `docs/interfaces/20261031-board_mcp.md#orchestration_validation`；交付页显示策略会怎么判。策略文件放工作区还是 Janus home 需要决定；如果放进仓库，它自己就是受保护文件。
- **约束**：第一阶段只做 observe，不改变任何交付或合并行为；永远不允许 AI 代替用户验收；不改权限模式（[ADR](../decisions/20261031-agent_native_permissions.md)）；新增存储要按[数据升级](../decisions/20261118-data_migrations.md)迁移。
- **为什么不直接做**：Janus 单用户，路径级策略的收益要先由 P0-1 的观察证明；直接上策略文件会增加配置面和照抄风险。先列为待用户决定。

<a id="p2_observations"></a>
#### P2：观察项

- **P2-2 ACP 1.35 归因字段**：等官方适配器开始发送 `sessionId` / `toolCallId`，再评估是否在 `backend/src/acp.rs` 和 `backend/src/orchestrator/mcp_router.rs` 里记录；现在没有实现，升级 SDK 没有收益。
- **P2-3 Skill 目录规模阈值**：在资源库页或文档里定一个观察阈值（例如 200 个 Skill 或目录超过 16 KB 后每季度复核一次），到阈值再评估 `find-skills` 类目录、质量分和盲评；当前状态见 [Skill 路由](../domains/20261116-resource_library.md#skill_router)，不改变「不做向量检索、不做 Skill 搜索工具」的 ADR（[ADR](../decisions/20261120-skill_catalog.md)）。
- **P2-4 机器可验证证据**：DeepMind 的 Lean 门禁说明形式化证明可以是最高强度的证据；但 Janus 的证据方法目前只有 `test` / `command` / `inspection` / `reasoning` / `unverified`（[证据方法](../interfaces/20261031-events.md#evidence_methods)），用户群和事件 schema 不支持现在加 `proof` 方法。等有用户提出可复现的 Lean / 形式化验证需求，再按[数据升级](../decisions/20261118-data_migrations.md)流程评估。

<a id="already"></a>
### 已有：不需要改的部分

- **独立验证和证据**：`verifies`、验证者不能改文件、证据方法 `test` / `command` / `inspection` / `reasoning` / `unverified`、`traceGaps`、交付 `acceptance.verifications` 和 `unverified`（[团队结构](../domains/20261031-goal_mode.md#team_structure)、[证据方法](../interfaces/20261031-events.md#evidence_methods)）。Claude Ship 和 DeepMind 的做法与这条路线一致。
- **审批归用户、权限归 Agent 原生模式**：审批和表单始终交给用户，负责人不能代批（[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)）；Janus 不另加读写开关（[ADR](../decisions/20261031-agent_native_permissions.md)）。Claude Approvals 的自动批准和 Anthropic 的 deny/ask/allow 都不复制。
- **看板是唯一通道**：任务、进展、阻塞、提问都走看板；没有 Agent 间私聊（[ADR](../decisions/20261031-board_over_mcp.md)）。TalkTo 的「直接聊天也对所有人可见」从侧面确认了不做私密通道。
- **worktree 和交付合并**：每个 Git Run 在自己的 worktree 中运行，交付时提交、必要时变基并快进用户的分支，冲突交给负责人一次（[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)、`backend/src/worktrees.rs:340` 的 `accept`）。Agent Hive 在团队内部合并并行结果，Codex 把 Git 交接留给用户；Janus 是交付时合并回源分支并把冲突交给负责人。
- **并发和资源上限**：`max_concurrent_attempts` 默认 3、`max_running_runs` 默认 8、`max_agent_sessions` 默认 12，外加派发队列和闲置回收（[任务调度](../domains/20261031-run_lifecycle.md#scheduling)、[座位](../domains/20261031-run_lifecycle.md#agent_seats)、[派发队列](../domains/20261031-run_lifecycle.md#dispatch_queue)）。这比 MAC Bench 的 8 Agent 团队更克制。
- **团队记忆**：Agent 直接写笔记、记来源、按需 `recall`、定期整理；共享资料仍只有用户写（[团队记忆](../domains/20261123-team_memory.md#memory_write)、[整理](../domains/20261123-team_memory.md#dream)）。窗口外线索中的 Devin Bots 持久记忆（2026-11-17，[官方博客](https://devin.ai/blog/introducing-devin-bots)，访问 2026-11-24）没有改变这套机制的必要性。
- **Skill / MCP 路由**：Skill 走描述目录和使用记录（[Skill 路由](../domains/20261116-resource_library.md#skill_router)）；MCP 走 `janus-tools` 单端点、按需启动、支持到协议 `2025-11-25`（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)）。Uber 的数据支持描述路由，ACP 1.35 不影响 HTTP 路由。
- **工作区值班**：按时间表醒来、派发独立 Run、收件箱报告，明确不做外部 webhook、PR 交付和代验收（[值班](../domains/20261121-loops.md)、[ADR](../decisions/20261121-workspace_loops.md)）。Agent Hive 的时间 / 事件触发不构成改变理由。
- **会话续接和事件回放**：`continues`、`retry` 的 `session: continue`、`loadSession` 回退，事件日志记录整个过程并可回放（Run 删除时事件一起删除，见[用户操作](../domains/20261031-run_lifecycle.md#user_actions)）。Antigravity 的「无限本地历史」在 Janus 里由事件日志承担，不是按会话文件保留。

<a id="not_worth"></a>
### 不值得做

- **AI 自动批准**：即使策略允许，也不让 Claude 或任何 Agent 代替用户批准 PR、验收或合并。理由：Janus 的产品边界是审批归用户（[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)）；Claude Approvals 自己警告无真人合并的风险。
- **PR Steward 式自动修复并合并**：修复可以由负责人做，但合并和验收必须分开；Janus 已经在交付时合并、验收只表示认可（[ADR](../decisions/20261123-merge_back_on_delivery.md)），不需要再让一个 Agent 自动批准。
- **100+ Agent 或 16 Agent 混合模型团队**：MAC Bench 显示 8 Agent 团队成本约 9 倍、协调收益不显著；T3 Code 的 100+ Agent 是演示；DiscernBench 显示角色化讨论可能提高过度升级。Janus 保持 3 到 5 个协作者、`owns` 清晰、独立验证。
- **Agent 间私聊 / 私有消息板**：TalkTo 和 Codex 的 Guardian 消息板都提供 Agent 间消息，但 Janus 的看板是唯一事实来源，私聊会绕开事件、冲突检测和权限边界（[ADR](../decisions/20261031-board_over_mcp.md)）。
- **云端企业控制面**：Agent 365、Atos AI Control Plane、Red Hat OpenShift 5 的 Agent 治理解决的是跨租户、跨厂商的组织问题；Janus 是单机单用户，不引入。
- **Shirudo 式 eBPF 运行时**：Linux 主机安全，和 Janus 的 macOS 本地工作空间不在一个层面；不引入。
- **自动写入 Skill**：团队记忆允许 Agent 写笔记，是因为有来源、校验和整理；Skill 会进入每次会话，仍然只由用户导入（[资源库](../domains/20261116-resource_library.md#skills)）。
- **现在增加 `proof` 证据方法**：形式化证明是方向，但会改事件 schema 和数据升级；先观察 P2-4。

<a id="open_questions"></a>
## 待验证问题

| 问题 | 现状 | 怎么验证 |
| --- | --- | --- |
| Claude Ship 的受保护文件实践是否真的减少了高风险合并？ | 只有 Anthropic 自报指标，没有独立数据（U） | 观察公开采用案例；在自己的仓库试用 observe 模式，记录策略判定和人工覆盖 |
| Janus 的独立验证覆盖率基线是多少？ | 没有统计；交付里有 `acceptance.verifications` 和 `unverified` 字段 | 用事件日志统计带 `owns` 的任务中有 `verifies` 的比例、交付后 7 天返工率 |
| Janus 的单用户场景是否需要路径级验收策略？ | 没有用户证据；P0-1 只做显示 | 先跑 P0-1 一个版本，看用户是否真的查看受保护路径提示，再决定 P2-1 |
| ACP 适配器什么时候发送 `sessionId` / `toolCallId`？ | PR 说没有实现（F） | 跟踪官方适配器 release 和 changelog；升级 SDK 前先确认字段被发送 |
| 资源库 Skill 到什么规模需要 `find-skills` 类目录？ | Uber 在 5,000 个 Skill 时使用；Janus 规模未知 | 统计当前 Skill 数、目录字节数、`skill.used` 覆盖率和未被读过的 Skill 比例 |
| DiscernBench 的「少数派信号覆盖」在 Janus 的验证者结构里是否存在？ | 论文是制度场景的模拟，不是 Janus 的 Run（U） | 回看已交付 Run 的验证意见，统计单个验证者的结论是否压过其他证据 |
| 调查中「61% 想要共享 Agent 会话」是否适用于 Janus 用户？ | 自报调查，样本偏向 Ruby 会议（U） | 访谈；观察是否有人要求多人共享一个 Run 或看板 |

<a id="decisions"></a>
## 待用户决定

以下事项会改变行为、信任边界或产品范围，本报告不假定已批准。每项给出推荐，最终由用户决定。

### D1 交付时改到受保护路径：只显示、要求确认，还是不处理？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **只显示（推荐）** | 交付卡片多一行「这一轮改了会改变自动化或信任边界的文件」；不改合并行为，不推翻 2026-11-23 的 ADR | 是。改动小、可逆、不增加审批负担 |
| 要求用户确认 | 受保护路径的交付停在「等你确认」再合并；会改变交付时合并的语义，值班派出的 Run 也会停 | 否。除非访谈显示用户确实需要 |
| 不处理 | 保持现状 | 否。本日证据说明可见性缺口真实存在 |

### D2 是否引入验收策略文件？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **暂不引入（推荐）** | 先做 P0-1 和 P1-1，观察一个版本 | 是。单用户收益未验证，配置面会增加 |
| 只做 observe | 新增策略文件和报告，不改变行为；需要 ADR、数据升级和文档 | 可作为下一步 |
| 直接 enforce | 按路径要求验证或人工确认；会改变交付和验收行为，必须先有 ADR 和用户同意 | 否 |

### D3 独立验证的最高强度

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **提醒（推荐）** | `teamwork::check` 对改了文件但没有验证者的任务给提醒；不拒绝 | 是。延续 2026-11-16 的设计，不增加状态 |
| 硬性要求 | 没有验证者就拒绝编排；会增加 token、时延，也可能让负责人为了通过而安排形式验证 | 否。先测基线 |
| 不改变 | 保持现状 | 否。本日证据支持至少提醒 |

### D4 是否为了基准式并行扩大并发？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **保持现状（推荐）** | `max_concurrent_attempts` 3、`max_agent_sessions` 12、`max_running_runs` 8 | 是。MAC Bench 的 8 Agent 团队成本约 9 倍，收益不显著 |
| 提高上限 | 需要按本机 CPU / 内存和 Agent 限流重新评估；可能拉高成本和失败率 | 否。除非用户有明确的高并行任务和压测数据 |

<a id="sources"></a>
## 来源

以下来源均于 2026-11-24 访问。标注 U 的条目只有二手来源、页面异常或无法核实。

**Anthropic**

- [Introducing Claude Ship](https://claude.com/resources/articles/introducing-claude-ship)，2026-11-23，F。
- [Claude Ship 文档](https://code.claude.com/docs/en/claude-ship)，文档未标日期，F。
- [Read and request reviews on a pull request](https://code.claude.com/docs/en/code-review-pull-requests)，文档未标日期，F。
- [Claude Approvals policy file reference](https://code.claude.com/docs/en/approvals-policy)，文档未标日期，F。
- [The enterprise guide to securing coding agents](https://claude.com/resources/articles/enterprise-guide-to-securing-coding-agents)，2026-11-23，F。
- [Claude Code v2.1.331 release](https://github.com/anthropics/claude-code/releases/tag/v2.1.331)，2026-11-23，F。
- [TechCrunch 报道](https://techcrunch.com/2026/11/23/anthropic-launches-claude-ship-letting-ai-agents-review-fix-and-approve-code-changes/)，2026-11-23，二手，仅用于日期交叉核对。
- [MarkTechPost 报道](https://www.marktechpost.com/2026/11/23/anthropic-claude-ship-pull-request-agents/)，2026-11-23，U（访问时返回 403，只有搜索摘要；只用于核对「需要 2.1.330+」这一说法，正文标 U）。

**OpenAI / xAI**

- [Codex rust-v0.172.0](https://github.com/openai/codex/releases/tag/rust-v0.172.0)，2026-11-24，F。
- [Codex releases](https://github.com/openai/codex/releases)，列有 2026-11-21 至 11-24 的 alpha 与正式版本，访问 2026-11-24，F。
- [Introducing Shirudo](https://openai.com/index/introducing-shirudo/)，2026-11-23，F。
- [OpenAI 的 Shirudo X 帖子](https://x.com/i/status/2124980590717911527)，2026-11-23，F（官方账号）。
- Grok Bot：2026-11-23 至 11-24 未找到官方更新，U。

**Google**

- [Antigravity changelog](https://antigravity.google/changelog.md)，v2.35.0 为 2026-11-23，F。
- [Finding faster quantum algorithms with agents and formal verification](https://deepmind.google/blog/finding-faster-quantum-algorithms-with-agents-and-formal-verification/)，2026-11-23，F。

**协议**

- [ACP releases](https://github.com/agentclientprotocol/agent-client-protocol/releases)，列有 2026-11-21 的 Rust crate v1.21.0 至 v1.21.2、schema v1.34.1 和 11-23 的 1.35.0，访问 2026-11-24，F。
- [ACP schema v1.35.0](https://github.com/agentclientprotocol/agent-client-protocol/releases/tag/schema-v1.35.0)，2026-11-23，F。
- [ACP schema v2.0.0-beta.4](https://github.com/agentclientprotocol/agent-client-protocol/releases/tag/schema-v2.0.0-beta.4)，2026-11-23，F。
- [ACP PR #2988](https://github.com/agentclientprotocol/agent-client-protocol/pull/2988)，2026-11-23 合并，F。
- [MCP Core Primitives WG charter 提交](https://github.com/modelcontextprotocol/modelcontextprotocol/commit/b0be8f0f2481be8d5ea5294cf3fa57cd03cedc48)，2026-11-23，F。
- [MCP releases](https://github.com/modelcontextprotocol/modelcontextprotocol/releases)，最新为 2026-07-28，F。
- [A2A releases](https://github.com/a2aproject/A2A/releases)，最新为 v1.0.1（2026-05-28），F。
- [AGNTCY SLIM releases](https://github.com/agntcy/slim/releases)，最新为 v3.4.6（2026-10-20），F。

**研究**

- [REPA: From Local Errors to System Failures](https://arxiv.org/abs/2611.31063)，2026-11-23，F。
- [MASSys: The Evaluation Substrate Matters](https://arxiv.org/abs/2611.28671)，2026-11-21，F。
- [DiscernBench](https://arxiv.org/abs/2611.28787)，2026-11-21，F。
- [Persuasion from Within: Model-Based Insider Attacks](https://arxiv.org/abs/2611.28660)，2026-11-21，F。
- [Digital Twins for Multi-Agent Systems](https://arxiv.org/abs/2611.28517)，2026-11-21，F。
- [Beyond Evidence Coverage](https://arxiv.org/abs/2611.29107)，2026-11-22，F。

**产品与社区**

- [Agent Hive README](https://github.com/ericwang915/agent-hive)，访问 2026-11-24，F（产品行为）/ U（采用数据）。
- [Agent Hive Show HN 50605688](https://news.ycombinator.com/item?id=50605688)，2026-11-23，U（1 分）。
- [Agent Hive Show HN 50554228](https://news.ycombinator.com/item?id=50554228)，2026-11-19，U（2 分）。
- [TalkTo README](https://github.com/plannotator/talkto)，访问 2026-11-24，F。
- [TalkTo Show HN 50616872](https://news.ycombinator.com/item?id=50616872)，2026-11-23，U（2 分）。
- [T3 Code 工作坊 X 帖子](https://x.com/i/status/2124880498220491122)，2026-11-23，U。
- [The State of Agentic Engineering 2026](https://www.superconductor.com/state-of-agentic-engineering-2026)，调查于 2026-11-10 至 11-12，U（自报、样本方法未公开）。
- [Uber: Optimizing Uber's Agent Skills](https://www.uber.com/us/en/blog/optimizing-ubers-agent-skills/)，2026-11-23，F。
- [Bespoke Labs Multi Agent Coordination Bench](https://benchmarks.bespokelabs.ai/multi-agent-coordination-bench)，2026-11-18，F；[GitHub](https://github.com/bespokelabsai/multi-agent-coordination-bench)，2026-11-18，F。
- [Perplexity Computer Multiagent 官方博客](https://www.perplexity.ai/hub/blog/introducing-computer-multiagent)，2026-11-17，U（403，只有搜索摘要）。
- [Microsoft Agent 365 at Ignite 2026](https://techcommunity.microsoft.com/blog/agent-365-blog/whats-new-in-microsoft-agent-365-ignite-2026/4571753)，2026-11-17，F。
- [Microsoft Foundry Agent Service at Ignite 2026](https://devblogs.microsoft.com/foundry/whats-new-in-foundry-agent-service-at-ignite-2026/)，2026-11-18，U（只有搜索摘要，未打开原文）。
- [AWS Quick Flows](https://aws.amazon.com/about-aws/whats-new/2026/11/aws-quick-flows-automations-chat/)，2026-11-20，F。
- [Cognition: Introducing Devin Bots](https://devin.ai/blog/introducing-devin-bots)，2026-11-17，F。
- [Cursor Projects multiplayer](https://cursor.com/blog/projects-multiplayer)，2026-11-10，U（只有搜索摘要，未打开原文）；[Cursor changelog](https://cursor.com/changelog)，U。
- [Red Hat / Network World](https://www.networkworld.com/article/4247825/red-hat-joins-race-to-be-air-traffic-controller-for-ai-agents.html)，2026-11-12，U（「编排平台」是二手说法）。
- [GitHub changelog](https://github.blog/changelog/)，2026-11-21 至 11-24，F（11-23 无 Agent HQ 条目）。
- [Atos 中国区新闻稿](https://www.globenewswire.com/news-release/2026/11/24/3409007/0/en/atos-launches-its-sovereign-agentic-studio-in-china-to-accelerate-enterprise-ai-from-pilot-to-scale.html)，2026-11-24，U（只有搜索摘要，未打开原文）。

<a id="janus_references"></a>
## Janus 现状索引

本报告引用的 Janus 文档和代码：

- [交付](../domains/20261031-goal_mode.md#delivery_in_goal_mode)；[团队结构](../domains/20261031-goal_mode.md#team_structure)；[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)。
- [任务调度](../domains/20261031-run_lifecycle.md#scheduling)；[座位](../domains/20261031-run_lifecycle.md#agent_seats)；[派发队列](../domains/20261031-run_lifecycle.md#dispatch_queue)；[工作目录](../domains/20261031-run_lifecycle.md#workspace_model)；[一次尝试](../domains/20261031-run_lifecycle.md#attempt)。
- [证据方法](../interfaces/20261031-events.md#evidence_methods)；[校验规则](../interfaces/20261031-board_mcp.md#orchestration_validation)；[完成消息的限额](../interfaces/20261031-board_mcp.md#inline_budget)。
- [ACP 传输](../interfaces/20261031-acp_integration.md#transport)；[ACP 的 `_meta` 扩展](../interfaces/20261031-acp_integration.md#session_meta)。
- [Skill 路由](../domains/20261116-resource_library.md#skill_router)；[Skill 使用记录](../domains/20261116-resource_library.md#skill_uses)；[MCP 路由](../domains/20261116-resource_library.md#mcp_router)。
- [团队记忆](../domains/20261123-team_memory.md#memory_write)；[整理](../domains/20261123-team_memory.md#dream)。
- [工作区值班](../domains/20261121-loops.md)；[安全边界](../operations/20261031-security.md#workspace_access)。
- [ADR：交付时合并](../decisions/20261123-merge_back_on_delivery.md)；[ADR：工作没有时间和费用上限](../decisions/20261121-no_work_limits.md)；[ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)；[ADR：看板](../decisions/20261031-board_over_mcp.md)；[ADR：团队记忆](../decisions/20261123-team_memory.md)；[ADR：Skill 目录](../decisions/20261120-skill_catalog.md)。
- 代码：`backend/src/teamwork.rs`（`check`、验证者提示）、`backend/src/worktrees.rs`（`accept`）、`backend/src/config.rs`（并发和座位）、`backend/src/orchestrator/mcp_router.rs`、`backend/src/orchestrator/memory.rs`、`backend/src/library/skill_router.rs`、`frontend/Sources/Janus/UI/Run/RunView.swift`。
