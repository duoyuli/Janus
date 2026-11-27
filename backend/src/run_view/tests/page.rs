//! The run page's own lines and lists: the delivery folded to one line and as text to copy, what
//! the round changed, the inspector's lists, each task's trace and setting, the team's notes.

use super::review::{ask, delivered};
use super::*;

fn page_of(log: &Log, lang: Lang) -> crate::run_view::page::Page {
    let f = fold(&log.events);
    let words = Words::of(
        lang,
        &json!({ "team": [
            { "id": "researcher", "name": "研究员", "connection_name": "Codex" },
        ]}),
    );
    let mut view = f.view(true, false, &words, false);
    view.title = "给 divide 增加除零保护".into();
    f.page(&view, &words)
}

fn artifact(log: &mut Log, task: &str, kind: &str, name: &str, path: &str) {
    log.add(ev("artifact.created").task(task).attempt(1).data(json!({
        "name": name, "kind": kind, "path": path, "desc": "", "hash": "abc"
    })));
}

#[test]
fn a_task_page_has_its_trace_its_downstream_and_its_setting() {
    let mut log = goal_run();
    log.add(ev("task.ready").data(json!({ "unlocked": ["T2"] })));
    let p = page_of(&log, Lang::Zh);
    let t1 = &p.tasks["T1"];
    assert_eq!(t1.trace, vec![3, 4], "its own events");
    assert_eq!(t1.downstream, vec!["T2".to_string()]);
    assert_eq!(t1.env[0].value, "Codex", "the member's Agent when the attempt named none");
    assert_eq!(t1.env[2].value, "s1");
    assert_eq!(p.tasks["T2"].trace, vec![5], "an event that unlocks it names it");
    assert!(p.tasks["T2"].downstream.is_empty());
    assert_eq!(p.progress.as_deref(), Some("子任务 0 / 2"));
}

#[test]
fn the_inspector_keeps_each_tasks_newest_and_the_whole_runs_diff_first() {
    let mut log = goal_run();
    artifact(&mut log, "T1", "git_diff", "diff", "/a/T1-1.diff");
    artifact(&mut log, "T1", "git_diff", "diff", "/a/T1-2.diff");
    artifact(&mut log, "T1", "file", "notes.md", "/a/notes.md");
    log.add(
        ev("artifact.created")
            .data(json!({ "name": "run.diff", "kind": "run_diff", "path": "/a/run.diff", "hash": "x" })),
    );
    let p = page_of(&log, Lang::Zh);
    let diffs: Vec<&str> = p.inspector.diffs.iter().map(|a| a.path.as_str()).collect();
    assert_eq!(diffs, vec!["/a/run.diff", "/a/T1-2.diff"]);
    assert_eq!(p.inspector.files.len(), 1);
}

#[test]
fn a_delivery_folds_to_a_line_once_the_person_wrote_and_copies_as_text() {
    let mut log = goal_run();
    delivered(&mut log, json!({ "headline": "做完了", "summary": "改了 a.rs", "merge": { "note": "已合并到 main" } }));
    let p = page_of(&log, Lang::Zh);
    assert_eq!(p.review_line, None, "a delivery waiting for review is shown whole");
    let copy = p.summary.unwrap();
    assert!(copy.starts_with("已完成：给 divide 增加除零保护\n做完了\n"), "{copy}");
    assert!(copy.contains("\n已合并到 main\n"), "{copy}");
    assert!(copy.ends_with("\n改了 a.rs"), "{copy}");
    ask(&mut log, "再加个测试");
    let p = page_of(&log, Lang::En);
    assert_eq!(p.review_line.as_deref(), Some("The lead is working on your message. Accept once it is done."));
    delivered(&mut log, json!({ "headline": "加好了" }));
    let p = page_of(&log, Lang::Zh);
    assert!(p.summary.unwrap().contains("\n第 2 次交付\n"));
    assert_eq!(p.earlier.len(), 1);
    assert_eq!(p.earlier[0].line, "做完了");
}

#[test]
fn the_team_page_says_where_the_run_works() {
    let p = page_of(&goal_run(), Lang::Zh);
    assert!(p.team.workspace.starts_with("Git 仓库，基线"), "{}", p.team.workspace);
    assert_eq!(p.team.branch, None, "a run working in place has no branch of its own");
}
