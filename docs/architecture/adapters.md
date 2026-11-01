# ACP 适配器管理

本文说明 ACP 适配器怎样按需下载、在磁盘上怎样存放、怎样升级和回滚，以及会话怎样找到要用的版本。Janus 怎样通过适配器和 Agent 通信见 [ACP 集成](../interfaces/acp_integration.md)，部署时的目录规范见[部署方案](../operations/deployment.md#layout)。实现在 `backend/src/adapters.rs`。

<a id="catalog"></a>
## 适配器清单

每个内置运行时通过一个 Lody 的 ACP 适配器接入。清单写在代码里（`ADAPTERS`）：

| id | 用于 | 来源 | 版本号 |
| --- | --- | --- | --- |
| `codex` | Codex | npm `acp-extension-codex` | npm 版本，例如 `1.1.2` |
| `claude` | Claude Code，以及 DeepSeek / MiniMax / GLM 等基于 Claude Code 的预设 | npm `acp-extension-claude` | npm 版本，例如 `0.54.3` |
| `grok` | Grok | GitHub `LodyAI/acp-extension-grok` 的默认分支 | `<package.json 版本>-<提交>`，例如 `0.1.4-2fc653b` |
| `pi` | Pi | GitHub `LodyAI/acp-extension-pi` 的默认分支 | 同上，例如 `0.2.0-fd00784` |

自定义命令的 Agent 自带启动命令，不需要适配器。Agent 和适配器的对应关系由连接的基础运行时（`base`）决定（`for_base`）。

<a id="layout"></a>
## 存放方式

```text
~/.janus/adapters/<id>/
├── <version>/                    # 一个已安装的版本
│   ├── .janus-adapter.json       # id、版本、来源提交或 npm 版本、安装时间
│   └── …                         # npm：package.json 和 node_modules；Git：仓库检出和构建产物
├── <previous-version>/           # 上一个版本，用于回滚
└── current -> <version>          # 当前版本，相对符号链接
```

- 只有带 `.janus-adapter.json` 的目录才算已安装的版本。
- 适配器的入口文件从包的 `package.json` 的 `bin` 字段读取，不写死路径（`entry_in`）。
- 安装和升级的完整输出追加到 `~/.janus/logs/adapters/<id>.log`。

<a id="on_demand"></a>
## 按需安装

适配器在第一次被需要时安装：

1. 在「设置 → Agents」检查某个 Agent 时，第一步「ACP 适配器」调用 `ensure_adapter`。已安装就直接通过；没有安装就当场安装最新版本，安装完成后继续后面的检查。
2. 也可以在应用的「设置 → ACP 适配器」中手动安装。
3. 创建 Run 或启动会话时如果适配器不存在，直接报错，提示去运行配置检查。会话不会在运行中途触发下载。

<a id="install_steps"></a>
## 安装步骤

安装在临时目录 `<id>/.staging-<时间>` 中进行：

| 来源 | 步骤 |
| --- | --- |
| npm | 写一个空的 `package.json`，执行 `npm install --omit=dev <包>@<版本，默认 latest>`，版本号取实际安装的包版本 |
| Git | `git clone --depth 1`；`package.json` 声明了 `packageManager: pnpm@x` 时用 `npx --yes pnpm@x` 按锁文件安装并执行 `build`，否则用 `npm install --omit=dev` 和 `npm run build`；版本号是 `<package.json 版本>-<提交>` |

之后：

1. 检查 Node 版本（至少 20；Pi 本身要求 22.19 以上）。
2. 确认入口文件存在。
3. 写入 `.janus-adapter.json`，把临时目录改名为 `<version>`。同一版本已经装过时，保留原来的副本，丢弃这次的。
4. 原子地切换 `current`：先建一个新链接，再改名覆盖旧链接（`switch_current`）。
5. 清理：保留当前版本和切换前的版本，删除更老的版本和残留的临时目录（`prune`）。

任何一步失败都会删除临时目录，`current` 保持不变，正在用的版本不受影响。同一个适配器同一时间只有一个安装或检查在进行。

<a id="upgrade_rollback"></a>
## 检查更新、升级与回滚

| 操作 | 行为 |
| --- | --- |
| 检查更新 | npm 来源执行 `npm view <包> version`；Git 来源执行 `git ls-remote <仓库> HEAD`，取最新提交。结果保存在内存中，应用显示「有新版本」 |
| 升级 | 按[安装步骤](#install_steps)安装最新版本并设为当前版本 |
| 切回 | 把 `current` 指向另一个已安装的版本，不需要下载 |

应用的「设置 → ACP 适配器」提供这三个操作，接口见 [HTTP API](../interfaces/http_api.md#adapters)。Git 来源只能安装默认分支的最新提交，npm 来源可以通过接口指定版本。

<a id="running_sessions"></a>
## 正在运行的会话

启动会话时，`adapter_entry` 把 `current` 解析成具体的版本目录，再从那里启动适配器进程。所以：

- 升级或切换版本只影响之后启动的会话，正在运行的会话继续用启动时的版本。
- 升级后上一个版本仍然保留，不会删掉正在使用的文件。更老的版本会被清理；如果有会话从升级前两个版本以前一直运行到现在，它的文件可能已被删除。

<a id="api_shape"></a>
## 状态数据

`GET /api/adapters` 为每个适配器返回：`current`（当前版本）、`versions`（已安装的版本及安装时间）、`latest` 和 `checkedAt`（上次检查的结果）、`updateAvailable`、`job`（进行中或上一次安装、检查的状态和结果）、`log`（安装日志的最后 15 行）。安装和检查在后台线程中执行，应用在有任务进行时每 1.5 秒刷新一次。
