//! "复制本机配置": copying an agent's own setup on this machine into a connection's isolated home,
//! without the user's history, sessions or other credentials.

use super::*;

/// What may be carried over from the user's own agents: how to reach a model, nothing else.
/// Instructions, hooks, plugins, MCP, skills, permissions and sandbox policy stay behind.
// @project-doc docs/operations/security.md#local_config_import
pub(super) const CODEX_KEEP: &[&str] = &[
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

pub(super) const CLAUDE_KEEP: &[&str] = &["env", "model"];

pub(super) const GROK_KEEP: &[&str] = &["models", "model"];

pub(super) const PI_SETTINGS_KEEP: &[&str] =
    &["defaultProvider", "defaultModel", "defaultThinkingLevel", "enabledModels"];

/// Claude `settings.json` env entries that configure the model endpoint; anything else is user preference.
pub(super) fn claude_env_kept(k: &str) -> bool {
    k.starts_with("ANTHROPIC_")
        || k.starts_with("CLAUDE_CODE_USE_")
        || k.starts_with("AWS_")
        || k.starts_with("CLOUD_ML_")
        || matches!(k, "API_TIMEOUT_MS" | "CLAUDE_CODE_MAX_OUTPUT_TOKENS" | "MAX_THINKING_TOKENS")
}

/// Keep only the allowed keys; a referenced model catalog is copied into `home` and re-pointed there.
pub(super) fn sanitize_codex(text: &str, home: &Path) -> Result<String> {
    let mut t: toml::Table = text.parse().context("config.toml 不是有效的 TOML")?;
    t.retain(|k, _| CODEX_KEEP.contains(&k));
    if let Some(path) = t.get("model_catalog_json").and_then(|v| v.as_str()).map(|p| PathBuf::from(expand_home(p))) {
        if path.starts_with(home) {
        } else if path.is_file() {
            let local = home.join("model-catalog.json");
            write_private(&local, &std::fs::read(&path)?)?;
            t.insert("model_catalog_json".into(), toml::Value::String(local.to_string_lossy().to_string()));
        } else {
            t.remove("model_catalog_json");
        }
    }
    Ok(toml::to_string(&t)?)
}

pub(super) fn sanitize_json(text: &str, keep: &[&str], what: &str) -> Result<String> {
    let mut v: Value = serde_json::from_str(text).with_context(|| format!("{what} 不是有效的 JSON"))?;
    if let Some(m) = v.as_object_mut() {
        m.retain(|k, _| keep.contains(&k.as_str()));
        if let Some(env) = m.get_mut("env").and_then(|e| e.as_object_mut()) {
            env.retain(|k, _| claude_env_kept(k));
        }
    }
    Ok(serde_json::to_string_pretty(&v)?)
}

pub(super) fn sanitize_toml(text: &str, keep: &[&str]) -> Result<String> {
    let mut t: toml::Table = text.parse().context("config.toml 不是有效的 TOML")?;
    t.retain(|k, _| keep.contains(&k));
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
        "claude" => rewrite("settings.json", &|t| sanitize_json(t, CLAUDE_KEEP, "settings.json"))?,
        "grok" => rewrite("config.toml", &|t| sanitize_toml(t, GROK_KEEP))?,
        "pi" => rewrite("settings.json", &|t| sanitize_json(t, PI_SETTINGS_KEEP, "settings.json"))?,
        _ => {}
    }
    for global in ["AGENTS.md", "CLAUDE.md"] {
        let _ = std::fs::remove_file(home.join(global));
    }
    Ok(())
}

pub(super) fn local_source(base: &str, source: &str) -> PathBuf {
    let default = match base {
        "codex" => "~/.codex",
        "claude" => "~/.claude",
        "grok" => "~/.grok",
        _ => "~/.pi/agent",
    };
    PathBuf::from(expand_home(if source.trim().is_empty() { default } else { source.trim() }))
}

/// Claude Code keeps its login in the macOS keychain; Claude Code itself reads it through `security`.
pub(super) fn claude_keychain_credentials() -> Option<String> {
    let user = std::env::var("USER").ok()?;
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-a", &user, "-w"])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && serde_json::from_str::<Value>(&text).is_ok()).then_some(text)
}

