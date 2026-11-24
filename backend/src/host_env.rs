//! The person's own environment, in every agent's and MCP server's HOME. Each works in a HOME of
//! its own (`Layout::agent_sandbox`, `Layout::server_sandbox`) that mirrors the person's: every
//! entry of their home folder is linked there at the same place, so sign-ins (the macOS keychain,
//! GitHub CLI, Lark CLI …), toolchains and settings work as in their terminal, and what a tool
//! refreshes is written back to theirs. Only what is hidden stays out: agents' own settings
//! (instructions, skills and MCP come from Janus alone) and what the person thinks too risky, in
//! one list they edit in the settings; Janus home is always hidden.
// @project-doc docs/operations/20261031-security.md#host_env

use crate::layout::Sandbox;
use crate::orchestrator::App;
use crate::util::now_ms;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// The stored list of hidden paths: one row (`KIND`, `ID`) holding `paths`. Without it the
/// defaults apply, so new defaults reach everyone who never changed the list.
pub const KIND: &str = "host_env";
const ID: &str = "hidden";

/// Why a path is hidden by default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// An agent's own settings: instructions, skills, MCP and sign-in.
    Agent,
    /// Credentials or personal data an agent at work has no business with.
    Sensitive,
}

impl Why {
    fn as_str(self) -> &'static str {
        match self {
            Why::Agent => "agent",
            Why::Sensitive => "sensitive",
        }
    }
}

/// The hidden paths a list starts with.
pub const DEFAULT_HIDDEN: &[(&str, Why)] = &[
    (".agents", Why::Agent),
    (".claude", Why::Agent),
    (".claude.json", Why::Agent),
    (".codeium", Why::Agent),
    (".codex", Why::Agent),
    (".config/claude", Why::Agent),
    (".config/codex", Why::Agent),
    (".config/github-copilot", Why::Agent),
    (".config/opencode", Why::Agent),
    (".copilot", Why::Agent),
    (".cursor", Why::Agent),
    (".gemini", Why::Agent),
    (".kiro", Why::Agent),
    (".opencode", Why::Agent),
    (".pi", Why::Agent),
    (".qwen", Why::Agent),
    (".windsurf", Why::Agent),
    ("Library/Application Support/Claude", Why::Agent),
    ("Library/Application Support/Cursor", Why::Agent),
    (".aws", Why::Sensitive),
    (".azure", Why::Sensitive),
    (".config/gcloud", Why::Sensitive),
    (".gnupg", Why::Sensitive),
    (".kube", Why::Sensitive),
    (".password-store", Why::Sensitive),
    ("Library/Application Support/1Password", Why::Sensitive),
    ("Library/Application Support/Google/Chrome", Why::Sensitive),
    ("Library/Application Support/Firefox", Why::Sensitive),
    ("Library/Cookies", Why::Sensitive),
    ("Library/Mail", Why::Sensitive),
    ("Library/Messages", Why::Sensitive),
];

/// Folders whose entries are mirrored one by one rather than linked whole: a sandbox's XDG
/// directories live in them, and they hold other agents' and tools' settings side by side.
const SPLIT: &[&str] =
    &[".cache", ".config", ".local", ".local/share", ".local/state", "Library", "Library/Application Support"];

/// What is never linked: the Trash, which macOS keeps from other processes anyway.
const SKIP: &[&str] = &[".Trash"];

/// A path in the home folder, as stored: relative, with `/` between its parts. `~/` in front is
/// taken off; an absolute path or one that climbs out (`..`) is refused.
pub fn normalize(path: &str) -> Result<String> {
    let p = path.trim();
    let p = p.strip_prefix("~/").unwrap_or(p).trim_end_matches('/');
    if p.is_empty() || p == "~" {
        bail!("请填写用户目录下的一个路径，例如 .aws");
    }
    let mut parts = vec![];
    for c in Path::new(p).components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => bail!("{path} 不是用户目录下的路径：只能填 ~/ 之后的部分，不能用 .."),
        }
    }
    if parts.is_empty() {
        bail!("请填写用户目录下的一个路径，例如 .aws");
    }
    Ok(parts.join("/"))
}

fn within(path: &str, folder: &str) -> bool {
    path == folder || path.starts_with(&format!("{folder}/"))
}

