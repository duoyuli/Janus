# 多 Agent 协作与 Agent Team 进展观察（2026-11-10）

> 覆盖窗口：2026-11-03 至 2026-11-10，另有少量更早发布、但当前形态仍在演进的条目。
> 2026-11-11 补充：用 OpenCLI 直接读取 X 后发现了 11 月 9-11 日的 Antigravity `/swarm`、Anthropic 1,000-Agent 实验等进展，摘要中「没有新东西」的判断已被修正，见[补充报告](multi_agent_collaboration_2026_11_11.md)。
> 目的：追踪多 Agent 协作和 Agent Team 的进展，判断哪些设计值得 Janus 借鉴。
> Janus 的当前设计见[目标模式与共享看板](../domains/goal_mode.md)和 [Run 生命周期](../domains/run_lifecycle.md)；本文的「现状」一栏按这两篇文档和当前代码核对。

## 摘要

本周没有出现推翻现有协作范式的新东西。进展集中在两件事：把多 Agent 的控制面做成正式接口，以及给「人和 Agent 混合团队」补治理。

- **控制面成为竞争点。** OpenAI 把 `spawn_agent`、`send_message`、`wait_agent`、`interrupt_agent`、`list_agents` 做成 Responses API 的一等动作；Claude Code 的 Agent Teams 仍是实验特性，但补上了 `TeammateIdle`、`TaskCreated`、`TaskCompleted` 三类 hook 和更明确的跨 Agent 授权规则。
- **工程样本里最值得抄的是安全审查。** Nous Research 的 `hermes-security` 给出了完整的多 Agent 安全审查流程：分块、并行 reviewer、覆盖缺口、测试确认、SARIF 导出、逐次授权。
- **共享记忆和并行假设开始产品化。** PagerDuty 的 Paige 把「同一条时间线」「按置信度排序的假设」「把结论写回共享记忆」做成值班体验的一部分，与本项目的工作区共享资料和看板高度同构。
- **研究侧偏冷。** 在相同思考 token 预算下，单 Agent 不输多 Agent；普通分解流水线成本约 3.2 倍而准确率没有显著提升；只有廉价模型承担足够比例的输出 token 时，异构流水线才回本。这支持 Janus「由主控决定是否协作」的设计，但也说明成本必须可见。
- **Janus 的差距不在协作模型，而在成本和证据。** 事件溯源、看板推送、冲突检测、验证和恢复已经比多数新框架完整；缺口是 token / 时间成本没有账本，验收证据没有强度分级，主控缺少「停下但保留上下文」的显式动作。

## 方法与限制

- X（Twitter）的网页抓取在本环境返回 HTTP 402，无法直接读取时间线；带 `site:x.com` 的搜索也不稳定，只能间接拿到少量账号和帖子。本期以 WebSearch 的索引结果、聚合站（aiagentstore、AgentOrchestrators）、厂商文档和 GitHub 发布说明交叉核对。
- 聚合站的日期偶有矛盾，例如把 2 月的事件混进 11 月的周报。正文只采用能与官方文档或发布日期对应的内容；无法确认的日期写「未确认」。
- OpenAI Hosted multi-agent 的官方参考页本次没有直接抓到，细节来自 Microsoft Foundry 的镜像文档和二次报道，正式实现以 OpenAI 文档为准。
- 如果要稳定覆盖 X，需要用户提供已登录的会话、X API 凭据，或指定要盯的账号列表；否则只能依赖搜索索引。

## 本周进展

### Anthropic：Claude Code Agent Teams 仍在实验期

