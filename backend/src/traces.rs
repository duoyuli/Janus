//! What the work left in the workspace, read back instead of taken on the agents' word.
//!
//! Collaborators share one working directory, so what each did is visible there: the files its
//! tool calls edited, and the working tree against HEAD. Janus keeps those traces per task and
//! uses them where a report could drift from reality: a task's diff covers its own files, a
//! finished task whose files someone changed afterwards is marked as possibly out of date, and
//! a result is checked against the files the task owns and edited (`gaps`). The board carries
//! intent and decisions; facts the workspace already holds are read from it.
// @project-doc docs/domains/goal_mode.md#traces

use crate::orchestrator::TaskDef;
use crate::teamwork::covers;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Default)]
pub struct Traces {
    /// Files each task edited (relative to the workspace), from its tool calls.
    touched: HashMap<String, BTreeSet<String>>,
    /// Who edited each file last, and the seq of that tool call.
    last_edit: HashMap<String, (String, i64)>,
    /// Reports already made: (kind, task, other task, file).
    reported: HashSet<(&'static str, String, String, String)>,
}

impl Traces {
    /// A tool call of `task`, recorded at `seq`, edited `paths`.
    pub fn record(&mut self, task: &str, paths: &[String], seq: i64) {
        self.touched.entry(task.into()).or_default().extend(paths.iter().cloned());
        for p in paths {
            self.last_edit.insert(p.clone(), (task.into(), seq));
        }
    }

    pub fn files(&self, task: &str) -> BTreeSet<String> {
        self.touched.get(task).cloned().unwrap_or_default()
    }

    /// Whether this is the first report of `kind` for the pair and file.
    pub fn first_report(&mut self, kind: &'static str, task: &str, other: &str, path: &str) -> bool {
        self.reported.insert((kind, task.into(), other.into(), path.into()))
    }

