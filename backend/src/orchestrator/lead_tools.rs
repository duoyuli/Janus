//! The lead's tools on the shared board: putting subtasks on it, waiting for it, answering,
//! sending work back, correcting or accepting results, cancelling, and handing decisions to the
//! person. Each tool's usage is in its description (`LeadTool::definition`).
// @project-doc docs/interfaces/20261031-board_mcp.md#lead_tools

use super::board::no_task;
use super::*;
use crate::lang::{Lang, tr};

/// A message the lead sends a collaborator, with a prefix so the session and the record both show
/// who wrote it.
pub(super) fn lead_reply(lang: Lang, text: &str) -> String {
    tr!(lang, "负责人：{text}", "The lead: {text}")
}

/// The tasks that took a finished task's result (they depend on it): the ones running now, with
/// their session channel, and the ones already finished on it. Changing that result, by
/// `amend` or by sending the task back with `retry`, leaves them on the old one.
pub(super) struct Takers {
    running: Vec<(String, String, mpsc::UnboundedSender<Steer>)>,
    done: Vec<String>,
}

/// The longest `wait` holds its call. Claude Code and Codex give an MCP tool call 60 seconds and
/// then fail it ("The operation timed out."), so a longer wait never returns: the lead gets an
/// error instead of what the board holds and the live progress of its subtasks.
// @project-doc docs/interfaces/20261031-board_mcp.md#wait
pub(super) const WAIT_MAX_SECS: u64 = 50;

/// How long one `wait` call may hold: what it asks for, at most `WAIT_MAX_SECS`.
pub(super) fn wait_limit(args: &Value) -> Duration {
    Duration::from_secs(args["timeout_sec"].as_u64().unwrap_or(WAIT_MAX_SECS).clamp(1, WAIT_MAX_SECS))
}

impl Takers {
    fn of(st: &RunState, task: &str) -> Takers {
        let downstream: Vec<&Task> = st.tasks.iter().filter(|x| x.def.deps.iter().any(|d| d == task)).collect();
        Takers {
            running: downstream
                .iter()
                .filter_map(|x| {
                    st.live.get(&SessionKey::task(&x.def.id)).map(|tx| (x.def.id.clone(), x.agent.clone(), tx.clone()))
                })
                .collect(),
            done: downstream.iter().filter(|x| x.status() == TaskStatus::Succeeded).map(|x| x.def.id.clone()).collect(),
        }
    }

    fn ids(running: &[(String, String, mpsc::UnboundedSender<Steer>)], lang: Lang) -> String {
        running.iter().map(|r| r.0.as_str()).collect::<Vec<_>>().join(list_sep(lang))
    }
}

impl App {
    pub(super) fn board_team(&self, run: &Run) -> String {
        let lang = run.lang;
        let st = run.st.lock().unwrap();
        let tasks: Vec<String> = st
            .tasks
            .iter()
            .map(|t| {
                let (it, agent, status) = (titled(lang, &t.def.id, &t.def.title), &t.agent, t.status().label_in(lang));
                tr!(lang, "- {it}· {agent} · {status}", "- {it} · {agent} · {status}")
            })
            .collect();
        drop(st);
        let team = team_lines(run);
        let tasks = if tasks.is_empty() { lang.pick("（还没有）", "(none yet)").into() } else { tasks.join("\n") };
        tr!(
            lang,
            "协作者：\n{team}\n\n看板上的子任务：\n{tasks}",
            "Collaborators:\n{team}\n\nSubtasks on the board:\n{tasks}"
        )
    }

