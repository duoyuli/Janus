//! Where Janus keeps the user's things, and the few settings it reads. Everything that belongs to
//! this machine's user (database, agent configs, credentials, ACP adapters, run data, logs) lives
//! in Janus home; the checkout only holds code.
//!
//! A setting comes from its environment variable, else from `<home>/config.toml`, else its default.

use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

// @project-doc docs/interfaces/configuration.md#settings
pub struct Settings {
    /// Janus home: `JANUS_HOME`, default `~/.janus`.
    pub home: PathBuf,
    /// HTTP listen address, also written into the board's MCP URL.
    pub addr: String,
    /// Node executable the ACP adapters run on and are installed with.
    pub node: String,
}

// @project-doc docs/operations/deployment.md#layout
static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    let home = std::env::var("JANUS_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".janus")
    });
    let file: toml::Table = std::fs::read_to_string(home.join("config.toml"))
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
    Settings {
        addr: pick("JANUS_ADDR", "addr", "127.0.0.1:7878"),
        node: pick("JANUS_NODE", "node", "node"),
        home,
    }
});

pub fn settings() -> &'static Settings {
    &SETTINGS
}

/// One line of Janus's own log: printed, and appended to `<home>/logs/janus.log`.
pub fn log(line: impl AsRef<str>) {
    let line = line.as_ref();
    eprintln!("{line}");
    // Tests run against throwaway homes of their own; they must not write into the user's log.
    if cfg!(test) {
        return;
    }
    let dir = settings().home.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("janus.log")) {
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
    #[test]
    fn utc_formats_unix_time() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(1_793_368_309_000), "2026-10-30T13:51:49Z");
    }
}
