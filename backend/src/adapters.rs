//! ACP adapters, installed on demand into Janus home and upgraded in place.
//!
//! Each built-in runtime reaches its agent through one adapter (Lody's acp-extension-*). An
//! adapter is installed the first time an agent needs it, or from the Agents page, and each
//! version gets its own directory:
//!
//! ```text
//! <home>/adapters/<id>/<version>/        npm: a package dir with node_modules; git: the checkout
//! <home>/adapters/<id>/<version>/.janus-adapter.json
//! <home>/adapters/<id>/current -> <version>
//! ```
//!
//! A new version is built in a staging directory and only becomes `current` once its entry file
//! exists, so a failed install leaves the working one alone. Each version records the source it
//! came from; a version from an older source (Codex and Claude came from npm until their steering
//! support moved to the repositories) still runs, and the next check replaces it. Sessions start from the resolved
//! version directory, so an upgrade never pulls files from under a running agent. The previous
//! version is kept for rolling back; older ones are removed.

use crate::orchestrator::{App, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex};

pub enum Source {
    /// Published on npm under this package name. No built-in adapter uses it right now; it stays
    /// for adapters whose releases are complete on npm.
    #[allow(dead_code)]
    Npm(&'static str),
    /// Built from the default branch of this repository.
    Git(&'static str),
}

pub struct AdapterDef {
    pub id: &'static str,
    pub name: &'static str,
    /// The npm package name, which is also the adapter's `bin` entry in its package.json.
    pub package: &'static str,
    pub source: Source,
}

// @project-doc docs/architecture/adapters.md#catalog
pub const ADAPTERS: &[AdapterDef] = &[
    // npm stops at older releases (codex 1.1.2, claude 0.54.3) without `_lody/session/steer`; the
    // repositories carry it, so a message reaches a running turn instead of waiting for its end.
    AdapterDef { id: "codex", name: "Codex", package: "acp-extension-codex", source: Source::Git("https://github.com/LodyAI/acp-extension-codex") },
    AdapterDef { id: "claude", name: "Claude Code", package: "acp-extension-claude", source: Source::Git("https://github.com/LodyAI/acp-extension-claude") },
    AdapterDef { id: "grok", name: "Grok", package: "acp-extension-grok", source: Source::Git("https://github.com/LodyAI/acp-extension-grok") },
    AdapterDef { id: "pi", name: "Pi", package: "acp-extension-pi", source: Source::Git("https://github.com/LodyAI/acp-extension-pi") },
];

/// The adapter a runtime needs: presets over Claude Code share Claude's; custom commands need none.
pub fn for_base(base: &str) -> Option<&'static AdapterDef> {
    ADAPTERS.iter().find(|a| a.id == base)
}

fn def(id: &str) -> Result<&'static AdapterDef> {
    ADAPTERS.iter().find(|a| a.id == id).ok_or_else(|| anyhow!("没有这个 ACP 适配器：{id}"))
}

/// The source a version was installed from, as recorded in its `.janus-adapter.json`.
fn source_label(source: &Source) -> String {
    match source {
        Source::Npm(pkg) => format!("npm:{pkg}"),
        Source::Git(repo) => format!("git:{repo}"),
    }
}

// @project-doc docs/architecture/adapters.md#source_change
/// Whether an installed version came from the adapter's current source. Versions installed
/// before the source was recorded are told apart by their layout: an npm install is a wrapper
/// package with the adapter under node_modules, a git install is the checkout itself.
fn from_source(d: &AdapterDef, dir: &Path) -> bool {
    let meta: Value = std::fs::read_to_string(dir.join(".janus-adapter.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Some(src) = meta["source"].as_str() {
        return src == source_label(&d.source);
    }
    let npm_layout = dir.join("node_modules").join(d.package).join("package.json").is_file() && !dir.join(".git").exists();
    matches!(d.source, Source::Npm(_)) == npm_layout
}

/// What is going on with one adapter right now: an install or update check in progress, or how
/// the last one ended. Kept in memory; the log file has the full output.
#[derive(Clone, Default)]
struct Job {
    state: &'static str,
    action: String,
    detail: String,
    started: i64,
    ended: i64,
}

static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);
/// The newest version each source offers, from the last check: (npm version or git commit, checked at).
static LATEST: LazyLock<Mutex<HashMap<String, (String, i64)>>> = LazyLock::new(Default::default);

