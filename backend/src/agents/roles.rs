//! Roles: saving and deleting them, the library items they use, and the default roles a first
//! start creates.

use super::*;
use rusqlite::OptionalExtension;

impl App {
    /// Roles as the app sees them: a default role the person edited also carries `seedUpdates`,
    /// the newer default text of the edited fields.
    pub fn roles(&self) -> Result<Vec<Value>> {
        let mut roles = self.db.list_config(ROLE)?;
        for r in roles.iter_mut() {
            let updates = seed_updates(r);
            if updates.as_object().is_some_and(|u| !u.is_empty()) {
                r["seedUpdates"] = updates;
            }
        }
        Ok(roles)
    }

    pub fn save_role(&self, body: &Value) -> Result<Value> {
        let name = str_of(body, "name");
        if name.is_empty() {
            bail!("数字员工需要一个名字");
        }
        let id = match str_of(body, "id") {
            "" => new_id("r"),
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        let connection = str_of(body, "connection");
        // The session controls this role chose, checked against what its agent declares.
        let session = if connection.is_empty() { json!({}) } else { role_session(body, &self.connection(connection)?) };
        let init = match str_of(body, "init") {
            "" => name.chars().filter(|c| !c.is_whitespace()).take(2).collect::<String>().to_uppercase(),
            i => i.to_string(),
        };
        let role = json!({
            "id": id, "name": name, "desc": str_of(body, "desc"),
            "prompt": body["prompt"].as_str().unwrap_or("").trim(),
            "connection": connection, "model": str_of(body, "model"), "effort": str_of(body, "effort"),
            "session": session,
            "lead": body["lead"].as_bool().unwrap_or(false),
            "color": match str_of(body, "color") { "" => "#64748b", c => c }, "init": init,
            "avatar": "",
        });
        let mut role = role;
        // Where a default role came from and its picture are Janus's records, not something the
        // editor sends: the picture changes through `set_role_avatar`.
        if let Some(prev) = self.db.get_config(ROLE, &id)? {
            if !prev["seed"].is_null() {
                role["seed"] = prev["seed"].clone();
            }
            role["avatar"] = json!(str_of(&prev, "avatar"));
        }
        self.db.put_config(ROLE, &id, &role, now_ms())?;
        Ok(role)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        if let Some(ws) = self
            .db
            .list_config(WORKSPACE)?
            .iter()
            .find(|w| w["members"].as_array().is_some_and(|m| m.iter().any(|m| m["role"] == id)))
        {
            bail!("工作区「{}」正在使用这个角色，请先移除", str_of(ws, "name"));
        }
        check_id(id)?;
        self.db.delete_config(ROLE, id)?;
        for path in self.cfg.layout.role_paths(id) {
            crate::layout::remove(&path);
        }
        Ok(())
    }

    /// Give a digital employee its own picture (`png`: a PNG, base64, the way the app sends it
    /// after cropping it square), or take it back to its initials (`None`). The record keeps the
    /// picture's hash, so the app knows there is one and when it changed.
    // @project-doc docs/domains/20261031-agents_and_roles.md#avatars
    pub fn set_role_avatar(&self, id: &str, png: Option<&str>) -> Result<Value> {
        use base64::Engine;
        check_id(id)?;
        let mut role = self.db.get_config(ROLE, id)?.ok_or_else(|| anyhow!("数字员工 {id} 不存在"))?;
        let file = self.cfg.layout.role_avatar(id);
        role["avatar"] = match png {
            Some(data) => {
                let data = data.split_once(";base64,").map_or(data, |(_, d)| d).trim();
                let bytes = base64::engine::general_purpose::STANDARD.decode(data).context("头像不是有效的 base64")?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    bail!("头像要是 PNG 图片");
                }
                if bytes.len() > MAX_AVATAR_BYTES {
                    bail!("头像超过 {} KB", MAX_AVATAR_BYTES / 1024);
                }
                write_avatar(&file, &bytes)?;
                json!(avatar_hash(&bytes))
            }
            None => {
                crate::layout::remove(&file);
                json!("")
            }
        };
        self.db.put_config(ROLE, id, &role, now_ms())?;
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
        if self.db.get_config("meta", "seeded")?.is_none() {
            for r in SEEDS.iter() {
                let mut role = self.save_role(&json!({ "id": r.id, "name": r.name, "desc": r.desc, "color": r.color, "init": r.init, "lead": r.lead, "prompt": r.prompt }))?;
                role["seed"] = json!({ "id": r.id, "desc": text_hash(r.desc), "prompt": text_hash(r.prompt) });
                self.ship_avatar(r, &mut role)?;
                self.db.put_config(ROLE, r.id, &role, now_ms())?;
            }
            return self.db.put_config("meta", "seeded", &json!(true), now_ms());
        }
        for (r, mut role) in self.seeded_roles()? {
            let mut changed = false;
            for (field, text) in [("desc", r.desc), ("prompt", r.prompt)] {
                let shipped = text_hash(text);
                let given = role["seed"][field].as_str().unwrap_or_default().to_string();
                if given == shipped {
                    continue;
                }
                let now = text_hash(str_of(&role, field));
                if now == given || now == shipped {
                    role[field] = json!(text);
                    role["seed"][field] = json!(shipped);
                    changed = true;
                }
            }
            // The picture follows the same rule: one the person never changed (or one from before
            // default roles had pictures) becomes this build's; one they chose or removed stays.
            let shipped = avatar_hash(r.avatar);
            let given = role["seed"]["avatar"].as_str().unwrap_or_default().to_string();
            let now = str_of(&role, "avatar").to_string();
            if given != shipped && (now == given || now == shipped) {
                self.ship_avatar(r, &mut role)?;
                changed = true;
            }
            if changed {
                self.db.put_config(ROLE, r.id, &role, now_ms())?;
            }
        }
        Ok(())
    }

