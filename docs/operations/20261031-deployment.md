# 部署方案

本文说明 Janus 的目录规范，以及怎样在一台机器上安装、启动、常驻、升级、备份和卸载 Janus。macOS 上最简单的方式是下载安装包，见 [macOS 应用](#macos_app)；以下各节是从源码安装服务的方式。日常排障见[运行手册](20261031-runbook.md)，ACP 适配器的安装与升级机制见 [ACP 适配器管理](../architecture/20261031-adapters.md)。

所有命令都在仓库根目录用 [just](https://github.com/casey/just) 执行，`just` 不带参数时列出全部命令。命令定义在仓库根目录的 `justfile`。

## 导航

- [目录规范](#layout)：新的数据、配置或日志放在哪里时阅读。
- [macOS 应用](#macos_app)：打包或排查应用启动时阅读。
- [前置条件](#prerequisites)：在新机器上准备环境时阅读。
- [安装](#install)：从源码安装时阅读。
- [启动](#run_as_service)：把服务作为后台服务运行时阅读。
- [同一个 Janus home 只运行一个实例](#single_instance)：遇到「已经有实例在运行」时阅读。
- [日常操作](#operate)：查看状态、日志、重启时阅读。
- [升级](#upgrade)：升级服务和数据时阅读。
- [备份与迁移到另一台机器](#backup)：备份或换机器时阅读。
- [卸载](#uninstall)：卸载时阅读。

<a id="layout"></a>
## 目录规范

仓库只放代码。运行 Janus 产生和需要的一切都在 Janus home（`JANUS_HOME`，默认 `~/.janus`，权限 `0700`）。顶层按「丢了还能不能再有」分类，看名字就知道哪些要备份、哪些可以删；属于一个对象（Agent、Run、对话、资源）的东西放在它的 id 下。取舍见 [ADR：Janus home 按「丢了能不能再有」分类](../decisions/20261121-home_layout.md)。

```text
~/.janus/
├── config.toml                      # 用户配置，可选
├── janus.pid                        # 单实例锁，内容是正在运行的 Janus 的进程号
├── bin/janus                        # 安装的 Janus 二进制（just install）
├── data/                            # 无法再有：备份 Janus 只需备份它
│   ├── janus.db                     # SQLite：Agent、角色、工作区、Run、事件
│   ├── secrets/                     # 0700，Janus 保存的全部凭据，文件 0600
│   │   ├── agents/<agent>.json      # Agent 的凭据字段和环境变量中的私有值
│   │   └── library/<资源>.json      # 资源库 MCP 服务器的环境变量和 HTTP header 值
│   ├── roles/<数字员工>/avatar.png  # 数字员工的头像，256×256 PNG，删除数字员工时一起删
│   ├── agents/<agent>/
│   │   ├── config/                  # Agent 的配置目录（CODEX_HOME、CLAUDE_CONFIG_DIR）：登录状态、设置、会话记录
│   │   └── home/                    # Agent 进程的 HOME 和 XDG 目录：用户目录的镜像（每项链接到用户的原位置，挡住的除外；原有的同名内容改名为 <名字>.janus-own），和它自己装的工具链和缓存
│   ├── library/
│   │   ├── skills/<sha256>/         # Skill 的内容，按内容寻址，存下后不再修改
│   │   ├── sources/<来源>/repo/     # Git 来源的仓库副本
│   │   └── servers/<资源>/          # MCP 路由启动的命令服务器的 HOME，跨 Run 保留；同样镜像用户目录
│   ├── runs/<run>/                  # 任务：Janus 为它记下的内容
│   │   ├── bundle/<成员>/           # 成员的资源包：Run 创建时复制的 Skill
│   │   ├── attachments/             # 随目标和消息粘贴的图片
│   │   ├── artifacts/<任务>/        # 每次尝试的回复和 diff
│   │   └── changes/                 # 整个任务相对开始时的 diff，按内容命名，负责人每一轮结束时有变化就存一份
│   ├── chats/<对话>/                # 对话：bundle/<成员>/、attachments/
│   ├── loops/<run>/                 # 工作区值班的会话：bundle/<成员>/、attachments/
│   ├── dreams/<run>/                # 团队记忆的整理：bundle/<成员>/
│   ├── deleted-runs/<run>.patch     # 删除 Run 时 worktree 里还没提交的改动
│   └── app/permissions.json         # macOS 权限引导：安装标识和是否处理过引导（应用写）
├── work/                            # Agent 读写的目录
│   ├── runs/<run>/                  # Git 仓库上每个任务自己的 worktree（元数据在原仓库的 .git/worktrees/）；runs/ 和 loops/ 下各有一个 .metadata_never_index，不让 Spotlight 索引这些副本
│   ├── loops/<run>/                 # Git 仓库上每个值班自己的副本（worktree），每次开工前对齐到工作区当前的提交
│   ├── dreams/<run>/                # 团队记忆的整理会话自己的空目录，不在工作区里
│   └── chats/<对话>/                # 对话的工作目录
├── backups/                         # 数据升级前的 janus.db：janus-v<旧数据版本>-<毫秒时间>.db，保留 5 份
├── cache/                           # 停止 Janus 后可以整个删掉，用到时重新生成
│   ├── adapters/<id>/<版本>/        # ACP 适配器，按需下载，自动升级时保留旧版本
│   ├── adapters/<id>/pinned          # 用户切回并固定的版本号，自动升级不再动它
│   ├── prices.json                  # 从 models.dev 同步的模型价格，每天更新
│   ├── mcp-tools/<资源>/<revision>.json  # MCP 服务器最近一次列出的工具，只留当前 revision
│   └── updates/                     # 应用内更新：<版本>/ 下载中的新版本，previous/ 上一个版本（应用写）
├── tmp/                             # 每次启动 Janus 时清空
│   ├── agents/<agent>/              # Agent 的 TMPDIR，配置检查用的空目录
│   ├── host-probe/                  # 设置里「试一条命令」以 Agent 的方式运行的沙箱（home/ 镜像用户目录，tmp/）
│   └── servers/<资源>/              # MCP 服务器的 TMPDIR，预先准备时的工作目录
└── logs/
    ├── janus.log                    # Janus 自己的日志
    ├── janus.out.log                # 以服务方式运行或由应用启动时，进程的标准输出和错误
    ├── update.log                   # 应用内更新替换应用包的记录（应用写）
    ├── adapters/<id>.log            # 适配器的安装和升级记录
    ├── agents/<agent>/              # 配置检查（test.log）和登录（login.log）的输出
    ├── library/<资源>.log           # 预先准备命令服务器时它的 stderr
    ├── runs/<run>/                  # 每个 ACP 子进程的 stderr，MCP 路由启动的服务器的 mcp-<run 或会话>-<资源名>.log
    ├── chats/<对话>/                # 同上，对话的
    ├── loops/<run>/                 # 同上，值班会话的（loop.log）
    └── dreams/<run>/                # 同上，整理会话的（dream.log）
```

| 分类 | 能不能删 | 说明 |
| --- | --- | --- |
| `data/` | 不能 | 删掉就丢失全部设置、记录和登录状态。备份时复制它（服务停止时），或至少复制 `janus.db` 和 `secrets/` |
| `work/` | 不要手动删 | 任务和对话还会用到；删除 Run 时 Janus 先把 worktree 里没提交的改动存成补丁再删，见 [ADR：每个任务在自己的 worktree 中运行](../decisions/20261119-run_worktrees.md) |
| `cache/` | 停止 Janus 后可以 | 适配器、价格和工具列表用到时重新下载或生成；`updates/previous/` 是手动退回上一个应用版本时用的 |
| `tmp/` | 可以 | 每次启动时清空 |
| `logs/` | 可以 | 删除 Run、对话、Agent、资源时它们的日志一起删 |
| `backups/` | 确认新版本正常后可以 | 退回旧版本或升级出问题时用，见[升级](#upgrade) |
| `bin/`、`config.toml` | | 程序和配置，配置项见[配置](../interfaces/20261031-configuration.md#settings) |

代码中的路径全部来自后端 `backend/src/layout.rs` 的 `Layout`（`App.cfg.layout`），应用中来自 `JanusHome`（`frontend/Sources/Janus/Core/JanusHome.swift`），两处的名字一致。删除 Run、对话、Agent 或资源时，删除的是 `Layout` 列出的属于它的全部路径（`RunDirs`、`agent_paths`、`item_paths`），删除 Run 时还删掉 Agent 自己存下的会话记录，见[删除](../interfaces/20261031-http_api.md#run_delete)。

仓库中的 `backend/target/`、`frontend/.build/` 和 `dist/` 是构建输出，都在 `.gitignore` 中，运行时不会写入仓库。删除仓库不影响任何数据。

<a id="new_files"></a>
## 新增文件放在哪里

新功能要在本机存东西时，按顺序回答下面的问题，第一个答「是」的决定位置：

| 问题 | 位置 | 例子 |
| --- | --- | --- |
| 是凭据（密钥、令牌、带凭据的值）吗？ | `data/secrets/<类别>/<id>.json`，文件 `0600`，写入时先写临时文件再改名 | Agent 的 API Key、MCP 服务器的 header |
| 是 Agent 读写的工作目录吗？ | `work/<类别>/<id>/` | worktree、对话的工作目录 |
| 只在一个进程运行期间用，重启后没有意义吗？ | `tmp/<类别>/<id>/` | `TMPDIR`、预先准备服务器时的目录 |
| 删掉后能从网络或别的数据重新生成吗？ | `cache/<名字>`，写明谁在什么时候重新生成、留多少 | 适配器、价格表、工具列表 |
| 是给人排查问题看的输出吗？ | `logs/<类别>/<id>/` 或 `logs/<类别>/<id>.log` | 子进程的 stderr |
| 以上都不是：用户的数据或 Janus 的记录 | `data/<类别>/<id>/`；只有应用用的放 `data/app/` | 产物、附件、Skill 内容 |

再按这几条落地：

1. **属于某个对象的，放在它的 id 下**：`<类别>` 是对象的种类（`agents`、`runs`、`chats`、`library` 下的 `servers` 等），名字用 id，不用会改的名称，也不从文件名的形状认归属（规则 10）。新对象种类在 `Layout` 中加一个 `*_paths` 方法，列出它在各分类下的全部路径，删除时用它。
2. **在 `Layout` 中加方法**：一个位置一个方法，注释写明放什么、谁写、什么时候删；应用要用时在 `JanusHome` 中加同名的属性。其他代码只调用这些方法，不在 home 上拼名字。
3. **会增长的写明回收**：`cache/`、`logs/` 和 `data/` 中每次运行都会新增的东西，写明谁在什么时候删、保留多少，并在代码里实现。
4. **顶层不加新名字**：上面的分类够用；确实需要新的顶层分类时，先写 ADR，再改 `layout::TOP`。
5. **更新文档和测试**：在上面的目录树和分类表中加上它；删除对象时它也要删的，在删除的测试中检查。
6. **移动已有的文件是一次数据升级**：在 `MIGRATIONS` 末尾加一条迁移（`Step::Home`），用 `backend/src/migrations/relocate.rs` 的 `Relocation` 列出移动和删除，它负责改名、改写数据库和 Agent 文件中记下的旧路径（规则 11）；不在读取代码里兼容旧位置。

<a id="macos_app"></a>
## macOS 应用

### 从安装包安装

1. 在 GitHub Release 中下载 `Janus-<版本>.dmg`，打开后把 Janus 拖进「应用程序」。
2. 第一次打开：安装包没有用 Apple 开发者证书签名，macOS 会提示无法验证开发者。先打开一次 Janus，再到「系统设置 › 隐私与安全性」，在安全性一栏中点「仍要打开」，确认后即可正常使用，以后不再询问。也可以在终端执行 `xattr -dr com.apple.quarantine /Applications/Janus.app` 去掉下载标记。
3. 只支持 Apple 芯片（M 系列）的 Mac，需要 macOS 15 或更高（只支持最近三个 macOS 大版本，见 [macOS 应用](../architecture/20261031-frontend.md)）。安装包自带 Node.js（运行 ACP 适配器用），不需要 Rust、`just` 和 Node.js。Codex 和 Claude Code 的适配器在第一次使用时从 npm 下载，并自带对应的 CLI。

应用包里带有 Janus 服务。打开应用时，如果 Janus 已经在运行（例如 `just start` 启动的服务），应用直接连接它；否则启动包内的服务，退出应用时停止。配置的端口被其他程序占用时，应用自动改用空闲端口，并在侧栏底部提示。应用启动的服务使用包内的 Node.js；你在 `JANUS_NODE` 或 `config.toml` 的 `node` 中指定了 Node 时，以你的为准。数据都在 Janus home，与从源码安装的 Janus 共用。细节见 [macOS 应用](../architecture/20261031-frontend.md#server_process)。

升级：有新版本时，侧栏底部显示「Janus <版本> 可以更新」，点击打开「设置 › 通用」并滚到「更新」一节，在那里点「更新到 <版本>」后应用下载更新、退出并重新打开；菜单栏 Janus 图标菜单中的「检查更新…」也打开这一节并立即检查。应用内更新不需要再到「隐私与安全性」中允许，见[应用内更新](../architecture/20261031-frontend.md#updates)。应用不在可写的目录中（例如直接从 DMG 中运行）时不能自动更新，先把它拖进「应用程序」。

首次安装或升级后的权限引导见 [macOS 权限](../architecture/20261031-frontend.md#macos_permissions)。发布版本用固定的自签名证书签名（见[签名](20261101-ci_release.md#signing)），macOS 把各个版本当作同一个应用，完整磁盘访问、应用程序管理和屏幕录制在更新后保留。之前的 ad-hoc 签名版本每次更新都会变成「另一个应用」，2026.11.18.5 起 bundle ID 也换成了 `io.github.duoyuli.janus`：从这些版本升级上来时还需要重新授权一次（通知权限也要重新允许），应用的偏好设置恢复默认，系统设置里显示已打开的 Janus 属于旧版本，要先选中它点「−」删除，再在「设置 › 通用 › macOS 权限」中重新申请；授权后重新打开 Janus 才会生效。这与首次打开安装包时的 Gatekeeper 放行分别处理。

### 从源码构建

需要 Rust 工具链和 Swift 6（Xcode 或命令行工具都可以）：

| 命令 | 结果 |
| --- | --- |
| `just mac` | 从源码运行应用（连接已在运行的 Janus，否则启动 `~/.janus/bin/janus`） |
| `just app` | 构建 `dist/Janus.app`，本机架构 |
| `just dmg` | 构建 `dist/Janus.app` 和 `dist/Janus-<版本>.dmg` |
| `just test-mac` | 运行应用的测试，需要 Xcode 或命令行工具 |

`frontend/scripts/package.sh` 只在 Apple 芯片的 Mac 上运行，依次：同时构建 release 服务和应用（各自的输出写进 `dist/work/server.log`、`app.log`，失败时打印出来）、下载 Node.js（`NODE_VERSION`，默认 24.22.0，按 nodejs.org 的 SHASUMS256.txt 校验，缓存在 `dist/cache/`）、生成图标、组装 `Janus.app`、ad-hoc 签名、用 `hdiutil` 打成带「应用程序」快捷方式的 DMG。发布由 CI 完成，见 [CI 与发布](20261101-ci_release.md)。

`Janus.app` 的内容：

| 路径 | 内容 |
| --- | --- |
| `Contents/MacOS/Janus` | 应用 |
| `Contents/MacOS/janus-server` | Janus 服务（release 构建的 `janus`） |
| `Contents/Resources/{en,zh-Hans}.lproj/` | 打包脚本从 `frontend/Sources/Janus/` 原样复制语言目录，保留语言标识的大小写，供 macOS 选择应用语言和本地化权限用途说明 |
| `Contents/Resources/node/` | Node.js 的 `node`、`npm`、`npx`，供 ACP 适配器运行和安装 |

<a id="prerequisites"></a>
## 前置条件

| 依赖 | 用途 | 要求 |
| --- | --- | --- |
| Rust 工具链（`cargo`） | 编译 Janus | 支持 edition 2024 |
| `just` | 执行构建和服务命令 | macOS 可用 `brew install just` |
| Swift 6（Xcode 或命令行工具） | 编译 macOS 应用 | 测试需要 Xcode |
| Node.js | 运行 ACP 适配器，安装适配器 | 22.19 及以上；安装包中的 Janus.app 自带，不需要另装 |
| `npm`、`npx` | 安装适配器 | 随 Node.js 安装 |
| `git` | 检查工作目录 | |

安装和检查适配器更新需要访问 npm。

<a id="install"></a>
## 安装

```sh
just install
```

`just install` 执行 `cargo build --release --locked`，把二进制安装到 `~/.janus/bin/janus`。安装时先写到临时文件再改名，不会把正在运行的服务替换成写了一半的文件。

安装后的服务不需要仓库。开发时用 `just dev`（等于 `cargo run`）启动服务，用 `just mac` 从源码运行应用。

<a id="run_as_service"></a>
## 启动

两种方式：

| 方式 | 命令 | 适用 |
| --- | --- | --- |
| 前台运行 | `just run` | 临时使用、排查问题 |
| 服务常驻（macOS） | `just start` | 日常使用：登录后自动启动，异常退出后自动重启 |

`just start` 生成 launchd 配置 `~/Library/LaunchAgents/local.janus.plist` 并启动服务。要点：

- launchd 启动的进程只有很短的 `PATH`，找不到 Homebrew 等位置的 `node`、`codex`、`claude`、`git`、`npm`。所以 `just start` 把执行时终端的 `PATH` 写进服务配置。这些工具的位置变化后，重新执行 `just start`。
- 服务在正常退出时不重启，异常退出后 10 秒内重启。
- 标准输出和错误写入 `~/.janus/logs/janus.out.log`；Janus 自己的日志在 `~/.janus/logs/janus.log`。
- `just plist` 只打印将要写入的配置，不安装。

启动后打开 Janus 应用（安装包中的 Janus.app，或从源码 `just mac`），它会连接这个服务，然后按顺序配置 Agent、角色和工作区。第一次检查某个 Agent 时，它需要的 ACP 适配器会自动下载安装，之后由 Janus 在后台保持最新。

其他系统没有 launchd：用 `just run`，或者把 `~/.janus/bin/janus` 交给 systemd 用户服务等进程管理器，并给它设置包含 `node` 和 Agent CLI 的 `PATH`。

<a id="single_instance"></a>
## 同一个 Janus home 只运行一个实例

启动时 Janus 对 `~/.janus/janus.pid` 加一把独占的系统文件锁（`lock_home`），拿到后再写入自己的进程号；锁被别的 Janus 持有时，新实例报错退出，不改动这个文件。原因是第二个实例启动时会把第一个实例正在跑的 Run 全部标记为中断，并且两个实例会同时驱动同一个数据库。加锁是原子的，两个同时启动的实例只有一个能拿到。

- 服务在跑时想用开发版本：先 `just stop`，或者给开发版本设置另一个 `JANUS_HOME`（例如复制一份 `~/.janus` 用于测试）。
- 锁随进程释放：进程无论怎样退出，留下的 `janus.pid` 都不影响下次启动。文件里的进程号只是给人和应用看的（应用据此找到正在运行的 Janus），不用来判断锁。

<a id="operate"></a>
## 日常操作

| 目的 | 命令 |
| --- | --- |
| 查看服务状态，以及 HTTP 服务是否响应 | `just status` |
| 持续查看日志 | `just logs` |
| 重启服务 | `just restart` |
| 停止服务并取消登录自启 | `just stop` |
| 查看版本 | `~/.janus/bin/janus --version` |

重启或停止时进行中的 Run 会中断，之后可以在应用中「从中断处继续」，见 [Run 生命周期](../domains/20261031-run_lifecycle.md#recovery)。

<a id="upgrade"></a>
## 升级

| 升级对象 | 做法 |
| --- | --- |
| Janus 本身（服务） | 更新仓库代码后执行 `just install`，再执行 `just restart` |
| Janus 本身（macOS 应用） | 在「设置 › 通用 › 更新」中点「更新到 <版本>」；侧栏底部的「可以更新」和菜单栏 Janus 图标菜单中的「检查更新…」都打开这一节。上一个版本保留在 `~/.janus/cache/updates/previous/Janus.app`，需要退回时退出应用、把它拖回「应用程序」。也可以下载 DMG 覆盖安装 |
| ACP 适配器 | 自动检查并升级，不需要操作；要回旧版本时在「设置 › 数字员工」的 Agent 卡片上「切回历史版本」，切换并固定，选「恢复自动更新」回到最新。不需要重启 Janus，见 [ACP 适配器管理](../architecture/20261031-adapters.md) |
| Agent CLI | 用它自己的方式升级（例如 Homebrew），然后在「设置 › 数字员工」重新检查 |

数据随版本自动升级：新版本的服务第一次启动时，先把 `data/janus.db` 备份到 `backups/`，再把表结构、设置对象、Run 快照和默认数字员工升级到这个版本的数据版本，日志 `logs/janus.log` 写明升级了什么和备份位置。不需要单独运行升级脚本；想先看看会改什么，执行 `janus migrate --dry-run`（在副本上执行，不修改数据）。详见[数据升级](../architecture/20261031-data_model.md#migrations)。

- 升级要由新版本的服务执行。安装包里的应用连上其他版本的服务（例如还在运行的 `just dev` 或旧的 launchd 服务）时会提示，点「停止旧服务并重试」后改用应用自带的服务；以服务方式运行时先 `just stop`，或 `just install && just restart` 升级服务。
- 退回旧版本：旧版本不能打开升级过的数据，会拒绝启动。先退出 Janus，把 `backups/` 中升级前的备份复制为 `data/janus.db`（同时删除 `data/janus.db-wal` 和 `data/janus.db-shm`），再打开旧版本；升级后新写入的数据不在备份里。
- 退回数据版本 14 之前的版本（2026-11-21 之前发布的）：那些版本把数据库放在 Janus home 顶层，也不认识按类别整理后的目录。要把 `backups/` 中 `janus-v13-…` 或更早的备份复制为顶层的 `janus.db`，旧版本会把它当作全部数据，`data/` 等目录中的文件它都用不到，见 [ADR：Janus home 按「丢了能不能再有」分类](../decisions/20261121-home_layout.md)。

<a id="backup"></a>
## 备份与迁移到另一台机器

- 备份：先 `just stop`，再复制 `~/.janus/data`；要连正在进行的任务一起保留，也复制 `work/`。`cache/`、`tmp/` 和 `logs/` 不用备份。
- 迁移：在新机器上安装 Janus，把备份放回 `~/.janus` 的同一位置，执行 `just start`。数据库中记录的路径是绝对路径，用户名或 `JANUS_HOME` 不同时，旧 Run 的产物链接会失效，Run 记录本身不受影响。

<a id="uninstall"></a>
## 卸载

```sh
just uninstall
```

`just uninstall` 停止服务，删除 launchd 配置和 `~/.janus/bin/janus`，保留其余数据。要彻底删除，再手动删除 `~/.janus`。
