# Janus 性能分析报告：工作区运行时界面卡顿

## 1. 审查抬头

审查日期：2026-11-17。

审查对象：commit `aa161d089956fafbb2297158d4a6a3453549d501`（`main`，2026-11-17 10:45:21 -0600，「Follow a conversation with the scroll APIs of macOS 15」）的代码快照，包括 `frontend/` 的 macOS 应用和 `backend/` 的 Rust 编排器。工作目录中未提交的改动不在本文范围内，本文也不讨论它们。

审查方式：先按 Project Doc 路由读 `docs/architecture/20261031-frontend.md`（含「实现约束」）、`docs/interfaces/20261031-events.md` 和 `docs/review/` 的报告约定，再沿「ACP 更新 → 事件写入 → 广播 → SSE → 解析 → derive → 视图失效 → 像素」逐段读代码。数字来自仓库外的一次性测量程序 `/tmp/janus-perf-20261117-8f3a`：把 `frontend/Sources/Janus/Core/` 的源码复制进去，用与后端 `orchestrator/turn.rs`、`acp_view.rs` 相同的事件映射和上限生成合成日志，每档重复多次取中位数；SQLite 用同目录的 `db.py` 测写入。没有启动真实 Agent 会话，没有访问网络，没有调用付费接口，没有修改仓库代码。

范围与限制：本文只分析，不修复。测量在单机单进程中进行，没有使用 Instruments，也没有启动应用测真实帧时间；视图成本用「每次 body 的纯计算」等价代码估计（`main.swift` 中逐字复制的分组与可见性函数），不是像素级帧时间。合成日志按后端代码的映射和上限生成，不代表某一次真实 Run 的事件分布；其中账本会话的建模与真实后端不同（合成 20k 日志单次约 3,032 个账本会话，5 次复跑的 checksum 合计 15,160；同一日志有 1,578 条 `attempt.started`，约为其 1.9 倍；真实后端每次尝试约 1 个会话，见 8.1），使 `derive`（含 `usageLedger`）的绝对值偏高约 2 倍，正文中的 14.7ms/6.4ms 不是真实 20k Run 的固定成本；`boardFacts`、视图过滤和 SQLite 数字不受这一建模影响；两个独立二进制（报告的 `perf` 与验证者另写的程序）复跑得到可复现的数字，见 8.5。凡是从代码或实测值外推的数字都标为「推算」（例如主线程约 40ms/批）。`/tmp` 下的测量目录可能被清理，附录重复了全部原始数字和复算命令。

## 2. 结论摘要

因果链：一次 Run 的每个 ACP 更新先由编排器写成一条 SQLite 事件，再经广播和 SSE 送到应用。应用在主 actor 上把 JSON 包成 `Event` 追加到 `RunModel.events`，每 60 毫秒合并一批，在后台线程跑 `derive`（其中包含 `usageLedger`），然后把结果整体赋给 `RunModel.state`。`state` 是普通存储属性，赋值不与旧值比较，所有读 `run.shown`、`run.state` 的视图在同一帧一起失效并在主线程重算：`RunView` 连同 `BoardView` 下的运行卡片、请求、交付结果、进展时间线、负责人会话和检查器全部重来。`ChatCard.body` 每次重建 `groups`（遍历整段会话事件、建 `done` 集合、逐条做 `ToolRow.hidden` 和 `ChatItem.visible`），`ProgressBoard` 即使折叠也先跑一遍 `boardFacts`，`agentSession`/`taskSession` 再过滤全部事件，可见的 Markdown 气泡重新解析。20k 事件时这组纯计算一次约 55.1ms（合成日志上的四项组合实测，是整棵视图成本的下界；其中 `derive` 部分因 8.1 的 session 建模偏高约 2 倍），其中约 40ms 在主线程（推算）；事件批次最多每 60ms 一轮，于是主线程几乎每一帧都在重算，滚动、输入、切页和回放拖动只能排队。事件量越大、会话越稠密，单次重算越贵，这就是「一跑起来就卡」的由来。

| 编号 | 症状 | 瓶颈位置 | 依据与数字 | 修复方向 | 预期收益 | 风险/成本 |
| --- | --- | --- | --- | --- | --- | --- |
| P-01 | 滚动、输入、切页普遍掉帧 | `frontend/Sources/Janus/Core/RunModel.swift:167` 整体替换 `state`；`frontend/Sources/Janus/UI/Run/RunView.swift:214` 起的整棵子树 | 实测：20k 事件四项组合纯计算 55.1ms（`boardFrameWork`，`main.swift:218-224`，下界；`derive` 部分偏高约 2 倍）；主线程约 40ms/批（推算） | 后台算好每帧要用的快照，按版本号失效；拆分可观察状态 | 每批主线程工作降到 <10ms，流式输出时恢复 60fps | 中高：改动面大，必须保证实时与回放一致 |
| P-02 | 长会话滚动、输入卡；冲突多时更明显 | `frontend/Sources/Janus/UI/Run/Session.swift:176-203` 每次 body 重建 `groups`；`Session.swift:407` 每个 `task.conflict` 扫描全部事件 | 实测：`chatGroups` 5.37ms/20k；`chatVisible` 42.2ms（200 个 `task.conflict`、同覆盖文件集）对 2.1ms（0 个）；收益随冲突数与数据形状变化 | 按事件数记忆化 `groups`；冲突覆盖关系一次建表 | 5.4ms → <1ms；42.2ms → <3ms（同样本） | 低：纯函数，加等价性测试即可 |
| P-03 | 进展时间线展开、收起都卡 | `frontend/Sources/Janus/UI/Run/SharedBoard.swift:16` 在 `if progressOpen` 之前跑 `boardFacts` | 实测：4.18ms/20k（5 次中位数）；dense 20k 16.0ms（3 次中位数） | 在后台 derive 阶段算一次，随 `state` 存进 `RunModel` | 每帧省 4–16ms | 低 |
| P-04 | 每 5 秒规律性一顿，开的 Run 越多越重 | `frontend/Sources/Janus/Core/Inbox.swift:115` 在主 actor 上 derive 全部累计事件 | 实测：derive 14.7ms/20k、40.6ms/50k（合成日志；session 建模偏高约 2 倍）；另加 `boardFacts` 4.2ms | 与 `RunModel` 一样移到后台，只把变化后的条目交回主线程 | 3 个 20k Run 的 5 秒刷新从约 57ms（合成日志口径）降到 <16ms | 低：注意与 Run 页的条目一致性 |
| P-05 | 打开、切回 Run、断线重连时长时间卡住 | `frontend/Sources/Janus/Core/RunModel.swift:93-94` 主线程 derive 加 `findDocuments`；`Core/RunDocs.swift:22-54` 全量扫描 | 实测：响应解析 447.8ms/20k、1124.0ms/50k（已在后台）；主线程 `derive` 14.7/40.6ms（合成日志，偏高约 2 倍）；`runDocsScan` 485.8ms/20k（另一事件集 755.8ms，不可合并比较） | load 的 derive 与文档扫描移出主线程；重连优先用 `?after=` 增量 | 打开/切回的主线程停顿从约 0.5–0.8 秒降到 <100ms | 中：文档菜单的更新时序与重连一致性 |
| P-06 | 拖动回放滑块一格一卡 | `frontend/Sources/Janus/Core/RunModel.swift:26` 的 `didSet` 同步 `derive(prefix)` | 实测：3.6/14.8/38.9ms（5k/20k/50k，合成日志，含 derive；20k 校正后约 7–8ms），另加整页重算 | 后台 derive、代际检查、逐帧合并 | 每格 <16ms | 低 |
| P-07 | 可见消息多、长时滚动更卡 | `frontend/Sources/Janus/UI/Markdown.swift:76` 每次 body 重新解析；`Session.swift:579` 无条件 `pretty()`；`UI/Run/SessionParts.swift:134` 每次 body 重算 diff | 实测：`MD.blocks`+`inline` 1.35ms/8KB；`InlineDiff` 4.37ms/2000 行；`JSON.pretty` 0.165ms/16KB | 按事件 seq 缓存解析结果；把 `pretty()` 移进展开体 | 5 条长消息由约 6.8ms/帧降到 <0.5ms | 低：缓存随事件只增不减，需限量 |
| P-08 | 会话长、任务多时每次重绘都慢 | `frontend/Sources/Janus/UI/Run/Session.swift:28,33`、`UI/Run/TaskView.swift:24`、`UI/Run/RunView.swift:562,617` 每次 body 全量过滤 | 实测：`agentSession` 2.71ms/6515；`runStatsScan` 4.00ms/20200；`taskViewFilter` 0.85ms/20200 | 在 derive 时按 target 分桶，视图只取桶 | 2.7ms → <0.5ms | 低 |
| P-09 | 事件到达变慢，积压和重连概率变高 | `backend/src/db.rs:183` 每条 autocommit 且默认 `synchronous=FULL`；`backend/src/orchestrator.rs:750` 每 Run 一把锁；`backend/src/main.rs:426` 每个订阅者单独序列化 | 实测：56.2µs/条（FULL）、28.2µs（NORMAL）、6.6µs（单事务批量） | WAL 下改 `synchronous=NORMAL`，按回合批量提交 | 写入耗时 −50%～−88% | 中：掉电可能丢最后几条事件；批量前须统一序号分配（P-09 正文） |
| P-10 | 滚动拖动粘滞、回到底部要试多次 | `frontend/Sources/Janus/UI/Run/Session.swift:242-251` 每帧几何回调、最多 8 次 landing、下一轮主循环 `scrollTo` | 回调本身未测；`ChatFollow.measured` 只在结果变化时赋值；最近两个提交 aa161d0、7d89402 修的是 macOS 15 上到不了底部 | 先修 P-01/P-02；再给布局回调和滚动次数加计数验证 | 保证回调 <1ms、滚动不再与重算抢主线程 | 低 |
| P-11 | 每批 derive 后重复文件检查 | `frontend/Sources/Janus/Core/RunModel.swift:168` → `Core/RunDocs.swift:52` 对每个候选 `fileExists` | 实测：全量 `runDocsScan` 485.8ms/20k（`scan.log`）；每批另做 O(候选数) 次 stat | 只在 `docScan` 变化时重算 `existing`；扫描移后台 | 每批 0 次文件系统调用 | 低 |
| P-12 | 侧栏、菜单栏每 5 秒轻微重算 | `frontend/Sources/Janus/Core/Inbox.swift:96-97` 每次读 `open`/`later` 都 split 加排序；`UI/MainView.swift:73` 每行再过滤 | 推理：约 50 条事项 × 10 行排序，几十 µs | `refresh` 时算一次，存成属性 | 可忽略，顺手清理 | 低 |
| P-13 | 每条用户消息派生和绘制时编译正则 | `frontend/Sources/Janus/Core/Attachments.swift:103,105` 用 `range(of:options:.regularExpression)` | 推理：每条消息每行 2 次编译；未单独测量 | 改成 `static let` 正则 | 小 | 低 |
| P-14 | 少量线性查找与重复排序 | `frontend/Sources/Janus/Core/Derive.swift:186` 的 `RunState.event`；`UI/Run/RunView.swift:766`；侧栏 `inbox.open` | 推理：标记数 × 事件数 | 建 seq 索引、一次排序、缓存 split 结果 | 小 | 低 |
| P-15 | 回放时实时批次仍让整页失效 | `UI/Run/RunView.swift:27,53,60`、`UI/Run/SharedBoard.swift:35,48,54` 读实时 `run.state` | 推理：每批一次整页失效，成本同 P-01；另有一处复制摘要按实时结果判断 | 回放路径统一读 `run.shown` | 回放时不再按实时批次重算 | 低：注意回放只读语义 |
| P-16 | 审计页每次重绘重算筛选、参与者与不变量 | `UI/Run/AuditView.swift:11-15,17,20,107-131` body 内过滤、收集参与者、跑不变量 | 实测：`auditBody`（rows+参与者）5.07ms/20,200；不变量未单独测量 | 不变量移入 derive，筛选按组合缓存 | 审计页每批省约 5ms | 低：等价性测试 |
| P-17 | 每批在主 actor 上增量扫描文档路径 | `Core/RunModel.swift:140-146,168`；`Core/RunDocs.swift:47,63-78` 逐事件正则 | 实测：全量扫描 485.8ms/20k；含 `.md` 文本约 0.49ms/2.4KB | 增量扫描移入后台 derive 任务 | 每批主线程省 0.5–2.5ms（含 `.md` 长文本时） | 低：`docScan` 归并 |

## 3. 事件流水线模型

