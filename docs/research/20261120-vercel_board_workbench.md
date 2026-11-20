# Vercel Board 与 Janus 的工作台演进（2026-11-20）

> 日期：2026-11-20。研究对象：Vercel Board（Experimental）的全部官方文档，以及 `vercel/board` 仓库中的 README、`templates/factory`、`templates/swarm` 和 `SPEC.md`。访问日期均为 2026-11-20。
> 研究问题：Janus 计划从「一次 Run 的多 Agent 协作」演进成长期存在的工作台，可以从多个入口发起工作，例如独立的手机端、监测 GitHub issue 并自动处理。本文研究 Vercel Board 怎样设计一个长期存在、可以从多个入口进入的协作系统，以及 Janus 应该借鉴什么、不借鉴什么、按什么顺序做。
> 来源：Board 文档 16 页（概览、概念索引和 8 个概念页、access、指南索引和 2 篇指南、CLI 参考、SDK 参考），仓库根 README 和 `SPEC.md`；两个模板的源码见同日的[模板源码研究](20261120-vercel_board_templates.md)。完整列表见[来源](#sources)。
> 限制：`SPEC.md` 约 279k 字符，只逐字读了前约 40k，其余内容来自抓取工具的摘要，引用其后半部分的结论标为 U。没有实际安装和运行 Board。Board 处于实验阶段，文档说明「Behavior, APIs, and limits may change」。
> 证据标记：沿用 [2026-11-16 研究](20261116-agent_team_market_landscape.md) 的约定。**F** 表示一手来源明确写出；**I** 表示由多个事实推出、未直接验证；**J** 表示本文判断；**U** 表示来源不完整或无法核实。Janus 的现状按 2026-11-20 的文档和代码核对。报告是判断，不是 Janus 当前实现的说明。

## 导航

- [摘要](#summary)：只读这一节就能知道结论。
- [Vercel Board 是什么](#board_overview)和[两个模板](#templates)：外部事实，评估具体机制时读。
- [Board 的设计取舍](#board_tradeoffs)：Board 和 Janus 在哪些根本问题上选择不同。
- [Janus 现状核对](#janus_today)：已有什么、外层缺什么。
- [演进方向](#direction)：目标形态、设计原则、入口、GitHub、手机端、无人值守。
- [分阶段建议](#phases)：P0 / P1 / P2，含落点、指标和风险。
- [与既有研究的关系](#prior_research)和[待确认问题](#open_questions)：判断本文和前几份研究的分工，以及哪些建议还要用户决定时阅读。

<a id="summary"></a>
## 摘要

- **Board 是团队级、长期存在的通信层，不是编排器**。它提供消息、thread、标签、带租约的任务队列、单一有序事件日志、权限和外部同步；Agent 在哪里运行、谁做判断、代码怎样回到仓库，都留给使用者（F）。
- **Janus 的 Run 这一层已经比 Board 深，缺的是外面那一层**。结构化结果、独立验证、文件痕迹、续接会话、worktree 交付，Board 都要使用者自己搭。Janus 缺的是跨 Run 的工作台：服务端的收件箱、工作区级事件流、多入口和对外交付（I/J）。
- **演进时保留 Run 作为执行单元，在工作区外面加一层工作台**。所有入口（Mac 应用、手机、GitHub issue、定时）都先生成一条「请求」进入收件箱，再按来源的信任规则决定是否开始 Run（J）。
- **Board 最值得借鉴的是信任规则，不是数据结构**。任务正文就是给认领者的指令，所以默认只执行自己创建的任务；一条消息不能证明有人批准，不可逆操作改用身份可验证的渠道（GitHub review）；对外同步默认只进不出，只发在 Board 上写的内容（F）。
- **factory 模板证明了「issue 到 PR」这条路可行，并给出了具体做法**。只把仓库有写权限的人当团队，但这一条只写在提示词里；每个改动开独立分支，经 GitHub API push 以获得签名；令牌按 thread 限定仓库，放在 sandbox 的网络策略里，不进 sandbox。细节见[模板源码研究](20261120-vercel_board_templates.md)（F）。
- **手机端是信任边界变化最大的一步，要先定前提再动手**。Janus 现在的 HTTP 服务没有鉴权、只监听回环地址。Board 本地版的答案是 `vb serve --tailscale` 共享到 tailnet（F），这是一个可参考的起点，但设备配对、按设备吊销和审批归属都要 Janus 自己设计（J）。
- **建议顺序：工作台地基 → GitHub issue → 手机端**。地基（服务端收件箱、工作区事件流、Run 的来源字段）不扩大信任边界，可以先做；GitHub 和手机端各自先写 ADR，按规则 5 单独提交（J）。

<a id="board_overview"></a>
## 1. Vercel Board 是什么

### 1.1 定位

文档的定义是「the communication layer for your team's people and coding agents」，要解决两类问题：Agent 之间协调（「A lead agent plans work as tasks, and a pool of workers claims and completes them」），以及人和 Agent 协作（「People ask for work from Slack, and agents answer in the same thread」）（F）。

入口是 `vercel board` CLI 和 `@vercel/board` SDK。Agent 只要能执行命令就能接入，安装方式是一个 skill（`npx skills add vercel/board --skill board`）。实验阶段所有套餐免费，以后可能收费；数据只存在美国，没有区域选项（F）。

### 1.2 数据模型

```
Team
└── Board              团队级容器，默认一个「General」，需要隔离时另建
    ├── Channel        路径，例如 dev、www/production；第一次发帖时创建，路径不可改
    │   └── Thread     根消息加回复，只有一层
    │       └── Message  Markdown 文本、标签、表情、附件、结构化数据（blocks）
    ├── Grants         按 channel 模式授予的权限
    └── 事件日志        整个 Board 一份，所有写入按顺序追加
```

一次协作通常是一个 Board 里的一组 channel。swarm 指南用 `build` 放任务、用 `decisions` 放要人拍板的问题。Board 本身不知道一次协作的起止（F/I）。

### 1.3 身份：principal 和 profile

- **principal** 由凭据决定：`vercel login` 的个人、部署的 OIDC token、已安装的 app。「Board has no accounts of its own」「Nothing a client sends sets a principal's name or handle」（F）。
- **profile** 只是名字，例如 `fx via ada`。「A profile is named, never detected」，它没有自己的权限，只能修改它自己写的内容。同时运行的 Agent 要用不同的 profile（`worker-1`、`worker-2`），共用一个 profile 就是同一个 actor（F）。
- 直接后果：Agent 用人的凭据写入，所以「A message on the board isn't proof that a person approved something」。合并、部署、发布这类不可逆操作，指南建议「ask for approval where identity is verified, such as a GitHub review」（F）。

### 1.4 单一事件日志和游标

- 每次写入追加一个事件。`read`、`tail`、`wait` 和 watcher 都读这份日志，消息 ID 也是游标（F）。
- `wait` 一次只返回一个匹配，返回时打印续读命令，Agent 按轮次循环不会漏事件；过滤条件对变化前后的消息都检查，所以跟随 `state=open` 也能收到关闭这次变化（F）。
- 只能收到自己能读的 channel 中的事件；`task.renewed` 这类记账事件默认隐藏（F）。

### 1.5 给并发写入准备的原语

| 机制 | 作用（F） |
| --- | --- |
| `--idempotency-key` | 同一个 key 重试只写一次；换 channel 复用返回 `duplicate_id` |
| `reply --after <id>` | 只有 `<id>` 仍是最新消息时才回复，否则 `newer_message` |
| `--if-revision` | 编辑和删除的乐观锁，冲突时 `revision_conflict` |
| `label --if` / `--if-unset` | 带条件写标签，失败时 `label_conflict` |
| `board.` 前缀 | 保留给系统，写入返回 `reserved_label` |

文档特别提醒「A label never expires」：用标签表示认领，Agent 挂掉后认领会一直留着，所以交给 Agent 的工作要用带租约的任务（F）。

### 1.6 任务

- 任务是带状态的 thread，状态有 `waiting`、`ready`、`claimed`、`blocked`、`completed`、`failed`、`canceled`，同时写在根消息的 `board.task` 标签上（F）。
- **租约**：认领最长 5 分钟，`renew` 续期；过期后任务回到队列，旧持有者的 `complete`、`fail`、`block` 返回 `claim_lost`。SDK 用 `epoch` 和 `token` 防止过期持有者写入（F）。
- **`tasks run <id> -- <cmd>`**：把任意命令包装成 worker，每过三分之一租约续期一次，按退出码完成或失败，Ctrl-C 和 SIGTERM 释放任务（F）。
- **依赖**：`--needs` 可以跨 channel、跨 owner；前置任务失败或取消时后续任务不就绪，`reopen` 原地重试（F）。
- **计划**：JSON 批量创建，每批最多 15 个，一批内全部成功或全部失败；未知字段直接拒绝，避免把 `needs` 拼错成 `need` 生成孤立任务（F）。
- **owner 规则**：`--ready` 默认只认领自己创建的任务，理由是「A task's text is instructions for whoever claims it」；执行别人的任务要用 `--from` 指定 owner。非 owner 修改了任务正文，任务被挂起等 owner 审核（F）。
- **提问会释放 worker**：「A question doesn't hold a worker.」worker 用 `tasks block --kind needs_input` 交出认领后退出；`unblock` 后下一个空闲 worker 认领，从头读 thread 继续。worker 是无状态的，thread 就是记忆（F）。

### 1.7 外部同步

| 平台 | 方向（F） |
| --- | --- |
| Slack | 双向：消息、回复、编辑、删除、表情、文件 |
| GitHub | 默认只进不出：PR 和 issue 各成一个 thread，根消息带 `pr=`、`state=`、`review=`、`head=` 标签；「A new board thread never opens an issue or a pull request」 |
| Linear | 双向：Board 上的新 thread 会开一个 Linear issue |

- 同步运行在使用者自己的部署里，平台凭据由 Vercel Connect 托管，「your project stores no platform secrets」（F）。
- 标签和任务状态永远不同步出去，外部平台只看到文本和表情（F）。
- 防外泄：「A synced channel is as readable as the platform channel it syncs with」；同步私有 Slack channel 之前先建私有 Board channel 并授权给同一批人（F）。
- 投递失败重试 5 次后记 `message.sync_failed`，不阻塞后续消息（F）。

### 1.8 权限

- 新 Board 默认三条 grant：团队全员（`g1`）、所有项目的部署（`g2`，包括 preview，并且有 `sync` 权限）、所有已安装 app（`g3`）。「Everyone on your team can read every channel by default」（F）。
- 文档自己指出 preview 部署「run any branch's code, and `g2` lets them in」（F）。
- `team.as(profile, { can })` 收窄 Agent 的权限，收窄后不能改 grant、不能注册 watcher、不能读同步记录（F）。

### 1.9 本地版和协议

- `npx @vercel/board serve`（`vb serve`）在 `127.0.0.1:4100` 运行本地 Board，数据在 Git 根目录的 `.board/`；`--tailscale` 把它共享到 tailnet（F）。
- `vb conformance` 检查一个 Board「speaks the protocol」；仓库 README 写「Any object that answers the protocol works」；`SPEC.md` 的原则包括「The log is the only source of truth」。仓库许可证是 Apache-2.0（F）。
- 有 schema 注册表，内置 `git-commit@1`、`vercel-deployment@1`，消息可以带校验过的结构化数据（F）。
- CLI 有 `hooks uninstall`，说明是「Removes turn-end hooks an earlier board installed in Claude Code, Codex, Cursor, and Pi」。可以推断早期版本往这些 Agent 的配置里装过回合结束 hook，后来撤回（I）。

<a id="templates"></a>
## 2. 两个模板：factory 和 swarm

文档没有讲代码怎样回到仓库，`vercel/board` 仓库里的两个官方模板补上了这一段。源码级的研究见 [Vercel Board 模板源码研究：factory 与 swarm](20261120-vercel_board_templates.md)，这里只列和本文结论相关的要点：

- **factory** 是长期在岗的单个 Agent：GitHub issue、Slack 和看板上的消息进入按仓库划分的 channel，每个 thread 一个会话、一个 sandbox。bug 先写失败的测试复现，功能需求先讨论，交付物一律是独立分支上的 PR，合并由人决定；开了 PR 后给自己排逐渐变稀的检查来跟进（F）。
- **swarm** 是一次性的多 Agent 并行：用户填目标、场景、Agent 数和预算（或时间上限），调度循环按看板状态补齐 sandbox；runner 代码负责认领、提交、推送和结算，模型只决定任务内部做什么；每个 work 配一个只读的 review（F）。
- **共同的底线**：凭据只放在 sandbox 的网络代理里，不进 sandbox；GitHub 令牌按仓库限定，swarm 的审查任务只拿只读令牌；fork 的 PR 不处理（F）。
- **factory 的「陌生人不是团队」只是写在提示词里的约定**，代码只硬性过滤了 fork 的 PR 和它自己开的 PR（F/I）。这也是本文建议 Janus 在代码里按来源判断信任的原因（J）。

<a id="board_tradeoffs"></a>
## 3. Board 的设计取舍

| 问题 | Board 的选择 | Janus 的选择 | 判断 |
| --- | --- | --- | --- |
| 做到哪一层 | 只做通信层和队列，判断、验证、交付由使用者搭 | 做完整的团队运行时 | 两者都自洽；Janus 的差异正是 Board 不做的部分（J） |
| 怎样派活 | worker 拉活，靠租约判断存活 | 编排器推送，直接掌握 ACP 进程 | Janus 知道进程死活，不需要租约（J） |
| Agent 的上下文 | worker 无状态，提问后释放，下一个 worker 重读 thread | 回到原会话继续，`session/load` 续接 | 续接保留推理过程，是 Janus 的优势，不应退回（J） |
| Agent 怎样说话 | 只能调用命令，「Your final message reaches nobody」 | 看板 MCP 工具加结构化结果 | 两者都把「说话」变成显式调用；Janus 多了格式核对（I） |
| 状态表达 | 除 `board.task` 外都是自由命名的字符串标签 | 类型化枚举（合并前检查第 10 条） | 不借鉴（J） |
| 审批的可信度 | 承认 Board 消息不能证明有人批准，转给 GitHub review | 审批只交给用户，负责人不能代批 | 方向一致；一旦有远程入口，Janus 也要回答「是谁批准的」（J） |
| 控制面 | serverless：定时路由补满 sandbox 池，watcher 唤醒负责人，状态全在 Board 上 | 本机常驻服务，状态在 SQLite 事件日志 | 本地优先决定了 Janus 要回答 Mac 睡眠和离线的问题（J） |
| 外部文本 | 默认只执行自己创建的任务；陌生人的 issue 只讨论 | 目前只有用户本人发起 | 多入口之后必须引入同样的规则（J） |

<a id="janus_today"></a>
## 4. Janus 现状核对（2026-11-20）

### 4.1 已经有的、和外层相关的能力

- **工作区**是长期对象：目录、成员、负责人、共享资料（`brief`，最多 8000 个字符，只有用户能写），见 [Agent、角色与工作区](../domains/20261031-agents_and_roles.md#workspaces)（F）。
- **收件箱**由应用每 5 秒轮询并在本地推导，跨工作区汇总待办。目标任务的候选范围是最新 50 条，更早任务的表单可能仍在等待。「稍后」和「标记已处理」保存在本机 `UserDefaults`（F）。[收件箱](../architecture/20261031-frontend.md#inbox) · [跨工作区待办实测](../review/20261120-feature_logic.md#inbox_coverage)
- **事件流**按 Run 提供：`GET /api/runs/{id}/stream?after=<seq>` 是 SSE，先补历史再推新事件，订阅在读取历史之前，不会漏事件，见[事件流](../interfaces/20261031-http_api.md#sse)（F）。
- **幂等**：Run 操作按 `commandId` 只执行一次，记录在 `commands` 表（F）。
- **并发和预算**：角色可以配置 `slots`（1 到 8）展开为多个成员实例；每个 Run 的协作者尝试数受 `max_concurrent_attempts` 限制（默认 3）；Run 预算是 6 小时墙钟加可选的美元上限，见[时间限制](../domains/20261031-run_lifecycle.md#timeouts)（F）。
- **交付**在有初始提交的 Git 仓库中使用独立 worktree。验收通常把结果提交并快进合回开始时的分支。非 Git 目录和没有初始提交的仓库直接在工作区目录执行。变基冲突时保留任务提交供人工处理。目录缺失、游离 HEAD 等边界的实际行为见同日审查（F）。[验收决策](../decisions/20261120-merge_back_on_accept.md) · [验收边界实测](../review/20261120-feature_logic.md#high_impact)

### 4.2 外层缺的能力

| 缺口 | 现状（F） | 为什么在工作台里会成为问题（J） |
| --- | --- | --- |
| 服务端收件箱 | 推导在应用里，推迟状态只在本机 | 手机和 Mac 看到的待处理事项、推迟状态会不一致 |
| 工作区级事件流 | 只有单个 Run 的 SSE；应用每 5 秒轮询 Run 列表 | 多个客户端要各自拼出「工作区里发生了什么」 |
| Run 的来源 | Run 只能由用户在应用里创建，没有来源字段 | 不知道一个 Run 从哪里来，就无法按来源设定信任和回写规则 |
| 跨 Run 的决策记录 | `ask_user` 的问答只留在本 Run 的事件里 | 长期工作台里同一个问题会被反复问 |
| 跨 Run 的并发和预算 | 上限和预算都是 Run 级 | 自动处理 issue 时，Run 的数量本身需要上限 |
| 对外交付 | 只有本地快进合并 | issue 来的工作，自然的交付物是 PR |
| 远程访问 | HTTP 服务没有鉴权，默认只监听 `127.0.0.1:7878`，见[安全边界](../operations/20261031-security.md)的「网络暴露」 | 手机端无法接入，也不能简单地改监听地址 |

<a id="direction"></a>
## 5. 演进方向

### 5.1 目标形态

```
Janus home（Mac 上常驻的服务）
└── 工作区（长期存在）
    ├── 入口         Mac 应用、手机、GitHub issue、定时……各自产生「请求」
    ├── 收件箱       服务端：请求、待你处理、等验收；所有客户端看到同一份
    ├── 工作区事件流  跨 Run，带游标；客户端各自从自己的游标续读
    ├── 决策记录     每个 Run 中你的回答，之后的 Run 可以查到
    ├── 共享资料     （已有）
    └── Run          （已有）执行单元，多一个类型化的来源字段
        └── 子任务、尝试、产物、worktree……
```

和 Board 的对应关系：Board 的 channel 大致对应 Janus 的 Run，Board 的 Board 加事件日志对应新增的工作区层。区别在于 Janus 保留 Run 的生命周期（创建、交付、验收），这是交付和验收能成立的前提（J）。

### 5.2 设计原则

1. **Run 仍是唯一的执行单元**。工作台只决定「要不要开始一个 Run、交付到哪里」，不改变 Run 内部的编排、验证和交付（J）。
2. **本地优先不变：Mac 是服务端，其他入口是客户端或请求来源**。手机端默认是遥控本机服务的客户端，Mac 关机或睡眠时不能开始新工作。是否需要云端运行，见[待确认问题](#open_questions)（J）。
3. **外部来的文字一律是数据，不是指令**。对应 Board 的「A task's text is instructions for whoever claims it」：issue 正文、评论、手机上收到的转发内容进入负责人说明时，放在标明来源的数据段里，不进系统层（F/J）。
4. **按来源决定信任**。你本人在 Mac 或已配对手机上提交的请求直接开始；GitHub 上有写权限的人提交的，按工作区设置自动开始或等你确认；其他人提交的只进收件箱，factory 的做法是「discuss, not an order」（F/J）。
5. **对外操作要有身份可验证的批准**。push、开 PR、回写评论、合并都要你批准；能放到 GitHub review 里批准的，优先用 GitHub review（F/J）。
6. **回写只发结论**。回写 GitHub 只发交付摘要和 PR 链接，不发看板过程、日志和路径（对应 Board「Send back only what was written on the board」）（F/J）。
7. **状态都用类型表达**。请求的来源、信任级别、收件箱状态都是枚举，不从 id 或标签字符串推断（合并前检查第 10 条）。新增字段要写迁移（第 11 条）（J）。

### 5.3 统一的「请求」模型

所有入口都产生同一种对象，生命周期如下：

```
received ──分流──▶ waiting_user ──你确认──▶ started（关联 Run）──▶ delivered ──▶ reported
    │                    │
    └──来源可信，自动──────┘            dismissed（你拒绝，或来源撤回）
```

- **字段**：`source`（枚举：`app`、`mobile`、`github_issue`、`schedule`）、`author`、`trust`（枚举，由来源和工作区规则算出，创建时固定下来）、原文、外部链接、关联的 Run。
- **分流规则借鉴 factory**：bug 类先要求复现，功能类先让你确认范围，问题类可以只回答不改代码。第一阶段只用简单规则，规则写在工作区设置里，不做成模型判断（J）。
- **原文被修改**：issue 在 Run 开始后被编辑，请求回到 `waiting_user`，Run 暂停等你确认。对应 Board「非 owner 修改任务正文时挂起任务」（F/J）。

### 5.4 GitHub issue 自动处理

| 环节 | 建议（J，括号内是借鉴来源） |
| --- | --- |
| 发现新 issue | 用本机 `gh` 定期轮询，不接收 webhook，不新增公网监听（Board 的同步需要公网可达的部署，Janus 不具备） |
| 信任 | 按 author association：`OWNER`、`MEMBER`、`COLLABORATOR` 视为团队；其余只进收件箱（factory） |
| 来自 fork 的 PR | 不处理（factory） |
| 执行 | 照常开 Run，在 worktree 中工作；`gh` 的令牌不注入 Agent 的环境，push 和开 PR 由 Janus 在验收后执行（factory 把令牌放在 sandbox 外） |
| 交付 | 新增一种交付方式：push 独立分支并开 PR，和现有的「快进合并回本地分支」并列，按请求来源选择默认值；不 push 默认分支（factory「never the default branch」） |
| 批准 | push 和开 PR 需要你在收件箱里批准；合并留给 GitHub review（Board 指南） |
| 回写 | 只回写交付摘要和 PR 链接；不回写过程（Board 同步规则） |
| 跟进 | PR 的 review 和 CI 状态作为新的请求回到收件箱，「要修改」时由你决定是否在原 Run 中继续（Board 的 `review=` 标签） |

需要注意的风险（J）：

- 仓库自己的脚本（测试、钩子、`postinstall`）在 Agent 运行时都能执行。factory 的警告同样适用于 Janus：只对信任其提交者的仓库开启自动处理。
- 签名：Janus 用用户的 Git 配置在本地提交，是否签名取决于用户；如果改为经 GitHub API push，提交会由 GitHub 签名，但需要 GitHub 令牌，属于新的凭据存放方式。
- 这一步新增了凭据（GitHub 令牌）、新的外部写操作和后台自动执行，按合并前检查第 5 条要先写 ADR、更新安全文档、单独提交。

### 5.5 手机端

**先确定前提**：「独立的手机端」是遥控 Mac 上服务的客户端，还是 Mac 不在线也能工作？本文按前者展开；后者意味着云端运行 Agent，与本地优先的定位冲突，需要另行研究（J）。

**手机端做什么**：查看收件箱、回答提问、审批、验收、用文字或语音发起请求、查看 Run 进度和交付摘要。不做：修改 Agent 设置和凭据、读取 MCP 服务器的值（`GET /api/library/mcp/{id}/values` 继续只回答回环地址）（J）。

**远程访问方式**：

| 方式 | 优点 | 代价 | 判断（J） |
| --- | --- | --- | --- |
| 只在同一局域网 | 不经过第三方 | 离开家或办公室就用不了；局域网内其他设备也能访问到端口 | 不够用 |
| tailnet（例如 Tailscale） | 不经过 Janus 自建的服务；Board 本地版也用 `vb serve --tailscale`（F） | 要求用户安装和登录第三方工具；Janus 仍然要自己做鉴权 | 第一阶段的候选 |
| Janus 自建中继 | 体验最好，可以接系统推送 | 引入一个 Janus 运营的云服务，所有流量经过它 | 推迟，需要单独评估 |

**无论哪种方式都要有的**（J）：

- **设备配对**：在 Mac 应用里显示一次性配对码，手机配对后拿到只属于这台设备的令牌；令牌存在 Janus home 的凭据目录，接口不返回；Mac 应用可以列出和吊销设备。
- **审批归属**：每个审批、回答、验收事件记下是哪台设备做的。这回答了 Board 指出的问题：远程入口来的批准到底是谁做的。
- **服务端收件箱是前提**：现在的推迟状态只在 Mac 的 `UserDefaults` 里，手机端上线前要先迁到服务端。
- **推送通知**：系统推送要经过 APNs，需要一个持有推送证书的服务端，属于新的外部服务。第一阶段可以只在 App 打开时刷新。

### 5.6 无人值守的约束

自动处理 issue 和定时任务让 Run 在你不在场时开始，需要工作区级的约束（J）：

- **同时运行的 Run 数上限**：现在的 `max_concurrent_attempts` 是 Run 级的，缺工作区或全局的 Run 数上限；swarm 用 `POOL_SIZE` 控制同时运行的 sandbox 数（F）。
- **每日预算**：Run 级美元上限之外，加工作区每日上限；达到后新请求停在收件箱。swarm 承认按标签汇总的预算会滞后一个对账周期（F），Janus 在本机汇总用量，可以做到不滞后。
- **紧急停止**：一键暂停工作区的所有自动入口和正在运行的自动 Run。
- **Mac 睡眠**：在设置和手机端如实显示「Mac 离线时不会开始新工作」，不承诺 24/7。
- **失败通知**：自动 Run 失败时进入收件箱，不静默重试。

### 5.7 不照搬的部分

- **租约和无状态 worker**：Janus 掌握进程，续接原会话是优势。
- **自由命名的字符串标签**：与类型化状态的规则冲突。
- **多用户和 grants**：除非决定支持多人共用一台 Janus，否则不需要。
- **默认对团队全开**：Board 默认让全员、所有部署和 app 读写全部 channel，自己也提醒 preview 部署的风险。Janus 的默认应该是只有你本人和你配对的设备。
- **Slack 和 Linear 双向同步**：先只做 GitHub；IM 入口等手机端做完后再评估。

<a id="phases"></a>
## 6. 分阶段建议

### P0：工作台地基（不扩大信任边界）

**P0-1 服务端收件箱**

- **做什么**：把收件箱的推导和「稍后」「已处理」状态从应用迁到服务端，提供一个跨工作区的收件箱接口；Mac 应用改为读取它。
- **为什么**：手机端和自动入口都要求所有客户端看到同一份待处理事项；现在的状态只在本机 `UserDefaults`（F）。
- **落点**：`frontend/Sources/Janus/Core/Inbox.swift` 的推导迁到后端，`docs/architecture/20261031-frontend.md#inbox`、`docs/interfaces/20261031-http_api.md`；推迟状态是新的存储数据，按合并前检查第 11 条加迁移。
- **指标**：Mac 应用和服务端推导结果一致（用现有 `DeriveTests` 的用例对照）；收件箱刷新不再需要逐个 Run 轮询。
- **风险**：推导逻辑在 Swift 和 Rust 各有一份会漂移；迁移后应用只展示，不再推导。

**P0-2 工作区事件流**

- **做什么**：提供跨 Run 的工作区事件流（SSE，带游标），内容是 Run 级别的摘要事件（创建、状态变化、需要你处理、交付），不是所有事件的复制。
- **为什么**：Board 的单一日志加游标让多个客户端各自续读不漏事件（F）；Janus 现在只有单个 Run 的 SSE。
- **落点**：`docs/interfaces/20261031-http_api.md#sse`、`docs/interfaces/20261031-events.md`；事件来源仍是各 Run 的 `events` 表，工作区流是派生视图还是独立存储，要在设计时用 ADR 决定。
- **风险**：如果另存一份，会有两个事实来源；优先做成派生视图。

**P0-3 请求与 Run 的来源字段**

- **做什么**：引入「请求」对象和 Run 的 `source` 枚举。第一阶段只有 `app` 一种来源，先把模型和收件箱的「请求」条目做出来。
- **落点**：`runs.config` 加字段并写迁移，`docs/architecture/20261031-data_model.md`、`docs/domains/20261031-run_lifecycle.md#create_run`。
- **风险**：过早设计太多来源；只为确定要做的来源加枚举值。

**P0-4 决策记录**

- **做什么**：把每个 Run 中你对 `ask_user` 的回答汇总到工作区，负责人说明附上最近的相关条目；你可以删除或改写。
- **为什么**：长期工作台里同一个问题会被反复问；Board 的 `decisions` channel 起的就是这个作用（F）。
- **边界**：只收你本人的回答，不收 Agent 的结论，延续「共享资料只有用户能写」的理由，见[工作区共享资料](../domains/20261031-agents_and_roles.md#workspace_brief)。

### P1：GitHub issue 入口（先写 ADR）

- **做什么**：按 5.4 节的表格实现：`gh` 轮询、按 author association 分流、PR 交付方式、批准后 push、只回写摘要。
- **前置**：ADR 和 `20261031-security.md` 更新（新凭据、新外部写操作、后台自动执行），单独提交（合并前检查第 5 条）；P0-1、P0-3 完成。
- **细化**：分流、PR 描述的生成、PR 跟进节奏和信任判断的具体做法，见[模板源码研究的 5.4 节](20261120-vercel_board_templates.md#implications)。
- **端到端**：`e2e/scenarios/` 新增场景，用本地假 `gh` 或假 GitHub 服务，不访问网络（合并前检查第 12 条）。
- **指标**：自动开始的 Run 中，交付后被你直接批准开 PR 的比例；被你拒绝的请求比例；未经你批准的外部写操作数（目标为 0）。
- **风险**：issue 正文的提示词注入；仓库脚本在 Agent 运行时执行；GitHub 速率限制。

### P1：工作区级的无人值守约束

- 同时运行的 Run 数上限、工作区每日预算、紧急停止，和 P1 的 GitHub 入口一起上线，不晚于它。
- 落点：`docs/domains/20261031-run_lifecycle.md#scheduling`、`docs/interfaces/20261031-configuration.md`；改变调度行为前写 ADR。

### P2：手机端（先写 ADR）

- **前置**：回答「遥控客户端还是云端」；P0-1、P0-2 完成。
- **ADR 要决定**：远程访问方式（推荐先评估 tailnet）、设备配对和吊销、哪些接口对远程开放、审批事件怎样记录设备、是否接系统推送。
- **指标**：从 Agent 提问到你在手机上回答的时间；手机上完成的审批和验收比例；被吊销设备的访问被拒（测试中要有反例，合并前检查第 1 条）。
- **风险**：这是 Janus 第一次接受非回环地址的请求，错误配置会暴露全部控制接口。

### P2：定时请求

- 定时产生请求（例如每周依赖检查），复用请求模型和收件箱；对应 [2026-11-16 研究的 P2-1](20261116-agent_team_market_landscape.md)。

<a id="prior_research"></a>
## 7. 与既有研究的关系

- **修正 [2026-11-16 研究](20261116-agent_team_market_landscape.md)「明确不建议」中的定位表述**：该报告写「Janus 是用户发起的本地工具」「不要宣称 24/7、always-on」。按 2026-11-20 的方向，Janus 会有非用户直接发起的 Run（GitHub issue、定时）。本文仍不建议宣称 24/7：Mac 离线时不工作，要如实说明。
- **提升 2026-11-16 研究的 P2-1（定时 / 事件触发的 Run）**：从「观察」提升为工作台主线的一部分，并按本文的请求模型重新设计；该报告「第一阶段只做本地 cron，不做公网 webhook」的边界保留。
- **与 [2026-11-17 动态数字员工](20261117-dynamic_digital_workers.md)和已定的下一阶段（资源库与动态能力路由）的关系**：两条线互相独立。工作台改的是 Run 外面，动态能力改的是 Run 里面的团队，先做哪条线由用户决定。
- **与 [ADR：验收时把结果合并回任务开始时的分支](../decisions/20261120-merge_back_on_accept.md)的关系**：PR 交付是新增的一种交付方式，不取代快进合并；本地发起的 Run 默认仍快进合并。

<a id="open_questions"></a>
## 8. 待确认问题

1. 手机端是遥控 Mac 的客户端，还是 Mac 不在线也要能工作？后者涉及云端运行环境。
2. 是否会有多人共用一个 Janus（例如团队共用一台 Mac mini）？会的话，grants 和身份模型需要重新评估。
3. GitHub 上 Janus 以谁的身份操作：用户自己的 `gh` 登录，还是一个专门的 GitHub App？前者简单但所有操作都记在用户名下，后者需要安装和托管 App 凭据。
4. 自动开始 Run 的默认值：可信来源的 issue 默认自动开始，还是一律等你确认？建议第一阶段一律等确认，用数据再决定。
5. 工作台和「资源库与动态能力路由」两条线的先后顺序。

<a id="sources"></a>
## 来源

Vercel Board 文档（F）：

- https://vercel.com/docs/board
- https://vercel.com/docs/board/concepts
- https://vercel.com/docs/board/concepts/boards
- https://vercel.com/docs/board/concepts/channels
- https://vercel.com/docs/board/concepts/threads
- https://vercel.com/docs/board/concepts/labels
- https://vercel.com/docs/board/concepts/actors
- https://vercel.com/docs/board/concepts/events
- https://vercel.com/docs/board/concepts/tasks
- https://vercel.com/docs/board/concepts/sync
- https://vercel.com/docs/board/access
- https://vercel.com/docs/board/guides
- https://vercel.com/docs/board/guides/agent-swarm
- https://vercel.com/docs/board/guides/agents-in-slack
- https://vercel.com/docs/board/cli-reference
- https://vercel.com/docs/board/sdk

`vercel/board` 仓库（F，`SPEC.md` 后半部分为 U）：

- <https://github.com/vercel/board>（README）
- 两个模板的源码：见[模板源码研究](20261120-vercel_board_templates.md)的来源
- <https://github.com/vercel/board/blob/main/SPEC.md>（只逐字读了前约 40k 字符）

Janus 现状（2026-11-20）：[根索引](../index.md)、[协作三方](../domains/20261119-collaboration_roles.md)、[目标模式与共享看板](../domains/20261031-goal_mode.md)、[Run 生命周期](../domains/20261031-run_lifecycle.md)、[Agent、角色与工作区](../domains/20261031-agents_and_roles.md)、[macOS 应用](../architecture/20261031-frontend.md)、[HTTP API](../interfaces/20261031-http_api.md)、[安全边界](../operations/20261031-security.md)、[数据模型](../architecture/20261031-data_model.md)、[ADR：角色并发槽位与 Run 级并发上限](../decisions/20261118-role_slots_and_concurrency_cap.md)、[ADR：验收时把结果合并回任务开始时的分支](../decisions/20261120-merge_back_on_accept.md)。
