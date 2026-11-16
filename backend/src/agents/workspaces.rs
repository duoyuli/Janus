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
                .ok_or_else(|| anyhow!("角色 {} 不存在", str_of(&m, "role")))?;
            members.push(json!({ "role": role["id"] }));
        }
        let lead = str_of(body, "lead");
        if !members.iter().any(|m| m["role"] == lead) {
            bail!("请从成员中选择一个主控");
        }
        let lead_role = self.db.get_config(ROLE, lead)?.unwrap_or_default();
        let lead_kind = self
            .connection(str_of(&lead_role, "connection"))
            .map(|c| str_of(&c, "base").to_string())
            .unwrap_or_default();
        if !crate::orchestrator::lead_capable(&lead_kind) {
            bail!("主控只能是 Claude Code 或 Codex 的角色（需要 /goal），「{}」不是", str_of(&lead_role, "name"));
        }
        if members.len() < 2 {
            bail!("除了主控，至少还需要一名成员");
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
                .ok_or_else(|| anyhow!("角色 {} 已被删除", str_of(&m, "role")))?;
            let conn_id = str_of(&role, "connection");
            if conn_id.is_empty() {
                bail!("角色「{}」还没有绑定 Agent 连接", str_of(&role, "name"));
            }
            let conn = self.connection(conn_id)?;
            let lead = m["role"] == ws["lead"];
            let session = role_session(&role, &conn);
            let permission = session_label(&session, &conn);
            let model_label = model_label(&role, &conn);
            // The role's hand-written servers, then the library resources it mounts. Definitions
            // are snapshotted; the values are read when a session opens (`attach_mcp_secrets`).
            let mut mcp = role["mcp"].as_array().cloned().unwrap_or_default();
            let mut mcp_refs = vec![];
            for id in role["mcpRefs"].as_array().cloned().unwrap_or_default() {
                let Some(id) = id.as_str() else { continue };
                if let Some(info) = self.mcp_info(id) {
                    mcp_refs.push(id.to_string());
                    mcp.push(info);
                }
            }
            check_mcp_names(str_of(&role, "name"), &mcp)?;
            let mut member = Member {
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
                kind: str_of(&conn, "base").into(),
                prompt: str_of(&conn, "prompt").into(),
                role_prompt: str_of(&role, "prompt").into(),
                skills: role["skills"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|id| id.as_str().and_then(|id| self.skill_info(id)))
                    .collect(),
                mcp,
                mcp_refs,
                mcp_env: json!({}),
                mcp_headers: json!({}),
                role_id: str_of(&role, "id").into(),
                runtime_name: str_of(&conn, "name").into(),
            };
            self.attach_mcp_secrets(&mut member);
            team.push(member);
        }
        if !team.iter().any(|m| m.lead) {
            bail!("执行环境没有主控");
        }
        Ok((PathBuf::from(str_of(ws, "path")), team))
    }
}

pub(super) const WORKSPACE: &str = "workspace";

/// The most a workspace's shared material may hold: it goes into every session's prompt.
pub(super) const BRIEF_LIMIT: usize = 8000;
