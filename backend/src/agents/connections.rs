//! Agent connections: saving and deleting them, their isolated homes and secrets, and the sandboxed
//! command and environment a session of one runs with.

use super::*;

impl App {
    pub(super) fn agent_dir(&self, id: &str) -> PathBuf {
        self.cfg.agents_dir.join(id)
    }

    pub(super) fn secrets(&self, id: &str) -> Value {
        std::fs::read_to_string(self.agent_dir(id).join("secrets.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    pub(super) fn redact(&self, id: &str, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(map) = self.secrets(id).as_object() {
            for v in map.values().filter_map(Value::as_str).filter(|s| s.len() >= 6) {
                out = out.replace(v, "<secret>");
            }
        }
        out
    }

    pub fn connection(&self, id: &str) -> Result<Value> {
        self.db.get_config(CONNECTION, id)?.ok_or_else(|| anyhow!("Provider {id} 不存在"))
    }

    /// Connections as the UI sees them: config plus derived state, never secret values.
    pub fn connections(&self) -> Result<Vec<Value>> {
        let mut out = self.db.list_config(CONNECTION)?;
        for c in out.iter_mut() {
            let id = str_of(c, "id").to_string();
            let dir = self.agent_dir(&id);
            let secrets = self.secrets(&id);
            c["secretsSet"] =
                json!(secrets.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default());
            c["home"] = json!(dir.join("home"));
            if account_login(c) {
                c["loggedIn"] = json!(dir.join("home/auth.json").exists());
            }
            if !c["login"].is_null() {
                c["loginLog"] = json!(self.redact(&id, &tail(&dir.join("login.log"), 12)));
            }
            c["phase"] = json!(connection_phase(c));
            mark_defaults(c);
        }
        Ok(out)
    }

    pub fn save_connection(&self, body: &Value) -> Result<Value> {
        let kind = str_of(body, "kind");
        let desc = kinds()
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == kind)
            .cloned()
            .ok_or_else(|| anyhow!("不支持的 Provider 类型：{kind}"))?;
        let auths = desc["auths"].as_array().cloned().unwrap_or_default();
        let auth_desc = match auths.iter().find(|a| a["id"] == str_of(body, "auth")).or(auths.first()) {
            Some(a) => a.clone(),
            None => json!({ "id": "", "fields": [] }),
        };
        // Editing keeps its id; a new agent gets a readable one that also names its folder.
        let id = match str_of(body, "id") {
            "" => match str_of(body, "newId") {
                "" => self.suggest_agent_id(kind, str_of(body, "name"))?,
                wanted => {
                    check_id(wanted)?;
                    if self.db.get_config(CONNECTION, wanted)?.is_some() || self.agent_dir(wanted).exists() {
                        bail!("ID「{wanted}」已被占用，换一个");
                    }
                    wanted.to_string()
                }
            },
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        let prev = self.db.get_config(CONNECTION, &id)?;
        if let Some(p) = &prev
            && p["kind"] != kind
        {
            bail!("不能修改已有 Provider 的类型");
        }
        let name = match str_of(body, "name") {
            "" => desc["name"].as_str().unwrap().to_string(),
            n => n.to_string(),
        };
        let env = parse_env(body["envText"].as_str().unwrap_or(""))?;
        let mut config = json!({
            "id": id, "kind": kind, "base": desc["base"], "name": name, "auth": auth_desc["id"],
            "bin": str_of(body, "bin"), "prompt": body["prompt"].as_str().unwrap_or("").trim(), "env": env,
            "status": "untested", "createdAt": prev.as_ref().map(|p| p["createdAt"].clone()).unwrap_or(json!(now_ms())),
        });
        if desc["command"] == true {
            let command = str_of(body, "command");
            if split_command(command)?.is_empty() {
                bail!("请填写启动命令");
            }
            config["command"] = json!(command);
        }

        // Plain fields are stored as config; secret fields go to the provider's own directory.
        let mut secrets = self.secrets(&id);
        let fields: Vec<Value> = auth_desc["fields"].as_array().cloned().unwrap_or_default();
        for f in &fields {
            let key = f["key"].as_str().unwrap();
            let value = str_of(body, key);
            if f["type"] == "secret" {
                if !value.is_empty() {
                    secrets[key] = json!(value);
                }
                if f["required"] == true && secrets[key].as_str().unwrap_or("").is_empty() {
                    bail!("请填写 {}", f["label"].as_str().unwrap());
                }
            } else {
                if f["required"] == true && value.is_empty() {
                    bail!("请填写 {}", f["label"].as_str().unwrap());
                }
                config[key] = json!(value);
            }
        }
        // Drop secrets the chosen auth method does not use, so switching methods leaves nothing behind.
        if let Some(map) = secrets.as_object_mut() {
            map.retain(|k, _| fields.iter().any(|f| f["key"] == k.as_str() && f["type"] == "secret"));
        }

        let dir = self.agent_dir(&id);
        let home = dir.join("home");
        std::fs::create_dir_all(&home)?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if let Some(env) = self.prepare_home(&config, &home)? {
            config["importedEnv"] = json!(env.keys().collect::<Vec<_>>());
            secrets["importedEnv"] = Value::Object(env);
        }
        write_private(&dir.join("secrets.json"), serde_json::to_string_pretty(&secrets)?.as_bytes())?;
        self.db.put_config(CONNECTION, &id, &config, now_ms())?;
        Ok(config)
    }

    /// "复制本机配置": copy the agent's own setup into its isolated home. Returns the credential
    /// variables found in the person's shell, which are stored as secrets.
    pub(super) fn prepare_home(&self, c: &Value, home: &Path) -> Result<Option<serde_json::Map<String, Value>>> {
        if str_of(c, "auth") != "import" {
            return Ok(None);
        }
        let base = str_of(c, "base");
        let src = local_source(base, str_of(c, "source"));
        let (copied, env) = import_local(base, &src, home)?;
        if copied.is_empty() && env.is_empty() {
            bail!("在 {} 和你的 shell 环境中都没有找到可以复制的配置", src.display());
        }
        Ok(Some(env))
    }

    /// Deleting a provider unbinds the roles that used it; only a running task keeps it alive.
    /// `codex`, then `codex-magpie` (from the name) or `codex-2`: short, readable, unique.
    pub fn suggest_agent_id(&self, kind: &str, name: &str) -> Result<String> {
        let taken = |id: &str| -> Result<bool> {
            Ok(self.db.get_config(CONNECTION, id)?.is_some() || self.agent_dir(id).exists())
        };
        let base = slug(kind).unwrap_or_else(|| "agent".into());
        if !taken(&base)? {
            return Ok(base);
        }
        if let Some(n) = slug(name).filter(|n| n != &base) {
            let id = if n.starts_with(&base) { n } else { format!("{base}-{n}") };
            if check_id(&id).is_ok() && !taken(&id)? {
                return Ok(id);
            }
        }
        (2..1000).map(|i| format!("{base}-{i}")).find(|id| !taken(id).unwrap_or(true)).context("没有可用的 ID")
    }

    pub fn delete_connection(&self, id: &str) -> Result<()> {
        // The id names the folder removed below.
        check_id(id)?;
        let busy = self.runs.lock().unwrap().values().any(|r| r.team.iter().any(|m| m.runtime == id));
        if busy {
            bail!("有正在运行的子任务在使用这个 Provider，请等它结束或取消后再删除");
        }
        // The roles bound to it come out as a role saved without an agent: nothing chosen for one.
        for mut role in self.db.list_config(ROLE)?.into_iter().filter(|r| r["connection"] == id) {
            role["connection"] = json!("");
            role["model"] = json!("");
            role["effort"] = json!("");
            role["session"] = json!({});
            self.db.put_config(ROLE, str_of(&role, "id"), &role, now_ms())?;
        }
        self.db.delete_config(CONNECTION, id)?;
        let _ = std::fs::remove_dir_all(self.agent_dir(id));
        Ok(())
    }

    /// How to start this provider's ACP agent, with its isolated home and credentials.
    // @project-doc docs/operations/20261031-security.md#agent_isolation
    // @project-doc docs/interfaces/20261031-configuration.md#agent_env
    pub fn spec_for(&self, id: &str) -> Result<RuntimeSpec> {
        let c = self.connection(id)?;
        if !kinds().as_array().unwrap().iter().any(|k| k["id"] == c["kind"]) {
            bail!("未知的 Provider 类型");
        }
        let s = self.secrets(id);
        let secret = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let home = self.agent_dir(id).join("home").to_string_lossy().to_string();
        // Sandbox first, then the variables configured on this agent (an empty value unsets one),
        // then its isolation and credentials, so
        // nothing the user typed can point the agent back at the host's own directories.
        // @project-doc docs/domains/20261031-agents_and_roles.md#agent_env
        let mut env = self.sandbox_env(id)?;
        for (k, v) in c["env"]
            .as_object()
            .into_iter()
            .chain(s["importedEnv"].as_object())
            .flat_map(|m| m.iter())
            .filter(|(k, _)| !is_sandbox_var(k))
        {
            override_env(&mut env, k, v.as_str().unwrap_or(""));
        }
        let mut set = |k: &str, v: String| env.push((k.to_string(), v));
        let runtime = Runtime::of(str_of(&c, "base"))?;
        let mut program = self.cfg.node.clone();
        let mut args = vec![];
        let adapter = match runtime {
            Runtime::Codex => {
                set("CODEX_HOME", home);
                // The adapter bundles the Codex CLI. A path chosen on the agent overrides it.
                if let Some(b) = configured_bin(&c) {
                    set("CODEX_PATH", b);
                }
                if c["auth"] == "apikey" {
                    set("OPENAI_API_KEY", secret("apiKey"));
                    if !str_of(&c, "baseUrl").is_empty() {
                        let provider = json!({
                            "model_provider": "janus",
                            "model_providers": { "janus": { "name": "Janus gateway", "base_url": str_of(&c, "baseUrl"), "env_key": "OPENAI_API_KEY", "wire_api": "responses", "requires_openai_auth": false } }
                        });
                        set("CODEX_CONFIG", provider.to_string());
                        set("MODEL_PROVIDER", "janus".into());
                    }
                }
                crate::adapters::for_base(runtime.as_str())
            }
            Runtime::Claude => {
                set("CLAUDE_CONFIG_DIR", home);
                // Janus reads a returned turn as the agent's answer. A command or subagent Claude
                // Code sends to the background would end the turn before the work is done.
                // @project-doc docs/interfaces/20261031-acp_integration.md#background_tasks
                set("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1".into());
                // The adapter ships its own Claude CLI. A path chosen on the agent overrides it.
                if let Some(b) = configured_bin(&c) {
                    set("CLAUDE_CODE_EXECUTABLE", b);
                }
                match str_of(&c, "auth") {
                    "oauth" => set("CLAUDE_CODE_OAUTH_TOKEN", secret("token")),
                    "apikey" => set("ANTHROPIC_API_KEY", secret("apiKey")),
                    _ => {}
                }
                crate::adapters::for_base(runtime.as_str())
            }
            Runtime::Custom => {
                // A custom agent runs in the same sandbox as the built-in ones; it gets only the variables configured here.
                let mut parts = split_command(str_of(&c, "command"))?;
                if parts.is_empty() {
                    bail!("没有填写启动命令");
                }
                program = expand_home(&parts.remove(0));
                args = parts;
                None
            }
        };
        let mut full = vec![];
        let mut pin = None;
        if let Some(adapter) = adapter {
            let (entry, version) = self.adapter_entry(adapter.id)?;
            full.push(entry.to_string_lossy().to_string());
            pin = Some(version);
        }
        full.extend(args);
        Ok(RuntimeSpec {
            id: id.to_string(),
            kind: runtime,
            name: str_of(&c, "name").to_string(),
            program,
            args: full,
            env,
            pin,
        })
    }

    /// A fresh user environment for one agent: its own HOME, XDG directories and temp dir under
    /// `~/.janus/agents/<id>/`, so any lookup of `~/...` lands in the sandbox instead of the host.
    pub fn sandbox_env(&self, id: &str) -> Result<Vec<(String, String)>> {
        let dir = self.agent_dir(id);
        let root = dir.join("root");
        let paths = [
            ("HOME", root.clone()),
            ("XDG_CONFIG_HOME", root.join(".config")),
            ("XDG_DATA_HOME", root.join(".local/share")),
            ("XDG_STATE_HOME", root.join(".local/state")),
            ("XDG_CACHE_HOME", root.join(".cache")),
            ("TMPDIR", dir.join("tmp")),
        ];
        let mut env: Vec<(String, String)> =
            HOST_ENV.iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();
        for (k, p) in paths {
            std::fs::create_dir_all(&p)?;
            env.push((k.to_string(), p.to_string_lossy().to_string()));
        }
        env.push(("JANUS_AGENT_ID".into(), id.to_string()));
        Ok(env)
    }
}

/// The only variables an agent inherits from the host: enough to find executables and render text.
/// Everything else (HOME, XDG_*, credentials, proxies, SSH, Git and agent settings) is set by Janus or absent.
pub(super) const HOST_ENV: &[&str] =
    &["PATH", "LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TERM", "USER", "LOGNAME", "SHELL", "TZ"];

/// Variables that only Janus may set; an agent's own environment settings cannot override them.
pub(super) fn is_sandbox_var(k: &str) -> bool {
    matches!(
        k,
        "HOME"
            | "XDG_CONFIG_HOME"
            | "XDG_DATA_HOME"
            | "XDG_STATE_HOME"
            | "XDG_CACHE_HOME"
            | "TMPDIR"
            | "PATH"
            | "CODEX_HOME"
            | "CLAUDE_CONFIG_DIR"
    ) || k.starts_with("JANUS_")
}

/// The variables the removed presets over Claude Code (DeepSeek, MiniMax, GLM) set, by kind, and
/// for GLM by the endpoint it was given. Kept only for data version 7.
fn preset_vars(kind: &str, endpoint: &str) -> Option<Vec<(&'static str, &'static str)>> {
    let quiet = ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    Some(match kind {
        "deepseek-claude" => vec![
            ("ANTHROPIC_BASE_URL", "https://api.deepseek.com/anthropic"),
            ("ANTHROPIC_MODEL", "deepseek-v4-pro[1m]"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "deepseek-v4-pro"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "deepseek-v4-pro"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "deepseek-v4-flash"),
            ("CLAUDE_CODE_SUBAGENT_MODEL", "deepseek-v4-pro"),
            quiet,
            ("CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK", "1"),
        ],
        "minimax-claude" => vec![
            ("ANTHROPIC_BASE_URL", "https://api.minimaxi.com/anthropic"),
            ("API_TIMEOUT_MS", "3000000"),
            quiet,
            ("ANTHROPIC_MODEL", "MiniMax-M3"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "MiniMax-M3"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "MiniMax-M3"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "MiniMax-M3"),
        ],
        "glm-claude" => vec![
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "glm-4.7"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "glm-5.2[1m]"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "glm-5.2[1m]"),
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "1000000"),
            quiet,
            ("API_TIMEOUT_MS", "3000000"),
            (
                "ANTHROPIC_BASE_URL",
                if endpoint == "zai" {
                    "https://api.z.ai/api/anthropic"
                } else {
                    "https://open.bigmodel.cn/api/anthropic"
                },
            ),
        ],
        _ => return None,
    })
}

