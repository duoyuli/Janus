# 工作区值班的代码审查

> 上级索引：[审查文档](index.md)

Codex 在 2026-11-21 对工作区值班（P0-2）的未提交改动做的只读审查。基线和快照是审查时的本机检查点，不在仓库历史中。

基线 `b253a01`，受审快照 `6184ec0c`，共 56 个变更文件。两者之间的 Git diff 与指定的 `p02.diff` 逐字节校验一致。审查期间工作区出现后续编辑，以下行号固定到受审快照。

共 30 项，高 9 项，中 20 项，低 1 项。并发时序和 macOS 行为按源码推演。派发唯一冲突和状态筛选另用内存 SQLite 验证，22 个变更 Rust 文件与 `rustfmt` 的只读输出一致。构建、运行时测试和端到端场景未执行。

权限核对覆盖了真实的 MCP 分流入口。服务端按 Run 和 `who` 校验会话令牌，再将 Loop 的工具调用送入 `loop_tool`。第 29 项是这一边界的测试缺口。见 `backend/src/orchestrator/mcp_server.rs:75`、`backend/src/orchestrator/board.rs:90`。

## 高严重度

### P02-01 · 高 · Agent 异步失败后，时钟卡在「醒着」，无法再次唤醒

- **位置** `backend/src/orchestrator/loops.rs:295`、`backend/src/orchestrator/actions.rs:723`、`backend/src/orchestrator/lead.rs:434`、`backend/src/orchestrator/loops.rs:291`
- **触发与影响** `resume_run` 启动后台会话就返回。适配器打不开会话，或执行首个 `session/prompt` 时退出，后台只记 `run.failed`。此时 `ticking` 已置位，`next` 已清空，再点「现在醒来」只会写 `pending`。从时钟和手动唤醒这两个入口都无法恢复，界面还会优先显示 `awake`。
- **建议修法** 让每次唤醒在会话启动失败、提示失败和正常结束时都走带唤醒编号的收尾逻辑。失败时记 `loop.failed`，清理本轮，按最新节奏处理待醒和重试。用首个 ACP 请求失败覆盖这条路径。

### P02-02 · 高 · 回答能唤醒 Loop，但回答正文到不了 Agent

- **位置** `backend/src/orchestrator/actions.rs:153`、`backend/src/orchestrator/actions.rs:161`、`backend/src/orchestrator/board.rs:874`、`backend/src/orchestrator/loops.rs:588`
- **触发与影响** 用户提交选项或文本后，`answer` 和 `note` 只进入 `question.answered` 事件。`escalate` 没有保留接收回答的一端，Loop 也没有看板的 `wait` 或推送。新提示只说「用户回答了」，没有问题、答案或备注。连续回答多道题时，多个触发还会合并。
- **建议修法** 按问题 key 和事件序号收集尚未交给 Agent 的回答，将对应问题、答案和备注放进唤醒提示，投递成功后推进游标。测试应核对适配器收到的正文，包含连续两次回答。

### P02-03 · 高 · 普通聊天和唤醒共用一条消息通道，可能用错轮次结算

- **位置** `backend/src/orchestrator/loops.rs:320`、`backend/src/orchestrator/turn.rs:210`、`backend/src/orchestrator/lead.rs:597`、`backend/src/orchestrator/lead.rs:609`
- **触发与影响** 用户的一轮还在执行时，唤醒作为普通 `Steer::Message` 可以直接插进该轮。反过来，唤醒期间排入用户消息，负责人的循环会先 `continue`，跳过本轮的 `loop_turn_ended`。最后一条聊天回复就可能成为唤醒摘要，下次时间和文件改动窗口也跟着用户那轮计算。
- **建议修法** 给唤醒消息保留独立的轮次标识，在当前对话结束后单独启动。由匹配该标识的结束事件结算，再处理普通排队消息。分别覆盖支持插话和只支持排队的适配器。

### P02-04 · 高 · 会话刚打开时，唤醒消息可以落进已经取空的留言队列

