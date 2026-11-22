//! Each run on a git repository works in its own worktree under `<Janus home>/work/runs`, so runs
//! on one repository do not write over each other. The worktree starts from the person's `HEAD`,
//! detached, and accepting the run brings its result back where it came from: one commit
//! fast-forwarded onto the branch the person was on (or their detached `HEAD`). Janus never leaves
//! a half-done merge in the person's checkout and never overwrites their uncommitted changes: when
//! that would happen, accepting fails and the result stays in the run's commit. Nothing is pushed
//! and the repository's hooks never run.
// @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
// @project-doc docs/operations/20261031-security.md#workspace_access

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// `git` writing in `dir`, with the repository's hooks switched off: they are code in the
/// repository (an agent may have written them) and would run on checkouts, commits, rebases and
/// merges. The identity is the repository's or the person's own. See `git_probe::command` for
/// the rest of its environment.
fn write(dir: &Path, args: &[&str]) -> Result<String> {
    let out = crate::git_probe::command(dir)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .context("运行 git 失败")?;
    if !out.status.success() {
        bail!("git {}：{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A read-only `git` query (it takes no lock, see `git_probe`); `None` when git fails.
fn read(dir: &Path, args: &[&str]) -> Option<String> {
    let out = crate::git_probe::read_only(dir, args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The branch checked out in `repo` (`main` for `refs/heads/main`); `None` on a detached `HEAD`.
/// A run started there brings its result back to it.
pub fn current_branch(repo: &Path) -> Option<String> {
    read(repo, &["symbolic-ref", "-q", "HEAD"])?.trim().strip_prefix("refs/heads/").map(str::to_owned)
}

/// Make the worktree at `path` from `base` (detached, no branch) and lock it while the run owns
/// it. Returns the ignored files `.worktreeinclude` brought along. A step that fails takes back
/// what the ones before it made.
pub fn create(repo: &Path, path: &Path, base: &str, run_id: &str) -> Result<Vec<String>> {
    add(repo, path, base, run_id).context("创建任务的独立副本失败")
}

/// Bring back the worktree of a run that is working again after it was removed (accepted), at
/// `from`: the commit holding its last accepted result, otherwise where it started. Returns
/// whether it had to be made.
pub fn ensure(repo: &Path, path: &Path, from: &str, run_id: &str) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if !repo.is_dir() {
        bail!("没法重建任务的独立副本：仓库 {} 不在了（移动或删除了）", repo.display());
    }
    // A worktree directory removed by hand leaves its entry (locked) behind; git would refuse the
    // path until that one entry goes.
    drop_worktree(repo, path).context("重建任务的独立副本失败")?;
    add(repo, path, from, run_id).context("重建任务的独立副本失败")?;
    Ok(true)
}

/// `git worktree add --detach`, lock it for the run and bring the `.worktreeinclude` files; when a
/// later step fails, the worktree goes again.
fn add(repo: &Path, path: &Path, from: &str, run_id: &str) -> Result<Vec<String>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        // Spotlight leaves the copies alone: searching the Mac finds the person's files once.
        let marker = parent.join(".metadata_never_index");
        if !marker.exists() {
            std::fs::write(&marker, "")?;
        }
    }
    let p = path.to_string_lossy();
    write(repo, &["worktree", "add", "--detach", &p, from])?;
    let rest = write(repo, &["worktree", "lock", "--reason", &format!("Janus 任务 {run_id}"), &p])
        .and_then(|_| copy_included(repo, path));
    if rest.is_err()
        && let Err(err) = drop_worktree(repo, path)
    {
        crate::config::log(format!("janus: could not take back the worktree {}: {err:#}", path.display()));
    }
    rest
}

fn rev(dir: &Path, name: &str) -> Option<String> {
    read(dir, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).map(|s| s.trim().to_string())
}

fn is_ancestor(dir: &Path, older: &str, newer: &str) -> bool {
    read(dir, &["merge-base", "--is-ancestor", older, newer]).is_some()
}

fn short(commit: &str) -> &str {
    commit.get(..8).unwrap_or(commit)
}

/// Where an accepted result goes: the tip it builds on, and what moves to bring it there.
struct Source {
    /// How the person knows it, in messages.
    name: String,
    tip: String,
    at: Place,
}

enum Place {
    /// A checkout that has it checked out: its files move with it.
    Checkout(PathBuf),
    /// A branch checked out nowhere: only the ref moves.
    Ref(String),
}

impl Source {
    /// The run's `branch` as it is now, or with `None`, the detached `HEAD` of the person's
    /// checkout at `repo`, which must still be at `since` (where the run started, or where the last
    /// acceptance left it): a checkout the person moved elsewhere, back to a parent included, is
    /// theirs. `head` is the run's result, named when it cannot go back.
    fn of(repo: &Path, branch: Option<&str>, since: &str, head: &str) -> Result<Source> {
        let Some(b) = branch else {
            if let Some(now) = current_branch(repo) {
                bail!(
                    "没有合并：这个任务要合并回 {} 的 detached HEAD，但那里现在检出的是分支 {now}。任务的结果在提交 {}，可以自己合并。",
                    repo.display(),
                    short(head)
                );
            }
            let tip = rev(repo, "HEAD").context("读取工作目录的 HEAD 失败")?;
            if tip != since {
                bail!(
                    "没有合并：{} 的 detached HEAD 在任务开始后移动过（从 {} 到 {}）。任务的结果在提交 {}，可以自己合并。",
                    repo.display(),
                    short(since),
                    short(&tip),
                    short(head)
                );
            }
            return Ok(Source { name: format!("{} 的 HEAD", repo.display()), tip, at: Place::Checkout(repo.into()) });
        };
        let Some(tip) = rev(repo, &format!("refs/heads/{b}")) else {
            bail!("没有合并：分支 {b} 已经不存在。任务的结果在提交 {}，可以自己合并。", short(head));
        };
        let at = match checked_out_at(repo, b) {
            Some(dir) => Place::Checkout(dir),
            None => Place::Ref(format!("refs/heads/{b}")),
        };
        Ok(Source { name: format!("分支 {b}"), tip, at })
    }

    /// Move the source forward to `head`, which has its tip in its history. A checked-out branch
    /// moves with its files; git refuses (and nothing changes) when that would overwrite the
    /// person's uncommitted changes, and Janus refuses first when it would overwrite a file there
    /// that is not tracked (git replaces ignored ones without a word).
    fn advance(&self, repo: &Path, head: &str) -> Result<()> {
        let dir = match &self.at {
            Place::Checkout(dir) => dir,
            Place::Ref(r) => {
                write(repo, &["update-ref", r, head, &self.tip])?;
                return Ok(());
            }
        };
        let in_the_way = untracked_in_the_way(dir, &self.tip, head);
        if !in_the_way.is_empty() {
            bail!(
                "没有合并到{}：{} 里有这些没纳入版本管理的文件（包括被忽略的），任务也新建了它们：{}。先移走或改名，再验收；任务的结果在提交 {}。",
                self.name,
                dir.display(),
                in_the_way.join("、"),
                short(head)
            );
        }
        if let Err(err) = write(dir, &["merge", "--ff-only", "-q", head]) {
            let touched = read(dir, &["diff", "--name-only", &self.tip, head]).unwrap_or_default();
            let overlap: Vec<String> =
                uncommitted(dir).into_iter().filter(|f| touched.lines().any(|t| t == f)).collect();
            if overlap.is_empty() {
                bail!("没有合并到{}：{err:#}。任务的结果在提交 {}。", self.name, short(head));
            }
            bail!(
                "没有合并到{}：{} 里这些文件有还没提交的改动，任务也改了它们：{}。先提交或暂存这些改动，再验收。",
                self.name,
                dir.display(),
                overlap.join("、")
            );
        }
        Ok(())
    }
}

/// The checkout (the person's directory or another worktree of theirs) that has `branch` checked
/// out. Janus's own worktrees are detached, so never one of them; a worktree whose directory is
/// gone (removed by hand, not pruned) has nothing checked out.
fn checked_out_at(repo: &Path, branch: &str) -> Option<PathBuf> {
    let list = read(repo, &["worktree", "list", "--porcelain"])?;
    let want = format!("branch refs/heads/{branch}");
    list.split("\n\n").find_map(|entry| {
        let mut lines = entry.lines();
        let path = PathBuf::from(lines.next()?.strip_prefix("worktree ")?);
        (lines.any(|l| l == want) && path.is_dir()).then_some(path)
    })
}

/// The files `head` adds over `tip` that are in `dir` already without being tracked there.
fn untracked_in_the_way(dir: &Path, tip: &str, head: &str) -> Vec<String> {
    read(dir, &["diff", "--name-only", "-z", "--no-renames", "--diff-filter=A", tip, head])
        .unwrap_or_default()
        .split('\0')
        .filter(|f| !f.is_empty() && std::fs::symlink_metadata(dir.join(f)).is_ok())
        .map(str::to_owned)
        .collect()
}

/// What accepting a run came to.
#[derive(Debug, PartialEq)]
pub struct Accepted {
    /// The commit holding the run's result, which the source has now: where its worktree comes
    /// back from, and where the source must still be (or have moved on from) next time.
    pub result: String,
    /// Whether accepting moved the source to it; `false` when the source had it already.
    pub brought: bool,
}

/// Accept a run: commit everything it changed in its worktree, and bring the result back to
/// `branch` (where the person's checkout was when the run started; `None` for a detached `HEAD`)
/// as one fast-forward. `since` is where the run started, or the result its last acceptance left:
/// the source must still be there or have moved on from it. When the branch moved on, the run's
/// own commits (those after `since`) are replayed onto it in the worktree first, so history stays
/// linear. A conflict there, a branch rewritten behind the run (reset, force-pushed, made again),
/// a detached `HEAD` that moved, or files in the person's checkout the result would overwrite leave
/// the person's side as it was and fail, with the result kept in the worktree's commit. A worktree
/// that is gone is not "nothing new": it fails, and the run stays delivered. With `remove` (no
/// session works there) the worktree goes afterwards; that failing is logged, the result is back.
pub fn accept(
    repo: &Path,
    path: &Path,
    branch: Option<&str>,
    since: &str,
    message: &str,
    remove: bool,
) -> Result<Accepted> {
    if !path.exists() {
        bail!(
            "没有合并：这个任务的独立副本不在了，无法核对它的结果。删除任务时没提交的改动存在 Janus 目录下的 data/deleted-runs/。"
        );
    }
    write(path, &["add", "-A"])?;
    let staged = !crate::git_probe::command(path)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("运行 git 失败")?
        .success();
    if staged {
        write(path, &["commit", "-q", "-m", message]).context("提交失败：请确认 git 配置了 user.name 和 user.email")?;
    }
    let mut head = rev(path, "HEAD").context("读取任务副本的提交失败")?;
    let source = Source::of(repo, branch, since, &head)?;
    if !is_ancestor(path, since, &source.tip) {
        bail!(
            "没有合并：{} 在任务开始后被改写过（回退、强推或重新建过），不再包含任务开始时的提交 {}。任务的结果在提交 {}，可以自己合并。",
            source.name,
            short(since),
            short(&head)
        );
    }
    let done = |result: String, brought: bool| {
        if remove && let Err(err) = drop_worktree(repo, path) {
            crate::config::log(format!("janus: could not remove the accepted worktree {}: {err:#}", path.display()));
        }
        Accepted { result, brought }
    };
    if is_ancestor(path, &head, &source.tip) {
        // Nothing the source does not have already.
        return Ok(done(head, false));
    }
    if !is_ancestor(path, &source.tip, &head) {
        if branch.is_none() {
            bail!("没有合并：{} 在任务开始后移动过。任务的结果在提交 {}，可以自己合并。", source.name, short(&head));
        }
        // Only the run's own commits: a commit of the branch the person rewrote since is not
        // replayed again.
        if let Err(err) = write(path, &["rebase", "-q", "--onto", &source.tip, since]) {
            let conflicted = read(path, &["diff", "--name-only", "-z", "--diff-filter=U"]).unwrap_or_default();
            let files: Vec<&str> = conflicted.split('\0').filter(|f| !f.is_empty()).collect();
            let _ = write(path, &["rebase", "--abort"]);
            crate::config::log(format!("janus: replaying {} onto {} failed: {err:#}", short(&head), source.name));
            bail!(
                "没有合并：{} 在任务开始后有了新的提交，和任务的改动冲突{}。任务的结果在提交 {}，可以在你的目录里用 git merge {} 自己合并。",
                source.name,
                if files.is_empty() { String::new() } else { format!("（{}）", files.join("、")) },
                short(&head),
                short(&head)
            );
        }
        head = rev(path, "HEAD").context("读取任务副本的提交失败")?;
    }
    // A replay finds nothing new when the person already brought the same change over.
    if head == source.tip {
        return Ok(done(head, false));
    }
    source.advance(repo, &head)?;
    Ok(done(head, true))
}

/// Data version 10: a run in a worktree records the branch accepting brings its result back to
/// (`runs.config.worktree.branch`, `null` for a detached `HEAD`). Runs from before take the branch
/// the checkout is on now when the run's start is in its history, so replaying the run's commits
/// there replays only them; otherwise `null`, and accepting such a run leaves merging its commit
/// to the person unless the checkout is detached where the run can fast-forward it.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn record_source_branch(tx: &rusqlite::Transaction) -> Result<usize> {
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, config FROM runs WHERE json_extract(config, '$.worktree.repo') IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = 0;
    for (id, config) in rows {
        let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&config) else { continue };
        if config["worktree"].get("branch").is_some() {
            continue;
        }
        let repo = PathBuf::from(config["worktree"]["repo"].as_str().unwrap_or_default());
        let base = config["baseRef"].as_str().unwrap_or_default();
        let branch = current_branch(&repo).filter(|b| {
            rev(&repo, &format!("refs/heads/{b}")).is_some_and(|tip| !base.is_empty() && is_ancestor(&repo, base, &tip))
        });
        config["worktree"]["branch"] = serde_json::json!(branch);
        tx.execute("UPDATE runs SET config = ?1 WHERE id = ?2", rusqlite::params![config.to_string(), id])?;
        changed += 1;
    }
    Ok(changed)
}

