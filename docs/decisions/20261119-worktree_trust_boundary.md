# ADR：worktree 写入用户仓库的边界

- 状态：已采纳（accepted），记录于 2026-11-19。验收和重建两行、「不改用户当前检出的分支和工作目录里的文件」已被 [ADR：验收时把结果合并回任务开始时的分支](20261120-merge_back_on_accept.md) 取代（superseded，2026-11-20）：验收时 Janus 把结果快进到任务开始时的分支，所有写操作都不运行钩子。
- 相关文档：[ADR：Git 仓库上的每个任务在自己的 worktree 中运行](20261119-run_worktrees.md)、[安全边界](../operations/20261031-security.md#workspace_access)、[ADR：用户数据、依赖和日志都放在 Janus home](20261031-user_home_layout.md)。

## 背景

此前 Janus 自己从不写用户的仓库：子任务的改动由 Agent 按它的权限模式写入，Janus 只做只读的 Git 查询（`git_probe::read_only`，`GIT_OPTIONAL_LOCKS=0`），不提交、不推送。为每个 Run 建 worktree（[ADR](20261119-run_worktrees.md)）后，Janus 本身要写用户仓库的 Git 数据。按合并前检查第 5 条，这扩大了信任边界，单独记录。

## 决策

Janus 只做下面这些写操作，都在 Run 的生命周期里由明确的事件触发：

| 时刻 | 写什么 | 位置 |
| --- | --- | --- |
| 创建 Run | `git worktree add --detach`、`git worktree lock` | 用户仓库的 `.git/worktrees/<run-id>`；worktree 目录在 `<Janus home>/worktrees/<run-id>` |
| 创建 Run | 复制 `.worktreeinclude` 列出的被忽略文件 | 只写进 worktree 目录，读用户目录 |
| 验收 | `git add -A`、`git commit --no-verify`（detached HEAD 上），`git update-ref` 新建或快进 `janus/<run-id>` | worktree 和用户仓库的对象库、`refs/heads/janus/` |
| 验收后没有会话在用时，或服务启动时 | `git worktree unlock`、`git worktree remove` | 用户仓库的 `.git/worktrees/` |
| 继续一个已移除 worktree 的 Run | `git worktree add --detach <路径> janus/<run-id>` | 同上 |
| 删除 Run | 存补丁后 `git worktree remove --force` | 补丁在 `<Janus home>/worktrees/deleted/` |

- **不碰的东西**：不推送，不改用户当前检出的分支和工作目录里的文件，不改 `janus/` 以外的分支，不删除任何分支（验收后的分支由用户决定去留）。
- **不运行仓库自己的代码**：提交用 `--no-verify`，不运行 `.git/hooks` 里的钩子；这些钩子可能是 Agent 写进去的。
- **提交者**：用仓库或全局 Git 配置里的身份；没有配置身份时，验收报错并保持「已交付」，不替用户编造身份。

## 取舍

- **由 Janus 提交，而不是让 Agent 提交**：提交只在用户验收时发生一次，内容是用户确认过的结果；Agent 仍然被要求不提交。
- **`--no-verify`**：跳过用户可能依赖的检查钩子（例如格式化），换来不执行仓库里可能被改过的脚本。用户合并分支前可以自己运行检查。

## 后果

- 安全文档的「工作目录访问」一节改为描述 worktree：任务不再直接改用户的目录，Janus 会写 `.git/worktrees/`、`janus/` 分支和提交。
- 部署方案的目录规范增加 `<Janus home>/worktrees/`。
