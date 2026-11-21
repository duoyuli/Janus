# 去掉工作限制与派发队列的代码审查

> 上级索引：[审查文档](index.md)

Codex 在 2026-11-21 对去掉工作限制（P0-0）和派发队列（P0-1）的未提交改动做的只读审查。基线和快照是审查时的本机检查点，不在仓库历史中。

**建议暂缓合并。** 共 19 项，其中高 5 项、中 11 项、低 3 项。

基线为 `0f4629d`，覆盖任务开始时的 50 个已跟踪文件差异和指定的 5 个新文件。审查期间工作区新增了 Loop 改动。下文标注 P0 快照行号，链接指向写报告时工作区内的对应语句。原始差异只在审查时的本机临时目录中，没有入库。

### 1. 【高】入队记录先于首条事件发布，并发创建可丢事件或报错

**触发与影响**　工作区 A 插入 `queued` 行后、写 `run.queued` 前，工作区 B 的创建请求或一次 `pump` 已可领取 A。A 的 `record` 与启动端都能从最大序号 0 分配出 1。若 `run.created` 先写入，A 的 POST 可能报 400，但任务已经启动。若创建端晚取序号，事件也可能变成先 `run.created`、后 `run.queued`。

**建议修法**　在一个数据库事务里插入 Run 和序号 1 的 `run.queued`，提交后再允许派发和广播。用两个工作区并发提交，断言两个请求各返回一个 id、每条日志都以唯一的 `run.queued` 开头。

**位置**　`backend/src/orchestrator/queue.rs:35` · `backend/src/orchestrator.rs:1213` · `backend/src/orchestrator/queue.rs:231` · `backend/src/orchestrator/lifecycle.rs:128`

### 2. 【高】领到槽位但尚无团队快照的 Run 无法从中断或失败处继续

**触发与影响**　领取先把行改成 `planning`。若进程在写入 `config.team` 前退出，重启会把它标为 `interrupted`。若此时适配器不可用，准备失败则记为 `failed`。两种记录都会显示「从中断处继续」，但恢复接口直接把缺失的 `team` 反序列化成数组，修好环境再试仍然报「团队记录无法读取」。

**建议修法**　区分首次准备和已有快照的执行。没有快照的首次准备应能按原排队顺序重新派发，或由重试操作重新准备。同一 id 的残留 worktree 要能安全复用或清理。增加在领取与写快照之间终止进程的恢复用例。

**位置**　`backend/src/orchestrator/queue.rs:104` · `backend/src/orchestrator/queue.rs:219` · `backend/src/orchestrator/actions.rs:731` · `backend/src/orchestrator/actions.rs:399`

### 3. 【高】较早的状态写入能覆盖取消，已停止的 Run 会一直占槽

**触发与影响**　负责人启动线程先把内存状态改为 `running`，释放 `run.st` 后暂缓写库。取消线程随后把内存和数据库都改为 `cancelled`，先前线程又把数据库写回 `running`。`pump` 按数据库选运行项，对内存只检查是否暂停。上限为 1 时，一个已经停止的 Run 就能阻住后续队列。

**建议修法**　按 Run 串行提交状态变更，或用带版本的条件更新拒绝迟到的写入。内存、数据库和状态通知按同一顺序生效，再释放转移锁调用 `pump`。回归测试应交错启动与取消，并检查下一个 Run 确实开始。

**位置**　`backend/src/orchestrator.rs:1251` · `backend/src/orchestrator/lead.rs:88` · `backend/src/orchestrator/actions.rs:217` · `backend/src/orchestrator/queue.rs:99`

### 4. 【高】槽位统计或领取写库失败时，调度器仍会启动任务

**触发与影响**　`working_goal_runs()` 出错会按零个占用计算。写 `planning` 出错也会广播成功，并把该 id 交给 `start_queued`。例如 SQLite 临时写锁使这次领取失败，行仍是 `queued`，另一轮 `pump` 或「立即开始」能再次领取同一项。

**建议修法**　统计失败时停止本次派发并记录错误。领取使用带 `status = 'queued'` 条件的更新，只有实际更新一行才广播和启动。用一次失败的 SQLite 更新验证该 id 不会被启动两次。

**位置**　`backend/src/orchestrator/queue.rs:97` · `backend/src/orchestrator/queue.rs:104` · `backend/src/db.rs:163`

