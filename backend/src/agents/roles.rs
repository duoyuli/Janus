//! Roles: saving and deleting them, the library items they use, and the default roles a first
//! start creates.

use super::*;
use crate::db::Setting;
use serde::{Deserialize, Serialize};

/// A digital employee, as stored (`configs` kind `role`).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Role {
    pub id: String,
    pub name: String,
    /// What it is responsible for, shown to the lead.
    pub desc: String,
    /// The group it is listed under, to manage people by team or trade; empty for none. Only
    /// the app shows it: no session is told.
    pub group: String,
    /// Its standing instructions.
    pub prompt: String,
    /// The agent connection it runs on; empty until one is bound.
    pub connection: String,
    pub model: String,
    pub effort: String,
    /// The session controls it chose from what its agent declares, by control id.
    pub session: Value,
    /// Whether it may lead a workspace.
    pub lead: bool,
    /// Whether, in a chat, it may propose new digital employees for the person to create
    /// (`propose_employees`). Only the person turns it on, in the editor: a role file never does.
    pub hires: bool,
    pub color: String,
    pub init: String,
    /// The hash of its picture (`data/roles/<id>/avatar.png`); empty shows the initials.
    pub avatar: String,
    /// The default it came from and the text it was given, for a default role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SeedMark>,
}

impl Setting for Role {
    const KIND: ConfigKind = ConfigKind::Role;
    fn id(&self) -> &str {
        &self.id
    }
}

/// Which default a role came from, and the hashes of the text and picture it was given, so a
/// later build can tell what the person changed (`sync_seeds`).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SeedMark {
    pub id: String,
    pub desc: String,
    pub prompt: String,
    /// Default roles have shipped pictures since data version 16; one given none has no hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// A role as the editor sends it (`POST /api/setup/roles`): what the person sets. Where a default
/// role came from and its picture are Janus's records, not something the editor sends.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RoleInput {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub group: String,
    pub prompt: String,
    pub connection: String,
    pub model: String,
    pub effort: String,
    pub session: Value,
    pub lead: bool,
    pub hires: bool,
    pub color: String,
    pub init: String,
}

impl From<&Role> for RoleInput {
    /// The role as its editor would send it back unchanged.
    fn from(r: &Role) -> RoleInput {
        RoleInput {
            id: r.id.clone(),
            name: r.name.clone(),
            desc: r.desc.clone(),
            group: r.group.clone(),
            prompt: r.prompt.clone(),
            connection: r.connection.clone(),
            model: r.model.clone(),
            effort: r.effort.clone(),
            session: r.session.clone(),
            lead: r.lead,
            hires: r.hires,
            color: r.color.clone(),
            init: r.init.clone(),
        }
    }
}

impl Role {
    /// A text field of the role by name, for the default text it may take (`desc` or `prompt`).
    fn field_mut(&mut self, field: &str) -> &mut String {
        if field == "desc" { &mut self.desc } else { &mut self.prompt }
    }
}

impl SeedMark {
    fn field(&self, field: &str) -> &str {
        if field == "desc" { &self.desc } else { &self.prompt }
    }
    fn field_mut(&mut self, field: &str) -> &mut String {
        if field == "desc" { &mut self.desc } else { &mut self.prompt }
    }
}

impl App {
    /// Roles as the app sees them: a default role the person edited also carries `seedUpdates`,
    /// the newer default text of the edited fields.
    pub fn roles(&self) -> Result<Vec<Value>> {
        let mut out = vec![];
        for r in self.db.list::<Role>()? {
            let updates = seed_updates(&r);
            let mut v = serde_json::to_value(&r)?;
            if !updates.is_empty() {
                v["seedUpdates"] = Value::Object(updates);
            }
            out.push(v);
        }
        Ok(out)
    }

    /// The role `id`.
    pub fn role(&self, id: &str) -> Result<Role> {
        self.db.get::<Role>(id)?.ok_or_else(|| anyhow!("数字员工 {id} 不存在"))
    }

