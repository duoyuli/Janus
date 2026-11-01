# Janus 代码审查报告

审查日期：2026-10-31。

审查对象：当前 checkout 中的 `backend/`、`frontend/app.html`、`backend/testdata/fake_acp.py`、`justfile` 以及与这些代码直接相关的工程文档。

审查方式：先读取 `AGENTS.md`、`docs/index.md` 和 Project Doc 相关章节，再核对当前 Git 状态、源码、调用链、事件推导、恢复逻辑和测试结果。本文记录当前代码快照的审查结果，不代表已经实施修复。

## 结论

当前实现已经覆盖 ACP 启动、Run 编排、共享看板、事件记录、恢复和前端回放等主要流程，`just test` 的 19 个 Rust 测试和示例项目的 2 个 Python 测试均通过。

当前最需要处理的是身份边界、秘密值持久化、操作幂等性和事件持久化一致性。这些问题可能导致协作者获得主控权限、凭据进入 SQLite、同一用户操作重复执行，或前端看到无法在重启后恢复的状态。

工程质量检查尚未通过：`cargo fmt --check` 失败，`cargo clippy --all-targets -- -D warnings` 报告 22 项诊断。当前审查没有修改业务代码，也没有修改 `examples/calc` 的已有未提交内容。

## 问题总览

| 编号 | 影响 | 状态 | 问题 | 位置 |
| --- | --- | --- | --- | --- |
| J-01 | 高 | 已确认 | MCP 的 `who` 没有绑定到发放 URL 的会话，协作者可以请求主控工具 | `backend/src/orchestrator.rs:1058` |
| J-02 | 高 | 已确认 | Agent 的 `envText` 原值写入 SQLite，并通过设置接口返回 | `backend/src/agents.rs:396` |
| J-03 | 高 | 已确认 | `commandId` 的检查、操作和记录不是原子流程 | `backend/src/main.rs:333` |
| J-04 | 高 | 已确认 | 事件持久化失败后仍然广播，事实来源与页面状态可能分离 | `backend/src/orchestrator.rs:390` |
| J-05 | 高 | 已确认 | Run 只保存 Provider ID，后续任务重新读取当前 Provider 配置 | `backend/src/orchestrator.rs:1480` |
| J-06 | 中高 | 已确认 | Run 查询固定 `LIMIT 50`，旧 Run 无法正常详情、恢复或验收 | `backend/src/db.rs:199` |
| J-07 | 中高 | 已确认 | Run 预算只在内存中保存，恢复时重置；主控提示没有 Run 级截止时间 | `backend/src/orchestrator.rs:930` |
| J-08 | 中高 | 已确认 | 连接和角色 ID 的更新入口没有统一校验，可能影响文件路径 | `backend/src/agents.rs:371` |
| J-09 | 中 | 已确认 | PID 文件锁不是原子锁，无法严格保证单实例 | `backend/src/main.rs:48` |
| J-10 | 中 | 已确认 | 适配器清理只保留当前和前一版本，可能删除仍在运行的旧版本文件 | `backend/src/adapters.rs:332` |
| J-11 | 中 | 已确认 | 格式化和 Clippy 质量门禁失败 | `backend/src/agents.rs:387` 等 |

## 详细问题

### J-01：MCP 会话身份没有绑定到 `who`

影响：高。恶意或行为异常的自定义 ACP Agent 可以绕过协作者权限，调用主控的 `orchestrate` 和 `reply` 工具，改变任务图或向其他协作者发送指令。

代码事实：`mcp` 只检查 Run 是否存在以及 token 是否匹配，然后用 URL 中的 `who` 判断是否为主控。`who == "lead"` 时，`tools/list` 返回主控工具。当前代码没有检查 `who` 是否属于发放该 URL 的会话，也没有检查任务 ID 是否对应当前 ACP 连接。

证据：[orchestrator.rs:1058](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1058)、[orchestrator.rs:1060](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1060)、[orchestrator.rs:1066](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1066)、[orchestrator.rs:1092](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1092)。

触发条件：协作者已经获得自己的 MCP URL 和 Run token，随后把 URL 最后的 `who` 改成 `lead`，再调用 `tools/list` 或 `tools/call`。

