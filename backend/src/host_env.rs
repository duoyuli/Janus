//! The person's own environment, shared with agents on purpose. Every agent and MCP server works in
//! a HOME of its own, so it finds none of the person's sign-ins (GitHub CLI, Lark CLI, the macOS
//! keychain …) or tool settings (rustup, nvm …). The person picks, in the settings, which paths
//! of their home to share; each is linked into every sandbox's HOME at the same place, so a tool
//! there reads and refreshes the very same sign-in. Nothing is shared until picked, and an agent's
//! own settings never are.
// @project-doc docs/operations/20261031-security.md#host_env

use crate::agents::str_of;
use crate::layout::Sandbox;
use crate::orchestrator::{App, now_ms};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// One stored row per shared path, its id the path.
pub const KIND: &str = "host_link";

/// The macOS keychain folder: where the GitHub CLI, the Lark CLI and Git's credential helper keep
/// their sign-ins. A process finds the login keychain under `$HOME/Library/Keychains`.
pub const KEYCHAIN: &str = "Library/Keychains";

/// What a known tool is, for grouping on the settings page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// Sign-ins and accounts.
    Login,
    /// Toolchains and their package caches.
    Toolchain,
    /// The macOS keychain.
    System,
}

impl Group {
    fn as_str(self) -> &'static str {
        match self {
            Group::Login => "login",
            Group::Toolchain => "toolchain",
            Group::System => "system",
        }
    }
}

/// A tool Janus knows where it keeps its things. Only the ones found on this Mac are offered;
/// anything else in the home folder is offered by its path (`others`).
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub group: Group,
    pub paths: &'static [&'static str],
    /// Its sign-in is in the keychain: picking it offers the keychain too.
    pub keychain: bool,
}

const fn tool(
    id: &'static str,
    name: &'static str,
    group: Group,
    paths: &'static [&'static str],
    keychain: bool,
) -> Tool {
    Tool { id, name, group, paths, keychain }
}

pub const TOOLS: &[Tool] = &[
    tool("keychain", "macOS Keychain", Group::System, &[KEYCHAIN], false),
    tool("gh", "GitHub CLI", Group::Login, &[".config/gh"], true),
    tool("glab", "GitLab CLI", Group::Login, &[".config/glab-cli"], true),
    tool("lark", "Lark / Feishu CLI", Group::Login, &[".lark-cli"], true),
    tool("git", "Git", Group::Login, &[".gitconfig", ".config/git", ".git-credentials"], true),
    tool("netrc", "netrc", Group::Login, &[".netrc"], false),
    tool("npm", "npm", Group::Login, &[".npmrc"], false),
    tool("aws", "AWS CLI", Group::Login, &[".aws"], false),
    tool("gcloud", "Google Cloud CLI", Group::Login, &[".config/gcloud"], false),
    tool("azure", "Azure CLI", Group::Login, &[".azure"], false),
    tool("docker", "Docker", Group::Login, &[".docker"], false),
    tool("kube", "kubectl", Group::Login, &[".kube"], false),
    tool("huggingface", "Hugging Face", Group::Login, &[".cache/huggingface"], false),
    tool("rust", "Rust (rustup, cargo)", Group::Toolchain, &[".rustup", ".cargo"], false),
    tool("nvm", "nvm", Group::Toolchain, &[".nvm"], false),
    tool("pyenv", "pyenv", Group::Toolchain, &[".pyenv"], false),
    tool("mise", "mise", Group::Toolchain, &[".config/mise", ".local/share/mise"], false),
    tool("asdf", "asdf", Group::Toolchain, &[".asdf", ".tool-versions"], false),
    tool("uv", "uv", Group::Toolchain, &[".config/uv", ".local/share/uv"], false),
    tool("go", "Go", Group::Toolchain, &["go"], false),
    tool("bun", "Bun", Group::Toolchain, &[".bun"], false),
    tool("deno", "Deno", Group::Toolchain, &[".deno"], false),
    tool("pnpm", "pnpm", Group::Toolchain, &["Library/pnpm"], false),
    tool("gradle", "Gradle", Group::Toolchain, &[".gradle"], false),
    tool("maven", "Maven", Group::Toolchain, &[".m2"], false),
];

/// Agents' own settings, instructions, skills and MCP, and Janus's: never shared, so a role gets
/// only what Janus gives it at launch.
const AGENT_OWN: &[&str] = &[
    ".agents",
    ".claude",
    ".claude.json",
    ".codeium",
    ".codex",
    ".config/claude",
    ".config/codex",
    ".config/github-copilot",
    ".config/opencode",
    ".copilot",
    ".cursor",
    ".gemini",
    ".janus",
    ".kiro",
    ".opencode",
    ".qwen",
    ".windsurf",
];