/// Whether something is at `path` (a link counts, even a broken one).
fn present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// What stays out of a mirror: the hidden paths and what is in them.
struct Rules {
    hidden: Vec<String>,
}

impl Rules {
    fn hidden(&self, rel: &str) -> bool {
        SKIP.contains(&rel) || self.hidden.iter().any(|h| within(rel, h))
    }

    /// A folder mirrored entry by entry: a wide one, or one with something hidden inside.
    fn split(&self, rel: &str) -> bool {
        SPLIT.contains(&rel) || self.hidden.iter().any(|h| h != rel && within(h, rel))
    }
}

/// The name a sandbox's own file or folder is kept under while the person's is linked there.
fn aside(at: &Path) -> PathBuf {
    let mut name = at.file_name().unwrap_or_default().to_os_string();
    name.push(".janus-own");
    at.with_file_name(name)
}

fn ours(at: &Path, target: &Path) -> bool {
    std::fs::symlink_metadata(at).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::read_link(at).is_ok_and(|t| t == target)
}

/// Link `rel` of `home` into `sandbox` at the same place. What the sandbox had there itself is
/// kept aside (`aside`) and comes back when the path is hidden.
fn link(sandbox: &Path, home: &Path, rel: &str) -> Result<()> {
    let (at, target) = (sandbox.join(rel), home.join(rel));
    match std::fs::symlink_metadata(&at) {
        Ok(m) if m.file_type().is_symlink() => {
            if std::fs::read_link(&at)? == target {
                return Ok(());
            }
            std::fs::remove_file(&at)?;
        }
        // A folder mirrored entry by entry before (something in it was hidden) holds only links
        // Janus made: it goes, the whole folder is linked now.
        Ok(m) if m.is_dir() && only_ours(&at, &target) => {
            for e in std::fs::read_dir(&at)?.flatten() {
                std::fs::remove_file(e.path())?;
            }
            std::fs::remove_dir(&at)?;
        }
        Ok(_) => {
            let kept = aside(&at);
            if present(&kept) {
                bail!("{} 已有自己的 {rel}，也已经存过一份，没有链接", sandbox.display());
            }
            std::fs::rename(&at, &kept)?;
        }
        Err(_) => {}
    }
    std::os::unix::fs::symlink(&target, &at)?;
    Ok(())
}

/// Whether folder `at` holds nothing but links Janus made to what is in `target`.
fn only_ours(at: &Path, target: &Path) -> bool {
    std::fs::read_dir(at).is_ok_and(|entries| entries.flatten().all(|e| ours(&e.path(), &target.join(e.file_name()))))
}

/// Take back the link `link` made, and put back what the sandbox kept aside. Anything else at
/// `rel` (the sandbox's own) is left alone; the person's own files are never touched.
fn unlink(sandbox: &Path, home: &Path, rel: &str) -> Result<()> {
    let at = sandbox.join(rel);
    if ours(&at, &home.join(rel)) {
        std::fs::remove_file(&at)?;
        let kept = aside(&at);
        if present(&kept) {
            std::fs::rename(&kept, &at)?;
        }
    }
    Ok(())
}

/// A real folder at `rel` in the sandbox, to mirror into: a link Janus made there goes, a file the
/// sandbox had is kept aside.
fn real_dir(sandbox: &Path, home: &Path, rel: &str) -> Result<()> {
    let at = sandbox.join(rel);
    match std::fs::symlink_metadata(&at) {
        Ok(m) if m.is_dir() => return Ok(()),
        Ok(m) if m.file_type().is_symlink() && ours(&at, &home.join(rel)) => std::fs::remove_file(&at)?,
        Ok(_) => std::fs::rename(&at, aside(&at))?,
        Err(_) => {}
    }
    std::fs::create_dir_all(&at)?;
    Ok(())
}

