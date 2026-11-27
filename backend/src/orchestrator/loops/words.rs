//! What the clients say about a Loop, written here once: its schedule in words (and in the shape
//! the Loop form edits), its state as a pill, when it wakes next, the time it asked for itself and
//! how many runs it dispatched. Times stay numbers: each client writes them in its own time zone.
// @project-doc docs/architecture/20261031-frontend.md#loops

use crate::lang::{Lang, tr};
use serde::Serialize;
use serde_json::{Value, json};

/// The minutes and hours the Loop form offers for "every few minutes" and "every few hours".
pub const MINUTE_CHOICES: [u32; 5] = [5, 10, 15, 20, 30];
pub const HOUR_CHOICES: [u32; 7] = [1, 2, 3, 4, 6, 8, 12];

/// A schedule as the Loop form shows it: one of the common shapes, or any other cron expression.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Shape {
    /// Every `minutes` minutes, on the clock.
    Minutes {
        minutes: u32,
    },
    /// Every `hours` hours, on the hour.
    Hours {
        hours: u32,
    },
    Daily {
        hour: u32,
        minute: u32,
    },
    Weekdays {
        hour: u32,
        minute: u32,
    },
    /// On one day of the week, 0 Sunday.
    Weekly {
        day: u32,
        hour: u32,
        minute: u32,
    },
    /// On one day of the month.
    Monthly {
        day: u32,
        hour: u32,
        minute: u32,
    },
    Cron {
        cron: String,
    },
}

/// The shape cron expression `cron` has, `Cron` when it is none of the common ones.
pub fn shape(cron: &str) -> Shape {
    let f: Vec<&str> = cron.split_whitespace().collect();
    let other = Shape::Cron { cron: f.join(" ") };
    if f.len() != 5 || f[3] != "*" {
        return other;
    }
    let (mi, h, d, w) = (f[0], f[1], f[2], f[4]);
    let every = |s: &str| s.strip_prefix("*/").and_then(|n| n.parse::<u32>().ok());
    let num = |s: &str| s.parse::<u32>().ok();
    if let Some(m) = every(mi)
        && h == "*"
        && d == "*"
        && w == "*"
        && MINUTE_CHOICES.contains(&m)
    {
        return Shape::Minutes { minutes: m };
    }
    let Some(minute) = num(mi).filter(|m| *m < 60) else { return other };
    if minute == 0 && h == "*" && d == "*" && w == "*" {
        return Shape::Hours { hours: 1 };
    }
    if minute == 0
        && let Some(x) = every(h)
        && d == "*"
        && w == "*"
        && HOUR_CHOICES.contains(&x)
    {
        return Shape::Hours { hours: x };
    }
    let Some(hour) = num(h).filter(|h| *h < 24) else { return other };
    match (d, w) {
        ("*", "*") => Shape::Daily { hour, minute },
        ("*", "1-5") => Shape::Weekdays { hour, minute },
        ("*", _) => num(w).filter(|x| *x <= 6).map_or(other, |day| Shape::Weekly { day, hour, minute }),
        (_, "*") => num(d).filter(|x| (1..=31).contains(x)).map_or(other, |day| Shape::Monthly { day, hour, minute }),
        _ => other,
    }
}

/// A day of the week, 0 Sunday.
fn weekday(d: u32, l: Lang) -> &'static str {
    const ZH: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    const EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    l.pick(ZH[(d % 7) as usize], EN[(d % 7) as usize])
}

/// The schedule in words: every 30 minutes, weekdays at 09:00, …
pub fn words(s: &Shape, l: Lang) -> String {
    let at = |h: &u32, m: &u32| format!("{h:02}:{m:02}");
    match s {
        Shape::Minutes { minutes } => tr!(l, "每 {minutes} 分钟", "Every {minutes} minutes"),
        Shape::Hours { hours: 1 } => l.pick("每小时", "Every hour").into(),
        Shape::Hours { hours } => tr!(l, "每 {hours} 小时", "Every {hours} hours"),
        Shape::Daily { hour, minute } => tr!(l, "每天 {}", "Every day at {}", at(hour, minute)),
        Shape::Weekdays { hour, minute } => tr!(l, "工作日 {}", "Weekdays at {}", at(hour, minute)),
        Shape::Weekly { day, hour, minute } => {
            tr!(l, "每{} {}", "Every {} at {}", weekday(*day, l), at(hour, minute))
        }
        Shape::Monthly { day, hour, minute } => {
            tr!(l, "每月 {day} 日 {}", "Monthly on day {day} at {}", at(hour, minute))
        }
        Shape::Cron { cron } => tr!(l, "cron：{cron}", "cron: {cron}"),
    }
}

