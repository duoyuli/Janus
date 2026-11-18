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
// @project-doc docs/domains/20261031-goal_mode.md#team_structure

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
pub fn overlaps(a: &str, b: &str) -> bool {
    covers(a, b) || covers(b, a)
}

/// `path` is `owned` or lies under it.
pub fn covers(owned: &str, path: &str) -> bool {
    path.strip_prefix(owned).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Check the team structure of the new tasks against each other and against the open tasks
/// already on the board, after the basic checks of `validate_orchestration`. A verification task
/// gets the task it checks added to its dependencies; a continuation task gets the task whose
/// session it reuses added to its dependencies.
pub fn check(new: &mut [TaskDef], board: &[(TaskDef, TaskStatus)], errors: &mut Vec<String>) {
    let mut add_deps: Vec<(usize, String)> = vec![];
    {
        let all: HashMap<&str, &TaskDef> =
            board.iter().map(|(d, _)| (d.id.as_str(), d)).chain(new.iter().map(|d| (d.id.as_str(), d))).collect();
        for (i, d) in new.iter().enumerate() {
            let label = format!("子任务 {}", d.id);
            if let Some(target) = d.verifies.as_deref() {
                match all.get(target) {
                    None => errors.push(format!("{label}：verifies 指向的子任务 {target} 不存在")),
                    Some(_) if target == d.id => errors.push(format!("{label}：不能验证自己")),
                    Some(t) => {
                        if t.assignee == d.assignee {
                            errors.push(format!(
                                "{label}：验证 {target} 的必须是另一个协作者，不能是它的执行者 {}",
                                d.assignee
                            ));
                        }
                        if !d.deps.iter().any(|x| x == target) {
                            add_deps.push((i, target.to_string()));
                        }
                    }
                }
                if !d.owns.is_empty() {
                    errors.push(format!("{label}：验证子任务只检查、不改文件，不要写 owns"));
                }
            }
            if let Some(prev) = d.continues.as_deref() {
                if d.verifies.is_some() {
                    errors.push(format!("{label}：验证子任务需要独立核对，不能使用 continues"));
                }
                match all.get(prev) {
                    None => errors.push(format!("{label}：continues 指向的子任务 {prev} 不存在")),
                    Some(_) if prev == d.id => errors.push(format!("{label}：不能续接自己")),
                    Some(t) => {
                        if t.assignee != d.assignee {
                            errors.push(format!(
                                "{label}：continues 指向的 {prev} 由 {} 执行，只有同一个协作者能续接原会话（当前是 {}）",
                                t.assignee, d.assignee
                            ));
                        }
                        if !d.deps.iter().any(|x| x == prev) {
                            add_deps.push((i, prev.to_string()));
                        }
                    }
                }
            }
        }
    }
    for (i, dep) in add_deps {
        if !new[i].deps.contains(&dep) {
            new[i].deps.push(dep);
        }
    }

    let all: HashMap<&str, &TaskDef> =
        board.iter().map(|(d, _)| (d.id.as_str(), d)).chain(new.iter().map(|d| (d.id.as_str(), d))).collect();
    // Tasks that may run at the same time: neither is upstream of the other. Finished tasks are
    // no longer changing anything.
    let open: Vec<&TaskDef> = board.iter().filter(|(_, s)| !s.terminal()).map(|(d, _)| d).chain(new.iter()).collect();
    let deps: HashMap<&str, &[String]> = all.iter().map(|(id, d)| (*id, d.deps.as_slice())).collect();
    let upstream = |id: &str| upstream_of(id, &deps);
    let ups: HashMap<&str, HashSet<String>> = open.iter().map(|d| (d.id.as_str(), upstream(&d.id))).collect();
    // The dependency a verifier or continuation gets on its target must not close a loop.
    for d in new.iter() {
        for target in [d.verifies.as_deref(), d.continues.as_deref()].into_iter().flatten() {
            if target != d.id && upstream(target).contains(&d.id) {
                errors.push(format!("子任务 {}：它关联的 {target} 依赖它，依赖之间有环", d.id));
            }
        }
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
            let shared: Vec<&str> =
                a.owns.iter().filter(|x| b.owns.iter().any(|y| overlaps(x, y))).map(String::as_str).collect();
            if !shared.is_empty() {
                errors.push(format!(
                    "子任务 {} 和 {} 可能同时执行，却都负责 {}：用 deps 让一个等另一个完成，或者重新划分 owns",
                    a.id,
                    b.id,
                    shared.join("、")
                ));
            }
        }
    }
}

