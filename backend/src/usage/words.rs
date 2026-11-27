//! The ledger as the run page reads it, in the person's language: the run card's line of numbers,
//! the reminders, the rows of the expanded usage and each task's and member's detail. Written once
//! here for every client (docs/architecture/20261031-frontend.md#usage_ledger); a missing number is
//! a word, never 0.

use super::{Completeness, Cost, Ledger, Reminder, Scope, TimeSplit, num};
use crate::lang::{Lang, tr};
use serde::Serialize;

/// What `GET /api/runs/{id}/usage?lang=` adds to the ledger (`words`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWords {
    /// Some session reported usage (tokens, a context snapshot or a cost): only then is there a
    /// ledger to open.
    pub has_usage: bool,
    /// The run card's line after the subtasks' progress, item by item: tokens as compactly
    /// reported, cost, wall-clock time (always) and the context ratio. Joined with 「 · 」.
    pub card: Vec<String>,
    /// The same with every digit, for a screen reader and the hover tip. Joined with 「，」.
    pub voice: Vec<String>,
    /// Which item of `card` and `voice` is the wall-clock time, which a client of a live run
    /// still going counts on from the run's start.
    pub elapsed: usize,
    /// The reminders in one line, prefixed when the numbers are not complete.
    pub reminders: Option<String>,
    /// The expanded usage's rows: tokens, makeup, cost, time, context, the subtasks' time, the
    /// subtotal.
    pub rows: Vec<Row>,
    /// 「3 个会话」.
    pub sessions: String,
    /// Each task's detail, in the ledger's order.
    pub tasks: Vec<Detail>,
    /// Each member's detail, in the ledger's order.
    pub members: Vec<Detail>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub label: String,
    pub value: String,
    /// What the row counts, for the hover tip.
    pub help: String,
}

#[derive(Debug, Serialize)]
pub struct Detail {
    pub id: String,
    pub text: String,
    pub help: String,
}

pub fn words(ledger: &Ledger, lang: Lang) -> UsageWords {
    let l = lang;
    let run = &ledger.run;
    let has_usage = ledger.sessions.iter().any(|s| s.turns > 0 || s.context.reports > 0 || !s.cost.is_empty());
    let (items, elapsed) = card(run, true, l);
    let (voice, _) = card(run, false, l);
    let reminders = (!ledger.reminders.is_empty()).then(|| {
        let head = if run.completeness == Completeness::Complete {
            l.pick("提醒", "Notice")
        } else {
            l.pick("提醒（按已上报数据）", "Notice (reported data only)")
        };
        let items: Vec<String> = ledger.reminders.iter().map(|r| reminder(r, l)).collect();
        format!("{head}{}{}", l.pick("：", ": "), items.join(l.pick("；", " · ")))
    });
    let mut rows = summary_rows(run, l);
    if let Some(time) = time_value(&ledger.time, l) {
        rows.push(row(l.pick("子任务", "Subtasks"), time, l));
    }
    let (lead, collaborators) = (tokens_text(&ledger.lead, l), tokens_text(&ledger.collaborators, l));
    rows.push(row(
        l.pick("小计", "Subtotal"),
        tr!(l, "负责人 {lead} · 协作者 {collaborators}", "Lead: {lead} · Collaborators: {collaborators}"),
        l,
    ));
    let n = run.sessions;
    UsageWords {
        has_usage,
        card: items,
        voice,
        elapsed,
        reminders,
        rows,
        sessions: tr!(l, "{n} 个会话", "Sessions: {n}"),
        tasks: ledger
            .tasks
            .iter()
            .map(|s| {
                let text = task_detail(s, l);
                let cost = cost_detail(&s.cost, l);
                let help = tr!(l, "{text} · {cost}，点按打开子任务页", "{text} · {cost}, open subtask");
                Detail { id: s.id.clone(), text, help }
            })
            .collect(),
        members: ledger
            .members
            .iter()
            .map(|s| {
                let text = member_detail(s, l);
                let help = match context_detail(s, l) {
                    Some(c) => format!("{text}{}", tr!(l, " · 上下文 {c}", " · Context: {c}")),
                    None => text.clone(),
                };
                Detail { id: s.id.clone(), text, help }
            })
            .collect(),
    }
}