/// Folders too wide to share whole: what is in them is offered instead.
const WIDE: &[&str] = &[
    ".cache",
    ".config",
    ".local",
    ".local/share",
    ".local/state",
    "Library",
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
];

/// Found in a home folder but nothing a tool runs on: system files and shell histories.
fn noise(name: &str) -> bool {
    matches!(
        name,
        ".DS_Store"
            | ".Trash"
            | ".CFUserTextEncoding"
            | ".localized"
            | ".bash_history"
            | ".bash_sessions"
            | ".zsh_history"
            | ".zsh_sessions"
            | ".lesshst"
            | ".viminfo"
            | ".python_history"
            | ".node_repl_history"
            | ".sqlite_history"
            | ".psql_history"
            | ".wget-hsts"
    ) || name.starts_with(".zcompdump")
}

/// A path in the home folder, as stored: relative, with `/` between its parts. `~/` in front is
/// taken off; an absolute path or one that climbs out (`..`) is refused.
pub fn normalize(path: &str) -> Result<String> {
    let p = path.trim();
    let p = p.strip_prefix("~/").unwrap_or(p).trim_end_matches('/');
    if p.is_empty() || p == "~" {
        bail!("请填写家目录下的一个路径，例如 .config/gh");
    }
    let mut parts = vec![];
    for c in Path::new(p).components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => bail!("{path} 不是家目录下的路径：只能填 ~/ 之后的部分，不能用 .."),
        }
    }
    if parts.is_empty() {
        bail!("请填写家目录下的一个路径，例如 .config/gh");
    }
    Ok(parts.join("/"))
}

fn within(path: &str, folder: &str) -> bool {
    path == folder || path.starts_with(&format!("{folder}/"))
}

/// Why a (normalized) path may not be shared, if it may not.
pub fn refused(path: &str) -> Option<String> {
    if WIDE.contains(&path) {
        return Some(format!("~/{path} 范围太大，不能整个共享：请选它里面具体的目录"));
    }
    if AGENT_OWN.iter().any(|a| within(path, a) || within(a, path)) {
        return Some(format!(
            "~/{path} 是 Agent 自己的配置（或包含它）：数字员工的指令、Skill 和 MCP 只由 Janus 在启动时给，不能共享"
        ));
    }
    None
}

/// Whether something is at `path` (a link counts, even a broken one).
fn present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// What else the home folder holds that a tool may run on: its hidden entries, and what is in
/// `.config` and `.local/share`, less the known tools' paths, agents' own settings and noise.
fn others(home: &Path) -> Vec<String> {
    let known: BTreeSet<&str> = TOOLS.iter().flat_map(|t| t.paths.iter().copied()).collect();
    let mut out = BTreeSet::new();
    for (dir, hidden_only) in [("", true), (".config", false), (".local/share", false)] {
        let Ok(entries) = std::fs::read_dir(home.join(dir)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if (hidden_only && !name.starts_with('.')) || noise(&name) {
                continue;
            }
            let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if !known.contains(path.as_str()) && refused(&path).is_none() {
                out.insert(path);
            }
        }
    }
    out.into_iter().collect()
}

/// What the settings page shows: the known tools found in `home` (with the paths of theirs that
/// are there), everything else found there, and what is shared, with what of it is gone.
pub fn survey(home: &Path, shared: &[String]) -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .filter_map(|t| {
            let found: Vec<&str> = t.paths.iter().copied().filter(|p| present(&home.join(p))).collect();
            (!found.is_empty()).then(|| {
                json!({ "id": t.id, "name": t.name, "group": t.group.as_str(), "paths": found, "keychain": t.keychain })
            })
        })
        .collect();
    let missing: Vec<&String> = shared.iter().filter(|p| !present(&home.join(p))).collect();
    json!({ "tools": tools, "others": others(home), "shared": shared, "missing": missing })
}

/// The name a sandbox's own file or folder is kept under while the person's is shared there.
fn aside(at: &Path) -> PathBuf {
    let mut name = at.file_name().unwrap_or_default().to_os_string();
    name.push(".janus-own");
    at.with_file_name(name)
}

/// Link `rel` of `home` into `sandbox` at the same place. What the sandbox had there itself is
/// kept aside (`aside`) and comes back when the path is no longer shared. Nothing at `rel` in
/// `home` (any more): nothing to link.
fn link(sandbox: &Path, home: &Path, rel: &str) -> Result<()> {
    let target = home.join(rel);
    if !present(&target) {
        return Ok(());
    }
    let at = sandbox.join(rel);
    match std::fs::symlink_metadata(&at) {
        Ok(m) if m.file_type().is_symlink() => {
            if std::fs::read_link(&at)? == target {
                return Ok(());
            }
            std::fs::remove_file(&at)?;
        }
        Ok(_) => {
            let kept = aside(&at);
            if present(&kept) {
                bail!("{} 已有自己的 {rel}，也已经存过一份，没有共享", sandbox.display());
            }
            std::fs::rename(&at, &kept)?;
        }
        Err(_) => {}
    }
    if let Some(parent) = at.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(&target, &at)?;
    Ok(())
}

