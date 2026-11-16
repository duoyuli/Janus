//! The kinds of agent Janus can run, their ways to sign in and the environment presets they bring.

use super::*;

/// Provider types for the "New provider" dialog: built-in runtimes, presets that
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
/// A preset's default variables: the fixed ones, then the chosen endpoint's.
pub(super) fn preset_env(desc: &Value, c: &Value) -> Vec<(String, String)> {
    let endpoint = desc["endpoints"].as_array().and_then(|e| e.iter().find(|e| e["id"] == c["endpoint"]));
    desc["fixedEnv"]
        .as_object()
        .into_iter()
        .chain(endpoint.and_then(|e| e["env"].as_object()))
        .flat_map(|m| m.iter())
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect()
}

/// The runtime binary the person chose for this agent, if any. The built-in adapters ship their
/// own CLI, so an empty value means "use the bundled one" rather than "look on PATH".
pub(super) fn configured_bin(c: &Value) -> Option<String> {
    let bin = str_of(c, "bin");
    (!bin.trim().is_empty()).then(|| expand_home(bin.trim()))
}

/// A configured variable replaces any default of the same name; an empty value removes it.
pub(super) fn override_env(env: &mut Vec<(String, String)>, k: &str, v: &str) {
    env.retain(|(name, _)| name != k);
    if !v.is_empty() {
        env.push((k.to_string(), v.to_string()));
    }
}

pub fn parse_env(text: &str) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
        let (k, v) = line.split_once('=').ok_or_else(|| anyhow!("环境变量第 {} 行缺少 =", i + 1))?;
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

pub(super) fn auth_hint(msg: &str) -> String {
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

/// Whether Janus drives this agent's account login itself: Codex with a ChatGPT account, which
/// keeps its credentials in `<home>/auth.json`.
pub(super) fn account_login(c: &Value) -> bool {
    c["base"] == "codex" && c["auth"] == "chatgpt"
}