/// Make `sandbox` mirror `home` but what `rules` hide, in folder `dir`: each entry is linked, a
/// split folder is mirrored in turn, and links to what is now hidden or gone are taken back.
/// Problems with single entries are collected, not fatal: the rest is still mirrored.
fn mirror_dir(sandbox: &Path, home: &Path, dir: &str, rules: &Rules, problems: &mut Vec<String>) {
    let join = |name: &str| if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };
    let mut seen = BTreeSet::new();
    for e in std::fs::read_dir(home.join(dir)).into_iter().flatten().flatten() {
        let rel = join(&e.file_name().to_string_lossy());
        seen.insert(rel.clone());
        let done = if rules.hidden(&rel) {
            unlink(sandbox, home, &rel)
        } else if rules.split(&rel) && e.file_type().is_ok_and(|t| t.is_dir()) {
            real_dir(sandbox, home, &rel).map(|()| mirror_dir(sandbox, home, &rel, rules, problems))
        } else {
            link(sandbox, home, &rel)
        };
        if let Err(err) = done {
            problems.push(format!("~/{rel}：{err:#}"));
        }
    }
    for e in std::fs::read_dir(sandbox.join(dir)).into_iter().flatten().flatten() {
        let rel = join(&e.file_name().to_string_lossy());
        if !seen.contains(&rel)
            && let Err(err) = unlink(sandbox, home, &rel)
        {
            problems.push(format!("~/{rel}：{err:#}"));
        }
    }
}

/// Make the sandbox HOME `sandbox` mirror `home`, but what `hidden` names.
fn mirror(sandbox: &Path, home: &Path, hidden: Vec<String>) -> Vec<String> {
    let mut problems = vec![];
    if let Err(err) = std::fs::create_dir_all(sandbox) {
        problems.push(format!("{}：{err:#}", sandbox.display()));
        return problems;
    }
    mirror_dir(sandbox, home, "", &Rules { hidden }, &mut problems);
    problems
}

/// A command's exit code and output, cut to a readable length.
async fn run(command: &str, env: Option<&[(String, String)]>, cwd: &Path) -> Value {
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args(["-c", command]).current_dir(cwd).stdin(std::process::Stdio::null()).kill_on_drop(true);
    if let Some(env) = env {
        cmd.env_clear().envs(env.iter().cloned());
    }
    match tokio::time::timeout(TRY_FOR, cmd.output()).await {
        Err(_) => json!({ "code": null, "output": format!("{} 秒没有结束，已停止", TRY_FOR.as_secs()) }),
        Ok(Err(err)) => json!({ "code": null, "output": format!("运行失败：{err}") }),
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            let text: String = text.chars().take(4000).collect();
            json!({ "code": out.status.code(), "output": text })
        }
    }
}

/// How long a command tried from the settings may run.
const TRY_FOR: Duration = Duration::from_secs(20);

