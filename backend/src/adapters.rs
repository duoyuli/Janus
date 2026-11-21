//! ACP adapters, installed on demand into Janus home and upgraded in place.
//!
//! Every built-in runtime uses a published npm package. An adapter is installed the first time an
//! agent needs it, or from the Agents page, and each version gets its own directory:
//!
//! ```text
//! cache/adapters/<id>/<version>/        the package and its node_modules
//! cache/adapters/<id>/<version>/.janus-adapter.json
//! cache/adapters/<id>/current -> <version>
//! ```
//!
//! A new version is installed in a staging directory and only becomes `current` once its entry
//! file exists, so a failed install leaves the working one alone. Sessions start from the resolved
//! version directory, so an upgrade never pulls files from under a running agent. The previous
//! version is kept for rolling back; older ones are removed.
//!
//! Installed adapters are checked for a newer version on npm shortly after Janus starts and then
//! every few hours (`check_adapter_updates`); the app reminds the person to upgrade. Nothing is
//! installed without them.

use crate::orchestrator::{App, now_ms};
use crate::wire::wire_enum;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often installed adapters are checked for a newer version.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// How long after start the first check waits, so it never holds up the start.
pub const FIRST_CHECK_AFTER: Duration = Duration::from_secs(45);

pub struct AdapterDef {
    pub id: &'static str,
    pub name: &'static str,
    /// The npm package name. Its `bin` entry is the adapter's script.
    pub package: &'static str,
}

// @project-doc docs/architecture/20261031-adapters.md#catalog
pub const ADAPTERS: &[AdapterDef] = &[
    AdapterDef { id: "codex", name: "Codex", package: "@agentclientprotocol/codex-acp" },
    AdapterDef { id: "claude", name: "Claude Code", package: "@agentclientprotocol/claude-agent-acp" },
];

/// The adapter of a connection's `base`; none for a base Janus doesn't run.
pub fn for_base(base: &str) -> Option<&'static AdapterDef> {
    ADAPTERS.iter().find(|a| a.id == base)
}

fn def(id: &str) -> Result<&'static AdapterDef> {
    ADAPTERS.iter().find(|a| a.id == id).ok_or_else(|| anyhow!("没有这个 ACP 适配器：{id}"))
}

wire_enum! {
    /// What is going on with one adapter right now: an install or update check in progress, or how
    /// the last one ended. Kept in memory; the log file has the full output. A restart leaves no job
    /// at all, so there is no stale "running" to clear.
    #[derive(Default)]
    enum JobState {
        #[default]
        Idle = "idle",
        Running = "running",
        Ok = "ok",
        Error = "error",
    }
}

#[derive(Clone, Default)]
struct Job {
    state: JobState,
    action: String,
    detail: String,
    started: i64,
    ended: i64,
}

/// Version directories that sessions run from, and how many sessions each.
type Pins = Arc<Mutex<HashMap<PathBuf, usize>>>;

/// What this Janus is doing with its adapters: the install or check running for each, the newest
/// versions npm offered, and the version directories sessions run from. `App` holds it, so two
/// homes (or tests running in parallel) never share it.
#[derive(Default)]
pub struct AdapterState {
    jobs: Mutex<HashMap<String, Job>>,
    /// The newest version npm offers, from the last check: (version, checked at).
    latest: Mutex<HashMap<String, (String, i64)>>,
    /// `prune` never removes a pinned version.
    pins: Pins,
}

