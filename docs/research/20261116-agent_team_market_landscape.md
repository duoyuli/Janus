# Agent Team 市场与 Grok Bot 研究（2026-11-16）

> 覆盖窗口：2026-08-11 至 2026-11-16。Grok Bot 于 2026-08-11 开始 beta，2026-11-02 发布 1.0；其余产品按各自当前文档核对，访问日期均为 2026-11-16。
> 研究对象：Grok Bot / xAI 的 Agent Team 与多 Agent 能力，以及 Claude Code Agent Teams、OpenAI Responses hosted multi-agent、Google Antigravity `/swarm`、GitHub Agent HQ、Microsoft Agent Framework 五个代表项目。
> 与现有研究的关系：本文复用 [2026-11-10 观察](20261110-multi_agent_collaboration.md)、[2026-11-11 X 补充](20261111-multi_agent_collaboration.md)、[2026-11-12 观察](20261112-multi_agent_collaboration.md) 和 [magpie 资源库](20261116-magpie_library.md) 的结论；同时按 2026-11-16 的代码和 ADR 修正其中已经过时的状态。
> 证据标记：**F** 表示一手来源明确写出；**I** 表示由多个事实推出、未直接验证；**J** 表示本文判断；**U** 表示只有二手来源、页面异常或暂时无法核实。报告是判断，不是 Janus 当前实现的说明。

## 导航