### 5. 【高】版本 18 只迁移状态列，旧任务在详情里仍显示执行中

**触发与影响**　版本 17 的目标最后停在 `run.budget_exhausted`。升级把行改成 `interrupted`，启动恢复因此跳过它。列表和收件箱按行显示「已中断」，详情用事件回放，仍可从 `run.created` 得到 `running`，不显示继续按钮，还会按活动任务拒绝删除。

**建议修法**　在同一迁移事务中为原 `waiting_input` 行追加一条新序号的 `run.interrupted`，保留历史预算事件。升级样例应带真实的创建和预算耗尽事件，同时核对数据库、详情状态及继续入口。

**位置**　`backend/src/migrations.rs:152` · `backend/src/db.rs:404` · `frontend/Sources/Janus/Core/RunModel.swift:118` · `frontend/Sources/Janus/Core/Derive.swift:409`

### 6. 【中】服务端口绑定失败也会消耗队首

**触发与影响**　有积压时启动服务，配置端口已被其他进程占用。`pump` 同步把前几条记录改为 `planning`，随后 `bind` 报错退出。端口恢复后再次启动，这批记录按执行中断处理，失去原来的自动派发机会。

**建议修法**　先完成监听绑定、种子同步等可能中止启动的步骤，再启用队列。用占用端口的启动失败验证队列行和首条事件均保持原状。

**位置**　`backend/src/main.rs:145` · `backend/src/main.rs:168`

### 7. 【中】有空位的并发 POST 可能在工作目录和团队准备前返回

**触发与影响**　A 已完成入队，B 的 `take_slots` 先领到 A 并后台启动它。A 自己找不到该 id，就立即返回。此时 GET 可以读到 `planning`、`live=false`、原工作区目录和空团队。改动后的并发测试特意等待最多 5 秒才取 live 对象，掩盖了「有空位时准备好才返回」的接口回归。

**建议修法**　创建时登记每个 id 的准备完成信号。已经获得槽位的请求等待对应准备结果，真正仍排队的请求立即返回。并发测试在 POST 返回后直接验证团队、工作目录和 live 状态。

**位置**　`backend/src/orchestrator/queue.rs:45` · `backend/src/orchestrator/queue.rs:52` · `backend/src/orchestrator/flow_tests/actions.rs:75` · `docs/interfaces/20261031-http_api.md:161`

### 8. 【中】恢复中断目标会绕过已满的跨工作区上限

**触发与影响**　上限为 1，重启后排队的 B 已经开始。此时对中断的 A 调 `resumeRun`，甚至只给 A 发一条消息，都会直接设成 `running` 并启动负责人。A、B 同时工作。ADR 把越过上限的用户入口写成「立即开始」，另外只给恢复暂停中的任务留了例外。

**建议修法**　对恢复中断或失败目标先领取执行槽位，满载时保留恢复意图或返回明确的冲突。恢复必须沿用 A 原有快照和看板。用两个工作区验证普通消息也遵守同一规则。

**位置**　`backend/src/orchestrator/actions.rs:81` · `backend/src/orchestrator/actions.rs:697` · `backend/src/orchestrator/actions.rs:716` · `docs/decisions/20261121-dispatch_queue.md:21`

### 9. 【中】取消或删除中途失败会留下状态已取消、日志仍排队的记录

**触发与影响**　取消先提交 `cancelled`，之后才写 `run.cancelled`，两步之间退出即可留下矛盾。删除也先提交 `cancelled`，随后若附件目录清理因权限失败，DELETE 报错，但任务已经永久出队。重启会跳过已取消行，详情仍由 `run.queued` 显示排队。

**建议修法**　将取消的条件状态更新和事件写入同一事务。删除失败时保留可重试且可解释的状态。若设计为先取消，应完整提交取消事件并通知客户端，再尝试清理。

**位置**　`backend/src/orchestrator/queue.rs:128` · `backend/src/orchestrator/queue.rs:131` · `backend/src/orchestrator/lifecycle.rs:531` · `backend/src/orchestrator/lifecycle.rs:571`

### 10. 【中】删除队首成功后，全局事件流不给其他客户端发通知

**触发与影响**　A 正在运行，B、C 排队。一个 HTTP 客户端删除 B，另一个应用保持 C 的页面打开。删除没有发送 Run 事件或变更通知，另一个应用既会保留已经删除的 B，也可一直显示 C 排第 2。只有发起删除的应用主动刷新列表。

