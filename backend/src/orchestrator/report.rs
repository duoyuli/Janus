//! The lead's report of a round: its final reply, split into the conclusion, the decisions it
//! leaves the person and what it suggests adding to the workspace's shared material, under the
//! headings its brief asks for; and the delivery's text as later sessions are told it.
//! The split is for showing the report in parts: a section not found stays empty and the whole
//! reply is still the delivery (docs/decisions/20261126-lead_report_in_prose.md).
// @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode

use super::*;
use crate::lang::Lang;

impl App {
    /// What the lead reports when a turn of it delivers: its last reply, since the earlier ones
    /// narrated the work, without the marker a lead that confirmed its goal to Janus starts with.
    /// The whole turn's text only when no reply was recorded.
    // @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode
    pub(super) fn lead_report(&self, run: &Run, text: &str) -> String {
        last_reply(&self.db.events(&run.id, 0).unwrap_or_default())
            .map(|t| crate::goal_settle::confirmed(&t).filter(|s| !s.is_empty()).unwrap_or(t))
            .unwrap_or_else(|| text.to_string())
    }
}

/// A heading Janus asks the lead for in its run's language and reads back in either.
pub(super) struct Heading {
    pub zh: &'static str,
    pub en: &'static str,
}

/// The heading the lead puts its suggested additions to the shared material under (see `goal_brief`).
pub(super) const BRIEF_HEADING: Heading =
    Heading { zh: "建议更新共享资料", en: "Suggested Updates to Shared Context" };

/// The heading of the lead's conclusion in its final reply, a sentence or two (see `goal_brief`).
pub(super) const HEADLINE_HEADING: Heading = Heading { zh: "结论", en: "Conclusion" };

/// The heading of the questions the lead's final reply leaves for the person (see `goal_brief`).
pub(super) const DECISIONS_HEADING: Heading = Heading { zh: "需要你决定", en: "Needs Your Decision" };

/// The lead's last message taken apart: the sections it ends with by the lead's brief
/// (`goal_brief`), each under its own heading, and the rest.
// @project-doc docs/domains/20261031-goal_mode.md#delivery_in_goal_mode
#[derive(Debug, Default, PartialEq)]
pub(super) struct LeadReport {
    /// The message without the sections below.
    pub summary: String,
    /// The conclusion in a sentence or two (`HEADLINE_HEADING`); empty when the lead left none.
    pub headline: String,
    /// Questions only the person can answer (`DECISIONS_HEADING`).
    pub decisions: Vec<String>,
    /// Lines to add to the workspace's shared material (`BRIEF_HEADING`).
    pub brief: Vec<String>,
}

/// The first section of `text` under a heading spelled one of `names` (any level, bold or not, a
/// trailing colon or not), up to the next heading: its lines without list markers, and `text`
/// without it. No such heading leaves `text` as it is.
pub(super) fn take_section(text: &str, names: &[&str]) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_heading = |l: &str| {
        let t = l.trim().trim_start_matches('#').replace("**", "");
        let heading = t.trim().trim_end_matches(['：', ':']).trim();
        names.iter().any(|n| heading.eq_ignore_ascii_case(n))
    };
    let Some(start) = lines.iter().position(|l| is_heading(l)) else {
        return (text.to_string(), vec![]);
    };
    let end =
        lines[start + 1..].iter().position(|l| l.trim_start().starts_with('#')).map_or(lines.len(), |i| start + 1 + i);
    let items = lines[start + 1..end]
        .iter()
        .map(|l| {
            let t = l.trim();
            let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
            let t = t
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                .map_or(t, |(_, rest)| rest);
            t.trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    let rest = [&lines[..start], &lines[end..]].concat().join("\n");
    (rest.trim().to_string(), items)
}

/// The lead's last reply: its trailing run of `agent.message` pieces (nothing else of the lead's
/// between them), joined back together.
pub(super) fn last_reply(events: &[Event]) -> Option<String> {
    // The lead's own steps, and what reached it (a prompt from Janus, a message): each ends a reply.
    let lead = |e: &&Event| {
        e.task.is_none()
            && (e.actor == "lead" || e.to.as_deref() == Some("lead"))
            && !matches!(
                e.kind(),
                EventKind::AgentStatus | EventKind::SessionUsage | EventKind::AgentThought | EventKind::MessageRouted
            )
    };
    let mine: Vec<&Event> = events.iter().filter(lead).collect();
    let tail: Vec<&str> = mine
        .iter()
        .rev()
        .skip_while(|e| e.kind() != EventKind::AgentMessage)
        .take_while(|e| e.kind() == EventKind::AgentMessage)
        .filter_map(|e| e.data.as_ref()?["text"].as_str())
        .collect();
    (!tail.is_empty()).then(|| tail.into_iter().rev().collect::<String>().trim().to_string())
}

/// The lead's last message, kept whole: the app folds a long one, and the conclusion the person
/// reads first is its own section (`LeadReport`).
pub(super) fn lead_summary(text: &str) -> String {
    text.trim().to_string()
}

/// The last delivery in the event log as one text (`delivery_text`).
pub(super) fn delivery_summary(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find(|e| e.kind() == EventKind::RunDelivered)
        .and_then(|e| e.data.as_ref())
        .map(delivery_text)
        .unwrap_or_default()
}

/// A delivery (`run.delivered` `data`) as one text for an agent that reads it later (the lead
/// reopened, a Loop, the team memory's consolidation): the conclusion, the report and where the
/// result is.
pub(super) fn delivery_text(d: &Value) -> String {
    let mut parts: Vec<&str> = vec![];
    for key in ["headline", "summary"] {
        let t = d[key].as_str().unwrap_or("").trim();
        if !t.is_empty() && !parts.contains(&t) {
            parts.push(t);
        }
    }
    if let Some(note) = d["merge"]["note"].as_str().filter(|n| !n.trim().is_empty()) {
        parts.push(note.trim());
    }
    parts.join("\n\n")
}

impl App {
    /// The run's last delivery as one text (`delivery_text`); nothing before it was delivered.
    pub(super) fn last_delivery_text(&self, run_id: &str) -> Option<String> {
        self.db
            .latest_event(run_id, EventKind::RunDelivered)
            .ok()
            .flatten()
            .and_then(|e| e.data)
            .map(|d| delivery_text(&d))
    }
}

impl Heading {
    pub(super) fn in_(&self, lang: Lang) -> &'static str {
        lang.pick(self.zh, self.en)
    }
}

impl LeadReport {
    pub(super) fn of(text: &str) -> LeadReport {
        let names = |h: &Heading| [h.zh, h.en];
        let (rest, headline) = take_section(text, &names(&HEADLINE_HEADING));
        let (rest, decisions) = take_section(&rest, &names(&DECISIONS_HEADING));
        let (rest, brief) = take_section(&rest, &names(&BRIEF_HEADING));
        LeadReport { summary: lead_summary(&rest), headline: headline.join("\n"), decisions, brief }
    }
}
