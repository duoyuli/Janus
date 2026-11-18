# 资源库

本文说明资源库的规则：资源有哪些、从哪里来、怎样更新，角色怎样选择，Run 怎样固定资源，会话怎样拿到它们。设计取舍见 [ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](../decisions/20261116-launch_time_resources.md)，接口见 [HTTP API](../interfaces/20261031-http_api.md#library)，凭据和第三方内容的边界见[安全边界](../operations/20261031-security.md#library)。

实现在 `backend/src/library.rs` 和 `backend/src/library/`。`offers` 扫描可导入的资源，`store` 和 `skill` 管理 Skill 内容及 `SKILL.md`，`mcp` 管理服务器定义和值，`sources` 管理 Git 来源，`deliver` 生成资源包和会话参数。

<a id="model"></a>
## 资源

资源库中的每一项是一个资源（`Item`），保存在 SQLite 的 `configs` 中（类型 `library_item`）：

| 字段 | 说明 |
| --- | --- |
| `id` | 不带含义的 id（`i` 加时间），不从它读出类型或来源 |
| `kind` | `skill` 或 `mcp` |
| `name` | Agent 看到的名字。同一类资源中唯一 |
| `description` | 说明。Skill 取 `SKILL.md` 的描述（列表中截到 240 个字符） |
| `origin` | 来源：`created`（在 Janus 中新建或粘贴的配置）、`folder`（从本机目录复制，记下目录）、`git`（从 Git 来源导入，记下来源 id 和路径） |
| `revision` | 内容的 sha256：Skill 是全部文件，MCP 是服务器定义 |
| `server` | 只有 MCP 有：传输方式和启动方式，见 [MCP 服务器](#mcp) |

规则：

- 所有修改依次执行（`LibraryState` 的锁），页面操作和后台检查不会同时改资源库。
- 新建和编辑时名字被占用会报错；导入时名字被占用自动加 `-2`、`-3`。
- 被角色使用的资源不能删除，先在角色中取消选择。已经开始的 Run 有自己的副本，删除资源不影响它们；但删除后 MCP 的值也被删除，旧 Run 恢复时拿不到。

<a id="skills"></a>
## Skill

Skill 是一个带 `SKILL.md` 的目录，名称和描述来自 front matter。

- **名字**：小写字母、数字和 `-`，最长 64 个字符。导入时由 front matter 的 `name`（没有时取目录名）转换而来，转换后为空时用 `skill`。交给 Agent 的 `SKILL.md` 中，front matter 的 `name` 改成资源库中的名字；缺少描述时补上资源的描述。
- **新建**：在编辑器中填写名字、描述和正文；也可以粘贴完整的 `SKILL.md`，这时描述以文件中的为准。没有描述时拒绝保存，因为 Agent 按描述判断何时使用 Skill。只有新建的 Skill 可以在 Janus 中编辑。
- 选择「从文件夹复制…」后，可以选单个 Skill 目录或包含多个 Skill 的父目录。Janus 使用与 [Git 来源](#sources) 相同的目录扫描规则，最多列出 500 个 Skill。列表默认勾选所有尚未导入的条目，可以逐项调整或全不选，再点「导入所选」一次复制。
- 本机导入按每个 Skill 的原目录记录来源，已复制的条目显示「已导入」。复制失败的条目会显示原因，可以单独重试。内容以复制时的版本为准，使用「刷新」可以重新复制原目录。
- **存放**：按内容的 sha256 放在 `~/.janus/library/store/<hash>/`，内容相同只存一份，存下后不再修改。复制时跳过隐藏文件（`.git`、`.env` 等）和 `node_modules`；符号链接只在指向目录内部时才读取；单个 Skill 最多 2000 个文件、20MB。没有资源使用的内容在每次修改后清理。

<a id="mcp"></a>
## MCP 服务器

服务器的定义保存在资源中，值保存在 `~/.janus/secrets/library/<id>.json`（目录 `0700`，文件 `0600`）。

| 传输方式 | 定义 | 值 |
| --- | --- | --- |
| `stdio` | `command`、`args`、`envKeys`（环境变量名） | 环境变量的值 |
| `http`（streamable HTTP） | `url`、`headerKeys`（HTTP header 名） | header 的值 |
| `sse` | `url`、`headerKeys` | header 的值 |

- **名字**：字母、数字、`-` 和 `_`，最长 64 个字符；规范化后是 `janus` 的名字（`janus`、`Janus`、`janus!` 等）保留给看板 MCP。
- **表单**：填写名字、传输方式、启动命令或地址和变量列表（`vars`）。变量的值留空表示保留已保存的值；从列表中删除的变量，它的值一起删除。
- **粘贴配置**：接受 Claude 的 `mcpServers`、VS Code 的 `servers`，或一个带 `name` 的服务器对象，一次导入其中所有服务器，配置中写的值存进值文件。用不了的条目作为警告返回。
- 资源、接口返回值、Run 快照和事件中只有变量名和「是否已设置」，没有值。

<a id="sources"></a>
## Git 来源

Git 来源是一个 GitHub 仓库或仓库中的一个目录。添加链接后，Janus 列出其中的 Skill 和 MCP 服务器，由用户选择导入哪些。来源保存在 `configs` 中（类型 `library_source`），检出在 `~/.janus/library/sources/<id>/repo/`。

- **链接格式**（`parse_github`）：`https://github.com/o/r`、`github.com/o/r`、`git@github.com:o/r.git`、`o/r`，以及 `…/tree/<ref>/<目录>` 和 `…/blob/<ref>/<目录>/SKILL.md`。`tree/` 后的第一段视为分支或标签。其他网站的链接直接拒绝。仓库、ref 和目录都相同的来源不能重复添加。
- **拉取**：`git clone --depth 1 --no-tags`（指定了 ref 时加 `--branch`），带 `GIT_TERMINAL_PROMPT=0`（私有仓库只能用本机已有的 git 凭据）和 `GIT_LFS_SKIP_SMUDGE=1`，180 秒超时。克隆成功后才替换旧的检出。没有找到任何 Skill 或服务器时，新来源不保留。
- **扫描**（`offers`）：从链接指定的目录开始，从所选目录向下最多 8 层，最多收集 500 个候选路径（Skill 目录或 MCP 清单文件）。有更深的子目录或额外候选路径时，预览说明截断原因，可改选更具体的子目录。添加 Git 来源时，如果没有找到可导入项，错误信息也保留扫描警告。含普通文件 `SKILL.md` 的目录是 Skill，找到后不再进入它的子目录；`mcp.json` 和 `.mcp.json` 中的每个服务器是一项，路径记为 `<文件>#<服务器名>`。跳过 `.git`、`node_modules` 和符号链接；链接指定的目录解析符号链接后必须仍在克隆目录内。
- **导入**：按路径选择。Skill 存进内容存储，服务器连同 manifest 中写的值一起保存。
- **检查更新**（`check_sources`）：服务启动 30 秒后检查一次，之后每 6 小时一次（`CHECK_EVERY`），也可以在应用中点「检查更新」。对每个来源执行 `git ls-remote`，不下载内容：没有指定 ref 时看 `HEAD`，否则依次找同名分支、同名标签（附注标签取它指向的提交）。远端提交（`latest`）与检出的提交（`commit`）不同时显示「有更新」；检查失败时记下错误并清空 `latest`，界面显示错误而不是旧的「有更新」。
- **更新**（`update_source`）：重新克隆、重新扫描，从这个来源导入的每项资源换成仓库中的新内容，内容变了才生成新版本。MCP 服务器保留已保存的值，只为还没有值的变量补上 manifest 中的值。仓库中已经没有的资源保持原样，在结果中列出，资源库中标为「来源中已删除」。
- **删除**：资源库中还有来自这个来源的资源时不能删除。

<a id="roles"></a>
## 角色的选择

角色的 `resources` 是资源 id 的列表，在角色编辑器中按类型勾选。

- 保存角色时只保留资源库中存在的 id，并去掉重复。
- 每项资源在每种运行时下的状态（`support`）：

| 资源 | Claude Code | Codex | 自定义命令 |
| --- | --- | --- | --- |
| Skill | 可以使用 | 可以使用 | 不支持：没有在启动时加载 Skill 的入口 |
| stdio 服务器 | 可以使用 | 可以使用 | 可以使用 |
| http 服务器 | 可以使用 | 可以使用 | 取决于 Agent 是否声明 HTTP MCP 能力 |
| sse 服务器 | 可以使用 | 不支持 | 取决于 Agent 是否声明 SSE MCP 能力 |

<a id="delivery"></a>
## Run 和会话怎样拿到资源

1. **固定**：创建 Run 时（`team_for`），每个成员的 `resources` 记下资源当时的完整记录（名字、版本、MCP 定义，不含值）。之后资源库的修改不影响这个 Run，从中断处继续时也用这份记录。
2. **资源包**：同时（`bundle_team`），成员的 Skill 从内容存储复制到 `~/.janus/runs/<run>/members/<成员 id>/bundle/`，按运行时摆放：Claude Code 是一个本地插件（`.claude-plugin/plugin.json` 和 `skills/<名字>/`），Codex 是 `.agents/skills/<名字>/`。自定义命令的成员没有资源包。复制的是普通文件，Agent 改了它也不影响资源库和其他成员。
3. **打开会话**：`App::opening` 用 `library::launch` 算出会话参数，`session/new` 和 `session/load` 用同一组：
   - Claude Code：`_meta.claudeCode.options.plugins` 指向资源包，Skill 在会话中显示为 `janus:<名字>`。
   - Codex：`_meta.additionalRoots` 指向资源包，Codex 读取其中的 `.agents/skills`，Skill 显示为 `$<名字>`。这个目录同时扩大了会话的文件访问范围。
   - MCP 服务器放进 `mcpServers`。值在这时从 `secrets/library/` 读取，只传服务器声明的、非空的变量；Agent 没有声明对应传输能力的服务器不放进去。看板 MCP 排在最后。
4. **记录**：每项资源怎样到达会话（`plugin`、`additionalRoots`、`mcpServers`，或 `skipped` 及原因）写进 `agent.status`（负责人）和 `attempt.started`（协作者）的 `data.config.resources`，见[事件](../interfaces/20261031-events.md)。

Janus 不把 Skill 或 MCP 列进提示，也不列工作目录中的项目 Skill：Claude Code 和 Codex 按自己的规则读取工作目录中的 `.claude/skills`、`.agents/skills`。
