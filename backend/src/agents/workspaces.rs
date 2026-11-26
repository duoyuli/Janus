//! Workspaces: a folder with its shared material and team, and the run team built from one.

use super::*;
use crate::db::Setting;
use serde::{Deserialize, Serialize};

/// A workspace, as stored (`configs` kind `workspace`): a folder, the shared material every
/// session of it reads, and its team.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// The folder, resolved.
    pub path: String,
    /// The role that leads its runs; one of `members`.
    pub lead: String,
    pub members: Vec<WorkspaceMember>,
    /// The shared material (`brief_section`).
    pub brief: String,
}

impl Setting for Workspace {
    const KIND: ConfigKind = ConfigKind::Workspace;
    fn id(&self) -> &str {
        &self.id
    }
}

/// A role in a workspace's team, and how many instances of it a run gets (the lead has one).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WorkspaceMember {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<i64>,
}

impl WorkspaceMember {
    fn slots(&self) -> i64 {
        self.slots.unwrap_or(1).clamp(1, MAX_SLOTS)
    }
}

/// A workspace as the editor sends it (`POST /api/setup/workspaces`).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct WorkspaceInput {
    pub id: String,
    pub name: String,
    pub path: String,
    pub lead: String,
    pub members: Vec<WorkspaceMember>,
    pub brief: String,
}

impl App {
    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        self.db.list::<Workspace>()
    }

    pub fn workspace(&self, id: &str) -> Result<Workspace> {
        self.db.get::<Workspace>(id)?.ok_or_else(|| anyhow!("工作区 {id} 不存在"))
    }

    // @project-doc docs/domains/20261031-agents_and_roles.md#workspaces
    pub fn save_workspace(&self, input: &WorkspaceInput) -> Result<Workspace> {
        let path = PathBuf::from(expand_home(&input.path));
        if !path.is_dir() {
            bail!("目录不存在：{}", path.display());
        }
        let path = path.canonicalize()?;
        let id = match input.id.as_str() {
            "" => new_id("w"),
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        let lead = input.lead.as_str();
        let mut members = vec![];
        for m in &input.members {
            let role = self.role(&m.role)?;
            let slots = if role.id == lead { 1 } else { m.slots() };
            members.push(WorkspaceMember { role: role.id, slots: (slots > 1).then_some(slots) });
        }
        if !members.iter().any(|m| m.role == lead) {
            bail!("请从成员中选择一个负责人");
        }
        let lead_role = self.db.get::<Role>(lead)?.unwrap_or_default();
        if !lead_role.lead {
            bail!("数字员工「{}」没有开启「可以担任负责人」", lead_role.name);
        }
        let bound = self.connection(&lead_role.connection).and_then(|c| Runtime::of(str_of(&c, "base"))).is_ok();
        if !bound {
            bail!("负责人「{}」还没有绑定 Agent", lead_role.name);
        }
        if members.len() < 2 {
            bail!("除了负责人，至少还需要一名成员");
        }
        let name = match input.name.trim() {
            "" => path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".into()),
            n => n.to_string(),
        };
        let brief = input.brief.trim();
        if brief.chars().count() > BRIEF_LIMIT {
            bail!("共享资料最多 {BRIEF_LIMIT} 个字符，现在有 {} 个，请精简", brief.chars().count());
        }
        let ws = Workspace {
            id,
            name,
            path: path.to_string_lossy().into(),
            lead: lead.to_string(),
            members,
            brief: brief.to_string(),
        };
        self.db.put(&ws, now_ms())?;
        Ok(ws)
    }

    /// Delete a workspace with its Loops and its team memory. Its runs stay, with the name and
    /// folder they recorded, and so do its consolidations; the runs its Loops dispatched are the
    /// person's work. A Loop made meanwhile is caught by the second pass, or deletes itself when it
    /// finds its workspace gone (`create_loop`).
    pub async fn delete_workspace(self: &Arc<Self>, id: &str) -> Result<()> {
        self.delete_loops_of(id).await?;
        self.db.delete_config(ConfigKind::Workspace, id)?;
        self.db.delete_workspace_memories(id)?;
        self.delete_loops_of(id).await
    }

    /// Snapshot the environment into the run's team. The lead keeps the fixed member id "lead";
    /// a collaborator role with `slots > 1` expands into multiple independent members that all
    /// record the same `role_id`.
    // @project-doc docs/domains/20261031-agents_and_roles.md#member_snapshot
    pub fn team_for(&self, ws: &Workspace) -> Result<(PathBuf, Vec<Member>)> {
        let mut reserved: std::collections::HashSet<String> = ["lead".to_string()].into_iter().collect();
        for m in &ws.members {
            if m.role != ws.lead && m.slots() <= 1 {
                reserved.insert(if m.role == "lead" { "lead-role".into() } else { m.role.clone() });
            }
        }
        let mut team = vec![];
        // The library as it is now, pinned for every member: every item is for every digital
        // worker. Values are read when a session opens.
        // @project-doc docs/domains/20261116-resource_library.md#delivery
        let library = self.items()?;
        for m in &ws.members {
            let role = self.db.get::<Role>(&m.role)?.ok_or_else(|| anyhow!("数字员工 {} 已被删除", m.role))?;
            let lead = m.role == ws.lead;
            if lead && !role.lead {
                bail!("数字员工「{}」没有开启「可以担任负责人」，请在工作区里换一个负责人", role.name);
            }
            let base = self.role_member(&role, &library, lead)?;
            let slots = if lead { 1 } else { m.slots() as usize };
            let base_id = if role.id == "lead" { "lead-role" } else { role.id.as_str() };
            for k in 1..=slots {
                let (id, name) = if lead {
                    ("lead".to_string(), base.name.clone())
                } else if slots == 1 {
                    (base_id.to_string(), base.name.clone())
                } else {
                    let mut candidate = format!("{base_id}-{k}");
                    let mut n = 2usize;
                    while !reserved.insert(candidate.clone()) {
                        candidate = format!("{base_id}-{k}-{n}");
                        n += 1;
                    }
                    (candidate, format!("{} #{k}", base.name))
                };
                team.push(Member { id, name, ..base.clone() });
            }
        }
        if !team.iter().any(|m| m.lead) {
            bail!("工作区没有负责人");
        }
        Ok((PathBuf::from(&ws.path), team))
    }
}