| 步骤 | 位置与线程 | 复杂度 | 实测（合成 20k 日志） | 备注 |
| --- | --- | --- | --- | --- |
| ACP 更新 | 适配器子进程 stdout，由 tokio 任务读取（`backend/src/orchestrator/turn.rs:505-568`） | 每个 update 按其文本长度 | 单条 `tool.call` 平均 6,009B，`agent.message` 平均 2,751B | 文本和思考先累积，不逐 token 落库 |
| 事件写入 | `backend/src/orchestrator.rs:749-780`，tokio 工作线程 | O(1)，但每 Run 经 `emitting` 锁、全部 Run 经 `Db` 的 Mutex 串行 | INSERT 56.2µs/条（autocommit + FULL） | 先写库再广播；写失败不广播 |
| 广播 | `orchestrator.rs:657` 的 `broadcast::Sender`，容量 4096（生产 `backend/src/main.rs:518`；测试 `orchestrator.rs:916`） | 每个订阅者一次 clone | 未单独测 | 只有一个应用连接时正常 |
| SSE | `backend/src/main.rs:417-434`，axum 任务 | 回放积压逐条 `serde_json::to_string`，实时同样逐条 | 未单独测；`GET` 全量读 50k 为 85.7ms（`db.log`），之后还要序列化 | 消费慢导致 `lagged` 时应用整份重读 |
| 应用接收 | `frontend/Sources/Janus/Core/API.swift:96-113` 的生产者 `Task`，非主线程 | 每条一行，`JSON.parse` 一次 | 1KB 0.026ms、4KB 0.058ms、16KB 0.196ms | 解析不在主线程 |
| 主 actor 消费 | `Core/RunModel.swift:113-129`，`Task` 继承主 actor | 每事件 O(1)：`Event.init` 加 append | 200 条 `Event.init` 0.030ms，约 0.15µs/条 | 序号缺口或 `lagged` 触发整份重读 |
| 合并与 derive | `RunModel.swift:157-174`，60ms 后 `Task.detached` | `derive` O(n)，`usageLedger` O(n)，`boardFacts` O(n + R²H + A²) | derive 0.90/4.05/14.75/40.60ms，`usageLedger` 0.34/1.59/6.44/17.10ms，`boardFacts` 0.24/0.98/4.18/10.40ms（1k/5k/20k/50k） | `derive` 已包含 `usageLedger`（`Derive.swift:395`），账本约占四成 |
| 状态替换 | `RunModel.swift:167`，主 actor | O(1) 赋值，但使所有读 `state`/`shown` 的视图失效 | 一次四项组合纯计算 55.1ms/20k（下界） | 不比较新旧值，事件只增也会整页重来 |
| 视图 body | `UI/Run/RunView.swift`、`RunView.swift:214-243`、`UI/Run/Session.swift:62-107`、`SharedBoard.swift:15-45` | 每 body 多次 O(n) 过滤、O(n) 分组 | `agentSession` 2.71ms/6515；`chatGroups` 5.37ms/20k；`chatVisible` 2.10ms/20k；`toolHidden` 2.30ms/20k；`SessionView.usage` 0.065ms/6515 | 每次派生都会重跑 |
| 布局与绘制 | SwiftUI 内部，主线程 | 可见行重解析 Markdown、diff、代码块 | `MD.blocks`+`inline` 1.35ms/8KB；`InlineDiff` 4.37ms/2000 行 | 未用 Instruments 测真实帧时间 |

表中的 `derive`/`usageLedger` 是合成日志上的实测；8.1 的 session 建模让这两个绝对值偏高约 2 倍（20k 的真实估计约 6–8ms，验证者独立测得 6.3–6.6ms），两个独立二进制复跑得到可复现的数字（8.5），`boardFacts`、视图过滤和 SQLite 数字不受影响。

一次真实 Run 的事件率推算（依据上述映射代码，不是实测）：

- Agent 的文本和思考只在下一次工具调用、回合结束或 1 秒没有新内容时各写一条，单条上限 8,000 字符（`orchestrator.rs:297`、`orchestrator/turn.rs:334-347`、`turn.rs:589-596`）。Token 流本身不产生事件。
- 每个工具调用最多两条：开始时 `tool.started`，完成或失败时 `tool.call`；后者带输入摘要（≤240 字符）、输出摘要（≤300 字符）和每条 diff 的 old/new（各 ≤12,000 字节，`acp_view.rs:137`）。
- `agent.status` 在同一会话最多每 1.5 秒一条（`turn.rs:560-572`）；`session.usage` 每个 `usage_update` 一条（`acp_view.rs:78`），每回合结束再一条（`acp_view.rs:111`），仓库不节流 ACP 的 `usage_update`。
- `agent.plan`、`session.commands`、`session.controls` 只在内容变化时各一条（`acp_view.rs:32-75`）。

推算：一个持续工作的会话约 1–2 条/秒（状态 0.67 条/秒，文本至多 1 条/秒，工具 0.1–0.2 条/秒）；5 个会话并行时约 5–10 条/秒，30 分钟约 9,000–18,000 条。字节率用实测分布（平均 1,120B，p90 3,755B，p99 7,846B，最大 10,446B，见附录）推算为 5.6–11.2KB/s，30 分钟约 10–20MB，与实测的 20,003 条、22.4MB 同一量级。

关键结论：一批事件到达后，工作量几乎与批内条数无关，因为 derive 和视图都按全部事件重算；它只与「批的次数」有关，而批次上限是每秒 16.7 次。20k 事件时每次四项组合纯计算 55.1ms（合成日志实测，下界），扣掉后台 derive 14.7ms 后主线程约 40ms/批（推算）；两项都来自合成日志，derive/usageLedger 因 session 建模偏高约 2 倍，把 derive 校正到 6–8ms 后整页组合约 47–48ms、主线程部分仍约 40ms/批（均为推算），量级不变。16.7 批/秒即每秒约 0.67 秒的主线程计算（推算），主线程被占满，滚动和输入的每一帧只能排队。50k 时按实测线性外推约 130ms/批（推算），情况更差。

## 4. 逐项问题

### P-01 `state` 整体替换使整个工作台每 60 毫秒重算

严重度：高。这是持续卡顿的总开关，后面的 P-02、P-03、P-07、P-08 都由它放大成每帧成本。

证据：`Core/RunModel.swift:167` 在每批事件后执行 `self.state = derived`；`RunModel.swift:24` 的 `state` 是 `@Observable` 的普通存储属性，赋值不比较内容；`UI/Run/RunView.swift:27` 读 `run.state`，`UI/Run/RunView.swift:214` 的 `BoardView` 读 `run.shown`，再把 `st` 以值类型传给 `RunCard`、`InboxSection`、`ResultCard`、`ProgressBoard`、`ChatCard` 和 `RunInspector`（`RunView.swift:227-238`）。这些视图都不是 `Equatable`，父视图重算就重建子视图的 body。

机制：Observation 只按「读过的属性」登记依赖，不判断值是否相同。每个事件批次都把 `state` 换成一个全新的 `RunState`，即使这一批只追加了一条 `agent.status`，也等于告诉所有读 `state` 的视图「全部重来」。`RunState` 里还持有任务、消息、产物、用量账本和整份事件数组，子视图收到的 `st` 也不可能被 SwiftUI 跳过比较。

实测：`boardFrameWork` 在 20k 事件上把 `derive`、`boardFacts`、`agentSession(lead)` 与 `chatGroups` 四项串起来跑一遍（`main.swift:218-224`），3 次中位数 55.1ms（52.8–55.4ms）。这是「每次派生后整页要做的主要纯计算」的下界，不是整棵视图的全部成本，也不是可以相加的单项成本之和：同一档单独测得的 `derive` 为 14.7ms、`boardFacts` 4.2ms，而 `agentSession`/`chatGroups` 在这份日志上的成本只能由 55.1ms 减去前三项反推（约 36ms，推算；8.2 的 2.71/5.37ms 是在另一份会话日志上测的，不能直接搬用）。主线程实际承担的是除 derive 之外的部分，约 40ms/批（推算：55.1−14.7）。合成日志的 session 建模使 `derive`/`usageLedger` 偏高约 2 倍（8.1）；把 derive 校正到 6–8ms 后，整页四项组合约 47–48ms（推算：55.1 − (14.7 − 7)），主线程部分是「整页组合 − derive」，仍约 40ms/批（推算），量级不变。事件批次上限 16.7 次/秒，因此 20k 事件时主线程每秒最多被要求约 0.67 秒的重算（推算）。

影响：滚动（每次滚动都要先排队等 body 完成）、输入（每个按键触发 `ChatCard` 重算，见 P-02）、切页（`RunView` 重建）、回放拖动（P-06）。这解释了用户说的「一跑起来就卡」：不是某一个操作慢，而是主线程没有空档。

修复方案：把「每帧要用的数据」提前算成不可变快照，并让视图只在快照版本变化时重算。具体形态：`RunModel` 在后台 derive 的同一个 detached 任务里同时产出 `BoardSnapshot`（运行卡摘要、请求列表、交付、进展事实）和 `SessionSnapshot`（当前 target 的 `groups`、`PlanStrip` 步骤、controls、commands、usage），主线程只做 `state = derived` 与 `snapshot = snapshots`，视图读 `snapshot` 的窄字段。第一版可以不拆 `RunState`，先让 `ChatCard`、`ProgressBoard`、`RunInspector` 接收单独的、带版本号的快照，避免为无关字段重算。进一步可以把 `RunState` 拆成 run、tasks、inbox、usage、sessions 五个可观察对象，视图分别观察；但要在回放时对齐同一版本。

预期改善：20k 事件每批主线程工作从约 40ms（推算）降到 <10ms，50k 从约 120ms（推算）降到 <15ms；流式输出时恢复 60fps，滚动和输入不再与派生抢主线程。

风险与前置条件：改动面最大，必须先有「整页四项组合纯计算」的自动基准（现有 `perf` 的 `boardFrameWork` 可扩展成测试），并在每个快照上写「实时与回放取同一版本」的测试。快照若按事件数缓存，`reload` 时必须整体丢弃，不能与新的 `events` 数组混用。

### P-02 `ChatCard.body` 每次重建 `groups`，冲突判断是 O(冲突数 × 事件数)

严重度：高。会话越长越贵，而且输入也会触发，是「滚动和输入卡」的直接来源之一。

证据：`UI/Run/Session.swift:62-107` 的 body 在第 89 行把计算属性 `groups` 传给 `ChatScroll`；`groups` 在 `Session.swift:176-203` 每次遍历 `info.evs`（整段会话事件），先在第 181 行用一次全量过滤建 `done` 集合，再逐条调用 `ToolRow.hidden`（`Session.swift:564-570`）和 `ChatItem.visible`（`Session.swift:397-419`）。`ChatItem.visible` 对 `task.conflict` 在 `Session.swift:407-409` 执行 `st.events.contains { ... }`，每个冲突事件扫全部事件。输入路径：`ComposerField` 每次按键写绑定（`UI/Components.swift:437-441`），`ChatCard.body` 在按钮禁用判断里读 `draft`（`Session.swift:145`、`Session.swift:153`），于是每次按键都让 `groups` 重算一遍。

机制：`info.evs` 由 `taskSession`/`agentSession` 每次过滤全部事件得到（`Session.swift:26-45`），`groups` 再对它做两次遍历并把结果作为新数组交给 `ChatScroll`；`ForEach(groups)` 即使 id 稳定，也要对整份分组数组做一次 diff，可见行的 `ChatItem`/`ToolRow` 会重新求值，长 Markdown 气泡随之重解析（P-07）。冲突分支没有索引，200 个冲突就是 200 次全表扫描。

实测：20k 会话事件、normal 档 `chatGroups` 5.37ms（detail 5.39ms、summary 5.56ms）；`chatVisible` 2.10ms、`toolHidden` 2.30ms（这两项包含在 `groups` 的工作里，单独测量用于定位）；200 个 `task.conflict`、20,200 条事件时 `chatVisible` 42.2ms，同样本 0 个冲突时 2.10ms。42.2ms 依赖这份合成日志的数据形状：它在 20,000 条会话事件后追加 200 条 `task.conflict`，每条都让 `ChatItem.visible` 用 `contains` 扫全部事件，并且冲突覆盖同一组文件；真实 Run 的冲突数通常更少、覆盖关系更窄，收益按冲突数缩小，不能用 42.2ms 代表所有 Run。

影响：长会话滚动、在会话里打字、切换显示档位、打开子任务页和 Agent 页（两处都用同一个 `ChatCard`）。

修复方案：两层。第一层把 `groups` 记忆化：在 `ChatCard` 用 `@Local` 保存 `(事件数, density, 最后一个 seq)` 与分组结果，只有 `info.evs.count` 或 `density` 变化才重算；`sent`、`draft`、`images` 这些输入状态变化不重算分组。第二层把冲突覆盖关系从「每个冲突扫一遍」改成一次建表：在构建 `groups` 时先扫一遍会话事件，把 `(task, other, paths)` 为键的越界冲突放进 `Set`，之后 `ChatItem.visible` 只查表；更好的位置是 `boardFacts` 已经收集了 `conflicts`（`Derive.swift:495`），把覆盖结果随 `BoardFacts` 一起返回。更彻底的做法是在后台 derive 时按 `SessionTarget` 预先分桶事件并生成分组，视图只消费。

预期改善：20k 会话的 `groups` 从 5.37ms 降到 <1ms；200 个冲突的 `chatVisible` 从 42.2ms 降到 <3ms（与 0 冲突时同量级）；打字不再触发全量分组。

风险与前置条件：记忆化的键必须包含 `density` 和事件数（事件只追加，用 count 加最后 seq 足够）；回放切换时 `info.evs` 来源换成 `replayState`，键要跟着换，否则会显示实时会话的分组。冲突覆盖表的语义必须与现在逐条扫描完全一致，用现有会话测试加一个「同一批文件被两个冲突覆盖」的用例。

### P-03 `ProgressBoard` 折叠时也计算 `boardFacts`

严重度：中高。它出现在工作台每一帧，哪怕进展时间线是收起的。

证据：`UI/Run/SharedBoard.swift:15-16` 的 body 先执行 `let f = boardFacts(st) { run.who($0).name }`，第 20 行才判断 `!tasks.isEmpty || !milestones.isEmpty`，第 33 行才判断 `run.progressOpen` 是否展开卡片。也就是说折叠状态只影响时间线是否绘制，不影响 `boardFacts`。