- **位置** `backend/src/orchestrator/actions.rs:110`、`backend/src/orchestrator/actions.rs:120`、`backend/src/orchestrator/session.rs:50`、`backend/src/orchestrator/lead.rs:541`
- **触发与影响** `send_steer` 先检查发送端，释放锁后才把消息写入 `notes`。如果这期间新会话注册发送端、取走 `notes` 并开始等待通道，后写入的唤醒就留到下次打开会话。本次投递仍返回成功，时钟一直等这一轮结束。
- **建议修法** 在同一临界区内选择发送端或把消息排入队列，使注册和取走留言也遵守同一顺序。保留消息 id，直到最终打开的 ACP 会话确认接收，覆盖恢复失败后更换适配器实例的情况。

### P02-05 · 高 · 重启会恢复已经被用户更改的旧定时器

- **位置** `backend/src/orchestrator/loops.rs:222`、`backend/src/orchestrator/loops.rs:234`、`backend/src/orchestrator/loops.rs:390`、`backend/src/orchestrator/loops.rs:285`
- **触发与影响** 一个手动 Loop 睡下时记了 `until=null`。把它改成固定间隔后，新截止时间只在内存中，服务重启会读回 null，此后不再自动醒。反方向把有截止时间的 Loop 改成手动，重启后仍会按旧时间自动执行。暂停期间手动醒过、再点继续，也会重现第一种情况。
- **建议修法** 在节奏变更、暂停和继续时记录可重放的生效时间与截止时间。启动时按事件顺序还原最后有效的安排，并用当前节奏校验。补充两种切换方向和暂停后继续的重启用例，同步修订 ADR 的恢复约定。

### P02-06 · 高 · 删除 Loop 时正在启动的 Agent 可以在删除完成后继续执行

- **位置** `backend/src/orchestrator/actions.rs:723`、`backend/src/orchestrator/lifecycle.rs:536`、`backend/src/orchestrator/lifecycle.rs:543`、`backend/src/orchestrator/session.rs:48`
- **触发与影响** 首醒或恢复期间，ACP 进程可能已经启动但还没登记到 `st.conns`。删除逻辑只终止当时登记的连接，并关闭事件写入，没有给这个 Run 设置启动方检查的终止状态。进程稍后仍能注册、读走已排队的提示并访问工作区。此时 Run 已从活动表移除，用户无法再通过它停止该进程。
- **建议修法** 先在登记连接所用的锁下标记 Run 正在删除，令所有尚未注册的启动任务在登记前终止并回收进程。等待启动任务及已登记连接退出，再清理记录和目录。

### P02-07 · 高 · 修改工作区目录后，Loop 在旧目录巡检，却向新目录派发

- **位置** `backend/src/orchestrator/loops.rs:187`、`backend/src/orchestrator/loops.rs:302`、`backend/src/orchestrator/loops.rs:354`、`backend/src/orchestrator/queue.rs:255`
- **触发与影响** 工作区设置允许保留 id 并把目录从 A 改成 B。Loop 的会话仍使用创建时保存的 `run.cwd`（A），派出的目标在当前工作区（B）执行。改动提醒还把 B 的轮前状态与 A 的轮后状态相比，会漏报或误报。
- **建议修法** 把会话工作目录、两次文件检查及派发统一绑定到同一个工作区版本。目录变更时停掉旧会话，在新目录重新打开后再允许自动派发。覆盖 A、B 各有不同文件的场景。

### P02-08 · 高 · macOS「任务」菜单会把 Loop 当成目标任务暂停或停止

