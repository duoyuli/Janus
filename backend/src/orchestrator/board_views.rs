//! What the board shows a session about its subtasks: one subtask in full (`board` with `task`),
//! and a collaborator's view of the others (`board`).

use super::board::no_task;
use super::*;
use crate::lang::tr;

impl App {
    /// Everything the board knows about one task, by id: what the board messages and handoffs only
    /// summarise. Its upstream and downstream tasks are named, so a caller can follow the chain
    /// one task at a time.
    // @project-doc docs/interfaces/20261031-board_mcp.md#task_detail
    pub(super) fn task_detail(&self, run: &Run, task: &str) -> std::result::Result<String, String> {
        let lang = run.lang;
        let sep = list_sep(lang);
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        // The usage ledger is read-only and never a reason to fail: a task without usage still
        // answers, saying 「未上报」.
        let usage = crate::usage::ledger(&events);
        let st = run.st.lock().unwrap();
        let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
            return Err(no_task(lang, task));
        };
        let name = |agent: &str| run.member(agent).map(|m| m.name.clone()).unwrap_or_else(|| agent.to_string());
        let label = |id: &str| {
            let status = st
                .tasks
                .iter()
                .find(|x| x.def.id == id)
                .map(|x| x.status().label_in(lang))
                .unwrap_or(lang.pick("不在看板上", "not on the board"));
            tr!(lang, "{id}（{status}）", "{id} ({status})")
        };
        let bullets = |items: &[String], indent: &str| {
            items.iter().map(|x| format!("{indent}- {x}")).collect::<Vec<_>>().join("\n")
        };
        let mine = |e: &&Event| e.task.as_deref() == Some(task);
        let (it, who, agent, status, tried, desc) = (
            titled(lang, task, &t.def.title),
            name(&t.agent),
            &t.agent,
            t.status().label_in(lang),
            t.attempts,
            &t.def.desc,
        );
        let mut out = vec![
            tr!(
                lang,
                "子任务 {it}· 执行者 {who}（{agent}）· {status} · 已尝试 {tried} 次",
                "Subtask {it} · assignee {who} ({agent}) · {status} · {tried} attempts so far"
            ),
            tr!(lang, "说明：{desc}", "Instructions: {desc}"),
        ];
        let accept = bullets(&t.def.accept, "");
        out.push(tr!(lang, "验收标准：\n{accept}", "Acceptance criteria:\n{accept}"));
        if !t.def.owns.is_empty() {
            let owns = t.def.owns.join(sep);
            out.push(tr!(lang, "负责的文件：{owns}", "Owns: {owns}"));
        }
        if let Some(v) = &t.def.verifies {
            out.push(tr!(lang, "独立验证：{v}", "Independently verifies: {v}"));
        }
        if let Some(c) = &t.def.continues {
            out.push(tr!(lang, "续接会话：{c}", "Continues the session of: {c}"));
        }
        let downstream: Vec<String> =
            st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).map(|x| label(&x.def.id)).collect();
        if !t.def.deps.is_empty() {
            let up = t.def.deps.iter().map(|d| label(d)).collect::<Vec<_>>().join(sep);
            out.push(tr!(lang, "上游：{up}", "Upstream: {up}"));
        }
        if !downstream.is_empty() {
            let down = downstream.join(sep);
            out.push(tr!(lang, "下游：{down}", "Downstream: {down}"));
        }
        if let Some(r) = &t.result {
            let summary = &r.summary;
            out.push(tr!(lang, "结果：{summary}", "Result: {summary}"));
            if let Some(vd) = t.verdict() {
                let label = vd.label_in(lang);
                out.push(tr!(lang, "验证判定：{label}", "Verdict: {label}"));
            }
            if let Some(h) = &r.handoff {
                out.push(tr!(lang, "交接说明：{h}", "Handoff: {h}"));
            }
            let totals = Summary::of_in(&t.def.accept, &r.checks, lang);
            let (passed, criteria, counts) = (totals.passed, totals.criteria, totals.counts(lang));
            let checks = r.checks.iter().map(|c| format!("\n  {}", c.line(lang))).collect::<String>();
            out.push(tr!(
                lang,
                "逐条验收（{criteria} 条，通过 {passed} 条）：{checks}",
                "Checks ({criteria} criteria, {passed} passed):{checks}"
            ));
            out.push(tr!(lang, "证据方法：{counts}", "Evidence methods: {counts}"));
            if !totals.reminders.is_empty() {
                let reminders = totals.reminders.join(item_sep(lang));
                out.push(tr!(
                    lang,
                    "提醒（不影响是否通过）：{reminders}",
                    "Reminders (they do not affect passing): {reminders}"
                ));
            }
            if !r.evidence.is_empty() {
                let evidence = bullets(&r.evidence, "");
                out.push(tr!(lang, "其他证据：\n{evidence}", "Other evidence:\n{evidence}"));
            }
            if !r.risks.is_empty() {
                let risks = bullets(&r.risks, "");
                out.push(tr!(lang, "风险：\n{risks}", "Risks:\n{risks}"));
            }
        }
        // Those who verified this task independently: their own conclusion, kept apart from the
        // evidence the executor reported.
        for v in st.tasks.iter().filter(|v| v.def.verifies.as_deref() == Some(task)) {
            let verdict_tag = v
                .verdict()
                .map(|vd| {
                    let label = vd.label_in(lang);
                    tr!(lang, " · 判定「{label}」", " · verdict \"{label}\"")
                })
                .unwrap_or_default();
            let (id, status) = (&v.def.id, v.status().label_in(lang));
            out.push(tr!(
                lang,
                "独立验证 {id}：{status}{verdict_tag}",
                "Independent verification {id}: {status}{verdict_tag}"
            ));
            if let Some(r) = &v.result {
                out.push(format!("  {}", r.summary));
                for c in &r.checks {
                    out.push(format!("  {}", c.line(lang)));
                }
                if !r.evidence.is_empty() {
                    let evidence = bullets(&r.evidence, "  ");
                    out.push(tr!(lang, "  独立证据：\n{evidence}", "  Its own evidence:\n{evidence}"));
                }
            }
            if let Some(f) = st.last_verification.get(&v.def.id).filter(|_| v.status() != TaskStatus::Succeeded) {
                out.push(tr!(lang, "  没通过核对：\n{f}", "  Failed its check:\n{f}"));
            }
        }
        if !t.amendments.is_empty() {
            let amendments = t.amendments.join(item_sep(lang));
            out.push(tr!(
                lang,
                "负责人的修订（以此为准）：{amendments}",
                "The lead's revisions (these stand): {amendments}"
            ));
        }
        if let Some(v) = st.last_verification.get(task).filter(|_| t.status() != TaskStatus::Succeeded) {
            out.push(tr!(lang, "最近一次没通过验收核对：\n{v}", "Last failed acceptance check:\n{v}"));
        }
        if let Some(e) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind() == EventKind::AttemptFailed)
            .filter(|_| t.status() != TaskStatus::Succeeded)
        {
            let error = e.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or(&e.text);
            out.push(tr!(lang, "最近一次尝试失败：{error}", "Last failed attempt: {error}"));
        }
        let files = st.traces.files(task);
        if !files.is_empty() {
            let files = files.into_iter().collect::<Vec<_>>().join(sep);
            out.push(tr!(lang, "改过的文件：{files}", "Files changed: {files}"));
        }
        if let Some(s) = t.success_seq() {
            let note = crate::traces::stale_note(&st.traces.edited_after(task, s), lang);
            if !note.is_empty() {
                out.push(note);
            }
        }
        if let Some(gaps) = events
            .iter()
            .rev()
            .filter(mine)
            .find(|e| e.kind() == EventKind::TaskSucceeded)
            .filter(|_| t.status() == TaskStatus::Succeeded)
            .and_then(|e| e.data.as_ref()?["traceGaps"].as_array().cloned())
            .filter(|g| !g.is_empty())
        {
            let gaps = gaps.iter().filter_map(|g| g.as_str()).map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n");
            out.push(tr!(
                lang,
                "痕迹核对（结果和工作目录对不上的地方）：\n{gaps}",
                "Trace check (where the result and the working directory disagree):\n{gaps}"
            ));
        }
        let arts: Vec<String> = t.artifacts.iter().map(|a| a.path.clone()).collect();
        if !arts.is_empty() {
            let arts = arts.join(sep);
            out.push(tr!(lang, "产物：{arts}", "Artifacts: {arts}"));
        }
        // A conflict is recorded on the task whose edit found it; both sides see it here.
        let theirs = |e: &&Event| {
            e.kind() == EventKind::TaskConflict
                && e.data.as_ref().is_some_and(|d| d["byLead"] != true && d["other"] == task)
        };
        let talk: Vec<String> = events
            .iter()
            .filter(|e| mine(e) || theirs(e))
            .filter_map(|e| {
                let mut d = e.data.clone().unwrap_or_default();
                if theirs(&e) {
                    d["other"] = json!(e.task.clone().unwrap_or_default());
                }
                let paths = d["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(sep);
                let other = d["other"].as_str().unwrap_or("?");
                match e.kind() {
                    EventKind::BoardPosted => {
                        let (label, text) = (PostKind::of(&d).label(lang), d["text"].as_str().unwrap_or(&e.text));
                        Some(format!("- [{label}] {text}"))
                    }
                    EventKind::QuestionAsked => {
                        let q = d["question"].as_str().unwrap_or(&e.text);
                        Some(tr!(lang, "- [提问] {q}", "- [Question] {q}"))
                    }
                    EventKind::QuestionAnswered => {
                        let a = d["answer"].as_str().unwrap_or(&e.text);
                        Some(tr!(lang, "- [回答] {a}", "- [Answer] {a}"))
                    }
                    EventKind::TaskConflict if d["byLead"] == true => Some(tr!(
                        lang,
                        "- [冲突] 负责人改了这个子任务的文件：{paths}",
                        "- [Conflict] The lead changed this subtask's files: {paths}"
                    )),
                    EventKind::TaskConflict if d["owned"] == true && theirs(&e) => Some(tr!(
                        lang,
                        "- [越界] {other} 改了这个子任务负责的 {paths}",
                        "- [Out of scope] {other} changed {paths}, which this subtask owns"
                    )),
                    EventKind::TaskConflict if d["owned"] == true => Some(tr!(
                        lang,
                        "- [越界] 改了 {other} 负责的 {paths}",
                        "- [Out of scope] Changed {paths}, which {other} owns"
                    )),
                    EventKind::TaskConflict => Some(tr!(
                        lang,
                        "- [冲突] 和 {other} 同时改了 {paths}",
                        "- [Conflict] Changed {paths} at the same time as {other}"
                    )),
                    EventKind::TaskSplitRequested => {
                        let reason = d["reason"].as_str().unwrap_or(&e.text);
                        let n = d["subtasks"].as_array().map(Vec::len).unwrap_or(0);
                        Some(tr!(
                            lang,
                            "- [请求拆分] {reason}（建议 {n} 个子任务）",
                            "- [Split requested] {reason} ({n} subtasks proposed)"
                        ))
                    }
                    _ => None,
                }
            })
            .collect();
        if !talk.is_empty() {
            let talk = talk.join("\n");
            out.push(tr!(lang, "看板记录：\n{talk}", "On the board:\n{talk}"));
        }
        out.extend(usage.task_lines(task, &Self::usage_names(run), lang));
        Ok(out.join("\n"))
    }

    /// A collaborator's view of the rest of the board: the other tasks, how they relate to its
    /// own (upstream, downstream, the same files) and what they wrote back lately. With `task`,
    /// the full detail of that one.
    // @project-doc docs/interfaces/20261031-board_mcp.md#peers
    pub(super) fn board_peers(&self, run: &Run, me: &str, args: &Value) -> std::result::Result<String, String> {
        if let Some(t) = args["task"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            return self.task_detail(run, t);
        }
        let lang = run.lang;
        let sep = list_sep(lang);
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        let st = run.st.lock().unwrap();
        let deps: Vec<String> =
            st.tasks.iter().find(|t| t.def.id == me).map(|t| t.def.deps.clone()).unwrap_or_default();
        let my_files = st.traces.files(me);
        let lines: Vec<String> = st
            .tasks
            .iter()
            .filter(|t| t.def.id != me)
            .map(|t| {
                let mut tags = vec![];
                if deps.contains(&t.def.id) {
                    tags.push(lang.pick("你的上游", "your upstream").to_string());
                }
                if t.def.deps.iter().any(|d| d == me) {
                    tags.push(lang.pick("依赖你", "depends on you").to_string());
                }
                let theirs = st.traces.files(&t.def.id);
                let common: Vec<String> = theirs.intersection(&my_files).cloned().collect();
                if !t.def.owns.is_empty() && !t.status().terminal() {
                    let owns = t.def.owns.join(sep);
                    tags.push(tr!(lang, "负责 {owns}", "owns {owns}"));
                }
                if !common.is_empty() {
                    let common = common.join(sep);
                    tags.push(tr!(lang, "和你都改了 {common}", "changed {common}, as you did"));
                } else if t.status() == TaskStatus::Running && !theirs.is_empty() {
                    let files = theirs.iter().take(5).cloned().collect::<Vec<_>>().join(sep);
                    tags.push(tr!(lang, "改过 {files}", "changed {files}"));
                }
                if let Some(s) = t.success_seq() {
                    let stale = st.traces.edited_after(&t.def.id, s);
                    if !stale.is_empty() {
                        let files = stale.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(sep);
                        tags.push(tr!(
                            lang,
                            "完成后 {files} 又被改过，结论可能已过期",
                            "{files} changed again after it finished; its conclusion may be out of date"
                        ));
                    }
                }
                let who = run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent);
                let (it, status) = (titled(lang, &t.def.id, &t.def.title), t.status().label_in(lang));
                let tags = if tags.is_empty() { String::new() } else { format!(" · {}", tags.join(" · ")) };
                tr!(lang, "- {it}· {who} · {status}{tags}", "- {it} · {who} · {status}{tags}")
            })
            .collect();
        let posts: Vec<String> = events
            .iter()
            .filter(|e| e.kind() == EventKind::BoardPosted && e.task.as_deref() != Some(me))
            .rev()
            .take(5)
            .map(|e| {
                let task = e.task.as_deref().unwrap_or("?");
                let text = clip(e.data.as_ref().and_then(|d| d["text"].as_str()).unwrap_or(&e.text), INLINE_ITEM);
                tr!(lang, "- {task}：{text}", "- {task}: {text}")
            })
            .collect();
        let lines = if lines.is_empty() { lang.pick("（没有）", "(none)").into() } else { lines.join("\n") };
        let posts = if posts.is_empty() {
            String::new()
        } else {
            let posts = posts.join("\n");
            tr!(lang, "\n\n最近写回看板的内容：\n{posts}", "\n\nRecently written to the board:\n{posts}")
        };
        Ok(tr!(lang, "看板上的其他子任务：\n{lines}{posts}", "Other subtasks on the board:\n{lines}{posts}"))
    }
}

/// Why a task's last result did not pass its acceptance check, one line per gap.
pub(super) fn verification_output(run: &Run, task: &str) -> String {
    let st = run.st.lock().unwrap();
    st.last_verification.get(task).cloned().unwrap_or_default()
}
