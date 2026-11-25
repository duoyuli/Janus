//! Every `git` Janus runs goes through here: queries against a working directory it shares with
//! running agents (`read_only`, `output`, `read`, `query`), writes to a run's worktree and the
//! person's repository (`write`), and fetches into Janus home (`fetch`). All of them start from
//! `command`, which keeps the environment Janus was started in out of them.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// A `git` command for a read-only query in `cwd`. Agents in the shared working directory may be
/// running git (commit, merge) at the same moment, so a probe must not take `index.lock`:
/// `GIT_OPTIONAL_LOCKS=0` stops `git status` and friends from refreshing the index. Older git
/// ignores the variable. The variable covers `status`, but porcelain `git diff` refreshes the
/// index regardless: compare against HEAD with the plumbing `git diff-index`, which never does.
/// See docs/research/20261112-multi_agent_collaboration.md (P0).
pub fn read_only(cwd: &Path, args: &[&str]) -> Command {
    let mut cmd = command(cwd);
    cmd.args(args).env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

/// What a read-only query printed, whatever its exit status (`diff --no-index` exits 1 when the
/// files differ); empty when git could not run.
pub fn output(cwd: &Path, args: &[&str]) -> String {
    read_only(cwd, args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// A read-only query limited to `scope` (paths taken literally, not as patterns); all of the
/// working tree when `scope` is empty.
pub fn scoped_output(cwd: &Path, args: &[&str], scope: &[String]) -> String {
    let mut all: Vec<&str> = vec!["--literal-pathspecs"];
    all.extend(args);
    if !scope.is_empty() {
        all.push("--");
        all.extend(scope.iter().map(String::as_str));
    }
    output(cwd, &all)
}

/// A read-only query that has to succeed to mean anything; `None` when git fails.
pub fn read(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = read_only(cwd, args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A read-only query off the async runtime's threads; a failure carries git's own message.
pub async fn query(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::from(read_only(cwd, args)).output().await?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git` writing in `dir`, with the repository's hooks switched off: they are code in the
/// repository (an agent may have written them) and would run on checkouts, commits, rebases and
/// merges. The identity is the repository's or the person's own.
pub fn write(dir: &Path, args: &[&str]) -> Result<String> {
    let out = command(dir).args(["-c", "core.hooksPath=/dev/null"]).args(args).output().context("运行 git 失败")?;
    if !out.status.success() {
        bail!("git {}：{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Talking to a remote for a folder in Janus home (the library's Git sources): cloning, asking for
/// a ref. Large files are not downloaded, and it gives up after `limit`. A failure gives git's
/// last line of complaint.
pub async fn fetch(cwd: &Path, args: &[&str], limit: Duration) -> Result<String> {
    let mut cmd = tokio::process::Command::from(command(cwd));
    cmd.args(args).env("GIT_LFS_SKIP_SMUDGE", "1").kill_on_drop(true);
    let out = tokio::time::timeout(limit, cmd.output()).await.context("拉取仓库超时")??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        bail!("git {}：{line}", args.first().unwrap_or(&""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What the files of a working tree were at one moment, read without writing anything to the
/// repository: the commit checked out, and a hash of every file that differs from it or is not
/// tracked (modified, deleted, untracked; ignored files are left out). Two equal states mean the
/// same files, so a check made on one still holds on the other.
// @project-doc docs/domains/20261031-goal_mode.md#evidence_reuse
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FilesState {
    pub head: String,
    pub files: std::collections::BTreeMap<String, String>,
}

/// Past this many changed files the state is not taken: hashing them all on every result would
/// cost more than the reruns it saves.
const STATE_FILES: usize = 2000;

impl FilesState {
    /// The state of `cwd` now; `None` when it is not a Git working tree or changes too many files.
    pub fn of(cwd: &Path) -> Option<FilesState> {
        use sha2::{Digest, Sha256};
        let head = read(cwd, &["rev-parse", "HEAD"])?.trim().to_string();
        let listed = read(cwd, &["ls-files", "-z", "--modified", "--deleted", "--others", "--exclude-standard"])?;
        let paths: std::collections::BTreeSet<&str> = listed.split('\0').filter(|p| !p.is_empty()).collect();
        if paths.len() > STATE_FILES {
            return None;
        }
        let files = paths
            .into_iter()
            .map(|p| {
                let hash = match std::fs::read(cwd.join(p)) {
                    Ok(bytes) => Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(),
                    Err(_) => "deleted".to_string(),
                };
                (p.to_string(), hash)
            })
            .collect();
        Some(FilesState { head, files })
    }

    /// The files that are not the same as in `earlier`, by path. A different commit is named by
    /// the caller; the paths it changed are not listed here.
    pub fn changed_since(&self, earlier: &FilesState) -> Vec<String> {
        let mut paths: Vec<String> = self
            .files
            .iter()
            .filter(|(p, h)| earlier.files.get(*p) != Some(*h))
            .map(|(p, _)| p.clone())
            .chain(earlier.files.keys().filter(|p| !self.files.contains_key(*p)).cloned())
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }
}

/// What could point git at another repository than `cwd`, or add settings to it, when Janus was
/// itself started from git (a hook, an editor's terminal): never passed on.
const INHERITED: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
];

/// `git` on a directory the person or an agent owns (`cwd`), apart from the environment Janus was
/// started in: variables that point it elsewhere are dropped, it never waits for an answer on a
/// terminal, and the repository's file-system monitor, a program it would run, stays off.
// @project-doc docs/operations/20261031-security.md#workspace_access
pub fn command(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    for name in INHERITED {
        cmd.env_remove(name);
    }
    cmd.args(["-c", "core.fsmonitor=false"])
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Janus started from inside another repository's git (a hook) does not carry that over.
    #[test]
    fn git_does_not_inherit_where_janus_was_started() {
        let cmd = command(Path::new("/tmp"));
        let envs: Vec<_> = cmd.get_envs().collect();
        for name in INHERITED {
            assert!(envs.iter().any(|(k, v)| *k == name && v.is_none()), "{name} is passed on");
        }
        assert!(envs.iter().any(|(k, v)| *k == "GIT_TERMINAL_PROMPT" && *v == Some("0".as_ref())));
        assert!(cmd.get_args().any(|a| a == "core.fsmonitor=false"));
    }
    use std::path::PathBuf;

    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
        dir
    }

    /// Rewrite a tracked file with the same content, so its stat no longer matches the index
    /// and a plain `git status` would refresh (rewrite) the index.
    fn stale_stat(dir: &Path) {
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    }

    fn out(mut cmd: Command) -> String {
        String::from_utf8(cmd.output().unwrap().stdout).unwrap()
    }

    #[test]
    fn status_probe_leaves_the_index_alone_and_reports_the_same() {
        let dir = repo("status");
        std::fs::write(dir.join("new.txt"), "n\n").unwrap();
        stale_stat(&dir);
        let index = || std::fs::read(dir.join(".git/index")).unwrap();
        let before = index();

        let probed = out(read_only(&dir, &["status", "--porcelain"]));
        assert_eq!(index(), before, "a read-only probe rewrote the index");

        // The same query without the variable refreshes the index, which is what the probe avoids,
        // and reports the same thing.
        let mut plain = Command::new("git");
        plain.args(["status", "--porcelain"]).current_dir(&dir);
        assert_eq!(out(plain), probed);
        assert_ne!(index(), before, "plain git status did not refresh the index; the test proves nothing");
        assert_eq!(probed, "?? new.txt\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The diff Janus records for a task: `diff-index` against HEAD leaves the index alone and
    /// shows the same change `git diff HEAD` does, while `git diff` rewrites the index even
    /// through the probe.
    #[test]
    fn diff_probe_uses_diff_index_which_leaves_the_index_alone() {
        let dir = repo("diff");
        let index = || std::fs::read(dir.join(".git/index")).unwrap();
        // Only the stat changed: no diff, and the index stays as it was.
        stale_stat(&dir);
        let before = index();
        assert_eq!(out(read_only(&dir, &["diff-index", "-p", "HEAD"])), "");
        assert_eq!(out(read_only(&dir, &["diff-index", "--shortstat", "HEAD"])), "");
        assert_eq!(index(), before, "diff-index rewrote the index");
        // A real change shows up as `git diff HEAD` would show it.
        std::fs::write(dir.join("a.txt"), "b\n").unwrap();
        let probed = out(read_only(&dir, &["diff-index", "-p", "HEAD"]));
        assert!(probed.contains("-a\n+b"), "{probed}");
        assert_eq!(index(), before, "diff-index rewrote the index");
        assert_eq!(out(read_only(&dir, &["diff", "HEAD"])), probed);
        // Back to the committed content with a newer stat: porcelain `git diff`, even through the
        // probe, rewrites the index to refresh it. This is why the probe uses `diff-index`.
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        stale_stat(&dir);
        let before = index();
        assert_eq!(out(read_only(&dir, &["diff", "HEAD"])), "");
        assert_ne!(index(), before, "porcelain diff left the index alone; the note above is wrong");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
