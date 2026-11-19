//! The resource library: skills and MCP servers, kept once and chosen by roles.
//!
//! An item has an opaque id; its kind, origin and revision are fields. A skill's files are stored
//! by content hash (`store`); an MCP server's definition is in the item and its values in
//! `secrets/library/<id>.json`. Roles refer to items by id. A run pins the items its members use
//! (`Member::resources`) and copies their skills into per-member bundles (`deliver`), which
//! sessions get at launch; nothing is ever written into an agent's own configuration.
// @project-doc docs/domains/20261116-resource_library.md#model

mod deliver;
mod mcp;
mod offers;
mod skill;
mod sources;
mod store;

pub use deliver::{Delivery, MCP_ROUTER, SKILL_ROUTER, bundle, launch, routed_server, support};
pub use mcp::{McpServer, McpValues};
pub use sources::{CHECK_EVERY, Source};

use crate::agents::{ROLE, new_id, str_of, write_private};
use crate::orchestrator::{App, expand_home, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const ITEM: &str = "library_item";

/// Library edits happen one at a time, from any page or the background check.
#[derive(Default)]
pub struct LibraryState {
    lock: tokio::sync::Mutex<()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Skill,
    Mcp,
}

impl ItemKind {
    fn label(self) -> &'static str {
        match self {
            ItemKind::Skill => "Skill",
            ItemKind::Mcp => "MCP 服务器",
        }
    }
}

/// Where an item came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Origin {
    /// Written in Janus: a skill in the editor, an MCP server in the form or a pasted config.
    Created,
    /// A skill copied from a folder on this machine; refreshing copies it again.
    Folder { path: String },
    /// Imported from a Git source: the skill's folder, or `<manifest>#<server>`.
    Git { source: String, path: String },
}

/// Who may use an item. Every digital worker gets the items for everyone, through the MCP and
/// skill routers; an item kept for some goes only to the members whose role picks it.
// @project-doc docs/domains/20261116-resource_library.md#reach
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    Everyone,
    Picked,
}

impl Reach {
    /// What a new item starts as: a skill is instructions anyone may read; an MCP server acts on
    /// the world, so the person opens it to everyone.
    pub fn default_for(body: &Body) -> Reach {
        match body {
            Body::Skill => Reach::Everyone,
            Body::Mcp { .. } => Reach::Picked,
        }
    }
}

/// Data version 5: every library item, and every item pinned in a run's member snapshot, says
/// who may use it, as a new item would (`Reach::default_for`). Roles keep what they picked: it
/// now means the items kept for some that the role gets.
pub(crate) fn record_reach(tx: &rusqlite::Transaction) -> Result<usize> {
    fn fill(o: &mut serde_json::Map<String, Value>) -> bool {
        if o.contains_key("reach") {
            return false;
        }
        let reach = if o.get("kind").and_then(Value::as_str) == Some("skill") { "everyone" } else { "picked" };
        o.insert("reach".into(), json!(reach));
        true
    }
    let mut changed = crate::migrations::edit_configs(tx, "library_item", |_, o| fill(o))?;
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT id, config FROM runs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, config) in rows {
        let Ok(mut config) = serde_json::from_str::<Value>(&config) else { continue };
        let mut any = false;
        for m in config["team"].as_array_mut().into_iter().flatten() {
            for r in m["resources"].as_array_mut().into_iter().flatten() {
                if let Some(o) = r.as_object_mut()
                    && fill(o)
                {
                    any = true;
                    changed += 1;
                }
            }
        }
        if any {
            tx.execute("UPDATE runs SET config = ?1 WHERE id = ?2", rusqlite::params![config.to_string(), id])?;
        }
    }
    Ok(changed)
}

/// What an item is. A skill's files are in the store under the item's revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    Skill,
    Mcp { server: McpServer },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    /// The name the agent sees; unique among items of one kind.
    pub name: String,
    /// Who may use it: every digital worker, or only those whose role picks it.
    pub reach: Reach,
    #[serde(default)]
    pub description: String,
    pub origin: Origin,
    /// sha256 of the content: the skill's files, or the server's definition.
    pub revision: String,
    pub updated_at: i64,
    #[serde(flatten)]
    pub body: Body,
}