    pub fn save_role(&self, input: &RoleInput) -> Result<Role> {
        let name = input.name.trim();
        if name.is_empty() {
            bail!("数字员工需要一个名字");
        }
        let id = match input.id.as_str() {
            "" => new_id("r"),
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        // The session controls this role chose, checked against what its agent declares.
        let session = match input.connection.as_str() {
            "" => json!({}),
            c => role_session(&input.session, &self.connection(c)?),
        };
        let init = match input.init.as_str() {
            "" => name.chars().filter(|c| !c.is_whitespace()).take(2).collect::<String>().to_uppercase(),
            i => i.to_string(),
        };
        let prev = self.db.get::<Role>(&id)?;
        let role = Role {
            id: id.clone(),
            name: name.to_string(),
            desc: input.desc.clone(),
            group: super::role_files::group_name(&input.group)?,
            prompt: input.prompt.trim().to_string(),
            connection: input.connection.clone(),
            model: input.model.clone(),
            effort: input.effort.clone(),
            session,
            lead: input.lead,
            hires: input.hires,
            color: if input.color.is_empty() { "#64748b".into() } else { input.color.clone() },
            init,
            // The picture changes through `set_role_avatar`.
            avatar: prev.as_ref().map(|p| p.avatar.clone()).unwrap_or_default(),
            seed: prev.and_then(|p| p.seed),
        };
        self.db.put(&role, now_ms())?;
        Ok(role)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        if let Some(ws) = self.workspaces()?.into_iter().find(|w| w.members.iter().any(|m| m.role == id)) {
            bail!("工作区「{}」正在使用这个角色，请先移除", ws.name);
        }
        check_id(id)?;
        self.db.delete_config(ConfigKind::Role, id)?;
        for path in self.cfg.layout.role_paths(id) {
            crate::layout::remove(&path);
        }
        Ok(())
    }

    /// Give a digital employee its own picture (`png`: a PNG, base64, the way the app sends it
    /// after cropping it square), or take it back to its initials (`None`). The record keeps the
    /// picture's hash, so the app knows there is one and when it changed.
    // @project-doc docs/domains/20261031-agents_and_roles.md#avatars
    pub fn set_role_avatar(&self, id: &str, png: Option<&str>) -> Result<Role> {
        use base64::Engine;
        check_id(id)?;
        let mut role = self.role(id)?;
        let file = self.cfg.layout.role_avatar(id);
        match png {
            Some(data) => {
                let data = data.split_once(";base64,").map_or(data, |(_, d)| d).trim();
                let bytes = base64::engine::general_purpose::STANDARD.decode(data).context("头像不是有效的 base64")?;
                self.put_role_avatar(role, &bytes)
            }
            None => {
                crate::layout::remove(&file);
                role.avatar = String::new();
                self.db.put(&role, now_ms())?;
                Ok(role)
            }
        }
    }

    /// Save `bytes`, a PNG, as the picture of `role`.
    pub(super) fn put_role_avatar(&self, mut role: Role, bytes: &[u8]) -> Result<Role> {
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("头像要是 PNG 图片");
        }
        if bytes.len() > MAX_AVATAR_BYTES {
            bail!("头像超过 {} KB", MAX_AVATAR_BYTES / 1024);
        }
        write_avatar(&self.cfg.layout.role_avatar(&role.id), bytes)?;
        role.avatar = avatar_hash(bytes);
        self.db.put(&role, now_ms())?;
        Ok(role)
    }