建议：为每个 Agent 会话生成独立的不可转移 token，并在服务端保存 token、Run、成员、任务和 ACP 会话之间的绑定。服务端根据绑定结果决定工具集合，不再信任 URL 中的 `who`。至少应增加「协作者 token 不能列出主控工具」和「协作者不能操作其他任务」的测试。

### J-02：`envText` 的值进入 SQLite 和设置接口响应

影响：高。用户把 API Key 或其他秘密写入 Agent 环境变量文本后，值会进入 `configs.data`，并由连接列表和保存接口返回。

代码事实：`save_connection` 将 `parse_env` 的完整结果放入 `config["env"]`，随后写入数据库并返回配置对象。`connections` 直接读取配置对象并返回。代码只对认证字段和「复制本机配置」得到的变量使用独立秘密文件。

证据：[agents.rs:396](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:396)、[agents.rs:397](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:397)、[agents.rs:464](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:464)。这与[安全边界文档](/Users/duoyu/Data/dev/agent/Janus/docs/operations/security.md:51)中「设置接口不返回凭据值」的约定不一致。

建议：在入口处解析环境变量后，只允许明确的非秘密配置进入 SQLite。对秘密变量使用独立的 `0600` 文件，并在列表、保存和错误响应中只返回变量名和是否已设置。不能只依赖页面提示用户不要填写凭据，因为 HTTP 接口仍然接受该输入。

### J-03：`commandId` 去重不是原子操作

影响：高。并发请求或进程在操作完成后崩溃时，同一用户操作可能执行两次。

代码事实：HTTP 处理函数先调用 `command_exists`，执行 `app.action`，再调用 `record_command`。检查和记录之间没有事务或 Run 级串行化。

证据：[main.rs:333](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:333)、[main.rs:337](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:337)、[main.rs:384](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:384)、[db.rs:305](/Users/duoyu/Data/dev/agent/Janus/backend/src/db.rs:305)。

触发条件：两个请求同时使用相同 `commandId`，或者第一个请求执行 `app.action` 后、写入 `commands` 前进程退出。

建议：把操作声明、执行中、成功、失败和可重试状态放入数据库状态机，并按 Run 串行化操作。对可重复的操作使用数据库原子认领；对不可重复的操作，恢复时必须能够判断操作是否已经产生事实事件。应增加并发请求和崩溃恢复测试。

### J-04：事件持久化失败后仍然向前端广播

影响：高。页面、SSE 和内存状态可能显示一个没有写入 SQLite 的事件，重启后该事件消失，恢复逻辑会得到不同状态。

代码事实：`store` 调用 `insert_event` 失败时只写日志，随后无条件广播事件。`set_status` 也忽略更新数据库状态的错误。

证据：[orchestrator.rs:390](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:390)、[orchestrator.rs:391](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:391)、[orchestrator.rs:394](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:394)、[orchestrator.rs:397](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:397)。事件文档要求事件写入 SQLite 后再广播，见[事件参考](/Users/duoyu/Data/dev/agent/Janus/docs/interfaces/events.md:35)。

建议：让事件写入失败向驱动任务传播，并在失败时停止继续推进。只有数据库提交成功后才广播。需要同时更新事件和索引状态时，使用同一个 SQLite 事务，避免事件日志和查询索引出现不同结果。

### J-05：Run 没有冻结 Provider 运行时配置

影响：高。用户修改或删除 Provider 后，同一 Run 后续启动的协作者可能使用不同的命令、凭据、环境或 ACP 适配器。

代码事实：创建 Run 时，成员快照只保存 Provider ID。启动任务时通过 `spec_for(&member.runtime)` 重新从当前配置读取 Provider。恢复 Run 时也再次调用 `spec_for`。

证据：[orchestrator.rs:460](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:460)、[orchestrator.rs:472](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:472)、[orchestrator.rs:1479](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1479)、[orchestrator.rs:1480](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1480)、[orchestrator.rs:2968](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:2968)。

建议：创建 Run 时保存不可变的 Provider 运行时快照、适配器版本和必要的秘密引用。后续任务只读取快照。若产品需要允许 Provider 动态更新，应把它作为显式的 Run 变更并记录事件，而不是隐式使用最新配置。

