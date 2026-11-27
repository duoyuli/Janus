//! The resource library: skills and MCP servers, kept once and given to every digital worker.
//!
//! An item has an opaque id; its kind, origin and revision are fields. A skill's files are stored
//! by content hash (`store`); an MCP server's definition is in the item and its values in
//! `secrets/library/<id>.json`. A run pins the whole library for each member
//! (`Member::resources`) and copies the skills into per-member bundles (`deliver`), which
//! sessions get at launch; nothing is ever written into an agent's own configuration.
// @project-doc docs/domains/20261116-resource_library.md#model

mod deliver;
pub(crate) mod mcp;
mod offers;
mod quality;
mod skill;
mod skill_list;
mod skill_router;
mod sources;
mod store;

pub use deliver::{Delivery, MCP_ROUTER, SKILL_ROUTER, bundle, launch};
pub use mcp::{McpServer, McpValues};
pub use skill_router::skills_read;
pub use sources::{CHECK_EVERY, Source};

use crate::agents::{str_of, write_private};
use crate::db::ConfigKind;
use crate::orchestrator::App;
use crate::util::new_id;
use crate::util::{expand_home, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Library edits happen one at a time, from any page or the background check.
#[derive(Default)]
pub struct LibraryState {
    lock: tokio::sync::Mutex<()>,
    /// How preparing each MCP server went, by item id, since this service started.
    pub(crate) warm: std::sync::Mutex<HashMap<String, Warm>>,
}

/// Preparing an MCP server ahead of its runs (`App::warm_mcp`): a command started once in its
/// sandbox so its packages are downloaded, a URL reached once; either way its tools are known and
/// its configuration is known to work.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Warm {
    Preparing { revision: String },
    Ready { revision: String, tools: usize },
    Failed { revision: String, error: String },
}

impl Warm {
    pub fn revision(&self) -> &str {
        match self {
            Warm::Preparing { revision } | Warm::Ready { revision, .. } | Warm::Failed { revision, .. } => revision,
        }
    }
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

/// The file beside a skill's SKILL.md that `run_code` runs as `skills.<name>(args)`, and the
/// largest one read: a script, not a bundle.
pub const SKILL_CODE: &str = "code.js";
const SKILL_CODE_MAX: u64 = 256 * 1024;
/// The id of the MCP router's one row of settings.
const ROUTER: &str = "settings";

/// The MCP router's settings: whether it gives `run_code`.
#[derive(Serialize, Deserialize)]
struct RouterSettings {
    code: bool,
}

impl crate::db::Setting for RouterSettings {
    const KIND: ConfigKind = ConfigKind::McpRouter;
    fn id(&self) -> &str {
        ROUTER
    }
}

/// The board MCP's and the router's names are reserved, in any case and with any punctuation
/// around them.
pub(crate) fn reserved_mcp_name(name: &str) -> bool {
    ["janus", MCP_ROUTER]
        .iter()
        .any(|r| name.eq_ignore_ascii_case(r) || crate::text::slug(name, 32).is_some_and(|s| s == *r))
}

fn mcp_revision(server: &McpServer) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(serde_json::to_vec(server).unwrap_or_default()).iter().map(|b| format!("{b:02x}")).collect()
}

impl App {
    /// Skill contents, by hash (`Layout::skills`).
    pub(crate) fn store_dir(&self) -> PathBuf {
        self.cfg.layout.skills()
    }

    fn values_path(&self, id: &str) -> Result<PathBuf> {
        crate::agents::check_id(id)?;
        Ok(self.cfg.layout.mcp_values(id))
    }

    pub fn items(&self) -> Result<Vec<Item>> {
        self.db
            .list_config(ConfigKind::LibraryItem)?
            .into_iter()
            .map(|v| serde_json::from_value(v).context("资源记录无法读取"))
            .collect()
    }

    pub fn item(&self, id: &str) -> Result<Item> {
        let v = self.db.get_config(ConfigKind::LibraryItem, id)?.ok_or_else(|| anyhow!("资源 {id} 不存在"))?;
        serde_json::from_value(v).context("资源记录无法读取")
    }

    fn put_item(&self, item: &Item) -> Result<()> {
        self.db.put_config(ConfigKind::LibraryItem, &item.id, &serde_json::to_value(item)?, now_ms())
    }

