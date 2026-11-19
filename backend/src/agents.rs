//! Setup objects: agent connections → roles → execution environments.
//!
//! A connection is one coding agent (Codex or Claude Code through their ACP adapters, or any ACP
//! agent started by a custom command). Each connection owns an isolated home under
//! `~/.janus/agents/<id>/home` that is handed to the agent as CODEX_HOME / CLAUDE_CONFIG_DIR, so
//! neither the user's own `~/.codex`, `~/.claude` nor their shell credentials leak in.
//! Secrets live in `~/.janus/agents/<id>/secrets.json` (0600) and never in SQLite or the API.
//!
//! The child modules each add one part: `catalog` (agent kinds), `connections`
//! (saving agents, their homes and the sandboxed launch), `checks` (connection checks and
//! sign-in), `roles`, `workspaces`, `session_options` (a session's controls and the no-plan
//! rule) and `local_config` (copying this machine's agent setup). The skills and MCP servers a
//! role uses are the library's (`crate::library`).

use crate::acp::{AcpConn, Incoming, RuntimeSpec};
use crate::orchestrator::{App, Member, expand_home, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod catalog;
mod checks;
mod connections;
mod local_config;
mod roles;
mod session_options;
mod workspaces;

pub use catalog::kinds;
use catalog::*;
pub use checks::CheckState;
pub(crate) use connections::{drop_preset_kinds, is_sandbox_var, sandbox_at};
pub use local_config::local_preview;
use local_config::*;
pub(crate) use roles::{add_generalist, record_seed_origins};
use session_options::*;
pub use session_options::{is_plan_value, plan_off_value, session_label, session_options};
use workspaces::*;

const CONNECTION: &str = "connection";

/// What an agent runs on. Codex and Claude Code start through their official ACP adapters; a
/// custom command is any other ACP agent. Presets over Claude Code run on `Claude`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Codex,
    Claude,
    Custom,
}

impl Runtime {
    /// The runtime of a connection's `base`.
    pub fn of(base: &str) -> Result<Runtime> {
        match base {
            "codex" => Ok(Runtime::Codex),
            "claude" => Ok(Runtime::Claude),
            "custom" => Ok(Runtime::Custom),
            other => bail!("不支持的 Agent 类型：{other}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Runtime::Codex => "codex",
            Runtime::Claude => "claude",
            Runtime::Custom => "custom",
        }
    }

    /// Agents that can lead: they pursue a goal on their own with `/goal`. Tests lead with the
    /// scripted custom agent.
    pub fn leads(self) -> bool {
        matches!(self, Runtime::Claude | Runtime::Codex) || (cfg!(test) && self == Runtime::Custom)
    }

    /// The permission mode a session starts in when its role leaves the mode open: Codex's Full
    /// access and Claude Code's Bypass permissions, so goals run without stopping for approvals.
    /// A custom agent keeps its own default.
    // @project-doc docs/domains/20261031-agents_and_roles.md#permission_modes
    pub fn default_mode(self) -> Option<&'static str> {
        match self {
            Runtime::Codex => Some("agent-full-access"),
            Runtime::Claude => Some("bypassPermissions"),
            Runtime::Custom => None,
        }
    }
}
pub(crate) const ROLE: &str = "role";

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
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// One value for the UI to switch on, instead of combining `status` with `login` and `loggedIn`.
/// The order is the order a person has to clear them: log in, then test the configuration.
pub(crate) fn connection_phase(c: &Value) -> &'static str {
    if c["login"]["state"] == "running" {
        "logging_in"
    } else if account_login(c) && c["loggedIn"].as_bool() != Some(true) {
        "logged_out"
    } else {
        match c["status"].as_str() {
            Some("testing") => "testing",
            Some("ok") => "ok",
            Some("error") => "error",
            _ => "untested",
        }
    }
}

/// Lowercase ASCII words joined by `-`, or None when nothing usable is left (e.g. an all-Chinese name).
pub(crate) fn slug(s: &str) -> Option<String> {
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let out: String = words.join("-").chars().take(32).collect();
    let out = out.trim_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

/// The ids of agents, roles and workspaces: they name folders and files under Janus home, so only
/// lowercase letters, digits and `-`, starting with a letter or digit.
pub fn check_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        bail!("ID「{id}」不合法：只能用小写字母、数字和 -，并以字母或数字开头");
    }
    Ok(())
}

pub(crate) fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().map(str::trim).unwrap_or("")
}

/// A fresh id from the clock. Two objects created within the same millisecond (a script, an
/// import) would otherwise get the same id and the second save would overwrite the first, so the
/// value only ever goes up.
pub(crate) fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = now_ms() as u64;
    let mut last = LAST.load(Ordering::SeqCst);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return format!("{prefix}{}", crate::orchestrator::to_base36(next)),
            Err(seen) => last = seen,
        }
    }
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// A connection with every field a saved one has, plus `extra`, for tests that write one directly.
#[cfg(test)]
pub(crate) fn test_connection(id: &str, kind: &str, base: &str, extra: Value) -> Value {
    let mut c = json!({
        "id": id, "kind": kind, "base": base, "name": id, "auth": "", "bin": "", "prompt": "", "env": {},
        "status": "ok", "createdAt": 1,
    });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        c[k] = v;
    }
    c
}

/// A role with every field a saved one has, plus `extra`, for tests that write one directly.
#[cfg(test)]
pub(crate) fn test_role(id: &str, extra: Value) -> Value {
    let mut r = json!({
        "id": id, "name": id, "desc": "", "prompt": "", "resources": [], "connection": "", "model": "", "effort": "",
        "session": {}, "lead": false, "color": "#64748b", "init": "",
    });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        r[k] = v;
    }
    r
}

#[cfg(test)]
mod new_id_tests {
    /// Objects created in the same millisecond still get distinct ids.
    #[test]
    fn ids_made_together_differ() {
        let ids: std::collections::HashSet<String> = (0..500).map(|_| super::new_id("r")).collect();
        assert_eq!(ids.len(), 500);
    }
}
