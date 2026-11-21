//! Skills' files, kept by the sha256 of their content under `data/library/skills/<hash>/` in Janus home. A
//! stored tree never changes: an edit or an update is a new tree, and a run copies the tree it
//! uses into its own bundle.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The most one skill may hold.
pub(super) const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_FILES: usize = 2000;
const MAX_DEPTH: usize = 16;

/// The regular files of a skill folder by relative path, sorted. Hidden entries (`.git`,
/// `.DS_Store`) and `node_modules` are left out; a symbolic link counts only when it leads to a
/// file or folder inside `root`, so nothing outside the chosen folder is ever read.
pub(super) fn files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    fn walk(root: &Path, dir: &Path, rel: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let Ok(real) = e.path().canonicalize() else { continue };
            if !real.starts_with(root) {
                continue;
            }
            let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if real.is_dir() {
                walk(root, &real, &path, depth + 1, out)?;
            } else if real.is_file() {
                out.push((path, real));
                if out.len() > MAX_FILES {
                    bail!("Skill 的文件超过 {MAX_FILES} 个");
                }
            }
        }
        Ok(())
    }
    let root = root.canonicalize().context("Skill 文件夹不存在")?;
    let mut out = vec![];
    walk(&root, &root, "", 0, &mut out)?;
    let total: u64 = out.iter().map(|(_, p)| p.metadata().map(|m| m.len()).unwrap_or(0)).sum();
    if total > MAX_BYTES {
        bail!("Skill 超过 20MB");
    }
    out.sort();
    Ok(out)
}

/// The content hash of a set of files: each relative path, its length and its bytes, in order.
pub(super) fn hash(files: &[(String, PathBuf)]) -> Result<String> {
    let mut h = Sha256::new();
    for (rel, real) in files {
        let bytes = std::fs::read(real)?;
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Store a skill folder and return its hash. Storing the same content twice keeps one copy.
pub(super) fn put(store: &Path, src: &Path) -> Result<String> {
    let files = files(src)?;
    if !files.iter().any(|(rel, _)| rel == "SKILL.md") {
        bail!("这个文件夹里没有 SKILL.md");
    }
    let hash = hash(&files)?;
    let dest = store.join(&hash);
    if dest.is_dir() {
        return Ok(hash);
    }
    std::fs::create_dir_all(store)?;
    let tmp = store.join(format!(".tmp-{hash}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    for (rel, real) in &files {
        let to = tmp.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&tmp))?;
        std::fs::copy(real, &to)?;
    }
    if std::fs::rename(&tmp, &dest).is_err() {
        // Another store of the same content got there first.
        let _ = std::fs::remove_dir_all(&tmp);
        if !dest.is_dir() {
            bail!("保存 Skill 失败");
        }
    }
    Ok(hash)
}

/// Store a skill that exists only as text (written in the editor).
pub(super) fn put_text(store: &Path, files: &[(&str, &str)]) -> Result<String> {
    std::fs::create_dir_all(store)?;
    let tmp = store.join(format!(".new-{}-{}", std::process::id(), crate::orchestrator::now_ms()));
    let _ = std::fs::remove_dir_all(&tmp);
    for (rel, text) in files {
        let to = tmp.join(rel);
        std::fs::create_dir_all(to.parent().unwrap_or(&tmp))?;
        std::fs::write(to, text)?;
    }
    let out = put(store, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

/// Copy a stored tree to `dest` as plain files: whatever an agent does to the copy stays there.
/// On APFS `fs::copy` clones (`fclonefileat`), so a bundle per member costs no disk until an agent
/// writes to it, however many skills are for everyone.
pub(super) fn copy_out(store: &Path, hash: &str, dest: &Path) -> Result<()> {
    let src = store.join(hash);
    for (rel, real) in files(&src).with_context(|| format!("资源库中缺少 Skill 内容 {hash}"))? {
        let to = dest.join(&rel);
        std::fs::create_dir_all(to.parent().unwrap_or(dest))?;
        std::fs::copy(real, to)?;
    }
    Ok(())
}

/// Remove stored trees no item uses any more.
pub(super) fn sweep(store: &Path, keep: &HashSet<String>) {
    for e in std::fs::read_dir(store).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with('.') && !keep.contains(&name) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("janus-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The same files give the same hash wherever they sit; any change gives another one.
    #[test]
    fn content_decides_the_hash() {
        let root = dir("hash");
        let (a, b, store) = (root.join("a"), root.join("b"), root.join("store"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d.join("refs")).unwrap();
            std::fs::write(d.join("SKILL.md"), "---\nname: x\n---\nbody").unwrap();
            std::fs::write(d.join("refs/notes.md"), "notes").unwrap();
        }
        let ha = put(&store, &a).unwrap();
        assert_eq!(ha, put(&store, &b).unwrap(), "same content, same tree");
        std::fs::write(b.join("refs/notes.md"), "changed").unwrap();
        let hb = put(&store, &b).unwrap();
        assert_ne!(ha, hb);
        assert!(store.join(&ha).join("refs/notes.md").is_file());
        sweep(&store, &HashSet::from([hb.clone()]));
        assert!(!store.join(&ha).exists() && store.join(&hb).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Links leading out of the folder, hidden files and `node_modules` are not stored.
    #[test]
    fn only_the_folders_own_files_are_kept() {
        let root = dir("links");
        let (skill, outside) = (root.join("skill"), root.join("outside"));
        std::fs::create_dir_all(skill.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(skill.join("SKILL.md"), "s").unwrap();
        std::fs::write(skill.join(".env"), "SECRET=1").unwrap();
        std::fs::write(skill.join("node_modules/x/i.js"), "x").unwrap();
        std::fs::write(outside.join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), skill.join("leak.txt")).unwrap();
        std::os::unix::fs::symlink(skill.join("SKILL.md"), skill.join("again.md")).unwrap();
        let names: Vec<String> = files(&skill).unwrap().into_iter().map(|(r, _)| r).collect();
        assert_eq!(names, vec!["SKILL.md", "again.md"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