机制：`boardFacts` 从头遍历全部事件，并在 `Derive.swift:547-558` 做 results × handoffs × results 的嵌套过滤和 artifacts 的 `contains` 双重循环；它还在 `Derive.swift:604-606` 为每个冲突重新构造成员集合。展开时才需要的结论、产物、交接、决定，在折叠时全部算完就丢。

实测：`boardFacts` 在 20k 事件 4.18ms（5 次中位数），dense 配置（每 20 条事件就有一条 result/handoff/artifact）16.0ms（3 次中位数）；1k/5k/50k 分别为 0.24/0.98/10.40ms。它是 `boardFrameWork` 55.1ms 的组成部分。

影响：工作台滚动、任意输入、每次事件批次；折叠状态下用户看不到任何收益，却持续付费。

修复方案：在后台 derive 的同一个 detached 任务里计算 `BoardFacts`（它只依赖 `RunState` 与名字映射，是纯函数），随 `state` 一起赋给 `RunModel`；`ProgressBoard` 只读 `run.facts`，折叠时连 `timelineTasks` 和 `milestoneMarks` 也不执行（把这两个函数移到 `progressOpen` 分支内）。`Inbox.entries` 已经自己调用 `boardFacts`（`Inbox.swift:37`），改为复用同一份缓存。

预期改善：每个批次省 4.2ms（realistic）到 16ms（dense），20k 档的整页成本从 55.1ms 降到约 51ms，重点是 dense 会话从约 67ms 降到约 51ms（推算）。

风险与前置条件：`boardFacts` 依赖名字闭包 `run.who`，而 `who` 读 `run.team`（来自 `runJSON`）和 `store.connection`；在后台线程计算时不能触碰主 actor 的 `RunModel`，必须把 team 和连接名先取成普通字典传进去。回放时用 `replayState` 计算，实时与回放不能共用缓存键。

### P-04 收件箱每 5 秒在主 actor 上全量 derive

严重度：高。它造成的是规律性的周期停顿，与事件流无关；开着的 Run 越多越明显。

证据：`Core/Inbox.swift:99-130` 的 `refresh` 是 `@MainActor`（`Inbox.swift:79-80`）；对每个进行中的 Run，它在第 110 行用 `?after=` 拉新事件，第 111 行在主 actor 上 `map(Event.init)`，第 113-116 行在「有新事件」时执行 `c.state = derive(c.events)`，第 118 行在 `Inbox.entries` 里再跑一次 `boardFacts`。轮询由 `Core/AppStore.swift:149-158` 每 5 秒触发，每次先 `loadRuns` 再 `inbox.refresh`。

机制：`cache` 里保存每个进行中 Run 累计的全部事件，`derive` 从头重算，不做增量；`derive` 还是串行地在主 actor 上执行，期间主线程不能绘制或响应输入。`boardFacts` 紧接着再遍历一遍。收件箱页和 Run 页各自维护一份事件数组和派生结果，同一批事件被算了两遍。

实测：`derive` 14.75ms/20k、40.60ms/50k（合成日志；session 建模使这两个值偏高约 2 倍，真实估计约 6–8ms/20k，见 8.1）；`boardFacts` 4.18ms/20k、10.40ms/50k。按合成日志的数字，单个 20k 事件的活动 Run 每 5 秒约 19ms 主线程，3 个这样的 Run 约 57ms（推算，按线性相加），50k 时单个约 51ms（实测 derive 40.6ms 加 `boardFacts` 10.4ms）；按校正后的 derive 估算，单个 20k Run 约 11–12ms、3 个约 34–36ms、单个 50k Run 约 26ms（推算：由 20k 实测按事件数线性外推到 50k，再加 `boardFacts`）。

影响：所有页面（不只是收件箱）每 5 秒出现一次可感知的停顿；如果此时正在流式输出，批次还会叠加，表现为「每隔几秒猛地一顿」。用户开着多个任务时更重。

修复方案：把每个 Run 的 `derive` 和 `boardFacts` 放进 `Task.detached`，主 actor 只负责 `map(Event.init)`、合并缓存和比较结果；用 `(run id, 最后 seq)` 作为任务键，避免同一 Run 并发派生。更省的做法是把 `RunModel` 的派生结果按 `(run id, seq)` 共享给收件箱：Run 页打开时不再重复算，Run 页没开时收件箱才算。缓存结果按 Run 分开，不合并成一个大字典。

预期改善：3 个 20k Run 的 5 秒刷新从约 57ms 主线程（合成日志口径）降到 <16ms；单个 50k Run 从约 51ms 降到 <16ms。

风险与前置条件：派生结果从后台交回时必须核对「这期间是否又有新事件」；不一致就丢弃这一轮，避免把旧状态盖到新事件上。收件箱与 Run 页「需要你处理」必须一致（`docs/architecture/20261031-frontend.md#inbox`），共享缓存要以同一份 `derive` 输出为准，不能各自实现一套。

### P-05 打开、切回、断线重连时主线程 derive 加全量文档扫描

严重度：高。单次卡顿最长，直接对应「切页卡住」。

证据：`Core/RunModel.swift:81-103` 的 `load` 是主 actor 方法：第 86 行 `events = data["events"].array.map(Event.init)`，第 93 行 `state = derive(events)`，第 94 行 `findDocuments()`。`findDocuments`（`RunModel.swift:140-147`）在首次加载时对全部事件调用 `RunDocs.scan`，再对每个候选调用 `RunDocs.existing` 的 `FileManager.default.fileExists`（`Core/RunDocs.swift:52`）。`AppStore.swift:174-176` 在从别的页面回到已经打开的 Run 时也重新 `load()`；`RunModel.swift:118-119`、`RunModel.swift:133-136` 在 `lagged`、序号缺口和流结束时同样走 `reload`。

机制：响应解析在非隔离的 `API.send` 里完成（`API.swift:75-86`），这一段已经在后台，但 21.7MB 的 `JSONSerialization` 仍要 447.8ms，用户要等它完成后才看得到内容；解析完把 JSON 数组交给主 actor 后，`Event.init` 的 map（4.6ms/20k）和 `derive`（14.7ms/20k，合成日志）都在主线程；`RunDocs.scan` 还要逐条提取事件文本里的 Markdown 路径（`session.prompt`、`agent.status` 除外，`RunDocs.swift:63-78`），20k 事件实测 0.49 秒（`scan.log` 的 realistic 日志；`misc.log` 另一份 20,200 条会话事件集为 0.76 秒，两份事件集文本长度不同，不能合并成一个区间）。主线程上的 map、derive 和文档扫描串起来，打开一个长 Run 就是一次 0.5 秒以上的主线程停顿，解析时间另算。

实测：`loadParse` 447.8ms/20k（21.7MB）、1124.0ms/50k（55.2MB）；`loadMapEvents` 4.6/11.1ms；`loadParse+Map+Derive` 464.5/1176.3ms。`runDocsScan` 在 2k/5k/20k 为 50.7/124.4/485.8ms（`scan.log` 的 realistic 日志），另一份 20,200 条会话事件集为 755.8ms（`misc.log`，文本更长，不可直接比较）；`RunDocs.paths` 对一段 2.4KB、含 `.md` 的文本调用 1000 次为 491.5ms，即约 0.49ms/次。`derive` 14.75/40.60ms（合成日志；session 建模使 20k 绝对值偏高约 2 倍，真实估计约 6–8ms，见 8.1）。

影响：打开 Run、从收件箱或首页切回 Run、事件流断开 2 秒后的重连、任何序号缺口触发的整份重读；长 Run 上表现为数秒无响应或转圈。

修复方案：`load` 只把网络读取和解析留在后台，`map(Event.init)`、`derive` 和 `RunDocs.scan` 全部放进 `Task.detached`，主 actor 拿到结果后一次性赋值；`RunDocs.existing` 的 `fileExists` 也放到后台。重连路径先尝试用 `GET /api/runs/{id}?after=<最后 seq>` 只取尾部：序号连续时直接追加，不重读整份；只有确认缺口时才退回全量。服务端已经支持 `after`（`backend/src/main.rs:333-337`），SSE 也支持从 `after` 回放（`main.rs:417-434`），不需要新接口。

预期改善：打开/切回 20k Run 的主线程停顿从约 0.5–0.8 秒降到 <100ms；50k 从 1.2 秒以上降到 <150ms；重连在正常情况下只传输并解析尾部几条事件。

风险与前置条件：`findDocuments` 依赖 `state.run.cwd`，后台计算前要先把 cwd 取出来；文档菜单在加载完成前保持旧列表或空列表，不能显示上一次的路径。增量重连必须证明序号连续（`events.last.seq + 1`），否则仍走全量，正确性不能被性能换掉。全量读取的响应大小（50k 为 55.2MB）不变时，解析成本仍在后台，用户只感知主线程停顿变短，网络与内存峰值不变。

### P-06 回放在主线程同步 derive，拖动滑块每格一次全量派生

严重度：中高。回放是审计页的主要操作，长 Run 上拖动会明显粘滞。

证据：`Core/RunModel.swift:26` 的 `replay` 属性 `didSet` 直接执行 `replayState = replay.map { n in derive(Array(events.prefix(n))) }`，`didSet` 在赋值线上同步运行，调用方是主 actor；`UI/Run/RunView.swift:195` 的 `Slider` 每产生一个值就写 `run.replay`，没有节流。

机制：`Array(events.prefix(n))` 复制前缀数组（O(n) 的引用复制），`derive(prefix)` 再从头派生到第 n 条。拖动过程中每个滑块步进都做一次，并且随后触发 P-01 的整页重算。也就是说每格的成本是「derive 加整页 body」，而不是一帧能做的工作。

实测：`replayPrefix`（前缀复制加 derive）在 5k/20k/50k 为 3.62/14.75/38.88ms（合成日志；derive 部分因 session 建模偏高约 2 倍，20k 校正后约 7–8ms，见 8.1）；整页四项组合纯计算在 20k 为 55.1ms。50k 拖动一格约 150–170ms（推算，合成日志口径：38.9ms 派生加整页成本外推约 130ms），实际拖动时连续触发会排队。

影响：审计与回放页的滑块、左右箭头键（`RunView.swift:193-199`）、边拖边看时间线的交互。

修复方案：`replay` 只保存位置，派生异步执行：写入后启动一个带代际号的 detached 任务，任务完成时若代际仍是最新才赋 `replayState`；拖动期间用 16–33ms 合并（与 derive 的 60ms 批处理分开，回放要求更跟手）。回放位置本身（`ReplayBar` 的文案、滑块拇指）仍然立即更新，只有右侧内容等派生结果。进一步可以在一个固定位置的 `derive` 上做增量：只回退到新位置之前的检查点再从那里前进，但要保证与全量 derive 结果逐字段相同。

预期改善：每格主线程工作 <16ms，50k 拖动不再出现 100ms 级以上的主线程卡顿。

风险与前置条件：回放正确性依赖「前 N 条事件的 derive」与全量结果一致，这是现有行为，改动只改变计算时机，不能改变结果；异步赋值要防止旧任务覆盖新位置（代际号或哈希位置）。`replay = nil`（回到实时）时取消在途任务，继续使用实时 `state`。

### P-07 文本与 Markdown 在每次 body 重算时重新解析

严重度：中高。会话越长、可见消息越长，每帧越贵；滚动时每个新进入视口的行都要解析。

证据：`UI/Markdown.swift:70-97` 的 `MarkdownText.body` 第 76 行执行 `MD.blocks(text)`，每个段落再执行 `MD.inline(p)`；`UI/Run/Session.swift:434-440` 的 `agent.message` 分支每次 body 都拼接 `[e] + more`、用正则去掉尾部 JSON 块并调用 `MarkdownText`；`Session.swift:579` 的 `ToolRow.body` 无条件执行 `d["args"].or(d["input"]).pretty()`，即使 `DisclosureGroup` 没有展开；`UI/Run/SessionParts.swift:133-135` 的 `InlineDiff.body` 每次 body 执行 `Self.lines(old:new:)`（只有展开时才会绘制，但展开后每帧都重算）；`RunView.swift:423`、`TaskView.swift:98,116`、`ResultCard` 也在 body 里调用 `MD.inline`。

机制：这些解析都是纯函数但没有缓存，视图求值一次就解析一次。`MD.blocks` 逐行做两个正则加一个标题正则，`MD.inline` 对整个字符串做一次多分支匹配并构造 `AttributedString`；`InlineDiff.lines` 用 `CollectionDifference` 求行差异，再为每个改动行保留前后 3 行，成本随行数增长，长文件上可能超线性；`pretty()` 用 `JSONSerialization` 重新序列化一遍参数。

实测：`MD.blocks` 8KB 0.86ms、`MD.inline` 单段 0.079ms、两者合计 1.35ms/8KB；`InlineDiff.lines` 200/1000/2000 行 0.57/2.73/4.37ms；`JSON.pretty` 2KB 0.036ms、16KB 0.165ms。20k 事件里 `agent.message` 共 3,826 条、10.5MB，占全部字节的 47%，平均 2.75KB；`tool.call` 1,077 条、6.5MB，平均 6.0KB。

影响：滚动长会话、在会话里输入（P-02 触发整段重算）、展开长 diff、显示交付总结和任务结果；同时也是 P-01 每帧成本的一部分。