/// Every task upstream of `id`, following dependencies transitively.
fn upstream_of(id: &str, deps: &HashMap<&str, &[String]>) -> HashSet<String> {
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
}

/// The open task, other than `me`, that owns `path` and may be running alongside `me`: neither
/// is upstream of the other. A task upstream or downstream works before or after `me`, and a
/// finished task's ownership has lapsed, so changing its files is not stepping on anyone.
pub fn owner_of<'a>(path: &str, me: &str, board: &[(&'a TaskDef, TaskStatus)]) -> Option<&'a str> {
    let deps: HashMap<&str, &[String]> = board.iter().map(|(d, _)| (d.id.as_str(), d.deps.as_slice())).collect();
    let mine = upstream_of(me, &deps);
    board
        .iter()
        .filter(|(d, s)| d.id != me && !s.terminal() && !mine.contains(&d.id))
        .filter(|(d, _)| !upstream_of(&d.id, &deps).contains(me))
        .find(|(d, _)| d.owns.iter().any(|o| overlaps(o, path)))
        .map(|(d, _)| d.id.as_str())
}

/// A collaborator claims `paths` while it works (`janus.claim`): ownership taken when the work
/// shows what it needs, not only what the lead guessed up front. It lapses when the task ends,
/// as all ownership does. Refused for a verifier, which changes no files, and for a path an
/// open task that may run alongside owns. Returns the paths not already the task's own.
pub fn claim(me: &TaskDef, paths: &[String], board: &[(&TaskDef, TaskStatus)]) -> Result<Vec<String>, String> {
    if me.verifies.is_some() {
        return Err(format!("{} 是验证子任务，只检查不改文件，不能认领文件", me.id));
    }
    let taken: Vec<String> =
        paths.iter().filter_map(|p| owner_of(p, &me.id, board).map(|o| format!("{p} 归 {o} 负责"))).collect();
    if !taken.is_empty() {
        return Err(format!(
            "不能认领：{}。需要改这些文件时，用 janus.board_post 写一条 blocker 请负责人协调",
            taken.join("；")
        ));
    }
    Ok(paths.iter().filter(|p| !me.owns.iter().any(|o| covers(o, p))).cloned().collect())
}

/// Targeted workspace facts handed to an independent verifier about the task it checks: its scoped
/// diff artifact, the files it actually edited, any trace discrepancies, weak-evidence reminders
/// (`reasoning` / `unverified` / unspecified), and risks the executor reported.
pub struct VerifiedContext<'a> {
    pub def: &'a TaskDef,
    pub diff_path: Option<&'a str>,
    pub edited_files: &'a [String],
    pub trace_gaps: &'a [String],
    pub weak_checks: &'a [String],
    pub risks: &'a [String],
}