impl Item {
    pub fn kind(&self) -> ItemKind {
        match self.body {
            Body::Skill => ItemKind::Skill,
            Body::Mcp { .. } => ItemKind::Mcp,
        }
    }
}

fn imported_offer(path: &str, item: &Item) -> Value {
    json!({ "path": path, "item": item.id, "name": item.name })
}

fn resolve_skill_folder(path: &str) -> Result<PathBuf> {
    let dir = PathBuf::from(expand_home(path)).canonicalize().context("文件夹不存在")?;
    if !dir.is_dir() {
        bail!("请选择文件夹");
    }
    Ok(dir)
}

/// The board MCP's and the router's names are reserved, in any case and with any punctuation
/// around them.
pub(crate) fn reserved_mcp_name(name: &str) -> bool {
    ["janus", MCP_ROUTER]
        .iter()
        .any(|r| name.eq_ignore_ascii_case(r) || crate::agents::slug(name).is_some_and(|s| s == *r))
}

fn mcp_revision(server: &McpServer) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(serde_json::to_vec(server).unwrap_or_default()).iter().map(|b| format!("{b:02x}")).collect()
}

impl App {
    fn library_dir(&self) -> PathBuf {
        self.cfg.data_dir.join("library")
    }

    pub(crate) fn store_dir(&self) -> PathBuf {
        self.library_dir().join("store")
    }

    fn values_path(&self, id: &str) -> Result<PathBuf> {
        crate::agents::check_id(id)?;
        Ok(self.cfg.data_dir.join("secrets").join("library").join(format!("{id}.json")))
    }

    pub fn items(&self) -> Result<Vec<Item>> {
        self.db.list_config(ITEM)?.into_iter().map(|v| serde_json::from_value(v).context("资源记录无法读取")).collect()
    }

    pub fn item(&self, id: &str) -> Result<Item> {
        let v = self.db.get_config(ITEM, id)?.ok_or_else(|| anyhow!("资源 {id} 不存在"))?;
        serde_json::from_value(v).context("资源记录无法读取")
    }

    fn put_item(&self, item: &Item) -> Result<()> {
        self.db.put_config(ITEM, &item.id, &serde_json::to_value(item)?, now_ms())
    }