    /// Give a default role the picture this build ships, recording it as the one it was given.
    fn ship_avatar(&self, r: &RoleSeed, role: &mut Value) -> Result<()> {
        write_avatar(&self.cfg.layout.role_avatar(r.id), r.avatar)?;
        let hash = avatar_hash(r.avatar);
        role["avatar"] = json!(hash);
        role["seed"]["avatar"] = json!(hash);
        Ok(())
    }

    /// The default roles that still exist, each with the default it came from.
    fn seeded_roles(&self) -> Result<Vec<(&'static RoleSeed, Value)>> {
        Ok(SEEDS
            .iter()
            .filter_map(|r| {
                let role = self.db.get_config(ROLE, r.id).ok().flatten()?;
                (role["seed"]["id"] == r.id).then_some((r, role))
            })
            .collect())
    }

    /// Adopt (`adopt: true`) or decline the newer default text of one field of a default role the
    /// person edited. Either way the role stops offering it until a later build ships another.
    pub fn seed_choice(&self, id: &str, field: &str, adopt: bool) -> Result<Value> {
        let (r, mut role) = self
            .seeded_roles()?
            .into_iter()
            .find(|(r, _)| r.id == id)
            .ok_or_else(|| anyhow!("{id} 不是默认数字员工"))?;
        let text = match field {
            "desc" => r.desc,
            "prompt" => r.prompt,
            _ => bail!("只能更新职责（desc）或指令（prompt）"),
        };
        if adopt {
            role[field] = json!(text);
        }
        role["seed"][field] = json!(text_hash(text));
        self.db.put_config(ROLE, id, &role, now_ms())?;
        Ok(role)
    }
}

/// The newer default text a default role's edited fields could take, `{field: text}`; empty when
/// there is none. Shown in the role editor.
fn seed_updates(role: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(r) = SEEDS.iter().find(|r| role["seed"]["id"] == r.id) {
        for (field, text) in [("desc", r.desc), ("prompt", r.prompt)] {
            let shipped = text_hash(text);
            if role["seed"][field] != shipped.as_str() && text_hash(str_of(role, field)) != shipped {
                out.insert(field.into(), json!(text));
            }
        }
    }
    Value::Object(out)
}

/// The hash a default text is remembered by: the first 16 hex digits of its SHA-256.
/// A picture is at most this large: the app sends 256×256 PNGs, far smaller.
const MAX_AVATAR_BYTES: usize = 1024 * 1024;

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

