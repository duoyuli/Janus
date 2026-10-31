# 部署方案

本文说明 Janus 的目录规范，以及怎样在一台机器上安装、启动、常驻、升级、备份和卸载 Janus。日常排障见[运行手册](runbook.md)，ACP 适配器的安装与升级机制见 [ACP 适配器管理](../architecture/adapters.md)。

所有命令都在仓库根目录用 [just](https://github.com/casey/just) 执行，`just` 不带参数时列出全部命令。命令定义在仓库根目录的 `justfile`。

<a id="layout"></a>
## 目录规范

仓库只放代码。运行 Janus 产生和需要的一切都在 Janus home（`JANUS_HOME`，默认 `~/.janus`，权限 `0700`）：

```text
~/.janus/
├── bin/janus                 # 安装的 Janus 二进制（just install）
├── config.toml               # 用户配置，可选
├── janus.db                  # SQLite：Agent、角色、工作区、Run、事件
├── agents/<agent-id>/        # 每个 Agent 的隔离配置目录、HOME、临时目录和凭据
├── secrets/roles/            # 角色 MCP 的环境变量值
├── skills/                   # Janus Skill 库
├── adapters/<id>/            # ACP 适配器，按需下载，每个版本一个目录
├── artifacts/<run>/<task>/   # Run 的产物：回复和 diff
├── runs/<run>/skills/        # Run 创建时的 Skill 快照
└── logs/
    ├── janus.log             # Janus 自己的日志
    ├── janus.out.log         # 以服务方式运行时进程的标准输出和错误
    ├── adapters/<id>.log     # 适配器的安装和升级记录
    └── <run>/                # 每个 ACP 子进程的 stderr
```

| 类别 | 位置 | 说明 |
| --- | --- | --- |
| 用户级数据 | `janus.db`、`agents/`、`secrets/`、`skills/`、`artifacts/`、`runs/` | 属于这台机器的这个用户，与仓库位置无关 |
| 全局依赖 | `adapters/` | 第一次用到时下载，见 [ACP 适配器管理](../architecture/adapters.md) |
| 配置与日志 | `config.toml`、`logs/` | 配置项见[配置](../interfaces/configuration.md#settings) |
| 程序 | `bin/janus` | 页面已经编进二进制，不依赖仓库 |

删除仓库不影响任何数据；删除 `~/.janus` 会丢失全部数据。理由见 [ADR：用户数据、依赖和日志都放在 Janus home](../decisions/user_home_layout.md)。

<a id="prerequisites"></a>
## 前置条件

| 依赖 | 用途 | 要求 |
| --- | --- | --- |
| Rust 工具链（`cargo`） | 编译 Janus | 支持 edition 2024 |
| `just` | 执行构建和服务命令 | macOS 可用 `brew install just` |
| Node.js | 运行 ACP 适配器，安装适配器 | 22.19 及以上（Pi 适配器的要求） |
| `npm`、`npx` | 安装适配器 | 随 Node.js 安装 |
| `git` | 安装 Grok 和 Pi 适配器，检查工作目录 | |
| Agent CLI | 被适配器启动 | 用到哪个装哪个：`codex`、`claude`、`grok`；Pi 不需要 |

安装和检查适配器更新需要访问 npm 和 GitHub。

<a id="install"></a>
## 安装

```sh
just install
```

`just install` 执行 `cargo build --release --locked`，把二进制安装到 `~/.janus/bin/janus`。安装时先写到临时文件再改名，不会把正在运行的服务替换成写了一半的文件。

release 构建把 `frontend/app.html` 编进二进制，安装后的 Janus 不需要仓库。开发时用 `just dev`（等于 `cargo run`），页面直接读仓库里的文件，改完刷新即可。

<a id="run_as_service"></a>
## 启动

两种方式：

| 方式 | 命令 | 适用 |
| --- | --- | --- |
| 前台运行 | `just run` | 临时使用、排查问题 |
| 服务常驻（macOS） | `just start` | 日常使用：登录后自动启动，异常退出后自动重启 |

`just start` 生成 launchd 配置 `~/Library/LaunchAgents/local.janus.plist` 并启动服务。要点：

- launchd 启动的进程只有很短的 `PATH`，找不到 Homebrew 等位置的 `node`、`codex`、`claude`、`grok`、`git`。所以 `just start` 把执行时终端的 `PATH` 写进服务配置。这些工具的位置变化后，重新执行 `just start`。
- 服务在正常退出时不重启，异常退出后 10 秒内重启。
- 标准输出和错误写入 `~/.janus/logs/janus.out.log`；Janus 自己的日志在 `~/.janus/logs/janus.log`。
- `just plist` 只打印将要写入的配置，不安装。

启动后打开 `http://127.0.0.1:7878`（或 `config.toml` 中配置的地址），按顺序配置 Agent、角色和工作区。第一次检查某个 Agent 时，它需要的 ACP 适配器会自动下载安装。

其他系统没有 launchd：用 `just run`，或者把 `~/.janus/bin/janus` 交给 systemd 用户服务等进程管理器，并给它设置包含 `node` 和 Agent CLI 的 `PATH`。

<a id="single_instance"></a>
## 同一个 Janus home 只运行一个实例

启动时 Janus 把自己的进程号写入 `~/.janus/janus.pid`。如果文件里记录的进程还在运行，新实例直接报错退出。原因是第二个实例启动时会把第一个实例正在跑的 Run 全部标记为中断，并且两个实例会同时驱动同一个数据库。

- 服务在跑时想用开发版本：先 `just stop`，或者给开发版本设置另一个 `JANUS_HOME`（例如复制一份 `~/.janus` 用于测试）。
- 进程异常退出后留下的 `janus.pid` 不影响下次启动：记录的进程已经不存在时，直接覆盖。

<a id="operate"></a>
## 日常操作

| 目的 | 命令 |
| --- | --- |
| 查看服务状态，以及 HTTP 服务是否响应 | `just status` |
| 持续查看日志 | `just logs` |
| 重启服务 | `just restart` |
| 停止服务并取消登录自启 | `just stop` |
| 查看版本 | `~/.janus/bin/janus --version` |

重启或停止时进行中的 Run 会中断，之后可以在页面上「从中断处继续」，见 [Run 生命周期](../domains/run_lifecycle.md#recovery)。

<a id="upgrade"></a>
## 升级

| 升级对象 | 做法 |
| --- | --- |
| Janus 本身 | 更新仓库代码后执行 `just install`，再执行 `just restart` |
| ACP 适配器 | 在「设置 → ACP 适配器」中检查更新、升级或切回上一个版本，不需要重启 Janus，见 [ACP 适配器管理](../architecture/adapters.md) |
| Agent CLI | 用它自己的方式升级（例如 Homebrew），然后在「设置 → Agents」重新检查 |

数据库结构由 Janus 启动时自动创建；不需要单独迁移。

<a id="backup"></a>
## 备份与迁移到另一台机器

- 备份：先 `just stop`，再复制整个 `~/.janus`。`adapters/` 可以不备份，用到时会重新下载。
- 迁移：在新机器上安装 Janus，把备份放回 `~/.janus`，执行 `just start`。数据库中记录的产物路径是绝对路径，用户名或 `JANUS_HOME` 不同时，旧 Run 的产物链接会失效，Run 记录本身不受影响。

<a id="uninstall"></a>
## 卸载

```sh
just uninstall
```

`just uninstall` 停止服务，删除 launchd 配置和 `~/.janus/bin/janus`，保留其余数据。要彻底删除，再手动删除 `~/.janus`。