    /// The digital employee's picture, if it has one.
    pub fn role_avatar(&self, id: &str) -> Result<Option<Vec<u8>>> {
        check_id(id)?;
        match std::fs::read(self.cfg.layout.role_avatar(id)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Claude Code runs `/goal` only in a workspace it trusts. The lead's agent has its own
    /// config directory, so record the trust there (never in the user's own Claude config).
    pub fn trust_claude_workspace(&self, id: &str, path: &Path) -> Result<()> {
        let file = self.agent_config(id).join(".claude.json");
        let mut cfg: Value = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();
        if cfg["projects"][&key]["hasTrustDialogAccepted"] == true {
            return Ok(());
        }
        if !cfg["projects"].is_object() {
            cfg["projects"] = json!({});
        }
        if !cfg["projects"][&key].is_object() {
            cfg["projects"][&key] = json!({});
        }
        cfg["projects"][&key]["hasTrustDialogAccepted"] = json!(true);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, serde_json::to_vec_pretty(&cfg)?)?;
        Ok(())
    }

    /// The default roles. A first start creates them, each remembering in `seed` which default
    /// it came from and the hash of the text it was given. On later starts a role's `desc` or
    /// `prompt` the person never edited moves to the text this build ships; an edited one stays as
    /// it is, and the app offers the new text (`seed_updates`). A default role the person deleted
    /// stays deleted.
    // @project-doc docs/domains/20261031-agents_and_roles.md#product_launch_roles
    pub fn sync_seeds(&self) -> Result<()> {
        if self.db.get_config(ConfigKind::Meta, "seeded")?.is_none() {
            for r in SEEDS.iter() {
                let input = RoleInput {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    desc: r.desc.clone(),
                    group: r.group.clone(),
                    prompt: r.prompt.clone(),
                    lead: r.lead,
                    hires: r.hires,
                    color: r.color.clone(),
                    init: r.init.clone(),
                    ..Default::default()
                };
                let mut role = self.save_role(&input)?;
                role.seed = Some(SeedMark {
                    id: r.id.clone(),
                    desc: text_hash(&r.desc),
                    prompt: text_hash(&r.prompt),
                    avatar: None,
                });
                self.ship_avatar(r, &mut role)?;
                self.db.put(&role, now_ms())?;
            }
            return self.db.put_config(ConfigKind::Meta, "seeded", &json!(true), now_ms());
        }
        for (r, mut role) in self.seeded_roles()? {
            let mut changed = false;
            for (field, text) in [("desc", r.desc.as_str()), ("prompt", r.prompt.as_str())] {
                let shipped = text_hash(text);
                let seed = role.seed.as_mut().expect("a seeded role");
                let given = seed.field(field).to_string();
                if given == shipped {
                    continue;
                }
                let now = text_hash(role.field_mut(field));
                if now == given || now == shipped {
                    *role.field_mut(field) = text.to_string();
                    *role.seed.as_mut().expect("a seeded role").field_mut(field) = shipped;
                    changed = true;
                }
            }
            // The picture follows the same rule: one the person never changed (or one from before
            // default roles had pictures) becomes this build's; one they chose or removed stays.
            let shipped = avatar_hash(r.avatar);
            let given = role.seed.as_ref().and_then(|s| s.avatar.clone()).unwrap_or_default();
            if given != shipped && (role.avatar == given || role.avatar == shipped) {
                self.ship_avatar(r, &mut role)?;
                changed = true;
            }
            if changed {
                self.db.put(&role, now_ms())?;
            }
        }
        Ok(())
    }

    /// Give a default role the picture this build ships, recording it as the one it was given.
    fn ship_avatar(&self, r: &RoleSeed, role: &mut Role) -> Result<()> {
        write_avatar(&self.cfg.layout.role_avatar(&r.id), r.avatar)?;
        let hash = avatar_hash(r.avatar);
        role.avatar = hash.clone();
        if let Some(seed) = role.seed.as_mut() {
            seed.avatar = Some(hash);
        }
        Ok(())
    }

    /// The default roles that still exist, each with the default it came from.
    fn seeded_roles(&self) -> Result<Vec<(&'static RoleSeed, Role)>> {
        Ok(SEEDS
            .iter()
            .filter_map(|r| {
                let role = self.db.get::<Role>(&r.id).ok().flatten()?;
                role.seed.as_ref().is_some_and(|s| s.id == r.id).then_some((r, role))
            })
            .collect())
    }

    /// Adopt (`adopt: true`) or decline the newer default text of one field of a default role the
    /// person edited. Either way the role stops offering it until a later build ships another.
    pub fn seed_choice(&self, id: &str, field: &str, adopt: bool) -> Result<Role> {
        let (r, mut role) = self
            .seeded_roles()?
            .into_iter()
            .find(|(r, _)| r.id == id)
            .ok_or_else(|| anyhow!("{id} 不是默认数字员工"))?;
        let text = match field {
            "desc" => r.desc.as_str(),
            "prompt" => r.prompt.as_str(),
            _ => bail!("只能更新职责（desc）或指令（prompt）"),
        };
        if adopt {
            *role.field_mut(field) = text.to_string();
        }
        *role.seed.as_mut().expect("a seeded role").field_mut(field) = text_hash(text);
        self.db.put(&role, now_ms())?;
        Ok(role)
    }
}

/// The newer default text a default role's edited fields could take, `{field: text}`; empty when
/// there is none. Shown in the role editor.
fn seed_updates(role: &Role) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Some(seed) = &role.seed else { return out };
    if let Some(r) = SEEDS.iter().find(|r| seed.id == r.id) {
        for (field, text, now) in [("desc", &r.desc, &role.desc), ("prompt", &r.prompt, &role.prompt)] {
            let shipped = text_hash(text);
            if seed.field(field) != shipped && text_hash(now) != shipped {
                out.insert(field.into(), json!(text));
            }
        }
    }
    out
}

