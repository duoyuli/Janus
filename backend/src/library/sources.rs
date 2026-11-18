//! Git sources: a GitHub repository, or one of its folders, whose skills and MCP servers can be
//! imported into the library. Adding a link clones it (shallow) into
//! `<home>/library/sources/<id>/repo` and lists what it offers; nothing is imported until the
//! person picks. An update fetches again and gives each imported item the content the repository
//! has now.
// @project-doc docs/domains/20261116-resource_library.md#sources

use super::offers::{self, Offer, ScanMode};
use super::{Body, Item, ItemKind, Origin, imported_offer, mcp, mcp_revision, skill, store};
use crate::orchestrator::{App, now_ms};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SOURCE: &str = "library_source";
const CLONE_TIMEOUT: Duration = Duration::from_secs(180);
/// How often the server asks each source's remote whether it moved on.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    /// The link, as GitHub shows the folder.
    pub url: String,
    pub clone_url: String,
    /// `owner/name`.
    pub repo: String,
    /// Branch or tag; empty for the default branch.
    #[serde(default)]
    pub git_ref: String,
    /// The folder looked in, relative to the repository; empty for all of it.
    #[serde(default)]
    pub path: String,
    /// The commit the offers were read from.
    pub commit: String,
    pub fetched_at: i64,
    /// The commit the last check found the remote at; another one than `commit` is an update.
    #[serde(default)]
    pub latest: String,
    #[serde(default)]
    pub check_error: String,
    #[serde(default)]
    pub checked_at: i64,
    pub offers: Vec<Offer>,
    /// Manifest entries that could not be read.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A folder of a GitHub repository, as a link names it.
