# 多 Agent 协作与 Agent Team 进展观察（2026-11-11，X 直接抓取）

> 覆盖窗口：2026-11-09 至 2026-11-11。
> 上一份报告[《2026-11-10 观察》](multi_agent_collaboration_2026_11_10.md)写于 x.com 抓取失败时，漏掉了 11 月 9-11 日的几条重要进展。本期用 OpenCLI 直接读取 X，补充事实并修正结论。

## 摘要

上一份说「本周没有出现推翻协作范式的新东西」，这个判断不准确。用 OpenCLI 直接扫 X 之后，11 月 9-11 日出现了三条实质性进展：

- **公开看板被大规模验证。** Anthropic 用 1 个 lead 加 999 个 helper，在 28.7 小时内复现了 Lean 形式的费马大定理证明。协调基座只有共享 Postgres 和 Git，没有消息工具、没有专门平台。
- **验证者成为标准角色。** Google Antigravity 发布 `/swarm` 预览：协调者、worker、verifier 和 monitor 分工，所有协作信息发在公开留言板上，worker 之间不私聊；空闲成员不耗 token。
- **扩展方式是复制小队，不是扩大单队。** 新论文在同等算力下比较 100 个独立 Agent、1 支 100 人队和 10 支 10 人队，结果小团队更稳、更省。

对 Janus 的结论：看板作为唯一事实来源的路线是对的，而且在大规模下被验证；真正缺的是**验证者角色**、**协作者请求拆分子任务**，以及**按小队复制**的扩展方式。上一份建议的成本账本仍然重要，优先级应提高。

## 方法与限制

- 使用 OpenCLI（`opencli twitter ...`），当前登录账号是 `@duoyuli_x`。搜索用 `--product live`，只读，不发帖、不点赞、不关注。
- 直接读取 X 的搜索结果，不经过 WebSearch 索引；官方账号 `@antigravity`、`@AnthropicAI` 的帖子用于交叉验证。
- 4 次搜索后 OpenCLI 返回 HTTP 429（Twitter 会话配额），冷却通常 15-30 分钟。后续每天应控制在 3-4 次查询内，并留间隔。
- 论文和厂商文档用 WebFetch 核对；无法确认的内容单独标注「未确认」。

## 本期新增

### Google Antigravity：`/swarm` 预览