impl AdapterState {
    fn begin(&self, id: &str, action: &str) -> Result<()> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.get(id).is_some_and(|j| j.state == JobState::Running) {
            bail!("{id} 的适配器已经在{}中", if action == "install" { "安装" } else { "处理" });
        }
        jobs.insert(
            id.to_string(),
            Job { state: JobState::Running, action: action.into(), detail: String::new(), started: now_ms(), ended: 0 },
        );
        Ok(())
    }

    fn finish(&self, id: &str, r: Result<String>) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.get_mut(id) {
            j.ended = now_ms();
            match r {
                Ok(detail) => (j.state, j.detail) = (JobState::Ok, detail),
                Err(err) => (j.state, j.detail) = (JobState::Error, format!("{err:#}")),
            }
        }
    }

    fn running(&self, id: &str) -> bool {
        self.jobs.lock().unwrap().get(id).is_some_and(|j| j.state == JobState::Running)
    }

    fn pin(&self, dir: PathBuf) -> Arc<VersionPin> {
        *self.pins.lock().unwrap().entry(dir.clone()).or_default() += 1;
        Arc::new(VersionPin { dir, pins: self.pins.clone() })
    }

    fn pinned(&self, dir: &Path) -> bool {
        dir.canonicalize().is_ok_and(|d| self.pins.lock().unwrap().contains_key(&d))
    }
}

/// Held by a session started from an adapter version, for as long as it runs (`RuntimeSpec::pin`):
/// the version's files stay while a session still needs them, however many upgrades come.
#[derive(Debug)]
pub struct VersionPin {
    dir: PathBuf,
    pins: Pins,
}

impl Drop for VersionPin {
    fn drop(&mut self) {
        let mut used = self.pins.lock().unwrap();
        if let Some(n) = used.get_mut(&self.dir) {
            *n -= 1;
            if *n == 0 {
                used.remove(&self.dir);
            }
        }
    }
}

impl App {
    fn adapters_dir(&self) -> PathBuf {
        self.cfg.layout.adapters()
    }

    fn adapter_log(&self, id: &str) -> PathBuf {
        self.cfg.layout.adapter_log(id)
    }

    /// The version directory `current` points to, if the adapter is installed.
    fn adapter_current(&self, id: &str) -> Option<PathBuf> {
        let link = self.adapters_dir().join(id).join("current");
        link.canonicalize().ok().filter(|p| p.is_dir())
    }

    // @project-doc docs/architecture/20261031-adapters.md#running_sessions
    /// The script a session of this adapter runs, and a pin that keeps its version installed
    /// while the session holds it.
    pub fn adapter_entry(&self, id: &str) -> Result<(PathBuf, Arc<VersionPin>)> {
        let d = def(id)?;
        let dir = self.adapter_current(id).ok_or_else(|| {
            anyhow!("{} 的 ACP 适配器还没有安装：在「设置 → 数字员工」中检查 Agent 会自动安装", d.name)
        })?;
        Ok((entry_in(d, &dir)?, self.adapter_state.pin(dir)))
    }

