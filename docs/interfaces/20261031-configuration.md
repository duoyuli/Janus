# 配置

本文列出 Janus 读取的环境变量、代码中的固定限制，以及 Janus 传给 Agent 子进程的变量。数据目录布局见[数据模型与存储](../architecture/20261031-data_model.md#locations)。

<a id="settings"></a>
## 编排器配置项

每一项先读环境变量；没有设置时读 `$JANUS_HOME/config.toml` 中对应的键；都没有时用默认值（`backend/src/config.rs`）。

| 环境变量 | `config.toml` 键 | 默认值 | 作用 |
| --- | --- | --- | --- |
| `JANUS_HOME` | 无 | `$HOME/.janus` | Janus home：数据、适配器、配置和日志，见[部署方案](../operations/20261031-deployment.md#layout) |
| `JANUS_ADDR` | `addr` | `127.0.0.1:7878` | HTTP 监听地址；也是写进看板 MCP URL 的地址 |
| `JANUS_NODE` | `node` | `node` | 运行 ACP 适配器的 Node 可执行文件；安装适配器前也用它检查 Node 版本 |
| `JANUS_MAX_CONCURRENT_ATTEMPTS` | `max_concurrent_attempts` | `3` | 单个 Run 同时运行的协作者尝试上限（范围 1 到 32），见 [ADR：角色并发槽位与全局并发上限](../decisions/20261118-role_slots_and_concurrency_cap.md) |
| `JANUS_MAX_RUNNING_RUNS` | `max_running_runs` | `8` | 所有工作区同时在做的目标 Run 上限（范围 1 到 32），更多的在派发队列里排队，见 [Run 生命周期：派发队列](../domains/20261031-run_lifecycle.md#dispatch_queue) |
| `JANUS_MAX_AGENT_SESSIONS` | `max_agent_sessions` | `12` | 同时开着的 Agent 进程上限（范围 1 到 64），目标 Run、对话和值班一起算；实际取它和 `max_running_runs + 1` 中较大的，见 [Agent 进程的上限与闲置回收](../domains/20261031-run_lifecycle.md#agent_seats) |
| `JANUS_HOST_HOME` | 无 | `$HOME` | 用户自己的用户目录：设置的「本机环境」从这里列出、链接路径，见[安全边界：共享的本机环境](../operations/20261031-security.md#host_env)；端到端测试用它指向替身目录 |
| `JANUS_RUN_TOKEN_BUDGET` | `run_token_budget` | 无 | 新建 Run 的 Token 用量参考值（正整数，非正数忽略）。应用将它和已上报用量一起展示，达到参考值后继续执行 |

`config.toml` 示例：

```toml
addr = "127.0.0.1:7878"
node = "/opt/homebrew/bin/node"
max_concurrent_attempts = 3
max_running_runs = 8
max_agent_sessions = 12
```

`JANUS_ADDR` 改成非回环地址会把全部接口暴露出去，见[安全边界](../operations/20261031-security.md#network_exposure)。

安装适配器时调用的 `npm` 从 Janus 进程的 `PATH` 中查找。

<a id="compile_time"></a>
## 编译期常量

| 名称（`orchestrator.rs`） | 值 | 作用 |
| --- | --- | --- |
| `MAX_ATTEMPTS` | 2 | 每个委派任务的尝试次数 |
| `STARTUP_TIMEOUT` | 10 分钟 | Agent 启动和打开会话 |
| `IDLE_AFTER` | 10 分钟 | 会话闲置这么久、Run 里没有别的事需要它时，关掉它的 Agent 进程，见 [Agent 进程的上限与闲置回收](../domains/20261031-run_lifecycle.md#agent_seats) |
| `STEER_TIMEOUT` | 15 秒 | 发送打断、插入消息等控制指令 |
| `ACP_REQUEST_TIMEOUT`（`acp.rs`） | 30 分钟 | 单个 ACP 控制请求；`session/prompt` 不限时，见[时间限制](../domains/20261031-run_lifecycle.md#timeouts) |

<a id="agent_env"></a>
## 传给 Agent 的变量

Agent 子进程的环境由 `spec_for` 完整构造，按以下顺序叠加，后者不能覆盖前者中由 Janus 控制的变量：

1. **沙箱变量**（`sandbox_env`）：
   - 从宿主继承：`PATH`、`LANG`、`LC_ALL`、`LC_CTYPE`、`LC_MESSAGES`、`TERM`、`USER`、`LOGNAME`、`SHELL`、`TZ`。
   - 指向 Agent 自己目录的：`HOME`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_STATE_HOME`、`XDG_CACHE_HOME`、`TMPDIR`。
   - `JANUS_AGENT_ID`。
   - 隔离的 `HOME` 里另有用户在「本机环境」中共享的路径，链接到本机的同一份，见[安全边界：共享的本机环境](../operations/20261031-security.md#host_env)。
2. **用户配置的变量**：Agent 设置中的环境变量文本，以及「复制本机配置」带来的凭据变量。沙箱变量不能被这里覆盖。
3. **运行时变量**：

| 运行时 | 变量 |
| --- | --- |
| Codex | `CODEX_HOME`、`CODEX_PATH`；自定义 API 时 `OPENAI_API_KEY`，填写 Base URL 时还有 `CODEX_CONFIG`、`MODEL_PROVIDER` |
| Claude Code | `CLAUDE_CONFIG_DIR`、`CLAUDE_CODE_EXECUTABLE`、`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`。订阅登录时设置 `CLAUDE_CODE_OAUTH_TOKEN`。选择「自定义 API」时，服务地址、模型和凭据均取自第 2 步，可使用 `ANTHROPIC_AUTH_TOKEN` 或 `ANTHROPIC_API_KEY` |

<a id="adapter_paths"></a>
## 适配器路径

适配器装在 `$JANUS_HOME/adapters/<id>/`，会话从 `current` 指向的版本目录启动，入口文件取自包的 `package.json` 的 `bin` 字段。详见 [ACP 适配器管理](../architecture/20261031-adapters.md)。
