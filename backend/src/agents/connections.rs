//! Agent connections: saving and deleting them, their isolated homes and secrets, and the sandboxed
//! command and environment a session of one runs with.

use super::environment::{private_env_value, protect_claude_env, read_secrets, write_secrets};
use super::*;

impl App {
    /// What Janus keeps for one agent (`Layout::agent`): its configuration directory and HOME.
    pub(super) fn agent_dir(&self, id: &str) -> PathBuf {
        self.cfg.layout.agent(id)
    }

    /// The agent's configuration directory (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`).
    pub(super) fn agent_config(&self, id: &str) -> PathBuf {
        self.cfg.layout.agent_config(id)
    }

    pub(super) fn redact(&self, id: &str, text: &str) -> String {
        let mut out = text.to_string();
        let Ok(secrets) = read_secrets(&self.cfg.layout.agent_secrets(id)) else { return "<secret>".into() };
        let mut values: Vec<_> = secrets
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
            .flat_map(|v| {
                v.as_str()
                    .into_iter()
                    .chain(v.as_object().into_iter().flat_map(|m| m.values().filter_map(Value::as_str)))
            })
            .filter(|s| !s.is_empty())
            .collect();
        values.sort_unstable_by_key(|v| std::cmp::Reverse(v.len()));
        for value in values {
            out = out.replace(value, "<secret>");
        }
        out
    }