/// Remove a run's worktree because the run is deleted. Changes not committed yet are kept as a
/// patch under `kept` first (ignored files, such as a copied `.env`, are not: a patch would carry
/// credentials); returns its path. When the repository is gone (moved or deleted), git cannot
/// read the worktree any more: its directory is moved under `kept` as it is.
pub fn remove(repo: &Path, path: &Path, kept: &Path, run_id: &str) -> Result<Option<PathBuf>> {
    if !repo.is_dir() || (path.exists() && read(path, &["rev-parse", "--git-dir"]).is_none()) {
        if !path.exists() {
            return Ok(None);
        }
        std::fs::create_dir_all(kept).context("创建保留目录失败，任务的副本保留，没有删除")?;
        let to = kept.join(run_id);
        std::fs::rename(path, &to).context("移走任务的副本失败，副本保留，没有删除")?;
        return Ok(Some(to));
    }
    if !path.exists() {
        // Removed by hand: its entry (locked) is still in the repository.
        drop_worktree(repo, path)?;
        return Ok(None);
    }
    // Nothing goes before the changes are safe: a step that fails leaves the worktree in place.
    // @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
    write(path, &["add", "-A"]).context("暂存副本里的改动失败，任务的副本保留，没有删除")?;
    let patch = write(path, &["diff", "--cached", "--binary", "HEAD"])
        .context("读取副本里没提交的改动失败，任务的副本保留，没有删除")?;
    let mut saved = None;
    if !patch.trim().is_empty() {
        std::fs::create_dir_all(kept).context("创建补丁目录失败，任务的副本保留，没有删除")?;
        let file = kept.join(format!("{run_id}.patch"));
        std::fs::write(&file, patch).context("保存补丁失败，任务的副本保留，没有删除")?;
        saved = Some(file);
    }
    drop_worktree(repo, path)?;
    Ok(saved)
}