    /// A server's values; none when it has no file (or the id is not one).
    pub fn mcp_values(&self, id: &str) -> McpValues {
        self.values_path(id)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn write_values(&self, id: &str, values: &McpValues) -> Result<()> {
        let path = self.values_path(id)?;
        if values.env.is_empty() && values.headers.is_empty() {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        write_private(&path, serde_json::to_string_pretty(values)?.as_bytes())
    }

    fn check_free_name(&self, kind: ItemKind, name: &str, except: Option<&str>) -> Result<()> {
        if kind == ItemKind::Skill && name == SKILL_ROUTER {
            bail!("{SKILL_ROUTER} 留给 Janus 的 Skill 路由，请换一个名字");
        }
        if self.items()?.iter().any(|i| i.kind() == kind && i.name == name && Some(i.id.as_str()) != except) {
            bail!("资源库里已经有叫 {name} 的{}，请换一个名字", kind.label());
        }
        Ok(())
    }

    /// `base`, else `base-2`, `base-3`…: imports never fail on a name that is taken.
    fn free_name(&self, kind: ItemKind, base: &str) -> Result<String> {
        let mut taken: HashSet<String> =
            self.items()?.into_iter().filter(|i| i.kind() == kind).map(|i| i.name).collect();
        if kind == ItemKind::Skill {
            taken.insert(SKILL_ROUTER.to_string());
        }
        (1..10_000)
            .map(|n| if n == 1 { base.to_string() } else { format!("{base}-{n}") })
            .find(|n| !taken.contains(n))
            .context("没有可用的名字")
    }

    fn new_item(&self, name: String, description: String, origin: Origin, revision: String, body: Body) -> Item {
        let reach = Reach::default_for(&body);
        Item { id: new_id("i"), name, reach, description, origin, revision, updated_at: now_ms(), body }
    }

    /// Open an item to every digital worker, or keep it for the roles that pick it.
    pub async fn set_reach(&self, id: &str, reach: Reach) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let mut item = self.item(id)?;
        item.reach = reach;
        item.updated_at = now_ms();
        self.put_item(&item)?;
        Ok(item)
    }

    /// The digital workers an item is given to, set from the item's side: each listed role picks it,
    /// every other role drops it. This is where the library page decides who may use a kept item;
    /// the choice is stored on the roles (`resources`), the one place a run reads it from.
    // @project-doc docs/domains/20261116-resource_library.md#roles
    pub async fn set_item_roles(&self, id: &str, roles: &[String]) -> Result<Value> {
        let _guard = self.library.lock.lock().await;
        self.item(id)?;
        let all = self.db.list_config(ROLE)?;
        if let Some(unknown) = roles.iter().find(|r| !all.iter().any(|x| x["id"] == r.as_str())) {
            bail!("没有数字员工 {unknown}");
        }
        for mut role in all {
            let rid = role["id"].as_str().unwrap_or_default().to_string();
            let mut picked: Vec<Value> = role["resources"].as_array().cloned().unwrap_or_default();
            let has = picked.iter().any(|x| x == id);
            let wanted = roles.contains(&rid);
            if has == wanted {
                continue;
            }
            if wanted {
                picked.push(json!(id));
            } else {
                picked.retain(|x| x != id);
            }
            role["resources"] = json!(picked);
            self.db.put_config(ROLE, &rid, &role, now_ms())?;
        }
        Ok(json!({ "roles": self.roles_using(id)? }))
    }

    /// Remove stored skill trees no item uses any more.
    fn sweep_store(&self) -> Result<()> {
        let keep: HashSet<String> =
            self.items()?.into_iter().filter(|i| i.kind() == ItemKind::Skill).map(|i| i.revision).collect();
        store::sweep(&self.store_dir(), &keep);
        Ok(())
    }

    // ---- Skills

    /// A skill written in the editor: a name, a description and the body of its SKILL.md.
    pub async fn create_skill(&self, body: &Value) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let (name, description, text) = editor_skill(body)?;
        self.check_free_name(ItemKind::Skill, &name, None)?;
        let revision = store::put_text(&self.store_dir(), &[("SKILL.md", &text)])?;
        let item = self.new_item(name, skill::short(&description), Origin::Created, revision, Body::Skill);
        self.put_item(&item)?;
        Ok(item)
    }

