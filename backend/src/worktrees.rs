//! Each run on a git repository works in its own worktree under `<Janus home>/worktrees`, so runs
//! on one repository do not write over each other. These are the only writes Janus makes to the
//! person's repository: the worktree's metadata, the `janus/<run-id>` branch and the commit made
//! when the person accepts the run. Nothing is pushed and the repository's hooks never run.
// @project-doc docs/domains/20261031-run_lifecycle.md#workspace_model
// @project-doc docs/operations/20261031-security.md#workspace_access

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The branch a run's accepted result is committed to.
pub fn branch(run_id: &str) -> String {
    format!("janus/{run_id}")
}

/// `git` writing in `dir`. The identity is the repository's or the person's own.
fn write(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().context("运行 git 失败")?;
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

/// Make the worktree at `path` from `base` (detached, no branch) and lock it while the run owns
/// it. Returns the ignored files `.worktreeinclude` brought along.
pub fn create(repo: &Path, path: &Path, base: &str, run_id: &str) -> Result<Vec<String>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy();
    write(repo, &["worktree", "add", "--detach", &p, base]).context("创建 worktree 失败")?;
    write(repo, &["worktree", "lock", "--reason", &format!("Janus 任务 {run_id}"), &p])?;
    copy_included(repo, path)
}

/// Bring back the worktree of a run that is working again after it was removed (accepted): at its
/// branch when the accepted result was committed there, otherwise at `base`. Always detached, so
/// the branch stays free for the person. Returns whether it had to be made.
pub fn ensure(repo: &Path, path: &Path, base: &str, run_id: &str) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A worktree directory removed by hand leaves its entry behind; git would refuse the path.
    let _ = write(repo, &["worktree", "prune"]);
    let p = path.to_string_lossy();
    let branch = branch(run_id);
    let from = if branch_tip(repo, &branch).is_some() { branch.as_str() } else { base };
    write(repo, &["worktree", "add", "--detach", &p, from]).context("重建 worktree 失败")?;
    write(repo, &["worktree", "lock", "--reason", &format!("Janus 任务 {run_id}"), &p])?;
    copy_included(repo, path)?;
    Ok(true)
}

fn branch_tip(repo: &Path, branch: &str) -> Option<String> {
    read(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).map(|s| s.trim().to_string())
}

/// What accepting a run left in the repository.
pub struct Accepted {
    /// The branch and commit holding the result; `None` when the run changed nothing.
    pub commit: Option<(String, String)>,
}

/// Commit everything the run changed and point `janus/<run-id>` at it: the branch is what the
/// person merges. The worktree stays detached, so the branch is never checked out there; a later
/// acceptance of the same run moves the branch forward, and refuses when the branch was moved
/// meanwhile. Hooks do not run (`--no-verify`). With `remove`, the worktree goes afterwards (no
/// session works in it any more). A failed commit (for example no git identity) leaves it as it was.
pub fn accept(repo: &Path, path: &Path, run_id: &str, message: &str, remove: bool) -> Result<Accepted> {
    if !path.exists() {
        return Ok(Accepted { commit: None });
    }
    write(path, &["add", "-A"])?;
    let changed = !Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(path)
        .status()
        .context("运行 git 失败")?
        .success();
    let commit = if changed {
        let branch = branch(run_id);
        let parent = write(path, &["rev-parse", "HEAD"])?.trim().to_string();
        write(path, &["commit", "-q", "--no-verify", "-m", message])
            .context("提交失败：请确认 git 配置了 user.name 和 user.email")?;
        let head = write(path, &["rev-parse", "HEAD"])?.trim().to_string();
        let r = format!("refs/heads/{branch}");
        match branch_tip(repo, &branch) {
            None => write(repo, &["update-ref", &r, &head, ""])?,
            Some(tip) if tip == parent => write(repo, &["update-ref", &r, &head, &parent])?,
            Some(_) => bail!("分支 {branch} 在上次验收后被改动过，没有覆盖它；这次的提交是 {head}，可以自己合并"),
        };
        Some((branch, head))
    } else {
        None
    };
    if remove {
        drop_worktree(repo, path);
    }
    Ok(Accepted { commit })
}

/// Remove a run's worktree because the run is deleted. Changes not committed yet are kept as a
/// patch under `kept` first; returns its path.
pub fn remove(repo: &Path, path: &Path, kept: &Path, run_id: &str) -> Result<Option<PathBuf>> {
    if !path.exists() {
        let _ = write(repo, &["worktree", "prune"]);
        return Ok(None);
    }
    let mut saved = None;
    if write(path, &["add", "-A"]).is_ok()
        && let Ok(patch) = write(path, &["diff", "--cached", "--binary", "HEAD"])
        && !patch.trim().is_empty()
    {
        std::fs::create_dir_all(kept)?;
        let file = kept.join(format!("{run_id}.patch"));
        std::fs::write(&file, patch)?;
        saved = Some(file);
    }
    drop_worktree(repo, path);
    Ok(saved)
}

/// Unlock and remove the worktree; when the repository cannot (it is gone or refuses), the
/// directory goes anyway and git forgets the entry next time it prunes.
fn drop_worktree(repo: &Path, path: &Path) {
    let p = path.to_string_lossy();
    let _ = write(repo, &["worktree", "unlock", &p]);
    if write(repo, &["worktree", "remove", "--force", &p]).is_err() {
        let _ = std::fs::remove_dir_all(path);
        let _ = write(repo, &["worktree", "prune"]);
    }
}

