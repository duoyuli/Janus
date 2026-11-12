# 多 Agent 协作与 Agent Team 进展观察（2026-11-12）

> 覆盖窗口：2026-11-11 至 2026-11-12。
> 上一份报告[《2026-11-11 X 直接抓取补充》](multi_agent_collaboration_2026_11_11.md)覆盖到 11 月 11 日。本期重点看大规模 Agent 团队的运行经验。

## 摘要

11 月 11-12 日没有新的协作范式发布，值得看的是规模化运行中的工程摩擦。一位运行 1,000 个 Agent 的开发者报告：每任务成本上升 49%、约 5% 的尝试失败，原因不是 Agent 本身，而是所有 Agent 同时准备工作区；让它们排队启动后，成本回到持平、失败归零。同一条线程还指出 `git status` 会写 `index.lock`，并可能让同时进行的合并失败。

本期结论：看板和验证者路线不需要改；需要补的是启动并发控制和只读 Git 探测不抢锁。后者改动很小，本期已实现并有测试覆盖；前者需要先决定默认并发数和「被挡住的任务」如何表示。

## 方法与限制

- 使用 OpenCLI（`opencli twitter ...`），当前登录账号是 `@duoyuli_x`。只读，不发帖、不点赞、不关注。
- 本期做了 3 次搜索和 1 次线程读取，没有触发 429。搜索范围是 11 月 10 日以来的英文和中文讨论，另外用官方账号（`@antigravity`、`@AnthropicAI`、`@GoogleDeepMind`）交叉核对。
- X 上的个人经验帖无法独立复现，数字按原作者的说法记录；没有官方文档或论文佐证的内容标为「未确认」。

## 本期观察

### 1,000 个 Agent 的启动瓶颈