- **位置** `frontend/Sources/Janus/App.swift:144`、`frontend/Sources/Janus/App.swift:177`、`backend/src/orchestrator/actions.rs:217`、`backend/src/orchestrator/actions.rs:254`
- **触发与影响** 在打开的 Loop 页使用菜单「暂停」或其快捷键只改 `Run.paused`，Loop 的节奏仍继续。「停止任务」把会话 Run 改成 `cancelled`，Loop 定义和时钟仍有效，后续恢复又会拒绝这个状态。页面按钮和菜单对同一个对象产生不同结果。
- **建议修法** 用真实 Run 类型决定菜单项，Loop 的暂停和继续调用专用接口，停止本轮走会话中断。服务端也按 Run 类型拒绝不适用的目标任务操作。

### P02-09 · 高 · 用户在应用里无法提交 Loop 的自由文本提问

- **位置** `frontend/Sources/Janus/UI/LoopView.swift:221`、`frontend/Sources/Janus/Core/Derive.swift:674`、`frontend/Sources/Janus/UI/InboxView.swift:125`、`frontend/Sources/Janus/UI/Run/Session.swift:596`
- **触发与影响** `ask_user` 允许省略 `options`。这种提问在收件箱没有快捷答案，「去处理」打开 Loop 页。该页只展示会话卡，卡片没有为 `question.escalated` 渲染回答表单。用户在聊天框里输入的文本也不会带问题 key，原问题一直待答。
- **建议修法** 在 Loop 页显示已有的 `LeadAsk` / `RequestCard`，让自由文本和备注按正确的 key 提交。产品端到端测试从收件箱点击「去处理」并通过页面回答。

## 中严重度

### P02-10 · 中 · 两条删除入口能分别留下没有会话的 Loop 或没有 Loop 的会话

- **位置** `backend/src/http/runs.rs:97`、`backend/src/orchestrator/lifecycle.rs:526`、`backend/src/orchestrator/loops.rs:252`、`backend/src/orchestrator/lifecycle.rs:567`、`frontend/Sources/Janus/Core/AppStore.swift:523`
- **触发与影响** 通用 Run 删除接口允许删除 Loop 的会话，却保留定义和时钟。中断后的 Loop 也能从应用「任务」菜单走到这个入口。反过来，专用接口先删定义，再删除会话。如果清理 Agent 会话或本地目录失败，数据库仍保留 Run，重试同一个 Loop 删除请求却已找不到定义。
- **建议修法** 按 Run 类型统一分派到可重试的 Loop 删除流程。先停钟并阻止新唤醒，保留供重试使用的关联，清理成功后在同一事务中移除定义与会话记录。派出的目标 Run 保留。

### P02-11 · 中 · 工作区删除时并发新建的 Loop 能逃过清理

- **位置** `backend/src/http/setup.rs:94`、`backend/src/http/setup.rs:97`、`backend/src/orchestrator/loops.rs:173`、`backend/src/orchestrator/loops.rs:205`
- **触发与影响** 删除工作区只列举一次 Loop，再逐个异步删除。另一个请求可在列举后、工作区删除前成功校验工作区并保存新 Loop。删除返回成功后，新 Loop 仍在全局列表中，到点会因工作区不存在而失败。
- **建议修法** 把停止接收新 Loop 和枚举待删 Loop 放在同一个受保护的工作区生命周期内。创建提交前复核父对象仍可用，清理到最后一个 Loop 后再完成父对象删除。

### P02-12 · 中 · 并发编辑、暂停或删除可以覆盖设置，甚至把已删除的 Loop 写回来

- **位置** `backend/src/orchestrator/loops.rs:216`、`backend/src/orchestrator/loops.rs:231`、`backend/src/orchestrator/loops.rs:116`、`backend/src/db.rs:639`
- **触发与影响** 编辑和暂停都先读取整份定义，再用 upsert 写回。编辑先读、暂停后写、编辑最后提交时，暂停会被旧的 `active` 覆盖。删除恰好发生在读写之间时，迟到的编辑还会重新插入定义，而它引用的会话 Run 已删除。
- **建议修法** 更新时使用只修改相应字段的条件 UPDATE，检查受影响行数。让配置、关联 Run 和时钟的变动按同一 Loop 串行提交，过期操作返回冲突。