修复方案：给解析结果加按事件 seq（加文本哈希校验）的小缓存，放在视图外的模型中，例如 `ChatCard` 持有一个 `[Int: [MD.Block]]`，容量按最近可见的几百条限制；或者更简单：在后台派生新事件时把 `agent.message` 的 `blocks` 预先算好，随分组一起交给视图，视图只做 `Text(attributed)`。`ToolRow` 的 `full` 移进 `DisclosureGroup` 的内容闭包，`InlineDiff.lines` 用 `@Local` 懒计算，只在首次展开时求值。`MarkdownText` 对外仍接受原始字符串，缓存对它透明。

预期改善：5 条 8KB 可见消息的每帧解析从约 6.8ms 降到 <0.5ms（缓存命中）；展开 2000 行 diff 后每帧不再重算 4.37ms；折叠的工具行不再序列化参数。

风险与前置条件：缓存键必须包含文本内容（同一条事件在回放中被替换时 seq 可能相同，`reload` 后事件对象会重建），用 `(seq, text.count, text.hashValue)` 或直接以文本为键；缓存不能影响 `MD` 的输出，`MarkdownTests` 之类的现有测试照跑。缓存只增不减会吃内存，按可见窗口淘汰。

### P-08 会话与页面级全量过滤在每次 body 重跑

严重度：中高。它不单独造成一次长卡顿，但乘上每 60 毫秒的批次，就是持续的固定税。

证据：`UI/Run/Session.swift:26-45` 的 `taskSession`/`agentSession` 每次调用都从 `st.events` 起过滤；`ChatCard` 在 `RunView.swift:234`、`TaskView.swift:50`、`Session.swift:681` 分别被调用，`BoardView` 每帧都重新算 `agentSession(st, "lead")`（`RunView.swift:218`）；`agentSession` 还在 `Session.swift:35-41` 对过滤结果再遍历一遍推导 human/idle。`UI/Run/TaskView.swift:24` 每次 body 从全部事件过滤出该任务的事件；`UI/Run/RunView.swift:562` 的每个成员行先对 `st.orderedTasks` 过滤一次，第 582 行对非负责人调用 `runStats`；`runStats` 在 `RunView.swift:617` 对每个已结束任务执行 `st.events.filter`（`first` 与 `vfail` 各调用一次 `vf`），所以团队页的成本是「成员数 × 已结束任务数 × 2 次全事件过滤」，不是外层任务循环套成员循环。

机制：这些函数都被写在 `body` 或 `body` 直接调用的计算属性里，没有按 `(事件数, target)` 缓存；会话事件本来在后台 derive 时已经完整走过一遍，视图再走一遍没有复用。`ChatCard.body` 还重复计算 `SessionView.plan/usage/controls/commands`（`Session.swift:70,88,95,98`），其中 `usage` 每次遍历整段会话。

实测：6515 条会话事件时 `taskSession` 0.78ms、`agentSession(lead)` 2.71ms、`SessionView.usage` 0.065ms、`plan` 0.050ms、`controls` 0.001ms、`commands` <0.001ms；20k 会话时按线性外推 `agentSession` 约 8ms（推算）。`runStatsScan`（6 个任务 × 4 个成员）4.00ms/20,200；`taskViewFilter` 0.85ms/20,200；审计页 body 5.07ms/20,200。

影响：工作台负责人会话每帧、子任务页和 Agent 页每次重绘、团队页的 Agent 面板（`runStats`）、审计页筛选。

修复方案：在后台 derive 完成时，按 `SessionTarget` 建立事件分桶（`(target, seq)` 列表或区间），并把 `plan/usage/controls/commands` 的最终值一起算好，随快照交给视图；多个视图读同一个会话时共享一份。`runStats` 改在 `boardFacts`/derive 阶段按成员汇总一次，视图只读数字。`TaskView` 的 `evs` 也放到任务快照里。分桶只保存事件引用和索引，不复制事件内容。

预期改善：`agentSession` 每帧 2.7ms/6515（20k 约 8ms）降到 <0.5ms；团队页 `runStats` 4.0ms 降到 <0.1ms；子任务页与审计页的固定过滤税消失。

风险与前置条件：分桶必须严格等价于现在的过滤条件（`task`、`data.other`、`data.target`、`data.tasks`、`data.unlocked` 等），需要把现有过滤函数保留为定义并在测试里比对分桶结果。回放时用回放前缀重新分桶，不能复用实时桶。

### P-09 后端每条事件一次 autocommit 和 fsync，广播逐订阅者序列化

严重度：中。它不是界面卡顿的直接来源，但会让事件到达变慢、放大 `lagged` 和整份重读的概率；在多个 Run 并行时更明显。

证据：`backend/src/db.rs:183-200` 的 `insert_event` 用 `Connection::execute` 单条插入，没有显式事务；`db.rs:80-105` 只设置 `journal_mode=WAL` 和 `foreign_keys=ON`，没有设置 `synchronous`，SQLite 默认是 `FULL`，WAL 下每次提交都要 fsync。`backend/src/orchestrator.rs:749-770` 的 `emit` 持有 `run.emitting`（`orchestrator.rs:502-504`）完成取号、写库和广播；`db.rs:31` 的 `Db(Mutex<Connection>)` 让所有 Run 的写入也串行。`backend/src/main.rs:426` 的 `to_sse` 对每个事件、每个订阅者执行一次 `serde_json::to_string`；`main.rs:424-434` 的 backlog 也要逐条序列化。

机制：每个事件一次提交意味着一次 fsync，写入线程（tokio worker）在 `emitting` 锁内阻塞；同一 Run 的多个会话只能排队。广播是同步 clone，订阅者慢不会阻塞生产者（broadcast 有界缓冲），但缓冲满后消费者会收到 `lagged`，应用随即整份重读（P-05），形成「越慢越重读」的放大。

实测：单条 INSERT 在 autocommit + `synchronous=FULL` 下 56.2µs，`NORMAL` 下 28.2µs，单事务批量 6.6µs/条（1,000 条总耗时 56.2/28.2/6.6ms）；读全量 10k 为 17.2ms（20.1MB）、50k 为 85.7ms（100.4MB），增量读 `seq >` 0.13ms（`read_after_100`）。事件条数与字节率见第 3 节推算。

影响：事件从产生到出现在界面上的延迟；高频工具调用时同一 Run 的写入排队；`lagged` 后的一次整份重读（与 P-05 叠加）。对帧率没有直接影响。

修复方案：`PRAGMA synchronous=NORMAL`（WAL 下只在检查点同步，掉电可能丢失最后几个事务，但不会损坏数据库）；按一次 ACP 回合或一次批量更新把事件收进一个事务提交；广播在事务提交后进行一次。`emitting` 锁保留（序号顺序和广播顺序是正确性要求），但锁内只做内存操作和一次批量提交。若实测显示单事件提交已足够快，可以只改 `synchronous`，把批量留给后续。

预期改善：单条写入从 56.2µs 降到 28.2µs（−50%），批量到 6.6µs（−88%）；高频回合的写入排队时间同比缩短，`lagged` 概率下降。

风险与前置条件：`synchronous=NORMAL` 是持久性取舍，需要写进 ADR 或安全/运维文档的已接受风险；事件是唯一事实来源，不能在提交前广播（现有 `store` 的顺序必须保留，J-04 的测试 `an_event_that_was_not_written_is_not_broadcast` 必须继续通过）。批量提交要保证任一事件失败时整批不广播，不能出现「部分写入、部分广播」。

批量提交与现有的序号分配、校验直接冲突，必须先统一序号分配再实施。`backend/src/orchestrator/actions.rs:37-41` 的 `expectedSequence` 通过 `db.max_seq` 读取已落库的最大序号来校验人的操作；同一文件的 `accept_archived`（`actions.rs:299`）和 `recover`（`actions.rs:626`）直接以 `db.max_seq + 1` 分配 `risk.accepted`、`run.interrupted` 等事件的序号；正常 Agent 事件的序号则来自 `Run.seq`（`orchestrator.rs:750-751`，受 `emitting` 锁保护）。批量窗口内事件还留在内存、没有提交时，这三条路径互相看不见对方：`expectedSequence` 读到旧的最大序号，会错误接受或拒绝操作（409）；`max_seq + 1` 可能与批量中尚未提交的序号重复，写入时撞上事件表的主键或产生乱序；`Db` 是单连接 `Mutex<Connection>`（`backend/src/db.rs:31`），批量事务持锁期间，`max_seq`、`events` 读取（收件箱的 `after` 拉取、SSE 回放）都会被阻塞。前置条件是：把序号分配收敛到一个所有者（例如仍由 `Run.emitting` 下的计数器分配，批量只合并写库与广播，`expectedSequence`、`accept_archived`、`recover` 都经同一所有者取号），并补「批量进行中到达 expectedSequence」「并发的 accept_archived 与批量提交」「批量失败回滚后再次取号」三类测试，再实施 Q9/S5。

### P-10 滚动与布局：每帧几何回调、最多 8 次 landing、下一轮主循环再滚动

严重度：中。回调本身不贵，问题在于它与 P-01/P-02 抢同一根主线程；最近两个提交说明这块仍在收敛。

证据：`UI/Run/Session.swift:236-239` 的 `onScrollPhaseChange` 每帧更新 `person`；`Session.swift:242-251` 的 `onScrollGeometryChange` 每帧计算 `ChatGeometry`，调用 `update`（只有结果变化时才写 `follow`，`Session.swift:302-306`），需要时 `DispatchQueue.main.async { position.scrollTo(y: g.end) }`；`Session.swift:339-362` 的 `measured` 最多允许 8 次 landing（`landingTries = 8`），到不了就放弃跟随；`Session.swift:254-256` 在新回复、发送、切换档位时 `toEnd()`。

机制：`.onScrollGeometryChange` 在滚动和内容尺寸变化时都会回调，回调里虽然只做少量状态比较，但布局测量本身要把未构建的惰性行估算出来；`groups` 每帧是新数组（P-02），`ForEach` 要重新 diff，`LazyVStack` 可能在滚动中重新测量，macOS 15 上 SwiftUI 测量时还会移动偏移（提交 aa161d0 的说明），所以需要多次 `scrollTo` 纠正。这些工作在主线程已经接近饱和时只能排队，手感就是「拖不动、回不到底」。

实测：没有单独测量回调耗时；`ChatFollow.measured` 的赋值有相等性判断（`Session.swift:302-306`），推测单次回调在微秒级（推理）。真正可测的是 P-01 的整页成本和 P-02 的分组成本。两个提交的背景可以直接引用：`7d89402` 把纠正滚动从布局回调里移到下一轮主循环，`aa161d0` 改用 `onScrollGeometryChange`/`onScrollPhaseChange` 并允许 8 次 landing。

影响：长会话滚动、新回复跟随、发送后回到底部、切换显示档位；在 macOS 15 上更容易出现「差一点到底」。

修复方案：本项不先做结构性改动。先修 P-01/P-02，让每帧 body 回到预算内；随后给 `onScrollGeometryChange` 和 `scrollTo` 加计数器（debug 构建），验证一次滚动期间的回调次数和滚动次数符合预期；如果仍粘滞，再把 `groups` 的 diff 成本降下来（P-02），并考虑在滚动期间暂停「跟随」的自动滚动，只在手势结束后追一次（保持现有的 `ChatFollow` 语义）。

预期改善：滚动回调保持 <1ms；滚动期间不再与全量重算争主线程，长会话拖动恢复顺滑；「回到最新」的 8 次上限保留作为兜底。

风险与前置条件：不能为了性能去掉「停在底部时跟随新内容」「往上翻不拉回」的行为，这些是已写进 `docs/architecture/20261031-frontend.md#session_view` 的交互约定；任何滚动策略的改动都要在 macOS 15 上跑窗口测试（`ChatScrollTests`）。

### P-11 每批派生后重复做文档候选的文件检查

严重度：中低。单次很便宜，但每 60 毫秒一次、没有变化也照做。

证据：`Core/RunModel.swift:168` 在每次 derive 后调用 `findDocuments()`；`RunModel.swift:140-147` 在 `docScanned < events.count` 时增量扫描新事件，然后无条件执行 `let found = RunDocs.existing(docScan)`，`Core/RunDocs.swift:52-54` 对每个候选调用 `fileExists`，再排序、比较、必要时赋值。`RunDocs.existing` 没有按 `docScan` 的版本缓存。

机制：`docScan` 是 `[路径: Doc]` 的候选集合，已找到的候选不会因为新事件而消失，但 `existing` 每次都要为每个候选做一次 stat 并重新排序。候选数随 Run 变长而增长（文档里提到 `.md` 的每一处都会成为候选），在网络卷或慢磁盘上，stat 会明显变慢。`findDocuments` 在主 actor 上，因此这些系统调用的延迟直接加在批处理的尾部。

实测：全量 `RunDocs.scan` 2k/5k/20k 为 50.7/124.4/485.8ms；同一批事件里 `RunDocs.paths` 对 2.4KB 文本调用 1000 次 491.5ms，而文本不含 `.md` 时 1000 次仅 54.3ms（`pathsGuard`），说明主要成本在路径正则和后续处理，不在空串快速返回。每批的 `existing` 成本没有单独测量，按候选数 × stat 推算（推理）。

影响：工作台每个事件批次；打开长 Run 的首次扫描（已在 P-05）；文档菜单的更新及时性。

