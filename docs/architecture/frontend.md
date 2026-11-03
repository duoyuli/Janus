# macOS 应用

本文说明 `frontend/` 中的 macOS 应用：它和 Janus 服务的关系、源码结构、怎样从事件推导界面状态、与服务同步的方式，以及主要页面和请求卡片。安装、打包和发布见[部署方案](../operations/deployment.md#macos_app)和 [CI 与发布](../operations/ci_release.md)，设计取舍见 [ADR：用原生 macOS 应用做展示层](../decisions/native_macos_app.md)。

## 形态

- SwiftUI 应用，Swift Package（`frontend/Package.swift`）。只用系统框架，没有第三方依赖。
- 只支持 Apple 芯片（arm64），最低 macOS 14。
- 它是 Janus 唯一的界面：所有数据和操作都通过 [HTTP API](../interfaces/http_api.md) 读写，编排器不提供网页。
- 安装包 `Janus.app` 里放着应用（`Contents/MacOS/Janus`）、Janus 服务（`Contents/MacOS/janus-server`，即 release 构建的 `janus`）和 Node.js（`Contents/Resources/node`），下载后不需要编译 Rust，也不需要安装 Node。

<a id="server_process"></a>
## 与 Janus 服务的关系

`Backend`（`Core/Backend.swift`）在启动时决定用哪个服务：

1. 读取 Janus 配置的地址：`JANUS_ADDR`，否则 `<Janus home>/config.toml` 的 `addr`，否则 `127.0.0.1:7878`，与服务自己的规则相同。
2. 这个地址上有 Janus 应答 `GET /api/meta`，并且它的 `dataDir` 就是这个 Janus home（例如 `just start` 启动的 launchd 服务，或 `just dev`）时，直接连接它，不另起服务。端口上是别的程序，或是另一个 Janus home 的 Janus，都不算。
3. `<Janus home>/janus.pid` 记录的进程还活着时，说明已有 Janus 在用这个 home，只是不在配置的地址上：从 `logs/janus.log` 最后一行「`http://<地址>  ·  home <目录>`」取得它的地址并连接。连不上时提示先退出那个 Janus，不再启动第二个（[单实例](../operations/deployment.md#single_instance)）。
4. 否则启动服务程序：优先用应用包内的 `janus-server`，其次是 `JANUS_SERVER_BIN`，最后是 `just install` 安装的 `<Janus home>/bin/janus`。30 秒内没有应答、或服务退出时，启动页显示日志末尾，提供「重试」和「打开日志目录」。

<a id="port_conflict"></a>
### 端口被占用

应用自己启动服务时，先按服务的方式（`SO_REUSEADDR`）试绑定配置的端口：

- 端口空闲就用它。
- 被占用时，依次试后面 20 个端口，都不行就由系统分配一个空闲端口。选中的地址通过 `JANUS_ADDR` 传给服务，服务写进看板 MCP 地址的也是它。
- 检查和服务真正绑定之间端口仍可能被抢走：服务因 `Address already in use` 退出时，换下一个端口重试，最多 3 次。
- 改用了其他端口时，侧栏底部的地址下显示原因，例如「端口 7878 被其他程序占用，改用 7879」。`config.toml` 不会被改写，下次启动仍先试配置的端口。

### 应用启动的服务

- 标准输出和错误追加到 `<Janus home>/logs/janus.out.log`，和服务方式运行时一样。
- 从 Finder 打开的应用只有很短的 `PATH`，找不到 `git` 和 Agent CLI。应用用登录 shell（`$SHELL -ilc`）读取用户的 `PATH`，再补上 Homebrew、`~/.local/bin` 等常见位置，传给服务。
- 包内的 Node.js 排在这个 `PATH` 的最前面，并设为 `JANUS_NODE`：ACP 适配器用它运行，`npm` / `npx` 安装适配器时也用它，Agent 进程继承同一个 `PATH`。用户自己设置了 `JANUS_NODE` 或 `config.toml` 的 `node` 时不覆盖。
- 退出应用时停止服务（SIGTERM，3 秒后 SIGKILL）；被 `kill` 或 Ctrl-C 结束时也一样。还有进行中的 Run 时，退出前先确认，这些 Run 会中断，下次可以「从中断处继续」。
- 连接的是别人启动的服务（launchd 服务）时，退出应用不影响它。

<a id="source_layout"></a>
## 源码结构

```text
frontend/
├── Package.swift
├── Info.plist                 # 应用包的 Info.plist 模板，版本号由打包脚本填入
├── Sources/Janus/
│   ├── App.swift              # 入口、菜单命令、AppDelegate（启动和停止服务）、启动页
│   ├── Core/
│   │   ├── JSON.swift         # 动态 JSON 值，事件和设置对象都按字段读取
│   │   ├── API.swift          # HTTP 请求和 SSE 事件流
│   │   ├── Backend.swift      # 找到或启动 Janus 服务
│   │   ├── AppStore.swift     # Agents、角色、工作区、Run 列表、适配器、Skill 库和 Skill 来源、MCP 库和 MCP 来源，以及当前屏幕
│   │   ├── RunModel.swift     # 打开的 Run：事件、订阅、操作、回放、草稿
│   │   ├── Derive.swift       # derive / boardFacts：由事件推导界面状态
│   │   ├── Format.swift       # 状态文字、事件图标与色调、时间和文本格式
│   │   ├── Setup.swift        # 会话控制项、权限模式标签、环境变量文本
│   │   ├── MarkdownDoc.swift  # Markdown 文档的块结构解析（MDDoc）
│   │   ├── RunDocs.swift      # 收集一个 Run 交付、保存和提到的 Markdown 文件
│   │   ├── Updater.swift      # 应用内更新：检查、按部分下载、校验签名、替换并重新打开
│   │   └── Snapshots.swift    # 开发用：按命令保存窗口截图
│   └── UI/                    # 侧栏、工作区首页、Run 各页面、设置窗口（Settings/）
├── Tests/JanusTests/          # Swift Testing 测试
└── scripts/                   # package.sh（打包，含下载 Node.js）、make-icon.swift（图标）
```

<a id="event_derivation"></a>
## 由事件推导状态

运行页不保存自己的业务状态。`derive(events)`（`Core/Derive.swift`）从头遍历事件，推导出任务、Agent 状态、待处理事项（`inbox`）、回答、消息、产物和交付结果；`boardFacts` 由此整理出待处理事项、结论、产物、交接和决定，进展时间线用其中的结论、产物、决定和待处理事项放置标记。每次尝试记录开始和结束时间（`Attempt.end`，在尝试成功、失败、被取消、Run 中断或新尝试开始时写入），时间线据此画任务条。回放时把前 N 条事件交给同样的函数。

这样做的约束：

- 新增或修改事件类型时，必须同步更新 `derive` 中对应的分支，否则界面不会反映新状态；看板条目依赖 `boardFacts` 对事件类型和 `data` 字段的解释。检查清单见[事件参考](../interfaces/events.md)。
- 缺少前提的事件（例如引用了看板上没有的任务）被跳过，不中断推导。早期计划模式 Run 的计划、评审等事件不再推导，这些 Run 只显示会话、交付和审计日志，见[早期版本的 Run](../domains/run_lifecycle.md#early_plan_runs)。
- 「待处理」分两类：用户要处理的事项（来自 `inbox`，以及主控可能卡住，标「等你决定」，排在前面）和主控在处理的事项（协作者的阻塞和提问、两个任务改了同一个文件、可能卡住的任务、失败的任务、依赖失败的任务，标「主控处理」）。可能卡住的任务来自 `task.stalled`，收到 `data.resolved: true`、执行它的 Agent 再有任何事件、新的尝试开始或任务不再执行时移除。主控可能卡住来自 `lead.stalled`，收到 `data.resolved: true` 或 Run 结束时移除。改了同一个文件来自 `task.conflict`，两个任务都不再执行时移除。只有存在前一类时，这一栏才用警示色。还在路上的消息只显示在各会话的「待发送」区，不进入这一栏。
- 同一个待处理事项（按 `key`）在 `inbox` 中只保留一条。事项被回答、撤回（`decision.withdrawn`）或 Run 结束后，从「需要你处理」中移除。

<a id="server_sync"></a>
## 与服务同步

| 场景 | 行为 |
| --- | --- |
| 打开 Run | `GET /api/runs/{id}` 取全部事件，再订阅 `/api/runs/{id}/stream?after=<最后一个 seq>`。从首页或其他页面回到已打开的 Run 时同样重新读取 |
| 收到事件 | `seq` 必须正好是上一个加 1；出现缺口或收到 `lagged` 时重新读取整个 Run（在新的任务中读取，因为读取会先取消当前的事件流任务）。一批事件到达后合并成一次推导（60 毫秒） |
| 事件流断开 | 2 秒后重新读取并订阅，只要这个 Run 还开着 |
| 发送操作 | 附加 `commandId`（去重）和 `expectedSequence`（当前最后一个 `seq`）；返回 409 时刷新一次再重试 |
| 中断的 Run | `resumeRun` 或 `message` 成功后重新连接 |
| 回放 | 审计页可以按 `seq` 回放；回放期间不发送任何操作。审计页顶部的系统不变量检查平时只显示一行结论，有不满足的项时自动展开 |
| Run 状态变化 | 立即刷新侧栏的 Run 列表；平时每 5 秒刷新一次 |
| Agent 检查、登录、适配器安装 | 在服务端后台进行，应用轮询直到结束 |

<a id="screens"></a>
## 页面

| 位置 | 内容 |
| --- | --- |
| 侧栏 | 顶部是当前工作区的下拉卡片（`WorkspacePicker`，名称和目录），点卡片任意位置打开菜单：选择工作区（进入它的首页，选当前的也一样）、新建工作区、工作区设置、在 Finder 中显示；在首页时卡片高亮。下面只列当前工作区的任务记录（带状态）。底部是[应用更新](#updates)、Skill 来源更新的提醒，服务地址和打开设置窗口的按钮 |
| 工作区首页 | 目标输入框（⌘↩ 交给主控；还没有任务时显示几个产品上线流程的示例目标）、一行团队摘要、[共享资料](../domains/agents_and_roles.md#workspace_brief)（超过 6 行时折叠，可展开；没有时只显示标题和「添加…」，「添加… / 编辑…」打开工作区设置）、等你处理的 Run（等你追加预算、等你验收）。完整的 Run 列表在侧栏，团队成员的模型和权限在工作区设置 |
| 工作区设置（表单） | 目录（系统的文件夹选择框）、名称、[共享资料](../domains/agents_and_roles.md#workspace_brief)（显示字数和上限，超过上限不能保存）、启用的角色、主控 |
| 运行页 | 工具栏切换「工作台 / 审计与回放 / 团队与权限」，「文档」菜单列出这个 Run 的 Markdown 文档（见[文档查看](#markdown_viewer)），并可复制交付摘要、导出事件日志（JSONL）。任务页和 Agent 页的工具栏左侧有返回按钮，此时分段控件不选中任何一项，点「工作台」也能返回 |
| 任务页 / Agent 页 | 任务页左侧是会话、验收标准和折叠的执行轨迹；右侧一张概要卡片列出执行者、上游、下游和尝试，运行环境折叠在其中，有产物时再显示产物。编排器发给 Agent 的任务说明在会话中每次尝试的开头展开查看。Agent 页是对话和它负责的任务 |
| 设置窗口（⌘,） | 系统的设置窗口（SwiftUI `Settings`，`UI/Settings/SettingsView.swift`），工具栏切换五个面板，窗口标题跟随面板，重新打开时回到上次的面板：Agents（添加、编辑、检查、登录、删除）、角色（模型、指令、Skill、MCP、权限、会话选项；[执行统计](../domains/agents_and_roles.md#role_stats)显示在角色编辑器标题下）、Skill 库（粘贴 GitHub 链接添加来源、检查更新、一键更新、删除、浏览每个来源的 Skill 和挂载它的角色；管理自建的 Skill，见 [Skill 来源](../domains/agents_and_roles.md#skill_sources)）、MCP 库（导入 GitHub 来源、检查更新、重新拉取、删除，浏览 stdio / HTTP 服务器和挂载它的角色，按变量名设置或清除凭据，清除成功后立即显示「未设置」，见 [MCP 来源](../domains/agents_and_roles.md#mcp_library)）、ACP 适配器（安装、检查更新、升级、切回，见 [ACP 适配器管理](adapters.md)）。角色编辑器可以按来源勾选 MCP 库中的服务器，也可以手工添加 stdio 服务器；来源里消失的资源标为找不到，保存时移除。主窗口中需要设置的地方（入门步骤、角色不能运行的提示）直接打开对应面板 |

工作台包含运行卡片（阶段条「推进目标 / 协作 / 交付 / 验收」）、需要你处理、交付结果、[进展](#run_timeline)和 Agent 面板，后四项有内容时才显示。目标可能是一大段粘贴的说明，标题和 Run 列表只显示它的第一行（`goalTitle`），完整目标在运行卡片中展开查看。交付结果依次显示主控的总结、交付物、证据、风险与限制（没有的项不显示）、主控建议写入共享资料的条目（`BriefSuggestions`，带「加入共享资料…」按钮），以及验收操作。

<a id="updates"></a>
## 应用内更新

`Updater`（`Core/Updater.swift`）从 Info.plist 的 `JanusUpdateRepo`（`duoyuli/Janus`）的最新 GitHub Release 更新 Janus.app。只有打包出的 `.app` 会检查，`swift run` 不会。发布产物和签名见 [CI 与发布](../operations/ci_release.md#update_parts)。

1. **检查**：启动 20 秒后检查一次，之后每 6 小时一次；菜单「Janus → 检查更新…」立即检查并用对话框告知结果。读取 `releases/latest` 的 `update.json`，版本号按点分数字比较，新版本要求的 macOS 更高时不提示。
2. **提醒**：侧栏底部显示「Janus <版本> 可以更新」（`UpdateBadge`），点击后确认对话框给出下载大小、Node.js 是否沿用本机的、更新会中断正在执行的任务，以及发布说明。
3. **下载**：`plan` 决定下载哪些部分：应用部分总是下载；Node.js 部分只在 `update.json` 的内容哈希与本机 `update-parts.json` 不同时下载。下载到 `~/.janus/updates/<版本>/`，逐个核对 SHA-256。
4. **拼装与校验**：解开应用部分，Node.js 用下载的部分，或用 `cp -c` 克隆本机的（APFS 上不占额外空间），去掉隔离标记。然后用 Security 框架校验：签名完整（含嵌套代码，`kSecCSStrictValidate`），版本号与发布一致；当前应用有固定签名者时，还要满足当前应用的指定要求，否则拒绝安装。沿用的 Node.js 导致签名不符时，改为下载 Node.js 部分再试一次。
5. **替换**：写出 `~/.janus/updates/install.sh` 并在后台启动，然后退出应用（跳过「还有正在执行的任务」的确认，确认已在第 2 步做过；退出照常停止应用启动的服务）。脚本等应用进程结束，把旧应用移到 `~/.janus/updates/previous/Janus.app`，移入新应用并 `open`，记录写在 `~/.janus/logs/update.log`。移入失败时放回旧应用。

以下情况不能自动更新，侧栏给出原因：应用被 macOS 放在隔离位置运行（路径含 `AppTranslocation`，没有拖进「应用程序」就打开时会这样）、应用所在目录不可写、下载或校验失败。

应用自己下载的文件不带 `com.apple.quarantine`，所以 Gatekeeper 不会像对浏览器下载的 DMG 那样要求在「隐私与安全性」中允许。设计取舍见 [ADR：应用内增量更新](../decisions/in_app_updates.md)。

<a id="markdown_viewer"></a>
## 文档查看

团队交付的大多是 Markdown 文档（PRD、调研报告、发布说明、验收记录），应用内置渲染，不用切到别的编辑器。

| 入口 | 行为 |
| --- | --- |
| 运行页工具栏「文档」 | `DocumentsMenu` 分三组列出这个 Run 的 Markdown 文件：交付物（`run.delivered` 的 `deliverables`）、任务产物（`artifact.created`）、Agent 写过或提到的文档。最后一组从事件的文本和 `data` 中找绝对路径和相对于 Run 目录的路径（`RunDocs.paths`），不看 `session.prompt` 和 `agent.status`；只列出存在的文件，每组按最近一次提到排序。事件增量扫描，重新读取 Run 时从头扫描 |
| 交付结果、任务页的「查看」 | 打开对应的交付物或产物 |
| 会话和交付总结中的链接 | 指向本机 `.md` 文件的链接在 Janus 中打开（`DocLinks`），其他链接交给系统 |
| 文件 > 打开 Markdown 文件…（⌘O） | 选择任意 Markdown 文件打开 |

文档窗口（`ArtifactWindow`，`UI/DocumentViewer.swift`）每个文件一个，再次打开时把已有窗口提到前面并重新读取。窗口顶部可以切换「预览 / 源文本」、重新读取（Agent 可能还在修改文件）、复制内容或路径、在 Finder 中显示、用默认应用打开。非 Markdown 的产物按等宽文本显示。应用以用户身份运行，先直接读本机文件；读不了时再通过 `/api/file` 读取，这个接口只提供产物目录。

渲染分两步：`MDDoc.parse` 把文档解析成块（标题、段落、有序和无序列表及嵌套、任务列表、代码块、引用、GFM 表格及列对齐、分隔线、单独一行的图片），行内格式（强调、行内代码、链接、删除线）交给 `AttributedString(markdown:)`。中文行之间的软换行直接相连，不插入空格。相对链接和图片按文档所在目录解析：指向另一个 Markdown 文件的链接打开新的文档窗口，`#标题` 跳到本文档对应的标题，本地图片直接显示。不支持内嵌 HTML 和脚注。

<a id="run_timeline"></a>
## 进展时间线

工作台用一张时间线跟进这次 Run 是怎样推进的（`ProgressBoard`，`UI/Run/SharedBoard.swift`；绘制在 `RunTimeline`，`UI/Run/Timeline.swift`），取代原来分开的任务图和共享看板。它只回答「什么时候、谁做了什么、交给了谁」，细节到审计日志中看。

- 每个任务一行，按开始时间从上往下排，还没开始的排在最后。左侧写任务标题、状态图标、执行者头像和名字，点按打开任务页。
- 任务的每次尝试是一条任务条，颜色是执行者的颜色，失败的尝试是红色，进行中的用虚线框；条上写用时，重试写「第 N 次」。还没开始的任务以虚线框放在「现在」之后，写出它在等什么。
- 交接画成连线：从上游任务最后一次尝试的结束处，连到下游任务开始的地方。任务的结论和产物（多个时显示数量）是任务条末尾的小图标。决定（你的、主控的）和待处理事项放在最上面一行「决定与待处理」，挨得太近时依次右移。
- 横轴是时刻。超过 3 分钟且超过总时长 8% 的空闲时段压缩成一段固定宽度的间隔（`TimeScale`），悬停显示空闲了多久，所以短时间内的密集工作不会被长时间等待挤成一条线。进行中的 Run 有一条「现在」线。
- 点按标记打开审计日志并定位到对应事件；悬停提示显示类型、时刻和摘要。时间线上不再展开详情卡片，也不提供筛选。

<a id="request_cards"></a>
## 请求卡片

所有需要用户处理的事项都用 `RequestCard`（`UI/Run/Requests.swift`）渲染为统一的卡片：

| 卡片 | 来源 |
| --- | --- |
| 提问 | `question.escalated`（协作者的提问由主控转给用户） |
| 主控请你决定 | 主控的 `ask_user`（`question.escalated`，没有任务）；没有选项时直接写回答 |
| 审批 | `approval.requested`，按 Agent 给出的选项渲染按钮 |
| 表单或链接 | `input.requested`，按 JSON Schema 渲染表单（单选、多选、文字、数字、开关，以及每题的「自己写回答」） |
| 预算用完 | `run.budget_exhausted` |
| 会话等待消息 | 人工接管中一轮结束 |

卡片出现在工作台的「需要你处理」，也内联在对应的会话中，在任一位置处理都生效。

<a id="session_messages"></a>
## 会话中的消息

会话视图（`ChatCard`）按[消息的流转](../interfaces/events.md#message_flow)显示用户和主控发给 Agent 的消息：

| 状态 | 显示位置 |
| --- | --- |
| 刚发出（`pending`） | 输入框上方的「待发送」区，标注「发送中」 |
| 排队（`queued`）、留言（`next`），还没送达 | 「待发送」区，标注排队或留言 |
| 插入当前这一轮（`steer`） | 对话中 `message.routed` 的位置，用「插话」框显示，标注「已插入这一轮，当前操作结束后读到」；Agent 读到后（`message.delivered` 带 `applied`）改为「Agent 已读到」，不另显示一条 |
| 作为新一轮（`prompt`） | 对话中 `message.routed` 的位置，普通消息气泡 |
| 排队或留言后送达 | 对话中 `message.delivered` 的位置，即它真正进入会话的时刻 |
| 主控打断这一轮后送达 | 「待发送」区显示「正在打断这一轮，马上送达」 |

会话中的其他内容：

- Agent 的消息按轻量 Markdown 渲染（段落、列表、粗体、行内代码和本地文件链接）。
- 每次工具调用一行，点开看完整输入和输出。Janus 看板工具按用途显示，例如「提交编排：T1 … → Researcher」「回复 T2：…」；同一次调用返回后替换「等待返回」那一行；Agent 自己上报的同一次看板调用（`mcp.janus.*`）不重复显示。连续超过 4 次工具调用折叠，只露出最后两次。
- 协作者问主控的问题显示为「问主控 · 主控会回答，不需要你处理」。看板推送在主控会话中显示为「看板推送 · 插入正在进行的这一轮」，点开看内容。两个任务改了同一个文件（`task.conflict`）在双方任务的会话中各显示一行。没通过验收核对、发回会话补充的提示显示为「验证没有通过 · 发回给它修复」。结构化结果逐条显示验收标准的 ✓ / ✗ 和证据。
- Agent 自主执行超过 20 秒没有新输出时，标题旁显示「N 秒没有新输出」，每 5 秒更新，原因写在悬停提示中。Agent 长时间思考或写大段内容时，ACP 不会有任何更新，这行字用来区分「在工作」和「卡住」。
- 输入框按会话状态提供插话、接管、打断、交还托管、以当前结果完成、留言或「发送并继续」，⌘↩ 发送。
- 目标输入框和会话输入框都可以附图片：⌘V 粘贴截图、浏览器「拷贝图像」或在 Finder 中拷贝的图片文件，把图片拖进来，或点回形针选择文件。剪贴板里同时有文字时（例如表格、富文本）按文字粘贴。图片先显示为输入框上方的缩略图，可以逐张移除，发送时一起提交；超过 3.75 MB 的图片缩到最长边 2048 像素并转为 JPEG（`Attach.prepare`）。输入框是 `NSTextView`（`ComposerField`），因为 SwiftUI 的文本框只能粘贴文字。对话中的用户消息和目标显示图片缩略图，点击用「预览」打开，`[附图 N]` 行不显示（`Attach.visibleText`）。

## 角色权限编辑

角色编辑器的「权限」区域只显示 Agent 声明的权限类控制项，其余控制项放在「会话选项」。权限选项按 Agent 原生的名字和说明显示，界面不标注「可写」「只读」，角色列表和工作区只显示 Agent 自己的权限模式名。Plan 类控制项和取值不显示。只有 Claude Code 和 Codex 的角色可以开启「可以担任主控」。规则见 [Agent、角色与工作区](../domains/agents_and_roles.md#permission_modes)。

<a id="interaction"></a>
## 交互约定

按 Apple 人机界面指南（HIG）统一的做法，新增界面时沿用：

- **菜单栏**：窗口里的命令在菜单栏都有对应项，不适用时禁用而不隐藏（`JanusCommands`，`App.swift`）。「文件」有新目标（⌘N）、新建工作区（⇧⌘N）、打开 Markdown 文件（⌘O）、导出事件日志（⇧⌘E）；「显示」有工作台 / 审计与回放 / 团队与权限（⌘1 / ⌘2 / ⌘3）、返回工作台（⌘[）、刷新（⌘R）和显示 / 隐藏侧栏；「Janus」菜单有检查更新…；「Run」菜单有暂停 / 继续（⇧⌘P）、从中断处继续、停止、复制交付摘要（⇧⌘C）和回到实时。
- **Esc**：运行页先退出回放，再从任务页或 Agent 页回到工作台。表单 sheet 中 Esc 等于「取消」。
- **停止 Run**：界面上 Run 级的 `cancel` 叫「停止」，状态显示「已停止」，避免和确认框的「取消」按钮混淆；任务级的取消仍叫「已取消」。
- **确认框**：`confirm` 的「取消」按钮响应 Esc。会造成数据丢失的操作（删除、停止 Run）先确认，按钮文字带「…」。表单 sheet 有未保存的修改时，取消或 Esc 先问「放弃未保存的更改？」。
- **不能执行时禁用并说明原因**：目标、回答、消息为空，或表单还有必填项时，提交按钮禁用，悬停提示或旁边的说明写出原因，不在点击后弹出提示。
- **反馈**：底部的提示（`Toast`）只用于确认已完成的操作和报告请求失败，使用系统材质，VoiceOver 会朗读。
- **颜色**：状态色（`Tone`）和品牌色 `Color.accent` 分别定义浅色和深色值，文字在卡片和对应色调的标签上都不低于 4.5:1。选中状态（选项、权限模式、Agent 类型）用系统强调色 `Color.accentColor`，并同时显示勾选图标，不只靠颜色区分。
- **指针与辅助功能**：自定义的可点按区域（任务节点、Agent 行、看板卡片、选项）加 `.hoverable()` 悬停反馈；只有图标的按钮用 `Label` 加 `.labelStyle(.iconOnly)` 提供文字标签；隐藏标签的选择器、开关和输入框也要给出标签文字。
- **做减法**：没有内容的区域不显示，不放「还没有…」占位文字（进展时间线、交付结果中的交付物 / 证据 / 风险、「需要你处理」都是这样）。次要信息放进悬停提示或二级页面：运行卡片只显示用时和任务进度，主控、工作区和开始时间在「团队与权限」；Agent 面板每行只显示名字、状态和当前在做的事，角色、运行时、模型和心跳在悬停提示中。
- **sheet 尺寸**：编辑器使用最小尺寸和理想尺寸，不写固定高度，窗口较小时也能完整显示。

## 实现约束

- 视图中的局部状态用 `Local`（`SwiftUI.State` 的别名）声明：新 SDK 中 `@State` 是宏，宏插件只随 Xcode 提供，只装命令行工具时无法编译。别名展开为同一个属性包装器，两种环境都能构建。
- 带数字插值的界面文字用 `Text(verbatim:)` 或 `String(...)`，否则 SwiftUI 会按地区格式加千位分隔符（`#1,295`）。
- 本地只用 `UserDefaults` 记住上次打开的工作区（`janus.ws`）和设置窗口的面板（`janus.settingsTab`）。