/// Tell the repository its worktree moved to `path` (a data upgrade moved Janus home's folders).
/// It only rewrites the two links between them; when the repository is gone there is nothing to
/// tell. Done again at every start for a worktree git cannot read (`heal`).
pub fn repair(repo: &Path, path: &Path) {
    if let Err(err) = write(repo, &["worktree", "repair", &path.to_string_lossy()]) {
        crate::config::log(format!(
            "janus: could not tell {} its worktree moved to {}: {err:#}",
            repo.display(),
            path.display()
        ));
    }
}

/// Repair a worktree that git cannot read although its repository is there (a move of Janus home
/// whose repair failed then).
pub fn heal(repo: &Path, path: &Path) {
    if repo.is_dir() && path.is_dir() && read(path, &["rev-parse", "--git-dir"]).is_none() {
        repair(repo, path);
    }
}

/// Remove the worktree at `path` and its entry in the repository, locked or not, with its
/// directory there or already gone; only that entry, never other worktrees of the person. Fails
/// when either is still there afterwards.
fn drop_worktree(repo: &Path, path: &Path) -> Result<()> {
    let p = path.to_string_lossy();
    if let Err(err) = write(repo, &["worktree", "remove", "--force", "--force", &p])
        && registered(repo, path)
    {
        return Err(err.context("删除任务的副本失败"));
    }
    if path.exists() {
        std::fs::remove_dir_all(path).with_context(|| format!("删除 {} 失败", path.display()))?;
    }
    Ok(())
}