修复方案：给 `docScan` 记录版本（例如「已扫描到第几条」），`existing` 只在扫描过新事件或文档候选集合变化时执行；其余批次直接复用上一次的 `documents`。`fileExists` 与排序移到后台，结果按版本回主线程。文档菜单不必每 60 毫秒都精确：文件是否存在可以在打开菜单时再确认一次，但不要把逐批检查当成实时性要求。

预期改善：稳态下每批 0 次文件系统调用；打开长 Run 的扫描从主线程移除后，单批不再有尾部系统调用。

风险与前置条件：Agent 可能在两批之间创建或删除 Markdown 文件，缓存会让文档菜单短暂过期；这本来就不是实时功能（`docs/architecture/20261031-frontend.md#markdown_viewer` 允许重新读取），但要在「打开菜单」和「重新读取 Run」时刷新。缓存键必须包含 `cwd`，切换目录后旧的绝对路径不能复用。

### P-12 侧栏与菜单栏每次求值都重新 split 和排序收件箱

严重度：低。量级很小，但属于「每次绘制都做可以缓存的事」。

证据：`Core/Inbox.swift:96-97` 的 `open` 和 `later` 是计算属性，每次读取都调用 `Inbox.split`（`Inbox.swift:68-73`，内部两次 filter 加两次 sort）；`UI/MainView.swift:73` 在侧栏的每一行任务里执行 `store.inbox.open.filter { $0.run == ... }`，`MainView.swift:276` 和 `App.swift:36,102` 的菜单栏也读 `store.inbox.open.count`。

机制：收件箱条目通常只有几十条，排序成本是几十微秒；问题不在绝对值，而在它被放在每一行的 body 里，行数（默认 10，展开更多）乘以条目数。菜单栏在每次刷新时也重算一遍。当前条目量下用户几乎感觉不到。

实测：未单独测量，按条目数 × 行数和排序复杂度推算为几十 µs（推理）。

影响：侧栏绘制、菜单栏数量更新；与 P-04 的 5 秒刷新叠加时更明显一点。

修复方案：`refresh` 结束时把 `open`、`later` 和按 Run 分组的计数算好存成属性，视图只读；`InboxModel` 已经有「内容不变不赋值」的守卫（`Inbox.swift:123`），把派生结果一起纳入即可。

预期改善：侧栏每行不再 split 和排序；菜单栏数量读取 O(1)。总体是顺手的清理，不会单独改变用户感受。

风险与前置条件：`now` 每次刷新都会变（`Inbox.swift:124`），推迟到期的条目要从「稍后」移回「需要你处理」，分组结果必须跟着 `now` 和 `snoozed` 重算；缓存不能跨刷新复用旧 `now`。

### P-13 每条用户消息派生和绘制时编译正则

严重度：低。属于既有约束的偏离，不是主要瓶颈。

证据：`Core/Attachments.swift:99-110` 的 `visibleText` 对每一行执行两次 `range(of:options:.regularExpression)`（`^\[附图 \d+\] ` 和 `^用户发来了 \d+ 张图片，请先查看：$`）；它被 `Core/Derive.swift:534` 的 `boardFacts`（每条 `user.message`）和 `UI/Run/Session.swift:530` 的 `userBubble` 调用。`Core/Attachments.swift:94` 的 `takesText` 也用了 `.regularExpression`，但只在粘贴时执行。`docs/architecture/20261031-frontend.md` 的「实现约束」明确要求逐条事件的处理中不用 `String.range(of:options:.regularExpression)`，正则要用 `regex(_:)` 写成 `static let`。

机制：`String.range(of:options:.regularExpression)` 每次调用都编译一次模式；消息行数越多、消息越多，编译次数线性增长。文本很短，单次成本小，但在 `boardFacts` 里对每条 `user.message` 都执行，并且 `boardFacts` 可能每帧运行（P-03）或在收件箱刷新时运行（P-04）。

实测：未单独测量；`pathsGuard` 显示 1000 次短文本正则匹配约 54ms（含 `RunDocs.paths` 的其他工作），可作为量级参考。按每条消息 1–3 行、每行 2 次编译推算，几十条消息是毫秒级以下（推理）。

影响：有大量用户消息的长会话，在 P-03/P-04 的路径上叠加。

修复方案：在 `Attachments` 里加两个 `private static let` 正则（用 `Core/Format.swift:6` 的 `regex(_:)`），`visibleText` 改用 `found(in:)`；`takesText` 同样处理。`MarkdownDoc.swift` 的逐行 `.regularExpression`（第 37、52、67、95、96、160、170、173、199 行）只服务文档窗口，不在 Run 页的热路径上，可以顺手改，但不属于本次卡顿的主因。

预期改善：每条用户消息的派生和绘制不再编译正则；对帧率的影响很小，但消除了一个明确的约束偏离。

风险与前置条件：正则语义必须完全一致（`^` 锚定、`\d+`、结尾 `$`），用现有附件测试和一条带附图消息的派生测试兜底；`regex(_:)` 在模式写错时会 `try!` 崩溃，新增模式要在启动测试里被覆盖。

### P-14 少量线性查找和重复排序

严重度：低。当前数据量下影响很小，但都是「事件一多就变差」的形状。

证据：`Core/Derive.swift:186` 的 `RunState.event(_:)` 在序号对不上时执行 `events.first { $0.seq == seq }`，是 O(n)；`ProgressBoard.milestoneMarks`（`SharedBoard.swift:91`）对每个待处理事项调用它，`timelineTasks`（`SharedBoard.swift:77`）对每个产物调用它；`UI/Run/RunView.swift:766-769` 的 `latest` 每次 body 排序全部产物并去重；`UI/Run/RunView.swift:669-670` 每 body 过滤两次 artifacts；侧栏 `Inbox.split` 见 P-12。

机制：事件数组按 seq 递增，`events[seq - 1]` 只在「数组从 1 开始且无缺口」时成立，所以代码保留了线性回退。大多数情况下第一个条件成立（`event(_:)` 先试下标），但回放前缀和重新加载后的数组仍满足这个条件；真正的风险是将来出现缺口时静默退化。`latest` 的排序是 O(A log A)，每帧执行。

实测：未单独测量；按标记数 × 事件数推算（推理）。20k 事件、几十个标记时 <1ms。

影响：进展时间线展开时、检查器每帧、审计页跳转。

修复方案：在 `RunState` 建一个 `seq -> index` 的字典或直接用「从 1 连续」的不变量加二分，保留线性兜底但只在必要时触发；`RunInspector.latest` 在 derive/快照阶段算一次；artifacts 的过滤同样上移。与 P-08 的快照改造一起做最省事。

预期改善：时间线和检查器的查找从 O(n) 降到 O(1)/O(log n)；对总帧时间的贡献很小，属于消除隐患。

风险与前置条件：序号从 1 连续是应用侧的不变量（`RunModel.subscribe` 在缺口时整份重读），但回放前缀和测试夹具可能构造非连续数组；索引必须在数组替换时重建，不能跨 `reload` 复用。

### P-15 回放时读实时 `run.state`，实时批次仍让回放界面整页失效

严重度：中高（回放路径）。只在回放时出现，但每次实时事件批次都会触发；兼一处显示口径不一致。

证据：`UI/Run/RunView.swift:27` 的 `navigationTitle` 读 `run.state.run.goal`，第 53 行「复制交付摘要」的禁用条件读 `run.state.result`，第 60 行删除按钮读 `run.state.run.status`；`UI/Run/SharedBoard.swift:35` 的 `live:`、第 48 行的 `nowMs`、第 54 行的 `waits` 都读 `run.state.active`。回放内容本身走 `run.shown`（`RunModel.swift:28`，回放时是 `replayState`），但这些外壳读的是实时 `state`。

机制：Observation 按读取的属性登记依赖。`RunModel.state` 每批事件都被替换（P-01），读它的视图就会在每一批后失效，即使当前正在回放、屏幕上显示的是 `replayState`。也就是说回放时实时事件仍在推进，回放界面仍按实时批次整页重算，滑块拖动（P-06）与实时批次叠加。显示上还有一处不一致：回放停在结果提交之前时，「复制交付摘要」仍按实时 `state.result` 判断，可以复制到尚未出现在回放位置的交付。

实测：未单独测量；按 P-01 的每批整页成本推算，20k Run 回放时每批可省去一次整页失效（推算），50k 时收益更大。回放位置变化触发的重算不受影响，仍需 P-06 的异步与合并。

影响：审计与回放页、拖动滑块与左右箭头键、回放期间实时 Agent 仍在输出的事件流。

修复方案：回放路径统一读 `run.shown`（或 `st.run`），例如 `shown.run.goal`、`shown.result`、`shown.run.status`；`SharedBoard` 的 `live`/`now` 已经在回放时取 `st`，只需把 `run.state.active` 换成 `st.run.active`，实时路径语义不变。复制交付摘要、删除按钮等 Run 级动作若确实要按实时状态启用，应放在不参与回放页重算的独立小视图里。

预期改善：回放时一批实时事件不再让整个 RunView/ProgressBoard 失效；回放的每帧预算只由回放位置和用户操作决定，与实时事件率解耦。

风险与前置条件：`run.shown` 在 `replay == nil` 时回落 `state`，语义不变；回放前缀没有 `result` 时，复制按钮应禁用还是复制前缀可见的交付，需要产品确认；删除是 Run 级操作，按实时状态启用是合理的，但界面要说明它作用于整个 Run 而不是回放位置。

### P-16 审计页每次 body 重算筛选、参与者与不变量

严重度：中。只在审计页打开时付费，但每个事件批次都会重算。

证据：`UI/Run/AuditView.swift:9-20` 的 body 每次读取 `run.state`，用三次条件过滤全部事件得到 `rows`（第 11-15 行），用 `contains` 逐事件收集 `actors`（第 17 行，O(n × 参与者数)），再调用 `invariants(events)`（第 20 行；实现在 `AuditView.swift:107-131`，一次遍历加按任务汇总尝试序号）。`invariants` 与筛选条件无关，却也每帧重跑。

机制：这些计算都在 `body` 里，没有缓存或分桶；审计页又读 `run.state`，所以 P-01 的每个批次都会让整页重算一次。筛选条件（`auditCat`、`auditTask`、`auditActor`）变化时也要重算，这部分无法避免，但事件批次带来的重算可以。

实测：`auditBody`（`rows` 加参与者，等价代码）5.07ms/20,200（`misc.log`，`progress` 分类、`T1` 筛选，5 次中位数）；`invariants` 未单独测量，按同一次遍历加字典统计推断与 `auditBody` 同量级（推理）。20k Run 且审计页打开时，每个事件批次约 5ms 以上的主线程税，16.7 批/秒时约 85ms/秒（推算）。

影响：审计与回放页的打开状态、筛选操作、回放拖动（每格还会再触发一次）。

修复方案：把 `invariants` 移到 derive 或快照阶段（只依赖事件，与筛选无关）；`rows` 与 `actors` 按「筛选组合 + 事件数」缓存，筛选变化时才重算，事件批次只增量更新。至少先把 `invariants` 移出 body，它是纯函数且每批都做。

预期改善：审计页每批从约 5ms 降到 <1ms（筛选行仍需一次过滤，但不再重复收集参与者和不变量）；事件越多收益越大。

风险与前置条件：`invariants` 的结论必须与现在逐事件检查完全一致，用现有审计/不变量测试兜底；筛选缓存的键要包含三个筛选值和事件数；回放时用 `replayState` 的事件重算，不能复用实时的不变量。

### P-17 每批在主 actor 上增量扫描文档路径

严重度：中低。每个批次都有，单批通常很小，遇到含 `.md` 的长文本会变成毫秒级。

证据：`Core/RunModel.swift:168` 在 `scheduleDerive` 的尾部（主 actor）无条件调用 `findDocuments()`；`RunModel.swift:140-146` 在 `docScanned < events.count` 时执行 `RunDocs.scan(events[docScanned...], cwd: state.run.cwd, into: &docScan)`，`Core/RunDocs.swift:47` 对每条新事件的 `data` 字符串和 `text` 调 `paths(in:)`，后者在 `RunDocs.swift:63-78` 用两条正则和 `NSRange` 提取路径。

机制：`derive` 本身被放到 `Task.detached`（`RunModel.swift:163`），但紧随其后的 `findDocuments` 在 `state = derived` 之后同步运行在主 actor 上；增量扫描的输入虽然只有新事件，却仍逐条做「字符串拼接 + `.md` 快速检查 + 正则」。`paths(in:)` 在文本含 `.md` 时走完整正则（2.4KB 约 0.49ms，`pathsGuard`），一次工具调用 diff 可达 12KB，单批可能多花几毫秒。这是 P-11 的「每批 `existing` stat」之外的另一半：P-11 是文件系统检查，这一条是路径正则扫描；P-05 是首次加载的全量扫描。

实测：全量 `RunDocs.scan` 20k 为 485.8ms（`scan.log` 的 realistic 日志），平均约 24µs/事件，但分布不均；`RunDocs.paths` 2.4KB 含 `.md` 文本约 0.49ms/次，不含 `.md` 文本 1000 次仅 54.3ms（`pathsGuard`）。每批的实际成本取决于新事件文本是否含 `.md`（推理）。

影响：每个事件批次的主 actor 尾部；工具调用集中、diff 里频繁出现 `README.md`、`docs/*.md` 时更明显；与 P-05、P-11 分开计。

修复方案：把 `RunDocs.scan` 的增量部分放进 derive 的同一个 detached 任务，主 actor 只接收新增候选并合并；或者按事件 seq 缓存路径提取结果，`findDocuments` 只合并新结果。`fileExists` 仍按 P-11 处理。

