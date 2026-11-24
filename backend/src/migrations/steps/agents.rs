//! Agent connections' data versions: the presets over Claude Code (7), agents started by a custom
//! command (9), and Claude's API key and private variables leaving the database (13).

use crate::agents::{
    CONNECTION, ROLE, check_id, private_env_value, protect_claude_env, read_secrets, str_of, write_secrets,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

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

/// Data version 9: Janus runs only Codex and Claude Code. Agents started by a custom command are
/// deleted, and the roles bound to one come out unbound, as deleting an agent leaves them. Members
/// of past runs that ran on one are recorded as Claude Code: their agent is gone, so such a run can
/// be read but not continued.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn drop_custom_agents(tx: &rusqlite::Transaction) -> Result<usize> {
    let custom: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id, data FROM configs WHERE kind = ?1")?;
        let rows: Vec<(String, String)> =
            stmt.query_map([CONNECTION], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .filter(|(_, data)| serde_json::from_str::<Value>(data).is_ok_and(|c| c["base"] == "custom"))
            .map(|(id, _)| id)
            .collect()
    };
    let mut changed = 0;
    for id in &custom {
        changed += tx.execute("DELETE FROM configs WHERE kind = ?1 AND id = ?2", rusqlite::params![CONNECTION, id])?;
    }
    changed += super::super::edit_configs(tx, ROLE, |_, role| {
        if !role.get("connection").and_then(Value::as_str).is_some_and(|c| custom.iter().any(|id| id == c)) {
            return false;
        }
        role.insert("connection".into(), json!(""));
        role.insert("model".into(), json!(""));
        role.insert("effort".into(), json!(""));
        role.insert("session".into(), json!({}));
        true
    })?;
    changed += super::super::edit(tx, super::super::Target::RunMember, |m| {
        if m.get("kind").and_then(Value::as_str) != Some("custom") {
            return false;
        }
        m.insert("kind".into(), json!("claude"));
        true
    })?;
    Ok(changed)
}

/// Version 13. The API key saved apart from the environment becomes its `ANTHROPIC_API_KEY`, and
/// private values typed into the environment move out of SQLite. The private file is written
/// first; if the transaction rolls back, running it again finds the values already there.
// @project-doc docs/architecture/20261031-data_model.md#migrations
pub(crate) fn migrate_claude_env(tx: &rusqlite::Transaction, home: Option<&Path>) -> Result<usize> {
    super::super::try_edit_configs(tx, CONNECTION, |id, c| {
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

/// The variables the removed presets over Claude Code (DeepSeek, MiniMax, GLM) set, by kind, and
/// for GLM by the endpoint it was given.
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
