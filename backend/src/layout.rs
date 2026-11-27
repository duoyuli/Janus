//! Where everything in Janus home lives. Every path Janus reads or writes there comes from a
//! method here; no other code joins names onto the home. The top level sorts by what losing a
//! thing costs, so a person can tell from the name what to back up and what may go:
//!
//! - `data/`: cannot be made again. Backing up Janus means backing up this.
//! - `work/`: the directories agents work in, one per run or chat.
//! - `cache/`: made again when needed; may go whenever Janus is stopped.
//! - `tmp/`: scratch of running processes; emptied each time Janus starts.
//! - `logs/`, `backups/`, `bin/`, `config.toml`, `janus.pid`.
//!
//! Inside a category, what belongs to one object (an agent, a run, a chat, a library item) sits
//! under that object's id, and `*_paths` lists everything an object owns, so deleting it is
//! deleting those paths. How to place something new: docs/operations/20261031-deployment.md#new_files.

// @project-doc docs/operations/20261031-deployment.md#layout
use crate::status::RunKind;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub struct Layout {
    home: PathBuf,
}

/// The names that may stand at the top of Janus home. Anything else there is not Janus's.
pub const TOP: &[&str] = &["config.toml", "janus.pid", "bin", "data", "work", "cache", "tmp", "logs", "backups"];