预期改善：稳态每批主线程不再做正则扫描；含 `.md` 的长 diff 批次省下约 0.5–2.5ms（推算）；文档菜单的更新时机不变。

风险与前置条件：`docScan` 是 `@ObservationIgnored` 的可变字典（`RunModel.swift:69`），后台任务不能直接改主 actor 的副本；要用「后台返回新增候选 + 主 actor 合并」的方式，并保证 `reload` 时清空。`RunDocs.scan` 的输入切片与 `docScanned` 必须严格对应事件数组，不能跨 `reload` 复用。

## 5. 已检查但认为不是问题

### 5.1 主线程事件解析（检查项）

结论：按事件到达的路径，`JSON.parse` 不在主线程，`Event` 构造在，但单条成本极低，不是卡顿来源；需要改的是整份读取时的 map（P-05）和每帧重算（P-01）。

证据与数字：`API.stream` 是非隔离函数，`Core/API.swift:96-113` 在 `AsyncThrowingStream` 的构建闭包里创建 `Task`，第 107 行对每条 SSE 数据执行 `JSON.parse`；这个任务不继承主 actor，所以解析在后台完成。`API.send`（`API.swift:75-86`）同样在非隔离方法里解析 HTTP 响应。`RunModel.subscribe` 中消费流的 `Task`（`Core/RunModel.swift:113-129`）才继承主 actor，每轮只做 `Event(j)`、序号检查和 `events.append`；`Event.init`（`Derive.swift:20-31`）只做字段读取，`raw` 与 `data` 共享同一份 JSON 值（Swift 值类型写时复制），没有深拷贝。实测 200 条 `Event.init` 0.030ms，即 0.15µs/条；20k 条合计约 3ms，且分散在整场 Run 的各批次里，单批通常只有几条。`events.append` 均摊 O(1)。

因此不把「SSE 每条都解析」列为问题；把 `data["events"].array.map(Event.init)` 在 `load` 中主线程执行 4.6ms/20k 计入 P-05，把每批之后的重算计入 P-01。

### 5.2 `derive` 内部是否 O(n²)（检查项）

结论：`derive` 对事件数近似线性，候选的几处「每次 removeAll/contains」都被一个小集合界定，不是 O(n²)；`boardFacts` 对「板上条目」是超线性的，但在当前协议下可接受。

证据与数字：`derive` 在 1k/5k/20k/50k 的实测为 0.90/4.05/14.75/40.60ms，比例接近 1:4.5:16.4:45，符合线性；dense 20k（板上条目多 12.5 倍）的 derive 为 14.58ms，与 realistic 的 14.75ms 几乎相同，说明它不随板上条目增长。逐项：`dropSess`/`dropKey` 的 `s.inbox.removeAll`（`Derive.swift:205,211`）遍历的是「未处理的请求」，数量由同时打开的请求决定，远小于事件数；`run.cancelled`（`Derive.swift:246-249`）和 `run.interrupted`（`Derive.swift:351-352`）只在整个 Run 取消/中断时遍历任务，各一次；`task.claimed` 的 `t.owns.contains`（`Derive.swift:369`）只遍历该任务负责的文件；`task.reopened`（`Derive.swift:376`）遍历任务表，但只在该事件上发生。

`boardFacts` 的 `results × handoffs` 嵌套（`Derive.swift:547-552`）最坏是 O(R²H)，artifacts 的两层 `contains`（`Derive.swift:553-558`）是 O(A²)；这里的 R/H/A 是结果、交接、产物的条数，不是事件数。实测 realistic 20k 4.18ms、dense 20k 16.0ms（板上条目约 12.5 倍，成本约 3.8 倍）；20k 事件里 realistic 只有 47 个结果、47 个交接、47 个产物（`stats.log`），远未到危险区。需要记住的是：如果将来每个事件都产生一条结果/交接，这段会明显恶化，应改成一次分组加哈希表；当前不是问题。

`usageLedger`（`Core/UsageLedger.swift:463-630`）也是 O(n) 加最后的按任务/成员过滤（O(T×S)），实测 1k/5k/20k/50k 为 0.34/1.59/6.44/17.10ms，占 derive 约 40%，是 derive 里最大的一块，但没有超线性。

### 5.3 被检查后排除的其他项

- **`Event` 与 `raw` 的内存放大**：`raw` 和 `data` 共享底层 JSON 值，`text`/`actor` 等字符串共享底层缓冲区，没有逐字段深拷贝；额外内存是每个 struct 的字段和 JSON 枚举包装，量级 O(n)，低于原始响应大小（20k 事件 21.7MB 响应）。没有测应用进程内存，此项按代码检查判断，不作为问题。
- **惰性列表**：会话用 `LazyVStack`（`Session.swift:227`），执行轨迹用 `LazyVStack`（`TaskView.swift:57`），审计用 `List`，只构建可见行。列表本身不是瓶颈；问题是交给它的 `groups` 每帧重建（P-02）和行内的 Markdown 重解析（P-07）。
- **正则与日期格式器**：`regex(_:)` 以及 `L.dateTime` 的格式器都是 `static let`（`Core/Format.swift:6,82-92`），`MD` 的模式（`UI/Markdown.swift:10-15`）、`RunDocs` 的模式（`Core/RunDocs.swift:81-84`）、`ResultCard.taskId`（`RunView.swift:515`）、`ChatItem.resultBlock`（`Session.swift:394`）也都是静态的；`L.category`（`Format.swift:68`）用前缀比较而不是正则。例外只有 P-13。
- **定时刷新只在变化时赋值**：`AppStore.loadRuns` 在 `r["runs"].array != runs` 时才赋值（`AppStore.swift:75`）；`InboxModel.refresh` 在 `out != all` 时才赋值（`Inbox.swift:123`）；`RunModel.documents` 在结果不同时才赋值（`RunModel.swift:146`）。这三处符合约束；`state` 的每次赋值不比较（P-01）是唯一明显的例外，`InboxModel` 的 `now` 每次刷新都写（`Inbox.swift:124`）只影响收件箱页自己的重绘，成本很小。
- **广播缓冲区容量**：`broadcast::channel(4096)`（生产 `backend/src/main.rs:518`，测试用 `orchestrator.rs:916`）对单个应用连接足够；正常消费不会 `lagged`。它只在主线程长时间卡住（P-01、P-05）导致消费停滞时溢出，因此是放大器而不是原因。
- **`GET /api/runs/{id}/usage` 是否被页面重复调用**：Run 页的用量来自 `RunState.usage`（`RunModel.swift:30`），不额外请求这个接口；它只服务外部调用。没有重复的全量读取。
- **回放时仍在后台 derive 实时事件**：`scheduleDerive` 没有在 `replay != nil` 时跳过（`RunModel.swift:157`），会浪费一个后台线程周期；但它不在主线程，也不改变回放结果，列为可选的微优化，不是卡顿原因。
- **`loadRuns` 每 5 秒解码 Run 列表**：响应只含最近的 50 个 Run 及配置，解码在非隔离的 `API.send` 中完成，赋值前有相等性守卫（`AppStore.swift:75`）；未测响应大小，判断为可接受，不列为问题。
- **收件箱每 5 秒串行 GET 每个进行中的 Run**：`Inbox.swift:110` 在 `for row in store.runs` 循环里逐个 `await` 拉取 `?after=`；网络等待发生在 `await` 上，不占主 actor，真正的主线程成本是响应回到主 actor 后的 `map(Event.init)` 与 `derive`（P-04）。串行只拉长一次刷新的墙钟时间（N 个进行中 Run 相加，本机回环延迟未单独测），常见 1–5 个活动 Run 时不影响帧率，不单列为问题；活动 Run 数上升后可以并发发起请求，主线程仍按 P-04 移出派生。

### 5.4 仓库已经做对的性能处理

- **解析与复制放到后台**：`JSON.parse` 把字符串和键复制成原生 UTF-8（`Core/JSON.swift:40-44`），避免 `JSONSerialization` 的延迟桥接 NSString；SSE 和 HTTP 响应都在非主线程解析（`API.swift:85,107`）。
- **派生合并与后台执行**：事件按 60 毫秒合并，`derive` 在 `Task.detached` 里跑，派生期间新到的事件会再排一轮（`RunModel.swift:157-174`）；这是正确的骨架，问题只在结果回主线程后的失效范围。
- **正则与格式器复用**：`regex(_:)` 统一编译一次（`Format.swift:6`），日期格式器只在 `L` 里创建一次（`Format.swift:82-92`），`MD`、`RunDocs`、`ResultCard` 的模式都是 `static let`。
- **惰性列表**：会话、执行轨迹、审计各自使用惰性容器，长列表只构建可见行。
- **派生结果的缓存与守卫**：`RunModel.documents` 只在变化时赋值（`RunModel.swift:58-65,146`），收件箱只在内容变化时替换条目（`Inbox.swift:123`），Run 列表只在内容变化时赋值（`AppStore.swift:75`）。
- **会话滚动状态**：`ChatFollow` 只在测量结果变化时写入（`Session.swift:302-306`），新回复、未构建行被测量、SwiftUI 调整偏移都不算「人往上翻」；`ChatScroll` 使用 `ScrollPosition.scrollTo(edge:)` 和明确的 landing 上限，而不是每帧盲目滚动（`Session.swift:236-256,339-362`）。
- **看板与收件箱同源**：`Inbox.entries` 复用 Run 页的 `derive` 与 `boardFacts`（`Inbox.swift:34-49`），两处结论一致；这个设计让派生成为唯一实现，也给后续缓存复用留了位置。
- **事件读取按 seq 增量**：数据库查询是 `WHERE seq > ? ORDER BY seq`（`db.rs:203-221`），收件箱和 SSE 都用 `after`；增量读取实测 0.13ms。问题在应用端没有充分利用它（P-05）。
- **文档菜单不逐事件检查文件**：`RunModel` 的注释和实现都说明文件是否存在只在每批派生后检查一次，不随每条事件或每次绘制检查（`RunModel.swift:63-65`）；方向正确，频率还可以再降（P-11）。
- **分类用前缀比较**：审计筛选的分类判断用 `hasPrefix`（`Format.swift:68-76`），避免每个事件每帧跑正则。

### 5.5 偏离既有实现约束的地方

对照 `docs/architecture/20261031-frontend.md` 的「实现约束」一节（第 346 行起）：

- **「每一行的构建不能遍历整份事件，需要的汇总在列表外算一次再传进去」**：`ChatItem.visible` 的冲突分支在行内扫描 `st.events`（`Session.swift:407-409`），每个冲突行一次；`groups` 本身在 `ChatCard.body` 里整段遍历，虽不在行内，但每次 body 都做（`Session.swift:176-203`）。这两处是 P-02。
- **「视图和逐条事件的处理中不用 `String.range(of:options:.regularExpression)`」**：`Attachments.visibleText` 在 `boardFacts` 和气泡绘制里逐行编译（`Attachments.swift:103,105`），是 P-13；`MarkdownDoc.parse` 在文档窗口里逐行编译（`MarkdownDoc.swift:37,52,67,95,96,160,170,173,199`），不在 Run 页热路径上，可顺手改。
- **「定时刷新的数据（Run 列表、收件箱）只在内容变化时赋值」**：Run 列表和收件箱条目做到了；`RunModel.state` 每次派生都赋值、不比较内容（`RunModel.swift:167`），是本报告的核心问题 P-01。`InboxModel.now` 每次刷新都写（`Inbox.swift:124`），只影响收件箱页自身，成本小。
- **derive 在后台线程执行**：`RunModel.scheduleDerive` 做到了（`RunModel.swift:163`）；`InboxModel.refresh` 没有（`Inbox.swift:115`，P-04），`RunModel.load` 和 `replay` 也没有（`RunModel.swift:93,26`，P-05、P-06）。这是同一条约束在不同入口上的不一致。
- **「长列表用惰性容器」**：已遵守（见 5.4）；没有发现把长列表改成非惰性容器的代码。

## 6. 优化路线

顺序按「先摘掉每帧的固定成本，再做结构性拆分」。每一步都给出验证指标和目标值；验证优先复用本报告的测量程序，再补一条不依赖合成日志的窗口测试。

### 6.1 低风险快速项

**Q1. 把 `boardFacts` 移出视图 body（P-03）**：在 `RunModel.scheduleDerive` 的 detached 任务里计算 `BoardFacts`，随 `state` 一起发布；`ProgressBoard` 读缓存，并把 `timelineTasks`/`milestoneMarks` 移进 `progressOpen` 分支。验证：`boardFrameWork` 的 20k 值下降 ≥4.2ms；用计数器断言折叠时 `boardFacts` 调用次数为 0。回归风险：`who` 的名字映射要在后台用普通字典，不能读 `RunModel`；回放要用回放前缀单独算。

**Q2. 记忆化会话分组，冲突覆盖一次建表（P-02）**：`ChatCard` 用 `(info.evs.count, info.evs.last?.seq, density)` 作为键缓存 `groups`；`ChatItem.visible` 的冲突分支改为查一个在分组构建时算好的集合；`draft`、`sent`、`images` 变化不触发分组。验证：20k 会话 `groups` <1ms；200 冲突 `chatVisible` <3ms；在 10k 条分组的会话里连打 20 个字符，`groups` 重建次数为 0。回归风险：回放时键必须跟着 `replayState` 的事件数组；冲突语义要与逐条扫描逐项一致，补「同一对任务、文件子集」的用例。

