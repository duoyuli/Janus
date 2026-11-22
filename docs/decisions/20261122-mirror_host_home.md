# ADR：Agent 的用户目录镜像用户自己的，只挡列出的路径

- 状态：已采纳（accepted），记录于 2026-11-22。取代 [ADR：用户自己选择共享给 Agent 的本机登录和工具配置](20261122-share_host_env.md)；补充 [ADR：每个 Agent 使用独立的配置目录](20261031-isolated_agent_homes.md)。
- 相关文档：[安全边界：本机环境](../operations/20261031-security.md#host_env)、[配置：传给 Agent 的变量](../interfaces/20261031-configuration.md#agent_env)、[macOS 应用：本机环境](../architecture/20261031-frontend.md#host_env)。

## 背景

上一版让用户逐项勾选要共享的路径，并为常见工具内置了路径清单。实际用下来，这份清单永远补不全：飞书 CLI 的配置在 `~/.lark-cli`，用户令牌和 DPoP 密钥却在 `~/Library/Application Support/lark-cli`，只勾前者时 Agent 能认出用户，`--as user` 却报 `token_missing`。用户也无从知道某个工具在 Agent 里能不能用。

隔离 HOME 要挡的从来只有一类东西：Agent 自己的配置（指令、Skill、MCP、登录），让数字员工只用 Janus 给的。它挡不住凭据：在隔离的 HOME 里用钥匙串或文件的完整路径照样能读，原 ADR 也写明「不是沙箱」。

## 决策

1. **镜像**：每个 Agent 和 MCP 服务器的 HOME 是用户目录的镜像，用户目录下的每一项都以符号链接放在同一位置（`host_env.rs`）。`Library`、`Library/Application Support`、`.config`、`.local`（及其 `share`、`state`）、`.cache` 逐项镜像，因为隔离 HOME 的 XDG 目录在里面，而且它们里面放着各家 Agent 和工具的东西；挡住的路径所在的目录也逐项镜像，其余的整项链接。
2. **黑名单**：一份可编辑的「Agent 看不到的路径」，全局一份。默认是常见 Agent 的配置（`~/.agents`、`~/.claude`、`~/.claude.json`、`~/.codex`、`~/.pi`、`~/.gemini`、`~/.cursor`、`~/.config/claude` 等，`Library/Application Support` 里的 Claude、Cursor）和我们认为高危的数据（`~/.aws`、`~/.azure`、`~/.config/gcloud`、`~/.gnupg`、`~/.kube`、`~/.password-store`，1Password、Chrome、Firefox 的数据，`Library/Cookies`、`Library/Mail`、`Library/Messages`）。用户可以删掉任何一项，也可以加；没改过时跟着默认清单更新。Janus home 在用户目录里时一直挡着，不在清单里。
3. `~/.ssh` 不默认挡：OpenSSH 按系统账户找用户目录，不看 `$HOME`，挡了也挡不住，用户还要用它推代码。`SSH_AUTH_SOCK` 传给 Agent，它能用用户的 ssh-agent。
4. **试一条命令**：设置里输入一条命令，Janus 分别以本机用户（Janus 的环境、用户目录）和 Agent 的方式（一个和 Agent 一样镜像的探测沙箱）各跑一次，并排给出退出码和输出。
5. 改动立即作用于所有已有的隔离 HOME，启动 Agent 或 MCP 服务器时再镜像一遍；用户目录里新出现的东西在下次启动时出现。读写这份设置和试命令的接口只回答本机的应用。
6. 隔离 HOME 里原有的同名内容改名为 `<名字>.janus-own` 留着，那个路径被挡住时放回；Janus 只删自己建的、指向用户同一路径的链接，从不删用户的文件。

## 取舍

- **与逐项共享相比**：不用维护每个工具的路径，终端里能用的命令在 Agent 里基本都能用，新装的工具也一样；要维护的是 Agent 配置的位置，数量少、变化慢。
- **与不隔离 HOME 相比**：Agent 自己的配置仍然挡得住；代价是每次启动要镜像一遍（几百次 `lstat` 和链接）。
- **代价**：Agent 能读写用户目录里没挡住的一切，工具写缓存、刷新令牌都写到用户自己的文件；钥匙串默认可见（已确认配置目录隔离的 Claude Code 不会因此用上用户自己的 Claude 登录）。新出的 Agent 把配置放在新位置时，要补进默认清单。

## 后果

- 设置对象 `host_env`（一行，`paths`）取代 `host_link`；数据版本 21 删掉原来逐项共享的记录。
- 新增 `POST /api/host-env/try`，探测沙箱在 `tmp/host-probe/`。
