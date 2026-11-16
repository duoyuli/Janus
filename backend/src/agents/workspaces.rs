//! Workspaces: a folder with its shared material and team, and the run team built from one.

use super::*;

impl App {
    pub fn workspaces(&self) -> Result<Vec<Value>> {
        self.db.list_config(WORKSPACE)
    }

    pub fn workspace(&self, id: &str) -> Result<Value> {
        self.db.get_config(WORKSPACE, id)?.ok_or_else(|| anyhow!("执行环境 {id} 不存在"))
    }

    // @project-doc docs/domains/agents_and_roles.md#workspaces
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
        let mut members = vec![];
        for m in body["members"].as_array().cloned().unwrap_or_default() {
            let role = self
                .db
                .get_config(ROLE, str_of(&m, "role"))?
                .ok_or_else(|| anyhow!("数字员工 {} 不存在", str_of(&m, "role")))?;
            members.push(json!({ "role": role["id"] }));
        }
        let lead = str_of(body, "lead");
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

    /// Snapshot the environment into the run's team. The lead keeps the fixed member id "lead".
    pub fn team_for(&self, ws: &Value) -> Result<(PathBuf, Vec<Member>)> {
        let mut team = vec![];
        for m in ws["members"].as_array().cloned().unwrap_or_default() {
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
            let session = role_session(&role, &conn);
            let permission = session_label(&session, &conn);
            let model_label = model_label(&role, &conn);
            // The library items the role uses, pinned as they are now. Values are read when a
            // session opens; an item deleted since the role was saved is left out.
            let resources: Vec<crate::library::Item> = role["resources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|id| self.item(id).ok())
                .collect();
            team.push(Member {
                // "lead" is reserved for whichever role leads this environment.
                id: if lead {
                    "lead".into()
                } else if role["id"] == "lead" {
                    "lead-role".into()
                } else {
                    str_of(&role, "id").into()
                },
                name: str_of(&role, "name").into(),
                role: str_of(&role, "role").into(),
                desc: str_of(&role, "desc").into(),
                runtime: conn_id.into(),
                lead,
                color: str_of(&role, "color").into(),
                init: str_of(&role, "init").into(),
                model: str_of(&role, "model").into(),
                effort: str_of(&role, "effort").into(),
                session,
                permission,
                model_label,
                kind: Runtime::of(str_of(&conn, "base"))?,
                prompt: str_of(&conn, "prompt").into(),
                role_prompt: str_of(&role, "prompt").into(),
                resources,
                role_id: str_of(&role, "id").into(),
                runtime_name: str_of(&conn, "name").into(),
            });
        }
        if !team.iter().any(|m| m.lead) {
            bail!("执行环境没有负责人");
        }
        Ok((PathBuf::from(str_of(ws, "path")), team))
    }
}

pub(super) const WORKSPACE: &str = "workspace";

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
        let c = app.save_connection(&json!({ "id": "codex-1", "kind": "codex", "name": "Codex" })).unwrap();
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
}