fn text_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Every `desc` and `prompt` each default role has shipped with, newest first, by hash. Data
/// version 2 uses it to tell an untouched default role from one the person edited.
const SEED_HISTORY: &[(&str, &str, &[&str])] = &[
    ("coordinator", "desc", &["11c26d05577fd4dd", "c073bc2d506f6974"]),
    (
        "coordinator",
        "prompt",
        &["e65ffc65888ae583", "a81c047b83d8845d", "00dfa19b6610feb6", "0cad66d4ff1444f7", "7dfaede38661aa89"],
    ),
    ("researcher", "desc", &["8f15e8966e580c22", "3c1578d280be92ab"]),
    ("researcher", "prompt", &["07d89d72db780be3", "ecde922ff521160c", "f371c15c3915aeb9"]),
    ("product-manager", "desc", &["55820f7da51d1b35", "6575fe20b36ad7cb"]),
    ("product-manager", "prompt", &["c1e2e315f041c00e", "7c9157e10580d1c8", "0c45315e34958031", "8432df1724ece14d"]),
    ("implementer", "desc", &["3f1d72fd83df989f", "0cff94e51cadbae7"]),
    ("implementer", "prompt", &["e688dd4dbc6bc43d", "a256fd34f7e21459", "e3dabe553ae3b911"]),
    ("reviewer", "desc", &["3a457fb394888f0b", "b91cafdf208c7a65"]),
    ("reviewer", "prompt", &["9cc3fe77d56480b7", "7ffd4c34321ef2e0"]),
];

/// Data version 2: default roles created before roles remembered their origin get a `seed`. A
/// field whose text is one Janus shipped counts as untouched (and moves to the current text on
/// start); an edited one is marked as given the newest earlier text, so the app offers the
/// current one instead of replacing it. Only the default roles of that time count (those in
/// `SEED_HISTORY`): a role of the person's under the id of a later default is theirs.
pub(crate) fn record_seed_origins(tx: &rusqlite::Transaction) -> Result<usize> {
    crate::migrations::edit_configs(tx, ROLE, |id, role| {
        if !SEED_HISTORY.iter().any(|h| h.0 == id) {
            return false;
        }
        let Some(r) = SEEDS.iter().find(|r| r.id == id) else { return false };
        if role.contains_key("seed") {
            return false;
        }
        let mut seed = json!({ "id": r.id });
        for (field, text) in [("desc", r.desc), ("prompt", r.prompt)] {
            let shipped = text_hash(text);
            let history: Vec<&str> =
                SEED_HISTORY.iter().filter(|h| h.0 == id && h.1 == field).flat_map(|h| h.2.iter().copied()).collect();
            let now = text_hash(role.get(field).and_then(Value::as_str).unwrap_or_default());
            seed[field] = json!(if now == shipped || history.contains(&now.as_str()) {
                now
            } else {
                history.iter().find(|h| **h != shipped).map_or(shipped.clone(), |h| h.to_string())
            });
        }
        role.insert("seed".into(), seed);
        true
    })
}

/// A default role, created on a first start with no agent bound yet.
pub(super) struct RoleSeed {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    color: &'static str,
    init: &'static str,
    lead: bool,
    prompt: &'static str,
    /// Its picture, a square PNG.
    avatar: &'static [u8],
}

/// The default role a new workspace pairs with the lead (three at a time).
pub(super) const GENERALIST: &str = "generalist";

/// The default role for a one-to-one talk about something on the person's mind.
pub(super) const OLD_FRIEND: &str = "inner-grill";

/// Data version 6: the general digital worker, for data seeded before it existed (`add_seed`).
pub(crate) fn add_generalist(tx: &rusqlite::Transaction) -> Result<usize> {
    add_seed(tx, GENERALIST)
}

/// Data version 15: 老友, for data seeded before it existed (`add_seed`).
pub(crate) fn add_old_friend(tx: &rusqlite::Transaction) -> Result<usize> {
    add_seed(tx, OLD_FRIEND)
}

