//! Discover resources under a local folder or a Git checkout before anything is imported.

use super::{ItemKind, skill, store};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 8;
const MAX_OFFERS: usize = 500;

/// A skill folder found under a folder or a Git checkout. MCP servers are added by hand or by
/// pasting their config, not discovered (see docs/domains/20261116-resource_library.md#sources).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub kind: ItemKind,
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Offer paths are relative to `root`. A skill's subfolders hold its material, so discovery
/// stops at its SKILL.md. It does not follow symbolic links.
#[derive(Default)]
struct Scan {
    skills: Vec<String>,
    depth_limited: bool,
    count_limited: bool,
}

impl Scan {
    fn add(&mut self, path: String) {
        if self.skills.len() == MAX_OFFERS {
            self.count_limited = true;
        } else {
            self.skills.push(path);
        }
    }

    fn walk(&mut self, root: &Path, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH {
            self.depth_limited = true;
            return;
        }
        let rel = |p: &Path| p.strip_prefix(root).unwrap_or(Path::new("")).to_string_lossy().into_owned();
        if std::fs::symlink_metadata(dir.join("SKILL.md")).is_ok_and(|m| m.is_file()) {
            self.add(rel(dir));
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
            if self.count_limited {
                break;
            }
            self.walk(root, &s, depth + 1);
        }
    }
}

fn scan(root: &Path, start: &Path) -> Scan {
    let mut result = Scan::default();
    if std::fs::symlink_metadata(start).is_ok_and(|m| m.is_dir())
        && root.canonicalize().ok().zip(start.canonicalize().ok()).is_some_and(|(r, s)| s.starts_with(r))
    {
        result.walk(root, start, 0);
    }
    result
}

/// Recheck which local paths are eligible when the person submits their selection.
pub(super) fn skill_paths(root: &Path) -> Vec<String> {
    scan(root, root).skills
}

pub(super) fn discover(root: &Path, start: &Path) -> (Vec<Offer>, Vec<String>) {
    let scan = scan(root, start);
    let mut out = vec![];
    let mut warnings = vec![];
    if scan.count_limited {
        warnings.push(format!("已达到 {MAX_OFFERS} 个候选路径，部分资源未列出。请选择更具体的子目录。"));
    }
    if scan.depth_limited {
        warnings.push(format!("只扫描到第 {MAX_DEPTH} 层，部分子目录未扫描。请选择更具体的子目录。"));
    }
    for rel in scan.skills {
        let dir = root.join(&rel);
        let folder = dir.file_name().unwrap_or_default().to_string_lossy();
        let mut text = String::new();
        if std::fs::File::open(dir.join("SKILL.md"))
            .and_then(|f| f.take(store::MAX_BYTES).read_to_string(&mut text))
            .is_err()
        {
            text.clear();
        }
        let meta = skill::parse_meta(&text, &folder);
        out.push(Offer {
            kind: ItemKind::Skill,
            path: rel,
            name: meta.name,
            description: skill::short(&meta.description),
        });
    }
    (out, warnings)
}