impl Layout {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Layout { home: home.into() }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// What stands at the top of the home that is not one of `TOP` (Finder's `.DS_Store` aside):
    /// something a person or another program put there, or left over from an older layout.
    pub fn foreign(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.home)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".DS_Store" && !TOP.contains(&n.as_str()))
            .collect();
        out.sort();
        out
    }

    // ---- Top level

    pub fn config_file(&self) -> PathBuf {
        self.home.join("config.toml")
    }

    /// The single-instance lock; it also names the owner's pid for the app.
    pub fn pid_file(&self) -> PathBuf {
        self.home.join("janus.pid")
    }

    /// Copies of the database taken before a data upgrade.
    pub fn backups(&self) -> PathBuf {
        self.home.join("backups")
    }

    fn data(&self) -> PathBuf {
        self.home.join("data")
    }

    pub fn db(&self) -> PathBuf {
        self.data().join("janus.db")
    }

    /// Emptied when Janus starts: nothing runs yet that could be using it.
    pub fn tmp(&self) -> PathBuf {
        self.home.join("tmp")
    }

    pub fn logs(&self) -> PathBuf {
        self.home.join("logs")
    }

    /// Janus's own log.
    pub fn janus_log(&self) -> PathBuf {
        self.logs().join("janus.log")
    }

    /// The credentials Janus keeps, one owner-only directory for all of them.
    fn secrets(&self) -> PathBuf {
        self.data().join("secrets")
    }

    /// The browsers and phones paired with Janus, one owner-only file per device: its name, its
    /// times and the SHA-256 of its token (`devices`). Written when a device is paired and as it
    /// is used, removed when it is unpaired.
    pub fn devices(&self) -> PathBuf {
        self.secrets().join("devices")
    }

    pub fn device(&self, id: &str) -> PathBuf {
        self.devices().join(format!("{id}.json"))
    }

    // ---- Agents

    /// What Janus keeps for one agent: `config/` and `home/`.
    pub fn agent(&self, id: &str) -> PathBuf {
        self.agents().join(id)
    }

    /// Where each agent's own folder (`agent`) is.
    pub fn agents(&self) -> PathBuf {
        self.data().join("agents")
    }

    /// The agent's own configuration directory (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`): its sign-in,
    /// settings and session transcripts.
    pub fn agent_config(&self, id: &str) -> PathBuf {
        self.agent(id).join("config")
    }

    /// `HOME` and the XDG directories of the agent's processes: what it installs for itself.
    pub fn agent_home(&self, id: &str) -> PathBuf {
        self.agent(id).join("home")
    }

    /// The credential fields of the agent and the private variables of its environment (`0600`).
    pub fn agent_secrets(&self, id: &str) -> PathBuf {
        self.secrets().join("agents").join(format!("{id}.json"))
    }

    pub fn agent_tmp(&self, id: &str) -> PathBuf {
        self.tmp().join("agents").join(id)
    }

    /// The configuration check's and the sign-in's output.
    pub fn agent_logs(&self, id: &str) -> PathBuf {
        self.logs().join("agents").join(id)
    }

    /// Where the agent's processes live: its `HOME` and its temp dir.
    pub fn agent_sandbox(&self, id: &str) -> Sandbox {
        Sandbox { home: self.agent_home(id), tmp: self.agent_tmp(id) }
    }

    /// Everything one agent owns, removed with it.
    pub fn agent_paths(&self, id: &str) -> Vec<PathBuf> {
        vec![self.agent(id), self.agent_secrets(id), self.agent_tmp(id), self.agent_logs(id)]
    }

    // ---- Digital employees (roles)

    /// What Janus keeps for one digital employee besides its record: `avatar.png`.
    pub fn role(&self, id: &str) -> PathBuf {
        self.data().join("roles").join(id)
    }

    /// The digital employee's own picture, a square PNG; none means its initials are shown.
    pub fn role_avatar(&self, id: &str) -> PathBuf {
        self.role(id).join("avatar.png")
    }

    /// Everything one digital employee owns besides its record, removed with it.
    pub fn role_paths(&self, id: &str) -> Vec<PathBuf> {
        vec![self.role(id)]
    }

    // ---- Library

    /// Skill contents, by the hash of what they hold; written once, never changed.
    pub fn skills(&self) -> PathBuf {
        self.data().join("library").join("skills")
    }

    /// The checkout of one Git source.
    pub fn source(&self, id: &str) -> PathBuf {
        self.data().join("library").join("sources").join(id)
    }

    /// `HOME` of the command server the MCP router runs for one item, kept across runs so its
    /// caches and its own sign-in survive.
    pub fn server_home(&self, id: &str) -> PathBuf {
        self.data().join("library").join("servers").join(id)
    }

    /// The server's temp dir; `warm/` in it is the folder it is prepared in.
    pub fn server_tmp(&self, id: &str) -> PathBuf {
        self.tmp().join("servers").join(id)
    }

    /// The HOME of every sandbox there is: each agent's and each MCP server's.
    pub fn sandbox_homes(&self) -> Vec<PathBuf> {
        let dirs = |dir: PathBuf| -> Vec<PathBuf> {
            std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()
        };
        let agents = dirs(self.agents()).into_iter().map(|a| a.join("home")).filter(|h| h.is_dir());
        agents.chain(dirs(self.data().join("library").join("servers"))).collect()
    }

    /// Where a command tried from the settings runs as an agent would (`host_env`), mirrored
    /// afresh each time; cleared with the rest of `tmp/` at start.
    pub fn host_probe(&self) -> Sandbox {
        let dir = self.tmp().join("host-probe");
        Sandbox { home: dir.join("home"), tmp: dir.join("tmp") }
    }

    pub fn server_sandbox(&self, id: &str) -> Sandbox {
        Sandbox { home: self.server_home(id), tmp: self.server_tmp(id) }
    }

    /// The environment and header values of one MCP server (`0600`).
    pub fn mcp_values(&self, id: &str) -> PathBuf {
        self.secrets().join("library").join(format!("{id}.json"))
    }

    /// The tool lists one MCP server gave, `<revision>.json`.
    pub fn mcp_tools(&self, id: &str) -> PathBuf {
        self.cache().join("mcp-tools").join(id)
    }

    /// The stderr of the server while it is prepared.
    pub fn library_log(&self, id: &str) -> PathBuf {
        self.logs().join("library").join(format!("{id}.log"))
    }

    /// Everything one library item owns besides its record, removed with it.
    pub fn item_paths(&self, id: &str) -> Vec<PathBuf> {
        vec![self.server_home(id), self.server_tmp(id), self.mcp_values(id), self.mcp_tools(id), self.library_log(id)]
    }

    // ---- Runs and chats

    pub fn run(&self, kind: RunKind, id: &str) -> RunDirs {
        let group = kind.dir();
        RunDirs {
            data: self.data().join(group).join(id),
            work: self.home.join("work").join(group).join(id),
            logs: self.logs().join(group).join(id),
        }
    }

    /// Changes never committed in the worktree of a run that was deleted, `<run>.patch`.
    pub fn deleted_runs(&self) -> PathBuf {
        self.data().join("deleted-runs")
    }

    // ---- Caches

    fn cache(&self) -> PathBuf {
        self.home.join("cache")
    }

    /// ACP adapters, a directory per version.
    pub fn adapters(&self) -> PathBuf {
        self.cache().join("adapters")
    }

    pub fn adapter_log(&self, id: &str) -> PathBuf {
        self.logs().join("adapters").join(format!("{id}.log"))
    }

    /// The version the person switched to by hand: automatic updates leave it alone while this
    /// file names it. No file means the adapter follows the newest version.
    pub fn adapter_pin(&self, id: &str) -> PathBuf {
        self.adapters().join(id).join("pinned")
    }

    /// Model prices from models.dev.
    pub fn prices(&self) -> PathBuf {
        self.cache().join("prices.json")
    }

    // ---- The macOS app's own (`JanusHome` writes them; the service only moves them)

    /// The app's own records, such as whether the macOS permission guidance was dealt with.
    pub fn app_state(&self) -> PathBuf {
        self.data().join("app")
    }

    /// In-app updates: the version being downloaded, and `previous/`.
    pub fn updates(&self) -> PathBuf {
        self.cache().join("updates")
    }

    // ---- Recognizing files Janus stored

    /// Whether `path` is an artifact of a run, `data/runs/<run>/artifacts/<task>/<file>` or the
    /// whole run's diff, `data/runs/<run>/changes/<file>`: the only Janus files a client reads as a run's, since
    /// the rest of the home holds the database and secrets.
    pub fn is_artifact(&self, path: &Path) -> bool {
        self.stored(path, &[RunKind::Goal], "artifacts", 3) || self.stored(path, &[RunKind::Goal], "changes", 2)
    }

    /// Whether `path` is an image pasted into a run, a chat or a Loop, `data/<runs|chats|loops>/<id>/attachments/<file>`.
    pub fn is_attachment(&self, path: &Path) -> bool {
        let kinds: Vec<RunKind> = RunKind::ALL.iter().copied().filter(|k| k.takes_images()).collect();
        self.stored(path, &kinds, "attachments", 2)
    }

    /// A file at `data/<group>/<id>/<part>/…`, `depth` names below the id; both sides resolved,
    /// so a link cannot lead out.
    fn stored(&self, path: &Path, kinds: &[RunKind], part: &str, depth: usize) -> bool {
        let Ok(path) = path.canonicalize() else { return false };
        kinds.iter().any(|&k| {
            let Ok(group) = self.data().join(k.dir()).canonicalize() else { return false };
            let Ok(rest) = path.strip_prefix(&group) else { return false };
            let parts: Vec<_> = rest.components().collect();
            parts.len() == depth + 1 && parts[1].as_os_str() == part && path.is_file()
        })
    }
}