impl App {
    fn adapters_dir(&self) -> PathBuf {
        self.cfg.data_dir.join("adapters")
    }

    fn adapter_log(&self, id: &str) -> PathBuf {
        self.cfg.data_dir.join("logs").join("adapters").join(format!("{id}.log"))
    }

    /// The version directory `current` points to, if the adapter is installed.
    fn adapter_current(&self, id: &str) -> Option<PathBuf> {
        let link = self.adapters_dir().join(id).join("current");
        link.canonicalize().ok().filter(|p| p.is_dir())
    }

    // @project-doc docs/architecture/adapters.md#running_sessions
    /// The script a session of this adapter runs, from the version that is current now.
    pub fn adapter_entry(&self, id: &str) -> Result<PathBuf> {
        let d = def(id)?;
        let dir = self.adapter_current(id).ok_or_else(|| {
            anyhow!("{} 的 ACP 适配器还没有安装：在「设置 → Agents」中运行配置检查会自动安装", d.name)
        })?;
        entry_in(d, &dir)
    }

    /// Every adapter with its installed versions, the newest known version and the current job.
    pub fn adapters(&self) -> Value {
        let jobs = JOBS.lock().unwrap().clone();
        let latest = LATEST.lock().unwrap().clone();
        let list: Vec<Value> = ADAPTERS
            .iter()
            .map(|d| {
                let current = self.adapter_current(d.id).and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
                let mut versions: Vec<Value> = std::fs::read_dir(self.adapters_dir().join(d.id))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().is_dir() && !e.path().is_symlink())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let meta: Value = std::fs::read_to_string(e.path().join(".janus-adapter.json")).ok().and_then(|t| serde_json::from_str(&t).ok())?;
                        Some(json!({ "version": name, "installedAt": meta["installedAt"], "ref": meta["ref"], "current": Some(&name) == current.as_ref() }))
                    })
                    .collect();
                versions.sort_by_key(|v| std::cmp::Reverse(v["installedAt"].as_i64().unwrap_or(0)));
                let job = jobs.get(d.id).map(|j| json!({ "state": j.state, "action": j.action, "detail": j.detail, "started": j.started, "ended": j.ended }));
                let (newest, checked) = latest.get(d.id).cloned().map(|(v, t)| (json!(v), json!(t))).unwrap_or((Value::Null, Value::Null));
                json!({
                    "id": d.id, "name": d.name, "package": d.package,
                    "source": match d.source { Source::Npm(p) => json!({ "kind": "npm", "package": p }), Source::Git(r) => json!({ "kind": "git", "repo": r }) },
                    "current": current, "versions": versions,
                    "latest": newest, "checkedAt": checked,
                    // A git version is `<package.json version>-<commit>`; the check only knows the commit.
                    // A version from an older source is always due for replacement.
                    "updateAvailable": self.adapter_current(d.id).is_some_and(|dir| !from_source(d, &dir)) || match (&current, newest.as_str(), &d.source) {
                        (Some(c), Some(n), Source::Git(_)) => !c.ends_with(&format!("-{n}")),
                        (Some(c), Some(n), Source::Npm(_)) => c != n,
                        _ => false,
                    },
                    "job": job, "log": tail(&self.adapter_log(d.id), 15),
                })
            })
            .collect();
        json!({ "adapters": list, "dir": self.adapters_dir() })
    }

    /// Start installing (the newest version, or `version` for npm sources) in the background.
    pub fn start_adapter_install(self: &Arc<Self>, id: &str, version: Option<String>) -> Result<()> {
        def(id)?;
        begin(id, "install")?;
        let app = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let r = app.install_adapter(&id, version.as_deref());
            finish(&id, r.map(|v| format!("已安装 {v}")));
        });
        Ok(())
    }

    /// Look up the newest version the source offers, in the background.
    pub fn start_adapter_check(self: &Arc<Self>, id: &str) -> Result<()> {
        def(id)?;
        begin(id, "check")?;
        let app = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let r = app.latest_adapter_version(&id);
            finish(&id, r.map(|v| format!("最新版本 {v}")));
        });
        Ok(())
    }

    /// Make an installed version current again (roll back, or forward).
    pub fn use_adapter_version(&self, id: &str, version: &str) -> Result<()> {
        def(id)?;
        if JOBS.lock().unwrap().get(id).is_some_and(|j| j.state == "running") {
            bail!("{id} 的适配器正在安装，等它结束后再切换");
        }
        let dir = self.adapters_dir().join(id);
        if version.contains('/') || version.starts_with('.') || !dir.join(version).join(".janus-adapter.json").is_file() {
            bail!("没有安装这个版本：{version}");
        }
        switch_current(&dir, version)?;
        self.adapter_note(id, &format!("切换到 {version}"));
        Ok(())
    }

    // @project-doc docs/architecture/adapters.md#on_demand
    /// Install the adapter now unless it is there already; for the connection check, which an
    /// agent needs to pass before it is used. A version from an older source is replaced; if that
    /// fails, the old version stays current and keeps working.
    pub fn ensure_adapter(&self, id: &str) -> Result<Option<String>> {
        let d = def(id)?;
        if let Some(dir) = self.adapter_current(id) {
            if from_source(d, &dir) {
                return Ok(None);
            }
            begin(id, "install")?;
            let r = self.install_adapter(id, None);
            finish(id, r.as_ref().map(|v| format!("已从新的来源安装 {v}")).map_err(|e| anyhow!("{e:#}")));
            return Ok(r.ok());
        }
        begin(id, "install")?;
        let r = self.install_adapter(id, None);
        finish(id, r.as_ref().map(|v| format!("已安装 {v}")).map_err(|e| anyhow!("{e:#}")));
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

    /// The newest version the source offers: npm's `latest`, or the commit at the head of the
    /// repository's default branch.
    fn latest_adapter_version(&self, id: &str) -> Result<String> {
        let d = def(id)?;
        let v = match d.source {
            Source::Npm(pkg) => run_text(Command::new("npm").args(["view", pkg, "version"]))?.trim().to_string(),
            Source::Git(repo) => {
                let head = run_text(Command::new("git").args(["ls-remote", repo, "HEAD"]))?;
                head.split_whitespace().next().context("git ls-remote 没有返回 HEAD")?.chars().take(7).collect()
            }
        };
        if v.is_empty() {
            bail!("没有查到最新版本");
        }
        LATEST.lock().unwrap().insert(id.to_string(), (v.clone(), now_ms()));
        Ok(v)
    }

    // @project-doc docs/architecture/adapters.md#install_steps
    /// Build a version in a staging directory, then make it current. Returns the version.
    fn install_adapter(&self, id: &str, version: Option<&str>) -> Result<String> {
        let d = def(id)?;
        let dir = self.adapters_dir().join(id);
        std::fs::create_dir_all(&dir)?;
        let log_path = self.adapter_log(id);
        std::fs::create_dir_all(log_path.parent().unwrap())?;
        let staging = dir.join(format!(".staging-{}", now_ms()));
        self.adapter_note(id, &format!("开始安装 {}{}", d.package, version.map(|v| format!("@{v}")).unwrap_or_default()));
        let built = (|| -> Result<(String, String)> {
            let node = &crate::config::settings().node;
            check_node(node, d)?;
            match d.source {
                Source::Npm(pkg) => {
                    std::fs::create_dir_all(&staging)?;
                    std::fs::write(staging.join("package.json"), r#"{ "name": "janus-adapter", "private": true }"#)?;
                    let spec = format!("{pkg}@{}", version.unwrap_or("latest"));
                    logged(Command::new("npm").args(["install", "--omit=dev", "--no-audit", "--no-fund", &spec]).current_dir(&staging), &log_path)?;
                    let installed: Value = serde_json::from_str(&std::fs::read_to_string(staging.join("node_modules").join(pkg).join("package.json"))?)?;
                    let v = installed["version"].as_str().context("package.json 没有 version")?.to_string();
                    Ok((v.clone(), v))
                }
                Source::Git(repo) => {
                    if version.is_some() {
                        bail!("{} 从 Git 仓库安装，只能装最新提交", d.name);
                    }
                    logged(Command::new("git").args(["clone", "--depth", "1", repo]).arg(&staging), &log_path)?;
                    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(staging.join("package.json"))?)?;
                    // A pinned package manager (pnpm@10.20.0) runs at exactly that version.
                    match pkg["packageManager"].as_str() {
                        Some(pm) if pm.starts_with("pnpm@") => {
                            logged(Command::new("npx").args(["--yes", pm, "install", "--frozen-lockfile"]).current_dir(&staging), &log_path)?;
                            if pkg["scripts"]["build"].is_string() {
                                logged(Command::new("npx").args(["--yes", pm, "run", "build"]).current_dir(&staging), &log_path)?;
                            }
                        }
                        // The build needs the dev dependencies (tsc, bundlers); running needs only
                        // the rest, so they are pruned once the build is done.
                        _ if pkg["scripts"]["build"].is_string() => {
                            logged(Command::new("npm").args(["install", "--no-audit", "--no-fund"]).current_dir(&staging), &log_path)?;
                            logged(Command::new("npm").args(["run", "build"]).current_dir(&staging), &log_path)?;
                            logged(Command::new("npm").args(["prune", "--omit=dev", "--no-audit", "--no-fund"]).current_dir(&staging), &log_path)?;
                        }
                        _ => {
                            logged(Command::new("npm").args(["install", "--omit=dev", "--no-audit", "--no-fund"]).current_dir(&staging), &log_path)?;
                        }
                    }
                    let sha = run_text(Command::new("git").args(["rev-parse", "--short=7", "HEAD"]).current_dir(&staging))?.trim().to_string();
                    let v = format!("{}-{sha}", pkg["version"].as_str().unwrap_or("0.0.0"));
                    Ok((v, sha))
                }
            }
        })();
        let (v, reference) = match built {
            Ok(x) => x,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging);
                self.adapter_note(id, &format!("安装失败：{err:#}"));
                return Err(err);
            }
        };
        let target = dir.join(&v);
        if target.join(".janus-adapter.json").is_file() && from_source(d, &target) {
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
                serde_json::to_vec_pretty(&json!({ "id": id, "version": v, "ref": reference, "source": source_label(&d.source), "installedAt": now_ms() }))?,
            )?;
            std::fs::rename(&staging, &target)?;
        }
        let previous = std::fs::read_link(dir.join("current")).ok().map(|p| p.to_string_lossy().to_string());
        switch_current(&dir, &v)?;
        prune(&dir, &v, previous.as_deref());
        self.adapter_note(id, &format!("已安装 {v} 并设为当前版本"));
        Ok(v)
    }
}