/// Data version 7: the presets over Claude Code are gone. An agent made from one becomes a Claude
/// Code agent signed in with an API key, its preset's variables written into its own (where the
/// person had not set or unset them), its endpoint choice dropped, and left to be checked again.
/// Its key stays where the preset kept it, outside the database, so the person enters it once more
/// as the API key.
// @project-doc docs/domains/20261031-agents_and_roles.md#providers
pub(crate) fn drop_preset_kinds(tx: &rusqlite::Transaction) -> Result<usize> {
    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, data FROM configs WHERE kind = ?1")?;
        stmt.query_map([CONNECTION], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut changed = 0;
    for (id, data) in rows {
        let mut c: Value = serde_json::from_str(&data)?;
        let Some(vars) = preset_vars(str_of(&c, "kind"), str_of(&c, "endpoint")) else { continue };
        let mut env = serde_json::Map::new();
        for (k, v) in vars {
            env.insert(k.to_string(), json!(v));
        }
        for (k, v) in c["env"].as_object().cloned().unwrap_or_default() {
            match v.as_str() {
                Some("") => {
                    env.remove(&k);
                }
                _ => {
                    env.insert(k, v);
                }
            }
        }
        let o = c.as_object_mut().context("connection")?;
        o.remove("endpoint");
        o.insert("kind".into(), json!("claude"));
        o.insert("base".into(), json!("claude"));
        o.insert("auth".into(), json!("apikey"));
        o.insert("env".into(), Value::Object(env));
        o.insert("status".into(), json!("untested"));
        tx.execute(
            "UPDATE configs SET data = ?1 WHERE kind = ?2 AND id = ?3",
            rusqlite::params![c.to_string(), CONNECTION, id],
        )?;
        changed += 1;
    }
    Ok(changed)
}
