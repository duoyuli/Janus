//! The card each request waiting on the person is answered with, in their language: who asks and
//! where, what about, the options or the form, and what the answer does. A form's JSON Schema is
//! read here into the fields a client draws (`Field`), so no client interprets a schema itself.
// @project-doc docs/architecture/20261031-frontend.md#request_cards

use super::board::QuickAction;
use super::card::{Pill, Tone};
use super::lines::in_workspace;
use super::words::Words;
use super::{Request, RequestKind, RunState, list, text, texts, truthy};
use crate::lang::tr;
use serde::Serialize;
use serde_json::{Value, json};

crate::wire::wire_enum! {
    /// Which card: what the request asks and how it is answered.
    pub enum CardKind {
        /// A collaborator's question to the person, answered with one of its options.
        Question = "question",
        /// The lead's decision handed to the person (`janus.ask_user`).
        Decision = "decision",
        /// An agent's permission request, answered with one of its own options.
        Approval = "approval",
        /// A form to finish elsewhere: a link to open.
        Link = "link",
        /// A form built from the agent's JSON Schema.
        Form = "form",
        /// A session the person holds waits for their reply.
        Session = "session",
    }
}

/// One request's card.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestCard {
    pub kind: CardKind,
    /// The decision it answers; empty for a session's turn.
    pub key: String,
    pub seq: i64,
    pub ts: i64,
    pub pill: Pill,
    /// Who asks (a member id), for the avatar.
    pub agent: String,
    pub title: String,
    /// What it says: the question, the form's message.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Why it came to the person.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// The command an approval would run, relative to the workspace.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Label and value rows under it: who asks and where, and an approval's policy.
    pub facts: Vec<(String, String)>,
    /// A question's or a decision's options, and the one the lead recommends.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub recommend: String,
    /// The answers an approval gives, as the agent offered them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<QuickAction>,
    /// What the agent says the approval affects, and what its 「always」 option means.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub impact: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub always: String,
    /// A form's fields.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    /// The line under the controls: where the answer goes, what waits on it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub footnote: String,
    /// The conversation a session's card opens (`task:<id>` or `agent:<id>`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session: String,
}

crate::wire::wire_enum! {
    /// How a form field is answered.
    pub enum FieldKind {
        /// One of `choices`.
        Choice = "choice",
        /// Any of `choices`.
        Multi = "multi",
        Bool = "bool",
        Number = "number",
        /// A short text on one line.
        Line = "line",
        Text = "text",
    }
}

/// One field of a form, as its schema describes it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub required: bool,
    pub kind: FieldKind,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
    /// The field for an answer of the person's own beside the choices, and its placeholder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<Custom>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Choice {
    /// What the answer carries.
    pub value: Value,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub desc: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Custom {
    pub key: String,
    pub placeholder: String,
}

/// The cards of every request waiting on the person, in the order they wait.
pub(super) fn cards(st: &RunState, open: &[super::board::OpenFact], w: &Words) -> Vec<RequestCard> {
    st.inbox.iter().map(|r| card(st, r, open, w)).collect()
}