### P02-13 · 中 · 上一轮还在收尾时，下一次唤醒已经能覆盖时钟

- **位置** `backend/src/orchestrator/loops.rs:349`、`backend/src/orchestrator/loops.rs:354`、`backend/src/orchestrator/loops.rs:371`、`backend/src/orchestrator/loops.rs:375`
- **触发与影响** 结束回调先清 `ticking`，然后读取 Git 状态、写 `loop.slept`、设置下次时间。此时手动唤醒可以先写下一轮的 `loop.woke`，读取到更早的睡下时间。旧回调随后把 `next` 写回正在执行的新一轮，事件顺序和页面上的下次时间都可能倒置。
- **建议修法** 将收尾保留为本轮占用时钟的一部分，新的触发在此期间并入待醒。耗时的 Git 检查可在锁外进行，提交结果时核对本轮编号，再一起发布睡下事件并交接时钟。

### P02-14 · 中 · 醒着时定时器到期，会反复零等待扫描并丢掉这次触发

- **位置** `backend/src/orchestrator/loops.rs:223`、`backend/src/orchestrator/loops.rs:410`、`backend/src/orchestrator/loops.rs:416`、`backend/src/orchestrator/loops.rs:421`
- **触发与影响** 长轮次执行中把节奏改为 1 分钟。截止后，扫描器因 `ticking` 跳过该项，却仍把同一个过期时间作为最早截止时间，持续用 0 毫秒超时重扫所有 Loop。到期原因没有进入 `pending`，收尾时又被下一次时间覆盖。
- **建议修法** 消费所有已到期的时刻。正在执行的 Loop 把原因合并到 `pending` 并清除该 `next`，空闲的进入唤醒队列，只用尚未消费的未来时刻计算等待。

### P02-15 · 中 · 自定节奏从调用工具时开始计时，长轮次会刚睡下就再醒

- **位置** `backend/src/orchestrator/loops.rs:504`、`backend/src/orchestrator/loops.rs:511`、`backend/src/orchestrator/loops.rs:68`、`docs/interfaces/20261031-board_mcp.md:332`
- **触发与影响** Loop 调用 `next_wake` 并指定 60 秒 后继续工作 90 秒。工具回执承诺本轮结束 60 秒后再醒，实现却已经保存了过去的绝对时刻，于是会立即重醒。普通聊天中调用同一工具也会返回成功，但结束回调不取 `chosen`，下一次唤醒又会清除它。
- **建议修法** 在有效唤醒轮次中保存经过范围检查的相对秒数，按该轮实际结束时间计算截止时间。普通对话调用时明确返回当前没有可设置的唤醒轮次。

### P02-16 · 中 · 已打开的会话切换为自定节奏后，客户端可能拿不到 `next_wake`

- **位置** `backend/src/orchestrator/loops.rs:221`、`backend/src/orchestrator/loops.rs:460`、`backend/src/orchestrator/loops.rs:616`、`backend/src/orchestrator/mcp_server.rs:37`
- **触发与影响** 工具列表和系统提示都按节奏决定是否包含 `next_wake`。编辑节奏只更新定义和时钟，已打开的会话继续复用，MCP 初始化又声明 `listChanged=false`。初始化时缓存工具表的客户端，从固定间隔切到自定后仍看不到新工具，只能落回默认间隔。
- **建议修法** 使已打开会话的工具表与生效节奏同步。可在当前轮结束后用新配置重建该会话，或实现工具表变化通知并同步更新节奏提示。验证同一 Loop 在两个方向切换。

### P02-17 · 中 · 「只创建」的 Loop 收不到第一条普通聊天消息

