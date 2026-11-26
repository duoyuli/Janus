//! A digital employee as a file: what the default roles ship as and what export writes. One
//! Markdown file per employee, `<id>.md`: front matter with its name, duty, group and look, then
//! its instructions. The same checks hold for the employees a chat proposes (`RoleFile::checked`),
//! and those are made the same way (`make_roles`).
// @project-doc docs/domains/20261031-agents_and_roles.md#role_files

use super::*;
use crate::front_matter;
use roles::{Role, RoleInput};
use serde::{Deserialize, Serialize};

/// What a role file says about one digital employee.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleFile {
    /// Empty for a proposed employee: making it takes one from its name.
    pub id: String,
    pub name: String,
    pub desc: String,
    /// Empty for none.
    pub group: String,
    pub color: String,
    pub init: String,
    pub lead: bool,
    pub prompt: String,
}

/// The longest group name: it heads a list and names an export's folder.
pub const MAX_GROUP: usize = 40;

impl RoleFile {
    /// Read a role file: `---`, `key: value` lines (`name` and the instructions after the closing
    /// `---` are required), `---`, then the instructions.
    pub fn parse(text: &str) -> Result<RoleFile, String> {
        let text = text.replace("\r\n", "\n");
        if front_matter::yaml(&text).is_none() {
            return Err("开头没有 --- 包起来的元数据".into());
        }
        let get = |key: &str| front_matter::field(&text, key).unwrap_or_default();
        let name = get("name");
        if name.is_empty() {
            return Err("元数据里没有 name".into());
        }
        let desc = get("desc");
        let lead = match get("lead").as_str() {
            "" | "false" => false,
            "true" => true,
            other => return Err(format!("lead 应当是 true 或 false，而不是 {other}")),
        };
        let prompt = front_matter::body(&text).to_string();
        RoleFile {
            id: get("id"),
            name,
            desc,
            group: get("group"),
            color: get("color"),
            init: get("init"),
            lead,
            prompt,
        }
        .checked()
    }

    /// The same, checked the way every source of digital employees is (a file, a chat's
    /// proposal): a name and instructions, a `#rrggbb` color or none, a group name that fits.
    pub fn checked(self) -> Result<RoleFile, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("没有名字".into());
        }
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("没有指令正文".into());
        }
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(format!("指令超过 {} KB", MAX_PROMPT_BYTES / 1024));
        }
        let color = Some(self.color).filter(|c| is_hex_color(c)).unwrap_or_default();
        let group = group_name(&self.group).map_err(|e| e.to_string())?;
        Ok(RoleFile {
            name,
            prompt,
            color,
            group,
            desc: self.desc.trim().to_string(),
            init: self.init.trim().to_string(),
            ..self
        })
    }

    /// The file for a saved digital employee; `parse` reads it back the same.
    pub fn of(role: &Role) -> RoleFile {
        RoleFile {
            id: role.id.clone(),
            name: role.name.clone(),
            desc: role.desc.clone(),
            group: role.group.clone(),
            color: role.color.clone(),
            init: role.init.clone(),
            lead: role.lead,
            prompt: role.prompt.clone(),
        }
    }

    pub fn render(&self) -> String {
        let q = front_matter::quote;
        let mut head =
            vec![format!("id: {}", self.id), format!("name: {}", q(&self.name)), format!("desc: {}", q(&self.desc))];
        if !self.group.is_empty() {
            head.push(format!("group: {}", q(&self.group)));
        }
        head.extend([
            format!("color: {}", q(&self.color)),
            format!("init: {}", q(&self.init)),
            format!("lead: {}", self.lead),
        ]);
        format!("---\n{}\n---\n\n{}\n", head.join("\n"), self.prompt.trim())
    }
}

fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

/// The longest instructions a digital employee is given: they go into every session it opens.
const MAX_PROMPT_BYTES: usize = 256 * 1024;

impl App {
    /// Save each of `roles` as a new digital employee on the agent `connection` (empty: none
    /// yet). Its id is the one it names, or one made from its name; an id already taken gets
    /// `-2`, `-3`, …: making never replaces anyone. What it may do beyond its instructions
    /// (`hires`) is never taken from it.
    pub fn make_roles(&self, roles: impl IntoIterator<Item = RoleFile>, connection: &str) -> Result<Vec<Role>> {
        if !connection.is_empty() {
            self.connection(connection)?;
        }
        let mut made = vec![];
        for r in roles {
            let base = [r.id.as_str(), r.name.as_str()]
                .into_iter()
                .find_map(|raw| crate::text::slug(raw, 36))
                .unwrap_or_else(|| new_id("r"));
            let input = RoleInput {
                id: self.free_role_id(&base)?,
                name: r.name,
                desc: r.desc,
                group: r.group,
                prompt: r.prompt,
                connection: connection.into(),
                lead: r.lead,
                color: r.color,
                init: r.init,
                ..Default::default()
            };
            made.push(self.save_role(&input)?);
        }
        Ok(made)
    }