    /// Validate an orchestration and, if it holds, put its tasks on the board for the scheduler.
    pub(super) fn board_orchestrate(
        self: &Arc<Self>,
        run: &Arc<Run>,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let _one_at_a_time = run.orchestrating.lock().unwrap();
        let defs = {
            let st = run.st.lock().unwrap();
            let board: Vec<(TaskDef, TaskStatus)> = st.tasks.iter().map(|t| (t.def.clone(), t.status())).collect();
            validate_orchestration(run, &board, args)
        };
        let defs = match defs {
            Ok(defs) => defs,
            Err(errors) => {
                self.emit(
                    run,
                    ev(
                        EventKind::OrchestrationRejected,
                        "lead",
                        format!("编排没有通过校验：{}", clip(&errors.join("；"), 120)),
                    )
                    .data(json!({ "errors": errors, "submitted": args })),
                );
                let errors = errors.join("\n- ");
                return Err(tr!(
                    run.lang,
                    "编排没有通过校验，没有子任务开始：\n- {errors}",
                    "The orchestration failed its check and no subtask started:\n- {errors}"
                ));
            }
        };
        let tasks: Vec<Value> = defs
            .iter()
            .map(|d| {
                let mut obj = json!({
                    "id": d.id, "title": d.title, "desc": d.desc, "deps": d.deps,
                    "accept": d.accept, "maxAttempts": d.max_attempts, "agent": d.assignee,
                    "owns": d.owns, "verifies": d.verifies,
                });
                if let Some(c) = &d.continues {
                    obj["continues"] = json!(c);
                }
                obj
            })
            .collect();
        // The record comes first: tasks go on the board only once their orchestration is in the
        // log, which is what a restart rebuilds the board from (FL-08).
        // @project-doc docs/interfaces/20261031-board_mcp.md#orchestrate
        let planned = run.last_seq.load(Ordering::SeqCst);
        let n = run.st.lock().unwrap().tasks.len() + defs.len();
        let kept = self.emit_kept(
            run,
            ev(
                EventKind::OrchestrationAccepted,
                "lead",
                format!("编排通过校验：{} 个子任务写入看板，开始执行", defs.len()),
            )
            .data(json!({ "tasks": tasks, "total": n, "plannedSeq": planned })),
        );
        if kept.is_none() {
            return Err(tr!(
                run.lang,
                "编排没有写进事件日志（Janus 的数据库写入失败），所以没有生效，没有子任务开始。稍后重试同一份编排。",
                "The orchestration was not written to the event log (Janus's database write failed), so it did not take effect and no subtask started. Try the same orchestration again shortly."
            ));
        }
        {
            let mut st = run.st.lock().unwrap();
            for d in &defs {
                st.tasks.push(Task::new(d.clone(), d.assignee.clone(), planned));
            }
        }
        // After delivery the scheduler has returned; new work written in a conversation starts it again.
        self.ensure_driver(run);
        run.wake.notify_one();
        let started =
            defs.iter().map(|d| format!("{}→{}", d.id, d.assignee)).collect::<Vec<_>>().join(list_sep(run.lang));
        Ok(tr!(run.lang, "编排已写入看板：{started}。", "The orchestration is on the board: {started}."))
    }