/// The run card's items after the subtasks' progress, and which one is the time.
fn card(run: &Scope, compact: bool, l: Lang) -> (Vec<String>, usize) {
    let mut parts = vec![];
    if let Some(total) = run.tokens.total {
        let n = if compact { compact_num(total) } else { num(total) };
        let how = completeness(run.completeness, l);
        parts.push(tr!(l, "用量 {n} tokens（{how}）", "Usage: {n} tokens ({how})"));
    }
    if !run.cost.lines.is_empty() {
        let c = cost_amounts(&run.cost, false, l).unwrap_or_else(|| cost_detail(&run.cost, l));
        parts.push(tr!(l, "费用 {c}", "Cost: {c}"));
    }
    let elapsed = parts.len();
    parts.push(elapsed_text(duration_text(run.duration_ms, l), l));
    if run.context.reports > 0 {
        if compact {
            let ratio = run.context.ratio().map(percent).unwrap_or_else(|| no_ratio(l));
            parts.push(tr!(l, "上下文 {ratio}", "Context: {ratio}"));
        } else if let Some(detail) = context_detail(run, l) {
            parts.push(tr!(l, "上下文 {detail}", "Context: {detail}"));
        }
    }
    (parts, elapsed)
}

/// 「耗时 3 分 05 秒（总时长）」.
fn elapsed_text(d: String, l: Lang) -> String {
    tr!(l, "耗时 {d}", "Elapsed: {d}")
}

fn no_ratio(l: Lang) -> String {
    l.pick("没有比例", "No ratio reported").into()
}

fn reminder(r: &Reminder, l: Lang) -> String {
    match r {
        Reminder::Tokens { total, threshold } => {
            let (total, threshold) = (num(*total), num(*threshold));
            tr!(
                l,
                "Token 用量偏高：已上报 {total} tokens，达到默认提醒阈值 {threshold}",
                "High token usage: {total} tokens reported, reaching the default threshold of {threshold}"
            )
        }
        Reminder::OneTask { task, total, share } => {
            let (total, share) = (num(*total), percent(*share));
            tr!(
                l,
                "大部分用量集中在一个子任务：{task} 占 {total} 的 {share}",
                "Most usage is concentrated in one subtask: {task} accounts for {share} of {total}"
            )
        }
        Reminder::IdleTeam { collaborators, minutes } => tr!(
            l,
            "协作者多但没有有效产出：{collaborators} 个协作者，{minutes} 分钟内没有任何子任务成功",
            "No subtasks completed in {minutes} minutes with {collaborators} collaborators"
        ),
        Reminder::CostUnknown => l
            .pick(
                "费用算不出来：有 Token 用量，但没有会话上报费用",
                "Cost unknown: tokens were reported, but no session reported a cost",
            )
            .into(),
    }
}

fn completeness(c: Completeness, l: Lang) -> &'static str {
    match c {
        Completeness::Missing => l.pick("未上报", "Not reported"),
        Completeness::Complete => l.pick("完整", "Complete"),
        Completeness::Partial => l.pick("部分上报", "Partially reported"),
    }
}

fn not_reported(l: Lang) -> String {
    l.pick("未上报", "Not reported").into()
}

/// `15,400 tokens（部分上报）`, or `未上报`.
fn tokens_text(s: &Scope, l: Lang) -> String {
    match s.tokens.total {
        Some(n) => {
            let (n, how) = (num(n), completeness(s.completeness, l));
            tr!(l, "{n} tokens（{how}）", "{n} tokens ({how})")
        }
        None => not_reported(l),
    }
}

/// `输入 1,200 · 缓存读取 5,000 · 输出 300`, parts of 0 left out.
fn breakdown(s: &Scope, l: Lang) -> Option<String> {
    let t = &s.tokens;
    let parts: Vec<String> = [
        (l.pick("输入", "Input"), t.input),
        (l.pick("缓存读取", "Cache Read"), t.cached_read),
        (l.pick("缓存写入", "Cache Write"), t.cached_write),
        (l.pick("输出", "Output"), t.output),
    ]
    .into_iter()
    .filter_map(|(label, n)| n.filter(|n| *n > 0).map(|n| format!("{label} {}", num(n))))
    .collect();
    if parts.is_empty() {
        return None;
    }
    let thought = t
        .thought
        .filter(|n| *n > 0)
        .map(|n| {
            let n = num(n);
            tr!(l, "（输出中推理 {n}）", " (reasoning output: {n})")
        })
        .unwrap_or_default();
    Some(parts.join(" · ") + &thought)
}