- **位置** `backend/src/orchestrator/loops.rs:187`、`backend/src/db.rs:179`、`backend/src/orchestrator/actions.rs:82`、`backend/src/orchestrator/actions.rs:94`
- **触发与影响** 新建手动 Loop 并选择「只创建」后，它的会话 Run 是离线 `planning`。此时在 Loop 页直接发送消息，动作入口先按目标任务的「正在开始」返回 409，到不了单会话恢复分支。用户必须额外唤醒一次才能开始普通对话。
- **建议修法** 将这个 `planning` 拦截限定在正在准备的目标 Run。离线 Loop 的首条消息走单会话恢复和投递，验证 Agent 收到文本、唤醒次数和下次时间保持原值。

### P02-18 · 中 · 可推迟的 Loop 提问在服务重启后消失

- **位置** `backend/src/orchestrator/actions.rs:95`、`backend/src/orchestrator/actions.rs:135`、`backend/src/orchestrator/actions.rs:705`、`frontend/Sources/Janus/Core/Derive.swift:412`
- **触发与影响** Loop 提问后先睡下，用户将问题留待稍后，期间重启服务。未答 key 只存在内存的 `pending` 中。恢复只重放未投递的消息，应用收到 `run.interrupted` 还会清空待办。无论会话是否重新打开，原问题都无法按原 key 回答。
- **建议修法** 从已持久化的提问、回答和撤回事件还原未决的 Loop 问题，让离线回答可以保存并触发恢复。将这种跨轮决定与只对当前 ACP 会话有效的审批分开处理。

### P02-19 · 中 · 同步唤醒失败时，详情仍报睡眠，收件箱也可能没有告警

- **位置** `backend/src/orchestrator/loops.rs:325`、`backend/src/orchestrator/loops.rs:147`、`backend/src/orchestrator/actions.rs:435`、`frontend/Sources/Janus/Core/Inbox.swift:125`
- **触发与影响** 服务重启后会话 Run 为 `interrupted`。若负责人连接已不可用，`resume_run` 在启动前返回错误，记录了 `loop.failed`，但 Run 状态仍为 `interrupted`。Loop 详情只按 Run 的 `failed` 推导失败，应用又提前过滤非活动的 Loop，用户看不到文档承诺的失败提醒。
- **建议修法** 根据最近一次唤醒的结果推导 Loop 的失败状态，成功唤醒后再清除。收件箱按 Loop 事件读取未解决的失败，使尚未打开 ACP 会话的错误也能显示。

### P02-20 · 中 · 无响应的 Loop 会话没有文档承诺的停滞提醒

- **位置** `backend/src/orchestrator/lead.rs:583`、`backend/src/orchestrator/turn.rs:19`、`backend/src/orchestrator/lifecycle.rs:276`、`docs/domains/20261121-loops.md:53`
- **触发与影响** Agent 接受提示后既不发更新也不结束时，`session/prompt` 会无限等待。Loop 走 `lead_conversation`，这条路径没有注册 `watch_session`，也没有运行目标调度循环中的 `watchdog`。服务不会发出 `lead.stalled`，之后的唤醒一直待处理。
- **建议修法** 在唤醒轮次开始时登记监视，接入现有停滞检查，在结束或失败时撤销。用一直保持连接但不返回的适配器验证只在工作期间产生提醒。

### P02-21 · 中 · 并发派发同一 key 时，失败方拿不到已有任务

- **位置** `backend/src/orchestrator/loops.rs:525`、`backend/src/orchestrator/loops.rs:531`、`backend/src/orchestrator/queue.rs:97`、`backend/src/migrations.rs:190`
- **触发与影响** 两个 Loop 同时派发同一工作区的同一 key，可以都通过预查。唯一索引正确地只保留一条 Run，但后到的插入返回 `SQLITE_CONSTRAINT_UNIQUE`，`enqueue` 原样向上报错。该请求没有收到契约约定的已有 Run id 和状态。内存 SQLite 已验证这一交错。
- **建议修法** 将按工作区和 key 创建或获取已有任务做成一个数据库操作，或只捕获该索引的唯一冲突并回读。只有真正创建的一方发排队事件、申请槽位。

### P02-22 · 中 · 唤醒提示漏掉已经开始准备或从中断恢复的派出任务

