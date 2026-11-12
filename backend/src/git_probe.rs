//! Read-only Git queries against a working directory Janus shares with running agents.

use std::path::Path;
use std::process::Command;

/// A `git` command for a read-only query in `cwd`. Agents in the shared working directory may be
/// running git (commit, merge) at the same moment, so a probe must not take `index.lock`:
/// `GIT_OPTIONAL_LOCKS=0` stops `git status` and friends from refreshing the index. Older git
/// ignores the variable. The variable covers `status`, but porcelain `git diff` refreshes the
/// index regardless: compare against HEAD with the plumbing `git diff-index`, which never does.
/// See docs/research/multi_agent_collaboration_2026_11_12.md (P0).
pub fn read_only(cwd: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd).env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-git-probe-{name}-{}", std::process::id()));
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