    /// Only skills written in Janus are edited here; others change where they came from.
    pub async fn edit_skill(&self, id: &str, body: &Value) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let mut item = self.item(id)?;
        if item.kind() != ItemKind::Skill || item.origin != Origin::Created {
            bail!("只能编辑在 Janus 中新建的 Skill；来自目录或 Git 来源的 Skill 请在原处修改，再刷新或更新来源");
        }
        let (name, description, text) = editor_skill(body)?;
        self.check_free_name(ItemKind::Skill, &name, Some(id))?;
        item.revision = store::put_text(&self.store_dir(), &[("SKILL.md", &text)])?;
        item.name = name;
        item.description = skill::short(&description);
        item.updated_at = now_ms();
        self.put_item(&item)?;
        self.sweep_store()?;
        Ok(item)
    }

    /// Copy a skill folder the person picked into the library.
    pub async fn import_skill_folder(&self, path: &str) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        self.import_local_skill(&resolve_skill_folder(path.trim())?)
    }

    /// Preview the skills in a selected local folder without copying them.
    pub fn scan_skill_folder(&self, path: &str) -> Result<Value> {
        let root = resolve_skill_folder(path)?;
        let (offers, warnings) = offers::discover(&root, &root);
        if offers.is_empty() && warnings.is_empty() {
            bail!("所选文件夹中没有找到 Skill（SKILL.md）");
        }
        let imports = self.folder_imports()?;
        let imported: Vec<Value> = offers
            .iter()
            .filter_map(|offer| imports.get(&root.join(&offer.path)).map(|item| imported_offer(&offer.path, item)))
            .collect();
        Ok(json!({ "path": root, "offers": offers, "imported": imported, "warnings": warnings }))
    }

    /// Copy the selected skills. A failed copy is reported separately so it can be retried.
    pub async fn import_skill_selection(&self, path: &str, paths: &[String]) -> Result<Value> {
        let _guard = self.library.lock.lock().await;
        let root = resolve_skill_folder(path)?;
        if paths.is_empty() {
            bail!("请至少选择一个 Skill");
        }
        let available: HashSet<_> = offers::skill_paths(&root).into_iter().collect();
        if let Some(path) = paths.iter().find(|path| !available.contains(*path)) {
            bail!("所选文件夹中没有 Skill 路径「{path}」。请重新选择文件夹");
        }
        let imports = self.folder_imports()?;
        let mut seen = HashSet::new();
        let (mut imported, mut warnings) = (Vec::new(), Vec::new());
        for path in paths.iter().filter(|p| seen.insert(p.as_str())) {
            let dir = root.join(path);
            if let Some(item) = imports.get(&dir) {
                imported.push(imported_offer(path, item));
                continue;
            }
            let copied = (|| {
                let src = dir.canonicalize().context("Skill 文件夹不存在")?;
                if src != dir {
                    bail!("Skill 文件夹的位置已变化");
                }
                self.import_local_skill(&src)
            })();
            match copied {
                Ok(item) => imported.push(imported_offer(path, &item)),
                Err(err) => warnings.push(format!("{}：{err:#}", dir.display())),
            }
        }
        if !warnings.is_empty() {
            self.sweep_store()?;
        }
        Ok(json!({ "imported": imported, "warnings": warnings }))
    }

    fn folder_imports(&self) -> Result<HashMap<PathBuf, Item>> {
        let mut imports = HashMap::new();
        for item in self.items()? {
            if let Origin::Folder { path } = &item.origin {
                imports.entry(PathBuf::from(path)).or_insert(item);
            }
        }
        Ok(imports)
    }

    fn import_local_skill(&self, dir: &Path) -> Result<Item> {
        self.import_skill_at(dir, Origin::Folder { path: dir.to_string_lossy().into_owned() })
    }

    fn import_skill_at(&self, dir: &Path, origin: Origin) -> Result<Item> {
        let store = self.store_dir();
        let revision = store::put(&store, dir)?;
        let text = std::fs::read_to_string(store.join(&revision).join("SKILL.md")).context("读取 SKILL.md 失败")?;
        let folder = dir.file_name().unwrap_or_default().to_string_lossy();
        let meta = skill::parse_meta(&text, &folder);
        let base = skill::skill_name(&meta.name).unwrap_or_else(|| "skill".into());
        let name = self.free_name(ItemKind::Skill, &base)?;
        let item = self.new_item(name, skill::short(&meta.description), origin, revision, Body::Skill);
        self.put_item(&item)?;
        Ok(item)
    }

    /// Copy a folder skill again from where it was imported.
    pub async fn refresh_item(&self, id: &str) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let mut item = self.item(id)?;
        let Origin::Folder { path } = &item.origin else {
            bail!("只有从本机目录导入的 Skill 可以刷新；Git 来源请更新来源");
        };
        let src = PathBuf::from(path);
        let revision = store::put(&self.store_dir(), &src).with_context(|| format!("读取 {path} 失败"))?;
        item.description = skill::short(&skill::read_meta(&src)?.description);
        if revision != item.revision {
            item.revision = revision;
            item.updated_at = now_ms();
        }
        self.put_item(&item)?;
        self.sweep_store()?;
        Ok(item)
    }

    /// A skill's SKILL.md as stored.
    pub fn skill_text(&self, item: &Item) -> Result<String> {
        std::fs::read_to_string(self.store_dir().join(&item.revision).join("SKILL.md")).context("读取 SKILL.md 失败")
    }

    // ---- MCP servers

    /// Save a server from the form. `vars` names the variables (stdio) or headers (http, sse) the
    /// server needs; an empty value keeps the one already saved.
    pub async fn save_mcp(&self, id: Option<&str>, body: &Value) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let prev = id.map(|id| self.item(id)).transpose()?;
        if prev.as_ref().is_some_and(|p| p.kind() != ItemKind::Mcp) {
            bail!("这个资源不是 MCP 服务器");
        }
        let name = str_of(body, "name").to_string();
        mcp::check_name(&name)?;
        self.check_free_name(ItemKind::Mcp, &name, id)?;
        let vars: Vec<(String, String)> = body["vars"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                let k = str_of(v, "name");
                (!k.is_empty()).then(|| (k.to_string(), v["value"].as_str().unwrap_or("").to_string()))
            })
            .collect();
        if let Some((bad, _)) =
            vars.iter().find(|(k, _)| !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            bail!("变量名只能包含字母、数字、- 和 _：{bad}");
        }
        let keys: Vec<String> = vars.iter().map(|(k, _)| k.clone()).collect();
        let server = match str_of(body, "transport") {
            "stdio" | "" => {
                let mut parts = match &body["args"] {
                    Value::Array(a) => std::iter::once(str_of(body, "command").to_string())
                        .chain(a.iter().filter_map(|x| x.as_str().map(str::to_owned)))
                        .collect(),
                    _ => crate::agents::split_command(str_of(body, "command"))?,
                };
                if parts.first().is_none_or(|c| c.is_empty()) {
                    bail!("MCP {name} 没有启动命令");
                }
                let command = expand_home(&parts.remove(0));
                McpServer::Stdio { command, args: parts, env_keys: keys }
            }
            t @ ("http" | "sse") => {
                let url = str_of(body, "url").to_string();
                mcp::check_url(&url)?;
                if t == "http" {
                    McpServer::Http { url, header_keys: keys }
                } else {
                    McpServer::Sse { url, header_keys: keys }
                }
            }
            other => bail!("不支持的传输方式：{other}"),
        };
        let mut item = match prev {
            Some(p) => p,
            None => self.new_item(
                name.clone(),
                String::new(),
                Origin::Created,
                String::new(),
                Body::Mcp { server: server.clone() },
            ),
        };
        let mut values = self.mcp_values(&item.id);
        for (k, v) in vars.into_iter().filter(|(_, v)| !v.is_empty()) {
            values.of(&server).insert(k, v);
        }
        self.write_values(&item.id, &values.kept_for(&server))?;
        let revision = mcp_revision(&server);
        if revision != item.revision || item.name != name {
            item.updated_at = now_ms();
        }
        item.name = name;
        item.description = str_of(body, "description").to_string();
        item.revision = revision;
        item.body = Body::Mcp { server };
        self.put_item(&item)?;
        Ok(item)
    }

    /// Add every server of a pasted config (`mcpServers`, `servers` or one server), with the values
    /// it names. Taken names get a suffix.
    pub async fn import_mcp_config(&self, text: &str) -> Result<Value> {
        let _guard = self.library.lock.lock().await;
        let (found, warnings) = mcp::parse_config(text)?;
        if found.is_empty() {
            bail!("没有可以导入的服务器：{}", warnings.join("；"));
        }
        let mut added = vec![];
        for f in found {
            let name = self.free_name(ItemKind::Mcp, &f.name)?;
            let item = self.new_item(
                name,
                f.description,
                Origin::Created,
                mcp_revision(&f.server),
                Body::Mcp { server: f.server.clone() },
            );
            self.write_values(&item.id, &f.values.kept_for(&f.server))?;
            self.put_item(&item)?;
            added.push(item);
        }
        Ok(json!({ "items": added, "warnings": warnings }))
    }

    // ---- Both

    /// Delete an item no role uses. Runs keep what they pinned.
    pub async fn delete_item(&self, id: &str) -> Result<()> {
        let _guard = self.library.lock.lock().await;
        let item = self.item(id)?;
        if let Some(role) = self.roles_using(id)?.first() {
            bail!(
                "数字员工「{}」正在使用 {}，请先在数字员工中取消选择",
                role["name"].as_str().unwrap_or(""),
                item.name
            );
        }
        self.db.delete_config(ITEM, id)?;
        let _ = std::fs::remove_file(self.values_path(id)?);
        // What the MCP router kept for it: its sandbox and its cached tool lists.
        let _ = std::fs::remove_dir_all(self.server_home(id));
        if let Ok(cached) = std::fs::read_dir(self.tools_dir()) {
            for f in cached.flatten().filter(|f| f.file_name().to_string_lossy().starts_with(&format!("{id}-"))) {
                let _ = std::fs::remove_file(f.path());
            }
        }
        self.sweep_store()
    }

    /// Where the MCP router runs a server for everyone: its own HOME and temp dir, kept across runs
    /// so its caches and its own sign-in survive, removed with the item.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    pub(crate) fn server_home(&self, id: &str) -> PathBuf {
        self.cfg.data_dir.join("library").join("servers").join(id)
    }

    /// The MCP router's cache of each server's tools, `<id>-<revision>.json`.
    pub(crate) fn tools_dir(&self) -> PathBuf {
        self.cfg.data_dir.join("library").join("tools")
    }

    fn roles_using(&self, id: &str) -> Result<Vec<Value>> {
        Ok(self
            .db
            .list_config(ROLE)?
            .into_iter()
            .filter(|r| r["resources"].as_array().is_some_and(|a| a.iter().any(|x| x == id)))
            .map(|r| json!({ "id": r["id"], "name": r["name"] }))
            .collect())
    }

    /// The library as the app shows it: each item with the roles using it, the state of its values,
    /// whether it is still in its source, and which runtimes can use it; then the sources.
    pub fn library_view(&self) -> Result<Value> {
        let sources = self.sources()?;
        let items: Vec<Value> = self.items()?.iter().map(|i| self.item_json(i, &sources)).collect::<Result<_>>()?;
        let sources: Vec<Value> = sources.iter().map(|s| self.source_json(s)).collect::<Result<_>>()?;
        Ok(json!({ "items": items, "sources": sources }))
    }

    /// One item, with its SKILL.md when it is a skill.
    pub fn item_view(&self, id: &str) -> Result<Value> {
        let item = self.item(id)?;
        let mut v = self.item_json(&item, &self.sources()?)?;
        if item.kind() == ItemKind::Skill {
            v["content"] = json!(self.skill_text(&item)?);
        }
        Ok(v)
    }

    fn item_json(&self, item: &Item, sources: &[Source]) -> Result<Value> {
        let mut v = serde_json::to_value(item)?;
        v["roles"] = json!(self.roles_using(&item.id)?);
        v["support"] = json!(
            [crate::agents::Runtime::Claude, crate::agents::Runtime::Codex, crate::agents::Runtime::Custom]
                .map(|r| json!({ "runtime": r, "support": support(r, &item.body) }))
        );
        if let Body::Mcp { server } = &item.body {
            let values = self.mcp_values(&item.id);
            let set = |k: &String| {
                match server {
                    McpServer::Stdio { .. } => values.env.get(k),
                    _ => values.headers.get(k),
                }
                .is_some_and(|v| !v.is_empty())
            };
            v["vars"] = json!(server.keys().iter().map(|k| json!({ "name": k, "set": set(k) })).collect::<Vec<_>>());
        }
        if let Origin::Git { source, path } = &item.origin {
            let s = sources.iter().find(|s| &s.id == source);
            v["source"] = json!({
                "url": s.map(|s| s.url.as_str()).unwrap_or(""),
                "inSource": s.is_some_and(|s| s.offers.iter().any(|o| &o.path == path && o.kind == item.kind())),
            });
        }
        Ok(v)
    }
}

/// A skill from the editor: its name, description and full SKILL.md.
fn editor_skill(body: &Value) -> Result<(String, String, String)> {
    let raw = str_of(body, "name");
    let name = skill::skill_name(raw).ok_or_else(|| anyhow!("Skill 名称只能用小写字母、数字和 -：{raw}"))?;
    let content = body["content"].as_str().unwrap_or("").trim();
    if content.is_empty() {
        bail!("Skill 需要内容");
    }
    // A whole SKILL.md keeps its own description; a bare body gets the one from the form.
    let text = skill::with_identity(content, &name, str_of(body, "description"));
    let description = skill::parse_meta(&text, &name).description;
    if description.is_empty() {
        bail!("Skill 需要一句描述：Agent 按它判断什么时候使用这个 Skill");
    }
    Ok((name, description, format!("{}\n", text.trim_end())))
}

/// The folder a skill is copied to in a bundle, and its SKILL.md as the agent reads it.
pub(crate) fn place_skill(store: &Path, item: &Item, dir: &Path) -> Result<()> {
    store::copy_out(store, &item.revision, dir)?;
    let md = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&md)?;
    std::fs::write(md, skill::with_identity(&text, &item.name, &item.description))?;
    Ok(())
}

#[cfg(test)]
mod tests;
