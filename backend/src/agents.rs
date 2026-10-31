//! Setup objects: agent connections → roles → execution environments.
//!
//! A connection is one coding agent (Codex / Claude Code / Pi) reached through a Lody ACP
//! adapter. Each connection owns an isolated home under `~/.janus/agents/<id>/home` that is
//! handed to the agent as CODEX_HOME / CLAUDE_CONFIG_DIR / PI_CODING_AGENT_DIR, so neither
//! the user's own `~/.codex`, `~/.claude`, `~/.pi` nor their shell credentials leak in.
//! Secrets live in `~/.janus/agents/<id>/secrets.json` (0600) and never in SQLite or the API.

use crate::acp::{AcpConn, Incoming, RuntimeSpec};
use crate::orchestrator::{App, Member, expand_home, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const CONNECTION: &str = "connection";
const ROLE: &str = "role";
const WORKSPACE: &str = "workspace";

/// The only variables an agent inherits from the host: enough to find executables and render text.
/// Everything else (HOME, XDG_*, credentials, proxies, SSH, Git and agent settings) is set by Janus or absent.
const HOST_ENV: &[&str] = &[
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

/// Provider types, laid out like Lody's "New provider" dialog: built-in runtimes, presets that
/// route a built-in runtime through another vendor, and any ACP agent started by a custom command.
// @project-doc docs/domains/agents_and_roles.md#providers
pub fn kinds() -> Value {
    let runtime = |cmd: &str| json!({ "cmd": cmd, "placeholder": format!("/path/to/{cmd}"), "detected": which(cmd) });
    let claude_preset = |id: &str,
                         name: &str,
                         desc: &str,
                         color: &str,
                         init: &str,
                         token: &str,
                         help: &str,
                         env: Value,
                         endpoints: Value| {
        json!({
            "id": id, "group": "preset", "base": "claude", "name": name, "desc": desc, "color": color, "init": init, "isolation": "CLAUDE_CONFIG_DIR",
            "runtime": runtime("claude"), "auths": [],
            "token": { "label": token, "help": help, "envKey": "ANTHROPIC_AUTH_TOKEN" },
            "fixedEnv": env, "endpoints": endpoints,
        })
    };
    json!([
        {
            "id": "codex", "group": "builtin", "base": "codex", "name": "Codex", "desc": "OpenAI Codex", "color": "#1f2937", "init": "CX",
            "isolation": "CODEX_HOME", "runtime": runtime("codex"),
            "auths": [
                { "id": "import", "label": "复制本机配置", "desc": "", "fields": [{ "key": "source", "label": "来源目录", "type": "text", "placeholder": "~/.codex" }] },
                { "id": "chatgpt", "label": "ChatGPT", "login": true, "desc": "创建后在浏览器里登录。", "fields": [] },
                { "id": "apikey", "label": "自定义 API", "desc": "OpenAI API，或兼容 Responses API 的网关。",
                  "fields": [
                      { "key": "baseUrl", "label": "Base URL", "type": "text", "placeholder": "https://api.openai.com/v1" },
                      { "key": "apiKey", "label": "API Key", "type": "secret", "required": true, "placeholder": "sk-..." }
                  ] }
            ]
        },
        {
            "id": "claude", "group": "builtin", "base": "claude", "name": "Claude", "desc": "Anthropic Claude Code", "color": "#b4532a", "init": "CC",
            "isolation": "CLAUDE_CONFIG_DIR", "runtime": runtime("claude"),
            "auths": [
                { "id": "import", "label": "复制本机配置", "desc": "", "fields": [{ "key": "source", "label": "来源目录", "type": "text", "placeholder": "~/.claude" }] },
                { "id": "oauth", "label": "Claude 订阅", "desc": "在终端运行 claude setup-token，把生成的 token 粘贴到这里。",
                  "fields": [{ "key": "token", "label": "OAuth Token", "type": "secret", "required": true, "placeholder": "sk-ant-oat01-..." }] },
                { "id": "apikey", "label": "API Key", "desc": "",
                  "fields": [{ "key": "apiKey", "label": "API Key", "type": "secret", "required": true, "placeholder": "sk-ant-api03-..." }] }
            ]
        },
        {
            "id": "grok", "group": "builtin", "base": "grok", "name": "Grok", "desc": "xAI Grok", "color": "#111111", "init": "GK",
            "isolation": "GROK_HOME", "runtime": runtime("grok"),
            "auths": [
                { "id": "import", "label": "复制本机配置", "desc": "", "fields": [{ "key": "source", "label": "来源目录", "type": "text", "placeholder": "~/.grok" }] },
                { "id": "account", "label": "xAI 账号", "login": true, "desc": "创建后用设备码登录。", "fields": [] },
                { "id": "apikey", "label": "API Key", "desc": "",
                  "fields": [{ "key": "apiKey", "label": "API Key", "type": "secret", "required": true, "placeholder": "xai-..." }] }
            ]
        },
        {
            "id": "pi", "group": "builtin", "base": "pi", "name": "Pi", "desc": "Pi 编码 Agent", "color": "#0e7490", "init": "PI",
            "isolation": "PI_CODING_AGENT_DIR", "runtime": Value::Null,
            "auths": [
                { "id": "import", "label": "复制本机配置", "desc": "", "fields": [{ "key": "source", "label": "来源目录", "type": "text", "placeholder": "~/.pi/agent" }] },
                { "id": "provider", "label": "OpenAI 兼容服务", "desc": "",
                  "fields": [
                      { "key": "baseUrl", "label": "Base URL", "type": "text", "required": true, "placeholder": "https://api.openai.com/v1" },
                      { "key": "apiKey", "label": "API Key", "type": "secret", "required": true },
                      { "key": "api", "label": "接口类型", "type": "select", "options": [["openai-completions", "OpenAI Chat Completions"], ["openai-responses", "OpenAI Responses"], ["anthropic-messages", "Anthropic Messages"]] },
                      { "key": "models", "label": "模型 ID", "type": "text", "required": true, "placeholder": "用逗号分隔，例如 gpt-5.1, gpt-5.1-mini" }
                  ] },

            ]
        },
        claude_preset("deepseek-claude", "DeepSeek over Claude Code", "用 DeepSeek 驱动 Claude Code，粘贴 API token 即可。", "#4d6bfe", "DS",
            "DeepSeek API token", "在 platform.deepseek.com 创建或复制。",
            json!({ "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic", "ANTHROPIC_MODEL": "deepseek-v4-pro[1m]", "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro", "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash", "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-pro",
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK": "1" }), json!([])),
        claude_preset("minimax-claude", "MiniMax over Claude Code", "用 MiniMax 驱动 Claude Code，粘贴 API key 即可。", "#e2445c", "MM",
            "MiniMax API key", "在 platform.minimaxi.com 创建或复制。",
            json!({ "ANTHROPIC_BASE_URL": "https://api.minimaxi.com/anthropic", "API_TIMEOUT_MS": "3000000", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
                    "ANTHROPIC_MODEL": "MiniMax-M3", "ANTHROPIC_DEFAULT_OPUS_MODEL": "MiniMax-M3", "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3", "ANTHROPIC_DEFAULT_HAIKU_MODEL": "MiniMax-M3" }), json!([])),
        claude_preset("glm-claude", "GLM over Claude Code", "用智谱 GLM 驱动 Claude Code，选择接入点并粘贴 API key。", "#2b2f36", "GL",
            "GLM API key", "在所选接入点的控制台创建或复制。",
            json!({ "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7", "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-5.2[1m]", "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5.2[1m]",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "API_TIMEOUT_MS": "3000000" }),
            json!([
                { "id": "bigmodel", "label": "bigmodel.cn（智谱）", "env": { "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic" } },
                { "id": "zai", "label": "z.ai", "env": { "ANTHROPIC_BASE_URL": "https://api.z.ai/api/anthropic" } }
            ])),
        {
            "id": "custom", "group": "custom", "base": "custom", "name": "自定义命令", "desc": "任意兼容 ACP 的 Agent", "color": "#64748b", "init": ">_",
            "isolation": Value::Null, "runtime": Value::Null, "auths": [], "command": true
        }
    ])
}

/// `KEY=VALUE` per line; a leading `export` and matching quotes are dropped, `#` starts a comment.
pub fn parse_env(text: &str) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("环境变量第 {} 行缺少 =", i + 1))?;
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("环境变量第 {} 行的名字无效：{k}", i + 1);
        }
        let v = v.trim();
        let v = match (v.chars().next(), v.chars().last()) {
            (Some(q @ ('"' | '\'')), Some(e)) if q == e && v.len() >= 2 => &v[1..v.len() - 1],
            _ => v,
        };
        out.insert(k.to_string(), json!(v));
    }
    Ok(out)
}

/// Split a launch command like a shell would for the simple cases: whitespace, single and double quotes.
pub(crate) fn split_command(cmd: &str) -> Result<Vec<String>> {
    let (mut out, mut cur, mut quote, mut any) = (vec![], String::new(), None, false);
    for c in cmd.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        bail!("启动命令里有未闭合的引号");
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

pub fn which(bin: &str) -> Option<String> {
    let out = std::process::Command::new("which").arg(bin).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn auth_hint(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if msg.contains("Authentication required")
        || msg.contains("401")
        || lower.contains("invalid api key")
        || lower.contains("unauthorized")
    {
        format!("{msg}。凭据无效或还没有登录")
    } else {
        msg.to_string()
    }
}

/// Runtimes whose account login Janus drives itself; both keep credentials in `<home>/auth.json`.
fn login_kind(c: &Value) -> Option<&str> {
    match (c["base"].as_str(), c["auth"].as_str()) {
        (Some("codex"), Some("chatgpt")) => Some("codex"),
        (Some("grok"), Some("account")) => Some("grok"),
        _ => None,
    }
}

/// Lowercase ASCII words joined by `-`, or None when nothing usable is left (e.g. an all-Chinese name).
fn slug(s: &str) -> Option<String> {
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let out: String = words.join("-").chars().take(32).collect();
    let out = out.trim_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

pub fn check_agent_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        bail!("ID 只能用小写字母、数字和 -，并以字母或数字开头（它也是配置目录的名字）");
    }
    Ok(())
}

/// Variables that only Janus may set; an agent's own environment settings cannot override them.
fn is_sandbox_var(k: &str) -> bool {
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

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().map(str::trim).unwrap_or("")
}

fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        crate::orchestrator::to_base36(now_ms() as u64)
    )
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

impl App {
    fn agent_dir(&self, id: &str) -> PathBuf {
        self.cfg.agents_dir.join(id)
    }

    fn secrets(&self, id: &str) -> Value {
        std::fs::read_to_string(self.agent_dir(id).join("secrets.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    fn redact(&self, id: &str, text: &str) -> String {
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

    // ------------------------------------------------------------------
    // Connections
    // ------------------------------------------------------------------

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
                    check_agent_id(wanted)?;
                    if self.db.get_config(CONNECTION, wanted)?.is_some()
                        || self.agent_dir(wanted).exists()
                    {
                        bail!("ID「{wanted}」已被占用，换一个");
                    }
                    wanted.to_string()
                }
            },
            id => id.to_string(),
        };
        let prev = self.db.get_config(CONNECTION, &id)?;
        if let Some(p) = &prev {
            if p["kind"] != kind {
                bail!("不能修改已有 Provider 的类型");
            }
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
    fn prepare_home(
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
            if check_agent_id(&id).is_ok() && !taken(&id)? {
                return Ok(id);
            }
        }
        (2..1000)
            .map(|i| format!("{base}-{i}"))
            .find(|id| !taken(id).unwrap_or(true))
            .context("没有可用的 ID")
    }

    pub fn delete_connection(&self, id: &str) -> Result<()> {
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
        let bin = |cmd: &str| match str_of(&c, "bin") {
            "" => which(cmd),
            b => Some(expand_home(b)),
        };
        // Sandbox first, then the variables configured on this agent, then its isolation and credentials,
        // so nothing the user typed can point the agent back at the host's own directories.
        let mut env = self.sandbox_env(id)?;
        env.extend(
            c["env"]
                .as_object()
                .into_iter()
                .chain(s["importedEnv"].as_object())
                .flat_map(|m| m.iter())
                .filter(|(k, _)| !is_sandbox_var(k))
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())),
        );
        let mut set = |k: &str, v: String| env.push((k.to_string(), v));
        let base = str_of(&c, "base").to_string();
        let mut program = crate::config::settings().node.clone();
        let mut args = vec![];
        let adapter = match base.as_str() {
            "codex" => {
                set("CODEX_HOME", home);
                set(
                    "CODEX_PATH",
                    bin("codex").context("找不到 codex 可执行文件，请填写运行时二进制路径")?,
                );
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
                if let Some(b) = bin("claude") {
                    set("CLAUDE_CODE_EXECUTABLE", b);
                }
                if let Some(fixed) = desc["fixedEnv"].as_object() {
                    for (k, v) in fixed {
                        set(k, v.as_str().unwrap_or("").into());
                    }
                }
                if let Some(ep) = desc["endpoints"]
                    .as_array()
                    .and_then(|e| e.iter().find(|e| e["id"] == c["endpoint"]))
                {
                    for (k, v) in ep["env"].as_object().unwrap() {
                        set(k, v.as_str().unwrap_or("").into());
                    }
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
                set("GROK_HOME", home);
                set(
                    "GROK_PATH",
                    bin("grok").context("找不到 grok 可执行文件，请填写运行时二进制路径")?,
                );
                set("GROK_DISABLE_AUTOUPDATER", "1".into());
                if c["auth"] == "apikey" {
                    set("XAI_API_KEY", secret("apiKey"));
                }
                crate::adapters::for_base(&base)
            }
            "pi" => {
                set("PI_CODING_AGENT_DIR", home);
                if c["auth"] == "provider" {
                    args.extend(["--provider".to_string(), "janus".to_string()]);
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
            let adapter = self.adapter_entry(adapter.id)?;
            full.push(adapter.to_string_lossy().to_string());
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

    /// Start a configuration check in the background, so it finishes even if the page that asked goes away.
    pub fn start_check(self: &Arc<Self>, id: &str) -> Result<Value> {
        let mut c = self.connection(id)?;
        if c["status"] == "testing"
            && c["testStartedAt"]
                .as_i64()
                .is_some_and(|t| now_ms() - t < 200_000)
        {
            return Ok(c);
        }
        c["status"] = json!("testing");
        c["testStartedAt"] = json!(now_ms());
        c["check"] = json!([]);
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            if let Err(err) = app.test_connection(&id).await {
                crate::config::log(format!("janus: check {id} failed to record: {err:#}"));
            }
        });
        Ok(c)
    }

    /// A check left half-done by a restart reads as "not tested" again.
    pub fn reset_stale_checks(&self) -> Result<()> {
        for mut c in self
            .db
            .list_config(CONNECTION)?
            .into_iter()
            .filter(|c| c["status"] == "testing")
        {
            c["status"] = json!("untested");
            self.db
                .put_config(CONNECTION, str_of(&c, "id"), &c, now_ms())?;
        }
        Ok(())
    }

    /// Record one step of the check so the page can show progress while it runs.
    fn check_step(
        &self,
        id: &str,
        steps: &mut Vec<Value>,
        name: &str,
        ok: Option<bool>,
        detail: String,
    ) {
        steps.retain(|s| s["step"] != name);
        steps.push(json!({ "step": name, "ok": ok, "detail": self.redact(id, &detail) }));
        if let Ok(mut c) = self.connection(id) {
            c["check"] = json!(steps);
            let _ = self.db.put_config(CONNECTION, id, &c, now_ms());
        }
    }

    /// What the chosen auth method needs before the agent can start, checked without launching it.
    fn credential_check(&self, c: &Value) -> (bool, String) {
        let id = str_of(c, "id");
        let home = self.agent_dir(id).join("home");
        let secrets = self.secrets(id);
        let has = |k: &str| secrets[k].as_str().is_some_and(|v| !v.is_empty());
        let desc = kinds()
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == c["kind"])
            .cloned()
            .unwrap_or(Value::Null);
        if !desc["token"].is_null() {
            return if has("token") {
                (
                    true,
                    format!(
                        "已保存 {}",
                        desc["token"]["label"].as_str().unwrap_or("token")
                    ),
                )
            } else {
                (
                    false,
                    format!(
                        "没有填写 {}",
                        desc["token"]["label"].as_str().unwrap_or("token")
                    ),
                )
            };
        }
        match (str_of(c, "base"), str_of(c, "auth")) {
            (_, "import") => {
                let files: Vec<String> = std::fs::read_dir(&home)
                    .map(|d| {
                        d.flatten()
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .filter(|n| {
                                matches!(
                                    n.as_str(),
                                    "config.toml"
                                        | "auth.json"
                                        | "settings.json"
                                        | "models.json"
                                        | ".credentials.json"
                                        | ".claude.json"
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let env: Vec<String> = secrets["importedEnv"]
                    .as_object()
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                if files.is_empty() && env.is_empty() {
                    (false, "没有复制到任何配置，请检查来源目录后重新保存".into())
                } else {
                    (
                        true,
                        [
                            files.join("、"),
                            env.iter()
                                .map(|e| format!("${e}"))
                                .collect::<Vec<_>>()
                                .join("、"),
                        ]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("，"),
                    )
                }
            }
            (_, "chatgpt") | (_, "account") => {
                if home.join("auth.json").exists() {
                    (true, "已登录".into())
                } else {
                    (false, "还没有登录".into())
                }
            }
            (_, "apikey") => {
                if has("apiKey") {
                    (true, "已保存 API Key".into())
                } else {
                    (false, "没有填写 API Key".into())
                }
            }
            (_, "oauth") => {
                if has("token") {
                    (true, "已保存 OAuth Token".into())
                } else {
                    (false, "没有填写 OAuth Token".into())
                }
            }
            ("pi", "provider") => {
                if home.join("models.json").exists() {
                    (true, "已生成 models.json".into())
                } else {
                    (false, "缺少 models.json，请重新保存".into())
                }
            }
            ("custom", _) => (true, "由启动命令自己负责".into()),
            _ => (true, "无需凭据".into()),
        }
    }

    /// The full configuration check: runtime → credentials → ACP handshake → session and models → one real reply.
    pub async fn test_connection(self: &Arc<Self>, id: &str) -> Result<Value> {
        let dir = self.agent_dir(id);
        let probe = dir.join("probe");
        std::fs::create_dir_all(&probe)?;
        let log = dir.join("test.log");
        let _ = std::fs::remove_file(&log);
        let started = now_ms();
        let mut steps: Vec<Value> = vec![];
        let c0 = self.connection(id)?;

        let outcome: Result<Value> = async {
            // An agent's adapter is installed the first time it is needed: here, before the check uses it.
            if let Some(adapter) = crate::adapters::for_base(str_of(&c0, "base")) {
                self.check_step(id, &mut steps, "ACP 适配器", None, format!("检查 {} 的适配器…", adapter.name));
                let app = self.clone();
                let installed = tokio::task::spawn_blocking(move || app.ensure_adapter(adapter.id)).await?;
                match installed {
                    Ok(Some(v)) => self.check_step(id, &mut steps, "ACP 适配器", Some(true), format!("已安装 {} {v}", adapter.package)),
                    Ok(None) => {
                        let v = self.adapter_entry(adapter.id).ok().and_then(|p| p.ancestors().find(|a| a.join(".janus-adapter.json").is_file()).and_then(|a| a.file_name().map(|n| n.to_string_lossy().to_string()))).unwrap_or_default();
                        self.check_step(id, &mut steps, "ACP 适配器", Some(true), format!("{} {v}", adapter.package));
                    }
                    Err(err) => {
                        self.check_step(id, &mut steps, "ACP 适配器", Some(false), format!("安装失败：{err:#}"));
                        return Err(err);
                    }
                }
            }
            let spec = match self.spec_for(id) {
                Ok(spec) => spec,
                Err(err) => {
                    self.check_step(id, &mut steps, "运行时", Some(false), format!("{err:#}"));
                    return Err(err);
                }
            };
            let program = spec.env.iter().find(|(k, _)| k.ends_with("_PATH") || k == "CLAUDE_CODE_EXECUTABLE").map(|(_, v)| v.clone()).unwrap_or_else(|| spec.program.clone());
            self.check_step(id, &mut steps, "运行时", Some(true), program);

            let (cred_ok, cred) = self.credential_check(&c0);
            self.check_step(id, &mut steps, "凭据", Some(cred_ok), cred.clone());
            if !cred_ok {
                bail!("{cred}");
            }

            self.check_step(id, &mut steps, "启动 Agent", None, "通过 ACP 握手…".into());
            let (conn, mut rx) = match tokio::time::timeout(Duration::from_secs(60), AcpConn::spawn(&spec, &probe, &log)).await {
                Ok(Ok(x)) => x,
                Ok(Err(err)) => {
                    self.check_step(id, &mut steps, "启动 Agent", Some(false), format!("{err:#}"));
                    return Err(err);
                }
                Err(_) => {
                    self.check_step(id, &mut steps, "启动 Agent", Some(false), "60 秒内没有完成 ACP 握手".into());
                    bail!("60 秒内没有完成 ACP 握手");
                }
            };
            self.check_step(id, &mut steps, "启动 Agent", Some(true), format!("{} {}", conn.agent_info["name"].as_str().unwrap_or("agent"), conn.agent_info["version"].as_str().unwrap_or("")));

            let result: Result<Value> = async {
                self.check_step(id, &mut steps, "打开会话", None, "读取模型列表…".into());
                let session = match tokio::time::timeout(Duration::from_secs(60), conn.request("session/new", json!({ "cwd": probe, "mcpServers": [], "_meta": crate::orchestrator::session_meta("janus-probe", &json!({})) }))).await {
                    Ok(r) => r,
                    Err(_) => Err(anyhow!("60 秒内没有打开会话")),
                };
                let session = match session {
                    Ok(s) => s,
                    Err(err) => {
                        self.check_step(id, &mut steps, "打开会话", Some(false), auth_hint(&format!("{err:#}")));
                        return Err(err);
                    }
                };
                let sid = session["sessionId"].as_str().context("session/new 没有返回 sessionId")?.to_string();
                let options = session_options(&session);
                let models = options["model"]["options"].as_array().map(|a| a.len()).unwrap_or(0);
                let perms: Vec<String> = options["controls"].as_array().cloned().unwrap_or_default().iter().filter(|c| c["permission"] == true).map(|c| match c["options"].as_array() {
                    Some(o) if !o.is_empty() => format!("{}：{}", str_of(c, "name"), o.iter().map(|x| str_of(x, "name").to_string()).collect::<Vec<_>>().join(" / ")),
                    _ => format!("{}（开关）", str_of(c, "name")),
                }).collect();
                let models = if models > 0 { format!("{models} 个模型，默认 {}", options["model"]["current"].as_str().unwrap_or("")) } else { "Agent 没有提供模型列表".into() };
                self.check_step(id, &mut steps, "打开会话", Some(true), if perms.is_empty() { format!("{models}；没有声明权限模式") } else { format!("{models}；{}", perms.join("；")) });

                self.check_step(id, &mut steps, "试运行", None, "发送一句测试消息…".into());
                let prompt = conn.request("session/prompt", json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": "Reply with exactly the word PONG and nothing else." }] }));
                tokio::pin!(prompt);
                let mut reply = String::new();
                let deadline = tokio::time::sleep(Duration::from_secs(120));
                tokio::pin!(deadline);
                let run: Result<()> = loop {
                    tokio::select! {
                        r = &mut prompt => break r.map(|_| ()),
                        _ = &mut deadline => break Err(anyhow!("120 秒内没有回复")),
                        Some(msg) = rx.recv() => match msg {
                            Incoming::Notification { params, .. } => {
                                let u = &params["update"];
                                if u["sessionUpdate"] == "agent_message_chunk" {
                                    reply.push_str(u["content"]["text"].as_str().unwrap_or(""));
                                }
                            }
                            Incoming::Request { id, method, .. } => { let _ = conn.respond(id, crate::orchestrator::cancelled_reply(&method)).await; }
                            Incoming::Closed => break Err(anyhow!("Agent 进程已退出")),
                        }
                    }
                };
                if let Err(err) = run {
                    self.check_step(id, &mut steps, "试运行", Some(false), auth_hint(&format!("{err:#}")));
                    return Err(err);
                }
                if reply.trim().is_empty() {
                    self.check_step(id, &mut steps, "试运行", Some(false), "Agent 没有返回任何内容，可能是模型或凭据有误".into());
                    bail!("Agent 没有返回任何内容");
                }
                self.check_step(id, &mut steps, "试运行", Some(true), format!("回复：{}", crate::orchestrator::short(reply.trim(), 40)));
                Ok(json!({ "agentInfo": conn.agent_info, "options": options, "reply": reply.trim() }))
            }
            .await;
            conn.kill().await;
            result
        }
        .await;

        let mut c = self.connection(id)?;
        c["checkedAt"] = json!(now_ms());
        c["elapsedMs"] = json!(now_ms() - started);
        c["check"] = json!(steps);
        match outcome {
            Ok(found) => {
                c["status"] = json!("ok");
                c["error"] = Value::Null;
                c["errorLog"] = Value::Null;
                c["agentInfo"] = found["agentInfo"].clone();
                c["options"] = found["options"].clone();
                c["reply"] = json!(crate::orchestrator::short(
                    found["reply"].as_str().unwrap_or(""),
                    80
                ));
            }
            Err(err) => {
                let failed = steps
                    .iter()
                    .find(|s| s["ok"] == false)
                    .map(|s| format!("{}：{}", str_of(s, "step"), str_of(s, "detail")))
                    .unwrap_or_else(|| auth_hint(&format!("{err:#}")));
                c["status"] = json!("error");
                c["error"] = json!(self.redact(id, &failed));
                c["errorLog"] = json!(self.redact(id, &tail(&log, 15)));
            }
        }
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        Ok(c)
    }

    /// Codex ChatGPT login into the connection's own CODEX_HOME. `codex login` opens the browser itself.
    pub fn start_login(self: &Arc<Self>, id: &str) -> Result<()> {
        let mut c = self.connection(id)?;
        let (bin_env, home_env, args): (&str, &str, &[&str]) = match login_kind(&c) {
            Some("codex") => ("CODEX_PATH", "CODEX_HOME", &["login"]),
            Some("grok") => ("GROK_PATH", "GROK_HOME", &["login", "--device-auth"]),
            _ => bail!("这个 Provider 的认证方式不需要登录"),
        };
        if c["login"]["state"] == "running" {
            return Ok(());
        }
        let spec = self.spec_for(id)?;
        let program = spec
            .env
            .iter()
            .find(|(k, _)| k == bin_env)
            .map(|(_, v)| v.clone())
            .unwrap();
        let dir = self.agent_dir(id);
        let log = std::fs::File::create(dir.join("login.log"))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.env_clear();
        let mut child = cmd
            .envs(spec.env.iter().cloned())
            .args(args)
            .env(home_env, dir.join("home"))
            .env("GROK_DISABLE_AUTOUPDATER", "1")
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("启动登录命令失败")?;
        c["login"] = json!({ "state": "running", "at": now_ms() });
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let status = tokio::time::timeout(Duration::from_secs(600), child.wait()).await;
            let state = match status {
                Ok(Ok(s)) if s.success() => "done",
                Ok(_) => "failed",
                Err(_) => {
                    let _ = child.kill().await;
                    "timeout"
                }
            };
            if let Ok(mut c) = app.connection(&id) {
                c["login"] = json!({ "state": state, "at": now_ms() });
                c["status"] = json!("untested");
                let _ = app.db.put_config(CONNECTION, &id, &c, now_ms());
            }
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // Roles
    // ------------------------------------------------------------------

    pub fn roles(&self) -> Result<Vec<Value>> {
        self.db.list_config(ROLE)
    }

    pub fn save_role(&self, body: &Value) -> Result<Value> {
        let name = str_of(body, "name");
        if name.is_empty() {
            bail!("角色需要一个名字");
        }
        let id = match str_of(body, "id") {
            "" => new_id("r"),
            id => id.to_string(),
        };
        let connection = str_of(body, "connection");
        // The session controls this role chose, checked against what its agent declares.
        let session = if connection.is_empty() {
            json!({})
        } else {
            role_session(body, &self.connection(connection)?)
        };
        let init = match str_of(body, "init") {
            "" => name
                .chars()
                .filter(|c| !c.is_whitespace())
                .take(2)
                .collect::<String>()
                .to_uppercase(),
            i => i.to_string(),
        };
        let library = skill_library();
        let skills: Vec<String> = body["skills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| check_skill_id(s).is_ok() && library.join(s).join("SKILL.md").is_file())
            .map(str::to_owned)
            .collect();
        // MCP environment values are secrets: SQLite keeps the variable names, the values live in a 0600 file.
        let old_secrets = self.role_secrets(&id);
        let mut secrets = serde_json::Map::new();
        let mut mcp = vec![];
        for m in body["mcp"].as_array().cloned().unwrap_or_default() {
            let mut entry = resolve_mcp(&m)?;
            let name = str_of(&entry, "name").to_string();
            let mut env = entry["env"].as_object().cloned().unwrap_or_default();
            if env.is_empty() {
                if let Some(prev) = old_secrets[&name].as_object() {
                    env = prev.clone();
                }
            }
            entry["envKeys"] = json!(env.keys().collect::<Vec<_>>());
            entry.as_object_mut().unwrap().remove("env");
            secrets.insert(name, Value::Object(env));
            mcp.push(entry);
        }
        self.write_role_secrets(&id, &Value::Object(secrets))?;
        let role = json!({
            "id": id, "name": name, "role": str_of(body, "role"), "desc": str_of(body, "desc"),
            "prompt": body["prompt"].as_str().unwrap_or("").trim(), "skills": skills, "mcp": mcp,
            "connection": connection, "model": str_of(body, "model"), "effort": str_of(body, "effort"),
            "session": session,
            "lead": body["lead"].as_bool().unwrap_or(false),
            "color": match str_of(body, "color") { "" => "#64748b", c => c }, "init": init,
        });
        self.db.put_config(ROLE, &id, &role, now_ms())?;
        Ok(role)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        if let Some(ws) = self.db.list_config(WORKSPACE)?.iter().find(|w| {
            w["members"]
                .as_array()
                .is_some_and(|m| m.iter().any(|m| m["role"] == id))
        }) {
            bail!("工作区「{}」正在使用这个角色，请先移除", str_of(ws, "name"));
        }
        let _ = std::fs::remove_file(self.role_secrets_path(id));
        self.db.delete_config(ROLE, id)
    }

    fn role_secrets_path(&self, id: &str) -> PathBuf {
        self.cfg
            .agents_dir
            .parent()
            .unwrap_or(&self.cfg.agents_dir)
            .join("secrets")
            .join("roles")
            .join(format!("{id}.json"))
    }

    pub fn role_secrets_of(&self, id: &str) -> Value {
        self.role_secrets(id)
    }

    fn role_secrets(&self, id: &str) -> Value {
        std::fs::read_to_string(self.role_secrets_path(id))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    fn write_role_secrets(&self, id: &str, secrets: &Value) -> Result<()> {
        let path = self.role_secrets_path(id);
        if secrets.as_object().is_none_or(|m| {
            m.values()
                .all(|v| v.as_object().is_none_or(|e| e.is_empty()))
        }) {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )?;
        write_private(&path, serde_json::to_string_pretty(secrets)?.as_bytes())
    }

    /// Claude Code runs `/goal` only in a workspace it trusts. The lead's agent has its own
    /// config directory, so record the trust there (never in the user's own Claude config).
    pub fn trust_claude_workspace(&self, id: &str, path: &Path) -> Result<()> {
        let file = self.agent_dir(id).join("home").join(".claude.json");
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

    /// First start: a usable set of roles that only lacks agent bindings.
    pub fn seed_roles(&self) -> Result<()> {
        if self.db.get_config("meta", "seeded")?.is_some() {
            return Ok(());
        }
        let seeds = [
            (
                "coordinator",
                "Lead",
                "主控",
                "理解目标、拆解任务、协调团队、最终验收",
                "#5452e8",
                "LD",
                true,
            ),
            (
                "researcher",
                "Researcher",
                "调研",
                "阅读代码与资料，梳理现状和约束",
                "#7c3aed",
                "RS",
                false,
            ),
            (
                "implementer",
                "Implementer",
                "实现",
                "直接在工作目录中修改代码并补充测试",
                "#11845a",
                "IM",
                false,
            ),
            (
                "reviewer",
                "Reviewer",
                "评审",
                "独立检查变更，寻找风险与反例",
                "#b0670a",
                "RV",
                false,
            ),
        ];
        for (id, name, role, desc, color, init, lead) in seeds {
            self.save_role(&json!({ "id": id, "name": name, "role": role, "desc": desc, "color": color, "init": init, "lead": lead }))?;
        }
        self.db.put_config("meta", "seeded", &json!(true), now_ms())
    }

    // ------------------------------------------------------------------
    // Execution environments
    // ------------------------------------------------------------------

    pub fn workspaces(&self) -> Result<Vec<Value>> {
        self.db.list_config(WORKSPACE)
    }

    pub fn workspace(&self, id: &str) -> Result<Value> {
        self.db
            .get_config(WORKSPACE, id)?
            .ok_or_else(|| anyhow!("执行环境 {id} 不存在"))
    }

    // @project-doc docs/domains/agents_and_roles.md#workspaces
    pub fn save_workspace(&self, body: &Value) -> Result<Value> {
        let path = PathBuf::from(expand_home(str_of(body, "path")));
        if !path.is_dir() {
            bail!("目录不存在：{}", path.display());
        }
        let path = path.canonicalize()?;
        let id = match str_of(body, "id") {
            "" => new_id("w"),
            id => id.to_string(),
        };
        let mut members = vec![];
        for m in body["members"].as_array().cloned().unwrap_or_default() {
            let role = self
                .db
                .get_config(ROLE, str_of(&m, "role"))?
                .ok_or_else(|| anyhow!("角色 {} 不存在", str_of(&m, "role")))?;
            members.push(json!({ "role": role["id"] }));
        }
        let lead = str_of(body, "lead");
        if !members.iter().any(|m| m["role"] == lead) {
            bail!("请从成员中选择一个主控");
        }
        let lead_role = self.db.get_config(ROLE, lead)?.unwrap_or_default();
        let lead_kind = self.connection(str_of(&lead_role, "connection")).map(|c| str_of(&c, "base").to_string()).unwrap_or_default();
        if !crate::orchestrator::lead_capable(&lead_kind) {
            bail!("主控只能是 Claude Code 或 Codex 的角色（需要 /goal），「{}」不是", str_of(&lead_role, "name"));
        }
        if members.len() < 2 {
            bail!("除了主控，至少还需要一名成员");
        }
        let name = match str_of(body, "name") {
            "" => path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".into()),
            n => n.to_string(),
        };
        let ws = json!({ "id": id, "name": name, "path": path, "lead": lead, "members": members });
        self.db.put_config(WORKSPACE, &id, &ws, now_ms())?;
        Ok(ws)
    }

    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        self.db.delete_config(WORKSPACE, id)
    }

    /// Snapshot the environment into the run's team. The lead keeps the fixed member id "lead".
    pub fn team_for(&self, ws: &Value) -> Result<(PathBuf, Vec<Member>)> {
        let mut team = vec![];
        for m in ws["members"].as_array().cloned().unwrap_or_default() {
            let role = self
                .db
                .get_config(ROLE, str_of(&m, "role"))?
                .ok_or_else(|| anyhow!("角色 {} 已被删除", str_of(&m, "role")))?;
            let conn_id = str_of(&role, "connection");
            if conn_id.is_empty() {
                bail!("角色「{}」还没有绑定 Agent 连接", str_of(&role, "name"));
            }
            let conn = self.connection(conn_id)?;
            let lead = m["role"] == ws["lead"];
            let session = role_session(&role, &conn);
            // Whether this member edits files is its agent's permission mode, nothing else.
            let write = session_writes(&session, &conn);
            let permission = session_label(&session, &conn);
            let model_label = model_label(&role, &conn);
            let mut limits = vec![if write {
                "按权限模式会修改文件，直接在工作目录中".to_string()
            } else {
                "按权限模式只读，不修改文件".to_string()
            }];
            limits.push(match permission.as_str() {
                "" => "Agent 没有权限模式，审批和提问转给你".into(),
                l => format!("权限：{l}；需要审批或填写的请求转给你"),
            });
            team.push(Member {
                // "lead" is reserved for whichever role leads this environment.
                id: if lead {
                    "lead".into()
                } else if role["id"] == "lead" {
                    "lead-role".into()
                } else {
                    str_of(&role, "id").into()
                },
                name: str_of(&role, "name").into(),
                role: str_of(&role, "role").into(),
                desc: str_of(&role, "desc").into(),
                limits,
                ws: "workspace".into(),
                writable: write,
                runtime: conn_id.into(),
                lead,
                color: str_of(&role, "color").into(),
                init: str_of(&role, "init").into(),
                model: str_of(&role, "model").into(),
                effort: str_of(&role, "effort").into(),
                session,
                permission,
                model_label,
                kind: str_of(&conn, "base").into(),
                prompt: str_of(&conn, "prompt").into(),
                role_prompt: str_of(&role, "prompt").into(),
                skills: role["skills"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|id| {
                        id.as_str()
                            .and_then(|id| read_skill(&skill_library().join(id), "Janus"))
                    })
                    .collect(),
                mcp: role["mcp"].as_array().cloned().unwrap_or_default(),
                mcp_env: self.role_secrets(str_of(&role, "id")),
                role_id: str_of(&role, "id").into(),
                runtime_name: str_of(&conn, "name").into(),
            });
        }
        if !team.iter().any(|m| m.lead) {
            bail!("执行环境没有主控");
        }
        Ok((PathBuf::from(str_of(ws, "path")), team))
    }
}

/// The ACP session config options Janus reads from an agent: model and reasoning effort, plus
/// `controls`, everything else the agent lets a session choose. Each agent declares its own:
/// Codex sandbox presets, Claude Code permission modes, Grok's permission mode and Plan toggle.
/// Roles pick values from `controls`; nothing about them is hard-coded per agent.
// @project-doc docs/domains/agents_and_roles.md#permission_modes
pub fn session_options(session: &Value) -> Value {
    let mut out = json!({ "controls": [] });
    let mut controls = vec![];
    for o in session["configOptions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let cat = match o["category"].as_str() {
            Some("model") => "model",
            Some("thought_level") => "effort",
            _ => {
                if let Some(c) = session_control(&o, "config") {
                    controls.push(c);
                }
                continue;
            }
        };
        if !out[cat].is_null() {
            continue;
        }
        out[cat] = json!({
            "id": o["id"], "current": o["currentValue"],
            "options": select_choices(&o["options"])
        });
    }
    // Agents that predate config options report permission modes as ACP session modes.
    let modes = &session["modes"]["availableModes"];
    if !controls.iter().any(|c| c["category"] == "mode") && modes.as_array().is_some_and(|m| !m.is_empty()) {
        controls.push(json!({
            "id": "mode", "name": "Mode", "desc": "", "category": "mode", "type": "select", "via": "mode",
            "current": session["modes"]["currentModeId"], "permission": true,
            "options": modes.as_array().unwrap().iter().map(|m| json!({ "value": m["id"], "name": m["name"], "desc": m["description"] })).collect::<Vec<_>>(),
        }));
    }
    if let Some(mode) = controls.iter().find(|c| c["category"] == "mode") {
        out["mode"] = json!({ "id": mode["id"], "current": mode["current"], "options": mode["options"] });
    }
    out["controls"] = json!(controls);
    out
}

fn select_choices(options: &Value) -> Vec<Value> {
    // Options may also come grouped: [{ group, name, options: [...] }].
    let mut out = vec![];
    for x in options.as_array().cloned().unwrap_or_default() {
        if x["options"].is_array() {
            out.extend(select_choices(&x["options"]));
        } else {
            out.push(json!({ "value": x["value"], "name": x["name"], "desc": x["description"] }));
        }
    }
    out
}

/// One session control in the shape roles use. `permission` marks the ones that decide what the
/// agent may do without asking: they are what the role editor shows under 权限.
fn session_control(o: &Value, via: &str) -> Option<Value> {
    let id = o["id"].as_str()?;
    let kind = o["type"].as_str().unwrap_or("select");
    if kind != "select" && kind != "boolean" {
        return None;
    }
    let category = o["category"].as_str().unwrap_or("");
    let permission = matches!(category, "mode" | "_permission")
        || matches!(id.replace('-', "_").as_str(), "mode" | "permission_mode" | "plan_mode" | "approval_policy" | "sandbox");
    Some(json!({
        "id": id, "name": o["name"].as_str().unwrap_or(id), "desc": o["description"],
        "category": category, "type": kind, "via": via, "current": o["currentValue"], "permission": permission,
        "options": if kind == "select" { json!(select_choices(&o["options"])) } else { json!([]) },
    }))
}

/// What a session should be opened with for a role: its chosen control values, keeping only the
/// ones that still match a control the agent declares.
pub fn role_session(role: &Value, conn: &Value) -> Value {
    let controls = conn["options"]["controls"].as_array().cloned().unwrap_or_default();
    let mut out = serde_json::Map::new();
    for (k, v) in role["session"].as_object().cloned().unwrap_or_default() {
        let fits = match controls.iter().find(|c| c["id"] == k.as_str()) {
            Some(c) if c["type"] == "boolean" => v.is_boolean(),
            Some(c) => c["options"].as_array().is_some_and(|o| o.iter().any(|x| x["value"] == v)),
            // Not probed yet: keep the choice, the session check decides.
            None => controls.is_empty() && (v.is_string() || v.is_boolean()),
        };
        if fits {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

/// Whether a session with these control values may change files, read from the agent's own
/// permission modes: a read-only preset, or Plan switched on, only reads. Agents that declare no
/// permission modes (Pi, most custom commands) edit freely.
// @project-doc docs/domains/agents_and_roles.md#writability
pub fn session_writes(session: &Value, conn: &Value) -> bool {
    let controls = conn["options"]["controls"].as_array().cloned().unwrap_or_default();
    for c in controls.iter().filter(|c| c["permission"] == true) {
        let id = str_of(c, "id").replace('-', "_");
        let v = session.get(c["id"].as_str().unwrap_or_default()).unwrap_or(&c["current"]);
        let plan_on = id.contains("plan") && (v == &json!(true) || v == "on");
        let read_only = v.as_str().is_some_and(|v| {
            let v = v.to_ascii_lowercase().replace(['-', '_', ' '], "");
            v == "readonly" || v == "plan"
        });
        if plan_on || read_only {
            return false;
        }
    }
    true
}

/// The model and reasoning effort a role runs with, by display name: the role's choice, or the
/// agent's default (marked as such) when the role left it open.
pub fn model_label(role: &Value, conn: &Value) -> String {
    let pick = |cat: &str, key: &str| -> Option<(String, bool)> {
        let opt = &conn["options"][cat];
        let chosen = str_of(role, key);
        let (value, default) = if chosen.is_empty() { (opt["current"].as_str()?.to_string(), true) } else { (chosen.to_string(), false) };
        let name = opt["options"]
            .as_array()
            .and_then(|o| o.iter().find(|x| x["value"] == value.as_str()))
            .and_then(|x| x["name"].as_str())
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .unwrap_or(value);
        Some((name, default))
    };
    let model = pick("model", "model");
    let effort = pick("effort", "effort");
    let mut s = match &model {
        Some((m, _)) => m.clone(),
        None => return String::new(),
    };
    if let Some((e, _)) = &effort {
        s.push_str(&format!("，推理强度 {e}"));
    }
    match (model.map(|m| m.1), effort.map(|e| e.1)) {
        (Some(true), Some(true) | None) => s.push_str("（Agent 默认）"),
        (Some(true), Some(false)) => s.push_str("（模型为 Agent 默认）"),
        (Some(false), Some(true)) => s.push_str("（推理强度为 Agent 默认）"),
        _ => {}
    }
    s
}

/// A human label for a role's permission choices, e.g. "Agent · Plan 开启".
pub fn session_label(session: &Value, conn: &Value) -> String {
    let controls = conn["options"]["controls"].as_array().cloned().unwrap_or_default();
    let mut parts = vec![];
    for c in controls.iter().filter(|c| c["permission"] == true) {
        let v = session.get(c["id"].as_str().unwrap_or_default()).unwrap_or(&c["current"]);
        let name = str_of(c, "name");
        parts.push(match v {
            Value::Bool(b) => format!("{name} {}", if *b { "开启" } else { "关闭" }),
            v => c["options"]
                .as_array()
                .and_then(|o| o.iter().find(|x| &x["value"] == v))
                .map(|x| str_of(x, "name").to_string())
                .unwrap_or_else(|| v.as_str().unwrap_or_default().to_string()),
        });
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

/// Directory listing for the folder picker. Browsers cannot hand a page the absolute path of a
/// chosen folder, so the picker is served by the local backend instead.
pub fn list_dir(path: &str, hidden: bool) -> Result<Value> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let dir = PathBuf::from(expand_home(if path.trim().is_empty() {
        &home
    } else {
        path.trim()
    }));
    let dir = dir
        .canonicalize()
        .with_context(|| format!("目录不存在：{}", dir.display()))?;
    let mut entries = vec![];
    for e in std::fs::read_dir(&dir).with_context(|| format!("无法读取 {}", dir.display()))? {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if (!hidden && name.starts_with('.')) || !e.path().is_dir() {
            continue;
        }
        entries
            .push(json!({ "name": name, "path": e.path(), "git": e.path().join(".git").exists() }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap().to_lowercase())
    });
    entries.truncate(500);
    Ok(
        json!({ "path": dir, "parent": dir.parent(), "home": home, "git": dir.join(".git").exists(), "entries": entries }),
    )
}

// ----------------------------------------------------------------------
// "复制本机配置": migrate an agent's own setup on this machine into a provider's isolated home
// ----------------------------------------------------------------------

/// What may be carried over from the user's own agents: how to reach a model, nothing else.
/// Instructions, hooks, plugins, MCP, skills, permissions and sandbox policy stay behind.
// @project-doc docs/operations/security.md#local_config_import
const CODEX_KEEP: &[&str] = &[
    "model",
    "model_provider",
    "model_providers",
    "model_reasoning_effort",
    "model_reasoning_summary",
    "model_verbosity",
    "model_context_window",
    "model_auto_compact_token_limit",
    "model_catalog_json",
    "openai_base_url",
    "service_tier",
];
const CLAUDE_KEEP: &[&str] = &["env", "model"];
const GROK_KEEP: &[&str] = &["models", "model"];
const PI_SETTINGS_KEEP: &[&str] = &[
    "defaultProvider",
    "defaultModel",
    "defaultThinkingLevel",
    "enabledModels",
];

/// Claude `settings.json` env entries that configure the model endpoint; anything else is user preference.
fn claude_env_kept(k: &str) -> bool {
    k.starts_with("ANTHROPIC_")
        || k.starts_with("CLAUDE_CODE_USE_")
        || k.starts_with("AWS_")
        || k.starts_with("CLOUD_ML_")
        || matches!(
            k,
            "API_TIMEOUT_MS" | "CLAUDE_CODE_MAX_OUTPUT_TOKENS" | "MAX_THINKING_TOKENS"
        )
}

/// Keep only the allowed keys; a referenced model catalog is copied into `home` and re-pointed there.
fn sanitize_codex(text: &str, home: &Path) -> Result<String> {
    let mut t: toml::Table = text.parse().context("config.toml 不是有效的 TOML")?;
    t.retain(|k, _| CODEX_KEEP.iter().any(|x| *x == k));
    if let Some(path) = t
        .get("model_catalog_json")
        .and_then(|v| v.as_str())
        .map(|p| PathBuf::from(expand_home(p)))
    {
        if path.starts_with(home) {
        } else if path.is_file() {
            let local = home.join("model-catalog.json");
            write_private(&local, &std::fs::read(&path)?)?;
            t.insert(
                "model_catalog_json".into(),
                toml::Value::String(local.to_string_lossy().to_string()),
            );
        } else {
            t.remove("model_catalog_json");
        }
    }
    Ok(toml::to_string(&t)?)
}

fn sanitize_json(text: &str, keep: &[&str], what: &str) -> Result<String> {
    let mut v: Value =
        serde_json::from_str(text).with_context(|| format!("{what} 不是有效的 JSON"))?;
    if let Some(m) = v.as_object_mut() {
        m.retain(|k, _| keep.contains(&k.as_str()));
        if let Some(env) = m.get_mut("env").and_then(|e| e.as_object_mut()) {
            env.retain(|k, _| claude_env_kept(k));
        }
    }
    Ok(serde_json::to_string_pretty(&v)?)
}

fn sanitize_toml(text: &str, keep: &[&str]) -> Result<String> {
    let mut t: toml::Table = text.parse().context("config.toml 不是有效的 TOML")?;
    t.retain(|k, _| keep.iter().any(|x| *x == k));
    Ok(toml::to_string(&t)?)
}

/// Bring an agent home in line with the rules above. Runs on import and, for homes made by
/// earlier versions, once at startup; files that are not configuration-for-a-model are removed.
pub fn sanitize_home(base: &str, home: &Path) -> Result<()> {
    let rewrite = |name: &str, f: &dyn Fn(&str) -> Result<String>| -> Result<()> {
        let p = home.join(name);
        if p.is_file() {
            let out = f(&std::fs::read_to_string(&p)?)?;
            write_private(&p, out.as_bytes())?;
        }
        Ok(())
    };
    match base {
        "codex" => rewrite("config.toml", &|t| sanitize_codex(t, home))?,
        "claude" => rewrite("settings.json", &|t| {
            sanitize_json(t, CLAUDE_KEEP, "settings.json")
        })?,
        "grok" => rewrite("config.toml", &|t| sanitize_toml(t, GROK_KEEP))?,
        "pi" => rewrite("settings.json", &|t| {
            sanitize_json(t, PI_SETTINGS_KEEP, "settings.json")
        })?,
        _ => {}
    }
    for global in ["AGENTS.md", "CLAUDE.md"] {
        let _ = std::fs::remove_file(home.join(global));
    }
    Ok(())
}

fn local_source(base: &str, source: &str) -> PathBuf {
    let default = match base {
        "codex" => "~/.codex",
        "claude" => "~/.claude",
        "grok" => "~/.grok",
        _ => "~/.pi/agent",
    };
    PathBuf::from(expand_home(if source.trim().is_empty() {
        default
    } else {
        source.trim()
    }))
}

/// Claude Code keeps its login in the macOS keychain; Claude Code itself reads it through `security`.
fn claude_keychain_credentials() -> Option<String> {
    let user = std::env::var("USER").ok()?;
    let out = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            &user,
            "-w",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && serde_json::from_str::<Value>(&text).is_ok()).then_some(text)
}

/// The user's login-shell environment. Janus may have been started from anywhere, so API keys
/// exported in ~/.zshrc and friends are read the way a new terminal would see them.
fn shell_env() -> HashMap<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = std::process::Command::new(&shell)
        .args(["-ilc", "printf '\\0JANUS_ENV\\0'; env -0"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if let Ok(out) = out {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some((_, tail)) = text.split_once("\0JANUS_ENV\0") {
            for entry in tail.split('\0') {
                if let Some((k, v)) = entry.split_once('=') {
                    env.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    env
}

/// Environment variables that carry an agent's credentials or endpoint when it is not logged in
/// through its own store: the standard ones, plus any a config file names (Codex `env_key`, Pi `apiKey`).
fn credential_vars(base: &str, src: &Path) -> Vec<String> {
    let mut vars: Vec<String> = match base {
        "codex" => vec![
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "OPENAI_ORG_ID",
            "OPENAI_PROJECT_ID",
        ],
        "claude" => vec![
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_CUSTOM_HEADERS",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "AWS_REGION",
            "AWS_PROFILE",
            "ANTHROPIC_VERTEX_PROJECT_ID",
            "CLOUD_ML_REGION",
        ],
        "grok" => vec!["XAI_API_KEY"],
        _ => vec![
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "GEMINI_API_KEY",
            "OPENROUTER_API_KEY",
            "XAI_API_KEY",
            "DEEPSEEK_API_KEY",
        ],
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    let named = |v: &str| {
        v.len() > 1
            && v.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    if base == "codex" {
        let t: Option<toml::Table> = std::fs::read_to_string(src.join("config.toml"))
            .ok()
            .and_then(|s| s.parse().ok());
        if let Some(providers) = t
            .as_ref()
            .and_then(|t| t.get("model_providers"))
            .and_then(|v| v.as_table())
        {
            for p in providers.values() {
                if let Some(k) = p.get("env_key").and_then(|v| v.as_str()) {
                    vars.push(k.to_string());
                }
            }
        }
    }
    if base == "pi" {
        let v: Option<Value> = std::fs::read_to_string(src.join("models.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        for p in v
            .as_ref()
            .and_then(|v| v["providers"].as_object())
            .into_iter()
            .flat_map(|m| m.values())
        {
            if let Some(k) = p["apiKey"].as_str().filter(|k| named(k)) {
                vars.push(k.to_string());
            }
        }
    }
    vars.sort();
    vars.dedup();
    vars
}

/// What a migration would copy, described without secrets.
fn plan_import(base: &str, src: &Path) -> Vec<(String, String)> {
    let has = |f: &str| src.join(f).is_file();
    let mut out = vec![];
    let mut add = |name: &str, note: String| out.push((name.to_string(), note));
    match base {
        "codex" => {
            if has("config.toml") {
                let t: Option<toml::Table> = std::fs::read_to_string(src.join("config.toml"))
                    .ok()
                    .and_then(|s| s.parse().ok());
                let get = |k: &str| {
                    t.as_ref()
                        .and_then(|t| t.get(k))
                        .and_then(|v| v.as_str())
                        .unwrap_or("默认")
                        .to_string()
                };
                add(
                    "config.toml",
                    format!("{} · {}", get("model"), get("model_provider")),
                );
            }
            if has("auth.json") {
                let mode = std::fs::read_to_string(src.join("auth.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .and_then(|v| v["auth_mode"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                add(
                    "auth.json",
                    if mode == "chatgpt" {
                        "ChatGPT 登录令牌".into()
                    } else {
                        "API Key".into()
                    },
                );
            }
        }
        "claude" => {
            if has("settings.json") {
                add("settings.json", "模型与接入点变量".into());
            }
            let account = src
                .parent()
                .map(|p| p.join(".claude.json"))
                .filter(|p| p.is_file() && src.ends_with(".claude"))
                .or_else(|| Some(src.join(".claude.json")).filter(|p| p.is_file()));
            if account.is_some() {
                add(".claude.json", "账号信息".into());
            }
            if has(".credentials.json") {
                add(".credentials.json", "登录令牌".into());
            } else if cfg!(target_os = "macos")
                && src == PathBuf::from(expand_home("~/.claude"))
                && claude_keychain_credentials().is_some()
            {
                add(".credentials.json", "登录令牌（来自钥匙串）".into());
            }
        }
        "grok" => {
            if has("config.toml") {
                add("config.toml", "模型与接入点".into());
            }
            if has("auth.json") {
                add("auth.json", "xAI 登录令牌".into());
            }
        }
        _ => {
            for (f, note) in [
                ("auth.json", "登录凭据"),
                ("models.json", "模型与服务"),
                ("settings.json", "默认模型"),
            ] {
                if has(f) {
                    add(f, note.into());
                }
            }
        }
    }
    out
}

/// Preview for the provider dialog: which files and shell variables would be copied, never their values.
pub fn local_preview(kind: &str, source: &str) -> Value {
    let base = kinds()
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == kind)
        .and_then(|k| k["base"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let src = local_source(&base, source);
    let files: Vec<Value> = plan_import(&base, &src)
        .into_iter()
        .map(|(f, n)| json!({ "file": f, "note": n }))
        .collect();
    let shell = shell_env();
    let env: Vec<Value> = credential_vars(&base, &src)
        .into_iter()
        .filter(|k| shell.get(k).is_some_and(|v| !v.is_empty()))
        .map(|k| json!({ "name": k }))
        .collect();
    json!({ "source": src, "exists": src.is_dir(), "files": files, "env": env })
}

/// Copy the files into `home` and return the credential variables found in the user's shell.
fn import_local(
    base: &str,
    src: &Path,
    home: &Path,
) -> Result<(Vec<String>, serde_json::Map<String, Value>)> {
    let plan = plan_import(base, src);
    let shell = shell_env();
    let env: serde_json::Map<String, Value> = credential_vars(base, src)
        .into_iter()
        .filter_map(|k| {
            shell
                .get(&k)
                .filter(|v| !v.is_empty())
                .map(|v| (k, json!(v)))
        })
        .collect();
    for (file, _) in &plan {
        let from = src.join(file);
        let body: Vec<u8> = match (base, file.as_str()) {
            ("claude", ".claude.json") => {
                let path = src
                    .parent()
                    .map(|p| p.join(".claude.json"))
                    .filter(|p| p.is_file() && src.ends_with(".claude"))
                    .unwrap_or_else(|| src.join(".claude.json"));
                let v: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                let mut keep = serde_json::Map::new();
                for k in [
                    "hasCompletedOnboarding",
                    "lastOnboardingVersion",
                    "oauthAccount",
                    "userID",
                    "firstStartTime",
                ] {
                    if let Some(x) = v.get(k) {
                        keep.insert(k.into(), x.clone());
                    }
                }
                serde_json::to_vec_pretty(&keep)?
            }
            ("claude", ".credentials.json") if !from.is_file() => claude_keychain_credentials()
                .context("无法从钥匙串读取 Claude 登录令牌")?
                .into_bytes(),
            _ => std::fs::read(&from)?,
        };
        write_private(&home.join(file), &body)?;
    }
    sanitize_home(base, home)?;
    Ok((plan.into_iter().map(|(f, _)| f).collect(), env))
}

// ----------------------------------------------------------------------
// Role extensions: skills and MCP servers mounted on top of an agent
// ----------------------------------------------------------------------

/// Janus's own skill library. Agents never read the user's agent skill folders; a skill enters
/// Janus by being written here (created in the UI, or copied once from a folder the user picks).
pub fn skill_library() -> PathBuf {
    crate::config::settings().home.join("skills")
}

fn check_skill_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok {
        bail!("Skill ID 只能用小写字母、数字、- 和 _");
    }
    Ok(())
}

/// A skill is a folder with SKILL.md; its front matter gives the name and the one-line description.
fn read_skill(dir: &Path, source: &str) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let id = dir.file_name()?.to_string_lossy().to_string();
    let mut name = id.clone();
    let mut desc = String::new();
    if let Some(front) = text
        .strip_prefix("---")
        .and_then(|t| t.split_once("\n---"))
        .map(|(f, _)| f)
    {
        let lines: Vec<&str> = front.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if let Some(v) = line.strip_prefix("name:") {
                name = v.trim().trim_matches(['"', '\'']).to_string();
            } else if let Some(v) = line.strip_prefix("description:") {
                desc = v.trim().trim_matches(['"', '\'']).to_string();
                // YAML block scalars (`>`, `|`, `>-`) continue on the indented lines that follow.
                if matches!(desc.as_str(), ">" | "|" | ">-" | "|-" | ">+" | "|+") {
                    desc = lines[i + 1..]
                        .iter()
                        .take_while(|l| l.starts_with(' ') || l.is_empty())
                        .map(|l| l.trim())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();
                }
            }
        }
    }
    Some(
        json!({ "id": id, "path": dir, "name": name, "description": short_desc(&desc), "source": source }),
    )
}

fn short_desc(s: &str) -> String {
    let s: String = s.chars().take(240).collect();
    if s.chars().count() == 240 {
        format!("{s}…")
    } else {
        s
    }
}

pub fn list_skills() -> Value {
    let root = skill_library();
    let mut out: Vec<Value> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|d| read_skill(&d, "Janus"))
        .collect();
    out.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    json!({ "skills": out, "root": root })
}

fn unique_skill_id(want: &str) -> Result<String> {
    let base = slug(want).unwrap_or_else(|| "skill".into());
    let root = skill_library();
    Ok((1..1000)
        .map(|i| {
            if i == 1 {
                base.clone()
            } else {
                format!("{base}-{i}")
            }
        })
        .find(|id| !root.join(id).exists())
        .context("没有可用的 Skill ID")?)
}

/// Write a new skill into the library from the editor.
pub fn create_skill(body: &Value) -> Result<Value> {
    let name = str_of(body, "name");
    let content = body["content"].as_str().unwrap_or("").trim();
    if name.is_empty() || content.is_empty() {
        bail!("Skill 需要名称和内容");
    }
    let id = unique_skill_id(name)?;
    let dir = skill_library().join(&id);
    std::fs::create_dir_all(&dir)?;
    let text = if content.starts_with("---") {
        content.to_string()
    } else {
        format!(
            "---\nname: {name}\ndescription: {}\n---\n\n{content}\n",
            str_of(body, "description").replace('\n', " ")
        )
    };
    std::fs::write(dir.join("SKILL.md"), text)?;
    read_skill(&dir, "Janus").context("写入 Skill 失败")
}

/// Copy a skill folder the user picked into the library. Only regular files are copied, symlinks
/// are resolved to their content, and nothing outside the chosen folder is followed.
pub fn import_skill(path: &str) -> Result<Value> {
    let src = PathBuf::from(expand_home(path.trim()))
        .canonicalize()
        .context("文件夹不存在")?;
    if !src.join("SKILL.md").is_file() {
        bail!("这个文件夹里没有 SKILL.md");
    }
    let id = unique_skill_id(
        &src.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    )?;
    let dest = skill_library().join(&id);
    let mut total = 0u64;
    copy_tree(&src, &src, &dest, &mut total)?;
    read_skill(&dest, "Janus").context("导入 Skill 失败")
}

fn copy_tree(root: &Path, from: &Path, to: &Path, total: &mut u64) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let real = match entry.path().canonicalize() {
            Ok(p) if p.starts_with(root) || p.is_file() => p,
            _ => continue,
        };
        if real.is_dir() {
            if real.starts_with(root) {
                copy_tree(root, &real, &to.join(&name), total)?;
            }
        } else if real.is_file() {
            *total += real.metadata()?.len();
            if *total > 20 * 1024 * 1024 {
                bail!("Skill 超过 20MB，没有导入");
            }
            std::fs::copy(&real, to.join(&name))?;
        }
    }
    Ok(())
}

impl App {
    pub fn delete_skill(&self, id: &str) -> Result<()> {
        check_skill_id(id)?;
        if let Some(role) = self.db.list_config(ROLE)?.iter().find(|r| {
            r["skills"]
                .as_array()
                .is_some_and(|s| s.iter().any(|x| x == id))
        }) {
            bail!("角色「{}」挂载了这个 Skill，请先移除", str_of(role, "name"));
        }
        std::fs::remove_dir_all(skill_library().join(id)).context("删除 Skill 失败")
    }
}

/// Skills that belong to the project itself, found inside the task's own workspace.
/// These are the only skills an agent reads from outside Janus's library.
pub fn project_skills(workspace: &Path) -> Vec<Value> {
    let mut out = vec![];
    for rel in [
        ".agents/skills",
        ".claude/skills",
        ".codex/skills",
        ".pi/skills",
        ".janus/skills",
    ] {
        for e in std::fs::read_dir(workspace.join(rel))
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = e.path();
            if p.canonicalize()
                .is_ok_and(|real| real.starts_with(workspace.canonicalize().unwrap_or_default()))
            {
                if let Some(mut s) = read_skill(&p, "项目") {
                    s["path"] = json!(format!("{rel}/{}", e.file_name().to_string_lossy()));
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Normalize an MCP entry from the role editor.
fn resolve_mcp(m: &Value) -> Result<Value> {
    let m = m.clone();
    let name = str_of(&m, "name").to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("MCP 名称只能包含字母、数字、- 和 _：{name}");
    }
    let (command, args) = match (&m["args"], str_of(&m, "command")) {
        (Value::Array(a), cmd) => (
            cmd.to_string(),
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
        ),
        (_, line) => {
            let mut parts = split_command(line)?;
            if parts.is_empty() {
                bail!("MCP {name} 没有启动命令");
            }
            let cmd = parts.remove(0);
            (cmd, parts)
        }
    };
    if command.is_empty() {
        bail!("MCP {name} 没有启动命令");
    }
    let env = match &m["envText"] {
        Value::String(t) if !t.trim().is_empty() => Value::Object(parse_env(t)?),
        _ => match &m["env"] {
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            json!(
                                v.as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| v.to_string())
                            ),
                        )
                    })
                    .collect(),
            ),
            _ => json!({}),
        },
    };
    Ok(json!({ "name": name, "command": expand_home(&command), "args": args, "env": env }))
}

#[cfg(test)]
mod session_option_tests {
    use super::*;

    #[test]
    fn controls_follow_what_each_agent_declares() {
        // Grok: permission mode under its own category, Plan as a boolean, model and effort apart.
        let grok = session_options(&json!({ "configOptions": [
            { "id": "plan_mode", "name": "Plan", "type": "boolean", "currentValue": false },
            { "id": "permission_mode", "name": "Permission Mode", "category": "_permission", "type": "select", "currentValue": "ask",
              "options": [{ "value": "ask", "name": "Ask Every Time" }, { "value": "always-approve", "name": "Always Approve" }] },
            { "id": "model", "category": "model", "type": "select", "currentValue": "grok-4", "options": [{ "value": "grok-4", "name": "Grok 4" }] },
        ] }));
        let c = grok["controls"].as_array().unwrap();
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|c| c["permission"] == true));
        assert_eq!(grok["model"]["current"], "grok-4");
        let role = json!({ "session": { "permission_mode": "always-approve", "plan_mode": "yes", "gone": "x" } });
        let conn = json!({ "options": grok });
        assert_eq!(role_session(&role, &conn), json!({ "permission_mode": "always-approve" }));
        assert_eq!(session_label(&role_session(&role, &conn), &conn), "Plan 关闭 · Always Approve");

        // Codex: one mode select; a non-permission option stays a plain session option.
        let codex = session_options(&json!({ "configOptions": [
            { "id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "agent",
              "options": [{ "value": "read-only", "name": "Read-only" }, { "value": "agent", "name": "Agent" }, { "value": "agent-full-access", "name": "Agent (full access)" }] },
            { "id": "fast", "name": "Fast", "type": "boolean", "currentValue": false },
        ] }));
        assert_eq!(codex["mode"]["current"], "agent");
        let fast = codex["controls"].as_array().unwrap().iter().find(|c| c["id"] == "fast").unwrap().clone();
        assert_eq!(fast["permission"], false);

        // Older agents: ACP session modes become a control set through session/set_mode.
        let legacy = session_options(&json!({ "modes": { "currentModeId": "default", "availableModes": [{ "id": "default", "name": "Default" }, { "id": "plan", "name": "Plan" }] } }));
        assert_eq!(legacy["controls"][0]["via"], "mode");
        assert_eq!(legacy["controls"][0]["options"].as_array().unwrap().len(), 2);

        // Whether a role edits files comes from the mode alone.
        let codex_conn = json!({ "options": codex });
        assert!(session_writes(&json!({}), &codex_conn), "Codex default Agent mode edits");
        assert!(!session_writes(&json!({ "mode": "read-only" }), &codex_conn));
        assert!(!session_writes(&json!({ "plan_mode": true }), &conn), "Grok with Plan on only reads");
        assert!(session_writes(&json!({}), &json!({ "options": { "controls": [] } })), "Pi declares no modes and edits");
        let claude = json!({ "options": session_options(&json!({ "configOptions": [{ "id": "mode", "category": "mode", "type": "select", "currentValue": "default",
            "options": [{ "value": "default" }, { "value": "acceptEdits" }, { "value": "plan" }] }] })) });
        assert!(!session_writes(&json!({ "mode": "plan" }), &claude));
        assert!(session_writes(&json!({ "mode": "acceptEdits" }), &claude));

        // Model and effort as the lead is told them.
        let opts = json!({ "options": {
            "model": { "current": "gpt-6", "options": [{ "value": "gpt-6", "name": "GPT-6" }, { "value": "sol", "name": "GPT-6.1 Sol" }] },
            "effort": { "current": "medium", "options": [{ "value": "medium", "name": "medium" }, { "value": "high", "name": "high" }] } } });
        assert_eq!(model_label(&json!({ "model": "sol", "effort": "high" }), &opts), "GPT-6.1 Sol，推理强度 high");
        assert_eq!(model_label(&json!({}), &opts), "GPT-6，推理强度 medium（Agent 默认）");
        assert_eq!(model_label(&json!({ "model": "sol" }), &opts), "GPT-6.1 Sol，推理强度 medium（推理强度为 Agent 默认）");
        assert_eq!(model_label(&json!({}), &json!({})), "");

        // Pi: nothing declared.
        assert_eq!(session_options(&json!({ "configOptions": [] }))["controls"], json!([]));
    }
}
