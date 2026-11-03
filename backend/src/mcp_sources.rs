//! MCP sources: GitHub repositories whose MCP server definitions sit beside the agents' own
//! tools, the way Skill sources sit beside the Skill library.
//!
//! Adding a link clones the repository (shallow) into `<home>/mcp-sources/<id>/repo`, finds every
//! `mcp.json` / `.mcp.json`, and indexes the servers they declare. A role mounts
//! `<source id>/<key>`; the key stays with its manifest entry across updates. Environment values
//! and HTTP header values live only in `<home>/secrets/mcp/<source id>/<key>.json` (0600), never in
//! `source.json`, SQLite, the API or a run snapshot.

use crate::agents::{ROLE, check_skill_id, reserved_mcp_name, slug, str_of, write_private};
use crate::orchestrator::{App, Member, now_ms};
use crate::skill_sources::{RepoAt, git_in, parse_github, remote_head};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Sources are cloned and swapped one at a time, so two updates never race on one folder.
static SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How deep a manifest may sit in a repository, and how many servers one repository may bring.
const MAX_DEPTH: usize = 8;
const MAX_SERVERS: usize = 500;
/// How often the server asks each source's remote whether it moved on.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// One server a manifest declares, before it gets its stable key.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundServer {
    /// The manifest file, relative to the repository.
    pub manifest: String,
    /// The name the manifest gave it.
    pub title: String,
    /// `stdio` or `http`.
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    /// Environment values from the manifest, in order. They are written to secrets at sync time.
    pub env: Vec<(String, String)>,
    /// HTTP header values from the manifest, in order.
    pub headers: Vec<(String, String)>,
    pub description: String,
}

/// Every folder under `start` holding `mcp.json` or `.mcp.json`, relative to `repo`. Symbolic
/// links are never followed; `.git` and `node_modules` are skipped.
pub fn manifest_files(repo: &Path, start: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > MAX_DEPTH {
            return;
        }
        for name in ["mcp.json", ".mcp.json"] {
            let p = dir.join(name);
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                out.push(p);
            }
        }
        let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !matches!(e.file_name().to_str(), Some(".git" | "node_modules")))
            .map(|e| e.path())
            .collect();
        subs.sort();
        for s in subs {
            walk(&s, depth + 1, out);
        }
    }
    let mut out = vec![];
    if std::fs::symlink_metadata(start).is_ok_and(|m| m.is_dir()) && crate::skill_sources::inside(repo, start) {
        walk(start, 0, &mut out);
    }
    out
}

/// The servers a set of manifests declares, plus warnings for the entries that could not be read.
/// A manifest may use Claude's `mcpServers`, VS Code's `servers`, or one server object with a
/// `name`.
pub fn scan(repo: &Path, start: &Path) -> (Vec<FoundServer>, Vec<Value>) {
    let mut servers = vec![];
    let mut warnings = vec![];
    for file in manifest_files(repo, start) {
        let rel = file.strip_prefix(repo).unwrap_or(&file).to_string_lossy().to_string();
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(json!({ "manifest": rel, "error": format!("读取失败：{e}") }));
                continue;
            }
        };
        match parse_manifest(&text, &rel) {
            Ok((mut found, mut warns)) => {
                servers.append(&mut found);
                warnings.append(&mut warns);
            }
            Err(e) => warnings.push(json!({ "manifest": rel, "error": format!("{e:#}") })),
        }
        if servers.len() >= MAX_SERVERS {
            servers.truncate(MAX_SERVERS);
            warnings.push(json!({ "manifest": "", "error": format!("服务器超过 {MAX_SERVERS} 个，其余已跳过") }));
            break;
        }
    }
    servers.sort_by(|a, b| (a.manifest.as_str(), a.title.as_str()).cmp(&(b.manifest.as_str(), b.title.as_str())));
    (servers, warnings)
}