/// The adapter's script inside an installed (or staged) version directory, from its package.json
/// `bin`. Found by the directory's layout rather than the adapter's source, so a version installed
/// from an earlier source keeps running until it is replaced.
fn entry_in(d: &AdapterDef, dir: &Path) -> Result<PathBuf> {
    let npm = dir.join("node_modules").join(d.package);
    let pkg_dir = if npm.join("package.json").is_file() && !dir.join(".git").exists() { npm } else { dir.to_path_buf() };
    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(pkg_dir.join("package.json")).with_context(|| format!("{} 的适配器缺少 package.json", d.name))?)?;
    let rel = match &pkg["bin"] {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.get(d.package).or_else(|| m.values().next()).and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => pkg["main"].as_str().unwrap_or_default().to_string(),
    };
    let entry = pkg_dir.join(rel.trim_start_matches("./"));
    if rel.is_empty() || !entry.is_file() {
        bail!("{} 的适配器入口文件不存在：{}", d.name, entry.display());
    }
    Ok(entry)
}

// @project-doc docs/architecture/adapters.md#layout
/// Point `current` at `version`, atomically: a new link renamed over the old one.
fn switch_current(dir: &Path, version: &str) -> Result<()> {
    let tmp = dir.join(format!(".current-{}", now_ms()));
    std::os::unix::fs::symlink(version, &tmp)?;
    std::fs::rename(&tmp, dir.join("current"))?;
    Ok(())
}