fn cost_detail(c: &Cost, l: Lang) -> String {
    cost_amounts(c, true, l).unwrap_or_else(|| not_reported(l))
}

/// Each reported amount with its currency (and, with `sessions`, how many sessions it sums), never
/// added up across currencies.
fn cost_amounts(c: &Cost, sessions: bool, l: Lang) -> Option<String> {
    let parts: Vec<String> = c
        .lines
        .iter()
        .map(|x| {
            let amount = money(x.amount);
            let (cur, n) = (&x.currency, x.sessions);
            if cur.is_empty() {
                tr!(l, "{amount}（未标注币种）", "{amount} (currency unspecified)")
            } else if sessions && n > 1 {
                tr!(l, "{amount} {cur}（{n} 个会话）", "{amount} {cur} ({n} sessions)")
            } else {
                format!("{amount} {cur}")
            }
        })
        .collect();
    match parts.len() {
        0 => None,
        1 => parts.into_iter().next(),
        _ => Some(
            parts.join(" + ")
                + l.pick(
                    "（币种或口径不同，未合并）",
                    " (reported separately due to different currencies or billing methods)",
                ),
        ),
    }
}

/// Wall-clock time; it never stands in for agent execution time.
fn duration_text(ms: Option<i64>, l: Lang) -> String {
    match ms {
        Some(ms) => {
            let d = dur(ms, l);
            tr!(l, "{d}（总时长）", "{d} (total time)")
        }
        None => l.pick("没有时长记录", "No duration recorded").into(),
    }
}

/// `52,000 / 200,000（26%） · 峰值 61,000`.
fn context_detail(s: &Scope, l: Lang) -> Option<String> {
    let c = &s.context;
    if c.reports == 0 {
        return None;
    }
    let used = c.used.map(num).unwrap_or_else(|| not_reported(l));
    let mut line = match c.size {
        Some(size) => {
            let (size, ratio) = (num(size), c.ratio().map(percent).unwrap_or_else(|| no_ratio(l)));
            tr!(l, "{used} / {size}（{ratio}）", "{used} / {size} ({ratio})")
        }
        None => used,
    };
    if let (Some(peak), Some(u)) = (c.peak, c.used)
        && peak > u
    {
        let peak = num(peak);
        line.push_str(&tr!(l, " · 峰值 {peak}", " · Peak: {peak}"));
    }
    Some(line)
}

fn row(label: &str, value: String, l: Lang) -> Row {
    let help = match label {
        "Token" => l.pick(
            "每轮 Agent 上报的 Token 之和（含子 Agent）；没上报写「未上报」，上下文快照不计入",
            "Sum of tokens each Agent reports per turn (subagents included); missing shows Not reported; context snapshots excluded",
        ),
        "构成" | "Breakdown" => l.pick("输入、缓存读取、缓存写入和输出；为 0 的不列", "Input, cache reads, cache writes, and output; zeros are left out"),
        "费用" | "Cost" => l.pick(
            "Agent 上报的原值；没上报就写「未上报」，不按 Token 估算",
            "As reported by the Agent; shows Not reported when missing, never estimated from tokens",
        ),
        "耗时" | "Elapsed" => l.pick(
            "总时长：从任务创建到最后一条事件；等你的时间单独列出",
            "Total time from run creation to the latest event; time waiting on you is listed separately",
        ),
        "上下文" | "Context" => l.pick(
            "上下文快照只看最近值和峰值，不算进 Token 总量",
            "Context snapshots show the latest and peak only, and aren't in total tokens",
        ),
        "小计" | "Subtotal" => {
            l.pick("总计含负责人；负责人和协作者分开列", "Totals include the lead; lead and collaborators are listed separately")
        }
        _ => l.pick(
            "每次尝试从开始到结束的时间相加；同时进行的尝试各算各的。验证是独立验证的尝试，返工是失败后或负责人发回后的再次尝试",
            "Each attempt's time from start to end, added up; attempts that ran side by side each count. Verification is the independent checks; rework is any attempt after a failure or one the lead sent back.",
        ),
    };
    Row { label: label.into(), value, help: help.into() }
}