impl App {
    /// A role as a run member: its agent, model, controls, standing instructions and the library
    /// pinned for the run. The id is the lead's fixed `lead` when `lead` is set, otherwise the
    /// role's id; a workspace team renames the instances of a role with several slots.
    // @project-doc docs/domains/20261031-agents_and_roles.md#member_snapshot
    pub fn role_member(&self, role: &Role, library: &[crate::library::Item], lead: bool) -> Result<Member> {
        let conn_id = role.connection.as_str();
        if conn_id.is_empty() {
            bail!("数字员工「{}」还没有绑定 Agent 连接", role.name);
        }
        let conn = self.connection(conn_id)?;
        let session = member_session(&role.session, &conn);
        let permission = session_label(&session, &conn);
        Ok(Member {
            id: if lead { "lead".into() } else { role.id.clone() },
            name: role.name.clone(),
            desc: role.desc.clone(),
            connection: conn_id.into(),
            lead,
            color: role.color.clone(),
            init: role.init.clone(),
            model: role.model.clone(),
            effort: role.effort.clone(),
            session,
            permission,
            runtime: Runtime::of(str_of(&conn, "base"))?,
            prompt: str_of(&conn, "prompt").into(),
            role_prompt: role.prompt.clone(),
            resources: library.to_vec(),
            role_id: role.id.clone(),
            connection_name: str_of(&conn, "name").into(),
            hires: role.hires,
        })
    }
}

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
                ConfigKind::Connection,
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
        let can = app
            .save_role(&crate::agents::input(
                json!({ "id": "can-lead", "name": "负责人", "connection": c["id"], "lead": true }),
            ))
            .unwrap();
        let cannot = app
            .save_role(&crate::agents::input(
                json!({ "id": "cannot-lead", "name": "成员", "connection": c["id"], "lead": false }),
            ))
            .unwrap();
        let members = json!([{ "role": can.id }, { "role": cannot.id }]);
        let saved = app
            .save_workspace(&crate::agents::input(
                json!({ "name": "W", "path": dir.to_string_lossy(), "members": members, "lead": can.id }),
            ))
            .unwrap();
        assert_eq!(saved.lead, can.id);
        let err = app
            .save_workspace(&crate::agents::input(
                json!({ "name": "W2", "path": dir.to_string_lossy(), "members": members, "lead": cannot.id }),
            ))
            .unwrap_err()
            .to_string();
        assert!(err.contains("没有开启「可以担任负责人」"), "{err}");
        // A workspace whose lead role loses the flag cannot start a run either.
        app.save_role(&crate::agents::input(
            json!({ "id": "can-lead", "name": "负责人", "connection": c["id"], "lead": false }),
        ))
        .unwrap();
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
                ConfigKind::Connection,
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
        app.save_role(&crate::agents::input(
            json!({ "id": "lead-r", "name": "PMO", "connection": "codex-1", "lead": true }),
        ))
        .unwrap();
        app.save_role(&crate::agents::input(json!({ "id": "dev", "name": "研发", "connection": "codex-1" }))).unwrap();
        app.save_role(&crate::agents::input(json!({ "id": "dev-1", "name": "专员", "connection": "codex-1" })))
            .unwrap();
        let ws = app
            .save_workspace(&crate::agents::input(json!({
                "name": "W", "path": dir.to_string_lossy(), "lead": "lead-r",
                "members": [{ "role": "lead-r", "slots": 4 }, { "role": "dev", "slots": 3 }, { "role": "dev-1" }]
            })))
            .unwrap();
        assert!(ws.members[0].slots.is_none(), "lead never stores >1 slots");
        assert_eq!(ws.members[1].slots, Some(3));
        let (_, team) = app.team_for(&ws).unwrap();
        let ids: Vec<&str> = team.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["lead", "dev-1-2", "dev-2", "dev-3", "dev-1"]);
        let devs: Vec<&Member> = team.iter().filter(|m| m.role_id == "dev").collect();
        assert_eq!(devs.len(), 3);
        assert_eq!(devs[0].name, "研发 #1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
