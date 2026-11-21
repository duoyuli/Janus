//! A skill is a folder with a `SKILL.md` whose front matter gives its name and description.

use anyhow::{Context, Result};
use std::path::Path;

/// What a `SKILL.md` says about its skill.
#[derive(Debug, PartialEq)]
pub(super) struct Meta {
    pub name: String,
    pub description: String,
}

/// Read a skill folder's front matter. A missing name falls back to the folder's name.
pub(super) fn read_meta(dir: &Path) -> Result<Meta> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).context("这个文件夹里没有 SKILL.md")?;
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(parse_meta(&text, &folder))
}

pub(super) fn parse_meta(text: &str, folder: &str) -> Meta {
    let name = field(text, "name").filter(|n| !n.is_empty()).unwrap_or_else(|| folder.to_string());
    Meta { name, description: field(text, "description").unwrap_or_default() }
}

/// A top-level front matter field of a `SKILL.md`, as text. YAML block scalars (`>`, `|`, `>-`)
/// continue on the indented lines that follow and are folded into one line.
pub(super) fn field(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = front_matter(text)?.lines().collect();
    let prefix = format!("{key}:");
    let i = lines.iter().position(|l| l.starts_with(&prefix))?;
    let value = unquote(&lines[i][prefix.len()..]);
    if !matches!(value.as_str(), ">" | "|" | ">-" | "|-" | ">+" | "|+") {
        return Some(value);
    }
    let block: Vec<&str> =
        lines[i + 1..].iter().take_while(|l| l.starts_with(' ') || l.is_empty()).map(|l| l.trim()).collect();
    Some(block.join(" ").trim().to_string())
}

fn unquote(v: &str) -> String {
    v.trim().trim_matches(['"', '\'']).trim().to_string()
}

/// The YAML between the opening `---` and the next `---` line.
fn front_matter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// A name every agent takes for a skill: lowercase ASCII letters, digits and `-`, at most 64.
/// None when nothing usable is left (a name in another script).
pub(super) fn skill_name(raw: &str) -> Option<String> {
    let mut out = String::new();
    for c in raw.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(64).collect();
    let out = out.trim_end_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

/// The `SKILL.md` an agent is given: the library's name in the front matter (the library may have
/// renamed the skill so that names stay unique), and a description when the file has none.
pub(super) fn with_identity(text: &str, name: &str, description: &str) -> String {
    let name_line = format!("name: {name}");
    let desc_line = format!("description: {}", serde_json::to_string(description).unwrap_or_default());
    let Some(front) = front_matter(text) else {
        return format!("---\n{name_line}\n{desc_line}\n---\n\n{text}");
    };
    let body = &text[3 + front.len()..];
    let mut lines: Vec<String> =
        front.lines().map(|l| if l.starts_with("name:") { name_line.clone() } else { l.to_string() }).collect();
    if !lines.iter().any(|l| l.starts_with("name:")) {
        lines.insert(usize::from(lines.first().is_some_and(String::is_empty)), name_line);
    }
    if !lines.iter().any(|l| l.starts_with("description:")) {
        lines.push(desc_line);
    }
    format!("---{}{body}", lines.join("\n"))
}

/// A short form of a description for lists.
pub(super) fn short(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = s.chars().take(240).collect();
    if cut.chars().count() < s.chars().count() { format!("{cut}…") } else { cut }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fit_every_agent() {
        assert_eq!(skill_name("Release Notes!").as_deref(), Some("release-notes"));
        assert_eq!(skill_name("pdf_tools").as_deref(), Some("pdf-tools"));
        assert_eq!(skill_name("密码学"), None);
        assert_eq!(skill_name(&"a".repeat(80)).map(|n| n.len()), Some(64));
    }

    #[test]
    fn front_matter_gives_name_and_folded_description() {
        let m = parse_meta("---\nname: \"pdf\"\ndescription: >\n  Read and\n  fill PDFs.\n---\nbody", "folder");
        assert_eq!(m, Meta { name: "pdf".into(), description: "Read and fill PDFs.".into() });
        assert_eq!(parse_meta("no front matter", "folder").name, "folder");
        let text = "---\nname: pdf\nwhen_to_use: |\n  A PDF is\n  attached.\ndisable-model-invocation: true\n---\n";
        assert_eq!(field(text, "when_to_use").as_deref(), Some("A PDF is attached."));
        assert_eq!(field(text, "disable-model-invocation").as_deref(), Some("true"));
        assert_eq!(field(text, "license"), None);
    }

    /// The agent sees the library's name; the rest of the file stays as written.
    #[test]
    fn the_agent_gets_the_librarys_name() {
        let text = "---\nname: pdf\ndescription: Read PDFs.\nlicense: MIT\n---\n\nBody\n";
        assert_eq!(
            with_identity(text, "pdf-2", "x"),
            "---\nname: pdf-2\ndescription: Read PDFs.\nlicense: MIT\n---\n\nBody\n"
        );
        let bare = with_identity("Body", "notes", "Take \"notes\".");
        assert_eq!(bare, "---\nname: notes\ndescription: \"Take \\\"notes\\\".\"\n---\n\nBody");
        let unnamed = with_identity("---\ndescription: d\n---\nBody", "n", "d");
        assert_eq!(unnamed, "---\nname: n\ndescription: d\n---\nBody");
    }
}
