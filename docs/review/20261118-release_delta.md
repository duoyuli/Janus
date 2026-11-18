# 2026-11-18 发版后增量审查

上次正式发布为 `v2026.11.17.5`（`57a35dd`）。本次审查截至 `origin/main` 的 `0796a42`，共 18 个提交、115 个文件，增加 7,683 行，删除 1,976 行。范围包括 Rust 编排、ACP 会话续接、用量账本、资源导入，以及 macOS 的状态派生和收件箱。基线通过 152 个后端测试、102 个 macOS 测试和 `just lint`。补充用例在源码临时副本运行，使用脚本化 ACP Agent、合成事件和临时 SQLite 数据库。未运行真实模型会话，未测量实际窗口帧率。

## 发版前优先处理

### R-01 · P1 · 重启可能放行已被独立验证拒绝的下游任务

在线调度根据下游任务写入看板时的 `planned_seq` 判断负责人当时是否已知验证结果。这个值留在内存。恢复时，编排器用 `orchestration.accepted.seq - 1` 代替。写入内存任务和发出接纳事件之间可以插入另一条事件。如果插入的恰好是前置验证的拒绝结论，两个时点会给出相反的调度决定。

在临时副本给这两个操作之间加等待点，让验证者在此时提交拒绝。运行时下游进入 `blocked`。模拟服务重启并恢复同一 Run 后，在没有修复、重新验证或重新编排的情况下，同一下游任务执行成功。建议在接纳事件里持久化实际采用的计划序号，恢复时读回它，并将上述交错加入在线与恢复的一致性测试。代码见 [写入看板](../../backend/src/orchestrator/board.rs)、[重建看板](../../backend/src/orchestrator/actions.rs) 和[判定依赖](../../backend/src/orchestrator.rs)。

### R-02 · P1 · 续接会话会重复累计美元费用，可能提前触发费用预算

同一成员在同一个 ACP 会话中，T1 上报累计 2 USD，T2 通过 `continues` 续接后上报累计 3 USD。代入后端账本，得到 Run 为 5 USD，T2 为 3 USD。正确结果分别为 3 USD 和 1 USD。若费用上限为 4 USD，调度器会提前停止启动新任务。Swift 账本也按成员、任务、会话组合分组。

按任务拆分每轮 Token 时，累计费用读数也跟着拆分，T2 因此失去了 T1 的最后一次读数。建议按原生会话和币种保存上次累计读数，将增量归入实际发生的任务和尝试，并保留计数器归零的处理。把上述序列加入前后端共用的账本用例，连同按时间段统计一起验证。代码见 [Rust 账本](../../backend/src/usage.rs)、[Swift 账本](../../frontend/Sources/Janus/Core/UsageLedger.swift) 和[费用调度](../../backend/src/orchestrator/lifecycle.rs)。

### R-03 · P1 · Token 参考值阻止任务执行（本次工作树已修复）

基线的测试明确要求：T1 上报 1,200 Token、超过 1,000 的上限后，Run 进入 `waiting_input`，T2 必须等用户追加预算。产品要求是 Token 仅用于观察。修复后参考值保存在 Run 快照中，在用量摘要下展示。调度器及恢复流程不再使用该值。旧版 Token 预算事件仅保留历史，旧客户端单独追加 Token 会收到明确错误，墙钟和费用上限不会被误改。

流程测试覆盖超额后 T2 自动执行并交付、旧版预算记录恢复后继续执行。Swift 测试覆盖旧 Token 事件既不创建待办，也不清除真实的美元预算待办。代码见[流程测试](../../backend/src/orchestrator/flow_tests/collaboration.rs)、[重启测试](../../backend/src/orchestrator/flow_tests.rs) 和[界面回放测试](../../frontend/Tests/JanusTests/DeriveTests.swift)。

## 协作与应用体验

