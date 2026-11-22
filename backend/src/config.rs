//! Where Janus keeps the user's things, and the few settings it reads. Everything that belongs to
//! this machine's user (database, agent configs, credentials, ACP adapters, run data, logs) lives
//! in Janus home; the checkout only holds code.
//!
//! A setting comes from its environment variable, else from `<home>/config.toml`, else its default.

use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

// @project-doc docs/interfaces/20261031-configuration.md#settings
pub struct Settings {
    /// Janus home: `JANUS_HOME`, default `~/.janus`.
    pub home: PathBuf,
    /// HTTP listen address, also written into the board's MCP URL.
    pub addr: String,
    /// Node executable the ACP adapters run on and are installed with.
    pub node: String,
    /// Maximum collaborator task attempts running at the same time in one run (`1..=32`, default `3`).
    pub max_concurrent_attempts: usize,
    /// Goal runs working at the same time across all workspaces; more wait in the dispatch queue
    /// (`1..=32`, default `8`).
    pub max_running_runs: usize,
    /// Agent processes open at the same time across goal runs, chats and Loops (`1..=64`, default
    /// `12`); never fewer than `max_running_runs + 1` (`seats`).
    pub max_agent_sessions: usize,
    /// Optional reference for displaying reported token usage.
    pub run_token_budget: Option<i64>,
    /// The person's own home folder, whose sign-ins and tool settings may be shared with agents
    /// (`host_env`): `JANUS_HOST_HOME`, default `$HOME`.
    pub host_home: PathBuf,
}

pub const DEFAULT_MAX_CONCURRENT_ATTEMPTS: usize = 3;
pub const DEFAULT_MAX_RUNNING_RUNS: usize = 8;
pub const DEFAULT_MAX_AGENT_SESSIONS: usize = 12;

/// A count from 1 to `max` from the environment, else `config.toml`, else `default`.
fn parse_count(env: Option<&str>, toml_val: Option<&toml::Value>, default: usize, max: i64) -> usize {
    env.filter(|v| !v.trim().is_empty())
        .and_then(|v| v.trim().parse::<i64>().ok())
        .or_else(|| toml_val.and_then(|v| v.as_integer().or_else(|| v.as_str()?.trim().parse().ok())))
        .map(|n| n.clamp(1, max) as usize)
        .unwrap_or(default)
}

/// A count from 1 to 32 from the environment, else `config.toml`, else `default`.
fn parse_concurrency(env: Option<&str>, toml_val: Option<&toml::Value>, default: usize) -> usize {
    parse_count(env, toml_val, default, 32)
}

fn parse_positive_i64(env: Option<&str>, toml_val: Option<&toml::Value>) -> Option<i64> {
    env.filter(|v| !v.trim().is_empty())
        .and_then(|v| v.trim().parse::<i64>().ok())
        .or_else(|| toml_val.and_then(|v| v.as_integer().or_else(|| v.as_str()?.trim().parse().ok())))
        .filter(|n| *n > 0)
}

// @project-doc docs/operations/20261031-deployment.md#layout
static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    let home = std::env::var("JANUS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".janus"));
    let file: toml::Table = std::fs::read_to_string(crate::layout::Layout::new(&home).config_file())
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_default();
    let pick = |env: &str, key: &str, default: &str| {
        std::env::var(env)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| file.get(key).and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or_else(|| default.to_string())
    };
    let max_concurrent_attempts = parse_concurrency(
        std::env::var("JANUS_MAX_CONCURRENT_ATTEMPTS").ok().as_deref(),
        file.get("max_concurrent_attempts"),
        DEFAULT_MAX_CONCURRENT_ATTEMPTS,
    );
    let max_running_runs = parse_concurrency(
        std::env::var("JANUS_MAX_RUNNING_RUNS").ok().as_deref(),
        file.get("max_running_runs"),
        DEFAULT_MAX_RUNNING_RUNS,
    );
    let max_agent_sessions = parse_count(
        std::env::var("JANUS_MAX_AGENT_SESSIONS").ok().as_deref(),
        file.get("max_agent_sessions"),
        DEFAULT_MAX_AGENT_SESSIONS,
        64,
    );
    let run_token_budget =
        parse_positive_i64(std::env::var("JANUS_RUN_TOKEN_BUDGET").ok().as_deref(), file.get("run_token_budget"));
    Settings {
        addr: pick("JANUS_ADDR", "addr", "127.0.0.1:7878"),
        node: pick("JANUS_NODE", "node", "node"),
        home,
        max_concurrent_attempts,
        max_running_runs,
        max_agent_sessions,
        run_token_budget,
        host_home: std::env::var("JANUS_HOST_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default())),
    }
});

pub fn settings() -> &'static Settings {
    &SETTINGS
}

/// One line of Janus's own log: printed, and appended to `logs/janus.log` in Janus home.
pub fn log(line: impl AsRef<str>) {
    let line = line.as_ref();
    eprintln!("{line}");
    // Tests run against throwaway homes of their own; they must not write into the user's log.
    if cfg!(test) {
        return;
    }
    let file = crate::layout::Layout::new(settings().home.clone()).janus_log();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{} {line}", utc(crate::orchestrator::now_ms()));
    }
}

/// `2026-10-30T14:05:09Z` for a Unix time in milliseconds (civil-from-days, no time zone database).
pub fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formats_unix_time() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_793_368_309_000), "2026-10-30T13:51:49Z");
    }

    #[test]
    fn concurrency_and_token_settings_clamp_and_ignore_non_positive() {
        let t: toml::Table =
            "max_concurrent_attempts = 6\nmax_running_runs = 2\nrun_token_budget = 500000".parse().unwrap();
        assert_eq!(parse_concurrency(None, t.get("max_concurrent_attempts"), 3), 6);
        assert_eq!(parse_concurrency(None, t.get("max_running_runs"), DEFAULT_MAX_RUNNING_RUNS), 2);
        assert_eq!(parse_concurrency(Some("99"), t.get("max_concurrent_attempts"), 3), 32);
        assert_eq!(parse_concurrency(Some("0"), None, 3), 1);
        assert_eq!(parse_concurrency(None, None, DEFAULT_MAX_RUNNING_RUNS), DEFAULT_MAX_RUNNING_RUNS);
        assert_eq!(parse_positive_i64(None, t.get("run_token_budget")), Some(500_000));
        assert_eq!(parse_positive_i64(Some("0"), t.get("run_token_budget")), None);
    }
}
