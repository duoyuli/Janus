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
| `reach` | 给谁用：`everyone`（所有数字员工）或 `picked`（只给选中它的数字员工），见[给谁用](#reach) |
| `description` | 说明。Skill 取 `SKILL.md` 的描述（列表中截到 240 个字符） |
| `origin` | 来源：`created`（在 Janus 中新建或粘贴的配置）、`folder`（从本机目录复制，记下目录）、`git`（从 Git 来源导入的 Skill，记下来源 id 和路径） |
| `revision` | 内容的 sha256：Skill 是全部文件，MCP 是服务器定义 |
| `server` | 只有 MCP 有：传输方式和启动方式，见 [MCP 服务器](#mcp) |

规则：

- 所有修改依次执行（`LibraryState` 的锁），页面操作和后台检查不会同时改资源库。
- 新建和编辑时名字被占用会报错；导入时名字被占用自动加 `-2`、`-3`。
- 删除资源时，选了它的角色一并去掉它（给所有人的资源也可能还留着收口时的选择，编辑器里看不到），应用在确认框里列出这些数字员工；各 Run 里正在运行的这个服务器先停掉，再删除它的隔离目录。已经开始的 Run 有自己的副本，删除资源不影响它们；但删除后 MCP 的值也被删除，旧 Run 恢复时拿不到。

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

Git 来源是一个 GitHub 仓库或仓库中的一个目录。添加链接后，Janus 列出其中的 Skill，由用户选择导入哪些。MCP 服务器的配置因人因环境而异，不从仓库导入，只在资源库里填写表单或粘贴配置添加；以前从来源导入的 MCP 服务器留在资源库中，更新来源时列为「来源中已删除」。来源保存在 `configs` 中（类型 `library_source`），检出在 `~/.janus/library/sources/<id>/repo/`。

- **链接格式**（`parse_github`）：`https://github.com/o/r`、`github.com/o/r`、`git@github.com:o/r.git`、`o/r`，以及 `…/tree/<ref>/<目录>` 和 `…/blob/<ref>/<目录>/SKILL.md`。`tree/` 后的第一段视为分支或标签。其他网站的链接直接拒绝。仓库、ref 和目录都相同的来源不能重复添加。
- **拉取**：`git clone --depth 1 --no-tags`（指定了 ref 时加 `--branch`），带 `GIT_TERMINAL_PROMPT=0`（私有仓库只能用本机已有的 git 凭据）和 `GIT_LFS_SKIP_SMUDGE=1`，180 秒超时。克隆成功后才替换旧的检出。没有找到任何 Skill 或服务器时，新来源不保留。
- **扫描**（`offers`）：从链接指定的目录开始，从所选目录向下最多 8 层，最多收集 500 个候选 Skill 目录。有更深的子目录或额外候选路径时，预览说明截断原因，可改选更具体的子目录。添加 Git 来源时，如果没有找到可导入项，错误信息也保留扫描警告。含普通文件 `SKILL.md` 的目录是 Skill，找到后不再进入它的子目录。跳过 `.git`、`node_modules` 和符号链接；链接指定的目录解析符号链接后必须仍在克隆目录内。
- **导入**：按路径选择，Skill 存进内容存储。以前扫描留下的 MCP 候选不能再导入。
- **检查更新**（`check_sources`）：服务启动 30 秒后检查一次，之后每 6 小时一次（`CHECK_EVERY`），也可以在应用中点「检查更新」。对每个来源执行 `git ls-remote`，不下载内容：没有指定 ref 时看 `HEAD`，否则依次找同名分支、同名标签（附注标签取它指向的提交）。远端提交（`latest`）与检出的提交（`commit`）不同时显示「有更新」；检查失败时记下错误并清空 `latest`，界面显示错误而不是旧的「有更新」。
- **更新**（`update_source`）：重新克隆、重新扫描，从这个来源导入的每个 Skill 换成仓库中的新内容，内容变了才生成新版本。仓库中已经没有的资源保持原样，在结果中列出，资源库中标为「来源中已删除」。
- **删除**：资源库中还有来自这个来源的资源时不能删除。

<a id="reach"></a>
## 给谁用

每项资源有一个 `reach`（`library::Reach`）：

- `everyone`：给所有数字员工。创建 Run 时，每个成员的资源快照都包含资源库里全部 `everyone` 的资源；会话经 [MCP 路由](#mcp_router)和 [Skill 路由](#skill_router)拿到它们。
- `picked`：收口，只给角色选中它的数字员工，照旧逐项注入。

新资源的默认值按类型（`Reach::default_for`）：Skill 是 `everyone`（说明文字和脚本，影响 Agent 怎么做事），MCP 服务器是 `picked`（能直接对外动手的工具，带凭据、有副作用），用户在资源库里放开（`POST /api/library/items/{id}/reach`）。数据版本 5 按同样的默认值给已有的资源和 Run 快照中的资源填上这个字段。设计取舍见 [ADR：资源库默认给所有数字员工](../decisions/20261119-library_routing.md)。

<a id="roles"></a>
## 角色的选择

角色的 `resources` 是资源 id 的列表；它只对收口（`picked`）的资源有意义，`everyone` 的资源每个员工本来就有。同一个选择有两个入口，改的是同一份数据：

- **数字员工编辑器**的「能用的资源」：先写一行给所有数字员工的资源有几个（自动就有，可展开看名字和这个 Agent 用不了的原因），下面直接列出所有收口资源的勾选框。新建数字员工时一步选好。
- **资源库**里收口资源的那一行写「只给 A、B」，展开后可以勾选数字员工（`POST /api/library/items/{id}/roles`，`set_item_roles`）：列出的角色选中它，其他角色去掉它。

编辑器只有动过勾选时才在保存时带上 `resources`；没带（或为 `null`）时保留角色现有的选择，所以资源库那边的修改不会被打开着的编辑器用旧列表覆盖。

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

1. **固定**：创建 Run 时（`team_for`），每个成员的 `resources` 记下资源当时的完整记录（名字、版本、`reach`、MCP 定义，不含值）：资源库里全部 `everyone` 的资源，加上角色选中的 `picked` 资源。之后资源库的修改不影响这个 Run，从中断处继续时也用这份记录。
2. **资源包**：同时（`bundle_team`），成员的 Skill 从内容存储复制到 `~/.janus/runs/<run>/members/<成员 id>/bundle/`，按运行时摆放：Claude Code 是一个本地插件（`.claude-plugin/plugin.json` 和 `skills/<名字>/`），Codex 是 `.agents/skills/<名字>/`。`everyone` 的 Skill 超过 10 个时改由 [Skill 路由](#skill_router)摆放。自定义命令的成员没有资源包。复制的是普通文件，Agent 改了它也不影响资源库和其他成员。
3. **打开会话**：`App::opening` 用 `library::launch` 算出会话参数，`session/new` 和 `session/load` 用同一组：
   - Claude Code：`_meta.claudeCode.options.plugins` 指向资源包，Skill 在会话中显示为 `janus:<名字>`。
   - Codex：`_meta.additionalRoots` 指向资源包，Codex 读取其中的 `.agents/skills`，Skill 显示为 `$<名字>`。这个目录同时扩大了会话的文件访问范围。
   - `everyone` 的 stdio 服务器不逐个放进 `mcpServers`，而是由这个 Run 的 [MCP 路由](#mcp_router)提供，会话只拿到一个 `janus-tools`；Agent 没有声明 HTTP MCP 能力时没有路由，改为逐个放进去。其他 MCP 服务器（`picked` 的，以及 http、sse 的）放进 `mcpServers`。值在这时从 `secrets/library/` 读取，只传服务器声明的、非空的变量；Agent 没有声明对应传输能力的服务器不放进去。看板 MCP 排在最后。
4. **记录**：每项资源怎样到达会话（`plugin`、`additionalRoots`、`skillRouter`、`mcpServers`、`mcpRouter`，或 `skipped` 及原因）写进 `agent.status`（负责人）和 `attempt.started`（协作者）的 `data.config.resources`，见[事件](../interfaces/20261031-events.md)。

Janus 不把 Skill 或 MCP 列进提示，也不列工作目录中的项目 Skill：Claude Code 和 Codex 按自己的规则读取工作目录中的 `.claude/skills`、`.agents/skills`。负责人说明只写一句：资源库对它和每个协作者开放，派出子任务后只用来查看。

<a id="mcp_router"></a>
## MCP 路由

每个 Run 有一个 MCP 端点 `janus-tools`（`/mcp/{run}/{token}/{who}/tools`，令牌和 `who` 的规则与[看板](../interfaces/20261031-board_mcp.md#transport)相同，`orchestrator::mcp_router`），提供成员快照中 `everyone` 的 stdio 服务器：

- **每个会话一份**：负责人和每个子任务（`who`）各有自己的服务器进程，和服务器直接交给 Agent 时一样，浏览器、终端、数据库连接这类有状态的服务器不会在会话之间串状态。同一个会话的几个请求同时到达时只启动一次，不同服务器同时启动。
- **预先准备**：在资源库里新建、修改或粘贴导入命令启动的 MCP 服务器后，Janus 在后台用它的隔离目录完整启动一次、列出工具、再停掉（`warm_mcp`，最多等 10 分钟，日志在 `~/.janus/logs/library/mcp-<名字>.log`）。`npx -y`、`uvx` 下载的包落在隔离目录里，之后各 Run 启动时直接用，能在 Agent 给的时间内启动；启动不了的服务器（命令写错、缺少值）当场在资源库里显示原因，不用等到 Run 里才发现。结果只在服务运行期间记着，按资源的 `revision` 对应（修改后旧结果作废）；服务重启后不重新准备，隔离目录里的包还在。资源库可以「重新准备」（`POST /api/library/mcp/{id}/warm`）。
- **启动**：会话打开时（`session/new` 或 `session/load` 之前）就在后台启动这个会话的服务器（`prewarm`），第一次下载包（`npx -y`）也有时间。命令和参数来自资源，环境和 Agent 一样是隔离的（`sandbox_at`，见[安全边界](../operations/20261031-security.md#agent_isolation)）：`HOME`、XDG 目录和 `TMPDIR` 在 `~/.janus/library/servers/<资源 id>/` 下，跨 Run 保留（`npx` 缓存、服务器自己的登录状态都在这里）；从宿主只继承 `PATH`、语言等少数变量，再加上资源保存的值，值不能覆盖这些隔离变量。工作目录是这个 Run 的工作目录（Git 仓库上是它的 worktree），标准错误写进 `~/.janus/logs/<run>/mcp-<who>-<名字>.log`。服务器向 Janus 发的 `roots/list` 回答这个工作目录，`ping` 照常回答，其他请求回答不支持。启动最多等 120 秒。
- **列工具**：`tools/list` 向这个会话的每个服务器要当前的工具列表，最多等 8 秒：Agent 一般只给 MCP 服务器十秒左右启动和列工具，一个慢的服务器不能让整个 `janus-tools` 失效。到时还没就绪的服务器这次不列出，继续在后台启动（原因写进服务日志）。预先准备过的服务器包已经下载好，一般在时限内就绪；没准备过的（例如还没准备完就开始了 Run）第一次可能赶不上。这个会话已经列过工具，之后就绪的工具它看不到，路由不向 Agent 推送列表变化。列表只在服务器进程运行期间记住，进程重新启动、或服务器发来 `notifications/tools/list_changed` 时重新要。Claude Code 和 Codex 自己按需搜索 MCP 工具，所以路由不另做工具搜索。`initialize` 的说明列出背后的服务器和它们的描述。
- **工具名**：`<服务器>__<工具>`。Agent 还会在前面加 `mcp__janus-tools__`，而模型的工具名最多 64 个字符，所以路由的名字最多 46 个字符；超长、或含有字母、数字、`-`、`_` 以外字符的名字，截短后带上原名的哈希，两个工具不会同名，每次 Run 也一样。调用时按完整名字在服务器当前的工具里查找。
- **生命周期**：服务器跟着会话，和交给 Agent 时一样活到会话结束：会话关闭（子任务结束、负责人的会话关掉）时结束这个会话的服务器；会话还在而服务器自己退出了，下次用到时重新启动。Run 停止、取消、失败或中断时，立刻结束它的所有服务器，正在执行的调用也一起结束，之后不再启动；交付后负责人的对话还能继续，所以交付时不停。修改资源（命令、值）时，各 Run 里正在运行的这个服务器停掉，下次用到时按新的设置启动；删除资源时同样先停掉。服务器在自己的进程组里启动，结束时先 TERM 整个进程组，3 秒后 KILL，`npx`、`uvx` 派生的子进程一起结束。
- **记录**：每次调用写一条 `mcp.routed`（调用者、子任务、服务器、工具、耗时、错误），不记参数，见[事件](../interfaces/20261031-events.md)。找不到的工具和服务器报的错作为工具错误（`isError`）返回给 Agent。负责人派出子任务后每调用一个新的工具，另记一条 `lead.routed`，看板上推给负责人，交付时列为风险，见[负责人的职责](20261031-goal_mode.md#lead_role)。
- **已知限制**：经 MCP 工具写的文件不在 Agent 上报的编辑里，文件认领、越界提醒和交付痕迹核对看不到它们；验证子任务也能用所有给所有人的 MCP，「只检查不改文件」靠指令和 Agent 的权限模式保证（权限不另加开关，见 [ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)）。同一个资源在几个会话里同时运行时共用一个隔离目录，有本地状态文件的服务器可能互相影响。
- 名字 `janus` 和 `janus-tools` 留给 Janus，资源库里的 MCP 服务器不能用。

<a id="skill_router"></a>
## Skill 路由

成员快照中 `everyone` 的 Skill 不超过 10 个（`SKILL_ROUTE_OVER`）时，照常每个放进资源包、直接加载。超过 10 个时，它们放在资源包的 `janus-skills/.skills/<名字>/` 下（隐藏目录，Agent 不会逐个加载），另外生成一个 Skill `janus-skills`：`SKILL.md` 的描述列出这些 Skill 的名字（不超过 1000 个字节，Codex 拒绝更长的描述），正文是每个 Skill 的完整描述和它的 `SKILL.md` 路径。Agent 判断任务用得上时，再读对应的 `SKILL.md`。这样提示词里只有一条描述。`picked` 的 Skill 不进路由，照常直接加载。名字 `janus-skills` 留给 Janus，资源库里的 Skill 不能用。

Skill 路由让 Agent 读取 Janus 目录下资源包里的文件：默认的权限模式（Codex 的 Full access、Claude Code 的 Bypass permissions）不需要确认，更严格的模式会询问。