    /// The version `current` points to, if the adapter is installed.
    pub fn adapter_version(&self, id: &str) -> Option<String> {
        self.adapter_current(id).and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()))
    }

    /// Every adapter with its installed versions, the newest known version and the current job.
    pub fn adapters(&self) -> Value {
        let jobs = self.adapter_state.jobs.lock().unwrap().clone();
        let latest = self.adapter_state.latest.lock().unwrap().clone();
        let list: Vec<Value> = ADAPTERS
            .iter()
            .map(|d| {
                let current = self
                    .adapter_current(d.id)
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
                let mut versions: Vec<Value> = std::fs::read_dir(self.adapters_dir().join(d.id))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().is_dir() && !e.path().is_symlink())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let meta: Value = std::fs::read_to_string(
                            e.path().join(".janus-adapter.json"),
                        )
                        .ok()
                        .and_then(|t| serde_json::from_str(&t).ok())?;
                        Some(json!({ "version": name, "installedAt": meta["installedAt"], "ref": meta["ref"], "current": Some(&name) == current.as_ref() }))
                    })
                    .collect();
                versions.sort_by_key(|v| std::cmp::Reverse(v["installedAt"].as_i64().unwrap_or(0)));
                let job = jobs.get(d.id).map(|j| json!({ "state": j.state.as_str(), "action": j.action, "detail": j.detail, "started": j.started, "ended": j.ended }));
                let (newest, checked) = latest.get(d.id).cloned().map(|(v, t)| (json!(v), json!(t))).unwrap_or((Value::Null, Value::Null));
                json!({
                    "id": d.id, "name": d.name, "package": d.package,
                    "source": json!({ "kind": "npm", "package": d.package }),
                    "current": current, "versions": versions,
                    "latest": newest, "checkedAt": checked,
                    "updateAvailable": match (&current, newest.as_str()) {
                        (Some(c), Some(n)) => newer(n, c),
                        _ => false,
                    },
                    "job": job, "log": tail(&self.adapter_log(d.id), 15),
                })
            })
            .collect();
        json!({ "adapters": list, "dir": self.adapters_dir() })
    }

    /// Start installing (the newest version, or `version`) in the background.
    pub fn start_adapter_install(self: &Arc<Self>, id: &str, version: Option<String>) -> Result<()> {
        def(id)?;
        self.adapter_state.begin(id, "install")?;
        let app = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let r = app.install_adapter(&id, version.as_deref());
            app.adapter_state.finish(&id, r.map(|v| format!("已安装 {v}")));
        });
        Ok(())
    }

    /// Look up the newest version npm offers, in the background.
    pub fn start_adapter_check(self: &Arc<Self>, id: &str) -> Result<()> {
        def(id)?;
        self.adapter_state.begin(id, "check")?;
        let app = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let r = app.latest_adapter_version(&id);
            app.adapter_state.finish(&id, r.map(|v| format!("最新版本 {v}")));
        });
        Ok(())
    }

    /// Make an installed version current again (roll back, or forward).
    pub fn use_adapter_version(&self, id: &str, version: &str) -> Result<()> {
        def(id)?;
        if self.adapter_state.running(id) {
            bail!("{id} 的适配器正在安装，等它结束后再切换");
        }
        let dir = self.adapters_dir().join(id);
        if version.contains('/') || version.starts_with('.') || !dir.join(version).join(".janus-adapter.json").is_file()
        {
            bail!("没有安装这个版本：{version}");
        }
        switch_current(&dir, version)?;
        self.adapter_note(id, &format!("切换到 {version}"));
        Ok(())
    }

    // @project-doc docs/architecture/20261031-adapters.md#on_demand
    /// Install the adapter now unless it is there already; for the connection check, which an
    /// agent needs to pass before it is used.
    pub fn ensure_adapter(&self, id: &str) -> Result<Option<String>> {
        def(id)?;
        if self.adapter_current(id).is_some() {
            return Ok(None);
        }
        self.adapter_state.begin(id, "install")?;
        let r = self.install_adapter(id, None);
        self.adapter_state.finish(id, r.as_ref().map(|v| format!("已安装 {v}")).map_err(|e| anyhow!("{e:#}")));
        r.map(Some)
    }

    fn adapter_note(&self, id: &str, line: &str) {
        let log = self.adapter_log(id);
        if let Some(dir) = log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(f, "== {} {line}", crate::config::utc(now_ms()));
        }
    }

    /// The newest version npm offers.
    fn latest_adapter_version(&self, id: &str) -> Result<String> {
        let d = def(id)?;
        let v = npm_latest(d)?;
        self.adapter_state.latest.lock().unwrap().insert(id.to_string(), (v.clone(), now_ms()));
        Ok(v)
    }

    /// Ask npm for the newest version of every installed adapter (blocking; run it off the async
    /// threads). What it finds shows as `latest` and `updateAvailable`, and the app reminds the
    /// person; nothing is installed. A lookup that fails (offline, npm missing) is logged and
    /// left out, never shown as the adapter's job, since nobody asked for it.
    // @project-doc docs/architecture/20261031-adapters.md#update_check
    pub fn check_adapter_updates(&self) {
        self.check_adapter_updates_with(npm_latest);
    }

    fn check_adapter_updates_with(&self, newest: impl Fn(&AdapterDef) -> Result<String>) {
        for d in ADAPTERS.iter().filter(|d| self.adapter_current(d.id).is_some() && !self.adapter_state.running(d.id)) {
            match newest(d) {
                Ok(v) => {
                    self.adapter_state.latest.lock().unwrap().insert(d.id.to_string(), (v, now_ms()));
                }
                Err(err) => crate::config::log(format!("janus: {} adapter not checked for updates: {err:#}", d.id)),
            }
        }
    }

    // @project-doc docs/architecture/20261031-adapters.md#install_steps
    /// Install a version in a staging directory, then make it current. Returns the version.
    fn install_adapter(&self, id: &str, version: Option<&str>) -> Result<String> {
        let d = def(id)?;
        let dir = self.adapters_dir().join(id);
        std::fs::create_dir_all(&dir)?;
        let log_path = self.adapter_log(id);
        std::fs::create_dir_all(log_path.parent().unwrap())?;
        let staging = dir.join(format!(".staging-{}", now_ms()));
        self.adapter_note(
            id,
            &format!("开始安装 {}{}", d.package, version.map(|v| format!("@{v}")).unwrap_or_default()),
        );
        let built = (|| -> Result<String> {
            let node = &self.cfg.node;
            check_node(node, d)?;
            std::fs::create_dir_all(&staging)?;
            std::fs::write(staging.join("package.json"), r#"{ "name": "janus-adapter", "private": true }"#)?;
            let spec = format!("{}@{}", d.package, version.unwrap_or("latest"));
            // npm lifecycle scripts are not part of the ACP protocol: skip them so an adapter
            // update does not execute package-provided code at install time.
            logged(
                Command::new("npm")
                    .args(["install", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund", &spec])
                    .current_dir(&staging),
                &log_path,
            )?;
            let installed: Value = serde_json::from_str(&std::fs::read_to_string(
                staging.join("node_modules").join(d.package).join("package.json"),
            )?)?;
            installed["version"].as_str().context("package.json 没有 version").map(str::to_owned)
        })();
        let v = match built {
            Ok(x) => x,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging);
                self.adapter_note(id, &format!("安装失败：{err:#}"));
                return Err(err);
            }
        };
        let target = dir.join(&v);
        if target.join(".janus-adapter.json").is_file() {
            // Already installed: keep that copy, just make it current.
            let _ = std::fs::remove_dir_all(&staging);
        } else {
            if let Err(err) = entry_in(d, &staging) {
                let _ = std::fs::remove_dir_all(&staging);
                self.adapter_note(id, &format!("安装失败：{err:#}"));
                return Err(err);
            }
            let _ = std::fs::remove_dir_all(&target);
            std::fs::write(
                staging.join(".janus-adapter.json"),
                serde_json::to_vec_pretty(
                    &json!({ "id": id, "version": v, "ref": v, "package": d.package, "installedAt": now_ms() }),
                )?,
            )?;
            std::fs::rename(&staging, &target)?;
        }
        let previous = std::fs::read_link(dir.join("current")).ok().map(|p| p.to_string_lossy().to_string());
        switch_current(&dir, &v)?;
        prune(&dir, &v, previous.as_deref(), &self.adapter_state);
        self.adapter_note(id, &format!("已安装 {v} 并设为当前版本"));
        Ok(v)
    }
}