/// Take back the link `link` made: only a link to the person's own path is removed, and what the
/// sandbox kept aside comes back.
fn unlink(sandbox: &Path, home: &Path, rel: &str) -> Result<()> {
    let at = sandbox.join(rel);
    let ours = std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::read_link(&at).is_ok_and(|t| t == home.join(rel));
    if ours {
        std::fs::remove_file(&at)?;
        let kept = aside(&at);
        if present(&kept) {
            std::fs::rename(&kept, &at)?;
        }
    }
    Ok(())
}

impl App {
    /// The paths of the person's home shared with every agent and MCP server.
    pub fn host_links(&self) -> Result<Vec<String>> {
        Ok(self.db.list_config(KIND)?.iter().map(|v| str_of(v, "path").to_string()).collect())
    }

    /// Link what is shared into a sandbox about to be used. A path that cannot be linked there is
    /// written to the log; the process starts without it.
    pub fn link_host(&self, sandbox: &Sandbox) -> Result<()> {
        for rel in self.host_links()? {
            if let Err(e) = link(&sandbox.home, &self.cfg.host_home, &rel) {
                crate::config::log(format!("没能把 ~/{rel} 共享进 {}：{e:#}", sandbox.home.display()));
            }
        }
        Ok(())
    }

    /// The settings page's view of the person's environment (`survey`).
    pub fn host_env(&self) -> Result<Value> {
        Ok(survey(&self.cfg.host_home, &self.host_links()?))
    }

    /// Share exactly `paths` from now on: each must be in the home folder, not refused, and there;
    /// one inside another is refused. Every sandbox there is changes at once, so sessions already
    /// running see it too.
    // @project-doc docs/operations/20261031-security.md#host_env
    pub fn save_host_env(&self, body: &Value) -> Result<Value> {
        let mut wanted = BTreeSet::new();
        for p in body["paths"].as_array().into_iter().flatten() {
            let p = normalize(p.as_str().unwrap_or(""))?;
            if let Some(why) = refused(&p) {
                bail!(why);
            }
            if !present(&self.cfg.host_home.join(&p)) {
                bail!("本机没有 ~/{p}");
            }
            wanted.insert(p);
        }
        for a in &wanted {
            if let Some(b) = wanted.iter().find(|b| *b != a && within(b, a)) {
                bail!("~/{b} 在 ~/{a} 里面，共享 ~/{a} 就够了");
            }
        }
        let before: BTreeSet<String> = self.host_links()?.into_iter().collect();
        for gone in before.difference(&wanted) {
            self.db.delete_config(KIND, gone)?;
        }
        for new in wanted.difference(&before) {
            self.db.put_config(KIND, new, &json!({ "path": new }), now_ms())?;
        }
        for sandbox in self.cfg.layout.sandbox_homes() {
            for gone in before.difference(&wanted) {
                if let Err(e) = unlink(&sandbox, &self.cfg.host_home, gone) {
                    crate::config::log(format!("没能从 {} 撤回 ~/{gone}：{e:#}", sandbox.display()));
                }
            }
            for rel in &wanted {
                if let Err(e) = link(&sandbox, &self.cfg.host_home, rel) {
                    crate::config::log(format!("没能把 ~/{rel} 共享进 {}：{e:#}", sandbox.display()));
                }
            }
        }
        self.host_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_kept_inside_home_and_away_from_agents_own_settings() {
        assert_eq!(normalize("~/.config/gh/").unwrap(), ".config/gh");
        assert_eq!(normalize("./.lark-cli").unwrap(), ".lark-cli");
        for bad in ["", "~", "/etc/passwd", "../x", ".config/../.codex"] {
            assert!(normalize(bad).is_err(), "{bad}");
        }
        for own in [".codex", ".claude/skills", ".agents", ".config/claude", ".janus"] {
            assert!(refused(own).is_some_and(|w| w.contains("Agent 自己的配置")), "{own}");
        }
        for wide in [".config", "Library", ".local/share"] {
            assert!(refused(wide).is_some_and(|w| w.contains("范围太大")), "{wide}");
        }
        assert!(refused(KEYCHAIN).is_none() && refused(".config/gh").is_none());
    }

    #[test]
    fn the_survey_offers_only_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        for p in [".config/gh", ".config/zed", ".codex", ".lark-cli", "Library/Keychains", ".local/share/mise"] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        for f in [".zshrc", ".zsh_history", ".DS_Store", ".claude.json", "notes.txt"] {
            std::fs::write(home.join(f), "").unwrap();
        }
        let s = survey(home, &[".config/gh".into(), ".aws".into()]);
        let ids: Vec<&str> = s["tools"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["keychain", "gh", "lark", "mise"]);
        assert_eq!(s["others"], json!([".config/zed", ".zshrc"]), "no noise, no agents' own, no known paths");
        assert_eq!(s["missing"], json!([".aws"]));
    }

