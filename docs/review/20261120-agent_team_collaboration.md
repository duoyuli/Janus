# Agent Team 协作机制与代码审查报告

审查日期为 2026-11-20。对象是 Janus 编排器后端（`backend/src/`）关于多 Agent 协作的核心模块、ACP 客户端集成、Git Worktree 隔离与合回逻辑、事件溯源机制以及相关领域模型与架构决策。源码行号基于提交 `63d0805`。文中的 Run 指一次目标协作任务，worktree 指 Git 为一次目标任务创建的独立工作目录。

本报告综合两轮深度探查与源码逐行审计，系统梳理了现有协作机制的全貌与亮点，并记录了 21 项缺陷与风险（编号 AT-01 至 AT-21）。其中前 4 项为系统级致命缺陷（P0），涉及父子进程死锁、子进程泄漏卡死、消息黑洞丢失与源码永久物理销毁；其余涵盖状态机撕裂、调度竞态、假证据绕过、上下文污染及权限防护真空。

## 导航

- [协作机制全貌](#mechanism_overview)：了解负责人、协作者和编排器的分工时阅读。
- [致命缺陷与系统挂起风险（P0）](#high_impact)：查 AT-01 至 AT-04 时阅读。
- [协作调度与状态机逻辑漏洞（P1）](#logic_defects)：查 AT-05 至 AT-13 时阅读。
- [数据一致性、恢复机制与持久化风险（P1/P2）](#data_persistence)：查 AT-14 至 AT-18 时阅读。
- [架构与决策层面的矛盾与设计缺陷](#design_paradox)：查 AT-19 至 AT-21 时阅读。
- [修复优先级与演进路线图](#roadmap)：查报告建议的修复顺序时阅读。

<a id="mechanism_overview"></a>
## 协作机制全貌

Janus 的多 Agent 协作采用「负责人（Lead）- 协作者（Collaborator）- 编排器（Orchestrator）」三方分工体系，以事件溯源（Event Sourcing）作为唯一事实来源：

| 协作主体 | 职责与行为约束 | 核心接口与工具 |
| --- | --- | --- |
| **负责人（Lead）** | 以 `/goal` 驱动，负责目标拆解、团队指派、验收核对、决定返工（`retry`）或接受（`accept`）。派出首个子任务后转为纯管理，不再修改工作区代码。 | 看板 MCP 路由提供 10 个管理工具：`team`、`orchestrate`、`wait`、`reply`、`retry`、`accept`、`amend`、`cancel`、`task_detail`、`ask_user`。 |
| **协作者（Collaborator）** | 在独立的 ACP 会话中执行分配的子任务，按验收标准自测，返回包含检查项、方法分级和裁决的结构化结果。禁私聊，一切经由看板。 | 看板 MCP 路由提供 4 个协作工具：`board_post`、`claim`、`peers`、`request_split`。提问与审批交给用户或负责人。 |
| **编排器（Janus）** | 负责机械性工作：DAG 拓扑无环与依赖校验、子进程与会话管理、文件归属冲突检测、结果格式核验、死循环看门狗及状态重放恢复。 | 核心数据结构 `Run` 与 `App`，基于 SQLite `events` 表追加写入并广播，管理独立 worktree 与 Agent 隔离配置目录。 |

### 核心机制亮点

1. **严格的事件溯源架构**：看板和任务状态并非易失内存数据，而是完全由只追加写入的不可变事件流重放得出，具备极高审计性与重启恢复能力。
2. **结构化验收 + 独立验证人（`verifies`）闭环**：要求协作者返回结构化验证证据（方法分级为 `test`、`command`、`inspection`、`reasoning`、`unverified`），且支持负责人指定独立的验证员进行二次交叉核验。
3. **全量编排一次性静态拓扑校验**：`validate_orchestration` 一次性完成有向无环图、依赖闭包、文件所有权（`owns`）冲突与指派人合法性检查，杜绝反复报错打断决策。
4. **会话级令牌派生安全**：看板 MCP 路径采用 `SHA256(run_token, who)` 隔离，协作者只能操作自身会话与任务，无法冒充负责人或跨任务越权。
5. **插话驱动避免长轮询等待**：引入官方 ACP Steering 插话机制，协作者提问、失败、冲突时经 2 秒防抖自动推入负责人当前轮，避免长轮询等待死锁。

---

<a id="high_impact"></a>
## 致命缺陷与系统挂起风险（P0）

### AT-01 Git Worktree 忽略文件提取引发标准管道死锁

**缺陷机制**。在中大型项目（包含庞大的 `node_modules`、`target` 或深度构建产物）中创建 worktree 时，系统同步执行 Git 查询以复制忽略文件。[worktrees.rs:330-346](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/worktrees.rs#L330-L346)

```rust
let mut child = Command::new("git")
    .args(["status", "--porcelain=v1", "--ignored=matching", "-z"])
    .stdout(Stdio::piped())
    .spawn()?;
let status = child.status()?; // 父进程同步阻塞等待子进程退出
let mut out = String::new();
child.stdout.take().unwrap().read_to_string(&mut out)?; // 退出后才读取管道
```

Git 输出的忽略项列表若超出操作系统内核管道缓冲区大小（Unix 默认 64 KB），Git 进程因写满缓冲区而被挂起；而 Rust 父进程正阻塞在 `child.status()?` 等待 Git 退出。父子进程陷入经典管道死锁。

**危害**。导致 worktree 创建流程永久卡死，发起 Run 时整个服务进程假死。

**修复建议**。改用 `child.wait_with_output()?`，或者在单独的异步任务中并发拉取 stdout，避免单向阻塞。

---

### AT-02 ACP 子进程强杀无超时保护与孤儿孙进程泄漏

**缺陷机制**。终止 ACP 连接时，系统对子进程调用两阶段强杀流程。[acp.rs:243-256](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/acp.rs#L243-L256)

```rust
if tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.is_err() {
    let _ = child.kill().await;
    let _ = child.wait().await; // 无任何超时保护
}
```

1. **永久卡死**：若适配器子进程因磁盘 I/O 挂起进入内核不可中断的 D 状态，第二次 `child.wait().await` 无超时，将永久挂起 `kill()`，导致上层 `close_session` 以及所有任务取消流程全部卡死。
2. **孙进程泄漏**：第 253 行的 `child.kill()` 仅向顶层 Node 进程发送 `SIGKILL`，未向进程组（`-pid`）发送。Node 包装器死亡后，其派生的 Codex CLI 或 Claude Code 等重型 Agent 进程沦为孤儿进程，在宿主机后台持续空耗 CPU 与内存。

**修复建议**。第二次 `wait` 必须增加超时保护（如 3 秒）；强杀时向进程组发送 `SIGKILL`（使用 `kill(-pid, libc::SIGKILL)`）。

---

### AT-03 负责人游标提前推进导致阻塞与提问事件黑洞沉没

**缺陷机制**。向负责人会话插话推送看板新消息时，系统在投递前提前更新了游标。[lead.rs:175-193](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/lead.rs#L175-L193)

```rust
st.lead.cursor = last; // 提前推进游标
...
let _ = tx.send(Steer::Message(Msg { id, text })); // 忽略发送错误
```

在将消息真正投递到负责人 ACP 会话之前，游标已经被推进到 `last`。若此时负责人 channel 刚好断开或正在重启，`tx.send` 失败被静默忽略。根据 Janus「已推送的消息算作已读，不会再出现在 `janus.wait`」的规则，由于游标已越过该事件，**这批关键的提问、阻塞、冲突事件既没有推送给负责人，未来也绝不会在 `janus.wait` 中出现**，彻底掉入黑洞。

**修复建议**。必须在确认 `tx.send` 成功后才更新 `st.lead.cursor`；投递失败时保持游标不推进。

---

### AT-04 Worktree 备份异常时仍强行物理删除目录（代码永久销毁）

**缺陷机制**。删除 Run 或服务重启清理已验收 worktree 时，清理逻辑存在执行分支缺陷。[worktrees.rs:261-272](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/worktrees.rs#L261-L272)

```rust
if write(path, &["add", "-A"]).is_ok()
    && let Ok(patch) = write(path, &["diff", "--cached", "--binary", "HEAD"])
    && !patch.trim().is_empty()
{
    // 写入备份 patch
}
drop_worktree(repo, path); // 无条件物理删除整个目录
```

若 `git add -A` 因索引锁（`.git/index.lock`）冲突或权限失败，`if` 条件直接跳过，未生成任何 patch 备份，紧接着直接调用 `drop_worktree` 执行 `git worktree remove --force`。协作者在 worktree 中的所有未提交代码被彻底物理删除且无任何补丁留存。

**修复建议**。若 `git add` 或生成补丁失败，严禁调用 `drop_worktree`，必须中止流程保留现场并向上报错。

---

<a id="logic_defects"></a>
## 协作调度与状态机逻辑漏洞（P1）

### AT-05 `board_orchestrate` 锁间隙（TOCTOU）导致任务 ID 重复与状态撕裂

**缺陷机制**。负责人在看板提交编排时，[board.rs:181-225](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/board.rs#L181-L225) 在 L181-185 拿到 `run.st` 读锁校验编排提案（检查 ID 唯一性与文件重叠），随后立即释放了锁；校验完成后（L219），重新拿写锁将任务推入 `st.tasks`。

在放锁间隙若存在并发重复调用，第二次调用在第一次写锁完成前同样通过了唯一性校验。最终导致两个相同 ID 的任务同时被推入 `st.tasks`。后续 `task_mut` 永远只能匹配到第一个任务，第二个任务的状态流转、产物提交和结算全部错乱。

**修复建议**。将编排方案的静态校验与追加写入合并在同一个持锁临界区内。

---

### AT-06 拓扑排序算法在重复依赖时出现伪环错误（假死拒签）

**缺陷机制**。若负责人下发的子任务 `deps` 中含有重复项（如 `deps: ["t1", "t1"]`）：[board_tools.rs:281-299](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/board_tools.rs#L281-L299)
- 入度计算（L283）：`*indegree += d.deps.iter().filter(...).count()` 将 `indegree` 累加了 2。
- 出队减少入度（L289-291）：`for d in defs.iter().filter(|d| d.deps.iter().any(|x| x == id))` 仅执行了一次 `*n -= 1`。

该任务的入度永远无法归零，永远无法出队，导致拓扑排序误判并向负责人报错「依赖之间有环」。合法 DAG 仅因重复 ID 被无情拒签，而大模型根本找不出所谓的环，陷入反复调整的死循环。

**修复建议**。在进入拓扑排序前，对所有任务的 `deps` 数组执行去重（`dedup`）。

---

### AT-07 验收标准核对纯按下标提取，完全不核对文本（假证据欺骗漏洞）

**缺陷机制**。协作者返回结果核对时：[evidence.rs:342-357](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/evidence.rs#L342-L357)

```rust
for (i, criterion) in accept.iter().enumerate() {
    let Some(c) = checks.get(i) else { ... };
    if !is_verifier && !c.passed { ... }
    else if c.evidence.trim().is_empty() { ... }
}
```

1. **完全无文本比对**：代码仅通过数组下标 `checks.get(i)` 提取对象，从未核对 `c.criterion` 是否与预设的标准文本相符。
2. **假证据绕过**：Agent 只要提交一个长度相等的数组，哪怕内容全填伪造的假任务（例如针对「跑通集成测试」，Agent 提交 `criterion: "随便写写", passed: true, evidence: "ok"`），系统直接无条件判定验收通过。
3. **错位误判**：若 Agent 颠倒了完成顺序，第二条任务的证据会直接对位错绑给第一条任务。

**修复建议**。必须根据 `criterion` 的内容文本进行严格精确或归一化匹配，不能仅依赖数组下标。

---

### AT-08 状态机非法转移在 Release 模式下无条件强推

**缺陷机制**。`Task::set_phase` 检查状态转移合法性时：[orchestrator.rs:428-435](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator.rs#L428-L435)

```rust
if !self.phase.can_transition_to(&to) {
    debug_assert!(false, "illegal phase transition...");
    tracing::warn!(...);
}
self.phase = to; // Release 模式下无条件执行强推
```

在生产 Release 模式下仅记录 warning 日志，随后依然无条件执行 `self.phase = to` 强推流转。同时，重放事件时调用 `Task::restore` 完全绕过 `can_transition_to` 检查。一旦调度边缘情况触发，状态机将被静默破坏。

**修复建议**。非法转移在 Release 模式下应当拒绝流转并返回错误，不能强行赋值。

---

### AT-09 路径未规范化与 macOS APFS 大小写不敏感引发文件并发写入撕裂

**缺陷机制**。文件归属与冲突检测逻辑：[teamwork.rs:21-47](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/teamwork.rs#L21-L47)
1. `read_owns` 未消除相对路径中的 `.` 和冗余斜杠 `//`。任务 A 声明 `owns: ["src/parser.rs"]`，任务 B 声明 `owns: ["src/./parser.rs"]`，前缀匹配失败，`overlaps` 判定不冲突，两个 Agent 同时并发写入同一文件。
2. Janus 运行在 macOS 环境（APFS 默认大小写不敏感且保留大小写）。`overlaps` 使用 Rust 严格区分大小写的字符串比对。任务 A 声明 `src/Utils.rs`，任务 B 声明 `src/utils.rs`，系统判定无重叠并准予并行执行，但在磁盘上修改的是同一个 inode，造成静默代码覆盖。

**修复建议**。路径解析标准化消除 `.` 与冗余斜杠；路径匹配统一按小写归一化比对。

---

### AT-10 任务尝试失败缺乏退避机制（Thundering Retry Burn-out）

**缺陷机制**。尝试遇到非验收异常（如 Agent 启动崩溃、Node 异常或网络闪断）失败时，系统立刻将其重设为 `TaskPhase::Pending` 并瞬间 `notify_one()` 唤醒调度器。[attempts.rs:39-46](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/attempts.rs#L39-L46) 与 [lifecycle.rs:382](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/lifecycle.rs#L382)

若遇到瞬态系统故障（如短暂端口冲突或内存紧张），调度循环将在几毫秒内以极高频率重试，瞬间耗尽全部 `max_attempts`（默认仅 2 次），导致任务被立即判定为永久失败，彻底丧失自愈机会。

**修复建议**。任务重试必须引入最小延迟退避（例如至少等待 3~5 秒或简易指数退避）。

---

### AT-11 上下文 75% 预算判定失效盲目复用超预算会话

**缺陷机制**。系统在判定是否续接会话时：[attempts.rs:336-341](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/attempts.rs#L336-L341)

```rust
let over_budget = self.session_context_ratio(run, &old).is_some_and(|r| r > 0.75);
```

若 ACP 适配器未能在前一轮及时上报上下文用量快照（`usage_update`），`session_context_ratio` 返回 `None`，`is_some_and` 恒为 `false`。系统误判为上下文未超标，盲目调用 `session/load` 续接一个实际已接近 100% 满额的会话，发出后续 prompt 时直接触发模型上下文溢出硬报错崩溃。

**修复建议**。在没有明确用量快照时，根据前序轮次累计 Token 进行保守估算，或在 `continues` 时增加严格兜底。

---

### AT-12 跨任务上下文污染：`st.notes` 全局残留注入无关任务

**缺陷机制**。负责人调用 `board_retry(note)` 时，系统将 `note` 存入 `st.notes` HashMap，Key 仅为协作者的 `agent_id`（而非复合键 `(agent_id, task_id)`）。[attempts.rs:222-226](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/attempts.rs#L222-L226) 与 [board.rs:408](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/board.rs#L408)

若该任务后来被取消，或者该协作者在此之后被指派去执行另一个完全不相关的全新子任务，启动会话时调用 `take_notes`，原先那条针对前一个任务的返工 note 会被无差别注入到全新任务的 system prompt 中，导致协作者在执行新任务时收到错误的旧上下文。

**修复建议**。`st.notes` 改为以 `(AgentId, TaskId)` 复合键存储与提取。

---

### AT-13 看门狗正则抹平行号列号引发死循环误报

**缺陷机制**。看门狗在计算工具调用指纹以侦测死循环时：[lifecycle.rs:770-800](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/lifecycle.rs#L770-L800)

`steady_text` 的时间戳模糊化正则表达式过于宽泛，将错误输出中的 `文件名:行号:列号`（如 `src/parser.rs:42:15` 和 `src/parser.rs:88:20`）全部替换为 `:#:#`。两次针对不同行号位置的编译报错被计算出完全相同的哈希指纹。Agent 在积极修复不同行号的错误时，会被看门狗误判为「连续 5 次执行相同工具且输出无变化」，误报 `task.stalled (loop)`。

**修复建议**。精确匹配时间戳格式，禁止将冒号前后的行号列号数字作为时间戳抹平。

---

<a id="data_persistence"></a>
## 数据一致性、恢复机制与持久化风险（P1/P2）

### AT-14 `recover()` 仅更新索引表却不发射 Attempt 终态事件

**缺陷机制**。服务中断重启时，`orphaned_attempts()` 找出此前未完结的 Attempt，执行 SQL 将其在 `attempts` 表中设为 `interrupted`。[actions.rs:773-781](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/actions.rs#L773-L781)

然而，全流程未向 `events` 表写入任何终态事件。macOS 客户端是纯 Event Sourcing 派生模型（`Derive.swift`）。由于事件流中没有任何 Attempt 终结事件，前端推导出的状态与 SQLite 索引表永久脱节，界面上任务一直显示为卡住的「运行中」。

**修复建议**。在 `recover()` 更新 `attempts` 表的同时，必须调用 `store()` 为每个被中断的 Attempt 追加一条 `attempt.failed` 或 `attempt.interrupted` 事件。

---

### AT-15 挂起的等待人工介入（Pending）在服务重启后直接丢失并全量重跑

**缺陷事实**。所有等待审批、表单和提问的 `pending` 句柄均保存在内存 `st.pending` 中。代码显式注释：「*Everything unfinished restarts from `pending`... a task that was waiting on a person cannot stay waiting.*」 [actions.rs:588-596](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/actions.rs#L588-L596)

一个耗时 30 分钟的任务已执行到 95% 并发起了一个提问，若服务在此期间重启，该任务直接回退为全新的 `Pending` 状态，之前的执行会话全部作废，必须从 0% 全量重新执行。

**修复建议**。将 `pending` 的决策元数据持久化，重启后在收件箱中恢复等待状态。

---

### AT-16 数据库迁移反序列化失败直接 `continue`（半迁移脏数据落库）

**缺陷机制**。遍历历史行更新 JSON 快照时，若旧数据反序列化失败，代码直接 `continue` 跳过。[migrations.rs:125-135](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/migrations.rs#L125-L135)

升级报告提示迁移成功，但损坏行并未被迁移，遗留了旧字段或缺失了新必填字段。在后续业务运行时反序列化崩溃（报 500）。

**修复建议**。反序列化失败时必须立即 `bail!()` 中断迁移并回滚事务。

---

### AT-17 协作者 MCP 接口未设容量与长度上限（DoS 隐患）

**缺陷事实**。协作者 MCP 接口输入未限制容量：[board.rs:1128-1250](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/board.rs#L1128-L1250)
- `board_post`：`text` 字段未作任何长度限制。
- `board_claim`：`paths` 数组未设容量上限。
- `board_request_split`：`subtasks` 数组未设容量上限。

失控或受注入的 Agent 可一次性 claim 上万个路径，或提交几十兆文本，直接塞爆 SQLite 和负责人的上下文窗口，引发 CPU 密集型卡顿。

**修复建议**。强制设置安全上限（如 `text` 最多 4000 字符，`paths` 最多 100 项，`subtasks` 最多 20 项）。

---

### AT-18 随机令牌弱降级逻辑极易被暴力破解

**缺陷机制**。`random_token()` 在读取 `/dev/urandom` 失败时，降级采用 `SHA256(timestamp + pid)` 生成令牌。[board.rs:1306-1313](https://github.com/duoyuli/Janus/blob/63d08050921acc62407516556d00fd9d4642ced1/backend/src/orchestrator/board.rs#L1306-L1313)

时间戳和进程 PID 搜索空间极小，攻击者可预测生成的 Run Token，越权接管看板控制权。

**修复建议**。移除弱伪随机降级，改用标准加密随机数库，若随机数生成失败直接 panic 或报错。

---

<a id="design_paradox"></a>
## 架构与决策层面的矛盾与设计缺陷

### AT-19 权限防护真空（「不校验权限」与「默认不受限运行」结合）

ADR 声明 Janus 自身不校验权限，完全依赖 Agent 原生模式；同时为保证无人值守，默认采用 `Bypass permissions`（Claude Code）与 `Full access`（Codex）。系统底层放弃了事前沙箱防护。若 Agent 遭遇外部恶意注入（如恶意的第三方 Skill、未受信任的 MCP 工具返回值），Agent 拥有完全宿主机执行权限，可随意读写网络和系统。

### AT-20 资源库全员全开与「负责人只做管理」的割裂（MCP 副作用脱离监管）

ADR `20261120-library_routed_only` 废除了权限收口，所有具备写权限和网络副作用的 MCP 工具对所有成员开放。但负责人派发后不得改动代码。Janus 在 MCP 协议层无法感知工具副作用；如果负责人或验证员调用了有副作用的 MCP 工具，这些操作不会产生 Agent tool call 痕迹，导致文件越界检测和对账机制彻底失效。

### AT-21 为规避幻觉抹除模型信息，牺牲了「异构模型交叉验证」质检杠杆

为避免负责人产生偏见，系统抹去了协作者的模型信息，并移除了跨模型审查指引。这实质上放弃了多 Agent 协作中最关键的质量收益——让不同底层模型（如 Claude 与 OpenAI）互相挑刺以消除单一模型的认知盲区。

---

<a id="roadmap"></a>
## 修复优先级与演进路线图

```
┌──────────────────────────────────────────────────────────────┐
│ P0 紧急修复 (稳定性与可用性基线)                               │
│  - AT-01: worktrees.rs 管道读写死锁修复                        │
│  - AT-02: acp.rs 进程组强杀与二次 wait 超时保底                │
│  - AT-03: lead.rs 负责人游标提前推进导致的事件黑洞             │
│  - AT-04: worktrees.rs 备份失败强删工作区的破坏性逻辑          │
└──────────────────────────────┬───────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│ P1 逻辑与状态机收敛 (协作准确性)                               │
│  - AT-05: 编排 TOCTOU 竞态合并临界区                           │
│  - AT-06: 拓扑排序前 deps.dedup() 消除假环                     │
│  - AT-07: 验收证据严格按 criterion 内容比对                   │
│  - AT-08: 状态机非法流转在 Release 下显式拦截                 │
│  - AT-09: teamwork 路径标准化与大小写归一化                    │
│  - AT-10: 尝试重试引入指数退避                                 │
│  - AT-11: 续接会话预算保护兜底                                 │
│  - AT-12: st.notes 按 (AgentId, TaskId) 精准隔离               │
│  - AT-13: 看门狗行号列号正则修正                               │
└──────────────────────────────┬───────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│ P2 架构韧性与边界强化 (系统防护与演进)                         │
│  - AT-14 ~ AT-16: recover 发送中断事件、pending 持久化、迁移中断 │
│  - AT-17 ~ AT-18: 看板 MCP 接口 payload 配额与强加密随机数    │
│  - AT-19 ~ AT-21: 引入 MCP 副作用声明与异构模型交叉核验机制   │
└──────────────────────────────────────────────────────────────┘
```