- **位置** `backend/src/db.rs:289`、`backend/src/orchestrator/queue.rs:172`、`backend/src/orchestrator/actions.rs:739`、`backend/src/orchestrator/loops.rs:307`
- **触发与影响** `dispatch_changes` 靠固定的事件名判断状态是否变过，名单遗漏 `run.starting`、`run.recovered` 等。在内存 SQLite 中，派出 Run 先排队，Loop 睡下后它变成 `planning`，使用补丁里的查询得到空列表。恢复为 `running` 也会被漏掉。
- **建议修法** 让状态转换和变更游标共用一个来源，或完整覆盖实际会改变 Run 状态的事件。按排队后开始准备、重试排队、从中断恢复分别验证下一次提示。

### P02-23 · 中 · `dispatches(status)` 先截断 50 条，再筛选状态

- **位置** `backend/src/orchestrator/loops.rs:539`、`backend/src/orchestrator/loops.rs:543`、`backend/src/orchestrator/loops.rs:564`、`backend/src/db.rs:263`
- **触发与影响** 一个 Loop 派过 51 个任务，其中唯一失败的是最早的那个。查询失败任务时，数据库先返回最近 50 个，随后应用筛掉这 50 个，工具告诉 Agent「还没有派出过任务」。即使只在查询范围内没有匹配状态，这句回复也会误报派发历史。
- **建议修法** 将合法的状态条件放到 SQL 的 `WHERE`，再按时间排序取最多 50 个。区分「还没有派发」与「没有处于该状态的任务」。

### P02-24 · 中 · 睡下的 Loop 被列为待办，恢复后的会话也会继续显示已中断

- **位置** `frontend/Sources/Janus/Core/Derive.swift:286`、`frontend/Sources/Janus/Core/Derive.swift:342`、`frontend/Sources/Janus/Core/Derive.swift:349`、`backend/src/orchestrator/lead.rs:605`
- **触发与影响** 应用只把 `kind=chat` 标作单会话。Loop 每轮结束发出 `turn=idle` 后会被加为「等你处理」的会话。重启后 Loop 再次打开时，服务端已恢复为 `running`，但客户端忽略带 session id 的打开事件，可能继续显示 `interrupted`，会话控件也按关闭状态渲染。
- **建议修法** 在派生状态中保存 Run 类型，让 Chat 和 Loop 共享正常空闲、会话恢复和清除旧错误的规则。分别验证正常睡下时收件箱为空、重启首醒后会话可操作。

### P02-25 · 中 · Loop 详情会少算派发总数，并保留过时的任务列表

- **位置** `frontend/Sources/Janus/Core/Loops.swift:61`、`frontend/Sources/Janus/UI/LoopView.swift:197`、`frontend/Sources/Janus/UI/LoopView.swift:261`、`frontend/Sources/Janus/Core/AppStore.swift:121`
- **触发与影响** 详情只读取 `/api/runs?loop=…&limit=200` 的第一页，用数组长度显示总数。实际有 201 个任务时最多显示 200，最早的任务无法查看。刷新还依赖全局最近一页目标数组，较早的派出任务状态改变时，即使已显示在右栏，该栏也可能一直保留旧状态。
- **建议修法** 总数使用服务端已有的 `status.dispatched` 各状态计数之和。列表按 Loop id 分页，消费 `next`，并在该 Loop 的派出任务发生变化时刷新对应页。

### P02-26 · 中 · 一次 Loop 事件会触发全量列表和历史重读

- **位置** `frontend/Sources/Janus/Core/AppStore.swift:238`、`frontend/Sources/Janus/Core/AppStore.swift:263`、`frontend/Sources/Janus/Core/AppStore.swift:123`、`backend/src/orchestrator/loops.rs:131`
- **触发与影响** 全局流已经传来具体 Loop id，应用却丢掉 id 并请求全部 Loop。`listChanges` 又没有识别 `loopAttention`，普通会话事件也会被当成未知 Run，触发全部列表、已打开的工作区历史和 Loop 状态重读。服务端还为每个 Loop 分别读取状态、事件和派发计数，多个活跃 Loop 会相互放大开销。
- **建议修法** 按 SSE 中的 Loop id 合并更新并读取单个 Loop，删除时移除对应条目。把已知的 Loop 会话纳入事件判定，只在首次加载、断线补读等需要重建列表时全量读取，符合 `AGENTS.md` 的事件流约定。