    #[test]
    fn a_link_reaches_the_persons_own_and_gives_back_what_the_sandbox_had() {
        let dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (dir.path().join("home"), dir.path().join("sandbox"));
        std::fs::create_dir_all(home.join(".config/gh")).unwrap();
        std::fs::write(home.join(".config/gh/hosts.yml"), "github.com").unwrap();
        // The sandbox once made its own, signed out.
        std::fs::create_dir_all(sandbox.join(".config/gh")).unwrap();
        std::fs::write(sandbox.join(".config/gh/config.yml"), "own").unwrap();
        link(&sandbox, &home, ".config/gh").unwrap();
        link(&sandbox, &home, ".config/gh").unwrap();
        assert_eq!(std::fs::read_to_string(sandbox.join(".config/gh/hosts.yml")).unwrap(), "github.com");
        assert!(sandbox.join(".config/gh.janus-own/config.yml").exists());
        // Nothing on this Mac: nothing linked.
        link(&sandbox, &home, ".aws").unwrap();
        assert!(!present(&sandbox.join(".aws")));
        unlink(&sandbox, &home, ".config/gh").unwrap();
        assert_eq!(std::fs::read_to_string(sandbox.join(".config/gh/config.yml")).unwrap(), "own");
        assert!(home.join(".config/gh/hosts.yml").exists(), "the person's own stays");
        // A folder the sandbox has of its own is never taken for a link.
        unlink(&sandbox, &home, ".config/gh").unwrap();
        assert!(sandbox.join(".config/gh/config.yml").exists());
    }

    /// Saving what is shared changes every sandbox at once and launches link it; agents' own
    /// settings, paths that are not there and one path inside another are refused.
    #[test]
    fn saving_changes_every_sandbox_and_refuses_what_may_not_be_shared() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::orchestrator::test_app(dir.path());
        let home = &app.cfg.host_home;
        for p in [".config/gh", ".codex", "Library/Keychains"] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        // An agent that already ran, and an MCP server's sandbox.
        let agent = app.cfg.layout.agent_sandbox("a1");
        std::fs::create_dir_all(&agent.home).unwrap();
        let server = app.cfg.layout.server_sandbox("s1");
        std::fs::create_dir_all(&server.home).unwrap();

        let v = app.save_host_env(&json!({ "paths": ["~/.config/gh", KEYCHAIN] })).unwrap();
        assert_eq!(v["shared"], json!([".config/gh", KEYCHAIN]));
        for h in [&agent.home, &server.home] {
            assert_eq!(std::fs::read_link(h.join(".config/gh")).unwrap(), home.join(".config/gh"));
            assert!(h.join(KEYCHAIN).is_dir());
        }
        // A new agent's sandbox gets them when it starts.
        let new = app.cfg.layout.agent_sandbox("a2");
        app.link_host(&new).unwrap();
        assert!(new.home.join(".config/gh").is_dir());

        for (bad, why) in [
            (json!([".codex"]), "Agent 自己的配置"),
            (json!([".aws"]), "本机没有"),
            (json!(["Library", KEYCHAIN]), "范围太大"),
            (json!([".config/gh", ".config/gh/hosts"]), "本机没有"),
            (json!(["/etc"]), "不是家目录下的路径"),
        ] {
            let err = app.save_host_env(&json!({ "paths": bad })).unwrap_err().to_string();
            assert!(err.contains(why), "{bad}: {err}");
        }
        std::fs::write(home.join(".config/gh/hosts.yml"), "").unwrap();
        let err =
            app.save_host_env(&json!({ "paths": [".config/gh", ".config/gh/hosts.yml"] })).unwrap_err().to_string();
        assert!(err.contains("在 ~/.config/gh 里面"), "{err}");
        assert_eq!(app.host_links().unwrap(), [".config/gh", KEYCHAIN], "a refused save changes nothing");

        app.save_host_env(&json!({ "paths": [KEYCHAIN] })).unwrap();
        for h in [&agent.home, &server.home, &new.home] {
            assert!(!present(&h.join(".config/gh")), "taken back from {}", h.display());
        }
        assert!(home.join(".config/gh/hosts.yml").exists());
    }
}
