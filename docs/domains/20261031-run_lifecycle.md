# Run 生命周期

本文说明 Run 和任务的状态机：创建、调度、尝试、验证、重试、交付、用户操作和恢复。负责人与协作者之间的协作规则见[目标模式与共享看板](20261031-goal_mode.md)，事件字段见[事件参考](../interfaces/20261031-events.md)。

## 导航

- [创建 Run](#create_run)、[派发队列](#dispatch_queue)：修改 `create_run`、快照内容，或同时在做的 Run 数和排队规则时阅读。
- [图片附件](#attachments)：修改 `backend/src/attachments.rs`、目标或消息的图片、提示中的图片块时阅读。
- [工作目录](#workspace_model)：涉及任务在哪里读写、diff 和 Git 时阅读。
- [任务调度](#scheduling)、[一次尝试](#attempt)、[验证](#verification)、[重试、返工与失败](#retry)：修改调度循环、尝试流程或 `retry` 时阅读。
- [停滞检测](#watchdog)：修改 `watchdog`、`watch_call`，或调整什么算「没有进展」时阅读。
- [交付条件](#delivery_conditions)：修改 `execute` 的结果判断时阅读。
- [Run 状态](#run_statuses)、[用户操作](#user_actions)、[中断与恢复](#recovery)：修改 Run 状态的含义、停止和暂停等操作，或服务重启后的恢复时阅读。

每个目标 Run 都以负责人用 `/goal` 推进目标开始，任务全部来自负责人的编排，见[目标模式与共享看板](20261031-goal_mode.md)。和一名数字员工的对话也是 Run（`kind` 为 `chat`），没有看板、调度和交付，规则见[对话](20261120-chats.md)；工作区值班的会话也是（`kind` 为 `loop`），见[工作区值班](20261121-loops.md)；本文其余部分说的都是目标 Run。目标 Run 由用户在工作区首页提交，或由值班派发（`origin`），两者走同一个[派发队列](#dispatch_queue)。调度循环（`execute`）只负责执行看板上的任务并判断何时交付。

<a id="create_run"></a>
## 创建 Run

每个目标 Run 都先进[派发队列](#dispatch_queue)，有空位时立即开始。`App::create_run`（`orchestrator/queue.rs`）依次执行：

1. 目标不能为空（只附了图片时目标记为「按附图完成」），工作区和工作目录必须存在，每个成员都能构建运行环境（`spec_for`）；缺少 CLI 或适配器没有安装时直接报错，不排队。
2. 保存随目标粘贴的图片，写入状态为 `queued` 的 Run（快照只有工作区、`goalImages` 和可选的 Token 展示参考值 `tokenLimit`），记一条 `run.queued`。
3. 有空位时这个 Run 立即离开队列并开始，开始完才返回，和不排队时一样；否则留在队列里，返回 id。

开始（`start_run`）时按工作区当时的设置准备：

1. 重新读取工作区和团队，检查目录和每个成员的运行环境；工作区已被删除或 Agent 不能用时，Run 记为 `failed`（`run.failed`，「任务没能开始」）。
2. 判断工作目录是不是 Git 仓库；是则记录当前 `HEAD` 作为基线和当前检出的分支（验收时合并回这里），并为这个 Run 创建 worktree（`worktrees::create`），见[工作目录](#workspace_model)。
3. 复制 Skill，写入完整的 Run 快照（团队、是否 Git、基线、工作区、worktree），见[数据模型与存储](../architecture/20261031-data_model.md#snapshots)和[图片附件](#attachments)。
4. 启动驱动任务，记 `run.created`，进入负责人的目标会话（`goal`）。

Run id 是 `r` 加当前毫秒时间戳的 base36 编码（`agents::new_id`）；同一毫秒内创建的 Run 依次加一，id 不会重复。

Run 快照中的工作区带着开始时的[共享资料](20261031-agents_and_roles.md#workspace_brief)，之后修改工作区的共享资料不影响这个 Run，从中断处继续时也使用快照中的版本。

<a id="dispatch_queue"></a>
## 派发队列

一个 Janus 服务只有一个派发队列，跨所有工作区，先进先出（`orchestrator/queue.rs`）。它限制的是同时在做的目标 Run 数，和 Run 内部的[并发上限](#scheduling) `max_concurrent_attempts` 是两层。

- **槽位**：`max_running_runs`（`JANUS_MAX_RUNNING_RUNS`，默认 8，范围 1 到 32，见[配置](../interfaces/20261031-configuration.md#settings)）。只有状态是 `planning` 或 `running`、且没有暂停的目标 Run 占槽位；已交付、已验收、已中断的 Run，对话，暂停中的 Run 都不占（它们开着的 Agent 进程算在[座位](#agent_seats)里）。
- **排队的 Run** 只有记录和 `run.queued` 事件：没有 worktree、团队快照和会话，团队、资源库和基线都在开始时才确定。队列就是状态为 `queued` 的 Run 按创建时间排序，不另存，服务重启后原样保留。
- **进队**：Run 的记录和第一条事件 `run.queued`（序号 1）在同一个数据库事务里写入，进队之前没有任何人能把它领走。
- **补位**（`pump`）：有 Run 状态变化（`set_status`）、暂停或继续时，把空出的槽位按顺序交给排队最久的 Run。数不清槽位（读数据库出错）时这一次不派。领走是带条件的更新（只改仍是 `queued` 的行）加一条 `run.starting`，在同一个事务里；没领到就不开始，所以同一个 Run 不会被开始两次。服务在开始监听之后才补位，启动失败不会消耗队首。
- **有空位时准备好才返回**：创建请求自己的 Run 在这个请求轮到补位之前不会被别的补位领走（`App.fresh`），所以有空位时请求返回时 Run 已经准备好、在运行；没轮上就立即再补位一次，不会空等。
- **准备没完成的 Run**：领走后、写入快照前服务停了的 Run，重启时回到队列原来的位置（`run.requeued`），不记为中断；准备失败（工作区不在、Agent 不能运行）的记为 `failed`，用户「从中断处继续」时重新排队。这类 Run 没有快照，不能直接恢复，也不能发消息。开始准备时如果发现上次留下的同名 worktree，先移走再重建。
- **用户操作**：排队的 Run 只接受 `start`（立即开始，不等空位）和 `cancel`（取消，`run.cancelled` 和状态在同一个事务里），其余操作返回 409；已经被领走时这两个操作也返回 409。也可以删除。「从中断处继续」和给中断的任务发消息同「马上开工」一样，是用户直接要求开始，不等空位。暂停一个 Run 会让出槽位，继续后它照常进行，这时同时在做的 Run 可能超过槽位数，新的 Run 继续排队直到有空位。
- **排位**：`GET /api/runs` 和 `GET /api/runs/{id}` 中排队的 Run 带 `queuePosition`（1 是下一个），和状态在同一条查询里算出，见 [HTTP API](../interfaces/20261031-http_api.md#run_list)。删除 Run 也发全局事件流通知，排在后面的 Run 的排位随之更新。

<a id="agent_seats"></a>
## Agent 进程的上限与闲置回收

每个 ACP 会话是一对常驻进程（适配器和 Agent，闲着时一对 Codex 约 250 MB 内存）。派发队列和 `max_concurrent_attempts` 管「同时做多少事」，这里管「同时开多少进程」，对目标 Run、对话和值班一起算（`seats.rs`、`orchestrator/parking.rs`），见 [ADR](../decisions/20261122-agent_seats.md)。

- **座位**：`max_agent_sessions`（`JANUS_MAX_AGENT_SESSIONS`，默认 12，范围 1 到 64，见[配置](../interfaces/20261031-configuration.md#settings)），实际取它和 `max_running_runs + 1` 中较大的：在做的目标 Run 的负责人一直占着座位，至少留一个给它们的协作者。每个会话在启动进程前拿一个座位（`open_session` 的 `take_seat`），进程被关掉或释放时还回。配置检查和登录这类一次性的进程不占座位。
- **满了的时候**：先请闲得最久的、可以关掉的会话让出座位（`Steer::Park`），新会话等空位，它的 Agent 状态是「等空位」（`agent.status`，`phase` 为 `waiting_for_seat`）；空出的座位按谁在等分先后：你在等的（对话或交付后的消息、「现在开工」、回答值班的问题、手动继续任务）最先，按时间表来的（排到的目标、值班按时开工）其次，协作者的尝试最后，同一类按先后。等的时候 Run 被取消或删除就不再等；每 10 秒再找一次可以关掉的会话。
- **闲置回收**：会话一轮结束后闲着超过 `IDLE_AFTER`（10 分钟），且 Run 里没有别的事需要它时，关掉它的 Agent 进程（`park`）：对话、两次开工之间的值班，以及目标已经完成、没有任务在跑的目标 Run 的负责人。目标还在做（负责人在等协作者）、这一轮还没结束、人工接管中的会话都不关。
- **关掉以后**：记一条 `session.parked`（`data.reason` 为 `idle` 或 `full`）；对话和值班的会话 Run 记为 `interrupted`，和服务重启后一样，界面上显示已关闭，退出 Janus 时不算在进行中；目标 Run 的状态不变。下一条消息或下一次开工照常打开它（`resume_run_by`），能加载原会话就加载，否则带上之前的对话新开；关闭过程中发来的消息等它离开内存后进入新会话，没送达的消息在重新打开时补上。值班的提问不受影响。

<a id="attachments"></a>
## 图片附件

用户可以在目标和发给会话的消息里粘贴图片。实现在 `backend/src/attachments.rs`，接口见 [HTTP API](../interfaces/20261031-http_api.md#images)。

- **保存**：图片随请求以 base64 上传，按文件头识别类型（PNG、JPEG、GIF、WebP，声明的类型不作数），每张最多 10 MB，一次最多 10 张；任何一张不合格时整条请求拒绝，不保存任何文件。文件写入 `~/.janus/data/runs/<run>/attachments/`（对话在 `data/chats/<run>/attachments/`），和这个 Run 的 Skill 快照放在一起。
- **点名**：Agent 收到的文字中，每张图片单独占一行 `[Image N] <路径>`。只有图片的消息会带英文查看提示。目标的图片写进负责人说明，并提示负责人把路径写进任务说明，方便协作者参考。
- **发送**：每次 `session/prompt` 和 `_session/steering` 发出前，`prompt_blocks` 找出文字点名的附件，在 Agent 声明了 `promptCapabilities.image` 时作为 ACP 图片块附在文字后面。只读取 `runs/*/attachments/` 下的文件，所以文字里看起来像附图行的内容不能让 Janus 发出任意文件。官方 Claude 和 Codex 适配器都声明了这项能力；没有声明的 Agent 只收到路径，由 Agent 自己打开文件。
- **跟随消息流转**：图片行就是消息文字的一部分，所以插话、排队、留言、交还托管和中断后恢复的消息都带着它们，不需要另外处理。
- **事件**：`run.created` 和 `user.message` 的 `data.images` 列出保存的图片（`path`、`mimeType`、`name`、`size`），事件只记录路径，不包含图片数据。

<a id="workspace_model"></a>
## 工作目录

- **Git 仓库（有提交）**：每个 Run 在自己的 worktree 中运行（`crate::worktrees`），位置是 `<Janus home>/work/runs/<run>`，从创建时的 `HEAD` 开始、处于 detached HEAD、用 `git worktree lock` 锁住。Run 的负责人和所有子任务共用这个 worktree，改动彼此立即可见；同一个仓库上的其他 Run 和用户自己的目录都不受影响。`runs.cwd` 是 worktree 的路径（规范化后的真实路径，和 Agent 自己的工具报告的路径一致），`runs.config.worktree.repo` 是原仓库，`runs.config.worktree.branch` 是创建时检出的分支（处于 detached HEAD 时为 `null`）。
- **worktree 里有什么**：仓库跟踪的文件，加上原仓库根目录 `.worktreeinclude`（`.gitignore` 语法，用户自己维护）列出、并且被忽略的文件，创建时复制过去（跳过符号链接，不覆盖已有文件；文件名不限于 ASCII），复制了哪些记在 `run.created` 的 `data.worktree.included`。用户目录里没提交的改动不带过去，创建前提醒（[创建前的提醒](../interfaces/20261031-http_api.md#run_preflight)）。依赖、构建产物等不复制，负责人说明和任务提示告诉 Agent 按项目自己的方式在 worktree 里准备环境、改了依赖就重新安装。
- **验收**：Janus 在 worktree 里提交全部改动，再把结果带回创建时检出的分支（`worktrees::accept`）：分支没动过时直接快进；有了新提交时先在 worktree 里把任务自己的提交（任务开始或上次验收之后的）变基到最新的分支上再快进，历史是线性的，不建 `janus/` 分支。分支必须仍在任务开始时（或上次验收带到）的提交上，或者从那里往前走了；被回退、强推或重新建过时验收失败，不会把用户删掉的历史带回来。分支检出在用户的目录（或用户的其他 worktree）时在那里 `git merge --ff-only`，文件随之更新，和任务无关的未提交改动保留；没有检出时只移动分支。变基冲突、源分支被删除、用户目录里未提交的改动会被覆盖、任务新建的文件在用户目录里已经有一个没纳入版本管理的同名文件（包括被忽略的，例如 `.env`；git 快进时会悄悄覆盖被忽略的文件，所以 Janus 先查）时，验收失败（400），Run 保持 `delivered`，用户的目录和分支不变，结果留在 worktree 的提交里，错误信息给出提交号或重叠的文件。创建时处于 detached HEAD 的 Run，只在工作目录仍在原来的提交（或上次验收带到的提交）上时快进它，否则同样失败。Janus 的所有 Git 写操作都带 `-c core.hooksPath=/dev/null`，不运行仓库的钩子。worktree 始终 detached。负责人的对话还开着时 worktree 保留，继续对话就接着在里面改，再验收时再带回一个提交；没有会话在用时（例如服务重启后验收）验收后移除，移除失败只记日志（结果已经带回）。服务启动时（`recover`）移除已验收 Run 留下的 worktree，并修复 git 读不了的 worktree（Janus home 移动时没修好的，`worktrees::heal`）；之后继续这个 Run 时，`resume_run` 从最后一次验收的结果所在的提交（`risk.accepted` 的 `data.result`；分支早已包含结果、这次没带回提交时也是它；没有就从基线）重建 worktree（`worktrees::ensure`）。worktree 目录被外部删掉时，重建先清掉仓库里那一条（加了锁的）残留记录再建，只动这一条，不 `prune` 整个仓库，用户自己的其他 worktree 记录不受影响。
- **删除 Run**：移除 worktree，还没提交的改动先存成补丁 `<Janus home>/data/deleted-runs/<run>.patch`；补丁不含被忽略的文件（例如复制进来的 `.env`），以免凭据进补丁。暂存、读出改动、写补丁或移除 worktree 任何一步失败时，worktree 和 Run 都保留，删除返回失败原因，排除后再删一次（`worktrees::remove`）。原仓库被移动或删除、git 读不了这个 worktree 时，整个目录原样移到 `data/deleted-runs/<run>/`，Run 照样能删；这样的 Run 也不能再从中断处继续（重建 worktree 时说明仓库不在了）。停止的 Run 保留 worktree，可以查看，但不能从中断处继续；失败和中断的 Run 保留 worktree，可以从中断处继续。
- **工作区是仓库中的一个目录**（例如 `apps/client`）：worktree 仍是整个仓库的，会话从 worktree 里对应的目录开始（`runs.cwd` 是这个目录，worktree 的根是 `<Janus home>/work/runs/<run>`），所以工作区里的相对路径在任务中指向同一个文件。验收、删除和重建都按整个 worktree 进行。
- **依次写入**：`git worktree add` 会读同一仓库所有 worktree 的元数据，另一个正在建的还没写完时会失败；两个任务同时验收进同一个分支会争它的提交。所以 Janus 里对 worktree 和原仓库的写操作（新建、重建、验收、移除、修复）都依次进行（`App.worktree_writes`）。
- **创建失败不留东西**：`git worktree add` 之后加锁或复制 `.worktreeinclude` 的文件失败时，worktree 当场收回；worktree 建好之后，资源包或图片保存失败时，这次的 worktree 和 `data/runs/<run>/` 目录一并收回，不留下没有记录的 worktree。用户另一个检出这个分支的 worktree 目录被手动删掉（没有 prune）时，Janus 当作分支没有检出，只移动分支。
- **验收的前提**：worktree 必须还在。它被外部删除时验收失败，Run 保持 `delivered`，提示到 `data/deleted-runs/` 找补丁；不会当作「没有改动」验收通过。创建时处于 detached HEAD 的 Run，验收时工作目录必须仍在创建时的提交（或上次验收带到的提交）上，用户切到别的提交（包括父提交）时不动它，提示自己合并。
- **非 Git 目录**（以及还没有提交的仓库、裸仓库）：负责人和所有任务直接在工作区目录中运行，没有 diff 产物。同一个目录上已有进行中的 Run 时只提醒，不阻止。
- 一个任务能否修改文件，完全由它的 Agent 权限模式决定，见 [Agent、角色与工作区](20261031-agents_and_roles.md#permission_modes)。负责人和协作者都不提交、不推送；只有用户验收时由 Janus 提交并合并回源分支。Janus 只隔离文件，端口、本机数据库这类共享资源由项目自己避免冲突。

设计取舍见 [ADR：Git 仓库上的每个任务在自己的 worktree 中运行](../decisions/20261119-run_worktrees.md)、[ADR：worktree 写入用户仓库的边界](../decisions/20261119-worktree_trust_boundary.md) 和 [ADR：验收时把结果合并回任务开始时的分支](../decisions/20261120-merge_back_on_accept.md)。

<a id="scheduling"></a>
## 任务调度

<a id="task_states"></a>
### 任务状态

| 状态 | 含义 | 终态 |
| --- | --- | --- |
| `pending` | 等待依赖完成或等待执行者空闲 | 否 |
| `running` | 有一次尝试在进行 | 否 |
| `blocked` | 依赖失败，无法开始；所有依赖都不再是失败状态（被重试或接受）后自动回到 `pending` | 否 |
| `succeeded` | 成功；负责人用 `retry` 让它返工时回到 `pending` | 是 |
| `failed` | 用完尝试次数 | 是 |
| `cancelled` | Run 被取消时正在执行 | 是 |
| `skipped_by_decision` | 被负责人取消（`cancel`）；连带取消的任务随上游的 `retry` 回到 `pending` | 是 |

这 7 个是调度器写出的状态（`TaskStatus`，`backend/src/status.rs`），合法转移集中在 `TaskStatus::can_transition_to`。调度器内部用 `TaskPhase` 记录任务状态，只在某个状态下才有的数据放在这个状态里：`running` 带它当前的尝试，`blocked` 带它在等的失败依赖，`skipped_by_decision` 带连带取消它的上游任务。离开一个状态时这些数据随之消失，不会残留到别的状态。任务最近一次完成的位置（`last_success_seq`）是历史，返工后仍保留，用来判断下游拿到的是不是旧结果；「现在是否已完成」只看状态。应用另外从事件推导几个只用于显示的状态，它们不参与调度：`ready`（`task.ready`）、`assigned`（`task.assigned`）、`verifying`（`verification.started`）、`verification_failed`（`verification.failed`）、`waiting_input`（`question.asked` / `input.requested`）、`waiting_approval`（`approval.requested`）。重启恢复时以看板重建出的 7 个状态为准。应用中这 13 个状态是 `TaskStatus` 的成员（`frontend/Sources/Janus/Core/Status.swift`），Run 的状态是 `RunStatus`；进行中、已结束、可以从中断处继续这些分组也在那里定义，界面不再各自列字符串。

### 启动条件

一个任务可以启动，需要同时满足：

- 状态为 `pending`。
- 所有依赖（`deps`）都已 `succeeded`，并且没有被未通过的独立验证挡住（见下文）。
- 执行者当前没有别的任务在执行。每个成员实例同一时间只做一个任务，派给同一实例的任务排队；工作区中配置了 `slots > 1` 的角色会展开为多个成员实例，可并行执行不同任务，见 [Agent、角色与工作区](20261031-agents_and_roles.md#workspaces)。
- 当前正在运行的协作者尝试总数低于 `max_concurrent_attempts`（默认 3，见 [ADR：角色并发槽位与全局并发上限](../decisions/20261118-role_slots_and_concurrency_cap.md)）。超出上限时，已就绪的任务保持 `pending`，等有槽位空出后自动启动。
- Run 没有暂停、没有取消。

依赖失败（失败、取消、阻塞或被跳过）时，下游任务变为 `blocked`（`task.blocked`）。有多个依赖失败时，处理好一个后任务仍保持 `blocked`，并再写一条 `task.blocked` 指向还没处理的依赖。

独立验证任务判定 `rejected` 或 `inconclusive` 时自身是 `succeeded`，但被验证的工作没有得到确认。在判定写入之前就编排好、依赖这个验证任务的任务，是按「验证会通过」安排的，同样变为 `blocked`（`task.blocked` 带 `data.verdict`），不会在没确认的工作上继续；负责人读到判定之后再编排、依赖它的任务（例如修复任务）照常开始。判断依据是任务写入看板时的事件序号（`Task::planned_seq`）和验证任务 `task.succeeded` 的序号（`DepGate::holds`），重启后从 `orchestration.accepted` 的序号重建。

尝试结束、负责人 `cancel` / `retry` / `accept` 都会改任务状态。尝试结束时，确认这次尝试仍是任务当前的尝试（`running` 状态带的尝试 id）和写入结果在同一把锁内完成；负责人已经取消或重试了这个任务时，这次尝试的结果按过期处理（`attempt.late_result`），尝试记录标为 `cancelled`，不会覆盖负责人的决定。

<a id="attempt"></a>
## 一次尝试

每次尝试是一个新的 ACP 子进程和会话（`App::attempt`）：

1. 打开会话（或续接原会话），默认调用 `session/new`，有两种续接：
   - 从中断处恢复未完成的尝试：Agent 支持 `loadSession` 时直接 `session/load` 加载被中断的会话，不看上下文占用；加载失败或不支持时新建会话。
   - 续接已经结束的会话：任务声明了 `continues`（只在首次尝试，加载前置任务的会话）；负责人 `retry` 选了 `session: "continue"`（加载这个任务自己上一次的会话）；独立验证因被验证的任务返工而复审（加载验证者自己上一次的会话）。目标会话最新上下文占用严格大于 75%、Agent 不支持 `loadSession`、加载失败，或要接着的任务从来没开过会话（例如开始前就被取消）时新建会话，否则 `session/load` 复用原会话；新建还是复用、新建的原因都记在 `attempt.started` 的 `data.continuation`（见 [ADR：热会话续接](../decisions/20261118-session_continuation.md)）。这次尝试为什么开始（负责人的重试或复审）记在 `data.reopen`。

   接入角色配置的 MCP 服务器；Agent 声明支持 HTTP MCP 时，还接入共享看板。
2. 按角色设置模型、推理强度和权限模式等会话控制项，见 [ACP 集成](../interfaces/20261031-acp_integration.md#session_setup)。
3. 发送任务提示（`task_prompt`）。回到这个任务自己的会话时（`retry` 的 `continue` 或复审），会话里已经有任务说明，只发新的内容（`continued_prompt`）：验证意见或上一轮的判定、负责人的说明、改过的说明、上次失败的原因和验收标准，见[重试、返工与失败](#retry)。其他情况发完整的任务提示：总目标、工作区共享资料（有时，与任务说明冲突时以任务说明为准）、任务说明、验收标准、工作目录、上游结果（含一句话摘要、专门给下游的 `handoff` 交接说明、前 5 条上游风险、负责人的修订、改过的文件及后续被他人或负责人修改的提醒）、同时进行的任务和上次失败原因；独立验证任务还会收到被验证任务的专属 diff 路径、实际改动文件、痕迹核对异常、弱证据条目提醒和风险（`teamwork::VerifiedContext`）。怎样验证、提问和返回结构化结果的约定，以及角色指令和 Provider 的自定义提示词，都在会话的[系统层](../interfaces/20261031-acp_integration.md#standing_instructions)中，见[协作者的约定](20261031-goal_mode.md#collaborator_rules)；只有没有系统层入口的 Agent 才附在任务提示末尾。
4. 收集流式输出、工具调用、审批请求和表单请求。单轮超时发送 `session/cancel` 后，仍等待最多 2 秒读取返回值中的 `prompt_usage`；每轮发送前记录事件序号和会话日志末尾位置。若尝试异常结束且本轮尚未上报 Token，则从 Janus 隔离目录（Claude Code 的 `CLAUDE_CONFIG_DIR`，即 `~/.janus/data/agents/<id>/config`）下的 `projects/**/<sid>.jsonl` 读取本轮新增记录，按 `message.id` 取最后一次读数补齐用量。同一会话前面几轮的读数照常保留。
5. 解析回复中最后一个 ```` ```json ```` 代码块作为结构化结果。

<a id="structured_result"></a>
### 结构化结果

```json
{
  "summary": "一句话结果",
  "handoff": "给下游子任务的接口约定、关键决策与注意事项（没有下游依赖时可填 null）",
  "verdict": "approved",
  "checks": [{ "criterion": "验收标准原文", "passed": true, "method": "test", "evidence": "怎么验证的、看到了什么" }],
  "evidence": ["其他可核对的证据"],
  "risks": ["已知风险，没有就留空数组"],
  "question": null
}
```

- `handoff` 可选（最长 1200 个字符），供上游执行者写给下游的接口签名、边界约定和注意事项，与给负责人扫读的一句话 `summary` 分离。
- `verdict` 供独立验证任务（带 `verifies`）填写对被验证产物的判定：`approved`（达标）、`rejected`（存在阻断问题，需原执行者修改）或 `inconclusive`（当前环境证据不足）；未填时按 `checks` 是否全部通过推导（任一条 `passed: false` 推导为 `rejected`），填写了允许值以外的内容时核对报错。
- `checks` 按验收标准的顺序逐条给出，验证方式由 Agent 自己决定。
- `method` 可选，说明这条依据的类型：`test`、`command`、`inspection`、`reasoning`、`unverified`，含义见[证据方法](../interfaces/20261031-events.md#evidence_methods)。省略它的结果没有方法，解析、恢复、核对和展示都按「未说明」处理，不当作 `reasoning`，也不因此判失败；存在但不是允许值时，原样保留在结果里并由核对报错，不会静默换成合法方法。证据方法由 `backend/src/evidence.rs` 解析和汇总，结构化结果的读取（`parse_result`）、事件里结果的重建和恢复（`resume_run`）都用同一段逻辑。
- 没有 JSON 块时，摘要取回复第一行，并记一条风险「Agent 没有按约定返回结构化结果」。
- `question` 不为空表示 Agent 在提问：`{"text": "问题", "options": ["A", "B"], "recommend": "A"}`。回答作为下一轮提示发回同一个会话。同一次尝试最多连续普通提问 4 轮（伴有 `request_split` 拆分提议的暂停不计入这 4 轮，总暂停硬上限为 8 轮）。提问由谁回答见[提问的流转](20261031-goal_mode.md#question_routing)。

<a id="verification"></a>
## 验证

结果提交后，Janus 核对结果（`verify_result` / `evidence::gaps_for`，`verification.*` 事件）。Janus 不运行任何验证命令，验证由协作者按验收标准自己完成，见[验收标准](../interfaces/20261031-board_mcp.md#acceptance_criteria)。核对将「任务自身是否完成了核查工作」与「独立验证者对目标产物的审批结论」分开：

- `summary` 不能为空。
- 普通执行任务：每条验收标准都有对应的 check（按顺序），check 有证据，且没有 `passed: false`。
- 独立验证任务（带 `verifies`）：每条标准都必须核对并给出具体证据，`verdict` 若填写必须是 `approved`、`rejected` 或 `inconclusive`，且 `verdict: "approved"` 不能与 `passed: false` 矛盾。只读验证者发现真实缺陷并给出可复现证据、填 `passed: false` 或 `verdict: "rejected"` / `"inconclusive"` 时，它的核查工作已经完成，第一轮即通过结果结构核对并标记验证任务完成（`task.succeeded`），不会触发无意义的自我修复轮；同时在看板和交付中把目标产物标为未通过确认（`[验证未通过]`、`Verification.passed == false`），唤醒负责人派修复任务和复验任务。
- check 给出 `method` 时必须是允许的值；其他值作为「不是允许值」的失败项报出，不会被改成合法方法。
- `reasoning`、`unverified` 和没有 `method` 只记提醒，不阻断：提醒随核对结果记录在 `verification.*` 和 `task.succeeded` 的 `data.acceptance` 中，看板的完成消息、`task_detail` 和交付都显示。

验证没通过时，同一次尝试中按以下顺序处理：

1. **在原会话里补一次**：把没核对、没证据或没达到的标准逐条发回同一个会话（`session.prompt`，`channel: "fix"`），请协作者修复后重新验证、补上证据；确实无法达到的如实写明原因。协作者保留着上下文，比开新会话从头做快，也更容易修对。补完后重新提交结果、重新核对。
2. **仍然没通过**：任务失败（`task.failed`，`reason: "verification"`），不自动开新会话重试，由负责人决定接受结果、修改后重试（可以选回到这个会话）还是取消，见[看板 MCP 工具](../interfaces/20261031-board_mcp.md#accept_tool)。

用户在会话中直接以当前结果结束时，不走修复这一步。

<a id="retry"></a>
## 重试、返工与失败

- 每个任务最多尝试 2 次（`MAX_ATTEMPTS`）。尝试失败（Agent 进程退出、超时、自行取消等，不含验证没通过）后还有剩余次数时，先等 3 秒（`RETRY_AFTER`，即 `Config.retry_after`），让启动失败、连接中断这类短暂的原因过去，再让任务回到 `pending`，下一次尝试带上上次失败的原因（`task.retry_scheduled`），新开会话。等待期间负责人的取消或重试照常生效。
- 用完次数或验证没通过后任务变为 `failed`（`task.failed`），Run 不停下，由负责人在看板上读到后用 `retry`、`accept`、`cancel` 处理。负责人已经派出了子任务，不自己接手（见[负责人的职责](20261031-goal_mode.md#lead_role)）。被它阻塞的任务不需要重新编排。
- 只有负责人能让结束的任务再做一次（看板的 [`retry`](../interfaces/20261031-board_mcp.md#retry_tool)）：失败、已取消、被负责人取消的任务，以及已完成但需要返工的任务。重试重新给满尝试次数（`task.reopened`，`reason: "lead.retry"`），依赖它的 `blocked` 任务在它成功后自动恢复。用户想重做某个任务时，给负责人发消息。
- **会话由负责人每次选择**：`session` 必填，`continue` 回到这个任务自己上一次的会话，`new` 新开会话（`SessionChoice`）。不设默认值，让负责人每次判断原来的上下文还有没有用。一个协作者可以同时有多个会话，每个任务一个；返工回到的是被返工任务自己的会话。
- **返工**：独立验证判定没通过时，Janus 不自动返工，由负责人判断问题出在哪个任务，用 `retry` 发回原来的执行者，`from` 指向这个验证。验证的判定、没通过的标准和证据、交接说明和风险原样进入提示，负责人只在 `note` 里写自己的判断，不新建子任务。
- **复审**：被返工的任务已完成的独立验证，以及 `from`，回到 `pending`，在被返工的任务完成后重新核对（`task.reopened`，`reason: "target.reworked"`）。`from` 验证的不是被返工的任务（例如问题在上游）时，把被返工的任务加入 `from` 的依赖。复审回到验证者自己的会话，提示附上它上一轮的判定、被验证任务这次的结果，以及现在和上一轮的 diff 产物；它先核对上一轮的问题，再做回归，验收标准仍逐条回答。复审期间，因原判定被挡住的下游任务回到 `pending` 等待；复审通过后开始，再次没通过时重新被挡住。
- **用了旧结果的下游**：依赖被返工任务的其他任务不会被重做或停下。还没开始的自动等返工完成；正在执行的收到一条负责人消息，说明上游在返工、文件可能会变（编排时 `owns` 按上下游先后执行校验，返工让两者可能同时改同一批文件）；已经完成的列给负责人。是否让它们返工或取消，由负责人决定，和 `amend` 修订结论时的处理相同。
- 重开的原因、会话选择和 `from` 是任务上的 `Reopen`（`Retry` 或 `Reverify`），在下一次尝试开始时取走；服务重启时从最后一条 `task.reopened` 重建，之后已经有尝试开始的不再重建。设计取舍见 [ADR：返工由负责人用 `retry` 发回原任务](../decisions/20261119-rework_in_original_session.md)。
- 被打断而没有任何输出的一轮不保存回复产物。
- 尝试结束后，它名下未处理的提问、审批和表单被撤回（`decision.withdrawn`）。过期尝试返回的结果被忽略（`attempt.late_result`）。

<a id="timeouts"></a>
### 时间限制

工作本身没有时间和费用上限：一轮提示、一次尝试、一个 Run 做多久都可以，用量和费用只观测、不触发暂停。卡住的会话由[停滞检测](#watchdog)报告给负责人（负责人自己卡住时报告给用户），由负责人或用户决定继续等、打断还是取消。下面几项只限制进程和控制请求有没有挂死：

| 限制 | 值 |
| --- | --- |
| Agent 启动和打开会话 | 10 分钟 |
| 单个 ACP 控制请求（打开会话、切换模式等；`session/prompt` 不限时） | 30 分钟 |
| 发送打断等控制指令 | 15 秒 |

<a id="watchdog"></a>
### 停滞检测

时间限制只在超时后才生效。为了更早发现卡住的协作者，调度循环每次唤醒（最多间隔 2 秒）都检查正在进行的尝试，发现以下两种情况时写入 `task.stalled` 事件：

| 情况 | 判断 | `data.reason` |
| --- | --- | --- |
| 没有动静 | 尝试的会话 10 分钟（`STALL_AFTER`，即 `Config.stall_after`）没有发出任何更新：文本、思考、工具调用和工具进度都算 | `idle` |
| 原地打转 | 连续 5 次（`LOOP_REPEAT`）完成同一个调用（`period: 1`，连续次数不受滑窗限制），或在最近 12 次工具调用的滑窗内连续 4 轮重复 2 步交替序列（`ABABABAB`，`period: 2`）、连续 3 轮重复 3 步循环序列（`ABCABCABC`，`period: 3`）。「同一个调用」指工具类型、标题、完整参数（`rawInput`）和调用结果（`rawOutput`、`content` 中的输出和 diff、完成状态）都相同（`call_fingerprint`）。比较输出时，耗时（`0.23s`）和时刻（`12:34:56`）不算区别；冒号后面的数字是文件位置（`parser.rs:42:15`），保留原样，所以修的是不同位置的编译错误时不算原地打转 | `loop` |

规则：

- 等人的时间不算：尝试在等提问的回答、审批或表单（`Run::held` 返回的守卫存在期间），或会话被用户接管或打断（`Run::set_human`）时，计时从头开始。这些状态由尝试自己在内存中标记，停滞检测不读事件日志。
- 同一段没有动静的时间只报告一次。会话再发出任何更新后重新计时，并写入一条 `data.resolved` 为 `true` 的 `task.stalled`，应用据此撤下「可能卡住了」，负责人不会读到它。同一个调用每连续满 5 次报告一次（5、10、15……），换了调用后重新计数。
- 负责人会话按同样的规则检查（看门狗按会话记录，负责人的会话是 `SessionKey::Lead`），结果写 `lead.stalled`，`to` 为 `user`：负责人之上没有别人会发现它卡住。应用在「待处理」中显示「负责人可能卡住了」，再有动静时撤下。负责人调用 Janus 看板工具不计入原地打转，反复 `wait` 是正常的。
- 只有操作和结果都没变才算重复：同一个文件连续编辑但每次改的内容不同，或「编辑 → 跑测试」交替而测试输出在变，都是在推进，不报告。比较结果前去掉其中每次运行都会变的耗时（数字加时间单位，如 `0.23s`、`120 ms`）和时刻（如 `12:34:56`），反复跑同一个失败的测试仍算重复；失败数量之类的其他数字照常比较。参数按原样比较。结果里还有别的每次都变的内容（例如进程号、临时路径）时，这样的循环不会被认出：协作者一直在调用工具，「没有动静」的检测也不会触发；工作没有时间上限，这时由负责人或用户看到后打断或取消。内存中只保存每次调用的哈希，不保存参数和输出。
- 停滞检测只报告，不打断、不重试也不取消。负责人在 `wait` 中读到 `[停滞]`，自己判断继续等、`reply`、`retry` 还是 `cancel`，见[目标模式与共享看板](20261031-goal_mode.md#failures_in_goal_mode)。
- 计时状态只在内存中（`Run.watch`），重启后从头开始；`task.stalled` 不影响恢复。

<a id="delivery_conditions"></a>
## 交付条件

调度循环每次唤醒都重新判断结果。「没有可执行任务」指没有任务在执行，也没有 `pending` 任务的依赖已全部成功。因暂停或并发上限而没有启动的任务仍算可执行：负责人在暂停期间结束时，Run 等用户继续，不会丢下这些任务直接交付。

| 条件 | 结果 |
| --- | --- |
| 没有可执行任务，且负责人会话异常结束 | Run 失败（`run.failed`），可以从中断处继续 |
| 没有可执行任务，且负责人目标已结束 | 交付（`run.delivered`），摘要取负责人最后一条回复 |
| 交付后的对话里，负责人通过看板提交了新任务 | 回到运行，任务完成后再次交付；应用在新任务开始时就不再提供上一次交付的验收 |
| 已验收的 worktree Run，对话中的一轮改了文件 | 再次交付，等用户再验收，验收时把新改动带回源分支；只是问问结果、没有改文件的一轮不改变验收记录 |
| 用户暂停，且没有任务在执行 | 暂停 |
| 其他 | 继续运行 |

任务失败不会让 Run 停下来等用户。负责人结束时还有协作者在等负责人回答的，提问转给用户，见[目标模式与共享看板](20261031-goal_mode.md#question_routing)。

交付时 `run.delivered` 的 `data.usage` 是一行用量摘要（token 及完整性、费用状态、墙钟时长），和 Run 页、`task_detail`、`GET /api/runs/{id}/usage` 读同一份账本，见[用量口径](../interfaces/20261031-events.md#usage_accounting)。用量不是交付条件：没有会话上报 token、费用缺失或口径未知时，摘要写「未上报」「部分上报」「口径未知」，Run 照常交付。

交付后负责人的会话不结束，而是保留为常规对话：用户可以直接写下一轮任务，或用 `/goal` 设置新的目标。负责人空闲等消息时不接收看板推送，协作者的提问转给用户；处理一条消息的这一轮里，看板推送和提问照第一个目标处理（见[提问的流转](20261031-goal_mode.md#question_routing)）。它在对话中通过看板提交的新任务会重新启动调度循环，完成后再次交付。

验收要等交付真的定下来：负责人正在处理用户在交付后交代的一轮，或还有等用户回答的请求时，`accept` 返回错误，说明先等它结束或先回答。

服务重启或会话退出后（负责人的 Agent 进程退出时，这个 Run 在没有调度在跑后离开内存），第一次给负责人发消息会重新打开会话：能 `session/load` 就恢复原会话（系统层和原来相同，不再注入，见[恢复会话时的系统层](../interfaces/20261031-acp_integration.md#reload_system_layer)），否则新建会话，并带上目标、交付摘要和最近的对话（不含这条还没送到的消息，它作为第一条提示发出）。已验收的 Run 也按同样方式恢复，继续工作或再次交付时重新打开。

<a id="run_statuses"></a>
## Run 状态

| 状态 | 含义 |
| --- | --- |
| `queued` | 在[派发队列](#dispatch_queue)里等空位，还没有开始任何东西 |
| `planning` | 刚开始，负责人会话正在启动 |
| `running` | 运行中；暂停时状态也保持 `running` |
| `delivered` | 已交付，等待验收 |
| `accepted` | 用户核对后验收。只能从 `delivered` 进入；继续下指令或设置新目标时重新打开 |
| `cancelled` | 用户取消 |
| `failed` | Run 失败，例如负责人会话异常结束 |
| `interrupted` | 编排器重启时 Run 还未结束 |

状态是 `RunStatus`（`backend/src/status.rs`），合法转移集中在 `RunStatus::can_transition_to`。`paused` 不是独立状态：暂停时状态保持 `running`，只有 `planning` 和 `running` 可以暂停和继续；只有 `delivered` 可以验收；已验收、已取消的 Run 不能再停止；已验收的 Run 在会话里继续工作或再次交付时重新打开；取消直接由 `status == RunStatus::Cancelled` 表示，不另存布尔标记。


<a id="user_actions"></a>
## 用户操作

用户通过 `POST /api/runs/{id}/actions` 操作 Run，参数见 [HTTP API](../interfaces/20261031-http_api.md#run_actions)：

- **回答与审批**：`answer`、`approve`、`input`。
- **介入会话**：`message`、`interrupt`、`takeover`、`release`、`finish`。会话正在运行时，消息通过 ACP 实时插入（支持 `_session/steering` 时，见[插话](../interfaces/20261031-acp_integration.md#steering)）或排队到这一轮结束；会话已结束时，消息保留到这个 Agent 下一次开始工作。每条消息发出时立即记录，之后再记录它的去向和送达时刻，见[消息的流转](../interfaces/20261031-events.md#message_flow)。
- **控制 Run**：`pause`、`resume`、`cancel`（界面上叫「停止」）、`accept`、`resumeRun`。
- **删除 Run**：应用里删除一个已结束的 Run（`DELETE /api/runs/{id}`，见 [HTTP API](../interfaces/20261031-http_api.md#run_delete)）。删除会移除它的事件、派生索引，以及 `~/.janus` 下的产物、日志、附件和成员资源包，不可恢复；工作目录中的文件不动。`planning`、`running` 的 Run 不能删除，要先停止；排队中的 Run 可以删除，先离开队列（记 `run.cancelled`），不会在删除期间开始。删除不是取消，也不影响其他 Run。

取消 Run（`run.cancelled`）和取消任务（`task.skipped_by_decision`）在用量账本里的范围不同：前者把取消时刻仍在跑的所有尝试记「已取消」，备注「任务被取消」；后者只影响那个任务的尝试，备注「负责人取消了这个子任务」，同一个 Run 里其他任务的尝试保持「未完成」。两者都不改动取消前已经结束的尝试（失败、验证没通过、过期保持原状态），已经上报的用量照常计入。Janus 不为这两类取消写尝试级事件，杀连接后可能还有一条失败事件，也不算真的失败。见[用量口径](../interfaces/20261031-events.md#usage_accounting)。

任务的重试、接受和取消由负责人通过看板处理，用户不直接操作任务；用户可以给负责人发消息提出要求。

<a id="recovery"></a>
## 中断与恢复

编排器启动时 `recover`：

- 仍在进行的尝试标记为 `interrupted`。
- 未结束的 Run 写入 `run.interrupted` 事件，状态改为 `interrupted`。
- 已验收的 Run 留下的 worktree 被移除（结果已经合并回源分支）。

用户「从中断处继续」（`resumeRun`），或直接给某个 Agent 发消息时，`resume_run` 从 `runs.config` 重建团队，从事件日志重建任务；在 worktree 中的 Run，worktree 不在了就先重建。恢复、验收历史交付（`accept_archived`）和删除一个不在运行的 Run 先拿 `App.revival`，一次只做一件：两条消息同时发给已中断或已交付的 Run 时，第二条等第一条把 Run 恢复好，再发进同一个 Run，不会各自恢复出一个负责人会话；恢复期间到达的验收按运行中的 Run 处理：

- 从所有 `orchestration.accepted` 事件重建看板任务（含 `owns`、`verifies`、`continues`）。编排事件的 `plannedSeq` 保存任务加入看板时的事件序号，恢复后沿用同一序号判断依赖的验证结论。旧事件缺少该字段时采用接纳前的序号，若已有明确的验证阻塞记录则保留该阻塞。Token 参考值只用于显示。已成功的任务从它的 `task.succeeded` 读回完整结果（摘要、`handoff`、验证判定 `verdict`、证据、风险、`traceGaps`、`checks` 及每条的证据方法，负责人 `accept` 的还有接受理由），证据方法计数和提醒随之重建，所以重启后看板、`task_detail` 和交付给出的证据汇总不变；没有完成的任务读回最后一次 `result.submitted`。失败和阻塞的任务保持原状态，被连带取消的任务记住是因哪个上游取消的，未完成的任务重新开始（负责人重试时选的会话和 `from`、复审的对象都从最后一条 `task.reopened` 重建，复审对象被加回验证的依赖）；重启前仍在进行的尝试，Agent 支持 `session/load` 时重新打开原会话。协作者和负责人的文件改动痕迹（`Actor::Task` 与 `Actor::Lead`）及已报告的冲突一并从事件日志恢复。
- 排队但还没送达的消息保留下来。
- 负责人以「从中断处继续」的说明重新启动，说明中列出看板上之前的任务。

`run.failed` 之后同样可以从中断处继续；停止（`cancelled`）的 Run 不能。恢复后调度照常进行，重启前的暂停不保留（`run.recovered` 之后应用也不再显示暂停）。已交付的 Run 在重启后仍可验收（`accept`），不需要恢复。

用量账本不存数据库表，重启后从事件重算，所以恢复前后的数字相同；取消、失败和过期尝试的用量也照常从事件里读回，见[用量口径](../interfaces/20261031-events.md#usage_accounting)。