fn card(st: &RunState, r: &Request, open: &[super::board::OpenFact], w: &Words) -> RequestCard {
    let l = w.lang;
    let d = &r.d;
    let task = r.task.as_deref().and_then(|t| st.task(t));
    let task_agent = task.map(|t| t.agent.clone()).unwrap_or_default();
    let whence = |agent: &str| -> (String, String) {
        let place = match (&r.task, task) {
            (Some(id), t) => format!("{id} · {}", t.map(|t| t.title.as_str()).unwrap_or("")),
            (None, _) => l.pick("负责人会话", "Lead Session").into(),
        };
        (l.pick("来自", "From").into(), format!("{} · {place}", w.name(agent)))
    };
    let mut c = RequestCard {
        kind: CardKind::Session,
        key: r.key.clone(),
        seq: r.seq,
        ts: r.ts,
        pill: Pill { text: String::new(), tone: Tone::Amber },
        agent: String::new(),
        title: String::new(),
        text: String::new(),
        reason: String::new(),
        command: String::new(),
        url: String::new(),
        facts: vec![],
        options: vec![],
        recommend: String::new(),
        actions: vec![],
        impact: String::new(),
        always: String::new(),
        fields: vec![],
        footnote: String::new(),
        session: String::new(),
    };
    match r.kind {
        RequestKind::Question if r.task.is_none() => {
            c.kind = CardKind::Decision;
            c.pill.text = l.pick("等你拍板", "Your Call").into();
            c.agent = "lead".into();
            let q = text(&d["question"]);
            c.title = tr!(l, "负责人等你拍板：{q}", "The lead needs your call: {q}");
            let why = text(&d["reason"]);
            if !why.is_empty() {
                c.reason = tr!(l, "为什么要你决定：{why}", "Why your decision is needed: {why}");
            }
            c.options = texts(&d["options"]);
            c.recommend = text(&d["recommend"]);
            c.facts.push(whence("lead"));
            c.footnote = l
                .pick(
                    "负责人在等这个决定期间，会继续做不依赖它的工作。",
                    "The lead will continue work that does not depend on this decision.",
                )
                .into();
        }
        RequestKind::Question => {
            c.kind = CardKind::Question;
            c.pill.text = l.pick("问题", "Question").into();
            let id = r.task.clone().unwrap_or_default();
            let q = task.and_then(|t| t.question.as_ref());
            let from = q.map(|q| q.from.clone()).filter(|f| !f.is_empty()).unwrap_or(task_agent.clone());
            let name = w.name(&from);
            let blocking = if q.is_some_and(|q| q.blocking) { l.pick(" · 阻塞中", " · Blocking") } else { "" };
            c.title =
                tr!(l, "「{name}」在 {id} 问你一个问题{blocking}", "{name} has a question for you in {id}{blocking}");
            c.agent = from.clone();
            c.text = q.map(|q| q.question.clone()).unwrap_or_default();
            let why = text(&d["reason"]);
            if !why.is_empty() {
                c.reason = tr!(l, "↳ 负责人 Agent 转交给你：{why}", "↳ The lead forwarded this to you: {why}");
            }
            c.options = q.map(|q| q.options.clone()).unwrap_or_default();
            c.recommend = text(&d["recommend"]);
            c.facts.push(whence(&from));
            let downstream: Vec<&str> =
                st.tasks.iter().filter(|t| t.deps.contains(&id)).map(|t| t.id.as_str()).collect();
            let affected = if downstream.is_empty() {
                String::new()
            } else {
                let list = downstream.join(w.and());
                tr!(l, "受影响的下游子任务：{list}。", " Affected downstream subtasks: {list}.")
            };
            c.footnote = tr!(
                l,
                "回答会记入日志，并发回给 Agent。{affected}",
                "Your answer is logged and sent back to the Agent.{affected}"
            );
        }
        RequestKind::Approval => {
            c.kind = CardKind::Approval;
            c.pill = Pill { text: l.pick("审批", "Approval").into(), tone: Tone::Red };
            c.agent = r.agent.clone().unwrap_or(task_agent);
            let name = w.name(&c.agent);
            let op = if truthy(&d["title"]) {
                text(&d["title"])
            } else {
                l.pick("执行操作", "Perform an operation").into()
            };
            c.title = tr!(l, "「{name}」等你拍板：{op}", "{name} needs your call: {op}");
            if truthy(&d["command"]) && d["command"] != d["title"] {
                c.command = format!("$ {}", in_workspace(&text(&d["command"]), &st.run.cwd));
            }
            c.facts.push(whence(&c.agent.clone()));
            if truthy(&d["policy"]) {
                c.facts.push((l.pick("原因", "Reason").into(), text(&d["policy"])));
            }
            c.actions = open
                .iter()
                .find(|o| o.you && o.seq == Some(r.seq) && o.kind.as_str() == "approval")
                .map(|o| o.actions.clone())
                .unwrap_or_default();
            c.impact = text(&d["impact"]);
            if let Some(a) = c.actions.iter().find(|a| a.kind == "allow_always") {
                let (label, help) = (&a.label, &a.help);
                c.always = tr!(l, "「{label}」：{help}", "“{label}”: {help}");
            }
        }
        RequestKind::Input => {
            c.agent = r.agent.clone().unwrap_or_default();
            let name = w.name(&c.agent);
            c.text = text(&d["message"]);
            c.facts.push(whence(&c.agent.clone()));
            if text(&d["mode"]) == "url" {
                c.kind = CardKind::Link;
                c.pill.text = l.pick("打开链接", "Open Link").into();
                c.title = tr!(l, "「{name}」需要你打开一个链接", "{name} needs you to open a link");
                c.url = text(&d["url"]);
            } else {
                c.kind = CardKind::Form;
                c.pill.text = l.pick("需要填写", "Form Required").into();
                c.title = tr!(l, "「{name}」需要你填一张表", "{name} needs you to fill in a form");
                c.fields = fields(&d["schema"], w);
                c.footnote = l.pick("内容原样交回 Agent。", "Your answer goes back to the Agent as written.").into();
            }
        }
        RequestKind::Session => {
            c.kind = CardKind::Session;
            c.pill.text = l.pick("会话", "Sessions").into();
            c.agent = r.agent.clone().unwrap_or(task_agent);
            let name = w.name(&c.agent);
            match &r.task {
                Some(id) => {
                    c.title = tr!(l, "「{name}」在 {id} 等你回话", "{name} is waiting for you in {id}");
                    c.text = l
                        .pick(
                            "你已接管。Agent 会一直等你：发消息、交还托管或结束子任务。",
                            "You've taken over. The Agent waits until you send a message, hand back control, or finish the subtask.",
                        )
                        .into();
                    c.session = super::sessions::task_key(id);
                }
                None => {
                    c.title = tr!(l, "「{name}」在等你回话", "{name} is waiting for you");
                    c.session = super::sessions::agent_key(&c.agent);
                }
            }
        }
    }
    c
}

