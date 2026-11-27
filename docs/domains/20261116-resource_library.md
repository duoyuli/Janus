# 资源库

本文说明资源库的规则：资源有哪些、从哪里来、怎样更新，Run 怎样固定资源，会话怎样经两个路由拿到它们。资源库里的每项资源都给所有数字员工，角色不选资源。设计取舍见 [ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](../decisions/20261116-launch_time_resources.md) 和 [ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](../decisions/20261120-library_routed_only.md)，接口见 [HTTP API](../interfaces/20261031-http_api.md#library)，凭据和第三方内容的边界见[安全边界](../operations/20261031-security.md#library)。

实现在 `backend/src/library.rs` 和 `backend/src/library/`。`offers` 扫描可导入的资源，`store` 和 `skill` 管理 Skill 内容及 `SKILL.md`，`mcp` 管理服务器定义和值，`sources` 管理 Git 来源，`deliver` 生成资源包和会话参数。MCP 路由在 `backend/src/orchestrator/mcp_router.rs`（自我介绍、找工具和代码模式分别在 `mcp_router/say.rs`、`find.rs`、`code.rs`），它和服务器之间的连接在 `mcp_client.rs`，代码模式的脚本运行器在 `backend/src/coderun.rs`。

## 导航

- [资源](#model)：了解资源库有哪些资源时阅读。
- [Skill](#skills)：修改 Skill 的导入和保存时阅读。
- [MCP 服务器](#mcp)：修改 MCP 服务器的字段和准备时阅读。
- [Git 来源](#sources)：修改从 GitHub 拉取和更新时阅读。
- [精选库](#catalog)：修改推荐列表时阅读。
- [Run 和会话怎样拿到资源](#delivery)：修改资源在 Run 开始时怎样固定和交给会话时阅读。
- [MCP 路由](#mcp_router)：修改 `janus-tools` 路由时阅读。
- [Skill 路由](#skill_router)：修改 `janus-skills` 路由时阅读。
- [Skill 的使用记录](#skill_uses)：修改 Agent 读 Skill 的识别和计数时阅读。
- [Skill 的改进提示](#skill_hints)：修改资源库对 Skill 描述的检查时阅读。

<a id="model"></a>
## 资源

资源库中的每一项是一个资源（`Item`），保存在 SQLite 的 `configs` 中（类型 `library_item`）：

| 字段 | 说明 |
| --- | --- |
| `id` | 不带含义的 id（`i` 加时间），不从它读出类型或来源 |
| `kind` | `skill` 或 `mcp` |
| `name` | Agent 看到的名字。同一类资源中唯一 |
| `description` | 说明。Skill 取 `SKILL.md` 的描述（列表中截到 240 个字符） |
| `origin` | 来源：`created`（在 Janus 中新建或粘贴的配置）、`folder`（从本机目录复制，记下目录）、`git`（从 Git 来源导入的 Skill，记下来源 id 和路径） |
| `revision` | 内容的 sha256：Skill 是全部文件，MCP 是服务器定义 |
| `server` | 只有 MCP 有：传输方式和启动方式，见 [MCP 服务器](#mcp) |

规则：

- 所有修改依次执行（`LibraryState` 的锁），页面操作和后台检查不会同时改资源库。
- 新建和编辑时名字被占用会报错；导入时名字被占用自动加 `-2`、`-3`。
- 删除资源时，各 Run 里正在运行的这个服务器先停掉，再删除它的隔离目录和记下的工具列表。已经开始的 Run 有自己的副本，删除资源不影响它们；但删除后 MCP 的值也被删除，旧 Run 恢复时拿不到。

<a id="skills"></a>
## Skill

Skill 是一个带 `SKILL.md` 的目录，名称和描述来自 front matter。

- **名字**：小写字母、数字和 `-`，最长 64 个字符。导入时由 front matter 的 `name`（没有时取目录名）转换而来，转换后为空时用 `skill`。交给 Agent 的 `SKILL.md` 中，front matter 的 `name` 改成资源库中的名字；缺少描述时补上资源的描述。
- **新建**：在编辑器中填写名字、描述和正文；也可以粘贴完整的 `SKILL.md`，这时描述以文件中的为准。没有描述时拒绝保存，因为 Agent 按描述判断何时使用 Skill。只有新建的 Skill 可以在 Janus 中编辑。
- **带代码的 Skill**：`SKILL.md` 旁边有 `code.js`（默认导出一个接收 args 对象的 async 函数）的 Skill，MCP 路由的 `run_code` 可以用 `skills.<名字>(args)` 调用。Agent 用 `run_code` 的 `save_as` 存下的脚本就是这种 Skill，来源记为在 Janus 中新建；编辑它时 `code.js` 保留。
- 选择「从文件夹复制…」后，可以选单个 Skill 目录或包含多个 Skill 的父目录。Janus 使用与 [Git 来源](#sources) 相同的目录扫描规则，最多列出 500 个 Skill。列表默认勾选所有尚未导入的条目，可以逐项调整或全不选，再点「导入所选」一次复制。
- 本机导入按每个 Skill 的原目录记录来源，已复制的条目显示「已导入」。复制失败的条目会显示原因，可以单独重试。内容以复制时的版本为准，使用「刷新」可以重新复制原目录。
- **存放**：按内容的 sha256 放在 `~/.janus/data/library/skills/<hash>/`，内容相同只存一份，存下后不再修改。复制时跳过隐藏文件（`.git`、`.env` 等）和 `node_modules`；符号链接只在指向目录内部时才读取；单个 Skill 最多 2000 个文件、20MB。没有资源使用的内容在每次修改后清理。

<a id="mcp"></a>
## MCP 服务器

服务器的定义保存在资源中，值保存在 `~/.janus/data/secrets/library/<id>.json`（目录 `0700`，文件 `0600`）。

| 传输方式 | 定义 | 值 |
| --- | --- | --- |
| `stdio` | `command`、`args`、`envKeys`（环境变量名） | 环境变量的值 |
| `http`（streamable HTTP） | `url`、`headerKeys`（HTTP header 名） | header 的值 |
| `sse` | `url`、`headerKeys` | header 的值 |

- **名字**：字母、数字、`-` 和 `_`，最长 64 个字符；规范化后是 `janus` 的名字（`janus`、`Janus`、`janus!` 等）保留给看板 MCP。
- **表单**：填写名字、传输方式、启动命令（`command` 和 `args` 分开提交；只给 `command` 时服务按 shell 规则拆分）或地址和变量列表（`vars`）。变量带字符串值时写入，空字符串清除这个值，没有 `value` 字段时保留已保存的值；从列表中删除的变量，它的值一起删除。应用的编辑器打开时读取已保存的值（`GET /api/library/mcp/{id}/values`），在表单和 JSON 里都能看到、修改；JSON 是同一个服务器的 Claude Code `.mcp.json` 写法，保存时仍按表单提交。
- **粘贴配置**（`POST /api/library/mcp/import`）：接受 Claude 的 `mcpServers`、VS Code 的 `servers`，或一个带 `name` 的服务器对象，一次导入其中所有服务器，配置中写的值存进值文件。用不了的条目作为警告返回。应用在新建服务器的 JSON 里粘贴了多个服务器时走这里。
- 资源、列表和详情接口、Run 快照和事件中只有变量名和「是否已设置」，没有值。值只经 `GET /api/library/mcp/{id}/values` 返回给本机的应用和配对的设备，见 [ADR：MCP 服务器的值在编辑器里能看能改](../decisions/20261119-mcp_values_visible.md)和 [ADR：MCP 的值和 Agent 的环境变量也给配对的设备](../decisions/20261126-credentials_to_paired_devices.md)。

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

<a id="catalog"></a>
## 精选库

应用的资源库在 Skill 和 MCP 两个标签的最下面各有一块「推荐」：社区常用、逐项核对过的 Skill 和 MCP 服务器，一键加入资源库。列表写在服务端的 `setup_view/picks.rs` 中（`GET /api/library/picks`，Mac 应用和 Web 客户端读同一份），随版本发布，显示时不访问网络；加入一项也由服务端做（`POST /api/library/picks/add`：Skill 经这个仓库的来源导入，没有来源时先加上；不需要填值的服务器直接加入，需要 API key 的交回编辑器的草稿和要填的说明）；挑选标准和不锁版本的取舍见 [ADR：资源库的精选库随应用发布](../decisions/20261119-library_catalog.md)。

- **挑选**（2026-11-26 和用户逐项定过，评审时的取舍见下）：Skill 18 个，第一个是 ego-browser（`citrolabs/ego-lite`，要先装 ego lite 浏览器），然后是文档（PDF、Word、Excel、PowerPoint、中文技术文档规范）、界面、写 Skill、写代码的方法和 Web 实践；MCP 服务器 8 个：Context7、DeepWiki、Exa、GitHub、Chrome DevTools、Hugging Face、Firecrawl、Tavily。不收：依赖和用户来回对话的 Skill（资源库的 Skill 给所有数字员工，协作者不能直接问用户；Grilling 是用户指定保留的例外）；和 Janus 自带能力重复的服务器（团队记忆之外的记忆、Agent 自带的抓网页和时间、模型自带的逐步推理、Janus 自己管理的 worktree）；同一件事已有更好的一项时的第二个（浏览器操作用 ego-browser，不再收 Playwright；搜索留 Exa 和 Tavily）；只服务单一厂商的文档。
- **Skill**：每项是一个 GitHub 仓库里的 Skill 目录；SKILL.md 在仓库根目录的（例如 `unclebob/ub-mode`）目录为空，按仓库名认出资源库里已有的。添加时，资源库已经有整个仓库（没有指定目录和 ref）的 [Git 来源](#sources)就直接从中导入，否则先添加这个仓库的来源再导入，和粘贴链接后勾选一样；来源的扫描结果里没有这个目录时报错，提示先更新来源。
- **MCP 服务器**：每项是一个命令启动的服务器或 HTTP 服务器，带它需要的值（环境变量或 header、在哪里获取、值的格式，例如 `Bearer {}`）。没有必填的值时一键添加，等同于在编辑器里保存；有必填的值时打开预填好的编辑器，值写到用户要填的部分为止（`Bearer `）。
- **已添加**：资源库里已经有同一个仓库同一个目录的 Skill（或同名的 Skill），或者同名、同命令、同地址的 MCP 服务器时，这一项显示「已添加」，排在没添加的后面。
- 添加后和其他资源一样，给所有数字员工。

<a id="delivery"></a>
## Run 和会话怎样拿到资源

资源库里的每项资源都给所有数字员工：资源没有「给谁用」，角色也不选资源。会话只经两个路由拿到它们，`mcpServers` 和提示词不随资源库增长。Agent 自带的 Skill 和 MCP（例如 Codex 登录账号后自带的 `codex_apps`、Claude Code 的内置 Skill）由 Agent 自己决定，不在此列。

1. **固定**：创建 Run 时（`team_for`），每个成员的 `resources` 记下整个资源库当时的完整记录（名字、版本、MCP 定义，不含值），从中断处继续时也用这份记录。Skill 按这份记录复制进资源包，之后资源库的修改不影响这个 Run。MCP 服务器分两层：这份记录决定 Run 有哪些服务器，之后新加进资源库的不会进来；每个服务器怎样运行看资源库当前的设置，改了命令、地址或值，在它下次启动时生效（正在运行的实例在保存时就停下）；从资源库删除的服务器在已开始的 Run 里也一起停用，不再列出它的工具，也不再启动（`routed_servers`）。
2. **资源包**：同时（`bundle_team`），成员的 Skill 从内容存储复制到 `~/.janus/data/runs/<run>/bundle/<成员 id>/`（对话在 `data/chats/<run>/bundle/` 下）中 [Skill 路由](#skill_router)的位置：Claude Code 是一个本地插件（`.claude-plugin/plugin.json` 和 `skills/janus-skills/`），Codex 是 `.agents/skills/janus-skills/`。复制的是普通文件，Agent 改了它也不影响资源库和其他成员。同时生成 `janus-skills` 的目录，排序用到这时为止的[使用记录](#skill_uses)。资源库里没有 Skill 时没有资源包。
3. **打开会话**：`App::opening` 用 `library::launch` 算出会话参数，`session/new` 和 `session/load` 用同一组：
   - Claude Code：`_meta.claudeCode.options.plugins` 指向资源包，会话中显示为 `janus:janus-skills`。
   - Codex：`_meta.additionalRoots` 指向资源包，Codex 读取其中的 `.agents/skills`，显示为 `$janus-skills`。这个目录同时扩大了会话的文件访问范围。
   - MCP 服务器不论传输方式，都不放进 `mcpServers`，由这个 Run 的 [MCP 路由](#mcp_router)提供：资源库里有服务器时，会话拿到一个 `janus-tools`，看板 MCP 排在它后面。服务器的值不交给 Agent。Agent 没有声明 HTTP MCP 能力时拿不到路由，也就拿不到资源库的服务器（记为 `skipped`）。
4. **记录**：每项资源怎样到达会话（`skillRouter`、`mcpRouter`，或 `skipped` 及原因）写进 `agent.status`（负责人）和 `attempt.started`（协作者）的 `data.config.resources`，见[事件](../interfaces/20261031-events.md)。

Janus 不把单个 Skill 或 MCP 服务器列进提示，也不列工作目录中的项目 Skill：Claude Code 和 Codex 按自己的规则读取工作目录中的 `.claude/skills`、`.agents/skills`。系统层只在负责人说明里写一句「资源库（`janus-tools` 和 `janus-skills`）对它和每个协作者开放，派出子任务后只用来查看」，不写 `janus-skills` 在哪里、怎么用：Agent 从 `session/new` 交给它的资源包按自己的方式加载 `janus-skills`，它的描述写明每项任务开始时先读，见 [Skill 路由](#skill_router)。

<a id="mcp_router"></a>
## MCP 路由

每个 Run 有一个 MCP 端点 `janus-tools`（`/mcp/{run}/{token}/lead/tools` 或 `/mcp/{run}/{token}/task/{task}/tools`，令牌和席位的规则与[看板](../interfaces/20261031-board_mcp.md#transport)相同，`orchestrator::mcp_router`），提供这个会话所属成员快照中的全部 MCP 服务器，命令启动的、HTTP 的和 SSE 的都在内。负责人的会话用负责人的快照，子任务的会话用它所派给的成员的快照。做法照 [magpie](https://github.com/yetone/magpie) 的 MCP 路由，设计取舍见 [ADR：MCP 路由照 magpie 只给三个工具](../decisions/20261126-mcp_router_three_tools.md) 和 [ADR：MCP 路由的代码模式](../decisions/20261126-run_code.md)：

- **只给三个工具**：`tools/list` 固定给 `find_tools`、`call_tool`，代码模式开着、背后有工具时再加 `run_code`，不论背后有多少服务器和工具，对 Claude Code 和 Codex 都一样。服务器自己的工具不列出，Agent 用 `find_tools` 找到，再用 `call_tool` 或 `run_code` 调用。
- **介绍自己**（`say`）：`initialize` 的 `instructions` 照 magpie 的写法，总长不超过 2000 个字符（Claude Code 保留 4096 个字符的一半）。第一行列出每个服务器和它做什么（`Janus 资源库的 MCP 服务器：exa（网页搜索）、github（…）。`，不超过 250 个字符：先保住名字，放不下的计为「另有 N 个」，剩下的空间分给用途，短的整句、长的平分后在词边界截断）；接着每个服务器一行，写标题（和名字不同时）、用途和工具，工具带说明的第一个分句，放不下时只写工具名，再放不下写「另有 N 个」，空间先给短的行；最后一句说怎样用 `find_tools`、`call_tool`，代码模式开着时加一句 `run_code`。用途优先取资源库的描述，其次取服务器在 `initialize` 里说的：`serverInfo.description`，没有时取它 `instructions` 的第一行（最多 300 个字符），标题取 `serverInfo.title`（最多 80 个字符）。下游 `instructions` 的其余部分不转交。从来没有介绍过自己的服务器，`initialize` 最多等它 5 秒。用语按 Run 的语言。
- **`find_tools {query, server?}`**（`find`）：把查询切成关键词：英文按非字母数字切开、去掉 a、the、tool、please 这类停用词和单个字母；中日韩文本没有空格，在换文字处切开（`截图screenshot` 是 `截图` 和 `screenshot`），再在「我、你、的、了、个、帮、把、给、这、一」和日文助词处切开（「帮我截个图」是「截」和「图」）。每个工具按路由名、服务器名和说明匹配：英文词相同，或四个字母以上的词互为前缀（`repo` 和 `repository`、`issues` 和 `issue`）；中日韩的关键词一个字时出现即算，更长时至少一半的相邻两字出现在文中即算（「发送飞书消息」里的「发送」「飞书」「消息」）；不是 ASCII 的关键词只对含有非 ASCII 字母的工具计数，所以「截图 screenshot」对英文工具就是 screenshot。查询就是工具名时直接命中；否则至少命中三分之二的关键词，并且其中一个在名字里（全部命中时不要求）。按名字里的词计 3 分、说明里的计 1 分排序，返回最相符的 5 个的名字、说明和 `inputSchema`，再列最多 15 个名字和说明的第一个分句。`query` 为空时列出全部工具；`server` 只找这个服务器的；没有命中时列出每个服务器的工具数和工具名（太长时每个服务器只列前 8 个，并说明空查询能列出全部），说明不要调用只是名字相近的工具，查询有中文而工具都是英文时提示用英文再找。`find_tools` 的描述也带上第一行和每个服务器的一行（最多 1200 个字符），给不展示 `instructions` 的 Agent 看。
- **`call_tool {name, arguments}`**：按名字找到工具后照常调用：先按路由名，再按服务器内的同名工具（不分大小写，只有一个时），最后按 Agent 自己加了前缀的名字（逐词对上路由名的末尾，`mcp__janus-tools__github__search` 是 `github__search`）。`arguments` 收到 JSON 字符串时先解析成对象。找不到时作为工具错误返回，列出名字里有相同词的工具（最多 5 个）。直接用 `tools/call` 调用路由名也照样执行，找不到时返回协议错误 `-32602`。
- **`run_code {code, args?, save_as?}`**（`code`，代码模式）：`code` 是一个 async 函数的函数体，在单独的进程里运行（`crate::coderun`，Janus 自己以 `janus run-code-helper` 启动，QuickJS，环境变量清空，没有文件、网络、子进程、定时器、`require` 和 `import`）。`tools.<路由名>(args)`（`-` 也可以写成 `_`，列表里没有的名字也照样去问路由）调用工具，和 `call_tool` 走同一条路、各记一条 `mcp.routed`；工具交回 `structuredContent`，没有时交回文本（是 JSON 对象或数组时解析好），图片等非文本内容交回 `content`；工具出错时抛出。`args` 是脚本里的 `args`。结果是 `console.log` 等打印的内容加上 `return` 的值（字符串原样、MCP 内容块原样、其他写成 JSON），各截到 40000 字节并说明还剩多少；没返回也没打印、Promise 一直不结束、语法错误、抛出异常和超限都作为工具错误返回。限制：最多 100 次工具调用，同时最多 8 个，总共 50 秒（父进程再等 2 秒后结束辅助进程），内存 512 MB。Agent 取消请求时脚本和进行中的调用一起停止。成员快照里带 `code.js` 的 Skill（最多 256 KB，见 [Skill 路由](#skill_router)）在脚本里是 `skills.<名字>(args)`，`run_code` 的描述列出最多 30 个；`save_as {name, description}` 在脚本没有出错时把它存成在 Janus 中新建的 Skill（`SKILL.md` 写明怎样经 `run_code` 调用，`code.js` 是导出这段脚本的 async 函数），同名的拒绝保存，结果末尾说明存没存下；存下后全局事件流发 `library`，从下一个 Run 起每个数字员工都能调用。代码模式由资源库 MCP 标签的开关控制（`App::code_mode`，`PUT /api/library/router`，默认开），关掉后只给两个工具。
- **用到才启动，同一个 Run 共用**：会话打开时不启动任何服务器；第一次调用某个服务器的工具时才启动它。同一个 Run 的负责人和各子任务共用这一个进程，MCP 的请求按 id 区分，可以同时处理；进程数最多是「Run 数 × 实际用到的服务器数」。同一个服务器的几个请求同时到达时只启动一次，不同服务器同时启动。
- **预先准备**：在资源库里新建、修改或粘贴导入 MCP 服务器后，Janus 在后台准备一次（`warm_mcp`，最多等 10 分钟）：命令启动的服务器用它的隔离目录完整启动一次、列出工具、记下名片、再停掉（日志在 `~/.janus/logs/library/<资源 id>.log`，工作目录是 `~/.janus/tmp/servers/<资源 id>/warm`）；HTTP、SSE 服务器连接一次、列出工具、记下名片，地址写错、值不对（例如服务器回答 401）当场显示。`npx -y`、`uvx` 下载的包落在隔离目录里，之后各 Run 启动时直接用；启动不了的服务器（命令写错、缺少值）当场在资源库里显示原因。准备结果只在服务运行期间记着，按资源的 `revision` 对应（修改后旧结果作废）。资源库可以「重新准备」（`POST /api/library/mcp/{id}/warm`）。
- **启动**：命令和参数来自资源，环境和 Agent 一样是隔离的（`sandbox_vars`，见[安全边界](../operations/20261031-security.md#agent_isolation)）：`HOME` 和 XDG 目录在 `~/.janus/data/library/servers/<资源 id>/`，跨 Run 保留（`npx` 缓存、服务器自己的登录状态都在这里），`TMPDIR` 在 `~/.janus/tmp/servers/<资源 id>/`；从宿主只继承 `PATH`、语言等少数变量，再加上资源保存的值，值不能覆盖这些隔离变量。工作目录是这个 Run 的工作目录（Git 仓库上是它的 worktree），标准错误写进 `~/.janus/logs/runs/<run>/mcp-<名字>.log`（对话在 `logs/chats/<run>/`）。服务器向 Janus 发的 `roots/list` 回答这个工作目录，`ping` 照常回答，其他请求回答不支持。启动最多等 120 秒。
- **协议版本**：Janus 支持 MCP 的 `2025-11-25`、`2025-06-18`、`2025-03-26` 和 `2024-11-05`（`mcp_client::VERSIONS`），都有 `initialize`，`tools/list` 和 `tools/call` 相同；去掉了 `initialize` 和会话的无状态版本 `2026-07-28` 不在其中。连接资源库的服务器时请求最新的版本，服务器回答了不在其中的版本时不用它，预先准备时在资源库里显示原因。Agent 连路由（和看板）时，请求的版本 Janus 支持就用它，否则回答 Janus 最新的版本，由 Agent 决定是否继续。
- **连接 URL 服务器**：HTTP（Streamable HTTP）和 SSE 服务器由 Janus 进程连接（`mcp_client`，`reqwest`，TLS 用系统证书库，遵循系统代理，连接最多等 15 秒），每个请求带上资源保存的 header，值在连接时从 `data/secrets/library/` 读取。Streamable HTTP 的每条消息是一个 POST，回应是 JSON 或事件流；服务器在 `initialize` 时给出的 `Mcp-Session-Id` 和协商的协议版本（`MCP-Protocol-Version`）随之后的每条消息发送，服务器不再认这个会话（404）时，这次调用报错，下次调用重新连接。SSE 服务器先打开事件流，等它给出发消息的地址，这个地址必须和服务器同源，否则拒绝连接（header 会随每条消息发送）；之后的回复从事件流到达，事件流断开时，下次调用重新连接。URL 服务器同一个 Run 共用一个连接。
- **名片：描述和找工具不启动服务器**：每个服务器的名片是它在 `initialize` 里说的标题和用途，加上它列出的工具，存在 `~/.janus/cache/mcp-tools/<资源 id>/<revision>.json`（只留当前 revision，预先准备时和每次启动后写下；读不出的旧格式当作没有）。`initialize`、`tools/list` 和 `find_tools` 对每个服务器，先看这个 Run 的进程是否在运行或已连接，是的话用它当前的工具和介绍；否则用名片；从来没有名片的才启动一次去要，`initialize` 最多等 5 秒、其余最多等 8 秒，没就绪的这次不算进去、继续在后台启动。服务器分页列出时（`nextCursor`）读完所有页，最多 100 页，服务器原样还回收到的 cursor 时停止。只能以 task 方式调用的工具（`execution.taskSupport` 为 `required`）不算进去，也调用不到：路由不做 task 方式的调用。进程运行期间，服务器发来 `notifications/tools/list_changed` 时重新要列表。调用的工具不在名片里时（例如包更新后新增的），路由让各服务器现场列一次再查找。路由自己说给 Agent 的话（工具不存在、Run 已停止、不支持的方法、找工具的结果）按 Run 的语言写中文或英文。
- **工具名**：`find_tools` 给出、`call_tool` 和 `run_code` 接受的名字是 `<服务器>__<工具>`。Agent 直接调用时还会在前面加 `mcp__janus-tools__`，而模型的工具名最多 64 个字符，所以路由的名字最多 46 个字符；超长、或含有字母、数字、`-`、`_` 以外字符的名字，截短后带上原名的哈希，两个工具不会同名，每次 Run 也一样。不带哈希的名字在第一个 `__` 处分成服务器和工具，所以服务器名含 `__` 或以 `_` 结尾时也带哈希（否则服务器 `a_` 的工具 `b` 和服务器 `a` 的工具 `_b` 都是 `a___b`）。调用时按完整名字在服务器当前的工具里查找。
- **生命周期**：服务器闲置 10 分钟后停掉（每分钟检查一次），再用到时重新启动。会话关闭不影响它们。服务器自己退出了，下次用到时重新启动。Run 停止、取消、失败或中断时，立刻结束它的所有服务器，正在执行的调用也一起结束，之后不再启动；交付后负责人的对话还能继续，所以交付时不停。修改资源（命令、值）时，各 Run 里正在运行的这个服务器停掉，下次用到时按新的设置启动；删除资源时同样先停掉。命令启动的服务器在自己的进程组里启动，结束时先 TERM 整个进程组，3 秒后 KILL，`npx`、`uvx` 派生的子进程一起结束；Streamable HTTP 服务器结束时发一个带会话 id 的 DELETE，结束服务器上的会话；SSE 服务器结束时关闭事件流。
- **超时和取消**：一次调用最多等 50 秒，短于 Claude Code 和 Codex 给 MCP 调用的 60 秒。Agent 放弃一个调用时发来的 `notifications/cancelled`（按会话和请求 id 对应）让路由停止等待，`run_code` 的脚本和它进行中的调用一起停止；路由停止等待时，不论是超时还是 Agent 取消，都向服务器发 `notifications/cancelled`，让它停下手里的工作，而不是做完了没人要。服务器发来的进度通知不转发：Claude Code 和 Codex 收到进度都不延长超时。
- **记录**：每次真实的工具调用（经 `call_tool`、`run_code` 或直接 `tools/call`）写一条 `mcp.routed`（调用者、子任务、服务器、路由名、耗时、错误），不记参数，见[事件](../interfaces/20261031-events.md)；`find_tools` 不记。服务器报的错、启动失败、超时和取消作为工具错误（`isError`）返回给 Agent，模型可以据此改正。负责人派出子任务后每调用一个新的工具，另记一条 `lead.routed`，看板上推给负责人，交付时列为风险，见[负责人的职责](20261031-goal_mode.md#lead_role)。
- **已知限制**：经 MCP 工具写的文件不在 Agent 上报的编辑里，越界提醒和交付痕迹核对看不到它们；验证子任务也能用资源库里所有的 MCP，「只检查不改文件」靠指令和 Agent 的权限模式保证（权限不另加开关，见 [ADR：权限由 Agent 原生模式决定](../decisions/20261031-agent_native_permissions.md)）。工具藏在三个工具后面，Agent 原生逐个工具的确认只作用于这三个工具。同一个服务器在一个 Run 里只有一个进程，会话之间不隔离：浏览器、终端这类会记状态的服务器，状态在同一个 Run 的会话之间共享。同一个资源在几个 Run 里同时运行时共用一个隔离目录，有本地状态文件的服务器可能互相影响。代码模式关掉时，带代码的 Skill 不能运行。
- 名字 `janus` 和 `janus-tools` 留给 Janus，资源库里的 MCP 服务器不能用。

<a id="skill_router"></a>
## Skill 路由

成员快照中的 Skill 不论多少，都放在资源包的 `janus-skills/.skills/<名字>/` 下（隐藏目录，Agent 不会逐个加载），另外生成一个 Skill `janus-skills`，它是这些 Skill 的目录（`library/skill_router.rs`）。这样提示词里只有一条描述，资源库里的 Skill 也不出现在 Agent 原生的 `/` 菜单中。名字 `janus-skills` 留给 Janus，资源库里的 Skill 不能用。设计取舍和调研见 [ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md) 和[调研：Skill 路由的通行做法](../research/20261120-skill_routing_practices.md)。

- **每个 Skill 写什么**：从这个 Skill 交给 Agent 的 `SKILL.md` 读出完整的 `description`，加上 `when_to_use`（Claude Code 的第二个路由字段），合成一行，最多 1536 个字符（和 Claude Code 列表的上限相同）。资源上存的描述截到 240 个字符，只用于列表显示，不用于目录。`disable-model-invocation: true` 的 Skill 照样列出，标注「只在点名时使用」，正文说明是用户或负责人点名：资源库 Skill 不进 Agent 的 `/` 菜单，隐藏后用户在目标里点名也找不到它。
- **带代码的 Skill**：目录里有 `code.js` 的 Skill，在它的一行末尾标出「带代码：MCP 路由的 run_code 里用 skills.<名字>(args) 调用」（名字里的 `-` 写成 `_`），见 [MCP 路由](#mcp_router)的 `run_code`。
- **顺序**：被读过的会话多的在前（见[使用记录](#skill_uses)），一样多的按名字。描述放不下时先保住常用的。
- **语言**：`janus-skills` 的描述和正文按 Run 的语言写中文或英文（`bundle_team` 生成资源包时传入 Run 的语言；资源库页判断目录分档时用应用当前的语言）。
- **描述**：`janus-skills` 自己的描述是 Agent 一直看得到的唯一部分，所以第一句写何时读它（「每项任务开始时，先读这份目录，看 Janus 资源库的 N 个 Skill 中有没有适用的……」，英文是「At the start of each task, read this catalog……」），再按顺序列出「名字（描述片段）」。片段依次取 100、50、24 个字符，取能让全部 Skill 都放下的最长一种；名字都放不下时列出放得下的名字和总数。总长不超过 1000 个字节（Codex 拒绝超过 1024 个字节的描述）。
- **正文**：先说明怎样用（按任务找描述相符的 Skill，读它的 `SKILL.md` 照着做，没有相符的就不用），并写一次路径规则 `<资源包>/…/janus-skills/.skills/<名字>/SKILL.md`。目录分两档（`listed`）：每个 Skill 一行加起来不超过 16 KB（约 5k token，和两种 Agent 自己的 Skill 列表预算相当）时，正文逐行列出；超过时正文只列名字，描述写进同一目录的 `catalog.txt`（每行「名字、制表符、描述」），正文给出 `grep -i -E` 的用法，让 Agent 用中英文关键词和同义词搜索，再读最相符的一两个。这样资源库再大，读一次目录的开销也大致不变。
- **不在系统层指路**：Janus 不在系统层写 `janus-skills` 的位置或用法。Agent 按原生方式加载它（Claude Code 作为本地插件，Codex 作为额外的根目录，见[交付方式](#delivery)），靠它的描述决定何时读。Agent 自己的 Skill 列表有预算（Claude Code 占上下文的 1%，超出时从最少用的开始只留名字；Codex 占 2%，超出时先截短再去掉描述），所以描述把何时读放在最前面。

Skill 路由让 Agent 读取 Janus 目录下资源包里的文件：默认的权限模式（Codex 的 Full access、Claude Code 的 Bypass permissions）不需要确认，更严格的模式会询问。

<a id="skill_uses"></a>
## Skill 的使用记录

Agent 读了资源库某个 Skill 的 `SKILL.md`，Janus 记一条 `skill.used`（`actor` 是读它的会话，协作者的带 `task` 和 `attempt`；`data.skill`、`data.item`、`data.revision`），见[事件](../interfaces/20261031-events.md)。

- **怎样认出**：一次工具调用完成时（`status` 为 `completed`），在它的 `rawInput`、`locations` 和标题里找 `…/janus-skills/.skills/<名字>/SKILL.md`（`skills_read`）：Claude Code 的 Read 带路径，Codex 的 shell 命令里有路径。名字要是这个成员快照里的 Skill。读目录、搜 `catalog.txt`、读 Skill 目录里的其他文件都不算。同一轮里读同一个 Skill 只记一次。
- **怎样计数**：资源库按 Skill 数「读过它的会话」，同一个 Run、同一个成员、同一个子任务和尝试里读几次都算一次，并记下最近一次的时间（`Db::skill_uses`）。资源库列表里每个 Skill 带 `uses`：`sessions` 和 `lastAt`（没读过时为 `null`），应用在展开的详情里显示「被 N 个会话读过 · 最近一次：…」。
- **用在哪里**：新 Run 的目录按这个计数排序（见上节）；资源库显示它，帮助用户看出哪些 Skill 一直没被用上，多半是描述没写清楚。
- 删除 Run 时它的事件一起删除，计数随之减少。

<a id="skill_hints"></a>
## Skill 的改进提示

Agent 只凭描述从目录里挑 Skill，所以资源库检查每个 Skill 的描述（`library/quality.rs`，用的是目录里的那一行），在列表的 `hints` 里按类别返回，同时按客户端的语言写成文字（`hintTexts`，`setup_view::library`）。它们只是提示：不拒绝保存，Skill 照常进入目录。

| 类别（`kind`） | 什么时候 |
| --- | --- |
| `short` | 描述太短：按字符数算不到 40（一个汉字算 2），说不清做什么、什么时候用 |
| `noWhen` | 描述没写什么时候用：没有 `when_to_use`，描述里也没有「when」「use for」「用于」「时使用」「适用」「当」这类说法。已经是 `short` 的不再提示 |
| `long` | 描述和 `when_to_use` 合起来超过 1536 个字符（`chars`），目录只保留前 `kept` 个：把关键用途和触发词写在开头 |
| `alike` | 和另一个 Skill（`skill`）的描述很像：两段描述的词（英文是 3 个字母以上的单词，中文是相邻的两个汉字）有 60% 以上相同（Jaccard），且各自至少有 6 个词，Agent 可能分不清用哪个 |

列表顶层的 `skillRouter.catalog` 是 `list` 或 `search`，说明新 Run 的目录会直接列出每个 Skill，还是让 Agent 按关键词搜索，和生成目录用同一个判断；应用的路由说明据此显示「先查目录」这一步。