### P02-27 · 中 · 非 Git 工作区的文件变更始终不会产生提醒

- **位置** `backend/src/orchestrator/loops.rs:180`、`backend/src/orchestrator/loops.rs:303`、`backend/src/worktrees.rs:326`、`docs/operations/20261031-security.md:98`
- **触发与影响** 创建允许任何存在的目录。位于 Git 仓库之外的普通目录中，`git status` 会失败，`uncommitted` 将错误变成空列表。无论 Agent 新建或修改什么，轮前轮后都为空，`loop.touched` 不会出现。安全文档却把这项提醒列为工作区写入后的可见信号。
- **建议修法** 为普通目录提供可比较的改动快照，或在设置、运行状态和安全文档明确显示该目录无法监测。将检查失败和确实没有改动区分开。

### P02-28 · 中 · 首次唤醒时，已经打开的 Loop 页可能一直把会话当成离线

- **位置** `frontend/Sources/Janus/Core/RunModel.swift:121`、`frontend/Sources/Janus/Core/RunModel.swift:208`、`frontend/Sources/Janus/Core/Derive.swift:276`、`frontend/Sources/Janus/UI/Run/Session.swift:81`
- **触发与影响** 选择「只创建」后立即打开详情，服务端返回 `live=false`，但 `run.created` 已把派生状态设成 `running`。第一次真正唤醒时，会话打开事件没有改变这个状态值，页面便不会重新读取 `live`。后台已经执行，页面却仍按关闭的会话渲染，停止本轮等控件不可用，直到重新打开页面。
- **建议修法** 收到这次会话打开的事件时直接更新连接状态，或对当前 Run 发一次定向重读。用保持同一个页面、从未唤醒到首次唤醒的流程检查按钮状态。

### P02-29 · 中 · Loop 权限反例从内部密钥构造令牌，违反合并前检查第 1 条

- **位置** `backend/src/orchestrator/flow_tests/loops.rs:49`、`backend/src/orchestrator/flow_tests.rs:457`、`e2e/scenarios/loops.py:36`、`AGENTS.md:41`
- **触发与影响** 禁止看板工具的后端测试直接读取 `RunState.mcp_token` 并按传入的 `who` 派生令牌。产品场景虽然使用实际下发的会话 URL，却只检查工具列表。这两处测试合起来也没有验证客户端实际收到的凭据会被禁止执行看板操作。
- **建议修法** 在产品场景使用替身 Agent 从会话启动参数收到的凭据验证实际拒绝结果，并覆盖会话身份不匹配的请求。后端反例也经真实的凭据交付入口。

## 低严重度

### P02-30 · 低 · 产品端到端场景没有核对 Loop 详情独有的数据，未满足合并前检查第 12 条

- **位置** `e2e/scenarios/loops.py:51`、`e2e/scenarios/loops.py:71`、`frontend/Sources/Janus/Core/Snapshots.swift:153`、`frontend/Sources/Janus/UI/LoopView.swift:230`、`AGENTS.md:52`
- **触发与影响** 详情断言只从全局 `store.loops` 读取暂停状态。即使当前详情的派发列表为空、状态或 key 错误，这个场景仍可通过。问答也是直接通过 HTTP 提交，场景没有检查页面能否回答，或 Agent 实际收到的答案。
- **建议修法** 让应用状态暴露当前 Loop 详情渲染使用的派出 Run id、key、状态和总数，并与该 Loop 的服务端结果逐项比对。通过页面提交回答，再检查替身 Agent 记录的提示正文。
