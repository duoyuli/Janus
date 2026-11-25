//! How the lead structures its team when it orchestrates: which files each task owns, and which
//! task an independent verifier checks.
//!
//! Collaborators all work in the same directory. Ownership is declared up front (`owns`), so two
//! tasks that may run at the same time cannot be given the same files; the conflict detection in
//! `record_edits` stays as the backstop for what was not declared. A verification task
//! (`verifies`) is done by another member, starts after the task it checks, changes no files,
//! and its failure cannot be waved through with `amend`: the lead orchestrates a fix and a new
//! verification, or cancels obsolete downstream work on the record. None of this adds task states or
//! event types; it is the orchestration's own data, checked when it is submitted.
// @project-doc docs/domains/20261031-goal_mode.md#team_structure

use crate::lang::{Lang, tr};
use crate::orchestrator::TaskDef;
use crate::status::TaskStatus;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The `owns` of one submitted task: paths relative to the workspace, a directory covering
/// everything under it. Leading `./` and trailing `/` are dropped; `..` and absolute paths are
/// reported, since a task may only change files inside the workspace.
pub fn read_owns(v: &Value, label: &str, errors: &mut Vec<String>, lang: Lang) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in v.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
        let raw_path = raw.trim();
        // One spelling per path: `src/./a.rs` and `src//a.rs` are `src/a.rs` (AT-09).
        let parts: Vec<&str> = raw_path.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        if raw_path.starts_with('/') || parts.contains(&"..") {
            errors.push(tr!(
                lang,
                "{label}：owns 里的 `{raw}` 不在工作目录内，写相对工作目录的路径",
                "{label}: `{raw}` in owns is outside the working directory; give paths relative to it"
            ));
            continue;
        }
        let p = parts.join("/");
        if p.is_empty() {
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

/// `path` is `owned` or lies under it. Letter case does not tell two paths apart: on macOS's
/// usual file system `src/Utils.rs` and `src/utils.rs` are the same file (AT-09).
pub fn covers(owned: &str, path: &str) -> bool {
    let (owned, path) = (owned.to_lowercase(), path.to_lowercase());
    path.strip_prefix(&owned).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Check the team structure of the new tasks against each other and against the open tasks
/// already on the board, after the basic checks of `validate_orchestration`. A verification task
/// gets the task it checks added to its dependencies; a continuation task gets the task whose
/// session it reuses added to its dependencies.
pub fn check(new: &mut [TaskDef], board: &[(TaskDef, TaskStatus)], errors: &mut Vec<String>, lang: Lang) {
    let mut add_deps: Vec<(usize, String)> = vec![];
    {
        let all: HashMap<&str, &TaskDef> =
            board.iter().map(|(d, _)| (d.id.as_str(), d)).chain(new.iter().map(|d| (d.id.as_str(), d))).collect();
        for (i, d) in new.iter().enumerate() {
            let id = &d.id;
            let label = tr!(lang, "子任务 {id}", "Subtask {id}");
            if let Some(target) = d.verifies.as_deref() {
                match all.get(target) {
                    None => errors.push(tr!(
                        lang,
                        "{label}：verifies 指向的子任务 {target} 不存在",
                        "{label}: verifies names subtask {target}, which does not exist"
                    )),
                    Some(_) if target == d.id => {
                        errors.push(tr!(lang, "{label}：不能验证自己", "{label}: a subtask cannot verify itself"))
                    }
                    Some(t) => {
                        if t.assignee == d.assignee {
                            let who = &d.assignee;
                            errors.push(tr!(
                                lang,
                                "{label}：验证 {target} 的必须是另一个协作者，不能是它的执行者 {who}",
                                "{label}: {target} must be verified by another collaborator, not by its assignee {who}"
                            ));
                        }
                        if !d.deps.iter().any(|x| x == target) {
                            add_deps.push((i, target.to_string()));
                        }
                    }
                }
                if !d.owns.is_empty() {
                    errors.push(tr!(
                        lang,
                        "{label}：验证子任务不能写 owns",
                        "{label}: a verification subtask cannot have owns"
                    ));
                }
            }
            if let Some(prev) = d.continues.as_deref() {
                if d.verifies.is_some() {
                    errors.push(tr!(
                        lang,
                        "{label}：验证子任务不能用 continues",
                        "{label}: a verification subtask cannot use continues"
                    ));
                }
                match all.get(prev) {
                    None => errors.push(tr!(
                        lang,
                        "{label}：continues 指向的子任务 {prev} 不存在",
                        "{label}: continues names subtask {prev}, which does not exist"
                    )),
                    Some(_) if prev == d.id => {
                        errors.push(tr!(lang, "{label}：不能续接自己", "{label}: a subtask cannot continue itself"))
                    }
                    Some(t) => {
                        if t.assignee != d.assignee {
                            let (theirs, mine) = (&t.assignee, &d.assignee);
                            errors.push(tr!(
                                lang,
                                "{label}：continues 指向的 {prev} 由 {theirs} 执行，只有同一个协作者能续接原会话（当前是 {mine}）",
                                "{label}: {prev}, named in continues, was done by {theirs}; only the same collaborator can continue its session (this one goes to {mine})"
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
                let id = &d.id;
                errors.push(tr!(
                    lang,
                    "子任务 {id}：它关联的 {target} 依赖它，依赖之间有环",
                    "Subtask {id}: {target}, which it refers to, depends on it, so the dependencies form a cycle"
                ));
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
                let (x, y, files) = (&a.id, &b.id, shared.join(lang.pick("、", ", ")));
                errors.push(tr!(
                    lang,
                    "子任务 {x} 和 {y} 可能同时执行，却都负责 {files}：用 deps 让一个等另一个完成，或者重新划分 owns",
                    "Subtasks {x} and {y} may run at the same time but both own {files}: make one wait for the other with deps, or divide owns again"
                ));
            }
        }
    }
}

/// Every task upstream of `id`, following dependencies transitively.
pub(crate) fn upstream_of(id: &str, deps: &HashMap<&str, &[String]>) -> HashSet<String> {
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
    /// Whether the files are still those it handed its result in on (`FilesSince::for_verifier`).
    pub files_since: &'a str,
}

/// What a collaborator's task prompt adds for the team structure: the files it owns, or, for a
/// verification task, what it checks, the targeted evidence package, and how to report `verdict`.
pub fn task_section(def: &TaskDef, verified: Option<&VerifiedContext<'_>>, lang: Lang) -> String {
    let list =
        |items: &[String], indent: &str| items.iter().map(|i| format!("{indent}- {i}")).collect::<Vec<_>>().join("\n");
    let sep = lang.pick("、", ", ");
    let mut p = String::new();
    if !def.owns.is_empty() {
        let owns = def.owns.join(sep);
        p.push_str(&tr!(
            lang,
            "\n\n你负责的文件：{owns}。只改这些；要改别的文件，先问负责人。",
            "\n\nFiles you own: {owns}. Change only these; ask the lead before you change another file."
        ));
    }
    if let Some(vc) = verified {
        let (id, title, accept) = (&vc.def.id, &vc.def.title, list(&vc.def.accept, "  "));
        p.push_str(&tr!(
            lang,
            "\n\n独立验证：你验证子任务 {id}「{title}」，它由别的协作者完成。\n- 不改任何文件，也不替它修问题；逐条核对它的验收标准：\n{accept}",
            "\n\nIndependent verification: you verify subtask {id} \"{title}\", which another collaborator did.\n- Change no files and do not fix its problems; check its acceptance criteria one by one:\n{accept}"
        ));
        if let Some(diff) = vc.diff_path.filter(|s| !s.is_empty()) {
            p.push_str(&tr!(lang, "\n- 它的改动（diff）：{diff}", "\n- Its changes (diff): {diff}"));
        }
        if !vc.edited_files.is_empty() {
            let files = vc.edited_files.join(sep);
            p.push_str(&tr!(
                lang,
                "\n- 执行者实际改动的文件：{files}",
                "\n- Files the assignee actually changed: {files}"
            ));
        }
        if !vc.trace_gaps.is_empty() {
            let gaps = list(vc.trace_gaps, "  ");
            p.push_str(&tr!(
                lang,
                "\n- 结果和工作目录对不上的地方：\n{gaps}",
                "\n- Where its result and the working directory disagree:\n{gaps}"
            ));
        }
        if !vc.weak_checks.is_empty() {
            let weak = list(vc.weak_checks, "  ");
            p.push_str(&tr!(
                lang,
                "\n- 执行者证据较弱的条目（只靠推理、未核实或没写方法）：\n{weak}",
                "\n- Criteria with weak evidence from the assignee (reasoning only, unverified, or no method):\n{weak}"
            ));
        }
        if !vc.risks.is_empty() {
            let risks = list(vc.risks, "  ");
            p.push_str(&tr!(lang, "\n- 执行者上报的风险：\n{risks}", "\n- Risks the assignee reported:\n{risks}"));
        }
        p.push_str(vc.files_since);
        p.push_str(lang.pick(
            "\n- 交结果时，checks 里逐条写明你做了什么、看到了什么，并用 verdict 给出判定：approved（达标）、rejected（有阻断问题，要原执行者修改）或 inconclusive（这里的证据不足以判定）。发现阻断问题也照常提交结果：你的工作是检查和报告，不是修复。",
            "\n- When you hand in your result, say in checks for each criterion what you did and what you saw, and give your verdict: approved (it meets them), rejected (a blocking problem the assignee must fix) or inconclusive (the evidence here cannot settle it). When you find a blocking problem, still hand in your result as usual: your job is to check and report, not to fix.",
        ));
    }
    p
}

/// How the files now compare with those a task handed its result in on.
#[derive(Debug, PartialEq, Eq)]
pub enum FilesSince {
    Same,
    /// Changed since: whether the commit moved, and the files that differ.
    Changed {
        commit: bool,
        paths: Vec<String>,
    },
}

impl FilesSince {
    pub fn of(earlier: &crate::git::FilesState, now: &crate::git::FilesState) -> FilesSince {
        let commit = earlier.head != now.head;
        let paths = now.changed_since(earlier);
        if !commit && paths.is_empty() { FilesSince::Same } else { FilesSince::Changed { commit, paths } }
    }

    fn changes(commit: bool, paths: &[String], lang: Lang) -> String {
        let mut parts = vec![];
        if commit {
            parts.push(lang.pick("提交变了", "the commit moved").to_string());
        }
        if !paths.is_empty() {
            let shown = paths.iter().take(12).map(String::as_str).collect::<Vec<_>>().join(lang.pick("、", ", "));
            let n = paths.len();
            let more =
                if n > 12 { tr!(lang, " 等 {n} 个文件", " and others, {n} files in all") } else { String::new() };
            parts.push(tr!(lang, "改了 {shown}{more}", "changed {shown}{more}"));
        }
        parts.join(lang.pick("，", "; "))
    }

    /// What a verifier is told about the files it checks, against those `target` handed its
    /// result in on: the same files give the same results to the same checks.
    // @project-doc docs/domains/20261031-goal_mode.md#evidence_reuse
    pub fn for_verifier(&self, target: &str, lang: Lang) -> String {
        match self {
            FilesSince::Same => tr!(
                lang,
                "\n- 文件状态：和 {target} 提交结果时完全相同（同一个提交，改动过的文件内容一致），它的证据描述的就是现在的文件。checks 里写明哪些条目采信了它的证据、哪些是你自己核实的。",
                "\n- Files: exactly as when {target} handed in its result (the same commit, the same content of the changed files), so its evidence describes the files as they are now. In checks, say which criteria rest on its evidence and which you verified yourself."
            ),
            FilesSince::Changed { commit, paths } => {
                let changes = Self::changes(*commit, paths, lang);
                tr!(
                    lang,
                    "\n- 文件状态：{target} 提交结果之后又有变化（{changes}）。它的证据描述的是变化之前的文件，只对没受这些变化影响的条目有效。",
                    "\n- Files: changed since {target} handed in its result ({changes}). Its evidence describes the files before that, so it holds only for criteria these changes do not touch."
                )
            }
        }
    }

    /// The line on the board under a finished verification: whether it checked the files the
    /// verified task handed in.
    pub fn for_lead(&self, target: &str, lang: Lang) -> String {
        match self {
            FilesSince::Same => {
                tr!(
                    lang,
                    "核对时的文件和 {target} 提交结果时相同",
                    "checked the same files {target} handed its result in on"
                )
            }
            FilesSince::Changed { commit, paths } => {
                let changes = Self::changes(*commit, paths, lang);
                tr!(
                    lang,
                    "核对时的文件和 {target} 提交结果时不同（{changes}）",
                    "checked files that differ from those {target} handed its result in on ({changes})"
                )
            }
        }
    }
}

/// Why `amend` is refused for a task, if it verifies another one: neither its verdict nor its
/// conclusion is the lead's to rewrite. A failed verification is settled by fixing and verifying
/// again (the tool's description says how), or by cancelling.
pub fn amend_refused(def: &TaskDef, lang: Lang) -> Option<String> {
    let target = def.verifies.as_deref()?;
    let id = &def.id;
    Some(tr!(
        lang,
        "{id} 是 {target} 的独立验证，janus.amend 不能改它的判定和结论",
        "{id} independently verifies {target}; janus.amend cannot change its verdict or conclusion"
    ))
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
        check(new, board, &mut e, Lang::Zh);
        e
    }

    #[test]
    fn owns_are_workspace_paths() {
        let mut e = vec![];
        let owns = read_owns(
            &json!(["./src/", "src", "README.md", "/etc/passwd", "../x", "a/../../b", ""]),
            "子任务 T1",
            &mut e,
            Lang::Zh,
        );
        assert_eq!(owns, vec!["src", "README.md"]);
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(overlaps("src", "src/a.rs") && overlaps("src/a.rs", "src") && overlaps("a", "a"));
        assert!(!overlaps("src", "src2/a.rs") && !overlaps("src/a.rs", "src/b.rs"));
        // Other spellings of the same file (AT-09).
        let owns = read_owns(&json!(["src/./parser.rs", "src//parser.rs", "lib/"]), "子任务 T2", &mut e, Lang::Zh);
        assert_eq!(owns, vec!["src/parser.rs", "lib"]);
        assert!(overlaps("src/Utils.rs", "src/utils.rs") && overlaps("SRC", "src/a.rs"));
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
        assert!(e.iter().any(|x| x.contains("V1") && x.contains("不能写 owns")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V2") && x.contains("T9 不存在")), "{e:?}");
        assert!(e.iter().any(|x| x.contains("V3") && x.contains("不能验证自己")), "{e:?}");
        let e = errors(&mut [def("T1", "m1", &["V1"], &[], None), def("V1", "m2", &[], &[], Some("T1"))], &[]);
        assert!(e.iter().any(|x| x.contains("有环")), "{e:?}");
        // It may verify a task from an earlier orchestration.
        let board = [(def("T1", "m1", &[], &[], None), TaskStatus::Succeeded)];
        assert!(errors(&mut [def("V1", "m2", &[], &[], Some("T1"))], &board).is_empty());
        assert!(
            amend_refused(&def("V1", "m2", &["T1"], &[], Some("T1")), Lang::Zh)
                .is_some_and(|x| x.contains("V1 是 T1 的独立验证"))
        );
        assert!(amend_refused(&def("T1", "m1", &[], &[], None), Lang::Zh).is_none());
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
    fn prompts_name_the_owned_files_and_the_checked_task() {
        let work = def("T1", "m1", &[], &["src", "README.md"], None);
        let p = task_section(&work, None, Lang::Zh);
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
            files_since: "",
        };
        let p = task_section(&def("V1", "m2", &["T1"], &[], Some("T1")), Some(&vc), Lang::Zh);
        assert!(
            p.contains("独立验证：你验证子任务 T1「T1」")
                && p.contains("- T1 做完")
                && p.contains("不改任何文件")
                && p.contains("/tmp/T1-a1.diff")
                && p.contains("src/lib.rs")
                && p.contains("负责 README.md")
                && p.contains("推理判断")
                && p.contains("verdict"),
            "{p}"
        );
        assert!(task_section(&def("T2", "m1", &[], &[], None), None, Lang::Zh).is_empty());
        let en = task_section(&def("V1", "m2", &["T1"], &[], Some("T1")), Some(&vc), Lang::En);
        assert!(en.contains("Independent verification: you verify subtask T1") && en.contains("verdict"), "{en}");
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
