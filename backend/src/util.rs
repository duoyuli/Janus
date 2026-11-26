//! Small facts every part of Janus asks for: the time, ids made from it, the person's home, and
//! writing a file whole.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Now, in milliseconds since the Unix epoch: the unit every stored time uses.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

/// A path with a leading `~` resolved against `HOME`.
pub fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}{rest}"),
        _ => p.to_string(),
    }
}

/// A fresh id from the clock: `prefix` and the time in base 36. Two made within the same
/// millisecond (a script, a burst of messages) would otherwise be equal, so the value only ever
/// goes up; every id Janus makes from the time comes from here.
pub fn new_id(prefix: &str) -> String {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = now_ms() as u64;
    let mut last = LAST.load(Ordering::SeqCst);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return format!("{prefix}{}", to_base36(next)),
            Err(seen) => last = seen,
        }
    }
}

/// Write `file` whole: into a hidden temporary file beside it, then renamed over it, so whoever
/// reads it meanwhile sees the old content or the new, never half of it.
pub fn write_whole(file: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = file.parent().with_context(|| format!("{} 没有所在目录", file.display()))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.persist(file)?;
    Ok(())
}

/// `n` in base 36, lowercase: run ids are `r` plus the time in it.
pub fn to_base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = vec![];
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

#[cfg(test)]
mod tests {
    /// Ids made in the same millisecond still differ.
    #[test]
    fn ids_made_together_differ() {
        let ids: std::collections::HashSet<String> = (0..500).map(|_| super::new_id("r")).collect();
        assert_eq!(ids.len(), 500);
    }
}
