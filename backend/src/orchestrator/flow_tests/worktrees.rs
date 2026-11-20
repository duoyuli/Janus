//! Runs on a git repository work in their own worktree (docs/decisions/20261119-run_worktrees.md):
//! accepting commits the result and brings it back to the branch the run started on, a restart
//! removes accepted worktrees and brings one back when work resumes, deleting keeps uncommitted work as a patch, and the person is reminded of
//! uncommitted changes and of runs still working in a directory outside git.

use super::*;

fn cwd_of(h: &Harness, run: &str) -> PathBuf {
    PathBuf::from(h.app.db.run(run).unwrap().unwrap()["cwd"].as_str().unwrap())
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Two runs on one repository each work in their own worktree, and `.worktreeinclude` brings an
/// ignored file along. Accepting brings the result back to the person's branch and checkout
/// while the lead's conversation keeps working in the worktree; after a restart the accepted
/// worktree is gone, and a new message brings it back at the accepted commit.
#[tokio::test(flavor = "multi_thread")]
async fn runs_work_apart_and_accepting_brings_the_result_back_to_the_branch() {
    let h = harness("worktree-accept", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    std::fs::write(h.repo.join(".gitignore"), ".env\n").unwrap();
    std::fs::write(h.repo.join(".worktreeinclude"), ".env\n").unwrap();
    sh(&h.repo, &["add", "-A"]);
    sh(&h.repo, &["commit", "-q", "-m", "ignore"]);
    std::fs::write(h.repo.join(".env"), "KEY=1\n").unwrap();

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let other = start(&h, json!([task("T9", "m1", json!({}))])).await;
    let (cwd, other_cwd) = (cwd_of(&h, &run), cwd_of(&h, &other));
    assert_ne!(cwd, other_cwd, "each run has its own worktree");
    assert_eq!(std::fs::read_to_string(cwd.join(".env")).unwrap(), "KEY=1\n", ".worktreeinclude brought it");
    let created = events(&h.app, &run).into_iter().find(|e| e.kind == "run.created").unwrap().data.unwrap();
    assert_eq!(created["worktree"]["included"], json!([".env"]));
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    assert!(cwd.join("notes.md").exists() && !other_cwd.join("notes.md").exists() && !h.repo.join("notes.md").exists());

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    act(&h, &run, json!({ "type": "accept" }));
    let accepted = events(&h.app, &run).into_iter().find(|e| e.kind == "risk.accepted").unwrap();
    let d = accepted.data.unwrap();
    let branch = git_branch(&h.repo);
    assert_eq!(d["branch"], json!(branch), "{d}");
    assert_eq!(d["commit"], json!(git_out(&h.repo, &["rev-parse", "HEAD"]).trim()), "{d}");
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n", "back in the checkout");
    assert!(git_out(&h.repo, &["branch", "--list", "janus/*"]).trim().is_empty(), "no branch of its own");
    assert!(cwd.exists(), "the lead's open conversation still works there");
    assert!(git_out(&cwd, &["symbolic-ref", "-q", "HEAD"]).is_empty(), "detached, so the branch stays free");

    // A restart removes it; working again brings it back on the branch.
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    assert!(!cwd.exists(), "nothing works there after a restart");
    app2.request_action(&run, &json!({ "type": "message", "agent": "lead", "text": "再补一份说明" }))
        .await
        .ok()
        .unwrap();
    wait_until(&app2, &run, "follow-up prompt", |evs| {
        evs.iter().any(|e| e.kind == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "followup"))
    })
    .await;
    assert_eq!(std::fs::read_to_string(cwd.join("notes.md")).unwrap(), "T1\n", "back at the accepted commit");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Wait until the run's status is `delivered`: the delivery is written before the status moves on.
async fn delivered(h: &Harness, run: &str) {
    for _ in 0..100 {
        if status(&h.app, run) == "delivered" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Accepting never overwrites the person's uncommitted work: it fails, the run stays delivered
/// and their file stays; once it is out of the way, accepting brings the result back.
#[tokio::test(flavor = "multi_thread")]
async fn accepting_over_the_persons_uncommitted_work_fails_and_keeps_the_run_delivered() {
    let h = harness("worktree-dirty", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    std::fs::write(h.repo.join("notes.md"), "mine\n").unwrap();
    let live = h.app.run(&run).unwrap();
    let err = format!("{:#}", h.app.action(&live, &json!({ "type": "accept" })).unwrap_err());
    assert!(err.contains("notes.md") && err.contains("先提交或暂存"), "{err}");
    assert_eq!(status(&h.app, &run), "delivered");
    assert!(!has(&events(&h.app, &run), "risk.accepted"));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "mine\n", "theirs stays");

    std::fs::remove_file(h.repo.join("notes.md")).unwrap();
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n");
    assert_eq!(status(&h.app, &run), "accepted");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Deleting a run removes its worktree and keeps what was never committed as a patch.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_run_keeps_its_uncommitted_work_as_a_patch() {
    let h = harness("worktree-delete", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    // Only a finished run can go.
    delivered(&h, &run).await;
    let cwd = cwd_of(&h, &run);
    h.app.delete_run(&run).await.unwrap();
    assert!(!cwd.exists());
    let patch = h.dir.join("data/worktrees/deleted").join(format!("{run}.patch"));
    assert!(std::fs::read_to_string(patch).unwrap().contains("notes.md"));
    assert!(git_out(&h.repo, &["worktree", "list"]).lines().count() == 1, "git forgot the worktree");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Before a run starts the person is reminded of what a git repository leaves out (changes not
/// committed) and, outside git, of a run still working in the same directory.
#[tokio::test(flavor = "multi_thread")]
async fn the_person_is_reminded_before_a_run_starts() {
    let h = harness("worktree-preflight", &[("lead", false), ("m1", true)], json!({ "T1": ["slow"] }));
    assert!(h.app.preflight("ws").await.unwrap().is_empty(), "a clean checkout needs no reminder");
    std::fs::write(h.repo.join("draft.md"), "half done\n").unwrap();
    let warnings = h.app.preflight("ws").await.unwrap();
    assert!(
        warnings.len() == 1 && warnings[0].contains("draft.md") && warnings[0].contains("不会带进"),
        "{warnings:?}"
    );

    std::fs::remove_dir_all(h.repo.join(".git")).unwrap();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    assert_eq!(cwd_of(&h, &run), h.repo.canonicalize().unwrap(), "outside git a run works in place");
    let warnings = h.app.preflight("ws").await.unwrap();
    assert!(warnings.len() == 1 && warnings[0].contains("正在运行的任务"), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&h.dir);
}