/// A default role that came after the data was first seeded, made as a first start would make it
/// (`sync_seeds` keeps its text current afterwards). Data never seeded gets it on its first start
/// instead. A role the person already has under this id stays theirs, unless it is the very text
/// shipped (one they copied in by hand): that one becomes the default role. One deleted later
/// stays deleted.
fn add_seed(tx: &rusqlite::Transaction, id: &str) -> Result<usize> {
    let seeded: bool = tx
        .query_row("SELECT COUNT(*) FROM configs WHERE kind = 'meta' AND id = 'seeded'", [], |r| r.get::<_, i64>(0))?
        > 0;
    if !seeded {
        return Ok(0);
    }
    let r = SEEDS.iter().find(|r| r.id == id).expect("a default role");
    let origin = json!({ "id": r.id, "desc": text_hash(r.desc), "prompt": text_hash(r.prompt) });
    let existing: Option<String> = tx
        .query_row("SELECT data FROM configs WHERE kind = ?1 AND id = ?2", rusqlite::params![ROLE, r.id], |row| {
            row.get(0)
        })
        .optional()?;
    let role = match existing {
        Some(data) => {
            let mut role: Value = serde_json::from_str(&data)?;
            if role.get("seed").is_some() || role["desc"] != r.desc || role["prompt"] != r.prompt {
                return Ok(0);
            }
            role["seed"] = origin;
            role
        }
        None => json!({
            "id": r.id, "name": r.name, "desc": r.desc, "prompt": r.prompt, "connection": "",
            "model": "", "effort": "", "session": {}, "lead": r.lead, "color": r.color, "init": r.init, "avatar": "", "seed": origin
        }),
    };
    tx.execute(
        "INSERT OR REPLACE INTO configs (kind, id, data, updated_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![ROLE, r.id, role.to_string(), crate::orchestrator::now_ms()],
    )?;
    Ok(1)
}

/// The default roles, two files each in `backend/roles/`: `<id>.md` with the metadata in a `---`
/// header and the role's instructions as the body, and `<id>.png`, its picture (256×256). They
/// are built into Janus,
/// in this order: a PMO lead, the general digital worker that a default workspace pairs with it,
/// the specialists (research and market, PRD, engineering, QA) for whoever wants them, and 老友
/// to talk something through with in a chat.
const ROLE_FILES: &[(&str, &str, &[u8])] = &[
    ("coordinator", include_str!("../../roles/coordinator.md"), include_bytes!("../../roles/coordinator.png")),
    ("researcher", include_str!("../../roles/researcher.md"), include_bytes!("../../roles/researcher.png")),
    (
        "product-manager",
        include_str!("../../roles/product-manager.md"),
        include_bytes!("../../roles/product-manager.png"),
    ),
    ("generalist", include_str!("../../roles/generalist.md"), include_bytes!("../../roles/generalist.png")),
    ("implementer", include_str!("../../roles/implementer.md"), include_bytes!("../../roles/implementer.png")),
    ("reviewer", include_str!("../../roles/reviewer.md"), include_bytes!("../../roles/reviewer.png")),
    ("inner-grill", include_str!("../../roles/inner-grill.md"), include_bytes!("../../roles/inner-grill.png")),
];

// @project-doc docs/domains/20261031-agents_and_roles.md#product_launch_roles
pub(super) static SEEDS: std::sync::LazyLock<Vec<RoleSeed>> = std::sync::LazyLock::new(|| {
    ROLE_FILES
        .iter()
        .map(|(file, text, avatar)| {
            RoleSeed::parse(text, avatar).unwrap_or_else(|err| panic!("backend/roles/{file}.md：{err}"))
        })
        .collect()
});

impl RoleSeed {
    /// A role file: `---`, a `key: value` line for each of `id`, `name`, `desc`, `color`, `init`
    /// and `lead` (a value may be in double quotes), `---`, then the instructions.
    fn parse(text: &'static str, avatar: &'static [u8]) -> Result<RoleSeed, String> {
        let rest = text.strip_prefix("---\n").ok_or("第一行应当是 ---")?;
        let (head, body) = rest.split_once("\n---\n").ok_or("元数据之后应当有一行 ---")?;
        let field = |key: &str| {
            head.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
                .map(|v| v.trim().trim_matches('"'))
                .filter(|v| !v.is_empty())
                .ok_or(format!("缺少 {key}"))
        };
        let (id, name, desc, color, init) =
            (field("id")?, field("name")?, field("desc")?, field("color")?, field("init")?);
        let lead = match field("lead")? {
            "true" => true,
            "false" => false,
            other => return Err(format!("lead 应当是 true 或 false，而不是 {other}")),
        };
        let prompt = body.trim();
        if prompt.is_empty() {
            return Err("没有指令正文".into());
        }
        Ok(RoleSeed { id, name, desc, color, init, lead, prompt, avatar })
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
        app.db.get_config(ROLE, id).unwrap()
    }