/// Read one manifest. Entries that cannot be used are reported as warnings; a file without a
/// recognized shape is an error.
pub fn parse_manifest(text: &str, rel: &str) -> Result<(Vec<FoundServer>, Vec<Value>)> {
    let doc: Value = serde_json::from_str(text).with_context(|| format!("{rel} 不是有效的 JSON"))?;
    let obj = doc.as_object().context("manifest 必须是一个 JSON 对象")?;
    let entries: Vec<(String, Value)> = if let Some(m) = obj.get("mcpServers").and_then(Value::as_object) {
        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else if let Some(m) = obj.get("servers").and_then(Value::as_object) {
        m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else if doc["name"].as_str().is_some_and(|n| !n.trim().is_empty())
        && (doc["command"].is_string() || doc["url"].is_string())
    {
        vec![(str_of(&doc, "name").to_string(), doc.clone())]
    } else {
        bail!("{rel} 里没有 mcpServers、servers 或单个服务器定义");
    };
    let mut out = vec![];
    let mut warnings = vec![];
    for (name, def) in entries {
        match parse_server(rel, &name, &def) {
            Ok(s) => out.push(s),
            Err(e) => warnings.push(json!({ "manifest": rel, "server": name, "error": format!("{e:#}") })),
        }
    }
    Ok((out, warnings))
}

fn parse_server(rel: &str, name: &str, def: &Value) -> Result<FoundServer> {
    let title = name.trim().to_string();
    if title.is_empty() {
        bail!("服务器没有名字");
    }
    if reserved_mcp_name(&title) {
        bail!("名字 {title} 规范化后是 janus，保留给 Janus 看板 MCP");
    }
    let ty = str_of(def, "type").to_ascii_lowercase();
    if ty == "sse" {
        bail!("不支持 SSE 传输，只支持 stdio 和 streamable HTTP");
    }
    let command = str_of(def, "command").to_string();
    let url = str_of(def, "url").to_string();
    let transport = if matches!(ty.as_str(), "http" | "streamable-http") || (command.is_empty() && !url.is_empty()) {
        "http"
    } else {
        "stdio"
    };
    let mut server = FoundServer {
        manifest: rel.to_string(),
        title,
        transport: transport.to_string(),
        command: String::new(),
        args: vec![],
        url: String::new(),
        env: vec![],
        headers: vec![],
        description: str_of(def, "description").to_string(),
    };
    if transport == "http" {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            bail!("服务器地址必须以 http:// 或 https:// 开头");
        }
        server.url = url;
        server.headers = pairs(def, "headers");
    } else {
        if command.is_empty() {
            bail!("stdio 服务器没有启动命令");
        }
        server.command = command;
        server.args = def["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        server.env = pairs(def, "env");
    }
    Ok(server)
}

/// A JSON object of scalar values, as ordered string pairs.
fn pairs(def: &Value, field: &str) -> Vec<(String, String)> {
    def[field]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    let k = k.trim();
                    if k.is_empty() {
                        return None;
                    }
                    let v = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k.to_string(), v))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Give each manifest entry a key: the one it had before when its manifest and name are unchanged,
/// else its name's slug, made unique. `name` is the key, so every server in a source has a stable,
/// valid ACP name.
pub fn assign_keys(found: &[FoundServer], before: &[Value]) -> Vec<(String, FoundServer)> {
    // First pass: every entry that existed before keeps its key, and those keys are reserved
    // before any new entry picks one. Otherwise a new entry sorted ahead of an old one could take
    // the old one's key and two servers would share an id.
    let mut taken: Vec<String> = vec![];
    let mut used_previous: Vec<usize> = vec![];
    let kept: Vec<Option<String>> = found
        .iter()
        .map(|server| {
            let (i, key) = before.iter().enumerate().find_map(|(i, b)| {
                let key = b["key"].as_str()?;
                // A key recorded before the reserved-name check existed is replaced, not carried on.
                (!used_previous.contains(&i)
                    && b["manifest"] == server.manifest.as_str()
                    && b["title"] == server.title.as_str()
                    && !reserved_mcp_name(key)
                    && !taken.iter().any(|t| t == key))
                .then(|| (i, key.to_string()))
            })?;
            used_previous.push(i);
            taken.push(key.clone());
            Some(key)
        })
        .collect();
    found
        .iter()
        .zip(kept)
        .map(|(server, kept)| {
            let key = kept.unwrap_or_else(|| {
                let base = slug(&server.title)
                    .filter(|k| !reserved_mcp_name(k))
                    .unwrap_or_else(|| "mcp".into());
                let key = (1..)
                    .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
                    .find(|k| !taken.contains(k))
                    .unwrap();
                taken.push(key.clone());
                key
            });
            (key, server.clone())
        })
        .collect()
}

fn entries(items: &[(String, FoundServer)]) -> Vec<Value> {
    items
        .iter()
        .map(|(key, s)| {
            json!({
                "key": key, "name": key, "title": s.title, "manifest": s.manifest,
                "transport": s.transport, "command": s.command, "args": s.args, "url": s.url,
                "envKeys": s.env.iter().map(|(k, _)| k).collect::<Vec<_>>(),
                "headerKeys": s.headers.iter().map(|(k, _)| k).collect::<Vec<_>>(),
                "description": s.description,
            })
        })
        .collect()
}

/// Split `<source id>/<key>`.
fn split_id(id: &str) -> Option<(&str, &str)> {
    let (source, key) = id.split_once('/')?;
    check_skill_id(source).ok()?;
    check_skill_id(key).ok()?;
    Some((source, key))
}

fn secret_object<'a>(v: &'a mut Value, field: &str) -> &'a mut Map<String, Value> {
    if !v[field].is_object() {
        v[field] = json!({});
    }
    v[field].as_object_mut().unwrap()
}

