//! A Loop's schedule: a standard five-field cron expression (minute, hour, day of month, month,
//! day of week) read in a named time zone. Times are wall-clock times in that zone, so `0 9 * * 1-5`
//! is nine in the morning on weekdays there, also across a change of daylight saving time.
// @project-doc docs/domains/20261121-loops.md#schedule

use anyhow::{Context, Result, bail};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

/// When a Loop wakes by itself, as stored with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub cron: String,
    /// An IANA time zone name, e.g. `Asia/Shanghai`.
    pub tz: String,
}

/// How far ahead `next_after` looks for a time that matches: long enough for 29 February.
const SEARCH_DAYS: i32 = 8 * 366;

impl Schedule {
    /// A schedule from what a request gave, checked: the expression parses and comes round at
    /// least once. Without a zone it is the service's own.
    pub fn new(cron: &str, tz: Option<&str>) -> Result<Schedule> {
        let cron = cron.split_whitespace().collect::<Vec<_>>().join(" ");
        let tz = match tz.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => t.to_string(),
            None => local_zone(),
        };
        let s = Schedule { cron, tz };
        let (fields, zone) = s.parsed()?;
        if next(&fields, &zone, Timestamp::now()).is_none() {
            bail!("这个时间表永远不会到点：{}", s.cron);
        }
        Ok(s)
    }

    /// The first time after `after_ms` (milliseconds since the epoch) it comes round.
    pub fn next_after(&self, after_ms: i64) -> Option<i64> {
        let (fields, zone) = self.parsed().ok()?;
        let after = Timestamp::from_millisecond(after_ms).ok()?;
        next(&fields, &zone, after).map(|t| t.as_millisecond())
    }

    /// The next `n` times after `after_ms` it comes round.
    pub fn upcoming(&self, after_ms: i64, n: usize) -> Vec<i64> {
        let mut out = vec![];
        let mut at = after_ms;
        while out.len() < n {
            let Some(t) = self.next_after(at) else { break };
            out.push(t);
            at = t;
        }
        out
    }

    /// A time as the schedule's zone writes it, e.g. `2026-11-22T09:00:00+08:00[Asia/Shanghai]`.
    pub fn local(&self, ms: i64) -> String {
        match (Timestamp::from_millisecond(ms), TimeZone::get(&self.tz)) {
            (Ok(t), Ok(zone)) => t.to_zoned(zone).to_string(),
            _ => crate::config::utc(ms),
        }
    }

    fn parsed(&self) -> Result<(Fields, TimeZone)> {
        let fields = Fields::parse(&self.cron)?;
        let zone = TimeZone::get(&self.tz).with_context(|| format!("没有这个时区：{}", self.tz))?;
        Ok((fields, zone))
    }
}

/// The service's time zone name, `UTC` when the system's has none.
pub fn local_zone() -> String {
    TimeZone::system().iana_name().unwrap_or("UTC").to_string()
}

/// The values each field allows, as bit sets.
struct Fields {
    minutes: u64,
    hours: u64,
    days: u64,
    months: u64,
    weekdays: u64,
    /// Whether day of month and day of week were both restricted: then a day matches either (as in
    /// cron), otherwise both.
    either_day: bool,
}

const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

impl Fields {
    fn parse(cron: &str) -> Result<Fields> {
        let parts: Vec<&str> = cron.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = parts[..] else {
            bail!("cron 要有 5 段（分 时 日 月 星期），现在是 {} 段：{cron}", parts.len());
        };
        let mut weekdays = field(weekday, 0, 7, &WEEKDAYS, 0).context("星期（第 5 段）")?;
        // 7 is Sunday too.
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays | 1) & !(1 << 7);
        }
        Ok(Fields {
            minutes: field(minute, 0, 59, &[], 0).context("分钟（第 1 段）")?,
            hours: field(hour, 0, 23, &[], 0).context("小时（第 2 段）")?,
            days: field(day, 1, 31, &[], 1).context("日（第 3 段）")?,
            months: field(month, 1, 12, &MONTHS, 1).context("月（第 4 段）")?,
            weekdays,
            either_day: !day.starts_with('*') && !weekday.starts_with('*'),
        })
    }

    fn day_matches(&self, d: Date) -> bool {
        let dom = self.days & (1 << d.day()) != 0;
        let dow = self.weekdays & (1 << d.weekday().to_sunday_zero_offset()) != 0;
        if self.either_day { dom || dow } else { dom && dow }
    }
}

/// One field: `*`, a value, a range `a-b`, each with an optional step `/n`, separated by commas.
/// `names` are the words a value may be written as, the first standing for `first`.
fn field(text: &str, min: u32, max: u32, names: &[&str], first: u32) -> Result<u64> {
    let value = |s: &str| -> Result<u32> {
        if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(s)) {
            return Ok(first + i as u32);
        }
        let v: u32 = s.parse().with_context(|| format!("「{s}」不是数字"))?;
        if !(min..=max).contains(&v) {
            bail!("{v} 不在 {min} 到 {max} 之间");
        }
        Ok(v)
    };
    let mut bits = 0u64;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s.parse().with_context(|| format!("步长「{s}」不是数字"))?;
                if step == 0 {
                    bail!("步长不能是 0");
                }
                (r, step)
            }
            None => (item, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a)?, value(b)?)
        } else {
            let v = value(range)?;
            // `a/n` runs from a to the end.
            (v, if item.contains('/') { max } else { v })
        };
        if lo > hi {
            bail!("范围 {range} 的起点大于终点");
        }
        for v in (lo..=hi).step_by(step as usize) {
            bits |= 1 << v;
        }
    }
    Ok(bits)
}