    /// Remove what the agent itself kept of these sessions in its configuration directory: Claude
    /// Code's `projects/<project>/<session>.jsonl` (with `<session>/` beside it) and
    /// `session-env/<session>/`, Codex's `sessions/<date>/rollout-<time>-<session>.jsonl`.
    // @project-doc docs/interfaces/20261031-http_api.md#run_delete
    pub(crate) fn forget_sessions(&self, id: &str, sessions: &std::collections::HashSet<String>) -> Result<()> {
        fn walk(dir: &Path, sessions: &std::collections::HashSet<String>, depth: u8) -> Result<()> {
            let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let of = |s: &String| {
                    let file = name.strip_suffix(".jsonl").unwrap_or(&name);
                    !s.is_empty()
                        && (file == s || file.strip_suffix(s.as_str()).is_some_and(|rest| rest.ends_with('-')))
                };
                if sessions.iter().any(of) {
                    crate::layout::remove(&e.path());
                } else if depth > 0 && e.file_type().is_ok_and(|t| t.is_dir()) {
                    walk(&e.path(), sessions, depth - 1)?;
                }
            }
            Ok(())
        }
        check_id(id)?;
        if sessions.is_empty() {
            return Ok(());
        }
        let config = self.agent_config(id);
        for top in ["projects", "session-env", "sessions"] {
            walk(&config.join(top), sessions, 4)?;
        }
        Ok(())
    }

    /// Every variable of the agent's environment with its value, private ones read from its secrets
    /// file, for the editor to show and edit (`GET /api/connections/{id}/env`, local app only).
    /// Everywhere else a private value is only its name.
    pub fn connection_env(&self, id: &str) -> Result<Value> {
        let c = self.connection(id)?;
        let secrets = read_secrets(&self.cfg.layout.agent_secrets(id))?;
        let env: serde_json::Map<String, Value> = c["env"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), json!(v.as_str().or_else(|| private_env_value(&secrets, k)).unwrap_or_default())))
            .collect();
        Ok(json!({ "env": env }))
    }

    pub fn connection(&self, id: &str) -> Result<Value> {
        self.db.get_config(ConfigKind::Connection, id)?.ok_or_else(|| anyhow!("Provider {id} 不存在"))
    }

    /// Connections as the UI sees them: config plus derived state, never secret values.
    pub fn connections(&self) -> Result<Vec<Value>> {
        let mut out = self.db.list_config(ConfigKind::Connection)?;
        for c in out.iter_mut() {
            let id = str_of(c, "id").to_string();
            let secrets = read_secrets(&self.cfg.layout.agent_secrets(&id))?;
            c["secretsSet"] =
                json!(secrets.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default());
            let config = self.agent_config(&id);
            c["home"] = json!(config);
            if account_login(c) {
                c["loggedIn"] = json!(config.join("auth.json").exists());
            }
            if !c["login"].is_null() {
                c["loginLog"] = json!(self.redact(&id, &tail(&self.cfg.layout.agent_logs(&id).join("login.log"), 12)));
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
        let auth_desc = match auths.iter().find(|a| a["id"] == body["auth"]).or(auths.first()) {
            Some(a) => a.clone(),
            None => json!({ "id": "", "fields": [] }),
        };
        // Editing keeps its id; a new agent gets a readable one that also names its folder.
        let id = match str_of(body, "id") {
            "" => match str_of(body, "newId") {
                "" => self.suggest_agent_id(kind, str_of(body, "name"))?,
                wanted => {
                    check_id(wanted)?;
                    if self.db.get_config(ConfigKind::Connection, wanted)?.is_some() || self.agent_dir(wanted).exists()
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
        let prev = self.db.get_config(ConfigKind::Connection, &id)?;
        if let Some(p) = &prev
            && p["kind"] != kind
        {
            bail!("不能修改已有 Provider 的类型");
        }
        let name = match str_of(body, "name") {
            "" => desc["name"].as_str().unwrap().to_string(),
            n => n.to_string(),
        };
        let dir = self.agent_dir(&id);
        let secrets_file = self.cfg.layout.agent_secrets(&id);
        let mut secrets = read_secrets(&secrets_file)?;
        let runtime = Runtime::of(str_of(&desc, "base"))?;
        let mut env = parse_env(body["envText"].as_str().unwrap_or(""), |name| {
            if runtime.env_holds_secrets() { private_env_value(&secrets, name) } else { None }
        })?;
        let mut config = json!({
            "id": id, "kind": kind, "base": desc["base"], "name": name, "auth": auth_desc["id"],
            "bin": str_of(body, "bin"), "prompt": body["prompt"].as_str().unwrap_or("").trim(),
            "createdAt": prev.as_ref().map(|p| p["createdAt"].clone()).unwrap_or(json!(now_ms())),
        });
        // A saved agent is checked again before it runs (`start_check`).
        set_check(&mut config, ConnCheck::Untested);

        // Plain fields are stored as config; secret fields go to the provider's own directory.
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

        if runtime.env_holds_secrets() {
            protect_claude_env(&mut env, &mut secrets);
        }
        config["env"] = Value::Object(env);
        let home = self.agent_config(&id);
        std::fs::create_dir_all(&home)?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if let Some(env) = self.prepare_home(&config, &home)? {
            config["importedEnv"] = json!(env.keys().collect::<Vec<_>>());
            secrets["importedEnv"] = Value::Object(env);
        }
        write_secrets(&secrets_file, &secrets)?;
        self.db.put_config(ConfigKind::Connection, &id, &config, now_ms())?;
        Ok(config)
    }

    /// "复制本机配置": copy the agent's own setup into its isolated home. Returns the credential
    /// variables found in the person's shell, which are stored as secrets.
    pub(super) fn prepare_home(&self, c: &Value, home: &Path) -> Result<Option<serde_json::Map<String, Value>>> {
        if str_of(c, "auth") != "import" {
            return Ok(None);
        }
        let runtime = Runtime::of(str_of(c, "base"))?;
        let src = local_source(runtime, str_of(c, "source"));
        let (copied, env) = import_local(runtime, &src, home)?;
        if copied.is_empty() && env.is_empty() {
            bail!("在 {} 和你的 shell 环境中都没有找到可以复制的配置", src.display());
        }
        Ok(Some(env))
    }

    /// Deleting a provider unbinds the roles that used it; only a running task keeps it alive.
    /// `codex`, then `codex-magpie` (from the name) or `codex-2`: short, readable, unique.
    pub fn suggest_agent_id(&self, kind: &str, name: &str) -> Result<String> {
        let taken = |id: &str| -> Result<bool> {
            Ok(self.db.get_config(ConfigKind::Connection, id)?.is_some() || self.agent_dir(id).exists())
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
        // The id names the paths removed below.
        check_id(id)?;
        // A run holds the connection while it works or keeps a session open; one that was stopped
        // and has none left does not, though it stays in memory until the service restarts.
        let busy =
            self.runs.lock().unwrap().values().any(|r| r.team.iter().any(|m| m.runtime == id) && r.holds_agents());
        if busy {
            bail!("有正在运行的子任务在使用这个 Provider，请等它结束或取消后再删除");
        }
        // The roles bound to it come out as a role saved without an agent: nothing chosen for one.
        for role in self.db.list::<Role>()?.into_iter().filter(|r| r.connection == id) {
            let unbound = Role {
                connection: String::new(),
                model: String::new(),
                effort: String::new(),
                session: json!({}),
                ..role
            };
            self.db.put(&unbound, now_ms())?;
        }
        self.db.delete_config(ConfigKind::Connection, id)?;
        for path in self.cfg.layout.agent_paths(id) {
            crate::layout::remove(&path);
        }
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
        let s = read_secrets(&self.cfg.layout.agent_secrets(id))?;
        let secret = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let home = self.agent_config(id).to_string_lossy().to_string();
        // Sandbox first, then the variables configured on this agent (an empty value unsets one),
        // then its isolation and credentials, so nothing the user typed can point the agent back
        // at the host's own directories.
        // @project-doc docs/domains/20261031-agents_and_roles.md#agent_env
        let mut env = self.sandbox_env(id)?;
        for (k, v) in c["env"].as_object().into_iter().flatten().filter(|(k, _)| !is_sandbox_var(k)) {
            let value = v
                .as_str()
                .or_else(|| private_env_value(&s, k))
                .with_context(|| format!("请在环境变量中重新填写 {k}"))?;
            override_env(&mut env, k, value);
        }
        for (k, v) in s["importedEnv"].as_object().into_iter().flatten().filter(|(k, _)| !is_sandbox_var(k)) {
            override_env(&mut env, k, v.as_str().unwrap_or(""));
        }
        let runtime = Runtime::of(str_of(&c, "base"))?;
        env.push((runtime.config_dir_var().to_string(), home));
        // The adapter bundles the agent's CLI. A path chosen on the agent overrides it.
        if let Some(b) = configured_bin(&c) {
            env.push((runtime.bin_var().to_string(), b));
        }
        env.extend(runtime.launch_env(&c, secret).into_iter().map(|(k, v)| (k.to_string(), v)));
        let (entry, version) = self.adapter_entry(runtime.as_str())?;
        Ok(RuntimeSpec {
            id: id.to_string(),
            kind: runtime,
            name: str_of(&c, "name").to_string(),
            program: self.cfg.node.clone(),
            args: vec![entry.to_string_lossy().to_string()],
            env,
            pin: Some(version),
        })
    }

    /// A fresh user environment for one agent: its own HOME, XDG directories and temp dir
    /// (`Layout::agent_sandbox`), so any lookup of `~/...` lands in the sandbox instead of the host.
    pub fn sandbox_env(&self, id: &str) -> Result<Vec<(String, String)>> {
        let sandbox = self.cfg.layout.agent_sandbox(id);
        let mut env = sandbox_vars(&sandbox)?;
        // The person's home folder, but what is hidden (sign-ins, tool settings).
        self.link_host(&sandbox)?;
        env.push(("JANUS_AGENT_ID".into(), id.to_string()));
        Ok(env)
    }
}

/// A fresh user environment in `sandbox`: HOME and the XDG directories under its home, its temp
/// dir, and from the host only the few variables in `HOST_ENV`. Each agent gets one; so does each
/// MCP server the router runs.
// @project-doc docs/operations/20261031-security.md#agent_isolation
pub(crate) fn sandbox_vars(sandbox: &crate::layout::Sandbox) -> Result<Vec<(String, String)>> {
    let root = sandbox.home.clone();
    let paths = [
        ("HOME", root.clone()),
        ("XDG_CONFIG_HOME", root.join(".config")),
        ("XDG_DATA_HOME", root.join(".local/share")),
        ("XDG_STATE_HOME", root.join(".local/state")),
        ("XDG_CACHE_HOME", root.join(".cache")),
        ("TMPDIR", sandbox.tmp.clone()),
    ];
    let mut env: Vec<(String, String)> =
        HOST_ENV.iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();
    for (k, p) in paths {
        std::fs::create_dir_all(&p)?;
        env.push((k.to_string(), p.to_string_lossy().to_string()));
    }
    Ok(env)
}

/// The only variables an agent inherits from the host: enough to find executables, render text and
/// reach the person's ssh-agent. Everything else (HOME, XDG_*, credentials, proxies, Git and agent
/// settings) is set by Janus or absent; the person's sign-ins reach it through its HOME (`host_env`).
pub(super) const HOST_ENV: &[&str] =
    &["PATH", "LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TERM", "USER", "LOGNAME", "SHELL", "TZ", "SSH_AUTH_SOCK"];

/// Variables that only Janus may set; an agent's own environment settings cannot override them.
pub(crate) fn is_sandbox_var(k: &str) -> bool {
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
