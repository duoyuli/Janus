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
    let seqs = |rows: &[crate::run_view::page::TraceRow]| rows.iter().map(|r| r.seq).collect::<Vec<_>>();
    assert_eq!(seqs(&t1.trace), vec![3, 4], "its own events");
    let head = t1.trace[1].attempt.as_ref().expect("an attempt's start is a header");
    assert_eq!((head.title.as_str(), head.session.as_str()), ("尝试 1", "s1"));
    assert_eq!((t1.trace[0].mark.icon, t1.trace[0].who.as_str()), ("next", "Janus"));
    assert_eq!(t1.downstream, vec!["T2".to_string()]);
    assert_eq!(t1.env[0].value, "Codex", "the member's Agent when the attempt named none");
    assert_eq!(t1.env[2].value, "s1");
    assert_eq!(seqs(&p.tasks["T2"].trace), vec![5], "an event that unlocks it names it");
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

#[test]
fn a_check_reads_as_the_apps_drew_it() {
    use crate::run_view::checks::{Mark, line};
    let none = line(Some(2), "有除零保护", None, Lang::Zh);
    assert_eq!(
        (none.mark, none.method.text.as_str(), none.voice.as_str()),
        (Mark::Unchecked, "未核对", "第 2 条，未核对，有除零保护")
    );
    let ran =
        line(None, "", Some(&json!({ "passed": true, "method": "test", "evidence": "cargo test 通过" })), Lang::En);
    assert_eq!(
        (ran.mark, ran.method.text.as_str(), ran.method.icon, ran.criterion.as_str()),
        (Mark::Passed, "Test", "checkmark.seal", "(No criterion specified)")
    );
    let odd = line(Some(1), "x", Some(&json!({ "passed": false, "method": "vibes", "evidence": "" })), Lang::Zh);
    assert_eq!(
        (odd.mark, odd.method.text.as_str(), odd.no_evidence.as_deref()),
        (Mark::Failed, "证据方法「vibes」不认识（没说明）", Some("没有给出证据"))
    );
    assert!(
        line(None, "x", Some(&json!({ "passed": true, "evidence": "a\nb" })), Lang::Zh).folds,
        "several lines fold"
    );
}
