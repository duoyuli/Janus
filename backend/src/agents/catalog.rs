//! The kinds of agent Janus can run and their ways to sign in.

use super::*;

/// Provider types for the "New provider" dialog: Codex and Claude Code.
// @project-doc docs/domains/20261031-agents_and_roles.md#providers
pub fn kinds() -> Value {
    let runtime = |cmd: &str| json!({ "cmd": cmd, "placeholder": format!("/path/to/{cmd}"), "detected": which(cmd) });
    json!([
        {
            "id": "codex", "base": "codex", "name": "Codex", "desc": "OpenAI Codex", "color": "#1f2937", "init": "CX",
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
            "id": "claude", "base": "claude", "name": "Claude", "desc": "Anthropic Claude Code", "color": "#b4532a", "init": "CC",
            "isolation": "CLAUDE_CONFIG_DIR", "runtime": runtime("claude"),
            "auths": [
                { "id": "import", "label": "复制本机配置", "desc": "", "fields": [{ "key": "source", "label": "来源目录", "type": "text", "placeholder": "~/.claude" }] },
                { "id": "oauth", "label": "Claude 订阅", "desc": "在终端运行 claude setup-token，把生成的 token 粘贴到这里。",
                  "fields": [{ "key": "token", "label": "OAuth Token", "type": "secret", "required": true, "placeholder": "sk-ant-oat01-..." }] },
                // Filled in as environment lines (`env`); the template is what the app's
                // "Insert template" button brings in.
                { "id": "apikey", "label": "自定义 API", "desc": "Anthropic API，或兼容 Anthropic API 的网关：用环境变量设置地址、令牌和模型。",
                  "env": true, "template": CLAUDE_API_TEMPLATE, "fields": [] }
            ]
        }
    ])
}

/// The environment of Claude's custom API, as a gateway in front of another model expects it.
const CLAUDE_API_TEMPLATE: &str = "export ANTHROPIC_BASE_URL=https://xxx.xxx.com
export ANTHROPIC_AUTH_TOKEN=sk-xxx
export ANTHROPIC_MODEL=deepseek-v4.1-flash
export ANTHROPIC_DEFAULT_FABLE_MODEL=deepseek-v4.1-flash
export ANTHROPIC_DEFAULT_OPUS_MODEL=deepseek-v4.1-flash
export ANTHROPIC_DEFAULT_SONNET_MODEL=deepseek-v4.1-flash
export ANTHROPIC_DEFAULT_HAIKU_MODEL=deepseek-v4.1-flash
export CLAUDE_CODE_SUBAGENT_MODEL=deepseek-v4.1-flash
export CLAUDE_CODE_EFFORT_LEVEL=max
export CLAUDE_CODE_AUTO_COMPACT_WINDOW=786432";

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

/// A bare name keeps its previously saved private value. Assignment lines are parsed as data.
pub(super) fn parse_env<'a>(
    text: &str,
    saved: impl Fn(&str) -> Option<&'a str>,
) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            let stored = saved(line).with_context(|| format!("环境变量第 {} 行缺少 =", i + 1))?;
            out.insert(line.to_string(), json!(stored));
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("环境变量第 {} 行的名字无效", i + 1);
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
