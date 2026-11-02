# 测试

本文说明 Janus 的测试结构、测试用的脚本化 Agent 和编写新测试时的约定。运行环境见[运行手册](runbook.md)。

<a id="running_tests"></a>
## 运行

```sh
just test
```

等于 `cd backend && cargo test`。

macOS 应用的测试见 [macOS 应用](#macos_app)。

测试不访问网络，不调用真实 Agent，也不安装适配器（脚本化 Agent 是自定义命令类型）。需要本机有 `git` 和 `python3`。

<a id="test_layout"></a>
## 测试分布

| 位置 | 内容 |
| --- | --- |
| `backend/src/db.rs` 的 `tests` | 操作去重、租约 fencing |
| `backend/src/agents.rs` 的 `session_option_tests` | 各 Agent 声明的会话控制项如何解析为权限模式 |
| `backend/src/agents.rs` 的 `seed_tests` | 默认角色的首次写入和旧版本（v1、v2）升级：保留绑定，不改动编辑过的角色，不恢复已删除的角色，只升级一次 |
| `backend/src/adapters.rs` 的 `tests` | 版本切换与旧版本清理、从 `package.json` 的 `bin` 读取入口 |
| `backend/src/config.rs` 的 `tests` | 日志时间格式 |
| `backend/src/orchestrator.rs` 的 `tests` | 主控说明中的协作者名单、任务提示中的共享资料、从主控回复中拆出「建议更新共享资料」 |
| `backend/src/orchestrator.rs` 的 `flow_tests` | 端到端流程：启动真实的编排器 `App`，用脚本化 Agent 跑完整 Run |

`flow_tests` 覆盖的流程包括：通过看板编排（先校验后执行、进展、提问与回答、交付）、主控结束后提问转给用户、重启后恢复看板并继续编排、审批和表单请求到达用户、任务在工作目录中运行并产生 diff、主控重试失败任务后下游恢复、暂停、重启后恢复失败和阻塞的任务、早期计划模式 Run 不能继续、按验收标准核对结果（原会话补一次，仍不行交给主控）、主控处理失败任务（接受、重试、取消、交给用户决定）、主控回复不等长回合、修订上游结论后下游以修订为准、停滞检测（没有动静只报告一次、原地打转、等审批和用户打断后掌握会话的时间不算）、工作区共享资料进入主控说明，主控的建议条目与交付摘要分开。停滞检测的测试把 `Config.stall_after` 改成 1 秒。

<a id="fake_agent"></a>
## 脚本化 Agent

`backend/testdata/fake_acp.py` 是一个按脚本行事的 ACP Agent，作为 `custom` 类型的 Agent 启动。`FAKE_DIR` 环境变量指向它的工作目录：

| 文件 | 作用 |
| --- | --- |
| `script.json` | 任务 id → 每次提示的结果列表，最后一个结果重复使用 |
| `calls.log` | Agent 记录收到的会话设置（`session/new` 的 `_meta`、`set_config_option`）和主控收到的提示 |
| `lead-go` | 目标模式主控在这个文件出现前一直工作，然后以文件内容作为最后回复结束 |

`script.json` 中可用的结果：`ok`、`fail`、`cancelled`、`slow`、`write:<文件>`、`permission`（发出带三个选项的 `session/request_permission`）、`elicit`（发出带表单的 `elicitation/create`）、`loop`（给出结果前连续 5 次发出相同的工具调用）、`ask`（以提问结束本轮，回答作为下一条提示）。

作为目标模式主控时，它先确认说明（`calls.log` 记 `brief ok`），再在收到 `/goal …` 后等待 `lead-go`。计数器保存在 `FAKE_DIR` 中，同一任务的多次尝试（不同进程）共用脚本进度。

<a id="harness"></a>
## 测试夹具

`flow_tests::harness(name, members, script)`：

- 在系统临时目录创建 `janus-flow-<name>-<pid>`，内含一个初始化过的 Git 仓库作为工作区。
- 注册一个 `fake` Agent，声明 `mode`（`read-only` / `ask` / `agent` / `full`）权限控制项。
- `members` 是 `(角色 id, 是否可写)` 列表，第一个是主控；不可写的角色使用 `read-only` 模式。
- 测试构建下 `lead_capable` 允许 `custom` 类型做主控，以便脚本化 Agent 担任主控。

辅助函数：`start`（创建 Run 并以主控身份提交一份编排）、`finish_lead`（让脚本化主控结束目标）、`act`（发送 Run 操作）、`tool`（以主控或协作者身份调用看板工具）、`wait_until`（等待事件条件）、`events`、`has`、`status`。

<a id="test_conventions"></a>
## 约定

- 每个 `flow_tests` 测试给 `harness` 传唯一的 `name`。测试并行运行，名字相同的测试会共用并互相删除临时目录，造成随机失败。
- 测试结束时删除临时目录。
- 新增事件类型或改变流程时，在 `flow_tests` 中用脚本化 Agent 覆盖，不依赖真实 Agent。
- 真实 Agent 的验证（例如 Codex 主控 + Pi 协作者）通过应用手动运行，结果不进入自动测试。

<a id="macos_app"></a>
## macOS 应用

- `just test-mac`（`cd frontend && swift test`）运行 `frontend/Tests/JanusTests` 中的 Swift Testing 测试：事件推导、看板事实、JSON、Markdown、环境变量文本和 Plan 控制项过滤。`@Test` 宏插件只随 Xcode 提供，只装命令行工具时测试无法编译；CI 的 macOS 运行环境自带 Xcode。
- 查看界面：设置 `JANUS_SNAPSHOT_DIR` 后，应用每 0.7 秒读取一次 `<目录>/cmd`，逐行执行 `open run:<id>`、`open home:<工作区>`、`open settings:<agents|roles|adapters>`、`page <board|audit|team|task:<id>|agent:<id>|replay:<seq>>`、`role <id>`、`workspace <id>`、`close-sheet`、`size <宽>x<高>`、`shot <名字>`，截图保存为 `<目录>/<名字>.png`。截的是应用自己的窗口，不需要屏幕录制权限。配合 `JANUS_HOME` 和 `JANUS_ADDR` 指向一份数据副本使用，不影响正在运行的 Janus。