fn read_secret(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn set_keys(secret: &Value, field: &str, keys: &[Value]) -> Vec<Value> {
    keys.iter()
        .filter(|k| {
            k.as_str()
                .is_some_and(|k| secret[field][k].as_str().is_some_and(|v| !v.is_empty()))
        })
        .cloned()
        .collect()
}

impl App {
    pub fn mcp_sources_root(&self) -> PathBuf {
        self.cfg.data_dir.join("mcp-sources")
    }

    pub fn mcp_secrets_root(&self) -> PathBuf {
        self.cfg.data_dir.join("secrets").join("mcp")
    }

    fn mcp_secret_path(&self, source: &str, key: &str) -> PathBuf {
        self.mcp_secrets_root().join(source).join(format!("{key}.json"))
    }

    fn read_mcp_source(&self, id: &str) -> Option<Value> {
        check_skill_id(id).ok()?;
        let text = std::fs::read_to_string(self.mcp_sources_root().join(id).join("source.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn mcp_source_list(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(self.mcp_sources_root())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.read_mcp_source(&e.file_name().to_string_lossy()))
            .collect();
        out.sort_by_key(|s| s["addedAt"].as_i64().unwrap_or(0));
        out
    }

    /// The servers a source brings, as the settings list shows them: no values, only key names
    /// and which of them are set.
    fn source_servers(&self, src: &Value) -> Vec<Value> {
        let id = str_of(src, "id");
        src["servers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let key = str_of(s, "key");
                if key.is_empty() {
                    return None;
                }
                let secret = read_secret(&self.mcp_secret_path(id, key));
                let mut v = s.clone();
                v["id"] = json!(format!("{id}/{key}"));
                v["sourceId"] = json!(id);
                v["source"] = json!(str_of(src, "repo"));
                v["envSet"] = json!(set_keys(&secret, "env", s["envKeys"].as_array().map(Vec::as_slice).unwrap_or(&[])));
                v["headerSet"] = json!(set_keys(&secret, "headers", s["headerKeys"].as_array().map(Vec::as_slice).unwrap_or(&[])));
                Some(v)
            })
            .collect()
    }

    pub fn list_mcp_sources(&self) -> Value {
        let list: Vec<Value> = self
            .mcp_source_list()
            .into_iter()
            .map(|s| {
                let servers = self.source_servers(&s);
                let mut s = s;
                s["servers"] = json!(servers);
                s
            })
            .collect();
        json!({ "sources": list, "root": self.mcp_sources_root() })
    }

    /// One library resource as the role editor and the role saver see it.
    pub fn mcp_resource(&self, id: &str) -> Option<Value> {
        let (source, key) = split_id(id)?;
        let src = self.read_mcp_source(source)?;
        self.source_servers(&src).into_iter().find(|s| s["key"] == key)
    }

    /// The resource as a run's member carries it: what to start or reach, and the names of its
    /// values, never the values themselves.
    pub fn mcp_info(&self, id: &str) -> Option<Value> {
        let resource = self.mcp_resource(id)?;
        let mut v = resource.clone();
        v["mcpId"] = json!(id);
        Some(v)
    }

    /// Values saved for one resource. `{"env": {...}, "headers": {...}}`; missing fields are empty.
    pub fn mcp_secret(&self, id: &str) -> Value {
        let Some((source, key)) = split_id(id) else { return json!({}) };
        let mut secret = read_secret(&self.mcp_secret_path(source, key));
        for field in ["env", "headers"] {
            secret_object(&mut secret, field);
        }
        secret
    }

    /// Set or clear values for one resource. A key with an empty value is removed; keys not
    /// mentioned keep their value. Returns the resource as the library lists it.
    pub fn write_mcp_secret(&self, id: &str, body: &Value) -> Result<Value> {
        let (source, key) = split_id(id).context("没有这个 MCP 资源")?;
        if self.mcp_resource(id).is_none() {
            bail!("没有这个 MCP 资源");
        }
        let path = self.mcp_secret_path(source, key);
        let mut secret = self.mcp_secret(id);
        for field in ["env", "headers"] {
            let Some(values) = body[field].as_object() else { continue };
            for (k, v) in values {
                let k = k.trim();
                if k.is_empty() || k.contains(['=', '\n', '\r']) {
                    bail!("变量名无效：{k}");
                }
                let value = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => String::new(),
                    _ => bail!("变量 {k} 的值必须是字符串"),
                };
                if value.is_empty() {
                    secret[field].as_object_mut().unwrap().remove(k);
                } else {
                    secret[field].as_object_mut().unwrap().insert(k.to_string(), json!(value));
                }
            }
        }
        let empty = secret["env"].as_object().is_none_or(Map::is_empty)
            && secret["headers"].as_object().is_none_or(Map::is_empty);
        if empty {
            let _ = std::fs::remove_file(&path);
        } else {
            let dir = path.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.parent().unwrap(), std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            write_private(&path, serde_json::to_string_pretty(&secret)?.as_bytes())?;
        }
        self.mcp_resource(id).context("没有这个 MCP 资源")
    }

    /// Put a role's MCP values in reach of its sessions: the role's own servers from
    /// `secrets/roles`, and library resources from `secrets/mcp`. Both maps are skipped by
    /// serialization, so they never enter the run record.
    pub fn attach_mcp_secrets(&self, m: &mut Member) {
        let mut env = self.role_secrets_of(&m.role_id);
        if !env.is_object() {
            env = json!({});
        }
        let mut headers = json!({});
        for s in &m.mcp {
            let name = str_of(s, "name");
            if name.is_empty() {
                continue;
            }
            let Some(id) = s["mcpId"].as_str() else { continue };
            let secret = self.mcp_secret(id);
            if secret["env"].as_object().is_some_and(|o| !o.is_empty()) {
                env[name] = secret["env"].clone();
            }
            if secret["headers"].as_object().is_some_and(|o| !o.is_empty()) {
                headers[name] = secret["headers"].clone();
            }
        }
        m.mcp_env = env;
        m.mcp_headers = headers;
    }

    /// Add a GitHub link: clone it and index its MCP servers.
    pub async fn add_mcp_source(&self, link: &str) -> Result<Value> {
        let at = parse_github(link)?;
        let clone = format!("https://github.com/{}.git", at.repo);
        self.add_mcp_source_at(at, &clone).await
    }

    pub(crate) async fn add_mcp_source_at(&self, at: RepoAt, clone_url: &str) -> Result<Value> {
        let _guard = SYNC.lock().await;
        if let Some(s) = self.mcp_source_list().into_iter().find(|s| {
            s["repo"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(&at.repo))
                && s["ref"] == at.git_ref.as_str()
                && s["path"] == at.path.as_str()
        }) {
            bail!("已经添加过这个来源：{}", str_of(&s, "url"));
        }
        let base = slug(&format!(
            "{}{}",
            at.repo.replace('/', "-"),
            if at.path.is_empty() { String::new() } else { format!("-{}", at.path.rsplit('/').next().unwrap_or("")) }
        ))
        .unwrap_or_else(|| "mcp".into());
        let root = self.mcp_sources_root();
        let id = (1..1000)
            .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
            .find(|id| !root.join(id).exists())
            .context("没有可用的来源 ID")?;
        let src = json!({
            "id": id, "url": at.url(), "repo": at.repo, "ref": at.git_ref, "path": at.path,
            "cloneUrl": clone_url, "addedAt": now_ms(), "servers": [], "warnings": [],
        });
        std::fs::create_dir_all(root.join(&id))?;
        match self.sync_mcp_source(src).await {
            Ok(v) => Ok(v),
            Err(e) => {
                let _ = std::fs::remove_dir_all(root.join(&id));
                Err(e)
            }
        }
    }

    /// Fetch the source again: its servers follow the repository, keys stay with their entries,
    /// and values already saved by the person win over the manifest's.
    pub async fn update_mcp_source(&self, id: &str) -> Result<Value> {
        let _guard = SYNC.lock().await;
        let src = self.read_mcp_source(id).context("没有这个来源")?;
        self.sync_mcp_source(src).await
    }

    /// Ask every source's remote for its current commit, without fetching anything.
    pub async fn check_mcp_sources(&self) -> Value {
        for src in self.mcp_source_list() {
            let id = str_of(&src, "id").to_string();
            let git_ref = str_of(&src, "ref").to_string();
            let target = if git_ref.is_empty() { "HEAD".to_string() } else { git_ref.clone() };
            let url = str_of(&src, "cloneUrl").to_string();
            let dir = self.mcp_sources_root().join(&id);
            let answer = git_in(&dir, &["ls-remote", "--", &url, &target]).await;
            // Record under the lock, on top of whatever an update wrote meanwhile.
            let _guard = SYNC.lock().await;
            let Some(mut now) = self.read_mcp_source(&id) else { continue };
            now["checkedAt"] = json!(now_ms());
            match answer.map(|out| remote_head(&out, &git_ref)) {
                Ok(Some(sha)) => {
                    now["updateAvailable"] = json!(now["commit"].as_str().is_some_and(|c| !c.is_empty() && c != sha));
                    now["latest"] = json!(sha);
                    now["checkError"] = json!("");
                }
                Ok(None) => now["checkError"] = json!(format!("远端没有 {target}")),
                Err(e) => now["checkError"] = json!(format!("{e:#}")),
            }
            let _ = std::fs::write(dir.join("source.json"), serde_json::to_string_pretty(&now).unwrap_or_default());
        }
        self.list_mcp_sources()
    }

    async fn sync_mcp_source(&self, mut src: Value) -> Result<Value> {
        let id = str_of(&src, "id").to_string();
        let dir = self.mcp_sources_root().join(&id);
        let fresh = dir.join(format!(".repo-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&fresh);
        let fresh_s = fresh.to_string_lossy().to_string();
        let mut args = vec!["clone", "--depth", "1", "--quiet", "--no-tags"];
        let git_ref = str_of(&src, "ref").to_string();
        if !git_ref.is_empty() {
            args.extend(["--branch", git_ref.as_str()]);
        }
        let url = str_of(&src, "cloneUrl").to_string();
        args.extend(["--", url.as_str(), fresh_s.as_str()]);
        let result: Result<(Value, Vec<(String, FoundServer)>)> = async {
            git_in(&dir, &args).await.context("拉取仓库失败")?;
            let commit = git_in(&fresh, &["rev-parse", "HEAD"]).await.unwrap_or_default();
            let (found, warnings) = scan(&fresh, &fresh.join(str_of(&src, "path")));
            if found.is_empty() {
                let why = warnings
                    .first()
                    .and_then(|w| w["error"].as_str())
                    .map(|e| format!("：{e}"))
                    .unwrap_or_default();
                bail!("这个位置没有找到可用的 mcp.json 或 .mcp.json{why}");
            }
            let before = src["servers"].as_array().cloned().unwrap_or_default();
            let items = assign_keys(&found, &before);
            src["servers"] = json!(entries(&items));
            src["warnings"] = json!(warnings);
            src["commit"] = json!(commit);
            src["updatedAt"] = json!(now_ms());
            src["checkedAt"] = json!(now_ms());
            src["latest"] = json!(commit);
            src["updateAvailable"] = json!(false);
            src["checkError"] = json!("");
            Ok((src.clone(), items))
        }
        .await;
        let (src, items) = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&fresh);
                return Err(e);
            }
        };
        // Swap the checkout in, then record it and the manifest's values (existing ones win).
        let repo = dir.join("repo");
        let old = dir.join(".repo-old");
        let _ = std::fs::remove_dir_all(&old);
        if repo.exists() {
            std::fs::rename(&repo, &old)?;
        }
        std::fs::rename(&fresh, &repo)?;
        let _ = std::fs::remove_dir_all(&old);
        self.merge_manifest_secrets(&id, &items)?;
        std::fs::write(dir.join("source.json"), serde_json::to_string_pretty(&src)?)?;
        let mut out = src;
        out["servers"] = json!(self.source_servers(&out));
        Ok(out)
    }

    /// Keep the values the person already set; add the keys a manifest brings, with its values.
    fn merge_manifest_secrets(&self, source: &str, items: &[(String, FoundServer)]) -> Result<()> {
        for (key, server) in items {
            let path = self.mcp_secret_path(source, key);
            let mut secret = self.mcp_secret(&format!("{source}/{key}"));
            for (k, v) in &server.env {
                secret_object(&mut secret, "env").entry(k.clone()).or_insert_with(|| json!(v));
            }
            for (k, v) in &server.headers {
                secret_object(&mut secret, "headers").entry(k.clone()).or_insert_with(|| json!(v));
            }
            let empty = secret["env"].as_object().is_none_or(Map::is_empty)
                && secret["headers"].as_object().is_none_or(Map::is_empty);
            if empty {
                continue;
            }
            let dir = path.parent().unwrap();
            std::fs::create_dir_all(dir)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.parent().unwrap(), std::fs::Permissions::from_mode(0o700))?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            write_private(&path, serde_json::to_string_pretty(&secret)?.as_bytes())?;
        }
        Ok(())
    }

    pub fn delete_mcp_source(&self, id: &str) -> Result<()> {
        let src = self.read_mcp_source(id).context("没有这个来源")?;
        let prefix = format!("{id}/");
        let users: Vec<String> = self
            .db
            .list_config(ROLE)?
            .iter()
            .filter(|r| {
                r["mcpRefs"]
                    .as_array()
                    .is_some_and(|s| s.iter().any(|x| x.as_str().is_some_and(|x| x.starts_with(&prefix))))
            })
            .map(|r| str_of(r, "name").to_string())
            .collect();
        if !users.is_empty() {
            bail!("角色「{}」挂载了 {} 中的 MCP，请先移除", users.join("」「"), str_of(&src, "repo"));
        }
        // An unfinished run carries a snapshot of the definitions and reads their values at each
        // session; deleting the source now would take those values away from a resumed session.
        // `runs_may_use_mcp` reads the whole table, not the app's newest-50 page, so an older
        // interrupted or failed run still blocks the deletion.
        if let Some(run) = self.db.runs_may_use_mcp()?.iter().find(|r| {
            r["config"]["team"].as_array().is_some_and(|team| {
                team.iter().any(|m| {
                    m["mcp"].as_array().is_some_and(|servers| {
                        servers.iter().any(|s| s["mcpId"].as_str().is_some_and(|m| m.starts_with(&prefix)))
                    })
                })
            })
        }) {
            bail!("Run「{}」还在使用这个来源中的 MCP，等它结束或删除后再删来源", str_of(run, "id"));
        }
        std::fs::remove_dir_all(self.mcp_sources_root().join(id)).context("删除来源失败")?;
        let _ = std::fs::remove_dir_all(self.mcp_secrets_root().join(id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::test_app;
    use serde_json::json;

    #[test]
    fn manifests_in_both_common_shapes_are_read() {
        let claude = r#"{
            "mcpServers": {
                "filesystem": { "command": "npx", "args": ["-y", "@mcp/fs", "."], "env": { "ROOT": "/tmp", "EMPTY": "" } },
                "docs": { "type": "http", "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer t" } },
                "old": { "type": "sse", "url": "https://example.com/sse" }
            }
        }"#;
        let (found, warnings) = parse_manifest(claude, "mcp.json").unwrap();
        assert_eq!(found.len(), 2, "{warnings:?}");
        let fs = found.iter().find(|s| s.title == "filesystem").unwrap();
        assert_eq!(fs.transport, "stdio");
        assert_eq!(fs.args, ["-y", "@mcp/fs", "."]);
        // serde_json keeps object keys sorted without the preserve_order feature.
        assert_eq!(fs.env, vec![("EMPTY".into(), "".into()), ("ROOT".into(), "/tmp".into())]);
        let docs = found.iter().find(|s| s.title == "docs").unwrap();
        assert_eq!(docs.transport, "http");
        assert_eq!(docs.url, "https://example.com/mcp");
        assert_eq!(docs.headers, vec![("Authorization".into(), "Bearer t".into())]);
        assert_eq!(warnings.len(), 1, "SSE is reported, not used");

        let vscode = r#"{ "servers": { "fetch": { "type": "stdio", "command": "uvx", "args": ["mcp-server-fetch"] } } }"#;
        let (found, warnings) = parse_manifest(vscode, ".vscode/mcp.json").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(found[0].command, "uvx");

        let single = r#"{ "name": "search", "url": "http://127.0.0.1:9000/mcp" }"#;
        assert_eq!(parse_manifest(single, "server.json").unwrap().0[0].transport, "http");
        assert!(parse_manifest(r#"{"name":"x"}"#, "mcp.json").is_err());
    }

    #[test]
    fn keys_stay_with_their_manifest_entries() {
        let server = |manifest: &str, title: &str| FoundServer {
            manifest: manifest.into(),
            title: title.into(),
            transport: "stdio".into(),
            command: "run".into(),
            args: vec![],
            url: String::new(),
            env: vec![],
            headers: vec![],
            description: String::new(),
        };
        let first = assign_keys(&[server("a/mcp.json", "fetch"), server("b/mcp.json", "fetch"), server("", "My Tool")], &[]);
        let keys: Vec<&str> = first.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["fetch", "fetch-2", "my-tool"]);
        // b's fetch keeps fetch-2 although a's is gone and a new fetch appears.
        let next = assign_keys(&[server("b/mcp.json", "fetch"), server("c/mcp.json", "fetch")], &entries(&first));
        assert_eq!(next[0].0, "fetch-2");
        assert_eq!(next[1].0, "fetch");
        // A new entry sorted ahead of an old one never takes the old one's key.
        let before = assign_keys(&[server("z/mcp.json", "fetch")], &[]);
        let after = assign_keys(&[server("a/mcp.json", "fetch"), server("z/mcp.json", "fetch")], &entries(&before));
        let keys: Vec<&str> = after.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["fetch-2", "fetch"], "z keeps fetch, the new a gets a fresh key");
    }

    /// A link's folder that passes through a symbolic link must not lead the scan out of the clone.
    #[test]
    fn a_start_folder_through_a_symlink_is_not_scanned() {
        let dir = std::env::temp_dir().join(format!("janus-mcp-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let outside = dir.join("outside");
        manifest(&outside.join("sub"), r#"{"mcpServers":{"leak":{"command":"x"}}}"#);
        std::fs::create_dir_all(&repo).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("link")).unwrap();
        assert!(manifest_files(&repo, &repo.join("link/sub")).is_empty());
        assert!(scan(&repo, &repo.join("link/sub")).0.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn commit(dir: &Path, msg: &str) {
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-q", "-m", msg]);
    }

    /// Bundles that must never reach the API, SQLite or a run snapshot.
    const TOKEN: &str = "s3cret-token-value";
    const LOCAL_TOKEN: &str = "kqzvw-local-token";

    fn manifest(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("mcp.json"), text).unwrap();
    }

    fn source_json(app: &App, id: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(app.mcp_sources_root().join(id).join("source.json")).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_source_is_scanned_mounted_updated_and_protected() {
        let dir = std::env::temp_dir().join(format!("janus-mcp-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let upstream = dir.join("upstream");
        manifest(
            &upstream.join("servers"),
            &format!(
                r#"{{"mcpServers": {{
                    "filesystem": {{"command": "npx", "args": ["-y", "@mcp/fs", "."], "env": {{"ROOT": "/tmp", "TOKEN": "{TOKEN}"}}}},
                    "docs": {{"type": "http", "url": "https://example.com/mcp", "headers": {{"Authorization": "Bearer {TOKEN}"}}}}
                }}}}"#
            ),
        );
        manifest(&upstream.join(".vscode"), r#"{"servers":{"fetch":{"type":"stdio","command":"uvx","args":["mcp-server-fetch"]}}}"#);
        std::fs::create_dir_all(upstream.join("node_modules/x")).unwrap();
        std::fs::write(upstream.join("node_modules/x/mcp.json"), r#"{"mcpServers":{"nope":{"command":"x"}}}"#).unwrap();
        sh(&upstream, &["init", "-q"]);
        commit(&upstream, "init");

        let app = test_app(&dir);
        let url = format!("file://{}", upstream.display());
        let at = || RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: String::new() };
        let src = app.add_mcp_source_at(at(), &url).await.unwrap();
        let id = src["id"].as_str().unwrap().to_string();
        assert_eq!(id, "acme-tools");
        let ids: Vec<&str> = src["servers"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["acme-tools/fetch", "acme-tools/docs", "acme-tools/filesystem"], "sorted by manifest path, node_modules skipped");
        assert!(app.add_mcp_source_at(at(), &url).await.is_err(), "the same repository twice");

        // Values only in the 0600 secrets file; the listed source and source.json carry key names.
        let secret_path = app.mcp_secret_path(&id, "filesystem");
        let secret = std::fs::read_to_string(&secret_path).unwrap();
        assert!(secret.contains(TOKEN));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&secret_path).unwrap().permissions().mode() & 0o777, 0o600);
        let listed = app.list_mcp_sources().to_string();
        assert!(!listed.contains(TOKEN), "the API never returns values: {listed}");
        assert!(!source_json(&app, &id).to_string().contains(TOKEN), "source.json never holds values");
        let listed = app.list_mcp_sources();
        let server = listed["sources"][0]["servers"].as_array().unwrap().iter().find(|s| s["key"] == "filesystem").unwrap();
        assert_eq!(server["envKeys"], json!(["ROOT", "TOKEN"]), "declared keys only");
        assert_eq!(server["envSet"], json!(["ROOT", "TOKEN"]));
        assert_eq!(app.mcp_resource("acme-tools/filesystem").unwrap()["name"], "filesystem");
        assert!(app.mcp_resource("acme-tools/nope").is_none());
        assert!(app.mcp_resource("../x/fetch").is_none());

        // A role mounts the resource and keeps its own hand-written MCP.
        app.db
            .put_config("connection", "fake", &json!({ "id": "fake", "kind": "custom", "base": "custom", "name": "Fake", "command": "true" }), now_ms())
            .unwrap();
        let role = app
            .save_role(&json!({
                "id": "r", "name": "审查", "connection": "fake", "mcpRefs": ["acme-tools/filesystem"],
                "mcp": [{ "name": "local", "command": "node local.js", "envText": format!("LOCAL_TOKEN={LOCAL_TOKEN}") }]
            }))
            .unwrap();
        assert_eq!(role["mcpRefs"], json!(["acme-tools/filesystem"]));
        assert_eq!(role["mcp"][0]["envKeys"], json!(["LOCAL_TOKEN"]));
        app.db
            .put_config("workspace", "ws", &json!({ "id": "ws", "name": "ws", "path": dir.to_string_lossy(), "lead": "r", "members": [{ "role": "r" }] }), now_ms())
            .unwrap();
        let ws = app.db.get_config("workspace", "ws").unwrap().unwrap();
        let (_, team) = app.team_for(&ws).unwrap();
        let member = &team[0];
        let names: Vec<&str> = member.mcp.iter().map(|m| str_of(m, "name")).collect();
        assert_eq!(names, ["local", "filesystem"], "hand-written first, then the mounted resource");
        assert_eq!(member.mcp[1]["transport"], "stdio");
        assert_eq!(member.mcp_env["filesystem"]["TOKEN"], TOKEN);
        assert_eq!(member.mcp_env["local"]["LOCAL_TOKEN"], LOCAL_TOKEN);
        let snapshot = serde_json::to_string(member).unwrap();
        assert!(!snapshot.contains(TOKEN) && !snapshot.contains(LOCAL_TOKEN), "the run record holds no values: {snapshot}");
        assert!(app.delete_mcp_source(&id).unwrap_err().to_string().contains("审查"));

        // Two servers with one name cannot be mounted together.
        app.db.put_config(ROLE, "dup", &json!({ "id": "dup", "name": "dup", "mcp": [{ "name": "fetch", "command": "x" }] }), now_ms()).unwrap();
        let err = app.save_role(&json!({ "id": "dup", "name": "dup", "mcpRefs": ["acme-tools/fetch"], "mcp": [{ "name": "fetch", "command": "x" }] })).unwrap_err();
        assert!(err.to_string().contains("重复"), "{err}");

        // Setting a value through the API updates the secret and never returns it.
        let updated = app.write_mcp_secret("acme-tools/filesystem", &json!({ "env": { "TOKEN": "rotated-value" } })).unwrap();
        assert_eq!(updated["envSet"], json!(["ROOT", "TOKEN"]));
        assert!(!serde_json::to_string(&updated).unwrap().contains("rotated"));
        assert_eq!(app.mcp_secret("acme-tools/filesystem")["env"]["TOKEN"], "rotated-value");
        let cleared = app.write_mcp_secret("acme-tools/filesystem", &json!({ "env": { "TOKEN": "" } })).unwrap();
        assert_eq!(cleared["envSet"], json!(["ROOT"]));
        assert!(app.mcp_secret("acme-tools/filesystem")["env"].get("TOKEN").is_none());

        // Upstream moves on: docs goes away, a new server appears, filesystem keeps its id.
        std::fs::remove_dir_all(upstream.join("servers")).unwrap();
        manifest(&upstream.join("servers"), r#"{"mcpServers":{"filesystem":{"command":"npx","args":["-y","@mcp/fs"],"env":{"ROOT":"/tmp","TOKEN":"new-token"}},"search":{"command":"search-mcp"}}}"#);
        commit(&upstream, "next");
        let checked = app.check_mcp_sources().await;
        assert_eq!(checked["sources"][0]["updateAvailable"], true, "{checked}");
        let src = app.update_mcp_source(&id).await.unwrap();
        assert_eq!(src["updateAvailable"], false);
        let ids: Vec<&str> = src["servers"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["acme-tools/fetch", "acme-tools/filesystem", "acme-tools/search"], "the untouched .vscode manifest stays");
        assert_eq!(app.mcp_secret("acme-tools/filesystem")["env"]["ROOT"], "/tmp", "saved values win on update");
        assert!(app.mcp_secret_path(&id, "docs").exists(), "a removed server's values stay for runs already started");

        // The role keeps the old hand-written MCP after saving; a stale ref is dropped like a skill.
        let role = app.db.get_config(ROLE, "r").unwrap().unwrap();
        assert_eq!(role["mcp"][0]["name"], "local");
        let saved = app.save_role(&json!({ "id": "r", "name": "审查", "mcpRefs": ["acme-tools/docs"], "mcp": [] })).unwrap();
        assert_eq!(saved["mcpRefs"], json!([]), "a resource an update removed is not kept");

        app.db.delete_config(ROLE, "r").unwrap();
        app.db.delete_config(ROLE, "dup").unwrap();
        app.db.delete_config("workspace", "ws").unwrap();
        app.delete_mcp_source(&id).unwrap();
        assert!(app.list_mcp_sources()["sources"].as_array().unwrap().is_empty());
        assert!(!app.mcp_secrets_root().join(&id).exists(), "deleting a source removes its values");

        // A place without manifests is refused and leaves nothing behind.
        let empty = RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: "nothing".into() };
        assert!(app.add_mcp_source_at(empty, &url).await.is_err());
        assert!(std::fs::read_dir(app.mcp_sources_root()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reserved_names_are_refused_after_normalization() {
        let text = r#"{"mcpServers": {
            "janus": {"command": "x"},
            "Janus": {"command": "x"},
            "janus!": {"command": "x"},
            "janus_mcp": {"command": "x"}
        }}"#;
        let (found, warnings) = parse_manifest(text, "mcp.json").unwrap();
        assert_eq!(found.len(), 1, "only the non-reserved name is imported");
        assert_eq!(found[0].title, "janus_mcp");
        assert_eq!(warnings.len(), 3, "janus, Janus and janus! are skipped: {warnings:?}");
        assert!(warnings.iter().all(|w| w["error"].as_str().is_some_and(|e| e.contains("janus"))), "{warnings:?}");

        let server = |title: &str| FoundServer {
            manifest: "mcp.json".into(),
            title: title.into(),
            transport: "stdio".into(),
            command: "run".into(),
            args: vec![],
            url: String::new(),
            env: vec![],
            headers: vec![],
            description: String::new(),
        };
        // A source recorded before this check may have key janus; an update replaces it.
        let legacy = json!([{ "manifest": "mcp.json", "title": "fetch", "key": "janus" }]);
        let items = assign_keys(&[server("fetch")], legacy.as_array().unwrap());
        assert_eq!(items[0].0, "fetch", "a legacy reserved key is never reused");
        let items = assign_keys(&[server("janus_mcp")], &[]);
        assert_eq!(items[0].0, "janus-mcp");
        assert!(!reserved_mcp_name(&items[0].0));
    }

    #[tokio::test]
    async fn an_unfinished_run_blocks_deletion_until_it_ends() {
        let dir = std::env::temp_dir().join(format!("janus-mcp-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let upstream = dir.join("upstream");
        manifest(&upstream, r#"{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}"#);
        sh(&upstream, &["init", "-q"]);
        commit(&upstream, "init");

        let app = test_app(&dir);
        let url = format!("file://{}", upstream.display());
        let at = || RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: String::new() };
        let src = app.add_mcp_source_at(at(), &url).await.unwrap();
        let id = src["id"].as_str().unwrap().to_string();
        app.db
            .insert_run(
                "r-wait",
                "测试目标",
                &dir.to_string_lossy(),
                &json!({ "team": [{ "mcp": [{ "name": "fetch", "mcpId": format!("{id}/fetch") }] }] }),
                now_ms(),
            )
            .unwrap();

        // waiting_input is not over: the person may still extend the budget and continue the run.
        for status in ["planning", "running", "waiting_input", "interrupted", "failed"] {
            app.db.set_run_status("r-wait", status).unwrap();
            let err = app.delete_mcp_source(&id).unwrap_err().to_string();
            assert!(err.contains("r-wait"), "{status}: {err}");
        }
        // Finished runs keep the old behaviour: the source can go.
        for status in ["delivered", "accepted", "cancelled"] {
            app.db.set_run_status("r-wait", status).unwrap();
            app.delete_mcp_source(&id).unwrap();
            if status != "cancelled" {
                app.add_mcp_source_at(at(), &url).await.unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The check covers the whole runs table, not the newest 50 rows the app lists: an old
    /// interrupted run still blocks deletion, and an old finished one does not.
    #[tokio::test]
    async fn an_unfinished_run_beyond_the_recent_page_blocks_deletion() {
        let dir = std::env::temp_dir().join(format!("janus-mcp-delete-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let upstream = dir.join("upstream");
        manifest(&upstream, r#"{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}"#);
        sh(&upstream, &["init", "-q"]);
        commit(&upstream, "init");

        let app = test_app(&dir);
        let url = format!("file://{}", upstream.display());
        let at = || RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: String::new() };
        let src = app.add_mcp_source_at(at(), &url).await.unwrap();
        let id = src["id"].as_str().unwrap().to_string();

        let config = json!({ "team": [{ "mcp": [{ "name": "fetch", "mcpId": format!("{id}/fetch") }] }] });
        let base = now_ms();
        app.db.insert_run("r-old", "旧目标", &dir.to_string_lossy(), &config, base).unwrap();
        for i in 1..=55 {
            let run = format!("r-new-{i}");
            app.db.insert_run(&run, "新目标", &dir.to_string_lossy(), &json!({}), base + i).unwrap();
            app.db.set_run_status(&run, "delivered").unwrap();
        }
        app.db.set_run_status("r-old", "interrupted").unwrap();
        assert!(
            !app.db.runs().unwrap().iter().any(|r| r["id"] == "r-old"),
            "the app's newest-50 page does not show the old run"
        );
        let err = app.delete_mcp_source(&id).unwrap_err().to_string();
        assert!(err.contains("r-old"), "an old resumable run still blocks deletion: {err}");

        // Once it ends it no longer holds the source.
        app.db.set_run_status("r-old", "delivered").unwrap();
        app.delete_mcp_source(&id).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn reserved_names_cannot_be_mounted_by_a_role_or_a_run() {
        let dir = std::env::temp_dir().join(format!("janus-mcp-reserved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let upstream = dir.join("upstream");
        manifest(&upstream, r#"{"mcpServers":{"fetch":{"command":"uvx","args":["mcp-server-fetch"]}}}"#);
        sh(&upstream, &["init", "-q"]);
        commit(&upstream, "init");

        let app = test_app(&dir);
        let url = format!("file://{}", upstream.display());
        let at = || RepoAt { repo: "acme/tools".into(), git_ref: String::new(), path: String::new() };
        let src = app.add_mcp_source_at(at(), &url).await.unwrap();
        let id = src["id"].as_str().unwrap().to_string();

        // Simulate a source.json written before the reserved-name check existed.
        let mut src = source_json(&app, &id);
        src["servers"] = json!([{
            "key": "janus", "name": "janus", "title": "janus!", "manifest": "mcp.json",
            "transport": "stdio", "command": "run", "args": [], "url": "",
            "envKeys": [], "headerKeys": [], "description": ""
        }]);
        std::fs::write(
            app.mcp_sources_root().join(&id).join("source.json"),
            serde_json::to_string_pretty(&src).unwrap(),
        )
        .unwrap();
        assert!(app.mcp_resource(&format!("{id}/janus")).is_some());

        let err = app
            .save_role(&json!({ "id": "r", "name": "审查", "mcpRefs": [format!("{id}/janus")], "mcp": [] }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("janus"), "role save refuses the mounted resource: {err}");

        // Runtime defence: a role stored before the check is refused when the team is built.
        app.db
            .put_config("connection", "fake", &json!({ "id": "fake", "kind": "custom", "base": "custom", "name": "Fake", "command": "true" }), now_ms())
            .unwrap();
        app.db
            .put_config(ROLE, "r2", &json!({
                "id": "r2", "name": "旧角色", "connection": "fake", "mcpRefs": [format!("{id}/janus")], "mcp": []
            }), now_ms())
            .unwrap();
        app.db
            .put_config("workspace", "ws", &json!({
                "id": "ws", "name": "ws", "path": dir.to_string_lossy(), "lead": "r2", "members": [{ "role": "r2" }]
            }), now_ms())
            .unwrap();
        let ws = app.db.get_config("workspace", "ws").unwrap().unwrap();
        let err = app.team_for(&ws).unwrap_err().to_string();
        assert!(err.contains("janus"), "building the run team refuses it: {err}");

        // Hand-written names get the same normalized check.
        let err = app
            .save_role(&json!({ "id": "r3", "name": "R3", "mcp": [{ "name": "janus_", "command": "node x.js" }] }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("janus"), "hand-written janus_ is refused: {err}");

        app.db.delete_config("workspace", "ws").unwrap();
        app.db.delete_config(ROLE, "r2").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
