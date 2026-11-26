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
//! every few hours (`check_adapter_updates`); a newer one is installed and made current without
//! asking anyone. A version the person switched to by hand is pinned in
//! `cache/adapters/<id>/pinned`: automatic updates leave it alone until they resume them.

use crate::orchestrator::{App, Change};
use crate::util::now_ms;
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
    ADAPTERS.iter().find(|a| a.id == id).ok_or_else(|| anyhow!("没有这个运行组件：{id}"))
}

wire_enum! {
    /// What is going on with one adapter right now: an install in progress, or how the last one
    /// ended. Kept in memory; the log file has the full output. A restart leaves no job at all, so
    /// there is no stale "running" to clear.
    #[derive(Default)]
    enum JobState {
        #[default]
        Idle = "idle",
        Running = "running",
        Ok = "ok",
        Error = "error",
    }
}

wire_enum! {
    /// Who started the job: Janus checking for a newer version by itself, or a person installing a
    /// version, retrying, or resuming automatic updates.
    enum JobAction {
        Install = "install",
        Auto = "auto",
    }
}

#[derive(Clone)]
struct Job {
    state: JobState,
    action: JobAction,
    detail: String,
    started: i64,
    ended: i64,
}

/// Version directories that sessions run from, and how many sessions each.
type Pins = Arc<Mutex<HashMap<PathBuf, usize>>>;

/// What this Janus is doing with its adapters: the install running for each, the newest versions
/// npm offered, and the version directories sessions run from. `App` holds it, so two homes (or
/// tests running in parallel) never share it.
#[derive(Default)]
pub struct AdapterState {
    jobs: Mutex<HashMap<String, Job>>,
    /// The newest version npm offers, from the last check: (version, checked at).
    latest: Mutex<HashMap<String, (String, i64)>>,
    /// The version directories sessions run from; `prune` never removes them.
    pins: Pins,
}

