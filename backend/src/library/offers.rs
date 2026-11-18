//! Discover resources under a local folder or a Git checkout before anything is imported.

use super::{ItemKind, mcp, skill, store};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 8;
const MAX_OFFERS: usize = 500;

#[derive(Clone, Copy)]
pub(super) enum ScanMode {
    Skills,
    SkillsAndMcp,
}

/// A skill folder, or a server identified by `<manifest>#<name>`.
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
fn scan(root: &Path, start: &Path, mode: ScanMode) -> (Vec<String>, Vec<String>) {
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        mode: ScanMode,
        skills: &mut Vec<String>,
        manifests: &mut Vec<String>,
    ) {
        if depth > MAX_DEPTH || skills.len() + manifests.len() >= MAX_OFFERS {
            return;
        }
        let rel = |p: &Path| p.strip_prefix(root).unwrap_or(Path::new("")).to_string_lossy().into_owned();
        if std::fs::symlink_metadata(dir.join("SKILL.md")).is_ok_and(|m| m.is_file()) {
            skills.push(rel(dir));
            return;
        }
        if matches!(mode, ScanMode::SkillsAndMcp) {
            for name in ["mcp.json", ".mcp.json"] {
                if std::fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.is_file()) {
                    manifests.push(rel(&dir.join(name)));
                }
            }
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
            walk(root, &s, depth + 1, mode, skills, manifests);
        }
    }
    let (mut skills, mut manifests) = (vec![], vec![]);
    if std::fs::symlink_metadata(start).is_ok_and(|m| m.is_dir())
        && root.canonicalize().ok().zip(start.canonicalize().ok()).is_some_and(|(r, s)| s.starts_with(r))
    {
        walk(root, start, 0, mode, &mut skills, &mut manifests);
    }
    (skills, manifests)
}

/// Recheck which local paths are eligible when the person submits their selection.
pub(super) fn skill_paths(root: &Path) -> Vec<String> {
    scan(root, root, ScanMode::Skills).0
}

pub(super) fn discover(root: &Path, start: &Path, mode: ScanMode) -> (Vec<Offer>, Vec<String>) {
    let (skills, manifests) = scan(root, start, mode);
    let mut out = vec![];
    let mut warnings = vec![];
    for rel in skills {
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
    for rel in manifests {
        match std::fs::read_to_string(root.join(&rel)).context("读取失败").and_then(|t| mcp::parse_config(&t)) {
            Ok((found, warns)) => {
                warnings.extend(warns.into_iter().map(|w| format!("{rel}：{w}")));
                out.extend(found.into_iter().map(|f| Offer {
                    kind: ItemKind::Mcp,
                    path: format!("{rel}#{}", f.name),
                    name: f.name,
                    description: f.description,
                }));
            }
            Err(e) => warnings.push(format!("{rel}：{e:#}")),
        }
    }
    (out, warnings)
}