/// The adapter's script inside an installed (or staged) version directory, from its package.json
/// `bin`.
fn entry_in(d: &AdapterDef, dir: &Path) -> Result<PathBuf> {
    let pkg_dir = dir.join("node_modules").join(d.package);
    let pkg: Value = serde_json::from_str(
        &std::fs::read_to_string(pkg_dir.join("package.json"))
            .with_context(|| format!("{} 的适配器缺少 package.json", d.name))?,
    )?;
    let rel = match &pkg["bin"] {
        Value::String(s) => s.clone(),
        Value::Object(m) => {
            m.get(d.package).or_else(|| m.values().next()).and_then(Value::as_str).unwrap_or_default().to_string()
        }
        _ => pkg["main"].as_str().unwrap_or_default().to_string(),
    };
    let entry = pkg_dir.join(rel.trim_start_matches("./"));
    if rel.is_empty() || !entry.is_file() {
        bail!("{} 的适配器入口文件不存在：{}", d.name, entry.display());
    }
    Ok(entry)
}

// @project-doc docs/architecture/20261031-adapters.md#layout
/// Point `current` at `version`, atomically: a new link renamed over the old one.
/// The version npm's `latest` tag points at for the adapter's package.
fn npm_latest(d: &AdapterDef) -> Result<String> {
    let v = run_text(Command::new("npm").args(["view", d.package, "version"]))?.trim().to_string();
    if v.is_empty() {
        bail!("没有查到最新版本");
    }
    Ok(v)
}