/// The fields of a form's schema in name order: single and multiple choices (`oneOf`, `anyOf`,
/// `enum`), yes or no, a number, a line or a text; a field that only adds the person's own answer
/// to another (`customAnswer`, `<name>_custom`, `<name>_note`) goes with that one.
fn fields(schema: &Value, w: &Words) -> Vec<Field> {
    let props = schema["properties"].as_object().cloned().unwrap_or_default();
    let mut names: Vec<String> = props.keys().cloned().collect();
    names.sort();
    let required: Vec<String> = texts(&schema["required"]);
    let custom_of: Vec<(String, String)> =
        names.iter().filter_map(|k| custom_for(k, &props[k], &names).map(|base| (base, k.clone()))).collect();
    names
        .iter()
        .filter(|k| custom_for(k, &props[*k], &names).is_none())
        .map(|k| {
            let p = &props[k];
            let choices = choices(p);
            let kind = if !choices.is_empty() {
                if text(&p["type"]) == "array" { FieldKind::Multi } else { FieldKind::Choice }
            } else {
                match text(&p["type"]).as_str() {
                    "boolean" => FieldKind::Bool,
                    "number" | "integer" => FieldKind::Number,
                    _ if p["maxLength"].as_i64().is_some_and(|m| m <= 120) => FieldKind::Line,
                    _ => FieldKind::Text,
                }
            };
            let custom = custom_of.iter().find(|(base, _)| base == k).map(|(_, c)| {
                let desc = text(&props[c]["description"]);
                Custom {
                    key: c.clone(),
                    placeholder: if desc.is_empty() {
                        w.lang.pick("或者写下你自己的回答", "Or Enter Your Own Answer").into()
                    } else {
                        desc
                    },
                }
            });
            Field {
                key: k.clone(),
                title: if truthy(&p["title"]) { text(&p["title"]) } else { k.clone() },
                description: text(&p["description"]),
                required: required.contains(k),
                kind,
                choices,
                custom,
            }
        })
        .collect()
}

/// The field `k` adds the person's own answer to, if that is what it is.
fn custom_for(k: &str, p: &Value, names: &[String]) -> Option<String> {
    let custom = &p["_meta"]["jetbrains"]["air"]["customAnswer"];
    if truthy(custom) {
        let marked = text(&custom["questionId"]);
        if !marked.is_empty() {
            return Some(marked);
        }
        let codex = text(&p["_meta"]["codex"]["questionId"]);
        if !codex.is_empty() {
            return Some(codex);
        }
    }
    for suffix in ["_custom", "_note"] {
        if let Some(base) = k.strip_suffix(suffix)
            && names.iter().any(|n| n == base)
        {
            return Some(base.to_string());
        }
    }
    None
}

fn choices(p: &Value) -> Vec<Choice> {
    let mut items: Vec<Value> = list(&p["oneOf"]).to_vec();
    for alt in [&p["anyOf"], &p["items"]["anyOf"], &p["items"]["oneOf"]] {
        if items.is_empty() {
            items = list(alt).to_vec();
        }
    }
    if items.is_empty() {
        let en = if list(&p["enum"]).is_empty() { list(&p["items"]["enum"]) } else { list(&p["enum"]) };
        items = en.iter().map(|v| json!({ "const": v, "title": text(v) })).collect();
    }
    items
        .iter()
        .map(|o| {
            let title = if truthy(&o["title"]) { text(&o["title"]) } else { text(&o["const"]) };
            let parts: Vec<&str> = title.split(" — ").collect();
            let label = if truthy(&o["_meta"]["label"]) { text(&o["_meta"]["label"]) } else { parts[0].to_string() };
            let desc = if truthy(&o["description"]) {
                text(&o["description"])
            } else if truthy(&o["_meta"]["description"]) {
                text(&o["_meta"]["description"])
            } else {
                parts[1..].join(" — ")
            };
            Choice { value: o["const"].clone(), label, desc }
        })
        .collect()
}
