# HTTP API

本文是编排器 HTTP 接口的参考：路由、请求与响应、SSE 事件流和 Run 操作。看板 MCP 端点见[看板 MCP 工具](board_mcp.md)，事件字段见[事件参考](events.md)。

所有路由在 `backend/src/main.rs` 中注册。服务没有鉴权，只应监听回环地址，见[安全边界](../operations/security.md#network_exposure)。

<a id="errors"></a>
## 通用约定

- 请求和响应体都是 JSON（导出和文件读取除外）。
- 处理失败时返回状态码和纯文本错误信息：参数或业务错误 400，资源不存在 404，状态冲突 409，看板令牌错误 403。
- 错误信息是中文，直接用于界面提示。

<a id="routes"></a>
## 路由

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/meta` | Agent 类型定义、本机 CLI 探测结果、数据目录、默认工作目录 |
| GET | `/api/inspect?path=` | 检查目录：是否存在、是否 Git 仓库、当前分支、未提交改动数 |
| GET | `/api/local-config?kind=&source=` | 预览「复制本机配置」会复制哪些文件和变量，不含值 |
| GET | `/api/agent-id?kind=&name=` | 根据类型和名字建议一个 Agent id |
| GET | `/api/adapters` | ACP 适配器的安装状态、版本和进行中的任务 |
| POST | `/api/adapters/{id}/{action}` | 安装（`install`）、检查更新（`check`）或切换版本（`use`） |
| GET | `/api/skills` | 列出全部 Skill：Janus 自建的，以及各来源的（带 `sourceId`、`source` 为 `owner/repo`） |
| POST | `/api/skills` | 新建 Skill |
| POST | `/api/skills/import` | 从目录复制 Skill，body：`{"path": "..."}` |
| DELETE | `/api/skills/{id}` | 删除 Skill |
| GET | `/api/skill-sources` | 列出 GitHub Skill 来源及每个来源的 Skill |
| POST | `/api/skill-sources` | 添加来源：拉取仓库并解析 Skill，body：`{"url": "https://github.com/o/r"}`，见 [Skill 来源](../domains/agents_and_roles.md#skill_sources) |
| POST | `/api/skill-sources/check` | 立即检查所有来源的远端是否有新提交，返回与 GET 相同的列表，见[检测更新](../domains/agents_and_roles.md#skill_source_updates) |
| POST | `/api/skill-sources/{id}/update` | 重新拉取来源 |
| DELETE | `/api/skill-sources/{id}` | 删除来源；有角色挂载其中的 Skill 时返回错误 |
| GET | `/api/mcp-sources` | 列出 GitHub MCP 来源及每个来源的服务器，见 [MCP 来源](#mcp_sources) |
| POST | `/api/mcp-sources` | 添加来源：拉取仓库并解析 `mcp.json` / `.mcp.json`，body：`{"url": "https://github.com/o/r"}` |
| POST | `/api/mcp-sources/check` | 立即检查所有来源的远端是否有新提交，返回与 GET 相同的列表 |
| POST | `/api/mcp-sources/{id}/update` | 重新拉取来源，返回更新后的来源 |
| DELETE | `/api/mcp-sources/{id}` | 删除来源；有角色挂载其中的 MCP，或未结束（`planning`、`running`、`waiting_input`、`interrupted`、`failed`）的 Run 还在使用时返回错误 |
| POST | `/api/mcp-sources/{id}/servers/{key}/secrets` | 设置或清除一个资源的环境变量和 HTTP header 值，见[资源凭据](#mcp_secrets) |
| GET | `/api/setup` | 一次返回 `connections`、`roles`、`workspaces` |
| POST | `/api/setup/{kind}` | 新建或更新设置对象，`kind` 是 `connections`、`roles` 或 `workspaces` |
| DELETE | `/api/setup/{kind}/{id}` | 删除设置对象 |
| POST | `/api/connections/{id}/test` | 在后台启动配置检查，立即返回 |
| POST | `/api/connections/{id}/login` | 启动 Codex 或 Grok 的账号登录 |
| GET | `/api/file?path=` | 读取产物文本 |
| GET | `/api/stats/roles` | 最近的 Run 中各角色的任务统计，见[角色的执行统计](../domains/agents_and_roles.md#role_stats) |
| GET | `/api/runs` | Run 列表 |
| POST | `/api/runs` | 创建 Run |
| GET | `/api/runs/{id}` | Run 详情和全部事件 |
| GET | `/api/runs/{id}/stream?after=` | SSE 事件流 |
| POST | `/api/runs/{id}/actions` | Run 操作 |
| GET | `/api/runs/{id}/export` | 导出事件为 NDJSON |
| POST | `/mcp/{run}/{token}/{who}` | 看板 MCP 服务 |

<a id="setup"></a>
## 设置对象

`POST /api/setup/{kind}` 的 body 就是设置对象本身，带 `id` 表示更新，不带表示新建；返回保存后的对象。字段和校验规则见 [Agent、角色与工作区](../domains/agents_and_roles.md)。

- 连接返回时不包含凭据字段的值。
- 配置检查的进度写在连接对象上，应用轮询 `/api/setup` 读取。

<a id="mcp_sources"></a>
## MCP 来源

`GET /api/mcp-sources` 返回 `{ "sources": [...], "root": "<来源目录>" }`。每个来源包含 `id`、`url`、`repo`、`ref`、`path`、`commit`、`updatedAt`、`checkedAt`、`latest`、`updateAvailable`、`checkError`、`warnings`，以及 `servers`。服务器字段：

| 字段 | 含义 |
| --- | --- |
| `id`、`sourceId`、`key` | 资源 id 是 `<sourceId>/<key>`，角色用它挂载 |
| `name`、`title` | ACP 中的服务器名称（即 key）和 manifest 中的原名 |
| `manifest` | 定义所在的文件，相对仓库 |
| `transport` | `stdio` 或 `http` |
| `command`、`args`、`url` | 启动命令或 HTTP 地址 |
| `envKeys`、`headerKeys` | manifest 声明的变量名 |
| `envSet`、`headerSet` | 其中已经有值的变量名 |
| `description` | manifest 中的说明 |

解析、更新和删除规则见 [MCP 来源](../domains/agents_and_roles.md#mcp_library)。`source.json` 和响应都不含变量值。

<a id="mcp_secrets"></a>
### 资源凭据

`POST /api/mcp-sources/{id}/servers/{key}/secrets` 的 body 是 `{"env": {...}, "headers": {...}}`，两个字段都可省略：

- 非空字符串写入 `<JANUS_HOME>/secrets/mcp/<来源 id>/<key>.json`（`0600`）。
- 空字符串删除该变量；没有提到的变量保持原值。
- 来源或资源不存在时返回 400。
- 响应是这个资源在库中的条目，只带 `envKeys`、`headerKeys`、`envSet`、`headerSet`，不返回值。

<a id="adapters"></a>
## ACP 适配器

`GET /api/adapters` 返回 `{ "adapters": [...], "dir": "<适配器目录>" }`，每项字段见 [ACP 适配器管理](../architecture/adapters.md#api_shape)。

`POST /api/adapters/{id}/{action}`，`id` 是 `codex`、`claude`、`grok` 或 `pi`：

| `action` | body | 说明 |
| --- | --- | --- |
| `install` | `{"version": "1.1.2"}`，可省略 | 后台安装并设为当前版本；省略版本时安装最新版。Git 来源不能指定版本 |
| `check` | 无 | 后台查询最新版本 |
| `use` | `{"version": "<已安装的版本>"}` | 切换当前版本，用于回滚 |

成功时立即返回最新的适配器列表；安装和检查的结果通过 `GET /api/adapters` 的 `job` 字段读取。同一适配器已有任务在进行时返回 400。

<a id="runs"></a>
## Run

### 创建

```http
POST /api/runs
Content-Type: application/json

{ "goal": "为 calc 增加除法", "workspace": "w123" }
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `goal` | 是 | 目标，不能为空 |
| `workspace` | 是 | 工作区 id |

返回 `{"id": "<run id>"}`。校验规则见 [Run 生命周期](../domains/run_lifecycle.md#create_run)。

### 详情

`GET /api/runs/{id}` 返回：

```json
{ "run": { "id": "...", "goal": "...", "cwd": "...", "status": "...", "config": {} }, "events": [], "live": true }
```

`live` 表示 Run 当前在内存中运行。`live` 为 `false` 的 Run 只能执行 `accept`、`resumeRun` 和 `message`。

<a id="sse"></a>
### 事件流

`GET /api/runs/{id}/stream?after=<seq>` 是 SSE：

1. 先发送 `seq > after` 的历史事件，再持续发送新事件。订阅发生在读取历史之前，两者之间不会漏事件。
2. 每条消息的 `data` 是一个事件的 JSON。
3. 服务端广播缓冲溢出时，发送 `event: lagged`。客户端收到后应重新读取 `GET /api/runs/{id}`。

### 导出

`GET /api/runs/{id}/export` 返回 `application/x-ndjson`，每行一个事件，文件名 `janus-<id>.jsonl`。

<a id="run_actions"></a>
## Run 操作

```http
POST /api/runs/{id}/actions
Content-Type: application/json

{ "type": "answer", "key": "q-T2-1-0", "task": "T2", "answer": "alpha", "commandId": "...", "expectedSequence": 128 }
```

通用字段：

| 字段 | 说明 |
| --- | --- |
| `type` | 操作类型，见下表 |
| `commandId` | 可选。同一个 Run 内重复的 `commandId` 直接返回 `{"ok": true, "duplicate": true}`，不再执行 |
| `expectedSequence` | 可选。与 Run 当前最大 `seq` 不一致时返回 409，客户端应刷新后重试 |

成功返回 `{"ok": true}`。

| `type` | 其他字段 | 说明 |
| --- | --- | --- |
| `answer` | `key`、`task`、`answer`、`note` | 回答提问；回答主控用 `ask_user` 提的问题时 `task` 为空 |
| `approve` | `key`、`task`、`allow`、`option`、`label` | 处理审批请求，`option` 是 Agent 给出的 `optionId` |
| `input` | `key`、`task`、`action`、`content` | 处理表单请求，`action` 是 `accept`、`decline` 或 `cancel` |
| `message` | `text`，`agent` 或 `task` | 给会话发消息；Run 已中断时先恢复 |
| `interrupt` | `agent` 或 `task` | 打断当前一轮 |
| `takeover` | `agent` 或 `task` | 接管会话，Agent 每轮结束后等用户消息 |
| `release` | `agent` 或 `task` | 交还托管 |
| `finish` | `agent` 或 `task` | 以当前结果结束会话 |
| `pause` / `resume` | 无 | 暂停时进行中的尝试继续完成，不再启动新任务 |
| `extendBudget` | `minutes`（默认 60，范围 1 到 1440） | 追加预算；已超时则从现在算起 |
| `cancel` | 无 | 取消 Run，终止所有会话 |
| `accept` | 无 | 验收；Run 不在运行时也可以执行 |
| `resumeRun` | 无 | 从中断处继续；Run 正在运行时返回 409，早期计划模式的 Run 返回 400 |

`answer`、`approve`、`input` 需要对应的请求仍在等待，否则返回 400「没有等待中的决定」。

<a id="files"></a>
## 产物读取

`GET /api/file?path=<绝对路径>` 返回纯文本。路径解析后必须位于 `$JANUS_HOME/artifacts/` 内，否则返回 403；文件不存在返回 404。Janus home 的其他位置有数据库和凭据，不能通过这个接口读取。
