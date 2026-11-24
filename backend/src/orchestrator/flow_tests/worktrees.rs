//! Runs on a git repository work in their own worktree (docs/decisions/20261119-run_worktrees.md):
//! delivering commits the result and brings it back to the branch the run started on, and
//! accepting brings what a delivery could not; a conflict goes to the lead; work after a delivery,
//! committed or not, is delivered (and merged) again; a restart removes accepted worktrees
//! (delivering one with work after its acceptance) and brings one back when work resumes, deleting
//! keeps uncommitted work as a patch, and the person is reminded of uncommitted changes and of
//! runs still working in a directory outside git.

use super::*;

fn cwd_of(h: &Harness, run: &str) -> PathBuf {
    PathBuf::from(h.app.db.run(run).unwrap().unwrap()["cwd"].as_str().unwrap())
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Two runs on one repository each work in their own worktree, and `.worktreeinclude` brings an
/// ignored file along. Delivering brings the result back to the person's branch and checkout
/// while the lead's conversation keeps working in the worktree, and accepting finds it there;
/// after a restart the accepted worktree is gone, and a new message brings it back at that commit.
#[tokio::test(flavor = "multi_thread")]
async fn runs_work_apart_and_delivering_brings_the_result_back_to_the_branch() {
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
    let created = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "run.created").unwrap().data.unwrap();
    assert_eq!(created["worktree"]["included"], json!([".env"]));
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    assert!(cwd.join("notes.md").exists() && !other_cwd.join("notes.md").exists() && !h.repo.join("notes.md").exists());

    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    let evs = events(&h.app, &run);
    let merged = evs.iter().find(|e| e.kind.as_str() == "run.merged").unwrap();
    assert_eq!(merged.actor, "janus");
    let d = merged.data.clone().unwrap();
    let branch = git_branch(&h.repo);
    let tip = git_out(&h.repo, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(d["branch"], json!(branch), "{d}");
    assert_eq!(d["commit"], json!(tip), "{d}");
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n", "back in the checkout");
    let delivery = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.clone().unwrap();
    assert_eq!(delivery["merge"]["state"], "merged", "{delivery}");
    assert!(delivery["merge"]["note"].as_str().unwrap().contains("结果已合并到"), "{delivery}");
    assert_eq!(delivery["changed"], json!(["notes.md"]), "this round's files, listed before the merge took them");
    assert!(git_out(&h.repo, &["branch", "--list", "janus/*"]).trim().is_empty(), "no branch of its own");
    act(&h, &run, json!({ "type": "accept" }));
    let accepted = events(&h.app, &run).into_iter().find(|e| e.kind.as_str() == "risk.accepted").unwrap();
    assert!(accepted.text.contains("结果已在"), "{}", accepted.text);
    assert_eq!(accepted.data.unwrap()["commit"], Value::Null, "nothing more to bring");
    assert_eq!(git_out(&h.repo, &["rev-parse", "HEAD"]).trim(), tip);
    assert!(cwd.exists(), "the lead's open conversation still works there");
    assert!(git_out(&cwd, &["symbolic-ref", "-q", "HEAD"]).is_empty(), "detached, so the branch stays free");

    // A restart removes it; working again brings it back on the branch.
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    assert!(!cwd.exists(), "nothing works there after a restart");
    app2.request_action(&run, &req(json!({ "type": "message", "agent": "lead", "text": "再补一份说明" })))
        .await
        .ok()
        .unwrap();
    wait_until(&app2, &run, "follow-up prompt", |evs| {
        evs.iter()
            .any(|e| e.kind.as_str() == "session.prompt" && e.data.as_ref().is_some_and(|d| d["channel"] == "followup"))
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

/// Bringing the result back never overwrites the person's uncommitted work: the delivery says what
/// is in the way and their file stays, and so does accepting until it is out of the way; then
/// accepting brings the result back.
#[tokio::test(flavor = "multi_thread")]
async fn the_persons_uncommitted_work_in_the_way_waits_for_them() {
    let h = harness("worktree-dirty", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    std::fs::write(h.repo.join("notes.md"), "mine\n").unwrap();
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    let evs = events(&h.app, &run);
    let merge = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.clone().unwrap()["merge"].clone();
    assert_eq!(merge["state"], "blocked", "{merge}");
    assert!(merge["note"].as_str().unwrap().contains("先移走或改名"), "{merge}");
    assert!(!has(&evs, "run.merged"));
    assert!(!has(&evs, "user.message"), "nothing for the lead to resolve");
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "mine\n", "theirs stays");
    let live = h.app.run(&run).unwrap();
    let err = format!("{:#}", h.app.action(&live, &action_of(json!({ "type": "accept" }))).unwrap_err());
    assert!(err.contains("notes.md") && err.contains("先移走或改名"), "{err}");
    assert_eq!(status(&h.app, &run), "delivered");
    assert!(!has(&events(&h.app, &run), "risk.accepted"));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "mine\n", "theirs stays");

    std::fs::remove_file(h.repo.join("notes.md")).unwrap();
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "T1\n");
    assert_eq!(status(&h.app, &run), "accepted");
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The whole run's diff (`run_diff`) comes with the delivery and keeps up with the lead's own
/// edits, which no task's diff has; an edit after the delivery is delivered and merged again.
#[tokio::test(flavor = "multi_thread")]
async fn the_runs_whole_diff_and_work_after_a_delivery_are_delivered() {
    let h = harness("worktree-diff", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    let evs = events(&h.app, &run);
    let d = evs.iter().find(|e| e.kind.as_str() == "run.delivered").unwrap().data.clone().unwrap();
    let first = &d["deliverables"][0];
    assert_eq!(first["kind"], "run_diff", "{d}");
    let diff = std::fs::read_to_string(first["path"].as_str().unwrap()).unwrap();
    assert!(diff.contains("notes.md"), "{diff}");
    let created = evs
        .iter()
        .find(|e| e.kind.as_str() == "artifact.created" && e.data.as_ref().unwrap()["kind"] == "run_diff")
        .unwrap();
    assert!(created.task.is_none(), "the whole run's, not a task's");

    // The lead edits a file itself after delivery: the run's diff has it, and it is delivered
    // and merged again.
    h.app
        .request_action(&run, &req(json!({ "type": "message", "agent": "lead", "text": "[write lead.md]" })))
        .await
        .ok()
        .unwrap();
    wait_until(&h.app, &run, "delivered again", |evs| {
        evs.iter().filter(|e| e.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    let diff = events(&h.app, &run)
        .into_iter()
        .rfind(|e| e.kind.as_str() == "artifact.created" && e.data.as_ref().unwrap()["kind"] == "run_diff")
        .unwrap();
    let diff = std::fs::read_to_string(diff.data.unwrap()["path"].as_str().unwrap()).unwrap();
    assert!(diff.contains("lead.md") && !diff.contains("notes.md"), "only what was not merged yet: {diff}");
    assert!(h.repo.join("lead.md").exists(), "merged with the second delivery");
    assert_eq!(events(&h.app, &run).iter().filter(|e| e.kind.as_str() == "run.merged").count(), 2);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A delivery that runs into the branch's new commits changes nothing on the person's side: the
/// conflict goes to the lead with what it needs to resolve it, and the turn that deals with it
/// delivers the run again. A lead that did not resolve it is not handed it again by that delivery;
/// the person's accept hands it over once more.
#[tokio::test(flavor = "multi_thread")]
async fn a_conflict_at_delivery_goes_to_the_lead_once() {
    let h = harness("worktree-conflict", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    // Meanwhile the person committed their own notes.md: replaying the run's commit conflicts.
    std::fs::write(h.repo.join("notes.md"), "mine\n").unwrap();
    sh(&h.repo, &["add", "notes.md"]);
    sh(&h.repo, &["commit", "-q", "-m", "mine"]);
    let tip = git_out(&h.repo, &["rev-parse", "HEAD"]).trim().to_string();
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    let merge = |n: usize| {
        let d = events(&h.app, &run).into_iter().filter(|e| e.kind.as_str() == "run.delivered").nth(n).unwrap();
        let m = d.data.unwrap()["merge"].clone();
        (m["state"].as_str().unwrap().to_string(), m["note"].as_str().unwrap().to_string())
    };
    assert_eq!(merge(0).0, "resolving");
    assert!(merge(0).1.contains("负责人正在任务的副本里解决"), "{}", merge(0).1);
    assert!(!has(&events(&h.app, &run), "run.merged"));
    assert_eq!(std::fs::read_to_string(h.repo.join("notes.md")).unwrap(), "mine\n", "the person's side is as it was");
    assert_eq!(git_out(&h.repo, &["rev-parse", "HEAD"]).trim(), tip);
    let told = |evs: &[Event]| {
        evs.iter()
            .filter(|e| e.kind.as_str() == "user.message" && e.actor == "janus")
            .filter_map(|e| e.data.as_ref().unwrap()["text"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    let first = told(&events(&h.app, &run));
    assert!(
        first.len() == 1 && first[0].contains(&format!("git rebase {tip}\n")) && first[0].contains("notes.md"),
        "{first:?}"
    );

    // The lead's turn did not resolve it: delivered again, left to the person this time.
    wait_until(&h.app, &run, "delivered again", |evs| {
        evs.iter().filter(|e| e.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    lead_idle(&h, &run).await;
    assert_eq!(merge(1).0, "conflict");
    assert!(merge(1).1.contains("仍然冲突"), "{}", merge(1).1);
    assert_eq!(told(&events(&h.app, &run)).len(), 1, "not handed over again by its own delivery");
    assert!(
        h.app.request_action(&run, &req(json!({ "type": "accept" }))).await.is_ok(),
        "handed to the lead, not refused"
    );
    assert!(!has(&events(&h.app, &run), "risk.accepted"));
    assert_eq!(told(&events(&h.app, &run)).len(), 2, "the person's accept hands it over");
    wait_until(&h.app, &run, "delivered a third time", |evs| {
        evs.iter().filter(|e| e.kind.as_str() == "run.delivered").count() == 3
    })
    .await;
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// Deleting a run removes its worktree and keeps what was never committed (a file written after
/// its delivery merged) as a patch.
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
    std::fs::write(cwd.join("draft.md"), "half done\n").unwrap();
    h.app.delete_run(&run).await.unwrap();
    assert!(!cwd.exists());
    let patch = h.app.cfg.layout.deleted_runs().join(format!("{run}.patch"));
    assert!(std::fs::read_to_string(patch).unwrap().contains("draft.md"));
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

/// A run that fails to be created after its worktree was made (an image that is not one) leaves
/// nothing behind: no worktree in the repository, no directory under Janus home.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_fails_to_start_leaves_no_worktree() {
    let h = harness("wt-failed-create", &[("lead", false), ("m1", false)], json!({}));
    let count = || {
        let out = std::process::Command::new("git").args(["worktree", "list"]).current_dir(&h.repo).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count()
    };
    let before = count();
    for _ in 0..2 {
        let bad = NewRun {
            goal: "带一张坏图".into(),
            workspace: "ws".into(),
            images: vec![json!({ "data": "not base64!", "name": "x.png" })],
            ..Default::default()
        };
        assert!(h.app.create_run(bad).await.is_err());
    }
    assert_eq!(count(), before, "git has no worktree left from the failed runs");
    let left: Vec<_> = std::fs::read_dir(h.app.cfg.layout.home().join("work/runs"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "no worktree directory left: {left:?}");
    assert!(h.app.db.runs(RunKind::Goal).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A workspace that is a folder of a repository (`apps/client`): the worktree is of the whole
/// repository, sessions start in that folder of it, so the folder's own files are where its paths
/// say; delivering brings the result back into the folder, and deleting removes the whole worktree
/// (FL-09).
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_folder_inside_a_repository_is_where_sessions_start() {
    let h = harness("wt-subdir", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let sub = h.repo.join("apps/client");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("local.md"), "client\n").unwrap();
    sh(&h.repo, &["add", "-A"]);
    sh(&h.repo, &["commit", "-q", "-m", "client"]);
    let mut ws = h.app.workspace("ws").unwrap();
    ws["path"] = json!(sub.canonicalize().unwrap());
    h.app.db.put_config(ConfigKind::Workspace, "ws", &ws, now_ms()).unwrap();

    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    let cwd = cwd_of(&h, &run);
    assert!(cwd.ends_with("apps/client"), "{}", cwd.display());
    assert!(cwd.join("local.md").exists(), "the folder's own file is where its path says");
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    assert!(cwd.join("notes.md").exists(), "the task writes in the folder");
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    assert!(sub.join("notes.md").exists(), "delivering brings it back into the folder");

    h.app.delete_run(&run).await.unwrap();
    assert!(!h.app.cfg.layout.run(RunKind::Goal, &run).work.exists(), "the whole worktree goes");
    assert_eq!(git_out(&h.repo, &["worktree", "list"]).lines().count(), 1);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// A run on a git repository works in its own worktree, even when every member's mode says
/// read-only (the lead's call, not Janus's): its change lands there, not in the person's
/// checkout until the delivery, and is kept as a diff.
#[tokio::test(flavor = "multi_thread")]
async fn tasks_work_in_the_runs_own_worktree() {
    let h = harness("inplace", &[("lead", false), ("m1", false)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| has(e, "task.succeeded")).await;
    let cwd = PathBuf::from(h.app.db.run(&run).unwrap().unwrap()["cwd"].as_str().unwrap());
    assert_ne!(cwd, h.repo.canonicalize().unwrap());
    assert_eq!(std::fs::read_to_string(cwd.join("notes.md")).unwrap(), "T1\n", "written in the worktree");
    assert!(!h.repo.join("notes.md").exists(), "the person's checkout is untouched while it works");
    let head = git_out(&h.repo, &["rev-parse", "HEAD"]).trim().to_string();
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    let evs = events(&h.app, &run);
    let created = evs.iter().find(|e| e.kind.as_str() == "run.created").unwrap().data.clone().unwrap();
    assert_eq!(created["worktree"]["repo"], json!(h.repo.canonicalize().unwrap()), "{created}");
    let branch = git_branch(&h.repo);
    assert_eq!(created["worktree"]["branch"], json!(branch), "the branch the run started on");
    // The lead commits, pushes or deploys when the user asks, and is told that a delivery brings
    // the result to the branch and how a push goes from a detached worktree; nothing in its brief
    // forbids what the user asked for.
    let brief = evs
        .iter()
        .filter(|e| e.kind.as_str() == "session.prompt")
        .filter_map(|e| e.data.as_ref().unwrap()["prompt"].as_str())
        .find(|p| p.contains("You are the lead"))
        .unwrap();
    assert!(brief.contains("Commit, push, deploy, or release only when the user asks for it"), "{brief}");
    assert!(brief.contains(&format!("git push origin HEAD:{branch}")), "{brief}");
    assert!(brief.contains(&format!("git rebase {branch}")), "{brief}");
    assert!(!brief.contains("Do not commit"), "{brief}");
    let started = evs.iter().find(|e| e.kind.as_str() == "attempt.started").unwrap();
    assert_eq!(started.data.as_ref().unwrap()["path"], json!(cwd));
    let git = &started.data.as_ref().unwrap()["git"];
    assert_eq!(
        *git,
        json!({ "repo": h.repo.canonicalize().unwrap(), "branch": branch, "worktree": cwd, "head": head }),
        "each attempt records the Git facts it works with"
    );
    assert!(started.data.as_ref().unwrap()["prompt"].as_str().unwrap().contains("单独建的 Git worktree"));
    assert!(
        evs.iter().any(|e| e.kind.as_str() == "artifact.created" && e.text.contains("T1-a1.diff")),
        "the change is kept as a diff"
    );
    assert!(!has(&evs, "verification.failed"));
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The run's worktree, after waiting for the lead's turn to end.
async fn lead_idle(h: &Harness, run: &str) {
    for _ in 0..100 {
        if h.app.run(run).is_some_and(|r| !r.st.lock().unwrap().lead.in_turn) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// After the person accepted, the lead commits more work when asked to: a commit leaves nothing
/// uncommitted, and still the run is delivered again, and that delivery brings the commit back.
/// Work a restart cut off before its turn ended (after an acceptance) is not removed with the
/// worktree: the run is delivered again, which brings the work back.
#[tokio::test(flavor = "multi_thread")]
async fn work_after_acceptance_committed_or_cut_off_is_delivered_again() {
    let h = harness("worktree-after", &[("lead", false), ("m1", true)], json!({ "T1": ["write:notes.md"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1", |e| task_event(e, "task.succeeded", "T1")).await;
    finish_lead(&h);
    wait_until(&h.app, &run, "delivery", |e| has(e, "run.delivered")).await;
    delivered(&h, &run).await;
    act(&h, &run, json!({ "type": "accept" }));
    let cwd = cwd_of(&h, &run);

    h.app
        .request_action(
            &run,
            &req(json!({ "type": "message", "agent": "lead", "text": "再写一份 [write more.md] 并提交 [commit]" })),
        )
        .await
        .ok()
        .unwrap();
    wait_until(&h.app, &run, "delivered again", |evs| {
        evs.iter().filter(|e| e.kind.as_str() == "run.delivered").count() == 2
    })
    .await;
    delivered(&h, &run).await;
    assert!(git_out(&cwd, &["status", "--porcelain"]).trim().is_empty(), "the lead committed everything");
    assert_eq!(std::fs::read_to_string(h.repo.join("more.md")).unwrap(), "follow-up\n", "on the branch");
    assert_eq!(git_out(&h.repo, &["log", "-1", "--format=%s"]).trim(), "follow-up", "the lead's own commit");
    lead_idle(&h, &run).await;
    act(&h, &run, json!({ "type": "accept" }));
    assert_eq!(status(&h.app, &run), "accepted");

    // A turn after acceptance changed a file when the service stopped.
    std::fs::write(cwd.join("cut.md"), "cut off\n").unwrap();
    let app2 = test_app(&h.dir);
    app2.recover().unwrap();
    assert_eq!(status(&app2, &run), "delivered");
    let again = events(&app2, &run).into_iter().rfind(|e| e.kind.as_str() == "run.delivered").unwrap();
    assert!(again.data.unwrap()["summary"].as_str().unwrap().contains("服务重启"), "{}", again.text);
    assert_eq!(std::fs::read_to_string(h.repo.join("cut.md")).unwrap(), "cut off\n", "brought back");
    app2.request_action(&run, &req(json!({ "type": "accept" }))).await.ok().unwrap();
    assert_eq!(status(&app2, &run), "accepted");
    assert!(!cwd.exists(), "nothing works there now");
    let _ = std::fs::remove_dir_all(&h.dir);
}