impl App {
    /// The paths hidden from every sandbox: the person's list (the defaults until they change it),
    /// and Janus home when it is in the home folder.
    pub fn host_hidden(&self) -> Result<Vec<String>> {
        let mut hidden: Vec<String> = match self.db.get_config(KIND, ID)? {
            Some(v) => {
                v["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect()
            }
            None => DEFAULT_HIDDEN.iter().map(|(p, _)| p.to_string()).collect(),
        };
        hidden.extend(self.janus_home_in_host());
        Ok(hidden)
    }

    /// Janus home, relative to the home folder when it is in it: never mirrored, or a sandbox
    /// (which lives in it) would hold itself.
    fn janus_home_in_host(&self) -> Option<String> {
        let home = self.cfg.layout.home();
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let host = self.cfg.host_home.canonicalize().unwrap_or_else(|_| self.cfg.host_home.clone());
        home.strip_prefix(&host).ok().map(|r| r.to_string_lossy().to_string()).filter(|r| !r.is_empty())
    }

    /// A path as the settings give it: under the home folder, or a full path in it (what the
    /// system's open panel picks), which is made relative to it.
    fn in_host_home(&self, path: &str) -> Result<String> {
        if !path.starts_with('/') {
            return Ok(path.to_string());
        }
        let host = &self.cfg.host_home;
        let real = host.canonicalize().unwrap_or_else(|_| host.clone());
        let p = Path::new(path);
        match p.strip_prefix(host).or_else(|_| p.strip_prefix(&real)) {
            Ok(rel) if !rel.as_os_str().is_empty() => Ok(rel.to_string_lossy().to_string()),
            _ => bail!("{path} 不在用户目录（{}）里，挡不了", host.display()),
        }
    }

    /// Mirror the person's home into a sandbox about to be used. Entries that cannot be linked are
    /// written to the log; the process starts without them.
    pub fn link_host(&self, sandbox: &Sandbox) -> Result<()> {
        let hidden = self.host_hidden()?;
        let _one = self.host_mirror.lock().unwrap_or_else(|p| p.into_inner());
        for p in mirror(&sandbox.home, &self.cfg.host_home, hidden) {
            crate::config::log(format!("没能把 {p} 链接进 {}", sandbox.home.display()));
        }
        Ok(())
    }

    /// The settings page's view: the hidden list, each path with why it is hidden by default
    /// (`agent`, `sensitive`, or `custom` for one the person added) and whether it is on this Mac;
    /// Janus home, hidden always; whether the list is still the defaults.
    pub fn host_env(&self) -> Result<Value> {
        let stored = self.db.get_config(KIND, ID)?;
        let defaults: Vec<(String, Why)> = DEFAULT_HIDDEN.iter().map(|(p, w)| (p.to_string(), *w)).collect();
        let listed: Vec<String> = match &stored {
            Some(v) => {
                v["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect()
            }
            None => defaults.iter().map(|(p, _)| p.clone()).collect(),
        };
        let hidden: Vec<Value> = listed
            .iter()
            .map(|p| {
                let why = defaults.iter().find(|(d, _)| d == p).map_or("custom", |(_, w)| w.as_str());
                json!({ "path": p, "why": why, "here": present(&self.cfg.host_home.join(p)) })
            })
            .collect();
        Ok(json!({ "hidden": hidden, "janusHome": self.janus_home_in_host(), "defaults": stored.is_none() }))
    }

    /// Hide exactly `paths` from now on (`null` goes back to the defaults): each a path in the home
    /// folder, none inside another. Every sandbox there is changes at once, so sessions already
    /// running see it too.
    // @project-doc docs/operations/20261031-security.md#host_env
    pub fn save_host_env(&self, body: &Value) -> Result<Value> {
        if body["paths"].is_null() {
            self.db.delete_config(KIND, ID)?;
        } else {
            let mut wanted: Vec<String> = vec![];
            for p in body["paths"].as_array().into_iter().flatten() {
                let p = normalize(&self.in_host_home(p.as_str().unwrap_or(""))?)?;
                if self.janus_home_in_host().is_some_and(|j| within(&p, &j) || within(&j, &p)) {
                    bail!("~/{p} 是 Janus 的数据目录（或包含它），它一直挡着，不用再加");
                }
                if !wanted.contains(&p) {
                    wanted.push(p);
                }
            }
            for a in &wanted {
                if let Some(b) = wanted.iter().find(|b| *b != a && within(b, a)) {
                    bail!("~/{b} 在 ~/{a} 里面，挡住 ~/{a} 就够了");
                }
            }
            self.db.put_config(KIND, ID, &json!({ "paths": wanted }), now_ms())?;
        }
        for home in self.cfg.layout.sandbox_homes() {
            let sandbox = Sandbox { home: home.clone(), tmp: PathBuf::new() };
            self.link_host(&sandbox)?;
        }
        self.host_env()
    }

    /// Run `command` (`/bin/sh -c`) twice: as this Mac's user (Janus's environment, in the home
    /// folder) and as an agent would (a sandbox of its own, mirrored like an agent's), and say how
    /// each went, so the person sees whether a tool works for agents.
    pub async fn try_host_command(&self, command: &str) -> Result<Value> {
        if command.trim().is_empty() {
            bail!("请填写要试的命令，例如 gh auth status");
        }
        let probe = self.cfg.layout.host_probe();
        let env = crate::agents::sandbox_vars(&probe)?;
        self.link_host(&probe)?;
        let mut mine: Vec<(String, String)> = std::env::vars().filter(|(k, _)| k != "HOME").collect();
        mine.push(("HOME".into(), self.cfg.host_home.to_string_lossy().to_string()));
        let (host, agent) =
            tokio::join!(run(command, Some(&mine), &self.cfg.host_home), run(command, Some(&env), &probe.home));
        Ok(json!({ "host": host, "agent": agent }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(hidden: &[&str]) -> Rules {
        Rules { hidden: hidden.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn paths_stay_inside_home() {
        assert_eq!(normalize("~/.aws/").unwrap(), ".aws");
        assert_eq!(normalize("./.config/gh").unwrap(), ".config/gh");
        for bad in ["", "~", "/etc/passwd", "../x", ".config/../.codex"] {
            assert!(normalize(bad).is_err(), "{bad}");
        }
        let r = rules(&[".codex", ".config/claude", "Library/Application Support/Claude"]);
        assert!(r.hidden(".codex") && r.hidden(".codex/auth.json") && r.hidden(".Trash"));
        assert!(!r.hidden(".codexx") && !r.hidden(".config"));
        assert!(r.split(".config") && r.split("Library") && r.split("Library/Application Support"));
        assert!(!r.split(".codex") && !r.split(".lark-cli"));
        assert!(
            rules(&[".ssh/id_rsa"]).split(".ssh"),
            "a folder with something hidden inside is mirrored entry by entry"
        );
    }

    /// Everything of the home folder reaches the sandbox but what is hidden: wide folders entry by
    /// entry, the rest linked whole; what the sandbox had itself is kept aside, and comes back when
    /// the person's is hidden; links to what is gone or hidden are taken back.
    #[test]
    fn a_sandbox_mirrors_home_but_what_is_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (dir.path().join("home"), dir.path().join("sandbox"));
        for d in [
            ".config/gh",
            ".config/claude",
            ".lark-cli",
            "Library/Keychains",
            "Library/Application Support/lark-cli",
            ".codex",
        ] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::write(home.join(".config/gh/hosts.yml"), "github.com").unwrap();
        std::fs::write(home.join(".gitconfig"), "[user]").unwrap();
        // The sandbox made its own before: a signed-out profile, and its XDG folder.
        std::fs::create_dir_all(sandbox.join(".lark-cli")).unwrap();
        std::fs::write(sandbox.join(".lark-cli/config.json"), "own").unwrap();
        std::fs::create_dir_all(sandbox.join(".config")).unwrap();

        let hidden = vec![".codex".to_string(), ".config/claude".to_string()];
        assert!(mirror(&sandbox, &home, hidden.clone()).is_empty());
        assert_eq!(std::fs::read_to_string(sandbox.join(".config/gh/hosts.yml")).unwrap(), "github.com");
        assert!(
            std::fs::symlink_metadata(sandbox.join(".config")).unwrap().is_dir(),
            "a wide folder is mirrored entry by entry"
        );
        assert!(ours(&sandbox.join(".gitconfig"), &home.join(".gitconfig")));
        assert!(ours(&sandbox.join(KEYCHAIN), &home.join(KEYCHAIN)));
        assert!(ours(
            &sandbox.join("Library/Application Support/lark-cli"),
            &home.join("Library/Application Support/lark-cli")
        ));
        assert!(!present(&sandbox.join(".codex")) && !present(&sandbox.join(".config/claude")), "agents' own stay out");
        assert_eq!(std::fs::read_to_string(sandbox.join(".lark-cli.janus-own/config.json")).unwrap(), "own");
        assert!(mirror(&sandbox, &home, hidden).is_empty(), "mirroring again changes nothing");

        // Hidden later: the link goes and the sandbox's own comes back; gone from home: link goes.
        std::fs::remove_file(home.join(".gitconfig")).unwrap();
        assert!(mirror(&sandbox, &home, vec![".lark-cli".into(), ".config/gh/hosts.yml".into()]).is_empty());
        assert_eq!(std::fs::read_to_string(sandbox.join(".lark-cli/config.json")).unwrap(), "own");
        assert!(!present(&sandbox.join(".gitconfig")));
        assert!(
            std::fs::symlink_metadata(sandbox.join(".config/gh")).unwrap().is_dir(),
            "split to hide one file inside"
        );
        assert!(!present(&sandbox.join(".config/gh/hosts.yml")));
        assert!(ours(&sandbox.join(".codex"), &home.join(".codex")), "no longer hidden");
        assert!(home.join(".config/gh/hosts.yml").exists() && home.join(".lark-cli").exists(), "the person's own stay");
        // Shown again: the folder split for it is one link again.
        assert!(mirror(&sandbox, &home, vec![]).is_empty());
        assert!(ours(&sandbox.join(".config/gh"), &home.join(".config/gh")));
    }

    const KEYCHAIN: &str = "Library/Keychains";

    /// The list starts as the defaults, is saved whole, changes every sandbox at once and launches
    /// mirror it; Janus home is always hidden; one path inside another is refused.
    #[test]
    fn the_hidden_list_starts_as_the_defaults_and_applies_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::orchestrator::test_app_with(&dir.path().join("janus"), |_| {});
        let home = app.cfg.host_home.clone();
        for p in [".codex", ".aws", ".config/gh", KEYCHAIN] {
            std::fs::create_dir_all(home.join(p)).unwrap();
        }
        let v = app.host_env().unwrap();
        assert_eq!(v["defaults"], true);
        let codex = v["hidden"].as_array().unwrap().iter().find(|h| h["path"] == ".codex").unwrap();
        assert_eq!((codex["why"].as_str(), codex["here"].as_bool()), (Some("agent"), Some(true)));
        assert!(v["hidden"].as_array().unwrap().iter().any(|h| h["path"] == ".aws" && h["why"] == "sensitive"));

        let agent = app.cfg.layout.agent_sandbox("a1");
        app.link_host(&agent).unwrap();
        assert!(ours(&agent.home.join(".config/gh"), &home.join(".config/gh")));
        assert!(!present(&agent.home.join(".codex")) && !present(&agent.home.join(".aws")));

        let v = app.save_host_env(&json!({ "paths": [".codex", "~/.config/gh", "notes"] })).unwrap();
        assert_eq!(v["defaults"], false);
        let whys: Vec<_> = v["hidden"].as_array().unwrap().iter().map(|h| h["why"].as_str().unwrap()).collect();
        assert_eq!(whys, ["agent", "custom", "custom"]);
        assert!(!present(&agent.home.join(".config/gh")), "hidden at once in a sandbox already there");
        assert!(ours(&agent.home.join(".aws"), &home.join(".aws")), "no longer hidden");

        // A full path in the home folder, as the open panel picks it, is made relative.
        let full = home.join(".aws").to_string_lossy().to_string();
        let v = app.save_host_env(&json!({ "paths": [full] })).unwrap();
        assert_eq!(v["hidden"][0]["path"], ".aws");
        for (bad, why) in [
            (json!([".config", ".config/gh"]), "在 ~/.config 里面"),
            (json!(["../x"]), "不是用户目录下的路径"),
            (json!(["/etc"]), "不在用户目录"),
        ] {
            let err = app.save_host_env(&json!({ "paths": bad })).unwrap_err().to_string();
            assert!(err.contains(why), "{bad}: {err}");
        }
        app.save_host_env(&json!({ "paths": null })).unwrap();
        assert_eq!(app.host_env().unwrap()["defaults"], true, "null goes back to the defaults");
    }

    /// A Janus home inside the home folder is never mirrored into the sandboxes it holds.
    #[test]
    fn janus_home_is_always_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::orchestrator::test_app_with(&dir.path().join("host-home/.janus"), |c| {
            c.host_home = dir.path().join("host-home");
        });
        assert_eq!(app.host_hidden().unwrap().last().unwrap(), ".janus");
        let agent = app.cfg.layout.agent_sandbox("a1");
        app.link_host(&agent).unwrap();
        assert!(!present(&agent.home.join(".janus")));
        let err = app.save_host_env(&json!({ "paths": [".janus/data"] })).unwrap_err().to_string();
        assert!(err.contains("Janus 的数据目录"), "{err}");
    }

    /// A command tried from the settings runs as the person and as an agent, each with its output.
    #[tokio::test]
    async fn a_command_runs_as_the_person_and_as_an_agent() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::orchestrator::test_app(dir.path());
        std::fs::create_dir_all(app.cfg.host_home.join(".aws")).unwrap();
        std::fs::write(app.cfg.host_home.join(".aws/config"), "aws").unwrap();
        std::fs::write(app.cfg.host_home.join(".gitconfig"), "git").unwrap();
        let v = app.try_host_command("cat ~/.gitconfig && cat ~/.aws/config").await.unwrap();
        assert_eq!((v["host"]["code"].as_i64(), v["host"]["output"].as_str()), (Some(0), Some("gitaws")));
        assert_eq!(v["agent"]["code"].as_i64(), Some(1), "~/.aws is hidden by default: {v}");
        assert!(v["agent"]["output"].as_str().unwrap().starts_with("git"), "{v}");
        assert!(app.try_host_command("  ").await.is_err());
    }
}
