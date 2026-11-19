//! Workspaces: a folder with its shared material and team, and the run team built from one.

use super::*;

impl App {
    pub fn workspaces(&self) -> Result<Vec<Value>> {
        self.db.list_config(WORKSPACE)
    }

    pub fn workspace(&self, id: &str) -> Result<Value> {
        self.db.get_config(WORKSPACE, id)?.ok_or_else(|| anyhow!("执行环境 {id} 不存在"))
    }

    // @project-doc docs/domains/20261031-agents_and_roles.md#workspaces
    pub fn save_workspace(&self, body: &Value) -> Result<Value> {
        let path = PathBuf::from(expand_home(str_of(body, "path")));
        if !path.is_dir() {
            bail!("目录不存在：{}", path.display());
        }
        let path = path.canonicalize()?;
        let id = match str_of(body, "id") {
            "" => new_id("w"),
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        let lead = str_of(body, "lead");
        let mut members = vec![];
        for m in body["members"].as_array().cloned().unwrap_or_default() {
            let role = self
                .db
                .get_config(ROLE, str_of(&m, "role"))?
                .ok_or_else(|| anyhow!("数字员工 {} 不存在", str_of(&m, "role")))?;
            let slots = if role["id"] == lead { 1 } else { m["slots"].as_i64().unwrap_or(1).clamp(1, MAX_SLOTS) };
            let mut entry = json!({ "role": role["id"] });
            if slots > 1 {
                entry["slots"] = json!(slots);
            }
            members.push(entry);
        }
        if !members.iter().any(|m| m["role"] == lead) {
            bail!("请从成员中选择一个负责人");
        }
        let lead_role = self.db.get_config(ROLE, lead)?.unwrap_or_default();
        if lead_role["lead"] != true {
            bail!("数字员工「{}」没有开启「可以担任负责人」", str_of(&lead_role, "name"));
        }
        let leads = self
            .connection(str_of(&lead_role, "connection"))
            .and_then(|c| Runtime::of(str_of(&c, "base")))
            .is_ok_and(Runtime::leads);
        if !leads {
            bail!("负责人只能是 Claude Code 或 Codex 的数字员工（需要 /goal），「{}」不是", str_of(&lead_role, "name"));
        }
        if members.len() < 2 {
            bail!("除了负责人，至少还需要一名成员");
        }
        let name = match str_of(body, "name") {
            "" => path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".into()),
            n => n.to_string(),
        };
        let brief = str_of(body, "brief").trim();
        if brief.chars().count() > BRIEF_LIMIT {
            bail!("共享资料最多 {BRIEF_LIMIT} 个字符，现在有 {} 个，请精简", brief.chars().count());
        }
        let ws = json!({ "id": id, "name": name, "path": path, "lead": lead, "members": members, "brief": brief });
        self.db.put_config(WORKSPACE, &id, &ws, now_ms())?;
        Ok(ws)
    }

    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        self.db.delete_config(WORKSPACE, id)
    }

    /// Snapshot the environment into the run's team. The lead keeps the fixed member id "lead";
    /// a collaborator role with `slots > 1` expands into multiple independent members that all
    /// record the same `role_id`.
    // @project-doc docs/domains/20261031-agents_and_roles.md#member_snapshot
    pub fn team_for(&self, ws: &Value) -> Result<(PathBuf, Vec<Member>)> {
        let raw_members = ws["members"].as_array().cloned().unwrap_or_default();
        let mut reserved: std::collections::HashSet<String> = ["lead".to_string()].into_iter().collect();
        for m in &raw_members {
            if m["role"] != ws["lead"] && m["slots"].as_i64().unwrap_or(1) <= 1 {
                let rid = str_of(m, "role");
                reserved.insert(if rid == "lead" { "lead-role".into() } else { rid.into() });
            }
        }
        let mut team = vec![];
        let everyone: Vec<crate::library::Item> =
            self.items()?.into_iter().filter(|i| i.reach == crate::library::Reach::Everyone).collect();
        for m in raw_members {
            let role = self
                .db
                .get_config(ROLE, str_of(&m, "role"))?
                .ok_or_else(|| anyhow!("数字员工 {} 已被删除", str_of(&m, "role")))?;
            let conn_id = str_of(&role, "connection");
            if conn_id.is_empty() {
                bail!("数字员工「{}」还没有绑定 Agent 连接", str_of(&role, "name"));
            }
            let conn = self.connection(conn_id)?;
            let lead = m["role"] == ws["lead"];
            if lead && role["lead"] != true {
                bail!("数字员工「{}」没有开启「可以担任负责人」，请在工作区里换一个负责人", str_of(&role, "name"));
            }
            let session = member_session(&role, &conn);
            let permission = session_label(&session, &conn);
            // The library items the member uses, pinned as they are now: every item for everyone,
            // and the items kept for some that its role picks. Values are read when a session
            // opens; an item deleted since the role was saved is left out.
            // @project-doc docs/domains/20261116-resource_library.md#reach
            let mut resources: Vec<crate::library::Item> = everyone.clone();
            resources.extend(
                role["resources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(|id| self.item(id).ok())
                    .filter(|i| i.reach == crate::library::Reach::Picked),
            );
            let slots = if lead { 1 } else { m["slots"].as_i64().unwrap_or(1).clamp(1, MAX_SLOTS) as usize };
            let base_id = if role["id"] == "lead" { "lead-role" } else { str_of(&role, "id") };
            let base_name = str_of(&role, "name");
            let kind = Runtime::of(str_of(&conn, "base"))?;
            for k in 1..=slots {
                let (id, name) = if lead {
                    ("lead".to_string(), base_name.to_string())
                } else if slots == 1 {
                    (base_id.to_string(), base_name.to_string())
                } else {
                    let mut candidate = format!("{base_id}-{k}");
                    let mut n = 2usize;
                    while !reserved.insert(candidate.clone()) {
                        candidate = format!("{base_id}-{k}-{n}");
                        n += 1;
                    }
                    (candidate, format!("{base_name} #{k}"))
                };
                team.push(Member {
                    id,
                    name,
                    desc: str_of(&role, "desc").into(),
                    runtime: conn_id.into(),
                    lead,
                    color: str_of(&role, "color").into(),
                    init: str_of(&role, "init").into(),
                    model: str_of(&role, "model").into(),
                    effort: str_of(&role, "effort").into(),
                    session: session.clone(),
                    permission: permission.clone(),
                    kind,
                    prompt: str_of(&conn, "prompt").into(),
                    role_prompt: str_of(&role, "prompt").into(),
                    resources: resources.clone(),
                    role_id: str_of(&role, "id").into(),
                    runtime_name: str_of(&conn, "name").into(),
                });
            }
        }
        if !team.iter().any(|m| m.lead) {
            bail!("执行环境没有负责人");
        }
        Ok((PathBuf::from(str_of(ws, "path")), team))
    }
}

pub(super) const WORKSPACE: &str = "workspace";
pub(super) const MAX_SLOTS: i64 = 8;

/// The most a workspace's shared material may hold: it goes into every session's prompt.
pub(super) const BRIEF_LIMIT: usize = 8000;

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use crate::orchestrator::test_app;

