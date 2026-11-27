//! Text cut to fit where it goes: a line of a list, an event's summary, a commit in a message;
//! and durations in words. One rule everywhere: lengths are in characters, and a cut is marked
//! with `…` (a tail of output, which is the latest part, is not marked).

use crate::lang::{Lang, tr};

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
    clip(&squash(s), n)
}

/// `s` on one line, every run of whitespace a single space, nothing cut.
pub fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` in at most `n` characters, cut at a word (anywhere in a script without spaces) and
/// marked with `…`: for prose read by an agent, where half a word reads worse than a shorter line.
pub fn cut_at_word(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let c: String = s.chars().take(n.saturating_sub(1)).collect();
    let c = match c.rfind(' ') {
        Some(i) if i > 0 => c[..i].to_string(),
        _ => c,
    };
    format!("{}…", c.trim_end_matches([' ', ',', '，', '、']))
}

/// The end of `s`: at most its last `n` characters, as a terminal shows the latest output.
pub fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    s.chars().skip(count - n).collect()
}

/// The first line of a reply with any text, for a one-line summary of it.
pub fn first_line(text: &str) -> String {
    clip(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("（空回复）"), 120)
}

/// A commit's short form, as people read it in messages.
pub fn short_sha(commit: &str) -> &str {
    commit.get(..8).unwrap_or(commit)
}

/// A rough duration in words: seconds under a minute, whole minutes after that.
pub fn span(ms: i64, lang: Lang) -> String {
    let (s, m) = (ms / 1000, ms / 60_000);
    if ms < 60_000 { tr!(lang, "{s} 秒", "{s} s") } else { tr!(lang, "{m} 分钟", "{m} min") }
}

/// A wall-clock duration to the second, as the app writes it (`L.dur`).
pub fn duration(ms: i64, lang: Lang) -> String {
    if ms < 1000 {
        return lang.pick("不到 1 秒", "under 1 s").into();
    }
    let s = (ms + 500) / 1000;
    let (h, m, sec) = (s / 3600, s % 3600 / 60, s % 60);
    match s {
        0..60 => tr!(lang, "{s} 秒", "{s} s"),
        60..3600 => tr!(lang, "{m} 分 {sec:02} 秒", "{m} min {sec:02} s"),
        _ => tr!(lang, "{h} 小时 {m} 分", "{h} h {m} min"),
    }
}

/// A moment as the person reads it, in this machine's time zone: 「今天 20:16」, 「昨天 …」,
/// 「明天 …」, else the day (with the year when it is not `now`'s), as the app wrote it (`L.dateTime`).
pub fn date_time(ms: i64, now: i64, lang: Lang) -> String {
    use jiff::{Timestamp, tz::TimeZone};
    let zone = TimeZone::system();
    let (Ok(at), Ok(now)) = (Timestamp::from_millisecond(ms), Timestamp::from_millisecond(now)) else {
        return String::new();
    };
    let (at, now) = (at.to_zoned(zone.clone()), now.to_zoned(zone));
    let time = format!("{:02}:{:02}", at.hour(), at.minute());
    let days = (at.date() - now.date()).get_days();
    match days {
        0 => return tr!(lang, "今天 {time}", "Today at {time}"),
        -1 => return tr!(lang, "昨天 {time}", "Yesterday at {time}"),
        1 => return tr!(lang, "明天 {time}", "Tomorrow at {time}"),
        _ => {}
    }
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = (at.year(), at.month(), at.day());
    let mon = MONTHS[(m - 1) as usize];
    if at.year() == now.year() {
        tr!(lang, "{m}月{d}日 {time}", "{mon} {d}, {time}")
    } else {
        tr!(lang, "{y}年{m}月{d}日 {time}", "{mon} {d}, {y}, {time}")
    }
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
        assert_eq!(span(59_000, Lang::Zh), "59 秒");
        assert_eq!(span(120_000, Lang::Zh), "2 分钟");
        assert_eq!(span(120_000, Lang::En), "2 min");
        assert_eq!(squash(" a \n b "), "a b");
        assert_eq!(cut_at_word("one two three four", 10), "one two…");
        assert_eq!(tail("负责人回复", 2), "回复");
        assert_eq!(tail("ok", 5), "ok");
        assert_eq!(duration(192_000, Lang::Zh), "3 分 12 秒");
        assert_eq!(duration(65_000, Lang::Zh), "1 分 05 秒");
        assert_eq!(duration(44_600, Lang::Zh), "45 秒");
        let now = jiff::Timestamp::now().as_millisecond();
        assert!(date_time(now, now, Lang::Zh).starts_with("今天 "));
        assert!(date_time(now - 86_400_000, now, Lang::En).starts_with("Yesterday at "));
        assert_eq!(duration(3_840_000, Lang::Zh), "1 小时 4 分");
        assert_eq!(duration(400, Lang::Zh), "不到 1 秒");
        assert_eq!(duration(65_000, Lang::En), "1 min 05 s");
    }
}
