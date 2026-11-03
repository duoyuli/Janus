//! Skill sources: GitHub repositories whose skills sit next to Janus's own library.
//!
//! Adding a link clones the repository (shallow) into `<home>/skill-sources/<id>/repo` and finds
//! every folder with a `SKILL.md`. Each skill gets a key that stays the same across updates, and
//! roles mount it as `<source id>/<key>`, beside the plain ids of the library's own skills. Runs
//! never read a source: like any skill, it is copied into the run's snapshot when the run starts.

use crate::agents::{ROLE, check_skill_id, list_skills, read_skill, slug, str_of};
use crate::orchestrator::{App, now_ms};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

/// One lock per folder. A source's own folder serializes its fetch, the recording of a check and
/// its deletion, so different sources never wait for each other; the sources root serializes
/// adding, so two adds of one link cannot both pass the duplicate check. Keyed by path, so two
/// Janus homes (or tests running in parallel) never share a lock.
pub(crate) fn folder_lock(dir: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<std::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);
    LOCKS.lock().unwrap().entry(dir.to_path_buf()).or_default().clone()
}

/// How deep a skill folder may sit in a repository, and how many one repository may bring.
const MAX_DEPTH: usize = 8;
const MAX_SKILLS: usize = 500;
const CLONE_TIMEOUT: Duration = Duration::from_secs(180);
/// How often the server asks each source's remote whether it moved on.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// The commit a remote's ref points at, from `git ls-remote` output: the default branch for an
/// empty ref, else the branch of that name, else the tag (peeled when annotated).
pub fn remote_head(out: &str, git_ref: &str) -> Option<String> {
    let lines: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once('\t')).collect();
    let find = |name: &str| lines.iter().find(|(_, n)| *n == name).map(|(sha, _)| sha.to_string());
    if git_ref.is_empty() {
        return find("HEAD");
    }
    find(&format!("refs/heads/{git_ref}"))
        .or_else(|| find(&format!("refs/tags/{git_ref}^{{}}")))
        .or_else(|| find(&format!("refs/tags/{git_ref}")))
}

/// A folder of a GitHub repository, as a link names it.
#[derive(Debug, PartialEq)]
pub struct RepoAt {
    /// `owner/name`.
    pub repo: String,
    /// Branch or tag; empty for the default branch.
    pub git_ref: String,
    /// The folder to look in, relative to the repository; empty for all of it.
    pub path: String,
}

impl RepoAt {
    pub fn url(&self) -> String {
        let mut u = format!("https://github.com/{}", self.repo);
        if !self.git_ref.is_empty() || !self.path.is_empty() {
            let r = if self.git_ref.is_empty() { "HEAD" } else { &self.git_ref };
            u.push_str(&format!("/tree/{r}"));
            if !self.path.is_empty() {
                u.push_str(&format!("/{}", self.path));
            }
        }
        u
    }
}

// @project-doc docs/domains/agents_and_roles.md#skill_sources
/// Read a GitHub link: `github.com/o/r`, `https://github.com/o/r.git`, `git@github.com:o/r.git`,
/// `.../tree/<ref>/<folder>`, `.../blob/<ref>/<folder>/SKILL.md`, or plain `o/r`. A ref with `/`
/// in it can't be told from the folder, so the first segment after `tree/` is taken as the ref.
pub fn parse_github(link: &str) -> Result<RepoAt> {
    let mut q = link.trim().trim_start_matches("git+");
    if let Some(i) = q.find(['#', '?']) {
        q = &q[..i];
    }
    for p in ["https://", "http://", "ssh://git@", "git@"] {
        q = q.strip_prefix(p).unwrap_or(q);
    }
    q = q.strip_prefix("www.").unwrap_or(q);
    let rest = if let Some(r) = q.strip_prefix("github.com:").or_else(|| q.strip_prefix("github.com/")) {
        r
    } else if q.contains('.') && q.split('/').next().is_some_and(|h| h.contains('.')) {
        bail!("只支持 GitHub 仓库链接，例如 https://github.com/owner/repo");
    } else {
        q
    };
    let parts: Vec<&str> = rest.trim_matches('/').split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        bail!("链接里没有仓库：需要 github.com/owner/repo");
    }
    let name = parts[1].trim_end_matches(".git");
    let ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['.', '-'])
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !ok(parts[0]) || !ok(name) {
        bail!("不是有效的 GitHub 仓库：{}/{}", parts[0], name);
    }
    let mut at = RepoAt { repo: format!("{}/{name}", parts[0]), git_ref: String::new(), path: String::new() };
    if parts.len() >= 4 && matches!(parts[2], "tree" | "blob") {
        at.git_ref = if parts[3] == "HEAD" { String::new() } else { parts[3].to_string() };
        let mut sub = &parts[4..];
        if parts[2] == "blob" && !sub.is_empty() {
            sub = &sub[..sub.len() - 1];
        }
        at.path = sub.join("/");
    }
    if at.path.split('/').any(|s| s == "..") || at.git_ref.starts_with('-') {
        bail!("链接中的路径无效");
    }
    Ok(at)
}