### J-06：Run 查询固定限制为 50 条

影响：中高。数据库中超过 50 条 Run 后，较早的 Run 不能通过详情接口找到，也不能正常恢复或验收。

代码事实：`Db::runs` 使用 `ORDER BY created_at DESC LIMIT 50`。详情、恢复和归档验收都使用该列表查询指定 ID，而不是直接按 ID 查询。

证据：[db.rs:199](/Users/duoyu/Data/dev/agent/Janus/backend/src/db.rs:199)、[main.rs:265](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:265)、[orchestrator.rs:2903](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:2903)、[orchestrator.rs:2938](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:2938)。

建议：增加 `get_run(id)` 数据库方法。列表接口单独保留分页或数量限制，详情、恢复和验收使用按 ID 查询。

### J-07：Run 预算不持久化，主控会话没有 Run 级截止时间

影响：中高。反复重启可以重新获得完整预算；主控会话可能继续运行到超过 Run 预算。

代码事实：`deadline_ms` 只保存在内存中的 `Run`。恢复 Run 时直接设置为当前时间加 6 小时。主控调用 `stream_prompt` 时传入 `None`，没有使用 Run 的剩余预算。

证据：[orchestrator.rs:285](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:285)、[orchestrator.rs:930](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:930)、[orchestrator.rs:2987](/Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:2987)。

建议：在 Run 创建事件或数据库字段中保存绝对截止时间。恢复时沿用该时间，追加预算时显式更新并记录事件。主控和协作者启动、提示、验证和等待用户请求时，都应使用同一个剩余预算，并在超时后终止会话或进入可恢复状态。

### J-08：连接和角色 ID 更新入口没有统一校验

影响：中高。恶意或异常 HTTP 请求可以把 ID 用作路径片段，导致配置和秘密文件写入预期目录之外。

代码事实：新建连接的 `newId` 会调用 `check_agent_id`，但更新分支直接使用 `body["id"]`。角色保存同样直接接受 `body["id"]`。这些 ID 随后用于 Agent 目录和角色秘密文件路径。

证据：[agents.rs:371](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:371)、[agents.rs:384](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:384)、[agents.rs:449](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:449)、[agents.rs:1108](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:1108)、[agents.rs:1181](/Users/duoyu/Data/dev/agent/Janus/backend/src/agents.rs:1181)。

建议：所有创建、更新和删除入口统一校验 ID。校验应拒绝路径分隔符、`.`、`..` 和超出允许长度的值。文件路径拼接后还应检查路径仍位于对应根目录内。

### J-09：PID 文件不能严格保证单实例

影响：中。两个 Janus 进程可以在检查 PID 文件后同时写入自己的 PID。旧 PID 被系统重新分配给其他进程时，也可能阻止合法启动。

代码事实：`lock_home` 先读取 PID、调用 `kill -0`，再写入 PID 文件。整个过程没有文件锁或原子创建。

证据：[main.rs:48](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:48)、[main.rs:51](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:51)、[main.rs:65](/Users/duoyu/Data/dev/agent/Janus/backend/src/main.rs:65)。

建议：在 macOS 上使用 `flock` 或等效的内核文件锁，并把 PID 作为诊断信息而不是互斥机制。启动失败或进程退出后由文件锁自动释放。

### J-10：适配器清理可能删除仍在使用的旧版本

影响：中。连续切换适配器版本时，长时间运行的旧 ACP 会话可能仍引用已被 `prune` 删除的目录。

代码事实：清理逻辑只保留当前版本和切换前的一个版本，不记录每个运行中会话使用的适配器版本，也不维护引用计数。

证据：[adapters.rs:332](/Users/duoyu/Data/dev/agent/Janus/backend/src/adapters.rs:332)、[adapters.rs:340](/Users/duoyu/Data/dev/agent/Janus/backend/src/adapters.rs:340)。

建议：启动会话时记录适配器版本，清理时保留仍被进程引用的目录。也可以采用延迟清理，并在确认没有活动会话后删除。