#[derive(Debug, PartialEq)]
pub struct RepoAt {
    pub repo: String,
    pub git_ref: String,
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

async fn git_in(cwd: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(CLONE_TIMEOUT, cmd.output()).await.context("拉取仓库超时")??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        bail!("git {}：{line}", args.first().unwrap_or(&""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Shallow-clone into `<dir>/repo` and return the commit. The previous checkout stays until the
/// clone has succeeded.
async fn fetch(dir: &Path, clone_url: &str, git_ref: &str) -> Result<String> {
    std::fs::create_dir_all(dir)?;
    let fresh = dir.join(format!(".repo-{}", now_ms()));
    let fresh_s = fresh.to_string_lossy().to_string();
    let mut args = vec!["clone", "--depth", "1", "--quiet", "--no-tags"];
    if !git_ref.is_empty() {
        args.extend(["--branch", git_ref]);
    }
    args.extend(["--", clone_url, fresh_s.as_str()]);
    if let Err(e) = git_in(dir, &args).await.context("拉取仓库失败") {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(e);
    }
    let commit = git_in(&fresh, &["rev-parse", "HEAD"]).await.unwrap_or_default();
    let repo = dir.join("repo");
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::rename(&fresh, &repo)?;
    Ok(commit)
}

/// The server a manifest entry `<manifest>#<name>` describes in a checkout.
fn server_at(repo: &Path, path: &str) -> Result<mcp::Found> {
    let (manifest, name) = path.rsplit_once('#').context("服务器路径无效")?;
    let text = std::fs::read_to_string(repo.join(manifest)).with_context(|| format!("读取 {manifest} 失败"))?;
    mcp::parse_config(&text)?.0.into_iter().find(|f| f.name == name).ok_or_else(|| anyhow!("{manifest} 中没有 {name}"))
}

impl App {
    fn source_dir(&self, id: &str) -> Result<PathBuf> {
        crate::agents::check_id(id)?;
        Ok(self.cfg.data_dir.join("library").join("sources").join(id))
    }

    pub(super) fn sources(&self) -> Result<Vec<Source>> {
        self.db
            .list_config(SOURCE)?
            .into_iter()
            .map(|v| serde_json::from_value(v).context("来源记录无法读取"))
            .collect()
    }

    fn source(&self, id: &str) -> Result<Source> {
        let v = self.db.get_config(SOURCE, id)?.ok_or_else(|| anyhow!("来源 {id} 不存在"))?;
        serde_json::from_value(v).context("来源记录无法读取")
    }

    fn put_source(&self, s: &Source) -> Result<()> {
        self.db.put_config(SOURCE, &s.id, &serde_json::to_value(s)?, now_ms())
    }

    /// A source as the app shows it: whether the remote moved on, and which offers are imported.
    pub(super) fn source_json(&self, s: &Source) -> Result<Value> {
        let mut v = serde_json::to_value(s)?;
        v["updateAvailable"] = json!(!s.latest.is_empty() && s.latest != s.commit);
        let imported: Vec<Value> = self
            .items()?
            .into_iter()
            .filter_map(|i| match &i.origin {
                Origin::Git { source, path } if source == &s.id => Some(imported_offer(path, &i)),
                _ => None,
            })
            .collect();
        v["imported"] = json!(imported);
        Ok(v)
    }

    /// Add a GitHub link: fetch it and list what it offers.
    pub async fn add_source(&self, link: &str) -> Result<Value> {
        let at = parse_github(link)?;
        let clone_url = format!("https://github.com/{}.git", at.repo);
        self.add_source_at(at, &clone_url).await
    }

    /// Add a source cloned from `clone_url` (tests clone a local repository).
    pub(crate) async fn add_source_at(&self, at: RepoAt, clone_url: &str) -> Result<Value> {
        let _guard = self.library.lock.lock().await;
        if self.sources()?.iter().any(|s| s.repo == at.repo && s.git_ref == at.git_ref && s.path == at.path) {
            bail!("已经添加过这个来源");
        }
        let id = crate::agents::new_id("s");
        let dir = self.source_dir(&id)?;
        let added = async {
            let commit = fetch(&dir, clone_url, &at.git_ref).await?;
            let repo = dir.join("repo");
            let (offers, warnings) = offers::discover(&repo, &repo.join(&at.path), ScanMode::SkillsAndMcp);
            if offers.is_empty() {
                if !warnings.is_empty() {
                    bail!("{}：{}", at.url(), warnings.join("；"));
                }
                bail!("{} 中没有找到 Skill（SKILL.md）或 MCP 服务器（mcp.json）", at.url());
            }
            let now = now_ms();
            let s = Source {
                id: id.clone(),
                url: at.url(),
                clone_url: clone_url.to_string(),
                repo: at.repo.clone(),
                git_ref: at.git_ref.clone(),
                path: at.path.clone(),
                latest: commit.clone(),
                commit,
                fetched_at: now,
                check_error: String::new(),
                checked_at: now,
                offers,
                warnings,
            };
            self.put_source(&s)?;
            self.source_json(&s)
        }
        .await;
        if added.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        added
    }

    /// Import offers of a source, by path. Taken names get a suffix.
    pub async fn import_from_source(&self, id: &str, paths: &[String]) -> Result<Vec<Item>> {
        let _guard = self.library.lock.lock().await;
        let s = self.source(id)?;
        let repo = self.source_dir(id)?.join("repo");
        let mut added = vec![];
        for path in paths {
            let offer = s.offers.iter().find(|o| &o.path == path).ok_or_else(|| anyhow!("来源中没有 {path}"))?;
            let origin = Origin::Git { source: id.to_string(), path: path.clone() };
            let item = match offer.kind {
                ItemKind::Skill => self.import_skill_at(&repo.join(path), origin)?,
                ItemKind::Mcp => {
                    let f = server_at(&repo, path)?;
                    let name = self.free_name(ItemKind::Mcp, &f.name)?;
                    let item = self.new_item(
                        name,
                        f.description,
                        origin,
                        mcp_revision(&f.server),
                        Body::Mcp { server: f.server.clone() },
                    );
                    self.write_values(&item.id, &f.values.kept_for(&f.server))?;
                    self.put_item(&item)?;
                    item
                }
            };
            added.push(item);
        }
        Ok(added)
    }

    /// Fetch a source again and give each item imported from it the content the repository has
    /// now. Saved values stay; values a manifest names for keys that have none are added. An item
    /// the repository no longer has is kept as it is and reported.
    pub async fn update_source(&self, id: &str) -> Result<Value> {
        let _guard = self.library.lock.lock().await;
        let mut s = self.source(id)?;
        let dir = self.source_dir(id)?;
        let commit = fetch(&dir, &s.clone_url, &s.git_ref).await?;
        let repo = dir.join("repo");
        (s.offers, s.warnings) = offers::discover(&repo, &repo.join(&s.path), ScanMode::SkillsAndMcp);
        let now = now_ms();
        (s.commit, s.latest, s.fetched_at, s.checked_at, s.check_error) =
            (commit.clone(), commit, now, now, String::new());
        self.put_source(&s)?;
        let (mut updated, mut missing) = (vec![], vec![]);
        for mut item in self.items()? {
            let Origin::Git { source, path } = item.origin.clone() else { continue };
            if source != id {
                continue;
            }
            if !s.offers.iter().any(|o| o.path == path && o.kind == item.kind()) {
                missing.push(item.name.clone());
                continue;
            }
            let revision = match &item.body {
                Body::Skill => {
                    let dir = repo.join(&path);
                    item.description = skill::short(&skill::read_meta(&dir)?.description);
                    store::put(&self.store_dir(), &dir)?
                }
                Body::Mcp { .. } => {
                    let f = server_at(&repo, &path)?;
                    let mut values = self.mcp_values(&item.id);
                    let mut named = f.values.clone();
                    for (k, v) in std::mem::take(named.of(&f.server)) {
                        values.of(&f.server).entry(k).or_insert(v);
                    }
                    self.write_values(&item.id, &values.kept_for(&f.server))?;
                    item.description = f.description;
                    let revision = mcp_revision(&f.server);
                    item.body = Body::Mcp { server: f.server };
                    revision
                }
            };
            if revision != item.revision {
                item.revision = revision;
                item.updated_at = now_ms();
                updated.push(item.name.clone());
            }
            self.put_item(&item)?;
        }
        self.sweep_store()?;
        Ok(json!({ "source": self.source_json(&s)?, "updated": updated, "missing": missing }))
    }

    /// Ask each source's remote for its commit without fetching. The answers are recorded under
    /// the library lock, on top of whatever an update wrote meanwhile; a failed check clears the
    /// last known update so the app shows the error instead.
    pub async fn check_sources(&self) -> Result<Value> {
        let mut answers = vec![];
        for s in self.sources()? {
            let target = if s.git_ref.is_empty() { "HEAD".to_string() } else { s.git_ref.clone() };
            let dir = self.source_dir(&s.id)?;
            std::fs::create_dir_all(&dir)?;
            let out = git_in(&dir, &["ls-remote", "--", &s.clone_url, &target]).await;
            answers.push((s.id, target, out.map(|o| remote_head(&o, &s.git_ref))));
        }
        let _guard = self.library.lock.lock().await;
        for (id, target, answer) in answers {
            let Ok(mut s) = self.source(&id) else { continue };
            s.checked_at = now_ms();
            (s.latest, s.check_error) = match answer {
                Ok(Some(sha)) => (sha, String::new()),
                Ok(None) => (String::new(), format!("远端没有 {target}")),
                Err(e) => (String::new(), format!("{e:#}")),
            };
            self.put_source(&s)?;
        }
        let sources: Vec<Value> = self.sources()?.iter().map(|s| self.source_json(s)).collect::<Result<_>>()?;
        Ok(json!({ "sources": sources }))
    }

    /// Delete a source no item still comes from.
    pub async fn delete_source(&self, id: &str) -> Result<()> {
        let _guard = self.library.lock.lock().await;
        let s = self.source(id)?;
        let from = self
            .items()?
            .into_iter()
            .filter(|i| matches!(&i.origin, Origin::Git { source, .. } if source == id))
            .count();
        if from > 0 {
            bail!("资源库中还有 {from} 项资源来自 {}，请先删除它们", s.url);
        }
        self.db.delete_config(SOURCE, id)?;
        let _ = std::fs::remove_dir_all(self.source_dir(id)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_links_in_every_usual_shape() {
        let at = |repo: &str, r: &str, p: &str| RepoAt { repo: repo.into(), git_ref: r.into(), path: p.into() };
        assert_eq!(parse_github("https://github.com/o/r").unwrap(), at("o/r", "", ""));
        assert_eq!(parse_github("git@github.com:o/r.git").unwrap(), at("o/r", "", ""));
        assert_eq!(parse_github("o/r").unwrap(), at("o/r", "", ""));
        assert_eq!(parse_github("github.com/o/r/tree/main/skills/pdf").unwrap(), at("o/r", "main", "skills/pdf"));
        assert_eq!(parse_github("https://github.com/o/r/blob/v1/a/SKILL.md").unwrap(), at("o/r", "v1", "a"));
        assert!(parse_github("https://gitlab.com/o/r").is_err());
        assert!(parse_github("github.com/o/r/tree/main/../x").is_err());
        assert_eq!(at("o/r", "", "a").url(), "https://github.com/o/r/tree/HEAD/a");
    }

    #[test]
    fn the_remote_head_follows_the_ref() {
        let out = "aaa\tHEAD\nbbb\trefs/heads/main\nccc\trefs/tags/v1\nddd\trefs/tags/v1^{}";
        assert_eq!(remote_head(out, "").as_deref(), Some("aaa"));
        assert_eq!(remote_head(out, "main").as_deref(), Some("bbb"));
        assert_eq!(remote_head(out, "v1").as_deref(), Some("ddd"), "an annotated tag is peeled");
        assert_eq!(remote_head(out, "nope"), None);
    }
}