/// Files with changes not committed in the person's checkout: a new run starts from `HEAD`
/// without them, which the person is reminded of.
pub fn uncommitted(repo: &Path) -> Vec<String> {
    read(repo, &["status", "--porcelain", "--untracked-files=normal"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.get(3..).map(str::to_owned))
        .collect()
}

/// Copy the ignored files `.worktreeinclude` names from the checkout into the worktree, the way
/// Claude Code and Codex do: `.gitignore` syntax, only files that are also ignored (tracked ones
/// are in the worktree already), no symbolic links, nothing already there overwritten.
fn copy_included(repo: &Path, path: &Path) -> Result<Vec<String>> {
    let include = repo.join(".worktreeinclude");
    if !include.is_file() {
        return Ok(vec![]);
    }
    let listed =
        read(repo, &["ls-files", "--others", "--ignored", &format!("--exclude-from={}", include.to_string_lossy())])
            .unwrap_or_default();
    let candidates: Vec<&str> = listed.lines().filter(|l| !l.is_empty()).collect();
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
    let Ok(mut child) = crate::git_probe::read_only(repo, &["check-ignore", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return vec![];
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(paths.join("\n").as_bytes());
    }
    child
        .wait_with_output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (r, w) = (dir.join("repo"), dir.join("home/worktrees/r1"));
        std::fs::write(r.join("README.md"), "edited, not committed\n").unwrap();
        let copied = create(&r, &w, &head(&r), "r1").unwrap();
        assert_eq!(copied, vec![".env"]);
        assert_eq!(std::fs::read_to_string(w.join(".env")).unwrap(), "KEY=1\n");
        assert_eq!(std::fs::read_to_string(w.join("README.md")).unwrap(), "tracked\n", "from HEAD, not the edit");
        assert!(!w.join(".secret").exists() && !w.join("node_modules").exists());
        assert_eq!(uncommitted(&r), vec!["README.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Accepting commits the run's changes to `janus/<id>` and removes the worktree; working again
    /// brings it back on that branch; deleting keeps uncommitted changes as a patch.
    #[test]
    fn accept_commits_to_the_runs_branch_and_the_worktree_comes_back_on_it() {
        let dir = repo("accept");
        let (r, w) = (dir.join("repo"), dir.join("home/worktrees/r2"));
        let base = head(&r);
        create(&r, &w, &base, "r2").unwrap();
        std::fs::write(w.join("new.txt"), "from the run\n").unwrap();
        let done = accept(&r, &w, "r2", "Janus 任务 r2", true).unwrap();
        let (branch, commit) = done.commit.unwrap();
        assert_eq!(branch, "janus/r2");
        assert!(!w.exists(), "the worktree is gone once accepted");
        assert_eq!(read(&r, &["show", &format!("{commit}:new.txt")]).unwrap(), "from the run\n");
        assert_eq!(head(&r), base, "the person's checkout did not move");
        assert!(!r.join("new.txt").exists());

        assert!(ensure(&r, &w, &base, "r2").unwrap());
        assert_eq!(std::fs::read_to_string(w.join("new.txt")).unwrap(), "from the run\n", "back on its branch");
        assert!(!ensure(&r, &w, &base, "r2").unwrap(), "already there");
        // Accepting again while a conversation still works there keeps the worktree, detached, and
        // moves the branch forward.
        std::fs::write(w.join("second.txt"), "more work\n").unwrap();
        let (_, second) = accept(&r, &w, "r2", "Janus 任务 r2", false).unwrap().commit.unwrap();
        assert!(w.exists() && read(&w, &["symbolic-ref", "-q", "HEAD"]).is_none(), "kept and detached");
        assert_eq!(read(&r, &["rev-parse", "janus/r2"]).unwrap().trim(), second);
        // A branch the person moved meanwhile is not written over.
        assert!(
            Command::new("git").args(["branch", "-f", "janus/r2", &base]).current_dir(&r).status().unwrap().success()
        );
        std::fs::write(w.join("third.txt"), "x\n").unwrap();
        assert!(accept(&r, &w, "r2", "Janus 任务 r2", false).is_err());
        std::fs::write(w.join("more.txt"), "unfinished\n").unwrap();
        let kept = remove(&r, &w, &dir.join("home/worktrees/deleted"), "r2").unwrap().unwrap();
        assert!(std::fs::read_to_string(kept).unwrap().contains("unfinished"));
        assert!(!w.exists());
        assert!(read(&r, &["rev-parse", "--verify", "janus/r2"]).is_some(), "the accepted branch stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run that changed nothing leaves no commit and no branch.
    #[test]
    fn accepting_a_run_without_changes_makes_no_branch() {
        let dir = repo("nothing");
        let (r, w) = (dir.join("repo"), dir.join("home/worktrees/r3"));
        create(&r, &w, &head(&r), "r3").unwrap();
        assert!(accept(&r, &w, "r3", "Janus 任务 r3", true).unwrap().commit.is_none());
        assert!(read(&r, &["rev-parse", "--verify", "--quiet", "refs/heads/janus/r3"]).is_none());
        assert!(!w.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