- [摘要](#summary)：只想知道结论时阅读。
- [相对既有研究的本报告增量](#increment)：判断本报告比前几份多了什么时阅读。
- [1. 研究问题、用户与市场范围](#questions)：了解研究范围时阅读。
- [2. 市场地图](#market_map)：了解有哪些同类产品时阅读。
- [3. Grok Bot / xAI 深潜](#grok_bot)：评估 Grok Bot 的借鉴项时阅读。
- [4. 其他代表项目](#other_projects)：查 Claude Code Agent Teams、Antigravity 等项目时阅读。
- [5. 横向对比](#comparison)：对照各家能力时阅读。
- [6. 市场机会、定位与渠道](#positioning)：讨论 Janus 的定位时阅读。
- [7. 对 Janus 的演进建议](#suggestions)：评估 P0 / P1 / P2 建议时阅读。
- [8. 待验证问题与下一步](#next_steps)：安排后续验证时阅读。
- [9. 来源](#sources)：核对出处时阅读。
- [附录：关键事实的核对表](#fact_check)：核对关键事实时阅读。
- [待用户决定的产品选项](#decisions)：做产品取舍时阅读。

<a id="summary"></a>
## 摘要

- **Grok Bot 是「云端常驻同事」，不是「多 Agent 编排框架」**。它的卖点是每个 Bot 有一台云电脑、共享一个账号级工具和 Skills 库、Bot 之间可以异步转交和群聊；代价是所有 Bot 共用一个电脑、一套登录和一份周用量，官方明确说「不要把不同 Bot 当安全边界」（F）。
- **Grok Bot 的核心设计值得借鉴的是产品层，而不是运行时层**。机器人名册、角色级记忆、按需技能、routines/触发器、独立审查模型和审批来源判断，都是 Janus 可以吸收的产品概念；共享云电脑、隐藏模型、没有独立花费上限则不适合 Janus（J）。
- **Janus 不需要追「24/7 常驻」的叙事**。Grok Bot、GitHub Agent HQ、Antigravity 都在往常驻、主动、云端执行走；Janus 的差异化仍是本地优先、事件可回放、权限交给 Agent 原生模式、看板是唯一协作通道（J）。
- **验证者已经成为市场标准角色**。Antigravity 每个 verifier 负责 2-4 个 worker；Grok Bot 用独立 Auto Review 模型审查动作；Microsoft Magentic 用 progress ledger 和 stall detection 判断团队是否在原地打转。Janus 已有 `verifies`、证据方法和停滞检测，差距在「默认化」和「交付时的证据摘要」（F/I）。
- **成本可见性从「建议」变成产品行为**。Grok Bot 用周用量池、超额按 token 计费；Antigravity 配额耗尽会暂停成员；OpenAI 默认并发 3；Claude Code 建议 3-5 个 teammate。Janus 已有用量账本和 6 小时墙钟预算，但还没有全局并发上限和 Run 级 token/费用预算（F/I）。
- **小团队、清晰文件归属、独立验证仍然比大团队可靠**。Coordination Tax 的 135 个任务、2430 次运行显示普通三角色流水线成本约 3.22 倍而准确率没有显著提升；并行小队论文显示 10 支 10 人小队优于 1 支 100 人队。Janus 的 3-5 人、`owns`、`verifies`、冲突检测方向正确（F）。
- **当前最该做的不是加新能力，而是把已有能力变成默认流程**。P0 是独立验证默认化、交付证据摘要、修正 Grok/Pi 能力声明和市场验证冲刺；P1 是并发上限、Run 级成本预算和可复用编排模板；P2 才考虑定时 Run、执行图谱、技能/记忆提案和同题只读对比（J）。

<a id="increment"></a>
## 相对既有研究的本报告增量

本节逐份对照四份既有研究，只写本报告相对它们新增、修正或没有新增的内容，不重复正文论证。四份文件是 [2026-11-10 观察](20261110-multi_agent_collaboration.md)、[2026-11-11 X 补充](20261111-multi_agent_collaboration.md)、[2026-11-12 观察](20261112-multi_agent_collaboration.md) 和 [magpie 资源库](20261116-magpie_library.md)。

### 相对《2026-11-10 多 Agent 协作与 Agent Team 进展观察》

**新增**

- **Grok Bot / xAI 深潜**：产品时间线、Bot 编排、群聊与异步转交、Memory / Routines、Auto Review 与审批、共享云电脑与成本、用户反馈，以及 `grok-4.20-multi-agent` API。11-10 没有任何 Grok Bot 材料。
- **GitHub Agent HQ / Copilot Fleet**：控制面、多厂商 Agent、issue / PR 分派、PR 比较、组织治理和并发口径。11-10 没有覆盖。
- **Microsoft Agent Framework 的框架层编排**：sequential、concurrent、handoff、group chat、Magentic、progress ledger、checkpoint 和 human-in-the-loop。11-10 只覆盖 agent-framework-go v0.3.0 的发布特性（Skill 组合、agent-as-tool），没有编排模式。
- **市场层分析**：目标用户、场景、痛点、五类市场划分、竞品差异、渠道、传播重点、风险、市场侧成功指标和 H1-H8 待验证假设。11-10 的落点是工程借鉴，没有市场侧内容。
- **证据分级**：全文统一使用 F / I / J / U 标记。11-10 只写了来源限制和「未确认」，没有统一标记。
- **一手来源升级**：直接读取 OpenAI hosted multi-agent 官方文档。11-10 明确说官方参考页没有抓到，细节来自 Microsoft Foundry 镜像和二次报道。

**修正**

- 11-10 的现状判断「没有 token / 时间成本账本」「验收证据没有强度分级」已过时：按 2026-11-16 的代码，`usage.rs` / `bill.rs` / `prices.rs` 和证据 `method` 已落地。报告把缺口改为 Run 级 token / 费用预算和独立验证默认化。
- 11-10 引用的 Anthropic 多 Agent 研究系统链接是不可打开的占位域名。报告删除该链接，把结论降为「来源未确认（U）」，不再当作一手事实。
- 11-10 的 P0「给验收证据加 `method`」和 P1「成本账本」不再重复建议（已实现）；报告 P0 改为独立验证默认化、交付证据摘要、修正对外能力声明和市场验证冲刺。

**没有新增**

- Hermes / hermes-security、PagerDuty Paige、CrowdStrike AI Kill Switch、FrontierAgent、NanoClaw、ClawTeam、Teamknitt、Federation of Agents 的具体事实和判断。报告只在 P0-2 和「明确不建议」中引用 hermes 的「零发现不等于安全」与 PagerDuty 的置信度排序作为旁证，没有重新核对这些来源，也没有新增结论。
- 11-10 的「负责人说明补团队规模 / 任务粒度」「`team` 显示最近活动和当前动作」「停下但保留上下文的中断」「安全审查编排模板」「紧急停止流程」没有作为独立建议进入报告的 P0-P2；这是本次范围取舍，不是否定原结论。

### 相对《2026-11-11 X 直接抓取补充》

**新增**

- **Antigravity 进入跨产品对比表**：控制面、任务状态、通信、角色、资源、权限、验证、成本、恢复和开放性各一行。11-11 只有单节事实和「与 Janus 的关系」，没有跨产品对比表。
- **编排模板升为 P1**：11-11 的 P2「可保存的编排模板」在报告中成为 P1-3，补充了模板的数据结构（任务、依赖、验收标准、角色类型、`owns` / `verifies`）、成功指标和「模板可能诱导照抄」的风险。
- **同题只读对比**：新增 P2-4，把 11-11「暂不做 Run 级并行 best-of-N」收窄到研究 / 审查类只读任务，明确不在共享工作目录并行写代码。
- **Grok Bot 与 Antigravity 的定位对比**：云端常驻同事 vs IDE 内 coordinator / worker / verifier。11-11 只有一句 Grok Bot 配额讨论，没有产品层材料。

**修正**

- 11-11 建议「新增验证者模式」，报告核对后确认 `verifies` 和证据方法已经存在，问题不是缺角色，而是没有默认化；P0-1 因此改成编排校验、默认角色 / 模板和交付摘要，并给出覆盖率、返工率、验证成本指标，不新增状态机。
- 11-11 的「成本账本」在报告中对应 P1-2；报告按当前代码把「没有账本」修正为「账本已有、缺 Run 级预算与预测」。

**没有新增**

- 1,000-Agent 实验和并行小队论文：报告复用 11-11 的数字和结论，包括「作者自称玩具实验」的保留怀疑，没有新增证据或复现。
- 11-11 的 P1「协作者请求拆分子任务（`request_split`）」没有进入报告的 P0-P2。报告没有给出新的反证，属于本次范围取舍。
- Claude Code 动态工作流的细节（JavaScript 脚本、默认 16 并发、单次最多 1,000 个 Agent、成本提示）在报告中压缩成 P1-3 的一句「workflow 可以保存和重跑」，没有新增事实。
- 11-11 的 X 抓取操作记录、Cursor Projects 未确认信号和 harness 变动没有进入报告。

### 相对《2026-11-12 多 Agent 协作与 Agent Team 进展观察》

**新增**

- **并发约束的跨产品证据**：把 11-12 的「启动并发是瓶颈」与 OpenAI 默认 3、Claude Code 建议 3-5、Antigravity 配额耗尽暂停放在一起，形成「并发从建议变成产品约束」的横向结论；11-12 只有一位开发者的运行经验。
- **P1-1 的实现细节**：补充落点（`orchestrator/lifecycle.rs`、`Config`、配置文档、调度文档）、默认值建议（3，可配置）和指标（启动 p95、尝试失败率、墙钟、成本 / 任务）。
- **成本可见性的横向对比**：Grok Bot 周用量与 token 超额、Antigravity 配额暂停、OpenAI 默认并发 3、Claude Code 3-5 人建议。11-12 只覆盖了「成本不可见」这一条。

**修正**

- 11-12 的「Janus 目前仍没有按 Run 或任务的 token / 费用账本」已过时：按 2026-11-16 的代码，`usage.rs` / `bill.rs` / `prices.rs` 和 `/api/runs/{id}/usage` 已经存在。报告把缺口改为 Run 级预算、预测和达到阈值时暂停。

**没有新增**

- Git 只读探测、`index.lock`、`GIT_OPTIONAL_LOCKS=0` 和 `git diff-index` 的实现细节：报告没有重新讨论，11-12 已实现并有测试覆盖。
- 「规则要变成机制」、Antigravity browser use、Gemini Lean 审计和 NeurIPS 论文线索：报告未覆盖，没有新增结论。
- 11-12 的 P2「把能自动化的合并检查放进 CI」不属于本报告范围；11-12 已自行注记 2026-11-15 处理，报告没有新增内容。

### 相对《magpie 的资源库（2026-11-16）》

**新增**

- **Grok 接入的产品决策门槛**：把 magpie 的「Grok 没有找到会话级 Skill / 指令注入入口（U）」从资源库事实升级为决策条件。报告 7.5 写「不要在资源注入入口确认前恢复 Grok 内置运行时」，下一步第 5 条要求先确认入口再谈集成；magpie 没有给出这条产品层结论。
- **跨文档一致性核对**：报告定稿时项目概览仍写内置 Grok / Pi，与 ADR「内置运行时只保留 Codex 和 Claude Code」和 `backend/src/agents.rs` 的 `Runtime` 枚举冲突；概览已随文档整理并入[根索引 `docs/index.md`](../index.md)（项目概览、术语和仓库结构统一放在根索引），运行时列表改为 Codex / Claude / Custom，正文 7.1 和 P0-3 已同步为修正后的状态，剩余对外材料留待发布前核对。magpie 只引用 ADR，没有检查这份文档。
- **对外口径**：Grok 只能通过自定义 ACP 命令接入，资源库不承诺 Skill 注入，市场材料不能写「内置支持」。这是把 magpie 的能力表转成市场表述规则。

**修正**

- 无。报告没有推翻或改写 magpie 对资源库机制、各 Agent 启动能力或 Git 来源安全做法的任何结论。

**没有新增**

- 资源库的数据模型、MCP 检测与路由、子代理、凭据引用、备份与更新策略：报告没有重新研究，也没有新增采纳或不采纳判断。
- magpie 的「以后考虑」项（MCP 路由、按角色关闭工具、资源套装、版本固定）没有进入报告的 P0-P2。

**四份都没有覆盖的主题**：Grok Bot / xAI 深潜、GitHub Agent HQ、Microsoft Agent Framework 的编排模式、市场侧用户 / 渠道 / 传播 / 风险 / 成功指标、H1-H8 验证计划和待用户决定的产品选项。这些是本报告的主要增量；论文、安全审查和资源注入机制的事实仍以 11-10 至 11-12 及 magpie 报告为准。

<a id="questions"></a>
## 1. 研究问题、用户与市场范围

### 1.1 研究问题

1. Grok Bot / xAI 的 Agent Team 到底怎样编排、怎样共享状态、怎样控制权限和成本？哪些是产品事实，哪些只是营销说法？
2. 有代表性的 Agent Team / 多 Agent 项目分别采用什么编排模型、角色结构、资源注入、并发与成本控制、验证与可观测性？
3. 这些做法对 Janus 的哪些模块和文档有直接借鉴意义？哪些应该明确不采纳？
4. 市场侧的目标用户、核心需求、竞争差异、渠道和成功指标是什么？哪些问题必须先通过用户验证才能写进 PRD？

### 1.2 目标用户与使用场景

| 用户 | 典型场景 | 核心痛点 | 现有替代方案 |
| --- | --- | --- | --- |
| macOS 上的独立开发者、技术负责人 | 跨 Codex / Claude 的多步骤重构、审查、发布准备 | 单一 Agent 容易漏项；多 Agent 过程不透明；云端 Agent 要交出代码和账号 | Claude Code Agent Teams、Cursor、GitHub Agent HQ、手工多开 |
| 2-10 人的小团队 | 需要可审计的 Agent 团队完成需求、实现、验证和交付 | 结果靠 Agent 自说自话；失败后难复现；成本不可见 | 云端 Agent 团队、项目管理工具加人工复核 |
| 安全 / 合规敏感的组织 | 不能把源码、凭据和运行记录交给第三方云 | 云端电脑共享登录；数据保留和审计不可控 | 本地脚本、手工流程、私有部署 |
| AI 工程 / 研究团队 | 对比不同 Agent、模型和编排策略 | 缺少统一的事件、成本和证据口径 | 自建框架、论文复现脚本 |

**J**：首批最可行的用户是「已经同时使用 Codex 和 Claude Code、且在意本地数据与审计」的 macOS 开发者和小团队。这个群体的规模没有可靠公开数据，不能用单个案例代替市场。

### 1.3 市场范围

本文把市场分为五类，重点看「可编排、可审计的 Agent 团队」，不覆盖单纯聊天机器人或单 Agent 编码助手：

1. **云端常驻 Agent 同事**：Grok Bot、PagerDuty Paige 一类，强调 24/7、云电脑、工具登录和主动执行。
2. **IDE / 终端 Agent 团队**：Claude Code Agent Teams、Google Antigravity `/swarm`，强调一个 lead 加多个 worker，在开发环境内并行。
3. **代码托管平台的 Agent 控制面**：GitHub Agent HQ，强调多厂商 Agent 在同一 issue / PR 上分派、比较和审计。
4. **托管 API 编排**：OpenAI Responses hosted multi-agent、xAI `grok-4.20-multi-agent`，把 spawn、消息、等待、中断做成 API 动作。
5. **Agent 框架 / SDK**：Microsoft Agent Framework 等，提供 sequential、concurrent、handoff、group chat、Magentic 等编排模式。

Janus 属于第 6 类：**本地优先、跨厂商、事件溯源的工作空间**。它和以上各类都有交集，但不在同一层竞争：Janus 不提供云电脑，也不提供托管模型，而是编排用户自己的 Agent 进程。

### 1.4 方法与资料限制

- **方法**：用 WebSearch 找官方页面和一手公告，再用 WebFetch 读取官方文档、官方博客和 GitHub 文档；对 x.ai 的一篇设计文章，直接抓取返回 403，改用 `r.jina.ai` 只读代理读取；对已有研究中的论文结论做交叉引用，没有重新跑实验。
- **时间窗口**：Grok Bot 从 2026-08-11 beta 到 2026-11-16 当前状态；其他产品以 2026-11-16 可打开的官方文档为准。Beta / preview 功能可能在本文写完后变化。
- **没有做的事**：没有购买 Grok Bot、Claude Code、Antigravity、GitHub Copilot 或 Microsoft Agent Framework 的付费账号；没有 hands-on 运行这些产品；没有读取任何厂商后台的采用数据；没有做用户访谈。因此本文的「用户反馈」「采用信号」只能来自公开页面和二手评论，不能当作实测。
- **抓取限制**：x.ai 的 `designing-grok-bot` 直接抓取返回 403；`docs.x.ai/developers/model-capabilities/text/multi-agent.md` 返回 404，改用无 `.md` 的页面；X / Twitter 的网页抓取在本环境不稳定，本文没有把 X 帖子作为关键事实来源。部分 x.ai 页面在抓取文本中显示「SpaceXSI」「SpaceX Super Intelligence LLC」和 Cursor 链接，可能是页面模板、抓取伪影或品牌变更，本文按 U 处理，不影响产品能力判断。
- **二手来源**：Grok Bot 的用量、稳定性和用户抱怨来自 `grokmag.com`、`grok-bots.com` 等评测站，部分是 SEO 内容，不能独立复现；本文只在「用户反馈」段落使用，并标 U。
- **日期**：官方页面没有标注日期的，写「文档未标注日期，访问于 2026-11-16」；搜索索引给出的日期只作为线索，不单独作为事实。

<a id="market_map"></a>
## 2. 市场地图

| 项目 | 类别 | 编排模型 | 角色 / 子 Agent | 资源注入 | 并发与成本 | 权限 / 审批 | 共享状态 | 验证与可观测性 | 采用信号 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Grok Bot（xAI） | 云端常驻 Agent 同事 | 用户指定 Bot；Bot 之间异步转交；群聊共享上下文 | 多个具名 Bot；一个 Chief of Staff 模式；2-6 个 Bot 一个群聊 | 账号级 Tools / Skills；每 Bot 的 Memory / Routines；MCP / API | 所有 Bot 共用一个云电脑；每个 Bot 同时一个 computer-use 任务；周用量池 | Auto Review + Ask-first 规则；审批卡；共享电脑不是安全边界 | 群聊、Bot 间消息、共享 `/workspace`、共享登录 | Bot 名册、群聊记录、审批卡、用量；没有完整审计日志 | 2026-11-02 发布 1.0；Team / Enterprise 计划；产品页称已退出 beta |
| Claude Code Agent Teams | IDE / 终端 Agent 团队 | lead 加 teammate；共享任务列表；mailbox 直聊 | lead + 独立 Claude Code 实例；3-5 个建议 | 每个 teammate 加载项目 CLAUDE.md、MCP、Skills；subagent 定义可复用 | 每个 teammate 独立上下文；token 显著增加；缓存 5 分钟 | 继承 lead 权限模式；跨 Agent 消息不算授权；审批冒泡到 lead | 共享任务列表、mailbox、文件锁 | `TeammateIdle` / `TaskCreated` / `TaskCompleted` hooks；无集中审计 | 实验特性，默认关闭；官方文档仍标 known limitations |
| OpenAI Responses multi-agent | 托管 API 编排 | root 加子 Agent 树；六个 hosted actions | `/root`、`/root/researcher`、`/root/reviewer/tester` | 所有 Agent 共享请求的工具；`fork_turns` 控制上下文 | 默认 `max_concurrent_subagents: 3`；无深度/总数硬上限；token 计费 | 文档未覆盖权限；所有 Agent 共享完整工具集 | 托管状态可回放；`multi_agent_call` 项 | Agent 树、状态、`last_task_message`；没有独立审计 | Beta；支持 GPT-6.1 Sol 和 GPT-5.6 |
| Google Antigravity `/swarm` | IDE / 终端 Agent 团队 | coordinator 加 worker；公开留言板；verifier + monitor | coordinator、worker、verifier（每 2-4 个 worker 一个）、monitor | worker 拿公开简报；共享工作区或独立工作副本；模型可按角色指定 | 空闲成员不耗 token；配额耗尽暂停；无硬性人数上限 | worker 不私聊；审批在 CLI / App 中处理 | 公开留言板、共享工作区、可选独立副本 | Board / Members / Graph 视图；monitor 处理全员沉默 | Preview；AI Pro / Ultra |
| GitHub Agent HQ | 代码托管平台的控制面 | 任务分派给一个或多个 Agent；异步会话；PR 比较 | Copilot、Claude、Codex、自定义 Agent | 每个 Agent 自带工具；仓库权限继承；`AGENTS.md` / 自定义 Agent | 每个会话 1 个 premium request；并发限制未公开 | 组织级 Agent / 模型策略；审计日志；第三方 Agent 继承仓库访问 | Issue / PR / 分支 / 会话日志 | Agents 标签页、live logs、Copilot metrics dashboard | 2026-02-04 Claude / Codex public preview；Copilot Pro+ / Enterprise |
| Microsoft Agent Framework | Agent 框架 / SDK | sequential、concurrent、handoff、group chat、Magentic | 自定义 Agent；Magentic manager 动态选人 | 每个 Agent 自带 tools；handoff 自动注入工具；checkpointing | 由应用控制；Magentic 有 max rounds / stalls / resets | `ApprovalRequiredAIFunction`；human-in-the-loop plan review | 共享会话 / 广播上下文；checkpoint | workflow events、progress ledger、checkpoint | 官方文档持续更新；开源；多语言 SDK |
| **Janus（对照）** | 本地优先工作空间 | 负责人 `/goal` + 看板 MCP；可选独立验证 | 负责人 + 协作者；负责人固定为 `lead` | 资源库在会话启动时注入 Skill / MCP；不写 Agent 配置 | 每成员同时一个任务；Run 6 小时墙钟；已有用量账本；无全局并发上限 | Agent 原生权限模式；审批和表单始终归用户；跨 Agent 消息不算授权 | 事件日志是唯一事实来源；共享工作目录；`owns` / `claim` / `traceGaps` | 事件回放、看板推送、停滞检测、证据方法、独立验证 | 本地单机；macOS 应用；无公开采用数据 |

<a id="grok_bot"></a>
## 3. Grok Bot / xAI 深潜

### 3.1 时间线与产品事实

| 时间 | 事实 | 类型 | 来源 |
| --- | --- | --- | --- |
| 2026-08-11 | Grok Bot beta 发布，定位「your team of always-on agents」；每个 Bot 有云电脑；Bot 之间可互发消息、共享线程；群聊里分工；只在该审批时找用户 | F | [Introducing Grok Bot](https://x.ai/news/introducing-grok-bot) |
| 2026-09 上旬 | 设计文章提出五个界面原语：Bots、Chats、Prompts、Tools、Artifacts；Bot 是主对象，不是聊天记录；Tools / Skills 属于账号，Memory / Routines 属于 Bot；约 50 个 Bot / 账号、6 个 / 群聊；Chief of Staff 模式 | F（设计文章；日期为搜索索引，页面未在抓取文本中标注） | [Designing Grok Bot](https://x.ai/news/designing-grok-bot)（经 `r.jina.ai` 读取） |
| 2026-11-02 | Grok Bot 1.0：Multiplayer teammates、Team Bots、Proactive teammates、Slack / Microsoft Teams / 视频会议、更聪明的审批、Team / Enterprise 计划 | F | [Grok Bot 1.0](https://x.ai/news/grok-bot-1-0) |
| 2026-11-16 | 产品页称「Grok Bot is out of beta」；个人计划 Premium $30/月、Super $100/月、Ultra $200/月；周用量，超额按 token 计费；macOS Apple silicon 下载；企业版 GA | F（产品页；与 beta 公告的时间点不同，按当前页面记录） | [Grok Bot 产品页](https://x.ai/bot) |
| 2026-11-16 | 官方 FAQ / 文档：所有 Bot 共用一个云电脑；一个 Bot 同时只能跑一个 computer-use 任务；不能自己选模型；成本与模型无关；Team 计划默认开启且没有关闭开关 | F | [FAQ](https://docs.x.ai/grok-bot/faq)、[Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps) |

**U**：产品页的「out of beta」「Enterprise GA」和 1.0 公告的「available starting today」「Team Bots on Team and Enterprise plans」不是同一时间点的同一句话；本文把它们当作不同阶段的状态，不把「GA」写成 8 月就成立。抓取文本中的「SpaceXSI」「© 2026 SpaceX Super Intelligence LLC」和 Cursor 下载链接也按 U 处理。

### 3.2 编排模型

- **Bot 是持久身份，不是一次会话**。每个 Bot 有名字、头像、描述、自己的 memory 和 routines；用户回到的是同一个 Bot，而不是一段聊天记录（F，[Bots](https://docs.x.ai/grok-bot/bots)、[设计文章](https://x.ai/news/designing-grok-bot)）。
- **用户是调度者，Chief of Staff 是可选协调者**。早期用户会建一个「Chief of Staff」Bot 来协调专家 Bot；xAI 明确否定了 dashboard 和 assignment board，理由是「Each one gave the user more coordination work」，改由协调 Bot 做日常路由，只在需要判断时找人（F，设计文章）。
- **群聊是共享上下文的主要载体**。2-6 个 Bot 一个群聊；所有相关 Bot 看到同一段对话；群聊里可以点名、回复、`@everyone`；Bot 之间的 handoff 对群可见（F，[Chat and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration)）。
- **Bot 之间可以异步转交**。一个 Bot 给另一个 Bot 发消息，接收方「wakes, handles the request, and can reply later」；适合一个 Bot 拥有源系统、另一个 Bot 拥有交付物，或同行评审、长任务（F）。
- **官方建议每个阶段只有一个 owner**。并行 handoff「create duplicate work and noisy updates」；没有关闭 Bot 互发消息的开关，只能通过指令要求它们不要互发（F）。
- **共享状态是群聊、Bot 间消息、共享 `/workspace` 和共享登录**。文件、浏览器会话和命令凭据在同一个云电脑上对所有 Bot 可见；官方说「Do not use separate Bots as a security boundary」（F，[Files and results](https://docs.x.ai/grok-bot/files-and-results)、[FAQ](https://docs.x.ai/grok-bot/faq)）。

**I**：Grok Bot 的编排更接近「一个小团队在共享办公室里互相喊话」，而不是 Janus 的「看板 + 事件日志」。它的优势是低摩擦、适合非工程角色；代价是过程难以完整审计，权限边界只能靠账号级隔离。

### 3.3 资源、记忆、技能与 routines

| 机制 | 事实 | 类型 | 来源 |
| --- | --- | --- | --- |
| Tools / Skills | 账号级共享；一个 Bot 可用的 MCP / API 和 Skills 对整个账号可见；Skills 是「一次性教会、之后复用」的指令集 | F | [设计文章](https://x.ai/news/designing-grok-bot)、[Skills, routines](https://docs.x.ai/grok-bot/skills-routines-and-automations) |
| Memory | 每个 Bot 私有；保存偏好、规则、个人事实和「dated log of what it has worked on」；自动保存，不先问；少数事实（名字、时区）跨 Bot 共享；没有 memory 管理界面，只能用聊天查看和修改 | F | [Bots](https://docs.x.ai/grok-bot/bots) |
| Routines | 定时或事件触发（Slack、GitHub、Linear、Sentry、PagerDuty、邮件、webhook）；最短 5 分钟一次；每 Bot 最多 50 个 routine；全部 routine 每小时最多 300 次，单个最多 200 次；配额失败不自动重试 | F | [Skills, routines](https://docs.x.ai/grok-bot/skills-routines-and-automations) |
| 技能学习 | 「Teach a task」录屏最多 10 分钟，生成草稿；草稿还要补决策规则和失败处理；Team 共享的 Skill 可以发布和同步 | F | 同上 |
| 模型 | 用户不能自己选模型；请求不显示底层模型；任务成本与模型无关 | F | [FAQ](https://docs.x.ai/grok-bot/faq) |

**J**：Grok Bot 的 Memory 是「自动写入、用户事后纠正」，适合个人助理，不适合 Janus 的跨 Run 共享资料。Janus 的工作区共享资料要求用户确认后才写入，这个边界应该保留；可以借鉴的是「从成功 Run 生成 Skill / 共享资料草稿」，而不是自动保存。

### 3.4 权限、审批与安全

- **Auto Review 默认开启**。每个动作先由自动审查模型判断是放行还是问用户；只有用户自己的消息算「请求」，邮件、网页等外部文字不算（F，[Approvals, security, privacy](https://docs.x.ai/grok-bot/approvals-security-and-privacy)）。
- **Ask-first 规则最多 20 条 Allow + 20 条 Ask-first，每条 1000 字符；两类都匹配时 Ask first 优先；用户规则只能比团队规则更严格**（F）。
- **高风险动作默认需要边界**：发消息 / 邀请、发布、购买 / 转账、删除 / 覆盖、权限变更、生产变更、法律条款（F）。
- **审批不撤销已完成的工作；后台工作的审批卡 30 分钟过期；群聊里的审批卡没有「Always allow」**（F）。
- **共享电脑不是安全边界**。所有 Bot 共用一个云电脑；一个 Bot 登录过的网站，其他 Bot 也能访问；删除 Bot 不会删除共享电脑上的文件和浏览器会话（F，[Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps)、[FAQ](https://docs.x.ai/grok-bot/faq)）。
- **凭据处理**：Vault 登录和 Secure Form 的值不会展示给模型；但 App 里 Bot 能看到已保存卡号和验证码；SSO、passkey、CAPTCHA、银行验证时把电脑交还给人（F）。
- **团队控制**：每组策略只能放宽或收紧到某个方向；网络策略取最严格；团队可设网络 allowlist、Auto Review 强制、本地执行「Never allow」、限制 Bot 分享、开 Audit Logs / OpenTelemetry（F，[Teams and enterprises](https://docs.x.ai/grok-bot/teams-and-enterprises)）。
- **没有完整审计日志的说明**。文档只建议「Preserve source links and an action log for important decisions」；企业版有 Audit Logs 和 OpenTelemetry，但个人版没有（F/U，页面没有给出个人版完整审计能力）。

**I**：Grok Bot 的 Auto Review 是「模型审查 + 用户审批」的双层设计，解决的是云端 Agent 误操作和 prompt injection。Janus 的边界不同：权限由 Agent 原生模式决定，审批始终归用户。Janus 不应该再叠一个模型审查层，但应该让审批卡显示「来源、目标、影响、是否可逆」，并确保外部内容不能冒充用户授权（J）。

### 3.5 并发、成本与限制

| 维度 | 事实 | 类型 | 来源 |
| --- | --- | --- | --- |
| 电脑 | 每个用户一台专用云电脑；同一账号的所有 Bot 共享；每个 Bot 有独立屏幕，但屏幕不是安全边界 | F | [Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps) |
| 并发 | 多个 Bot 可以同时用浏览器 / 桌面工具；但一个 Bot 同时只能跑一个 computer-use 任务 | F | 同上 |
| 用量 | 订阅含周用量；用完后可开按需用量并设月度上限；成本按 token，和模型无关；所有 Bot 回复都计用量，包括群聊和 Bot 间消息 | F | [FAQ](https://docs.x.ai/grok-bot/faq)、[Chat and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration) |
| 花费上限 | 团队文档称「A separate Grok Bot spend cap is not available today」；FAQ 又说个人可设月度上限；两者口径不同 | F/U | [Teams and enterprises](https://docs.x.ai/grok-bot/teams-and-enterprises)、[FAQ](https://docs.x.ai/grok-bot/faq) |
| 保留 | Bot 60-365 天；电脑和消息 30-365 天；不支持 Legacy Privacy Mode | F | [Teams and enterprises](https://docs.x.ai/grok-bot/teams-and-enterprises) |
| 规模 | 约 50 个 Bot / 账号；6 个 / 群聊；群聊 2-6 个 Bot | F（设计文章） | [Designing Grok Bot](https://x.ai/news/designing-grok-bot) |

**U**：第三方评测称有用户一天用掉约 42% 周额度、有人因为 Bot 互相 review 用满 100%；这些数字来自 `grokmag.com` 和 `grok-bots.com`，无法独立核实，只能说明「用量焦虑」是真实存在的反馈主题，不能当作采用率或成本基准。

**J**：Grok Bot 的成本模型对 Janus 的启示不是「也做周用量池」，而是「成本必须可见、可预算、可暂停」。Janus 已有 `usage.rs`、`bill.rs`、`prices.rs` 和 `/api/runs/{id}/usage`，下一步应该把「Run 级 token / 费用预算」做成产品行为，而不是只显示事后账单。

### 3.6 用户反馈与采用信号

| 信号 | 内容 | 类型 | 来源 |
| --- | --- | --- | --- |
| 产品状态 | 2026-11-02 发布 1.0；产品页称已退出 beta；Team / Enterprise 可用；个人 $30 / $100 / $200 三档 | F | [Grok Bot 1.0](https://x.ai/news/grok-bot-1-0)、[产品页](https://x.ai/bot) |
| 正面反馈 | 持久性（关掉应用任务还继续）、能操作没有 API 的工具、价格降到 $20-30 后批评从价格转向用量和可靠性 | U | `grok-bots.com`、`grokmag.com` |
| 负面反馈 | 周用量消耗快、浏览器层脆弱、所有 Bot 共享一个电脑、单个任务卡住会影响其他 Bot、部分任务半途而废、看不到底层模型 | U | 同上 |
| 外部摩擦 | 有报道称 Amazon 在 2026-10 开始阻止 Grok Bot 下单；属于单一报道，未确认 | U | `grokmag.com` |
| 采用数据 | 官方没有公布用户数、Bot 数、任务数或留存；本文不引用任何未经核实的「百万用户」数字 | U | 无 |

**J**：Grok Bot 的市场信号说明「常驻、主动、能操作真实工具」对非工程用户有吸引力；但它也暴露了云端 Agent 的三个结构性问题：共享凭据、成本不可预测、过程不透明。Janus 的本地优先和事件审计正好对应这三点，但 Janus 目前没有「主动」和「常驻」的体验，不能直接对标。

### 3.7 xAI API 的多 Agent 模型

- **模型名**：`grok-4.20-multi-agent`，beta；接口和行为可能变化（F，[xAI Multi-Agent 文档](https://docs.x.ai/developers/model-capabilities/text/multi-agent)）。
- **规模**：4 或 16 个 Agent；4 个适合快速研究，16 个适合深度研究；更多 Agent 意味着更多 token 和更高延迟（F）。
- **编排**：多个 Agent 讨论和协作，一个 leader agent 汇总最终答案；只有 leader 的工具调用和最终回复返回给调用方，子 Agent 状态默认不暴露（F）。
- **工具**：内置 `web_search`、`x_search`、`code_execution`、`collections_search`；支持远程 MCP；不支持客户端函数调用和自定义工具（F）。
- **成本**：leader 和所有子 Agent 的输入、输出、推理 token 都计费；服务端工具调用也计费；16 个 Agent 显著高于 4 个（F）。
- **限制**：只支持 Responses API，不支持 Chat Completions；不支持 `max_tokens`；只有 leader 输出（F）。

**I**：xAI 的多 Agent API 是「一次请求内的研究小队」，不是长期团队。它和 Grok Bot 的「持久 Bot 团队」是两个产品层：前者是模型能力，后者是工作空间。Janus 不需要接入这个 API 才能获得多 Agent 能力；Janus 的协作者本身就是独立的 Agent 进程。

### 3.8 对 Janus 的启示

| Grok Bot 的做法 | 对 Janus 的处置 | 理由 |
| --- | --- | --- |
| Bot 名册、持久身份、当前动作 | 借鉴为「角色 / 成员面板」的信息架构；不新增持久 Agent 运行时 | Janus 的角色已经是持久配置，Run 是执行单位；把「谁在做什么」显示清楚即可 |
| 每 Bot 私有 Memory + 自动保存 | 不采纳自动写入；保留工作区共享资料的用户确认 | 错误记忆会进入之后每次 Run；Janus 的共享资料已经规定只有用户能改 |
| Account 级 Tools / Skills 共享 | 部分采纳：资源库已经是账号级；继续按角色引用 | Janus 的资源库比 Grok Bot 更细，按角色选择，启动时注入 |
| Routines / 事件触发 | P2 观察；先做本地定时 Run 的 ADR | 会扩大信任边界和后台成本，不能作为顺手功能 |
| 群聊 / Bot 间 handoff | 不采纳直接私聊；保留看板唯一通道 | 看板是事件日志的一部分，私聊会绕开审计、冲突检测和权限边界 |
| Auto Review + Ask-first | 不叠加模型审查层；增强审批来源和可逆性提示 | 权限由 Agent 原生模式决定是 Janus 的既定边界 |
| 共享云电脑 / 共享登录 | 明确不采纳 | 与本地优先、Agent 隔离目录、原生权限冲突 |
| 隐藏模型 / 成本与模型无关 | 明确不采纳 | Janus 应该显示模型、token、费用和预算 |
| 周用量池 / 超额 token 计费 | 借鉴「预算 + 暂停 + 用户决定」的交互，不复制定价 | Janus 是本地工具，成本来自用户自己的 Agent 订阅 / API |

<a id="other_projects"></a>
## 4. 其他代表项目

### 4.1 Claude Code Agent Teams

**事实（F，[官方文档](https://code.claude.com/docs/en/agent-teams)）**：

- 实验特性，默认关闭；需要 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`；官方明确列出 session resumption、task coordination、shutdown 的 known limitations。
- 一个 lead 会话加多个 teammate；每个 teammate 是独立 Claude Code 实例、独立上下文；teammate 之间可以直接发消息。
- 共享任务列表有 pending / in progress / completed 三种状态，可声明依赖；领取任务用文件锁防并发；teammate 可以自领未分配、未阻塞的任务。
- mailbox 是 `~/.claude/teams/{team}/inboxes/{agent}.json`；lead 可以显式分配，也可以让 teammate 自领。
- 三个 hook：`TeammateIdle`、`TaskCreated`、`TaskCompleted`，退出码 2 可以阻止完成或继续施压。
- 权限：teammate 继承 lead 的权限模式；不能在 spawn 时单独设权限模式；跨 Agent 消息不算授权，teammate 不能代替用户批准，也不能把被拒绝的操作转给另一个 teammate。
- 成本：每个 teammate 独立上下文，token 显著增加；建议 3-5 个 teammate，每个 5-6 个任务；进程内 teammate 的缓存默认 5 分钟。
- 限制：一个会话一个团队、不能嵌套团队、lead 固定、进程内 teammate 不支持 `/resume`、任务状态可能滞后、关闭可能很慢。

**J**：Claude Code 的团队体验领先在交互层（分屏、直接对话、任务列表），但恢复和权限仍是短板。Janus 的事件溯源、`resume_run`、看板唯一通道和原生权限更完整；可以借鉴的是 `TeammateIdle` 这类「完成前的质量门禁」思路，但不能直接把 hook 变成 Janus 运行命令，那会改变「Janus 不运行验证命令」的边界。

### 4.2 OpenAI Responses hosted multi-agent

**事实（F，[官方文档](https://developers.openai.com/api/docs/guides/responses-multi-agent)）**：

- Beta；支持 GPT-6.1 Sol 和所有 GPT-5.6 模型；用 `multi_agent.enabled: true` 开启；需要 `OpenAI-Beta: responses_multi_agent=v1`。
- 根 Agent 是 `/root`，子 Agent 路径如 `/root/reviewer/tester`；root 负责最终综合。
- 六个 hosted actions：`spawn_agent`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`、`list_agents`；由 OpenAI 服务执行，应用不能自己执行或回填结果。
- `max_concurrent_subagents` 默认 3，覆盖整棵树的活跃子 Agent，不含 root；没有固定深度或总数上限；文档建议大多数场景用默认值。
- 所有 Agent 共享请求的工具；任何 Agent 都可以发 `function_call`，应用必须返回 `function_call_output`；没有按 Agent 隔离工具。
- 托管状态可回放，包括 `multi_agent_call`、`multi_agent_call_output`、`agent_message`；服务端 compaction 隐式开启；不支持 `/responses/compact`、`reasoning.summary`、`max_tool_calls`。
- 文档明确说：严格顺序的任务、共享状态争用严重的任务，不适合加子 Agent。

**J**：这是和 Janus 看板语义最接近的 API：spawn、消息、等待、中断、列表。Janus 已经有 `wait`、`team`、`retry`、`cancel`、`reply`，缺的是「中断但保留上下文」和「执行树视图」。但 Janus 不应该把编排做成模型 API 的内部动作；Janus 的编排器是独立进程，事件日志是唯一事实来源。

### 4.3 Google Antigravity `/swarm`

**事实（F，[官方文档](https://antigravity.google/docs/swarm/)）**：

- Preview；Antigravity App 和 CLI 可用；Google AI Pro / Ultra 计划。
- Coordinator 是当前对话；它按文件、包或问题切成不冲突的部分，创建具名 worker；coordinator 自己不写代码。
- 每个 worker 从干净上下文开始，拿到公开简报：「你拥有什么、不能碰什么、什么算完成」。
- 通常每 2-4 个 worker 配一个 `@verifier`；verifier 读 diff、跑测试，把问题发回写代码的 worker。
- `@monitor` 平时休眠，只在全员沉默但活没干完时发帖提醒。
- 所有任务分配、跨 worker 提问、diff 摘要和测试结果都发在公开留言板；worker 之间不私聊。
- 默认共享工作区；并发编辑时 coordinator 可以给每个 worker 独立工作副本；移除 worker 前先让它保存改动。
- 空闲成员不耗 token；配额耗尽会暂停，不会自动重启；被终止的 worker 永久消失。
- 官方建议：用能干净切分的任务；verifier 换一个模型做跨模型审计；把验收方式讲清楚。

**J**：Antigravity 是「公开看板 + verifier + monitor」最直接的产品验证，和 Janus 的看板路线高度一致。Janus 的差距是 verifier 不是默认角色、monitor 由停滞检测代替；优势是持久恢复、事件回放、跨厂商 Agent。应该借鉴 verifier 默认化和跨模型审计，不引入 worker 私聊。

### 4.4 GitHub Agent HQ / Copilot Fleet

**事实（F，[GitHub 博客 2026-02-04](https://github.blog/2026-02-04-pick-your-agent-use-claude-and-codex-on-agent-hq)、[Agent HQ 公告](https://github.blog/news-insights/company-news/welcome-home-agents/)、[Fleet 文档](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/fleet)）**：

- Agent HQ 在 2025-10-28 的 GitHub Universe 发布，定位「一个 command center 来 assign、steer、track 多个 Agent」；支持 GitHub、VS Code、Mobile、CLI。
- 2026-02-04，Claude 和 Codex 进入 public preview，仅 Copilot Pro+ / Enterprise；在 GitHub、GitHub Mobile、VS Code 1.109+ 可用；Claude / Codex 默认关闭，需要在设置中显式启用。
- 可以给一个 issue 分配一个或多个 Agent；每个会话消耗 1 个 premium request；Agent 默认异步运行并打开 draft PR。
- 同一个任务分配给多个 Agent，可以比较它们的不同解法；也可以按审查职责拆分（架构、边界情况、最小改动）。
- 组织级控制：允许哪些 Agent / 模型、审计日志、访问管理、Code Quality、自动首轮审查；Copilot metrics dashboard 提供 org-wide 可见性。
- Fleet 文档：主 Agent 作为 orchestrator，拆任务、识别依赖、分配子 Agent、并行运行、审查合并；每个子 Agent 有独立上下文；并行只适合独立任务；会消耗更多 AI Credits；没有公开并发上限。

**J**：GitHub Agent HQ 验证了「多厂商 Agent 同台」的需求，但它把协作放在 issue / PR / 分支上，天然有隔离和审查。Janus 的共享工作目录不适合「同题多 Agent 并行写代码」；更适合「不同任务分给不同 Agent」和「只读研究 / 审查的同题对比」。

### 4.5 Microsoft Agent Framework

**事实（F，[官方文档](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/)、[Magentic](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/magentic)、[Handoff](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/handoff)）**：

- 内置五种编排：sequential、concurrent、handoff、group chat、Magentic。
- Handoff：mesh 拓扑，Agent 之间直接转交；接收方获得完整对话上下文；工具调用和 handoff 控制内容会被过滤；支持 `ApprovalRequiredAIFunction` 和 checkpointing；autonomous mode 默认每个 Agent 最多 50 轮。
- Magentic：一个 manager 动态选人，维护 shared context、progress ledger 和计划；支持 max rounds / max stalls / max resets；有 plan review 的 human-in-the-loop；stall 触发自动 replan。
- 文档明确说 Magentic 只在 Magentic-One 原始设计附近验证过，超出该范围的效果未测试。

**J**：Microsoft 的贡献是把「编排模式」做成可选的结构，而不是让模型即兴发挥。Janus 不需要引入框架依赖，但可以把「顺序、并行、handoff、群聊、Magentic」作为编排模板的词汇；Magentic 的 `progress ledger`（是否满足、是否在循环、是否在进展、下一个谁）值得借鉴成看板上的结构化进展，而不是新增一个自由文本字段。

### 4.6 研究侧证据

- **The Coordination Tax（[arXiv 2610.37700](https://arxiv.org/html/2610.37700v1)）**：135 个代码任务、2430 次运行；普通三角色流水线成本约 3.22 倍、耗时 3.24 倍，准确率没有显著提升（−1.5pp）；换便宜模型后成本降到 0.39 倍，但 pass@1 掉 6.7pp；按任务路由（oracle）能到 94.3%，每个解决任务约 $0.0098。结论是分解只有在廉价模型承担足够比例的输出 token 时才回本（F）。
- **Test-Time Compute Scales Better with Parallel Agent Teams（[arXiv 2611.09798](https://arxiv.org/abs/2611.09798)）**：12 个长任务、27B 模型、100 个 Agent 预算；10 支 10 人小队在 10/12 个任务上优于独立 Agent；同等美元成本下，一半任务超过前沿模型。结论是复制小队，而不是扩大单队（F）。
- **Anthropic 1,000-Agent 实验（[官方博客](https://claude.dev/blog/safely-scaling-multi-agent-teams-with-fable-5-2/)）**：1 lead + 999 helper，28.7 小时复现 Lean 费马大定理；共享 Postgres + Git 作为协调基座；最难的任务递归拆成 2,418 个任务、10 层深；作者自称「玩具实验」，不是新数学结果（F，但适用范围有限）。
- **Anthropic 多 Agent 研究系统**：多 Agent 约 15 倍 token，token 消耗解释 80% 的性能方差；适合高价值、高度并行的任务（U，来自既有研究引用；原博客链接为不可打开的占位域名，已删除，原始来源未确认）。
- **单 Agent 在同等思考 token 预算下不输多 Agent（[Tran & Kiela](https://www.semanticscholar.org/paper/Single-Agent-LLMs-Outperform-Multi-Agent-Systems-on-Tran-Kiela/a9bba78d6a7441ed43d05a0343518eeac3ac0c7e)）**：多 Agent 的此前优势可以用未计入的计算量解释（F，来自既有研究引用）。

**综合判断（J）**：多 Agent 不是默认更优；它只在「任务能干净切开 + 有独立验证 + 成本可见 + 团队规模小」时稳定回本。Janus 的产品方向应该围绕这四个条件，而不是围绕「更多 Agent」。

<a id="comparison"></a>
## 5. 横向对比

| 维度 | Grok Bot | Claude Code Agent Teams | OpenAI hosted multi-agent | Antigravity `/swarm` | GitHub Agent HQ | Microsoft Agent Framework | Janus |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 控制面 | 用户 + Chief of Staff Bot | lead 逐轮决定 | root Agent + API actions | coordinator 逐轮决定 | 用户在 issue / PR 分派 | 代码定义 workflow | 负责人 `/goal` + 看板 |
| 任务状态 | Bot 对话 / 群聊 | pending / in progress / completed | Agent 树 + 状态 | 公开留言板 | Issue / PR / session | workflow state / ledger | 7 个调度状态 + 事件溯源 |
| 通信 | Bot 私聊 + 群聊 | 直接消息 + mailbox | 消息 + mailbox | 公开留言板，不私聊 | PR 评论 / 会话 | handoff / group chat | 只经看板 |
| 角色 | 多个具名 Bot | lead + teammates | root + subagents | coordinator / worker / verifier / monitor | 多厂商 Agent | 自定义 Agent + manager | 负责人 + 协作者 |
| 资源 | 账号级 Tools / Skills | 项目 CLAUDE.md / MCP / Skills | 请求级工具，所有 Agent 共享 | 每个 worker 简报 + 工作副本 | 仓库工具 + 自定义 Agent | 每个 Agent 工具 | 资源库启动时注入 |
| 共享状态 | 共享云电脑、登录、`/workspace` | 任务列表、mailbox、工作目录 | 托管状态 | 公开看板、共享工作区 | Issue / PR / 分支 | workflow context / checkpoint | 事件日志、共享工作目录 |
| 权限 | Auto Review + Ask-first | 继承 lead，消息不算授权 | 文档未覆盖 | 审批在 App / CLI | 组织策略 + 审计 | `ApprovalRequiredAIFunction` | Agent 原生模式，审批归用户 |
| 验证 | 独立 Auto Review 模型 | hooks + lead 核对 | 无内建验证者 | verifier + monitor | 人工 review + 自动首轮审查 | progress ledger + HITL | `verifies` + 证据方法 + 停滞检测 |
| 成本 | 周用量 + token 超额 | token 显著增加 | token 计费，默认并发 3 | 配额暂停 | premium request | 应用控制 | 用量账本 + 6 小时墙钟 |
| 恢复 | 云端持久 | 进程内 teammate 不支持 `/resume` | 托管状态可回放 | 被杀 worker 不复活 | 异步会话 | checkpoint | `resume_run` |
| 开放性 | 封闭产品 | Claude Code 生态 | OpenAI 模型 | Google 生态 | 多厂商，但平台控制 | 开源框架 | 跨厂商 ACP，本地 |

<a id="positioning"></a>
## 6. 市场机会、定位与渠道

### 6.1 竞争差异

| 对手 | 它更强的地方 | Janus 更强的地方 | 对 Janus 的含义 |
| --- | --- | --- | --- |
| Grok Bot | 常驻、主动、云电脑、Slack / 会议、非工程角色体验 | 本地数据、事件回放、跨厂商 Agent、权限归用户、成本可见 | 不追「常驻同事」；主打「可审计的本地 Agent 团队」 |
| Claude Code Agent Teams | 终端体验、直接对话、生态完整 | 持久恢复、看板唯一事实源、跨厂商、独立验证 | 借鉴交互，不复制 mailbox 私聊 |
| Antigravity `/swarm` | verifier / monitor、公开看板、配额管理 | 跨厂商、事件溯源、恢复、资源库 | 把 verifier 默认化；monitor 用停滞检测代替 |
| GitHub Agent HQ | 多厂商同台、issue / PR 集成、组织治理 | 本地运行、任意工作目录、ACP 跨 Agent | 不做同题并行写代码；做只读对比 |
| Microsoft Agent Framework | 编排模式丰富、checkpoint、HITL | 开箱即用的产品、事件日志、macOS 应用 | 用模板表达模式，不引入框架依赖 |

**定位陈述（J）**：Janus 是「你自己的机器上的、可审计的跨厂商 Agent 团队工作空间」——不把代码和凭据交给云端，不隐藏模型和成本，不要求用户相信 Agent 的自述；每个任务都有负责人、文件归属、独立验证和事件记录。

### 6.2 渠道建议

| 渠道 | 受众 | 内容 | 衡量方式 | 备注 |
| --- | --- | --- | --- | --- |
| GitHub 仓库 / Release | 开发者、Agent 工具早期采用者 | 可运行的示例、变更日志、架构说明、benchmark | star / fork / release 下载 / issue 质量 | 只准备材料，不自行发布 |
| Hacker News / Reddit / macOS 社区 | 技术决策者、独立开发者 | 「local-first agent team」演示、成本与审计对比 | 点击、讨论质量、waitlist 转化 | 需要用户授权后才发帖 |
| AI 编码 newsletter / 播客 | 已使用 Codex / Claude Code 的人 | 跨厂商团队、验证者、证据方法 | 订阅转化、引用、回访 | 先做 2-3 个定向合作 |
| 设计伙伴 / 访谈 | 2-10 人小团队、受监管组织 | 白手套安装、真实目标、反馈 | 访谈到试用的转化、7 天留存、NPS | 目标 12 次访谈、3 个设计伙伴（假设目标） |
| 企业安全评审 | 安全 / 合规 / 平台团队 | 本地数据流、权限模型、审计日志、凭据隔离 | 评审通过率、试点转化 | 不宣称未取得的认证 |

### 6.3 传播重点

- **一句话**：「Your agents, your machine, your audit trail.」
- **三个支撑点**：跨厂商（Codex / Claude / 自定义 ACP）、事件可回放（每个任务、每次尝试、每个证据）、权限归用户（Agent 原生模式，审批不被 Agent 代替）。
- **不要说的话**：「24/7 常驻」「替代你的团队」「完全安全」「零配置」——这些与当前能力不符，也会把 Janus 拉进和 Grok Bot 的正面比较。
- **可信内容**：用真实 Run 的事件日志做 demo；展示一个失败的 Run 如何恢复、一个验证任务如何发现问题、成本如何被预算拦住。不要编造用户案例或转化数据。

### 6.4 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| 市场太小 | 本地优先 + macOS + 多 Agent 是交叉细分，可能不足以支撑独立产品 | 先验证设计伙伴；同时保留单 Agent / 单 Run 价值 |
| 云端体验差距 | Grok Bot / Antigravity 的常驻、主动、云电脑体验更强 | 不正面竞争；强调数据主权和审计 |
| 设置摩擦 | ACP 适配器、API Key、角色配置对非技术用户偏高 | 提供模板、诊断、默认角色；先服务技术用户 |
| 成本不可控 | 多 Agent token 成本高，用户可能一次 Run 就放弃 | P1 成本预算和预测；默认小团队 |
| 信任反噬 | 本地 Agent 仍可能删文件、泄露数据；「本地」不等于安全 | 审批 UI、可逆性提示、隔离目录、安全文档 |
| 供应商变化 | ACP 适配器、模型价格、Grok / Claude 条款会变 | 适配器版本固定、价格来源可替换、能力声明保守 |
| 研究偏差 | 二手评测、厂商营销、单一案例 | 本文标注 F/I/J/U；下一步做 hands-on 和访谈 |

<a id="suggestions"></a>
## 7. 对 Janus 的演进建议

### 7.1 当前状态核对

先按 2026-11-16 的代码和文档核对，避免把已经实现的能力重复建议：

- **已经实现**：事件溯源、`resume_run`、看板 MCP、`owns` / `claim` / `task.conflict` / `traceGaps`、`verifies` 独立验证、证据方法 `test` / `command` / `inspection` / `reasoning` / `unverified`、用量账本 `usage.rs` / `bill.rs` / `prices.rs`、6 小时墙钟预算、停滞检测、资源库启动时注入。
- **已经过时的旧结论**：2026-11-10 研究说「没有 token 账本」「证据没有强度分级」，现在这两项已经落地；2026-11-11 研究建议的验证者模式，`verifies` 已经存在，但还没有默认化。
- **本次研究期间已修正**：项目概览、术语和仓库结构已并入[根索引 `docs/index.md`](../index.md)，运行时列表和术语表为 Codex / Claude / Custom，与 2026-11-16 的 ADR [内置运行时只保留 Codex 和 Claude Code](../decisions/20261116-codex_and_claude_only.md) 和 `backend/src/agents.rs` 的 `Runtime` 枚举一致。仍需在发布前检查 `README.md`、应用内文案和发布说明是否还有「内置 Grok / Pi」的旧口径；未核对前不能视为对外材料已通过。
- **尚未实现**：全局并发上限、Run 级 token / 费用预算、可复用编排模板、定时 / 事件触发 Run、执行图谱、技能 / 记忆提案队列。

### 7.2 P0：小改动，先做

#### P0-1 独立验证默认化，并优先用不同模型

- **为什么**：Antigravity 每 2-4 个 worker 配一个 verifier；Grok Bot 用独立 Auto Review 模型；Hermes 安全审查把「有代码依据」和「测试确认」分开。Janus 已有 `verifies` 和证据方法，但负责人说明只是「重要改动安排独立验证」，默认角色和模板没有强制；实际 Run 可能没有验证任务。跨模型验证还能降低同一模型自我确认的偏差（F/I）。
- **落点**：`backend/src/teamwork.rs` 的编排校验（对带 `owns` 的任务给出「没有独立验证」提醒，不直接拒绝）；`backend/src/agents/roles.rs` 的默认角色 / 工作区模板；`docs/domains/20261031-goal_mode.md#team_structure` 和 `docs/interfaces/20261031-board_mcp.md#orchestration_validation` 的规则说明；应用的任务创建页显示「建议添加验证任务」。
- **成功指标**：带代码改动的任务中，有 `verifies` 的比例（事件 `orchestration.accepted` / `task.succeeded`）；独立验证发现问题的比例；交付后 7 天内被同一目录新 Run 重做的任务比例；每个验证任务的 token / 时间成本。先测基线，4 周目标可设为覆盖率 80%、返工率下降（目标值待基线，不写成事实）。
- **风险 / 边界**：验证会增加 token 和时延；验证者可能复述执行者结论。要求验证任务给出实际命令和输出（`method: test` / `command`），并优先换模型；不要把验证做成第二个执行者。

#### P0-2 交付页显示「证据强度 + 未验证风险」摘要

- **为什么**：Janus 的 `run.delivered` 已经有 `acceptance`、证据方法计数、`verifications` 和 `traceGaps`，但用户需要在交付页一眼看出「哪些是测试确认、哪些只是推理、哪些没有独立验证」。Grok Bot 的审批卡、PagerDuty 的置信度排序、Hermes 的覆盖缺口都在解决同一个问题（F/I）。
- **落点**：`frontend/Sources/Janus/UI/` 的 Run 详情 / 交付页；`docs/domains/20261031-goal_mode.md#delivery_in_goal_mode`；`docs/interfaces/20261031-events.md#evidence_methods`。不改事件 schema，只聚合已有字段。
- **成功指标**：交付页证据摘要的查看率；用户从交付到验收的时间；交付后争议 / 返工率；用户对「知道哪些没验证」的访谈评分。数据来自应用事件和访谈，不需要新增遥测。
- **风险 / 边界**：不要做成一个「可信度分数」，那会掩盖具体缺口；显示计数、标签和逐条证据，保留「未说明」而不是默认成「推理」。

#### P0-3 修正对外能力声明（Grok / Pi）

- **状态（2026-11-16 同步）**：项目概览、术语和仓库结构已并入[根索引 `docs/index.md`](../index.md)，运行时列表和术语表为 Codex / Claude / Custom；该项转为发布前核对 `README.md`、应用内文案和发布说明，不再要求修改根索引中的概览内容。
- **为什么**：报告定稿时项目概览仍写内置 Grok、Pi，与 ADR 和代码不一致，概览已随文档整理并入[根索引 `docs/index.md`](../index.md) 并修正；如果 README、应用文案或发布说明继续声称「内置支持 Grok」，用户仍会按错误预期安装，产生支持成本和信任损失（F）。
- **落点**：发布前核对 `README.md`、应用内文案和发布说明是否还有 Grok / Pi 的内置支持声明；本文不改这些文件。
- **成功指标**：发布前对外材料核对完成；发布说明和产品页不再出现「内置 Grok / Pi」；用户关于 Grok 连接的支持问题下降。
- **风险 / 边界**：Grok 仍可通过自定义命令接入任意 ACP Agent，但资源库对它不支持 Skill 注入；对外要写「可通过自定义 ACP 命令接入」，不能写「内置支持」。

#### P0-4 市场验证冲刺：12 次访谈 + 3 个设计伙伴

- **为什么**：本文没有任何一手用户访谈；Grok Bot 的反馈来自二手评测，不能代表 Janus 的目标用户。目标用户、愿付价格、本地优先是否是真需求、跨厂商是否刚需，都必须先验证（J）。
- **落点**：本文件后续更新为访谈记录摘要；外部落地页 / 预约表单只在用户授权后创建；不自行发帖、投放或发邮件。访谈提纲围绕最近一次多 Agent 任务、失败原因、云 vs 本地的取舍、愿意付多少钱、必须有的审计 / 权限能力。
- **成功指标**：完成 12 次访谈（至少覆盖 3 类用户）；3 个设计伙伴愿意用真实目标试用；5 个明确的「愿意付费 / 不愿意」信号；记录 3 个最常见的拒绝理由。数据来源：访谈笔记、设计伙伴试用事件（经同意）、落地页转化（如获授权）。
- **风险 / 边界**：样本偏差（只访谈熟人）；不要用「用户说喜欢」代替行为信号；不要虚构转化率或付费意愿。

### 7.3 P1：需要一到两个模块

#### P1-1 全局并发启动上限与排队

- **为什么**：OpenAI 默认 `max_concurrent_subagents: 3`；Claude Code 建议 3-5 个 teammate；Antigravity 在配额耗尽时暂停；1,000-Agent 的运行经验显示启动并发是瓶颈，槽位忙时应挡住下一个而不是堆积。Janus 目前每个成员同时一个任务，但没有全局并发上限，所有就绪任务会在同一轮启动，可能同时拉起大量 ACP 子进程（F/I）。
- **落点**：`backend/src/orchestrator/lifecycle.rs` 的调度循环、`backend/src/config.rs` 的 `Config`、`docs/interfaces/20261031-configuration.md#settings`、`docs/domains/20261031-run_lifecycle.md#scheduling`；改变调度行为前按仓库规则写 ADR。默认值建议 3（可配置），不要新增任务状态；因上限没启动的任务留在 `pending`。
- **成功指标**：尝试启动的 p95 延迟；因启动 / 资源失败导致的尝试失败率；每个任务的平均墙钟时间；Run 总墙钟时间；成本 / 任务。目标：失败率下降，墙钟时间增加不超过 10%（先测基线）。
- **风险 / 边界**：上限过低会拉长 Run；默认值要看本机 CPU / 内存；不要把它做成「全局任务队列」或新的调度器。

#### P1-2 Run 级 token / 费用预算与预测

- **为什么**：Grok Bot 用周用量和 token 超额；Antigravity 配额耗尽暂停；Claude Code 多 Agent token 显著增加；Coordination Tax 说明分解不一定回本。Janus 已有用量账本和 6 小时墙钟，但墙钟不能阻止 token 失控；用户需要「花到这个数就停，问我」的能力（F/I）。
- **落点**：`backend/src/usage.rs`、`backend/src/bill.rs`、`backend/src/prices.rs`；`docs/domains/20261031-run_lifecycle.md#timeouts` 增加预算段；`docs/interfaces/20261031-configuration.md#settings` 增加可选 `runCostBudget`；Run 详情显示预测和已用。达到阈值时进入 `waiting_input`，由用户追加或取消；不自动切换模型。
- **成功指标**：超出预算仍未暂停的 Run 数（应为 0）；预算预测误差；每个交付任务的 token / 费用；用户追加预算的比例；因预算取消的 Run 比例。数据来自 `session.usage`、`run.delivered.data.usage`、`/api/runs/{id}/usage`。
- **风险 / 边界**：价格覆盖不全时显示「估算」而不是账单；不要因为缺价格就阻塞 Run；不要把预算做成自动模型路由。

#### P1-3 可复用编排模板（从成功 Run 生成草稿）

- **为什么**：Claude Code 的 workflow 可以保存和重跑；Grok Bot 用 Skills / routines 复用流程；Antigravity 的 `/swarm` 把团队结构留在会话里；GitHub 的自定义 Agent 可以复用。Janus 的 `orchestration.accepted` 已经记录了任务图、依赖、`owns`、`verifies` 和验收标准，但每个 Run 都从零编排（F/I）。
- **落点**：`~/.janus/templates/` 或 SQLite 新表；创建 Run 的界面；`docs/domains/20261031-goal_mode.md#lead_brief`；`docs/architecture/20261031-data_model.md`。模板是数据（任务、依赖、验收标准、角色类型、`owns` / `verifies` 结构），不是可执行脚本；创建 Run 时作为建议给负责人，用户可改。
- **成功指标**：使用模板的新 Run 比例；从创建 Run 到第一次 `orchestration.accepted` 的时间；模板 Run 与非模板 Run 的交付成功率、返工率、成本；用户修改模板的比例。数据来自事件日志和模板表。
- **风险 / 边界**：模板会诱导负责人照抄；模板里可能带过时的路径 / 验收标准；要求创建时校验工作目录和资源版本，显示「按当前目标调整」。

### 7.4 P2：观察，先写 ADR

#### P2-1 定时 / 事件触发的本地 Run（routines）

- **为什么**：Grok Bot 的 routines 支持时间、Slack、GitHub、Linear、Sentry、PagerDuty、邮件、webhook；GitHub Agent HQ 把 Agent 接进 issue / PR 工作流。Janus 目前只有用户发起的 Run。对「定期审查依赖 / 安全 / 市场信息」这类任务，本地定时 Run 是自然扩展（F/I）。
- **落点**：新的本地调度模块、`~/.janus` 配置、`docs/operations/20261031-security.md`、`docs/domains/20261031-run_lifecycle.md`、ADR。第一阶段只做本地 cron，不做公网 webhook；必须显式开启、设置预算、默认不自动 push / 发布；触发事件和结果写入事件日志。
- **成功指标**：定时 Run 的完成率、成本 / 次、用户干预率、误报率；是否有未经用户批准的外部动作（目标为 0）。数据来自事件日志和用户反馈。
- **风险 / 边界**：这是信任边界变化（后台执行、可能的网络监听），必须单独 ADR + 安全文档；不能在当前功能提交里顺手加。

#### P2-2 执行图谱 / 审计视图

- **为什么**：GitHub Agent HQ 有 Agents 标签页和 live logs；Microsoft Magentic 有 progress ledger 事件；Grok Bot 有 Bot 名册和群聊记录。Janus 的事件日志已经包含「目标 → 任务 → 尝试 → 工具调用 → 改动的文件 → 产物」，但界面没有一张可下钻的图（F/I）。
- **落点**：`frontend/Sources/Janus/Core/Derive.swift` 新增派生视图；`frontend/Sources/Janus/UI/`；`docs/interfaces/20261031-events.md`；`docs/architecture/20261031-frontend.md`。只读，不新增事件类型。
- **成功指标**：失败 Run 的诊断时间（可用性测试）；`task_detail` / 看板使用率；支持问题数量；用户对「看得懂发生了什么」的评分。
- **风险 / 边界**：图容易变成装饰；先做「失败 / 冲突 / 验证没通过」三种场景的下钻，不做全量可视化。

#### P2-3 技能 / 共享资料提案队列

- **为什么**：Grok Bot 自动保存 Memory 和 Skills，Team Bot 的 learnings 还要经过 Owner / Manager 审核；Janus 已经有 `briefSuggestions`（用户确认后写入共享资料）和资源库。下一步可以把「一次成功 Run 的做法」生成 Skill 草稿或共享资料更新建议，由用户审核（F/I）。
- **落点**：`backend/src/library/`、交付页、`docs/domains/20261116-resource_library.md`、`docs/domains/20261031-agents_and_roles.md#workspace_brief`。提案是草稿，必须用户确认；版本化、可回滚。
- **成功指标**：提案采纳率；被采纳 Skill 的复用次数；创建 Skill 的时间；用户修改率；错误提案的回滚率。
- **风险 / 边界**：错误经验会长期影响后续 Run；不要自动写入；不要把「Run 成功」等同于「做法可复用」。

#### P2-4 同题多 Agent 只读对比

- **为什么**：GitHub Agent HQ 允许把同一个任务分给多个 Agent 比较解法；Janus 支持跨厂商角色，适合在「研究 / 审查 / 方案评估」这类不写文件的任务上做同题对比。代码改动不适合，因为共享工作目录会冲突（F/I）。
- **落点**：只用编排模板和文档，不改代码：负责人在 `orchestrate` 里给 2-3 个协作者相同的只读任务，最后自己综合；`docs/domains/20261031-goal_mode.md#lead_brief` 说明适用边界。
- **成功指标**：对比任务的数量；用户对结论质量的评分；每个对比的成本；是否出现文件冲突（目标为 0）。
- **风险 / 边界**：成本翻倍；容易变成「为了比较而比较」；不要在共享目录做并行写代码的 best-of-N。

### 7.5 明确不建议

- **不要复制 Grok Bot 的共享云电脑 / 共享登录**。这和 Janus 的本地优先、Agent 隔离目录、原生权限冲突；Grok Bot 官方自己说「不要把不同 Bot 当安全边界」。
- **不要加无审计的 Bot 间私聊**。看板是唯一事实来源；私聊会绕开事件、冲突检测和权限边界。可以做「任务详情里的上下文线程」，但不做独立消息通道。
- **不要自动写入 Memory / Skill**。错误结论会进入之后每次 Run；保留用户确认和版本化。
- **不要默认大团队或无限 spawn**。3-5 个协作者、清晰 `owns`、独立验证；不要为了并行而并行。
- **不要隐藏模型和成本**。Janus 的差异之一是透明；显示模型、token、费用和预算。
- **不要用模型审查替代用户审批**。可以增强审批来源和可逆性提示，但最终决定权归用户；跨 Agent 消息不能算授权。
- **不要在资源注入入口确认前恢复 Grok 内置运行时**。当前 ADR 已删除 Grok / Pi；Grok 没有找到会话级 Skill 注入入口，恢复会重蹈「角色资源无效」的问题。
- **不要在共享工作目录做同题并行写代码的 best-of-N**。需要目录隔离和评审机制，成本高；先观察。
- **不要把「零发现」当安全结论**。没有覆盖率的零发现没有意义；验证报告要列出覆盖缺口（沿用 Hermes 的结论）。
- **不要宣称 24/7、always-on 或「替代团队」**。Janus 是用户发起的本地工具，能力和边界要如实描述。

### 7.6 市场侧成功指标

| 指标 | 定义 | 数据来源 | 首轮目标 / 假设 |
| --- | --- | --- | --- |
| 认知 | GitHub Release 下载、landing page 访问、waitlist 提交 | GitHub API、站点分析（如获授权） | 先测基线，不设虚构目标 |
| 激活 | 安装后 24 小时内交付第一个 Run；工作区有 ≥2 个角色；Run 使用 ≥2 个协作者 | 本地事件日志（用户同意后汇总） | 首 Run 交付率 ≥ 50%；多角色工作区 ≥ 40%（假设） |
| 参与 | 每周活跃工作区、每周 Run 数、每 Run 任务数 | 事件日志 | 周活跃 ≥ 30%；每 Run 3-8 个任务（假设） |
| 质量 | 首次验收通过率、独立验证覆盖率、交付后 7 天返工率、`traceGaps` 率 | `task.succeeded`、`verification.*`、`run.delivered`、`/api/stats/roles` | 验证覆盖率 80%；返工率下降 20%（假设，先测基线） |
| 成本 | 每个交付任务的 token / 费用、预算命中率、预测误差 | `usage.rs`、`bill.rs`、`/api/runs/{id}/usage` | 预算外 Run = 0；预测误差 < 30%（假设） |
| 信任 | 每 Run 审批请求数、用户干预数、未授权外部动作 | `approval.requested`、`session.prompt`、审计 | 未授权外部动作 = 0 |
| 留存 / 增长 | 7 天 / 30 天留存、NPS、设计伙伴转化、付费转化 | 本地 opt-in 分析、访谈、设计伙伴 | 7 天留存 ≥ 40%；3 个设计伙伴（假设） |
| 研究验证 | 完成访谈数、问题-方案匹配评分、价格意愿 | 访谈笔记、问卷 | 12 次访谈；5 个明确付费信号（假设） |

**注意**：上表的百分比是首轮假设目标，不是已经测得的市场数据；Janus 目前没有遥测，任何汇总都必须先获得用户同意，或只在设计伙伴范围内做。

<a id="next_steps"></a>
## 8. 待验证问题与下一步

| 假设 | 验证方式 | 如果被推翻 |
| --- | --- | --- |
| H1：目标用户愿意用本地控制 / 审计换取云端便利 | 12 次访谈 + 3 个设计伙伴试用 | 转向「本地 + 可选云」或缩小到安全敏感细分 |
| H2：跨厂商 Agent（Claude + Codex + 自定义）是刚需 | 访谈中让用户排序；观察实际角色配置 | 聚焦单一 Agent 的深度体验 |
| H3：独立验证 + 证据摘要能显著降低返工 | A/B：有 / 无 verifier 的 Run 对比 | 保留能力但不默认化 |
| H4：用户需要定时 / 主动 Run | 访谈 + 设计伙伴的重复任务清单 | 保持用户发起，不做后台调度 |
| H5：编排模板能缩短首次交付时间且不导致照抄 | 模板使用前后对比；用户修改率 | 只做文档示例，不做模板功能 |
| H6：默认并发 3 可接受 | 本机压测 + 设计伙伴观察 | 改为按 CPU / 内存动态配置 |
| H7：Grok 用户需要 Janus 接入 Grok | 访谈使用 Grok 的用户；评估自定义 ACP 成本 | 把 Grok Bot 当互补产品，不做集成 |
| H8：macOS-only 足够首批用户 | 访谈 / waitlist 地区分布 | 优先做跨平台或 Web 控制面 |

**下一步（按顺序）**：

1. 发布前核对剩余对外材料（`README.md`、应用内文案、发布说明）是否还有内置 Grok / Pi 的旧口径；项目概览、术语和仓库结构已并入[根索引 `docs/index.md`](../index.md) 并修正，不再重复修改。
2. 做 12 次访谈，优先找「同时用 Codex 和 Claude Code」的 macOS 开发者；记录原话和拒绝理由。
3. 用 3 个设计伙伴跑真实目标，观察验证覆盖率、成本、返工；不要用演示数据代替。
4. 在拿到基线后再决定 P1 的默认并发数和预算阈值；不要先写死。
5. 如果要做 Grok 集成，先确认 Grok 的 ACP 适配器是否有会话级 Skill / 指令注入入口；没有就只保留自定义命令，不做内置运行时。

<a id="sources"></a>
## 9. 来源

### Grok Bot / xAI

- [Introducing Grok Bot](https://x.ai/news/introducing-grok-bot)（beta 发布，2026-08-11；访问 2026-11-16）
- [Grok Bot 1.0](https://x.ai/news/grok-bot-1-0)（2026-11-02；访问 2026-11-16）
- [Designing Grok Bot for a world of persistent agents](https://x.ai/news/designing-grok-bot)（直接抓取 403，经 `r.jina.ai` 读取；日期为搜索索引，页面未在抓取文本中标注）
- [Grok Bot 产品页](https://x.ai/bot)（价格、计划、out of beta；访问 2026-11-16）
- [Grok Bot 101](https://x.ai/bot/guides/grok-bot-101)（设置、权限、review agent、限制；访问 2026-11-16）
- [Grok Bot 文档首页](https://docs.x.ai/grok-bot)（概念与章节；访问 2026-11-16）
- [Bots](https://docs.x.ai/grok-bot/bots)
- [Chat and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration)
- [Skills, routines, and automations](https://docs.x.ai/grok-bot/skills-routines-and-automations)
- [Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps)
- [Files and results](https://docs.x.ai/grok-bot/files-and-results)
- [Approvals, security, and privacy](https://docs.x.ai/grok-bot/approvals-security-and-privacy)
- [Teams and enterprises](https://docs.x.ai/grok-bot/teams-and-enterprises)
- [FAQ](https://docs.x.ai/grok-bot/faq)
- [xAI Multi-Agent 模型文档](https://docs.x.ai/developers/model-capabilities/text/multi-agent)（beta；访问 2026-11-16）

### 其他项目

- [Claude Code：Orchestrate teams of Claude Code sessions](https://code.claude.com/docs/en/agent-teams)
- [OpenAI：Multi-agent](https://developers.openai.com/api/docs/guides/responses-multi-agent)
- [Google Antigravity：Swarm (/swarm)](https://antigravity.google/docs/swarm/)
- [GitHub：Pick your agent: Use Claude and Codex on Agent HQ](https://github.blog/2026-02-04-pick-your-agent-use-claude-and-codex-on-agent-hq)
- [GitHub：Introducing Agent HQ](https://github.blog/news-insights/company-news/welcome-home-agents/)
- [GitHub Docs：Delegate work with parallel processing (/fleet)](https://docs.github.com/en/copilot/concepts/agents/copilot-cli/fleet)
- [Microsoft Agent Framework：Workflow orchestrations](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/)
- [Microsoft Agent Framework：Magentic](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/magentic)
- [Microsoft Agent Framework：Handoff](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/handoff)

### 研究 / 论文 / 二手反馈

- [The Coordination Tax and the Heterogeneity Dividend（arXiv 2610.37700）](https://arxiv.org/html/2610.37700v1)
- [Test-Time Compute Scales Better with Parallel Agent Teams（arXiv 2611.09798）](https://arxiv.org/abs/2611.09798)
- [Anthropic：Safely scaling multi-agent teams with Claude Fable 5.2](https://claude.dev/blog/safely-scaling-multi-agent-teams-with-fable-5-2/)
- Anthropic：How we built our multi-agent research system（原链接为不可打开的占位域名，已删除；来源未确认，U）
- [Tran & Kiela：Single-Agent LLMs Outperform Multi-Agent Systems on Multi-Hop Reasoning Under Equal Thinking Token Budgets](https://www.semanticscholar.org/paper/Single-Agent-LLMs-Outperform-Multi-Agent-Systems-on-Tran-Kiela/a9bba78d6a7441ed43d05a0343518eeac3ac0c7e)
- [grokmag：Grok Bot limits and problems](https://grokmag.com/grok-bot/limits)（二手评测，U）
- [grok-bots.com：Grok Bot Review](https://grok-bots.com/grok-bot-review)（二手评测，U）
- [flaviocopes：What's new in Grok Bot 1.0](https://flaviocopes.com/grok-bot-1-0)（二手总结，U）
- [grokbotwiki：What's new in Grok Bot 1.0](https://www.grokbotwiki.com/guides/grok-bot-whats-new)（二手总结，U）

<a id="fact_check"></a>
## 附录：关键事实的核对表

| 事实 | 类型 | 核对方式 |
| --- | --- | --- |
| Grok Bot 所有 Bot 共用一个云电脑，共享登录 | F | [Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps)、[FAQ](https://docs.x.ai/grok-bot/faq) |
| Grok Bot 一个 Bot 同时只能跑一个 computer-use 任务 | F | [Computer and apps](https://docs.x.ai/grok-bot/computer-and-apps) |
| Grok Bot 群聊 2-6 个 Bot；约 50 个 Bot / 账号 | F | [Chat and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration)、[设计文章](https://x.ai/news/designing-grok-bot) |
| Grok Bot 不能自己选模型，成本与模型无关 | F | [FAQ](https://docs.x.ai/grok-bot/faq) |
| Grok Bot 团队文档称没有独立 spend cap；FAQ 说个人可设月度上限 | F/U | 两页口径不同，需按具体计划确认 |
| xAI `grok-4.20-multi-agent` 支持 4 / 16 个 Agent，leader 汇总，只有 leader 输出 | F | [xAI Multi-Agent 文档](https://docs.x.ai/developers/model-capabilities/text/multi-agent) |
| Claude Code Agent Teams 仍是实验特性，默认关闭 | F | [官方文档](https://code.claude.com/docs/en/agent-teams) |
| OpenAI hosted multi-agent 默认并发 3，六个 hosted actions | F | [官方文档](https://developers.openai.com/api/docs/guides/responses-multi-agent) |
| Antigravity `/swarm` 有 verifier、monitor，worker 不私聊 | F | [官方文档](https://antigravity.google/docs/swarm/) |
| GitHub Agent HQ 的 Claude / Codex 在 2026-02-04 进入 public preview | F | [GitHub 博客](https://github.blog/2026-02-04-pick-your-agent-use-claude-and-codex-on-agent-hq) |
| Microsoft Magentic 有 progress ledger、stall detection、plan review | F | [Magentic 文档](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/magentic) |
| Janus 当前没有内置 Grok / Pi 运行时 | F | [ADR：内置运行时只保留 Codex 和 Claude Code](../decisions/20261116-codex_and_claude_only.md)、`backend/src/agents.rs` 的 `Runtime` 枚举 |
| Janus 已有用量账本、证据方法、独立验证 | F | [Run 生命周期](../domains/20261031-run_lifecycle.md#verification)、[看板 MCP 工具](../interfaces/20261031-board_mcp.md#acceptance_criteria)、[事件](../interfaces/20261031-events.md#usage_accounting) |
| Janus 没有全局并发上限和 Run 级费用预算 | F | `backend/src/orchestrator/lifecycle.rs`、`docs/interfaces/20261031-configuration.md#compile_time` |

<a id="decisions"></a>
## 待用户决定的产品选项

以下 3 项是进入 PRD 前需要用户选择的产品方向，不是技术实现问题；每项内的选项互斥。推荐只是报告基于现有证据的建议，最终由用户决定。选定一个方向后才进入 PRD 的范围和优先级；技术实现方式、默认值和数据格式由负责人在选定方向后按仓库规则写 ADR 和方案。无论选哪项，以下边界不变：事件日志是唯一事实来源、看板是唯一协作通道、权限由 Agent 原生模式决定、审批始终归用户、不自动写入 Memory 或 Skill。

### D1 产品定位：本地审计优先、云端主动，还是混合？

| 选项 | 对范围的影响 | 对优先级的影响 | 对成功标准的影响 |
| --- | --- | --- | --- |
| **本地审计优先**（推荐） | 保持本机运行、跨厂商 ACP、事件回放；不建设云电脑、账号托管和常驻运行时。首批用户按报告 1.2 收窄到 macOS 开发者 / 技术负责人，2-10 人团队和受监管组织留作后续验证 | P0 的验证、证据、能力声明、市场验证和 P1 的并发、预算、模板全部保留 | 以激活、验证覆盖率、返工率、未授权外部动作 = 0、留存为主；不以常驻时长或主动任务数衡量 |
| 云端主动 | 新增云执行、共享登录、后台常驻和安全模型重写；需要新 ADR 和安全评审，周期最长 | 云端基础设施会挤占 P0 / P1，验证和成本能力被推迟 | 转向任务完成量、主动任务占比、留存；必须先解决共享凭据、成本不可预测和过程不透明（Grok Bot 已暴露的问题） |
| 混合 | 本地为主，增加可选云端执行后端；需要定义数据出域、凭据和审批边界 | 需要一个执行后端接入项目，排在 P0 / P1 之后才合理 | 同时看本地 / 云端 Run 占比、数据出域审批率、两类 Run 的成功率和成本 |

**推荐：本地审计优先**。报告证据显示 Janus 的差异在本地数据、事件回放、跨厂商和权限归用户；云端路线正面竞争，且共享凭据、成本不可预测、过程不透明这三个结构性问题没有解决；H1（用户愿意用本地控制换云端便利）还没有一手验证。若访谈显示「不能云端 / 常驻」是主要拒绝理由，再评估混合。

### D2 主动运行：仅用户发起、本地定时，还是外部触发？

三个选项按范围递进；本次 PRD 只能选一个作为边界。

| 选项 | 对范围的影响 | 对优先级的影响 | 对成功标准的影响 |
| --- | --- | --- | --- |
| **仅用户发起**（推荐，现状） | 不新增调度器、后台执行和网络监听；P2-1 保持观察 | 资源集中在 P0 / P1，不改信任边界 | 看用户发起后的首次交付率、验证覆盖率、返工率、成本和留存；不看主动任务数 |
| 本地定时 | 新增本地调度模块、配置和预算控制；必须显式开启、默认不自动 push / 发布；需要 ADR 和安全文档 | P2-1 提前，至少占用一个模块的工期，可能挤占 P1 | 增加定时 Run 完成率、成本 / 次、用户干预率、误报率；未授权外部动作仍须为 0 |
| 外部触发 | 在本地定时之上增加外部事件来源（GitHub / Slack / webhook）或网络监听；凭据和鉴权面最大 | 排在本地定时之后；需要单独的安全评审和 ADR | 增加触发成功率、重复触发抑制、来源鉴权、外部事件到 Run 的延迟；未授权外部动作仍须为 0 |

**推荐：仅用户发起，先验证 H4**。报告的 P2-1 把定时 / 触发 Run 列为观察项，并明确它是信任边界变化；Grok Bot 的 routines 和 Agent HQ 证明市场方向，但 Janus 用户是否需要主动 Run 还没有访谈证据。若设计伙伴的重复任务清单显示稳定需求，再按「本地定时 → 外部触发」的顺序写 ADR。

### D3 Grok 策略：只作为竞品、自定义 ACP 实验，还是恢复内置运行时？

| 选项 | 对范围的影响 | 对优先级的影响 | 对成功标准的影响 |
| --- | --- | --- | --- |
| **只作为竞品**（推荐） | 内置运行时保持 Codex / Claude Code；Grok 只做市场对标；用户仍可自行用自定义 ACP 命令接入，但产品不承诺 Skill / 指令注入 | 不增加适配器工作；P0-3 修正对外声明，资源留给 P0 / P1 | 不再出现「内置 Grok」的支持问题；完成 Grok 用户访谈（H7）；自定义接入不纳入官方支持指标 |
| 自定义 ACP 实验 | 做一次受控实验，确认 Grok 适配器是否有会话级 Skill / 指令注入、角色资源是否生效、维护成本多少；不改内置运行时，不作支持承诺 | 排在 P0-4 和 H7 之后；需要一个人力窗口 | 实验报告给出「可注入 / 不可注入」结论和证据；若不可注入则回到「只作为竞品」 |
| 恢复内置运行时 | 把 Grok 重新加入 `Runtime`、安装包、资源库和支持文档；需要修订 ADR、更新安全文档 | 会挤占 P0 / P1，并带来长期支持成本 | 增加 Grok 用户的资源注入成功率、激活、留存和支持成本；必须以「自定义 ACP 实验」的注入结论和 H7 的需求结论为前提 |

**推荐：只作为竞品**。magpie 没有找到 Grok 的会话级注入入口（U），ADR 已经删除 Grok / Pi，H7（Grok 用户需要 Janus 接入 Grok）没有验证；在没有入口和需求证据前恢复内置，会重蹈「角色资源无效」的问题。若访谈显示 Grok 用户占比显著且明确要求，先做自定义 ACP 实验，不直接恢复内置。

**与现有建议的关系**：这 3 项不改变报告已有的 F / I / J / U 事实、P0-P2 建议、成功指标和限制。选择「本地审计优先」「仅用户发起」「只作为竞品」时，第 7 节的 P0-P2 按当前顺序执行，P0-3 的能力声明修正和 P2-1 的观察状态不变；选择其他选项时，PRD 需要相应调整范围、优先级和成功指标，并在实施前补 ADR。