**Q3. 把输入从会话重算中隔离（P-02 的输入路径）**：把 `ChatCard` 里依赖 `draft`/`images` 的发送按钮拆成独立的小视图，让按键只失效输入区，不失效 `groups` 和消息行。验证：按键时 `ChatCard.body` 的求值次数（debug 计数器）只增加输入区；长会话打字主线程每键 <4ms。回归风险：`⌘↩` 发送、禁用态文案、附图缩略图的行为不变；输入法组合期仍不能写回（现有 `ComposerField` 约束）。

**Q4. 缓存文本解析结果（P-07）**：给 `MarkdownText`/`MD.inline` 增加按 `(seq, 文本)` 的小缓存，或把 `agent.message` 的 blocks 在后台派生时预计算；`ToolRow` 的 `pretty()` 移进展开内容；`InlineDiff.lines` 改为首次展开时计算。验证：8KB 消息二次解析 <0.1ms；5 条长消息的可见帧解析从约 6.8ms 降到 <0.5ms；`JSON.pretty` 在折叠时调用 0 次。回归风险：缓存键要含文本，不能只按 seq；缓存上限与淘汰策略；`MD` 的输出必须逐字节一致，现有 Markdown 测试照跑。

**Q5. 把 `load`、`replay`、`inbox.refresh` 的派生移出主线程（P-04、P-05、P-06）**：`load` 的 `map(Event.init)`、`derive`、`RunDocs.scan`、`existing` 放进 detached 任务；`replay` 用代际号加 16–33ms 合并；`InboxModel` 用 detached 任务派生每个 Run；三处都只在结果最新时赋值。验证：打开 20k Run 的主线程停顿 <100ms（用主线程卡顿检测或 `DispatchQueue.main` 上的心跳计时）；3 个 20k Run 的 5 秒刷新 <16ms；50k 回放每格 <16ms。回归风险：`load` 期间用户可能看到旧状态，需要加载指示；`replay = nil` 要取消在途任务；收件箱与 Run 页条目一致性测试。

**Q6. 重连先增量、全量兜底（P-05）**：`reload` 先用 `GET /api/runs/{id}?after=<last seq>`；返回的事件序号连续就追加，出现缺口再整份读取。验证：50k Run 断线重连时主线程停顿 <16ms，只传输尾部事件（服务端日志计数）。回归风险：必须证明连续性（返回第一条 = 本地最后一条 + 1），否则回退；`live`、`runJSON` 等元数据仍要刷新。

**Q7. 文档候选检查按版本缓存（P-11）**：`RunDocs.existing` 只在 `docScanned` 增长或 `cwd` 变化时执行；其余批次复用 `documents`；`fileExists` 移后台。验证：稳态每批 0 次 stat（用 `fs_usage` 或代码计数器）；文档菜单在打开时仍能显示新文件。回归风险：菜单短暂过期，打开菜单或重新读取时刷新。

**Q8. 小清理（P-12、P-13、P-14）**：`InboxModel.refresh` 存好 `open`/`later`/按 Run 计数；`Attachments` 改用静态正则；`RunState.event` 加索引、`RunInspector.latest` 上移。验证：侧栏每行不再 split；`Attachments` 相关测试通过；时间线标记查找为 O(1)。回归风险：推迟到期的分组仍随 `now` 重算；正则语义一致。

**Q9. 后端写入（P-09，需先决定持久性取舍）**：WAL 下设置 `synchronous=NORMAL`，并按 ACP 回合或按批把事件放进一个事务；提交成功后再广播。验证：`db.py` 复测单条 ≤28.2µs、批量 ≤6.6µs；`an_event_that_was_not_written_is_not_broadcast` 继续通过；一次 5 会话并发压测无 `lagged`。回归风险：掉电可能丢最后几个事务，需要写进 ADR 或已接受风险；批量失败不能部分广播；批量前必须先统一序号分配：`expectedSequence` 读 `db.max_seq`（`orchestrator/actions.rs:37-41`），`accept_archived`/`recover` 用 `db.max_seq + 1` 取号（`actions.rs:299/626`），与内存中未提交的批量序号会冲突，且 `Db` 单连接锁（`db.rs:31`）会阻塞读取（见 P-09 风险）。此项不影响帧率，排在快速项末尾。

### 6.2 结构性改动

**S1. 拆分状态与失效范围（P-01 的总解）**：把「每帧要用的数据」从 `RunState` 里拆成带版本号的快照（运行卡、请求/交付、进展、会话），视图只观察自己用到的部分；`RunModel` 在主线程只做一次快照替换，不再让整棵 `RunView` 重算。验证：`boardFrameWork` 在 20k <10ms、50k <15ms（主线程部分）；在一个只追加 `agent.message` 的批次里，失效的视图只有会话卡片（用 `body` 求值计数器或 `Self._printChanges` 在 debug 构建里核对）；Instruments 的 Time Profiler 显示主线程 `body` 占比低于 30%。回归风险：实时与回放版本必须对齐；快照构建失败或延迟时不能出现半新半旧；改动面大，建议先做 Q1–Q5 再动。

**S2. 增量 derive 与检查点**：`derive` 接受「已有状态 + 新事件」，只在末尾追加，保留全量 `derive(events)` 作为 `reload`、回放和校验的路径；`usageLedger` 同样增量累加。验证：50k Run、单条事件的增量派生 <2ms（p95）；用随机前缀对「增量结果」与「全量结果」逐字段比较，至少覆盖 1000 个前缀（现有 `backend/testdata/usage_cases.json` 用于账本口径）；回放仍走全量前缀，结果与改造前一致。回归风险：任何事件类型漏处理会静默错位，必须用「全量 vs 增量」随机测试兜底；`RunState` 里的引用类型（`TaskState`、`Attempt`）要在增量时正确复制或隔离，避免实时状态污染回放。

**S3. 会话模型与按 target 分桶**：为每个 `SessionTarget` 建一个会话模型，增量接收事件、维护 `groups`、`plan`、`usage`、`controls`、`commands`，并按可见窗口缓存 Markdown 解析结果；`ChatCard` 只订阅当前会话。验证：20k 事件下打字和流式输出保持 60fps（Instruments 掉帧计数 <1%）；滚动 1 万条分组无 >16ms 的停顿；内存较当前增加 <30MB（Instruments Allocations）。回归风险：切换会话时的滚动位置、`ChatFollow` 语义、回放位置都要逐一测；分桶等价于现有过滤条件。

**S4. 传输与重读**：保留 SSE（它是单机唯一通道，协议简单），但重连、`lagged`、切回都先用 `after` 增量；如果将来出现需要跨进程恢复的大 Run，再考虑把事件按块分页或在 `GET` 响应里直接给已序列化的片段，避免「读 100MB、解析 100MB」。验证：50k Run 的重连不解析整份响应（网络字节计数）；`lagged` 后的恢复时间 <200ms。回归风险：增量恢复的正确性依赖序号连续；分页会改变客户端读取逻辑，需要单独设计，不在本次范围。

**S5. 后端批量写入与持久性决定（P-09 的完整版）**：按回合批量提交、提交后广播、WAL 下 `synchronous=NORMAL`，并补一条「提交失败整批不广播」的测试；如果产品要求掉电不丢最后几条，改回 `FULL` 只批量提交，收益仍有 −88%。验证：同一份 `db.py` 基准；并发压测；恢复测试（杀掉进程后重启，事件序号无缺口）。回归风险：持久性语义变化需要 ADR；`Run.emitting` 锁内的批量窗口不能跨越 ACP 等待，否则事件延迟增加；实施前先统一序号分配（见 P-09 风险），覆盖 `expectedSequence`（`orchestrator/actions.rs:37-41`）、`actions.rs:299/626` 的 `max_seq + 1` 与未提交批量序号之间的 409、重复序号和单连接锁阻塞读取这三类问题。

每档完成后都用同一组指标复测，并把结果追加到本文末尾的复算记录之外（报告是时点快照，后续数据写入新的审查或 `LESSONS.md` 的清理记录）。

## 7. 不要做的事

优化必须保持下面的不变量。某些看起来最省事的改法会破坏它们，因此明确排除。

**事件仍然是唯一事实来源**：不用界面缓存代替事件，不把派生结果写回事件或数据库，不为性能丢弃、合并或改写已写入的事件。恢复、看板、审计、导出和回放都从同一份日志读，任何「只给界面看」的裁剪只能发生在显示层。

**先落库再广播**：`App::store` 的顺序是 J-04 修复的核心（`backend/src/orchestrator.rs:774-780`，测试 `an_event_that_was_not_written_is_not_broadcast`）。批次提交只能缩短 fsync，不能改成「先广播、后台补写」。

**`derive`、`usageLedger`、`boardFacts` 是纯函数**：同样的输入必须得到同样的输出，回放前缀、重启恢复和历史 Run 共用它们。增量版本必须能以「全量」为基准逐字段比对通过，前后端的用量口径继续用 `backend/testdata/usage_cases.json` 对齐；不能为了快，在函数里藏跨调用的可变状态。

**回放等于前缀派生**：`RunModel.replay` 的语义是「前 N 条事件从零推导」，不能为了省事复用实时状态再回退，也不能让增量检查点改变前缀结果。优化可以改变计算时机和线程，不能改变回放显示的数字。

**不节流、不合并、不删除后端事件**：`agent.status` 驱动停滞检测和 Agent 当前状态（`orchestrator/turn.rs:560-572`、`Derive.swift:276-288`），`tool.call` 驱动冲突、痕迹、看板和时间线（`turn.rs:534-557`），`session.usage` 是用量账本的唯一输入。显示层可以折叠连续工具调用（现有「普通」「摘要」档），日志不能少记。也不要用减少事件字段（截断 `text`、丢掉 `diffs`、去掉 `raw`）来降字节：审计、恢复、diff 审阅和交付都依赖它们，改格式需要单独的 ADR。

**不放宽 60 毫秒合并或跳过批次**：问题在每批的 O(n) 成本，不在批次频率。跳过批次会让界面落后于事件，还会让序号续接和回放更容易出错；把 60 改成 200 只会把卡顿变成一顿一顿的大卡。

**不删 5 秒轮询、也不改成每个 Run 一条 SSE**：收件箱要看所有工作区的进行中 Run，轮询是刻意设计；为每个 Run 建流会让连接数、服务端 fan-out 和派生来源都变多，而且不解决主线程派生的问题（P-04）。现有「内容变化才赋值」的守卫要保留。

**不为了少重绘而抑制真实的更新**：不能给 `state` 加一个粗糙的 `Equatable` 让 SwiftUI 跳过重算，也不能用 `@ObservationIgnored` 遮住会变化的字段。正确做法是缩小「谁观察什么」，不是让观察失效。

**不回退惰性列表或重写为 AppKit 表格**：`LazyVStack`/`List` 已符合约束；在快照和缓存做完之前，重写列表无法证明收益，还会破坏滚动、输入法、无障碍和既有窗口测试。

**不做磁盘缓存**：运行时不得向仓库写文件，缓存也不应复制事件日志；需要的缓存放在内存，按事件 seq 加版本，`load` 时整体丢弃。

**不碰权限、凭据和隔离边界**：不能为了减少一次读取把 `~/.codex`、`~/.claude` 或 `~/.janus/agents/<id>/` 的内容搬进日志、事件或缓存；不新增网络监听、下载来源、第三方脚本执行；不改凭据存放位置。角色 Skill、MCP 和指令仍然只在会话启动时注入。

**不更改产品行为**：默认显示档位、消息完整内容、折叠与展开的默认值、验收证据的完整性都不能为了帧率改变。性能优化的目标是同样的界面更流畅，不是更少的信息。

**遵守仓库规则**：不新增 `#[allow(...)]`，不留死代码，不新增超过 2,000 行的源文件，不绕过测试和格式检查；改行为时同步文档，审查报告按约定只写时点快照，不回改正文。

**测量本身的边界**：不启动真实 Agent 会话、不调用付费接口；合成的日志不能写成实测，推算不能写成实测；在用户的真实 Run 上采样时要避开 prompt、凭据和个人目录内容。

## 8. 附录

### 8.1 测量方法、脚本与合成日志

测量目录：`/tmp/janus-perf-20261117-8f3a`。它不在仓库内，是一次性目录；里面的 `src/` 是 `frontend/Sources/Janus/Core/` 相关源码的副本，可执行文件 `perf`、`stats`、`loadmeas`、`textmeas`、`miscmeas`、`scanmeas` 由同目录的 `main.swift`、`gen.swift`、`stats.swift`、`text.swift` 等编译而成；`db.py` 是 SQLite 写入脚本；`perf.db` 是 `db.py` 生成的数据库（约 105MB）。

测量方式是单进程、单线程重复执行同一段纯计算，每档 3–5 次，报告取中位数；原始日志列出了 min/max。没有使用 Instruments，也没有实例化 SwiftUI 视图：`main.swift` 里把 `UI/Run/Session.swift` 的分组（`groups`）、可见性（`ChatItem.visible`）、工具行隐藏（`ToolRow.hidden`）和 `agentSession`/`taskSession` 逐字复制为等价函数，并用注释标明；因此这些数字是「视图每帧要做的纯计算」，不是真实帧时间。

