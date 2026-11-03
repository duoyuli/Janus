//! Images the person pastes into a goal or a message.
//!
//! The app sends them base64 with the goal or the message. They are stored in
//! `<home>/runs/<run>/attachments/`, and the text the agent receives names each one on a line of
//! its own (`[附图 N] <path>`). That text is what every delivery path already carries (steering,
//! queued turns, notes for the next session, restored messages after a restart), so the images
//! travel with it. When a prompt goes out, each named attachment is added as an ACP image block if
//! the agent declares `promptCapabilities.image`; the path stays in the text either way, so an
//! agent without image input can still open the file.

// @project-doc docs/domains/run_lifecycle.md#attachments
use crate::orchestrator::now_ms;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// At most this many images in one goal or message, each at most this large after decoding.
pub const MAX_IMAGES: usize = 10;
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// How a line naming an attachment starts: `[附图 1] /path/to/file.png`.
const MARKER: &str = "[附图 ";

/// The image type, from the file's first bytes. The declared type is not trusted.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

pub fn dir(data_dir: &Path, run_id: &str) -> PathBuf {
    data_dir.join("runs").join(run_id).join("attachments")
}

/// Decode and store the images of one goal or message. Returns, for each, `{path, mimeType,
/// name, size}`; nothing is stored when any of them is refused.
pub fn save(data_dir: &Path, run_id: &str, images: &[Value]) -> Result<Vec<Value>> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if images.len() > MAX_IMAGES {
        bail!("一次最多附 {MAX_IMAGES} 张图片");
    }
    let mut decoded = vec![];
    for (i, img) in images.iter().enumerate() {
        let data = img["data"].as_str().unwrap_or("").trim();
        // A data URL is accepted as well as bare base64.
        let data = data.split_once(";base64,").map_or(data, |(_, d)| d);
        let bytes = STANDARD.decode(data).with_context(|| format!("第 {} 张图片的数据不是有效的 base64", i + 1))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            bail!("第 {} 张图片超过 {} MB", i + 1, MAX_IMAGE_BYTES / 1024 / 1024);
        }
        let (mime, ext) = sniff(&bytes).with_context(|| format!("第 {} 张图片不是 PNG、JPEG、GIF 或 WebP", i + 1))?;
        let name: String = img["name"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(80).collect();
        decoded.push((bytes, mime, ext, name));
    }
    if decoded.is_empty() {
        return Ok(vec![]);
    }
    let dir = dir(data_dir, run_id);
    std::fs::create_dir_all(&dir)?;
    let mut out = vec![];
    for (bytes, mime, ext, name) in decoded {
        let path = dir.join(format!("{}-{}.{ext}", now_ms(), NEXT.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(&path, &bytes)?;
        out.push(json!({ "path": path, "mimeType": mime, "name": name, "size": bytes.len() }));
    }
    Ok(out)
}

/// The lines naming stored images, `[附图 N] <path>`.
pub fn lines(saved: &[Value]) -> String {
    saved
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{MARKER}{}] {}", i + 1, s["path"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text an agent receives for a message: the person's words, then a line per image.
pub fn with_lines(text: &str, saved: &[Value]) -> String {
    if saved.is_empty() {
        return text.to_string();
    }
    if text.is_empty() {
        format!("用户发来了 {} 张图片，请先查看：\n{}", saved.len(), lines(saved))
    } else {
        format!("{text}\n\n附图：\n{}", lines(saved))
    }
}

/// Whether `path` is a file this module stored: `<home>/runs/<run>/attachments/<file>`.
fn is_attachment(runs: &Path, path: &Path) -> bool {
    let (Ok(runs), Ok(path)) = (runs.canonicalize(), path.canonicalize()) else { return false };
    let Ok(rest) = path.strip_prefix(&runs) else { return false };
    let parts: Vec<_> = rest.components().collect();
    parts.len() == 3 && parts[1].as_os_str() == "attachments" && path.is_file()
}

/// ACP content for one prompt: the text, then every attachment the text names, as image blocks
/// when the agent takes images. Only files under `<home>/runs/*/attachments/` are read, so text
/// that merely looks like a marker cannot make Janus send an arbitrary file.
pub fn prompt_blocks(data_dir: &Path, text: &str, images: bool) -> Vec<Value> {
    let mut out = vec![json!({ "type": "text", "text": text })];
    if !images {
        return out;
    }
    let runs = data_dir.join("runs");
    let mut seen = HashSet::new();
    for line in text.lines() {
        let Some((_, path)) = line.trim().strip_prefix(MARKER).and_then(|r| r.split_once("] ")) else { continue };
        let path = PathBuf::from(path.trim());
        if !seen.insert(path.clone()) || !is_attachment(&runs, &path) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Some((mime, _)) = sniff(&bytes) else { continue };
        out.push(json!({ "type": "image", "mimeType": mime, "data": STANDARD.encode(&bytes) }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn images_are_stored_named_and_sent_only_from_attachments() {
        let home = std::env::temp_dir().join(format!("janus-attachments-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let data = STANDARD.encode(PNG);
        let saved = save(&home, "r1", &[json!({ "data": format!("data:image/png;base64,{data}"), "name": "截图.png", "mimeType": "image/jpeg" })]).unwrap();
        assert_eq!(saved[0]["mimeType"], "image/png", "the type comes from the bytes");
        assert_eq!(saved[0]["name"], "截图.png");
        let path = saved[0]["path"].as_str().unwrap();
        assert!(path.contains("/runs/r1/attachments/"), "{path}");

        let text = with_lines("看看这个报错", &saved);
        assert_eq!(text, format!("看看这个报错\n\n附图：\n[附图 1] {path}"));
        assert!(with_lines("", &saved).starts_with("用户发来了 1 张图片"));

        let blocks = prompt_blocks(&home, &text, true);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["text"], text);
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["mimeType"], "image/png");
        assert_eq!(blocks[1]["data"], data);
        assert_eq!(prompt_blocks(&home, &text, false).len(), 1, "an agent without image input gets the path only");
        let twice = format!("{text}\n{text}");
        assert_eq!(prompt_blocks(&home, &twice, true).len(), 2, "one block per file");

        // A marker naming a file outside the attachments folders is ignored.
        let other = home.join("secret.png");
        std::fs::write(&other, PNG).unwrap();
        assert_eq!(prompt_blocks(&home, &format!("[附图 1] {}", other.display()), true).len(), 1);
        let sneaky = format!("[附图 1] {}/runs/r1/attachments/../../../secret.png", home.display());
        assert_eq!(prompt_blocks(&home, &sneaky, true).len(), 1);

        // Refused: not an image, too many. Nothing is stored for a refused message.
        assert!(save(&home, "r2", &[json!({ "data": STANDARD.encode(b"hello") })]).is_err());
        assert!(save(&home, "r2", &[json!({ "data": "%%%" })]).is_err());
        let many: Vec<Value> = (0..=MAX_IMAGES).map(|_| json!({ "data": data })).collect();
        assert!(save(&home, "r2", &many).is_err());
        assert!(!dir(&home, "r2").exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}