2026-11-10，Google 在 Antigravity 的桌面应用和 CLI 中预览 `/swarm`（[官方文档](https://antigravity.google/docs/swarm/)、[官方推文](https://x.com/antigravity/status/2120279327589773639)），同时把 Gemini 4 Carbon 开放给付费开发者和 AI Ultra 用户（[博客](https://antigravity.google/blog/gemini-4-carbon-in-google-antigravity)）。

角色与流程：

- **协调者**：当前对话本身。它给 swarm 起名，按文件、包或问题把任务切成互不重叠的部分，创建 `@parser`、`@fix-1` 到 `@fix-4` 这样的 worker 和编号组。协调者自己不写代码，只分派、判断和汇报。
- **worker**：每个从干净的上下文开始，拿到一份公开简报，写明「你拥有什么、不能碰什么、什么算完成」。
- **verifier**：通常每 2-4 个 worker 配一个。它读 diff、跑测试，把问题发回写代码的 worker，由后者修。
- **monitor**：一个 `@monitor`，平时休眠，只在全员沉默但活没干完时发帖提醒。

协作方式：

- 所有任务分配、跨 worker 提问、diff 摘要和测试结果都发在**公开留言板**上，用 `@name`、`@group` 或 `@all` 提及；worker 之间不私聊，团队对话保持在一处、按顺序。
- 给单个 worker 的直接消息不上面板：应用里打开该 worker 的对话，CLI 里用 `@verifier ...` 前缀。
- 默认共享工作区；当 worker 同时编辑或构建时，协调者可以给每个 worker 一份独立的工作副本。文档特别说明这**不是字面意义的 git worktree**；移除 worker 前会先让它提交。
- 空闲的 worker 或等待汇报的协调者在看板上休眠，不耗 token，被提及才醒。整个 swarm 的配额消耗远高于单会话；配额耗尽的成员会暂停，不自动重试，配额刷新后也不会自动恢复；被终止的 worker 永久消失，只能新建。
- worker 默认继承当前模型；可以按角色指定不同模型，协调者也能把等待中的 worker 换到另一个模型并保留它的对话。
- 官方建议：用在不同文件、包或问题上能干净切分的任务；verifier 换一个模型做跨模型审计；把工作描述和验收方式讲清楚，让协调者自己决定怎么切。

与 Janus 的关系：这是对「公开看板、协作者私聊」最直接的产品验证。差异是 Antigravity 把**验证者**和**monitor**做成独立角色，而 Janus 目前由主控核对结果、由停滞检测发现卡住。Antigravity 也不追求持久恢复：被杀的 worker 不会回来；Janus 的 `resume_run` 更强。

### Anthropic：1,000 个 Agent 复现费马大定理

2026-11-09，Anthropic 在官方博客介绍了这次实验（[Safely scaling multi-agent teams with Claude Fable 5.2](https://claude.dev/blog/safely-scaling-multi-agent-teams-with-fable-5-2/)）。

规模与结果：

- 1 个 lead 加 999 个 helper，使用早期版本的 Claude Fable 5.2，28.7 小时完成。60 个 statement 中 54 个完成，5 个后来不再需要，1 个放弃。
- 输入只有 60 个 statement、一句经典证明路线的说明、定义，以及 47 个此前已证明的引理；没有研究员逐步指导。
- 输出 token 约为 9 月那次的三分之一，证明长度约为七分之一。9 月那次是数十个 Agent、11 天、约 60 亿 token、1,300 万行。

协调基座：

- 每个 Agent 有自己的 shell、文件编辑器、Lean 工具链和 Mathlib、自己的 Git 分支，以及一个共享 Postgres。
- lead 在没有给定 schema 的情况下，前 10 分钟自己建了 5 张表：`Announce`（全队公告）、`Assign`（谁负责哪个 statement）、`Notes`（公开留言板：认领、判断难度、进展）、`Subtasks`（共享待办，先到先得）、`Landed`（完成记录：片段、commit、通过与否）。
- 初始把 60 个 statement 分给 16-17 人的 crew。27 分钟时，helper 判定 47 个 statement 在预算内不可行，lead 把人力集中到剩下 10 个；70 分钟完成，随后又把人力调回之前放弃的部分。
- 3 小时后，lead 说「不要等我」，允许自由认领；5.6 小时时有 73 个 helper 写了 756 个任务。
- 最难的 Langlands-Tunnell 通过递归拆分长成 2,418 个任务、最多 10 层深的树；子任务可以假设自己的孩子已经成立，因此数百个 helper 能并行推进。它在 28.4 小时完成，最终定理在 28.7 小时完成。
- 到第 30 小时，数据库累计约 36,612 次写入和 311,734 次读取；第 18 小时曾从存档重启并追加 12 小时预算。

安全设计：

- 每个 Agent 一个轻量 VM。单机在约 100 个 Agent 时就因内存、磁盘和 CPU 耗尽而崩溃。
- 没有网络，只留一个到网关的管道，网关在沙箱外给每个请求打上 Agent 身份，防止冒充。
- 数据库用无特权角色、5 秒语句超时、单次写入 256 MiB 上限；内核 lockdown，去掉可加载模块、kexec、原始内存访问和内核探针。
- 每个工具调用（shell 命令、文件编辑）都经过一个 fail-closed 的 LLM 分类器。触发阻断时整个团队停下等人工复核，防止另一个 Agent 完成被阻断的动作。超过 100 万次工具调用，没有一次达到阻断阈值。另有离线监控。

需要保留的怀疑：

- 作者自己称这是「玩具实验」，并且是复现已有形式化，不是新的数学结果。
- token 和证明长度只有相对值，没有绝对计数和独立评测；任务树的父子关系主要是事后根据标签重建的。
- CoT 监控在这次运行中并未启用，「本可以抓到」是事后回放的结果。
- 覆盖时 Lean 代码还没有公开仓库。

与 Janus 的关系：共享 Postgres 就是看板，Git 就是产物，这与 Janus「事件日志是唯一事实来源、协作者只经看板协作」的设计高度一致。差异有三点：Janus 的 DAG 是扁平的、只有主控能编排；它允许 helper 自己拆任务；它用 per-Agent VM 做隔离，Janus 用本机进程和 Agent 原生权限。规模和威胁模型不同，不能直接照搬隔离方案。

### 论文：并行小队优于大团队

[Test-Time Compute Scales Better with Parallel Agent Teams](https://arxiv.org/abs/2611.09798)（arXiv 2611.09798）：

- 在验证器能打分的前提下，比较三种花算力的方式：100 个独立 Agent 取最好（Best-of-N）、1 支 100 人协作队、10 支 10 人小队各跑一份再取最好。
- 任务：12 个长任务，包括 GPU kernel、形式证明和漏洞利用；模型 27B；预算 100 个 Agent。
- 结果：10 人小队在 10/12 个任务上优于独立 Agent；在 4 个跑了大团队的任务上全部胜出；同等美元成本下，一半任务超过前沿模型。
- 解释：独立 Agent 的均值低，Best-of-N 很快饱和；一支大团队只有一次采样，吃不到方差；多支小队同时有较高均值和方差。
- 结论：扩展测试时算力时，应**复制小队**，而不是扩大单队。

与 Janus 的关系：如果要在 Run 层面扩展，应该并行启动多个独立 Run 再选最好的，而不是往一个 Run 里加更多协作者。Janus 的 Run 已经是天然的隔离单位，但工作目录是共享的，直接做 best-of-N 需要先解决目录隔离。

### Claude Code 动态工作流

本周 X 上重新讨论了这个功能（功能本身在 2026 年 5 月已发布，见 [Claude Code 文档](https://code.claude.com/docs/en/workflows)）。

- 计划和循环从对话移到 JavaScript 脚本，中间结果放在脚本变量里，主控的上下文只保留最终结果。脚本可读、可重跑、可在同一会话恢复。
- 官方把四种编排放在一起对比：subagent 由 Claude 逐轮决定；skill 按提示执行；agent team 由 lead 逐轮决定；workflow 由脚本决定。workflow 的规模是「几十到几百个 Agent 一次运行」。
- 限制：默认 16 并发（可通过环境变量设到 1-256）；单次 `parallel()` 或 `pipeline()` 最多 4,096 项；单次 run 最多 1,000 个 Agent；脚本本身不能读写文件或执行 shell；运行中不能接受用户输入；失败会重跑失败的 Agent 及其之后的所有 Agent；碰到 usage limit 会暂停等待。
- 成本可见：`/workflows` 显示每个 Agent 的 token 用量；超过 25 个 Agent 或预计 150 万 token 会出现 Large workflow 提示。
- 规模建议：small 少于 5 个 Agent，medium 少于 10 个，large 少于 50 个。

与 Janus 的关系：Janus 更接近 agent team，计划在 lead 的上下文里逐轮生成。脚本模式的价值是确定性和可重跑，可以借鉴成「可保存的编排模板」，但不必引入 JavaScript 执行面。

### 其他信号

- **Cursor Projects multiplayer（未确认）**：有帖子说 Cursor 的 Projects 让多人共享一个工作区和上下文层，Agent 之间可以互相发消息；开放问题从「Agent 怎么协调」变成「谁的意图优先」。如果属实，它和 Janus 的单用户模型差异较大，暂不跟进。
- **配额成为一等约束**：Antigravity 的 worker 在配额耗尽时暂停且不自动重启，Grok Bot 的讨论也把用量限制当作 agent team 的规格。多 Agent 的成本控制正在从「建议」变成产品行为。
- **Harness 变动**：一条 HN 讨论提到 Claude Code 一年 244 次发布，超过四分之三改了系统提示或工具描述。这与 Janus 的适配器稳定性有关，但不是协作模型本身的变化。

## 对 Janus 的建议（修正版）

### 新增 P0：验证者模式

- **现状**：协作者自己按验收标准验证，主控核对结果；Janus 不运行验证命令。这是有意的边界，但「谁核对」没有结构化的角色。
- **借鉴**：Antigravity 每个 verifier 负责 2-4 个 worker，读 diff、跑测试、把问题发回执行者；Anthropic 的最终结果由 Lean 内核和独立 grader 检查。
- **建议**：先做编排模板和文档，不改状态机。对可自动检查的任务，主控在 `orchestrate` 里额外提交一个验证任务，`deps` 指向被验证的任务，执行者指定另一个成员，验收标准要求它运行测试并给出证据。验证任务和普通任务一样在看板上流转，结果里引用原任务 id。
- **落点**：[目标模式与共享看板](../domains/goal_mode.md#lead_brief)、[看板 MCP 工具](../interfaces/board_mcp.md)、角色默认配置；不需要新事件类型。
- **验收**：一个 flow test：执行者和验证者不是同一人，验证任务失败时原任务不会被主控直接接受。
- **风险**：增加 token 和时延；验证者可能只是复述执行者的结论，所以验收标准必须要求可复现的命令和输出。

### 新增 P1：协作者请求拆分子任务

- **现状**：只有主控能 `orchestrate`，任务 DAG 是扁平的；协作者遇到过大的任务只能提问或硬做。
- **借鉴**：1,000-Agent 实验允许 helper 把过大的任务拆成子任务，树长到 10 层；Janus 的优势是编排器独占状态写入，不能放弃。
- **建议**：增加一个 `request_split` 看板工具。协作者提交子任务建议和理由，主控看到后用 `orchestrate` 批准并建立依赖。这样保留「只有编排器改状态」的不变量，同时获得递归拆分的好处。
- **落点**：`member_tools`、`board_post` 的替代路径、看板事件、[目标模式与共享看板](../domains/goal_mode.md#board_flow)。
- **验收**：反例：协作者直接 `orchestrate` 仍然被拒绝；`request_split` 不改变任务状态，只有主控批准后才生效。
- **风险**：主控需要及时处理拆分请求，否则协作者会停等；可以加一条看板推送规则。

### 新增 P2：可保存的编排模板

- **现状**：每次 Run 的编排由主控临场决定，事件日志记录了结果，但不能直接复用。
- **借鉴**：Claude Code 把一次成功的 workflow 保存成命令；Antigravity 的 `/swarm` 也把一次团队结构留在会话里。
- **建议**：允许把一次 `orchestration.accepted` 的任务图保存为模板，下次创建 Run 时作为建议提供给主控。模板是数据（任务、依赖、验收标准、角色类型），不是可执行脚本，不引入新的执行面。
- **落点**：数据库或 `~/.janus` 下的模板目录、创建 Run 的界面、主控说明。
- **风险**：模板可能诱导主控跳过对当前目标的分析；说明里要写「按目标调整，不要照抄」。

### 暂不做

- **Run 级并行 best-of-N**：方向正确，但需要工作目录隔离和评审机制，成本高；先观察是否有更便宜的做法。
- **协作者自领任务**：1,000-Agent 实验在后期允许自领，但 Janus 的调度器需要确定性、每个成员同一时间只做一个任务、冲突检测依赖明确归属；自领会削弱这些不变量。
- **per-Agent VM 和工具调用分类器**：Janus 面向本地单用户、Agent 原生权限，威胁模型不同；引入会扩大信任边界，必须先写 ADR。
- **按角色分配模型**：Antigravity 和 Anthropic 都这么做，但 Coordination Tax 的结论是按任务路由更有效；在没有成本账本前不要自动化。

### 对上一份建议的调整

- 「成本账本」从 P1 提高优先级：Antigravity 的配额暂停和 Anthropic 的 1,000-Agent 运行都说明，没有成本可见性就无法安全扩大规模。
- 「验收证据加 `method` 字段」仍然成立，而且和验证者模式互补：verifier 产生可复现的证据，`method` 让主控一眼看出哪些是推理、哪些是测试。
- 「主控说明补规模和粒度」仍然成立，但可以加上验证者和小队复制的规则。

## 操作记录

```text
opencli twitter whoami -f json
opencli twitter search '"agent teams" since:2026-11-03 -filter:replies' --product live --limit 20 --window background -f json
opencli twitter search '"multi-agent" since:2026-11-09 -filter:replies' --product live --limit 25 --window background -f json
opencli twitter search 'Antigravity swarm since:2026-11-08 -filter:replies' --product live --limit 20 --window background -f json
opencli twitter search 'from:AnthropicAI since:2026-11-08 -filter:replies' --product live --limit 15 --window background -f json
opencli twitter search 'from:antigravity since:2026-11-08 -filter:replies' --product live --limit 15 --window background -f json
```

第 6 次查询返回 HTTP 429，冷却 15-30 分钟。下次每天控制在 3-4 次查询，优先搜 `since:<前一天>`，先搜 `agent teams`、`agent swarm`、`multi-agent` 三个词，再用官方账号的 `from:` 查询补充。

## 来源

- [Google Antigravity：Swarm (/swarm) 文档](https://antigravity.google/docs/swarm/)
- [Google Antigravity：Gemini 4 Carbon and /swarm](https://antigravity.google/blog/gemini-4-carbon-in-google-antigravity)
- [@antigravity：/swarm 发布推文](https://x.com/antigravity/status/2120279327589773639)
- [Google：Gemini 4 Carbon is now available](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-4-carbon-now-available/)
- [Anthropic：Safely scaling multi-agent teams with Claude Fable 5.2](https://claude.dev/blog/safely-scaling-multi-agent-teams-with-fable-5-2/)
- [Test-Time Compute Scales Better with Parallel Agent Teams（arXiv 2611.09798）](https://arxiv.org/abs/2611.09798)
- [Claude Code：Orchestrate subagents at scale with dynamic workflows](https://code.claude.com/docs/en/workflows)
- [Claude Code：Agent Teams](https://code.claude.com/docs/en/agent-teams)
- [The Coordination Tax and the Heterogeneity Dividend（arXiv 2610.37700）](https://arxiv.org/html/2610.37700v1)
- [@AnthropicAI：Fable 5.2 与 Mythos 5.2 发布推文](https://x.com/i/status/2119867707546071125)
