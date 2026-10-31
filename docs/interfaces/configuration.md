# 配置

本文列出 Janus 读取的环境变量、代码中的固定限制，以及 Janus 传给 Agent 子进程的变量。数据目录布局见[数据模型与存储](../architecture/data_model.md#locations)。

<a id="settings"></a>
## 编排器配置项

每一项先读环境变量；没有设置时读 `$JANUS_HOME/config.toml` 中对应的键；都没有时用默认值（`backend/src/config.rs`）。

| 环境变量 | `config.toml` 键 | 默认值 | 作用 |
| --- | --- | --- | --- |
| `JANUS_HOME` | 无 | `$HOME/.janus` | Janus home：数据、适配器、配置和日志，见[部署方案](../operations/deployment.md#layout) |
| `JANUS_ADDR` | `addr` | `127.0.0.1:7878` | HTTP 监听地址；也是写进看板 MCP URL 的地址 |
| `JANUS_NODE` | `node` | `node` | 运行 ACP 适配器的 Node 可执行文件；安装适配器前也用它检查 Node 版本 |
| `JANUS_ROOT` | 无 | 编译时的仓库目录（`backend/` 的上一级） | 只用于开发：开发构建或设置了这个变量时，页面从 `$JANUS_ROOT/frontend/app.html` 读取；工作区目录选择器的默认目录是其中的 `examples/calc` |

`config.toml` 示例：

```toml
addr = "127.0.0.1:7878"
node = "/opt/homebrew/bin/node"
```

`JANUS_ADDR` 改成非回环地址会把全部接口暴露出去，见[安全边界](../operations/security.md#network_exposure)。

安装适配器时调用的 `npm`、`npx` 和 `git` 从 Janus 进程的 `PATH` 中查找。

<a id="compile_time"></a>
## 编译期常量

| 名称（`orchestrator.rs`） | 值 | 作用 |
| --- | --- | --- |
| `MAX_ATTEMPTS` | 2 | 每个委派任务的尝试次数 |
| `ATTEMPT_TIMEOUT` | 30 分钟 | 单次尝试 |
| `STARTUP_TIMEOUT` | 10 分钟 | Agent 启动和打开会话 |
| `STEER_TIMEOUT` | 15 秒 | 发送打断、插入消息等控制指令 |
| `RUN_BUDGET_MS` | 6 小时 | Run 的墙钟预算 |
| `ACP_REQUEST_TIMEOUT`（`acp.rs`） | 30 分钟 | 单个 ACP 请求 |

<a id="agent_env"></a>
## 传给 Agent 的变量

Agent 子进程的环境由 `spec_for` 完整构造，按以下顺序叠加，后者不能覆盖前者中由 Janus 控制的变量：

1. **沙箱变量**（`sandbox_env`）：
   - 从宿主继承：`PATH`、`LANG`、`LC_ALL`、`LC_CTYPE`、`LC_MESSAGES`、`TERM`、`USER`、`LOGNAME`、`SHELL`、`TZ`。
   - 指向 Agent 自己目录的：`HOME`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_STATE_HOME`、`XDG_CACHE_HOME`、`TMPDIR`。
   - `JANUS_AGENT_ID`。
2. **用户配置的变量**：Agent 设置中的环境变量文本，以及「复制本机配置」带来的凭据变量。沙箱变量不能被这里覆盖。
3. **运行时变量**：

| 运行时 | 变量 |
| --- | --- |
| Codex | `CODEX_HOME`、`CODEX_PATH`；自定义 API 时 `OPENAI_API_KEY`，填写 Base URL 时还有 `CODEX_CONFIG`、`MODEL_PROVIDER` |
| Claude Code | `CLAUDE_CONFIG_DIR`、`CLAUDE_CODE_EXECUTABLE`；预设的固定变量和接入点变量；认证变量 `CLAUDE_CODE_OAUTH_TOKEN`、`ANTHROPIC_API_KEY` 或预设的 `ANTHROPIC_AUTH_TOKEN` |
| Grok | `GROK_HOME`、`GROK_PATH`、`GROK_DISABLE_AUTOUPDATER=1`；API Key 时 `XAI_API_KEY` |
| Pi | `PI_CODING_AGENT_DIR`；OpenAI 兼容服务时附加参数 `--provider janus` |
| 自定义 | 只有上面两层 |

<a id="adapter_paths"></a>
## 适配器路径

适配器装在 `$JANUS_HOME/adapters/<id>/`，会话从 `current` 指向的版本目录启动，入口文件取自包的 `package.json` 的 `bin` 字段。详见 [ACP 适配器管理](../architecture/adapters.md)。
