//! The usage bill of Settings › 用量账单 as a receipt: the period in the person's calendar, the
//! headline (an estimate at list prices, or the token total), the stats, each Agent with its models,
//! the token parts, the notes and the whole bill as text to copy. Built from `bill::period`; the
//! clients draw the lines (`GET /api/usage/receipt`).
// @project-doc docs/architecture/20261031-frontend.md#usage_bill

use crate::lang::{Lang, tr};
use serde_json::{Value, json};

const DAY: i64 = 24 * 60 * 60 * 1000;

/// `1,234,567`.
pub fn num(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `15.4K`, `21.4M`, `6.45B`: three significant digits.
pub fn compact(n: i64) -> String {
    for (size, unit) in [(1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if (n.abs() as f64) >= size {
            let v = n as f64 / size;
            let digits = if v.abs() >= 100.0 {
                0
            } else if v.abs() >= 10.0 {
                1
            } else {
                2
            };
            let mut s = format!("{v:.digits$}");
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            return s + unit;
        }
    }
    num(n)
}

/// `$6,389.62`, `$0.35`, `<$0.01`: an estimate in US dollars.
pub fn usd(v: f64) -> String {
    if v > 0.0 && v < 0.005 {
        return "<$0.01".into();
    }
    let cents = (v * 100.0).round() as i64;
    format!("${}.{:02}", num(cents / 100), cents % 100)
}

/// A reported amount as the agent gave it, to four decimals at most.
fn money(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == r.round() && r.abs() < 1e15 {
        return format!("{}", r as i64);
    }
    format!("{r:.4}").trim_end_matches('0').to_string()
}

/// Each reported amount with its currency and how many sessions it sums; never added across
/// currencies.
fn cost_detail(cost: &Value, lang: Lang) -> Option<String> {
    let lines = cost["lines"].as_array()?;
    if lines.is_empty() {
        return None;
    }
    let parts: Vec<String> = lines
        .iter()
        .map(|l| {
            let (amount, currency, sessions) = (
                money(l["amount"].as_f64().unwrap_or(0.0)),
                l["currency"].as_str().unwrap_or(""),
                l["sessions"].as_i64().unwrap_or(1),
            );
            if currency.is_empty() {
                tr!(lang, "{amount}（未标注币种）", "{amount} (currency unspecified)")
            } else if sessions > 1 {
                tr!(lang, "{amount} {currency}（{sessions} 个会话）", "{amount} {currency} ({sessions} sessions)")
            } else {
                format!("{amount} {currency}")
            }
        })
        .collect();
    Some(parts.join(lang.pick("；", "; ")))
}

/// The days a period covers in the person's calendar (`offset`: minutes east of UTC): `[from, to)`,
/// whole days, the last of them today (or the last day of last month).
pub fn range(period: &str, now: i64, offset: i64) -> Option<(i64, i64)> {
    let shift = offset * 60 * 1000;
    let local = now + shift;
    let today = local.div_euclid(DAY) * DAY;
    let tomorrow = today + DAY;
    let date = jiff::civil::Date::new(1970, 1, 1).ok()?.checked_add(jiff::Span::new().days(today / DAY)).ok()?;
    let month = date.first_of_month();
    let day_ms = |d: jiff::civil::Date| -> Option<i64> {
        Some(d.since(jiff::civil::Date::new(1970, 1, 1).ok()?).ok()?.get_days() as i64 * DAY)
    };
    let (from, to) = match period {
        "week" => (tomorrow - 7 * DAY, tomorrow),
        "days30" => (tomorrow - 30 * DAY, tomorrow),
        "thisMonth" => (day_ms(month)?, tomorrow),
        "lastMonth" => (day_ms(month.checked_sub(jiff::Span::new().months(1)).ok()?)?, day_ms(month)?),
        _ => return None,
    };
    Some((from - shift, to - shift))
}

/// A date in `lang`'s pattern, in the person's calendar: 2026年11月26日, Nov 26, 2026.
fn date(ms: i64, offset: i64, lang: Lang, time: bool) -> String {
    let at = jiff::Timestamp::from_millisecond(ms + offset * 60 * 1000).unwrap_or_default();
    let dt = at.to_zoned(jiff::tz::TimeZone::UTC).datetime();
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = (dt.year(), dt.month(), dt.day());
    let clock = format!("{:02}:{:02}", dt.hour(), dt.minute());
    match (lang, time) {
        (Lang::Zh, false) => format!("{y}年{m}月{d}日"),
        (Lang::Zh, true) => format!("{y}年{m}月{d}日 {clock}"),
        (Lang::En, false) => format!("{} {d}, {y}", MONTHS[(m - 1) as usize]),
        (Lang::En, true) => format!("{} {d}, {y}, {clock}", MONTHS[(m - 1) as usize]),
    }
}

fn row(label: &str, value: String) -> Value {
    json!({ "label": label, "value": value })
}

/// A payer's or a model's amount: the estimate with the tokens beside it when priced, else the
/// tokens; and what hovering it says.
fn amount(estimate: Option<f64>, tokens: &Value, turns: i64, lang: Lang) -> Value {
    let total = tokens["total"].as_i64();
    let tokens_text = total.map_or_else(|| lang.pick("未上报", "Not reported").to_string(), compact);
    let help = total.map_or_else(
        || lang.pick("没有上报 Token", "No tokens reported").to_string(),
        |t| {
            let t = num(t);
            tr!(lang, "{t} tokens · {turns} 轮", "{t} tokens · Turns: {turns}")
        },
    );
    json!({ "tokens": tokens_text, "estimate": estimate.map(usd), "help": help })
}

/// The receipt of `bill` (as `bill::period` returns it), issued at `issued`.
pub fn receipt(bill: &Value, issued: i64, offset: i64, lang: Lang) -> Value {
    let tokens = &bill["tokens"];
    let total = tokens["total"].as_i64();
    let estimate = &bill["estimate"];
    let amount_est = estimate["amount"].as_f64().unwrap_or(0.0);
    let priced = !estimate.is_null() && amount_est > 0.0;
    let not_reported = lang.pick("未上报", "Not reported");
    let headline = if priced { usd(amount_est) } else { total.map_or(not_reported.to_string(), compact) };
    let headline_caption = if priced {
        lang.pick("按 models.dev API 价格折算 · 非账单", "Estimated at models.dev API rates · Not an invoice")
    } else {
        lang.pick("Token 总量", "Total Tokens")
    };
    let cost = cost_detail(&bill["cost"], lang);
    let cost_part = match &cost {
        None => lang.pick("Agent 未上报费用", "No cost reported by agents").to_string(),
        Some(c) => tr!(lang, "Agent 上报费用 {c}", "Agent-reported cost: {c}"),
    };
    let detail: Vec<String> =
        total.map(|t| format!("{} tokens", num(t))).into_iter().chain(std::iter::once(cost_part)).collect();
    let headline_detail = detail.join(" · ");
    let (from, to) = (bill["from"].as_i64().unwrap_or(0), bill["to"].as_i64().unwrap_or(0));
    let meta = vec![
        row(
            lang.pick("区间", "Period"),
            format!("{} – {}", date(from, offset, lang, false), date(to - 1, offset, lang, false)),
        ),
        row(lang.pick("生成于", "Generated"), date(issued, offset, lang, true)),
    ];
    let n = |k: &str| bill[k].as_i64().unwrap_or(0);
    let (runs, sessions, active, days) = (n("runs"), n("sessions"), n("activeDays"), n("days"));
    let turns = num(n("turns"));
    let mut stats = vec![
        row(lang.pick("任务", "Runs"), tr!(lang, "{runs} 个", "{runs}")),
        row(lang.pick("轮次", "Turns"), tr!(lang, "{turns} 轮", "{turns}")),
        row(lang.pick("会话", "Sessions"), tr!(lang, "{sessions} 个", "{sessions}")),
        row(lang.pick("活跃天数", "Active Days"), format!("{active} / {days}")),
    ];
    if let (Some(t), true) = (total, active > 0) {
        stats
            .push(row(lang.pick("活跃日均", "Daily Average (Active Days)"), format!("{} tokens", compact(t / active))));
    }
    let mut notes = vec![];
    if !estimate.is_null() {
        let mut first = lang
            .pick(
                "估算按 models.dev 的 API 价格（不含长上下文加价），不是账单。",
                "Estimated at models.dev API rates, excluding long-context surcharges. This is not an invoice.",
            )
            .to_string();
        if let Some(at) = estimate["fetchedAt"].as_i64() {
            let when = date(at, offset, lang, true);
            first += &tr!(lang, "价格更新于 {when}。", " Prices updated {when}.");
        }
        notes.push(first);
        let unpriced = estimate["unpricedTurns"].as_i64().unwrap_or(0);
        if unpriced > 0 {
            let models: Vec<&str> =
                estimate["unpricedModels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let models = models.join(lang.pick("、", ", "));
            notes.push(tr!(
                lang,
                "{unpriced} 轮没有价格（{models}），没有计入估算。",
                "Turns without pricing were excluded from the estimate: {unpriced} ({models})."
            ));
        }
    }
    let unreported = n("unreportedSessions");
    if unreported > 0 {
        notes.push(tr!(
            lang,
            "{unreported} 个会话没上报 Token，没算进来。",
            "{unreported} sessions reported no tokens and aren't counted."
        ));
    }
    notes.push(lang.pick("Token 总量含缓存读取。", "Total tokens include cache reads.").into());
    let groups: Vec<Value> = bill["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| {
            let cost = (g["cost"]["reportedSessions"].as_i64().unwrap_or(0) > 0)
                .then(|| cost_detail(&g["cost"], lang).unwrap_or_else(|| not_reported.into()));
            json!({
                "name": g["name"], "cost": cost,
                "amount": amount(g["estimate"].as_f64(), &g["tokens"], g["turns"].as_i64().unwrap_or(0), lang),
                "totalText": g["tokens"]["total"].as_i64().map_or(not_reported.to_string(), num),
                "models": g["models"].as_array().into_iter().flatten().map(|m| json!({
                    "model": m["model"],
                    "amount": amount(m["estimate"].as_f64(), &m["tokens"], m["turns"].as_i64().unwrap_or(0), lang),
                    "totalText": m["tokens"]["total"].as_i64().map_or(not_reported.to_string(), num),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut totals = vec![row(lang.pick("Token 总量", "Total Tokens"), total.map_or(not_reported.to_string(), num))];
    for (zh, en, key) in [
        ("输入", "Input", "input"),
        ("缓存读取", "Cache Read", "cachedRead"),
        ("缓存写入", "Cache Write", "cachedWrite"),
        ("输出", "Output", "output"),
    ] {
        if let Some(v) = tokens[key].as_i64().filter(|v| *v > 0) {
            totals.push(row(lang.pick(zh, en), num(v)));
        }
    }
    if let (Some(input), Some(read)) = (tokens["input"].as_i64(), tokens["cachedRead"].as_i64()) {
        let prompt = input + read + tokens["cachedWrite"].as_i64().unwrap_or(0);
        if prompt > 0 {
            totals.push(row(
                lang.pick("缓存命中率", "Cache Hit Rate"),
                format!("{}%", (read as f64 / prompt as f64 * 100.0).round() as i64),
            ));
        }
    }
    let title = lang.pick("用量账单", "Usage");
    let groups_title = lang.pick("Agent / 模型", "Agent / Model");
    // The bill as plain text, for copying.
    let line =
        |r: &Value| tr!(lang, "{}：{}", "{}: {}", r["label"].as_str().unwrap_or(""), r["value"].as_str().unwrap_or(""));
    let mut text: Vec<String> = vec![title.to_string()];
    text.extend(meta.iter().map(line));
    text.push(String::new());
    text.push(tr!(lang, "Token 总量：{headline}（{headline_detail}）", "Total tokens: {headline} ({headline_detail})"));
    text.push(String::new());
    text.extend(stats.iter().map(line));
    if !groups.is_empty() {
        text.push(String::new());
        text.push(groups_title.into());
        for g in &groups {
            let (name, t) = (g["name"].as_str().unwrap_or(""), g["totalText"].as_str().unwrap_or(""));
            let mut l = tr!(lang, "{name}：{t}", "{name}: {t}");
            if let Some(c) = g["cost"].as_str() {
                l += &tr!(lang, "（{c}）", " ({c})");
            }
            text.push(l);
            for m in g["models"].as_array().into_iter().flatten() {
                text.push(
                    format!("  · {}：{}", m["model"].as_str().unwrap_or(""), m["totalText"].as_str().unwrap_or(""))
                        .replace('：', lang.pick("：", ": ")),
                );
            }
        }
    }
    text.push(String::new());
    text.extend(totals.iter().map(line));
    text.push(String::new());
    text.extend(notes.iter().cloned());
    json!({
        "title": title, "meta": meta, "priced": priced, "headline": headline, "headlineCaption": headline_caption,
        "headlineDetail": headline_detail,
        "headlineHelp": total.map_or_else(|| lang.pick("没有会话上报 Token", "No session reported tokens").to_string(), |t| format!("{} tokens", num(t))),
        "stats": stats, "groupsTitle": groups_title,
        "amountTitle": if priced { lang.pick("估算 · Token", "Estimate · Tokens") } else { "TOKEN" },
        "groups": groups, "totals": totals, "notes": notes, "text": text.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang::{En, Zh};

    #[test]
    fn numbers_read_as_the_receipt_writes_them() {
        assert_eq!((num(3_111_893), num(0), num(-1234)), ("3,111,893".into(), "0".into(), "-1,234".into()));
        assert_eq!(
            (compact(15_400), compact(21_400_000), compact(6_450_000_000), compact(999)),
            ("15.4K".into(), "21.4M".into(), "6.45B".into(), "999".into())
        );
        assert_eq!(compact(120_000), "120K");
        assert_eq!(
            (usd(6389.62), usd(0.35), usd(0.001), usd(0.0)),
            ("$6,389.62".into(), "$0.35".into(), "<$0.01".into(), "$0.00".into())
        );
        assert_eq!((money(0.25), money(3.0), money(0.123456)), ("0.25".into(), "3".into(), "0.1235".into()));
    }

    #[test]
    fn a_period_is_whole_days_in_the_persons_calendar() {
        // 2026-11-26 10:00 in UTC+8 is 02:00 UTC.
        let now = jiff::Timestamp::from_second(1_795_658_400).unwrap().as_millisecond();
        let (from, to) = range("week", now, 480).unwrap();
        assert_eq!(to - from, 7 * DAY);
        assert_eq!(date(to - 1, 480, Zh, false), date(now, 480, Zh, false), "the last day is today");
        let (from, to) = range("lastMonth", now, 480).unwrap();
        assert_eq!(date(from, 480, En, false), "Oct 1, 2026");
        assert_eq!(date(to - 1, 480, Zh, false), "2026年10月31日");
        let (from, _) = range("thisMonth", now, 480).unwrap();
        assert_eq!(date(from, 480, Zh, true), "2026年11月1日 00:00");
        assert!(range("year", now, 0).is_none());
    }

    #[test]
    fn a_receipt_leads_with_the_estimate_and_lists_who_paid() {
        let bill = json!({
            "from": 0, "to": DAY, "runs": 2, "turns": 1200, "sessions": 3, "activeDays": 1, "days": 1, "unreportedSessions": 1,
            "tokens": { "total": 3_111_893, "input": 1000, "cachedRead": 3000, "output": 50 },
            "cost": { "lines": [{ "currency": "USD", "amount": 0.25, "sessions": 2 }], "reportedSessions": 2 },
            "estimate": { "amount": 77.04, "unpricedTurns": 2, "unpricedModels": ["m-x"] },
            "agents": [{ "name": "Claude", "tokens": { "total": 15_400 }, "turns": 3, "estimate": 1.5, "cost": {},
                "models": [{ "model": "opus", "tokens": { "total": 15_400 }, "turns": 3, "estimate": 1.5 }] }],
        });
        let r = receipt(&bill, 0, 0, Zh);
        assert_eq!((r["headline"].as_str(), r["priced"].as_bool()), (Some("$77.04"), Some(true)));
        assert_eq!(r["headlineDetail"], "3,111,893 tokens · Agent 上报费用 0.25 USD（2 个会话）");
        assert_eq!(r["meta"][0]["value"], "1970年1月1日 – 1970年1月1日");
        assert_eq!(r["stats"][1]["value"], "1,200 轮");
        assert_eq!(r["stats"][4]["value"], "3.11M tokens");
        assert_eq!(r["groups"][0]["amount"]["estimate"], "$1.50");
        assert_eq!(r["groups"][0]["amount"]["tokens"], "15.4K");
        assert!(r["groups"][0]["cost"].is_null(), "no cost reported for this agent");
        let notes: Vec<&str> = r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
        assert_eq!(notes[1], "2 轮没有价格（m-x），没有计入估算。");
        assert_eq!(notes[2], "1 个会话没上报 Token，没算进来。");
        let totals: Vec<&str> = r["totals"].as_array().unwrap().iter().map(|t| t["label"].as_str().unwrap()).collect();
        assert_eq!(totals, ["Token 总量", "输入", "缓存读取", "输出", "缓存命中率"]);
        assert_eq!(r["totals"][4]["value"], "75%");
        assert!(r["text"].as_str().unwrap().contains("Claude：15,400\n  · opus：15,400"));
        let plain = receipt(&json!({ "tokens": {}, "estimate": null, "cost": {} }), 0, 0, En);
        assert_eq!(
            (plain["headline"].as_str(), plain["headlineCaption"].as_str()),
            (Some("Not reported"), Some("Total Tokens"))
        );
        assert_eq!(plain["headlineDetail"], "No cost reported by agents");
    }
}