    /// `base`, or `base-2`, `base-3`, … when a digital employee already has it.
    fn free_role_id(&self, base: &str) -> Result<String> {
        for n in 1.. {
            let id = if n == 1 { base.to_string() } else { format!("{base}-{n}") };
            if self.db.get::<Role>(&id)?.is_none() {
                return Ok(id);
            }
        }
        unreachable!()
    }

    /// The file for digital employee `id` (`<id>.md`), as export writes it.
    pub fn role_file(&self, id: &str) -> Result<String> {
        Ok(RoleFile::of(&self.role(id)?).render())
    }

    /// Give every digital employee of group `from` the name `to`. Returns how many moved.
    pub fn rename_role_group(&self, from: &str, to: &str) -> Result<usize> {
        let to = group_name(to)?;
        if from.is_empty() || to.is_empty() {
            bail!("分组需要一个名字");
        }
        let mut moved = 0;
        for mut role in self.db.list::<Role>()?.into_iter().filter(|r| r.group == from) {
            role.group = to.clone();
            self.db.put(&role, now_ms())?;
            moved += 1;
        }
        Ok(moved)
    }

    /// Put every digital employee of `group` (empty: those in no group) on the agent
    /// `connection`, as if each were saved with it: one already on it keeps its model and
    /// options, the others take the agent's defaults.
    pub fn bind_role_group(&self, group: &str, connection: &str) -> Result<Vec<Role>> {
        self.connection(connection)?;
        let mut out = vec![];
        for role in self.db.list::<Role>()?.into_iter().filter(|r| r.group == group) {
            let same = role.connection == connection;
            let input = RoleInput {
                connection: connection.into(),
                model: if same { role.model.clone() } else { String::new() },
                effort: if same { role.effort.clone() } else { String::new() },
                session: if same { role.session.clone() } else { json!({}) },
                ..RoleInput::from(&role)
            };
            out.push(self.save_role(&input)?);
        }
        Ok(out)
    }
}

/// A group name as saved: one line, trimmed, at most `MAX_GROUP` characters.
pub fn group_name(raw: &str) -> Result<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > MAX_GROUP {
        bail!("分组名最多 {MAX_GROUP} 个字");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::test_app;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-role-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_role_file_reads_back_as_written() {
        let file = RoleFile {
            id: "counsel".into(),
            name: "法务 \"顾问\"".into(),
            desc: "审合同: 找风险".into(),
            group: "律所".into(),
            color: "#11845a".into(),
            init: "法".into(),
            lead: true,
            prompt: "逐条读合同。\n\n- 标出风险".into(),
        };
        assert_eq!(RoleFile::parse(&file.render()).unwrap(), file);
        assert!(RoleFile::parse("no front matter").is_err());
        assert!(RoleFile::parse("---\ndesc: d\n---\nbody").unwrap_err().contains("name"));
        assert!(RoleFile::parse("---\nname: n\n---\n\n").unwrap_err().contains("指令"));
        assert!(RoleFile::parse("---\nname: n\nlead: yes\n---\nbody").unwrap_err().contains("lead"));
        let long = format!("---\nname: n\ngroup: {}\n---\nbody", "组".repeat(41));
        assert!(RoleFile::parse(&long).unwrap_err().contains("分组"), "a group too long to save is refused");
    }

    #[test]
    fn a_group_is_renamed_and_put_on_one_agent_together() {
        let home = dir("groups");
        let app = test_app(&home);
        for (id, group) in [("a", "旧组"), ("b", "旧组"), ("c", "")] {
            app.save_role(&crate::agents::input(json!({ "id": id, "name": id, "group": group }))).unwrap();
        }
        assert!(app.save_role(&crate::agents::input(json!({ "name": "x", "group": "长".repeat(41) }))).is_err());
        assert_eq!(app.rename_role_group("旧组", "  新\n组 ").unwrap(), 2);
        assert_eq!(app.role("a").unwrap().group, "新 组");
        assert!(app.rename_role_group("新 组", " ").is_err(), "a group keeps a name");
        assert!(app.bind_role_group("新 组", "missing").is_err());
        app.db
            .put_config(
                ConfigKind::Connection,
                "cc",
                &crate::agents::test_connection("cc", "claude", "claude", json!({})),
                1,
            )
            .unwrap();
        let bound = app.bind_role_group("新 组", "cc").unwrap();
        assert_eq!(bound.len(), 2);
        assert!(bound.iter().all(|r| r.connection == "cc"));
        assert_eq!(app.role("c").unwrap().connection, "", "other groups are left alone");
        let _ = std::fs::remove_dir_all(&home);
    }
}
