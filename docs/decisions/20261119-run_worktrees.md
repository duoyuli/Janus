# ADR：Git 仓库上的每个任务在自己的 worktree 中运行

- 状态：已采纳（accepted），记录于 2026-11-19。第 6 条和第 7 条中「从 `janus/<run-id>` 重建 worktree」已被 [ADR：验收时把结果合并回任务开始时的分支](20261120-merge_back_on_accept.md) 取代（superseded，2026-11-20）。
- 取代：[ADR：任务直接在工作目录中运行](20261031-in_place_workspaces.md) 中 Git 仓库的部分；非 Git 目录仍然直接在工作目录中运行。
- 相关文档：[ADR：worktree 写入用户仓库的边界](20261119-worktree_trust_boundary.md)、[Run 生命周期](../domains/20261031-run_lifecycle.md#workspace_model)、[安全边界](../operations/20261031-security.md#workspace_access)、[部署方案](../operations/20261031-deployment.md#layout)。

## 背景

2026-10-30 起，负责人和所有子任务都直接在工作区目录中运行（ADR `20261031-in_place_workspaces`）。同一个目录上同时有两个 Run 时，它们互相看不到对方的子任务、负责的文件和改动：一边的 diff 和痕迹对账里混进另一边的改动，冲突检测查不出跨 Run 的同时修改，结果可能互相覆盖（审查报告 CO-07）。

2026-11-19 用户和 Agent 逐项梳理了这个问题，参考了 Claude Code（`--worktree`、`.worktreeinclude`）和 Codex 应用（托管 worktree、本地环境的设置脚本）的做法。用户给出的目标是：在 worktree 里能跑通原目录里能跑的全部流程，包括端到端测试。

## 决策

1. **每个 Run 一个 worktree**：隔离的粒度是 Run，不是子任务。一个 Run 内部的负责人和协作者共用这个 worktree，`owns`、冲突检测、痕迹对账、返工和 diff 都和原来一样。
2. **Git 仓库上的每个 Run 都用**：创建 Run 时，如果工作区目录是有提交的 Git 仓库，就为它创建 worktree，之后这个 Run 的所有会话都在 worktree 里，不再改用户的目录。非 Git 目录（以及还没有任何提交的仓库）不隔离，仍直接在目录中运行。
3. **从 `HEAD` 开始，不带未提交的改动**：worktree 从创建时的 `HEAD` 开始，处于 detached HEAD，不建分支。用户目录里有未提交的改动时，创建前提醒「这些改动不会带进这个任务」，用户确认后照常开始。
4. **只带代码，环境在 worktree 里重新准备**：worktree 里只有 Git 跟踪的文件，加上仓库根目录 `.worktreeinclude`（`.gitignore` 语法，用户自己维护，和 Claude Code、Codex 共用）列出的被忽略文件，创建时复制过去：只复制同时被忽略的文件，跳过符号链接，不覆盖已有文件。依赖和构建产物不复制，由 Agent 在 worktree 里按项目自己的方式准备，因为 Run 改的可能正是环境本身。协作约定和负责人说明告诉 Agent 这一点。
5. **Janus 只隔离文件**：端口、本机数据库、全局缓存这类共享资源的冲突由项目自己处理（例如测试自动选空闲端口）。Janus 自己的 `just e2e` 改成自动选空闲端口。
6. **验收时由 Janus 提交到分支**：用户验收一个 worktree 中的 Run 时，Janus 在 worktree 里提交全部改动，把分支 `janus/<run-id>` 指向这个提交（第一次新建，之后只快进；分支在上次验收后被用户移动过就拒绝，不覆盖）；合并由用户决定。worktree 始终处于 detached HEAD，分支不被它检出，用户可以在自己的目录里检出、合并或删除这个分支。负责人和协作者仍然不提交，提交只在验收时由 Janus 做一次，不运行仓库的 Git 钩子（`--no-verify`）。没有改动时不提交、不建分支。
7. **生命周期**：worktree 放在 `<Janus home>/worktrees/<run-id>`，不放进用户的仓库；创建后用 `git worktree lock` 锁住。交付后、验收前保留，用户可以打开目录检查。验收后，负责人的对话还开着时 worktree 保留（会话的进程就在里面，删掉目录会让它失去当前目录），继续对话就接着在里面改，再验收时在分支上再提交一次；没有会话在用时（例如服务重启后验收）验收后移除，服务启动时也移除已验收 Run 留下的 worktree。之后继续这个 Run 时，从 `janus/<run-id>` 重建 worktree。（实现时修订：最初写的是验收时一律删除 worktree，但这样会和交付后仍然开着的负责人对话冲突。）取消或失败的 Run 保留 worktree，可以从中断处继续。删除 Run 时删除 worktree；里面还有未提交的改动时，先把改动存成补丁 `<Janus home>/worktrees/deleted/<run-id>.patch`。
8. **非 Git 目录的提醒**：非 Git 目录上已经有进行中（规划、执行、等你处理）的 Run 时，创建新 Run 前提醒「这个目录上有一个正在运行的任务，确定继续执行吗？」，用户确认后照常开始。不加锁。

## 取舍

- **按 Run 而不是按子任务隔离**：按子任务隔离就是 2026-10-30 以前的设计，需要集成合并、冲突处理和 diff 归属，负责人和协作者也看不到彼此的实时改动；用户要解决的是多个 Run 互相覆盖，按 Run 隔离就够。
- **每个 Run 都隔离，而不是只在并发时隔离**：行为统一，结果在哪里不取决于启动时有没有别的 Run；代价是即使只有一个 Run，结果也要验收后从分支合并。
- **验收时提交到分支，而不是应用回用户目录**：Claude Code 和 Codex 都不自动提交；Janus 选择在验收这个明确的时刻提交一次，分支就是交付物，多个 Run 的结果互不干扰，合并冲突交给 Git。
- **不带未提交的改动**：Codex 会把未提交的改动带进 worktree，Claude Code 不带；带过去会让 Janus 的提交里混进用户的半成品，所以只提醒，由用户决定先提交还是照样开始。
- **不复制依赖、不链接依赖**：复制或链接用户目录的依赖，测出来的是旧环境；链接还会让一个 Run 的安装改坏共享的目录。两家工具也都只复制少数被忽略的文件。
- **不管端口等共享资源**：互斥锁或资源分配都要项目配合，项目自己做到可以并行最可靠。

## 后果

- Run 的工作目录（`runs.cwd`）是 worktree 的路径；`runs.config.worktree` 记录原仓库（`repo`），只有 worktree 中的 Run 有这个字段。
- diff 产物、痕迹对账和会话都以 worktree 为准；负责人说明和任务提示写明这是一个隔离的新目录。
- 新增目录 `<Janus home>/worktrees/`，写入用户仓库的 `.git/worktrees/`、`refs/heads/janus/*` 和提交，边界见 [ADR：worktree 写入用户仓库的边界](20261119-worktree_trust_boundary.md)。
- 创建页在确认前提醒未提交的改动和非 Git 目录上进行中的 Run；运行卡片显示 worktree 和验收后的分支。
