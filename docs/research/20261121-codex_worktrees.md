# 2026-11-21 Codex 怎样用 git worktree

- 调研日期：2026-11-21。
- 问题：OpenAI Codex 怎样用 git worktree 跑 Agent 的任务，怎样让用户尽量不用想 worktree；和 Janus 的 `backend/src/worktrees.rs` 对照。
- 方式：Codex 和 Antigravity（`agy` 的 `/swarm`，多个 Agent 交叉核对）各自独立 clone `github.com/openai/codex` 阅读源码，另查公开文档，没有改 Janus。两份报告按原文收录。
- 范围限制：公开仓库里没有 Codex 桌面应用和 IDE 扩展的源码，那部分只来自官方文档。

## Codex 的报告


#### Scope and evidence boundary

I cloned `https://github.com/openai/codex` into `codex/` with a shallow clone. The inspected `HEAD` is `357fe240c91563c1e6c9690bb27a2e609282d615`. The checkout was clean after the clone. The open-source tree contains the CLI, TUI, app-server, and supporting Rust crates. It does not contain the macOS or Windows desktop UI implementation, nor an IDE extension implementation. The desktop and IDE conclusions below therefore come from OpenAI's current product documentation, while the source conclusions are limited to the cloned repository.

I use four evidence labels in this report.

| Label | Meaning |
| --- | --- |
| Source confirmed | Directly implemented in the cloned source or covered by its tests |
| Product documented | Described in current official OpenAI documentation, with no matching public implementation in this clone |
| Source absence | A targeted search found no implementation in the inspected source tree |
| Inference | A conclusion from the preceding evidence, with the uncertainty called out |

The Janus comparison uses the read-only checkout at `/Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs`. No Janus source, `~/.janus`, `~/.codex`, or `~/.claude` state was modified.

#### 1. Creation

#### Open-source Codex behavior

- **Source confirmed**. `WorktreeManager::create` canonicalizes the requested source directory, finds the repository root, preserves the source subdirectory as a relative destination cwd, resolves `base` to a commit with `rev-parse`, and allocates a managed path. The default when `base` is absent is the source repository's current `HEAD`. [worktree/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L62-L86)