/// Whether the repository has a worktree entry for `path`.
fn registered(repo: &Path, path: &Path) -> bool {
    let place =
        |p: &Path| p.canonicalize().ok().or_else(|| Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?)));
    let want = place(path);
    read(repo, &["worktree", "list", "--porcelain"]).unwrap_or_default().lines().any(|l| {
        l.strip_prefix("worktree ")
            .is_some_and(|w| Path::new(w) == path || (want.is_some() && place(Path::new(w)) == want))
    })
}

/// Files with changes not committed in the person's checkout: a new run starts from `HEAD`
/// without them, which the person is reminded of.
pub fn uncommitted(repo: &Path) -> Vec<String> {
    changed_files(repo).unwrap_or_default()
}

/// Files with changes not committed in `dir`; `None` outside a git repository (or when git
/// fails), where changes cannot be seen.
pub fn changed_files(dir: &Path) -> Option<Vec<String>> {
    let out = read(dir, &["status", "--porcelain", "-z", "--untracked-files=normal"])?;
    let mut files = vec![];
    let mut entries = out.split('\0').filter(|e| !e.is_empty());
    while let Some(e) = entries.next() {
        files.extend(e.get(3..).map(str::to_owned));
        // A rename or copy names where it came from next: both are changed.
        if matches!(e.as_bytes().first(), Some(b'R' | b'C')) {
            files.extend(entries.next().map(str::to_owned));
        }
    }
    Some(files)
}

/// Copy the ignored files `.worktreeinclude` names from the checkout into the worktree, the way
/// Claude Code and Codex do: `.gitignore` syntax, only files that are also ignored (tracked ones
/// are in the worktree already), no symbolic links, nothing already there overwritten.
fn copy_included(repo: &Path, path: &Path) -> Result<Vec<String>> {
    let include = repo.join(".worktreeinclude");
    if !include.is_file() {
        return Ok(vec![]);
    }
    // NUL-separated: a name outside ASCII (`配置.env`) would otherwise come quoted and escaped.
    let listed = read(
        repo,
        &["ls-files", "-z", "--others", "--ignored", &format!("--exclude-from={}", include.to_string_lossy())],
    )
    .unwrap_or_default();
    let candidates: Vec<&str> = listed.split('\0').filter(|l| !l.is_empty()).collect();
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let ignored = ignored_of(repo, &candidates);
    let mut copied = vec![];
    for rel in candidates.into_iter().filter(|r| ignored.iter().any(|i| i == r)) {
        let from = repo.join(rel);
        let to = path.join(rel);
        let Ok(meta) = std::fs::symlink_metadata(&from) else { continue };
        if !meta.is_file() || to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to).with_context(|| format!("复制 {rel} 失败"))?;
        copied.push(rel.to_string());
    }
    Ok(copied)
}