### J-11：格式化和 Clippy 质量门禁失败

影响：中。当前代码可以通过测试，但不能通过仓库应有的格式化和严格 Clippy 检查，后续变更容易继续积累结构和可维护性问题。

验证事实：`cargo fmt --manifest-path backend/Cargo.toml -- --check` 报告格式差异；`cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings` 报告 22 项诊断，包含嵌套条件、过多参数、无必要的 `?`、集合处理和迭代器用法问题。

建议：先运行 `cargo fmt` 统一格式，再按模块修复 Clippy。对确实承载跨层上下文的函数，优先引入结构体参数或拆分职责，不要用大量 `#[allow]` 消除质量门禁。

## 修复顺序

第一阶段处理 J-01、J-02 和 J-08，先收紧会话身份、秘密值和文件路径边界。

第二阶段处理 J-03 和 J-04，建立操作幂等性和事件持久化的一致性基础。

第三阶段处理 J-05、J-06 和 J-07，修复 Run 快照、历史查询和预算恢复行为。

第四阶段处理 J-09、J-10 和 J-11，补充进程锁、适配器生命周期管理，并恢复格式化和 Clippy 质量门禁。

## 建议的验收用例

| 用例 | 预期结果 |
| --- | --- |
| 使用协作者 token 请求 `/mcp/{run}/{token}/lead` | 请求被拒绝，协作者只能看到 `board_post` |
| 保存包含 `API_KEY=secret` 的 Provider 环境变量 | SQLite、连接列表和保存响应不包含 `secret` |
| 并发发送两个相同 `commandId` 的操作 | 只有一个操作产生事实事件，另一个请求返回重复结果 |
| 模拟 SQLite 写事件失败 | 不向 SSE 广播未持久化事件，Run 进入可诊断失败状态 |
| Run 创建后修改 Provider | 后续任务仍使用 Run 快照中的运行时配置 |
| 创建超过 50 个 Run 后读取最早 Run | 详情、恢复和验收仍按 ID 正常工作 |
| Run 接近预算后重启 Janus | 恢复使用原截止时间，不重新获得完整预算 |
| 使用包含 `../` 的连接或角色 ID | 请求被拒绝，不创建或覆盖根目录之外的文件 |
| 连续升级适配器且保留旧会话 | 旧会话使用的适配器目录不会被删除 |
| 执行 `cargo fmt --check` 和严格 Clippy | 两个命令均通过 |

## 验证记录

| 检查 | 结果 |
| --- | --- |
| `python3 .agents/skills/project-doc/scripts/check_library.py` | 通过，31 篇文档、46 个代码锚点、0 个结构问题 |
| `just test` | 通过，19 个 Rust 测试 |
| `python3 -m unittest discover -s examples/calc -v` | 通过，2 个测试 |
| `cargo fmt --manifest-path backend/Cargo.toml -- --check` | 失败，存在格式差异 |
| `cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings` | 失败，22 项诊断 |
| 根仓库状态 | `main` 与 `origin/main` 同步；`examples/calc` 存在已有未提交删除 |

## 审查操作日志

1. 读取 `AGENTS.md`、仓库本地 `project-doc` skill、`docs/index.md` 和相关分类索引，确认文档路由和只读审查边界。
2. 检查当前分支、提交、根仓库状态和 `examples/calc` 子仓库状态。
3. 阅读架构、领域、接口、运维和决策文档，并沿代码中的 `@project-doc` 锚点核对实现。
4. 审查 HTTP 路由、MCP 看板、ACP 进程、Provider 配置、SQLite、Run 调度、恢复逻辑、适配器生命周期和前端事件推导。
5. 运行 Project Doc 结构检查、Rust 测试、示例项目测试、格式检查和 Clippy。
6. 创建本报告。审查过程中没有修改业务代码、没有提交、没有推送，也没有处理 `examples/calc` 的已有未提交状态。

## 范围限制

本次审查是当前源码和静态测试结果审查，没有启动真实 Janus 服务，也没有使用真实 ACP Provider、真实凭据或远程适配器安装流程。因此，报告中的运行时隔离、真实 Agent 行为和网络部署表现仍需在修复后单独验证。
