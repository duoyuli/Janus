//! How the lead structures its team when it orchestrates: which files each task owns, and which
//! task an independent verifier checks.
//!
//! Collaborators all work in the same directory. Ownership is declared up front (`owns`), so two
//! tasks that may run at the same time cannot be given the same files; the conflict detection in
//! `record_edits` stays as the backstop for what was not declared. A verification task
//! (`verifies`) is done by another member, starts after the task it checks, changes no files,
//! and its failure cannot be waved through with `accept`: the lead orchestrates a fix and a new
//! verification, or cancels the verification on the record. None of this adds task states or
//! event types; it is the orchestration's own data, checked when it is submitted.
// @project-doc docs/domains/goal_mode.md#team_structure

use crate::orchestrator::TaskDef;
use crate::status::TaskStatus;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The `owns` of one submitted task: paths relative to the workspace, a directory covering
/// everything under it. Leading `./` and trailing `/` are dropped; `..` and absolute paths are
/// reported, since a task may only change files inside the workspace.
pub fn read_owns(v: &Value, label: &str, errors: &mut Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in v.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
        let p = raw.trim().trim_start_matches("./").trim_end_matches('/').to_string();
        if p.is_empty() {
            continue;
        }
        if p.starts_with('/') || p.split('/').any(|s| s == "..") {
            errors.push(format!("{label}：owns 里的 `{raw}` 不在工作目录内，写相对工作目录的路径"));
            continue;
        }
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Two owned paths cover a common file: the same path, or one is a directory holding the other.
fn overlaps(a: &str, b: &str) -> bool {
    let under = |x: &str, dir: &str| x.len() > dir.len() && x.starts_with(dir) && x.as_bytes()[dir.len()] == b'/';
    a == b || under(a, b) || under(b, a)
}

/// Check the team structure of the new tasks against each other and against the open tasks
/// already on the board, after the basic checks of `validate_orchestration`. A verification task
/// gets the task it checks added to its dependencies.
pub fn check(new: &mut [TaskDef], board: &[(TaskDef, TaskStatus)], errors: &mut Vec<String>) {
    let mut add_deps: Vec<(usize, String)> = vec![];
    {
        let all: HashMap<&str, &TaskDef> = board.iter().map(|(d, _)| (d.id.as_str(), d)).chain(new.iter().map(|d| (d.id.as_str(), d))).collect();
        for (i, d) in new.iter().enumerate() {
            let Some(target) = d.verifies.as_deref() else { continue };
            let label = format!("任务 {}", d.id);
            match all.get(target) {
                None => errors.push(format!("{label}：verifies 指向的任务 {target} 不存在")),
                Some(_) if target == d.id => errors.push(format!("{label}：不能验证自己")),
                Some(t) => {
                    if t.assignee == d.assignee {
                        errors.push(format!("{label}：验证 {target} 的必须是另一个协作者，不能是它的执行者 {}", d.assignee));
                    }
                    if !d.deps.iter().any(|x| x == target) {
                        add_deps.push((i, target.to_string()));
                    }
                }
            }
            if !d.owns.is_empty() {
                errors.push(format!("{label}：验证任务只检查、不改文件，不要写 owns"));
            }
        }
    }
    for (i, dep) in add_deps {
        new[i].deps.push(dep);
    }

    let all: HashMap<&str, &TaskDef> = board.iter().map(|(d, _)| (d.id.as_str(), d)).chain(new.iter().map(|d| (d.id.as_str(), d))).collect();
    // Tasks that may run at the same time: neither is upstream of the other. Finished tasks are
    // no longer changing anything.
    let open: Vec<&TaskDef> = board.iter().filter(|(_, s)| !s.terminal()).map(|(d, _)| d).chain(new.iter()).collect();
    let deps: HashMap<&str, &[String]> = all.iter().map(|(id, d)| (*id, d.deps.as_slice())).collect();
    let upstream = |id: &str| -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut todo = vec![id.to_string()];
        while let Some(x) = todo.pop() {
            for d in deps.get(x.as_str()).copied().unwrap_or_default() {
                if seen.insert(d.clone()) {
                    todo.push(d.clone());
                }
            }
        }
        seen
    };
    let ups: HashMap<&str, HashSet<String>> = open.iter().map(|d| (d.id.as_str(), upstream(&d.id))).collect();
    // The dependency a verifier gets on its task must not close a loop.
    for d in new.iter().filter(|d| d.verifies.as_deref().is_some_and(|t| t != d.id && upstream(t).contains(&d.id))) {
        errors.push(format!("任务 {}：它验证的 {} 依赖它，依赖之间有环", d.id, d.verifies.as_deref().unwrap_or_default()));
    }
    let fresh: HashSet<&str> = new.iter().map(|d| d.id.as_str()).collect();
    for (i, a) in open.iter().enumerate() {
        for b in &open[i + 1..] {
            if !fresh.contains(a.id.as_str()) && !fresh.contains(b.id.as_str()) {
                continue;
            }
            if ups[a.id.as_str()].contains(&b.id) || ups[b.id.as_str()].contains(&a.id) {
                continue;
            }
            let shared: Vec<&str> = a.owns.iter().filter(|x| b.owns.iter().any(|y| overlaps(x, y))).map(String::as_str).collect();
            if !shared.is_empty() {
                errors.push(format!(
                    "任务 {} 和 {} 可能同时执行，却都负责 {}：用 deps 让一个等另一个完成，或者重新划分 owns",
                    a.id,
                    b.id,
                    shared.join("、")
                ));
            }
        }
    }
}

