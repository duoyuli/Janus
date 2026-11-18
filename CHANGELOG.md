# 更新记录 / Changelog

Janus 的版本按日期命名，新版本写在最上面，每节列出用户能看到的变化，先中文后英文。发布前先在 `CHANGELOG.md` 写下这一版的 change log：`Release` 工作流把对应小节作为 GitHub Release 的说明，找不到「`## <版本>`」这一节时发布失败。格式和发布步骤见 [CI 与发布](docs/operations/20261101-ci_release.md#release)。

Janus versions are named by date, newest first. Each section lists the changes people can see, in Chinese and then in English. Write the section for a version in `CHANGELOG.md` before releasing it: the `Release` workflow uses that section as the GitHub Release notes and fails when there is no `## <version>` section. See [CI and releases](docs/operations/20261101-ci_release.md#release) for the format and the release steps.

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
