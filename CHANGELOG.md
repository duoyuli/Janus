# 更新记录 / Changelog

Janus 的版本按日期命名，新版本写在最上面，每节列出这一版的变化：用户能看到的变化，以及缺陷修复、性能优化和稳定性改进（包括内部的），先中文后英文。发布前先在 `CHANGELOG.md` 写下这一版的 change log：`Release` 工作流把对应小节作为 GitHub Release 的说明，找不到「`## <版本>`」这一节时发布失败。格式和发布步骤见 [CI 与发布](docs/operations/20261101-ci_release.md#release)。

Janus versions are named by date, newest first. Each section lists the changes of that version: what people can see, and bug fixes, performance and stability improvements (internal ones included), in Chinese and then in English. Write the section for a version in `CHANGELOG.md` before releasing it: the `Release` workflow uses that section as the GitHub Release notes and fails when there is no `## <version>` section. See [CI and releases](docs/operations/20261101-ci_release.md#release) for the format and the release steps.

## 2026.11.23.10

- 折叠行更好认、更好点：以前只有标题前那个小三角能点，很难点中，也不像按钮。现在箭头放在一个浅灰小底板上，展开时转向下；整行标题都能点，鼠标移上去整行高亮。收件箱的「稍后处理」「进行中」、对话里的思考和工具调用、执行轨迹、系统规则、高级设置，以及计划、资源库、用量明细、逐条验收的展开都统一成这样。

- Fold rows are easier to spot and to click: only the small triangle before the title used to respond, which was hard to hit and did not look like a button. The chevron now sits on a small gray plate and turns down when open; the whole title row toggles and lights up under the pointer. The inbox's Later and Working sections, thoughts and tool calls in conversations, the execution trace, system rules and advanced settings, as well as the plan, the library, usage details and per-task acceptance, all work this way now.

## 2026.11.23.9

- 修复任务交付后收件箱出现两条：负责人交付时它的这一轮也结束了，以前除了「已交付，等你验收」还会多一条「这一轮结束，等你回话」，指向同一个任务。现在只剩「等你验收」一条，任务页的「待处理」也一样。

- Fixed a delivered run showing up twice in the inbox: a delivery also ends the lead's turn, so besides "Delivery awaits your review" there was a "finished its turn and is waiting for you" item for the same run. Only the review item is left now, on the run page's pending list too.

## 2026.11.23.8

- 收件箱的每一项都可以「忽略」：不再提醒，从收件箱和侧栏的数量里去掉，不回答任何请求，任务本身不变（等验收的仍是已交付，在任务记录和任务页还能处理）。以前只能点进任务去处理或验收，只有已中断和失败的任务能「标记已处理」，现在统一叫「忽略」。审批、表单这类让 Agent 停着的事项也能忽略，悬停提示会写明对方会一直等着。升级后，以前标记过已处理、仍在等你的条目会重新出现一次。

- Every inbox item can now be ignored: it stops reminding you and leaves the inbox and the sidebar counts, without answering anything or changing the run (a delivery waiting for acceptance stays delivered and can still be handled from history or the run page). Before, you had to open the run to handle or accept it, and only stopped runs could be marked as handled; that is now Ignore too. Approvals, forms and other items that keep an agent waiting can be ignored as well, and their tooltip says the agent keeps waiting. After the update, items you had marked as handled that still wait on you show up once more.

## 2026.11.23.7

- 自动更新：「设置 › 通用 › 更新」新增「自动更新」开关，默认打开。打开时，Janus 一发现新版本就在后台下载并校验（和手动更新一样只下载有变化的部分、核对 SHA-256、要求和当前版本同一个签名者），然后等没有任务、对话和值班在进行时（每 30 秒看一次，连续两次都空闲）自动重开换上新版本。等待期间侧栏显示「Janus X 已下载，空闲时更新」，更新一节可以点「现在更新」立即换上。关掉开关后和以前一样，只提醒有新版本，由你点更新。

- Automatic updates: Settings › General › Updates has a new Automatic Updates switch, on by default. With it on, Janus downloads and verifies a new version in the background as soon as it finds one (as a manual update does: only the parts that changed, checked against their SHA-256 and required to come from the same signer as the running version), then reopens on the new version once no run, conversation or Loop is working (checked every 30 seconds, idle twice in a row). While it waits, the sidebar says the version is downloaded and will be installed when idle, and Update Now in the updates section installs it at once. With the switch off, Janus only tells you about new versions and you choose when to update, as before.

## 2026.11.23.6

- 结束 Agent 会话和资源库 MCP 服务器时，改为直接向它们的进程组发信号，不再调用 `kill` 命令。在 Linux 上（Debian 的 `kill`），原来的命令会把信号发给所有能发到的进程；macOS 不受影响，这次是在把测试挪到 Linux 时发现的。
- 发布和 CI 提速：发布时不再重复跑 CI 已经跑过的测试，改为打包和 CI 同时进行、CI 通过后才发布；打包时服务和应用同时编译；CI 的应用只编译一次，编排器的测试改在 Linux 上跑，不再排队等 macOS 运行器。

- Ending an agent's session or a library MCP server now signals its process group directly instead of running the `kill` command. On Linux (Debian's `kill`) the old command sent the signal to every process it could reach; macOS was not affected. It came up while moving the tests to Linux.
- Faster releases and CI: a release no longer reruns the tests CI already ran; it packages while CI tests the same commit and publishes once CI passes. Packaging builds the server and the app side by side. CI builds the app once, and runs the orchestrator's tests on Linux instead of waiting for a macOS runner.

## 2026.11.23.5

- Git 仓库上的任务，结果在交付时就合并回你的分支，不用再等你点验收：每次交付，Janus 都把任务的改动提交，你的分支前进过就先把它们变基到最新的分支上，再快进你的分支，你在自己的目录里马上能看到。你对负责人说「提交一下」「合并到 main」，它把工作做完、交付，代码就到了；验收只表示你认可了这次结果。交付卡片写明合并到了哪个提交，决策里记一条「交付时合并到你的分支」。值班派出的任务交付时也一样合并。
- 交付时和分支上的新提交冲突，你的目录和分支都不动：Janus 照常交付，把冲突的文件和解决步骤交给负责人，它在任务的副本里解决、跑完检查，再次交付时合并。同一个冲突只自动交给它一次，没解决时交付说明仍然冲突，你可以继续和它处理，或者点验收让它再试一次。你目录里没提交的改动挡住时，交付说明原因，你处理后点验收再合并。
- 修复验收之后负责人按你的要求提交的改动不再交付：以前只看有没有未提交的文件，一个提交让副本变干净，任务一直停在「已验收」，这次提交永远到不了你的分支。现在有未提交的改动，或者有上次合并之后的新提交，都会再次交付并合并。
- 修复验收之后的一轮被服务重启打断时，重启会删掉任务的副本，这一轮的提交跟着丢失。现在副本保留，任务再次交付并合并。
- 负责人解决冲突后分支又前进了，下一次合并不再把分支自己的提交重放一遍、产生和任务无关的冲突，只重放任务自己的提交。负责人的变基或合并做到一半时，Janus 不再把冲突标记提交上去，而是交给负责人做完或中止。
- 要推送或发布时，负责人先在任务的副本里把工作变基到最新的分支上再推送，交付时你的分支快进到同一个提交，本地和远端不再分叉。
- 任务页「改动」里整个任务的 diff 不再混进分支上别人的提交，只含下一次交付要合并的改动。

- On a Git repository, a run's result now goes back to your branch when it is delivered, not when you accept it: each delivery commits the run's changes, replays them onto your branch if it moved, and fast-forwards your branch, so you see them in your own checkout at once. Ask the lead to commit or to merge into main, and the work is on the branch once it is delivered; accepting only says you approve the result. The delivery card says which commit it was merged as, and the decisions list a "Merged into your branch on delivery" entry. Runs a Loop dispatches merge on delivery too.
- When a delivery conflicts with new commits on your branch, your checkout and branch stay as they are: Janus delivers as usual and hands the conflicting files and the steps to resolve them to the lead, which resolves them in the run's copy, runs its checks and delivers again, merging then. The same conflict goes to the lead on its own only once; if it is still there, the delivery says so, and you can keep working it out with the lead or accept to have it try again. Uncommitted changes of yours in the way are named in the delivery; sort them out and accept to merge.
- Fixed changes the lead committed at your request after you accepted a run never being delivered: only uncommitted files counted, so a commit left the copy clean, the run stayed accepted and the commit never reached your branch. Uncommitted changes or commits made after the last merge now deliver and merge the run again.
- Fixed a restart during a turn after acceptance removing the run's copy, and the turn's commits with it. The copy now stays and the run is delivered and merged again.
- After the lead resolves a conflict and the branch moves again, the next merge no longer replays the branch's own commits and hits conflicts unrelated to the run; only the run's commits are replayed. A rebase or merge the lead left half done is no longer committed with its conflict markers; it goes back to the lead to finish or abort.
- To push or release, the lead first rebases the work onto the latest branch in the run's copy and then pushes, and the delivery fast-forwards your branch to that same commit, so local and remote no longer diverge.
- The whole-run diff under Changes no longer includes other people's commits on the branch, only what the next delivery merges.

## 2026.11.23.4

- 团队记忆改为按需检索：笔记不再塞进每个任务的说明里，Agent 只知道这个工作区有多少条，开始工作前和遇到不熟悉的事时自己检索。检索也改了：以前要每个词都出现才算找到，换个说法、中文不分词就搜不到；现在按标题、摘要、正文的命中程度排序，返回最相关的几条，中文换个说法也能搜到，不带关键词时列出全部笔记。被取消的任务写的笔记，以前按关键词还能搜到，现在搜不到了。
- 工作区首页重新排了一下：团队记忆做成一张卡片，放在右栏共享资料下面，两者都是这个工作区的任务知道的东西：共享资料由你写、每个任务都带上，团队记忆由 Agent 写、需要时检索。目标输入框回到名称正下方，不再被一个空的团队记忆区块压在下面。团队记忆每条笔记一行（类型和标题），悬停看摘要和是谁写的，点开可以改；没有笔记时只有一行说明，不再挂着点不了的「现在整理」。
- 共享资料和团队记忆的标题旁各加了一个「?」，点开说明它们怎么工作：谁来写、怎样交给 Agent、冲突时以谁为准、怎样积累和整理。
- 修复一个时快时慢的后端测试（整理团队记忆的会话还没记下说明就去读），它让上一次打包失败。

- Team memory is now searched when needed: notes are no longer put in every run's instructions; agents learn how many there are and search them before they start and whenever they meet something unfamiliar. The search is better too: it used to need every word to appear, so other wording or Chinese without spaces found nothing; it now ranks notes by matches in the title, the summary and the body and returns the best few, finds Chinese notes in other words, and lists every note when given no words. Notes from cancelled runs, which a search used to find, are no longer found.
- The workspace home is rearranged: team memory is now a card in the right column under the shared context, both being what the workspace's runs know: you write the shared context and every run gets it; agents write team memory and search it when they need it. The goal composer is back right under the name instead of below an empty team memory section. Each note is one line (kind and title); hover for its summary and who wrote it, click to edit. With no notes the card has one line of text and no Consolidate Now button that cannot be clicked.
- The shared context and team memory each have a "?" beside their title that explains how they work: who writes them, how agents get them, which one wins when they disagree, and how they grow and get consolidated.
- Fixed a backend test that failed now and then (it read the consolidation session's instructions before they were recorded) and made the last packaging fail.

## 2026.11.23.3

- 修复目标里明确要求「提交并发布」时，负责人仍回复「规则禁止我直接 commit、push 或 deploy」，跑完验证就停下、改动留着没提交。原因是 Janus 写给负责人和协作者的固定约定里有一句无条件的「不要 commit、push、部署」。现在改为：你在目标或回答里要求时，Agent 照做，并且只做到你要求的程度（只说提交就不推送），不再反复问你；没有要求时和以前一样，改动保持未提交，验收时由 Janus 提交。任务在 worktree 里自己做的提交，验收时会和剩下的改动一起合并回你的分支；负责人也知道怎样从 worktree 推送到你指定的分支。Janus 自己仍然从不推送。
- 团队记忆移到工作区首页上部，放在目标输入框之前，不再夹在值班和任务记录之间。
- 修复任务页右边「改动」显示 0：负责人自己动手改的文件不在任何子任务的 diff 里，没派子任务时这里一直是空的。现在负责人每一轮结束后，Janus 保存一份「整个任务」相对开始时的改动（就是验收时会合并回去的内容），放在「改动」的最上面和交付物的第一个，可以打开审阅，意见交给负责人。
- 验收时分支有了新提交、和任务的改动冲突，不再让你在自己的目录里 git merge（那样会留下没完成的合并）。现在你的目录和分支都不动，Janus 把冲突交给负责人：它在任务自己的副本里把提交变基到最新的分支上、解决冲突、跑检查，再告诉你哪些文件冲突、怎么合的，然后再次交付，你再点验收就能直接合并。你目录里有未提交的改动挡着、分支被删除这类情况仍由你处理。

- Fixed a lead that, with a goal asking it to commit and release, replied that rules forbade it to commit, push or deploy, stopped after verifying, and left the changes uncommitted. Janus's fixed instructions to the lead and collaborators said "do not commit, push, or deploy" with no exception. Agents now do these when you ask for them in the goal or an answer, and only as far as you asked (a request to commit is not one to push), without asking you again; when you don't, changes stay uncommitted as before and Janus commits them when you accept. Commits a run makes in its worktree come back to your branch with the rest when you accept, and the lead is told how to push from the worktree to the branch you mean. Janus itself still never pushes.
- Team memory now appears near the top of the workspace home, before the goal composer, instead of between Loops and task history.
- Fixed Changes on the run page showing 0: files the lead edited itself are in no subtask's diff, so a run without subtasks always showed none. After each of the lead's turns Janus now keeps the whole run's changes since it started (what accepting brings back), at the top of Changes and first among the deliverables, to review with comments that go to the lead.
- When accepting meets new commits on the branch that conflict with the run, you are no longer told to git merge in your own checkout (which left a merge half done). Your checkout and branch stay as they are, and Janus hands the conflict to the lead: it replays the run's commits onto the latest branch in the run's own copy, resolves the conflict, runs checks and tells you which files conflicted and how it merged them, then delivers again so the next accept merges straight away. Uncommitted changes of yours in the way, a deleted branch and the like are still yours to sort out.

## 2026.11.23.2

- 新增团队记忆（#4）：工作区里的 Agent 在任务中记下之后的任务还用得上的事（怎么跑测试、踩过的坑、为什么这样做），之后这个工作区的每个任务都拿到这些笔记的索引，需要时读全文。负责人和协作者都能写，每条记着是谁在哪个任务里写的；笔记不跨工作区，像密钥、令牌的内容不收，被取消的任务写的不给之后的任务。工作区页新增「团队记忆」一节：可以改、删笔记，或把一条加进共享资料，让它成为团队必须遵守的规则。
- 团队记忆会自动整理：攒够 5 个交付的任务、并且距上次整理超过一天时，负责人开一个只能动笔记的会话，合并重复的、改正被推翻的、删掉过时的；也可以在工作区页点「现在整理」。整理完后收件箱出现「团队记忆整理好了」，打开能看到改了什么。
- Janus 启动的 Claude Code 不再使用它自带的 auto memory：那份记忆按工作目录保存，而每个任务和对话都有自己的目录，下一个任务读不到；现在任务里学到的都放进团队记忆。你自己在 Janus 之外用 Claude Code 不受影响。

- New team memory (#4): agents in a workspace note what later runs will need (how to run the tests, a trap they fell into, why something is the way it is), and every later run in that workspace gets the index of these notes and reads one in full when it needs to. The lead and collaborators both write notes, each saying who wrote it in which run; notes stay within their workspace, credential-like text is refused, and notes from cancelled runs are not handed on. The workspace page has a new Team Memory section where you can edit or delete a note, or add it to the shared context to make it a rule for the team.
- Team memory is consolidated on its own: after 5 delivered runs and at least a day since the last time, the lead opens a session that can only touch the notes, to merge duplicates, correct what later runs disproved and remove what is out of date; you can also click Consolidate Now on the workspace page. When it is done, "Team memory consolidated" lands in your inbox with what changed.
- Claude Code started by Janus no longer uses its own auto memory: it was kept per working directory, and every run and chat has its own, so the next run never read it. What a run learns now goes to the team memory. Using Claude Code yourself outside Janus is unaffected.

## 2026.11.23

- 修复值班表单里的时间输入：在小时上慢慢打「18」（两个数字之间停顿约一秒）会变成 08，要再打一次才对（#3）。系统的时间输入框把这样的停顿当成小时已经输完。现在时间是「时」「分」两个菜单，和频率、星期等菜单在同一行：小时 00–23，分钟每 5 分钟一档，编辑已有值班时原来不在档上的分钟也保留。

- Fixed the time in the Loop form: typing 18 slowly into the hour (a pause of about a second between the digits) set 08, and it took a second try (#3). The system's time field takes such a pause as the end of the hour. The time is now two menus, hour and minute, in the same row as the frequency and weekday menus: hours 00–23 and minutes every five, keeping an edited Loop's minute when it falls between them.

## 2026.11.22.5

- 修复 Codex 负责人显示 Full access，用 `/goal` 推进目标时却仍在沙箱里：读钥匙串、写工作目录外的文件、启动后台进程都会弹审批，任务停下来等你。Codex 的适配器只把会话模式带进普通提示，`/goal` 的各轮用的是线程自己的默认设置（需要时询问、只能写工作目录）。现在 Janus 启动 Codex 时按数字员工的权限模式一并设好线程的审批和沙箱，Full access 在目标里也真正不受限；选了其他模式的照那个模式执行。Claude Code 不受影响。

- Fixed a Codex lead showing Full access yet running its `/goal` inside a sandbox: reading the keychain, writing outside the workspace or starting a background process asked for approval and held the run until you answered. Codex's adapter applies the session mode only to ordinary prompts, while `/goal` turns use the thread's own defaults (ask when needed, write only the workspace). Janus now sets the thread's approval and sandbox from the digital employee's permission mode when it starts Codex, so Full access is unrestricted in goals too, and other modes apply as chosen. Claude Code is unaffected.

## 2026.11.22.4

- 值班派出的任务从头到尾自己闭环：自己做、自己验收、不合格自己返工。任务交付、失败或被你取消时立即叫醒值班，开工的提示里写着结束的任务和结果（交付摘要、失败原因，或者「是你取消的，不要再派」），值班不用再隔一会儿醒来查一次。值班的工作方式里写明，任务没结束时不查它的半成品，也不为同一件事再派一个任务（换个 key 也不行）。暂停的值班不被叫醒，下次开工时一并告诉它。已有的值班要删掉重建才会用上新的工作方式（叫醒是服务端的，升级后就生效）。
- 新增值班工具 `note_dispatch`：值班知道、而它派出的任务不知道的事，可以转给那个任务的负责人，消息里写明来自哪个值班；只能转给它自己派出、还在进行中的任务。
- 修复值班在两次开工之间闲置一会儿后，收件箱里出现「已中断，可以从中断处继续」，看起来像值班出了问题要你处理：那只是它的会话关掉省资源，下次开工会重新打开。对话也一样不会出现。
- 「挡住的路径」写明文件夹和单个文件都能挡（点「+」在选择窗口里选文件即可）。

- A run a Loop dispatches sees its goal through on its own: it does the work, checks it and redoes what falls short. When it is delivered, fails or you cancel it, the Loop wakes right away, and the wake's prompt names the run and how it ended (its summary, why it failed, or that you cancelled it and it should not be dispatched again), so the Loop no longer wakes every so often to look. The Loop's instructions say not to check a run's work in progress and not to dispatch another run about the same thing, under another key either. A paused Loop is not woken and hears of it at its next wake. Existing Loops pick up the new instructions once recreated; the wake on a run's end works as soon as you upgrade.
- New Loop tool `note_dispatch`: what the Loop knows and a run it dispatched does not goes to that run's lead, marked as coming from the Loop; only for runs it dispatched that are still working.
- Fixed the inbox showing a Loop as "interrupted, continue from where it stopped" after it sat idle between wakes, as if it needed you: its session had only closed to save resources and opens again at the next wake. The same holds for chats.
- Hidden Paths says that both folders and single files can be hidden (pick a file in the panel behind +).

## 2026.11.22.3

- 本机环境改为镜像你的用户目录：Agent 和 MCP 服务器的用户目录里，你本机的每一项都链接到原位置，登录、钥匙串、工具链和配置直接能用，以后装的新工具也一样，不用再一项项打开。只有「挡住的路径」里列出的看不到：默认是常见 Agent 自己的配置（`~/.claude`、`~/.codex`、`~/.pi`、`~/.agents` 等，数字员工的指令、Skill 和 MCP 仍只由 Janus 给）和一些高危数据（`~/.aws`、`~/.gnupg`、`~/.kube`、浏览器、邮件和信息等），清单只列你这台 Mac 上有的路径；你可以点 × 移除、恢复默认，或者点标题旁的「+」在系统的选择窗口里直接选；Janus 自己的数据目录一直挡着。升级时原来逐项打开的记录换成这份默认清单。
- 修复飞书 CLI 打开后，Agent 里 `--as user` 仍报 `token_missing` 的问题：它的用户令牌在 `~/Library/Application Support/lark-cli`，原来只共享了 `~/.lark-cli`。
- 新增「试一条命令」：在设置里输入一条命令，同时在本机和 Agent 的环境里各跑一次，并排看结果，就知道某个工具在 Agent 里能不能用。
- Agent 能用你的 ssh-agent（传入 `SSH_AUTH_SOCK`）；界面里的「家目录」改叫「用户目录」。
- 界面文案整体打磨：同一件事在每个页面只用一个说法（例如要你动手的事统一叫「待处理」，单个事项写「等你回答 / 等你审批 / 等你拍板」；Agent 配置统一叫「检查」；侧栏的两组改叫「团队」「对话」），说明文字压到一句左右，不再出现「编排器」「墙钟」这类内部用词；任务和 Agent 的状态换成和值班一样的口吻（干活中、摸鱼中、待开工、已派活、等上游、思考中、跑测试、出错了），让排队的任务立刻开始的按钮叫「马上开工」；英文界面一起改写。
- 默认数字员工的职责描述更短。你没改过的会自动换成新文本；改过的保留，编辑器里提示有新版本。
- 工作区首页更好区分：「待处理」每行开头是它在等什么的图标（提问、审批、等你验收、已中断……），值班有自己的紫色身份标（值班页标题处也是它），值班派出的任务改为在标题后面标出来自哪个值班；团队摘要去掉了和「工作区操作」菜单重复的「管理团队…」。
- 标题旁的添加、编辑、移除改成图标按钮：共享资料、新建值班、挡住的路径和 diff 审阅里的意见；新建值班的表单把「它自己不改文件……」的说明放到标题下面，写提示之前先看到。
- 修复英文界面里，负责人停下而目标还没完成时，Janus 发出的「目标确认」提示被标成通用的「Janus Prompt」。
- 修复新建对话后马上删除时报「Directory not empty」、删不掉的问题：对话的会话还在打开，往正在删除的日志目录里写了文件。现在删除会先让还在打开的会话和资源库服务器停下，等正在建的日志建完再删目录。

- Host Environment now mirrors your home folder: in every Agent's and MCP server's home folder, each item of yours is linked to where it is, so sign-ins, the keychain, toolchains and settings just work, and so will tools you install later, without switching anything on. Only the paths under Hidden Paths stay out of sight: by default common Agents' own settings (`~/.claude`, `~/.codex`, `~/.pi`, `~/.agents` and more; digital employees still get instructions, skills and MCP from Janus only) and some sensitive data (`~/.aws`, `~/.gnupg`, `~/.kube`, browsers, mail and messages and more). The list shows only paths on your Mac; remove any with its ×, restore the defaults, or pick more in the system's open panel with the + beside the heading; Janus's own data folder is always hidden. On upgrade, the items you switched on one by one give way to this default list.
- Fixed Agents getting `token_missing` from `lark-cli --as user` after the Lark CLI was switched on: its user tokens are in `~/Library/Application Support/lark-cli`, and only `~/.lark-cli` was shared.
- New Try a Command: type a command in Settings and it runs both for you and in an Agent's environment, side by side, so you see whether a tool works for Agents.
- Agents can use your ssh-agent (`SSH_AUTH_SOCK` is passed on); the Chinese interface calls the home folder 「用户目录」.
- The app's wording got a full pass: each thing has one name on every page (what needs you is always Pending; an Agent's setup test is always a check; the Chinese sidebar groups are now 团队 and 对话), notes are cut to about a sentence, and internal words such as orchestrator and wall time are gone. Task and Agent states take the Loop's easygoing tone (Working, Idle, Ready, Assigned, Waiting on upstream, Thinking, Running tests, Error), and the button that starts a queued run right away is Start Now. The English text was rewritten to match.
- The default digital employees have shorter responsibilities. Ones you never edited move to the new text; edited ones stay and the editor offers the new version.
- The workspace page tells things apart: each Pending row starts with an icon for what it waits on (question, approval, ready to accept, interrupted…), a Loop has its own violet badge (also on its page), and a run a Loop dispatched names that Loop in a tag after its title. The team line drops Manage Team…, which the workspace menu already offers.
- Add, edit and remove beside a heading are icon buttons: shared context, new Loop, hidden paths and diff comments. The new-Loop form explains how a Loop works under its title, before you write the prompt.
- Fixed the English interface labelling Janus's goal check (sent when the lead stops while the goal is still open) as the generic Janus Prompt.
- Fixed deleting a chat right after creating it failing with "Directory not empty": its session was still opening and wrote into the log folder being removed. Deleting now stops a session or library server still starting, and waits for a log being made, before removing the folders.

## 2026.11.22.2

- 限制同时开着的 Agent 进程：每个会话都是常驻的适配器和 Agent 进程（一个闲着的 Codex 约占 250 MB 内存），之前已交付任务的负责人、打开过的每个对话、两次开工之间的值班都一直开着，直到服务重启。现在会话闲置 10 分钟、它所在的任务或值班没有别的事需要它时，就关掉进程，下次发消息或开工时自动重新打开；所有 Agent 进程一起最多 12 个（可用 `JANUS_MAX_AGENT_SESSIONS` 或 `config.toml` 的 `max_agent_sessions` 调整，不少于同时在做的任务数加一），满了时先关掉闲得最久的会话，新来的显示「等空位」，按你在等的、按时间表来的、协作者的先后拿到位置。被关掉的对话在列表里显示「已关闭」，退出 Janus 时也不再算作进行中。
- 值班开工时发出的消息只有你写的提示（你回答过它的问题时带上回答），不再拼上开工次数、时间、时间表、工作目录和派出任务的变化；这些规则写在值班的工作方式里，派出任务的情况它用工具自己查。
- 「本机环境」并进设置的「通用 › 权限」，和 macOS 权限放在一起；读不出来时写明原因并可以重试，不再一直转圈。

- Open agent processes are now limited. Each session is an adapter and an agent that stay in memory (an idle Codex is about 250 MB), and before, delivered runs' leads, every chat ever opened and Loops between wakes stayed open until a restart. A session idle for 10 minutes whose run or Loop needs nothing of it now closes its process, and the next message or wake opens it again; all agent processes together are limited to 12 (`JANUS_MAX_AGENT_SESSIONS` or `max_agent_sessions` in `config.toml`, never fewer than running goal runs plus one). When all are taken the idlest session closes, newcomers show Waiting for a Seat, and seats go to what you are waiting for first, then to schedules, then to collaborators. A closed chat reads as closed in the list and no longer counts as running when you quit Janus.
- A Loop's wake now sends only your prompt (with your answers to its questions when there are any) instead of a header with the wake number, time, schedule, working directory and dispatch changes; its working instructions hold the rules, and it looks up its dispatches with its tools.
- Host Environment moved into Settings › General › Permissions, next to the macOS permissions; when it cannot be read it says why and offers to retry instead of spinning.

## 2026.11.22.1

- 新增设置里的「本机环境」：Agent 和 MCP 服务器都在自己独立的家目录里工作，之前用不了你本机的 GitHub CLI、飞书 CLI、钥匙串里的登录，也找不到 rustup、nvm 这类工具链。现在可以逐项打开要共享的内容，它们会链接进每个 Agent 和 MCP 服务器的家目录，用的就是你本机的同一份登录，改动立即生效，正在运行的会话也一样。页面列出 Janus 认识、你这台 Mac 上有的工具（登录与账号、工具链、macOS 钥匙串），以及家目录里的其他配置，也可以手填路径；默认一项都不共享。打开一个登录存在钥匙串里的工具（GitHub CLI、飞书 CLI、Git）时，钥匙串会一起打开。Agent 自己的配置（`~/.codex`、`~/.claude`、`~/.agents` 等）不能共享，数字员工的指令、Skill 和 MCP 仍只由 Janus 给。
- 工作区首页重新排版，工作区的一切都在这一页，侧栏只列工作区：值班不再缩进在侧栏里，工作区的数字算上它的值班。首页从上到下是名称和团队（共享资料在右上角）、目标输入框、「等你处理」、值班和任务记录。「等你处理」紧跟在输入框下面，数字和侧栏一致，每行写明在等什么（「等你验收」「等你决定」「2 件事」），值班的提问和没能开工也在这里；任务记录不再重复列出等你处理的任务，值班派出的任务前面是一个小图标。
- 共享资料和团队分开编辑：首页共享资料卡片上的「添加… / 编辑…」和交付结果的「加入共享资料…」打开只编辑共享资料的表单，工作区设置里只管目录、名称和团队。
- 修复工作区首页的「等你处理」只看最近 50 个任务、更早的等你处理的任务不显示的问题，现在和收件箱用同一份列表。

- New Host Environment in Settings. Agents and MCP servers work in home folders of their own, so they could not use your GitHub CLI, Lark CLI or keychain sign-ins, nor find toolchains such as rustup or nvm. You can now switch on, one by one, what to share: it is linked into every Agent's and MCP server's home folder, so they use the very same sign-in as you, and a change applies at once, to sessions already running too. The page lists the tools Janus knows that are on your Mac (sign-ins and accounts, toolchains, the macOS keychain) and the other settings in your home folder, and takes a path of your own; nothing is shared until you switch it on. Switching on a tool that keeps its sign-in in the keychain (GitHub CLI, Lark CLI, Git) switches on the keychain too. Agents' own settings (`~/.codex`, `~/.claude`, `~/.agents` and the like) cannot be shared: digital employees' instructions, skills and MCP still come from Janus only.
- A workspace's home is laid out anew, with everything about the workspace on it and only workspaces in the sidebar: Loops are no longer listed under their workspace there, and a workspace's count includes its Loops. From the top: the name and team (shared context at the top right), the goal box, Needs Attention, Loops and the run history. Needs Attention sits right under the goal box, its count matches the sidebar's, and each row says what it waits for (Ready to Accept, Decision Required, 2 Items); a Loop's questions and failed wakes are there too. The run history no longer lists again the runs that need attention, and runs a Loop dispatched carry a small mark.
- Shared context is edited on its own: Add… / Edit… on the shared context card and Add to Shared Context… on a delivery open a sheet for the shared context only, and Workspace Settings keep the folder, name and team.
- Fixed Needs Attention on a workspace's home looking only at the 50 newest runs, so older runs waiting for you were missing; it now uses the same list as the inbox.

## 2026.11.21.7

- 值班改为按时间表开工：在「什么时候开工」里选「按时间表」（每隔几分钟、每隔几小时、每天、工作日、每周、每月，或者直接写一条 5 段的 cron 表达式），表单下面实时显示接下来三次开工的时间；也可以选「只在你叫它时」。原来的「由它自己决定」不再是一个设置：值班仍可以自己约一次时间表之外的开工（比如等 CI 跑完再看），值班页写明它约的时间和原因。升级时已有的值班自动换成时间表：固定间隔换成对应的钟点（每 30 分钟就是每个整点和半点），「自己决定」换成每 30 分钟，「只在叫它时」不变。
- 值班在 Git 仓库里有了自己的副本：每次开工前对齐到工作区当前的提交，它改的文件只在副本里，不会碰你的工作区，下次开工前清掉。之前值班直接在工作区目录里干活，靠前后比较目录判断它改了什么，你或其他程序同时改的文件也会被算成值班改的。
- 值班第一次开工时自己起名。之前取会话标题当名字，用 Codex 时会变成「[Loop wake #1] …」这样的一串。值班的工作方式里不再重复你写的提示；每次开工的摘要取它最后一条消息，不再是开头那句「我先看看」。
- 中文界面里值班「开工」「摸鱼」：「干活中」「摸鱼中 · 明天 09:00 开工」「现在开工」「开工 3 次」，时间只显示到分钟；工作区首页值班一节的按钮改叫「新建…」。
- 任务的独立副本（Git worktree）在界面上不再出现「worktree」这个词和 Janus 目录下的路径：交付说明、删除确认、排队提示和子任务页的 Git 信息只写仓库、分支和提交；验收冲突时列出冲突的文件，并给出可以自己合并的提交。
- 修复任务副本的一批问题：副本目录被手动删掉后，任务既不能继续也删不干净；建副本中途失败时留下残留；验收会悄悄覆盖你目录里被忽略的同名文件（例如 `.env`），或者在你把分支回退后把删掉的历史带回来；分支已经包含结果时继续任务会从起点重建；两个任务同时验收进同一个分支会互相冲突；仓库被移动后任务删不掉；`.worktreeinclude` 里的中文文件名被跳过。清理副本时只删 Janus 自己的那一条，不再清理整个仓库里其他过期的 worktree 记录。
- Janus 调用 git 时不再继承启动环境里指向别处的 `GIT_DIR` 这类变量，不运行仓库的文件系统监视程序，不等终端输入；Spotlight 不再索引任务和值班的副本。

- Loops now wake on a schedule: under When It Wakes, choose On a schedule (every few minutes or hours, daily, weekdays, weekly, monthly, or a five-field cron expression of your own), and the form shows the next three times as you edit; or choose Only when you call it. Its own call is no longer a setting: a Loop can still ask to look again once outside its schedule (say, after CI finishes), and its page shows when and why. Existing Loops get a schedule on upgrade: a fixed interval becomes the matching clock times (every 30 minutes means on the hour and the half hour), its own call becomes every 30 minutes, and only when called stays.
- In a Git repository a Loop has its own copy: before each wake it moves to the workspace's current commit, files it changes stay in the copy, never in your workspace, and are cleared before the next wake. Before, a Loop worked right in the workspace folder and Janus compared the folder before and after, so files you or other programs changed meanwhile were blamed on the Loop.
- A Loop names itself on its first wake. Before, its name came from the session title, which under Codex became a string like “[Loop wake #1] …”. A Loop's working instructions no longer repeat your prompt, and a wake's summary comes from its last message instead of an opening “let me take a look”.
- In the Chinese interface Loops now 「开工」 and 「摸鱼」 (start work and slack off), and wake times show to the minute; the button in a workspace's Loop section is now New….
- A task's own copy (its Git worktree) no longer shows the word worktree or Janus's paths in the app: delivery notes, delete confirmations, the queue note and a subtask's Git line show the repository, branch and commit only; a conflict on acceptance names the files and the commit to merge yourself.
- Fixed a set of problems with tasks' copies: a copy whose folder was deleted by hand left the task unable to continue or be deleted; a copy that failed half way through creation was left behind; accepting could silently overwrite an ignored file of yours with the same name (such as `.env`), or bring back history after you reset the branch; continuing a task whose result the branch already had rebuilt it from the start; two tasks accepted into one branch at once collided; a task whose repository was moved could not be deleted; `.worktreeinclude` skipped file names outside ASCII. Cleaning up a copy now removes only Janus's own entry instead of pruning every stale worktree record in the repository.
- Janus's git calls no longer inherit variables such as `GIT_DIR` that point elsewhere, never run the repository's file system monitor and never wait for terminal input; Spotlight no longer indexes the copies of tasks and Loops.

## 2026.11.21.6

- 新增工作区的「值班」（英文界面叫 Loop）：在工作区首页点「新建值班…」，写下要它做什么，选多久醒一次（固定间隔、由它自己决定、只在叫它时）。值班由工作区的负责人来跑，不带团队；每次醒来看一眼、做判断，一两句话能答的直接答，要改东西的事派成独立的任务（同一件事按 key 只派一次），结果进收件箱。只有你能决定的事它会问你，不等回答；你的回答会唤醒它，并写在那次醒来的提示里。新建时不用起名字，它第一次醒来时自己起。值班出现在工作区首页和侧栏里，值班页有它的会话（随时可以和它说话）、待回答的问题、派出的任务和它记下的状态，可以现在唤醒、暂停、继续、编辑和删除。值班醒来期间改了工作区的文件、或者没能醒来，收件箱会提醒你；工作区不在 Git 仓库里时，值班页会说明看不出它改了哪些文件。
- 所有目标任务改为经过一个派发队列开始：同时在工作的任务最多 8 个（所有工作区一起算，可以用 `JANUS_MAX_RUNNING_RUNS` 调整），其余按先后排队。排队中的任务显示「排队中 · 第 N 位」，可以立即开始（不等空位）或取消；开始时才按当时的团队和资源库准备工作目录。暂停的任务会让出位置。没能开始的任务点「从中断处继续」会重新排队，服务重启时还没开始的任务也会重新排队。
- 去掉任务的 6 小时时长上限、美元费用上限和单次尝试 30 分钟的超时：Token 和费用只用于统计和显示，不再让任务停下。随之去掉「等待追加预算」状态和「追加预算」操作；升级时处于这个状态的任务改为「已中断」，可以从中断处继续。
- Agent 在会话里的回复、任务的完整目标和交付总结按完整的 Markdown 显示：表格画成表格，代码块带语言和复制按钮，引用、嵌套列表和任务列表也和文档窗口一样。之前这些只支持段落、列表、粗体和行内代码，表格和代码块会挤成一段竖线和反引号。消息里的标题只比正文略大，块间距更紧，适合在气泡里阅读。
- 会话中每条消息的 Markdown 只解析一次，之后按文本复用；Markdown 解析逐行用到的正则改为只编译一次，长对话滚动和收到新事件时的重绘不因完整渲染变慢。
- 修复并发创建任务时可能丢失第一条记录或报错、取消和删除排队任务时与自动开始竞争、领取位置失败时任务仍被启动等问题；修复菜单栏「任务」里的暂停和停止会作用到值班上的问题（值班的暂停和删除在值班页）。

- New workspace Loops (「值班」 in the Chinese interface): on a workspace's home, New Loop… takes what it should do and how often it wakes (a fixed interval, its own call, or only when called). A Loop is run by the workspace's lead without a team. Each time it wakes it takes a look and decides: it answers what a sentence or two answers, and dispatches anything that changes things as a run of its own (once per key), whose result goes to the inbox. It asks you only what you alone can decide and does not wait; your answer wakes it and comes in that wake's prompt. You do not name it: it names itself the first time it wakes. Loops show on the workspace home and in the sidebar; a Loop's page has its session (talk to it any time), questions waiting for you, the runs it dispatched and the notes it keeps, and lets you wake it now, pause, resume, edit or delete it. The inbox tells you when a Loop changed files in the workspace or could not wake; for a workspace outside a Git repository the Loop's page says changes cannot be seen.
- Every goal run now starts through one dispatch queue: at most 8 runs work at once across all workspaces (`JANUS_MAX_RUNNING_RUNS` changes it), the rest wait in order. A queued run shows its place, and can start now (without waiting) or be cancelled; its working directory is prepared with the team and library of the moment it starts. Paused runs give their place up. A run that could not start queues again when continued, and runs not yet started queue again after a restart.
- Runs no longer stop on a 6-hour time limit, a dollar cost limit or a 30-minute limit per attempt: tokens and cost are only counted and shown. The waiting-for-budget status and the Extend Budget action are gone; runs in that status become interrupted on upgrade and can be continued.
- Agent replies in sessions, a task's full goal and its delivery summary now render as full Markdown: tables are drawn as tables, code blocks get their language and a copy button, and quotes, nested lists and task lists look the same as in the document window. Before, only paragraphs, lists, bold and inline code were supported, so tables and code blocks collapsed into a paragraph of pipes and backticks. Headings in messages stay close to the body size and blocks sit closer together, to read well inside a bubble.
- Each message's Markdown is parsed once and reused by its text, and the per-line patterns of the Markdown parser are compiled once, so the full rendering does not slow down scrolling long conversations or redrawing on new events.
- Fixed creating runs at the same moment possibly losing a run's first record or failing, cancelling or deleting a queued run racing with it starting, and a run being started even when taking its place failed; fixed Pause and Stop in the Task menu acting on a Loop (a Loop is paused and deleted on its own page).

## 2026.11.21.5

- 「设置 → 通用」新增「发送消息」：可以选择 ⌘↩ 发送（默认，↩ 换行）或 ↩ 发送（⇧↩ 换行，⌘↩ 也能发送）。目标输入框、任务会话和对话都按这个设置，切换后已经打开的输入框立即生效，输入框的提示也写出当前的发送键。用输入法打字时，选字过程中按 ↩ 是让候选字上屏，不会把消息发出去。

- Settings › General has a new Send Messages option: ⌘↩ sends (the default, ↩ starts a new line), or ↩ sends (⇧↩ starts a new line and ⌘↩ still sends). The goal box, task sessions and chats all follow it; composers that are already open switch at once, and their hints name the current key. While an input method is composing, ↩ commits the composition instead of sending the message.

## 2026.11.21.4

- 服务返回的错误提示都改为中文，例如找不到任务时显示「任务不存在」，不再是英文的 `run not found`、`unknown kind`。
- 不再兼容早期的图片和 diff 记录：早期消息里点名图片的 `[附图 N]` 行原样显示，早期 diff 里只记了文件名的新文件不再从工作目录补内容。新记录不受影响。
- 删除 Agent、数字员工或工作区，以及安装、检查、切换适配器版本时，id 会按 URL 转义，带特殊字符的 id 也能正确处理；读取头像等原始数据时连不上服务，提示和其他请求一致。
- 修复数据升级测试从 2026.11.21.3 升级时失败，从最近三个版本升级都重新纳入验证。

- Error messages from the service are all in Chinese now, e.g. 「任务不存在」 when a task cannot be found, instead of English `run not found` or `unknown kind`.
- Early image and diff records are no longer specially handled: the `[附图 N]` lines naming images in early messages show as they are, and a new file that an early diff recorded only by name is no longer filled in from the working directory. New records are unaffected.
- Deleting an agent, a digital employee or a workspace, and installing, checking or switching an adapter version, now escape the id in the URL, so ids with special characters work; failing to reach the service while reading raw data such as an avatar shows the same message as other requests.
- Fixed the data upgrade test failing when upgrading from 2026.11.21.3; upgrades from the last three releases are verified again.

## 2026.11.21.3

- 数字员工可以有自己的头像：在数字员工编辑器里点「更换头像…」选一张图片，或者把图片拖到头像上，应用会从中间裁成正方形；「移除头像」回到原来的颜色和文字。设置、工作区、任务的团队和看板、收件箱、对话里都显示这张头像，删除数字员工时头像一起删除。
- 7 位默认数字员工都有了拟人的默认头像。升级后，没换过头像的默认数字员工自动用上；以后的版本换了默认头像，你没换过的跟着更新，换成自己的或移除了的保持不变。
- 默认数字员工的 id 改为它的职能：PMO 是 `pmo`，市场运营是 `marketing`，研发工程师是 `developer`，质量工程师是 `qa`，老友是 `friend`。升级时工作区成员、负责人和历史任务里的引用一起改；你自己用这些旧 id 建的数字员工不受影响。

- Digital employees can have their own picture: click "Change Avatar…" in the digital employee editor to pick an image, or drop one onto the avatar, and the app crops it to a square from the center; "Remove Avatar" goes back to the color and initials. The picture shows in Settings, workspaces, a task's team and board, the inbox and chats, and is deleted with the digital employee.
- The seven default digital employees now have person-like default pictures. After upgrading, default employees whose picture you never changed get theirs; when a later version changes a default picture, yours follows unless you replaced or removed it.
- The default digital employees' ids now say what they do: the PMO is `pmo`, marketing `marketing`, the engineer `developer`, quality `qa`, and 老友 `friend`. Upgrading updates workspace members, leads and past tasks to match; digital employees you made yourself under these old ids are left alone.

## 2026.11.21.2

- 新增默认的数字员工「老友」：像老朋友一样陪你把一件事聊透，好奇、会追问、敢说真话，适合在对话里用。全新安装直接就有；已经在用的，升级后自动加上。你自己已经建过同名同 id 的「老友」时保留你的。
- Claude 的「自定义 API」编辑时显示全部环境变量的值，令牌也在，改哪行就改哪行（本地优先：值仍只存在这台 Mac 上 Agent 的凭据文件里，只有本机的 Janus 应用读得到，网页读不到）。模板加上 `ANTHROPIC_DEFAULT_FABLE_MODEL`。
- 修复升级到 2026.11.21 后，`~/.janus` 顶层留下旧的 `permissions` 目录。
- 修复很早的数据（数据版本 2 之前）升级时，用户自己建的、和之后新增的默认数字员工同 id 的角色被误当作默认数字员工。

- New default digital employee "老友" (Old Friend): talks a matter through with you like an old friend, curious, asking more and telling the truth, made for chats. New installs have it; existing ones get it on upgrading. A "老友" of your own with the same id stays yours.
- Editing Claude's "Custom API" shows every environment variable with its value, the token included, so you change just the line you need (local first: values stay in the agent's credentials file on this Mac, readable only by the Janus app on this Mac, not by web pages). The template adds `ANTHROPIC_DEFAULT_FABLE_MODEL`.
- Fixed an old `permissions` folder left at the top of `~/.janus` after upgrading to 2026.11.21.
- Fixed very old data (before data version 2) treating a role of your own as a default one when it had the id of a later default.

## 2026.11.21

- Claude 的「自定义 API」改为一个环境变量输入框：直接粘贴多行 `export KEY=value`，一起设置网关地址、令牌和各档模型；点「填入模板」带入常用的变量，已经填写的不会被覆盖。令牌等私有值只保存在 Agent 自己的凭据文件里，编辑时只显示变量名。原来单独填写的 API Key 升级后作为 `ANTHROPIC_API_KEY` 继续使用。
- `~/.janus` 按「丢了还能不能再有」整理：`data/` 是无法恢复的数据（备份只需要它），`work/` 是 Agent 工作的目录，`cache/` 停止 Janus 后可以删，`tmp/` 每次启动清空，日志都在 `logs/`；任务和对话分开存放。升级时自动搬移，升级前的数据库备份在 `backups/`；退回更早的版本前，先把备份放回 `~/.janus/janus.db`。
- 删除任务或对话时，Agent 自己留下的会话记录一起删除，删掉的对话不再留在 Agent 的目录里。应用下载新版本前清掉以前留下的旧版本，Agent 的临时文件每次启动时清理，资源库 MCP 服务器的工具列表只保留当前版本。
- 修复在同一个 Git 仓库上同时创建多个任务时，偶尔因为另一个 worktree 还没建好而创建失败（`failed to read .git/worktrees/…/commondir`）：现在 worktree 依次建立。

- Claude's "Custom API" is now one box for environment variables: paste several `export KEY=value` lines to set the gateway address, the token and the models together, or click "Insert Template" to bring in the usual variables without replacing what you typed. Private values such as the token are kept only in the agent's own credentials file, and editing shows only their names. An API key entered separately before keeps working as `ANTHROPIC_API_KEY`.
- `~/.janus` is sorted by whether a thing can be had again: `data/` holds what cannot (back up just this), `work/` the folders agents work in, `cache/` what can go while Janus is stopped, `tmp/` what is cleared at each start, and all logs are in `logs/`; tasks and chats are kept apart. Upgrading moves everything by itself, with a backup of the database in `backups/`; before going back to an earlier version, put that backup back as `~/.janus/janus.db`.
- Deleting a task or a chat also deletes the session records the agent kept of it, so a deleted chat no longer stays in the agent's folder. The app clears versions left over from earlier updates before downloading a new one, and agents' temporary files are cleared at each start; the tool lists of library MCP servers keep only the current version.
- Fixed creating several tasks at once on one Git repository sometimes failing because another worktree was still being made (`failed to read .git/worktrees/…/commondir`): worktrees are now made one at a time.

## 2026.11.20.9

- 对话和交付后与负责人的对话重新打开时，能恢复原会话就原样接着聊：Agent 不再收到「之前的对话」，也不会再收到第二份数字员工的指令，会话页也不再多出这两条。原会话恢复不了、只能新开时，才带上最近的对话。子任务回到原会话时，会话页同样不再重复显示指令。

- When a chat, or the conversation with the lead after delivery, is reopened and its session can be restored, it simply carries on: the agent no longer gets the earlier conversation or a second copy of the digital employee's instructions, and the session page no longer shows them again. Only when the old session cannot be restored does the new one get the recent conversation. A subtask going back to its own session no longer shows its instructions twice either.

## 2026.11.20.8

- 资源库的 Skill 更容易被 Agent 找到并用上：`janus-skills` 的描述先写明开始任务时先读它，再带上各个 Skill 的名字和描述片段；负责人、协作者和对话的会话说明里都写明它的位置，Agent 自己的 Skill 列表放不下描述时也找得到。目录用每个 Skill 完整的描述和 `when_to_use`，不再截到 240 个字；Skill 多了，目录只列名字，Agent 按关键词搜索；常用的 Skill 排在前面。标了只能手动调用的 Skill 写明只在点名时使用。
- 资源库显示每个 Skill 被几个会话读过、最近一次在什么时候，并指出描述哪里可以改进：太短、没写什么时候用、太长被截断、和别的 Skill 太像。「Skill 路由怎样工作」的说明改为四步，加上「先查目录」，并按 Skill 实际的目录顺序和方式演示。
- 资源库的 MCP 服务器按 MCP 规范处理得更完整：分页列出的工具都能列全；只能以 task 方式调用的工具不再列出；Agent 放弃的调用会通知服务器停下，一次调用最多等 5 分钟；只支持 Janus 不认识的协议版本的服务器在准备时写明原因；工具说明带上服务器名，Agent 搜工具时更容易找到。

- Library skills are easier for agents to find and use. The `janus-skills` description now says to read it at the start of a task and carries each skill's name with part of its description, and the instructions of every lead, collaborator and chat session say where it is, so agents find it even when their own skill list has no room for its description. The catalog uses each skill's full description and `when_to_use` instead of cutting it at 240 characters; with many skills it lists only names and the agent searches it by keyword; the most used skills come first. Skills marked as manual-only say they are used only when someone names them.
- The library shows how many sessions read each skill and when it was last read, and points out how its description could be better: too short, never saying when to use it, too long to keep whole, or too much like another skill's. "How the Skill Router Works" now has four steps, adding "Look Up the Catalog", and plays them in the library's actual catalog order and mode.
- Library MCP servers are handled more fully by the MCP spec: tool lists given in pages are read in full; tools that can only be called as tasks are no longer listed; a call the agent gives up on tells the server to stop, and a call waits at most 5 minutes; a server that only speaks a protocol version Janus does not know says so when it is prepared; tool descriptions carry the server's name, so the agents' tool search finds them more easily.

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
