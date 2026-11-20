# 更新记录 / Changelog

Janus 的版本按日期命名，新版本写在最上面，每节列出用户能看到的变化，先中文后英文。发布前先在 `CHANGELOG.md` 写下这一版的 change log：`Release` 工作流把对应小节作为 GitHub Release 的说明，找不到「`## <版本>`」这一节时发布失败。格式和发布步骤见 [CI 与发布](docs/operations/20261101-ci_release.md#release)。

Janus versions are named by date, newest first. Each section lists the changes people can see, in Chinese and then in English. Write the section for a version in `CHANGELOG.md` before releasing it: the `Release` workflow uses that section as the GitHub Release notes and fails when there is no `## <version>` section. See [CI and releases](docs/operations/20261101-ci_release.md#release) for the format and the release steps.

## 2026.11.20.7

- 工作区首页重新显示「任务记录」：按时间从新到旧列出这个工作区的全部任务，正在进行的也在其中。此前它从 2026.11.20.5 起一直是空的。
- 新建对话页重新排版，和工作区首页一致：顶部是标题和一句说明，数字员工卡片一行两张、同一行等高，职责最多显示三行，选中的卡片带底色；「开始对话」在卡片右下方，双击卡片可以直接开始。
- 交付后负责人的会话退出、下一条消息重新打开它时，这条消息只交给负责人一次，不会同时出现在「之前的对话」里。

- The workspace page shows its task records again: every task of the workspace, newest first, including the one in progress. Since 2026.11.20.5 the list had stayed empty.
- The new chat page is laid out like a workspace page: a title and one line of explanation at the top, then the digital employee cards two to a row at equal height, with up to three lines of their duties and a tint on the selected one. "Start a chat" sits at the lower right of the cards, and double-clicking a card starts right away.
- When the lead's session has exited after delivery and your next message reopens it, the message reaches the lead once instead of also appearing in the earlier conversation it is given.

## 2026.11.20.6

- 应用改为订阅服务端的一条全局事件流来跟进变化，不再按固定间隔逐个读取任务：任务和对话再多，侧栏、收件箱和工作区页也及时更新，不会越用越卡。
- 对话只注入数字员工自己的指令，Janus 的团队协作指令只在 Agent Team 中注入。对话没有接管模式；Agent 还在处理时显示「正在处理」，可以「停止这一轮」或插话。「开始对话」按钮移到右下角。
- 会话页把注入的指令按来源命名，并各用一行说明：Agent Team 中有「Agent Team 协作指令」和「数字员工的指令」两份，对话只有「数字员工的指令」；对话重新打开时带上的往来显示为「之前的对话」。会话的显示档位只留「详细」和「摘要」。
- 合回结果更稳妥：worktree 里的改动存不下补丁时不删除 worktree；worktree 不在时验收失败而不是当作没有改动；负责人还在处理或有提问没回答时不能验收；工作区是仓库的子目录时，会话从对应的子目录开始。
- 任务返工后，它已经通过的独立验证会重新核对，不再沿用旧结论。从资源库删除的 MCP 服务器，之后开始的会话不再拿到。
- 协作更可靠：尝试失败后等 3 秒再重试；负责人会话刚好结束时，看板消息不会丢；同时提交的两份编排不会出现重复的子任务；重复写的依赖不再被当成环；`src/./a.rs`、大小写不同的写法算同一个文件；修不同行的编译错误不再被误报为原地打转；协作者一次写到看板的内容有上限；结束会话时 Agent 进程和适配器一起结束。

- The app now follows the server through one global event stream instead of reading each task on a timer: the sidebar, inbox and workspace pages stay current however many tasks and chats there are, and the app does not slow down as they pile up.
- A chat gets only the digital employee's own instructions; Janus's team collaboration instructions go into Agent Team sessions only. Chats have no take-over mode; while the agent is working the chat says so and you can stop the turn or steer it. The "Start a chat" button moved to the lower right.
- Session pages name the injected instructions by where they come from, each with a line explaining it: an Agent Team session has "Agent Team Collaboration Instructions" and "Digital Employee Instructions", a chat only the latter, and the exchanges a reopened chat brings along show as "Earlier Conversation". The session view now has just two levels, Detailed and Summary.
- Bringing results back is safer: a worktree whose changes cannot be saved as a patch is not deleted; accepting fails when the worktree is gone instead of treating it as no changes; a run cannot be accepted while the lead is still working or a question is unanswered; when a workspace is a subdirectory of a repository, sessions start in that subdirectory.
- After a task is reworked, independent verifications it had passed are checked again instead of keeping the old verdict. An MCP server deleted from the library no longer reaches sessions that start afterwards.
- Collaboration is more reliable: a failed attempt is retried after 3 seconds; board updates are not lost when the lead's session ends at that moment; two orchestrations submitted at once cannot create duplicate subtasks; a dependency listed twice is no longer reported as a cycle; `src/./a.rs` and spellings differing only in case count as the same file; fixing compiler errors on different lines is no longer reported as going in circles; what a collaborator posts to the board in one call is capped; closing a session ends the agent process along with its adapter.

## 2026.11.20.5

- 侧栏重新整理：收件箱固定在最上面；下面「Agent Team」列出所有工作区，点一个打开它，「+」新建工作区；再下面「Chat」列出和单个数字员工的对话，「+」新建对话。两栏一起滚动，不再留出大块空白。
- 任务记录从侧栏移到工作区首页：「等你处理」下面按时间从新到旧列出这个工作区的全部任务，每行有开始时间和状态，右键可以删除已结束的任务。
- 新建对话改为先选人再开始：「和谁对话」的卡片放在页面正中，选中一名数字员工后点「和 … 开始对话」（双击卡片也可以）。会话立即打开、注入数字员工的指令，然后由你先说；你发的第一条消息成为对话的标题。
- 侧栏的对话和工作区页的任务记录不再只显示最近 50 个：滚动到底会接着读取更早的，所有对话和任务都能找到、打开和删除。
- 英文界面里设置的分类改为单词：General、Employees、Library、Usage。

- The sidebar is reorganized: the inbox stays at the top; below it, Agent Team lists every workspace (click one to open it, + makes a new one), and Chat lists your chats with a single digital employee (+ starts one). The two scroll together, with no large gap between them.
- Task records moved from the sidebar to the workspace page: under Needs Attention, every task of the workspace is listed newest first, with when it started and how it stands; right-click to delete a finished one.
- Starting a chat now means picking someone first: the "Talk With" cards sit in the middle of the page; pick a digital employee and click "Start a chat with …" (or double-click the card). The session opens right away with the employee's instructions, and you speak first; your first message becomes the chat's title.
- Chats in the sidebar and task records on a workspace page are no longer cut off at the newest 50: scrolling to the end loads older ones, so every chat and task can be found, opened and deleted.
- In the English interface, the settings sections are single words: General, Employees, Library, Usage.

## 2026.11.20.4

- 新增「对话」：在侧栏「全局」下打开，选一名数字员工直接和它对话（⌥⌘N）。它带着自己的指令、模型、权限和整个资源库，在 `~/.janus/chats/` 下为这次对话新建的目录里工作，没有团队和看板。会话和工作台里负责人的对话一样：可以插话、接管、打断、审批、粘贴图片、用 `@` 引用文件、切换模型和模式，用量照常统计；对话里的审批和表单也会进收件箱。
- 服务重启或会话结束后，对话直接发消息就会重新打开，能恢复原会话就恢复，否则带上最近的对话新开会话。删除对话时，它的记录和工作目录一起删除，打开着的会话也可以直接删除。
- Janus 会自动检查 ACP 适配器有没有新版本：服务启动后查一次，之后每 6 小时一次。有新版本时侧栏底部提醒「N 个 ACP 适配器有新版本」，点击到设置的「数字员工」页升级；只有比当前版本更新的才提醒，不会自动安装，正在运行的会话不受影响。
- 交付后继续和负责人对话时，服务重启后重新打开的会话也能用资源库里的工具。

- New: Chat. Open it under Global in the sidebar, pick a digital employee and talk with it directly (⌥⌘N). It brings its own instructions, model, permissions and the whole resource library, and works in a directory made for the chat under `~/.janus/chats/`, with no team and no board. The session works like the lead's on the workbench: steer, take over, interrupt, approve, paste images, reference files with `@`, switch model and mode, with usage counted as usual; its approvals and forms also reach the inbox.
- After a restart, or when its session ended, a chat reopens with your next message, resuming the old session when it can and otherwise starting a new one with the recent conversation. Deleting a chat removes its record and its working directory, even while its session is open.
- Janus now checks for new ACP adapter versions on its own: shortly after the server starts, then every 6 hours. When one is out, the bottom of the sidebar says how many adapters have a new version; click it to upgrade them in Settings › Digital Employees. Only versions newer than the installed one count, nothing is installed automatically, and running sessions are not affected.
- When you go on talking with the lead after delivery, a session reopened after a restart can use the resource library's tools again.

## 2026.11.20.3

- 资源库里的每项资源都给所有数字员工，去掉「给谁用」：资源库里不再有「给所有数字员工」开关和分组，MCP 编辑器里不再选给谁，数字员工编辑器里不再勾选资源。升级后，原来只给部分数字员工的资源也给所有人；不想让数字员工用的，从资源库删除。
- 数字员工的会话只拿到两个入口，不随资源库变大：所有 MCP 服务器（命令、HTTP、SSE）都经 `janus-tools`，所有 Skill（不论几个）都经 `janus-skills`。HTTP 和 SSE 服务器由 Janus 带着你保存的请求头去连接，地址和密钥都不交给 Agent；SSE 服务器现在 Codex 也能用。
- 添加或修改 HTTP、SSE 服务器后，Janus 会先连接一次，资源库里显示「已准备好 · N 个工具」，地址或密钥不对时直接显示原因。

- Every resource in the library is for every digital employee; "who gets it" is gone. The library no longer has the "for all digital employees" switch or its groups, the MCP editor no longer asks who gets a server, and the digital employee editor no longer has resource checkboxes. On upgrade, resources that were limited to some digital employees go to everyone; delete from the library what you don't want them to use.
- A digital employee's session gets just two entries, however large the library grows: every MCP server (command, HTTP, SSE) goes through `janus-tools`, and every skill, however few, through `janus-skills`. Janus connects to HTTP and SSE servers itself with the headers you saved, so neither their addresses nor their keys reach the Agent; SSE servers now work with Codex too.
- After you add or change an HTTP or SSE server, Janus connects once and the library shows "Ready · N tools", or why it failed when the address or key is wrong.

## 2026.11.20.2

- 验收任务后，结果直接合并回任务开始时你所在的分支，出现在你的工作目录里，不再生成需要你自己合并的 `janus/<任务>` 分支。任务期间这个分支有了新提交（你自己提交的，或另一个任务先合并的），任务的改动接在最新的提交之后，历史保持线性。
- 合并有冲突、会覆盖你还没提交的改动，或分支已被删除时，验收不会成功：你的目录和分支保持原样，不会留下合并了一半的文件，错误信息说明原因和结果所在的提交，任务仍是「已交付」。和任务无关的未提交改动不受影响。
- Janus 对你的仓库做的 Git 操作（创建 worktree、提交、合并）都不运行仓库的 Git 钩子。
- Git 仓库上的任务，每次执行都显示 Git 信息：仓库、合并回的分支、开始时的提交和 worktree 路径；子任务页标题下和负责人对话的开头也会显示。
- 升级后，已有的 worktree 中的任务按你现在检出的分支记录合并目标；这个分支不包含任务起点的，验收时由你自己合并。
- 工作区目录的检查结果改为说明未提交的改动不会带进任务。

- Accepting a task now merges its result back into the branch you were on when it started, so it lands in your working directory; there is no `janus/<task>` branch to merge yourself anymore. If that branch got new commits during the task (yours, or another task merged first), the task's changes go on top of the latest commit and history stays linear.
- When the merge conflicts, would overwrite changes you have not committed, or the branch was deleted, acceptance does not go through: your directory and branch stay as they were with no half-merged files, the error says why and which commit holds the result, and the task stays Delivered. Uncommitted changes unrelated to the task are left alone.
- Git operations Janus runs on your repository (creating worktrees, committing, merging) no longer run the repository's Git hooks.
- For tasks on a Git repository, every attempt shows its Git details: the repository, the branch it merges back into, the starting commit and the worktree path; they also appear under a subtask's title and at the start of the lead's conversation.
- On upgrade, existing tasks in a worktree record the branch you have checked out now as their merge target; where that branch does not contain the task's starting point, you merge the result yourself on acceptance.
- The working directory check now says uncommitted changes are not brought into tasks.

## 2026.11.20

- 资源库分成「Skill」和「MCP」两个标签。每个标签最上面用一段动画讲清楚它的路由怎样工作：Skill 超过 10 个时合并成一个 `janus-skills`、用到再读；MCP 服务器只经一个 `janus-tools` 入口，用到才启动、同一个任务共用、闲置就停。动画用你资源库里真实的名字，可以点某一步跳过去，也可以收起。
- 已添加的 Skill 按来源分组：每个 GitHub 仓库一组（检查更新、更新、再导入仓库里其他 Skill 都在组里），然后是本机文件夹和在 Janus 里新建的，组可以折叠；MCP 服务器分成「给所有数字员工」和「只给选中的数字员工」两组。
- 新的 MCP 编辑器：在「表单」和「JSON」之间切换，JSON 和 Claude Code 的 `.mcp.json` 写法相同，改了立刻同步回表单；可以直接粘贴 README 里的配置，一次粘贴多个服务器就一起添加。说明、「每个会话单独运行」和给谁用（直接勾选数字员工）都在编辑器里。环境变量和请求头的值在编辑器里能看能改，清空后保存就删掉；这些值只返回给本机的 Janus 应用，网页读不到。
- 每个标签最下面是「推荐」：社区常用的 Skill（Anthropic、Superpowers、Matt Pocock、Vercel）和 MCP 服务器（Context7、DeepWiki、GitHub、Playwright 等），一键添加；需要 API key 的会打开填好其余内容的编辑器，告诉你去哪里拿 key。已经添加过的标为「已添加」。
- 去掉「自定义命令」类型的 Agent，Agent 只有 Codex 和 Claude Code；数字员工绑定了 Agent 就可以担任负责人。升级后，已有的自定义命令 Agent 会被删除，绑定它的数字员工改为没有绑定 Agent，用过它的旧任务仍能查看，但不能继续运行。

- The Resource Library has two tabs, Skills and MCP. Each starts with a short animation of how its router works: past 10 skills, they are combined into one `janus-skills` and read when needed; MCP servers sit behind one `janus-tools` entry, start on first use, are shared within a run, and stop when idle. The animation uses the names in your library; click a step to jump to it, or hide it.
- Added skills are grouped by source: one group per GitHub repository (with checking for updates, updating, and importing more of its skills), then local folders and skills written in Janus; groups fold. MCP servers are grouped into "for all digital employees" and "only for selected digital employees".
- A new MCP editor switches between a form and JSON in Claude Code's `.mcp.json` shape, kept in step as you type. Paste a README's config directly; pasting several servers adds them all. Description, "run once per session", and who gets it (tick digital employees directly) are all in the editor. Environment variable and header values can now be seen and changed there, and an emptied value is removed on save; these values go only to the Janus app on this Mac, never to web pages.
- Each tab ends with Recommended: popular skills (Anthropic, Superpowers, Matt Pocock, Vercel) and MCP servers (Context7, DeepWiki, GitHub, Playwright, and more), added in one click. One that needs an API key opens the editor with everything else filled in and says where to get the key. What you already have is marked Added.
- The custom command Agent type is gone: Agents are Codex or Claude Code, and a digital employee can lead once it is bound to an Agent. On upgrade, existing custom command Agents are deleted, the digital employees bound to one are left without an Agent, and past runs that used one can still be viewed but not continued.

## 2026.11.19.6

- 资源库里的 Skill 和 MCP 服务器默认给所有数字员工，不用再为某种工具单独建数字员工或改角色。每个会话只多出两个路由：Skill 不超过 10 个时逐个加载，更多时合并成一个 `janus-skills`，用到时再读；命令启动的 MCP 服务器由每个任务的 `janus-tools` 提供：用到才启动，同一个任务的数字员工共用一个进程（浏览器、终端这类会记住状态的服务器可以设成每个会话单独运行），闲置 10 分钟后停掉，在任务的工作目录（worktree）里、和 Agent 一样的隔离环境中运行；停止任务时连同它们派生的进程一起结束。添加或修改这类服务器时，Janus 会先在后台启动它一次，把需要的包下载好，资源库里显示「已准备好 · N 个工具」或启动失败的原因。负责人派出子任务后再调用资源库的工具，看板上会提醒它，交付时列为风险。有副作用或带凭据的工具可以关掉「给所有数字员工」，只给选中它的数字员工。新 Skill 默认给所有人，新 MCP 服务器默认只给选中它的数字员工，放开前会确认。升级后已有的资源按同样的默认值处理。
- 新增默认的「通用数字员工」，什么子任务都能接。它绑定 Agent 之后，新建工作区默认启用负责人加 3 个通用数字员工；原来的专职数字员工保留。负责人派出子任务后，资源库里的工具只用来查看。
- 资源库页面改为「Skill 路由」和「MCP 路由」两张卡片，收口的资源折叠在下面；每项一行，缺少的值用警示色标出，详情展开再看。收口资源给哪些数字员工，可以在数字员工编辑器里勾，也可以在资源库里勾。删除资源时一并从选了它的数字员工里去掉。Skill 的主要添加方式是把 GitHub 链接拖进或粘贴到链接框；MCP 服务器填写表单或粘贴配置添加，不再从 GitHub 仓库导入。GitHub 来源有更新时在页面顶部提示。
- 设置里的 ACP、Agent 和数字员工合并成一页「数字员工」：上面是 Agent 卡片，每张卡片带登录、检查和 ACP 适配器的版本与升级；下面是数字员工。
- 添加和编辑 Agent 只有「取消」和「创建」（编辑时是「保存」）两个按钮，保存后立即检查，进度显示在 Agent 卡片上。
- 去掉预设的 Agent 类型（DeepSeek、MiniMax、GLM over Claude Code）。用其他厂商的接口驱动 Claude Code 时，选 Claude 的 API Key，在环境变量里填厂商的地址和模型。已有的预设 Agent 升级后变成这样的 Claude Code Agent，原来的地址和模型变量都保留，但需要在编辑页重新填一次 API Key 并检查。

- Skills and MCP servers in the Resource Library now go to every digital employee by default, so you no longer create a digital employee or change a role just for one tool. Each session gets two routers: skills load one by one up to ten, beyond that they are combined into one `janus-skills` and read when needed; command-started MCP servers are served by each run's `janus-tools`: started on first use and shared by the run's digital employees (a server that remembers state, such as a browser or terminal, can run once per session), stopped after ten idle minutes, running in the run's working directory (its worktree) in the same sandbox as an Agent; stopping a task stops them along with the processes they spawned. When you add or change such a server, Janus starts it once in the background to download what it needs, and the library shows "Ready · N tools" or why it failed to start. When the lead calls a library tool after handing out subtasks, the board reminds it and the delivery lists it as a risk. Turn off "For All Digital Employees" on a tool with side effects or credentials to give it only to the digital employees that select it. New skills go to everyone and new MCP servers start selected-only; Janus asks before giving a server to everyone. Existing resources follow the same defaults after the upgrade.
- A new default "General Digital Employee" takes any kind of subtask. Once it is bound to an Agent, a new workspace starts with the lead and three general digital employees; the specialist digital employees remain. After the lead hands out subtasks, it uses the library's tools only to look things up.
- The Resource Library page shows two cards, Skill Router and MCP Router, with the selected-only resources folded below. Each item is one line, missing values are highlighted, and details unfold on demand. Selected-only resources can be ticked for digital employees in the employee editor or in the library. Deleting a resource also removes it from the digital employees that selected it. Skills are added mainly by dropping or pasting a GitHub link; MCP servers are added with a form or by pasting a configuration and are no longer imported from GitHub repositories. Updates to GitHub sources show at the top of the page.
- ACP, Agents, and Digital Employees in Settings are now one page, Digital Employees: Agent cards on top, each with sign-in, checks, and its ACP adapter version and upgrade; the digital employees below.
- Adding or editing an Agent has just Cancel and Create (Save when editing); saving starts the check right away, with progress on the Agent card.
- The preset Agent types (DeepSeek, MiniMax, and GLM over Claude Code) are gone. To run Claude Code on another vendor's API, choose Claude with an API key and set the vendor's address and models in the environment variables. Existing preset Agents become such Claude Code Agents after the upgrade and keep their address and model variables, but you need to enter the API key again in the editor and run the check.

## 2026.11.19.5

- Git 仓库上的每个任务都在自己的 worktree 里进行（放在 Janus 目录的 `worktrees/` 下），同一个仓库上同时跑几个任务也不会互相覆盖，你自己的工作目录不会被改动。worktree 里只有仓库跟踪的文件，加上仓库根目录 `.worktreeinclude` 列出的被忽略文件（例如 `.env`，和 Claude Code、Codex 用同一个文件）；依赖和构建产物由 Agent 在 worktree 里按项目自己的方式准备。
- 验收时，Janus 把任务的改动提交到 `janus/<任务>` 分支，什么时候合并由你决定；交付页和「团队与权限」页显示结果在哪个分支。提交不运行仓库的 Git 钩子，需要仓库或全局配置了提交身份。
- 创建任务前会提醒：工作目录里还没提交的改动不会带进任务；不是 Git 仓库的目录上已经有任务在跑时，两个任务会改同一批文件。确认后照常开始。
- 删除任务时一起删除它的 worktree，还没提交的改动先存成补丁，放在 Janus 目录的 `worktrees/deleted/` 下。
- 不是 Git 仓库的目录和以前一样，任务直接在目录里修改文件。

- Each task on a Git repository works in its own worktree (under `worktrees/` in the Janus folder), so several tasks on one repository no longer write over each other, and your own working directory is left alone. A worktree holds the tracked files plus the ignored files listed in the repository's `.worktreeinclude` (such as `.env`; the same file Claude Code and Codex use); the agents prepare dependencies and build outputs there the way the project does.
- When you accept a task, Janus commits its changes to the `janus/<task>` branch, and you decide when to merge it; the delivery and Team & Permissions show which branch holds the result. The commit skips the repository's Git hooks and needs a commit identity configured for the repository or globally.
- Before a task starts, Janus reminds you that changes not committed in your working directory are not brought along, or, in a directory outside Git, that another task already working there would edit the same files. You can continue anyway.
- Deleting a task deletes its worktree too; changes never committed are first saved as a patch under `worktrees/deleted/` in the Janus folder.
- Directories outside Git work as before: tasks edit the directory in place.

## 2026.11.19.4

- 负责人分工更清楚：目标小到一个人就能做完时，负责人自己做；一旦派出子任务，它就只做管理，不再自己改文件，也不接手做不下去的子任务，改为让原来的人返工、换人，或告诉你缺什么能力。派出后它如果还是改了文件，会在看板上提醒它，负责人的对话里显示一行提醒，交付时列为风险。
- 交付页标出改了文件、却没有独立验证的子任务（「没有独立验证」），概览里写明有几个；负责人的总结要说明这些子任务是怎么核对的。
- 你直接在某个协作者的会话里发消息、接管、交还、打断或让它以当前结果结束时，负责人会在看板上读到，不会按过时的情况安排工作，也不会把你的改动改回去。
- 负责人要求接着某个子任务原来的会话做、而那个子任务从来没开过会话时，会话标题写明「新开会话（这个子任务之前没有会话）」，负责人也会当场知道。
- 默认数字员工 PMO 的指令按上面的分工更新：没改过的自动换成新版，改过的在编辑器里提示新版，由你选择。

- Clearer roles for the lead: it does a goal itself only when one person can finish it. Once it hands out a subtask it only manages: it no longer edits files and does not take over a subtask that is stuck; it sends it back for rework, gives it to someone else, or tells you which ability is missing. If it still edits files after handing out work, the board reminds it, its conversation shows a note, and the delivery lists it as a risk.
- The delivery marks subtasks that changed files without an independent verification ("No independent verification") and counts them in the overview; the lead's summary has to say how it checked them.
- When you message, take over, hand back, interrupt or end a collaborator's session yourself, the lead reads it on the board, so it does not plan on stale facts or undo your change.
- When the lead asks to continue a subtask's earlier session but that subtask never opened one, the attempt header says "New session (this subtask had no session before)" and the lead is told right away.
- The default PMO digital worker's instructions follow this split: untouched ones move to the new text automatically; if you edited yours, the editor offers the new text and you choose.

## 2026.11.19.3

- 负责人让一个已完成的子任务返工时，会知道哪些子任务已经用了它的旧结果：正在执行的会在会话里收到「上游在返工」的通知，提醒它们用到的文件可能会变；正在执行的和已经完成的都列给负责人，由负责人决定要不要一起返工或停下。修订结论时也是同样的处理。

- When the lead sends a finished subtask back for rework, it now learns which subtasks already used the old result. Running ones get a note in their session that their upstream is being reworked and the files they use may change; both running and finished ones are listed for the lead, who decides whether to rework or stop them too. Amending a conclusion works the same way.

## 2026.11.19.2

- 独立验证判定没通过时，负责人可以让原来的子任务按验证意见返工，不再为修复另建子任务。返工默认回到这个子任务原来的会话，之前的上下文还在；负责人每次都要选择接着原会话做还是新开会话，一个数字员工可以为不同的子任务保留各自的会话。验证意见原样交给执行者，不经负责人转述。
- 返工完成后，验证者在自己原来的会话中复审，先核对上一轮指出的问题，再检查有没有改坏别的；复审通过后，被它挡住的子任务自动开始。
- 会话里每次尝试的标题写明它在哪个会话、为什么开始，例如「接着原会话 · 按 V1 的意见返工」「T1 返工后复审」，返工和复审在同一个会话卡片里接着显示。进展中，负责人的返工记为一条决定，写明选的会话、依据的验证意见和负责人的说明。

- When an independent verification rejects a subtask, the lead can send that subtask back for rework on the verification's findings instead of creating a new subtask for the fix. The rework goes back into the subtask's own session by default, with its earlier context. The lead chooses every time whether to continue that session or start a new one, and one digital worker can keep a separate session for each of its subtasks. The findings reach the assignee verbatim, without the lead restating them.
- Once the rework is done, the verifier checks it again in its own earlier session: first the problems it raised, then whether anything else broke. When the re-check passes, subtasks it was holding back start on their own.
- Each attempt in a session now says which session it is in and why it started, for example "Continues its own session · Rework on V1's findings" or "Re-check after T1 was reworked", so a rework and its re-check carry on in the same session card. In Progress, the lead's rework is recorded as a decision with the chosen session, the findings it acts on and the lead's note.

## 2026.11.19

- 协作者连续修改同一个文件、或者「改代码 → 跑测试」交替进行时，只要每次改的内容或测试输出在变，就不再被报为「原地打转」，负责人也不会因此被打断。真正卡住的循环照样会报告：参数和结果都没变的调用，包括反复跑同一个失败测试、只有耗时和时间不同的情况。
- 负责人等待看板时，只在子任务完成或者有需要它处理的事（提问、失败、阻塞、冲突等）时才被唤醒。「开始」「进展」这类只是告知的消息会跟着下一次唤醒一起送到，不会丢，也不再单独占用负责人的一轮和 token。
- 任务进行中，运行卡片上的耗时每秒走动，等你审批或回答时也一样。任务结束、回放或只读查看时，显示记录下来的耗时。

- When a collaborator keeps editing the same file, or alternates between changing code and running tests, it is no longer reported as going in circles as long as the edits or the test output keep changing, so the lead is not interrupted for it. Real loops are still reported: calls whose input and result do not change, including running the same failing test again where only the timings and times differ.
- While the lead waits on the board, it is woken only when a subtask finishes or something needs it (a question, a failure, a blocker, a conflict and the like). Informational messages such as starts and progress come along with the next wake-up. Nothing is lost, and they no longer cost the lead a turn and tokens of their own.
- While a task runs, the elapsed time on its run card ticks every second, also while it waits for your approval or answer. A finished task, a replay or a read-only view shows the recorded time.

## 2026.11.18.8

- 交付后用 `/goal` 推进新目标时，协作者的提问仍交给负责人回答，看板消息也照常推送给负责人；只有负责人空闲、等你的下一条消息时，还没回答的提问才转给你。之前第一个目标完成后，这些提问全部转给你。
- 同时给已交付或已中断的任务发两条消息时，两条都会送到负责人，只恢复一次会话。之前第二条可能丢失。服务重启后，「从中断处继续」也不会再误报「任务已被另一个 Janus 实例接管」。
- 回放时会话输入框显示「回放中」并只读，写下的草稿保留。之前在回放中点发送会清空草稿，消息也没有发出去。发送失败的消息会回到输入框。
- 服务在登录过程中重启后，Agent 卡片不再一直显示「等待登录」，可以重新登录。
- 配置检查进行中再次点检查，或者检查期间保存了 Agent，旧检查的结果不会再覆盖新的。检查没有完成时提示「检查没有完成」，不再显示没有原因的「检查未通过」。
- 在连接服务失败后点「重试」，退出 Janus 时仍会提醒还有正在执行的任务。
- 操作在服务重启时被打断，用同一个请求重试会提示无法确认是否生效，不再当作已成功。
- 删除任务时某个目录删不掉，排除原因后可以再删一次，不会留下删不掉的残余。
- 删除 Agent 后，用到它的数字员工的会话设置一并清空。
- 应用内更新在新版本移入失败时，旧版本完整放回原处并打开。

- After delivery, when a follow-up `/goal` has the lead orchestrate new work, collaborators' questions still go to the lead and board messages are still pushed to it. Only while the lead is idle, waiting for your next message, do unanswered questions come to you. Before, every question came to you once the first goal was done.
- Two messages sent at once to a delivered or interrupted task both reach the lead, and the session is restored once. Before, the second could be lost. After a restart, Resume Interrupted Run no longer wrongly says another Janus instance took over the task.
- While replaying, a session's message box shows Replaying and is read-only, and your draft stays. Before, sending during a replay cleared the draft without sending anything. A message that fails to send comes back to the message box.
- An agent card no longer stays on Waiting for sign-in after the service restarts in the middle of a sign-in; you can sign in again.
- Checking an agent again while a check runs, or saving the agent during a check, no longer lets the old check's result overwrite the new one. A check that did not finish now says so instead of reporting a failure with no reason.
- After Retry on a failed service connection, quitting Janus still warns you about tasks in progress.
- Retrying an action that a service restart cut off now says it cannot tell whether the action took effect, instead of reporting success.
- When a folder cannot be removed while deleting a task, you can delete it again once the cause is fixed; nothing is left behind.
- Deleting an agent also clears the session settings of the digital employees that used it.
- When an in-app update cannot move the new version in, the old version is put back whole and opened.

## 2026.11.18.7

- 每次打开 Janus 都会显示主窗口。之前只留着菜单栏图标、关掉窗口时退出 Janus，下次打开就只有菜单栏图标，没有窗口。

- Janus now opens its main window at every launch. Before, quitting while only the menu bar icon was showing meant the next launch showed just the menu bar icon and no window.

## 2026.11.18.6

- 完整磁盘访问和应用程序管理不再需要手动确认，Janus 会自动检测这两项是否已授权，和屏幕录制一样显示「已授权」或「未检测到授权」。之前点过的「已手动确认」不再算数：如果系统设置里打开的其实是更新前的旧 Janus，这里会如实显示未授权。引导会在检测到授权后自动进入下一项；检测不到时，卡片会说明怎样删除同名的旧记录、添加当前的 Janus.app。所有权限都已授权时，首次启动不再弹出引导。
- 同时创建的两个任务不会再因为编号相同而创建失败。
- 回复很快的 Agent 在连接检查中不会再被误报为「没有返回任何内容」。
- 会话选项的值为空白时会被拒绝，不再发给 Agent。

- Full Disk Access and App Management no longer ask you to confirm them by hand. Janus detects whether each is granted and shows Authorized or No Authorization Detected, as it already did for Screen Recording. Earlier manual confirmations no longer count: if the switch in System Settings belongs to the Janus from before an update, the page now says the permission is missing. The guide moves on once it detects a grant; when it cannot, the card explains how to remove the old entry with the same name and add the current Janus.app. When everything is already granted, the checklist no longer opens on first launch.
- Two tasks created at the same moment no longer fail on a duplicate id.
- A connection check no longer reports "no output" for an agent that replies very quickly.
- A session option with a blank value is refused instead of being sent to the agent.

## 2026.11.18.5

- 从这个版本起，Janus 用固定的证书签名，应用标识改为 `io.github.duoyuli.janus`。macOS 会把之后的每个版本当作同一个应用，屏幕录制、完整磁盘访问等授权在更新后不再丢失。从旧版本升级到这个版本时，权限引导会弹出，需要最后重新授权一次（包括通知），应用的语言、外观等偏好设置恢复默认；`~/.janus` 中的数据不受影响。之后的更新不再弹出权限引导。
- 屏幕录制已经在系统设置里打开、Janus 却检测不到时，权限页会说明原因并提供「重新打开 Janus」：macOS 要在应用重新打开后才让新授权生效；如果系统设置里打开的是更新前的旧 Janus，先在列表中删除它，再重新授权。

- Starting with this release, Janus is signed with a fixed certificate and its app identifier is `io.github.duoyuli.janus`. macOS treats every later version as the same app, so Screen Recording, Full Disk Access and other permissions survive updates. Upgrading from an earlier version shows the permission checklist for one last round of granting (notifications too), and app preferences such as language and appearance return to their defaults; your data in `~/.janus` is unaffected. Later updates no longer show the checklist.
- When Screen Recording is on in System Settings but Janus does not detect it, the permissions page explains why and offers "Reopen Janus": macOS applies a new grant only after the app reopens, and a switch left over from the Janus before an update has to be removed and granted again.

## 2026.11.18.4

- 升级后第一次启动时，Janus 自动把已有数据升级到新版本：先把数据库备份到 `~/.janus/backups/`，再更新数据结构，例如去掉数字员工和历史任务中已经不用的「定位」字段。想先看看会改什么，可以运行 `janus migrate --dry-run`。
- 默认数字员工的职责和指令随版本更新：没改过的自动换成新版；改过的保留你的内容，并在编辑器中提示有新版，可以选择「采用新版」或「保留我的」。
- 应用连接到其他版本的 Janus 服务时（例如还在运行的旧服务）会提示，并可以一键停止旧服务、改用应用自带的服务，避免数据没有升级。
- 旧版本的 Janus 不会打开已经升级过的数据；要退回旧版本，请先从 `~/.janus/backups/` 恢复升级前的备份。

- On the first start after an upgrade, Janus upgrades your existing data: it backs up the database to `~/.janus/backups/` and then updates the stored data, for example removing the unused Role field from digital employees and past tasks. Run `janus migrate --dry-run` to preview the changes.
- The default digital employees' responsibilities and instructions now update with each release. Text you never edited moves to the new version. Text you edited stays yours, and the editor offers the new version with "Use the New Version" and "Keep Mine".
- When the app finds a Janus service of another version running (for example an old service still in the background), it says so and can stop that service and start its own, so your data does get upgraded.
- An older Janus will not open upgraded data. To go back to an older version, first restore the pre-upgrade backup from `~/.janus/backups/`.

## 2026.11.18.3

- 数字员工编辑器中，「可以担任负责人」移到职责下方。职责和指令的说明更简洁；指令的说明对所有 Agent 统一，表示它会作为系统提示词交给 Coding Agent。
- 重写了默认数字员工的职责和指令。职责写明适合派给它的工作；PMO 按目标规模选择流程，小目标直接派给合适的人，并按职责而不是名字找人；研究要求注明来源和日期，PRD 的功能和验收标准都编号，研发先读项目约定并运行检查，质量工程师独立验证时只检查不改文件。已有的默认数字员工不会自动更新。

- In the digital employee editor, "Can lead" now sits under Responsibilities. The notes under Responsibilities and Instructions are shorter, and the Instructions note is the same for every Agent: the instructions are given to the coding agent as its system prompt.
- The default digital employees have rewritten responsibilities and instructions. Responsibilities say what work suits each one. The PMO picks a process by goal size, sends small goals straight to the right person, and finds people by responsibility rather than name. Research cites sources with dates, PRDs number features and acceptance criteria, engineering reads project conventions and runs checks, and QA does not edit files when it verifies. Existing default digital employees are not updated automatically.

## 2026.11.18.2

- 「macOS 权限」和「更新」并入「通用」设置。侧栏的「可以更新」和菜单中的「检查更新…」直接打开「通用」中的更新部分。切换外观后，语言下拉框会立即跟随。
- 数字员工没有选择权限模式时，Codex 默认使用 Full access，Claude Code 默认使用 Bypass permissions，执行目标时不再停下来等待审批。需要审批时，可在数字员工的「权限」中改选其他模式。
- 负责人只按名称和职责了解协作者，不再看到协作者使用的 Agent、模型、权限模式或 token 用量。数字员工编辑器删除「定位」，在职责下说明它的作用；指令和资源移到前面，Agent、模型和权限放在一起。

- macOS Permissions and Updates are now part of General settings. "Update available" in the sidebar and "Check for Updates…" in the menu open the update section there. The language menu follows the new appearance as soon as you switch it.
- When a digital employee has no permission mode selected, Codex runs in Full access and Claude Code in Bypass permissions, so goals no longer stop for approvals. To require approvals, choose another mode under the digital employee's Permissions.
- The lead knows collaborators only by name and responsibilities. It no longer sees their Agent, model, permission mode or token usage. The digital employee editor drops the Role field and explains what Responsibilities is for. Instructions and resources come first, and the Agent, model and permissions are grouped together.

## 2026.11.18

- 「通用」设置支持切换简体中文、英文和应用外观。导入本机 Skill 时，可以从一个父目录预览并勾选多个 Skill；达到扫描上限时会提示如何找到其余条目。
- 工作区可以为同一角色配置多个并发槽位。负责人能收到看板进展和协作者的拆分请求，返工或后续任务可以续接原会话。
- 独立验证会给出通过、拒绝或证据不足的判定。修复任务直接收到未通过项和证据。服务重启后，先前受阻的下游任务继续等待。
- Token 参考值只用于观察用量，超过后所有 Agent 和任务照常运行。续接会话按新增费用计入各任务，单轮异常结束时可从本地 Claude 会话日志补齐该轮用量。
- 长任务没有新事件时，收件箱复用已有条目。达到费用上限的 Run 随后交付时，预算待办自动撤回，转为等待验收。

- General settings now let you switch between English and Simplified Chinese and choose the app’s appearance. You can preview and select several local skills from one folder. If the scan reaches a limit, the preview tells you how to find the remaining items.
- Workspace roles can run several subtasks concurrently. The lead receives board updates and requests to split work. Follow-up tasks and retries can resume an earlier session.
- Independent checks report an approved, rejected or inconclusive verdict. Repair tasks receive the failed checks and their evidence. Downstream work held by a verdict stays on hold after a service restart.
- The token reference is for reporting only. Agents and tasks keep running after usage crosses it. When a session is reused, each task is charged only the increase in reported cost. If a turn ends abnormally, Janus can recover its usage from the local Claude session log.
- The inbox reuses existing entries when long runs have no new events. When a run delivers after reaching its cost limit, its budget request clears and the delivery is ready for acceptance.

## 2026.11.17.5

- 设置新增「macOS 权限」，集中处理完整磁盘访问、应用程序管理和屏幕录制。点击「一键开始授权」按顺序打开授权入口，每一步都可以跳过。
- 首次安装和升级后自动显示权限清单，同一安装版本不重复提醒。升级后清除旧的手动确认，回到应用时刷新可查询的权限状态。
- 完整磁盘访问和应用程序管理由用户在系统设置中确认，显示「已手动确认」；屏幕录制使用系统检查与申请，只有检测到授权才显示「已授权」。独立服务或其他终端启动的 Agent 可能需要单独授权。

- Settings now includes a macOS permissions page for Full Disk Access, App Management and Screen Recording. One button starts the authorization steps in order, and each step can be skipped.
- A permission checklist appears after installation or an update, without repeated reminders for the same installed build. Updates clear earlier manual confirmations, and returning to the app refreshes permissions that macOS allows Janus to check.
- Full Disk Access and App Management require confirmation in System Settings and are labelled as manually confirmed. Screen Recording uses the system check and request APIs and is labelled as authorized only when detected. Agents started by a separate service or another terminal may need their own authorization.

## 2026.11.17.4

- 任务可以删除了：运行页工具栏右侧的删除按钮、侧栏任务记录的右键菜单和「任务」菜单都能删除一个任务。删除前会说明它的事件记录、子任务、产物和日志会从这台电脑删除且不可恢复，工作目录中的文件不受影响；勾选「不再提醒」后，之后点删除直接执行。正在进行的任务要先停止才能删除。
- 会话往上翻看时停在原处：新回复不再把对话拉回底部，底部出现「回到最新」（有新内容时写「有新消息」），点一下回到最新并继续跟随；停在底部时照常跟随新回复，发送消息后也回到最新。
- 长任务的页面更流畅：会话和子任务的执行轨迹只构建看得到的行，Agent 回复的 Markdown、审计日志和事件文字的处理不再每次重新编译正则，运行中任务的状态推导移到后台线程，侧栏和收件箱没有变化时不再每 5 秒重绘。在 4,800 条事件的任务上，负责人对话每收到一条新事件的重绘从约 43 毫秒降到约 11 毫秒。
- 最低系统要求提高到 macOS 15：Janus 只支持最近三个 macOS 大版本（15、26、27），仍在 macOS 14 上的 Janus 不会收到这次更新。

- Runs can now be deleted: the trash button at the right of a run's toolbar, the context menu on a task record in the sidebar, and the Task menu all remove one. A warning says its event log, subtasks, artifacts and logs will be removed from this Mac and cannot be recovered, while files in the working directory stay put; ticking "不再提醒" (don't ask again) makes later deletions immediate. A run that is still working must be stopped first.
- Scrolling up in a conversation now keeps your place: new replies no longer pull it back to the bottom. A "回到最新" (back to latest) button appears, reading "有新消息" (new messages) once something has arrived; it takes you back and resumes following. At the bottom, the conversation follows new replies as before, and sending a message also brings you to the latest.
- Long runs are smoother: conversations and a task's trace build only the rows in view, agent replies in Markdown, the audit log and event text no longer recompile their patterns on every draw, a live run's state is derived off the main thread, and the sidebar and inbox no longer redraw every five seconds when nothing changed. On a run with 4,800 events, redrawing the lead's conversation for each new event went from about 43 ms to about 11 ms.
- Janus now requires macOS 15 or later, supporting the three latest major versions of macOS (15, 26 and 27); copies running on macOS 14 are not offered this update.

## 2026.11.17.3

- 工作台（任务页）默认只保留一屏能回答「到哪一步、要不要我处理」的内容：运行卡片压成一行摘要（子任务进度、token、费用、耗时、上下文），用量明细、交付里的逐条验收和进展时间线都改为页内展开；交付的运行级风险默认列出前 5 条，验收操作移到首屏；进展移到负责人对话上方并默认收起。
- 检查器按状态选择默认页签（还没有子任务时显示团队），对话和检查器的高度随窗口变化。
- 子任务页去掉「子任务用量账单」卡片，右列直接显示验收标准。

- The workbench (a run's page) now opens on what needs attention: the run card is a single summary line (task progress, tokens, cost, time, context), and the usage ledger, per-task acceptance evidence and the progress timeline unfold in place. Run-level risks show the first five with an expand-all link, the accept action sits on the first screen, and the timeline moves above the lead's conversation, folded by default.
- The inspector picks its first tab by state (team before any task exists), and the conversation and inspector heights follow the window.
- The task page no longer shows the task usage bill; its right column starts with the acceptance criteria.

## 2026.11.17.2

- 审阅 diff 时显示未跟踪新文件的内容，中文文件名也能正确对应：新任务的 diff 直接记录文件内容和新增行，旧 diff 从工作目录读取当前内容并标注；可以直接对新文件的行写审阅意见。

- The diff review now shows the content of new files Git does not track, including files with non-ASCII names: new tasks record the content and added lines in the diff, and older diffs are filled from the working tree and labelled. Lines in a new file can be reviewed and commented on.

## 2026.11.17

- 修复目标输入框和会话输入框在使用中文等输入法时偶尔吞字：输入法还在组合候选时，界面刷新不会再覆盖输入框内容，正在输入的字不会消失。
- 外部填入文字（例如点示例目标）时保留光标位置；输入法组合期间按 ⌘↩ 不再发送尚未上屏的内容。

- Fixed characters sometimes disappearing while typing with an input method in the goal and message boxes: a redraw no longer overwrites the field while a composition is in progress.
- When text is filled in from elsewhere (for example by choosing an example goal), the caret stays where it was; ⌘↩ no longer sends a draft while the input method is still composing.

## 2026.11.16.5

- 深色主题换成 Zed One Dark 的中性灰和柔和蓝：内容区 `#1e2227`、卡片 `#282c34`、侧栏 `#21252b`、品牌蓝 `#61afef`，状态色同步调整；浅色主题不变。

- The dark theme now uses the neutral grays and soft blue of Zed One Dark: content area `#1e2227`, cards `#282c34`, sidebar `#21252b`, brand blue `#61afef`, with the status colors adjusted to match. The light theme is unchanged.

## 2026.11.16.4

- 修复应用内更新后停在「正在重新打开…」：退出被窗口上的面板挡住时，5 秒后结束进程，安装脚本可以继续替换并重新打开。
- 更新不再弹任何浮层：版本、是否最新、发布说明、下载大小和「更新到 …」都在设置 › 更新 页面里展示，点击直接开始；侧栏和菜单里的「可以更新 / 检查更新」也打开这一页。

- Fixed in-app updates stopping at "Reopening…": when a panel on the window blocks quitting, the process now ends after 5 seconds so the install script can replace the app and reopen it.
- Updates no longer show any overlay. The version, whether it is the latest, the release notes, the download size, and "Update to …" are all on the Settings › Updates page, and clicking starts the update directly. "Update available" in the sidebar and "Check for Updates" in the menu open the same page.

## 2026.11.16.3

- 工作区里的负责人只能从开启了「可以担任负责人」并绑定 Claude Code 或 Codex 的数字员工中选择；其他数字员工不再显示负责人选项，保存和启动任务时也会校验。
- 应用图标换成 Janus 的双面门；菜单栏显示同一个标记，有待你处理的事项时在旁边显示数量。
- 界面把 Run 叫「任务」，Run 里的执行项叫「子任务」，空状态、侧栏、账单、事件说明和内置提示词都统一了叫法。

- A workspace's lead can only be chosen from digital employees that have "can lead" turned on and are bound to Claude Code or Codex. Other digital employees no longer show the lead option, and the rule is also checked when saving a workspace and when starting a task.
- The app icon is now Janus's two-faced gate. The menu bar shows the same mark, with a count beside it when something is waiting for you.
- The interface calls a run a "task" and the items inside a run "subtasks". Empty states, the sidebar, bills, event descriptions, and the built-in prompts all use these names.

## 2026.11.16.2

- 设置改为 ACP、Agent、资源库、数字员工、用量账单、更新六个分类，ACP 排在第一位。
- 角色统一显示为「数字员工」，主控统一显示为「负责人」；按钮、提示、收件箱、工作区设置和内置提示词都使用新名字。
- 资源库的类型改用 Skill 和 MCP，和 Agent 生态里的叫法一致。
- 新增「更新」面板：显示当前版本、是否已是最新、上次检查时间，可以查看更新内容或立即检查更新。

- Settings now has six sections: ACP, Agent, Resource Library, Digital Employees, Usage Bill, and Updates, with ACP first.
- Roles are shown as "digital employees" and the controller as the "lead". Buttons, hints, the inbox, workspace settings, and the built-in prompts use the new names.
- Resource library types are now called Skill and MCP, matching the names used across the agent ecosystem.
- Added the Updates panel: it shows the current version, whether it is the latest, and when it was last checked, and lets you read the release notes or check for updates right away.