| 编号 | 复现与影响 | 修复与验收 |
| --- | --- | --- |
| R-04 · P2 | 负责人收到验证拒绝后的指令包含「取消验证并写明理由」。验证任务已是 `succeeded`，调用 `janus.cancel(V1)` 返回工具成功和「这些子任务都已经结束，没有需要取消的」，看板结论没有变化。 | 在指令中明确可以取消的下游任务 ID，保留历史验证结论；如果产品需要接受例外，单独定义有审计记录的决定。用一次完整的拒绝、处置、再读看板验证指引。[负责人指令](../../backend/src/teamwork.rs) · [看板操作](../../backend/src/orchestrator/board.rs) |
| R-05 · P2 | 依赖 V1 的修复任务收到的 `context.handoff` 只有 `status: succeeded` 和摘要等字段，`verdict` 为空。启动提示也只写了验证任务的执行状态。复现中摘要恰好提到缺陷；若失败细节仅在逐项检查中，下游就要另查 `task_detail`。 | 交接中单列验证判定、未通过项和对应证据。用中性摘要、失败检查和 `rejected` 组合测试实际修复提示。[交接生成](../../backend/src/orchestrator/attempts.rs) |
| R-06 · P2 | 异常用量补记以「整个原生会话曾报告过任意一轮」为退出条件。先用 ACP 记录 80 Token，下一轮异常退出，临时本地会话日志中有已记录的 80 和新增的 480。补记后账本仍为 80，应为 560。多轮问答和热续接都会遇到此路径。 | 记录可对账的消息或轮次边界，只补上尚未入账的增量。测试重复执行补记仍得到 560，避免重读完整日志时双算。[异常用量补记](../../backend/src/orchestrator/attempts.rs) |
| R-07 · P2 | 收到美元预算耗尽后，已在执行的任务仍能结束。若 Run 随后直接交付，macOS 详情页的状态已是 `delivered`，其待办仍包含 `budget`，显示追加或停止操作。已用事件回放复现。全局收件箱对已交付 Run 另按数据库行生成验收项。 | 交付时撤回预算卡，并覆盖不经过追加预算就交付的序列。在详情页和全局收件箱检查同一 Run。[状态派生](../../frontend/Sources/Janus/Core/Derive.swift) · [预算卡](../../frontend/Sources/Janus/UI/Run/Requests.swift) |
| R-08 · P2 | 本地文件夹包含 501 个平级合法 Skill 时，扫描返回 500 项和空的 `warnings`。深度超过 8 也会静默停止。用户看不到预览已截断，无法从这次预览勾选遗漏项。 | 保留扫描上限，在预览中返回并展示截断原因，提示可选择更具体的子目录。测试刚好达到上限与还有下一项两种情况。[资源扫描](../../backend/src/library/offers.rs) · [导入预览](../../frontend/Sources/Janus/UI/Settings/LibraryView.swift) |

## 性能

### R-09 · P2 · 空闲刷新仍在主线程扫描整份看板

macOS 每 5 秒刷新活跃 Run。无新增事件时会复用 `RunState`，但仍在主线程为每个 Run 调用 `Inbox.entries`，后者调用 `boardFacts` 遍历历史。切换应用语言还会让缓存失效。这个周期性开销在上个版本已存在，本次的语言切换增加了重新派生的入口。

在本机使用 Release 构建，对单个活跃 Run 的合成日志重复 7 次取中位数。5,000 条事件时，单算收件箱为 4.13 ms，单算 `derive` 为 2.86 ms。20,000 条时分别为 15.48 ms 和 7.07 ms。两项独立计时，未包含网络和界面渲染。建议先缓存每个 Run 派生出的收件项，在事件序号、语言或展示信息变化时更新，时效性文案独立刷新。验收时同时覆盖多个长 Run、空闲轮询和切换语言。[刷新定时器](../../frontend/Sources/Janus/Core/AppStore.swift) · [收件箱](../../frontend/Sources/Janus/Core/Inbox.swift)

### R-10 · P2 · 美元预算检查反复读取完整事件日志

配置费用上限后，每条 `session.usage` 都会使预算缓存失效，包括只带上下文占用的快照。下一次调度检查会读取该 Run 从序号 0 开始的全部事件，在共享 SQLite 连接锁内解析 JSON，然后构建完整账本，只取其中的美元金额。事件随 Run 增长，并且每次读取之间若都有用量更新，总读取量可接近平方增长。此项来自代码路径分析，尚无生产负载的锁等待测量。

建议结合 R-02 修正后的费用增量维护每个 Run 的预算读数，只在恢复时重建。用只有上下文更新的长 Run 验证调度不再读取全历史，并比较账本与预算值。[预算检查](../../backend/src/orchestrator/lifecycle.rs) · [账本入口](../../backend/src/orchestrator/board.rs) · [SQLite 读取](../../backend/src/db.rs)
