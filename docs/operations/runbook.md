# 运行手册

本文是日常使用和排障的速查：第一次怎样跑起来、重启和恢复、常见问题去哪里看。安装、常驻、升级、备份和卸载的完整说明见[部署方案](deployment.md)，测试见[测试](testing.md)，安全限制见[安全边界](security.md)。

<a id="first_run"></a>
## 第一次运行

前置条件见[部署方案](deployment.md#prerequisites)。在仓库根目录：

```sh
just install   # 编译并安装到 ~/.janus/bin/janus
just start     # 以服务方式启动（macOS）；或 just run 在前台运行
```

开发时可以直接 `just dev`（等于 `cd backend && cargo run`）启动服务，再用 `just mac` 从源码运行应用。

打开 Janus 应用（安装包中的 Janus.app，或从源码 `just mac`），它会连接这个服务。按顺序配置：

1. 「设置 → Agents」：添加 Agent 并运行配置检查。第一步「ACP 适配器」会在需要时自动下载安装适配器，之后五步全部通过即可。
2. 「设置 → 角色」：为角色绑定 Agent，选择模型和权限模式。
3. 新建工作区：选择目录，添加成员，指定一名 Claude Code 或 Codex 角色为主控。
4. 在工作区首页输入目标，交给主控。

`examples/calc/` 可以作为第一个工作区。

<a id="restart"></a>
## 重启

- 开发时改了 Swift 代码：重新 `just mac`。改了 Rust 代码：重新 `just dev`。
- 已安装的服务：`just install` 后 `just restart`。
- 重启时进行中的 Run 变为 `interrupted`，Agent 会话全部关闭。在应用的运行页点击「从中断处继续」，或直接给某个 Agent 发消息，即可恢复，见 [Run 生命周期](../domains/run_lifecycle.md#recovery)。
- 同一个 `JANUS_HOME` 不要同时运行两个 Janus。Run 租约会阻止两个实例驱动同一个 Run，但设置对象没有并发保护。

<a id="troubleshooting"></a>
## 排查

日志都在 `~/.janus/logs/`：`janus.log` 是 Janus 自己的日志，`janus.out.log` 是服务进程的输出，`adapters/<id>.log` 是适配器安装记录，`<run>/` 下是每个 ACP 子进程的 stderr。`just logs` 持续查看 `janus.log`。

| 现象 | 检查 |
| --- | --- |
| 配置检查停在「ACP 适配器」 | `~/.janus/logs/adapters/<id>.log` 的最后几行；常见原因是没有网络、`node` 版本太旧、找不到 `npm` / `git`。在「设置 → ACP 适配器」可以重试 |
| 配置检查停在「运行时」 | Agent CLI 是否在 `PATH` 中，或在 Agent 设置中填写二进制路径。以服务方式运行时，`PATH` 来自执行 `just start` 的终端 |
| 配置检查停在「启动 Agent」 | 检查日志 `~/.janus/agents/<id>/test.log`（账号登录的日志是同目录的 `login.log`）；60 秒内没有完成握手通常是适配器或 Node 版本问题，可以在「设置 → ACP 适配器」切回上一个版本 |
| 配置检查停在「打开会话」或「试运行」 | 凭据或模型配置错误；提示中会带上 Agent 返回的认证错误 |
| 升级适配器后 Agent 异常 | 在「设置 → ACP 适配器」切回上一个版本，新启动的会话立即使用旧版本 |
| 创建 Run 时提示主控不能做主控 | 主控角色必须绑定 Claude Code 或 Codex |
| 主控拒绝 `/goal` | Agent 版本是否支持 `/goal`；Claude Code 是否允许 hooks；主控日志 `~/.janus/logs/<run>/lead-goal.log` |
| 任务卡在等待 | 工作台「需要你处理」中是否有审批、表单、提问或主控请你做的决定；看板「待处理」中标「主控处理」的事项（提问、失败、依赖失败）是否长时间没有进展 |
| 协作者没有看板工具 | Agent 是否声明 `mcpCapabilities.http`；Pi 不支持，属预期行为 |
| 某次尝试失败 | `~/.janus/logs/<run>/<task>-a<n>.log` 是该子进程的 stderr；事件 `attempt.failed` 的 `data.error` 是失败原因 |
| 发出的消息没进对话 | 看输入框上方的「待发送」区：「发送中」表示会话还没处理到它，「排队」表示要等这一轮结束，「留言」表示这个 Agent 当前没有会话 |
| 界面不更新 | SSE 断开或缓冲溢出时应用会自动重新读取 Run；仍无变化时用 ⌘R 刷新 |
| macOS 应用打不开，提示无法验证开发者 | 安装包没有用 Apple 证书签名：到「系统设置 → 隐私与安全性」点「仍要打开」，见[部署方案](deployment.md#macos_app) |
| macOS 应用停在「Janus 服务没有启动」 | 启动页显示的是 `~/.janus/logs/janus.out.log` 的末尾。端口被占用时应用会自动换端口（见 [macOS 应用](../architecture/frontend.md#port_conflict)），所以多半是另一个用同一 Janus home 的 Janus 还在运行却连不上，先退出它（`just stop`）再点「重试」 |
| macOS 应用侧栏显示「端口 … 被其他程序占用，改用 …」 | 正常：配置的端口被别的程序占着，这次服务用了提示中的端口。想固定端口时，在 `~/.janus/config.toml` 中把 `addr` 改成空闲的端口 |
| macOS 应用里 Agent 检查报找不到 `node` 或 CLI | 应用从登录 shell 读取 `PATH`；确认在新开的终端里 `which node` 能找到。连接的是 launchd 服务时，`PATH` 来自执行 `just start` 的终端 |
| 服务起不来 | `just status` 看进程状态；`~/.janus/logs/janus.out.log` 看启动错误，例如端口被占用，或「另一个 Janus 正在使用」，见[部署方案](deployment.md#single_instance) |

完整的事件记录可以在运行页的工具栏导出，或请求 `GET /api/runs/{id}/export`。

<a id="data_cleanup"></a>
## 数据清理

- 删除 `~/.janus/artifacts/` 或 `~/.janus/logs/` 会丢失产物或日志，数据库中的记录仍然指向这些路径。
- 删除 `~/.janus/adapters/` 后，下次检查 Agent 时会重新下载适配器。
- 删除整个 `~/.janus/` 会丢失所有 Agent、角色、工作区、凭据和 Run 记录。
- 没有内置备份工具，备份方法见[部署方案](deployment.md#backup)。