    /// The runtime alone is not enough: the role must also allow being the lead.
    #[test]
    fn only_a_role_that_may_lead_can_be_the_workspace_lead() {
        let dir = std::env::temp_dir().join(format!("janus-workspace-lead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = test_app(&dir);
        // A connection written directly: copying the user's own agent config is not the subject.
        app.db
            .put_config(
                "connection",
                "codex-1",
                &test_connection(
                    "codex-1",
                    "codex",
                    "codex",
                    json!({ "name": "Codex", "options": { "controls": [] } }),
                ),
                now_ms(),
            )
            .unwrap();
        let c = json!({ "id": "codex-1" });
        let can =
            app.save_role(&json!({ "id": "can-lead", "name": "负责人", "connection": c["id"], "lead": true })).unwrap();
        let cannot = app
            .save_role(&json!({ "id": "cannot-lead", "name": "成员", "connection": c["id"], "lead": false }))
            .unwrap();
        let members = json!([{ "role": can["id"] }, { "role": cannot["id"] }]);
        let saved = app
            .save_workspace(
                &json!({ "name": "W", "path": dir.to_string_lossy(), "members": members, "lead": can["id"] }),
            )
            .unwrap();
        assert_eq!(saved["lead"], can["id"]);
        let err = app
            .save_workspace(
                &json!({ "name": "W2", "path": dir.to_string_lossy(), "members": members, "lead": cannot["id"] }),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("没有开启「可以担任负责人」"), "{err}");
        // A workspace whose lead role loses the flag cannot start a run either.
        app.save_role(&json!({ "id": "can-lead", "name": "负责人", "connection": c["id"], "lead": false })).unwrap();
        let err = app.team_for(&saved).unwrap_err().to_string();
        assert!(err.contains("没有开启「可以担任负责人」"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collaborator_slots_expand_with_shared_role_id_and_avoid_id_collisions() {
        let dir = std::env::temp_dir().join(format!("janus-workspace-slots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = test_app(&dir);
        app.db
            .put_config(
                "connection",
                "codex-1",
                &test_connection(
                    "codex-1",
                    "codex",
                    "codex",
                    json!({ "name": "Codex", "options": { "controls": [] } }),
                ),
                now_ms(),
            )
            .unwrap();
        app.save_role(&json!({ "id": "lead-r", "name": "PMO", "connection": "codex-1", "lead": true })).unwrap();
        app.save_role(&json!({ "id": "dev", "name": "研发", "connection": "codex-1" })).unwrap();
        app.save_role(&json!({ "id": "dev-1", "name": "专员", "connection": "codex-1" })).unwrap();
        let ws = app
            .save_workspace(&json!({
                "name": "W", "path": dir.to_string_lossy(), "lead": "lead-r",
                "members": [{ "role": "lead-r", "slots": 4 }, { "role": "dev", "slots": 3 }, { "role": "dev-1" }]
            }))
            .unwrap();
        assert!(ws["members"][0]["slots"].is_null(), "lead never stores >1 slots");
        assert_eq!(ws["members"][1]["slots"], 3);
        let (_, team) = app.team_for(&ws).unwrap();
        let ids: Vec<&str> = team.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["lead", "dev-1-2", "dev-2", "dev-3", "dev-1"]);
        let devs: Vec<&Member> = team.iter().filter(|m| m.role_id == "dev").collect();
        assert_eq!(devs.len(), 3);
        assert_eq!(devs[0].name, "研发 #1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