**建议修法**　删除提交后广播携带 Run id 的删除通知，让订阅者移除该行并刷新受影响的排位。在应用保持页面打开时从另一个客户端删除队首，直接断言名次变化，测试中去掉手工 reload。

**位置**　`backend/src/orchestrator/lifecycle.rs:577` · `backend/src/http/streams.rs:58` · `frontend/Sources/Janus/Core/AppStore.swift:519` · `e2e/scenarios/queue.py:35`

### 11. 【中】自动领取与手动开始或取消竞争时，HTTP 会把状态冲突当成参数错误

**触发与影响**　操作请求先读到 `queued`，其获取队列锁前 `pump` 已完成领取。`leave_queue` 返回 false 后的 `bail!` 被映射为 `Refused::Invalid` 和 HTTP 400。相同请求若正好读到准备中的 `planning` 则返回 409。应用只对 409 重读状态，因此用户可直接看到错误并停留在过期页面。

**建议修法**　让领取操作返回可区分的「已离队」结果，并在开始和取消入口统一映射到 `Refused::Conflict`。对已经 live 的 Run 再请求 `start` 也应返回状态冲突。

**位置**　`backend/src/orchestrator/actions.rs:63` · `backend/src/orchestrator/queue.rs:118` · `backend/src/orchestrator/queue.rs:128` · `backend/src/http.rs:59` · `frontend/Sources/Janus/Core/RunModel.swift:229`

### 12. 【中】工作目录准备期间，任务详情仍显示排队并提供已失效的按钮

**触发与影响**　被自动或手动领取后只有全局 `planning` 通知，单个 Run 的日志在工作树和资源准备完以前仍只有 `run.queued`。详情依赖 `derive(events)`，因此复制较大 worktree 时仍显示「同时在做的任务已经满了」以及「取消任务」「立即开始」。此时两个操作都会被服务拒绝。

**建议修法**　领取成功时持久化可回放的准备状态，或让在线详情采用已读到的服务端 `planning` 状态。显示「正在准备」并撤下排队操作。端到端场景应保持详情打开，核对自动派发前后的实际视图。

**位置**　`backend/src/orchestrator/queue.rs:105` · `backend/src/orchestrator/lifecycle.rs:128` · `frontend/Sources/Janus/UI/Run/RunView.swift:23` · `frontend/Sources/Janus/UI/Run/RunView.swift:142`

### 13. 【中】同一 HTTP 响应中的状态和队列名次可能来自两个时刻

**触发与影响**　列表或详情先读到 `status=queued`，随后 `pump` 领取该项，读取全局队列时已经找不到它。响应仍声明排队，却缺少接口承诺的 `queuePosition`。详情的事件甚至可能已包含 `run.created`。

**建议修法**　让状态行和全局名次在同一次 SQLite 读事务或同一条查询中取得，详情事件按同一快照读取。先按全局队列计算名次，再进行工作区过滤和分页。

**位置**　`backend/src/http/runs.rs:50` · `backend/src/http/runs.rs:66` · `backend/src/http/runs.rs:127` · `docs/interfaces/20261031-http_api.md:178`

### 14. 【中】排队目标落在最近 50 条以外时，已打开详情的名次不再跟进

**触发与影响**　C 在队列中排第 2，其后又创建至少 50 个更新的目标，所以全局首页没有 C。另一个工作区取消 C 前面的 B。应用虽刷新列表，C 的详情仍回退到打开时的 `runJSON`，而 C 自己没有新事件，继续显示第 2。此时单独 GET C 已是第 1。

**建议修法**　收到队列成员变化通知时，对当前打开的排队目标重读名次，或复用覆盖该 id 的最新分页行。用超过一页的跨工作区队列验证，无需增加轮询。

**位置**　`frontend/Sources/Janus/Core/RunModel.swift:102` · `frontend/Sources/Janus/Core/AppStore.swift:114` · `backend/src/http/runs.rs:49`

### 15. 【中】旧客户端提交美元上限仍得到成功响应，实际运行已没有上限

**触发与影响**　旧客户端发送合法目标并附上 `maxCostUsd: 1`。`NewRun` 删除了字段，Serde 默认忽略未知字段，服务照常返回 id。调用方收不到预算已失效的信号，后续费用可以超过其明确提交的金额。

