//! What the person reads about a run's facts, in their language: each open item's title and the
//! line below it, each decision's title, what passed between tasks, what a delivery asks them to
//! look at, and the answers one click gives. Written here once for every client
//! (docs/decisions/20261126-backend_owns_derived_state.md).
// @project-doc docs/architecture/20261031-frontend.md#event_derivation

use super::board::{BoardFacts, DecisionKind, HandoffKind, OpenFact, OpenKind, QuickAction};
use super::review::{Signal, SignalKind};
use super::{RunState, text};
use crate::lang::{Lang, tr};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The language to write in, and the run's members by name.
pub struct Words {
    pub lang: Lang,
    names: HashMap<String, String>,
}

impl Words {
    /// For a run whose snapshot is `config` (its `team`).
    pub fn of(lang: Lang, config: &Value) -> Words {
        let names = super::list(&config["team"]).iter().map(|m| (text(&m["id"]), text(&m["name"]))).collect();
        Words { lang, names }
    }

    /// Who an id is: the person, Janus, or a member by its name.
    pub fn name(&self, id: &str) -> String {
        match id {
            "user" => self.lang.pick("你", "You").into(),
            "janus" => "Janus".into(),
            _ => self.names.get(id).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| id.into()),
        }
    }

    /// The list separator: 「、」 or ", ".
    pub fn and(&self) -> &'static str {
        self.lang.pick("、", ", ")
    }

    /// The clause separator: 「；」 or " · ".
    fn semi(&self) -> &'static str {
        self.lang.pick("；", " · ")
    }

    /// A task's status in a word or two, as its pill reads.
    pub fn task_status(&self, s: super::TaskShown) -> String {
        use super::TaskShown::*;
        let l = self.lang;
        match s {
            Pending => l.pick("等上游", "Waiting on upstream"),
            Ready => l.pick("待开工", "Ready"),
            Assigned => l.pick("已派活", "Assigned"),
            Running => l.pick("干活中", "Working"),
            WaitingInput => l.pick("等你回答", "Waiting for your answer"),
            WaitingApproval => l.pick("等你审批", "Waiting for approval"),
            Blocked => l.pick("上游出错", "Upstream failed"),
            SkippedByDecision => l.pick("负责人取消", "Cancelled by lead"),
            Verifying => l.pick("核对中", "Verifying"),
            VerificationFailed => l.pick("验收没通过", "Acceptance failed"),
            Succeeded => l.pick("已完成", "Completed"),
            Failed => l.pick("失败", "Failed"),
            Cancelled => l.pick("已取消", "Cancelled"),
        }
        .into()
    }

    /// Above the run card of a run that stopped short: a restart interrupted it (`interrupted`),
    /// or it failed; either way it goes on from where it stopped.
    pub fn resume_note(&self, interrupted: bool) -> String {
        let l = self.lang;
        let what = if interrupted {
            l.pick(
                "Janus 服务重启，任务中断了。做完的保留，没做完的接着做。",
                "The Janus service restarted and interrupted this run. Finished subtasks stay; the rest pick up where they stopped.",
            )
        } else {
            l.pick("可以从失败处继续。", "You can resume from the failure.")
        };
        tr!(
            l,
            "{what} 也可以点右侧的 Agent，补充信息后再继续。",
            "{what} Or open an Agent on the right, add details, then continue."
        )
    }

    /// The board's facts in words.
    pub fn board(&self, f: &mut BoardFacts, st: &RunState) {
        for o in &mut f.open {
            self.open_fact(o, st);
        }
        for d in &mut f.decisions {
            self.decision(d);
        }
        for h in &mut f.handoffs {
            let l = self.lang;
            h.text = match h.kind {
                HandoffKind::Split => {
                    tr!(
                        l,
                        "请求拆分（建议 {} 个子任务）：{}",
                        "Split requested (proposed subtasks: {}): {}",
                        h.count,
                        h.summary
                    )
                }
                HandoffKind::Progress => tr!(l, "进展：{}", "Progress: {}", h.summary),
                HandoffKind::Note => tr!(l, "备注：{}", "Note: {}", h.summary),
                HandoffKind::Handoff => h.summary.clone(),
            };
        }
    }

    /// An open item's kind in a word or two.
    pub fn label(&self, kind: OpenKind) -> String {
        let l = self.lang;
        match kind {
            OpenKind::Approval => l.pick("等你审批", "Waiting for approval"),
            OpenKind::Input => l.pick("等你填写", "Form Required"),
            OpenKind::Question => l.pick("等你拍板", "Your Call"),
            OpenKind::Session => l.pick("等你回话", "Waiting for You"),
            OpenKind::LeadStall => l.pick("负责人可能卡住了", "The lead may be stuck"),
            OpenKind::LoopTouched => l.pick("值班改了文件", "Loop Changed Files"),
            OpenKind::LoopFailed => l.pick("值班没能开工", "The Loop could not wake"),
            OpenKind::Conflict => l.pick("同时改了一个文件", "Concurrent File Edits"),
            OpenKind::Overstep => l.pick("越界", "Out-of-Scope Edit"),
            OpenKind::Trace => l.pick("结果和工作目录对不上", "Result Differs from Workspace"),
            OpenKind::Stall => l.pick("可能卡住了", "May Be Stuck"),
            OpenKind::Blocker => l.pick("阻塞", "Blocked"),
            OpenKind::LeadQuestion => l.pick("问负责人", "Question for the Lead"),
            OpenKind::Failed => l.pick("失败", "Failed"),
            OpenKind::Blocked => l.pick("等上游", "Waiting on upstream"),
            OpenKind::SplitRequest => l.pick("请求拆分", "Split Requested"),
        }
        .into()
    }

    /// An open item's title, the line below it, its badge and its one-click answers.
    pub fn open_fact(&self, o: &mut OpenFact, st: &RunState) {
        let l = self.lang;
        let lead = l.pick("负责人在处理，你不用管", "The lead is on it; nothing for you to do").to_string();
        let who = self.name(&o.who);
        let task = o.tasks.first().cloned().unwrap_or_default();
        o.label = self.label(o.kind);
        o.sub = lead.clone();
        match o.kind {
            OpenKind::Question if !o.tasks.is_empty() => {
                o.title = tr!(l, "「{}」问你一个问题", "{} has a question for you", who);
                o.actions = self.answers(o, &task, false);
            }
            OpenKind::Question => {
                o.title =
                    tr!(l, "负责人等你拍板：{}", "The lead needs your call: {}", crate::text::clip(&o.question, 40));
                o.actions = self.answers(o, "", true);
            }
            OpenKind::Approval => {
                o.title = tr!(l, "「{}」等你拍板", "{} needs your call", who);
                o.actions = self
                    .permission_options(&o.options)
                    .into_iter()
                    .map(|p| {
                        let body = json!({
                            "type": "approve", "key": o.decision, "task": task, "agent": o.who,
                            "allow": p.kind.starts_with("allow"),
                            "option": if p.id.is_empty() { Value::Null } else { json!(p.id) }, "label": p.name,
                        });
                        QuickAction {
                            label: p.label,
                            help: p.help,
                            prominent: p.kind == "allow_once",
                            body,
                            kind: p.kind,
                        }
                    })
                    .collect();
            }
            OpenKind::Input if o.mode == "url" => {
                o.title = tr!(l, "「{}」需要你打开一个链接", "{} needs you to open a link", who);
            }
            OpenKind::Input => o.title = tr!(l, "「{}」需要你填一张表", "{} needs you to fill in a form", who),
            OpenKind::Session => {
                o.title = tr!(l, "「{}」这一轮结束，等你回话", "{} finished its turn and is waiting for you", who)
            }
            OpenKind::LoopTouched => {
                o.title = l.pick("值班在自己的副本里改了文件", "The Loop changed files in its own copy").into();
                o.sub = o.lines.iter().take(5).cloned().collect::<Vec<_>>().join(self.and());
            }
            OpenKind::LoopFailed => {
                o.title = l.pick("值班没能开工", "The Loop could not wake").into();
                o.sub = l
                    .pick(
                        "下次开工会再试；也可以在值班页叫它现在开工",
                        "It retries at the next wake; you can also wake it now from the Loop page",
                    )
                    .into();
            }
            OpenKind::LeadStall => {
                o.title = l.pick("负责人可能卡住了", "The lead may be stuck").into();
                o.sub = l
                    .pick(
                        "可以在负责人的会话里发消息、打断并接管，或者继续等",
                        "Send a message in the lead’s session, interrupt and take over, or keep waiting",
                    )
                    .into();
            }
            OpenKind::Conflict | OpenKind::Overstep => {
                let owned = o.kind == OpenKind::Overstep;
                o.title = match (o.by_lead, owned) {
                    (true, true) => tr!(l, "负责人改了 {} 负责的文件", "The lead edited files owned by {}", task),
                    (true, false) => tr!(l, "负责人和 {} 改了同一个文件", "The lead and {} edited the same file", task),
                    (false, true) => tr!(l, "{} 改了 {} 负责的文件", "{} edited files owned by {}", task, o.other),
                    (false, false) => {
                        tr!(l, "{} 和 {} 改了同一个文件", "{} and {} edited the same file", task, o.other)
                    }
                };
                o.text = o.lines.join(self.and());
            }
            OpenKind::SplitRequest => o.title = tr!(l, "{} 请求拆分子任务", "{} requested a subtask split", task),
            OpenKind::Trace => {
                o.title = tr!(l, "{} 的结果和工作目录对不上", "The result for {} differs from the workspace", task);
                o.text = o.lines.join("\n");
            }
            OpenKind::Stall => o.title = tr!(l, "{} 可能卡住了", "{} may be stuck", task),
            OpenKind::Blocker => o.title = tr!(l, "{} 遇到阻塞", "{} is blocked", task),
            OpenKind::LeadQuestion => o.title = tr!(l, "{} 问负责人", "{} asked the lead", task),
            OpenKind::Failed => {
                o.title = tr!(l, "{} 失败，等负责人决定", "{} failed. Waiting for the lead to decide.", task);
                if o.text.is_empty() {
                    o.text = o.lines.join(self.semi());
                }
                o.sub =
                    l.pick("负责人可以重做、接受结果或取消", "The lead can retry, accept the result, or cancel").into();
            }
            OpenKind::Blocked => {
                let id = o.tasks.last().cloned().unwrap_or_default();
                o.title = tr!(l, "{} 等待 {}", "{} is waiting for {}", id, o.other);
                o.text = tr!(
                    l,
                    "依赖的 {} 没有完成，{} 还不能开始。",
                    "{1} cannot start until its dependency {0} is complete.",
                    o.other,
                    id
                );
            }
        }
        // The person's requests name the task they are about below the title.
        if matches!(o.kind, OpenKind::Question | OpenKind::Approval | OpenKind::Input | OpenKind::Session) {
            o.sub = if o.task_title.is_empty() && st.task(&task).is_none() {
                String::new()
            } else {
                format!("{task} · {}", o.task_title)
            };
        }
    }

    /// A question's options as answers; the recommended one stands out.
    fn answers(&self, o: &OpenFact, task: &str, lead: bool) -> Vec<QuickAction> {
        super::texts(&o.options)
            .into_iter()
            .map(|a| {
                let recommended = a == o.recommend;
                let help =
                    if lead && recommended { self.lang.pick("负责人建议", "Lead’s recommendation") } else { "" };
                let body = json!({ "type": "answer", "key": o.decision, "task": task, "answer": a, "note": "" });
                QuickAction { label: a, help: help.into(), prominent: recommended, body, kind: String::new() }
            })
            .collect()
    }

    /// An approval's options in the same words whatever the agent wrote: allow once, always allow,
    /// deny, always deny; the agent's own words and what "always" does are kept as the help.
    pub fn permission_options(&self, options: &Value) -> Vec<PermissionOption> {
        let l = self.lang;
        let fallback = json!([{ "optionId": "", "name": "", "kind": "allow_once" }, { "optionId": "", "name": "", "kind": "reject_once" }]);
        let list = if super::list(options).is_empty() { super::list(&fallback) } else { super::list(options) };
        list.iter()
            .map(|o| {
                let kind = text(&o["kind"]);
                let name = if super::truthy(&o["name"]) { text(&o["name"]) } else { text(&o["optionId"]) };
                let word = match kind.as_str() {
                    "allow_once" => Some(l.pick("允许一次", "Allow Once")),
                    "allow_always" => Some(l.pick("始终允许", "Always Allow")),
                    "reject_once" => Some(l.pick("拒绝", "Deny")),
                    "reject_always" => Some(l.pick("始终拒绝", "Always Deny")),
                    _ => None,
                };
                let label = word.map(str::to_owned).unwrap_or_else(|| name.clone());
                let note = match kind.as_str() {
                    "allow_always" => Some(
                        l.pick("之后同类操作不再询问", "Future operations of this kind will be allowed automatically"),
                    ),
                    "reject_always" => Some(
                        l.pick("之后同类操作直接拒绝", "Future operations of this kind will be denied automatically"),
                    ),
                    _ => None,
                };
                let own = (!name.is_empty() && name != label)
                    .then(|| tr!(l, "Agent 的原话：{}", "Agent’s wording: {}", name));
                let help = note.map(str::to_owned).into_iter().chain(own).collect::<Vec<_>>().join(l.pick("。", ". "));
                PermissionOption {
                    id: text(&o["optionId"]),
                    name: if name.is_empty() { label.clone() } else { name },
                    kind,
                    label,
                    help,
                }
            })
            .collect()
    }

    fn decision(&self, d: &mut super::board::DecisionFact) {
        let l = self.lang;
        let task = d.task.clone();
        d.title = match d.kind {
            DecisionKind::Dispatched => {
                d.text = d
                    .dispatched
                    .iter()
                    .map(|x| {
                        let deps = if x.deps.is_empty() {
                            String::new()
                        } else {
                            tr!(l, "（依赖 {}）", " (depends on {})", x.deps.join(self.and()))
                        };
                        format!("{} {} → {}{deps}", x.id, x.title, self.name(&x.agent))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                l.pick("负责人派出子任务", "The lead dispatched subtasks").into()
            }
            DecisionKind::Rejected => l.pick("派工方案没通过校验", "The dispatch plan failed validation").into(),
            DecisionKind::Answered => {
                d.text =
                    if d.note.is_empty() { d.answer.clone() } else { format!("{}{}{}", d.answer, self.semi(), d.note) };
                tr!(l, "回答 {} 的提问", "Answered {}’s question", task)
            }
            DecisionKind::Approved => l.pick("批准一次操作", "Approved an operation").into(),
            DecisionKind::Denied => l.pick("拒绝一次操作", "Denied an operation").into(),
            DecisionKind::RiskAccepted => l.pick("接受风险并验收", "Accepted risks and approved delivery").into(),
            DecisionKind::Merged => l.pick("交付时合并到你的分支", "Merged into your branch on delivery").into(),
            DecisionKind::Amended => tr!(l, "负责人修订 {} 的结论", "Lead revised the findings for {}", task),
            DecisionKind::Cancelled => tr!(l, "负责人取消 {}", "Lead cancelled {}", task),
            DecisionKind::Reopened => {
                let session = match d.session.as_str() {
                    "continue" => Some(l.pick("会话：接上原会话", "Session: picks up its own").to_string()),
                    "new" => Some(l.pick("会话：新开会话", "Session: start a new one").to_string()),
                    _ => None,
                };
                let parts: Vec<String> = [
                    session,
                    (!d.from.is_empty()).then(|| tr!(l, "依据 {} 的意见", "On {}'s findings", d.from)),
                    (!d.note.is_empty()).then(|| tr!(l, "负责人的说明：{}", "Lead's note: {}", d.note)),
                    (!d.instructions.is_empty()).then(|| tr!(l, "新说明：{}", "New instructions: {}", d.instructions)),
                    d.accept
                        .as_ref()
                        .map(|a| tr!(l, "新的验收标准：{}", "New acceptance criteria: {}", a.join(self.semi()))),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !parts.is_empty() {
                    d.text = parts.join("\n");
                }
                if d.from.is_empty() {
                    tr!(l, "负责人重做 {}", "Lead reopened {}", task)
                } else {
                    tr!(l, "负责人让 {} 返工", "Lead sent {} back for rework", task)
                }
            }
            DecisionKind::AcceptedByLead => tr!(l, "负责人接受 {} 的结果", "Lead accepted the result for {}", task),
            DecisionKind::Message => {
                if d.images > 0 {
                    let images = tr!(l, "（附 {} 张图片）", "(Attached images: {})", d.images);
                    d.text =
                        [d.text.clone(), images].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
                }
                tr!(l, "给 {} 的补充", "Follow-up for {}", self.name(d.to.as_deref().unwrap_or("")))
            }
        };
    }

    /// What a delivery asks the person to look at, in words.
    pub fn signal(&self, s: &mut Signal) {
        let l = self.lang;
        let task = s.task.clone().unwrap_or_default();
        match s.kind {
            SignalKind::Decision | SignalKind::Risk => {}
            SignalKind::FailedChecks => {
                s.text = tr!(l, "{} 有 {} 条验收标准没通过", "{}: {} criteria not met", task, s.count)
            }
            SignalKind::TraceGaps => {
                s.text = tr!(l, "{} 的结果和工作目录对不上 {} 处", "{} differs from the workspace: {}", task, s.count);
                s.detail = s.lines.join(self.semi());
            }
            SignalKind::Unverified => {
                s.text = tr!(
                    l,
                    "{} 改了文件，没有独立验证",
                    "{} changed files with no independent verification",
                    s.tasks.join(self.and())
                );
                s.detail = l
                    .pick("负责人要在总结里写明怎么核对的", "The lead's summary has to say how it checked them")
                    .into();
            }
            SignalKind::LeadEdits => {
                s.text = tr!(
                    l,
                    "负责人自己改了 {} 个文件，没有子任务验收和独立验证",
                    "Files the lead changed itself: {}, with no subtask acceptance or independent verification",
                    s.count
                );
                s.detail = s.lines.join(self.and());
            }
        }
    }
}

/// One of an approval's options, as the person reads it.
pub struct PermissionOption {
    /// The agent's `optionId`.
    pub id: String,
    /// The agent's own words for it.
    pub name: String,
    pub kind: String,
    pub label: String,
    pub help: String,
}