/// What a collaborator's task prompt adds for the team structure: the files it owns, or, for a
/// verification task, what it checks and how. `verified` is the task it verifies.
pub fn task_section(def: &TaskDef, verified: Option<&TaskDef>) -> String {
    let mut p = String::new();
    if !def.owns.is_empty() {
        p.push_str(&format!(
            "\n\n你负责的文件：{}。只改这些文件；其他文件归别的任务或主控，需要改时先用 janus.board_post 写一条 blocker 说明原因，或在结果的 question 中请主控决定。",
            def.owns.join("、")
        ));
    }
    if let Some(v) = verified {
        p.push_str(&format!(
            "\n\n你是任务 {id}「{title}」的独立验证者，它由别的协作者完成。不要修改任何文件，也不要替它修问题：读它的改动（git diff 或它改过的文件，用 janus.peers 传 task={id} 查看完整结果），运行测试、启动程序或复现它声称的行为，逐条核对它的验收标准：\n{accept}\n不要只复述执行者的结论：结果的 checks 中每条都写明你实际执行的命令或操作和看到的输出；发现的问题写成能复现的步骤，没有达到的如实写 passed: false。",
            id = v.id,
            title = v.title,
            accept = v.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"),
        ));
    }
    p
}

/// Why `accept` is refused for a task, if it verifies another one: a failed verification is
/// settled by fixing and verifying again, or by cancelling the verification on the record.
pub fn accept_refused(def: &TaskDef) -> Option<String> {
    let target = def.verifies.as_deref()?;
    Some(format!(
        "{id} 是 {target} 的独立验证，验证没通过不能直接接受。请用 janus.task_detail 查看 {id} 发现的问题，再用 janus.orchestrate 派一个修复任务（可以交给 {target} 的执行者，依赖 {id}）和一个新的验证任务（verifies 指向修复任务）；确认不需要验证时，用 janus.cancel 取消 {id} 并写明理由。",
        id = def.id
    ))
}

/// The part of the lead's brief on how to organise the team.
pub fn lead_guide() -> &'static str {
    "团队怎么组织：
