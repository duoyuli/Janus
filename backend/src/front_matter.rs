//! Markdown files that start with YAML front matter: a skill's `SKILL.md` and a digital
//! employee's role file. Only what those files use is read: top-level `key: value` lines, quoted
//! or not, and block scalars folded into one line.

/// The YAML between the opening `---` and the next `---` line.
pub fn yaml(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// What follows the front matter's closing `---` line; the whole text when there is none.
pub fn body(text: &str) -> &str {
    let Some(front) = yaml(text) else { return text };
    let rest = &text[3 + front.len() + 4..];
    rest.split_once('\n').map_or("", |(_, b)| b)
}

/// A top-level front matter field, as text. YAML block scalars (`>`, `|`, `>-`) continue on the
/// indented lines that follow and are folded into one line.
pub fn field(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = yaml(text)?.lines().collect();
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

/// A value as written: a double-quoted one with its escapes (`\"`, `\n`) read, a single-quoted
/// one with `''` read as `'`.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && v.starts_with('"')
        && v.ends_with('"')
        && let Ok(s) = serde_json::from_str::<String>(v)
    {
        return s.trim().to_string();
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].replace("''", "'").trim().to_string();
    }
    v.trim_matches(['"', '\'']).trim().to_string()
}

/// A value for a `key: value` line that reads back as written (`field`): in double quotes, with
/// line breaks and quotes escaped.
pub fn quote(v: &str) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_back_as_written() {
        let text = "---\nname: \"pdf\"\ndescription: >\n  Read and\n  fill PDFs.\nnote: 'it''s'\n---\nbody\nmore";
        assert_eq!(field(text, "name").as_deref(), Some("pdf"));
        assert_eq!(field(text, "description").as_deref(), Some("Read and fill PDFs."));
        assert_eq!(field(text, "note").as_deref(), Some("it's"));
        assert_eq!(field(text, "license"), None);
        assert_eq!(body(text), "body\nmore");
        assert_eq!(body("no front matter"), "no front matter");
        let odd = "say \"hi\": now\nthen # not a comment";
        let written = format!("---\nx: {}\n---\n", quote(odd));
        assert_eq!(field(&written, "x").as_deref(), Some(odd));
    }
}
