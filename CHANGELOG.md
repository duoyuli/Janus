# 更新记录 / Changelog

Janus 的版本按日期命名，新版本写在最上面，每节列出用户能看到的变化，先中文后英文。发布前先在 `CHANGELOG.md` 写下这一版的 change log：`Release` 工作流把对应小节作为 GitHub Release 的说明，找不到「`## <版本>`」这一节时发布失败。格式和发布步骤见 [CI 与发布](docs/operations/20261101-ci_release.md#release)。

Janus versions are named by date, newest first. Each section lists the changes people can see, in Chinese and then in English. Write the section for a version in `CHANGELOG.md` before releasing it: the `Release` workflow uses that section as the GitHub Release notes and fails when there is no `## <version>` section. See [CI and releases](docs/operations/20261101-ci_release.md#release) for the format and the release steps.

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