/// The hash a default text is remembered by: the first 16 hex digits of its SHA-256.
/// A picture is at most this large: the app sends 256×256 PNGs, far smaller.
pub(super) const MAX_AVATAR_BYTES: usize = 1024 * 1024;

/// Write a picture whole: a new file renamed over the old one.
fn write_avatar(file: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(file.parent().context("头像没有所在目录")?)?;
    let tmp = file.with_extension("png.part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

fn avatar_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn text_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A default role, created on a first start with no agent bound yet.
pub(crate) struct RoleSeed {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) desc: String,
    pub(crate) group: String,
    pub(crate) color: String,
    pub(crate) init: String,
    pub(crate) lead: bool,
    /// Whether it proposes digital employees in a chat; a default role's file says so with
    /// `hires: true`, which only default roles are read for.
    pub(crate) hires: bool,
    pub(crate) prompt: String,
    /// Its picture, a square PNG.
    pub(crate) avatar: &'static [u8],
}

/// The default roles, two files each in `backend/roles/`: `<id>.md` with the metadata in a `---`
/// header and the role's instructions as the body, and `<id>.png`, its picture (256×256). They
/// are built into Janus,
/// in this order: the 产品研发 group, a PMO lead and the specialists (research and market, PRD,
/// engineering, QA) a default workspace starts with; then the 对话 group, 老友 to talk something
/// through with in a chat and 招聘顾问, who designs digital employees in one.
const ROLE_FILES: &[(&str, &str, &[u8])] = &[
    ("pmo", include_str!("../../roles/pmo.md"), include_bytes!("../../roles/pmo.png")),
    ("marketing", include_str!("../../roles/marketing.md"), include_bytes!("../../roles/marketing.png")),
    (
        "product-manager",
        include_str!("../../roles/product-manager.md"),
        include_bytes!("../../roles/product-manager.png"),
    ),
    ("developer", include_str!("../../roles/developer.md"), include_bytes!("../../roles/developer.png")),
    ("qa", include_str!("../../roles/qa.md"), include_bytes!("../../roles/qa.png")),
    ("friend", include_str!("../../roles/friend.md"), include_bytes!("../../roles/friend.png")),
    ("recruiter", include_str!("../../roles/recruiter.md"), include_bytes!("../../roles/recruiter.png")),
];

// @project-doc docs/domains/20261031-agents_and_roles.md#product_launch_roles
pub(crate) static SEEDS: std::sync::LazyLock<Vec<RoleSeed>> = std::sync::LazyLock::new(|| {
    ROLE_FILES
        .iter()
        .map(|(file, text, avatar)| {
            RoleSeed::parse(text, avatar).unwrap_or_else(|err| panic!("backend/roles/{file}.md：{err}"))
        })
        .collect()
});

impl RoleSeed {
    /// A role file (`RoleFile`) that also names its `id`, `desc`, `color` and `init`: a default
    /// role leaves nothing to fill in.
    fn parse(text: &str, avatar: &'static [u8]) -> Result<RoleSeed, String> {
        let f = super::role_files::RoleFile::parse(text)?;
        for (key, value) in [("id", &f.id), ("desc", &f.desc), ("color", &f.color), ("init", &f.init)] {
            if value.is_empty() {
                return Err(format!("缺少 {key}"));
            }
        }
        let hires = crate::front_matter::field(text, "hires").is_some_and(|v| v == "true");
        Ok(RoleSeed {
            id: f.id,
            name: f.name,
            desc: f.desc,
            group: f.group,
            color: f.color,
            init: f.init,
            lead: f.lead,
            hires,
            prompt: f.prompt,
            avatar,
        })
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;
    use crate::orchestrator::{App, test_app};
    use std::sync::Arc;

    fn app(name: &str) -> (Arc<App>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("janus-seed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (test_app(&dir), dir)
    }

    fn role(app: &App, id: &str) -> Option<Value> {
        app.db.get_config(ConfigKind::Role, id).unwrap()
    }

    /// A digital employee's picture: only a PNG of a sane size is taken; it survives editing the
    /// role, goes back to the initials when removed, and goes with the role.
    #[test]
    fn a_role_keeps_its_own_picture() {
        use base64::Engine;
        let (app, dir) = app("avatar");
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"rest of the picture"].concat();
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        app.save_role(&crate::agents::input(json!({ "id": "pic", "name": "画像" }))).unwrap();
        assert_eq!(role(&app, "pic").unwrap()["avatar"], "", "a new role shows its initials");
        assert!(app.role_avatar("pic").unwrap().is_none());

        assert!(app.set_role_avatar("pic", Some(&b64(b"GIF89a not a png"))).is_err(), "only PNG");
        assert!(app.set_role_avatar("pic", Some(&b64(&vec![0x89; MAX_AVATAR_BYTES + 1]))).is_err(), "not too large");
        assert!(app.set_role_avatar("nobody", Some(&b64(&png))).is_err(), "only an existing role");
        assert!(app.set_role_avatar("../x", Some(&b64(&png))).is_err(), "an id that leaves its folder");

        let saved = app.set_role_avatar("pic", Some(&format!("data:image/png;base64,{}", b64(&png)))).unwrap();
        assert_eq!(saved.avatar, avatar_hash(&png));
        assert_eq!(app.role_avatar("pic").unwrap().as_deref(), Some(png.as_slice()));
        app.save_role(&crate::agents::input(json!({ "id": "pic", "name": "改了名", "avatar": "" }))).unwrap();
        assert_eq!(role(&app, "pic").unwrap()["avatar"], json!(avatar_hash(&png)), "editing keeps the picture");

        assert_eq!(app.set_role_avatar("pic", None).unwrap().avatar, "");
        assert!(app.role_avatar("pic").unwrap().is_none() && !app.cfg.layout.role_avatar("pic").exists());
        app.set_role_avatar("pic", Some(&b64(&png))).unwrap();
        app.delete_role("pic").unwrap();
        assert!(app.cfg.layout.role_paths("pic").iter().all(|p| !p.exists()), "the picture goes with the role");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Default roles come with their pictures. A later build's picture replaces one the person never
    /// changed (and is given to roles seeded before pictures existed); one they removed or replaced
    /// with their own stays as they left it.
    #[test]
    fn default_roles_follow_their_shipped_pictures() {
        use base64::Engine;
        let (app, dir) = app("seed-avatars");
        app.sync_seeds().unwrap();
        for r in SEEDS.iter() {
            assert_eq!(role(&app, &r.id).unwrap()["avatar"], json!(avatar_hash(r.avatar)), "{}", r.id);
            assert_eq!(app.role_avatar(&r.id).unwrap().as_deref(), Some(r.avatar));
        }
        // As if seeded before pictures: no picture, nothing recorded as given.
        let mut old = role(&app, "developer").unwrap();
        old["avatar"] = json!("");
        old["seed"].as_object_mut().unwrap().remove("avatar");
        app.db.put_config(ConfigKind::Role, "developer", &old, 1).unwrap();
        std::fs::remove_file(app.cfg.layout.role_avatar("developer")).unwrap();
        // One the person took back to initials, one they gave their own picture.
        app.set_role_avatar("qa", None).unwrap();
        let mine = [b"\x89PNG\r\n\x1a\n".as_slice(), b"mine"].concat();
        app.set_role_avatar("pmo", Some(&base64::engine::general_purpose::STANDARD.encode(&mine))).unwrap();

        app.sync_seeds().unwrap();
        let shipped = |id: &str| avatar_hash(SEEDS.iter().find(|r| r.id == id).unwrap().avatar);
        assert_eq!(
            role(&app, "developer").unwrap()["avatar"],
            json!(shipped("developer")),
            "an older role gets its picture"
        );
        assert_eq!(role(&app, "qa").unwrap()["avatar"], "", "a removed picture stays removed");
        assert_eq!(app.role_avatar("pmo").unwrap(), Some(mine), "the person's own picture stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every role file in `backend/roles/` is built in and reads: its id is its file name, ids
    /// are unique, and a file that breaks the format says what is wrong.
    #[test]
    fn every_role_file_is_built_in() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("roles");
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".md"))
            .collect();
        files.sort();
        let mut listed: Vec<String> = ROLE_FILES.iter().map(|(id, _, _)| format!("{id}.md")).collect();
        listed.sort();
        assert_eq!(files, listed, "add each new file to ROLE_FILES in roles.rs");
        for ((file, _, _), r) in ROLE_FILES.iter().zip(SEEDS.iter()) {
            assert_eq!(r.id, *file, "the id is the file's name");
            check_id(&r.id).unwrap();
        }
        let ids: std::collections::HashSet<_> = SEEDS.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), SEEDS.len());
        assert!(RoleSeed::parse("---\nid: x\n---\n\n指令", b"").err().is_some_and(|e| e.contains("name")));
        assert!(
            RoleSeed::parse("---\nid: x\nname: n\n---\n\n指令", b"").err().is_some_and(|e| e.contains("缺少 desc"))
        );
        assert!(RoleSeed::parse("id: x", b"").is_err());
        for r in SEEDS.iter() {
            assert!(r.avatar.starts_with(b"\x89PNG\r\n\x1a\n") && r.avatar.len() <= MAX_AVATAR_BYTES, "{}.png", r.id);
        }
    }

    #[test]
    fn a_first_start_seeds_every_default_role() {
        let (app, dir) = app("fresh");
        app.sync_seeds().unwrap();
        for r in SEEDS.iter() {
            let role = role(&app, &r.id).unwrap();
            assert_eq!(
                (role["prompt"].as_str().unwrap(), role["seed"]["prompt"].as_str().unwrap()),
                (r.prompt.as_str(), text_hash(&r.prompt).as_str())
            );
            assert_eq!(role["hires"], r.id == "recruiter", "only 招聘顾问 proposes employees: {}", r.id);
        }
        // A deleted default role stays deleted on the next start.
        app.db.delete_config(ConfigKind::Role, "qa").unwrap();
        app.sync_seeds().unwrap();
        assert!(role(&app, "qa").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A default role as an earlier build left it: given `old` for both texts.
    fn given(app: &App, id: &str, desc: &str, prompt: &str) {
        let mut r = role(app, id).unwrap();
        r["desc"] = json!(desc);
        r["prompt"] = json!(prompt);
        r["seed"]["desc"] = json!(text_hash("旧职责"));
        r["seed"]["prompt"] = json!(text_hash("旧指令"));
        app.db.put_config(ConfigKind::Role, id, &r, 1).unwrap();
    }

    /// A newer build moves untouched default text forward and leaves edited text alone, offering
    /// the new text instead. The person can take it or keep theirs.
    #[test]
    fn new_default_text_replaces_only_what_the_person_never_edited() {
        let (app, dir) = app("sync");
        app.sync_seeds().unwrap();
        let developer = SEEDS.iter().find(|r| r.id == "developer").unwrap();
        given(&app, "developer", "旧职责", "旧指令");
        given(&app, "qa", "旧职责", "我改过的指令");
        app.sync_seeds().unwrap();

        let r = role(&app, "developer").unwrap();
        assert_eq!(
            (r["desc"].as_str().unwrap(), r["prompt"].as_str().unwrap()),
            (developer.desc.as_str(), developer.prompt.as_str())
        );
        assert!(seed_updates(&crate::agents::input(r.clone())).is_empty());
        let qa = SEEDS.iter().find(|r| r.id == "qa").unwrap();
        let r = role(&app, "qa").unwrap();
        assert_eq!(r["prompt"], "我改过的指令", "edited text is the person's");
        assert_eq!(r["desc"], qa.desc, "the untouched field still moves");
        assert_eq!(Value::Object(seed_updates(&crate::agents::input(r.clone()))), json!({ "prompt": qa.prompt }));
        let listed = app.roles().unwrap().into_iter().find(|r| r["id"] == "qa").unwrap();
        assert_eq!(listed["seedUpdates"]["prompt"], qa.prompt, "the app is offered the new text");

        // Saving from the editor keeps the origin; the editor cannot set it.
        let mut body = r.clone();
        body["seed"] = json!({ "id": "developer" });
        app.save_role(&crate::agents::input(body)).unwrap();
        assert_eq!(role(&app, "qa").unwrap()["seed"], r["seed"]);

        // Keeping the person's text stops the offer; a later start leaves it alone.
        let kept = app.seed_choice("qa", "prompt", false).unwrap();
        assert_eq!(kept.prompt, "我改过的指令");
        app.sync_seeds().unwrap();
        let r = role(&app, "qa").unwrap();
        assert_eq!(r["prompt"], "我改过的指令");
        assert!(seed_updates(&crate::agents::input(r.clone())).is_empty());

        // Taking the new text replaces it.
        given(&app, "qa", &qa.desc, "又改了一次");
        let taken = app.seed_choice("qa", "prompt", true).unwrap();
        assert_eq!(taken.prompt, qa.prompt);
        assert!(app.seed_choice("qa", "name", true).is_err(), "only desc and prompt have default text");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod id_tests {
    use super::*;
    use crate::orchestrator::test_app;

    /// Ids name files under Janus home: one that could leave its folder is refused when a role or a
    /// workspace is saved, and deleting a role by such an id touches nothing outside.
    #[test]
    fn ids_that_could_leave_their_folder_are_refused() {
        let dir = std::env::temp_dir().join(format!("janus-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = test_app(&dir);
        // `<home>/../outside.json`, should an id ever name a path.
        let outside = dir.join("outside.json");
        std::fs::write(&outside, "keep").unwrap();
        for bad in ["../../outside", "a/b", "Upper", "-lead", "x".repeat(41).as_str()] {
            let err = app.save_role(&crate::agents::input(json!({ "id": bad, "name": "R" }))).unwrap_err().to_string();
            assert!(err.contains("不合法"), "role id {bad}: {err}");
            let err = app
                .save_workspace(&crate::agents::input(json!({ "id": bad, "name": "W", "path": dir.to_string_lossy() })))
                .unwrap_err()
                .to_string();
            assert!(err.contains("不合法"), "workspace id {bad}: {err}");
        }
        assert!(app.delete_role("../../outside").is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep", "nothing outside Janus home is removed");
        // An agent's id names the folder its deletion removes.
        let err = app
            .save_connection(&json!({ "id": "../x", "kind": "claude", "name": "A", "auth": "apikey", "apiKey": "k" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("不合法"), "{err}");
        assert!(app.delete_connection("..").is_err());
        assert!(outside.exists(), "deleting an agent never leaves the agents folder");
        // Deleting an agent unbinds its roles completely, as if saved without one.
        let bound = crate::agents::test_role(
            "bound",
            json!({ "connection": "gone", "model": "m", "effort": "high", "session": { "mode": "full" } }),
        );
        app.db.put_config(ConfigKind::Role, "bound", &bound, 1).unwrap();
        app.db
            .put_config(
                ConfigKind::Connection,
                "gone",
                &crate::agents::test_connection("gone", "claude", "claude", json!({})),
                1,
            )
            .unwrap();
        app.delete_connection("gone").unwrap();
        let role = app.db.get_config(ConfigKind::Role, "bound").unwrap().unwrap();
        assert_eq!(
            [&role["connection"], &role["model"], &role["effort"], &role["session"]],
            [&json!(""), &json!(""), &json!(""), &json!({})]
        );
        // A valid id is kept as given.
        assert_eq!(
            app.save_role(&crate::agents::input(json!({ "id": "my-role", "name": "R" }))).unwrap().id,
            "my-role"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