/// Whether version `a` comes after `b`, comparing the dotted numbers in turn (a pre-release
/// suffix is ignored). A version installed by hand that is ahead of npm's is not an update.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parts(a) > parts(b)
}

fn switch_current(dir: &Path, version: &str) -> Result<()> {
    let tmp = dir.join(format!(".current-{}", now_ms()));
    std::os::unix::fs::symlink(version, &tmp)?;
    std::fs::rename(&tmp, dir.join("current"))?;
    Ok(())
}

/// Keep the current and the previous version; remove older ones and leftover staging dirs.
fn prune(dir: &Path, current: &str, previous: Option<&str>, state: &AdapterState) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "current" || name == current || Some(name.as_str()) == previous || state.pinned(&e.path()) {
            continue;
        }
        let stale_staging = name.starts_with(".staging-") || name.starts_with(".current-");
        if stale_staging || e.path().join(".janus-adapter.json").is_file() {
            let _ =
                if e.path().is_symlink() { std::fs::remove_file(e.path()) } else { std::fs::remove_dir_all(e.path()) };
        }
    }
}

/// Node must be new enough for the adapters. The floor is 22.19: it was the Pi adapter's minimum
/// and has not been lowered since Pi was dropped.
fn check_node(node: &str, d: &AdapterDef) -> Result<()> {
    let out = run_text(Command::new(node).arg("--version"))
        .with_context(|| format!("找不到 Node（{node}），安装 {} 的适配器需要它", d.name))?;
    let v: Vec<u64> = out.trim().trim_start_matches('v').split('.').filter_map(|x| x.parse().ok()).collect();
    let old = match v.as_slice() {
        [major, minor, ..] => *major < 22 || (*major == 22 && *minor < 19),
        _ => true,
    };
    if old {
        bail!("Node 版本 {} 太旧，ACP 适配器需要 Node 22.19 以上", out.trim());
    }
    Ok(())
}