合成日志（`gen.swift`）：第一条 `run.created`，随后 `orchestration.accepted` 带 6 个任务（`owns` 和依赖各一），4 个 Agent（含负责人）按轮次产生事件；每个轮次包含 `task.ready`、`attempt.started`、`agent.status`、`agent.message`（平均约 2.4KB 的散文）、`agent.thought`、`tool.call`（diff 约 5KB）、`session.usage` 等。`realistic` 档每 250 条事件出现一条看板事实（结果/交接/产物/决定），`dense` 档每 20 条一条。每条事件都先编码成线上 JSON 再 `JSON.parse` 回来，保证与 SSE 收到的形状一致。`stats.log` 的分布来自这份 20,003 条的 realistic 日志。

session 建模与真实后端的差异（影响绝对值的解读）：`gen.swift` 给每条 `agent.status` 生成新的 `session` id（`gen.swift:111`），而真实后端只在 `attempt.started` 写入该次尝试的 ACP 会话 id（`backend/src/orchestrator/attempts.rs:229-231`），`agent.status` 的 data 只有 `phase`（`orchestrator/turn.rs:560-572`）。账本按 `current[owner]` 归并会话（`UsageLedger.swift:493-505`），于是合成 20k 日志单次产生约 3,032 个账本会话（`run.log` 的 checksum 是 5 次复跑的合计 15,160；同一日志有 1,578 条 `attempt.started`，约 1.9 倍），真实后端每次尝试约 1 个会话。这使 `derive`（含 `usageLedger`）的绝对值偏高约 2 倍：报告自己的 `perf` 复跑为 20k derive 15.152ms / usageLedger 6.533ms，验证者按真实会话建模另写的二进制为 6.596ms / 3.102ms（`/tmp/janus-verify-7c2e/verify_realistic.log`）。`boardFacts` 不依赖会话数，与验证者的 5.711ms 在 ±50% 内一致（报告 4.178ms）；SQLite 数字同样不受影响。

局限：合成日志的任务数、尝试数和事件分布由脚本决定，不等于任何一次真实 Run；它只用于比较同一类工作的相对成本和验证复杂度。账本会话建模让 `derive`/`usageLedger` 的绝对值偏高约 2 倍，`boardFacts` 不受影响。真实帧时间、应用进程内存、服务端序列化耗时、菜单栏和滚动回调的耗时都没有测。

### 8.2 原始数字

以下全部为实测（合成日志或 `db.py` 基准），单位毫秒，括号内为中位数。

**派生与看板**（`run.log`，reps 5；50k 与 dense 为 3）：

| 事件数 | `derive` | `usageLedger` | `boardFacts` | `derive`+`boardFacts` |
| --- | --- | --- | --- | --- |
| 1,000 | 0.899 | 0.343 | 0.237 | 1.039 |
| 5,000 | 4.052 | 1.592 | 0.976 | 4.593 |
| 20,000 | 14.749 | 6.438 | 4.178 | 19.431 |
| 50,000 | 40.603 | 17.096 | 10.400 | 48.775 |

`derive` 已包含 `usageLedger`（`Derive.swift:395`），上表两列不是相加关系；`derive`+`boardFacts` 是另一次组合测量。上表 20k/50k 的 `derive`/`usageLedger` 受 8.1 的 session 建模影响，绝对值偏高约 2 倍，只能用于相对比较；`boardFacts` 列不受影响，`derive`+`boardFacts` 列中的 derive 部分仍偏高。

**dense 看板、回放、JSON、会话视图**（`run.log`）：

| 项目 | 数字 |
| --- | --- |
| dense 5,000 `derive` / `boardFacts` | 3.758 / 1.604 |
| dense 20,000 `derive` / `boardFacts` | 14.580 / 16.001 |
| `replayPrefix` 5,000 / 20,000 / 50,000 | 3.623 / 14.750 / 38.878 |
| `jsonParse` 1KB / 4KB / 16KB | 0.026 / 0.058 / 0.196 |
| `Event.init` 200 条 | 0.030 |
| `taskSession` / `agentSession(lead)`（6,515 条） | 0.784 / 2.710 |
| `SessionView.plan` / `usage` / `controls` / `commands`（6,515 条） | 0.050 / 0.065 / 0.001 / 0.000 |
| `chatGroups` 20k normal / detail / summary | 5.366 / 5.387 / 5.563 |
| `chatVisible` 20k / `toolHidden` 20k | 2.103 / 2.301 |
| `boardFrameWork` 20k | 55.102（min 52.839，max 55.425；3 次；derive+boardFacts+agentSession+chatGroups 四项组合，下界） |

**冲突与页面级扫描**（`misc.log`，n=20,200、200 个冲突时）：

| 项目 | 数字 |
| --- | --- |
| `chatVisible` conflicts=200 | 42.199 |
| `chatVisible` conflicts=0（20,000 条） | 2.096 |
| `runStatsScan`（6 任务 × 4 成员） | 4.002 |
| `taskViewFilter` | 0.850 |
| `auditBody`（progress，T1） | 5.067 |
| `runDocsScan` | 755.849 |

**文本与 Markdown**（`text.log`）：

| 项目 | 数字 |
| --- | --- |
| `MD.blocks` 8KB | 0.859 |
| `MD.inline` 单段 | 0.079 |
| `MD.blocks`+`inline` 8KB | 1.348 |
| `InlineDiff.lines` 200 / 1,000 / 2,000 行 | 0.573 / 2.726 / 4.372 |
| `JSON.pretty` 2KB / 16KB | 0.036 / 0.165 |

**SQLite**（`db.log`，`db.py`）：

| 项目 | 数字 |
| --- | --- |
| 单条 INSERT，autocommit + `synchronous=FULL` | 56.2µs/条（1,000 条 56.2ms） |
| 单条 INSERT，autocommit + `synchronous=NORMAL` | 28.2µs/条（1,000 条 28.2ms） |
| 单事务批量，`synchronous=FULL` | 6.6µs/条（1,000 条 6.6ms） |
| 读全量 10,000 条 | 17.2ms（20.1MB） |
| 读全量 50,000 条 | 85.7ms（100.4MB） |
| `seq >` 增量读 100 条（10k / 50k 库） | 0.13ms / 0.13ms |

**全量响应解析**（`load.log`）：

| 项目 | 20,003 条 | 50,006 条 |
| --- | --- | --- |
| 响应大小 | 21.7MB | 55.2MB |
| `JSON.parse`（后台） | 447.8 | 1124.0 |
| `Event.init` map（主线程） | 4.6 | 11.1 |
| parse + map + derive | 464.5 | 1176.3 |

**事件大小分布**（`stats.log`，20,003 条 realistic）：总计 22,417,803B，平均 1,120B，p50 282B，p90 3,755B，p99 7,846B，最大 10,446B。按类型：`agent.message` 3,826 条 / 10,526,191B（平均 2,751B）；`tool.call` 1,077 条 / 6,472,301B（6,009B）；`agent.thought` 1,394 条 / 1,626,889B（1,167B）；`agent.status` 3,306 条 / 707,914B（214B）；`session.controls` 1,578 条 / 638,686B（404B）；`session.commands` 1,578 条 / 514,024B（325B）；`attempt.started` 1,578 条 / 441,719B（279B）；`tool.started` 1,434 条 / 373,694B（260B）；`task.ready` 1,578 条 / 227,933B（144B）；`session.usage` 780 条 / 178,657B（229B）；`result.submitted` 47 条 / 152,783B（3,250B）；`task.conflict` 137 条 / 29,103B（212B）。`agent.message` 和 `tool.call` 合计占 76% 的字节。

**文档扫描**（`scan.log`，另有一次复跑）：

| 项目 | 首次记录 | 2026-11-17 复跑 |
| --- | --- | --- |
| `runDocsScan` 2,000 / 5,000 / 20,000 | 50.703 / 124.405 / 485.815 | 45.955 / 111.906 / 524.874 |
| `pathsGuard` 2.4KB 不含 `.md` × 1000 | 54.285 | 54.101 |
| `pathsGuard` 2.4KB 含 `.md` × 1000 | 491.514 | 508.334 |

同一个可执行文件两次运行的差异约在 ±10%，报告正文使用首次记录；复跑放在这里说明波动范围。`misc.log` 的 `runDocsScan` 755.849 是另一份事件集（20,000 条会话事件加 200 条冲突，文本更长），与 `scan.log` 的 realistic 日志不可直接比较。

注意：`run.log` 里也有一条 `chatVisible label=conflicts=200 n=20200`，值为 2.283ms。那段代码过滤的是不含冲突事件的基础数组（`conflictEvs.filter`），冲突扫描从未执行，因此这个数字不能当作冲突成本；200 个冲突的正确测量在 `misc.log`（42.199ms），正文使用后者。

### 8.3 复算命令

```sh
cd /tmp/janus-perf-20261117-8f3a
./perf        # run.log：derive、usageLedger、boardFacts、dense、replayPrefix、jsonParse、Event.init、会话视图、chat、boardFrameWork
./stats       # stats.log：事件大小分布和按类型统计
./loadmeas    # load.log：/api/runs/{id} 全量响应解析、Event map、parse+map+derive
./textmeas    # text.log：MD.blocks、MD.inline、InlineDiff、JSON.pretty
./miscmeas    # misc.log：冲突扫描、runStats、taskView、auditBody、runDocsScan
./scanmeas    # scan.log：RunDocs 全量扫描和路径正则
python3 -I db.py /tmp/janus-perf-20261117-8f3a   # db.log：重建 perf.db，测三种写入和两种读取
```

`db.py` 会删除并重建 `/tmp/janus-perf-20261117-8f3a/perf.db`（约 105MB），不影响仓库；其余程序只向标准输出打印。测量程序依赖同目录 `src/` 的源码副本，副本与审查对象的 `Core/` 一致；若要复核某条实现，先从仓库同一 commit 重新复制再编译。

### 8.4 实测、推算与未测的边界

- **实测**：8.2 的全部数字，来源是合成日志上的纯计算和 `db.py` 的 SQLite 基准。它们能证明相对成本和复杂度，不等于用户机器上的真实帧时间。`derive`/`usageLedger` 受 8.1 的 session 建模影响，绝对值偏高约 2 倍，只能用于相对比较；两个独立二进制复跑都能复现（8.5），`boardFacts` 与 SQLite 不受影响。
- **推算**：第 3 节的事件率与字节率（由 `turn.rs`、`acp_view.rs` 的映射和上限加实测分布推得）；50k 的整页成本（按 20k 实测线性外推）；多个活动 Run 的收件箱成本（按单个成本相加）；主线程约 40ms/批（55.1ms 减去后台 derive 14.7ms；两项都来自合成日志，derive 偏高约 2 倍，校正后整页组合约 47–48ms、主线程部分仍约 40ms/批）；「每秒约 0.67 秒主线程计算」（40ms × 16.7）；P-04 校正后的 11–12ms/34–36ms/26ms（50k 的 26ms 由 20k 实测按事件数线性外推）；P-17 的每批 0.5–2.5ms；内存和文件系统调用次数。
- **未测**：真实 SwiftUI 帧时间、掉帧率、Instruments 轨迹、应用进程内存、服务端序列化耗时、菜单栏与滚动回调的耗时、真实 Run 的事件分布、审计页 `invariants` 的单独耗时、回放时实时批次失效的单独耗时。这些需要在后续实现阶段用 Instruments 和窗口测试补测，不能引用本报告的数字冒充。

### 8.5 独立验证者的复跑（2026-11-17）

验证者（子任务 `perf-verify`）用报告自己的二进制和自写的测量程序独立复跑，测量目录 `/tmp/janus-verify-7c2e`。

- 报告二进制 `/tmp/janus-perf-20261117-8f3a/perf` 复跑：20k `derive` 15.152ms、`usageLedger` 6.533ms、`boardFacts` 3.737ms，与 8.2 记录的 14.749/6.438/4.178 相差 ≤11%；另一次复跑记录在 `upstream_perf_rerun.log`，为 14.370/6.212/3.957。`loadmeas` 复跑 440.2/1110.4ms，`miscmeas` 的冲突扫描 40.468ms，也在记录值的 ±11% 内。
- 验证者自写二进制（`/tmp/janus-verify-7c2e/main.swift`，自造 20k 日志、按真实后端「每次尝试约一个会话」建模）：`verify_realistic.log` 中 20k `derive` 6.596ms、`usageLedger` 3.102ms、`boardFacts` 5.711ms、账本会话 694 个；5k 为 1.656/0.785/1.313ms。`boardFacts` 与报告相差约 +37%（在 ±50% 内），`derive`/`usageLedger` 约为报告的一半，与 8.1 的 session 建模解释一致。
- SQLite（自写 `verify_db.py`，输出 `db_verify.log`）：autocommit + `synchronous=FULL` 54.7µs/条、`NORMAL` 25.3µs/条、单事务 6.6µs/条；与 8.2 的 56.2/28.2/6.6 相差 ≤11%。
- 验证者自己复跑了 `perf` 的 20k `derive`/`usageLedger`/`boardFacts`，也用它自己的二进制复现了 `boardFacts` 与 SQLite；两组数字在各自建模下都可复现。结论：报告的测量可复现；`derive`/`usageLedger` 的绝对值偏高约 2 倍；复杂度、失效范围与因果链的结论不变。
- 复跑命令示例：`cd /tmp/janus-verify-7c2e && swiftc -O src/*.swift main.swift -o perfverify && REAL_SESSIONS=1 ./perfverify 1000 5000 20000`（`src/` 是仓库 `Core/` 的源码副本）；SQLite 为 `python3 -I verify_db.py`。