/// What a collaborator's task prompt adds for the team structure: the files it owns, or, for a
/// verification task, what it checks, the targeted evidence package, and how to report `verdict`.
pub fn task_section(def: &TaskDef, verified: Option<&VerifiedContext<'_>>) -> String {
    let mut p = String::new();
    if !def.owns.is_empty() {
        p.push_str(&format!(
            "\n\n你负责的文件：{}。只改这些文件；其他文件归别的子任务或负责人，需要改时先用 janus.board_post 写一条 blocker 说明原因，或在结果的 question 中请负责人决定。",
            def.owns.join("、")
        ));
    }
    if let Some(vc) = verified {
        let v = vc.def;
        p.push_str(&format!(
            "\n\n你是子任务 {id}「{title}」的独立验证者，它由别的协作者完成。不要修改任何文件，也不要替它修问题：读它的改动（用 janus.peers 传 task={id} 可查看完整结果），运行测试、启动程序或复现它声称的行为，逐条核对它的验收标准：\n{accept}",
            id = v.id,
            title = v.title,
            accept = v.accept.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"),
        ));
        if let Some(diff) = vc.diff_path.filter(|s| !s.is_empty()) {
            p.push_str(&format!("\n专属 diff 产物：{diff}（可直接读取核对改动范围）"));
        }
        if !vc.edited_files.is_empty() {
            p.push_str(&format!("\n执行者实际改动的文件：{}", vc.edited_files.join("、")));
        }
        if !vc.trace_gaps.is_empty() {
            p.push_str(&format!(
                "\n痕迹核对异常（结果与工作目录不一致，请重点核查）：\n{}",
                vc.trace_gaps.iter().map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n")
            ));
        }
        if !vc.weak_checks.is_empty() {
            p.push_str(&format!(
                "\n执行者证据较弱的条目（仅靠推理、未核实或未说明方法，请优先用 test 或 command 实测）：\n{}",
                vc.weak_checks.iter().map(|w| format!("- {w}")).collect::<Vec<_>>().join("\n")
            ));
        }
        if !vc.risks.is_empty() {
            p.push_str(&format!(
                "\n执行者上报的风险：\n{}",
                vc.risks.iter().map(|r| format!("- {r}")).collect::<Vec<_>>().join("\n")
            ));
        }
        p.push_str(
            "\n不要只复述执行者的结论：结果的 checks 中每条都写明你实际执行的命令或操作和看到的输出；完成检查后，在结构化结果 JSON 中用 verdict 写明对被验证产物的判定：\"approved\"（产物达标）、\"rejected\"（存在阻断问题，需要原执行者修改）或 \"inconclusive\"（当前环境证据不足以判定）。即使发现阻断问题填了 passed: false 或 verdict: \"rejected\"，也请给出具体可复现的证据并正常提交结果——你的工作是独立检查和报告，不是修复。",
        );
    }
    p
}

/// Why `accept` is refused for a task, if it verifies another one: a failed verification is
/// settled by fixing and verifying again, or by cancelling the verification on the record.
pub fn accept_refused(def: &TaskDef) -> Option<String> {
    let target = def.verifies.as_deref()?;
    Some(format!(
        "{id} 是 {target} 的独立验证，验证没通过不能直接接受。请用 janus.task_detail 查看 {id} 发现的问题，再用 janus.orchestrate 派一个修复子任务（交给 {target} 的执行者时可设 continues=\"{target}\" 复用原会话，依赖 {id}）和一个新的验证子任务（verifies 指向修复子任务）；确认不需要验证时，用 janus.cancel 取消 {id} 并写明理由。",
        id = def.id
    ))
}

/// The part of the lead's brief on how to organise the team.
pub fn lead_guide() -> &'static str {
    "Organizing the team:
