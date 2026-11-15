# ACP 适配器管理

本文说明 ACP 适配器怎样按需下载、在磁盘上怎样存放、怎样升级和回滚，以及会话怎样找到要用的版本。Janus 怎样通过适配器和 Agent 通信见 [ACP 集成](../interfaces/acp_integration.md)，部署时的目录规范见[部署方案](../operations/deployment.md#layout)。实现在 `backend/src/adapters.rs`。

<a id="catalog"></a>
## 适配器清单

每个内置运行时通过一个发布在 npm 上的 ACP 适配器接入。清单写在代码里（`ADAPTERS`）：

| id | 用于 | 来源 | 版本号 |
| --- | --- | --- | --- |
| `codex` | Codex | npm `@agentclientprotocol/codex-acp` | 包版本，例如 `2.6.0` |
| `claude` | Claude Code，以及 DeepSeek / MiniMax / GLM 等基于 Claude Code 的预设 | npm `@agentclientprotocol/claude-agent-acp` | 例如 `0.107.0` |
| `grok` | Grok | npm `@xai-official/grok`，启动参数 `agent stdio` | 例如 `1.0.87` |
| `pi` | Pi | npm `pi-acp` | 例如 `0.0.34` |

自定义命令的 Agent 自带启动命令，不需要适配器。Agent 和适配器的对应关系由连接的基础运行时（`base`）决定（`for_base`）。

Codex、Claude Code 和 Grok 的适配器自带对应的 CLI，用户不需要另外安装。Pi 的适配器会启动本机的 `pi` 命令（`PI_ACP_PI_COMMAND` 可以覆盖路径），需要先安装 Pi。Pi 适配器不会把 ACP 的 MCP 服务器转发给 pi，所以 Pi 会话看不到看板工具；它只适合不依赖看板的用途。

插话使用官方约定的 `_session/steering` 扩展，见 [ACP 集成](../interfaces/acp_integration.md#steering)。

<a id="layout"></a>
## 存放方式

```text
~/.janus/adapters/<id>/
├── <version>/                    # 一个已安装的版本
│   ├── .janus-adapter.json       # id、版本、npm 包、安装时间
│   └── node_modules/…            # 适配器包和它的依赖
├── <previous-version>/           # 上一个版本，用于回滚
└── current -> <version>          # 当前版本，相对符号链接
```

- 只有带 `.janus-adapter.json` 的目录才算已安装的版本。
- 适配器的入口文件从包的 `package.json` 的 `bin` 字段读取，不写死路径（`entry_in`）。
- 安装和升级的完整输出追加到 `~/.janus/logs/adapters/<id>.log`。

<a id="on_demand"></a>
## 按需安装

适配器在第一次被需要时安装：

1. 在「设置 → Agents」检查某个 Agent 时，第一步「ACP 适配器」调用 `ensure_adapter`。已安装就直接通过；没有安装就当场安装最新版本，安装完成后继续后面的检查。当前版本来自另一个包时，从现在的包安装并切换，见[来源变化后的版本](#source_change)。
2. 也可以在应用的「设置 → ACP 适配器」中手动安装。
3. 创建 Run 或启动会话时如果适配器不存在，直接报错，提示去运行配置检查。会话不会在运行中途触发下载。

<a id="install_steps"></a>
## 安装步骤

安装在临时目录 `<id>/.staging-<时间>` 中进行：

1. 检查 Node 版本（至少 22.19）。
2. 写一个空的 `package.json`，执行 `npm install --omit=dev --ignore-scripts --no-audit --no-fund <包>@<版本，默认 latest>`。生命周期脚本被禁用，安装和升级不执行包自带的脚本。
3. 版本号取实际安装的包版本，确认入口文件存在。
4. 写入 `.janus-adapter.json`（含包名），把临时目录改名为 `<version>`。同一版本的包已经装过时，保留原来的副本，丢弃这次的。
5. 原子地切换 `current`：先建一个新链接，再改名覆盖旧链接（`switch_current`）。
6. 清理：保留当前版本和切换前的版本，删除更老的版本和残留的临时目录（`prune`）。

任何一步失败都会删除临时目录，`current` 保持不变，正在用的版本不受影响。同一个适配器同一时间只有一个安装或检查在进行。

<a id="upgrade_rollback"></a>
## 检查更新、升级与回滚

| 操作 | 行为 |
| --- | --- |
| 检查更新 | 执行 `npm view <包> version`。结果保存在内存中，应用显示「有新版本」 |
| 升级 | 按[安装步骤](#install_steps)安装最新版本并设为当前版本 |
| 切回 | 把 `current` 指向另一个已安装的版本，不需要下载 |

应用的「设置 → ACP 适配器」提供这三个操作，接口见 [HTTP API](../interfaces/http_api.md#adapters)。安装接口可以指定版本。

<a id="source_change"></a>
## 来源变化后的版本

适配器的包可能改变。每个版本在 `.janus-adapter.json` 中记录包名（`matches_package` 据此判断）；没有记录包名的旧版本下一次检查或升级时被替换。

- 旧包装的版本照常启动，正在运行和之后启动的会话都不受影响。
- `GET /api/adapters` 对它返回 `updateAvailable: true`，应用显示「有新版本」，升级即从现在的包安装。
- 下一次配置检查（`ensure_adapter`）自动从现在的包安装并切换；安装失败时保留旧版本，检查继续。

<a id="running_sessions"></a>
## 正在运行的会话

启动会话时，`adapter_entry` 把 `current` 解析成具体的版本目录，再从那里启动适配器进程。所以：

- 升级或切换版本只影响之后启动的会话，正在运行的会话继续用启动时的版本。
- 升级后上一个版本仍然保留，不会删掉正在使用的文件。更老的版本会被清理；如果有会话从升级前两个版本以前一直运行到现在，它的文件可能已被删除。

<a id="api_shape"></a>
## 状态数据

`GET /api/adapters` 为每个适配器返回：`current`（当前版本）、`versions`（已安装的版本及安装时间）、`latest` 和 `checkedAt`（上次检查的结果）、`updateAvailable`、`job`（进行中或上一次安装、检查的状态和结果）、`log`（安装日志的最后 15 行）。`job.state` 是 `running`、`ok` 或 `error`；任务只在内存中，进程重启后没有遗留的「进行中」。安装和检查在后台线程中执行，应用在有任务进行时每 1.5 秒刷新一次。
