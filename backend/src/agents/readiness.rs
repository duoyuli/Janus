//! What the setup screens read about whether work can start: why a digital employee cannot run,
//! what keeps a workspace's team from running (grouped by cause), and the team a new workspace
//! starts with. Worked out once here for every client.
// @project-doc docs/interfaces/20261031-http_api.md#setup

use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Where a problem is fixed: the settings page of the Agents or of the digital employees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixAt {
    Agents,
}

/// One thing that keeps a workspace's team from running, and where to fix it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Problem {
    pub text: String,
    /// The button that goes there.
    pub action: String,
    pub tab: FixAt,
}

/// The team a new workspace starts with, and each group's when one is taken as the team.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DefaultTeam {
    /// The first lead-capable role marked to lead; empty when there is none.
    pub lead: String,
    /// The lead's group (the lead and everyone in its group who has an Agent); a lead in no
    /// group, or no lead, takes every role that has an Agent.
    pub members: Vec<String>,
    /// By group: its members bound to an Agent, and of them the ones that can lead, in order.
    pub groups: BTreeMap<String, GroupTeam>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GroupTeam {
    pub members: Vec<String>,
    pub leads: Vec<String>,
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// The connection `id` names, when there is one.
fn connection<'a>(connections: &'a [Value], id: &str) -> Option<&'a Value> {
    connections.iter().find(|c| !id.is_empty() && s(&c["id"]) == id)
}

/// Why `role` cannot run: no Agent bound, or its Agent has not passed its check.
pub fn unavailable(role: &Value, connections: &[Value], lang: Lang) -> Option<String> {
    let Some(c) = connection(connections, s(&role["connection"])) else {
        return Some(lang.pick("还没有绑定 Agent", "No Agent bound yet").into());
    };
    let name = s(&c["name"]);
    (s(&c["phase"]) != "ok").then(|| tr!(lang, "Agent「{name}」还没通过检查", "Agent “{name}” hasn't passed its check"))
}

/// What keeps `workspace`'s team from running, grouped by cause: members whose role was deleted,
/// roles with no Agent, and each Agent that has not passed its check or is not signed in (one
/// problem however many roles use it).
pub fn problems(workspace: &Value, roles: &[Value], connections: &[Value], lang: Lang) -> Vec<Problem> {
    let l = lang;
    let and = l.pick("、", ", ");
    let fix = |text: String, action: &str| Problem { text, action: action.into(), tab: FixAt::Agents };
    let members: Vec<Option<&Value>> = workspace["members"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| roles.iter().find(|r| s(&r["id"]) == s(&m["role"])))
        .collect();
    let mut out = vec![];
    let deleted = members.iter().filter(|r| r.is_none()).count();
    let settings = l.pick("打开数字员工设置", "Open Digital Employee Settings");
    if deleted > 0 {
        out.push(fix(
            tr!(
                l,
                "有 {deleted} 个数字员工已被删除，去工作区设置里移除或换人。",
                "{deleted} digital employees were deleted. Remove or replace them in Workspace Settings."
            ),
            settings,
        ));
    }
    let present: Vec<&Value> = members.into_iter().flatten().collect();
    let unbound: Vec<&str> = present
        .iter()
        .filter(|r| connection(connections, s(&r["connection"])).is_none())
        .map(|r| s(&r["name"]))
        .collect();
    if !unbound.is_empty() {
        let names = unbound.join(and);
        out.push(fix(tr!(l, "还没绑 Agent：{names}", "No Agent yet: {names}"), settings));
    }
    let mut untested: Vec<(&str, Vec<&str>)> = vec![];
    for r in &present {
        let Some(c) = connection(connections, s(&r["connection"])) else { continue };
        if s(&c["phase"]) == "ok" {
            continue;
        }
        let name = s(&c["name"]);
        match untested.iter_mut().find(|(n, _)| *n == name) {
            Some((_, roles)) => roles.push(s(&r["name"])),
            None => untested.push((name, vec![s(&r["name"])])),
        }
    }
    for (agent, names) in untested {
        let affected = names.join(and);
        out.push(fix(
            tr!(
                l,
                "Agent「{agent}」还没通过检查或没登录，这些数字员工跑不起来：{affected}",
                "Agent “{agent}” hasn't passed its check or isn't signed in, so these digital employees can't run: {affected}"
            ),
            l.pick("去检查", "Check"),
        ));
    }
    out
}

