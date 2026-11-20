# Multi-Agent 协作审查与优化方案（agy）

## 导航

- [1. 审查抬头](#header)：查看审查日期、基线提交、信息来源与范围限制时阅读。
- [2. 结论与基线评估](#summary)：快速了解 Janus 现有协作底座的优势、核心瓶颈总表时阅读。
- [3. 业内实践与实证研究对照](#industry_practices)：对照 2025 至 2026 年主流多 Agent 编码系统与论文结论时阅读。
- [4. 逐项协作瓶颈与优化思路（MA-01 至 MA-07）](#findings)：评估具体模块改动、代码证据、设计方案与风险边界时阅读。
- [5. 建议的演进路线图](#roadmap)：按改动规模与预期收益排定实施优先级时阅读。
- [6. 附：文档与代码一致性核对（DOC-01 至 DOC-02）](#doc_drift)：清理顺带发现的现状文档残留时阅读。

<a id="header"></a>
## 1. 审查抬头

审查日期：2026-11-18。

审查标识：Antigravity（`agy`）。

审查对象：commit `57a35ddc62223b55bd74bf33a1e7d896f78464a9`（`main`，2026-11-17 22:07:42 -0600，「Guide macOS permission recovery after app updates」）的代码快照，重点覆盖 `backend/src/orchestrator/`（`lifecycle.rs`、`lead.rs`、`board.rs`、`board_tools.rs`、`attempts.rs`、`turn.rs`、`session.rs`、`prompts.rs`、`messages.rs`）、`backend/src/teamwork.rs`、`backend/src/traces.rs`、`backend/src/agents/workspaces.rs` 以及 `docs/` 中的架构、领域、接口、ADR 与既有研究报告。工作树中已有的未提交文件不在本文审查范围内，本文也不改动它们。

审查方式：先按 Project Doc 路由完整阅读 `docs/index.md` 及架构、领域、接口、决策、研究、审查索引与相关正文，再逐段核对「工作区快照 → 负责人启动 → 看板编排校验 → 任务调度 → ACP 会话生命周期 → 上下游交接 → 共享目录痕迹与冲突检测 → 验收与独立验证 → 停滞检测与用量账本」完整代码链路；同时检索并交叉比对 2025 至 2026 年外部多 Agent 编码产品与实证研究。

范围与限制：本文是一份架构审查与协作优化分析报告，不修改业务代码，未启动真实付费 Agent 会话做端到端基准测试。下文列出的 `MA-01` 至 `MA-07` 属于**协作吞吐、上下文损耗与验证闭环的架构演进项**，不是安全漏洞或阻断性正确性缺陷；涉及调度策略、会话复用或隔离模型的项在实施前仍应按仓库规则单独编写 ADR。外部论文与产品机制按公开文档与综述归纳，对 Janus 预期收益的判断属于工程推演，落地后需结合事件日志与 `/api/stats/roles` 实测验证。

<a id="summary"></a>
## 2. 结论与基线评估

### 2.1 Janus 已经具备的底座优势

对照业内主流多 Agent 方案，Janus 当前的协作基座已经在多个关键维度上做出清晰且自洽的取舍，优化时应当保留这些核心不变量，而不是推倒重来：

- **单一事实源与公开看板**：所有编排、进展、阻塞、提问、裁决和交付先写入 SQLite 事件日志再广播，协作者之间不建立绕开审计的私聊通道，中断后可由 `resume_run` 重建状态（见 [ADR：事件是唯一事实来源](../decisions/20261031-events_as_source_of_truth.md) 与 [ADR：通过 MCP 看板协作](../decisions/20261031-board_over_mcp.md)）。
- **看板主动推送（`push_board`）**：协作者提问、失败、阻塞、停滞、文件冲突和痕迹异常在 2 秒合并窗口后经 ACP `_session/steering` 注入负责人正在执行的轮次（`backend/src/orchestrator/lead.rs:149-188`），避免负责人自己动手时阻塞团队。
- **共享目录的「承诺 + 环境痕迹」双轨对齐**：事前用 `owns` 互斥校验阻止并行任务声明重叠路径，事中支持协作者 `claim` 动态认领并用 `record_edits` 实时报告冲突与越界，事后用 `traces::gaps` 对账并按任务负责范围裁剪 diff 产物（`backend/src/teamwork.rs:52-168`、`backend/src/traces.rs:81-151`）。
- **按验收标准验证与独立验证原语**：结构化结果支持 `checks[].method` 证据分级与原会话 1 次自动补救（`PromptChannel::Fix`），`verifies` 强制异人审查且禁止负责人用 `accept` 直接放行失败的独立验证（`backend/src/teamwork.rs:193-199`）。
- **清晰的权限与配置隔离边界**：权限完全交由各 Agent 原生模式决定，Skill 与 MCP 在会话启动时注入隔离运行环境，不触碰用户本机 Agent 配置（见 [ADR：权限交给 Agent 原生模式](../decisions/20261031-agent_native_permissions.md) 与 [ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](../decisions/20261116-launch_time_resources.md)）。

### 2.2 审查发现的协作瓶颈总览

当前多 Agent 协作的主要摩擦集中在**同质角色无法真正并行、返工冷启动丢失上下文与缓存、负责人派工与等待信息不全、上下游交接信息截断过重、共享目录追踪留有盲区、长任务缺少动态拆分提议、交替死循环与成本暴走缺少主动护栏**七个接缝处：

| 编号 | 协作环节 | 核心症状与代码位置 | 优化方向摘要 | 改动规模 | 建议阶段 |
| --- | --- | --- | --- | --- | --- |
| MA-01 | 团队拓扑与调度并发 | 单角色映射单 `Member` 且每成员限 1 个在跑任务（`workspaces.rs:100-108`、`lifecycle.rs:186-195`），默认工作区只有 1 名研发时同质子任务被迫 100% 串行；多角色同时就绪又无全局并发上限 | 支持工作区角色并发槽位（`slots`）展开同质分片实例 + 增加 Run 级全局并发上限（如默认 3） | 中（需 ADR） | Phase 2 |
| MA-02 | 会话生命周期与返工闭环 | 每次尝试结束即杀掉 ACP 会话（`attempts.rs:400`），已完成任务拒绝 `reply`（`board.rs:270-275`）；独立验证打回或 `retry` 时必须冷启动重读代码，丢上下文且击穿 Prompt Cache | 复用已有的 `session/load` 能力，在 `orchestrate` 支持 `continues`、在 `retry` 支持原会话热恢复，打通「实现 ↔ 验证 ↔ 修复」热闭环 | 中（需 ADR） | Phase 2 |
| MA-03 | 负责人感知与等待效率 | `team_lines_vec` 不展示协作者挂载的 Skill / MCP（`lead.rs:748-765`），负责人盲派工；`wait` 上限仅 45 秒且 500 毫秒轮询 SQLite（`board.rs:208-227`），超时无进展脉搏，长任务导致负责人反复空转消耗上下文 | `team_lines` 暴露成员生效资源摘要；`wait` 改为 `Notify` 唤醒、放宽超时上限并在超时返回运行中任务的实时脉搏 | 小至中 | Phase 1 / 2 |
| MA-04 | 上下游上下文交接 | 下游 `task_prompt` 只拿到截断到 300 字符的上游 `summary`，未注入上游 `risks` 与接口契约细节（`attempts.rs:112-129`），易引发交接失真与并行契约漂移 | 结构化结果新增面向下游的 `handoff` 字段（与一句话 `summary` 分离）并透传非空 `risks`；在负责人指南中强化「契约先行」编排模式 | 小 | Phase 1 |
| MA-05 | 共享目录痕迹与独立验证 | 负责人自身的编辑不进 `Traces`（`turn.rs:557-559`）；验证者提示词未直通专属 diff 路径、`traceGaps` 和弱证据条目（`teamwork.rs:180-188`）；重编译任务并行共享目录易争抢构建锁 | 将 `lead` 编辑纳入 `Traces`；给 `verifies` 提示直通 diff 产物、`traceGaps` 与弱证据项并鼓励跨模型指派；中长期评估按需 APFS 克隆隔离 | 小（隔离为大） | Phase 1（隔离为 Phase 3） |
| MA-06 | 协作者过程协调 | 协作者遇到任务过大或边界不合理时，只有不等待的 `board_post(blocker)` 或结束整轮的 `question`（且限 4 轮，`attempts.rs:310-313`），缺少结构化的子任务拆分提议 | 新增协作者只读提议工具 `request_split`（或结构化范围调整请求），推送给负责人由其通过 `orchestrate` 裁决，保持编排器独占状态写入 | 中 | Phase 3 |
| MA-07 | 停滞检测与成本闸门 | `watch_call` 只抓连续 5 次完全相同调用（`lifecycle.rs:344-359`），漏掉「改代码 ↔ 跑测试失败」交替死循环；用量账本仅有只读提醒，无 Run 级 token / 费用暂停闸门 | `watch_call` 升级滑动窗口短周期检测；在墙钟预算旁增加可选 Run 级 token / 估算费用上限，触发 `WaitingInput` 等用户追加或停止 | 小至中 | Phase 1 / 3 |

<a id="industry_practices"></a>
## 3. 业内实践与实证研究对照

结合仓库 `docs/research/` 已有的产品调研（Claude Code Agent Teams、OpenAI Responses Hosted Multi-Agent、Google Antigravity `/swarm`、GitHub Agent HQ、Microsoft Agent Framework、Grok Bot）以及 2025 至 2026 年的工程与学术观察，以下四条规律与 Janus 的下一步演进直接相关：

### 3.1 多 Agent 失败主要发生在「接缝」而非单点推理

UC Berkeley 提出的 **MAST**（Multi-Agent System Failure Taxonomy）对主流多 Agent 框架失败轨迹的归因显示，复杂任务的高失败率集中于三类结构性接缝：

1. **系统设计与终止/循环问题（约 44%）**：包括局部步骤重复打转（约 15.7%）、偏离角色或目录边界（约 13.3%）、终止条件认知不清（约 12.4%）。
2. **Agent 间失调与交接损耗（约 32%）**：上游只传模糊摘要导致下游曲解意图（Context Bleed / Loss in Translation），或并行编码时各自修改心智中的接口契约导致集成崩溃（Contract Drift）。
3. **验证走过场（约 23.5%）**：审查 Agent 出现「讨好型验证（Sycophancy）」，仅凭代码表面合理性放行而未针对薄弱环节做独立实测（约 17.3%）。

**对 Janus 的含义**：Janus 已经用 `owns`、`traces` 和 `verifies` 管住了大量边界问题；下一步把**交接载荷结构化（MA-04）**、**向验证者精准喂入薄弱点与跨模型交叉审查（MA-05）**、**捕获交替循环**（MA-07）补齐，正好命中失败率最高的三处接缝。

### 3.2 热会话复用（Session Reuse）与 Prompt Cache 是多轮协作的胜负手

多 Agent 系统的 token 消耗通常是单 Agent 的 3 至 15 倍。业内最新架构普遍将「进程与会话复用」作为核心基础设施：

- **OpenAI Hosted Multi-Agent** 将 `spawn_agent`（新建干净上下文）与 `followup_task`（在已有子 Agent 会话上追加任务）拆成两个一等动作。
- **Google Antigravity `/swarm`** 让完成当前回合的 worker 进入 `waiting` 休眠（不耗 token），当 `@verifier` 发现缺陷或协调者补充要求时，直接唤醒原 worker 会话继续修改。
- **推理缓存层面**：Anthropic 与 OpenAI 的前缀 KV Cache 依赖稳定的会话历史前缀。如果「实现 → 验证 → 修复」的返工每次都销毁进程重开 `session/new`，执行者不仅要耗费数十秒重启 ACP 适配器与 MCP 子进程，还要重新读取整批源码，彻底放弃前缀缓存命中率。

**对 Janus 的含义**：Janus 无需引入常驻守护进程池，只要利用 ACP 已有的 `session/load` 机制把「同执行者的后续修复 / 重试」接回原会话（MA-02），就能以很小的架构改动拿到热会话复用的延迟与缓存红利。

### 3.3 「同质角色分片扇出 + 小团队」优于「冗长异质流水线」

- *The Coordination Tax*（arXiv 2610.37700，135 个代码任务、2,430 次运行）表明，把普通任务硬拆成固定多角色串行流水线会使成本增加约 3.22 倍、耗时增加约 3.24 倍，而准确率无显著提升。
- *Test-Time Compute Scales Better with Parallel Agent Teams*（arXiv 2611.09798）与 Antigravity `/swarm` 的实践均指向同一模式：**只有在模块边界可干净切分时才做并行扇出**，此时最常见的是**同一「研发」或「审查」角色同时领走 2 至 4 个互不重叠的模块切片**，再配 1 名异构模型的独立验证者。

**对 Janus 的含义**：当前「1 个角色 = 1 个串行成员」的绑定限制了最核心的同质模块并行场景，解耦「角色定义」与「工作区并发槽位数」（MA-01）是释放并行吞吐的关键。

### 3.4 协调信息应携带「结构化进展脉搏」

Microsoft Agent Framework 的 **Magentic 编排器**通过维护结构化的 `Progress Ledger`（当前目标是否推进、是否在原地打转、阻塞在哪一步）来决定继续等待还是重新规划，而不是让协调者在黑盒超时中盲等。

**对 Janus 的含义**：负责人调用 `janus.wait` 等待期间，如果超时返回能附带在跑子任务的轻量实时脉搏（MA-03），既能减少无效轮询，又能让负责人在 10 分钟硬停滞阈值触发前做出更合理的判断。

<a id="findings"></a>
## 4. 逐项协作瓶颈与优化思路

<a id="ma_01"></a>
### MA-01：解耦「角色」与「并发槽位」，补齐全局并发上限

- **问题现状**：
  1. 在 `backend/src/agents/workspaces.rs:100-108` 中，`team_for` 将工作区启用的每个角色 1:1 展开为单个 `Member`（`id` 直接取角色 `id`）；而在 `backend/src/orchestrator/lifecycle.rs:186-195` 中，调度器用 `busy: HashSet<String>` 限制每个 `t.agent` 同一时间只能运行 1 个子任务。在默认 5 角色团队（PMO、市场运营、产品经理、研发工程师、质量工程师）或常见的「1 负责人 + 1 研发 + 1 审查」工作区中，负责人即使把代码干净切分成 3 个互不重叠的 `owns` 模块交给研发工程师，**这 3 个子任务也只能完全串行执行**。用户若想并行，只能在设置页手工复制多个一模一样的研发角色。
  2. 反过来，当工作区配置了较多角色且多个无依赖任务同时就绪时，`lifecycle.rs:187-260` 会在单次循环内对全部就绪任务同时调用 `tokio::spawn` 启动尝试，瞬时并发拉起多个 Node 适配器、Agent CLI 和 stdio MCP 子进程，放大启动延迟、本机负载与 API 限流风险（参见 [2026-11-12 观察](../research/20261112-multi_agent_collaboration.md)）。
- **优化思路**：
  1. **工作区成员支持并发槽位（`slots`）**：允许工作区成员条目从 `{ "role": "implementer" }` 扩展为 `{ "role": "implementer", "slots": 3 }`（缺省为 1，保持向后自然语义）。当 `slots > 1` 时，`team_for` 自动展开为 `implementer-1`、`implementer-2`、`implementer-3` 三个 `Member` 快照，共用同一个 `role_id: "implementer"`。由于会话键早已重构为按任务区分的 `SessionKey::Task(task_id)`，且 `backend/src/orchestrator/board.rs:439-445` 的 `role_stats` 已经按 `role_id` 归集统计，展开分片不会破坏会话控制或角色统计。
  2. **增加 Run 级全局并发上限（`max_concurrent_attempts`）**：在 `Config` 中提供可配置上限（建议默认 3 或 4），`lifecycle.rs` 调度时统计当前 `TaskStatus::Running` 的任务总数，达到上限时其余就绪任务保持 `pending`（且 `runnable` 判断仍将其视为未完成工作，防止提前交付）。
- **边界与风险**：修改调度规则前需先写 ADR 并同步更新 [Run 生命周期](../domains/20261031-run_lifecycle.md#scheduling) 与 [配置](../interfaces/20261031-configuration.md)；每个分片成员的 Skill 资源包可以按 `role_id` 共用只读目录，避免重复复制文件。

<a id="ma_02"></a>
### MA-02：基于 `session/load` 支持「实现 ↔ 验证 ↔ 修复」原会话热恢复

- **问题现状**：
  1. `backend/src/orchestrator/attempts.rs:400` 在每次尝试拿到结果后立即执行 `close_session` 终止 ACP 子进程；`backend/src/orchestrator/board.rs:270-275` 对已结束的子任务调用 `janus.reply` 时直接报错，要求负责人改用 `retry` 或新建子任务。
  2. 当独立验证任务 `V1` 发现 `T1` 存在缺陷而未通过时，`backend/src/teamwork.rs:195-198` 引导负责人通过 `orchestrate` 新建修复子任务 `T2`（交给 `T1` 的原执行者）和新的验证任务 `V2`。此时 `T2` 是一个全新的 ACP 进程和空白会话，原执行者在 `T1` 中读过的数十个文件、推理出的架构上下文以及云端模型的 KV Prompt Cache 全部作废，只能从头再读一遍代码。`janus.retry` 重跑失败任务时同样丢失上一轮会话记忆。
- **优化思路**：
  1. Janus 在中断恢复（`attempts.rs:178-190`）与交付后负责人对话（`lead.rs:423-433`）中**已经验证并实现了 `conn.capabilities["loadSession"]` + `session/load` + `drain_replay`**。
  2. 在 `orchestrate` 的任务定义中增加可选字段 `continues: "<上游已完成或失败的子任务 id>"`（校验要求 `continues` 指向的任务与当前任务 `assignee` 相同，且自动将该任务加入依赖），并在 `janus.retry` 中增加可选布尔参数 `continue_session`（默认 `false` 保持干净重跑，设为 `true` 时复用上次会话）。
  3. 会话打开时：若目标前置任务的最后一次尝试记录了 `sessionId` 且适配器声明 `loadSession == true`，则通过 `session/load` 恢复原会话，仅将本次增量修复说明、验收差距或 `V1` 的独立验证报告作为新一轮 `session/prompt` 发入；若 Agent 不支持 `loadSession` 或加载失败，则自动回退到现有的 `session/new` 完整提示词，并把实际走的路径记入 `attempt.started` 事件。
- **边界与风险**：长时间多轮复用可能接近模型上下文窗口上限；可结合 `usage.rs` 已记录的上下文快照比例（如上下文占用已超过 75% 时自动回退为新建干净会话并注入摘要），既拿满缓存收益又防止爆窗。需先写 ADR。

<a id="ma_03"></a>
### MA-03：向负责人暴露协作者装备（Skill / MCP），消除 `wait` 盲等与高频空转

- **问题现状**：
  1. **协作者能力信息缺失**：`backend/src/orchestrator/lead.rs:748-765` 的 `team_lines_vec` 只输出协作者的 `id`、名字、角色名、`desc`、Agent 类型、模型标签和权限模式，**没有输出 `m.resources`（该角色绑定的 Skill 和 MCP 服务器）**。除非用户把所有 Skill 和 MCP 名字手抄进角色职责说明里，否则负责人并不知道哪位数字员工挂载了浏览器自动化、Figma、数据库或专项审查资源。
  2. **`wait` 短超时与轮询开销**：`backend/src/orchestrator/board_tools.rs:55-59` 将 `janus.wait` 的 `timeout_sec` 限制在 1 至 45 秒（默认 40 秒），`backend/src/orchestrator/board.rs:208-227` 在循环中每 500 毫秒读一次 SQLite，超时后仅返回「40 秒内看板上没有新消息，子任务仍在进行。可以再次调用 janus.wait。」。当协作者执行一个 12 分钟的实现与测试任务时，负责人会被唤醒约 18 次做无意义的工具调用，每次都触发一次完整上下文推理并在负责人会话里堆满空轮询记录。
- **优化思路**：
  1. **在 `team_lines_vec` 中展示成员资源概览**：按类型拼入协作者已挂载的资源名称（例如 `· 资源：Skill [project-doc, ui-audit]；MCP [playwright]`，仅输出名称或简短类型，绝不读取或输出 `secrets/library/` 中的凭据值，且提示实际可用性以运行时支持矩阵为准）。
  2. **`board_wait_inner` 改为事件唤醒 + 实时脉搏摘要**：用 `Notify`（在 `emit` 写入看板相关事件或任务全部进入终态时唤醒）替代 500 毫秒定时查库；在 MCP 客户端超时允许的范围内将 `timeout_sec` 上限提高到 120 至 180 秒；当超时仍无新看板事件时，附带每个 `Running` 子任务的一行实时状态脉搏（从内存 `Run` 与最新事件读取：当前 `AgentPhase`、最近一次工具调用标题、已运行时长、已改文件数），让负责人清楚看到协作者是在正常跑测试还是疑似走偏。
- **边界与风险**：改动集中在只读信息拼装与等待唤醒逻辑，不改变状态机和事件 schema；需注意 MCP 客户端自身的 HTTP 请求超时上限，避免 `timeout_sec` 超过客户端断连阈值。

<a id="ma_04"></a>
### MA-04：结构化上下游交接载荷（`handoff` + `risks`）与「契约先行」引导

- **问题现状**：
  1. 在 `backend/src/orchestrator/attempts.rs:112-129` 构建下游任务的 `upstream` 段落时，上游结果中只有 `short(&r.summary, INLINE_SUMMARY)`（最多 300 个字符）、`amendments`、改过的文件列表和产物路径被放进提示词；上游报告的 `r.risks` 只把条数 `r.risks.len()` 写进了 `context.handoff` 事件，**下游协作者在初始提示词里完全看不到上游留下的风险提示与接口细节**。
  2. 当上游是「需求澄清 / 接口设计 / 根因定位」任务、下游是「编码实现 / 联调验证」任务时，300 个字符的一句话摘要装不下函数签名、字段约定或边界假设；如果下游协作者没有主动调用 `janus.peers(task="...")` 查看详情，就会靠猜测补全细节，引发 MAST 分类中最常见的交接失真（Context Bleed）。
- **优化思路**：
  1. **分离面向看板的 `summary` 与面向下游的 `handoff`**：在结构化结果 JSON（`prompts.rs:139-141`、`parse_result`）中新增可选字段 `handoff`（字符串，限 1,000 至 1,200 个字符），专门填写「下游任务或验证者必须知道的接口签名、关键约定、文件路径与未决事项」；同时在 `attempts.rs:112-129` 中，若上游存在 `handoff` 或非空 `risks`，自动注入直接依赖它的下游任务提示词中。没有下游依赖的叶子任务可省略 `handoff`。
  2. **在 `teamwork::lead_guide` 中沉淀「契约先行（Contract-First）」范式**：明确引导负责人在拆分多模块并行编码任务前，若模块间存在共享数据结构、API 路由或事件协议，先安排一个短前置任务（或由负责人自己）把接口定义文件落盘，再扇出并行的模块实现子任务，从源头避免契约漂移。
- **边界与风险**：`handoff` 设为可选字段，旧结果和省略该字段的回复完全兼容，`verify_result` 不因缺少 `handoff` 判失败，避免给简单任务增加额外负担。

<a id="ma_05"></a>
### MA-05：补齐负责人编辑的痕迹追踪，强化跨模型聚焦式独立验证

- **问题现状**：
  1. **负责人的文件改动处于 `Traces` 盲区**：`backend/src/orchestrator/turn.rs:557-559` 只在 `let (Some(task), false) = (ctx.task, edited.is_empty())` 时调用 `record_edits`，而负责人的 `ctx.task` 为 `None`。如 [目标模式文档](../domains/20261031-goal_mode.md#conflicts) 所述，负责人自己的文件编辑不计入 `Traces`：若负责人在子任务运行时顺手改了子任务 `owns` 的文件，不会产生冲突记录；若负责人在 `T1` 完成后改了 `T1` 触碰过的文件，`traces::edited_after` 也不会向依赖 `T1` 的下游任务或验证者发出过期提醒。
  2. **独立验证者的输入未充分利用已有痕迹数据**：`backend/src/teamwork.rs:180-188` 的 `task_section` 只把被验证任务的 `id`、`title` 和 `accept` 列表传给验证者，让验证者自己去跑 `git diff` 或调 `peers`。但此时 Janus 其实已经在 `T1` 结束时算出了**限定在 `T1` 范围内的专属 diff 产物路径（`T1-a1.diff`）、`traceGaps`（结果与工作目录不一致之处）以及 `Summary.reminders`（仅靠 `reasoning` 或 `unverified` 通过的薄弱标准）**。
  3. **同目录并发构建干扰**：共享工作目录在轻量任务上非常高效，但当多个编译型子任务同时运行 `cargo test` 或 `swift test` 时，不仅争抢构建目录锁，还会因为隔壁任务改到一半的语法错误导致本任务的测试编译失败。
- **优化思路**：
  1. **将 `"lead"` 纳入 `Traces` 记录**：负责人完成编辑类工具调用时，以 `"lead"` 身份记入 `Traces.last_edit`；若命中的路径正由某个 `Running` 子任务负责（`owns` 或 `claim`），向负责人和看板发出提醒；上游任务完成后被 `"lead"` 修改的文件同样计入 `edited_after` 过期提示。
  2. **为 `verifies` 任务注入「审查靶向包」**：在 `attempts.rs` 构造验证任务提示时，自动附上被验证任务的 diff 产物路径、改过的文件清单、已发现的 `traceGaps` 以及证据方法为 `reasoning` / `unverified` / 未说明的条目，要求验证者优先针对这些薄弱条目执行 `test` 或 `command` 实测；同时在 `lead_guide` 与工作区提示中鼓励**异构模型交叉验证**（例如 Claude 负责实现时优先指派 Codex 角色做 `verifies`，反之亦然，充分发挥 Janus 跨厂商 ACP 的独特优势）。
  3. **（Phase 3 可选，需独立 ADR）按需轻量副本隔离**：保持默认共享目录不变（见 [ADR：任务直接在工作目录中运行](../decisions/20261031-in_place_workspaces.md)）；针对需要独立编译测试或「同题多 Agent 只读对比」的场景，后续可评估利用 macOS APFS 原生 `cp -c`（Copy-on-Write 克隆，毫秒级且不占双倍磁盘，应用更新器 `Updater` 已在使用该机制）提供可选的任务级只读/隔离工作副本。
- **边界与风险**：第 1、2 项均为纯增量信息打通，风险极低；第 3 项涉及产物合并与恢复逻辑，必须等真实场景确有高频编译锁痛点后再单独立项写 ADR。

<a id="ma_06"></a>
### MA-06：增加协作者「请求拆分子任务 / 调整边界」的轻量协调通道

- **问题现状**：
  1. 当前任务图是单层 DAG，只有负责人能调用 `orchestrate`。协作者在深入阅读代码后，如果发现子任务复杂度远超预期，或需要跨多个其他模块联动修改，手头只有三个工具：`board_post`、`claim`、`peers`（`backend/src/orchestrator/board_tools.rs:138-168`）。
  2. 写 `board_post(kind="blocker")` 不会暂停当前回合，协作者往往不等负责人回应就继续硬做；而在结果 JSON 中填 `question` 虽然能停下等回复，但每轮提问都会消耗一次回合，且 `backend/src/orchestrator/attempts.rs:310-313` 硬编码了连续提问超过 4 轮即直接将尝试判为失败。
- **优化思路**：
  - 在协作者 MCP 工具中增加只读提议工具 `request_split`（或在结构化结果的停顿请求中增加 `proposal` 分支）：允许协作者提交「已完成的前置部分 + 建议拆出的子任务列表（标题、建议 `owns`、建议验收标准）及拆分理由」。
  - 该调用写入一条带结构化数据的看板事件并纳入 `needs_lead` 立即触发 `push_board`，同时可选择在结果中优雅挂起等待负责人裁决（不计入普通歧义提问的 4 次失败上限）。负责人核对后直接调用 `orchestrate` 采纳拆分、用 `amend` / `cancel` 收尾原任务，或用 `reply` 驳回并要求继续。
- **边界与风险**：严格遵守「只有编排器改任务状态、只有负责人提交编排」的架构原则，协作者不能自行派发子任务，防止出现递归无限扩权与失控的 token 扇出（见 [2026-11-11 X 补充观察](../research/20261111-multi_agent_collaboration.md)）。

<a id="ma_07"></a>
### MA-07：升级交替死循环检测与 Run 级成本预算闸门

- **问题现状**：
  1. **交替死循环漏检**：`backend/src/orchestrator/lifecycle.rs:344-359` 的 `watch_call` 只比较当前工具调用哈希是否与**紧邻上一次**完全一致（`if w.call == call { w.repeats += 1 } else { w.repeats = 1 }`）。而在真实编码任务中，Agent 最常见的卡死形态是 2 步或 3 步交替循环——例如「编辑 `foo.rs` → 运行 `cargo test` 报错 → 再改 `foo.rs` → 再跑 `cargo test` 报同款错」（`A → B → A → B`），此时 `w.repeats` 永远是 1，看门狗直到 30 分钟单次尝试超时前都不会发出任何 `[停滞]` 预警。
  2. **缺少主动的成本止损闸门**：`usage.rs` 已经能从事件实时算出精确到任务、成员与模型的 token 与上报费用，并在超过 50 万 token 等阈值时生成文字提醒，但这些提醒不暂停调度；6 小时的墙钟预算（`RUN_BUDGET_MS`）无法阻止多 Agent 并行死循环在半小时内烧掉高额 API 费用。
- **优化思路**：
  1. **短周期模式检测**：在 `Watch` 结构中保留最近 `K` 次（如 12 次）工具调用的指纹环形缓冲区，检测周期长度为 1、2、3 的连续重复序列（如 `ABABAB` 重复满 4 轮，或同一文件连续修改加测试失败满 N 轮），命中时同样发出 `task.stalled`（`reason: "loop"`）推送给负责人提前干预。
  2. **可选的 Run 级 Token / 估算费用预算**：在设置或创建 Run 时允许指定可选的 `maxTokens` / `maxCostUsd` 阈值；`lifecycle.rs:151` 调度循环每次唤醒时读取账本（或增量累加器），达到预算上限且仍有未完成任务时，直接复用现有的 `Outcome::Budget` → `RunStatus::WaitingInput` 与 `extendBudget` 操作流，交由用户决定追加额度还是停止验收。
- **边界与风险**：部分自定义 Agent 不上报 token 或费用，预算闸门应按「仅对已上报部分生效，未上报时不误拦」处理，并在界面写明口径；改变预算暂停规则需同步更新 [Run 生命周期](../domains/20261031-run_lifecycle.md#timeouts) 与 [配置](../interfaces/20261031-configuration.md)。

<a id="roadmap"></a>
## 5. 建议的演进路线图

建议按「先打通已有信息与检测盲区，再升级核心并发与热会话复用，最后按需演进弹性能力」分三步推进，每个独立主题单独提交：

1. **Phase 1：低风险快赢（不改状态机与存储表，立竿见影改善协作质量）**
   - 落地 **MA-03（前半）**：`team_lines_vec` 暴露协作者挂载的 Skill 与 MCP 摘要，解决盲派工。
   - 落地 **MA-04**：结构化结果支持可选 `handoff` 字段并在下游提示词中透传 `handoff` 与 `risks`，更新 `lead_guide` 补充契约先行约定。
   - 落地 **MA-05（前两项）**：将负责人编辑纳入 `Traces`，给 `verifies` 验证任务直通专属 diff 产物路径、`traceGaps` 和弱证据条目，并引导跨模型交叉验证。
   - 落地 **MA-07（前半）**：`watch_call` 支持 2 至 3 步交替死循环检测。
2. **Phase 2：核心协作吞吐与低延迟闭环（需先写 ADR）**
   - 落地 **MA-01**：工作区角色支持并发槽位 `slots` + Run 级 `max_concurrent_attempts` 全局并发上限，解决同质研发任务被迫串行的问题。
   - 落地 **MA-02**：基于 `session/load` 实现 `continues` 与 `retry(continue_session)` 原会话热恢复，降低「实现 ↔ 验证 ↔ 修复」循环的冷启动延迟与 token 重算成本。
   - 落地 **MA-03（后半）**：`board_wait` 改为 `Notify` 事件驱动唤醒，并在超时返回运行中子任务的实时进展脉搏。
3. **Phase 3：进阶弹性与治理能力（视真实 Run 数据按需立项，需独立 ADR）**
   - 落地 **MA-07（后半）**：Run 级 token / 费用主动预算暂停闸门。
   - 落地 **MA-06**：协作者 `request_split` 拆分提议工具。
   - 结合 [动态数字员工与资源装配研究](../research/20261117-dynamic_digital_workers.md) 评估任务前资源附加，以及高冲突编译场景下的按需 APFS 副本隔离。

<a id="doc_drift"></a>
## 6. 附：文档与代码一致性核对（DOC-01 至 DOC-02）

本次审查过程中对照代码顺带核实出 2 处轻微的现状文档残留，按只读审查原则在此记录，留待后续文档维护或对应模块改动时一并清理：

- **DOC-01**：[目标模式与共享看板](../domains/20261031-goal_mode.md#lead_brief) 第 65 行写负责人说明（`goal_brief`）包含「负责人角色的 Skill、MCP，以及工作目录中的项目 Skill」，但核对 `backend/src/orchestrator/lead.rs:656-744` 的 `App::goal_brief` 实现，其中并不包含这项内容（按 [ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](../decisions/20261116-launch_time_resources.md)，资源已统一通过 ACP 启动参数注入，不再写进说明文本）。同一文档第 135 行「协作者接入看板的条件」仍列出「Run 是目标模式」，而在早期计划模式代码删除后，所有 Run 均为目标模式，`backend/src/orchestrator/attempts.rs:171-175` 只检查 `mcpCapabilities.http == true`。
- **DOC-02**：[Run 生命周期](../domains/20261031-run_lifecycle.md#attachments) 第 41 行在说明图片附件能力声明时仍写「官方 Claude、Codex 和 Grok 适配器都声明了这项能力」；内置运行时已在 [ADR：内置运行时只保留 Codex 和 Claude Code](../decisions/20261116-codex_and_claude_only.md) 中删除 Grok，建议后续整理口径时改写为仅列内置支持的 Claude 与 Codex 或注明外部适配器背景。