fn run_text(cmd: &mut Command) -> Result<String> {
    let out = cmd.stdin(Stdio::null()).output().with_context(|| format!("无法运行 {:?}", cmd.get_program()))?;
    if !out.status.success() {
        bail!("{:?} 失败：{}", cmd.get_program(), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Run a step of an install with its output appended to the adapter's log.
fn logged(cmd: &mut Command, log: &Path) -> Result<()> {
    let line = format!(
        "$ {:?} {}",
        cmd.get_program(),
        cmd.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
    );
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(f, "{line}")?;
    let status = cmd
        .stdin(Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f)
        .status()
        .with_context(|| format!("无法运行 {:?}", cmd.get_program()))?;
    if !status.success() {
        bail!("{line} 失败（exit {}），详见 {}", status.code().unwrap_or(-1), log.display());
    }
    Ok(())
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_switch_atomically_and_old_ones_are_pruned() {
        let dir = std::env::temp_dir().join(format!("janus-adapters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            std::fs::create_dir_all(dir.join(v)).unwrap();
            std::fs::write(dir.join(v).join(".janus-adapter.json"), "{}").unwrap();
        }
        std::fs::create_dir_all(dir.join(".staging-1")).unwrap();
        switch_current(&dir, "1.1.0").unwrap();
        switch_current(&dir, "1.2.0").unwrap();
        assert_eq!(std::fs::read_link(dir.join("current")).unwrap(), PathBuf::from("1.2.0"));
        prune(&dir, "1.2.0", Some("1.1.0"), &AdapterState::default());
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, ["1.1.0", "1.2.0", "current"], "the previous version stays for rolling back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A version a session still runs from survives the clean-up, however old it is, and goes
    /// once that session ends.
    #[test]
    fn a_version_in_use_is_not_pruned() {
        let dir = std::env::temp_dir().join(format!("janus-adapters-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            std::fs::create_dir_all(dir.join(v)).unwrap();
            std::fs::write(dir.join(v).join(".janus-adapter.json"), "{}").unwrap();
        }
        let state = AdapterState::default();
        let session = state.pin(dir.join("1.0.0").canonicalize().unwrap());
        prune(&dir, "1.2.0", Some("1.1.0"), &state);
        assert!(!AdapterState::default().pinned(&dir.join("1.0.0")), "pins belong to one Janus, not to the process");
        assert!(dir.join("1.0.0").is_dir(), "a running session keeps its version");
        drop(session);
        prune(&dir, "1.2.0", Some("1.1.0"), &state);
        assert!(!dir.join("1.0.0").exists(), "once the session ends the version goes");
        assert!(dir.join("1.1.0").is_dir() && dir.join("1.2.0").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_later_version_is_an_update() {
        assert!(newer("0.108.0", "0.107.3"));
        assert!(newer("1.0.0", "0.99.9"));
        assert!(newer("2.10.0", "2.9.0"), "numbers, not text");
        assert!(!newer("0.107.0", "0.107.0"));
        assert!(!newer("0.107.0", "9.9.9"), "a version ahead of npm's is not an update");
        assert!(!newer("1.2.0-beta.1", "1.2.0"));
    }

    /// The periodic check asks about installed adapters only, records what it finds for the app
    /// (`updateAvailable` only for a later version), and leaves a failed lookup unshown.
    #[test]
    fn installed_adapters_are_checked_for_a_later_version() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        // The test app's stand-in adapters are installed as 9.9.9; remove one to see it skipped.
        std::fs::remove_file(app.adapters_dir().join("codex").join("current")).unwrap();
        let asked = Mutex::new(vec![]);
        app.check_adapter_updates_with(|d| {
            asked.lock().unwrap().push(d.id);
            Ok("10.0.0".into())
        });
        assert_eq!(*asked.lock().unwrap(), ["claude"], "an adapter that is not installed is not checked");
        let listed = app.adapters();
        let claude = listed["adapters"].as_array().unwrap().iter().find(|a| a["id"] == "claude").unwrap().clone();
        assert_eq!((claude["latest"].as_str(), claude["updateAvailable"].as_bool()), (Some("10.0.0"), Some(true)));
        assert!(claude["checkedAt"].is_i64());
        assert!(claude["job"].is_null(), "a check nobody asked for is not shown as the adapter's job");

        app.check_adapter_updates_with(|_| Ok("9.9.9".into()));
        let claude = app.adapters()["adapters"][1].clone();
        assert_eq!(claude["updateAvailable"], false, "the installed version is the newest");
        app.check_adapter_updates_with(|_| bail!("offline"));
        assert_eq!(app.adapters()["adapters"][1]["latest"], "9.9.9", "a failed lookup keeps what was known");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_comes_from_the_package_bin() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-entry-{}", std::process::id()));
        let d = def("claude").unwrap();
        let pkg = dir.join("node_modules").join(d.package);
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{ "bin": { "claude-agent-acp": "./dist/index.js" }, "main": "dist/lib.js" }"#,
        )
        .unwrap();
        std::fs::write(pkg.join("dist/index.js"), "").unwrap();
        assert_eq!(entry_in(d, &dir).unwrap(), pkg.join("dist/index.js"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