    /// A digital employee's picture: only a PNG of a sane size is taken; it survives editing the
    /// role, goes back to the initials when removed, and goes with the role.
    #[test]
    fn a_role_keeps_its_own_picture() {
        use base64::Engine;
        let (app, dir) = app("avatar");
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"rest of the picture"].concat();
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        app.save_role(&json!({ "id": "pic", "name": "画像" })).unwrap();
        assert_eq!(role(&app, "pic").unwrap()["avatar"], "", "a new role shows its initials");
        assert!(app.role_avatar("pic").unwrap().is_none());

        assert!(app.set_role_avatar("pic", Some(&b64(b"GIF89a not a png"))).is_err(), "only PNG");
        assert!(app.set_role_avatar("pic", Some(&b64(&vec![0x89; MAX_AVATAR_BYTES + 1]))).is_err(), "not too large");
        assert!(app.set_role_avatar("nobody", Some(&b64(&png))).is_err(), "only an existing role");
        assert!(app.set_role_avatar("../x", Some(&b64(&png))).is_err(), "an id that leaves its folder");

        let saved = app.set_role_avatar("pic", Some(&format!("data:image/png;base64,{}", b64(&png)))).unwrap();
        assert_eq!(saved["avatar"], json!(avatar_hash(&png)));
        assert_eq!(app.role_avatar("pic").unwrap().as_deref(), Some(png.as_slice()));
        app.save_role(&json!({ "id": "pic", "name": "改了名", "avatar": "" })).unwrap();
        assert_eq!(role(&app, "pic").unwrap()["avatar"], json!(avatar_hash(&png)), "editing keeps the picture");