/// Every folder under `start` holding a regular `SKILL.md`, relative to `repo`. Symbolic links are
/// never followed, `.git` and `node_modules` are skipped, and a skill's own subfolders are not
/// searched (they hold its material, not more skills).
pub fn scan(repo: &Path, start: &Path) -> Vec<String> {
    fn walk(repo: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        if out.len() >= MAX_SKILLS || depth > MAX_DEPTH {
            return;
        }
        if std::fs::symlink_metadata(dir.join("SKILL.md")).is_ok_and(|m| m.is_file()) {
            out.push(dir.strip_prefix(repo).unwrap_or(Path::new("")).to_string_lossy().into_owned());
            return;
        }
        let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !matches!(e.file_name().to_str(), Some(".git" | "node_modules")))
            .map(|e| e.path())
            .collect();
        subs.sort();
        for s in subs {
            walk(repo, &s, depth + 1, out);
        }
    }
    let mut out = vec![];
    if std::fs::symlink_metadata(start).is_ok_and(|m| m.is_dir()) && inside(repo, start) {
        walk(repo, start, 0, &mut out);
    }
    out
}

/// Whether `start` really lies inside `repo`. The link's folder is joined onto the clone, and a
/// symbolic link among its parent folders could otherwise lead the scan out of the clone.
pub(crate) fn inside(repo: &Path, start: &Path) -> bool {
    match (repo.canonicalize(), start.canonicalize()) {
        (Ok(r), Ok(s)) => s.starts_with(r),
        _ => false,
    }
}

/// Give each skill folder a key: the one it had before when its folder is unchanged, else its
/// folder's name (the repository's name for a skill at the top), made unique.
fn assign_keys(repo_name: &str, found: &[String], before: &[Value]) -> Vec<Value> {
    let mut taken: Vec<String> = vec![];
    let mut out = vec![];
    let mut keyed: Vec<(String, Option<String>)> = found
        .iter()
        .map(|p| {
            let prev = before
                .iter()
                .find(|b| b["path"] == p.as_str())
                .and_then(|b| b["key"].as_str())
                .map(str::to_owned);
            (p.clone(), prev)
        })
        .collect();
    for (_, prev) in &keyed {
        if let Some(k) = prev {
            taken.push(k.clone());
        }
    }
    for (path, prev) in keyed.iter_mut() {
        let key = match prev.take() {
            Some(k) => k,
            None => {
                let leaf = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(repo_name);
                let base = slug(leaf).unwrap_or_else(|| "skill".into());
                let k = (1..)
                    .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
                    .find(|k| !taken.contains(k))
                    .unwrap();
                taken.push(k.clone());
                k
            }
        };
        out.push(json!({ "key": key, "path": path }));
    }
    out
}