/// Token, makeup, cost, wall-clock time and context.
fn summary_rows(s: &Scope, l: Lang) -> Vec<Row> {
    let mut rows = vec![row("Token", tokens_text(s, l), l)];
    if let Some(b) = breakdown(s, l) {
        rows.push(row(l.pick("构成", "Breakdown"), b, l));
    }
    rows.push(row(l.pick("费用", "Cost"), cost_detail(&s.cost, l), l));
    let mut time = vec![duration_text(s.duration_ms, l)];
    if let Some(w) = s.wait_ms {
        let w = dur(w, l);
        time.push(tr!(l, "其中等待你 {w}", "Waiting for you: {w}"));
    }
    rows.push(row(l.pick("耗时", "Elapsed"), time.join(" · "), l));
    if let Some(c) = context_detail(s, l) {
        rows.push(row(l.pick("上下文", "Context"), c, l));
    }
    rows
}

/// 「42 分：执行 20 分 · 验证 15 分 · 返工 7 分」; nothing before any attempt.
fn time_value(t: &TimeSplit, l: Lang) -> Option<String> {
    let total = t.work_ms + t.verify_ms + t.rework_ms;
    if total <= 0 {
        return None;
    }
    let work = dur(t.work_ms, l);
    let mut parts = vec![tr!(l, "执行 {work}", "work {work}")];
    if t.verify_ms > 0 {
        let v = dur(t.verify_ms, l);
        parts.push(tr!(l, "验证 {v}", "verification {v}"));
    }
    if t.rework_ms > 0 {
        let r = dur(t.rework_ms, l);
        parts.push(tr!(l, "返工 {r}", "rework {r}"));
    }
    let (total, parts) = (dur(total, l), parts.join(" · "));
    Some(tr!(l, "{total}：{parts}", "{total}: {parts}"))
}

/// A task's tokens and wall-clock time, or 「未开始」.
fn task_detail(s: &Scope, l: Lang) -> String {
    if s.sessions == 0 {
        return l.pick("未开始", "Not started").into();
    }
    let mut parts = vec![tokens_text(s, l)];
    if let Some(ms) = s.duration_ms {
        parts.push(dur(ms, l));
    }
    parts.join(" · ")
}

/// A member's tokens, or 「没有会话」, and how many sessions it used.
fn member_detail(s: &Scope, l: Lang) -> String {
    let tokens = if s.sessions == 0 { l.pick("没有会话", "No sessions").into() } else { tokens_text(s, l) };
    let n = s.sessions;
    tr!(l, "{tokens} · {n} 个会话", "{tokens} · Sessions: {n}")
}

/// A wall-clock duration, as the apps write one (`L.dur`).
pub fn dur(ms: i64, l: Lang) -> String {
    if ms < 1000 {
        return l.pick("不到 1 秒", "Under 1 sec").into();
    }
    let s = (ms + 500) / 1000;
    let (h, m, sec) = (s / 3600, s % 3600 / 60, s % 60);
    match s {
        0..60 => tr!(l, "{s} 秒", "{s} sec"),
        60..3600 => tr!(l, "{m} 分 {sec:02} 秒", "{m} min {sec:02} sec"),
        _ => tr!(l, "{h} 小时 {m} 分", "{h} hr {m} min"),
    }
}