    /// Board entries for the lead since it last looked, waiting up to the given time for some.
    pub(super) async fn board_wait(&self, run: &Run, args: &Value) -> String {
        struct WaitGuard<'a>(&'a Run);
        impl Drop for WaitGuard<'_> {
            fn drop(&mut self) {
                self.0.st.lock().unwrap().lead.waiting = false;
            }
        }
        run.st.lock().unwrap().lead.waiting = true;
        let _guard = WaitGuard(run);
        self.board_wait_inner(run, args).await
    }

    pub(super) async fn board_wait_inner(&self, run: &Run, args: &Value) -> String {
        let lang = run.lang;
        let limit = wait_limit(args);
        let until = tokio::time::Instant::now() + limit;
        loop {
            let notified = run.board_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let cursor = run.st.lock().unwrap().lead.cursor;
            let events = self.db.events(&run.id, cursor).unwrap_or_default();
            let last = events.last().map(|e| e.seq).unwrap_or(cursor);
            let mut entries = vec![];
            let mut urgent = false;
            for e in &events {
                if let Some(entry) = board_entry(run, e) {
                    entries.push(entry);
                    urgent |= ends_board_wait(e);
                }
            }
            let (open, cancelled) = {
                let st = run.st.lock().unwrap();
                (st.tasks.iter().any(|t| !t.status().terminal()), st.cancelled())
            };
            let timed_out = tokio::time::Instant::now() >= until || cancelled;
            // Entries that only inform wait for one that needs the lead, or for the timeout.
            if !entries.is_empty() && (urgent || !open || timed_out) {
                run.st.lock().unwrap().lead.cursor = last;
                let entries = entries.join("\n");
                let mut text = tr!(lang, "看板上的新消息：\n{entries}", "New on the board:\n{entries}");
                let pulse = if open && !urgent { running_pulse(run) } else { String::new() };
                if !pulse.is_empty() {
                    text.push_str(&tr!(lang, "\n子任务实时进展：\n{pulse}", "\nLive progress of subtasks:\n{pulse}"));
                }
                return text;
            }
            if entries.is_empty() {
                run.st.lock().unwrap().lead.cursor = last;
            }
            if !open {
                return tr!(
                    lang,
                    "看板上没有新消息，也没有在执行或等待的子任务。",
                    "Nothing new on the board, and no subtask running or waiting."
                );
            }
            if timed_out {
                let (secs, pulse) = (limit.as_secs(), running_pulse(run));
                return if pulse.is_empty() {
                    tr!(
                        lang,
                        "{secs} 秒内看板上没有新消息，子任务仍在进行。",
                        "Nothing new on the board in {secs} s; subtasks are still going."
                    )
                } else {
                    tr!(
                        lang,
                        "{secs} 秒内看板上没有新消息。子任务实时进展：\n{pulse}",
                        "Nothing new on the board in {secs} s. Live progress of subtasks:\n{pulse}"
                    )
                };
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// The lead answers a collaborator's question, or adds instructions to its task.
    pub(super) fn board_reply(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(tr!(lang, "text 不能为空", "text cannot be empty"));
        }
        let (agent, question, live) = {
            let st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(no_task(lang, task));
            };
            let session = SessionKey::task(task);
            let question = st
                .pending
                .iter()
                .find(|(_, p)| p.ask == Ask::Question && p.owner.session == session)
                .map(|(k, _)| k.clone());
            (t.agent.clone(), question, st.live.get(&session).cloned())
        };
        if let Some(key) = question {
            self.emit(
                run,
                ev(EventKind::QuestionAnswered, "lead", format!("负责人回答：{}", clip(text, 40)))
                    .task(task)
                    .to(&agent)
                    .data(json!({ "key": key, "answer": text, "via": "lead" })),
            );
            self.resolve(run, &key, Decision::Answer { answer: text.to_string(), note: String::new() })
                .map_err(|e| format!("{e:#}"))?;
            return Ok(tr!(
                lang,
                "已回答 {task} 的提问，{agent} 会接着做。",
                "Answered {task}'s question; {agent} carries on."
            ));
        }
        let status = run
            .st
            .lock()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.def.id == task)
            .map(|t| t.status())
            .unwrap_or(TaskStatus::Pending);
        if live.is_none() && status.terminal() {
            // A reply to a finished task would otherwise wait for whatever this agent does next.
            let status = status.label_in(lang);
            return Err(tr!(
                lang,
                "{task} 已经结束（{status}），消息不会送达。要它接着做，用 janus.retry。",
                "{task} has ended ({status}), so the message would not arrive. To have it go on, use janus.retry."
            ));
        }
        let msg = self.post_message(run, "lead", &agent, task, &lead_reply(lang, text), &[]);
        match live {
            Some(tx) => {
                tx.send(Steer::Redirect(msg))
                    .map_err(|_| tr!(lang, "{task} 的会话刚刚结束", "{task}'s session has just ended"))?;
                Ok(tr!(lang, "已发给正在执行 {task} 的 {agent}。", "Sent to {agent}, working on {task}."))
            }
            None => {
                self.route_message(run, &agent, task, 0, &msg.id, Delivery::Next);
                run.st.lock().unwrap().notes.entry(agent.clone()).or_default().push(Note::new(msg, task));
                Ok(tr!(
                    lang,
                    "{task} 现在没有在执行，{agent} 下一次开始工作时收到。",
                    "{task} is not running now; {agent} gets it the next time it starts work."
                ))
            }
        }
    }

    /// The lead runs an ended task again: a failed or cancelled one, or a finished one it sends
    /// back for rework. It names the session the next attempt starts in, and may hand over the
    /// findings of a verification (`from`) and a note for this round. The verifications of the
    /// task, and `from`, check its new result again once it is done. Tasks blocked on it start
    /// once it succeeds.
    // @project-doc docs/domains/20261031-run_lifecycle.md#retry
    pub(super) fn board_retry(self: &Arc<Self>, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = |k: &str| args[k].as_str().map(str::trim).filter(|s| !s.is_empty());
        let instructions = text("instructions");
        let note = text("note");
        let from = text("from");
        let accept: Option<Vec<String>> = args["accept"].as_array().map(|a| {
            a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect()
        });
        if accept.as_ref().is_some_and(|a| a.is_empty()) {
            return Err(tr!(
                lang,
                "accept 不能是空数组；不传则沿用原来的",
                "accept cannot be an empty array; leave it out to keep the current criteria"
            ));
        }
        let Some(session) = text("session").and_then(SessionChoice::parse) else {
            return Err(tr!(lang, "session 必填：continue 或 new", "session is required: continue or new"));
        };
        // Its own session too full to go back into: refused here, so the lead knows the next attempt
        // starts without that context and writes what it needs into the note.
        let too_full = match session {
            SessionChoice::Continue => {
                self.last_task_session(&run.id, task).and_then(|sid| self.session_too_full(run, &sid))
            }
            SessionChoice::New => None,
        };
        let reverify: Vec<String> = {
            let mut st = run.st.lock().unwrap();
            let Some(t) = st.tasks.iter().find(|t| t.def.id == task) else {
                return Err(no_task(lang, task));
            };
            if !t.status().terminal() {
                let status = t.status().label_in(lang);
                return Err(tr!(
                    lang,
                    "{task} 现在是「{status}」，还没有结束；要补充指示请用 janus.reply",
                    "{task} is {status} and has not ended; to add instructions, use janus.reply"
                ));
            }
            if let Some(ratio) = too_full {
                let (used, limit) =
                    (format!("{:.0}%", ratio * 100.0), format!("{:.0}%", CONTINUE_CONTEXT_LIMIT * 100.0));
                return Err(tr!(
                    lang,
                    "{task} 原来的会话上下文已用了 {used}（超过 {limit}），不能续接，改用 session=\"new\"。",
                    "{task}'s own session has used {used} of its context (over {limit}) and cannot be continued; use session=\"new\"."
                ));
            }
            if let Some(v) = from {
                let Some(vt) = st.tasks.iter().find(|x| x.def.id == v) else {
                    return Err(tr!(
                        lang,
                        "from 指向的子任务 {v} 不存在",
                        "from names subtask {v}, which does not exist"
                    ));
                };
                if vt.verdict().is_none() {
                    return Err(tr!(
                        lang,
                        "from 要指向一个已经给出判定的独立验证子任务，{v} 不是",
                        "from must name an independent verification that has given its verdict; {v} is not one"
                    ));
                }
                if v == task {
                    return Err(tr!(lang, "from 不能是 {task} 自己", "from cannot be {task} itself"));
                }
                // `from` checks the reworked result after it, so `task` must not wait on `from`.
                let deps: HashMap<&str, &[String]> =
                    st.tasks.iter().map(|x| (x.def.id.as_str(), x.def.deps.as_slice())).collect();
                if crate::teamwork::upstream_of(task, &deps).contains(v) {
                    return Err(tr!(
                        lang,
                        "{task} 依赖 {v}，{v} 不能在 {task} 返工之后再复审它；去掉 from，或返工 {v} 验证的子任务",
                        "{task} depends on {v}, so {v} cannot review it again after the rework; drop from, or rework the subtask {v} verifies"
                    ));
                }
            }
            // Verifications of the task's old result, and the one whose findings it takes, check
            // the new result again.
            let reverify: Vec<String> = st
                .tasks
                .iter()
                .filter(|v| v.status() == TaskStatus::Succeeded)
                .filter(|v| v.def.verifies.as_deref() == Some(task) || Some(v.def.id.as_str()) == from)
                .map(|v| v.def.id.clone())
                .collect();
            let t = Run::task_mut(&mut st, task).expect("found above");
            // A new run of the task brings a new conclusion; earlier corrections were about the old one.
            t.reopen(Reopen::Retry {
                session,
                from: from.map(str::to_owned),
                note: note.map(str::to_owned),
                revised: instructions.is_some() || accept.is_some(),
            });
            if let Some(i) = instructions {
                t.def.desc = i.to_string();
            }
            if let Some(a) = &accept {
                t.def.accept = a.clone();
            }
            for id in &reverify {
                Run::task_mut(&mut st, id).expect("listed above").reverify(task);
            }
            reverify
        };
        // Tasks the lead's `cancel` dropped only because of this one come back with it (and theirs
        // with them); a task the lead cancelled by name stays cancelled.
        let mut revived: Vec<(String, String)> = vec![];
        {
            let mut st = run.st.lock().unwrap();
            let mut queue = vec![task.to_string()];
            while let Some(up) = queue.pop() {
                for t in st.tasks.iter_mut().filter(|t| t.skipped_because() == Some(up.as_str())) {
                    t.set_phase(TaskPhase::Pending);
                    t.retry_base = t.attempts;
                    revived.push((t.def.id.clone(), up.clone()));
                    queue.push(t.def.id.clone());
                }
            }
        }
        let mut data = json!({ "reason": ReopenReason::LeadRetry.as_str(), "session": session.as_str() });
        if let Some(i) = instructions {
            data["instructions"] = json!(i);
        }
        if let Some(a) = &accept {
            data["accept"] = json!(a);
        }
        if let Some(v) = from {
            data["from"] = json!(v);
        }
        if let Some(n) = note {
            data["note"] = json!(n);
        }
        let how = match session {
            SessionChoice::Continue => "在原会话中",
            SessionChoice::New => "在新会话中",
        };
        let title = match from {
            Some(v) => format!("负责人要求 {task} 按 {v} 的意见{how}返工"),
            None => format!("负责人要求 {task} {how}重新执行"),
        };
        self.emit(run, ev(EventKind::TaskReopened, "lead", title).task(task).data(data));
        for id in &reverify {
            self.emit(
                run,
                ev(EventKind::TaskReopened, "janus", format!("{task} 返工，{id} 在它完成后重新验证"))
                    .task(id)
                    .data(json!({ "reason": ReopenReason::TargetReworked.as_str(), "target": task })),
            );
        }
        for (id, up) in &revived {
            self.emit(
                run,
                ev(EventKind::TaskReopened, "janus", format!("{up} 重新执行，因它取消的 {id} 恢复等待"))
                    .task(id)
                    .data(json!({ "reason": ReopenReason::UpstreamRetry.as_str(), "upstream": up })),
            );
        }
        self.ensure_driver(run);
        run.wake.notify_one();
        let sep = list_sep(lang);
        let never_opened = session == SessionChoice::Continue && self.last_task_session(&run.id, task).is_none();
        let mut reply = match session {
            SessionChoice::Continue if never_opened => tr!(
                lang,
                "{task} 之前没有开过会话，会在新会话中重新执行。",
                "{task} never opened a session, so it runs again in a new one."
            ),
            SessionChoice::Continue => {
                tr!(lang, "{task} 会在原会话中重新执行。", "{task} runs again in its own session.")
            }
            SessionChoice::New => tr!(lang, "{task} 会在新会话中重新执行。", "{task} runs again in a new session."),
        };
        if !reverify.is_empty() {
            let ids = reverify.join(sep);
            reply.push_str(&tr!(lang, "它完成后 {ids} 重新验证。", " Once it is done, {ids} verify it again."));
        }
        if !revived.is_empty() {
            let ids = revived.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(sep);
            reply.push_str(&tr!(lang, "因它被取消的 {ids} 也恢复了。", " {ids}, cancelled because of it, are back."));
        }
        // What already took the old result: a running one is told the work under it is changing
        // (it may share files with the rework), a finished one is the lead's to redo or keep.
        let Takers { running, done } = Takers::of(&run.st.lock().unwrap(), task);
        self.tell_takers(
            run,
            &running,
            &tr!(
                lang,
                "上游子任务 {task} 正在返工，它的结果和改过的文件可能会变，用到时先读最新内容。必须等它返工完才能继续时，用 janus.board_post 写一条 blocker。",
                "The upstream subtask {task} is being reworked; its result and the files it changed may change, so read the latest content where you use them. If you cannot go on until the rework is done, post a blocker with janus.board_post."
            ),
        );
        if !running.is_empty() {
            let ids = Takers::ids(&running, lang);
            reply.push_str(&tr!(
                lang,
                "{ids} 正在基于 {task} 的旧结果执行，已告诉它们上游在返工，它们可能和返工改同一批文件。",
                " {ids} are running on {task}'s old result and were told of the rework; they may change the same files as the rework."
            ));
        }
        if !done.is_empty() {
            let ids = done.join(sep);
            reply.push_str(&tr!(
                lang,
                "{ids} 已经基于 {task} 的旧结果完成。",
                " {ids} already finished on {task}'s old result."
            ));
        }
        Ok(reply)
    }

    /// `amend` on a task that failed its acceptance check: the lead accepts the result it handed
    /// in. The task counts as done; the failure stays on record as a risk, with `reason`.
    fn accept_result(&self, run: &Arc<Run>, task: &str, reason: &str) -> std::result::Result<String, String> {
        let lang = run.lang;
        let events = self.db.events(&run.id, 0).map_err(|e| format!("{e:#}"))?;
        let mine = |kind: EventKind| events.iter().rev().find(|e| e.kind() == kind && e.task.as_deref() == Some(task));
        let failed =
            mine(EventKind::TaskFailed).filter(|e| e.data.as_ref().is_some_and(|d| d["reason"] == "verification"));
        let submitted = mine(EventKind::ResultSubmitted).filter(|r| failed.is_some_and(|f| r.seq < f.seq));
        let (Some(failed), Some(submitted)) = (failed, submitted) else {
            return Err(tr!(
                lang,
                "{task} 不是没通过验收的子任务，没有可以接受的结果",
                "{task} did not fail its acceptance check, so there is no result to accept"
            ));
        };
        let d = submitted.data.clone().unwrap_or_default();
        // The accepted result keeps the checks and handoff, so its evidence methods are still shown.
        let mut result = result_from_json(&d);
        let error = failed.data.as_ref().and_then(|d| d["error"].as_str()).unwrap_or("验证失败").to_string();
        result.risks.push(format!("{error}；负责人接受了结果：{reason}"));
        {
            // Checked and settled in one step, so a retry or cancel in between is not overwritten.
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task).filter(|t| t.status() == TaskStatus::Failed) else {
                return Err(tr!(lang, "{task} 已经不在失败状态", "{task} is no longer failed"));
            };
            t.set_phase(TaskPhase::Succeeded);
            t.result = Some(result.clone());
        }
        let mut event_data = json!({
            "summary": result.summary, "evidence": result.evidence, "risks": result.risks,
            "checks": result.checks, "acceptedBy": "lead", "reason": reason
        });
        if let Some(h) = &result.handoff {
            event_data["handoff"] = json!(h);
        }
        let seq = self.emit(
            run,
            ev(EventKind::TaskSucceeded, "lead", format!("负责人接受 {task} 的结果：{}", clip(reason, 60)))
                .task(task)
                .attempt(submitted.attempt.unwrap_or(0))
                .data(event_data),
        );
        if let Some(t) = Run::task_mut(&mut run.st.lock().unwrap(), task) {
            t.last_success_seq = Some(seq);
        }
        self.recheck_verifications(run, task, submitted.attempt.unwrap_or(0), seq);
        run.wake.notify_one();
        Ok(tr!(lang, "已接受 {task} 的结果。", "Accepted {task}'s result."))
    }

    /// `task` has a new result (attempt `n`, recorded at `seq`). A verification's conclusion is
    /// about one result of its target, so: verifications of `task` that concluded before this result
    /// check it again, and when `task` is itself a verification whose attempt began before its
    /// target's current result, or whose target is being reworked, it checks again once the target
    /// is done. Each is reopened as the lead's `retry` reopens one (`target.reworked`), so a restart
    /// rebuilds it, and its conclusion on the earlier result stays in the log.
    // @project-doc docs/domains/20261031-goal_mode.md#team_structure
    pub(super) fn recheck_verifications(&self, run: &Arc<Run>, task: &str, n: i64, seq: i64) {
        let started = self
            .db
            .events(&run.id, 0)
            .unwrap_or_default()
            .iter()
            .rfind(|e| e.kind() == EventKind::AttemptStarted && e.task.as_deref() == Some(task) && e.attempt == Some(n))
            .map_or(0, |e| e.seq);
        let reopened: Vec<(String, String)> = {
            let mut st = run.st.lock().unwrap();
            let mut stale: Vec<(String, String)> = st
                .tasks
                .iter()
                .filter(|v| v.def.verifies.as_deref() == Some(task) && v.def.id != task)
                .filter(|v| v.status() == TaskStatus::Succeeded && v.last_success_seq.is_some_and(|s| s < seq))
                .map(|v| (v.def.id.clone(), task.to_string()))
                .collect();
            let own = st.tasks.iter().find(|t| t.def.id == task).and_then(|t| t.def.verifies.clone());
            if let Some(target) = own
                && let Some(tt) = st.tasks.iter().find(|t| t.def.id == target)
                && (tt.status() != TaskStatus::Succeeded || tt.last_success_seq.is_some_and(|s| s > started))
            {
                stale.push((task.to_string(), target));
            }
            for (id, target) in &stale {
                Run::task_mut(&mut st, id).expect("listed above").reverify(target);
            }
            stale
        };
        for (id, target) in &reopened {
            self.emit(
                run,
                ev(
                    EventKind::TaskReopened,
                    "janus",
                    format!("{id} 的结论针对 {target} 之前的结果，在 {target} 的新结果上重新验证"),
                )
                .task(id)
                .data(json!({ "reason": ReopenReason::TargetReworked.as_str(), "target": target })),
            );
        }
        if !reopened.is_empty() {
            run.wake.notify_one();
        }
    }

    /// Tell the running tasks that took a result it changed, as a notice from Janus into their
    /// current turn.
    fn tell_takers(&self, run: &Arc<Run>, running: &[(String, String, mpsc::UnboundedSender<Steer>)], text: &str) {
        for (id, agent, tx) in running {
            let msg = self.post_message(run, "janus", agent, id, &format!("[Janus] {text}"), &[]);
            let _ = tx.send(Steer::Redirect(msg));
        }
    }

    /// The lead corrects the conclusion of a finished task. Tasks that build on it hear about it:
    /// running ones right away, ones that have not started with their handoff. Finished ones are
    /// named in the reply so the lead can decide whether they need redoing. On a task that failed
    /// its acceptance check, the lead accepts its result instead (`accept_result`). A verification
    /// is never amended: its verdict changes only when it verifies again.
    pub(super) fn board_amend(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let task = args["task"].as_str().unwrap_or("").trim();
        let text = args["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(tr!(lang, "text 不能为空", "text cannot be empty"));
        }
        let (status, refused) = {
            let st = run.st.lock().unwrap();
            let t = st.tasks.iter().find(|t| t.def.id == task);
            (t.map(|t| t.status()), t.and_then(|t| crate::teamwork::amend_refused(&t.def, lang)))
        };
        if let Some(why) = refused {
            return Err(why);
        }
        if status == Some(TaskStatus::Failed) {
            return self.accept_result(run, task, text);
        }
        let takers = {
            let mut st = run.st.lock().unwrap();
            let Some(t) = Run::task_mut(&mut st, task) else {
                return Err(no_task(lang, task));
            };
            if t.status() != TaskStatus::Succeeded {
                let status = t.status().label_in(lang);
                return Err(tr!(
                    lang,
                    "{task} 现在是「{status}」：只能修订已完成子任务的结论，或接受没通过验收核对的结果；还在执行的用 janus.reply",
                    "{task} is {status}: only a finished subtask's conclusion can be revised, or the result of one that failed its acceptance check accepted. For one still running, use janus.reply"
                ));
            }
            t.amendments.push(text.to_string());
            Takers::of(&st, task)
        };
        let Takers { running, done } = takers;
        self.emit(
            run,
            ev(EventKind::ResultAmended, "lead", format!("负责人修订 {task} 的结论：{}", clip(text, 60)))
                .task(task)
                .data(json!({ "text": text })),
        );
        self.tell_takers(
            run,
            &running,
            &tr!(
                lang,
                "负责人修订了上游子任务 {task} 的结论，以修订为准：{text}",
                "The lead revised the conclusion of the upstream subtask {task}, and the revision stands: {text}"
            ),
        );
        let mut out = tr!(lang, "已修订 {task} 的结论。", "Revised {task}'s conclusion.");
        if !running.is_empty() {
            let ids = Takers::ids(&running, lang);
            out.push_str(&tr!(lang, "正在执行的 {ids} 已收到。", " The running {ids} got it."));
        }
        if !done.is_empty() {
            let ids = done.join(list_sep(lang));
            out.push_str(&tr!(lang, "{ids} 已经基于旧结论完成。", " {ids} already finished on the old conclusion."));
        }
        Ok(out)
    }

    /// The lead drops tasks it no longer needs; tasks that depend on them are dropped too.
    /// A running one is stopped.
    pub(super) fn board_cancel(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let reason = args["reason"].as_str().unwrap_or("").trim().to_string();
        let wanted: Vec<String> = args["tasks"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect())
            .unwrap_or_default();
        if wanted.is_empty() {
            return Err(tr!(lang, "tasks 不能为空", "tasks cannot be empty"));
        }
        let (dropped, conns) = {
            let mut st = run.st.lock().unwrap();
            if let Some(missing) = wanted.iter().find(|id| !st.tasks.iter().any(|t| &t.def.id == *id)) {
                return Err(no_task(lang, missing));
            }
            let mut dropped: Vec<(String, Option<String>)> = vec![];
            let mut queue: Vec<(String, Option<String>)> = wanted.iter().map(|id| (id.clone(), None)).collect();
            while let Some((id, because)) = queue.pop() {
                let t = Run::task_mut(&mut st, &id).unwrap();
                if matches!(t.status(), TaskStatus::Succeeded | TaskStatus::SkippedByDecision)
                    || dropped.iter().any(|(d, _)| d == &id)
                {
                    continue;
                }
                t.set_phase(TaskPhase::SkippedByDecision { because: because.clone() });
                dropped.push((id.clone(), because));
                for d in st.tasks.iter().filter(|x| x.def.deps.contains(&id)) {
                    queue.push((d.def.id.clone(), Some(id.clone())));
                }
            }
            let conns: Vec<Arc<AcpConn>> =
                dropped.iter().filter_map(|(id, _)| st.conns.remove(&SessionKey::task(id))).collect();
            (dropped, conns)
        };
        for conn in conns {
            tokio::spawn(async move { conn.kill().await });
        }
        for (id, because) in &dropped {
            let text = match because {
                Some(dep) => format!("{id} 依赖的 {dep} 被负责人取消，一并取消"),
                None => format!("负责人取消 {id}：{}", if reason.is_empty() { "不再需要" } else { &reason }),
            };
            self.emit(
                run,
                ev(EventKind::TaskSkippedByDecision, "lead", text)
                    .task(id)
                    .data(json!({ "reason": reason, "dependency": because })),
            );
        }
        run.wake.notify_one();
        if dropped.is_empty() {
            let st = run.st.lock().unwrap();
            if let Some(t) = st.tasks.iter().find(|t| {
                wanted.contains(&t.def.id)
                    && t.status() == TaskStatus::Succeeded
                    && matches!(t.verdict(), Some(Verdict::Rejected | Verdict::Inconclusive))
            }) {
                let id = &t.def.id;
                return Err(tr!(
                    lang,
                    "{id} 的独立验证已完成，原判定保留，取消不能撤销它。",
                    "{id}'s independent verification is complete and its verdict stands; cancelling cannot undo it."
                ));
            }
            return Ok(tr!(
                lang,
                "这些子任务都已经结束，没有需要取消的。",
                "These subtasks have all ended; nothing to cancel."
            ));
        }
        let ids = dropped.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(list_sep(lang));
        Ok(tr!(lang, "已取消：{ids}。", "Cancelled: {ids}."))
    }

    /// The lead hands the person a decision only they can make. It does not wait here: the answer
    /// comes back through `wait`, and the lead keeps working on what does not depend on it.
    pub(super) fn board_ask_user(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<String, String> {
        self.escalate(run, args)?;
        Ok(tr!(run.lang, "已交给用户。", "Handed to the user."))
    }

    /// The lead's (or a Loop's) question for the person (`ask_user`): checked, put on record and
    /// left pending for the person's answer, which nobody awaits here.
    pub(super) fn escalate(&self, run: &Arc<Run>, args: &Value) -> std::result::Result<(), String> {
        let question = args["question"].as_str().unwrap_or("").trim();
        let why = args["why"].as_str().unwrap_or("").trim();
        if question.is_empty() || why.is_empty() {
            return Err(tr!(run.lang, "question 和 why 都不能为空", "question and why cannot be empty"));
        }
        let options: Vec<String> = args["options"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let recommend = args["recommend"].as_str().unwrap_or("");
        let key = crate::util::new_id("q-lead-");
        // Nobody awaits the answer here: it reaches the lead through `wait`.
        self.add_pending(run, &key, Owner { session: SessionKey::Lead, attempt: 0 }, Ask::Question)
            .map_err(|e| format!("{e:#}"))?;
        self.emit(run, ev(EventKind::QuestionEscalated, "lead", format!("负责人请你决定：{}", clip(question, 60))).to("user").data(json!({
            "key": key, "question": question, "options": options, "recommend": recommend, "reason": why, "from": "lead"
        })));
        Ok(())
    }
}

/// One-line live heartbeat per running task (plus pending count) when `janus.wait` times out.
fn running_pulse(run: &Run) -> String {
    let now = now_ms();
    let watches: HashMap<SessionKey, (i64, AgentPhase, String)> = run
        .watch
        .lock()
        .unwrap()
        .iter()
        .map(|(k, w)| (k.clone(), (w.started_ms, w.phase, w.last_tool.clone())))
        .collect();
    let lang = run.lang;
    let st = run.st.lock().unwrap();
    let mut lines = vec![];
    for t in st.tasks.iter().filter(|t| t.status() == TaskStatus::Running) {
        let who = run.member(&t.agent).map(|m| m.name.as_str()).unwrap_or(&t.agent);
        let edited = st.traces.files(&t.def.id).len();
        let detail = match watches.get(&SessionKey::task(&t.def.id)) {
            Some((started, phase, tool)) => {
                let elapsed = span((now - *started).max(0), lang);
                let tool_part = if tool.is_empty() {
                    String::new()
                } else {
                    let tool = clip(tool, 40);
                    tr!(lang, "（{tool}）", " ({tool})")
                };
                let phase = phase.label_in(lang);
                tr!(lang, "{phase}{tool_part}，已运行 {elapsed}", "{phase}{tool_part}, running for {elapsed}")
            }
            None => lang.pick("正在启动会话", "starting its session").to_string(),
        };
        let files_part = if edited > 0 {
            tr!(lang, "，已改 {edited} 个文件", ", {edited} files changed")
        } else {
            String::new()
        };
        let it = titled(lang, &t.def.id, &t.def.title);
        lines.push(tr!(lang, "- {it}· {who}：{detail}{files_part}", "- {it} · {who}: {detail}{files_part}"));
    }
    let pending = st.tasks.iter().filter(|t| t.status() == TaskStatus::Pending).count();
    if pending > 0 {
        lines.push(tr!(lang, "- 另有 {pending} 个子任务在等待开始", "- {pending} more subtasks waiting to start"));
    }
    lines.join("\n")
}