[@hasajedi](https://x.com/hasajedi/status/2120912456046284927)（2026-11-12）报告把单仓库上的 Agent 集群从 100 扩到 1,000 的结果：

- 每任务成本先上升 49%，约 5% 的尝试失败。原因不是 Agent 的推理，而是它们同时准备工作区、把机器压满。
- 根因是 git worktree 的创建：数百个 Agent 同时创建 worktree，最多 16 个并行，每个要 5-10 秒，单独运行时只要 1.2 秒。
- 处理方式是「每个 git 槽位都忙时，把下一个 Agent 挡住」，而不是让它继续堆积。之后每任务成本回到持平，每个任务恰好完成一次。
- 同一线程补充：`git status` 不是只读操作，刷新索引时会写 `index.lock`。一个进程在 worktree 里跑 `status` 时，另一个进程的合并可能因为 `index.lock: File exists` 失败。只读探测应该用 `--no-optional-locks`，或者只读 HEAD。
- 机器负载会显著影响测量：同一基准在空闲时每任务 3.15 秒 CPU，有其他构建时 5.68 秒，约 80% 的差异来自负载。作者建议记录 load average，负载高时丢弃测量结果，而不是解释成 Agent 变慢。

这条经验针对 worktree 隔离模型；Janus 的协作者共享同一个工作目录，没有 worktree 争用。但同时启动大量会话的摩擦是一样的：每个尝试都会启动 ACP 子进程、写入会话配置、准备环境。

### 成本仍然是最缺的可见性

[@designwithdnkys](https://x.com/designwithdnkys/status/2120917035039248746)（2026-11-12）引用 Anthropic 的测量说多 Agent 系统的 token 消耗约为普通对话的 15 倍。这个数字来自二手转述，标为「未确认」；[上一份报告](multi_agent_collaboration_2026_11_11.md)的 1,000-Agent 实验和 Antigravity 的配额暂停已经从另外两个角度说明同一件事：没有成本可见性，扩大规模只能靠猜。Janus 目前仍没有按 Run 或任务的 token / 费用账本。

### 规则要变成机制，而不是「不要」清单

[@zaidwardtech](https://x.com/zaidwardtech/status/2120922609860726833)（2026-11-12）的经验：Agent 仓库里的 `AGENTS.md` 写满「不要」规则，但在多 Agent 构建中真正生效的是被改写成机制、Agent 无法跳过的规则；只被读到的规则只是建议。这条和 Janus 的[合并前检查](../operations/agent_change_lessons.md#checklist)直接相关：测试和 clippy 仍是人工核对项，不在 CI 中（教训 L-02）。

### 其余观察

- Google Antigravity（2026-11-11）继续推送 browser use：Agent 可以操作浏览器并行处理多个任务，用户决定登录哪些站点，下载在批准前暂停（[推文](https://x.com/antigravity/status/2120556423474028576)）。这是 Agent 原生权限的又一种实现，Janus 不另加权限层，不适用。
- Google DeepMind（2026-11-11）说 Gemini agents 生成了超过 600 万行 Lean 证明，审计代码后人类专家只需复核关键的 1%（[推文](https://x.com/GoogleDeepMind/status/2120484249321033911)）。这支持「独立验证 + 分级证据」的方向，和 Janus 已有的 `verifies` 一致。
- @neuronz77（2026-11-12）描述自己的编排循环：规划阶段的评审 Agent、设计 / 规格、编码规则与约束、harness、实现 → 评审 → 验证 → QA → 报告，之后再并行；并把领域知识沉淀为可复用的数字资产。属于实践分享，未给出可验证的结果。
- NeurIPS 2026 有多篇 agent orchestration 论文（[@SFResearch](https://x.com/SFResearch/status/2120889730053370136)），本期没有拿到论文细节，下一期再补。

## 对 Janus 的建议

### P0（本期已实现）：只读 Git 探测不抢索引锁

- **现状**：Janus 和协作者共用工作目录。`/api/inspect` 的 `git status --porcelain`（`backend/src/main.rs:277`）和记录改动时的 `git diff` / `git ls-files`（`backend/src/orchestrator.rs:3480`、`4859`）都是只读查询，但没有关闭 optional locks。
- **借鉴**：`git status` 刷新索引时会写 `index.lock`；和 Agent 的 `git commit`、merge 同时发生时，可能让 Agent 的操作失败。
- **做法**：新增 `backend/src/git_probe.rs`，只读查询统一通过 `read_only()` 生成命令并设置 `GIT_OPTIONAL_LOCKS=0`（旧版 git 会忽略这个变量，比命令行参数更兼容）。`git_out()`、异步 `git()` 和 `/api/inspect` 都走这个入口；clone、`ls-remote` 等写操作不变。
- **更正（同日复核）**：`GIT_OPTIONAL_LOCKS=0` 只管得住 `git status` 这类查询；`git diff` 在只有文件时间戳变化时仍会刷新并改写索引，变量和 `--no-optional-locks` 都不起作用（git 2.54 实测）。记录任务 diff 用的 `git diff HEAD` 和 `git diff --shortstat HEAD` 因此改为底层命令 `git diff-index -p HEAD` 和 `git diff-index --shortstat HEAD`：它们从不刷新索引，对真实改动的输出与 `git diff HEAD` 相同，只有时间戳变化的文件不出现在 diff 中。
- **落点**：`backend/src/git_probe.rs`、`backend/src/orchestrator.rs`、`backend/src/main.rs`。
- **验收**：测试 `git_probe::tests::status_probe_leaves_the_index_alone_and_reports_the_same`：只读探测后 `.git/index` 不变，普通 `git status` 会改写索引，两者输出一致。`diff_probe_uses_diff_index_which_leaves_the_index_alone`：`diff-index` 不改写索引、输出与 `git diff HEAD` 一致，而经过同一入口的 `git diff` 仍会改写索引。端到端：真实服务和脚本化 Agent 跑完一个改文件的 Run（`/api/inspect`、记录 diff、交付），`.git/index` 始终不变。`just test` 的 69 个测试通过。`cargo clippy --all-targets -- -D warnings` 仍有 6 个改动前就存在的 `too_many_arguments` 诊断，数量没有增加。

### P1（需要先决定）：限制同时启动的尝试数

- **现状**：调度器为每个协作者保持「同一时间一个任务」（`orchestrator.rs:816` 的 `busy`），但没有全局并发上限。所有就绪任务会在同一轮里全部 `tokio::spawn`（`orchestrator.rs:873-914`），每个都会启动 ACP 子进程并准备工作区。
- **借鉴**：1,000-Agent 的经验是「槽位忙时挡住下一个」，而不是让它堆积。共享工作目录的 Janus 不需要 worktree 槽位，但需要会话启动槽位。
- **做法**：增加一个可配置的并发启动上限（例如 `maxConcurrentAttempts`），达到上限时把任务留在 `pending`，下一轮调度再启动。不要新增任务状态或事件类型；`finished` / `runnable` 的判断要把「只因并发上限没启动」算作还有工作。
- **风险**：上限过低会拉长 Run；上限过高没有作用。需要决定默认值（建议先取 CPU 核数或一个固定小值），并在[配置项](../interfaces/configuration.md#settings)和[任务调度](../domains/run_lifecycle.md#scheduling)中写明。改变调度行为前按仓库规则先写 ADR。

### P2：把合并检查中能自动化的部分放进 CI

- **现状**：`just test` 和 `cargo clippy --all-targets -- -D warnings` 仍是人工执行（[经验教训](../operations/agent_change_lessons.md#l02)）。
- **借鉴**：能被机制执行的规则才会稳定生效。
- **做法**：在 CI 中加 clippy 和仓库文档检查；测试已经在 CI 中。这一项不涉及多 Agent 运行时，但和「规则变机制」的结论一致。

## 操作记录

```text
opencli twitter whoami -f json
opencli twitter search '(("agent teams" OR "agent team" OR "multi-agent" OR "multi agent" OR "agent swarm") since:2026-11-11) -filter:replies' --product live --limit 30 -f md
opencli twitter search '(from:AnthropicAI OR from:OpenAI OR from:antigravity OR from:GoogleDeepMind OR from:github OR from:claudeai OR from:semianalysis OR from:simonw) since:2026-11-09' --product live --limit 25 -f md
opencli twitter search '("agent orchestrator" OR "agent orchestration" OR "multi-agent orchestration" OR "agent worktree" OR "verifier agent" OR "agent factory") since:2026-11-10 -filter:replies' --product live --limit 25 -f md
opencli twitter thread 2120912456046284927 -f md
```

本期 3 次搜索和 1 次线程读取，没有触发 429。下期继续控制在 3-4 次查询。

## 来源

- [@hasajedi：100 → 1,000 agents on one repo](https://x.com/hasajedi/status/2120912456046284927)
- [@hasajedi：启动并发是瓶颈](https://x.com/hasajedi/status/2120912459124855112)
- [@hasajedi：git status 会写 index.lock](https://x.com/hasajedi/status/2120912461301690579)
- [@hasajedi：机器负载影响测量](https://x.com/hasajedi/status/2120912463558263017)
- [@designwithdnkys：多 Agent token 成本约为 15 倍（二手）](https://x.com/designwithdnkys/status/2120917035039248746)
- [@zaidwardtech：规则要写成机制](https://x.com/zaidwardtech/status/2120922609860726833)
- [@antigravity：browser use 逐步开放](https://x.com/antigravity/status/2120556423474028576)
- [@GoogleDeepMind：Gemini agents 与 Lean 证明审计](https://x.com/GoogleDeepMind/status/2120484249321033911)
- [@SFResearch：NeurIPS 2026 agent orchestration 论文](https://x.com/SFResearch/status/2120889730053370136)