pub(crate) async fn git_in(cwd: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(CLONE_TIMEOUT, cmd.output())
        .await
        .context("拉取仓库超时")??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        bail!("git {}：{line}", args.first().unwrap_or(&""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl App {
    pub fn skill_sources_root(&self) -> PathBuf {
        self.cfg.data_dir.join("skill-sources")
    }

    fn read_source(&self, id: &str) -> Option<Value> {
        check_skill_id(id).ok()?;
        let text = std::fs::read_to_string(self.skill_sources_root().join(id).join("source.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn sources(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(self.skill_sources_root())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.read_source(&e.file_name().to_string_lossy()))
            .collect();
        out.sort_by_key(|s| s["addedAt"].as_i64().unwrap_or(0));
        out
    }

    /// The skills a source brings, as the library lists its own (id, name, description, path, source).
    fn source_skills(&self, src: &Value) -> Vec<Value> {
        let id = str_of(src, "id");
        let repo = self.skill_sources_root().join(id).join("repo");
        src["skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let mut v = read_skill(&repo.join(str_of(s, "path")), str_of(src, "repo"))?;
                v["id"] = json!(format!("{id}/{}", str_of(s, "key")));
                v["sourceId"] = json!(id);
                v["relPath"] = s["path"].clone();
                Some(v)
            })
            .collect()
    }

    /// The library's own skills, then every source's.
    pub fn all_skills(&self) -> Value {
        let mut out = list_skills();
        let list = out["skills"].as_array_mut().unwrap();
        for src in self.sources() {
            list.extend(self.source_skills(&src));
        }
        out
    }

    /// Where a skill a role mounts lives: `<id>` in the library, `<source>/<key>` in a source.
    pub fn skill_dir(&self, id: &str) -> Option<PathBuf> {
        let dir = match id.split_once('/') {
            None => {
                check_skill_id(id).ok()?;
                crate::agents::skill_library().join(id)
            }
            Some((src, key)) => {
                let s = self.read_source(src)?;
                let entry = s["skills"].as_array()?.iter().find(|e| e["key"] == key)?;
                self.skill_sources_root().join(src).join("repo").join(str_of(entry, "path"))
            }
        };
        std::fs::symlink_metadata(dir.join("SKILL.md")).is_ok_and(|m| m.is_file()).then_some(dir)
    }

    /// The skill as a run's member carries it: name, description and path, under the id the role uses.
    pub fn skill_info(&self, id: &str) -> Option<Value> {
        let dir = self.skill_dir(id)?;
        let label = match id.split_once('/') {
            Some((src, _)) => self.read_source(src).map(|s| str_of(&s, "repo").to_string()).unwrap_or_default(),
            None => "Janus".into(),
        };
        let mut v = read_skill(&dir, &label)?;
        v["id"] = json!(id);
        Some(v)
    }

    pub fn list_skill_sources(&self) -> Value {
        let list: Vec<Value> = self
            .sources()
            .into_iter()
            .map(|s| {
                let skills = self.source_skills(&s);
                let mut s = s;
                s["skills"] = json!(skills);
                s
            })
            .collect();
        json!({ "sources": list, "root": self.skill_sources_root() })
    }

    /// Add a GitHub link: clone it and index its skills.
    pub async fn add_skill_source(&self, link: &str) -> Result<Value> {
        let at = parse_github(link)?;
        let clone = format!("https://github.com/{}.git", at.repo);
        self.add_source_at(at, &clone).await
    }

    pub(crate) async fn add_source_at(&self, at: RepoAt, clone_url: &str) -> Result<Value> {
        let _adding = folder_lock(&self.skill_sources_root()).lock_owned().await;
        if let Some(s) = self.sources().into_iter().find(|s| {
            s["repo"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(&at.repo)) && s["ref"] == at.git_ref.as_str() && s["path"] == at.path.as_str()
        }) {
            bail!("已经添加过这个来源：{}", str_of(&s, "url"));
        }
        let base = slug(&format!("{}{}", at.repo.replace('/', "-"), if at.path.is_empty() { String::new() } else { format!("-{}", at.path.rsplit('/').next().unwrap_or("")) }))
            .unwrap_or_else(|| "source".into());
        let root = self.skill_sources_root();
        let id = (1..1000)
            .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
            .find(|id| !root.join(id).exists())
            .context("没有可用的来源 ID")?;
        let src = json!({
            "id": id, "url": at.url(), "repo": at.repo, "ref": at.git_ref, "path": at.path,
            "cloneUrl": clone_url, "addedAt": now_ms(), "skills": [],
        });
        let _guard = folder_lock(&root.join(&id)).lock_owned().await;
        std::fs::create_dir_all(root.join(&id))?;
        match self.sync(src).await {
            Ok(v) => Ok(v),
            Err(e) => {
                let _ = std::fs::remove_dir_all(root.join(&id));
                Err(e)
            }
        }
    }

    /// Fetch the source again: its skills follow the repository, keys stay with their folders.
    pub async fn update_skill_source(&self, id: &str) -> Result<Value> {
        check_skill_id(id)?;
        let _guard = folder_lock(&self.skill_sources_root().join(id)).lock_owned().await;
        let src = self.read_source(id).context("没有这个来源")?;
        self.sync(src).await
    }

    // @project-doc docs/domains/agents_and_roles.md#skill_source_updates
    /// Ask every source's remote for its current commit, without fetching anything, and record
    /// whether it differs from the local copy. Returns the sources as listed.
    pub async fn check_skill_sources(&self) -> Value {
        for src in self.sources() {
            let id = str_of(&src, "id").to_string();
            let git_ref = str_of(&src, "ref").to_string();
            let target = if git_ref.is_empty() { "HEAD".to_string() } else { git_ref.clone() };
            let url = str_of(&src, "cloneUrl").to_string();
            let dir = self.skill_sources_root().join(&id);
            let answer = git_in(&dir, &["ls-remote", "--", &url, &target]).await;
            // Record under the lock, on top of whatever an update wrote meanwhile.
            let _guard = folder_lock(&dir).lock_owned().await;
            let Some(mut now) = self.read_source(&id) else { continue };
            now["checkedAt"] = json!(now_ms());
            match answer.map(|out| remote_head(&out, &git_ref)) {
                Ok(Some(sha)) => {
                    now["updateAvailable"] = json!(now["commit"].as_str().is_some_and(|c| !c.is_empty() && c != sha));
                    now["latest"] = json!(sha);
                    now["checkError"] = json!("");
                }
                Ok(None) => now["checkError"] = json!(format!("远端没有 {target}")),
                Err(e) => now["checkError"] = json!(format!("{e:#}")),
            }
            let _ = std::fs::write(dir.join("source.json"), serde_json::to_string_pretty(&now).unwrap_or_default());
        }
        self.list_skill_sources()
    }

    async fn sync(&self, mut src: Value) -> Result<Value> {
        let dir = self.skill_sources_root().join(str_of(&src, "id"));
        let fresh = dir.join(format!(".repo-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&fresh);
        let fresh_s = fresh.to_string_lossy().to_string();
        let mut args = vec!["clone", "--depth", "1", "--quiet", "--no-tags"];
        let git_ref = str_of(&src, "ref").to_string();
        if !git_ref.is_empty() {
            args.extend(["--branch", git_ref.as_str()]);
        }
        let url = str_of(&src, "cloneUrl").to_string();
        args.extend(["--", url.as_str(), fresh_s.as_str()]);
        let result: Result<Value> = async {
            git_in(&dir, &args).await.context("拉取仓库失败")?;
            let commit = git_in(&fresh, &["rev-parse", "HEAD"]).await.unwrap_or_default();
            let found = scan(&fresh, &fresh.join(str_of(&src, "path")));
            if found.is_empty() {
                bail!("这个位置没有找到任何 SKILL.md");
            }
            let name = str_of(&src, "repo").rsplit('/').next().unwrap_or("skill").to_string();
            let before = src["skills"].as_array().cloned().unwrap_or_default();
            src["skills"] = json!(assign_keys(&name, &found, &before));
            src["commit"] = json!(commit);
            src["updatedAt"] = json!(now_ms());
            src["checkedAt"] = json!(now_ms());
            src["latest"] = json!(commit);
            src["updateAvailable"] = json!(false);
            src["checkError"] = json!("");
            Ok(src.clone())
        }
        .await;
        let src = match result {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&fresh);
                return Err(e);
            }
        };
        // Swap the checkout in, then record it.
        let repo = dir.join("repo");
        let old = dir.join(".repo-old");
        let _ = std::fs::remove_dir_all(&old);
        if repo.exists() {
            std::fs::rename(&repo, &old)?;
        }
        std::fs::rename(&fresh, &repo)?;
        let _ = std::fs::remove_dir_all(&old);
        std::fs::write(dir.join("source.json"), serde_json::to_string_pretty(&src)?)?;
        let skills = self.source_skills(&src);
        let mut out = src;
        out["skills"] = json!(skills);
        Ok(out)
    }

    pub fn delete_skill_source(&self, id: &str) -> Result<()> {
        check_skill_id(id)?;
        // A fetch in progress would write the source back after it is gone.
        let Ok(_guard) = folder_lock(&self.skill_sources_root().join(id)).try_lock_owned() else {
            bail!("这个来源正在拉取，完成后再删除")
        };
        let src = self.read_source(id).context("没有这个来源")?;
        let prefix = format!("{id}/");
        let users: Vec<String> = self
            .db
            .list_config(ROLE)?
            .iter()
            .filter(|r| r["skills"].as_array().is_some_and(|s| s.iter().any(|x| x.as_str().is_some_and(|x| x.starts_with(&prefix)))))
            .map(|r| str_of(r, "name").to_string())
            .collect();
        if !users.is_empty() {
            bail!("角色「{}」挂载了 {} 中的 Skill，请先移除", users.join("」「"), str_of(&src, "repo"));
        }
        std::fs::remove_dir_all(self.skill_sources_root().join(id)).context("删除来源失败")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::test_app;

    #[test]
    fn github_links_in_every_usual_shape() {
        let p = |s| parse_github(s).unwrap();
        let plain = RepoAt { repo: "yetone/magpie".into(), git_ref: String::new(), path: String::new() };
        assert_eq!(p("https://github.com/yetone/magpie.git"), plain);
        assert_eq!(p("github.com/yetone/magpie"), plain);
        assert_eq!(p("git@github.com:yetone/magpie.git"), plain);
        assert_eq!(p("yetone/magpie"), plain);
        assert_eq!(p("https://github.com/yetone/magpie/"), plain);
        let tree = p("https://github.com/anthropics/skills/tree/main/document-skills");
        assert_eq!((tree.git_ref.as_str(), tree.path.as_str()), ("main", "document-skills"));
        assert_eq!(tree.url(), "https://github.com/anthropics/skills/tree/main/document-skills");
        let blob = p("https://github.com/o/r/blob/v2/skills/pdf/SKILL.md");
        assert_eq!((blob.git_ref.as_str(), blob.path.as_str()), ("v2", "skills/pdf"));
        assert!(parse_github("https://gitlab.com/o/r").is_err());
        assert!(parse_github("https://github.com/o").is_err());
        assert!(parse_github("https://github.com/o/r/tree/main/../x").is_err());
    }

    #[test]
    fn the_remote_head_follows_the_ref() {
        let out = "aaa\tHEAD\nbbb\trefs/heads/main\nccc\trefs/tags/v1\nddd\trefs/tags/v1^{}\n";
        assert_eq!(remote_head(out, "").as_deref(), Some("aaa"));
        assert_eq!(remote_head(out, "main").as_deref(), Some("bbb"));
        assert_eq!(remote_head(out, "v1").as_deref(), Some("ddd"), "an annotated tag is peeled");
        assert_eq!(remote_head(out, "gone"), None);
    }

    #[test]
    fn keys_stay_with_their_folders() {
        let first = assign_keys("repo", &["a/pdf".into(), "b/pdf".into(), "".into()], &[]);
        let keys: Vec<&str> = first.iter().map(|k| k["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["pdf", "pdf-2", "repo"]);
        // b/pdf keeps pdf-2 although a/pdf is gone and a new c/pdf appears.
        let next = assign_keys("repo", &["b/pdf".into(), "c/pdf".into()], &first);
        assert_eq!(next[0]["key"], "pdf-2");
        assert_eq!(next[1]["key"], "pdf");
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn skill(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: does {name}\n---\nbody\n")).unwrap();
    }

    #[tokio::test]
    async fn a_source_is_cloned_indexed_mounted_and_updated() {
        let dir = std::env::temp_dir().join(format!("janus-skill-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let upstream = dir.join("upstream");
        skill(&upstream.join("skills/pdf"), "pdf");
        skill(&upstream.join(".claude/skills/review"), "review");
        skill(&upstream.join("skills/pdf/examples/inner"), "inner"); // part of pdf, not a skill
        std::fs::create_dir_all(upstream.join("node_modules/x")).unwrap();
        std::fs::write(upstream.join("node_modules/x/SKILL.md"), "no").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(upstream.join("skills"), upstream.join("linked")).unwrap();
        sh(&upstream, &["init", "-q"]);
        sh(&upstream, &["add", "-A"]);
        sh(&upstream, &["commit", "-q", "-m", "init"]);

        let app = test_app(&dir);
        let url = format!("file://{}", upstream.display());
        let at = || RepoAt { repo: "acme/skills".into(), git_ref: String::new(), path: String::new() };
        let src = app.add_source_at(at(), &url).await.unwrap();
        let id = src["id"].as_str().unwrap().to_string();
        assert_eq!(id, "acme-skills");
        let names: Vec<&str> = src["skills"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["review", "pdf"], "hidden folders count, node_modules, symlinks and a skill's own folders do not");
        assert!(app.add_source_at(at(), &url).await.is_err(), "the same repository twice");

        // The library lists it, a role can mount it, and a run gets it.
        let all = app.all_skills();
        assert!(all["skills"].as_array().unwrap().iter().any(|s| s["id"] == "acme-skills/pdf" && s["source"] == "acme/skills"));
        assert_eq!(app.skill_info("acme-skills/pdf").unwrap()["name"], "pdf");
        assert!(app.skill_dir("acme-skills/nope").is_none());
        assert!(app.skill_dir("../x/pdf").is_none());
        app.db.put_config(ROLE, "r", &json!({ "id": "r", "name": "审查", "skills": ["acme-skills/pdf"] }), now_ms()).unwrap();
        assert!(app.delete_skill_source(&id).unwrap_err().to_string().contains("审查"));

        // Upstream moves on: a new skill appears, review goes away, pdf keeps its id.
        std::fs::remove_dir_all(upstream.join(".claude")).unwrap();
        skill(&upstream.join("skills/lint"), "lint");
        sh(&upstream, &["add", "-A"]);
        sh(&upstream, &["commit", "-q", "-m", "next"]);
        let listed = app.check_skill_sources().await;
        assert_eq!(listed["sources"][0]["updateAvailable"], true, "{listed}");
        let src = app.update_skill_source(&id).await.unwrap();
        assert_eq!(src["updateAvailable"], false);
        assert_eq!(app.check_skill_sources().await["sources"][0]["updateAvailable"], false, "nothing new after the update");
        let ids: Vec<&str> = src["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["acme-skills/lint", "acme-skills/pdf"]);
        assert!(app.skill_dir("acme-skills/pdf").is_some());

        app.db.delete_config(ROLE, "r").unwrap();
        app.delete_skill_source(&id).unwrap();
        assert!(app.list_skill_sources()["sources"].as_array().unwrap().is_empty());

        // A place without skills is refused and leaves nothing behind.
        let empty = RepoAt { repo: "acme/skills".into(), git_ref: String::new(), path: "nothing".into() };
        assert!(app.add_source_at(empty, &url).await.is_err());
        assert!(std::fs::read_dir(app.skill_sources_root()).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