- Estimate the scope first. Handle work one person can finish in a turn and work with tightly coupled steps yourself. Delegate work that divides cleanly by file, module, or question. Keep parallel work within the available collaborator instances and give each subtask an independently verifiable change.
- Establish shared contracts first. When parallel modules share data structures, signatures, routes, or event protocols, define those contracts and types in an upstream subtask or implement them yourself before dispatching the parallel work.
- State what each subtask owns, what it must leave untouched, and what counts as done. Use owns for files or directories relative to the working directory. A directory covers all files below it. Janus rejects overlaps between subtasks that could run at the same time.
- Collaborators can call janus.claim before editing files outside owns. The board reports claims and out-of-scope edits to files another active subtask owns. If a subtask needs splitting, the collaborator can propose a split with janus.request_split, and you decide whether to revise the plan.
- On completion, Janus checks each result against the subtask's edits. The completion entry flags missing changes to owned files, changes outside the assigned scope, and edits made by a verifier. Inspect the current working directory before relying on a summary.
- Arrange independent verification for important changes. Assign another collaborator a subtask with verifies set to the id being verified. Prefer a different model when the team has multiple models or Agents. The verification subtask starts after the target finishes. The verifier inspects the result without changing files and returns an approved, rejected, or inconclusive verdict. You cannot bypass an unsuccessful verification with janus.accept. Assign a fix and a new verification, or cancel the verification with janus.cancel and state why. When assigning the fix to the original collaborator, continues can reuse its session and avoid rereading the code. If a second collaborator is unavailable, verify the result yourself."
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
            continues: None,
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
        let owns = read_owns(
            &json!(["./src/", "src", "README.md", "/etc/passwd", "../x", "a/../../b", ""]),
            "子任务 T1",
            &mut e,
        );
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
        assert!(
            errors(&mut [def("T1", "m1", &[], &["src"], None), def("T2", "m2", &["T1"], &["src"], None)], &[])
                .is_empty()
        );
        let board = [(def("T0", "m1", &[], &["src"], None), TaskStatus::Running)];
        assert!(
            errors(&mut [def("T1", "m2", &["T0"], &[], None), def("T2", "m3", &["T1"], &["src/a.rs"], None)], &board)
                .is_empty()
        );
        assert!(
            errors(&mut [def("T1", "m1", &[], &["src"], None), def("T2", "m2", &[], &["docs"], None)], &[]).is_empty()
        );
        // Against an open task on the board, but not a finished one.
        let e = errors(&mut [def("T2", "m2", &[], &["src/a.rs"], None)], &board);
        assert!(e.len() == 1 && e[0].contains("T0 和 T2"), "{e:?}");
        let done = [(def("T0", "m1", &[], &["src"], None), TaskStatus::Succeeded)];
        assert!(errors(&mut [def("T2", "m2", &[], &["src/a.rs"], None)], &done).is_empty());
        // Two open tasks already on the board are not this orchestration's problem.
        let both = [
            (def("A", "m1", &[], &["x"], None), TaskStatus::Running),
            (def("B", "m2", &[], &["x"], None), TaskStatus::Pending),
        ];
        assert!(errors(&mut [def("T3", "m3", &[], &["y"], None)], &both).is_empty());
    }

    #[test]
    fn a_verifier_is_someone_else_and_waits_for_the_work() {
        let mut new = [def("T1", "m1", &[], &["src"], None), def("V1", "m2", &[], &[], Some("T1"))];
        assert!(errors(&mut new, &[]).is_empty());
        assert_eq!(new[1].deps, vec!["T1"], "the verifier starts after the task it checks");
        // A verifier that does not overlap in time with the work may read the same files.
        let e = errors(
            &mut [
                def("T1", "m1", &[], &[], None),
                def("V1", "m1", &[], &["src"], Some("T1")),
                def("V2", "m2", &[], &[], Some("T9")),
                def("V3", "m2", &[], &[], Some("V3")),
            ],
            &[],
        );
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("另一个协作者")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("不要写 owns")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V2") && x.contains("T9 不存在")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V3") && x.contains("不能验证自己")), "{e:?}");
        let e = errors(&mut [def("T1", "m1", &["V1"], &[], None), def("V1", "m2", &[], &[], Some("T1"))], &[]);
        assert!(e.iter().any(|x| x.contains("有环")), "{e:?}");
        // It may verify a task from an earlier orchestration.
        let board = [(def("T1", "m1", &[], &[], None), TaskStatus::Succeeded)];
        assert!(errors(&mut [def("V1", "m2", &[], &[], Some("T1"))], &board).is_empty());
        assert!(
            accept_refused(&def("V1", "m2", &["T1"], &[], Some("T1"))).is_some_and(|x| x.contains("janus.orchestrate"))
        );
        assert!(accept_refused(&def("T1", "m1", &[], &[], None)).is_none());
    }

    #[test]
    fn a_file_belongs_to_an_open_task_that_may_run_alongside() {
        let (a, b, c, d) = (
            def("A", "m1", &[], &["src"], None),
            def("B", "m2", &[], &[], None),
            def("C", "m3", &["A"], &["docs"], None),
            def("D", "m1", &[], &["lib.rs"], None),
        );
        let board = [
            (&a, TaskStatus::Running),
            (&b, TaskStatus::Running),
            (&c, TaskStatus::Pending),
            (&d, TaskStatus::Succeeded),
        ];
        assert_eq!(owner_of("src/main.rs", "B", &board), Some("A"));
        assert_eq!(owner_of("src", "B", &board), Some("A"), "a whole directory touches A's files");
        assert_eq!(owner_of("src/main.rs", "A", &board), None, "its own files");
        assert_eq!(owner_of("docs/x.md", "A", &board), None, "C runs after A: not alongside");
        assert_eq!(owner_of("docs/x.md", "B", &board), Some("C"), "pending but unordered with B");
        assert_eq!(owner_of("lib.rs", "B", &board), None, "a finished task's ownership has lapsed");
        assert_eq!(owner_of("srcx/a.rs", "B", &board), None);
    }

    #[test]
    fn claims_take_free_files_and_are_refused_for_taken_ones() {
        let (a, mut b, v) = (
            def("A", "m1", &[], &["src"], None),
            def("B", "m2", &[], &["docs"], None),
            def("V", "m3", &[], &[], Some("A")),
        );
        let board = [(&a, TaskStatus::Running), (&b, TaskStatus::Running), (&v, TaskStatus::Pending)];
        assert_eq!(
            claim(&b, &["README.md".into(), "docs/a.md".into()], &board).unwrap(),
            vec!["README.md"],
            "docs/a.md is already B's"
        );
        let e = claim(&b, &["src/a.rs".into(), "x".into()], &board).unwrap_err();
        assert!(e.contains("src/a.rs 归 A 负责") && e.contains("blocker"), "{e}");
        assert!(claim(&v, &["y".into()], &board).unwrap_err().contains("验证子任务"));
        b.owns.clear();
        let done = [(&a, TaskStatus::Succeeded)];
        assert_eq!(claim(&b, &["src/a.rs".into()], &done).unwrap(), vec!["src/a.rs"], "A is done");
    }

    #[test]
    fn prompts_name_the_owned_files_and_the_checked_task() {
        let work = def("T1", "m1", &[], &["src", "README.md"], None);
        let p = task_section(&work, None);
        assert!(p.contains("你负责的文件：src、README.md") && !p.contains("独立验证者"), "{p}");
        let edited = vec!["src/lib.rs".to_string()];
        let gaps = vec!["负责 README.md，但没有改动".to_string()];
        let weak = vec!["第 1 条是推理判断".to_string()];
        let risks = vec!["边界未覆盖".to_string()];
        let vc = VerifiedContext {
            def: &work,
            diff_path: Some("/tmp/T1-a1.diff"),
            edited_files: &edited,
            trace_gaps: &gaps,
            weak_checks: &weak,
            risks: &risks,
        };
        let p = task_section(&def("V1", "m2", &["T1"], &[], Some("T1")), Some(&vc));
        assert!(
            p.contains("子任务 T1「T1」的独立验证者")
                && p.contains("- T1 做完")
                && p.contains("不要修改任何文件")
                && p.contains("/tmp/T1-a1.diff")
                && p.contains("src/lib.rs")
                && p.contains("负责 README.md")
                && p.contains("推理判断")
                && p.contains("verdict"),
            "{p}"
        );
        assert!(task_section(&def("T2", "m1", &[], &[], None), None).is_empty());
    }

    #[test]
    fn continuation_requires_the_same_assignee_and_orders_after_target() {
        let mut fix = def("T2", "m1", &[], &["src"], None);
        fix.continues = Some("T1".into());
        let mut batch = [def("T1", "m1", &[], &["src"], None), fix];
        assert!(errors(&mut batch, &[]).is_empty());
        assert_eq!(batch[1].deps, vec!["T1"], "continues adds target to deps so owns do not clash");

        let mut wrong_member = def("T2", "m2", &[], &[], None);
        wrong_member.continues = Some("T1".into());
        let mut verifier_cont = def("V1", "m2", &[], &[], Some("T1"));
        verifier_cont.continues = Some("T1".into());
        let mut self_cont = def("T3", "m1", &[], &[], None);
        self_cont.continues = Some("T3".into());
        let e = errors(&mut [def("T1", "m1", &[], &[], None), wrong_member, verifier_cont, self_cont], &[]);
        assert!(e.iter().any(|x| x.contains("T2") && x.contains("同一个协作者")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("验证子任务")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("T3") && x.contains("不能续接自己")), "{e:?}");
    }
}
