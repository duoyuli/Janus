# 安全边界

本文说明 Janus 已经做到的隔离、凭据怎样保存，以及使用时必须注意的限制。Janus 是单机、单用户的实验性工具，这些边界不等于安全沙箱。

<a id="agent_isolation"></a>
## Agent 运行环境隔离

每个 Agent 子进程的环境由 Janus 完整构造（`spec_for`、`sandbox_env`），不继承 Janus 自身进程的环境：

- 只从宿主继承 `PATH`、语言环境、`TERM`、`USER`、`LOGNAME`、`SHELL`、`TZ`。
- `HOME`、XDG 目录和 `TMPDIR` 指向 `~/.janus/agents/<id>/` 下的专属目录，Agent 查找 `~/...` 时落在自己的目录里。
- Agent 的配置目录（`CODEX_HOME`、`CLAUDE_CONFIG_DIR`、`GROK_HOME`、`PI_CODING_AGENT_DIR`）是 `~/.janus/agents/<id>/home`。用户自己的 `~/.codex`、`~/.claude`、`~/.grok`、`~/.pi` 不会被读取或修改。
- 用户在 Agent 设置中填写的环境变量不能覆盖上面这些由 Janus 控制的变量。
- 自定义命令的 Agent 使用同样的隔离环境。

完整的变量清单见[配置](../interfaces/configuration.md#agent_env)。

例外：Claude Code 做主控时，Janus 在它的隔离配置 `~/.janus/agents/<id>/home/.claude.json` 中把工作目录标记为已信任。这只修改 Janus 为该 Agent 创建的配置，不修改用户自己的 Claude 配置。

<a id="local_config_import"></a>
## 复制本机配置

选择「复制本机配置」时，只复制与模型接入有关的内容：

| 运行时 | 保留的配置项 |
| --- | --- |
| Codex | `model`、`model_provider`、`model_providers`、`model_reasoning_*`、`model_verbosity`、`model_context_window`、`model_auto_compact_token_limit`、`model_catalog_json`（引用的模型目录一并复制）、`openai_base_url`、`service_tier` |
| Claude Code | `model`，以及 `settings.json` 中配置模型接入点的 `env` 项 |
| Grok | `models`、`model` |
| Pi | `defaultProvider`、`defaultModel`、`defaultThinkingLevel`、`enabledModels` |

指令文件、hooks、插件、MCP、Skill、权限和沙箱策略都不复制。

凭据来源：

- 运行时自己的凭据文件（例如 `auth.json`）。
- Claude Code 在 macOS 钥匙串中的登录信息，通过 `security` 命令读取。
- 用户登录 shell 中导出的凭据变量：标准变量，以及配置文件中引用的变量（Codex 的 `env_key`、Pi 的 `apiKey`）。Janus 以交互式登录 shell 读取环境，与新开终端看到的一致。

应用中的预览（`/api/local-config`）只列出会复制的文件和变量名，不显示值。

<a id="secrets"></a>
## 凭据保存

| 内容 | 位置 | 权限 |
| --- | --- | --- |
| Agent 的凭据字段、复制来的凭据变量 | `~/.janus/agents/<id>/secrets.json` | `0600` |
| 角色手工 MCP 的环境变量值 | `~/.janus/secrets/roles/<role-id>.json` | `0600` |
| MCP 资源库的环境变量和 HTTP header 值 | `~/.janus/secrets/mcp/<来源 id>/<key>.json` | `0600` |
| Janus home | `~/.janus/` | `0700` |

- SQLite、`source.json`、Run 快照和设置接口只记录凭据字段和变量的名字（`secretsSet`、`importedEnv`、`envKeys`、`headerKeys`、`envSet`、`headerSet`），不返回值。
- 登录日志返回给应用前，凭据值替换为 `<secret>`。
- 切换认证方式时，新方式用不到的凭据被删除。

不要把凭据写进 Agent 设置的「环境变量」文本：这些变量作为普通配置保存在 SQLite 中，并通过设置接口原样返回。

<a id="workspace_access"></a>
## 工作目录访问

- 任务直接在用户选择的目录中运行，改动立即生效。并行的任务可能修改同一批文件。
- 一个 Agent 能做什么由它自己的权限模式执行。Janus 不做操作系统级的文件、网络或终端限制，权限模式也不是 Janus 的强制安全策略。
- Janus 不提交、不推送。主控和任务提示中都要求 Agent 不要 commit、push 或部署，但这只是指令，不是强制。
- 非 Git 目录没有 diff，也无法用 Git 回退。

<a id="network_exposure"></a>
## 网络暴露

- HTTP 服务没有鉴权。默认只监听 `127.0.0.1:7878`。
- 把 `JANUS_ADDR` 改为非回环地址，会把设置、凭据名、运行记录、目录浏览和全部控制接口暴露给能访问该地址的任何客户端。
- 看板 MCP 端点用每个 Run 的随机令牌区分 Run，防止其他 Run 和本机其他进程误用；它不是面向网络的认证机制。
- `/api/file` 只返回 `~/.janus/artifacts/` 内的文件，Janus home 中的数据库和凭据不能通过它读取。选择目录用系统的文件夹选择框，服务不提供目录浏览接口。

<a id="adapter_install"></a>
## 适配器安装

- 适配器从 GitHub（`LodyAI/acp-extension-codex`、`LodyAI/acp-extension-claude`、`LodyAI/acp-extension-grok`、`LodyAI/acp-extension-pi`）的默认分支下载，依赖从 npm 安装，Janus 不校验提交或包的签名，信任边界与手动执行 `git clone`、`npm install` 相同。
- 安装命令在 Janus 进程自己的环境中运行（`PATH`、npm 配置、代理），不是 Agent 的隔离环境。安装过程会执行包自带的安装脚本和构建脚本；有构建脚本时连同开发依赖一起安装，构建后再删去开发依赖。
- 来源写在代码中，应用和接口不能指定其他来源。

<a id="skill_import"></a>
## Skill 导入

- Agent 不读取用户自己的 Agent Skill 目录。Skill 只能在应用中新建，或从用户选择的目录复制一次进入 Janus Skill 库。
- 复制时只复制普通文件，符号链接取其内容，不跟随到所选目录之外。
- 工作目录中的项目 Skill 只有真实路径仍在工作目录内时才会列给 Agent。
- GitHub 来源的仓库是第三方内容。Janus 只克隆和读取，不执行仓库中的任何文件；解析和 Run 快照都不跟随符号链接，克隆时不下载 Git LFS 内容、不弹出凭据输入。Skill 的内容会作为说明交给 Agent，是否可信由用户在添加来源和挂载到角色时判断。

<a id="mcp_sources"></a>
## MCP 来源

- GitHub 来源的仓库是第三方内容。Janus 只克隆和读取其中的 `mcp.json` / `.mcp.json`，不执行仓库中的任何文件；解析不跟随符号链接，克隆时不下载 Git LFS 内容、不弹出凭据输入。
- manifest 中的 `command`、`args`、`url` 会原样交给 Agent，由 Agent 在会话中启动或连接。是否信任一个来源、是否把其中的服务器挂到角色，由用户在导入和挂载时判断。
- manifest 声明的 `env` / `headers` 值写到 `secrets/mcp/`（`0600`）。接口只返回变量名和 `envSet` / `headerSet`；`source.json`、SQLite、日志和 Run 快照不保存值。删除来源时一并删除这些值。
- 资源库中的 HTTP 服务器需要 Agent 支持 HTTP MCP。Agent 没有声明该能力时，Janus 不把 http 服务器放进会话。
- 删除来源有保护：角色仍挂载其中的服务器，或有 `planning`、`running`、`waiting_input`、`interrupted`、`failed` 的 Run 快照还引用它时，接口返回错误。

<a id="app_updates"></a>
## 应用更新的来源

- 更新只来自 Info.plist 中 `JanusUpdateRepo` 指定的 GitHub 仓库的最新 Release，通过 HTTPS 下载，每个部分都核对 `update.json` 中的 SHA-256。
- 当前应用用固定证书签名时，新版本必须满足当前应用的指定要求（同一个 bundle id、同一张证书），否则拒绝安装。这挡住了被篡改的 Release 附件；能签出被接受的更新的只有持有签名证书的人，证书的 `.p12` 和 Actions secrets 要妥善保管。
- 当前应用是 ad-hoc 签名时没有签名者可以要求，只依靠 HTTPS 和 SHA-256，信任程度与手动下载 DMG 相同。
- 替换脚本只移动应用包、打开新应用，不使用管理员权限；应用所在目录不可写时放弃更新。

<a id="scope_limits"></a>
## 适用范围

当前是单机、单用户模式，适合本地开发和实验。不提供多租户、远程执行、强隔离执行或生产级访问控制。在用于重要项目之前，先评估共享工作目录、无鉴权和缺少操作系统级沙箱这三项限制。
