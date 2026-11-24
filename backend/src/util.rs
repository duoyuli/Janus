//! Small facts every part of Janus asks for: the time, ids made from it, the person's home.

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
