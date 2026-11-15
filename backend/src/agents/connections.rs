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
            for v in map
                .values()
                .filter_map(Value::as_str)
                .filter(|s| s.len() >= 6)
            {
                out = out.replace(v, "<secret>");
            }
        }
        out
    }

    pub fn connection(&self, id: &str) -> Result<Value> {
        self.db
            .get_config(CONNECTION, id)?
            .ok_or_else(|| anyhow!("Provider {id} 不存在"))
    }

    /// Connections as the UI sees them: config plus derived state, never secret values.
    pub fn connections(&self) -> Result<Vec<Value>> {
        let mut out = self.db.list_config(CONNECTION)?;
        for c in out.iter_mut() {
            let id = str_of(c, "id").to_string();
            let dir = self.agent_dir(&id);
            let secrets = self.secrets(&id);
            c["secretsSet"] = json!(
                secrets
                    .as_object()
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            );
            c["home"] = json!(dir.join("home"));
            if login_kind(c).is_some() {
                c["loggedIn"] = json!(dir.join("home/auth.json").exists());
            }
            if !c["login"].is_null() {
                c["loginLog"] = json!(self.redact(&id, &tail(&dir.join("login.log"), 12)));
            }
            c["phase"] = json!(connection_phase(c));
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
        let auth_desc = match auths
            .iter()
            .find(|a| a["id"] == str_of(body, "auth"))
            .or(auths.first())
        {
            Some(a) => a.clone(),
            None => json!({ "id": "", "fields": [] }),
        };
        // Editing keeps its id; a new agent gets a readable one that also names its folder.
        let id = match str_of(body, "id") {
            "" => match str_of(body, "newId") {
                "" => self.suggest_agent_id(kind, str_of(body, "name"))?,
                wanted => {
                    check_id(wanted)?;
                    if self.db.get_config(CONNECTION, wanted)?.is_some()
                        || self.agent_dir(wanted).exists()
                    {
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
            && p["kind"] != kind {
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
        if let Some(endpoints) = desc["endpoints"].as_array().filter(|e| !e.is_empty()) {
            let pick = endpoints
                .iter()
                .find(|e| e["id"] == str_of(body, "endpoint"))
                .unwrap_or(&endpoints[0]);
            config["endpoint"] = pick["id"].clone();
        }

        // Plain fields are stored as config; secret fields go to the provider's own directory.
        let mut secrets = self.secrets(&id);
        let mut fields: Vec<Value> = auth_desc["fields"].as_array().cloned().unwrap_or_default();
        if !desc["token"].is_null() {
            fields.push(json!({ "key": "token", "label": desc["token"]["label"], "type": "secret", "required": true }));
        }
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
            map.retain(|k, _| {
                fields
                    .iter()
                    .any(|f| f["key"] == k.as_str() && f["type"] == "secret")
            });
        }

        let dir = self.agent_dir(&id);
        let home = dir.join("home");
        std::fs::create_dir_all(&home)?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if let Some(env) = self.prepare_home(&config, &secrets, &home)? {
            config["importedEnv"] = json!(env.keys().collect::<Vec<_>>());
            secrets["importedEnv"] = Value::Object(env);
        }
        write_private(
            &dir.join("secrets.json"),
            serde_json::to_string_pretty(&secrets)?.as_bytes(),
        )?;
        self.db.put_config(CONNECTION, &id, &config, now_ms())?;
        Ok(config)
    }

    /// Write the agent's own config files into its isolated home.
    /// Returns the credential variables captured by "复制本机配置", which are stored as secrets.
    pub(super) fn prepare_home(
        &self,
        c: &Value,
        secrets: &Value,
        home: &Path,
    ) -> Result<Option<serde_json::Map<String, Value>>> {
        match (str_of(c, "base"), str_of(c, "auth")) {
            ("pi", "provider") => {
                let models: Vec<Value> = str_of(c, "models").split([',', '，', ' ', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(|m| json!({
                    "id": m, "name": m, "reasoning": true, "input": ["text"], "contextWindow": 128000, "maxTokens": 16384,
                    "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
                })).collect();
                if models.is_empty() {
                    bail!("至少填写一个模型 ID");
                }
                let api = match str_of(c, "api") {
                    "" => "openai-completions",
                    a => a,
                };
                let catalog = json!({ "providers": { "janus": { "baseUrl": str_of(c, "baseUrl"), "api": api, "apiKey": secrets["apiKey"], "models": models } } });
                write_private(
                    &home.join("models.json"),
                    serde_json::to_string_pretty(&catalog)?.as_bytes(),
                )?;
            }
            (base, "import") => {
                let (copied, env) =
                    import_local(base, &local_source(base, str_of(c, "source")), home)?;
                if copied.is_empty() && env.is_empty() {
                    bail!(
                        "在 {} 和你的 shell 环境中都没有找到可以复制的配置",
                        local_source(base, str_of(c, "source")).display()
                    );
                }
                return Ok(Some(env));
            }
            _ => {}
        }
        Ok(None)
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
            let id = if n.starts_with(&base) {
                n
            } else {
                format!("{base}-{n}")
            };
            if check_id(&id).is_ok() && !taken(&id)? {
                return Ok(id);
            }
        }
        (2..1000)
            .map(|i| format!("{base}-{i}"))
            .find(|id| !taken(id).unwrap_or(true))
            .context("没有可用的 ID")
    }

    pub fn delete_connection(&self, id: &str) -> Result<()> {
        // The id names the folder removed below.
        check_id(id)?;
        let busy = self
            .runs
            .lock()
            .unwrap()
            .values()
            .any(|r| r.team.iter().any(|m| m.runtime == id));
        if busy {
            bail!("有正在运行的任务在使用这个 Provider，请等它结束或取消后再删除");
        }
        for mut role in self
            .db
            .list_config(ROLE)?
            .into_iter()
            .filter(|r| r["connection"] == id)
        {
            role["connection"] = json!("");
            role["model"] = json!("");
            role["effort"] = json!("");
            self.db
                .put_config(ROLE, str_of(&role, "id"), &role, now_ms())?;
        }
        self.db.delete_config(CONNECTION, id)?;
        let _ = std::fs::remove_dir_all(self.agent_dir(id));
        Ok(())
    }

    /// How to start this provider's ACP agent, with its isolated home and credentials.
    // @project-doc docs/operations/security.md#agent_isolation
    // @project-doc docs/interfaces/configuration.md#agent_env
    pub fn spec_for(&self, id: &str) -> Result<RuntimeSpec> {
        let c = self.connection(id)?;
        let desc = kinds()
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == c["kind"])
            .cloned()
            .context("未知的 Provider 类型")?;
        let s = self.secrets(id);
        let secret = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let home = self
            .agent_dir(id)
            .join("home")
            .to_string_lossy()
            .to_string();
        // Sandbox first, then a preset's defaults, then the variables configured on this agent (which
        // override the defaults; an empty value unsets one), then its isolation and credentials, so
        // nothing the user typed can point the agent back at the host's own directories.
        // @project-doc docs/domains/agents_and_roles.md#agent_env
        let mut env = self.sandbox_env(id)?;
        env.extend(preset_env(&desc, &c));
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
        let base = str_of(&c, "base").to_string();
        let mut program = crate::config::settings().node.clone();
        let mut args = vec![];
        let adapter = match base.as_str() {
            "codex" => {
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
                crate::adapters::for_base(&base)
            }
            "claude" => {
                set("CLAUDE_CONFIG_DIR", home);
                // Janus reads a returned turn as the agent's answer. A command or subagent Claude
                // Code sends to the background would end the turn before the work is done.
                // @project-doc docs/interfaces/acp_integration.md#background_tasks
                set("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1".into());
                // The adapter ships its own Claude CLI. A path chosen on the agent overrides it.
                if let Some(b) = configured_bin(&c) {
                    set("CLAUDE_CODE_EXECUTABLE", b);
                }
                match (desc["group"].as_str(), str_of(&c, "auth")) {
                    (Some("preset"), _) => {
                        set(desc["token"]["envKey"].as_str().unwrap(), secret("token"))
                    }
                    (_, "oauth") => set("CLAUDE_CODE_OAUTH_TOKEN", secret("token")),
                    (_, "apikey") => set("ANTHROPIC_API_KEY", secret("apiKey")),
                    _ => {}
                }
                crate::adapters::for_base(&base)
            }
            "grok" => {
                // The official CLI keeps its native binary and configuration under GROK_HOME.
                set("GROK_HOME", home);
                if c["auth"] == "apikey" {
                    set("XAI_API_KEY", secret("apiKey"));
                }
                crate::adapters::for_base(&base)
            }
            "pi" => {
                set("PI_CODING_AGENT_DIR", home);
                // pi-acp runs the pi CLI; a path chosen on the agent overrides the one on PATH.
                if let Some(b) = configured_bin(&c) {
                    set("PI_ACP_PI_COMMAND", b);
                }
                crate::adapters::for_base(&base)
            }
            "custom" => {
                // A custom agent runs in the same sandbox as the built-in ones; it gets only the variables configured here.
                let mut parts = split_command(str_of(&c, "command"))?;
                if parts.is_empty() {
                    bail!("没有填写启动命令");
                }
                program = expand_home(&parts.remove(0));
                args = parts;
                None
            }
            k => bail!("未知的 Agent 类型 {k}"),
        };
        let mut full = vec![];
        if let Some(adapter) = adapter {
            let entry = self.adapter_entry(adapter.id)?;
            full.push(entry.to_string_lossy().to_string());
            full.extend(adapter.args.iter().map(|a| a.to_string()));
        }
        full.extend(args);
        Ok(RuntimeSpec {
            id: id.to_string(),
            kind: base,
            name: str_of(&c, "name").to_string(),
            program,
            args: full,
            env,
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
        let mut env: Vec<(String, String)> = HOST_ENV
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
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
pub(super) const HOST_ENV: &[&str] = &[
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TERM",
    "USER",
    "LOGNAME",
    "SHELL",
    "TZ",
];

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
            | "GROK_HOME"
            | "PI_CODING_AGENT_DIR"
    ) || k.starts_with("JANUS_")
}
