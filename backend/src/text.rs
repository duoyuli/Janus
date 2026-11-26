//! Text cut to fit where it goes: a line of a list, an event's summary, a commit in a message.
//! One rule everywhere: lengths are in characters, and a cut is marked with `…`.

/// `s` trimmed, at most `n` characters of it, marked with `…` when cut.
pub fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    format!("{}…", cut.trim_end())
}

/// `s` on one line (every run of whitespace a single space), clipped to `n` characters.
pub fn one_line(s: &str, n: usize) -> String {
    clip(&s.split_whitespace().collect::<Vec<_>>().join(" "), n)
}

/// The first line of a reply with any text, for a one-line summary of it.
pub fn first_line(text: &str) -> String {
    clip(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("（空回复）"), 120)
}

/// A commit's short form, as people read it in messages.
pub fn short_sha(commit: &str) -> &str {
    commit.get(..8).unwrap_or(commit)
}

/// A duration in words: seconds under a minute, minutes after that.
pub fn span(ms: i64) -> String {
    if ms < 60_000 { format!("{} 秒", ms / 1000) } else { format!("{} 分钟", ms / 60_000) }
}

/// A name made for a file or an id: lowercase ASCII letters, digits and single `-`, at most `max`
/// characters. None when nothing usable is left (a name in another script).
pub fn slug(raw: &str, max: usize) -> Option<String> {
    let mut out = String::new();
    for c in raw.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(max).collect();
    let out = out.trim_end_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_are_counted_in_characters_and_marked() {
        assert_eq!(clip("  短  ", 4), "短");
        assert_eq!(clip("一二三四五", 3), "一二三…");
        assert_eq!(clip("ab  cd", 3), "ab…");
        assert_eq!(one_line("a\n  b\tc", 10), "a b c");
        assert_eq!(first_line("\n\n  结论在这\n其余"), "结论在这");
        assert_eq!(first_line(""), "（空回复）");
        assert_eq!(short_sha("0123456789"), "01234567");
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(span(59_000), "59 秒");
        assert_eq!(span(120_000), "2 分钟");
    }
}