impl AdapterState {
    fn begin(&self, id: &str, action: JobAction) -> Result<()> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.get(id).is_some_and(|j| j.state == JobState::Running) {
            bail!("运行组件正在更新，等它结束后再试");
        }
        jobs.insert(
            id.to_string(),
            Job { state: JobState::Running, action, detail: String::new(), started: now_ms(), ended: 0 },
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

    /// Forget how the last job ended, so its failure mark goes away (the person picked a version).
    fn clear_job(&self, id: &str) {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.get(id).is_some_and(|j| j.state != JobState::Running) {
            jobs.remove(id);
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
        let dir = self
            .adapter_current(id)
            .ok_or_else(|| anyhow!("{} 的运行组件还没有安装：在「设置 › 数字员工」中检查 Agent 会自动安装", d.name))?;
        Ok((entry_in(d, &dir)?, self.adapter_state.pin(dir)))
    }

    /// The version the person switched to by hand, if they did: automatic updates leave it alone.
    /// A pin naming a version that is gone (someone emptied `cache/`) is no pin: there is nothing
    /// to stay on, and the next install starts following the newest version again.
    fn adapter_pinned(&self, id: &str) -> Option<String> {
        let v = std::fs::read_to_string(self.cfg.layout.adapter_pin(id)).ok()?.trim().to_string();
        self.adapter_version_installed(id, &v).then_some(v)
    }

    fn pin_adapter(&self, id: &str, version: &str) -> Result<()> {
        std::fs::create_dir_all(self.adapters_dir().join(id))?;
        std::fs::write(self.cfg.layout.adapter_pin(id), version)?;
        Ok(())
    }

    fn unpin_adapter(&self, id: &str) {
        let _ = std::fs::remove_file(self.cfg.layout.adapter_pin(id));
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
                let pinned = self.adapter_pinned(d.id);
                let pinned_now = pinned.as_deref().is_some_and(|p| current.as_deref() == Some(p));
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
                let job = jobs.get(d.id).map(|j| json!({ "state": j.state.as_str(), "action": j.action.as_str(), "detail": j.detail, "started": j.started, "ended": j.ended }));
                let (newest, checked) = latest.get(d.id).cloned().map(|(v, t)| (json!(v), json!(t))).unwrap_or((Value::Null, Value::Null));
                json!({
                    "id": d.id, "name": d.name, "package": d.package,
                    "source": json!({ "kind": "npm", "package": d.package }),
                    "current": current, "versions": versions, "pinned": pinned_now,
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

    /// Start installing in the background: a version (the body's, else the newest). When the person
    /// had pinned an older version, this resumes automatic updates: the pin goes first, and the
    /// newest version already on disk becomes current without downloading it again.
    /// An adapter's job starts; the app hears of it on the global stream.
    fn adapter_begin(&self, id: &str, action: JobAction) -> Result<()> {
        self.adapter_state.begin(id, action)?;
        let _ = self.changes.send(Change::Adapters);
        Ok(())
    }

    /// An adapter's job ends, with what it came to.
    fn adapter_finish(&self, id: &str, r: Result<String>) {
        self.adapter_state.finish(id, r);
        let _ = self.changes.send(Change::Adapters);
    }

    pub fn start_adapter_install(self: &Arc<Self>, id: &str, version: Option<String>) -> Result<()> {
        def(id)?;
        self.adapter_begin(id, JobAction::Install)?;
        let app = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let r = app.install_requested(&id, version.as_deref());
            app.adapter_finish(&id, r.map(|v| format!("已安装 {v}")));
        });
        Ok(())
    }

    /// An install the person asked for, blocking. Any install ends a pin: the person went back to
    /// following the newest version (or picked one, which a later release still replaces).
    fn install_requested(&self, id: &str, version: Option<&str>) -> Result<String> {
        let resume = version.is_none() && self.adapter_pinned(id).is_some();
        self.unpin_adapter(id);
        if resume { self.resume_auto_updates(id) } else { self.install_adapter(id, version) }
    }

    /// Make the newest version current again after the person resumes automatic updates, downloading
    /// it only if it is not on disk (so an offline resume still works when it is); offline with no
    /// known newest, the latest installed version is used.
    fn resume_auto_updates(&self, id: &str) -> Result<String> {
        let known = self.adapter_state.latest.lock().unwrap().get(id).map(|(v, _)| v.clone());
        let newest = known.map(Ok).unwrap_or_else(|| self.latest_adapter_version(id));
        match newest {
            Ok(v) if self.adapter_version_installed(id, &v) => {
                switch_current(&self.adapters_dir().join(id), &v)?;
                self.adapter_note(id, &format!("切换到 {v}（恢复自动更新）"));
                Ok(v)
            }
            Ok(v) => self.install_adapter(id, Some(&v)),
            Err(err) => match self.newest_installed(id) {
                Some(v) => {
                    switch_current(&self.adapters_dir().join(id), &v)?;
                    self.adapter_note(id, &format!("切换到 {v}（恢复自动更新，没能查询最新版本：{err:#}）"));
                    Ok(v)
                }
                None => Err(err),
            },
        }
    }

    fn adapter_version_installed(&self, id: &str, version: &str) -> bool {
        self.adapters_dir().join(id).join(version).join(".janus-adapter.json").is_file()
    }

    /// The highest installed version by number, for a resume that cannot ask npm.
    fn newest_installed(&self, id: &str) -> Option<String> {
        std::fs::read_dir(self.adapters_dir().join(id))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir() && !e.path().is_symlink())
            .filter(|e| e.path().join(".janus-adapter.json").is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .max_by(|a, b| {
                if newer(a, b) {
                    std::cmp::Ordering::Greater
                } else if newer(b, a) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
    }

    /// Make an installed version current again (roll back, or forward) and pin it: the person chose
    /// it, so automatic updates leave it alone until they resume them.
    pub fn use_adapter_version(&self, id: &str, version: &str) -> Result<()> {
        def(id)?;
        if self.adapter_state.running(id) {
            bail!("运行组件正在更新，等它结束后再切换");
        }
        let dir = self.adapters_dir().join(id);
        if version.contains('/') || version.starts_with('.') || !dir.join(version).join(".janus-adapter.json").is_file()
        {
            bail!("没有安装这个版本：{version}");
        }
        switch_current(&dir, version)?;
        self.pin_adapter(id, version)?;
        self.adapter_state.clear_job(id);
        self.adapter_note(id, &format!("切换到 {version} 并固定，不再自动更新"));
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
        self.adapter_begin(id, JobAction::Install)?;
        let r = self.install_adapter(id, None);
        self.adapter_finish(id, r.as_ref().map(|v| format!("已安装 {v}")).map_err(|e| anyhow!("{e:#}")));
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
        let _ = self.changes.send(Change::Adapters);
        Ok(v)
    }

    /// Ask npm for the newest version of every installed adapter (blocking; run it off the async
    /// threads). A later version is installed and made current right away, as the adapter's `auto`
    /// job: nobody is asked and nothing pops up. A version the person pinned is left alone, and a
    /// failed lookup (offline, npm missing) is logged only — never shown as the adapter's job,
    /// since nobody asked for it. A failed install stays as the job's error and the next check
    /// tries again.
    // @project-doc docs/architecture/20261031-adapters.md#update_check
    pub fn check_adapter_updates(&self) {
        self.check_adapter_updates_with(npm_latest, |id, version| self.install_adapter(id, Some(version)));
    }

    fn check_adapter_updates_with(
        &self,
        newest: impl Fn(&AdapterDef) -> Result<String>,
        install: impl Fn(&str, &str) -> Result<String>,
    ) {
        for d in ADAPTERS.iter().filter(|d| self.adapter_current(d.id).is_some() && !self.adapter_state.running(d.id)) {
            let current = self.adapter_version(d.id).unwrap_or_default();
            match newest(d) {
                Ok(v) => {
                    self.adapter_state.latest.lock().unwrap().insert(d.id.to_string(), (v.clone(), now_ms()));
                    let _ = self.changes.send(Change::Adapters);
                    if self.adapter_pinned(d.id).is_some() || !newer(&v, &current) {
                        continue;
                    }
                    if self.adapter_begin(d.id, JobAction::Auto).is_err() {
                        continue;
                    }
                    let done = install(d.id, &v);
                    self.adapter_finish(d.id, done.map(|v| format!("已自动更新到 {v}")));
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
        // The log line names the version only: the app shows Janus's own lines in the failure
        // popover, and package names never belong in the interface (docs/architecture/20261031-adapters.md#update_check).
        self.adapter_note(id, &format!("开始安装 {}", version.unwrap_or("最新版本")));
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
            installed["version"].as_str().context("运行组件清单里没有版本号").map(str::to_owned)
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
        prune(&dir, &v, previous.as_deref(), &self.adapter_state, self.adapter_pinned(id).as_deref());
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
            .with_context(|| format!("{} 的运行组件不完整（缺少清单文件）", d.name))?,
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
        bail!("{} 的运行组件不完整（入口文件不存在）", d.name);
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

/// Keep the current, the previous and the pinned version; remove older ones and leftover staging
/// dirs. The pinned version stays even when it is neither current nor the one before it: the person
/// chose it and can switch back to it.
fn prune(dir: &Path, current: &str, previous: Option<&str>, state: &AdapterState, pinned: Option<&str>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "current"
            || name == current
            || Some(name.as_str()) == previous
            || Some(name.as_str()) == pinned
            || state.pinned(&e.path())
        {
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
        .with_context(|| format!("找不到 Node（{node}），安装 {} 的运行组件需要它", d.name))?;
    let v: Vec<u64> = out.trim().trim_start_matches('v').split('.').filter_map(|x| x.parse().ok()).collect();
    let old = match v.as_slice() {
        [major, minor, ..] => *major < 22 || (*major == 22 && *minor < 19),
        _ => true,
    };
    if old {
        bail!("Node 版本 {} 太旧，运行组件需要 Node 22.19 以上", out.trim());
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

/// Run a step of an install with its output appended to the adapter's log. The command line and
/// what it prints stay in the log (the person can open it from the failure popover); the error
/// itself names no tool or package, because the app shows it in the interface.
fn logged(cmd: &mut Command, log: &Path) -> Result<()> {
    let line = format!(
        "$ {:?} {}",
        cmd.get_program(),
        cmd.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
    );
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(f, "{line}")?;
    let status =
        cmd.stdin(Stdio::null()).stdout(f.try_clone()?).stderr(f).status().with_context(|| "无法运行安装工具")?;
    if !status.success() {
        bail!("安装命令失败（exit {}），详见 {}", status.code().unwrap_or(-1), log.display());
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
        prune(&dir, "1.2.0", Some("1.1.0"), &AdapterState::default(), None);
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, ["1.1.0", "1.2.0", "current"], "the previous version stays for rolling back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The version the person pinned stays through a clean-up even when it is neither current nor
    /// the version before it: it is the one the menu switches back to.
    #[test]
    fn a_pinned_version_is_not_pruned() {
        let dir = std::env::temp_dir().join(format!("janus-adapters-keep-pinned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            std::fs::create_dir_all(dir.join(v)).unwrap();
            std::fs::write(dir.join(v).join(".janus-adapter.json"), "{}").unwrap();
        }
        prune(&dir, "1.2.0", Some("1.1.0"), &AdapterState::default(), Some("1.0.0"));
        assert!(dir.join("1.0.0").is_dir(), "the version the person picked is kept");
        assert!(dir.join("1.1.0").is_dir() && dir.join("1.2.0").is_dir());
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
        prune(&dir, "1.2.0", Some("1.1.0"), &state, None);
        assert!(!AdapterState::default().pinned(&dir.join("1.0.0")), "pins belong to one Janus, not to the process");
        assert!(dir.join("1.0.0").is_dir(), "a running session keeps its version");
        drop(session);
        prune(&dir, "1.2.0", Some("1.1.0"), &state, None);
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

    /// One adapter as `GET /api/adapters` lists it.
    fn listed(app: &Arc<App>, id: &str) -> Value {
        app.adapters()["adapters"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone()
    }

    /// An installed version on disk, as an earlier install would have left it.
    fn add_version(app: &Arc<App>, id: &str, version: &str) {
        let dir = app.adapters_dir().join(id).join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".janus-adapter.json"),
            json!({ "id": id, "version": version, "ref": version, "installedAt": now_ms() }).to_string(),
        )
        .unwrap();
    }

    /// The periodic check asks about installed adapters with no job running, and a later version is
    /// installed as the adapter's `auto` job. The same version starts nothing, a failed lookup only
    /// logs, and a pinned adapter is left alone.
    #[test]
    fn a_later_version_is_installed_automatically() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        // The test app's stand-in adapters are installed as 9.9.9; remove one to see it skipped.
        std::fs::remove_file(app.adapters_dir().join("codex").join("current")).unwrap();
        let asked = Mutex::new(vec![]);
        let installed = Mutex::new(vec![]);
        let mut heard = app.changes.subscribe();
        app.check_adapter_updates_with(
            |d| {
                asked.lock().unwrap().push(d.id);
                Ok("10.0.0".into())
            },
            |id, v| {
                installed.lock().unwrap().push((id.to_string(), v.to_string()));
                Ok(v.to_string())
            },
        );
        assert_eq!(*asked.lock().unwrap(), ["claude"], "an adapter that is not installed is not checked");
        assert_eq!(*installed.lock().unwrap(), [("claude".to_string(), "10.0.0".to_string())]);
        let claude = listed(&app, "claude");
        assert_eq!((claude["latest"].as_str(), claude["updateAvailable"].as_bool()), (Some("10.0.0"), Some(true)));
        assert!(claude["checkedAt"].is_i64());
        assert_eq!((claude["job"]["action"].as_str(), claude["job"]["state"].as_str()), (Some("auto"), Some("ok")));
        assert!(claude["job"]["ended"].is_i64());
        let mut notices = 0;
        while let Ok(Change::Adapters) = heard.try_recv() {
            notices += 1;
        }
        assert_eq!(notices, 3, "the app hears of the newer version, the update's start and its end");

        // The installed version is the newest: nothing starts.
        app.check_adapter_updates_with(
            |_| Ok("9.9.9".into()),
            |id, v| {
                installed.lock().unwrap().push((id.to_string(), v.to_string()));
                Ok(v.to_string())
            },
        );
        assert_eq!(installed.lock().unwrap().len(), 1, "the newest version starts nothing");
        assert_eq!(listed(&app, "claude")["updateAvailable"], false);

        // A lookup that fails keeps what was known and starts nothing.
        app.check_adapter_updates_with(|_| bail!("offline"), |_, _| panic!("nothing to install"));
        assert_eq!(listed(&app, "claude")["latest"], "9.9.9", "a failed lookup keeps what was known");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An automatic install that fails leaves the working version current and the failure on the
    /// job, for the card to show; it is not retried until the next check.
    #[test]
    fn a_failed_automatic_install_keeps_the_version_and_records_the_failure() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-auto-failed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        app.check_adapter_updates_with(
            |d| Ok(if d.id == "claude" { "10.0.0" } else { "9.9.9" }.into()),
            |_, _| bail!("离线"),
        );
        let claude = listed(&app, "claude");
        assert_eq!(claude["current"], "9.9.9", "the working version stays current");
        assert_eq!(claude["job"]["state"], "error");
        assert_eq!(claude["job"]["action"], "auto");
        assert!(claude["job"]["detail"].as_str().unwrap().contains("离线"));
        assert_eq!(claude["pinned"], false, "a failed update does not pin anything");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pin left naming a version no longer on disk (someone emptied `cache/`) is no pin: the
    /// adapter is listed as not pinned and a newer version installs as usual.
    #[test]
    fn a_pin_naming_a_missing_version_is_ignored() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-stale-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        std::fs::write(app.cfg.layout.adapter_pin("claude"), "8.0.0").unwrap();
        assert_eq!(listed(&app, "claude")["pinned"], false);
        let installed = Mutex::new(vec![]);
        app.check_adapter_updates_with(
            |d| Ok(if d.id == "claude" { "10.0.0" } else { "9.9.9" }.into()),
            |id, v| {
                installed.lock().unwrap().push((id.to_string(), v.to_string()));
                Ok(v.to_string())
            },
        );
        assert_eq!(*installed.lock().unwrap(), [("claude".to_string(), "10.0.0".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An adapter with a job running is not checked at all, so a manual install and an automatic
    /// one never overlap.
    #[test]
    fn an_adapter_with_a_job_running_is_not_checked() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        app.adapter_state.begin("claude", JobAction::Install).unwrap();
        let asked = Mutex::new(vec![]);
        app.check_adapter_updates_with(
            |d| {
                asked.lock().unwrap().push(d.id);
                Ok(if d.id == "claude" { "10.0.0" } else { "9.9.9" }.into())
            },
            |_, _| panic!("a running job is left alone"),
        );
        assert_eq!(*asked.lock().unwrap(), ["codex"], "the busy adapter is skipped");
        app.adapter_finish("claude", Ok("完成".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `use` pins the version: the check records the newest but neither installs nor switches, and
    /// a restart reads the pin back from disk.
    #[test]
    fn a_version_the_person_picked_is_pinned_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-pinned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        add_version(&app, "claude", "10.0.0");
        app.use_adapter_version("claude", "9.9.9").unwrap();
        let claude = listed(&app, "claude");
        assert_eq!((claude["current"].as_str(), claude["pinned"].as_bool()), (Some("9.9.9"), Some(true)));
        assert_eq!(std::fs::read_to_string(app.cfg.layout.adapter_pin("claude")).unwrap().trim(), "9.9.9");
        assert!(claude["job"].is_null(), "picking a version clears the last job's failure mark");

        app.check_adapter_updates_with(
            |d| Ok(if d.id == "claude" { "11.0.0" } else { "9.9.9" }.into()),
            |_, _| panic!("a pinned adapter is left alone"),
        );
        let claude = listed(&app, "claude");
        assert_eq!(
            (claude["current"].as_str(), claude["latest"].as_str(), claude["updateAvailable"].as_bool()),
            (Some("9.9.9"), Some("11.0.0"), Some(true)),
            "the newest version is still recorded, for resuming later"
        );
        assert_eq!(claude["pinned"], true);

        // A restart keeps the person's choice: the pin is on disk, not in the process.
        drop(app);
        let app = crate::orchestrator::test_app(&dir);
        let claude = listed(&app, "claude");
        assert_eq!((claude["current"].as_str(), claude["pinned"].as_bool()), (Some("9.9.9"), Some(true)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `install {}` on a pinned adapter resumes automatic updates: the pin goes and the newest
    /// version already on disk is switched to, without downloading it again.
    #[test]
    fn install_resumes_automatic_updates_from_the_newest_installed_version() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        add_version(&app, "claude", "10.0.0");
        app.use_adapter_version("claude", "9.9.9").unwrap();
        // The newest known version is the one on disk.
        app.check_adapter_updates_with(
            |d| Ok(if d.id == "claude" { "10.0.0" } else { "9.9.9" }.into()),
            |_, _| panic!("pinned"),
        );
        app.install_requested("claude", None).unwrap();
        assert_eq!(app.adapter_version("claude").as_deref(), Some("10.0.0"));
        assert!(!app.cfg.layout.adapter_pin("claude").exists());
        let claude = listed(&app, "claude");
        assert_eq!((claude["current"].as_str(), claude["pinned"].as_bool()), (Some("10.0.0"), Some(false)));
        assert!(
            claude["versions"].as_array().unwrap().iter().any(|v| v["version"] == "9.9.9"),
            "the older version stays for switching back"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a failure shows is Janus's own wording: the job's reason and the log lines the popover
    /// shows name neither the install tool, nor the package, nor `node_modules`. The raw log keeps
    /// the command line for `查看日志`.
    #[test]
    fn a_failed_install_keeps_internal_names_out_of_the_reason() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-safe-reason-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::orchestrator::test_app(&dir);
        // The test app runs `python3` as the runtime, and its version check refuses it.
        app.start_adapter_install("claude", Some("10.0.0".into())).unwrap();
        let mut job = Value::Null;
        for _ in 0..200 {
            job = listed(&app, "claude")["job"].clone();
            if job["state"] != "running" {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(job["state"], "error");
        let detail = job["detail"].as_str().unwrap();
        for bad in ["npm", "@agentclientprotocol", "node_modules"] {
            assert!(!detail.to_lowercase().contains(bad), "`{bad}` shown in the reason: {detail}");
        }
        let log = std::fs::read_to_string(app.cfg.layout.adapter_log("claude")).unwrap();
        let notes: Vec<&str> = log.lines().filter(|l| l.starts_with("== ")).collect();
        assert!(notes.iter().any(|l| l.contains("开始安装 10.0.0")), "the step is in the log: {notes:?}");
        assert!(!notes.iter().any(|l| l.to_lowercase().contains("npm") || l.contains("@agentclientprotocol")));
        assert_eq!(listed(&app, "claude")["current"], "9.9.9", "the working version stays current");
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