/// The first wall-clock minute after `after` in `zone` that the fields allow. A time that a change
/// of daylight saving time skips comes as the hour after it; one it repeats comes once.
fn next(f: &Fields, zone: &TimeZone, after: Timestamp) -> Option<Timestamp> {
    let start = after.to_zoned(zone.clone()).date();
    let mut day = start;
    for _ in 0..SEARCH_DAYS {
        if f.months & (1 << day.month()) != 0 && f.day_matches(day) {
            for h in (0..24).filter(|h| f.hours & (1 << h) != 0) {
                for m in (0..60).filter(|m| f.minutes & (1 << m) != 0) {
                    let at = day.at(h as i8, m as i8, 0, 0).to_zoned(zone.clone()).ok()?.timestamp();
                    if at > after {
                        return Some(at);
                    }
                }
            }
        }
        day = day.checked_add(1.day()).ok()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> i64 {
        s.parse::<jiff::Zoned>().unwrap().timestamp().as_millisecond()
    }

    fn sh(cron: &str) -> Schedule {
        Schedule::new(cron, Some("Asia/Shanghai")).unwrap()
    }

    #[test]
    fn it_comes_round_on_the_wall_clock_of_its_zone() {
        let now = at("2026-11-21T08:59:30+08:00[Asia/Shanghai]");
        assert_eq!(sh("0 9 * * *").next_after(now), Some(at("2026-11-21T09:00:00+08:00[Asia/Shanghai]")));
        // Strictly after: at nine sharp the next is tomorrow's.
        let nine = at("2026-11-21T09:00:00+08:00[Asia/Shanghai]");
        assert_eq!(sh("0 9 * * *").next_after(nine), Some(at("2026-11-22T09:00:00+08:00[Asia/Shanghai]")));
        // Every 30 minutes is on the hour and the half hour, not counted from now.
        assert_eq!(
            sh("*/30 * * * *").upcoming(at("2026-11-21T10:07:00+08:00[Asia/Shanghai]"), 2),
            vec![at("2026-11-21T10:30:00+08:00[Asia/Shanghai]"), at("2026-11-21T11:00:00+08:00[Asia/Shanghai]")]
        );
        // 2026-11-21 is a Saturday: weekdays at nine is Monday.
        assert_eq!(sh("0 9 * * 1-5").next_after(now), Some(at("2026-11-23T09:00:00+08:00[Asia/Shanghai]")));
        assert_eq!(sh("0 9 * * MON-FRI"), Schedule { cron: "0 9 * * MON-FRI".into(), tz: "Asia/Shanghai".into() });
        assert_eq!(sh("0 9 * * 0").next_after(now), sh("0 9 * * 7").next_after(now));
        assert_eq!(sh("30 8 1 * *").next_after(now), Some(at("2026-12-01T08:30:00+08:00[Asia/Shanghai]")));
        // Day of month and day of week both given: either one.
        assert_eq!(sh("0 9 1 * 1").next_after(now), Some(at("2026-11-23T09:00:00+08:00[Asia/Shanghai]")));
        assert_eq!(sh("0 0 29 2 *").next_after(now), Some(at("2028-02-29T00:00:00+08:00[Asia/Shanghai]")));
        assert_eq!(sh("0 9-18/3,20 * * *").upcoming(now, 6).len(), 6);
    }

    #[test]
    fn daylight_saving_time_keeps_the_wall_clock() {
        let ny = Schedule::new("30 2 * * *", Some("America/New_York")).unwrap();
        // 2026-03-08 skips 02:00–03:00 there: 02:30 comes as 03:30.
        let before = at("2026-03-07T12:00:00-05:00[America/New_York]");
        assert_eq!(
            ny.upcoming(before, 2),
            vec![at("2026-03-08T03:30:00-04:00[America/New_York]"), at("2026-03-09T02:30:00-04:00[America/New_York]")]
        );
        let nine = Schedule::new("0 9 * * *", Some("America/New_York")).unwrap();
        assert_eq!(nine.next_after(before), Some(at("2026-03-08T09:00:00-04:00[America/New_York]")));
    }

    #[test]
    fn what_is_not_a_schedule_is_refused() {
        for (cron, why) in [
            ("0 9 * *", "5 段"),
            ("0 9 * * * *", "5 段"),
            ("60 * * * *", "不在 0 到 59"),
            ("* 24 * * *", "不在 0 到 23"),
            ("*/0 * * * *", "步长"),
            ("5-1 * * * *", "起点大于终点"),
            ("0 9 * * MONDAY", "不是数字"),
            ("0 0 30 2 *", "永远不会到点"),
        ] {
            let err = format!("{:#}", Schedule::new(cron, Some("UTC")).unwrap_err());
            assert!(err.contains(why), "{cron}: {err}");
        }
        let err = Schedule::new("0 9 * * *", Some("Mars/Olympus")).unwrap_err();
        assert!(format!("{err:#}").contains("没有这个时区"));
        assert!(!Schedule::new("0  9 * *  *", None).unwrap().tz.is_empty());
        assert_eq!(Schedule::new("0  9 * *  *", None).unwrap().cron, "0 9 * * *");
    }
}
