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
| 角色 MCP 的环境变量值 | `~/.janus/secrets/roles/<role-id>.json` | `0600` |
| Janus home | `~/.janus/` | `0700` |

- SQLite 和设置接口只记录凭据字段和变量的名字（`secretsSet`、`importedEnv`、`envKeys`），不返回值。
- 登录日志返回给应用前，凭据值替换为 `<secret>`。
- 切换认证方式时，新方式用不到的凭据被删除。

不要把凭据写进 Agent 设置的「环境变量」文本：这些变量作为普通配置保存在 SQLite 中，并通过设置接口原样返回。

<a id="workspace_access"></a>
## 工作目录访问

- 任务直接在用户选择的目录中运行，改动立即生效。并行的写任务可能修改同一批文件。
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

- 适配器从 npm（`acp-extension-codex`、`acp-extension-claude`）和 GitHub（`LodyAI/acp-extension-grok`、`LodyAI/acp-extension-pi`）下载，Janus 不校验包的签名，信任边界与手动执行 `npm install` 相同。
- 安装命令在 Janus 进程自己的环境中运行（`PATH`、npm 配置、代理），不是 Agent 的隔离环境。安装过程会执行包自带的安装脚本和构建脚本。
- 来源写在代码中，应用和接口不能指定其他来源。

<a id="skill_import"></a>
## Skill 导入

- Agent 不读取用户自己的 Agent Skill 目录。Skill 只能在应用中新建，或从用户选择的目录复制一次进入 Janus Skill 库。
- 复制时只复制普通文件，符号链接取其内容，不跟随到所选目录之外。
- 工作目录中的项目 Skill 只有真实路径仍在工作目录内时才会列给 Agent。

<a id="scope_limits"></a>
## 适用范围

当前是单机、单用户模式，适合本地开发和实验。不提供多租户、远程执行、强隔离执行或生产级访问控制。在用于重要项目之前，先评估共享工作目录、无鉴权和缺少操作系统级沙箱这三项限制。