Agent Teams 在 2026 年 2 月随 Claude Opus 4.6 发布，属于研究预览（[heise，2026-02-05](https://www.heise.de/en/news/Anthropic-introduces-Claude-Opus-4-6-with-Agent-Teams-11167248.html)）。截至 2026-11-10，官方文档仍要求设置 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`，并明确标注为实验特性（[Claude Code 文档](https://code.claude.com/docs/en/agent-teams)）。

当前形态：

- 一个 lead 会话加若干 teammate；每个 teammate 是独立的 Claude Code 实例，有自己的上下文窗口，可以直接给彼此发消息。
- 协调靠共享任务列表和 mailbox。mailbox 是 `~/.claude/teams/{team}/inboxes/{agent}.json`，任务领取用文件锁防止并发抢占。
- lead 可以显式分配任务，teammate 也可以在做完当前任务后自己领取下一个未分配、未阻塞的任务。
- 任务有 pending、in progress、completed 三种状态，可以声明依赖；依赖完成后自动解除阻塞。
- 提供 `TeammateIdle`、`TaskCreated`、`TaskCompleted` 三个 hook，退出码 2 可以阻止完成或继续给 teammate 施压。
- 跨 Agent 消息不被当作授权：消息里的「已批准」按不可信输入处理；teammate 不能代替用户批准权限，也不能把被拒绝的操作转给另一个 teammate 绕过检查。
- 建议规模是 3-5 个 teammate；token 成本显著高于单会话，且进程内 teammate 的缓存只有 5 分钟。

已知限制：一个会话只有一个团队、不能嵌套团队、lead 固定、spawn 时不能单独设权限模式、进程内 teammate 不支持 `/resume`、任务状态可能滞后、关闭可能很慢。这些限制说明「团队」目前仍然是一个交互层功能，不是可靠的任务系统。

判断：Anthropic 把团队做进了终端交互（分屏、直接对话、任务列表），体验领先；但恢复和权限仍是短板。Janus 的事件溯源和 `resume_run` 在恢复上更完整，权限完全交给 Agent 原生模式也更干净。

### OpenAI：把多 Agent 动作提升为 API 一等接口

Hosted multi-agent（beta）在 Responses API 中暴露六个动作（[Microsoft Foundry 镜像文档](https://learn.microsoft.com/bg-bg/azure/foundry/openai/how-to/responses-multi-agent)、[mixed-news 报道](https://mixed-news.com/en/openai-gpt-6-1-sol-multi-agent-beta-no-subagent-limit/)）：

| 动作 | 作用 |
| --- | --- |
| `spawn_agent` | 创建子 Agent 并分配初始任务 |
| `send_message` | 给已有 Agent 排队一条消息，不启动新一轮 |
| `followup_task` | 给已有非根 Agent 追加工作并恢复它的回合 |
| `wait_agent` | 等待调用方 mailbox 里的更新 |
| `interrupt_agent` | 中断另一个 Agent 正在进行的回合，不删除它的上下文 |
| `list_agents` | 返回 Agent 树、状态和每个 Agent 最新的任务消息 |

结构上，根 Agent 是 `/root`，子 Agent 在 `/root/researcher`、`/root/reviewer/tester` 这样的路径下。`max_concurrent_subagents` 默认 3，官方没有给出深度或总数的硬上限。动作由 OpenAI 服务执行，应用不能自己执行这些动作或回填结果；响应中以 `multi_agent_call` 项呈现。启用方式是 `multi_agent.enabled` 加 `OpenAI-Beta: responses_multi_agent=v1`，与 `/responses/compact`、`reasoning.summary`、`max_tool_calls` 不兼容。

判断：这套接口和 Janus 看板的语义最接近。`interrupt_agent`（中断但保留上下文）和 `list_agents`（Agent 树与状态）正是 Janus 目前最缺的两个控制面动作。

### Microsoft：agent-framework-go v0.3.0

2026-11-03 发布预发布版（[onlylabs 的发布摘要](https://www.onlylabs.fyi/signals/77041e11-2ac8-4e8d-b3e3-55ddf9519e10)、[项目仓库](https://github.com/microsoft/agent-framework-go)）。

- Skill 变成可组合来源：聚合、缓存、过滤、去重；发现过程感知会话上下文；脚本参数从位置参数改成结构化 JSON。
- Agent-as-tool：任意 Agent 可以包成工具，输入参数名和描述可配置（`agenttool.New`）。
- 后端无关的工具追踪；子工作流输出可以独立转发。
- 其他：Cosmos DB 聊天历史、Foundry API key 认证、上下文压缩状态隔离。

判断：语言和部署模型与 Janus 不同，直接借鉴有限。可借鉴的是「角色即工具」和「Skill 来源可组合」：Janus 的角色已经能被主控编排，但还没有把角色当作可复用的工具描述暴露给别的团队或 Run。

### Nous Research：Hermes v0.23 与 hermes-security

Hermes Agent v0.23.0（2026-11-05，[发布说明](https://github.com/NousResearch/hermes-agent/releases/tag/v0.23.0)）值得注意的协作与治理变化：

- 一个 gateway 拥有所有会话：CLI、TUI、Desktop、编辑器（ACP）、bot 和 cron 共用一条队列和一份历史，避免重复或丢失消息。
- MCP 工具可以声明按次价格和每次会话的花费上限；超限前必须批准，即使在 YOLO 模式。
- 群聊支持 lead bot 或 `auto` 路由；bot 之间的循环用 `max_hops` 限制，默认 4。
- Skill 可以锁定（`hermes curator lock`），即使 `--yolo` 也不能改写。
- 后台 subagent 不再随回合停止而终止；`/bg` 和路由聊天会等待委派完成。
- 审批门禁增强：管道执行 `curl … | sudo bash`、PowerShell 删除、隐藏凭据上传都会触发批准；过期的批准卡不能批准下一个请求；提示会显示隐藏 Unicode 和控制字符。

`hermes-security` 是官方安全审查插件（Apache-2.0，需要 Hermes 0.21.5+，[插件页](https://hermes-agent.nousresearch.com/marketplace/hermes-security)）。它把多 Agent 协作用在安全审查上：

- 先建威胁模型，确定攻击者能到达什么、哪些输入可信。
- 按目录把代码切成约 4000 行的块；仓库在 90 块（约 36 万行）以内时全读，超出后按风险排序（路由、请求输入、shell / SQL、反序列化、鉴权、webhook）读到预算用完。
- 最多 10 个 reviewer 并行，另有 1 个 reviewer 专门扫入口和信任边界；最危险的两块会有第二个独立 reviewer；没读完的块可以重派，最多 6 轮，每轮必须有进展。
- 报告保留证据、被否决的候选、测试结果和覆盖缺口；区分「有代码依据」和「测试确认」；建议补丁只读，不自动应用。
- 导出 Markdown、JSON、CSV 和 SARIF。
- 运行被测应用需要逐次授权（一个 origin、一小时、200 次请求），并明确声明「不是沙箱」；密钥只保留类型、位置和单向指纹；仓库内容按数据处理，不执行 git hook。

判断：这是本期最完整的多 Agent 安全审查工程样本。最值得借鉴的不是「并行」，而是「覆盖缺口」和「测试确认」这两个概念：零发现加上部分覆盖，不等于代码干净。

### PagerDuty：Paige 的共享时间线与并行假设

Paige 是 PagerDuty 的 SRE Agent，2026-11-03 GA（[PagerDuty 博客](https://www.pagerduty.com/blog/announcements/reliability-in-the-ai-era-ai-that-works-the-incident-with-you/)）。

- Paige 可以像普通值班人一样加入升级策略；默认 human-in-the-loop，自治权按事故逐次授予。
- SRE Workbench 让用户和 Paige 从同一份证据、拓扑图和时间线调查；时间线可按全部、仅人、仅 Paige、审计日志过滤，保留 90 天。
- Deep Investigation 并行验证多个根因假设，按置信度排序，给出最可能原因，并说明排除了什么、为什么。
- 事故结束后把修复过程和有效 / 无效做法写回共享记忆，供人和 Agent 共同使用。

判断：与 Janus 的看板、进展时间线、工作区共享资料高度同构。真正的差距在两处：结构化记录「排除了什么」，以及把交付后的结论自动变成下一次可用的共享资料。

### CrowdStrike：AI Kill Switch 与 Agent Graph

CrowdStrike 在 2026-11-03 发布 AI Kill Switch（[官方博客](https://www.crowdstrike.com/en-us/blog/crowdstrike-introduces-ai-kill-switch/)）。

- 已上线的是「Agent Circuit Breaker」：通过 Falcon AI Gateway 或第三方网关切断 Agent 的模型访问。这是当前唯一实际可用的终止手段。
- 规划中的能力由一个控制面协调：端点终止进程并阻止重启、云上隔离、运行时撤销身份权限、SaaS 限制、浏览器会话结束。
- Agent Graph 把「意图 → 执行 → 接触到的资源」连起来，帮助判断在哪里干预；Charlotte Agentic SOAR 负责跨平台的自动化处置。
- 官方没有说明如何识别和匹配不同环境中的同一个 Agent，也没有给出终止延迟；部分能力标注为仍在开发。

判断：本地单机工具不需要云端处置面，但「终止、阻止重启、撤销令牌、统一入口」这四件事可以整理成 Janus 的紧急停止流程。Agent Graph 对应 Janus 的事件日志、产物和改动文件，值得做成一个审计视图。

### 其他项目

- **FrontierAgent（Apodex 开源）**：Agent Team 模式是 coordinator 加并行 subagent，Plan 页显示任务板；共享工作目录，写入进入同一个 diff 日志；写操作和危险 shell 需要批准，即使 `-y` 也会拦住硬拒绝项；sandbox 在启动时统一选择（native / bubblewrap / Docker），不能按 subagent 配置；失败类型不会在 trace 中分类（[README](https://raw.githubusercontent.com/ApodexAI/FrontierAgent/master/apodex/README.md)）。
- **NanoClaw**：在 Slack 里按需生成 Agent 团队，每个 Agent 有独立身份、容器和 token，有 lead 协调，共享 Canvas；Agent 只在被 @ 时回复，避免互相刷屏；可以单独撤销某个 Agent（[博客](https://nanoclaw.dev/pl/blog/nanoclaw-add-to-slack/)）。
- **ClawTeam v0.2.0**：文件或 P2P inbox、tmux worktree、团队模板；MIT，约 5.6k star；2026-03 发布后更新趋缓（[目录页](https://agent-orchestrators.com/tools/clawteam/)）。

### 研究侧：多 Agent 不是默认更优

- **Teamknitt（[arXiv 2611.12545](https://export.arxiv.org/abs/2611.12545)）**：概念和系统论文，没有对比结果。主张团队拓扑、信息共享范围、依赖和干预点应当是配置项，而不是写死在代码里；人在依赖波次边界干预；把「已排队 / 已应用 / 已理解 / 已改进」分开记录，信任和共享理解标记为未测量。
- **Federation of Agents（[arXiv 2509.20175](https://ar5iv.labs.arxiv.org/html/2509.20175)）**：能力向量、语义检索、策略门加资源惩罚；小集群（3-5）多轮草稿-批评。HealthBench Hard 得分 0.13，虽低但是最佳单 Agent 的 13 倍、无协调集成的 6.5 倍；单次查询约 536.7 秒（RTX A4000），冷启动问题明显。
- **The Coordination Tax（[arXiv 2610.37700](https://arxiv.org/html/2610.37700v1)）**：135 个代码任务、2430 次运行。普通三角色流水线成本约 3.22 倍、耗时 3.24 倍，准确率没有显著提升（−1.5pp）；换成便宜模型后成本降到 0.39 倍，但 pass@1 掉 6.7pp；按任务路由（oracle）能到 94.3%，每个解决任务约 $0.0098。结论是分解只有在廉价模型承担足够比例的输出 token 时才回本。
- **Tran & Kiela（[arXiv 2604.02460](https://www.semanticscholar.org/paper/Single-Agent-LLMs-Outperform-Multi-Agent-Systems-on-Tran-Kiela/a9bba78d6a7441ed43d05a0343518eeac3ac0c7e)）**：在相同思考 token 预算下，单 Agent 在多跳推理上优于多 Agent；此前的优势可以用未计入的计算量解释。
- **Anthropic 的多 Agent 研究系统复盘（[工程博客](https://www.anthropic.com/engineering/multi-agent-research-system)）**：多 Agent 约 15 倍 token，token 消耗解释了 80% 的性能方差；适合高价值、高度并行的任务，不适合所有任务。

判断：这些结果支持 Janus 现有的「主控决定是否协作，不强制规划」，但也说明必须把成本和收益暴露给主控和用户，否则会滑向「为了协作而协作」。

## 横向对比

| 维度 | Janus | Claude Code Agent Teams | OpenAI Hosted multi-agent | PagerDuty Paige | Hermes security |
| --- | --- | --- | --- | --- | --- |
| 控制面 | 主控通过看板 MCP 编排；编排器调度 | lead 加共享任务列表；teammate 自领任务 | API 动作：spawn / send / wait / interrupt / list | 人机共享工作台 | 一次扫描一个编排 |
| 任务与状态 | 7 个调度状态，事件溯源，重启恢复 | pending / in progress / completed，无持久恢复 | Agent 树与状态，由服务维护 | 事故时间线 | 分块与 reviewer 轮次 |
| 通信 | 只经看板；协作者之间无直接对话 | 直接消息加 mailbox | 消息与 mailbox | 共享时间线 | reviewer 之间不直接对话 |
| 审计 | 全部状态和消息是事件 | 会话记录，团队配置随会话删除 | `multi_agent_call` 项 | 90 天时间线与审计过滤 | JSONL trace、SARIF |
| 权限 | Agent 原生模式；审批和表单始终归用户 | 继承 lead 模式；跨 Agent 消息不算授权 | 未在本次来源中说明 | 逐次授予自治权 | 逐次授权运行应用；补丁只读 |
| 恢复 | `resume_run` 从事件重建 | 进程内 teammate 不支持 `/resume` | 未说明 | 事故结束后写回记忆 | 扫描可续跑 |
| 成本控制 | 6 小时墙钟预算 | 建议 3-5 人，token 显著增加 | 默认并发 3 | 未说明 | 分块与轮次上限 |
| 团队规模 | 由工作区角色决定 | 建议 3-5 | 默认并发 3，无硬上限 | 人加 Agent | 最多 10 个 reviewer |

## 对 Janus 的借鉴建议

下面按投入从低到高排序。每条都写清现状、建议、落点和风险；没有核实的 ACP 字段不作为设计前提。

### P0：小改动，先做

#### 1. 主控说明补团队规模与任务粒度

- **现状**：`goal_brief` 讲了协作要点，但没有给出规模、任务粒度和同文件冲突的具体数字；是否并行完全靠主控自行判断。
- **借鉴**：Claude Code 建议 3-5 个 teammate、每个 5-6 个任务；Anthropic 研究系统建议 3-5 个并发 subagent；Federation of Agents 的集群上限也是 3-5。
- **建议**：在说明里加一段短规则：默认同时使用 3-5 个协作者；一个任务对应一个可交付物；每个协作者保持 5-6 个任务；会改同一批文件的任务用依赖串行；任务说明要能独立看懂。
- **落点**：`goal_brief`（`backend/src/orchestrator.rs`，按仓库规则不要再往这个文件堆功能，可先只改文案）；[目标模式与共享看板](../domains/goal_mode.md#lead_brief)。
- **验收**：测试断言说明包含这段；用一个 flow test 观察主控拆分出的任务数量和依赖。
- **风险**：说明变长会占用主控上下文；用短句和列表，不要写成教程。

#### 2. 让 `team` 显示「最近活动」和「当前动作」

- **现状**：`team` 只列协作者信息和任务状态（id、执行者、状态、标题）。主控判断谁卡住，主要靠 `[停滞]` 推送或 `task_detail`。
- **借鉴**：OpenAI 的 `list_agents` 返回每个 Agent 的状态和最新任务消息；Claude Code 的 agent panel 显示 teammate 是空闲还是工作中。
- **建议**：在每个任务行后加「最近一次事件距今」「当前是否在等审批 / 提问 / 工具调用」「尝试次数 / 上限」。数据从内存中的 watch 状态和事件计算，不新增事件类型；重启后显示「未知」。
- **落点**：`board_team`、`team_lines_vec`；[看板 MCP 工具](../interfaces/board_mcp.md)的 `team`。
- **验收**：单元测试覆盖「运行中」「等待回答」「刚失败」三种；用 `fake_acp` 制造长时间工具调用。
- **风险**：时间信息会抖动，只给粗粒度（小于 1 分钟、几分钟、10 分钟以上）。

#### 3. 给验收证据加「方式」字段

- **现状**：结构化结果是 `checks: [{criterion, passed, evidence}]`，验证只核对结构（有 check、有证据、没有 false），不区分证据强度。文档已经说明「执行者的证据是它的说法，关键结果由主控自己核对」。
- **借鉴**：`hermes-security` 区分「有代码依据」和「测试确认」；PagerDuty 的假设按置信度排序并说明排除了什么。
- **建议**：增加可选字段 `method`，取值 `test`、`command`、`inspection`、`reasoning`。`result_gaps` 对 `passed: true` 但 `method: reasoning` 的条目生成提醒，不判失败；看板和 UI 显示标记。这样不改变「Janus 不运行验证命令」的边界，只把信息摆到台面上。
- **落点**：结构化结果 schema、`verify_result` / `result_gaps`、`fake_acp.py`、[Run 生命周期](../domains/run_lifecycle.md#structured_result)、[看板 MCP 工具](../interfaces/board_mcp.md#acceptance_criteria)、事件文档和 `DeriveTests`。
- **验收**：正例（`method: test` 通过）和反例（只有 `reasoning` 的条目产生提醒但不失败）各一个。
- **风险**：增加协作者的输出负担；字段缺失时按 `reasoning` 处理会误伤，建议缺失时不提醒，只在 UI 里标「未说明」。

### P1：需要一到两个模块

#### 4. 成本账本

- **现状**：只有 6 小时墙钟预算；没有 token、费用或尝试次数的汇总展示。当前后端没有读取 ACP 的 token 用量字段，适配器也没有统一上报。
- **借鉴**：Anthropic 的多 Agent 系统约 15 倍 token，token 解释了 80% 的性能方差；Coordination Tax 论文给出分解的回本条件；Hermes 给 MCP 工具做按次价格和会话上限。
- **建议**：先做「可观测三件套」：每次尝试的墙钟时间、尝试次数、产物大小。看板的 `task_detail` 和 Run 详情显示；主控在 `team` 里看到每个协作者已用时间。等适配器能上报 usage 时再记 token，不提前假设字段。预算可以保留墙钟阈值，另加「尝试次数上限」作为第二道闸。
- **落点**：`attempt` 的计时、`Task` / `Run` 统计、`db.rs` 可选列、应用的 Run 详情；如果新增事件类型，必须同步 `derive` / `boardFacts`、`board_entry` 和 `resume_run`。
- **验收**：flow test 断言统计数字；不访问网络、不调用真实 Agent。
- **风险**：新增事件类型会影响恢复和界面推导；先只在 `team` / `task_detail` 里计算，不加事件。

#### 5. 「停下但保留上下文」的显式中断

- **现状**：`reply` 已经能插话；不能插话时打断当前轮后发送。`cancel` 结束任务。没有「停止当前轮、保留会话、等主控下一步」的独立动作。
- **借鉴**：OpenAI 的 `interrupt_agent` 中断当前回合但不删除上下文；Claude Code 的 teammate 可以直接被 Esc 打断后继续对话。
- **建议**：给主控一个 `interrupt` 工具：停止当前轮、保留会话、任务回到可继续状态，随后用 `reply` 恢复。只在适配器支持打断时提供；不支持时明确返回错误，不假装成功。
- **落点**：`lead_tools`、ACP 的打断控制、[看板 MCP 工具](../interfaces/board_mcp.md)、[目标模式与共享看板](../domains/goal_mode.md#board_flow)。
- **验收**：反例：不支持打断的适配器上调用 `interrupt` 必须被拒绝且不改变任务状态。
- **风险**：打断可能丢失未完成的工具结果；需要定义清楚「打断后任务算什么状态」，并让 UI 显示。

#### 6. 安全审查编排模板与 SARIF 产物

- **现状**：默认 5 个角色组成产品上线团队；没有安全审查模板；产物是回复和 diff。
- **借鉴**：`hermes-security` 的分块、并行 reviewer、覆盖缺口、测试确认、SARIF 导出、逐次授权。
- **建议**：先做文档和编排模板，不改核心代码：按目录分块、每块一个 reviewer、单独一个入口和信任边界 reviewer、报告必须列出覆盖率和被否决的候选、区分「代码依据」和「测试确认」、补丁只读。若要把 SARIF 作为正式产物，让产物目录接受 `.sarif` 并在应用里显示，属于单独的小功能。
- **落点**：`docs/` 新增模式说明；角色和任务模板；产物读取路径。
- **验收**：用示例仓库跑一次只读审查，确认报告包含覆盖缺口；不把零发现写成「安全」。
- **风险**：安全审查会扩大外部依赖（osv-scanner 等）和网络访问；按仓库规则，扩大信任边界要单独提交、先写 ADR、更新安全文档。

#### 7. 紧急停止流程

- **现状**：用户 `cancel` 终止 Run，正在执行的尝试被取消。没有单独的「终止所有子进程、撤销看板令牌、阻止恢复」清单。
- **借鉴**：CrowdStrike 的 Agent Circuit Breaker：终止、阻止重启、撤销权限、统一入口。
- **建议**：把「停止」的语义写成明确流程：取消看板上所有未结束任务、终止所有 ACP 子进程、使看板令牌失效、Run 进入终态、列出未保存的产物。补一个测试确认取消后没有遗留子进程。不做按 Agent 身份的权限撤销，本地单用户收益低。
- **落点**：`action` 的 `cancel` 路径、ACP 子进程管理、[Run 生命周期](../domains/run_lifecycle.md#user_actions)、[运行手册](../operations/runbook.md)。
- **验收**：测试启动一个长任务后取消，断言子进程退出、令牌失效。
- **风险**：与现有 `cancel` 重复；先确认语义再动，避免变成第二个停止按钮。

### P2：观察，不急

- **竞争假设模式**：为调试类目标提供一个编排模板，要求 2-3 个协作者分别验证不同假设并互相反驳，结果里记录「排除了什么」。可以先用 prompt 和文档实现，不改 schema。
- **Agent Graph 审计视图**：把「目标 → 任务 → 尝试 → 工具调用 → 改动的文件 → 产物」做成一页可下钻的视图。现有的事件日志、`task_detail` 和进展时间线已经具备数据基础。
- **有界 peer messaging**：让协作者之间直接问问题，用 `max_hops` 限制循环。**现在不建议做**：Janus 的看板是唯一事实来源，直接消息会绕开事件、冲突检测和权限边界；如果将来要做，必须带 hop 上限、事件记录和「消息不能代替批准」的规则。
- **完成门禁 hook**：在任务标记完成前运行用户配置的检查。这会把「Janus 不运行验证命令」变成「Janus 运行命令」，属于信任边界变化，需要单独的 ADR 和安全文档，不能作为顺手功能。

### 明确不建议

- **默认并行**：单 Agent 在同等 token 预算下不输多 Agent；简单任务开团队只会增加协调成本。
- **无限 spawn / 嵌套团队**：Claude Code 自己不允许嵌套，OpenAI 虽然没有硬上限但默认并发只有 3。Janus 应保持「主控编排、协作者不再派生」。
- **自动执行修复**：PagerDuty 和 CrowdStrike 都把最终执行留给人或明确的授权范围；Janus 的审批和表单始终归用户，这条边界不要动。
- **按角色分配模型而不测量**：Coordination Tax 的结论是按任务路由比按角色分配更有效；在没有账本之前，不要引入自动模型分配。
- **把「零发现」当作结论**：没有覆盖率的零发现没有意义，安全审查必须报告覆盖缺口。

## 用事件日志衡量收益

Janus 的优势是所有状态变化都是事件，可以直接算协作是否值得。建议在下一期内先确定指标，再决定是否改代码：

- 每次交付的任务数、尝试次数 / 任务、验证未通过率。
- 墙钟时间；适配器上报后加 token 和费用。
- 人工干预次数：`answer`、`approve`、`input`、`message`、`takeover`、`ask_user` 的回答。
- 停滞事件数 / 任务、冲突事件数 / 任务、提问转交用户的比例。
- 交付后 7 天内被同一目录的新 Run 重做的任务比例。
- 对照组：同一目标由一个主控独立完成的成本。没有对照组，无法判断「多 Agent 更好」。

## 下周观察清单

- **产品**：Claude Code Agent Teams 是否转 GA、是否解除恢复限制；OpenAI Hosted multi-agent 是否 GA、是否加预算或深度限制；Microsoft Agent Framework 是否统一多语言接口；Hermes 是否把 security 插件并入核心；PagerDuty SRE Workbench 是否开放。
- **论文**：Teamknitt 是否给出实证结果；Coordination Tax 的结论是否被复现；同等 token 预算下的单 / 多 Agent 对比是否成为标准评测方式。
- **关键词**：agent teams、multi-agent orchestration、subagent、A2A、MCP、agent handoff、agent fleet、kill switch、shared memory、hypothesis testing。
- **要盯的账号**：Anthropic、OpenAI、Nous Research、TestingCatalog，以及 Simon Willison 一类持续做实测的开发者。X 的稳定抓取方式待解决。

## 来源

- [Claude Code：Orchestrate teams of Claude Code sessions](https://code.claude.com/docs/en/agent-teams)
- [Claude Code：Message your other Claude Code sessions](https://code.claude.com/docs/en/cross-session-messaging)
- [heise：Anthropic introduces Claude Opus 4.6 with Agent Teams](https://www.heise.de/en/news/Anthropic-introduces-Claude-Opus-4-6-with-Agent-Teams-11167248.html)
- [Microsoft Foundry：Use multi-agent orchestration with the Azure OpenAI Responses API](https://learn.microsoft.com/bg-bg/azure/foundry/openai/how-to/responses-multi-agent)
- [mixed-news：GPT-6.1 Sol can spawn its own subagents](https://mixed-news.com/en/openai-gpt-6-1-sol-multi-agent-beta-no-subagent-limit/)
- [agent-framework-go v0.3.0 发布摘要](https://www.onlylabs.fyi/signals/77041e11-2ac8-4e8d-b3e3-55ddf9519e10)
- [microsoft/agent-framework-go](https://github.com/microsoft/agent-framework-go)
- [Hermes Agent v0.23.0](https://github.com/NousResearch/hermes-agent/releases/tag/v0.23.0)
- [hermes-security 插件](https://hermes-agent.nousresearch.com/marketplace/hermes-security)
- [PagerDuty：Reliability in the AI Era](https://www.pagerduty.com/blog/announcements/reliability-in-the-ai-era-ai-that-works-the-incident-with-you/)
- [CrowdStrike：Introduces AI Kill Switch](https://www.crowdstrike.com/en-us/blog/crowdstrike-introduces-ai-kill-switch/)
- [ApodexAI/FrontierAgent README](https://raw.githubusercontent.com/ApodexAI/FrontierAgent/master/apodex/README.md)
- [NanoClaw：on-demand agent teams in Slack](https://nanoclaw.dev/pl/blog/nanoclaw-add-to-slack/)
- [ClawTeam 目录页](https://agent-orchestrators.com/tools/clawteam/)
- [Teamknitt（arXiv 2611.12545）](https://export.arxiv.org/abs/2611.12545)
- [Federation of Agents（arXiv 2509.20175）](https://ar5iv.labs.arxiv.org/html/2509.20175)
- [The Coordination Tax and the Heterogeneity Dividend（arXiv 2610.37700）](https://arxiv.org/html/2610.37700v1)
- [Single-Agent LLMs Outperform Multi-Agent Systems on Multi-Hop Reasoning Under Equal Thinking Token Budgets](https://www.semanticscholar.org/paper/Single-Agent-LLMs-Outperform-Multi-Agent-Systems-on-Tran-Kiela/a9bba78d6a7441ed43d05a0343518eeac3ac0c7e)
- [Anthropic：How we built our multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)
- [Agent Collaboration Agentic AI News：Week Ending 2026-11-10](https://aiagentstore.ai/ai-agent-news/topic/agent-collaboration)