        assert_eq!(app.set_role_avatar("pic", None).unwrap()["avatar"], "");
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
            assert_eq!(role(&app, r.id).unwrap()["avatar"], json!(avatar_hash(r.avatar)), "{}", r.id);
            assert_eq!(app.role_avatar(r.id).unwrap().as_deref(), Some(r.avatar));
        }
        // As if seeded before pictures: no picture, nothing recorded as given.
        let mut old = role(&app, "implementer").unwrap();
        old["avatar"] = json!("");
        old["seed"].as_object_mut().unwrap().remove("avatar");
        app.db.put_config(ROLE, "implementer", &old, 1).unwrap();
        std::fs::remove_file(app.cfg.layout.role_avatar("implementer")).unwrap();
        // One the person took back to initials, one they gave their own picture.
        app.set_role_avatar("reviewer", None).unwrap();
        let mine = [b"\x89PNG\r\n\x1a\n".as_slice(), b"mine"].concat();
        app.set_role_avatar("coordinator", Some(&base64::engine::general_purpose::STANDARD.encode(&mine))).unwrap();

        app.sync_seeds().unwrap();
        let shipped = |id: &str| avatar_hash(SEEDS.iter().find(|r| r.id == id).unwrap().avatar);
        assert_eq!(
            role(&app, "implementer").unwrap()["avatar"],
            json!(shipped("implementer")),
            "an older role gets its picture"
        );
        assert_eq!(role(&app, "reviewer").unwrap()["avatar"], "", "a removed picture stays removed");
        assert_eq!(app.role_avatar("coordinator").unwrap(), Some(mine), "the person's own picture stays");
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
            check_id(r.id).unwrap();
        }
        let ids: std::collections::HashSet<_> = SEEDS.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), SEEDS.len());
        assert!(RoleSeed::parse("---\nid: x\n---\n\n指令", b"").err().is_some_and(|e| e.contains("缺少 name")));
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
            let role = role(&app, r.id).unwrap();
            assert_eq!(
                (role["prompt"].as_str().unwrap(), role["seed"]["prompt"].as_str().unwrap()),
                (r.prompt, text_hash(r.prompt).as_str())
            );
        }
        // A deleted default role stays deleted on the next start.
        app.db.delete_config(ROLE, "reviewer").unwrap();
        app.sync_seeds().unwrap();
        assert!(role(&app, "reviewer").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A default role as an earlier build left it: given `old` for both texts.
    fn given(app: &App, id: &str, desc: &str, prompt: &str) {
        let mut r = role(app, id).unwrap();
        r["desc"] = json!(desc);
        r["prompt"] = json!(prompt);
        r["seed"]["desc"] = json!(text_hash("旧职责"));
        r["seed"]["prompt"] = json!(text_hash("旧指令"));
        app.db.put_config(ROLE, id, &r, 1).unwrap();
    }

    /// A newer build moves untouched default text forward and leaves edited text alone, offering
    /// the new text instead. The person can take it or keep theirs.
    #[test]
    fn new_default_text_replaces_only_what_the_person_never_edited() {
        let (app, dir) = app("sync");
        app.sync_seeds().unwrap();
        let implementer = SEEDS.iter().find(|r| r.id == "implementer").unwrap();
        given(&app, "implementer", "旧职责", "旧指令");
        given(&app, "reviewer", "旧职责", "我改过的指令");
        app.sync_seeds().unwrap();

        let r = role(&app, "implementer").unwrap();
        assert_eq!(
            (r["desc"].as_str().unwrap(), r["prompt"].as_str().unwrap()),
            (implementer.desc, implementer.prompt)
        );
        assert!(seed_updates(&r).as_object().unwrap().is_empty());
        let reviewer = SEEDS.iter().find(|r| r.id == "reviewer").unwrap();
        let r = role(&app, "reviewer").unwrap();
        assert_eq!(r["prompt"], "我改过的指令", "edited text is the person's");
        assert_eq!(r["desc"], reviewer.desc, "the untouched field still moves");
        assert_eq!(seed_updates(&r), json!({ "prompt": reviewer.prompt }));
        let listed = app.roles().unwrap().into_iter().find(|r| r["id"] == "reviewer").unwrap();
        assert_eq!(listed["seedUpdates"]["prompt"], reviewer.prompt, "the app is offered the new text");

        // Saving from the editor keeps the origin; the editor cannot set it.
        let mut body = r.clone();
        body["seed"] = json!({ "id": "implementer" });
        app.save_role(&body).unwrap();
        assert_eq!(role(&app, "reviewer").unwrap()["seed"], r["seed"]);

        // Keeping the person's text stops the offer; a later start leaves it alone.
        let kept = app.seed_choice("reviewer", "prompt", false).unwrap();
        assert_eq!(kept["prompt"], "我改过的指令");
        app.sync_seeds().unwrap();
        let r = role(&app, "reviewer").unwrap();
        assert_eq!(r["prompt"], "我改过的指令");
        assert!(seed_updates(&r).as_object().unwrap().is_empty());

        // Taking the new text replaces it.
        given(&app, "reviewer", reviewer.desc, "又改了一次");
        let taken = app.seed_choice("reviewer", "prompt", true).unwrap();
        assert_eq!(taken["prompt"], reviewer.prompt);
        assert!(app.seed_choice("reviewer", "name", true).is_err(), "only desc and prompt have default text");
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
            let err = app.save_role(&json!({ "id": bad, "name": "R" })).unwrap_err().to_string();
            assert!(err.contains("不合法"), "role id {bad}: {err}");
            let err = app
                .save_workspace(&json!({ "id": bad, "name": "W", "path": dir.to_string_lossy() }))
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
        app.db.put_config(ROLE, "bound", &bound, 1).unwrap();
        app.db
            .put_config(CONNECTION, "gone", &crate::agents::test_connection("gone", "claude", "claude", json!({})), 1)
            .unwrap();
        app.delete_connection("gone").unwrap();
        let role = app.db.get_config(ROLE, "bound").unwrap().unwrap();
        assert_eq!(
            [&role["connection"], &role["model"], &role["effort"], &role["session"]],
            [&json!(""), &json!(""), &json!(""), &json!({})]
        );
        // A valid id is kept as given.
        assert_eq!(app.save_role(&json!({ "id": "my-role", "name": "R" })).unwrap()["id"], "my-role");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