/// `15.4K`, `2.1M`: a count in a few characters, for the run card.
fn compact_num(n: i64) -> String {
    for (size, unit) in [(1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if (n.unsigned_abs() as f64) >= size {
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

/// `26%`.
fn percent(ratio: f64) -> String {
    format!("{}%", (ratio * 100.0).round() as i64)
}

/// An amount exactly as the agent gave it, to four decimals at most.
fn money(v: f64) -> String {
    let rounded = (v * 10_000.0).round() / 10_000.0;
    if rounded == rounded.round() && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let mut s = format!("{rounded:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::super::Tokens;
    use super::*;

    #[test]
    fn numbers_are_written_as_the_apps_wrote_them() {
        assert_eq!(compact_num(15_400), "15.4K");
        assert_eq!(compact_num(999), "999");
        assert_eq!(compact_num(2_000_000), "2M");
        assert_eq!(compact_num(123_456), "123K");
        assert_eq!(money(0.25), "0.25");
        assert_eq!(money(3.0), "3");
        assert_eq!(money(0.123456), "0.1235");
        assert_eq!(percent(0.26), "26%");
        assert_eq!(dur(65_000, Lang::Zh), "1 分 05 秒");
        assert_eq!(dur(500, Lang::En), "Under 1 sec");
        assert_eq!(dur(7_260_000, Lang::En), "2 hr 1 min");
    }

    fn run(
        tokens: Tokens,
        how: Completeness,
        ms: i64,
        context: super::super::Context,
        cost: Vec<super::super::CostLine>,
    ) -> Ledger {
        let mut l = super::super::ledger(&[]);
        l.run.tokens = tokens;
        l.run.completeness = how;
        l.run.duration_ms = Some(ms);
        l.run.context = context;
        l.run.cost = Cost { reported_sessions: cost.len(), lines: cost, missing_sessions: 0 };
        l
    }

    #[test]
    fn the_run_card_keeps_the_reported_numbers_in_order() {
        let context = super::super::Context { used: Some(78_000), peak: Some(90_000), size: Some(200_000), reports: 4 };
        let usd = |sessions| super::super::CostLine { currency: "USD".into(), amount: 44.2864, sessions };
        let tokens =
            Tokens { total: Some(76_800_000), input: Some(50_000_000), output: Some(1_000_000), ..Default::default() };
        let all = run(tokens, Completeness::Complete, 2_878_000, context, vec![usd(1)]);
        let w = words(&all, Lang::Zh);
        assert_eq!(
            w.card.join(" · "),
            "用量 76.8M tokens（完整） · 费用 44.2864 USD · 耗时 47 分 58 秒（总时长） · 上下文 39%"
        );
        assert_eq!(w.elapsed, 2);
        // A screen reader gets every digit, never the compact form.
        assert_eq!(
            words(&all, Lang::En).voice.join(", "),
            "Usage: 76,800,000 tokens (Complete), Cost: 44.2864 USD, Elapsed: 47 min 58 sec (total time), Context: 78,000 / 200,000 (39%) · Peak: 90,000"
        );
        // The card leaves 「N 个会话」 to the expanded rows.
        let many = run(tokens, Completeness::Complete, 2_878_000, context, vec![usd(9)]);
        let w = words(&many, Lang::Zh);
        assert!(w.card.contains(&"费用 44.2864 USD".to_string()));
        assert!(w.rows.iter().any(|r| r.value == "44.2864 USD（9 个会话）"));
    }

    #[test]
    fn the_run_card_leaves_out_what_was_not_reported() {
        let partial = run(
            Tokens { total: Some(15_400), ..Default::default() },
            Completeness::Partial,
            65_000,
            Default::default(),
            vec![],
        );
        assert_eq!(
            words(&partial, Lang::En).card.join(" · "),
            "Usage: 15.4K tokens (Partially reported) · Elapsed: 1 min 05 sec (total time)"
        );
        let context = super::super::Context { used: Some(52_000), peak: None, size: Some(200_000), reports: 2 };
        let context_only = run(Tokens::default(), Completeness::Missing, 2_000, context, vec![]);
        assert_eq!(words(&context_only, Lang::Zh).card.join(" · "), "耗时 2 秒（总时长） · 上下文 26%");
    }

    #[test]
    fn an_empty_ledger_says_what_is_missing_and_still_has_its_time() {
        let w = words(&super::super::ledger(&[]), Lang::Zh);
        assert!(!w.has_usage);
        assert_eq!(w.card, vec!["耗时 没有时长记录".to_string()]);
        assert_eq!(w.elapsed, 0);
        assert_eq!(w.rows.last().map(|r| r.value.as_str()), Some("负责人 未上报 · 协作者 未上报"));
        assert_eq!(w.sessions, "0 个会话");
        assert!(w.reminders.is_none());
    }
}
