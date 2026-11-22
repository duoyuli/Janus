# ADR：用户自己选择共享给 Agent 的本机登录和工具配置

- 状态：已采纳（accepted），记录于 2026-11-22。补充 [ADR：每个 Agent 使用独立的配置目录](20261031-isolated_agent_homes.md)：隔离的 HOME 不变，用户可以逐项把本机的路径链接进去。
- 相关文档：[安全边界：共享的本机环境](../operations/20261031-security.md#host_env)、[配置：传给 Agent 的变量](../interfaces/20261031-configuration.md#agent_env)、[macOS 应用：本机环境](../architecture/20261031-frontend.md#host_env)、[ADR：角色的 Skill、MCP 和指令只在会话启动时注入](20261116-launch_time_resources.md)。

## 背景

每个 Agent 和资源库路由启动的 MCP 服务器都在自己的 HOME 里运行（`sandbox_vars`）。命令行工具在里面找不到用户的登录和配置：

- macOS 按 `$HOME/Library/Keychains` 找登录钥匙串，HOME 换掉后 `security list-keychains` 只剩系统钥匙串，GitHub CLI、飞书 CLI、Git 的 `osxkeychain` 都存在这里。
- 工具自己的目录（`~/.config/gh`、`~/.lark-cli`、`~/.gitconfig`、`~/.aws`、`~/.rustup` 等）也在 HOME 下。

实测：把 `~/Library/Keychains` 和 `~/.config/gh` 链接进隔离的 HOME 后 `gh api` 可用，只设 `GH_CONFIG_DIR` 不够；`~/.lark-cli` 加钥匙串后 `lark-cli auth status` 可用。又实测：隔离的 HOME 本来就挡不住凭据，在里面给出钥匙串的完整路径照样能读，用绝对路径也能读 `~/.config/gh/hosts.yml`。隔离 HOME 的作用是不让用户自己的指令、Skill、MCP 被 Agent 默认读进来，原 ADR 也写明「不是沙箱」。

常用工具不可能列全，用户各自的开发环境也不同。

## 决策

1. 保留隔离的 HOME。用户在设置的「本机环境」中逐项选择要共享的用户目录路径，全局一份，所有 Agent 和 MCP 服务器共用；默认一项都不共享。
2. 选中的路径以符号链接放进每个隔离 HOME 的同一位置，工具读到、刷新的都是用户本机的同一份。保存时立即改动所有已有的隔离 HOME（正在运行的会话也生效），每次启动 Agent 或 MCP 服务器时再补一遍。
3. 可选的路径分三层，不靠穷举：Janus 认识的常用工具（登录与账号、工具链、钥匙串），只列本机存在的；用户目录下其他的隐藏项、`~/.config/*` 和 `~/.local/share/*`，按路径列出；以及用户手填的用户目录下的路径。
4. 一律拒绝：Agent 自己的配置（`~/.codex`、`~/.claude`、`~/.agents` 等）和 `~/.janus`，以及包含它们的目录；`.config`、`Library` 这类整片的目录；用户目录以外的路径、本机不存在的路径、一个在另一个里面的路径。
5. 选中的工具登录存在钥匙串里时，应用同时打开钥匙串，并说明一句；关掉工具时钥匙串保留，由用户自己关。
6. 读写这份设置的接口（`GET/PUT /api/host-env`）只回答本机的应用：它列出用户目录的内容，也决定 Agent 能用到什么。
7. 隔离 HOME 里原来就有同名的文件或目录时，先改名为 `<名字>.janus-own` 留着，不再共享时放回；撤回时只删指向本机同一路径的链接。

## 取舍

- **与不再隔离 HOME 相比**：所有工具都能直接用，但 Codex 会读 `~/.agents/skills`，用户个人的 Skill、指令和 MCP 会进到每个数字员工里，违背启动时注入的决定。
- **与注入令牌环境变量相比**（例如启动时用 `gh auth token` 设 `GH_TOKEN`）：每个工具要单独适配，飞书这类没有对应变量，令牌还会出现在进程环境里。
- **与在 Agent 环境里各自登录相比**：每个 Agent、每个工具都要登一遍，钥匙串仍不可用，令牌退回明文保存。
- **代价**：共享钥匙串后，Agent 也能读到钥匙串里的其他条目。按背景中的实测，这和现状相比没有扩大 Agent 实际能读到的范围，只是让工具默认找得到；设置页写明这一点，由用户决定。

## 后果

- 设置对象多一种 `host_link`，每个共享的路径一行，id 是路径；没有旧数据，不需要迁移。
- 新的配置项 `JANUS_HOST_HOME` 指定用户自己的用户目录，默认 `$HOME`；端到端测试用它指向替身目录，不碰真实的用户目录。
- 已确认：钥匙串可见时，配置目录隔离的 Claude Code 仍是未登录状态，不会用上用户自己的 Claude 登录。