- 先估计规模：一个人能在一轮里做完、或者步骤彼此紧密依赖的工作，你自己做；能按文件、模块或问题干净切开的，才分给协作者。同时执行的任务一般不超过协作者人数，每个任务的产出是一处可以单独核对的改动。
- 每个任务的说明写清三件事：负责什么、不能碰什么、做到什么算完成。会改文件的任务用 owns 列出它负责的文件或目录（相对工作目录，目录包含其下所有文件）；可能同时执行的任务 owns 不能重叠，Janus 校验时会拒绝重叠的编排。
- 重要的改动安排独立验证：再提交一个任务，verifies 写被验证任务的 id，交给另一个协作者。验证任务自动在被验证的任务完成后开始，只检查不改文件，验收标准要求它给出实际运行的命令和输出。验证没通过时不能用 janus.accept 放行：派修复任务和新的验证任务，或者 janus.cancel 取消验证并写明理由。没有第二个协作者时，你自己核对。"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn def(id: &str, assignee: &str, deps: &[&str], owns: &[&str], verifies: Option<&str>) -> TaskDef {
        TaskDef {
            id: id.into(),
            title: id.into(),
            desc: id.into(),
            deps: deps.iter().map(|x| x.to_string()).collect(),
            accept: vec![format!("{id} 做完")],
            assignee: assignee.into(),
            max_attempts: 2,
            owns: owns.iter().map(|x| x.to_string()).collect(),
            verifies: verifies.map(str::to_owned),
        }
    }

    fn errors(new: &mut [TaskDef], board: &[(TaskDef, TaskStatus)]) -> Vec<String> {
        let mut e = vec![];
        check(new, board, &mut e);
        e
    }

    #[test]
    fn owns_are_workspace_paths() {
        let mut e = vec![];
        let owns = read_owns(&json!(["./src/", "src", "README.md", "/etc/passwd", "../x", "a/../../b", ""]), "任务 T1", &mut e);
        assert_eq!(owns, vec!["src", "README.md"]);
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(overlaps("src", "src/a.rs") && overlaps("src/a.rs", "src") && overlaps("a", "a"));
        assert!(!overlaps("src", "src2/a.rs") && !overlaps("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn tasks_that_may_overlap_in_time_cannot_share_files() {
        // Parallel and overlapping: refused.
        let e = errors(&mut [def("T1", "m1", &[], &["src"], None), def("T2", "m2", &[], &["src/main.rs"], None)], &[]);
        assert!(e.len() == 1 && e[0].contains("T1 和 T2") && e[0].contains("deps"), "{e:?}");
        // Ordered through a chain of deps, or disjoint: fine.
        assert!(errors(&mut [def("T1", "m1", &[], &["src"], None), def("T2", "m2", &["T1"], &["src"], None)], &[]).is_empty());
        let board = [(def("T0", "m1", &[], &["src"], None), TaskStatus::Running)];
        assert!(errors(&mut [def("T1", "m2", &["T0"], &[], None), def("T2", "m3", &["T1"], &["src/a.rs"], None)], &board).is_empty());
        assert!(errors(&mut [def("T1", "m1", &[], &["src"], None), def("T2", "m2", &[], &["docs"], None)], &[]).is_empty());
        // Against an open task on the board, but not a finished one.
        let e = errors(&mut [def("T2", "m2", &[], &["src/a.rs"], None)], &board);
        assert!(e.len() == 1 && e[0].contains("T0 和 T2"), "{e:?}");
        let done = [(def("T0", "m1", &[], &["src"], None), TaskStatus::Succeeded)];
        assert!(errors(&mut [def("T2", "m2", &[], &["src/a.rs"], None)], &done).is_empty());
        // Two open tasks already on the board are not this orchestration's problem.
        let both = [(def("A", "m1", &[], &["x"], None), TaskStatus::Running), (def("B", "m2", &[], &["x"], None), TaskStatus::Pending)];
        assert!(errors(&mut [def("T3", "m3", &[], &["y"], None)], &both).is_empty());
    }

    #[test]
    fn a_verifier_is_someone_else_and_waits_for_the_work() {
        let mut new = [def("T1", "m1", &[], &["src"], None), def("V1", "m2", &[], &[], Some("T1"))];
        assert!(errors(&mut new, &[]).is_empty());
        assert_eq!(new[1].deps, vec!["T1"], "the verifier starts after the task it checks");
        // A verifier that does not overlap in time with the work may read the same files.
        let e = errors(&mut [def("T1", "m1", &[], &[], None), def("V1", "m1", &[], &["src"], Some("T1")), def("V2", "m2", &[], &[], Some("T9")), def("V3", "m2", &[], &[], Some("V3"))], &[]);
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("另一个协作者")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("不要写 owns")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V2") && x.contains("T9 不存在")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V3") && x.contains("不能验证自己")), "{e:?}");
        let e = errors(&mut [def("T1", "m1", &["V1"], &[], None), def("V1", "m2", &[], &[], Some("T1"))], &[]);
        assert!(e.iter().any(|x| x.contains("有环")), "{e:?}");
        // It may verify a task from an earlier orchestration.
        let board = [(def("T1", "m1", &[], &[], None), TaskStatus::Succeeded)];
        assert!(errors(&mut [def("V1", "m2", &[], &[], Some("T1"))], &board).is_empty());
        assert!(accept_refused(&def("V1", "m2", &["T1"], &[], Some("T1"))).is_some_and(|x| x.contains("janus.orchestrate")));
        assert!(accept_refused(&def("T1", "m1", &[], &[], None)).is_none());
    }

    #[test]
    fn prompts_name_the_owned_files_and_the_checked_task() {
        let work = def("T1", "m1", &[], &["src", "README.md"], None);
        let p = task_section(&work, None);
        assert!(p.contains("你负责的文件：src、README.md") && !p.contains("独立验证者"), "{p}");
        let p = task_section(&def("V1", "m2", &["T1"], &[], Some("T1")), Some(&work));
        assert!(p.contains("任务 T1「T1」的独立验证者") && p.contains("- T1 做完") && p.contains("不要修改任何文件"), "{p}");
        assert!(task_section(&def("T2", "m1", &[], &[], None), None).is_empty());
    }
}