/// Loop `view` (as `App::loop_view` gives it) with what the clients show of it, written in `lang`:
/// `scheduleText` (its schedule in words, or that it wakes only when called), `scheduleShape`
/// (the form's shape of its cron expression; `null` without a schedule), `pill` (its state:
/// working, paused, error or idle), `wakesAt` (when it wakes next, while it sleeps), `asked` (the
/// time it asked for itself and why, when that is what wakes it next) and `dispatchedCount`.
// @project-doc docs/interfaces/20261031-http_api.md#loops
pub fn shown(mut view: Value, lang: Lang) -> Value {
    let l = lang;
    let s = view["status"].clone();
    let cron = view["schedule"]["cron"].as_str().map(shape);
    let (text, tone) = match s["phase"].as_str().unwrap_or_default() {
        "awake" => (l.pick("干活中", "Working"), "blue"),
        "paused" => (l.pick("已暂停", "Paused"), "gray"),
        "failed" => (l.pick("出错了", "Error"), "red"),
        _ => (l.pick("摸鱼中", "Idle"), "gray"),
    };
    let sleeping = s["phase"] == "sleeping";
    let asked = (sleeping && s["nextWakeReason"] == "self" && s["nextWakeAt"].is_i64())
        .then(|| json!({ "at": s["nextWakeAt"], "why": s["nextWakeWhy"].as_str().unwrap_or_default() }));
    let dispatched: i64 = s["dispatched"].as_object().map(|m| m.values().filter_map(Value::as_i64).sum()).unwrap_or(0);
    view["scheduleText"] = json!(match &cron {
        Some(c) => words(c, l),
        None => l.pick("只在你叫它时", "Only when you call it").into(),
    });
    view["scheduleShape"] = json!(cron);
    view["pill"] = json!({ "text": text, "tone": tone });
    view["wakesAt"] = if sleeping { s["nextWakeAt"].clone() } else { Value::Null };
    view["asked"] = json!(asked);
    view["dispatchedCount"] = json!(dispatched);
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_reads_as_the_form_shows_it() {
        let cases = [
            ("*/15 * * * *", Shape::Minutes { minutes: 15 }, "每 15 分钟"),
            ("0 * * * *", Shape::Hours { hours: 1 }, "每小时"),
            ("0 */6 * * *", Shape::Hours { hours: 6 }, "每 6 小时"),
            ("5 9 * * *", Shape::Daily { hour: 9, minute: 5 }, "每天 09:05"),
            ("0 18 * * 1-5", Shape::Weekdays { hour: 18, minute: 0 }, "工作日 18:00"),
            ("30 8 * * 0", Shape::Weekly { day: 0, hour: 8, minute: 30 }, "每周日 08:30"),
            ("0 7 15 * *", Shape::Monthly { day: 15, hour: 7, minute: 0 }, "每月 15 日 07:00"),
            ("*/7 * * * *", Shape::Cron { cron: "*/7 * * * *".into() }, "cron：*/7 * * * *"),
            ("0 9 * 1 *", Shape::Cron { cron: "0 9 * 1 *".into() }, "cron：0 9 * 1 *"),
        ];
        for (cron, want, text) in cases {
            let s = shape(cron);
            assert_eq!(s, want, "{cron}");
            assert_eq!(words(&s, Lang::Zh), text);
        }
        assert_eq!(words(&shape("30 8 * * 2"), Lang::En), "Every Tuesday at 08:30");
        assert_eq!(json!(shape("5 9 * * *")), json!({ "kind": "daily", "hour": 9, "minute": 5 }));
    }

    #[test]
    fn a_loop_says_its_state_and_when_it_wakes() {
        let sleeping = json!({
            "schedule": { "cron": "0 9 * * 1-5" },
            "status": { "phase": "sleeping", "nextWakeAt": 100, "nextWakeReason": "self", "nextWakeWhy": "再看 CI",
                        "dispatched": { "running": 1, "delivered": 2 } },
        });
        let v = shown(sleeping, Lang::Zh);
        assert_eq!(v["scheduleText"], "工作日 09:00");
        assert_eq!(v["pill"], json!({ "text": "摸鱼中", "tone": "gray" }));
        assert_eq!((v["wakesAt"].as_i64(), v["dispatchedCount"].as_i64()), (Some(100), Some(3)));
        assert_eq!(v["asked"], json!({ "at": 100, "why": "再看 CI" }));
        let paused = shown(json!({ "schedule": null, "status": { "phase": "paused", "nextWakeAt": 5 } }), Lang::En);
        assert_eq!(paused["scheduleText"], "Only when you call it");
        assert!(paused["scheduleShape"].is_null() && paused["wakesAt"].is_null() && paused["asked"].is_null());
        assert_eq!(paused["pill"]["text"], "Paused");
    }
}
