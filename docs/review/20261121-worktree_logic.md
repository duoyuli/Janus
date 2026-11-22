# 2026-11-21 worktree 逻辑审查

- 审查日期：2026-11-21。
- 对象：`2d3f498` 时 Run 的 worktree 全流程（`backend/src/worktrees.rs`、`queue.rs`、`lifecycle.rs`、`actions.rs` 和相关文档），以及给值班开 worktree 的方案。
- 方式：Codex 和 Antigravity（`agy`，Gemini 3.8 Flash）各自独立只读审查，在自己的临时仓库里复现 git 行为，没有构建或运行 Janus。两份报告按原文收录，Codex 的编号是 F-01 至 F-12，agy 的是 Bug 1 至 Bug 10。
- 范围限制：没有覆盖应用端；结论是当天代码快照的，修复情况见[清理记录](../../LESSONS.md#cleanup_log)。

## Codex 的报告


### Scope and method

Reviewed committed `HEAD` `2d3f498486bc1adc015eafe6d760ff44ae0fe1e7`. The checkout had unrelated uncommitted Loop-schedule changes, which were ignored. I read the worktree implementation, queue, lifecycle, actions, Loop code, the requested ADRs and operational documents, and searched all backend worktree callers. I did not build or run tests in Janus. I used disposable Git repositories under this output directory for the Git behaviors called out below, then removed them.

Severity is high when user data or the selected branch can be changed incorrectly, medium when a run can become stuck or an operation fails without corruption, and low when the edge case is uncommon or the result is recoverable.

### Confirmed findings

#### F-01 — High — acceptance can overwrite an ignored untracked file in the person’s checkout

**Location:** `backend/src/worktrees.rs:141-163`, especially `Source::advance`; `backend/src/worktrees.rs:323-337`, especially `changed_files`.

**Scenario**

1. The source branch is checked out in the person’s directory.
2. The directory contains an ignored, uncommitted file such as `.env` (a file named by `.worktreeinclude` is a realistic example).
3. The run produces a commit containing that path. An Agent can create such a commit despite the “do not commit” instruction, or another run can bring one into the result.
4. Acceptance calls `git merge --ff-only` in the person’s checkout. If that merge fails, the overlap check only calls `git status --porcelain --untracked-files=normal`, which omits ignored files.

**Result:** Git fast-forwarded the branch and replaced the ignored file without an error. The overlap list is empty because Janus never examined ignored files. I reproduced this with Git 2.54 using an ignored `notes.md` containing `mine` and a worktree commit containing the same path. The merge succeeded and the file became `run`.

This contradicts the no-overwrite behavior documented in `docs/domains/20261031-run_lifecycle.md:67-72` and `docs/operations/20261031-security.md:84-90`. Before touching a checked-out source, enumerate ignored and ordinary untracked paths and reject any collision with `git diff --name-only <source-tip> <result>`. Keep the check under the same per-repository lock as the merge.

#### F-02 — High — an externally removed Janus worktree cannot be rebuilt because Janus locks it

**Location:** `backend/src/worktrees.rs:53-61` and `backend/src/worktrees.rs:64-72`.

`create` always writes a Git `locked` marker. If the worktree directory is deleted outside Janus, `ensure` sees that `path.exists()` is false and runs `git worktree prune`, then tries `git worktree add`. Git deliberately keeps a missing locked worktree during prune. In a disposable Git 2.54 repository the next add failed with:

```text
fatal: '…/wt' is a missing but locked worktree; use 'add -f -f' to override, or 'unlock' and 'prune' or 'remove' to clear
```

An interrupted or accepted Run cannot resume even though its run record and result still exist. The comment at line 57 says prune handles this case, but the lock makes that assumption false. For a path known to be Janus-owned, unlock the stale entry before a targeted prune/remove, verify that no admin entry remains, and only then rebuild. Do not use a broad prune as the only recovery step.

#### F-03 — Medium — deleting a missing locked worktree silently leaves a stale Git entry

**Location:** `backend/src/worktrees.rs:276-281`.

When the path is gone, `remove` calls `git worktree prune`, ignores its error, and returns `Ok(None)`. Because the entry is locked, prune leaves it in `.git/worktrees`. `delete_run` then deletes the Run rows and Janus directories as if cleanup succeeded (`backend/src/orchestrator/lifecycle.rs:549-577`).

The repository is left reporting a missing, locked worktree. A future Run cannot reuse that path, and the user has no record that Git metadata still needs repair. The missing-path branch should unlock and remove the specific Janus entry, or return an error and retain the Run until cleanup is verified.

#### F-04 — High — a branch reset behind the Run is silently overwritten at acceptance

**Location:** `backend/src/worktrees.rs:107-135` and `backend/src/worktrees.rs:214-244`.

`Source::of` records only the branch name and current tip. In `accept`, if the current tip is an ancestor of the Run result, the code skips rebase and calls `Source::advance`. It does not require the branch tip to be the original `since` commit or a descendant of it.

**Scenario:** the branch starts at `B`, the Run produces `R` on top of `B`, and the person resets the branch to parent `A` (`A` is before `B`). `is_ancestor(R, A)` is false, `is_ancestor(A, R)` is true, and `git merge --ff-only R` moves the branch from `A` to `R`, resurrecting `B` and the user’s intentionally removed history. I reproduced that exact graph in a disposable repository. A branch recreated at unrelated history can also be rebased and advanced, applying the Run to the wrong incarnation of the branch.

Acceptance should require the source tip to equal `since` or be a descendant of `since`. A reset, force-push, deletion/recreation, or other identity change should fail with the result commit left in the Run worktree for manual handling.

#### F-05 — Medium — an already-integrated result is recorded as no commit and resumes from the wrong tree

**Location:** `backend/src/worktrees.rs:212-219`; `backend/src/orchestrator/actions.rs:359-386`; `backend/src/orchestrator/lifecycle.rs:457-467` and `810-817`.

If the source branch already contains the Run worktree’s `HEAD`, `accept` returns `None` as “nothing new”. The `risk.accepted` event therefore stores `commit: null`. After a restart, recovery removes the accepted worktree. `resume_run` has no accepted commit to read and calls `ensure` from the original `baseRef`.

**Scenario:** the Run result is `R`, and before acceptance the source branch is fast-forwarded to `R` or to a descendant `S` containing `R`. Acceptance succeeds with no commit in the event. Restart followed by a message recreates the Run worktree from `baseRef`, so the continuing Agent sees neither `R` nor any intervening `S` changes. The source branch still has the result, but the Run’s working tree and subsequent reasoning are stale.

Record the actual source tip used for the accepted state, or store a separate accepted worktree base and use it in `ensure` when the result was already present. The event needs enough information to rebuild the exact tree that was accepted.

#### F-06 — Medium — worktree removal errors are discarded and callers can report success with a leaked worktree

**Location:** `backend/src/worktrees.rs:312-320`.

`drop_worktree` ignores failures from unlock, `git worktree remove --force`, fallback `remove_dir_all`, and prune. On a read-only parent or other filesystem failure, both Git removal and the fallback can fail while the function still returns. I reproduced the first failure in a disposable repository by making the worktree’s parent mode `0555`; Git reported permission denied and the directory remained.

This matters most to `accept_archived` and other `remove=true` paths. They can mark a Run accepted while its worktree and Git admin entry remain. The same helper is used during failed-start cleanup. Return a `Result`, verify both the directory and the Git entry are gone, and keep the Run in its prior state when cleanup cannot be completed.

#### F-07 — Medium — failures after `git worktree add` can orphan a worktree

**Location:** `backend/src/worktrees.rs:43-48`, `backend/src/worktrees.rs:64-72`, and `backend/src/worktrees.rs:340-370`; caller `backend/src/orchestrator/queue.rs:270-301`.

`create` performs `add`, `lock`, and `.worktreeinclude` copying, but an error from locking or copying is returned without undoing the successful earlier step. In `start_run`, the cleanup block begins only after `create` has returned a `WorktreeOf`; an error from `create` therefore bypasses that cleanup. A copy failure from `create_dir_all` or `std::fs::copy` is enough to leave the directory and `.git/worktrees` entry behind while the Run is marked failed.

Wrap creation in a guard that removes the just-created worktree on every later error, including lock and include-copy failures. Add a test that injects a copy failure and asserts no directory or Git admin entry remains, matching the promise in `docs/domains/20261031-run_lifecycle.md:70-71`.

#### F-08 — Medium — acceptance and removal are not serialized with each other or with other repository operations

**Location:** creation/rebuild serialization in `backend/src/orchestrator/queue.rs:270-280` and `backend/src/orchestrator/actions.rs:459-467`; acceptance in `backend/src/orchestrator/actions.rs:359-381`; accepted-worktree cleanup in `backend/src/orchestrator/lifecycle.rs:796-805`.

The `worktree_adds` mutex covers only add/ensure. Two delivered Runs on the same branch can both read the same source tip, then race in `Source::advance`. One wins; the other gets a fast-forward or compare-and-swap failure even when the changes are disjoint. The user must retry. A simultaneous acceptance and deletion can also let acceptance commit or advance the branch after deletion has removed the Run from the live map, leaving a branch change without the corresponding event.

Use a per-repository lock for acceptance, add/ensure, repair, and removal, or re-read and safely retry after a source-tip race. Keep the Run event/status transition in the same serialized operation.

#### F-09 — Medium — moving or deleting the user repository makes existing Runs unresumable and often undeletable

**Location:** the absolute repository snapshot in `backend/src/orchestrator/queue.rs:268-288` and the Git-dependent cleanup in `backend/src/worktrees.rs:276-296`; callers `backend/src/orchestrator/actions.rs:459-467` and `backend/src/orchestrator/lifecycle.rs:549-558`.

The Run stores the original absolute repository path. If that directory is moved, the worktree’s `.git` file still points at the old worktree admin path. If the repository is deleted, the same Git operations fail. `ensure` cannot rebuild from the old path, acceptance cannot commit or merge, and `delete_run` first calls `git add -A` in the broken worktree, so deletion returns an error and retains the Run.

Provide a repair operation that accepts the new repository path and runs `git worktree repair`, and provide an explicit unrecoverable-repository cleanup path that preserves whatever can be archived without pretending Git history was saved. At minimum, surface this state distinctly instead of leaving a Run that cannot be resumed or deleted.

#### F-10 — Medium — Janus-home relocation treats `git worktree repair` as best effort after committing the migration

**Location:** `backend/src/layout/relocate.rs:80-118` and `backend/src/worktrees.rs:299-309`.

The home-layout migration moves a worktree and then calls `repair`, but `repair` returns no error and only logs a failure. The database/path rewrite and migration version still complete when the repository is unavailable or Git refuses the repair. The moved worktree then has stale absolute links and later Git operations fail. The migration will not automatically retry because its version is already recorded as applied.

Make repair a tracked migration obligation. Preflight the repositories before moving, or persist a pending repair and retry at startup. Do not silently treat a moved but unusable worktree as a successful migration.

#### F-11 — Medium — deletion patches omit ignored changes, including changes to `.worktreeinclude` files

**Location:** `backend/src/worktrees.rs:276-296`.

`remove` stages with `git add -A` and saves only `git diff --cached --binary HEAD`. Ignored files are excluded from both. A disposable worktree with an ignored `.env` changed to `secret=changed` produced an empty cached diff, so removing the worktree discarded it without a `.patch`.

This conflicts with the broad “uncommitted changes are kept as a patch” wording in `docs/domains/20261031-run_lifecycle.md:68` when the changed file was one of the explicitly copied `.worktreeinclude` files. Either preserve ignored files through a separately documented, permission-safe mechanism or narrow the contract and add tests that make the exclusion intentional. Storing credentials in a patch needs an explicit security decision.

#### F-12 — Low — a stale branch checkout record makes acceptance fail instead of falling back to the ref

**Location:** `backend/src/worktrees.rs:128-135` and `backend/src/worktrees.rs:169-176`.

`checked_out_at` trusts every `branch refs/heads/...` entry from `git worktree list --porcelain` and does not verify that the returned directory exists. If the person manually deletes another worktree without pruning it, acceptance chooses that missing path and runs `git merge` there. It does not use the safe `update-ref` path for a branch that is no longer actually checked out.

Verify the path before selecting `Place::Checkout`, and repair or prune that specific stale entry before deciding whether the branch is checked out.

### Suspicions and lower-confidence edges

- `backend/src/orchestrator/queue.rs:257-268` treats any successful `git rev-parse --is-inside-work-tree` as Git. Git returns success with `false` for a bare repository, so a workspace pointing at a bare repository can be classified as Git and then fail at `--show-toplevel`. Bare repositories may be outside the supported workspace model. If they are unsupported, reject them explicitly and add a regression test.
- `ensure` and the missing-path branch of `remove` run a repository-wide `git worktree prune`. Besides the Janus entry, this can delete stale metadata for unrelated user worktrees. A targeted stale-entry operation would better match the trust boundary in `docs/operations/20261031-security.md:84-90`.
- `.worktreeinclude` parsing uses newline-delimited Git output in `backend/src/worktrees.rs:349-369`. Paths containing newlines or Git’s quoted special characters are not covered by tests and can be copied incorrectly. This is a portability/data-coverage concern rather than a demonstrated loss with ordinary filenames.

### Missing tests

The existing flow tests cover independent worktrees, `.worktreeinclude` for a simple `.env`, normal dirty-file protection, branch movement to another branch, detached `HEAD`, a sub-folder workspace, restart after acceptance, deletion patches, and failed bundling. The following cases are absent:

- ignored untracked collision in the checked-out source, especially a copied `.worktreeinclude` file;
- ignored-file changes in a deletion patch;
- externally deleted locked worktree followed by resume, delete, and same-path recreation;
- removal failure after `git worktree remove --force` and failed cleanup after `create`/include copying;
- branch reset behind the Run, branch deletion/recreation, and a branch already containing the Run result;
- two Runs accepting into one repository concurrently, plus acceptance racing deletion or recovery;
- repository moved, repository deleted, and Janus-home relocation with a failed `git worktree repair`;
- stale branch checkout records, symlinked `/var` versus `/private/var` paths, and unusual `.worktreeinclude` filenames;
- bare-repository rejection if bare workspaces are not supported.

### Loop worktree plan

The current Loop is deliberately different from a goal Run. `backend/src/orchestrator/loops.rs:226-234` creates a `RunKind::Loop` session with `git: false` and the workspace directory as `cwd`. `wake_loop` records changes directly in that directory (`:378-407`), and `loop_turn_ended` compares normal Git status before and after the turn (`:534-581`). Giving each Loop a worktree needs more than changing the path.

- **Move to the current tip:** `worktrees::ensure` only creates a missing worktree (`backend/src/worktrees.rs:53-61`). It does not reset an existing worktree to the workspace’s current branch tip. Before each wake, capture the current top-level repository, branch or detached `HEAD`, and commit, then discard/recreate or reset the Loop worktree. A detached worktree avoids “branch already checked out elsewhere” failures.
- **Session lifetime:** ACP starts with a fixed `cwd` (`backend/src/orchestrator/lead.rs:691`). The current Loop session remains open between wakes. Do not move or reset its worktree while that process is alive. Finish the session after each wake and reopen it in the new worktree, or keep a worktree/session pair stable for the entire turn and serialize cleanup after the turn.
- **Locking:** goal creation/rebuild uses `worktree_adds`, but acceptance, deletion, repair, and recovery do not share it. Loop wake, goal acceptance, another Loop’s wake, and user deletion must use a common per-repository lock. A worktree reset racing a goal acceptance can otherwise prune or remove metadata in use.
- **Discard semantics:** `worktrees::remove` creates a patch and omits ignored files. The Loop plan says report changes and discard them. Add a dedicated discard operation that handles tracked, untracked, ignored, and Agent-created commits, and verify that the directory and Git admin entry are gone before the next wake.
- **Change reporting:** `changed_files` sees ordinary status only. It misses ignored `.worktreeinclude` files and changes already committed by an Agent. Compare the wake worktree with its captured starting tree, including commits and ignored paths, then report the result before cleanup.
- **Workspace subfolders:** mirror `queue.rs:268-288`. Make the worktree for the repository root and start the Loop in the corresponding prefix, such as `apps/client`. Delete and rebuild the whole worktree, not only the subfolder.
- **Branch and detached-HEAD changes:** if the person switches branches or moves a detached `HEAD` while the Loop sleeps, the next wake must use the new tip. Never move the person’s checkout or create a branch for the Loop. A reset/force-push must be treated as a new baseline, not as an acceptance merge.
- **Uncommitted person changes:** start each wake from `HEAD`, as goal Runs do, and report that uncommitted files in the person’s directory were not included. Do not run the Loop in the person’s checkout during the transition.
- **Repository and home moves:** preserve the canonical top-level and subfolder prefix, and run `git worktree repair` when Janus home moves. If the repository is missing, keep the Loop asleep with an actionable repair state rather than repeatedly creating broken worktrees.
- **Delete races:** `delete_loop` stops its timer and deletes the session (`:303-316`), while `wake_loop` does not take the settings lock and can already be in flight. Add cancellation/epoch checks and wait for an in-progress wake before removing the Loop worktree, so a late wake cannot recreate or touch a deleted Run.
- **Outside Git:** detect Git status at every wake if the plan allows a workspace to gain or lose its repository. The existing Loop snapshot hard-codes `git: false`; it cannot represent a later Git worktree without rebuilding the Run/session state.

## Antigravity 的报告


### 1. Executive Summary

A comprehensive, read-only architectural and code-level audit was conducted on Janus's Git worktree implementation (`backend/src/worktrees.rs`, `backend/src/orchestrator/queue.rs`, `lifecycle.rs`, `actions.rs`, `loops.rs`, and related modules).

The overall design (isolating each goal run in a dedicated, locked, detached worktree under `<Janus home>/work/runs/<id>`, and fast-forwarding/rebasing back to the source branch or detached HEAD upon user acceptance) is conceptually sound and avoids interfering with the user's active checkout during run execution. However, multiple concrete bugs, edge cases, and failure modes were uncovered across the worktree lifecycle, ranging from leaked worktrees and broken pruning on locked trees, to unrecoverable states when repositories or worktrees are deleted/moved, non-ASCII `.worktreeinclude` failures, and concurrency race conditions.

---

### 2. Complete Lifecycle Analysis

```
[queued] 
   │
   ▼ (take_slots -> start_run)
[planning] ──> worktrees::create(repo, path, base_ref, id)
   │               ├─ git worktree add --detach <path> <base_ref>
   │               ├─ git worktree lock --reason "Janus 任务 <id>"
   │               └─ copy_included(repo, path) [.worktreeinclude]
   │
   ▼ (drive -> execute)
[running] ──> Lead & Collaborators work concurrently inside worktree
   │           Diff artifacts generated via git diff-index (probe)
   ▼
[delivered] ──> Lead finishes turn; delivery recorded
   │
   ├──────────────────────────────┬──────────────────────────────┐
   ▼ (user accept)                ▼ (user continue)              ▼ (user delete)
worktrees::accept              Lead continues turn          worktrees::remove
├─ git add -A & commit         in existing worktree         ├─ git add -A
├─ check source (branch/HEAD)     │                         ├─ git diff --cached (save patch)
├─ replay if source moved         ▼                         ├─ git worktree unlock
│  (git rebase <tip>)          Redelivered                  └─ git worktree remove --force
├─ source.advance                 │
│  (git merge --ff-only)          ▼ (user accept)
└─ drop_worktree (if remove)   [accepted]
   │                              │
   ▼                              ▼ (server restart: recover removes worktree)
[accepted]                     Worktree gone
                                  │
                                  ▼ (user continues after restart: resume_run)
                               worktrees::ensure(repo, path, accepted_commit, id)
                               ├─ git worktree prune
                               ├─ git worktree add --detach
                               ├─ git worktree lock
                               └─ copy_included
```

### Lifecycle Transition Invariants & Restart Behavior
- **Queued**: Run row exists in SQLite; no filesystem directories or worktrees exist. On restart, `recover()` sees `never_started` and requeues via `requeue()`.
- **Planning / Starting**: Worktree is created before database row is updated with snapshot. If interrupted here, `recover()` requeues the run. Next attempt at `start_run` checks `if path.exists()`, calling `remove` to start clean.
- **Running / Interrupted**: Worktree remains on disk. On restart, status becomes `Interrupted`. `resume_run` continues with existing worktree files if `path.exists()` is true.
- **Delivered**: Worktree remains on disk with uncommitted or committed files.
- **Accepted**:
  - While server stays running and lead conversation remains active, worktree remains in place (`remove = false`).
  - Upon server restart, `recover()` removes worktrees of accepted runs.
  - If resumed later, `resume_run` calls `ensure()` using the commit recorded in `risk.accepted.data.commit` (falling back to `base_ref`).
- **Deleted**: Uncommitted changes are saved as a patch in `<Janus home>/data/deleted-runs/<id>.patch`, then worktree is unlocked and removed.

---

### 3. Confirmed Bugs

#### Bug 1: `worktrees::ensure` fails to rebuild worktree when directory was deleted by hand/externally (Locked worktrees cannot be pruned)
- **File & Line**: `backend/src/worktrees.rs:53-62` (`ensure`)
- **Concrete Scenario**:
  1. A run was accepted and its worktree was created and locked with `git worktree lock`.
  2. The worktree directory on disk was removed (e.g. user manually deleted `~/.janus/work/runs/<id>`, disk cleaner purged it, or directory was deleted while Git metadata remained).
  3. The user continues the run via API / UI (`resume_run` -> `ensure`).
  4. `ensure` observes `!path.exists()`, and executes:
     ```rust
     let _ = write(repo, &["worktree", "prune"]);
     add(repo, path, from, run_id).context("重建 worktree 失败")?;
     ```
  5. In Git, `git worktree prune` **deliberately skips locked worktrees** to protect them from being pruned when mount points or drives are temporarily disconnected.
  6. Git leaves the entry in `.git/worktrees/<id>`.
  7. `add` then executes `git worktree add --detach <path> <from>`, which fails immediately:
     `fatal: '<path>' is a missing but locked worktree; use 'add -f -f' to override, or 'unlock' and 'prune' or 'remove' to clear` (exit code 128).
- **What Goes Wrong**: Stuck run! The run cannot be resumed, continued, or recovered, crashing with `"重建 worktree 失败"`.
- **Severity**: **High**
- **Suggested Fix**: In `ensure`, unlock the path before pruning:
  ```rust
  let p = path.to_string_lossy();
  let _ = write(repo, &["worktree", "unlock", &p]);
  let _ = write(repo, &["worktree", "prune"]);
  add(repo, path, from, run_id).context("重建 worktree 失败")?;
  ```

---

#### Bug 2: `worktrees::remove` permanently leaks locked worktree metadata if worktree directory is already missing from disk
- **File & Line**: `backend/src/worktrees.rs:278-282` (`remove`)
- **Concrete Scenario**:
  1. A run had an active, locked worktree.
  2. The worktree directory was deleted (either by `recover()` for accepted runs, or manually).
  3. The user deletes the run (`delete_run` -> `worktrees::remove`).
  4. `remove` checks:
     ```rust
     if !path.exists() {
         let _ = write(repo, &["worktree", "prune"]);
         return Ok(None);
     }
     ```
  5. Because the worktree was locked with `git worktree lock`, `git worktree prune` **refuses to prune it**.
  6. `remove` returns `Ok(None)` without unlocking the worktree or calling `drop_worktree`.
- **What Goes Wrong**: Leaked Git metadata! Phantom locked entries permanently remain in `.git/worktrees/` in the user's repository. Git will never prune them automatically.
- **Severity**: **Medium**
- **Suggested Fix**: Unlock the worktree before pruning when `!path.exists()`:
  ```rust
  if !path.exists() {
      let p = path.to_string_lossy();
      let _ = write(repo, &["worktree", "unlock", &p]);
      let _ = write(repo, &["worktree", "prune"]);
      return Ok(None);
  }
  ```

---

#### Bug 3: `delete_run` is permanently blocked if the user's repository was moved or deleted
- **File & Line**: `backend/src/orchestrator/lifecycle.rs:550-560` and `backend/src/worktrees.rs:278-297`
- **Concrete Scenario**:
  1. A run was created for workspace repository at `/path/to/repo`.
  2. The user moves, renames, or deletes `/path/to/repo` on their disk.
  3. The user attempts to delete the run from Janus (`delete_run`).
  4. `delete_run` calls `crate::worktrees::remove(Path::new(repo), &self.worktree_root(run_id), &kept, run_id)`.
  5. If `path.exists()`: `remove` runs `git add -A` in `path`. Because `path/.git` points to `/path/to/repo/.git/worktrees/<id>`, which does not exist, Git fails: `fatal: not a git repository: (null)` (exit code 128).
  6. If `!path.exists()`: `remove` runs `write(repo, &["worktree", "prune"])`. Since `repo` does not exist, `Command::new("git").current_dir(repo)` fails to spawn with `std::io::ErrorKind::NotFound`.
  7. `remove` returns `Err(...)`.
  8. `delete_run` returns `Err(Refused::Invalid(err))`.
- **What Goes Wrong**: The user cannot delete the run through API/UI; deletion fails permanently every time with error 400/500.
- **Severity**: **High**
- **Suggested Fix**: In `lifecycle.rs:550`, verify `repo.is_dir()`. If the repository is gone, skip Git operations, delete `path` directly, and proceed:
  ```rust
  if let Some(repo_str) = row["config"]["worktree"]["repo"].as_str() {
      let repo = Path::new(repo_str);
      if repo.is_dir() {
          let kept = self.cfg.layout.deleted_runs();
          if let Err(err) = crate::worktrees::remove(repo, &self.worktree_root(run_id), &kept, run_id) {
              return Err(Refused::Invalid(err));
          }
      } else {
          crate::config::log(format!("janus: repository {} no longer exists; removing worktree dir directly", repo.display()));
          let _ = std::fs::remove_dir_all(&self.worktree_dir(run_id));
      }
  }
  ```

---

#### Bug 4: Worktree directory and Git lock leaked if `create` fails inside `copy_included` or if `start_run` fails before database write
- **File & Line**: `backend/src/worktrees.rs:45-48` (`create`) & `backend/src/orchestrator/queue.rs:279-300` (`start_run`)
- **Concrete Scenario**:
  1. `worktrees::create` executes:
     ```rust
     add(repo, path, base, run_id).context("创建 worktree 失败")?;
     copy_included(repo, path)
     ```
  2. `add` successfully creates the worktree and locks it in `.git/worktrees/`.
  3. `copy_included` encounters an I/O or permission error while copying a file and returns `Err`.
  4. `create` propagates the `Err` without rolling back `add`.
  5. In `queue.rs:279`, `start_run` errors out at `?`.
  6. The database never writes `row["config"]["worktree"]` (only written on full success).
  7. The run is marked `Failed`. If the user subsequently deletes the failed run, `delete_run` checks `row["config"]["worktree"]["repo"]`, which is `None`. It deletes `dirs.work` from disk, but leaves the locked entry in `.git/worktrees/` forever.
- **What Goes Wrong**: Leaked worktree! A phantom locked worktree is left in `.git/worktrees/` indefinitely.
- **Severity**: **Medium**
- **Suggested Fix**: In `worktrees::create`, roll back on error in `copy_included`:
  ```rust
  pub fn create(repo: &Path, path: &Path, base: &str, run_id: &str) -> Result<Vec<String>> {
      add(repo, path, base, run_id).context("创建 worktree 失败")?;
      if let Err(err) = copy_included(repo, path) {
          drop_worktree(repo, path);
          return Err(err);
      }
      ...
  }
  ```

---

#### Bug 5: `checked_out_at` causes `accept` to fail with `No such file or directory` if a checked-out worktree was deleted from disk
- **File & Line**: `backend/src/worktrees.rs:169-177` (`checked_out_at`)
- **Concrete Scenario**:
  1. The user had another worktree for branch `feat` (e.g. `/path/to/other-wt`).
  2. The user deleted `/path/to/other-wt` directory from disk without running `git worktree remove`.
  3. A run targeting branch `feat` is accepted.
  4. `checked_out_at` parses `git worktree list --porcelain`. Git still lists `/path/to/other-wt` as containing `branch refs/heads/feat`.
  5. `checked_out_at` returns `Some(PathBuf::from("/path/to/other-wt"))` without checking `path.is_dir()`.
  6. `Source::advance` matches `Place::Checkout(dir)` and invokes `write(dir, &["merge", "--ff-only", ...])`.
  7. Spawning `git` with `current_dir("/path/to/other-wt")` fails with `std::io::ErrorKind::NotFound` (`运行 git 失败: No such file or directory`).
- **What Goes Wrong**: Acceptance fails completely with an unexpected OS error instead of cleanly updating the ref or checking whether the worktree is valid.
- **Severity**: **Medium**
- **Suggested Fix**: In `checked_out_at`, only return checkouts that exist on disk:
  ```rust
  lines.any(|l| l == want).and_then(|| {
      let p = PathBuf::from(path);
      p.is_dir().then_some(p)
  })
  ```

---

#### Bug 6: `.worktreeinclude` fails to copy any non-ASCII files (Chinese, Unicode, etc.) due to Git's `core.quotepath` escaping
- **File & Line**: `backend/src/worktrees.rs:348-360` (`copy_included`)
- **Concrete Scenario**:
  1. Repository `.worktreeinclude` specifies an ignored non-ASCII file (e.g. `配置.env`, `密钥.pem`, or unicode filenames).
  2. `copy_included` queries Git:
     `git ls-files --others --ignored --exclude-from=.worktreeinclude`
  3. By default in Git, `core.quotepath = true`. Git escapes non-ASCII characters as C-style octal strings wrapped in quotes: `"\"\\351\\205\\215\\347\\275\\256.env\""`.
  4. `candidates` receives the literal string `"\"\\351\\205\\215\\347\\275\\256.env\""`.
  5. Line 360 executes `std::fs::symlink_metadata(&from)`. Rust searches for a file named literally `"\"\\351\\205\\215\\347\\275\\256.env\""`, which returns `NotFound`.
  6. The file is silently skipped (`continue`), and is never copied into the worktree.
- **What Goes Wrong**: Missing files in worktree! Critical non-ASCII configuration or secret files specified by the user in `.worktreeinclude` are silently dropped.
- **Severity**: **Medium**
- **Suggested Fix**: Pass `-c core.quotepath=false` to `read` in `copy_included`:
  ```rust
  let listed = read(
      repo,
      &["-c", "core.quotepath=false", "ls-files", "--others", "--ignored", &format!("--exclude-from={}", include.to_string_lossy())],
  ).unwrap_or_default();
  ```

---

#### Bug 7: `uncommitted` / `changed_files` slices `l.get(3..)`, misparsing renamed files (`R  old -> new`), breaking overlap detection
- **File & Line**: `backend/src/worktrees.rs:331-338` (`changed_files`) & lines 150-163 (`advance`)
- **Concrete Scenario**:
  1. The user has an uncommitted file rename in their checkout (e.g. via `git mv old.txt new.txt` or staging a rename).
  2. The run modified `old.txt` or `new.txt`.
  3. On accept, `git merge --ff-only` fails because of the uncommitted rename in the user's checkout.
  4. `advance` attempts to detect overlapping files:
     ```rust
     let touched = read(dir, &["diff", "--name-only", &self.tip, head]).unwrap_or_default();
     let overlap: Vec<String> =
         uncommitted(dir).into_iter().filter(|f| touched.lines().any(|t| t == f)).collect();
     ```
  5. `changed_files` slices `l.get(3..)`. For a rename, porcelain output is `R  old.txt -> new.txt`.
  6. `uncommitted(dir)` contains `"old.txt -> new.txt"`.
  7. `touched.lines()` yields `"old.txt"` or `"new.txt"`.
  8. `t == f` never matches. `overlap` is empty.
  9. Instead of the informative error message listing the conflicting files, the user gets a generic merge failure.
- **What Goes Wrong**: Degraded error reporting; user is not informed which files caused the conflict.
- **Severity**: **Low**
- **Suggested Fix**: In `changed_files`, if a line contains ` -> `, split and push both paths:
  ```rust
  if let Some(target) = l.get(3..) {
      if let Some((old_p, new_p)) = target.split_once(" -> ") {
          return vec![old_p.to_owned(), new_p.to_owned()];
      }
      return vec![target.to_owned()];
  }
  ```

---

#### Bug 8: Spurious rebase conflicts because `accept` rebase omits `--onto <source.tip> <base_ref>` when branch history was amended
- **File & Line**: `backend/src/worktrees.rs:225` (`accept`)
- **Concrete Scenario**:
  1. User was on branch `feat` at commit `C1`.
  2. Run `r1` started from `C1`. In the worktree, the agent commits `R1` (touching only `run.txt`).
  3. While `r1` is running, the user amends commit `C1` on `feat` (`git commit --amend` -> `C1_amended`).
  4. User accepts `r1`.
  5. `accept` runs `git rebase -q <feat_tip>`.
  6. Because `--onto` is omitted, `git rebase` computes `merge-base(HEAD, feat_tip)`, which is the parent of `C1`.
  7. Git attempts to replay `C1` onto `C1_amended`, which causes an immediate rebase conflict in `feat.txt`, even though the run only modified `run.txt`!
- **What Goes Wrong**: False rebase conflict! Clean run commits fail to merge back and require manual user intervention.
- **Severity**: **Medium**
- **Suggested Fix**: Use `git rebase --onto <source.tip> <base_ref>` instead of `git rebase <source.tip>`, ensuring only the commits made during the run (`base_ref..HEAD`) are replayed.

---

#### Bug 9: Concurrent `accept` calls on the same repository race on Git index and ref locks
- **File & Line**: `backend/src/orchestrator/actions.rs:359-387` (`accept_worktree`)
- **Concrete Scenario**:
  1. Two runs `r1` and `r2` on the same repository finish and are delivered.
  2. The user (or UI/API) accepts both runs simultaneously or in rapid succession.
  3. Unlike worktree creation (which is guarded by `self.worktree_adds`), `accept_worktree` has **no synchronization mutex**.
  4. Both threads concurrently invoke `git merge --ff-only` or `git update-ref` in the user's repository.
  5. Git's `.git/index.lock` or `.git/refs/heads/...lock` causes one call to fail with `File exists`.
- **What Goes Wrong**: Intermittent 500/Conflict errors on accept under concurrent usage.
- **Severity**: **Medium**
- **Suggested Fix**: Protect `accept_worktree` with a repository-level lock or reuse `self.worktree_adds` to serialize writes to the target repository.

---

#### Bug 10: Service crash after `source.advance` but before `risk.accepted` causes retry to record `commit: null`, losing the accepted commit from future `ensure` resumes
- **File & Line**: `backend/src/worktrees.rs:214-220` & `backend/src/orchestrator/actions.rs:373-387`
- **Concrete Scenario**:
  1. User accepts a run. `accept` advances the source branch with `source.advance(repo, head)`.
  2. The service process crashes or is killed before recording `risk.accepted` in SQLite.
  3. On restart, the run is still `delivered`.
  4. User accepts again.
  5. In `accept`, `head == source.tip` because the branch was already moved in step 1.
  6. `is_ancestor(path, &head, &source.tip)` evaluates to `true`.
  7. `accept` returns `Ok(None)` ("这个任务没有新的改动，没有提交").
  8. `risk.accepted` is recorded with `commit: null`.
  9. If the user later continues the run, `last_accepted_commit` finds `null` and falls back to `base_ref`!
  10. `ensure` rebuilds the worktree from `base_ref` (the starting commit), losing the accepted commit from the worktree!
- **What Goes Wrong**: Historical state inconsistency and potential regression if run is resumed later.
- **Severity**: **Medium**
- **Suggested Fix**: In `accept`, if `head != since` (or if worktree has commits ahead of `since`), but `head == source.tip`, recognize that the source was already advanced to `head` and return `Ok(Some(head))` instead of `Ok(None)`.

---

### 4. Suspicions and Edge Cases

1. **Subdirectory workspace with `uncommitted(&run.cwd)` when `run.cwd` is deleted**:
   If the agent or lead deleted the subdirectory `apps/client` during the run, `crate::worktrees::uncommitted(&run.cwd)` returns empty because `cwd` doesn't exist, preventing `changed_since_acceptance` from triggering redelivery.
2. **Detached HEAD rewound by user in repo causing `accept` to undo the rewind**:
   If the user rewound detached HEAD (`git reset --hard HEAD~1`), but the run made no commits, `accept` will fast-forward detached HEAD back to where it was, silently undoing the user's rewind.
3. **Global commit signing (`commit.gpgsign = true`)**:
   `write(path, &["commit", ...])` does not pass `--no-gpg-sign`. If the user has global GPG signing enabled with a GUI pinentry, `accept` in headless/daemon mode may hang or fail.

---

### 5. Missing Test Coverage

The following critical scenarios currently lack automated test coverage in `flow_tests/worktrees.rs` or `worktrees.rs`:

1. **Rebase on accept in integration flow**: `worktrees.rs` has a unit test, but `flow_tests/worktrees.rs` lacks an end-to-end integration test where the source branch advances during run execution and is rebased on accept.
2. **Branch switched by user during run**: Acceptance flow test when the user switches to another branch in their checkout while the run is in progress.
3. **Detached HEAD run in integration flow**: Creation and acceptance of a run started while the workspace was on a detached HEAD.
4. **`ensure` after worktree directory manual deletion**: Test verifying `ensure` can recreate a worktree whose directory was deleted by hand without failing on locked worktree error.
5. **`delete_run` with missing repository**: Test verifying `delete_run` succeeds even if the user deleted or moved the Git repository folder.
6. **Concurrent accept calls**: Test verifying that two runs on the same repository being accepted concurrently do not collide on Git index/ref locks.
7. **Non-ASCII paths in `.worktreeinclude`**: Test verifying that files with Chinese or unicode names are correctly copied.

---

### 6. Detailed Critique & Pitfalls of the Proposed Workspace Loop Worktree Plan

### Plan Overview
The proposed plan aims to assign each workspace Loop (a long-lived agent session that wakes on schedule, inspects, and dispatches goal runs) its own dedicated worktree instead of operating directly in the user's workspace directory. Before each wake:
1. The Loop's worktree is moved to the tip of the workspace's current branch.
2. Files changed by the Loop are reported to the user and discarded before the next wake.
3. Outside Git repositories, the Loop continues running in the workspace directory.

### Pitfalls & Design Traps

#### 1. Branch Checked Out Elsewhere (Fatal Git Restriction)
- **The Issue**: Git strictly forbids checking out the same branch in more than one worktree (`fatal: 'main' is already checked out at '...'`).
- **Impact**: If the workspace is on `main`, the Loop worktree cannot check out `main`.
- **Requirement**: The Loop worktree **must always remain in detached HEAD mode** (e.g. `git checkout --detach <sha>` or `git reset --hard <sha>`). It must never attach to a branch ref.

#### 2. Detached HEAD in User's Workspace
- **The Issue**: If the user's workspace is on a detached HEAD, `worktrees::current_branch(&repo)` returns `None`.
- **Requirement**: The Loop updater cannot assume a named branch exists; it must resolve `rev(repo, "HEAD")` directly.

#### 3. Sub-folder Workspaces (`apps/client`)
- **The Issue**: Git worktrees are always rooted at the top-level repository.
- **Impact**: If the workspace is configured for `apps/client`, the Loop's worktree will be rooted at the repository root.
- **Requirement**: The Loop's agent process must have its `cwd` set to `<loop_worktree_root>/apps/client`. File diffing (`dirty_before` / `touched`) and discarding must properly handle the directory offset.

#### 4. Discarding Changes vs. `.worktreeinclude`
- **The Issue**:
  - `git reset --hard` only resets tracked files; untracked files remain.
  - Untracked files left by a Loop can block future `git checkout --detach <new_commit>` if the new commit introduces files with identical names (`error: untracked working tree files would be overwritten`).
  - Running `git clean -fdx` would wipe out ignored files brought in by `.worktreeinclude` (e.g. `.env`).
  - If the Loop modifies an ignored file (e.g. editing `.env`), `git reset --hard` will not revert it.
- **Requirement**: Discarding must run *before* updating to the new tip. Untracked files should be cleaned (`git clean -fd`), but files specified in `.worktreeinclude` must be protected or freshly recopied from the user checkout after cleaning.

#### 5. Locking and Pruning
- **The Issue**: If the Loop's worktree is locked, Bug 1 and Bug 2 apply: any manual deletion or cleanup of the Loop's worktree directory will cause future `ensure` or recreation attempts to fail because `git worktree prune` ignores locked worktrees.
- **Requirement**: Implement the Bug 1 fix (unlock before prune/add) so the Loop can recover cleanly from directory loss.

#### 6. Deletion Lifecycle (`delete_loop`)
- **The Issue**: `delete_loop` calls `delete_run(&def.run)`. Currently, loop runs have `worktree: None` in their record.
- **Requirement**: Loop runs must record `worktree: { repo: ... }` in `runs.config` so that `delete_run` cleans up the worktree. Furthermore, Bug 3 must be fixed so deleting a Loop doesn't fail if the user repository was moved.

#### 7. Concurrent `git worktree add` Races
- **The Issue**: Creating or recreating the Loop's worktree will race with goal run creation if not serialized.
- **Requirement**: Always hold `self.worktree_adds` lock when adding/recreating the Loop's worktree.

#### 8. Long-Lived Agent Sessions vs. Dynamic Worktree Movement
- **The Issue**: In `loops.rs`, the Loop session can remain open across turns. If the worktree files underneath a running agent are rewritten via `git reset --hard` or `git checkout --detach` while the agent or its tool subprocesses are executing, race conditions, file descriptor invalidations, and stale caches can occur.
- **Requirement**: Worktree updates must occur strictly when the session is completely idle between wakes.

#### 9. Repository Disk Footprint
- **The Issue**: Unlike goal run worktrees which are pruned upon acceptance or deletion, Loop worktrees persist indefinitely. For large repositories with multiple Loops, this multiplies disk usage substantially.
