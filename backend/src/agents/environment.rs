//! Claude's manual environment. Public settings can be edited in place. Every other value is
//! write-only and represented by its name with a null value in the connection's env map.

use super::*;
use serde_json::Map;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::DirBuilderExt;

/// An agent's secrets file (`Layout::agent_secrets`); none yet reads as empty.
pub(super) fn read_secrets(file: &Path) -> Result<Value> {
    match std::fs::read(file) {
        Ok(bytes) => Ok(Value::Object(serde_json::from_slice(&bytes).context("无法读取 Agent 凭据文件")?)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn write_secrets(file: &Path, secrets: &Value) -> Result<()> {
    // Rename a complete 0600 file in an owner-only folder. A failed write must never truncate
    // the previous credentials.
    let dir = file.parent().context("凭据文件没有所在目录")?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut tmp, secrets)?;
    tmp.flush()?;
    tmp.persist(file)?;
    Ok(())
}

/// Keep the API key in its original private slot, including after restoring an older database
/// backup. A null entry in the current connection decides whether the variable is actually set.
pub(super) fn private_env_value<'a>(secrets: &'a Value, name: &str) -> Option<&'a str> {
    if name == "ANTHROPIC_API_KEY" { &secrets["apiKey"] } else { &secrets["env"][name] }.as_str()
}

/// An unknown variable may contain a gateway credential. Only ordinary Claude settings are
/// returned by the settings API, including an endpoint URL only if it has no embedded credentials.
fn is_public(name: &str, value: &str) -> bool {
    match name {
        "ANTHROPIC_BASE_URL" => reqwest::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
        }),
        "ANTHROPIC_MODEL"
        | "ANTHROPIC_SMALL_FAST_MODEL"
        | "ANTHROPIC_DEFAULT_OPUS_MODEL"
        | "ANTHROPIC_DEFAULT_SONNET_MODEL"
        | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
        | "CLAUDE_CODE_SUBAGENT_MODEL"
        | "CLAUDE_CODE_EFFORT_LEVEL"
        | "CLAUDE_CODE_AUTO_COMPACT_WINDOW"
        | "CLAUDE_CODE_MAX_OUTPUT_TOKENS"
        | "MAX_THINKING_TOKENS"
        | "API_TIMEOUT_MS"
        | "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
        | "CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK" => true,
        _ => false,
    }
}

// @project-doc docs/operations/20261031-security.md#secrets
pub(super) fn protect_claude_env(env: &mut Map<String, Value>, secrets: &mut Value) {
    let mut private = Map::new();
    for (name, value) in env {
        if let Some(s) = value.as_str()
            && !s.is_empty()
            && !is_public(name, s)
        {
            if name == "ANTHROPIC_API_KEY" {
                secrets["apiKey"] = value.take();
            } else {
                private.insert(name.clone(), value.take());
            }
        }
    }
    if !private.is_empty() {
        secrets["env"] = Value::Object(private);
    }
}

/// Version 13. The API key saved apart from the environment becomes its `ANTHROPIC_API_KEY`, and
/// private values typed into the environment move out of SQLite. The private file is written
/// first; if the transaction rolls back, running it again finds the values already there.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn migrate_claude_env(tx: &rusqlite::Transaction, home: Option<&Path>) -> Result<usize> {
    crate::migrations::try_edit_configs(tx, CONNECTION, |id, c| {
        if c["kind"] != "claude" {
            return Ok(false);
        }
        check_id(id)?;
        // Where version 13 kept them; version 14 moves them (`layout::move_to_categories`).
        let file = home.map(|h| h.join("agents").join(id).join("secrets.json"));
        let mut secrets = match &file {
            Some(file) => read_secrets(file)?,
            None => json!({}), // The dry run projects the database shape without touching credential files.
        };
        let before = secrets.clone();
        let api_auth = c["auth"] == "apikey";
        let env = c.get_mut("env").and_then(Value::as_object_mut).context("Claude Agent 环境变量格式无效")?;
        let original = env.clone();
        if api_auth && let Some(key) = private_env_value(&secrets, "ANTHROPIC_API_KEY").filter(|s| !s.is_empty()) {
            env.insert("ANTHROPIC_API_KEY".into(), json!(key));
        }
        protect_claude_env(env, &mut secrets);
        let changed = *env != original;
        if secrets != before
            && let Some(file) = &file
        {
            write_secrets(file, &secrets)?;
        }
        Ok(changed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn claude_exports_reach_the_runtime_and_private_values_round_trip_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::orchestrator::test_app(dir.path());
        let exports = [
            ("ANTHROPIC_BASE_URL", "https://gateway.example.com"),
            ("ANTHROPIC_AUTH_TOKEN", "test-token-value"),
            ("ANTHROPIC_MODEL", "deepseek-v4.1-flash"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "deepseek-v4.1-flash"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "deepseek-v4.1-flash"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "deepseek-v4.1-flash"),
            ("CLAUDE_CODE_SUBAGENT_MODEL", "deepseek-v4.1-flash"),
            ("CLAUDE_CODE_EFFORT_LEVEL", "max"),
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "786432"),
        ];
        let mut body = json!({ "kind":"claude", "auth":"apikey", "newId":"gateway",
            "envText":exports.map(|(k,v)| format!("export {k}={v}")).join("\n") });
        let saved = app.save_connection(&body).unwrap();
        let id = "gateway";
        assert_eq!(saved["id"], id);
        assert!(app.credential_check(&saved).0);
        let env: HashMap<_, _> = app.spec_for(id).unwrap().env.into_iter().collect();
        for (k, v) in exports {
            assert_eq!(env[k], v);
        }
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
        assert_eq!(saved["env"].get("ANTHROPIC_AUTH_TOKEN"), Some(&Value::Null));
        assert_eq!(saved["env"]["ANTHROPIC_MODEL"], "deepseek-v4.1-flash");
        for value in [saved, app.connection(id).unwrap(), json!(app.connections().unwrap())] {
            assert!(!value.to_string().contains("test-token-value"));
        }
        let file = app.cfg.layout.agent_secrets("gateway");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(app.redact(id, "token=test-token-value"), "token=<secret>");

        body["id"] = json!(id);
        body["envText"] = json!("ANTHROPIC_AUTH_TOKEN\nANTHROPIC_MODEL=other");
        app.save_connection(&body).unwrap();
        let launched = || app.spec_for(id).unwrap().env.into_iter().collect::<HashMap<_, _>>();
        assert_eq!(launched()["ANTHROPIC_AUTH_TOKEN"], "test-token-value");
        body["envText"] = json!("ANTHROPIC_AUTH_TOKEN=");
        app.save_connection(&body).unwrap();
        assert!(!launched().contains_key("ANTHROPIC_AUTH_TOKEN"));
        body["envText"] = json!("ANTHROPIC_AUTH_TOKEN");
        assert!(app.save_connection(&body).is_err(), "an absent value cannot be retained");
        body["envText"] = json!("export ANTHROPIC_API_KEY='new-test-key'\nHOME=/outside");
        app.save_connection(&body).unwrap();
        assert_eq!(launched()["ANTHROPIC_API_KEY"], "new-test-key");
        assert_ne!(launched()["HOME"], "/outside");
        body["envText"] = json!("MY_GATEWAY_SECRET=private-value");
        let saved = app.save_connection(&body).unwrap();
        assert!(!launched().contains_key("ANTHROPIC_API_KEY"), "deleting a line removes its value");
        assert!(!saved.to_string().contains("private-value"));
        assert!(!std::fs::read_to_string(&file).unwrap().contains("new-test-key"));
    }
}