/// Which of `paths` the repository's own ignore rules ignore (`git check-ignore`).
fn ignored_of(repo: &Path, paths: &[&str]) -> Vec<String> {
    use std::io::Write;
    let Ok(mut child) = crate::git_probe::read_only(repo, &["check-ignore", "-z", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return vec![];
    };
    // Written from another thread while the answer is read here: git writes each ignored path as
    // it reads it, so with many paths both pipes fill and neither side would move (AT-01).
    let input = paths.join("\0");
    let writer = child.stdin.take().map(|mut stdin| std::thread::spawn(move || stdin.write_all(input.as_bytes())));
    let out = child
        .wait_with_output()
        .map(|o| String::from_utf8_lossy(&o.stdout).split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(w) = writer {
        let _ = w.join();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-worktrees-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let r = dir.join("repo");
        let git = |args: &[&str]| assert!(Command::new("git").args(args).current_dir(&r).status().unwrap().success());
        git(&["init", "-q"]);
        git(&["config", "user.name", "t"]);
        git(&["config", "user.email", "t@t"]);
        std::fs::write(r.join(".gitignore"), ".env\nnode_modules/\n.secret\n").unwrap();
        std::fs::write(r.join(".worktreeinclude"), ".env\nREADME.md\n").unwrap();
        std::fs::write(r.join("README.md"), "tracked\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        std::fs::write(r.join(".env"), "KEY=1\n").unwrap();
        std::fs::write(r.join(".secret"), "no\n").unwrap();
        std::fs::create_dir_all(r.join("node_modules/x")).unwrap();
        std::fs::write(r.join("node_modules/x/i.js"), "x\n").unwrap();
        dir
    }

    fn head(dir: &Path) -> String {
        read(dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string()
    }

    /// Only ignored files `.worktreeinclude` names come along: not the ignored ones it leaves out,
    /// not tracked ones (already there), and the person's checkout is untouched.
    #[test]
    fn a_worktree_has_the_code_and_the_included_ignored_files_only() {
        let dir = repo("create");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/r1"));
        std::fs::write(r.join("README.md"), "edited, not committed\n").unwrap();
        let copied = create(&r, &w, &head(&r), "r1").unwrap();
        assert_eq!(copied, vec![".env"]);
        assert_eq!(std::fs::read_to_string(w.join(".env")).unwrap(), "KEY=1\n");
        assert_eq!(std::fs::read_to_string(w.join("README.md")).unwrap(), "tracked\n", "from HEAD, not the edit");
        assert!(!w.join(".secret").exists() && !w.join("node_modules").exists());
        assert_eq!(uncommitted(&r), vec!["README.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const HOOKS: [&str; 5] = ["pre-commit", "post-commit", "post-merge", "post-checkout", "post-rewrite"];

    fn git(dir: &Path, args: &[&str]) {
        assert!(Command::new("git").args(args).current_dir(dir).status().unwrap().success(), "git {args:?}");
    }

    fn file(dir: &Path, name: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(name)).ok()
    }

    /// The run's result comes back to the branch the person is on as one commit, with the files
    /// in their checkout; their uncommitted change to another file stays, and no hook runs.
    #[test]
    fn accepting_fast_forwards_the_branch_the_person_is_on() {
        let dir = repo("accept");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/r2"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        // The repository's hooks are its own code; none runs on Janus's writes.
        for hook in HOOKS {
            let h = r.join(".git/hooks").join(hook);
            std::fs::write(&h, format!("#!/bin/sh\ntouch \"{}/{hook}-ran\"\n", dir.display())).unwrap();
            std::fs::set_permissions(&h, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        }
        create(&r, &w, &base, "r2").unwrap();
        std::fs::write(w.join("new.txt"), "from the run\n").unwrap();
        std::fs::write(r.join("README.md"), "the person's own edit\n").unwrap();
        let commit = accept(&r, &w, Some(&branch), &base, "Janus 任务 r2", false).unwrap();
        assert!(commit.brought);
        let commit = commit.result;
        assert_eq!(head(&r), commit, "the branch moved forward");
        assert_eq!(read(&r, &["rev-parse", "HEAD^"]).unwrap().trim(), base, "one commit on top, no merge");
        assert_eq!(file(&r, "new.txt").as_deref(), Some("from the run\n"), "the checkout has the result");
        assert_eq!(file(&r, "README.md").as_deref(), Some("the person's own edit\n"), "their change stays");
        assert_eq!(current_branch(&r).as_deref(), Some(branch.as_str()));
        assert!(read(&w, &["symbolic-ref", "-q", "HEAD"]).is_none(), "the worktree stays detached");
        for hook in HOOKS {
            assert!(!dir.join(format!("{hook}-ran")).exists(), "{hook} ran");
        }
        assert!(read(&r, &["branch", "--list", "janus/*"]).unwrap().trim().is_empty(), "no branch of its own");

        // Nothing new: nothing to bring back. More work later is one more commit on the branch.
        let again = accept(&r, &w, Some(&branch), &commit, "Janus 任务 r2", false).unwrap();
        assert_eq!(again, Accepted { result: commit.clone(), brought: false });
        std::fs::write(w.join("second.txt"), "more work\n").unwrap();
        let second = accept(&r, &w, Some(&branch), &commit, "Janus 任务 r2", true).unwrap().result;
        assert_eq!(read(&r, &["rev-parse", "HEAD^"]).unwrap().trim(), commit);
        assert!(!w.exists(), "removed when no session works there");
        // Working again brings the worktree back where the last acceptance left the result.
        assert!(ensure(&r, &w, &second, "r2").unwrap());
        assert_eq!(file(&w, "second.txt").as_deref(), Some("more work\n"));
        assert!(!ensure(&r, &w, &second, "r2").unwrap(), "already there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The branch moved on while the run worked: the run's commit is replayed on top of it
    /// (linear history). A conflicting replay leaves the branch and the checkout as they were.
    #[test]
    fn a_branch_that_moved_on_gets_the_result_replayed_on_top_or_nothing() {
        let dir = repo("moved");
        let (r, w1, w2) = (dir.join("repo"), dir.join("home/work/runs/a"), dir.join("home/work/runs/b"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        create(&r, &w1, &base, "a").unwrap();
        create(&r, &w2, &base, "b").unwrap();
        std::fs::write(w1.join("a.txt"), "run a\n").unwrap();
        std::fs::write(w2.join("a.txt"), "run b\n").unwrap();
        std::fs::write(r.join("mine.txt"), "the person's commit\n").unwrap();
        git(&r, &["add", "mine.txt"]);
        git(&r, &["commit", "-q", "-m", "mine"]);
        let mine = head(&r);

        let a = accept(&r, &w1, Some(&branch), &base, "a", false).unwrap().result;
        assert_eq!(head(&r), a);
        assert_eq!(read(&r, &["rev-parse", "HEAD^"]).unwrap().trim(), mine, "replayed on the person's commit");
        assert_eq!(read(&r, &["rev-list", "--merges", "--count", "HEAD"]).unwrap().trim(), "0");
        assert_eq!(head(&w1), a, "the worktree holds what the branch has");

        let err = accept(&r, &w2, Some(&branch), &base, "b", true).unwrap_err().to_string();
        assert!(err.contains("冲突"), "{err}");
        assert_eq!(head(&r), a, "the branch did not move");
        assert_eq!(file(&r, "a.txt").as_deref(), Some("run a\n"));
        assert!(read(&r, &["status", "--porcelain"]).unwrap().trim().is_empty(), "nothing half-merged");
        assert!(w2.exists(), "kept with its result: a failed acceptance changes nothing");
        assert_eq!(read(&w2, &["show", "HEAD:a.txt"]).unwrap(), "run b\n", "the result is committed there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The person's uncommitted change to a file the run also changed is never overwritten. A
    /// branch checked out nowhere just moves.
    #[test]
    fn uncommitted_changes_in_the_way_stop_it_and_a_branch_checked_out_nowhere_just_moves() {
        let dir = repo("dirty");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/d"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        create(&r, &w, &base, "d").unwrap();
        std::fs::write(w.join("README.md"), "from the run\n").unwrap();
        std::fs::write(r.join("README.md"), "the person's draft\n").unwrap();
        let err = accept(&r, &w, Some(&branch), &base, "d", false).unwrap_err().to_string();
        assert!(err.contains("README.md") && err.contains("先提交或暂存"), "{err}");
        assert_eq!(head(&r), base);
        assert_eq!(file(&r, "README.md").as_deref(), Some("the person's draft\n"));

        // The person moves to another branch: theirs is checked out nowhere, so only it moves.
        git(&r, &["checkout", "-q", "-b", "elsewhere"]);
        let commit = accept(&r, &w, Some(&branch), &base, "d", false).unwrap().result;
        assert_eq!(read(&r, &["rev-parse", &branch]).unwrap().trim(), commit);
        assert_eq!(head(&r), base, "the checkout on the other branch did not move");
        assert_eq!(file(&r, "README.md").as_deref(), Some("the person's draft\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run started on a detached `HEAD` brings its result back to it while the checkout is still
    /// there, and refuses once it moved.
    #[test]
    fn a_detached_head_moves_forward_only_from_where_it_was() {
        let dir = repo("detached");
        let (r, w1, w2) = (dir.join("repo"), dir.join("home/work/runs/x"), dir.join("home/work/runs/y"));
        git(&r, &["checkout", "-q", "--detach"]);
        let base = head(&r);
        assert!(current_branch(&r).is_none());
        create(&r, &w1, &base, "x").unwrap();
        create(&r, &w2, &base, "y").unwrap();
        std::fs::write(w1.join("x.txt"), "x\n").unwrap();
        std::fs::write(w2.join("y.txt"), "y\n").unwrap();
        let x = accept(&r, &w1, None, &base, "x", false).unwrap().result;
        assert_eq!(head(&r), x);
        assert!(current_branch(&r).is_none() && file(&r, "x.txt").is_some());
        let err = accept(&r, &w2, None, &base, "y", false).unwrap_err().to_string();
        assert!(err.contains("移动过"), "{err}");
        assert_eq!(head(&r), x);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A detached `HEAD` the person moved back to a parent of where the run started is theirs too:
    /// accepting leaves it there. A worktree that is gone fails instead of passing as "nothing new".
    #[test]
    fn a_detached_head_moved_back_and_a_missing_worktree_are_refused() {
        let dir = repo("detached-back");
        let r = dir.join("repo");
        std::fs::write(r.join("b.txt"), "b\n").unwrap();
        git(&r, &["add", "-A"]);
        git(&r, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "b"]);
        git(&r, &["checkout", "-q", "--detach"]);
        let base = head(&r);
        let w = dir.join("home/work/runs/z");
        create(&r, &w, &base, "z").unwrap();
        std::fs::write(w.join("z.txt"), "z\n").unwrap();
        git(&r, &["checkout", "-q", "HEAD~1"]);
        let parent = head(&r);
        let err = accept(&r, &w, None, &base, "z", false).unwrap_err().to_string();
        assert!(err.contains("移动过"), "{err}");
        assert_eq!(head(&r), parent, "the person's choice stays");
        assert!(file(&r, "b.txt").is_none());

        drop_worktree(&r, &w).unwrap();
        let err = accept(&r, &w, None, &base, "z", false).unwrap_err().to_string();
        assert!(err.contains("不在了"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// When the changes cannot be saved as a patch (git's index is locked), deleting fails and the
    /// worktree with its files stays; once the lock is gone it goes, with the patch.
    #[test]
    fn deleting_keeps_the_worktree_when_its_changes_cannot_be_saved() {
        let dir = repo("locked");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/r4"));
        let base = head(&r);
        create(&r, &w, &base, "r4").unwrap();
        std::fs::write(w.join("only-here.txt"), "work\n").unwrap();
        let git_dir = PathBuf::from(read(&w, &["rev-parse", "--absolute-git-dir"]).unwrap().trim());
        std::fs::write(git_dir.join("index.lock"), "").unwrap();
        let kept = dir.join("home/data/deleted-runs");
        assert!(remove(&r, &w, &kept, "r4").is_err());
        assert!(w.join("only-here.txt").exists(), "the file is still there");
        std::fs::remove_file(git_dir.join("index.lock")).unwrap();
        let patch = remove(&r, &w, &kept, "r4").unwrap().unwrap();
        assert!(std::fs::read_to_string(patch).unwrap().contains("work"));
        assert!(!w.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deleting keeps uncommitted changes as a patch; a run that changed nothing brings nothing.
    #[test]
    fn deleting_keeps_a_patch_and_nothing_changed_brings_nothing() {
        let dir = repo("nothing");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/r3"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        create(&r, &w, &base, "r3").unwrap();
        assert!(!accept(&r, &w, Some(&branch), &base, "r3", false).unwrap().brought);
        assert_eq!(head(&r), base);
        std::fs::write(w.join("more.txt"), "unfinished\n").unwrap();
        let kept = remove(&r, &w, &dir.join("home/data/deleted-runs"), "r3").unwrap().unwrap();
        assert!(std::fs::read_to_string(kept).unwrap().contains("unfinished"));
        assert!(!w.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worktree whose directory was removed by hand stays in the repository, locked: rebuilding
    /// it, deleting the run and making one at that path again all work, and only that entry goes.
    #[test]
    fn a_worktree_removed_by_hand_is_rebuilt_and_deleted_cleanly() {
        let dir = repo("by-hand");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/h"));
        let base = head(&r);
        create(&r, &w, &base, "h").unwrap();
        // One of the person's own worktrees, gone too but not Janus's to clean.
        let theirs = dir.join("theirs");
        git(&r, &["worktree", "add", "-q", "--detach", &theirs.to_string_lossy()]);
        std::fs::remove_dir_all(&theirs).unwrap();
        std::fs::remove_dir_all(&w).unwrap();
        assert!(ensure(&r, &w, &base, "h").unwrap(), "rebuilt although its entry was locked");
        assert_eq!(file(&w, "README.md").as_deref(), Some("tracked\n"));
        std::fs::remove_dir_all(&w).unwrap();
        assert_eq!(remove(&r, &w, &dir.join("kept"), "h").unwrap(), None);
        assert!(!registered(&r, &w), "its entry went with it");
        assert!(registered(&r, &theirs), "the person's stale worktree is theirs to prune");
        create(&r, &w, &base, "h").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A step after `git worktree add` that fails (an included file cannot be read) takes the
    /// worktree back: no directory, no entry.
    #[test]
    fn a_worktree_that_cannot_be_finished_is_taken_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = repo("half");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/f"));
        std::fs::set_permissions(r.join(".env"), std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(create(&r, &w, &head(&r), "f").is_err());
        assert!(!w.exists() && !registered(&r, &w));
        std::fs::set_permissions(r.join(".env"), std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Accepting moves the branch only forward from where the run started: a branch the person
    /// reset (or rewrote) behind it is refused, so history they removed does not come back.
    #[test]
    fn a_branch_rewritten_behind_the_run_is_refused() {
        let dir = repo("rewritten");
        let r = dir.join("repo");
        let first = head(&r);
        std::fs::write(r.join("b.txt"), "b\n").unwrap();
        git(&r, &["add", "b.txt"]);
        git(&r, &["commit", "-q", "-m", "b"]);
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        let w = dir.join("home/work/runs/rw");
        create(&r, &w, &base, "rw").unwrap();
        std::fs::write(w.join("run.txt"), "run\n").unwrap();
        git(&r, &["reset", "-q", "--hard", &first]);
        let err = accept(&r, &w, Some(&branch), &base, "rw", false).unwrap_err().to_string();
        assert!(err.contains("改写过"), "{err}");
        assert_eq!(head(&r), first, "what the person removed stays removed");
        assert!(file(&r, "b.txt").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file the result adds that the person's checkout has untracked, ignored ones included, is
    /// not overwritten: accepting fails and names it.
    #[test]
    fn an_ignored_file_in_the_way_is_not_overwritten() {
        let dir = repo("ignored");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/i"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        create(&r, &w, &base, "i").unwrap();
        std::fs::write(w.join(".secret"), "from the run\n").unwrap();
        git(&w, &["add", "-f", ".secret"]);
        let err = accept(&r, &w, Some(&branch), &base, "i", false).unwrap_err().to_string();
        assert!(err.contains(".secret") && err.contains("没纳入版本管理"), "{err}");
        assert_eq!(file(&r, ".secret").as_deref(), Some("no\n"));
        assert_eq!(head(&r), base);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The person brought the result over already: accepting brings nothing, and records the commit
    /// that holds it, so the worktree comes back with it.
    #[test]
    fn a_result_already_on_the_branch_is_recorded() {
        let dir = repo("integrated");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/g"));
        let base = head(&r);
        let branch = current_branch(&r).unwrap();
        create(&r, &w, &base, "g").unwrap();
        std::fs::write(w.join("g.txt"), "g\n").unwrap();
        git(&w, &["add", "-A"]);
        git(&w, &["commit", "-q", "-m", "g"]);
        let result = head(&w);
        git(&r, &["merge", "-q", "--ff-only", &result]);
        assert_eq!(
            accept(&r, &w, Some(&branch), &base, "g", true).unwrap(),
            Accepted { result: result.clone(), brought: false }
        );
        assert!(ensure(&r, &w, &result, "g").unwrap());
        assert_eq!(file(&w, "g.txt").as_deref(), Some("g\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The branch's other worktree was removed by hand (not pruned): the branch is checked out
    /// nowhere, so accepting moves only it.
    #[test]
    fn a_branch_whose_checkout_is_gone_just_moves() {
        let dir = repo("stale");
        let r = dir.join("repo");
        let other = dir.join("other");
        git(&r, &["worktree", "add", "-q", "-b", "feat", &other.to_string_lossy()]);
        let base = head(&other);
        let w = dir.join("home/work/runs/s");
        create(&r, &w, &base, "s").unwrap();
        std::fs::write(w.join("s.txt"), "s\n").unwrap();
        std::fs::remove_dir_all(&other).unwrap();
        let got = accept(&r, &w, Some("feat"), &base, "s", false).unwrap();
        assert!(got.brought);
        assert_eq!(read(&r, &["rev-parse", "feat"]).unwrap().trim(), got.result);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The repository moved or was deleted: git cannot read the worktree, so deleting the run moves
    /// its directory, with its files, under the kept folder.
    #[test]
    fn a_worktree_whose_repository_is_gone_is_kept_as_it_is() {
        let dir = repo("gone");
        let (r, w) = (dir.join("repo"), dir.join("home/work/runs/o"));
        create(&r, &w, &head(&r), "o").unwrap();
        std::fs::write(w.join("work.txt"), "work\n").unwrap();
        std::fs::rename(&r, dir.join("moved")).unwrap();
        let kept = remove(&r, &w, &dir.join("kept"), "o").unwrap().unwrap();
        assert_eq!(file(&kept, "work.txt").as_deref(), Some("work\n"));
        assert!(!w.exists());
        assert!(ensure(&r, &w, "HEAD", "o").unwrap_err().to_string().contains("不在了"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Names outside ASCII come along through `.worktreeinclude`, and a rename counts both names
    /// as changed.
    #[test]
    fn names_outside_ascii_and_renames_are_read_as_they_are() {
        let dir = repo("names");
        let r = dir.join("repo");
        std::fs::write(r.join(".gitignore"), ".env\nnode_modules/\n.secret\n配置.env\n").unwrap();
        std::fs::write(r.join(".worktreeinclude"), ".env\n配置.env\n").unwrap();
        git(&r, &["add", "-A"]);
        git(&r, &["commit", "-q", "-m", "names"]);
        std::fs::write(r.join("配置.env"), "k=v\n").unwrap();
        let w = dir.join("home/work/runs/n");
        let mut copied = create(&r, &w, &head(&r), "n").unwrap();
        copied.sort();
        assert_eq!(copied, vec![".env", "配置.env"]);
        git(&r, &["mv", "README.md", "说明.md"]);
        let mut changed = uncommitted(&r);
        changed.sort();
        assert_eq!(changed, vec!["README.md", "说明.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