/// The team a new workspace starts with, from the roles and the Agents they are bound to.
// @project-doc docs/domains/20261031-agents_and_roles.md#product_launch_roles
pub fn default_team(roles: &[Value], connections: &[Value]) -> DefaultTeam {
    let bound = |r: &&Value| connection(connections, s(&r["connection"])).is_some();
    let can_lead = |r: &&Value| r["lead"].as_bool().unwrap_or(false) && bound(r);
    let lead = roles.iter().find(can_lead).map(|r| s(&r["id"]).to_string()).unwrap_or_default();
    let has_agent: Vec<&Value> = roles.iter().filter(|r| !s(&r["connection"]).is_empty()).collect();
    let group = roles.iter().find(|r| s(&r["id"]) == lead).map(|r| s(&r["group"])).filter(|g| !g.is_empty());
    let members = has_agent
        .iter()
        .filter(|r| group.is_none_or(|g| s(&r["group"]) == g || s(&r["id"]) == lead))
        .map(|r| s(&r["id"]).to_string())
        .collect();
    let mut groups: BTreeMap<String, GroupTeam> = BTreeMap::new();
    for r in roles.iter().filter(|r| !s(&r["group"]).is_empty()) {
        let g = groups.entry(s(&r["group"]).to_string()).or_default();
        if bound(&r) {
            g.members.push(s(&r["id"]).to_string());
        }
        if can_lead(&r) {
            g.leads.push(s(&r["id"]).to_string());
        }
    }
    DefaultTeam { lead, members, groups }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conns() -> Vec<Value> {
        vec![
            json!({ "id": "c1", "name": "Claude", "phase": "ok" }),
            json!({ "id": "c2", "name": "Codex", "phase": "untested" }),
        ]
    }

    #[test]
    fn a_workspaces_problems_are_grouped_by_cause() {
        let roles = vec![
            json!({ "id": "a", "name": "甲", "connection": "c2" }),
            json!({ "id": "b", "name": "乙", "connection": "c2" }),
            json!({ "id": "c", "name": "丙", "connection": "" }),
            json!({ "id": "d", "name": "丁", "connection": "c1" }),
        ];
        let ws = json!({ "members": [{ "role": "a" }, { "role": "b" }, { "role": "c" }, { "role": "d" }, { "role": "gone" }] });
        let p = problems(&ws, &roles, &conns(), Lang::Zh);
        let texts: Vec<&str> = p.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "有 1 个数字员工已被删除，去工作区设置里移除或换人。",
                "还没绑 Agent：丙",
                "Agent「Codex」还没通过检查或没登录，这些数字员工跑不起来：甲、乙",
            ]
        );
        assert_eq!(p[2].action, "去检查");
        assert!(problems(&json!({ "members": [{ "role": "d" }] }), &roles, &conns(), Lang::Zh).is_empty());
        assert_eq!(
            unavailable(&roles[0], &conns(), Lang::En).as_deref(),
            Some("Agent “Codex” hasn't passed its check")
        );
        assert_eq!(unavailable(&roles[2], &conns(), Lang::Zh).as_deref(), Some("还没有绑定 Agent"));
        assert_eq!(unavailable(&roles[3], &conns(), Lang::Zh), None);
    }

    #[test]
    fn a_new_workspace_starts_with_the_leads_group() {
        let roles = vec![
            json!({ "id": "pm", "connection": "c1", "lead": true, "group": "产品研发" }),
            json!({ "id": "dev", "connection": "c1", "group": "产品研发" }),
            json!({ "id": "free", "connection": "c1", "group": "" }),
            json!({ "id": "w", "connection": "c1", "group": "写作", "lead": true }),
            json!({ "id": "none", "connection": "", "group": "写作" }),
        ];
        let t = default_team(&roles, &conns());
        assert_eq!((t.lead.as_str(), t.members.clone()), ("pm", vec!["pm".to_string(), "dev".to_string()]));
        assert_eq!(t.groups["写作"], GroupTeam { members: vec!["w".into()], leads: vec!["w".into()] });
        // A lead in no group takes every role with an Agent.
        let mut ungrouped = roles.clone();
        ungrouped[0]["group"] = json!("");
        assert_eq!(default_team(&ungrouped, &conns()).members, ["pm", "dev", "free", "w"]);
        // No lead: every role with an Agent.
        let t = default_team(&roles[1..3], &conns());
        assert_eq!((t.lead.as_str(), t.members.len()), ("", 2));
    }
}