/// Keep the current and the previous version; remove older ones and leftover staging dirs.
fn prune(dir: &Path, current: &str, previous: Option<&str>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "current" || name == current || Some(name.as_str()) == previous {
            continue;
        }
        let stale_staging = name.starts_with(".staging-") || name.starts_with(".current-");
        if stale_staging || e.path().join(".janus-adapter.json").is_file() {
            let _ = if e.path().is_symlink() { std::fs::remove_file(e.path()) } else { std::fs::remove_dir_all(e.path()) };
        }
    }
}

/// Node must satisfy the adapter's `engines.node`; it is checked before anything is downloaded.
fn check_node(node: &str, d: &AdapterDef) -> Result<()> {
    let out = run_text(Command::new(node).arg("--version")).with_context(|| format!("找不到 Node（{node}），安装 {} 的适配器需要它", d.name))?;
    let v: Vec<u64> = out.trim().trim_start_matches('v').split('.').filter_map(|x| x.parse().ok()).collect();
    if v.first().copied().unwrap_or(0) < 20 {
        bail!("Node 版本 {} 太旧，ACP 适配器需要 Node 20 以上（Pi 需要 22.19 以上）", out.trim());
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
    let line = format!("$ {:?} {}", cmd.get_program(), cmd.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "));
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(f, "{line}")?;
    let status = cmd.stdin(Stdio::null()).stdout(f.try_clone()?).stderr(f).status().with_context(|| format!("无法运行 {:?}", cmd.get_program()))?;
    if !status.success() {
        bail!("{line} 失败（exit {}），详见 {}", status.code().unwrap_or(-1), log.display());
    }
    Ok(())
}

fn begin(id: &str, action: &str) -> Result<()> {
    let mut jobs = JOBS.lock().unwrap();
    if jobs.get(id).is_some_and(|j| j.state == "running") {
        bail!("{id} 的适配器已经在{}中", if action == "install" { "安装" } else { "处理" });
    }
    jobs.insert(id.to_string(), Job { state: "running", action: action.into(), detail: String::new(), started: now_ms(), ended: 0 });
    Ok(())
}

fn finish(id: &str, r: Result<String>) {
    let mut jobs = JOBS.lock().unwrap();
    if let Some(j) = jobs.get_mut(id) {
        j.ended = now_ms();
        match r {
            Ok(detail) => (j.state, j.detail) = ("ok", detail),
            Err(err) => (j.state, j.detail) = ("error", format!("{err:#}")),
        }
    }
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
        prune(&dir, "1.2.0", Some("1.1.0"));
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, ["1.1.0", "1.2.0", "current"], "the previous version stays for rolling back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_comes_from_the_package_bin() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-entry-{}", std::process::id()));
        let pkg = dir.join("node_modules").join("acp-extension-claude");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(pkg.join("package.json"), r#"{ "bin": { "acp-extension-claude": "./dist/index.js" }, "main": "dist/lib.js" }"#).unwrap();
        std::fs::write(pkg.join("dist/index.js"), "").unwrap();
        assert_eq!(entry_in(def("claude").unwrap(), &dir).unwrap(), pkg.join("dist/index.js"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Claude used to come from npm and now builds from its repository: an npm version installed
    /// earlier still starts, and is recognised as due for replacement; a recorded source decides.
    #[test]
    fn versions_from_an_earlier_source_still_run_and_are_replaced() {
        let dir = std::env::temp_dir().join(format!("janus-adapter-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = def("claude").unwrap();
        let npm = dir.join("0.54.3");
        let pkg = npm.join("node_modules").join("acp-extension-claude");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(npm.join("package.json"), r#"{ "name": "janus-adapter", "private": true }"#).unwrap();
        std::fs::write(pkg.join("package.json"), r#"{ "bin": { "acp-extension-claude": "dist/index.js" } }"#).unwrap();
        std::fs::write(pkg.join("dist/index.js"), "").unwrap();
        std::fs::write(npm.join(".janus-adapter.json"), r#"{ "version": "0.54.3", "ref": "0.54.3" }"#).unwrap();
        assert_eq!(entry_in(d, &npm).unwrap(), pkg.join("dist/index.js"), "the old layout still starts");
        assert!(!from_source(d, &npm), "an npm version is not from the repository");

        let git = dir.join("0.79.0-abc1234");
        std::fs::create_dir_all(git.join("dist")).unwrap();
        std::fs::create_dir_all(git.join(".git")).unwrap();
        std::fs::write(git.join("package.json"), r#"{ "name": "acp-extension-claude", "bin": { "acp-extension-claude": "dist/index.js" } }"#).unwrap();
        std::fs::write(git.join("dist/index.js"), "").unwrap();
        std::fs::write(git.join(".janus-adapter.json"), format!(r#"{{ "source": "{}" }}"#, source_label(&d.source))).unwrap();
        assert_eq!(entry_in(d, &git).unwrap(), git.join("dist/index.js"));
        assert!(from_source(d, &git));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
