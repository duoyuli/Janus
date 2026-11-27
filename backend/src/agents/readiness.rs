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

/// A role's permission choices in its Agent's own words ("Sandbox Full access · Plan Off"): each
/// control the Agent marks as a permission, at the role's value or the control's default.
pub fn perm_label(role: &Value, connections: &[Value], lang: Lang) -> String {
    let Some(c) = connection(connections, s(&role["connection"])) else { return String::new() };
    c["options"]["controls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| x["permission"] == true)
        .map(|x| {
            let v = match &role["session"][s(&x["id"])] {
                Value::Null => &x["default"],
                v => v,
            };
            if let Some(b) = v.as_bool() {
                return format!(
                    "{} {}",
                    s(&x["name"]),
                    if b { lang.pick("开启", "On") } else { lang.pick("关闭", "Off") }
                );
            }
            x["options"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|o| &o["value"] == v)
                .map(|o| s(&o["name"]).to_string())
                .unwrap_or_else(|| s(v).to_string())
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Who does a role's work, in one line: its Agent, its model and its permission line
/// ("Claude Code · claude-sonnet · Bypass permissions").
pub fn setup_line(role: &Value, connections: &[Value], lang: Lang) -> String {
    let agent = connection(connections, s(&role["connection"])).map(|c| s(&c["name"])).unwrap_or_default();
    let perms = perm_label(role, connections, lang);
    [agent, s(&role["model"]), perms.as_str()].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// Whether `role` can lead a workspace: marked to lead and bound to an Agent Janus knows (Codex
/// and Claude Code both pursue goals with /goal).
pub fn can_lead(role: &Value, connections: &[Value]) -> bool {
    role["lead"].as_bool().unwrap_or(false) && connection(connections, s(&role["connection"])).is_some()
}

/// One step still to take before a first goal can run, and whether it is done.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Step {
    pub text: String,
    pub done: bool,
    /// Where it is done: the settings page of the Agents (`agents`), or a new workspace (`workspace`).
    pub go: &'static str,
}

/// What a first goal needs, in order: an Agent that passed its check, digital employees bound to
/// an Agent (at least two, one of them able to lead), a workspace.
// @project-doc docs/architecture/20261031-frontend.md#screens
pub fn onboarding(connections: &[Value], roles: &[Value], workspaces: usize, lang: Lang) -> Vec<Step> {
    let bound: Vec<&Value> = roles.iter().filter(|r| connection(connections, s(&r["connection"])).is_some()).collect();
    let l = lang;
    vec![
        Step {
            text: l.pick("接入一个 Agent", "Connect an Agent").into(),
            done: connections.iter().any(|c| s(&c["phase"]) == "ok"),
            go: "agents",
        },
        Step {
            text: l.pick("定义数字员工（含一个负责人）", "Create Digital Employees, Including a Lead").into(),
            done: bound.iter().any(|r| r["lead"].as_bool().unwrap_or(false)) && bound.len() >= 2,
            go: "agents",
        },
        Step { text: l.pick("新建工作区", "New Workspace").into(), done: workspaces > 0, go: "workspace" },
    ]
}

/// The digital employees of one group, as the lists show them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoleGroup {
    /// The group's name; empty for the roles in no group.
    pub group: String,
    /// Its name as the lists show it.
    pub title: String,
    /// Its roles by id, in the order they are listed.
    pub roles: Vec<String>,
}

/// The groups digital employees are listed under: named ones by name (numbers by value, case
/// ignored), then the roles in no group. Settings, the new-chat page and the workspace editor
/// list them this way.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_groups
pub fn role_groups(roles: &[Value], lang: Lang) -> Vec<RoleGroup> {
    let mut named: Vec<&str> = roles.iter().map(|r| s(&r["group"])).filter(|g| !g.is_empty()).collect();
    named.sort_by(|a, b| natural(a, b));
    named.dedup();
    if roles.iter().any(|r| s(&r["group"]).is_empty()) {
        named.push("");
    }
    named
        .into_iter()
        .map(|g| RoleGroup {
            group: g.into(),
            title: if g.is_empty() { lang.pick("未分组", "No Group").into() } else { g.into() },
            roles: roles.iter().filter(|r| s(&r["group"]) == g).map(|r| s(&r["id"]).to_string()).collect(),
        })
        .collect()
}

/// `a` before `b` as names read: case ignored, runs of digits by their value.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = vec![];
        for c in s.to_lowercase().chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, part)) if *d == digit => part.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (x, y) = (parts(a), parts(b));
    for ((dx, px), (dy, py)) in x.iter().zip(y.iter()) {
        let o = match (dx, dy) {
            (true, true) => {
                let (nx, ny) = (px.trim_start_matches('0'), py.trim_start_matches('0'));
                nx.len().cmp(&ny.len()).then_with(|| nx.cmp(ny))
            }
            _ => px.cmp(py),
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

/// A workspace's team in a line: how many digital employees (and clones, when some have more
/// than one), and who leads.
pub fn team_line(workspace: &Value, roles: &[Value], lang: Lang) -> String {
    let l = lang;
    let lead = s(&workspace["lead"]);
    let members = workspace["members"].as_array().cloned().unwrap_or_default();
    let n = members.len();
    let slots: i64 =
        members.iter().map(|m| if s(&m["role"]) == lead { 1 } else { m["slots"].as_i64().unwrap_or(1).max(1) }).sum();
    let count = if slots > n as i64 {
        tr!(l, "{n} 个数字员工（共 {slots} 个分身）", "Digital employees: {n} · Clones: {slots}")
    } else {
        tr!(l, "{n} 个数字员工", "Digital employees: {n}")
    };
    let lead = match roles.iter().find(|r| !lead.is_empty() && s(&r["id"]) == lead).map(|r| s(&r["name"])) {
        None => String::new(),
        Some(name) if name == l.pick("负责人", "Lead") => l.pick(" · 有负责人", " · Lead assigned").into(),
        Some(name) => tr!(l, " · 负责人：{name}", " · Lead: {name}"),
    };
    format!("{count}{lead}")
}

/// How much shared material a workspace has: none yet, or how many characters.
pub fn brief_status(workspace: &Value, lang: Lang) -> String {
    let n = s(&workspace["brief"]).trim().chars().count();
    if n == 0 { lang.pick("还没写", "Empty").into() } else { tr!(lang, "{n} 字", "{n} characters") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_steps_groups_and_team_lines_read_as_the_lists_show_them() {
        let roles = vec![
            json!({ "id": "pm", "name": "负责人", "connection": "c1", "lead": true, "group": "组10" }),
            json!({ "id": "dev", "name": "开发", "connection": "c1", "group": "组9" }),
            json!({ "id": "x", "name": "X", "connection": "", "group": "" }),
        ];
        let steps = onboarding(&conns(), &roles, 0, Lang::Zh);
        assert_eq!(steps.iter().map(|s| s.done).collect::<Vec<_>>(), [true, true, false]);
        assert_eq!(steps[2].go, "workspace");
        assert!(!onboarding(&conns(), &roles[1..], 1, Lang::En)[1].done);
        let groups = role_groups(&roles, Lang::En);
        let names: Vec<(&str, &str)> = groups.iter().map(|g| (g.group.as_str(), g.title.as_str())).collect();
        assert_eq!(names, [("组9", "组9"), ("组10", "组10"), ("", "No Group")]);
        assert_eq!(groups[2].roles, ["x"]);
        let ws =
            json!({ "lead": "pm", "members": [{ "role": "pm" }, { "role": "dev", "slots": 3 }], "brief": " 两个字 " });
        assert_eq!(team_line(&ws, &roles, Lang::Zh), "2 个数字员工（共 4 个分身） · 有负责人");
        let ws1 = json!({ "lead": "dev", "members": [{ "role": "dev" }] });
        assert_eq!(team_line(&ws1, &roles, Lang::En), "Digital employees: 1 · Lead: 开发");
        assert_eq!(brief_status(&ws, Lang::Zh), "3 字");
        assert_eq!(brief_status(&ws1, Lang::En), "Empty");
    }

    #[test]
    fn a_roles_permission_line_reads_in_its_agents_words() {
        let conns = vec![json!({ "id": "c1", "options": { "controls": [
            { "id": "mode", "name": "模式", "permission": true, "default": "full",
              "options": [{ "value": "full", "name": "完全访问" }, { "value": "ro", "name": "只读" }] },
            { "id": "plan", "name": "Plan", "permission": true, "default": false },
            { "id": "model", "name": "模型", "permission": false, "default": "x" },
        ] } })];
        let role = json!({ "connection": "c1", "lead": true, "session": { "mode": "ro" } });
        assert_eq!(perm_label(&role, &conns, Lang::Zh), "只读 · Plan 关闭");
        assert_eq!(perm_label(&json!({ "connection": "c1", "session": {} }), &conns, Lang::En), "完全访问 · Plan Off");
        assert_eq!(perm_label(&json!({ "connection": "gone" }), &conns, Lang::Zh), "");
        assert!(can_lead(&role, &conns) && !can_lead(&json!({ "lead": true, "connection": "" }), &conns));
        let named = vec![json!({ "id": "c1", "name": "Claude Code", "options": conns[0]["options"] })];
        assert_eq!(
            setup_line(&json!({ "connection": "c1", "model": "sonnet", "session": {} }), &named, Lang::Zh),
            "Claude Code · sonnet · 完全访问 · Plan 关闭"
        );
    }

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