**建议修法**　在创建入口明确拒绝携带旧 `maxCostUsd` 的请求，返回说明该字段已移除的 4xx，再由调用方决定是否按新语义提交。用旧版请求体验证不会创建 Run。

**位置**　`backend/src/orchestrator.rs:152` · `backend/src/http/runs.rs:98` · `docs/decisions/20261121-no_work_limits.md:14`

### 16. 【中】产品端到端场景没有覆盖跨工作区派发和带队列重启

**触发与影响**　队列场景只创建一个工作区，第一次重启发生在提交目标前，最后一次重启发生在取消任务后。后端的恢复测试在旧 App 和驱动仍存活时直接构造第二个 App。若槽位被误改成每个工作区各一份，现有两组队列用例仍能通过。真实进程退出时已领取但未完成准备的窗口也不在覆盖范围。

**建议修法**　按合并前检查第 12 条，在一个槽位下用两个工作区提交目标，保持应用打开并在有待派发项时真正停止服务。重启后对照 HTTP 与应用状态检查 FIFO、自动启动次数以及未完成准备的恢复结果。

**位置**　`e2e/scenarios/queue.py:12` · `e2e/scenarios/queue.py:15` · `e2e/scenarios/queue.py:65` · `backend/src/orchestrator/flow_tests/queue.rs:91`

### 17. 【低】历史记录中已调整的 Token 展示参考值退回初始值

**触发与影响**　旧日志里已有 `run.budget_extended`，其中 `kind=tokens`、`tokenLimit=6200`。旧版详情优先显示 6200。新版只读创建快照，若初始值是 1000 就显示 1000，若没有初始值则整行消失。这组历史事件原本有专门的回放测试。

**建议修法**　回放这类旧事件时仅提取 Token 展示参考值，保留现行的运行状态和收件箱逻辑。恢复历史样例，同时断言旧事件不会暂停 Run。

**位置**　`frontend/Sources/Janus/UI/Run/RunView.swift:335` · `backend/src/migrations.rs:153` · `docs/decisions/20261121-no_work_limits.md:17`

### 18. 【低】只有图片的排队目标在详情里无法辨认

**触发与影响**　用户仅粘贴截图创建两个目标，两条记录都显示「按附图完成」。`run.queued` 已保存图片，`derive` 也已读出路径，但 `QueuedView` 只画文字。用户在选择取消或立即开始前看不到要处理的是哪张图。

**建议修法**　在排队卡片中复用普通目标详情的 `StoredImages`。用仅带图片的目标核对排队页状态和截图。

**位置**　`backend/src/orchestrator/queue.rs:21` · `frontend/Sources/Janus/Core/Derive.swift:271` · `frontend/Sources/Janus/UI/Run/RunView.swift:135` · `frontend/Sources/Janus/UI/Run/RunView.swift:361`

### 19. 【低】接口和运行手册仍给出与新状态机冲突的操作说明

**触发与影响**　按 HTTP 详情文档，`live=false` 只支持验收、恢复和消息，SDK 据此会隐藏排队 Run 唯一可用的开始、取消操作。生命周期文档仍要求先停止 `waiting_input` 的 Run 才能删除，还称持续产生不同输出的工具循环「只能靠时间限制」，但这条尝试上限已删除。

**建议修法**　按 `queued`、准备中和已归档分别写清可用操作。删除 Run 级 `waiting_input` 的限制，并将停滞检测遗漏该类循环时的处理改为人工介入。仓库规则要求同步更新旧行为的说明。

**位置**　`docs/interfaces/20261031-http_api.md:235` · `docs/domains/20261031-run_lifecycle.md:257` · `docs/domains/20261031-run_lifecycle.md:204` · `docs/decisions/20261121-no_work_limits.md:21`

## 验证范围

在内存 SQLite 中使用仓库表结构和 SQL，按上述顺序复核了版本 18 的迁移、准备中断时缺少团队、首事件主键冲突、迟到的状态写入和排位读取。并发线程、HTTP 与 SwiftUI 的影响由调用链核对。

已执行只读的差异空白、Rust 格式、Python 语法和项目文档锚点检查。按审查要求，未运行 Cargo 构建或测试、Clippy、应用测试及端到端套件。