/// The user's login-shell environment. Janus may have been started from anywhere, so API keys
/// exported in ~/.zshrc and friends are read the way a new terminal would see them.
pub(super) fn shell_env() -> HashMap<String, String> {
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
pub(super) fn credential_vars(base: &str, src: &Path) -> Vec<String> {
    let mut vars: Vec<String> = match base {
        "codex" => vec!["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_ORG_ID", "OPENAI_PROJECT_ID"],
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
    let named = |v: &str| v.len() > 1 && v.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if base == "codex" {
        let t: Option<toml::Table> = std::fs::read_to_string(src.join("config.toml")).ok().and_then(|s| s.parse().ok());
        if let Some(providers) = t.as_ref().and_then(|t| t.get("model_providers")).and_then(|v| v.as_table()) {
            for p in providers.values() {
                if let Some(k) = p.get("env_key").and_then(|v| v.as_str()) {
                    vars.push(k.to_string());
                }
            }
        }
    }
    if base == "pi" {
        let v: Option<Value> =
            std::fs::read_to_string(src.join("models.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        for p in v.as_ref().and_then(|v| v["providers"].as_object()).into_iter().flat_map(|m| m.values()) {
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
pub(super) fn plan_import(base: &str, src: &Path) -> Vec<(String, String)> {
    let has = |f: &str| src.join(f).is_file();
    let mut out = vec![];
    let mut add = |name: &str, note: String| out.push((name.to_string(), note));
    match base {
        "codex" => {
            if has("config.toml") {
                let t: Option<toml::Table> =
                    std::fs::read_to_string(src.join("config.toml")).ok().and_then(|s| s.parse().ok());
                let get =
                    |k: &str| t.as_ref().and_then(|t| t.get(k)).and_then(|v| v.as_str()).unwrap_or("默认").to_string();
                add("config.toml", format!("{} · {}", get("model"), get("model_provider")));
            }
            if has("auth.json") {
                let mode = std::fs::read_to_string(src.join("auth.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .and_then(|v| v["auth_mode"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                add("auth.json", if mode == "chatgpt" { "ChatGPT 登录令牌".into() } else { "API Key".into() });
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
            for (f, note) in [("auth.json", "登录凭据"), ("models.json", "模型与服务"), ("settings.json", "默认模型")]
            {
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
    let files: Vec<Value> =
        plan_import(&base, &src).into_iter().map(|(f, n)| json!({ "file": f, "note": n })).collect();
    let shell = shell_env();
    let env: Vec<Value> = credential_vars(&base, &src)
        .into_iter()
        .filter(|k| shell.get(k).is_some_and(|v| !v.is_empty()))
        .map(|k| json!({ "name": k }))
        .collect();
    json!({ "source": src, "exists": src.is_dir(), "files": files, "env": env })
}

/// Copy the files into `home` and return the credential variables found in the user's shell.
pub(super) fn import_local(
    base: &str,
    src: &Path,
    home: &Path,
) -> Result<(Vec<String>, serde_json::Map<String, Value>)> {
    let plan = plan_import(base, src);
    let shell = shell_env();
    let env: serde_json::Map<String, Value> = credential_vars(base, src)
        .into_iter()
        .filter_map(|k| shell.get(&k).filter(|v| !v.is_empty()).map(|v| (k, json!(v))))
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
                for k in ["hasCompletedOnboarding", "lastOnboardingVersion", "oauthAccount", "userID", "firstStartTime"]
                {
                    if let Some(x) = v.get(k) {
                        keep.insert(k.into(), x.clone());
                    }
                }
                serde_json::to_vec_pretty(&keep)?
            }
            ("claude", ".credentials.json") if !from.is_file() => {
                claude_keychain_credentials().context("无法从钥匙串读取 Claude 登录令牌")?.into_bytes()
            }
            _ => std::fs::read(&from)?,
        };
        write_private(&home.join(file), &body)?;
    }
    sanitize_home(base, home)?;
    Ok((plan.into_iter().map(|(f, _)| f).collect(), env))
}