    /// Files `task` edited that another task edited after `since` (the seq it finished at), with
    /// who did: its result describes them as they were.
    pub fn edited_after(&self, task: &str, since: i64) -> Vec<(String, String)> {
        self.touched
            .get(task)
            .into_iter()
            .flatten()
            .filter_map(|p| match self.last_edit.get(p) {
                Some((by, seq)) if by != task && *seq > since => Some((p.clone(), by.clone())),
                _ => None,
            })
            .collect()
    }
}

/// `edited_after` as a line for a prompt or the board; empty when nothing changed.
pub fn stale_note(stale: &[(String, String)]) -> String {
    if stale.is_empty() {
        return String::new();
    }
    let list: Vec<String> = stale.iter().map(|(p, by)| format!("{p}（{by} 改）")).collect();
    format!("它完成后 {} 又被改过：这些文件以工作目录里的现状为准，结论可能已过期", list.join("、"))
}

/// The paths a task's diff is limited to: what it edited and what it owns. Empty when there is
/// no record of either; the diff then covers the whole working tree.
pub fn scope(def: &TaskDef, touched: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<String> = def.owns.clone();
    out.extend(touched.iter().filter(|p| !def.owns.iter().any(|o| covers(o, p))).cloned());
    out
}

/// `git diff --shortstat` and the count of new files in words: "2 个文件 · +10 −3 · 1 个新文件".
pub fn diff_summary(shortstat: &str, new_files: usize) -> String {
    let num = |word: &str| -> usize {
        shortstat
            .split(',')
            .find(|part| part.contains(word))
            .and_then(|part| part.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    let (files, plus, minus) = (num("changed"), num("insertion"), num("deletion"));
    let mut out = vec![];
    if files > 0 {
        out.push(format!("{files} 个文件"));
        out.push(format!("+{plus} −{minus}"));
    }
    if new_files > 0 {
        out.push(format!("{new_files} 个新文件"));
    }
    out.join(" · ")
}

/// The files the working tree changed against HEAD: the output of
/// `git diff-index --name-only HEAD` and of `git ls-files --others --exclude-standard`.
pub fn changed_files(name_only: &str, untracked: &str) -> BTreeSet<String> {
    name_only.lines().chain(untracked.lines()).map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// Where a finished task's traces disagree with its role: a verifier that edited files, a task
/// that edited files outside what it owns, or owned files the working tree shows no change in.
/// `changed` is the working tree against HEAD; None outside a Git repository, where only the
/// tool calls are known.
pub fn gaps(def: &TaskDef, touched: &BTreeSet<String>, changed: Option<&BTreeSet<String>>) -> Vec<String> {
    let list = |v: &[&String]| v.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("、");
    let mut out = vec![];
    if def.verifies.is_some() {
        if !touched.is_empty() {
            out.push(format!("验证任务改了文件：{}。验证者只检查不改文件，这些改动没有经过独立验证", list(&touched.iter().collect::<Vec<_>>())));
        }
        return out;
    }
    if def.owns.is_empty() {
        return out;
    }
    let outside: Vec<&String> = touched.iter().filter(|p| !def.owns.iter().any(|o| covers(o, p))).collect();
    if !outside.is_empty() {
        out.push(format!("改了负责范围（{}）以外的文件：{}", def.owns.join("、"), list(&outside)));
    }
    let in_owns = |p: &String| def.owns.iter().any(|o| covers(o, p));
    let edited = touched.iter().any(in_owns);
    match changed {
        Some(changed) if !edited && !changed.iter().any(in_owns) => {
            out.push(format!("负责 {}，但工作目录里这些文件相对 HEAD 没有改动", def.owns.join("、")));
        }
        None if !edited => out.push(format!("负责 {}，但没有看到改动这些文件的工具调用（也可能是用命令改的）", def.owns.join("、"))),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(owns: &[&str], verifies: Option<&str>) -> TaskDef {
        TaskDef {
            id: "T1".into(),
            title: "T1".into(),
            desc: String::new(),
            deps: vec![],
            accept: vec![],
            assignee: "m1".into(),
            max_attempts: 2,
            owns: owns.iter().map(|x| x.to_string()).collect(),
            verifies: verifies.map(str::to_owned),
        }
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_result_is_checked_against_what_the_task_left() {
        // Did what it owns, nothing else: no gap.
        assert!(gaps(&def(&["src"], None), &set(&["src/a.rs"]), Some(&set(&["src/a.rs"]))).is_empty());
        // Changed its files by command (no tool call), the working tree shows it: no gap.
        assert!(gaps(&def(&["src"], None), &set(&[]), Some(&set(&["src/b.rs"]))).is_empty());
        // Owned files untouched in the working tree.
        let g = gaps(&def(&["src"], None), &set(&[]), Some(&set(&["docs/x.md"])));
        assert!(g.len() == 1 && g[0].contains("负责 src") && g[0].contains("没有改动"), "{g:?}");
        // Outside what it owns.
        let g = gaps(&def(&["src"], None), &set(&["src/a.rs", "README.md"]), Some(&set(&["src/a.rs", "README.md"])));
        assert!(g.len() == 1 && g[0].contains("以外的文件：README.md"), "{g:?}");
        // No Git: only the tool calls are known.
        assert!(gaps(&def(&["src"], None), &set(&[]), None)[0].contains("工具调用"));
        // A verifier changes nothing.
        let g = gaps(&def(&[], Some("T0")), &set(&["src/a.rs"]), Some(&set(&["src/a.rs"])));
        assert!(g.len() == 1 && g[0].contains("验证任务改了文件"), "{g:?}");
        assert!(gaps(&def(&[], Some("T0")), &set(&[]), Some(&set(&["src/a.rs"]))).is_empty(), "another task's change is not the verifier's");
        // Nothing declared: nothing to hold it to.
        assert!(gaps(&def(&[], None), &set(&["x"]), Some(&set(&[]))).is_empty());
    }

    #[test]
    fn later_edits_by_others_mark_a_result_out_of_date() {
        let mut t = Traces::default();
        t.record("T1", &["a.rs".into(), "b.rs".into()], 10);
        // T1 finished at 20; T2 changes a.rs afterwards, b.rs stays T1's.
        t.record("T2", &["a.rs".into()], 30);
        assert_eq!(t.edited_after("T1", 20), vec![("a.rs".to_string(), "T2".to_string())]);
        assert!(t.edited_after("T1", 40).is_empty(), "edits before the result do not count");
        assert!(t.edited_after("T2", 20).is_empty(), "its own edits");
        assert!(stale_note(&t.edited_after("T1", 20)).contains("a.rs（T2 改）"));
        assert!(stale_note(&[]).is_empty());
        assert!(t.first_report("same", "T1", "T2", "a.rs") && !t.first_report("same", "T1", "T2", "a.rs"));
    }

    #[test]
    fn a_diff_covers_the_tasks_own_files() {
        assert_eq!(scope(&def(&["src"], None), &set(&["src/a.rs", "README.md"])), vec!["src", "README.md"]);
        assert!(scope(&def(&[], None), &set(&[])).is_empty(), "no record: the whole tree");
        assert_eq!(changed_files(" a.rs\n\nb.rs\n", "new.txt\n"), set(&["a.rs", "b.rs", "new.txt"]));
        assert_eq!(diff_summary(" 2 files changed, 10 insertions(+), 3 deletions(-)\n", 1), "2 个文件 · +10 −3 · 1 个新文件");
        assert_eq!(diff_summary(" 1 file changed, 1 deletion(-)\n", 0), "1 个文件 · +0 −1");
        assert_eq!(diff_summary("", 2), "2 个新文件");
    }
}
