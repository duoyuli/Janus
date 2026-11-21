//! Setup objects: agent connections → roles → execution environments.
//!
//! A connection is one coding agent: Codex or Claude Code, through its ACP adapter. Each
//! connection owns an isolated home under
//! `data/agents/<id>/config` in Janus home that is handed to the agent as CODEX_HOME / CLAUDE_CONFIG_DIR, so
//! neither the user's own `~/.codex`, `~/.claude` nor their shell credentials leak in.
//! Secrets live in `data/secrets/agents/<id>.json` (0600) and never in SQLite or the API.
//!
//! The child modules each add one part: `catalog` (agent kinds), `connections`
//! (saving agents, their homes and the sandboxed launch), `checks` (connection checks and
//! sign-in), `roles`, `workspaces`, `session_options` (a session's controls and the no-plan
//! rule) and `local_config` (copying this machine's agent setup). The skills and MCP servers a
//! role uses are the library's (`crate::library`).

use crate::acp::{AcpConn, Incoming, RuntimeSpec};
use crate::orchestrator::{App, Member, expand_home, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod catalog;
mod checks;
mod connections;
mod environment;
mod local_config;
mod roles;
mod runtime;
mod session_options;
mod workspaces;

pub use catalog::kinds;
use catalog::*;
pub use checks::CheckState;
pub(crate) use connections::{drop_custom_agents, drop_preset_kinds, is_sandbox_var, sandbox_vars};
pub(crate) use environment::migrate_claude_env;
pub use local_config::local_preview;
use local_config::*;
pub(crate) use roles::{add_generalist, add_old_friend, record_seed_origins, rename_default_roles};
pub use runtime::{InstructionChannel, Runtime};
use session_options::*;
pub use session_options::{is_plan_value, plan_off_value, session_label, session_options};
use workspaces::*;

const CONNECTION: &str = "connection";

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

/// Where a connection's configuration check is. Stored as the connection's `status` (with
/// `testStartedAt`, `error` and `errorLog`), read and written only through `check_state` and
/// `set_check`, so a check never reads as running without its start or as failed without its
/// reason, and a passing check leaves no old error behind.
// @project-doc docs/domains/20261031-agents_and_roles.md#connection_check
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConnCheck {
    Untested,
    Testing { started: i64 },
    Ok,
    Error { error: String, log: String },
}

impl ConnCheck {
    fn status(&self) -> &'static str {
        match self {
            ConnCheck::Untested => "untested",
            ConnCheck::Testing { .. } => "testing",
            ConnCheck::Ok => "ok",
            ConnCheck::Error { .. } => "error",
        }
    }
}

pub(crate) fn check_state(c: &Value) -> ConnCheck {
    match c["status"].as_str() {
        Some("testing") => ConnCheck::Testing { started: c["testStartedAt"].as_i64().unwrap_or_default() },
        Some("ok") => ConnCheck::Ok,
        Some("error") => ConnCheck::Error { error: str_of(c, "error").into(), log: str_of(c, "errorLog").into() },
        _ => ConnCheck::Untested,
    }
}

pub(crate) fn set_check(c: &mut Value, to: ConnCheck) {
    c["status"] = json!(to.status());
    let (error, log) = match &to {
        ConnCheck::Error { error, log } => (json!(error), json!(log)),
        _ => (Value::Null, Value::Null),
    };
    c["error"] = error;
    c["errorLog"] = log;
    // The start of the latest check stays after it ends: a late write from an older check, which
    // started at another time, is told apart by it (`Check::owns`).
    if let ConnCheck::Testing { started } = to {
        c["testStartedAt"] = json!(started);
    }
}

/// Where an account login (Codex with ChatGPT) is: stored as the connection's `login.state`, with
/// when it changed. A login cut off by a timeout or a restart reads as `Failed`/`Timeout`, and the
/// next login starts afresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoginState {
    None,
    Running,
    Done,
    Failed,
    Timeout,
}

pub(crate) fn login_state(c: &Value) -> LoginState {
    match c["login"]["state"].as_str() {
        Some("running") => LoginState::Running,
        Some("done") => LoginState::Done,
        Some("failed") => LoginState::Failed,
        Some("timeout") => LoginState::Timeout,
        _ => LoginState::None,
    }
}

pub(crate) fn set_login(c: &mut Value, to: LoginState) {
    let state = match to {
        LoginState::None => {
            c["login"] = Value::Null;
            return;
        }
        LoginState::Running => "running",
        LoginState::Done => "done",
        LoginState::Failed => "failed",
        LoginState::Timeout => "timeout",
    };
    c["login"] = json!({ "state": state, "at": now_ms() });
}

/// One value for the UI to switch on, instead of combining `status` with `login` and `loggedIn`.
/// The order is the order a person has to clear them: log in, then test the configuration.
pub(crate) fn connection_phase(c: &Value) -> &'static str {
    if login_state(c) == LoginState::Running {
        "logging_in"
    } else if account_login(c) && c["loggedIn"].as_bool() != Some(true) {
        "logged_out"
    } else {
        check_state(c).status()
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
        "id": id, "name": id, "desc": "", "prompt": "", "connection": "", "model": "", "effort": "",
        "session": {}, "lead": false, "color": "#64748b", "init": "", "avatar": "",
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
