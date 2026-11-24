# 2026-11-24 MCP 扩展进展与 Janus MCP 路由对照

> 覆盖窗口：主窗口为 2026-11-24 00:00 至 18:50 UTC（中部时间 2026-11-23 18:00 至 11-24 12:50）；返工核对到约 19:10 UTC。与今日早间报告重叠的时段是 00:00 至 12:25 UTC（约 12.5 小时），12:25 UTC 之后是完全新增的时段。
> 信息来源：Anthropic 官方文章、MCP 官方博客与 SEP 原文、Claude Code 官方文档、GitHub release 与提交、arXiv、Hacker News（Algolia API），以及本仓库的代码和文档。多数外部来源的核对时点为 2026-11-24 18:44 UTC，Events overview 和 Claude Code MCP 文档在返工时重新打开（约 18:55 UTC）；所有链接的访问日期为 2026-11-24。
> 抓取限制：`modelcontextprotocol.io/extensions/skills/overview` 在本机被拦，Skill 扩展以 GitHub 上的 SEP-2640 原文为准；Codex 0.174 alpha 的 release 说明为空，用户可感知能力无法核实；Hacker News 分数是 18:44 UTC 的快照；没有运行 Claude Code 经 ACP 连接 Janus 路由的协商测试。
> 证据标记：**F** 一手来源明确写出；**I** 由多个事实推出、未直接验证；**J** 本文判断；**U** 二手来源、页面打不开或无法核实。
> 与早间报告的关系：本报告是同日第二份快照，主题是 MCP 扩展与 Janus 的 MCP 路由现状；早间报告是[《2026-11-24 多 Agent 协作与 Agent Team 进展观察》](20261124-multi_agent_collaboration.md)。本报告不重复早间报告的观察，只在[相对今日早间报告的增量](#increment)一节写状态变化。

## 导航

- [摘要](#summary)：只想知道结论时阅读。
- [方法与限制](#method)：判断结论可信度时阅读。
- [与早间报告的重叠范围](#overlap)：核对本报告不重复哪些内容时阅读。
- [相对今日早间报告的增量](#increment)：想知道这一天 MCP 方向新增了什么时阅读。
- [主窗口观察](#observations)：查 7 组线索的事实和判断时阅读。
- [Janus MCP 路由现状逐条对照](#comparison)：评估每项能力在 Janus 里有没有、缺在哪时阅读。
- [判断哪些值得引入](#suggestions)：看「值得做 / 待用户决定 / 观察 / 不做」的结论时阅读。
- [待验证问题](#open_questions)：安排实测时阅读。
- [待用户决定](#decisions)：做产品取舍时阅读。
- [来源](#sources)：核对出处时阅读。
- [Janus 现状索引](#janus_references)：找文档锚点和代码路径时阅读。

<a id="summary"></a>
## 摘要

- **本窗口的主新闻是 MCP 从「工具协议」向「带 Skill、任务和用户交互的扩展协议」前进。** Anthropic 在 2026-11-24 宣布 Skills over MCP 在 Claude 和 Claude Code 公开测试、Tasks 与 Elicitation GA、File Uploads 即将推出；同一天 MCP 官方博客发布两周年回顾，给出 SDK 月下载 7 亿+、贡献者 2,500+、Events 进入 beta（该扩展要求协议 2026-07-28 或更高）、Enterprise-Managed Authorization 稳定、下一次规范发布计划在 2027 年 3 月（年份由页面日期推出，其余为 F；[Anthropic 文章](https://claude.com/resources/articles/skills-over-mcp-tasks-and-more)、[MCP 博客](https://blog.modelcontextprotocol.io/posts/mcp-turns-two/)，2026-11-24）。
- **这些能力主要落在服务器侧和客户端侧，不等于 Janus 自动获得。** Janus 的 Agent 不直连资源库服务器，所有服务器只经 `janus-tools` 路由提供（[资源库：MCP 路由](../domains/20261116-resource_library.md#mcp_router)）。Claude / Claude Code 作为 MCP 客户端的新能力，只有 Janus 自己在路由里实现才会到达资源库服务器。
- **Skills over MCP 的规范把 Skill 内容定为不可信输入，并要求显式用户政策和按来源命名空间。** SEP-2640 明确写「Skill content is untrusted input」「Explicit user policy governs whether a skill is loaded at all」「Digests are not a security boundary」（[SEP-2640](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2640-skills-extension.md)，F）。这与 Janus 现行的「Skill 只由用户导入」和 [ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md) 一致；Run 开始时自动复制服务器附带的 Skill 会直接冲突。
- **Janus 的 MCP 路由仍是 2025-11-25 时代的工具路由。** 能力声明只有 `tools`（`backend/src/orchestrator/mcp_server.rs:37`），方法只有 `initialize`、`ping`、`tools/list`、`tools/call`（`backend/src/orchestrator/mcp_router.rs:493-581`），没有 `resources`、`prompts`、`tasks`，也不支持无状态的 `2026-07-28`（`backend/src/orchestrator/mcp_client.rs:21-25`）。
- **本报告没有 P0。** 现在唯一建议做的是 P1-1：资源库显示「N 个工具只能以任务方式调用」，不再静默省略（`backend/src/orchestrator/mcp_router.rs:234-238`、`:288`）。Skills over MCP 列为 P2（待用户决定），先写 ADR、不自动复制；服务器发起的 Elicitation 暂不做；`2026-07-28` 保持不迁移，待实测 Claude Code 的协商与回退。
- **不要写成「Janus 已经坏了」。** Claude Code 的 v2 runtime 文档明确说，对不支持新版本的服务器「connects … as v1 does」，并可用 `MCP_PROTOCOL_NEGOTIATION=legacy` 强制旧握手（[Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md)，F）。Janus 与 Claude Code 经 ACP 的实际协商结果仍是 U，验证方法见[待验证问题](#open_questions)。

<a id="method"></a>
## 方法与限制

- **覆盖窗口**：主窗口 2026-11-24 00:00 至 18:50 UTC（中部时间 2026-11-23 18:00 至 11-24 12:50）。与早间报告重叠 00:00 至 12:25 UTC；12:25 UTC 之后为完全新增时段。
- **窗口外但保留的线索**：Selective Verification（arXiv:2611.28173，2026-11-20）早于主窗口，但与「验证者什么时候介入」直接相关，只吸收结论、不计入本日进展。
- **来源核对**：外部事实来自 2026-11-24 18:44 UTC 的一次逐条核对：WebFetch 读页面，`curl` 读 raw Markdown，`gh api` 读 release 和提交，HN 用 Algolia API，arXiv 读 abs 页面。报告在 18:50 UTC 前后写成，返工在 19:10 UTC 前后完成；之后的变化不在本报告内。
- **本报告作者没有做的事**：没有再联网打开每个页面，引用保留核对时的原文和访问日期；没有运行任何产品；没有改代码、ADR 或现状文档。外部事实全部来自本次核对，代码事实来自对 HEAD `496784e` 的阅读，并抽查了关键行号。
- **代码核对**：阅读了 `backend/src/orchestrator/mcp_client.rs`、`mcp_router.rs`、`mcp_server.rs`、`backend/src/library/deliver.rs`、`backend/src/library.rs`、`backend/src/acp.rs`、`backend/src/orchestrator/turn.rs`、`backend/src/http.rs`、`backend/src/http/streams.rs` 和 `frontend/Sources/Janus/UI/Settings/LibraryView.swift`。行号在其他子任务改动这些文件后会偏移。
- **日期口径**：GitHub release 时间戳用 UTC；页面日期用来源写出的日期；本地时间统一按仓库的 UTC-6，不按早间报告页眉的「北京时间」推算。
- **文档更新**：Claude Code 的 MCP 文档在 2026-11-24 18:44 UTC 快照之后更新过 tasks 与功能开关段落（当前版本把「取不到功能开关」细化为 v2.1.317+ 的交互式终端仍可拿到 tasks，核心门槛不变）。本报告其余内容按 18:44 快照口径；本轮返工重新核对了 elicitation、tasks 和 Events 的现状。
- **证据标记**：**F** 一手来源明确写出；**I** 由多个事实推出、未直接验证；**J** 本文判断；**U** 只有二手来源、页面异常或无法核实。标 U 的条目不写成事实。

<a id="overlap"></a>
## 与早间报告的重叠范围

- **重叠时段**：2026-11-24 00:00 至 12:25 UTC。12:25 UTC 是早间报告的提交时间（`ef528fd`，2026-11-24T06:25:04-06:00，即 12:25:04Z）。早间报告页眉写的「北京时间」是它自己的口径，本报告不据此推算；判断增量按它已经列出的条目，而不是按时钟先后——例如它引用的 Codex `rust-v0.172.0` 发布于 00:03:55Z，仍属于它覆盖的条目。
- **主题重叠**：早间报告覆盖 2026-11-23 至 11-24 的多 Agent 协作与 Agent Team 进展；本报告覆盖同一时段内的 MCP 扩展与 Janus MCP 路由。交集是 MCP / ACP 协议条目和 Codex 版本。

| 条目 | 早间报告位置 | 本报告处理 |
| --- | --- | --- |
| Claude Ship、Claude Approvals、Anthropic 企业安全指南 | §1、§2 | 已覆盖，不重复 |
| Antigravity 2.35、DeepMind 的 Lean 验证门禁 | §3 | 已覆盖，不重复 |
| Codex 0.172.0、Shirudo | §4 | 已覆盖；本报告只写 0.173.0-alpha.5 与 0.174.0-alpha.1/2 的状态 |
| ACP 1.35、MCP Core Primitives 工作组章程、「MCP 最新发布标签是 2026-07-28」 | §5 | 部分覆盖；发布标签已覆盖，Claude Code v2 runtime 与对旧服务器的回退未覆盖 |
| REPA | §6 | 已覆盖，不重复 |
| Agent Hive、TalkTo、T3 Code、Agentic Engineering 调查 | §7 | 已覆盖，不重复 |
| Uber 的 Skill 规模数据 | §8 | 已覆盖，不重复 |
| 补充窗口的 MASSys、DiscernBench、Insider Attacks、PAV-Twin、Beyond Evidence Coverage | 补充 | 已覆盖，不重复 |
| Anthropic 的 Skills over MCP / Tasks GA / Elicitation GA / File Uploads 公告 | 无 | **未覆盖**，本报告主新闻 |
| MCP 两周年博客（下载量、Events beta、EMA、下一次发布） | 无 | **未覆盖** |
| SEP-2640 Skills Extension、SEP-2663 Tasks Extension | 无 | **未覆盖** |
| Claude Code MCP 文档的 tasks、elicitation、v2 runtime | 仅 §5 提到发布标签 | **部分覆盖**；本报告补客户端行为 |
| Preflight、Selective Verification 论文 | 无 | **未覆盖** |
| Moonbooth、TabbyML、LLM agents 热帖、MyGraph | 无 | **未覆盖** |
| Codex 0.174 alpha | 无（只到 0.172.0 和 0.173.0-alpha.4） | **未覆盖**；只记状态 |

<a id="increment"></a>
## 相对今日早间报告的增量

- **MCP 两周年博客**：2026-11-24T09:00Z 发布，给出 SDK 近 30 天下载 7 亿+、累计 50 亿、贡献者 2,500+、Events 扩展 11 月进入 beta（SEP-3415 Final，要求协议 2026-07-28 或更高）、Skills over MCP beta、Enterprise-Managed Authorization 稳定、MCP Apps 是第一个官方扩展、下一次规范发布计划在 2027 年 3 月（时间线为 F，年份由页面日期推出为 I）。早间报告没有。
- **Anthropic 的 MCP 扩展公告**：Skills over MCP 在 Claude 和 Claude Code 公开测试，Tasks 与 Elicitation GA，File Uploads 即将推出；支持客户端是 Claude 和 Claude Code，Enterprise 尚未开放（F）。早间报告没有。
- **SEP-2640 与 SEP-2663**：Skill 扩展的传输绑定、`skill://` 资源、`skills/list`、`skills/get`，以及把 tasks 从 2025-11-25 核心移到扩展、改为每请求 client capabilities 协商（F）。早间报告没有。
- **Claude Code MCP 文档的 v2 行为**：v2 runtime 基于 MCP TypeScript SDK 2.0，新增协议版本 2026-07-28；MCP tasks 需要 v2.1.304+ 且服务器连接在 2026-07-28；对不支持的服务器按 v1 连接（F）。早间报告只提到 2026-07-28 是发布标签。
- **Codex 0.174 alpha 状态**：`rust-v0.174.0-alpha.1`（01:46Z）、`alpha.2`（16:14Z）和 `0.173.0-alpha.5`（06:44Z）发布，release 说明只有版本名；当天 `main` 有 67 条提交，其中 8 条与 Guardian 相关（F），但没有可核对的用户可感知新能力（U）。早间报告覆盖 0.172.0，未覆盖这些。
- **Preflight 与 Selective Verification 论文**：前者是供应链的 pre-execution 验证器，后者讨论验证者何时介入（摘要为 F）。早间报告没有；本报告只吸收与验证规则有关的结论，不改证据方法或 schema。
- **社区条目**：Moonbooth、TabbyML 的 20-Agent 实验、LLM agents 热帖、MyGraph 是早间报告没有的；只有 TabbyML 的结构与 Janus 有关，其余只列一行。

<a id="observations"></a>
## 主窗口观察（2026-11-24 00:00 至 18:50 UTC）

<a id="anthropic_mcp"></a>
### 1. Anthropic：Skills over MCP 公开测试，Tasks 与 Elicitation GA

**事实（F）**

- Anthropic 于 2026-11-24 发布《MCP servers can now ship skills, run tasks, and more with Claude》，页面元数据的 `datePublished` 和可见的 Date 行都是 2026-11-24。原文写「Skills over MCP is now in public beta in Claude, so MCP servers can ship skills alongside their tools」，并说「Claude loads a skill only when it's needed, keeping context use low. Skills update with the server」（[文章](https://claude.com/resources/articles/skills-over-mcp-tasks-and-more)，2026-11-24，F）。
- 支持范围：Skills over MCP 在 Claude 和 Claude Code 公开测试，Pro、Max、Ultra 默认开启，Team 需管理员开启，Enterprise 尚未开放。文章举的客户例子是 Shopify、HubSpot、Atlassian 和 Carta（Carta 标「coming soon」）；这是厂商举例，不是独立采用数据（同上，F）。
- Tasks 与 Elicitation「now generally available in Claude and Claude Code for Pro, Max, and Ultra plans, and rolling out to Team and Enterprise organizations」。Tasks 让服务器跑耗时操作、甚至完整 agent loop，Claude 继续工作并在结果就绪时取回；Elicitation 让服务器暂停，用表单或链接向用户要输入，再继续原来的工作（同上，F）。
- File Uploads 尚未可用：原文说「File Uploads is rolling out soon」，让 Claude 把 PDF、表格和图片直接交给服务器，不经过上下文窗口。不能写成已经可用（同上，F / 可用性为 U）。
- 文章说「The latest MCP spec is stateless and supported across Claude products」。版本号 `2026-07-28` 不在可见正文里，只出现在 4 处链接 URL 中，例如 `bringing-mcp-2026-07-28-to-claude`；因此「文章明确写了版本号」只能算 I。MCP 官方博客另有一手表述（见下一节，F）。
- 文章自报 MCP 月下载 7 亿+、Claude 有 10,000+ 个 MCP connector（同上，F，厂商自报）。

三项能力要分清服务器侧、客户端侧和规范侧，不能混为一谈：

| 能力 | 服务器侧 | 客户端侧 | 规范侧 |
| --- | --- | --- | --- |
| Skills over MCP | 服务器随工具一起发布 Skill；实现 `skills/list`、`skills/get`，每个文件作为 `skill://` 资源 | Claude 和 Claude Code 公开测试；Enterprise 未开放 | SEP-2640（Final），格式完全委托 Agent Skills 规范 |
| Tasks | 服务器在后台执行耗时操作或完整 agent loop，Claude 继续工作并取回结果 | Claude 和 Claude Code GA；Claude Code 还要求 v2.1.304+、连接在 2026-07-28、功能开关、主会话、交互式或特定的流式输入 | SEP-2663（Final）把 tasks 从 2025-11-25 核心移到扩展，按每请求 client capabilities 协商 |
| Elicitation | 服务器在调用中暂停，用表单或链接向用户要信息，再继续 | Claude 和 Claude Code GA；Claude Code 在 2026-07-28 连接，以及 2025-11-25 及更早协议下用户 MCP 配置里的 stdio / sse / http / ws 服务器（需 v2.1.287+）上声明 form 和 url 两种模式；只有其他较早连接（例如 claude.ai connector）是裸 `elicitation: {}`，仅支持 form | 2026-07-28 规范的 elicitation；表单与链接两种模式 |

**对 Janus 的含义（J）**

- **Janus 不在这条客户端路径上。** Janus 的 Agent 通过 ACP 启动，拿到的是 HTTP 的 `janus-tools` 和 `janus` 两个端点；资源库服务器由 Janus 进程连接，不由 Claude Code 或 Codex 直接连接（[资源库：Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)、[MCP 路由](../domains/20261116-resource_library.md#mcp_router)）。Claude Code 对「它自己连接的服务器」支持这些能力（各有协议和版本门槛），不会穿透到 Janus 的路由后面。
- **Agent 自带的服务器不受 Janus 管理。** Codex 登录后自带的 `codex_apps`、Claude Code 自己的服务器等由 Agent 自己决定，Janus 不修改 Agent 配置（[ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](../decisions/20261120-library_routed_only.md) 第 4 条、仓库约定「角色的 Skill、MCP 和指令只在会话启动时注入」）。这些连接上的新能力是 Agent 自己的行为，不是 Janus 的能力。
- **所以这三项对 Janus 的实际影响，全部取决于路由要不要实现。** 本报告在[对照表](#comparison)和[建议](#suggestions)里逐项判断：Tasks 只做展示（P1），Skills 先写 ADR（P2），Elicitation 暂不做。
- **Enterprise 未开放与 Janus 无关。** Janus 是单机、单用户工具，没有组织管理员和计划层级（[根索引](../index.md)「项目」）。

<a id="mcp_two_years"></a>
### 2. MCP 两周年：规模、扩展成熟度和下一次发布

**事实（F）**

- MCP 官方博客于 2026-11-24T09:00Z 发布《Two Years of MCP Builders》，raw 前置元数据的 `date` 和 `publishDate` 一致（[博客](https://blog.modelcontextprotocol.io/posts/mcp-turns-two/)、[raw 源](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/blog/content/posts/mcp-turns-two.md)，2026-11-24，F）。
- 下载量：图表注脚写「more than 700 million in the last 30 days, as of November 23, 2026」，覆盖 Python、TypeScript、C#、Rust、Ruby SDK；正文写累计超过 50 亿，其中 45 亿来自过去一年（同上，F）。口径是「最近 30 天」，不是自然月。
- 贡献者：超过 2,500 名（同上，F）。
- 扩展状态：Events 扩展在 2026 年 11 月进入 beta；Skills over MCP 也在 beta；Enterprise-Managed Authorization 成为稳定扩展（同上，F）。
- Events 的官方 overview 写：扩展标识是 `io.modelcontextprotocol/events`，SEP-3415 为 Final，扩展处于 Beta；它要求协议 2026-07-28 或更高，支持 poll（`events/poll`）、push（`events/stream`）和 webhook（`events/subscribe`）三种投递方式（[Events overview](https://modelcontextprotocol.io/extensions/events/overview)，访问 2026-11-24，F）。
- 下一次规范发布：Core Maintainers 在 AGNTCon + MCPCon Europe 上定下下一次发布的基础，计划「March of next year」；页面日期是 2026-11-24，所以是 2027 年 3 月（年份为 I，其余为 F）。
- MCP Apps 是第一个官方扩展，时间线写 2026 年 1 月；OpenAI Apps SDK 与社区工作一起成为它的基础（同上，F）。

**对 Janus 的含义（J）**

- **协议不会立刻换代。** 下一次规范发布在 2027 年 3 月，Janus 支持到 2025-11-25 的工具路由仍能工作；`2026-07-28` 虽然是当前发布标签，但 Janus 的不迁移决定（[调研：MCP 路由的通行做法](20261120-mcp_router_practices.md) 第 73 行）没有因为两周年而改变。
- **新功能主要走「官方扩展」这条线。** Skills、Events、EMA、MCP Apps 都是扩展，不是核心协议。Janus 若引入，应把它当作可选扩展逐项评估，而不是一次性升级协议。
- **Events 是唯一可能碰到「工作区值班外部触发」的条目。** 但它要求协议 2026-07-28 或更高，而 Janus 最高只到 2025-11-25，即使引入也要先迁移协议；而且 [ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md) 的「不做」明确列出「全局 webhook 路由」，任何「服务器事件唤醒值班」都要先改 ADR、由用户决定。
- **EMA、MCP Apps 与 Janus 当前模型不在同一层。** 前者面向企业身份，后者面向宿主渲染服务器 UI；单机单用户、以事件日志和看板为核心的 Janus 没有对应位置（理由见[不做](#not_worth)）。

<a id="sep_2640"></a>
### 3. SEP-2640：Skills Extension 的传输绑定和安全要求

**事实（F）**

- SEP-2640《Skills Extension》状态为 Final，创建于 2026-04-23，扩展标识是 `io.modelcontextprotocol/skills`（[SEP 原文](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2640-skills-extension.md)，访问 2026-11-24，F）。官网 overview 页在本机被拦，未使用；以下都来自 SEP 原文。
- 每个 Skill 目录中的每个文件作为一个 MCP resource 暴露，约定用 `skill://<skill-path>/<file-path>`；每个 Skill 必须在根上有 `SKILL.md`，固定地址是 `skill://<skill-path>/SKILL.md`（同上，F）。
- 声明扩展的服务器必须实现 `skills/list`（枚举它提供的 Skill）和 `skills/get`（按 URI 返回单个 Skill，包括不在列表里的）；可选的 `resources/directory/read` 列出目录资源的直接子项，客户端不得对未声明 `directoryRead: true` 的服务器调用它（同上，F）。
- 格式完全委托给 Agent Skills 规范：SEP「does not redefine, constrain, or extend the skill format」，Agent Skills 规范的未来版本自动适用，SEP 只定义传输绑定（同上，F）。
- 对宿主的硬性安全要求包括：「Skill content is untrusted input」；宿主必须把 MCP 提供的 Skill 内容当作不可信的模型输入；必须标记来源服务器；必须把它当作比远程工具调用更高风险的表面；没有逐 Skill 的显式用户批准，不得让 Skill 内容导致宿主侧代码执行；必须按来源命名空间解析 Skill 名字；「Digests are not a security boundary」；「Explicit user policy governs whether a skill is loaded at all」（同上，F）。

**对 Janus 的含义（J）**

- **SEP 的安全要求与 Janus 的两个敏感点正好对上。** 第三方 Skill 指令进入上下文，是「不可信输入」；Janus 现在只允许用户新建、从本机目录复制或从 Git 来源导入 Skill（[资源库：Skill](../domains/20261116-resource_library.md#skills)、[安全边界：资源库](../operations/20261031-security.md#library)）。SEP 要求的「显式用户政策决定是否加载」与这条规则同向。
- **自动复制会绕过用户判断。** 如果在 Run 开始时调用 `skills/list` 把服务器附带的 Skill 复制进资源包，用户没有逐项看过内容就进入每次 Run；这既与 ADR 的「Skill 目录由 `janus-skills` 提供，Skill 只由用户导入」冲突，也不满足 SEP 的「Explicit user policy governs whether a skill is loaded at all」。
- **digest 只能用于版本固定，不能当作信任依据。** Janus 的 Skill 内容按 sha256 存放（[资源库：Skill](../domains/20261116-resource_library.md#skills)），可以用来固定 revision，但 SEP 明确说 digest 不是安全边界。
- **格式委托意味着兼容性跟着 Agent Skills 规范走。** 如果引入，Janus 解析 `SKILL.md` 的方式要按该规范校验；当前资源库已经按 `SKILL.md` 的 front matter 读名字和描述（[资源库：Skill](../domains/20261116-resource_library.md#skills)），这部分可以复用。

<a id="claude_code_mcp"></a>
### 4. Claude Code MCP 文档：tasks、elicitation 和 v2 runtime

**事实（F）**

- Claude Code 通过两个客户端 runtime 连接 MCP 服务器：v1 基于 MCP TypeScript SDK 1.x，v2 是同一套代码换到 SDK 2.0，新增 MCP 协议版本 2026-07-28（[Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md)，页面未标日期，访问 2026-11-24，F）。
- MCP tasks 需要 Claude Code v2.1.304 或更高，且服务器要支持 tasks、连接在协议 2026-07-28、会话能取到功能开关、调用来自主会话而不是子 Agent，并且会话是交互式的，或是使用特定 `--input-format stream-json` 的 Agent SDK / `claude -p` 运行；设置 `MCP_SDK_GENERATION=v1` 或 `MCP_PROTOCOL_NEGOTIATION=legacy` 会排除这条路径（同上，F）。18:44 UTC 之后该文档更新了这一段：取不到功能开关时，v2.1.317+ 的交互式终端仍可拿到 tasks，核心门槛不变（见[方法与限制](#method)的「文档更新」）。因此 Tasks GA 不等于所有 Claude Code 会话、所有服务器都能拿到 tasks；只支持到 2025-11-25 的 Janus 路由不会进入该路径（I）。
- `execution.taskSupport` 这个字段名在 Claude Code MCP 文档全文中 0 命中（18:44 UTC 快照的 175,392 字节里 grep 无结果）；文档只说「a tool that works only as a task」。该字段的一手位置在 MCP 2025-11-25 的 schema（`schema/2025-11-25/schema.ts:1241`），并且 [SEP-2663](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2663-tasks-extension.md) 已把旧的任务机制移出核心、改为扩展（见[跨来源核对](#cross_source)）。**引用该字段时不能引 Claude Code 文档**（该词在文档中为 U）。
- elicitation：Claude Code 可以显示服务器在任务中途发起的表单或链接请求，无需用户配置。它在 2026-07-28 连接上声明 `elicitation: {form: {}, url: {}}`；在 2025-11-25 及更早协议下，对用户 Claude Code MCP 配置里的 stdio、sse、http、ws 服务器也声明 form 和 url 两种模式（需 Claude Code v2.1.287 或更高）；只有其他较早连接（例如 claude.ai connector）声明裸 `elicitation: {}`，仅支持表单模式（[文档](https://code.claude.com/docs/en/mcp.md)，2026-11-24 快照，F）。在 tasks 路径上，非交互会话（`claude -p`、Agent SDK、VS Code、桌面、云端）不弹窗，Claude Code 会请求服务器取消任务（同上，F）。
- 旧协议服务器仍可连接：v2 会先问 HTTP、stdio 和 claude.ai connector 服务器是否支持新版本，对支持的才用新版本，原文写「**Claude Code connects to every other server as v1 does.**」；`MCP_PROTOCOL_NEGOTIATION` 可设为 `auto` 或 `legacy`，`legacy` 让所有服务器保持旧握手（同上，F）。**不能据此写成「只支持旧版本的服务器会被拒绝」，也不能写成「Janus 已经坏了」。**

**对 Janus 的含义（J）**

- **Claude Code 的 tasks 门槛把 Janus 路由排除在外。** Janus 最高只协商到 2025-11-25（`backend/src/orchestrator/mcp_client.rs:21-25`），Claude Code 不会通过这条连接做 task 增补；Tasks GA 对 Janus 的直接影响是「服务器可能更愿意把工具标成只能任务调用」，而不是「Janus 突然要支持 tasks」。
- **服务器发起的 elicitation 到不了 Claude Code。** 资源库服务器发起的 `elicitation/create` 由 Janus 路由回答 `-32601`（`backend/src/orchestrator/mcp_client.rs:218-221`），Claude Code 根本看不到这个请求，它的 elicitation 对话框只适用于 Agent 直连的服务器。
- **ACP 层的 elicitation 与 MCP 层是两件事。** Janus 已在 ACP 的 `session/new` 声明 `elicitation: {form: {}, url: {}}`（`backend/src/acp.rs:152`），把 Agent 发起的表单/链接转成用户请求卡片（`backend/src/orchestrator/turn.rs:833`、`:889-902`）。这条链路正常，不受本窗口影响。
- **协商问题要实测，不能推断。** 文档只覆盖 Claude Code 这一侧的行为，没有覆盖 Janus 对探测请求的具体应答。验证方法见[待验证问题](#open_questions)。

<a id="codex"></a>
### 5. Codex：0.174 alpha 只有状态更新

**事实（F/U）**

- `rust-v0.174.0-alpha.1` 于 2026-11-24T01:46:17Z 发布，`alpha.2` 于 16:14:14Z 发布，`0.173.0-alpha.5` 于 06:44:18Z 发布；三个 release 的正文都只有版本名（[releases](https://github.com/openai/codex/releases)，访问 2026-11-24，F）。
- 2026-11-24 00:00 至 18:30Z 的 `main` 提交共 67 条，其中 commit message 含 `guardian`（不区分大小写）的 8 条，主题包括给 Guardian v3 root reviewer 预热、要求 Guardian 先读用户指令、按 root thread 批量截断审查历史、保留被取消调用的审查证据等（[commits](https://github.com/openai/codex/commits)，F）。
- `alpha.1` 到 `alpha.2` 有 38 个提交，包括插件归档签名校验、Guardian v3 细化、`Let extensions supply per-request authentication for MCP servers (#63532)`、Code Mode 的 MCP 文本回退去重等；`0.173.0-alpha.5` 到 `0.174.0-alpha.1` 有 9 个提交，包括 `Support encrypted context from MCP session-start hooks (#63474)` 和 gateway enrollment（同上，F）。这些是 `main` 上的提交，**不能写成「0.174 新增了 X」**；哪些进入了哪个 alpha 无法确认（U）。

**对 Janus 的含义（J）**

- **只记一句状态更新，不单独成建议。** 早间报告已经覆盖 0.172.0 的 Guardian 审批层和 MCP 懒启动（[早间报告 §4](20261124-multi_agent_collaboration.md#openai)）；0.174 alpha 没有可核对的用户可感知新能力（U），不构成 Janus 的依据。
- **Guardian 的细化继续说明 Codex 把审批做成自己的内部层。** Janus 不复制这层，审批和表单始终交给用户（[ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md) 第 4 条、[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md) 第 5 条）；这条结论与早间报告一致，不重复展开。
- **MCP 相关提交可以先记着。** 「每请求认证」和「MCP session-start hooks 的加密上下文」如果进入正式版，可能影响 Janus 将来对 MCP 扩展的评估；现在没有 release 归属（U），不作为建议。

<a id="papers"></a>
### 6. 论文：Preflight 与 Selective Verification 能给验证规则什么结论

**事实（F）**

- **Preflight: Agentic Last-Mile Verification for AI Software Supply Chains**（arXiv:2611.31110，2026-11-23）：一个面向 Python 依赖的程序级执行前验证器，提取应用实际引用的符号，迭代合成并验证函数级 harness，在受限环境执行解析后的制品，记录行为指纹，并在有可用基线时把目标行为与旧版本比较（[arXiv](https://arxiv.org/abs/2611.31110)，访问 2026-11-24，F）。
- Preflight 的结果：在 4 个真实供应链事件、5 个应用上下文里给出 4 个恶意判定；在 LiteLLM 客户端上下文给出干净判定，因为受影响的代理服务器路径没有被执行；100 个良性包全部干净，零误报；在 Ultralytics 和 ctx 的消融中，函数级 harness 能暴露恶意行为，只做导入探测则不能（同上，F）。
- **Selective Verification in Multi-Agent Orchestration with Large Language Models**（arXiv:2611.28173，2026-11-20，窗口外）：用九张二元票的精确模型研究验证者何时介入。普通条件下 worker 错误率 20%、共享失败率 10%、验证者错误率 2%；共享失败时 worker 错误率 80% 的情况下，检查接近票的判定错误率 7.93%，同一检查率下随机检查 9.06%；90% 时顺序反转（9.98% 对 9.41%），按「预期错误下降」排序检查是 6.73%；把分支票数传给根节点能减少根收到的消息数（[arXiv](https://arxiv.org/abs/2611.28173)，访问 2026-11-24，F）。

**对 Janus 的含义（J，只作类比）**

- **Preflight 的可吸收结论是「验证要跑实际使用路径」。** 只检查依赖存在、清单声明或导入成功，可能漏掉真正被调用的路径；对 Janus 的验证任务，如果改动涉及依赖或集成，验证者应运行受影响的入口路径并给出输出（`method` 用 `test` 或 `command`），而不是只写 `inspection`。这是对验证者提示的建议，不新增证据方法、不改事件 schema（[证据方法](../interfaces/20261031-events.md#evidence_methods)）。
- **Selective Verification 的可吸收结论是「按预期错误下降排序验证预算」。** 单一启发式（例如「只查接近票」）在错误率极高时会失效；Janus 没有投票机制，这条只能作为弱证据，支持「把验证放在最可能改变结论的任务上」，不支持改验证模型或看板结构。
- **两篇都不构成产品需求。** 它们是供应链安全和投票模型的论文，不是 Janus 场景的实测；标 I，不据此新增证据方法或改 schema。

<a id="community"></a>
### 7. 社区：Moonbooth、TabbyML、一篇热帖和 MyGraph

**事实（F/U）**

- **Moonbooth**（Show HN 50631044，2026-11-24T17:01:05Z，访问时 1 分）：给独立 Agent 交换工作流的临时 URL，站点写「A shared space for AI agents」「No signup. No setup. Free booths last 24 hours」「A booth URL returns a discovery document in plain JSON over HTTP」（[HN](https://news.ycombinator.com/item?id=50631044)、[站点](https://www.moonbooth.ai)，F；发布日期 U，站点没有日期）。
- **TabbyML《Beating ClickBench SOTA with a team of agentic systems engineers》**：博客发布于 2026-11-15，2026-11-24 才在 HN 出现（访问时 2 分）。团队有 20 个 agent，约 350 次实验，6 个改动通过审查进入最终引擎；角色分只读调查、一个评审门、实现与基准测试的隔离 lane；负责人先审计划再写补丁，验证 agent 把批准的计划带进隔离 lane，每个候选与已接受的基线在自己的主机上比较（[博客](https://www.tabbyml.com/blog/beating-clickbench-sota-with-a-team-of-agentic-systems-engineers/)、[HN](https://news.ycombinator.com/item?id=50630431)，F，厂商自报）。
- TabbyML 自报结果：hot query 总时间从 40.5 s 降到 27.9 s，hot geomean 从 0.403 s 降到 0.304 s（同上，F，厂商自报，无独立复现）。
- **《LLM agents work for their parent companies, not for you》**：2026-11-01 的观点文章，2026-11-24 在 HN 访问时 696 分（任务线索写 665 分，分数在涨）。作者用 Claude Code 的 Skill 尝试从 Instagram reel 找乐队并在 Bandcamp 购买，Bandcamp 加 challenge 后模型不绕过，但同一 Skill 的 YouTube 分支会下载歌曲，据此论证 Agent 最终服从母公司政策（[文章](https://michaelharley.net/posts/2026/11/01/llm-agents-work-for-their-parent-companies-not-for-you/)、[HN](https://news.ycombinator.com/item?id=50627340)，F 内容 / U 分数快照）。
- **MyGraph**（Show HN 50630397，2026-11-24T16:18:27Z，访问时 1 分）：一个给 Agent 当长期记忆的知识图谱，每条事实指向来源，默认拒绝没有引用的写入，13 个 MCP 工具、无查询语言，单个 Rust 二进制把图和索引放内存、全部存 SQLite；仓库 Apache-2.0，1 star（[GitHub](https://github.com/dieron/mygraph)、[HN](https://news.ycombinator.com/item?id=50630397)，F 产品行为 / U 采用信号）。

**对 Janus 的含义（J）**

- **Moonbooth 与 Janus 的方向相反。** 它是托管式、临时 URL、JSON discovery 的 Agent 协调面；Janus 是本地优先、看板是唯一 Agent 通道（[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)）。没有采用信号（1 分），只列一行，不引入。
- **TabbyML 是「独立验证 + 小步接受」的正面旁证。** 它的只读调查、一个评审门、隔离验证 lane、候选与基线对比，结构上和 Janus 的负责人、`owns`、`verifies`、证据方法相近；可以作为验证者提示和交付审查的旁证，但它是厂商博客、无独立复现，20 个 Agent 也超出 Janus 的 3 到 5 人团队，不能用它论证扩大团队。
- **LLM agents 热帖是观点，不是产品证据。** 它和 Janus 的本地优先取向共鸣，但不能作为需求证据；只列一行。
- **MyGraph 是「来源引用是常见设计」的注脚。** Janus 的团队记忆已经要求每条笔记带来源（[团队记忆](../domains/20261123-team_memory.md#memory_write)）；MyGraph 的 1 star / 1 分不构成引入建议。

<a id="cross_source"></a>
### 8. 跨来源核对

- **2026-07-28 无状态协议**：MCP 官方博客时间线明写「the stateless protocol with spec 2026-07-28 in July」（F）；Anthropic 文章说最新规范是无状态的、Claude 各产品支持（F），版本号只在链接里（I）；Claude Code v2 runtime 明写新增 MCP 协议版本 2026-07-28（F）；Claude Code 对不支持的服务器按 v1 连接（F）。Janus 路由最高只到 2025-11-25（`backend/src/orchestrator/mcp_client.rs:21-25`，代码为 F），所以它不会被协商到 2026-07-28；这不等于它坏了（J）。
- **Tasks 与 `execution.taskSupport`**：该字段是 MCP 2025-11-25 核心规范的字段（`schema/2025-11-25/schema.ts:1241`，F）；SEP-2663 明确把它描述的旧 tasks 机制从核心移出、改为扩展，并采用每请求 client capabilities 协商（F）；Claude Code MCP 文档不含这个词（F 为「不含」）；Janus 目前支持的握手只会看到旧字段（I）。
- **Elicitation 的两条链路**：Claude Code 作为 MCP 客户端支持服务器发起的 form / url elicitation（2026-07-28 连接和部分旧协议连接；其他较早连接仅 form，F）；Janus 作为资源库服务器的客户端不支持，统一回答 `-32601`（`backend/src/orchestrator/mcp_client.rs:218-221`，代码为 F）；Janus 作为 ACP 客户端支持 Agent 发起的 elicitation（`backend/src/acp.rs:152`、`backend/src/orchestrator/turn.rs:833`，F）。三者不是同一件事。
- **未核实（U）**：Claude Code 通过 ACP 连接 Janus 路由时 `initialize` 的探测和回退行为；Codex 0.174 alpha 的用户可感知能力；MCP Events 的客户端支持矩阵（overview 只给了链接，未打开）；File Uploads 的实际可用时间。

<a id="comparison"></a>
## Janus MCP 路由现状逐条对照

判定口径：**已有** 表示代码实现了这项能力；**部分** 表示有实现但有明显缺口；**缺失** 表示没有对应的代码路径；**不适用** 表示 Janus 的模型里没有这个对象。代码行号以 HEAD `496784e` 为准（`backend/src/orchestrator/` 下的文件以下简称 `mcp_*`）。

| 候选项 | 外部证据 | 对应 Janus 概念 | Janus 当前状态 | 结论 |
| --- | --- | --- | --- | --- |
| Skills over MCP（服务器随工具发布 Skill） | F：Anthropic 2026-11-24 公开测试；F：MCP 博客说 Skills over MCP beta；F：SEP-2640 Final | 资源库 Skill、`janus-skills` 目录、资源包 | **缺失**。路由能力声明只有 `tools`（`mcp_server.rs:37`），没有 `resources`；没有 `skills/*` 分支（兜底 `mcp_router.rs:580`）。Skill 只由用户在应用中新建、从本机目录复制或从 Git 来源导入（[资源库：Skill](../domains/20261116-resource_library.md#skills)、[安全边界：资源库](../operations/20261031-security.md#library)），Run 开始时复制进资源包（`backend/src/library/deliver.rs:29-46`） | **P2，待用户决定**；不自动复制，先写 ADR |
| SEP-2640 的 `skill://` 资源与 `skills/list`、`skills/get` | F：SEP-2640 原文；官网 overview 页被拦（U，未使用） | MCP 路由、`resources` 能力、Skill 内容存储 | **缺失**。没有 `resources/*`、`skills/list`、`skills/get`、`skill://`；`grep` 无结果，能力声明见 `mcp_server.rs:37`（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)） | 跟随上一条；若引入，先决定「用户逐项批准导入」而不是自动加载 |
| Tasks（新扩展，SEP-2663） | F：Anthropic Tasks GA；F：Claude Code 要求 v2.1.304+ 和 2026-07-28；F：SEP-2663 把 tasks 移到扩展 | 长任务、Run 生命周期、工具调用 | **缺失**。两侧 `initialize` 都没有 `tasks` 能力（`mcp_server.rs:37`、`mcp_client.rs:226-230`）；没有 `tasks/*` 分支（`mcp_router.rs:493-581`）；`tools/call` 固定发 `{name, arguments}`（`mcp_router.rs:539-543`）；[MCP 路由](../domains/20261116-resource_library.md#mcp_router)只写工具调用 | **不实现全协议**；只列入观察，等有实际服务器需要再单独立项 |
| `execution.taskSupport == "required"` 的工具（旧 2025-11-25 核心字段） | F：`schema/2025-11-25/schema.ts:1241`；F：SEP-2663 已把旧机制移出核心；U：Claude Code 文档无此词 | 资源库工具列表、预热结果 `Warm::Ready.tools` | **部分**。`callable` 过滤掉 `required`（`mcp_router.rs:234-238`），所有列表路径过滤（`:458`、`:468`），预热的工具数也只数能调用的（`:288`）；资源库 UI 只显示「N 个工具」（`backend/src/library.rs:49-53`、`frontend/Sources/Janus/UI/Settings/LibraryView.swift:437-452`）；文档写明「不列出，也调用不到」（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)，第 116 行） | **P1-1**：资源库显示「N 个工具只能以任务方式调用」，不改协议 |
| Elicitation（服务器 → 用户） | F：Anthropic Elicitation GA；F：Claude Code 在 2026-07-28 连接和 2025-11-25 及更早的用户配置 stdio / sse / http / ws 服务器上支持 form / url（需 v2.1.287+），其他较早连接仅 form | 审批和表单、`input.requested`、会话 | **部分（分层）**：ACP 层 Agent → 用户已有（`backend/src/acp.rs:152`、`backend/src/orchestrator/turn.rs:833`、`:889-902`）；MCP 层服务器 → 路由缺失，统一 `-32601`（`mcp_client.rs:218-221`），路由 `initialize` 也不声明 `elicitation`（`mcp_client.rs:226-230`）；文档见 [MCP 路由](../domains/20261116-resource_library.md#mcp_router) 与 [ACP 用户请求的回复](../interfaces/20261031-acp_integration.md#user_request_replies) | **暂不做**；观察触发条件，若做只接受表单模式（form）并标来源 |
| File Uploads | F：Anthropic 说「rolling out soon」，当前不可用 | 工具调用参数、资源包 | **缺失（现在不适用）**。`tools/call` 只转发 JSON 的 `name` 和 `arguments`（`mcp_router.rs:539-543`），没有文件上传通道；[MCP 路由](../domains/20261116-resource_library.md#mcp_router)只说明转发这两个字段，没有文件上传条目 | **不引入**；等 GA 且有用户实际需要的服务器再评估 |
| 无状态协议 2026-07-28 | F：MCP 博客明写 spec 2026-07-28；F：Anthropic 说最新规范无状态；F：Claude Code v2 新增该版本；F：旧服务器按 v1 连接 | 协议版本协商、路由 | **缺失**。`VERSIONS` 只有 2025-11-25、2025-06-18、2025-03-26、2024-11-05（`mcp_client.rs:21-25`）；服务器要不在集合里的版本时直接拒绝（`:231-235`），Agent 请求不支持的版本时回答 2025-11-25 由 Agent 决定（`:31-35`）；文档见 [MCP 路由](../domains/20261116-resource_library.md#mcp_router) 第 114 行 | **保持不迁移**（20261120 决策）；先实测 Claude Code 的协商与回退，重评条件见观察 |
| Events 扩展 | F：MCP 博客说 2026 年 11 月进入 beta；F：Events overview 说 SEP-3415 Final、扩展 Beta、要求协议 2026-07-28+，支持 poll / push / webhook | 工作区值班的外部触发 | **缺失**。Events 要求 2026-07-28 或更高，Janus 最高只到 2025-11-25；MCP 端点只有 POST，`GET` 返回 405（`backend/src/http.rs:175-176`、`backend/src/http/streams.rs:110-113`）；值班只按 cron 醒来（[ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md) 第 1、5 条与「不做」） | **不做**；任何「服务器事件唤醒值班」先改 ADR，由用户决定 |
| Enterprise-Managed Authorization | F：MCP 博客说已是稳定扩展 | 资源库 URL 服务器的凭据 | **缺失（不适用）**。URL 服务器只用资源里保存的静态 header（`mcp_client.rs:69-85`），没有 OAuth 流程；精选库明确不收需要 OAuth 登录的服务器（[ADR：资源库的精选库随应用发布](../decisions/20261119-library_catalog.md) 第 2 条） | **不引入**；单机单用户没有企业身份面 |
| MCP Apps（`ui://`、工具 `_meta.ui`） | F：MCP 博客说 MCP Apps 是第一个官方扩展（2026 年 1 月） | 应用界面、工具结果渲染 | **缺失**。没有 `resources`，`grep` 无 `ui://`；工具对象上的 `_meta` 会原样转发（`mcp_router.rs:496-511`），但客户端拿不到 UI 资源（能力声明 `mcp_server.rs:37`，兜底 `mcp_router.rs:580`）；[MCP 路由](../domains/20261116-resource_library.md#mcp_router)没有资源读取 | **不引入**；Janus 没有渲染服务器 UI 资源的页面 |
| `resources` / `prompts` 能力 | F：SEP-2640 的 Skill 文件经 `resources` 读取；本窗口没有新的通用需求 | MCP 路由、上下文预算 | **缺失**。能力声明只有 `tools`（`mcp_server.rs:37`）；`grep` 无 `resources/`、`prompts/`；方法兜底 `-32601`（`mcp_router.rs:580`）；[调研：MCP 路由的通行做法](20261120-mcp_router_practices.md) 第 74 行决定不转发 | **常规转发不做**；只有 Skills over MCP 会重新打开这个议题，另案 |
| Sampling（`sampling/createMessage`） | 本窗口没有新证据；MCP 规范既有请求类型（本报告未核对原文，U） | 模型调用、审批归属 | **缺失**。服务器发起的请求落到 `-32601`（`mcp_client.rs:218-221`）；路由 `initialize` 不声明 `sampling`（`:226-230`）；文档写明「其他请求回答不支持」（[MCP 路由](../domains/20261116-resource_library.md#mcp_router) 第 113 行） | **不引入**：第三方服务器借模型会绕开用户的审批和权限模式 |
| 服务器 `initialize` 的 `instructions` | 本窗口没有新证据（U） | 路由的说明文字、2048 字符预算 | **缺失（有意不转）**。只读 `protocolVersion`，服务器自己的 `instructions` 被忽略（`mcp_client.rs:231-239`）；[20261120 调研](20261120-mcp_router_practices.md) 第 74 行决定「待有实际需要再评估」 | **不做**；没有实际需求，且会和现有的说明预算竞争 |

<a id="suggestions"></a>
## 判断哪些值得引入

<a id="three_categories"></a>
### 结论总表（值得做 / 待用户决定 / 观察 / 不做）

| 类别 | 条目 | 理由 |
| --- | --- | --- |
| **值得做** | P1-1 资源库显示只能以任务方式调用的工具数 | 改动小、只显示、不改协议；消除「服务器看起来没有工具」的误导 |
| **待用户决定（P2）** | Skills over MCP：是否引入、以哪种方式引入（[D1](#d1)） | 会改变信任边界和 Skill 来源；推荐现在不引入；若引入选「用户逐项批准后导入」，先写 ADR |
| **观察** | P2-1 `2026-07-28` 协商实测；P2-2 Tasks 全协议；P2-3 Events 作为值班触发；P2-4 File Uploads / MCP Apps | 都取决于 Agent 或服务器生态的实际变化，且多数会改变协议支持范围或信任边界 |
| **不做** | 自动复制服务器 Skill；服务器发起的 Elicitation；EMA / OAuth；sampling；常规 `resources` / `prompts` 转发；路由自己的工具搜索；外部 webhook；现在实现 tasks 协议 | 与既有 ADR、单机单用户、审批归用户、Skill 只由用户导入等约束冲突，或数据升级成本过高 |

<a id="already"></a>
### 已有（不需要改的部分）

- **工具路由本身**：`initialize`、`ping`、`tools/list`、`tools/call` 四个方法；上游分页最多 100 页；取消转发、5 分钟调用超时、工具名 `<服务器>__<工具>` 和哈希、说明前加 `[<服务器>] `、`initialize` 的 2048 字符预算、未知工具 `-32602`、按 Run 启动和回收服务器、`roots/list` 与 `ping` 的答复（[MCP 路由](../domains/20261116-resource_library.md#mcp_router)，代码见 `backend/src/orchestrator/mcp_router.rs:493-581`、`backend/src/orchestrator/mcp_client.rs:214-225`）。本窗口的外部进展没有推翻这些。
- **Skill 交付模型**：Skill 只由用户在应用中新建、从本机目录复制或从 Git 来源导入，Run 创建时复制进成员资源包，由 `janus-skills` 目录按需查找（[资源库：Skill](../domains/20261116-resource_library.md#skills)、[Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)、`backend/src/library/deliver.rs:29-46`）。它和 SEP-2640 的「显式用户政策」同向，本报告不建议改。
- **Agent 发起的表单和审批**：ACP 层已支持 `elicitation/create` 的 form 和 url 两种模式，并转成用户请求卡片（`backend/src/acp.rs:152`、`backend/src/orchestrator/turn.rs:833`、`:889-902`；[ACP 用户请求的回复](../interfaces/20261031-acp_integration.md#user_request_replies)）。这与 MCP 层的服务器请求是两件事。
- **看板和审批归属**：看板是 Agent 之间唯一的协作通道，审批和表单始终交给用户（[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)、[ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md)）。本报告不对这两条提出修改。

<a id="worth_doing"></a>
### 值得做

<a id="p1_task_only"></a>
#### P1-1 资源库显示「N 个工具只能以任务方式调用」

- **是什么（F/I）**：在 Janus 目前支持的 2025-11-25 握手下，服务器仍可能把工具标成 `execution.taskSupport: "required"`（F：`schema/2025-11-25/schema.ts:1241`）；路由现在把它们从所有列表里过滤掉，也调用不到（F：`mcp_router.rs:234-238`、`:458`、`:468`），预热的工具数只数能调用的（F：`:288`）。Claude Tasks GA 之后，服务器更可能提供这类工具（I，不是直接证据）。
- **为什么值得做**：用户看到「已准备好 · 0 个工具」时，无法区分「服务器没有工具」和「有工具但 Janus 不支持任务方式」。这是一处诚实的可见性缺口；补齐它不需要实现 tasks 协议，也不需要改变 `tools/list` 的行为。
- **最小落点**：
  - `backend/src/library.rs:49-53`：`Warm::Ready` 从 `{ revision, tools }` 改为带两个计数（例如 `tools` 和 `task_only`），或者加一个只读字段；`Warm` 只在服务运行期间存在，不落库。
  - `backend/src/orchestrator/mcp_router.rs:288`：在预热结果里同时数 `callable` 和 `!callable` 的工具；`remember_tools` 已经保存了全部工具，不需要额外请求。
  - `backend/src/library.rs` 的 HTTP 返回和 `frontend/Sources/Janus/UI/Settings/LibraryView.swift:437-452`：`ready` 时显示「已准备好 · N 个工具」和「其中 M 个只能以任务方式调用」。
  - [资源库：MCP 路由](../domains/20261116-resource_library.md#mcp_router) 第 116 行补一句「资源库显示其中有多少个只能以任务方式调用」，不改变「不列出、调用不到」的行为。
  - 测试：`backend/src/orchestrator/flow_tests/library_router.rs:127`、`:154` 已断言 `probe__task_only` 不出现在 `tools/list`、调用返回 `-32602`；新增预热结果里 `task_only` 计数的断言即可，现有 fixture `backend/testdata/fake_mcp.py:39` 已经够用，不要另加 fixture。
- **约束和边界**：只针对旧握手下仍会收到 `execution.taskSupport` 的服务器；不实现 task 增补调用、`tasks/*`、任务存储或任务 UI；不新增持久化字段，因此不需要数据迁移；文件都远低于 2000 行。**不要**把这条扩大成「列出这些工具」或「让 Agent 试着调用」。
- **优先级理由**：P1 而不是 P0——它不阻塞任何现有功能；但比观察项更值得现在做，因为改动小、可逆，并且直接修正一个会误导用户的显示。

<a id="observe"></a>
### 观察（含触发条件）

<a id="p2_protocol"></a>
#### P2-1 实测 Claude Code 与 Janus 路由的 `2026-07-28` 协商

- **现状（F）**：Janus 只支持到 2025-11-25（`mcp_client.rs:21-25`）；Claude Code v2 新增 2026-07-28，但对不支持的服务器按 v1 连接（[Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md)，F）。**没有运行验证**（U）。
- **触发条件**：实测发现 Claude Code 在 `auto` 模式下因为 Janus 回答 2025-11-25 而无法列出或调用工具；或 Claude Code / Codex 的后续版本不再接受旧版本握手；或用户实际使用的服务器要求 2026-07-28。
- **重评时做什么**：迁移是路由和客户端的重写，按 [20261120 调研](20261120-mcp_router_practices.md) 第 73 行的结论单独立项、先写 ADR；不要在没有实测证据时改 `VERSIONS`。
- **验证方法**：见[待验证问题](#open_questions)第 1 条。

<a id="p2_tasks"></a>
#### P2-2 Tasks 全协议

- **触发条件**：用户实际使用的服务器把核心工具标成 `taskSupport: required`，P1-1 的显示已经无法满足；或者两个 Agent 的 tasks 能力不再要求 2026-07-28，Janus 不升级就会丢失功能。
- **为什么不现在做**：完整 tasks 需要任务创建、`tasks/get`、`tasks/update`、`tasks/cancel`、长时间等待、结果回投、Run 取消时的撤回，以及 UI 和持久化；这是一次协议和存储的扩展，不是路由的小改。SEP-2663 还把协商改成每请求 client capabilities（F），Janus 的两侧 `initialize` 都要改。
- **约束**：会改变协议支持范围和数据结构，需要 ADR、迁移和 `data_shape`；在没有用户需求前不做。

<a id="p2_events"></a>
#### P2-3 MCP Events 作为工作区值班的触发

- **现状（F）**：Events 是官方扩展（SEP-3415 Final，Beta），要求协议 2026-07-28 或更高，支持 poll、push 和 webhook 三种投递（[overview](https://modelcontextprotocol.io/extensions/events/overview)，F）；Janus 不支持 2026-07-28，MCP 端点也没有 GET/SSE 推送通道（`http.rs:175-176`、`streams.rs:110-113`），值班只按 cron 醒来（[ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md)）。
- **触发条件**：用户明确要求「服务器事件唤醒值班」，并愿意先改 ADR。到那时要先定义来源信任、事件去重、唤醒频率和失败处理；**不要**因为协议多了一个 beta 就改方向。
- **结论**：现在不做。ADR 已经把「全局 webhook 路由」列为不做，改变它属于用户决定。

<a id="p2_others"></a>
#### P2-4 File Uploads 与 MCP Apps

- **File Uploads**：Anthropic 说「rolling out soon」（F），当前不可用（U）。Janus 的 `tools/call` 只转发 JSON 参数（`mcp_router.rs:539-543`），没有文件通道。触发条件是 GA 且有用户实际使用的服务器把文件上传作为必需能力；现在不引入。
- **MCP Apps**：官方第一个扩展（F），需要 `resources` 和宿主渲染 UI。Janus 的界面是任务、看板、事件和审批，没有渲染服务器 UI 资源的页面；触发条件是 Agent 的客户端能力或用户需求明确要求，且要先有 resources 能力。现在不引入。

<a id="not_worth"></a>
### 不做（理由）

- **Run 开始时自动复制服务器附带的 Skill**：第三方 Skill 指令会在用户没有逐项看过的情况下进入每次 Run；违反 SEP-2640 的「Explicit user policy governs whether a skill is loaded at all」和 Janus 的「Skill 只由用户导入」（[资源库：Skill](../domains/20261116-resource_library.md#skills)、[安全边界：资源库](../operations/20261031-security.md#library)）。如果用户要，走「逐项批准后导入」，见[待用户决定](#decisions) D1。
- **让 Agent 直接读 `skill://` 资源**：需要给路由加 `resources` 能力，第三方内容按需进入上下文；来源标记、按来源命名空间和用户可见的清单都还没有；版本也不固定。至少要先完成 D1 的 ADR，不能作为「顺便支持」。
- **服务器发起的 Elicitation**：Janus 的 MCP 层现在是 `-32601`（`mcp_client.rs:218-221`）；要做的话需要新的用户请求来源标识、UI、超时和取消策略，URL 模式还有钓鱼面。审批和表单始终归用户（[ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md) 第 4 条、[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md) 第 5 条），不能在「服务器请求」和「Agent 请求」之间混淆归属。现在不做，触发条件见 D2。
- **Enterprise-Managed Authorization / OAuth**：单机单用户没有企业身份，精选库明确不收需要 OAuth 登录的服务器（[ADR：资源库的精选库随应用发布](../decisions/20261119-library_catalog.md) 第 2 条）；引入会扩大凭据的存放和使用面（[安全边界：资源库](../operations/20261031-security.md#library)）。
- **Sampling**：让第三方服务器借用用户的模型，会绕开 Agent 原生的权限模式和用户的审批；Janus 没有这个需求，也没有对应的用户界面。
- **常规转发 `resources` / `prompts` / `instructions`**：2026-11-20 的调研已经决定不做（[调研](20261120-mcp_router_practices.md) 第 74 行）；上下文会随服务器增长，信任面也变大。只有 Skills over MCP 会重新打开这个议题，按 D1 单独处理。
- **路由自己的工具搜索**：会让所有调用在 Agent 看来都是同一个工具，破坏 Agent 原生的逐工具权限规则和确认（[调研](20261120-mcp_router_practices.md) 第 69 行、[ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md)）；两个 Agent 自己已经有延迟工具搜索。
- **外部 webhook / 服务器事件唤醒值班**：[ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md) 的「不做」明确列出；改变它需要先改 ADR，属于用户决定。
- **现在实现完整的 tasks 协议**：协议、存储、UI 和失败处理都是一次大改；没有用户需求，也没有证据表明 Janus 的服务器已经在用任务方式提供核心工具（U）。先做 P1-1 的诚实显示。

<a id="open_questions"></a>
## 待验证问题

| 问题 | 现状 | 怎么验证 |
| --- | --- | --- |
| Claude Code 经 ACP 连接 Janus 路由时，`initialize` 如何协商 2026-07-28？ | Claude Code 文档说对不支持的服务器按 v1 连接（F）；Janus 侧的实际应答未测（U） | 1. 用当前 Janus 启动一个 Claude Code 连接，让它连 `janus-tools`；2. 在中间加一个记录请求的本地代理，或在 Janus 侧临时记录 `initialize` 的请求与响应；3. 记录客户端是否先探测 2026-07-28、Janus 是否回答 2025-11-25、之后 `tools/list` 和 `tools/call` 是否正常；4. 用 `MCP_PROTOCOL_NEGOTIATION=legacy` 做对照组，只有 legacy 正常而 auto 异常才是 Janus 要处理的问题；5. 同时记录 Claude Code 版本（v2 默认门槛和 tasks 门槛不同：tasks 要 v2.1.304+）和是否取到功能开关 |
| Claude Code 的非交互会话在 Janus 路由上收到服务器 elicitation 时会怎样？ | 文档说非交互 tasks 路径不弹窗、请求服务器取消（F）；经 Janus 路由时请求先被 `-32601` 拦下（代码为 F），Agent 看到什么未测（U） | 用一个会在 `tools/call` 中发 `elicitation/create` 的本地测试服务器，经 `janus-tools` 调用，记录 Agent 收到的工具错误文本；对照组直连该服务器 |
| 用户实际使用的服务器里，有多少工具是 `taskSupport: required`？ | 没有统计；现有测试 fixture 只有一个样本（F：`backend/testdata/fake_mcp.py:39`） | 扫描现有 `~/.janus/cache/mcp-tools/<资源 id>/<revision>.json` 和预热结果，统计 `required` 的工具数和服务器数；没有数据时，用 P1-1 上线后的显示收集 |
| MCP Events 的事件模型能否用于工作区值班？ | F：Events 是 SEP-3415 Final、Beta，要求协议 2026-07-28+，支持 poll / push / webhook；Janus 不支持该协议（F），ADR 不做外部 webhook | 读 Events 规范和客户端支持矩阵，评估是否先迁移协议并改 ADR；在用户明确要求前不改值班的 ADR |
| Codex 0.174 alpha 是否有用户可感知的 MCP 变化？ | release body 只有版本名（F）；提交归属未知（U） | 等 0.174 正式版 release notes；或用 `codex --version` 对照 `main` 的提交逐项验证，不能按 commit 主题推断 |
| Preflight / Selective Verification 的结论是否适用于 Janus 的验证者？ | 两篇都是类比（I）；没有 Janus 场景的实验 | 用 Janus 已交付 Run 的验证记录做小样本回看：验证者是否运行了受影响路径、是否把预算放在最可能改变结论的任务上；不改 schema |

<a id="decisions"></a>
## 待用户决定

以下事项会改变协议支持范围、信任边界或产品方向，本报告不假定已批准。每项给出推荐。

<a id="d1"></a>
### D1 是否引入 Skills over MCP？（P2，待用户决定）

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **不引入（推荐）** | 维持现状：Skill 只由用户新建、从本机目录复制或从 Git 来源导入；服务器附带的 Skill 不进入上下文 | 是。与 SEP-2640 的显式用户政策和现有 ADR 一致，改动为零 |
| 用户逐项批准后导入（如果引入，选这条） | 新增「从 MCP 服务器导入 Skill」的来源：应用列出服务器提供的 Skill，用户勾选后导入为资源项，按 revision 固定；需要 ADR、数据迁移（`origin` 增加来源类型）、UI 和测试 | 仅当用户明确要时；它仍把 Skill 内容当第三方内容，但用户做了导入决定 |
| Run 开始时调用 `skills/list` 自动复制进资源包 | 第三方 Skill 指令未经逐项批准进入每次 Run；违反「Skill 只由用户导入」；版本随服务器变化；服务器不可达时 Run 开始会变慢或失败 | 否。信任边界和可解释性都不可接受 |
| 让 Agent 直接读 `skill://` 资源 | 路由要声明并转发 `resources`；内容按需进入上下文；用户看不到清单，版本不固定 | 否。至少要先完成本表的 ADR 讨论，单独评估 |

- **如果选「逐项批准后导入」**：信任边界按 SEP-2640 处理——标记来源服务器、按来源命名空间取名、把内容当不可信输入；`origin` 需要新增 `mcp`（或类似）类型并记录服务器 id 和 Skill 路径，按[数据升级](../decisions/20261118-data_migrations.md)加迁移和 `data_shape`；导入时按内容 hash 固定 revision，刷新由用户发起；服务器不可达只在导入或刷新时报错，不影响已有 Run。
- **推荐理由**：Janus 的 Skill 模型已经把「可信与否」交给用户导入的那一刻（[安全边界：资源库](../operations/20261031-security.md#library)），自动复制会把这一点抹掉；而 SEP-2640 恰好把「显式用户政策」写成硬要求。

<a id="d2"></a>
### D2 是否支持资源库服务器发起的 Elicitation？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **不做（推荐）** | 服务器发起的 elicitation 继续得到 `-32601`；Agent 自己的表单/审批不受影响 | 是。单用户、没有实际需求，UI 和来源标识成本高 |
| 只对本地命令服务器、只接受表单模式（form） | 缩小钓鱼面；仍需新的「来自服务器」请求事件、UI、超时和 Run 取消时的撤回 | 仅当用户有具体服务器因为这个能力无法使用时 |
| 全部支持（form + url） | URL 模式有钓鱼风险；来源和审批归属容易混淆 | 否 |

- **如果做**：落点是 MCP 路由把服务器请求转成现有的用户请求卡片并等待，但必须新增来源字段（区分 Agent 与服务器）、显示服务器名字和地址、只接受表单模式（form）、超时和 Run 取消时向服务器回 `cancel`；审批仍然只属于用户，不能借这个通道让服务器「批准」任何操作。
- **推荐理由**：Janus 的审批/表单链路现在只服务 Agent（[审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)、[ACP 用户请求](../interfaces/20261031-acp_integration.md#user_request_replies)），加一条第三方服务器到用户的通道会改变信任边界，而收益没有证据。

<a id="d3"></a>
### D3 是否把 MCP Events 用作工作区值班的外部触发？

| 选项 | 影响 | 推荐 |
| --- | --- | --- |
| **保持不做（推荐）** | 值班仍只按 cron 醒来；[ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md) 的「不做外部 webhook」不变 | 是。没有用户需求，且 Events 要求 2026-07-28+，Janus 尚未支持 |
| 先写 ADR 评估 | 需要定义来源信任、去重、唤醒频率、预算和失败处理；会改变值班的产品方向 | 仅在用户明确要「服务器事件唤醒值班」时 |

<a id="sources"></a>
## 来源

以下来源均于 2026-11-24 访问。多数来源的核对时点为 18:44 UTC；Events overview 和 Claude Code MCP 文档在返工时重新打开（约 18:55 UTC），Claude Code 文档的 tasks 段落已更新，elicitation 事实与快照一致。标 U 的条目只有二手来源、页面异常或无法核实。

**Anthropic 与 MCP 规范**

- [MCP servers can now ship skills, run tasks, and more with Claude](https://claude.com/resources/articles/skills-over-mcp-tasks-and-more)，2026-11-24，F。
- [Two Years of MCP Builders](https://blog.modelcontextprotocol.io/posts/mcp-turns-two/)，2026-11-24T09:00Z，F；[raw 源](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/blog/content/posts/mcp-turns-two.md)，F。
- [SEP-2640 Skills Extension（Final）](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2640-skills-extension.md)，Created 2026-04-23，F。
- [SEP-2663 Tasks Extension（Final）](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/seps/2663-tasks-extension.md)，Created 2026-04-27，F。
- [MCP 2025-11-25 schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-11-25/schema.ts)，`execution.taskSupport` 在第 1241 行，F。
- [MCP releases](https://github.com/modelcontextprotocol/modelcontextprotocol/releases)，最新为 2026-07-28，F。
- [MCP Events overview](https://modelcontextprotocol.io/extensions/events/overview)，SEP-3415 Final、扩展 Beta、要求协议 2026-07-28+，访问 2026-11-24，F。
- [Skills 扩展 overview](https://modelcontextprotocol.io/extensions/skills/overview)，访问时被拦，U（未使用，以 SEP 原文替代）。

**Claude Code**

- [Claude Code MCP 文档](https://code.claude.com/docs/en/mcp.md)，页面未标日期，F；`execution.taskSupport` 一词在全文 0 命中，该词为 U。

**Codex**

- [Codex releases](https://github.com/openai/codex/releases)，2026-11-24，F。
- [rust-v0.174.0-alpha.1](https://github.com/openai/codex/releases/tag/rust-v0.174.0-alpha.1)，2026-11-24T01:46:17Z，F（时间）/ U（用户可感知能力）。
- [rust-v0.174.0-alpha.2](https://github.com/openai/codex/releases/tag/rust-v0.174.0-alpha.2)，2026-11-24T16:14:14Z，F / U。
- [rust-v0.173.0-alpha.5](https://github.com/openai/codex/releases/tag/rust-v0.173.0-alpha.5)，2026-11-24T06:44:18Z，F / U。
- [Codex commits](https://github.com/openai/codex/commits)，2026-11-24，F（提交主题）/ U（进入哪个 release）。

**论文**

- [Preflight: Agentic Last-Mile Verification for AI Software Supply Chains](https://arxiv.org/abs/2611.31110)，2026-11-23，F。
- [Selective Verification in Multi-Agent Orchestration with Large Language Models](https://arxiv.org/abs/2611.28173)，2026-11-20，F（窗口外）。

**社区**

- [Moonbooth Show HN 50631044](https://news.ycombinator.com/item?id=50631044)，2026-11-24，F（提交时间）/ U（采用信号）。
- [Moonbooth 站点](https://www.moonbooth.ai)，无发布日期，U。
- [TabbyML: Beating ClickBench SOTA with a team of agentic systems engineers](https://www.tabbyml.com/blog/beating-clickbench-sota-with-a-team-of-agentic-systems-engineers/)，2026-11-15，F（厂商自报）。
- [TabbyML HN 50630431](https://news.ycombinator.com/item?id=50630431)，2026-11-24，U（2 分）。
- [LLM agents work for their parent companies, not for you](https://michaelharley.net/posts/2026/11/01/llm-agents-work-for-their-parent-companies-not-for-you/)，2026-11-01，F（观点内容）/ U（分数稳定性）。
- [HN 50627340](https://news.ycombinator.com/item?id=50627340)，2026-11-24，U（访问时 696 分快照）。
- [MyGraph](https://github.com/dieron/mygraph)，最近 push 2026-11-21，F（README）/ U（采用信号）。
- [MyGraph Show HN 50630397](https://news.ycombinator.com/item?id=50630397)，2026-11-24，U（1 分）。

**本仓库**

- [2026-11-24 多 Agent 协作与 Agent Team 进展观察](20261124-multi_agent_collaboration.md)，2026-11-24，本报告只引用、不重复。
- [调研：MCP 路由的通行做法](20261120-mcp_router_practices.md)，2026-11-20，第 64、69、73、74 行的已决事项。

<a id="janus_references"></a>
## Janus 现状索引

本报告引用的 Janus 文档章节和代码：

- [资源库：Skill](../domains/20261116-resource_library.md#skills)；[Run 和会话怎样拿到资源](../domains/20261116-resource_library.md#delivery)；[MCP 路由](../domains/20261116-resource_library.md#mcp_router)；[Skill 路由](../domains/20261116-resource_library.md#skill_router)。
- [安全边界：资源库](../operations/20261031-security.md#library)。
- [审批和表单](../domains/20261031-goal_mode.md#user_requests_in_goal_mode)；[ACP 用户请求的回复](../interfaces/20261031-acp_integration.md#user_request_replies)；[证据方法](../interfaces/20261031-events.md#evidence_methods)。
- [ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md)；[ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)；[ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](../decisions/20261120-library_routed_only.md)；[ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md)；[ADR：资源库的精选库随应用发布](../decisions/20261119-library_catalog.md)；[ADR：工作区值班派发独立的 Run](../decisions/20261121-workspace_loops.md)；[ADR：数据升级](../decisions/20261118-data_migrations.md)。
- 代码：`backend/src/orchestrator/mcp_client.rs:21-25`（`VERSIONS`）、`:31-35`（对 Agent 的版本回答）、`:69-85`（URL 服务器的静态 header）、`:218-221`（服务器请求的答复）、`:226-230`（路由作为客户端的 `initialize` 能力）、`:231-235`（服务器版本严格校验）；`backend/src/orchestrator/mcp_server.rs:34-41`（路由/看板的 `initialize`，能力声明在 `:37`）；`backend/src/orchestrator/mcp_router.rs:234-238`（`callable`）、`:288`（预热计数）、`:458`、`:468`（列表过滤）、`:493-581`（方法集）、`:539-543`（`tools/call` 参数）、`:580`（兜底 `-32601`）；`backend/src/library/deliver.rs:29-46`（资源包）、`:92`（会话参数）；`backend/src/library.rs:49-53`（`Warm`）、`:616`（`place_skill`）；`backend/src/acp.rs:152`（ACP elicitation 能力）；`backend/src/orchestrator/turn.rs:833`、`:889-902`（用户请求的回答）；`backend/src/http.rs:175-176`、`backend/src/http/streams.rs:110-113`（端点只有 POST）；`frontend/Sources/Janus/UI/Settings/LibraryView.swift:437-452`（预热状态显示）。
- 测试：`backend/src/orchestrator/flow_tests/library_router.rs`（路由流程测试）；`backend/testdata/fake_mcp.py:39`（`taskSupport: required` 的 fixture）。