/// Remove a file or a whole directory; one already gone is fine.
pub fn remove(path: &Path) {
    let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

/// The two places a sandboxed process (an agent's, or an MCP server's) gets: `HOME` with the XDG
/// directories under it, and `TMPDIR`.
#[derive(Clone, Debug)]
pub struct Sandbox {
    pub home: PathBuf,
    pub tmp: PathBuf,
}

/// The three places of one run or chat. A goal run's `work` is its worktree (only on a Git
/// repository); a chat's is the scratch folder it works in.
#[derive(Clone, Debug)]
pub struct RunDirs {
    /// What Janus recorded for it: bundles, attachments, artifacts.
    pub data: PathBuf,
    pub work: PathBuf,
    /// The stderr of every process it started.
    pub logs: PathBuf,
}

impl RunDirs {
    /// A member's resources, laid out the way its agent reads them.
    pub fn bundle(&self, member: &str) -> PathBuf {
        self.data.join("bundle").join(member)
    }

    /// Images pasted into the goal or a message.
    pub fn attachments(&self) -> PathBuf {
        self.data.join("attachments")
    }

    /// What one task's attempts left: their replies and diffs.
    pub fn artifacts(&self, task: &str) -> PathBuf {
        self.data.join("artifacts").join(task)
    }

    /// What the whole run changed so far, against where it started: one diff per content.
    pub fn changes(&self) -> PathBuf {
        self.data.join("changes")
    }

    /// One process's stderr.
    pub fn log(&self, name: &str) -> PathBuf {
        self.logs.join(name)
    }
}