- **Source confirmed**. The allocation root defaults to `$CODEX_HOME/worktrees`. It uses a random UUID, a four-character hexadecimal bucket, and the repository directory name, giving a shape such as `$CODEX_HOME/worktrees/ab12/project`. On macOS it writes `.metadata_never_index` at the pool root. It does not put the task title or branch name in the path. [paths.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/paths.rs#L13-L45)

- **Source confirmed**. Creation runs `git worktree add --detach --no-checkout`, writes destination-only `config.worktree`, and materializes the requested commit with `git --work-tree=. reset --hard --no-recurse-submodules`. The returned `ManagedWorktree` has `branch: None`. The source checkout is never reset. [worktree/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L88-L165)

- **Source confirmed**. The manager does not call `git worktree lock`. A targeted search of the worktree, TUI, and exec code found no worktree-lock operation. Ownership is represented separately by an atomically written `codex-thread.json` record in the worktree's Git metadata. A second thread cannot claim an already-owned checkout. [metadata.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/metadata.rs#L1-L90)

- **Source confirmed**. Git commands scrub inherited repository selectors, disable hooks and fsmonitor, clear attributes and tree filters, skip Git LFS smudge, and disable terminal prompts. Working-tree commands also disable configured clean, smudge, and process filters for that command. These are process-local overrides rather than persistent repository changes. [git.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/git.rs#L1-L12) [git.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/git.rs#L142-L215)

- **Source confirmed**. The interactive `--worktree` path used by TUI startup and `codex exec --worktree` passes `base: None`, so it starts at the source `HEAD`. A separate managed-worktree flow for the TUI command center and model-facing tool chooses `default_worktree_base`. That helper prefers a cached remote HEAD, then cached `origin/main`, `origin/master`, local `main`, or local `master`. It does not fetch. [worktree_startup.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/worktree_startup.rs#L243-L255) [agents_overview_new.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/app/agents_overview_new.rs#L255-L335) [git.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/git.rs#L97-L140)

- **Source confirmed**. Tests show a dirty source checkout remains dirty and the new checkout contains only the committed base. There is no low-level stash or carry-forward of source edits. [worktree tests](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/tests/worktree.rs#L532-L578)

#### Desktop product behavior

- **Product documented**. The current Worktrees documentation lets a user choose a starting branch. It describes a current-branch choice that can carry unstaged local changes into the new worktree, and it says the default worktree is a detached `HEAD`. The documentation does not expose the implementation of that carry-forward step. [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

- **Product documented**. The desktop UI treats branch creation as an explicit Git action after the worktree exists. The documentation says users can create a branch, commit, push, create a pull request, or open the worktree in an IDE. I found no hard-coded branch naming algorithm in the public source. [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

#### Janus comparison

Janus already uses the stronger isolation form for a task run. `create` calls detached worktree creation, then locks the checkout with a run-specific reason and copies selected ignored files. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:43>)

Janus also records the source branch or detached source `HEAD`, so acceptance has a defined destination. The acceptance path can replay the run's own commits onto a branch that moved, then fast-forward it. It refuses a moved detached `HEAD`, rewritten history, conflicting rebase, overlapping uncommitted edits, or files that the result would overwrite. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:97>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:221>)

The main creation difference is base selection. Janus starts from the person's current `HEAD`, while Codex's managed command-center flow defaults to a cached repository default branch. Codex's interactive `--worktree` flow also starts from current `HEAD`. For Janus runs, current `HEAD` is the safer default because acceptance is explicitly tied to the source checkout that launched the run.

#### 2. Files and environment setup

#### Files that enter the checkout

- **Source absence**. Codex's public worktree manager materializes tracked files from the selected commit. Its source tree has no `.worktreeinclude` reader, no ignored-file copy routine, and no special `AGENTS.override.md` handling. The cloned repository has a top-level `.worktreeinclude` containing `user.bazelrc`, but it is repository content and no code reference to that filename was found in the clone. This is not proof that the private desktop application has no such code. [Cloned repository `.worktreeinclude`](./codex/.worktreeinclude)

- **Product documented**. The desktop Worktrees documentation says ignored files are copied only when listed in `.worktreeinclude`. It calls out ignored `AGENTS.override.md` as an automatic copy, skips symbolic links, and does not overwrite files already present in the destination. It limits this behavior to local desktop managed worktrees. [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

- **Product documented**. The local-environment documentation says setup scripts run automatically after a desktop worktree is created. The intended use is installing dependencies or running a build needed by the separate checkout. The configuration is stored in the repository's `.codex` directory. [OpenAI Local environments documentation](https://learn.chatgpt.com/docs/environments/local-environment)

- **Product documented**. Codex Cloud uses a prepared environment with repositories, tools, dependencies, and access. A setup script and an optional start skill can prepare the environment before the task. A new task gets a prepared filesystem, while continuing an existing task keeps its files and uncommitted changes. [OpenAI Cloud environments documentation](https://learn.chatgpt.com/docs/environments/cloud-environments)

- **Source confirmed**. The local CLI and TUI worktree creation code does not install dependencies or run a project setup script. It allocates and materializes the checkout, then changes the session cwd and loads configuration. [exec/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/exec/src/lib.rs#L432-L525) [worktree_startup.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/worktree_startup.rs#L243-L303)

#### Janus comparison

Janus implements the useful part of the documented desktop behavior in its own explicit form. `copy_included` reads `.worktreeinclude`, confirms each candidate is actually ignored, copies regular files only, skips symbolic links, and never overwrites a destination file. `create` and `ensure` both invoke it. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:464>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:50>)

Janus deliberately does not treat every ignored file as transferable. That is safer for credentials and machine-local state. If Janus later adds a special-case override file, it should retain the existing regular-file, no-overwrite, and no-symlink checks rather than broadening the copy set.

There is no evidence that Codex's local CLI installs dependencies automatically. A Janus setup hook would therefore be a product decision, not a necessary worktree primitive. The clean boundary is create, copy the explicitly allowed files, optionally run a named setup profile, then start the run with setup output recorded separately.

#### 3. Returning results and handling conflicts

#### Local CLI and TUI

- **Source confirmed**. The local worktree manager only creates, lists, binds, and removes checkouts. It has no accept, merge, cherry-pick, patch export, or sync-back operation. `codex exec --worktree` starts a task in the new cwd and leaves the checkout managed by Git. There is no `Drop` implementation on its `ManagedExecWorktree` wrapper that removes or applies the result. [worktree/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L40-L347) [exec/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/exec/src/lib.rs#L250-L261)

- **Source confirmed**. The TUI's managed-worktree tool returns `root`, `workspaceRoot`, and `sourceCwd`, binds the checkout to the thread, and tells the model to use the returned workspace root explicitly. It attaches the checkout to the thread but does not replay changes into the source checkout. [managed_worktree_tools.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/managed_worktree_tools.rs#L252-L313)

- **Source confirmed**. The command dispatcher allows `--worktree` for a new interactive session, `codex fork`, `codex exec`, and `codex exec fork`. It rejects resume and code review in this mode. [cli/src/main.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/cli/src/main.rs#L2344-L2383)

- **Product documented**. The desktop UI provides Git controls for staging, reverting, committing, pushing, and creating a pull request in the selected project or worktree. The user performs the Git handoff explicitly. The docs describe a Handoff from a cloud task to a local computer that resumes in the associated worktree. They do not document an automatic branch merge or cherry-pick performed when the task finishes. [OpenAI Local environments documentation](https://learn.chatgpt.com/docs/environments/local-environment) [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

- **Product documented**. The app announcement describes worktrees as an isolated copy and says users can check changes out locally or continue without touching the local Git state. This supports a review or explicit handoff model. It does not specify the private app's exact Git command sequence. [Introducing the Codex app](https://openai.com/index/introducing-the-codex-app/)

#### Cloud task path

- **Source confirmed**. `codex cloud diff` prints a task attempt's unified diff. `codex cloud apply` applies that diff to the current local checkout through the cloud-task client. [cloud-tasks/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/cloud-tasks/src/lib.rs#L663-L694)

- **Source confirmed**. The apply helper shells out to `git apply --3way`. A preflight uses `git apply --check` and leaves the working tree untouched. The result reports applied, skipped, and conflicted paths. The client distinguishes success, partial, and error. A partial or error result does not claim that the task was cleanly integrated. [git-utils/apply.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/git-utils/src/apply.rs#L38-L125) [cloud-tasks-client/http.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/cloud-tasks-client/src/http.rs#L451-L582)

- **Inference**. The cloud apply flow is a patch handoff, not a managed local-worktree acceptance flow. It applies into the current checkout and does not move a branch ref, create a merge commit, or commit for the user. The exact remote workspace creation and commit process is not exposed by this source tree.

#### Janus comparison

Janus has a more explicit result contract than Codex's local manager. `accept` stages and commits all run changes, checks whether the source still contains the start commit, rebases the run's own commits when the branch advanced, and advances the source only with a fast-forward. A failed acceptance leaves the result commit in the run worktree. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:221>)

That behavior should remain the default for Janus. Codex's public cloud path is useful as a separate explicit export or handoff option, but `git apply --3way` by itself would not preserve Janus's source-branch invariants or its guarantee that the person's checkout remains unchanged after a conflict.

#### 4. Lifecycle, cleanup, and recovery

#### Local source behavior

- **Source confirmed**. `WorktreeManager::remove` refuses to delete the current source checkout, refuses ignored local files, and delegates normal removal to `git worktree remove`, which protects tracked dirty changes. Incomplete creation uses a force removal only for rollback. The public manager does not expose a force-delete operation for a user-owned checkout. [worktree/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L284-L362)

- **Source confirmed**. Listing reads `git worktree list --porcelain`, requires the managed pool layout, validates the linked `.git` file and common directory, checks the Git backlink, and skips unsafe or stale registrations. It removes empty UUID buckets after removal. It does not call `git worktree prune`, and no automatic stale-registration prune was found in the inspected source. [worktree/src/lib.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L168-L272) [worktree tests](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/tests/worktree.rs#L713-L834)

- **Source confirmed**. Desktop-compatible settings parse `worktree-auto-cleanup-enabled` and `worktree-keep-count`, with a default keep count of 15. `WorktreeSettings::for_cli` shares the configured root but forcibly disables automatic cleanup for CLI allocation. The worktree crate itself contains no retention loop, age policy, disk quota, or snapshot-before-delete implementation. [settings.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/settings.rs#L1-L91)

- **Source confirmed**. TUI startup recovery retains an allocated checkout if the session never binds to it and prints a manual `git worktree remove <checkout-path>` instruction. The managed-worktree tool keeps completed operation status only for a bounded period while the TUI session remains open. After restart it tells the model to inspect the worktree list and browser before retrying. [worktree_startup.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/worktree_startup.rs#L56-L90) [managed_worktree_tools.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/managed_worktree_tools.rs#L225-L313)

- **Source confirmed**. The command-center flow owns a pending checkout until the UI accepts the creation event. If the transition is cancelled or a later check fails, it removes only a clean checkout. If removal fails or startup cannot bind, it retains the checkout and reports its path for manual recovery. [agents_overview_new.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/app/agents_overview_new.rs#L10-L39) [managed_worktree_creation.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/app/managed_worktree_creation.rs#L230-L303)

#### Desktop and cloud product behavior

- **Product documented**. The current desktop Worktrees documentation says automatic cleanup keeps the most recent 15 worktrees by default, can be configured or disabled, protects pinned chats and in-progress work, and saves a snapshot before deleting an eligible worktree so it can be restored. It describes cleanup by archive or count. It does not state an age threshold or disk-size threshold. [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

- **Product documented**. Codex Cloud saved task state is recoverable for up to seven days. This is a cloud VM retention rule, not a local Git worktree cleanup rule. [Using Codex Cloud](https://help.openai.com/en/articles/20001545-using-codex-cloud) [OpenAI Cloud environments documentation](https://learn.chatgpt.com/docs/environments/cloud-environments)

- **Source and product boundary**. The cleanup keys in the public Rust crate show that the CLI and desktop share a configuration contract, but the actual desktop cleanup and snapshot code is not in this clone. The documented desktop behavior therefore cannot be audited against public implementation here.

#### Janus comparison

Janus treats deletion as a data-preservation operation. Before removal, it saves a binary patch for uncommitted tracked changes and intentionally excludes copied ignored files such as credentials. If the repository is gone, it moves the whole directory under the retained area. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:329>)

Janus also handles a hand-deleted worktree during `ensure` by dropping the stale locked registration before rebuilding it, and its `remove` path handles a missing directory without treating the run as cleanly accepted. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:50>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:343>)

#### 5. What users see

#### Open-source CLI and TUI

- **Source confirmed**. The session picker asks where the new or forked conversation should run. Its choices are `Use current Git worktree` and `Create new Git worktree`. The normal conversation wording makes the worktree an execution choice rather than a filesystem concept. [worktree_picker.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/chatwidget/worktree_picker.rs#L48-L97)

- **Source confirmed**. The TUI has a separate `Worktrees` surface with `Continue current conversation`, `Start new conversation`, and `Browse worktrees`. The browser shows a thread title or status and the working-directory path. Actions include resume, copy working directory, and delete. The delete confirmation says thread history is kept and deletion may disrupt other sessions. [worktree_picker.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/chatwidget/worktree_picker.rs#L100-L145) [worktree_picker.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/chatwidget/worktree_picker.rs#L203-L391)

- **Source confirmed**. Normal startup keeps the path out of the primary flow. A path is shown when a checkout is retained because startup or registration failed. The message gives the exact checkout path and a manual removal command. [worktree_startup.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/worktree_startup.rs#L56-L90) [managed_worktree_creation.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/app/managed_worktree_creation.rs#L294-L303)

- **Source confirmed**. Model-facing creation returns filesystem paths as structured data and explicitly says the task cwd stays where it was. The caller must use `workspaceRoot` and request permissions when needed. Registration failure retains the checkout and tells the caller to inspect the browser rather than create a duplicate. [managed_worktree_tools.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/managed_worktree_tools.rs#L1-L3) [managed_worktree_tools.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/managed_worktree_tools.rs#L271-L313)

- **Source confirmed**. The local path is named by a pool bucket and repository name. The owner-facing name comes from the conversation title and the owner metadata. There is no task ID or branch label in the filesystem path, which makes the internal checkout less prominent in normal UI use. [paths.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/paths.rs#L13-L40) [metadata.rs](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/metadata.rs#L19-L90)

#### Desktop, IDE, and cloud surfaces

- **Product documented**. The desktop app presents separate threads, reviewable diffs, inline comments, editor opening, and explicit local checkout or Handoff actions. The underlying path is not the main task identity. [Introducing the Codex app](https://openai.com/index/introducing-the-codex-app/) [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

- **Source absence**. No desktop UI or IDE extension source was present in the cloned repository, so I cannot confirm how those surfaces render the path or implement Handoff. The public docs confirm the user concepts but do not expose the app's internal branch or merge commands.

- **Product documented**. Cloud tasks present an environment and task workspace rather than a local path. A task can be continued in its saved environment, while a new task receives a separate workspace and does not recover uncommitted changes from a previous task. [Using Codex Cloud](https://help.openai.com/en/articles/20001545-using-codex-cloud)

#### How Codex keeps worktrees from becoming the user's mental model

The public behavior uses three layers. The task or conversation is the primary identity. The worktree is selected as an execution location and normally remains out of the main interaction. A browser, copy action, or recovery message exposes the path when the user needs operational control. This is an inference from the picker, browser, recovery messages, and product documentation. It is not evidence that the private desktop app uses the same internal types as the public TUI.

#### 6. Ranked recommendations for Janus

#### P0. Keep detached runs and Janus acceptance semantics

Do not replace Janus's acceptance path with a generic patch apply. Keep detached creation and the run lock in `create` and `add`, keep source-branch capture in `Source::of`, and keep rebase plus fast-forward acceptance in `accept` and `Source::advance`. The open-source Codex manager has no local result replay, while its cloud path only applies a unified diff with `git apply --3way`. Janus's current code gives a stronger guarantee that the person's checkout stays untouched on conflict. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:43>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:97>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:221>)

#### P0. Preserve explicit ownership and serialized Git writes

Keep the run ID in the worktree lock reason and in the persisted run record. Keep the existing orchestrator write mutex around add, accept, ensure, and remove operations. Add a recovery or browser view that can map a stale checkout back to its run, following Codex's owner metadata and browser model. The relevant Janus lifecycle points are `ensure`, `accept`, `remove`, and `drop_worktree`. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:50>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:329>) [Janus orchestrator](</Users/duoyu/Data/dev/agent/Janus/backend/src/orchestrator.rs:1034>)

#### P1. Keep `.worktreeinclude` narrow and idempotent

Keep the current allowlist, ignored-file verification, regular-file check, symlink exclusion, and no-overwrite behavior in `copy_included`. Keep the call from both `create` and `ensure` so a rebuilt run receives the same permitted state. If Janus adopts Codex's documented ignored `AGENTS.override.md` convenience, make it an explicit special-case with the same safety checks. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:464>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:50>)

#### P1. Make setup an explicit post-creation phase

If Janus needs dependency installation, add named setup profiles after `create` succeeds and before the agent starts. Record setup output and failure state with the run. Do not mix setup commands into the Git transaction or silently copy the whole ignored set. This follows Codex's documented desktop setup-script boundary while preserving Janus's rollback behavior in `add`. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:67>) [OpenAI Local environments documentation](https://learn.chatgpt.com/docs/environments/local-environment)

#### P1. Keep acceptance refusal checks ahead of destructive Git operations

Retain the checks for newly added files already present in the source checkout and overlap with uncommitted source files before `merge --ff-only`. Retain the branch-moved rebase and abort path. These checks are in `Source::advance` and are more protective than Codex's local manager, which has no acceptance operation. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:148>)

#### P1. Make cleanup policy visible and recoverable

Keep the current patch preservation in `remove`, including the deliberate exclusion of ignored files that may contain credentials. Add an explicit retention policy for completed or abandoned runs with active-run protection, and expose the retained patch or directory through the run UI. Codex's desktop documentation provides a useful product reference with a configurable count and snapshot restore, while its public source does not show the implementation. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:329>) [OpenAI Worktrees documentation](https://learn.chatgpt.com/docs/environments/git-worktrees)

#### P2. Keep internal paths out of normal task language

Use the run or task identity in ordinary UI messages. Show the worktree path in an explicit workspace browser, a copy-path action, and recovery errors. Janus already has a stable internal path under its run pool, so this recommendation mainly concerns presentation around `create`, `ensure`, `accept`, and `remove`. Codex's TUI demonstrates this split between conversation labels and operational path access. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:43>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:329>) [worktree picker](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/tui/src/chatwidget/worktree_picker.rs#L203-L391)

#### P2. Limit `git worktree prune` to recovery

Janus uses prune in `ensure` to recover a hand-deleted locked registration. Keep that use scoped to the known run path. Do not add a broad startup prune that might alter unrelated worktrees owned by the user or another tool. Codex validates and skips stale registrations without performing a general prune. [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:50>) [Janus `worktrees.rs`](</Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs:388>) [Codex worktree manager](https://github.com/openai/codex/blob/357fe240c91563c1e6c9690bb27a2e609282d615/codex-rs/worktree/src/lib.rs#L168-L272)

#### Bottom line

Codex has two different worktree stories. The open-source CLI and TUI provide isolated detached checkouts, durable thread ownership, explicit browser and delete actions, and manual recovery, but no local result merge. The desktop product documentation adds ignored-file transfer, setup scripts, Handoff, Git review controls, and automatic cleanup with snapshots. Codex Cloud uses isolated task workspaces and a separate unified-diff apply path. Janus already exceeds the local Codex manager on acceptance safety and patch preservation. The main ideas worth carrying over are the explicit ownership model, selective file transfer, setup as a separate phase, visible recovery, and a user-facing browser that keeps worktree paths operational rather than central.


## Antigravity 的报告


- **Target Output**: `/private/tmp/claude-501/-Users-duoyu-Data-dev-agent-Janus/a61695d0-6016-49bf-bb3f-789aae62418f/scratchpad/agy-wt-research/findings.md`
- **Source Examined**: Cloned OpenAI Codex repository (`github.com/openai/codex`, revision commit `d6015b3992b9b7754b5df0193bb9f074d2047525`)
- **Comparison Targets**: 
  - Janus: `/Users/duoyu/Data/dev/agent/Janus/backend/src/worktrees.rs` (read-only)
  - Claude Code: Public documentation (`--worktree`, `.worktreeinclude`)

---

#### Executive Summary: The Paradox of "Invisibility"

The framing question of this investigation is:
> **"How does OpenAI Codex keep the user from having to think about worktrees at all?"**

**Key Finding**: **Codex does NOT keep worktrees invisible to the user.** 
In fact, Codex's product design takes the polar opposite approach:
1. **Worktrees are an explicit user choice**: Codex prompts the user in the CLI and TUI (`Where should the new conversation run? 1. Use current Git worktree, 2. Create new Git worktree`).
2. **Worktrees are exposed as first-class parallel workspaces**: Codex provides a full interactive Worktree Browser, shows the worktree filesystem path in the status line, and allows users to copy the path to clipboard (`Action::Copy`) to open it in their own editor or terminal.
3. **No automatic sync-back**: Codex has **zero automated merge, rebase, or patch integration** back to the primary checkout. Changes made by Codex remain in the worktree. The user is expected to inspect the worktree, test changes, and perform standard Git commands (`git merge`, `git push`, PR creation) manually.
4. **No `.worktreeinclude` support**: Codex does not implement `.worktreeinclude`. (The file present in the Codex root repository was committed for Anthropic's Claude Code).

In contrast, **Janus** aims for true, seamless worktree invisibility: automatically isolating each run in a detached worktree under `~/.janus/work/runs/<id>`, transferring ignored files via `.worktreeinclude`, and fast-forwarding or replaying linear commits onto the user's checked-out branch upon task acceptance.

By studying Codex's implementation (`codex-rs/worktree`, `codex-rs/git-utils`, `codex-rs/tui`), we uncover essential isolation techniques, security mitigations, and filesystem patterns that Janus can adopt to make its own worktrees truly invisible and resilient.

---

#### Detailed Findings

#### 1. Creation, Base, Location, Naming, Branch/Detached, and Locking

#### A. Creation Triggers and Preconditions
- **[Confirmed from Source] CLI Entrypoints**:
  - Allowed: `codex --worktree`, `codex fork <session_id> --worktree`, `codex exec --worktree`, and `codex exec fork <session_id> --worktree` (`codex-rs/utils/cli/src/shared_options.rs:71-72`, `codex-rs/cli/src/main.rs:2349-2381`).
  - Unsupported commands explicitly reject the flag: `codex resume --worktree`, `codex exec resume --worktree`, `codex exec review --worktree`, `codex login --worktree`, `codex archive --worktree`, `codex queue --worktree` (`codex-rs/cli/src/snapshots/codex__tests__unsupported_worktree_commands.snap`).
- **[Confirmed from Source] TUI Entrypoints**:
  - `/worktree` slash command, `/new` and `/fork` command intercepts (`codex-rs/tui/src/chatwidget/worktree_picker.rs:86, 113, 125`), and Agents Overview keymap `w` (`codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_agents_actions.snap:10`).
- **[Confirmed from Source] Preconditions Checked Before Creation** (`codex-rs/tui/src/app/managed_worktree_creation.rs:71-169`):
  1. *Project Trust*: Project must not be untrusted (`:71-74`).
  2. *Local Execution*: Local sessions only; rejected for remote workspace / daemon remote environments (`:75-81`).
  3. *Primary Session Idle*: Primary session must be idle without queued input (`:82-89`).
  4. *No Concurrency*: No concurrent worktree creation pending (`:90-93`).
  5. *MCP Ready*: MCP inventory must finish loading (`:96-102`).
  6. *No Running Subagents*: No other agent is running (`:108-127`).
  7. *Fork History*: If in Fork mode, conversation history must be saved/resumable (`:128-148`).
  8. *No Terminal Blockers*: Active background terminals block directory changes (`:160-169`).

#### B. Base Commit, Uncommitted Changes, and Stash
- **[Confirmed from Source] Base Commit Resolution**:
  - Defaults to `HEAD`:
    ```rust
    // codex-rs/worktree/src/lib.rs:76-85
    let revision = format!("{}^{{commit}}", request.base.as_deref().unwrap_or("HEAD"));
    let head_sha = git_stdout(&source_root, ["rev-parse", "--verify", "--end-of-options", revision.as_str()])?;
    ```
  - Annotated tags and refnames are peeled to commit objects (`codex-rs/worktree/tests/worktree.rs:380-394`).
  - Fallback branch resolution: `default_worktree_base(cwd)` inspects `git for-each-ref` checking `origin/HEAD`, `origin/main`, `origin/master`, `main`, `master` (`codex-rs/worktree/src/git.rs:99-140`).
- **[Confirmed from Source] Uncommitted Changes & Stash**:
  - Uncommitted changes in the user's checkout are **NOT carried over** (`codex-rs/worktree/tests/worktree.rs:533-578`). Source changes remain untouched in the source checkout.
  - Git stash is **never touched, created, or popped**.

#### C. Location and Naming Convention
- **[Confirmed from Source] Root Directory**:
  - Default: `<codex_home>/worktrees` (typically `~/.codex/worktrees`) (`codex-rs/worktree/src/settings.rs:44`).
  - Configurable: via TOML `[desktop] git-worktree-root = "/path/to/worktrees"`. CLI shares this desktop pool (`codex-rs/worktree/src/settings.rs:27-60`).
- **[Confirmed from Source] Bucket Layout and Directory Naming**:
  - Format: `{git-worktree-root}/{bucket}/{repository_name}` (`codex-rs/worktree/src/paths.rs:13-40`).
  - `{bucket}`: 4-character hex prefix from a random UUID v4 (`&identifier[..4]`), looping up to 65,536 times to find an unused folder name.
  - `{repository_name}`: Source repository folder name (`source_root.file_name()`).
  - *Example*: `~/.codex/worktrees/a1b2/Janus`.
- **[Confirmed from Source] macOS Spotlight Indexing Suppression**:
  - On macOS (`target_os = "macos"`), creates an empty `{git-worktree-root}/.metadata_never_index` file to disable Spotlight indexing (`codex-rs/worktree/src/paths.rs:17-23`).
- **[Confirmed from Source] Subdirectory Preservation**:
  - Preserves working directory relative to repository root (`root.join(relative_cwd)`). E.g., running from `repo/backend/src` starts the session at `~/.codex/worktrees/a1b2/repo/backend/src` (`codex-rs/worktree/src/lib.rs:70-72, 148-156`, `codex-rs/worktree/tests/worktree.rs:333-355`).
  - Validates `is_safe_worktree_cwd`; rolls back if the path escapes via symlink or does not exist at base commit (`codex-rs/worktree/src/lib.rs:149-156, 364-369`).

#### D. Branch vs. Detached HEAD
- **[Confirmed from Source] Detached HEAD**:
  - Worktrees are created exclusively with `--detach` and `--no-checkout`:
    ```rust
    // codex-rs/worktree/src/lib.rs:88-99
    git_output(&source_root, GitOperation::WorkingTree, [
        OsStr::new("worktree"),
        OsStr::new("add"),
        OsStr::new("--detach"),
        OsStr::new("--no-checkout"),
        root.as_os_str(),
        OsStr::new(&head_sha),
    ])?;
    ```
  - It does NOT create a dedicated branch (e.g. no `worktree-<name>` like Claude Code).
  - Verified by test: `!git_output(&worktree.root, &["symbolic-ref", "--quiet", "HEAD"]).status.success()` (`codex-rs/worktree/tests/worktree.rs:325-330`).

#### E. Locking
- **[Confirmed from Source] No `git worktree lock`**:
  - Codex does **NOT** run `git worktree lock`.
  - Instead, worktree ownership is tracked via a versioned JSON metadata record stored inside the Git administrative directory:
    `.git/worktrees/<name>/codex-thread.json`
    ```json
    {
      "version": 1,
      "ownerThreadId": "019f1234-5678-7000-8000-000000000001"
    }
    ```
  - Written atomically via `tempfile::NamedTempFile` and `persist_noclobber` (`codex-rs/worktree/src/metadata.rs:61-86`).
  - Thread binding prevents two threads from claiming the same worktree (`bail!("worktree already belongs to thread {existing}")`).

---

#### 2. Files, `.worktreeinclude`, Environment Scripts, and Dependency Install

#### A. What Files Come Along
- **[Confirmed from Source] Tracked Files Only**:
  - Materialized via:
    ```rust
    // codex-rs/worktree/src/lib.rs:130-140
    git_output(&root, GitOperation::WorkingTree, [
        "--work-tree=.",
        "reset",
        "--hard",
        "--no-recurse-submodules",
        head_sha.as_str(),
    ])
    ```
  - Only files committed to Git at `head_sha` exist in the new worktree.
- **[Confirmed from Source] Ignored and Untracked Files**:
  - **Zero ignored or untracked files are copied**.
  - No `.env`, `node_modules`, `target/`, or local configurations are transferred.

#### B. The Story of `.worktreeinclude`
- **[Confirmed from Source] No `.worktreeinclude` Support in Codex**:
  - An exact grep across the entire `codex-rs` codebase reveals **0 references** to `.worktreeinclude`.
  - The `.worktreeinclude` file located at the root of `github.com/openai/codex` (containing `user.bazelrc`) was committed by OpenAI engineer Adam Perry in commit `357fe240c915` (`chore: update .worktreeinclude`).
  - Git log inspection confirms it was added for team members using **Anthropic's Claude Code** (`claude --worktree`), which natively supports `.worktreeinclude`. Codex itself does not parse or respect this file.

#### C. Environment Setup Scripts and Dependency Install
- **[Confirmed from Source] No Automated Dependency Installation**:
  - Codex never invokes package managers (`cargo`, `npm`, `pnpm`, `pip`, `poetry`, `bazel`) when setting up a worktree.
- **[Confirmed from Source] No Environment Scripts**:
  - Codex does not run `.codex/setup.sh` or any hook scripts.
- **[Confirmed from Source] Hardened Git Execution Environment** (`codex-rs/worktree/src/git.rs:40-52, 142-182`):
  Codex aggressively scrubs the environment and disables repository-configured helpers to prevent arbitrary code execution during checkout:
  1. *Hooks Disabled*: `-c core.hooksPath=/dev/null` (`NUL` on Windows).
  2. *Filesystem Monitor Disabled*: `-c core.fsmonitor=`.
  3. *LFS Smudge Skipped*: `GIT_LFS_SKIP_SMUDGE=1`.
  4. *Attributes Disabled*: `-c attr.tree= -c core.attributesFile=`.
  5. *Filters Scrubbed*: Discovers all configured clean, smudge, and process filters in `.git/config` and explicitly neutralizes them with empty drivers and `required=false`.
  6. *Submodules Suppressed*: `--no-recurse-submodules`.
  7. *Environment Scrubbing*: Unsets `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_COMMON_DIR`, and any inherited `GIT_CONFIG_*` variables.

#### D. Git Configuration and Trust
- **[Confirmed from Source] Per-Worktree Git Config**:
  - Sets `core.worktree` inside the worktree's specific `config.worktree` (`codex-rs/worktree/src/lib.rs:107-127`), avoiding mutation of shared repo config.
- **[Confirmed from Source] Trust Inheritance**:
  - The worktree inherits project trust from the primary repository (`codex-rs/git-utils/src/worktree.rs:34-90`, `codex-rs/core/src/worktree_trust_tests.rs:86-96`).

---

#### 3. Synchronization Back to Primary Checkout, Conflicts, and Branch Movement

#### A. How Results Get Back
- **[Confirmed from Source] No Automated Synchronization**:
  - There is **no merge, no rebase, no patch application, and no cherry-pick** back to the user's primary checkout.
  - The `WorktreeManager` API (`codex-rs/worktree/src/lib.rs`) contains only `create`, `list`, `bind_thread`, `owner`, and `remove`.
  - The `apply_git_patch` utility in `codex-rs/chatgpt` is used solely for applying Web task patches in `codex apply <task_id>`, never for worktree reconciliation.
  - The model-facing tool prompt states:
    > *"The task stays in its existing checkout; use the returned workspaceRoot and request filesystem permissions when needed. Uncommitted changes are not copied."* (`codex-rs/tui/src/managed_worktree_tool_specs.rs:9`).
- **[Confirmed from Source] Manual Handoff Model**:
  - Changes made by Codex remain committed (or uncommitted) in `<codex_home>/worktrees/<bucket>/<repo>`.
  - The user interacts with the worktree via the TUI Worktree Browser (`codex-rs/tui/src/worktree_browser.rs:43-47`):
    - `Action::Resume`: Continue chatting with Codex inside the worktree.
    - `Action::Copy`: Copy the absolute path to clipboard to open in VS Code, terminal, etc.
    - `Action::Remove`: Manually delete the worktree when finished.
  - **[Inferred]** Because git worktrees share `.git/objects`, any commit made in the worktree is immediately available in the user's primary repository. Users manually run `git merge <commit>` or create PRs from the worktree.

#### B. Conflicts and Uncommitted Changes
- **[Confirmed from Source] Primary Checkout Safety**:
  - Because Codex never touches or fast-forwards the primary checkout, **conflicts in the primary checkout are impossible during Codex execution**.
  - Dirty files in the primary checkout are never overwritten.
- **[Confirmed from Source] Worktree Removal Safety** (`codex-rs/worktree/src/lib.rs:304-326`):
  - When the user selects "Delete worktree", Codex runs:
    `git ls-files --others --ignored --exclude-standard -z`
  - If any ignored files (e.g. build output, `.env`) exist, removal is refused:
    `"worktree contains ignored local files; remove them before deleting it"`
  - It then calls `git worktree remove` without `--force`. If uncommitted or untracked modifications exist, Git refuses removal (`codex-rs/worktree/tests/worktree.rs:85-135`).

---

#### 4. Lifecycle, Cleanup, Retention Limits, and Crash Recovery

#### A. Deletion Triggers
- **[Confirmed from Source] Deletion Triggers**:
  1. *Immediate Rollback on Setup Failure*: If `git worktree add`, config initialization, or safe cwd verification fails, Codex immediately deletes the worktree and the empty bucket (`remove_empty_bucket`) (`codex-rs/worktree/src/lib.rs:100-103, 142-146, 150-152`).
  2. *Explicit User Removal*: Via the Worktree Browser (`Action::Remove`).
  3. *Desktop LRU Pool Cleanup*: Managed by the Desktop GUI.

#### B. Limits on Count, Age, and Disk Size
- **[Confirmed from Source] CLI Has Cleanup Disabled**:
  - `WorktreeSettings::for_cli` sets `auto_cleanup_enabled: false` unconditionally (`codex-rs/worktree/src/settings.rs:28-36`).
  - Tested: `cli_creation_shares_default_and_custom_desktop_pools_without_enabling_cleanup` (`worktree/tests/worktree.rs:170-203`).
- **[Confirmed from Source] Desktop LRU Count**:
  - Desktop config `[desktop] worktree-keep-count` defaults to `15` (`DEFAULT_WORKTREE_KEEP_COUNT = 15`) (`settings.rs:16, 72-84`).
- **[Confirmed from Source] No Age or Disk Size Limits**:
  - There is no TTL / expiration timer, and no disk space ceiling.
- **[Confirmed from Source] Bucket Collision Limit**:
  - 4-hex-digit prefix allows up to 65,536 buckets (`paths.rs:25-39`).

#### C. Crash and Restart Recovery
- **[Confirmed from Source] Survives Process Restart**:
  - The thread binding in `codex-thread.json` resides on disk in `.git/worktrees/<name>/`.
  - On restart, `WorktreeManager::list()` reconstructs the state of all worktrees:
    - `Owner::Resumable`: Thread exists and is active; user can resume immediately.
    - `Owner::Archived`: Thread has been archived.
    - `Owner::Unavailable`: Metadata corrupted or thread deleted.
    - `Owner::None`: Unbound worktree.
- **[Confirmed from Source] Startup Failure Guard**:
  - If Codex crashes before `bind_thread` completes, `StartupRecovery` prints:
    > *"Startup did not finish binding a thread to this worktree: <path>. The checkout was kept. Inspect it and confirm no session is using it. To remove it, run git worktree remove <checkout-path> from the source repository... Do not use --force."* (`codex-rs/tui/src/worktree_startup.rs:57-88`).

#### D. Pruning and Manual Directory Deletion
- **[Confirmed from Source] No `git worktree prune`**: Codex never runs `git worktree prune`.
- **[Confirmed from Source] Resilient Handling of Manually Deleted Directories**:
  - In `WorktreeManager::list()` (`codex-rs/worktree/src/lib.rs:215-225`):
    - If a user deletes a worktree directory with `rm -rf`, `fs::symlink_metadata(path)` fails.
    - `list()` **silently ignores and skips the stale registration**.
  - If a directory path is reused by another repository or moved, Codex verifies the Git backlink (`.git/worktrees/<name>/gitdir` points to `<worktree>/.git`). If mismatched, it skips the entry (`codex-rs/worktree/tests/worktree.rs:714-784`).

---

#### 5. What the User Sees in the UI

| UI Element | What the User Sees | Code Evidence |
|---|---|---|
| **Slash Command** | `/worktree` (`"start or continue a conversation in a new worktree"`) | `tui/src/slash_command.rs:38` |
| **New Session Prompt** | *"Where should the new conversation run? 1. Use current Git worktree, 2. Create new Git worktree"* | `tui/src/chatwidget/worktree_picker.rs:48-98` |
| **Fork Session Prompt** | *"Where should the forked conversation run? 1. Use current Git worktree, 2. Create new Git worktree"* | `tui/src/chatwidget/worktree_picker.rs:55` |
| **Worktree Picker** | *"Worktrees: 1. Continue current conversation, 2. Start new conversation, 3. Browse worktrees"* | `tui/src/chatwidget/worktree_picker.rs:100-146` |
| **Worktree Browser** | *"Managed worktrees: Select a worktree to resume, copy its path, or delete it"* | `tui/src/worktree_browser.rs:20-41` |
| **Worktree Naming** | Thread title (e.g. `"Fix login form"`) + Subtitle (`"updated 5m ago · ~/.codex/worktrees/a1b2/Janus"`) | `tui/src/chatwidget/worktree_picker.rs:230-247` |
| **Status Line** | `<model> <tier> · <worktree_cwd>` (displayed continuously at the bottom of the screen) | `tui/src/chatwidget/status_surfaces.rs:499-520` |
| **Action Menu** | Title: `"Worktree: Fix login form"`, Subtitle: `<path>`, Actions: `Resume`, `Copy working directory`, `Delete worktree` | `tui/src/chatwidget/worktree_picker.rs:290-358` |
| **Delete Confirmation** | *"Delete this worktree? Keeps thread history; may disrupt other sessions"* | `tui/src/chatwidget/worktree_picker.rs:360-391` |

---

#### 6. Comparison Matrix: Janus vs. OpenAI Codex vs. Claude Code

| Feature | Janus (`worktrees.rs`) | OpenAI Codex (`codex-rs/worktree`) | Claude Code (`--worktree`) |
|---|---|---|---|
| **Design Philosophy** | **Completely Invisible** (Background run execution) | **Visible Workspace** (User-selected alternate environment) | **Parallel Session Flag** (User opt-in per CLI run) |
| **Worktree Location** | `~/.janus/work/runs/<id>` | `<codex_home>/worktrees/<bucket>/<repo>` | `.claude/worktrees/<name>/` inside repo |
| **Branching Strategy** | Detached `HEAD` | Detached `HEAD` | Dedicated branch `worktree-<name>` |
| **Git Locking** | `git worktree lock --reason` | Application metadata (`codex-thread.json`) | `git worktree lock` on active session |
| **Ignored Files Copy** | `.worktreeinclude` copied | **None** (zero ignored files copied) | `.worktreeinclude` copied |
| **Sync-Back Mechanism**| **Automated linear fast-forward / rebase replay** | **None** (User copies path / merges manually) | **Manual** (User merges `worktree-<name>`) |
| **Uncommitted Changes**| Kept as `<id>.patch` on deletion | Retained in worktree directory | Persisted in branch or discarded |
| **Cleanup Trigger** | Automatic on run acceptance / deletion | Manual in CLI / LRU count (15) in Desktop | Clean on clean exit; persists if dirty |
| **macOS Spotlight Guard**| ❌ Missing (`.metadata_never_index` absent)| ✅ Present (`.metadata_never_index` written)| ❌ Missing (relies on `.gitignore`) |

---

#### Concrete Recommendations for Janus

Janus's goal is to make worktrees **completely invisible** to the user. Based on our analysis of Codex and Janus's `backend/src/worktrees.rs`, here are ranked, concrete improvements:

#### Rank 1 (High Impact): Automated Conflict Branch Ref Handoff
- **Applies to**: `Janus: backend/src/worktrees.rs:281-289` (in `accept`).
- **Current Behavior**: If the branch moved while the run was executing and replaying commits fails with a conflict:
  ```rust
  if let Err(err) = write(path, &["rebase", "-q", "--onto", &source.tip, since]) {
      let _ = write(path, &["rebase", "--abort"]);
      bail!(
          "没有合并：{} 在任务开始后有了新的提交，和任务的改动冲突。任务的结果在提交 {}（worktree {}），可以自己合并。（{err:#}）",
          source.name, short(&head), path.display()
      );
  }
  ```
- **Invisibility Problem**: This leaks the internal worktree path (`path.display()`) and asks the user to manually `cd` into an internal `~/.janus/work/runs/<id>` directory. Even worse, `head` is not referenced by any branch ref, putting it at risk of garbage collection if the worktree is deleted.
- **Recommended Fix**: Before aborting or failing, bind the commit to a named ref in the source repository:
  ```rust
  let branch_name = format!("refs/heads/janus/{run_id}");
  write(repo, &["update-ref", &branch_name, &head])?;
  bail!(
      "没有合并：{} 在任务开始后有了新的提交，与任务改动冲突。改动已保存在分支 janus/{}（提交 {}），可在你的工作目录中直接合并或比对。",
      source.name, run_id, short(&head)
  );
  ```
  The user can then run `git merge janus/<run_id>` or `git diff janus/<run_id>` directly in their normal working directory without ever knowing where the worktree lived.

---

#### Rank 2 (High Impact): Suppress macOS Spotlight Indexing on Worktree Roots
- **Applies to**: `Janus: backend/src/worktrees.rs:69-74` (in `add` / `create`).
- **Codex Evidence**: `codex-rs/worktree/src/paths.rs:17-23`:
  ```rust
  #[cfg(target_os = "macos")]
  {
      let marker = root.join(".metadata_never_index");
      fs::write(&marker, b"").with_context(|| format!("cannot disable Spotlight indexing at {}", marker.display()))?;
  }
  ```
- **Invisibility Problem**: Janus's frontend is a native macOS SwiftUI app. When Janus creates runs under `~/.janus/work/runs/<id>`, macOS `mds` / Spotlight indexes the full source tree of every run. This causes:
  1. File search pollution: searching files in Spotlight/Alfred displays duplicate hits inside `~/.janus/work/runs/...`.
  2. Indexing lock contention: Spotlight indexing can lock build files or `.git` objects during fast task cycles.
- **Recommended Fix**: Ensure that `~/.janus/work/.metadata_never_index` is created when initializing the Janus work directory, or create `.metadata_never_index` in `path` before adding the worktree.

---

#### Rank 3 (Medium Impact): Leaf Directory Naming with Repository Name
- **Applies to**: `Janus: backend/src/worktrees.rs:69-74` and layout path helpers.
- **Codex Evidence**: `codex-rs/worktree/src/paths.rs:29`:
  `bucket.join(repository_name)` -> e.g. `~/.codex/worktrees/a1b2/Janus`.
- **Invisibility Problem**: Janus puts worktrees directly at `~/.janus/work/runs/<run_id>`. When agent sub-processes, build scripts, linters, or language servers run `std::env::current_dir()?.file_name()`, they see a random UUID string like `01937f2a` instead of `Janus` or `my-project`. This can break tools that rely on directory naming (e.g. Docker Compose project names, Go modules, Swift packages).
- **Recommended Fix**: Structure the worktree path as `~/.janus/work/runs/<run_id>/<repo_basename>`.

---

#### Rank 4 (Medium Impact): Comprehensive Git Environment and Filter Scrubbing
- **Applies to**: `Janus: backend/src/worktrees.rs:18-29` (`write`) and `create`.
- **Codex Evidence**: `codex-rs/worktree/src/git.rs:40-52, 142-182`.
- **Invisibility Problem**: Janus sets `-c core.hooksPath=/dev/null`, which protects against pre-commit/post-checkout hooks. However, repository `.gitattributes` or `.git/config` can still define `filter.<driver>.clean`, `filter.<driver>.smudge`, `filter.<driver>.process`, or custom `fsmonitor`. When `git worktree add` runs, Git executes these external filter scripts, potentially hanging or failing.
- **Recommended Fix**:
  Add the following Git options to worktree creation:
  ```rust
  .args([
      "-c", "core.hooksPath=/dev/null",
      "-c", "core.fsmonitor=",
      "-c", "attr.tree=",
      "-c", "core.attributesFile=/dev/null",
  ])
  .env("GIT_LFS_SKIP_SMUDGE", "1")
  .env("GIT_TERMINAL_PROMPT", "0")
  ```

---

#### Rank 5 (Low-Medium Impact): Sanitize Worktree Internal Paths from User Error Messages
- **Applies to**: `Janus: backend/src/worktrees.rs:240-244, 392-396`.
- **Current Behavior**:
  ```rust
  bail!("没有合并：这个任务的 worktree {} 不在了，无法核对它的结果...", path.display());
  ```
- **Invisibility Problem**: Path strings inside `~/.janus/work/runs/...` leak into user dialogs when tasks fail or disk errors occur.
- **Recommended Fix**: Keep paths in backend logs (`crate::config::log`), but describe errors in terms of task domains to the user:
  ```rust
  bail!("没有合并：任务的工作副本已不可用（可能被外部清理）。未提交的改动已备份至 data/deleted-runs/。");
  ```

---

#### Conclusion
OpenAI Codex achieves safety by treating worktrees as **user-visible, explicit, parallel sandboxes**, intentionally avoiding the complexity of automatic sync-backs and background reconciliation. 

Janus has chosen the more ambitious and user-friendly path of **complete invisibility**. By adopting Codex's isolation strengths (Spotlight suppression, leaf repository naming, filter scrubbing) and fixing conflict handoff through named branch refs, Janus can achieve true, seamless worktree invisibility without leaking implementation details to the user.