    /// A server's values; none when it has no file (or the id is not one).
    pub fn mcp_values(&self, id: &str) -> McpValues {
        self.values_path(id)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// The values a server's variables hold, for the editor that shows and changes them. Only this
    /// call returns values: the library's views, run snapshots and events keep names only.
    // @project-doc docs/operations/20261031-security.md#secrets
    pub fn mcp_values_view(&self, id: &str) -> Result<Value> {
        let item = self.item(id)?;
        let Body::Mcp { server } = &item.body else { bail!("这个资源不是 MCP 服务器") };
        let mut values = self.mcp_values(id);
        let held = values.of(server);
        let out: serde_json::Map<String, Value> =
            server.keys().iter().filter_map(|k| held.get(k).map(|v| (k.clone(), json!(v)))).collect();
        Ok(json!({ "values": out }))
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
        Item { id: new_id("i"), name, description, origin, revision, updated_at: now_ms(), body }
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

    /// Only skills written in Janus are edited here; others change where they came from. A skill
    /// saved from `run_code` keeps its code.
    pub async fn edit_skill(&self, id: &str, body: &Value) -> Result<Item> {
        let _guard = self.library.lock.lock().await;
        let mut item = self.item(id)?;
        if item.kind() != ItemKind::Skill || item.origin != Origin::Created {
            bail!("只能编辑在 Janus 中新建的 Skill；来自目录或 Git 来源的 Skill 请在原处修改，再刷新或更新来源");
        }
        let (name, description, text) = editor_skill(body)?;
        self.check_free_name(ItemKind::Skill, &name, Some(id))?;
        let code = self.skill_code(&item);
        let mut files = vec![("SKILL.md", text.as_str())];
        if let Some(code) = &code {
            files.push((SKILL_CODE, code));
        }
        item.revision = store::put_text(&self.store_dir(), &files)?;
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

    /// A skill's `code.js`, when it has one no larger than `SKILL_CODE_MAX`: a function the MCP
    /// router's `run_code` calls as `skills.<name>(args)`.
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    pub fn skill_code(&self, item: &Item) -> Option<String> {
        let path = self.store_dir().join(&item.revision).join(SKILL_CODE);
        let meta = std::fs::metadata(&path).ok()?;
        (item.body == Body::Skill && meta.is_file() && meta.len() <= SKILL_CODE_MAX)
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
    }

    /// Save a script `run_code` ran without an error as a skill written in Janus (`save_as`): a
    /// SKILL.md saying how to run it, in the app's language, and the script as its `code.js`.
    /// Like every library skill it is for every digital employee, from the next run. A name the
    /// library has is refused, never written over. The app hears of it (`Change::Library`).
    pub async fn save_code_skill(&self, name: &str, description: &str, code: &str) -> Result<Item> {
        let name = skill::skill_name(name).ok_or_else(|| anyhow!("Skill 名称只能用小写字母、数字和 -：{name}"))?;
        if description.trim().is_empty() {
            bail!("Skill 需要一句描述：写明它做什么、什么时候用、要哪些 args");
        }
        if code.len() as u64 > SKILL_CODE_MAX {
            bail!("代码超过 {} KB，存不成 Skill", SKILL_CODE_MAX / 1024);
        }
        crate::coderun::skill_source(code)?;
        let _guard = self.library.lock.lock().await;
        self.check_free_name(ItemKind::Skill, &name, None)?;
        let key = name.replace('-', "_");
        let body = crate::lang::tr!(
            self.language(),
            "用 Janus 的 MCP 路由运行：它的 run_code 工具以 `skills.{key}(args)` 调用这个 Skill，args 按上面的描述；代码自己调用路由的工具。代码是这个目录里的 {SKILL_CODE}。",
            "Run it through Janus's MCP router: its run_code tool calls this skill as `skills.{key}(args)`, with the args its description names, and the code calls the router's tools itself. Its code is {SKILL_CODE}, in this folder."
        );
        let text = format!("{}\n", skill::with_identity(&body, &name, description).trim_end());
        let revision = store::put_text(&self.store_dir(), &[("SKILL.md", &text), (SKILL_CODE, code)])?;
        let item = self.new_item(name, skill::short(description), Origin::Created, revision, Body::Skill);
        self.put_item(&item)?;
        let _ = self.changes.send(crate::orchestrator::Change::Library);
        Ok(item)
    }

    /// Whether the MCP router gives `run_code` (code mode); on until the person turns it off.
    pub fn code_mode(&self) -> bool {
        self.db.get::<RouterSettings>(ROUTER).ok().flatten().is_none_or(|s| s.code)
    }

    /// Turn code mode on or off, for the sessions that start from now on.
    pub fn save_code_mode(&self, body: &Value) -> Result<Value> {
        let code = body["code"].as_bool().ok_or_else(|| anyhow!("code 只能是 true 或 false"))?;
        self.db.put(&RouterSettings { code }, now_ms())?;
        Ok(json!({ "code": code }))
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
        // A variable's value: a string sets it (an empty one clears it); none keeps the saved
        // value, for a client that never read it.
        let vars: Vec<(String, Option<String>)> = body["vars"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                let k = str_of(v, "name");
                (!k.is_empty()).then(|| (k.to_string(), v["value"].as_str().map(str::to_owned)))
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
            transport => McpServer::remote(transport, str_of(body, "url").to_string(), keys)?,
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
        for (k, v) in vars {
            match v {
                Some(v) if !v.is_empty() => values.of(&server).insert(k, v),
                Some(_) => values.of(&server).remove(&k),
                None => None,
            };
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
        // A server running in a run picks up its new values or command on its next start.
        self.stop_item_servers(&item.id);
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

    /// Delete an item. Runs keep what they pinned.
    pub async fn delete_item(&self, id: &str) -> Result<()> {
        let _guard = self.library.lock.lock().await;
        self.item(id)?;
        self.db.delete_config(ConfigKind::LibraryItem, id)?;
        crate::agents::check_id(id)?;
        // Its servers stop before the sandbox they run in goes; then everything it owns goes.
        self.stop_item_servers(id);
        for path in self.cfg.layout.item_paths(id) {
            crate::layout::remove(&path);
        }
        self.sweep_store()
    }

    /// The library as the app shows it: each item with the state of its values and whether it is
    /// still in its source, each skill with how often it was used and how findable it is; then
    /// the sources.
    pub fn library_view(&self) -> Result<Value> {
        let sources = self.sources()?;
        let all = self.items()?;
        let notes = self.skill_notes(&all)?;
        let items: Vec<Value> = all.iter().map(|i| self.item_json(i, &sources, &notes)).collect::<Result<_>>()?;
        let skills: Vec<&Item> = all.iter().filter(|i| i.kind() == ItemKind::Skill).collect();
        let used: HashMap<String, u64> = notes.uses.iter().map(|(id, (n, _))| (id.clone(), *n)).collect();
        let order = skill_list::catalog_order(&skills, &used);
        let source_ids: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
        let groups = skill_list::groups(&skills, &source_ids);
        let sources: Vec<Value> = sources.iter().map(|s| self.source_json(s)).collect::<Result<_>>()?;
        let catalog = if notes.listed { "list" } else { "search" };
        Ok(json!({
            "items": items,
            "sources": sources,
            "skillRouter": { "catalog": catalog, "order": order },
            "skillGroups": groups,
            "mcpRouter": { "code": self.code_mode() },
        }))
    }

    /// One item, with its SKILL.md when it is a skill.
    pub fn item_view(&self, id: &str) -> Result<Value> {
        let item = self.item(id)?;
        let notes = self.skill_notes(&self.items()?)?;
        let mut v = self.item_json(&item, &self.sources()?, &notes)?;
        if item.kind() == ItemKind::Skill {
            v["content"] = json!(self.skill_text(&item)?);
        }
        Ok(v)
    }

    /// What the library says about its skills beyond their fields: the sessions that read each
    /// (`skill.used`), and the hints on its description, judged against all the others.
    // @project-doc docs/domains/20261116-resource_library.md#skill_hints
    fn skill_notes(&self, items: &[Item]) -> Result<SkillNotes> {
        let mut entries = vec![];
        for item in items.iter().filter(|i| i.body == Body::Skill) {
            let md = self.skill_text(item).unwrap_or_default();
            entries.push((item.id.clone(), skill_router::Entry::read(item, &md)));
        }
        let listed = skill_router::listed(entries.iter().map(|(_, e)| e), self.language());
        Ok(SkillNotes { uses: self.db.skill_uses()?, hints: quality::skill_hints(&entries), listed })
    }

    fn item_json(&self, item: &Item, sources: &[Source], notes: &SkillNotes) -> Result<Value> {
        let mut v = serde_json::to_value(item)?;
        if item.body == Body::Skill {
            let (sessions, last) = notes.uses.get(&item.id).copied().map_or((0, None), |(n, at)| (n, Some(at)));
            v["uses"] = json!({ "sessions": sessions, "lastAt": last });
            v["hints"] = json!(notes.hints.get(&item.id).cloned().unwrap_or_default());
        }
        if let Body::Mcp { server } = &item.body {
            let values = self.mcp_values(&item.id);
            let set = |k: &String| values.for_server(server).get(k).is_some_and(|v| !v.is_empty());
            v["vars"] = json!(server.keys().iter().map(|k| json!({ "name": k, "set": set(k) })).collect::<Vec<_>>());
            if let Some(w) = self.library.warm.lock().unwrap().get(&item.id).filter(|w| w.revision() == item.revision) {
                v["warm"] = json!(w);
            }
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

/// Per skill, by item id: how many sessions read it and when last, and the hints on its
/// description; and whether the router's catalog lists them all or is searched.
struct SkillNotes {
    uses: HashMap<String, (u64, i64)>,
    hints: HashMap<String, Vec<quality::Hint>>,
    listed: bool,
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
